use crate::darwin_types::{Loading, Location, Pport, StationMessage, TrainOrder, TrainStatus};
use crate::state::AppState;
use compact_str::CompactString;
// use anyhow::Result;

use chrono::{Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::Europe::London;

use gtfs_realtime::{
    FeedEntity, TripUpdate, VehiclePosition,
    trip_update::{StopTimeEvent, StopTimeUpdate},
};

use std::collections::HashMap;

pub fn process_pmap(pport: Pport, state: &AppState) {
    if let Some(ur) = pport.update_record {
        for ts in ur.train_status {
            update_trip(&ts, state);
        }
        for to in ur.train_order {
            update_trip_from_order(&to, state);
        }
        for msg in ur.station_message {
            process_station_message(&msg, state);
        }
        for load in ur.loading.iter().chain(ur.loading_alias.iter()) {
            process_loading(load, state);
        }
        for schedule_formation in ur.schedule_formations {
            process_formation(&schedule_formation, state);
        }
    }
    // We currently ignore schedule_record (sR) as we rely on static GTFS for basic schedule
    // and uR for updates.
}

fn process_formation(
    schedule_formation: &crate::formations::v2::ScheduleFormations,
    state: &AppState,
) {
    state
        .formations
        .insert(schedule_formation.rid.clone(), schedule_formation.clone());

    println!("Processed Formation for RID: {}", schedule_formation.rid);
}

fn update_trip(ts: &TrainStatus, state: &AppState) {
    // 1. Construct Trip ID: Try lookup, fallback to {uid}_{ssd}
    let date_parsed =
        NaiveDate::parse_from_str(&ts.ssd, "%Y-%m-%d").unwrap_or_else(|_| Utc::now().date_naive());

    let trip_id = if let Some(found_id) = state.gtfs.find_trip_id(&ts.uid, date_parsed) {
        // println!("Match found: {} -> {}", ts.uid, found_id);
        found_id
    } else {
        println!("No static match for UID: {} on {}", ts.uid, ts.ssd);
        return;
    };

    // Update RID mapping
    state.rid_to_trip_id.insert(ts.rid.clone(), trip_id.clone());

    println!(
        "Processed TrainStatus for RID: {}, Trip: {}",
        ts.rid, trip_id
    );

    // Fetch static stop sequence for loop handling
    // We assume the static stops are sorted by sequence, or we iterate in order.
    // Darwin locations usually come in order.
    let trip_stops = state.gtfs.get_trip_stops(&trip_id).unwrap_or_default();
    let mut current_static_idx = 0;

    // Static GTFS may encode trips after midnight using times >= 24:00:00. Keep
    // that information for resolving Darwin's HH:MM-only realtime timestamps, but
    // do not use it to move the GTFS-RT start_date away from the service date.
    let trip_start_secs = state.gtfs.get_trip_start_time(&trip_id).unwrap_or(0);

    // 2. Prepare GTFS-RT Entity
    let mut entity = state
        .trip_updates
        .entry(trip_id.clone())
        .or_insert_with(|| {
            let mut fe = FeedEntity::default();
            fe.id = trip_id.to_string();
            let mut tu = TripUpdate::default();
            tu.trip.trip_id = Some(trip_id.to_string());

            // GTFS-RT start_date is the GTFS service date (the date used to select
            // the static service), not necessarily the civil date of the first stop.
            // In static GTFS, after-midnight stop_times are commonly >= 24:00:00.
            // Adding those seconds to SSD incorrectly advances start_date by a day.
            tu.trip.start_date = Some(date_parsed.format("%Y%m%d").to_string());

            // route_id? We don't have it easily. gtfs-rt spec says optional if trip_id is unique.
            fe.trip_update = Some(tu);
            fe
        });

    let trip_update = entity.trip_update.as_mut().unwrap();

    // 3. Process Locations
    let mut platform_updates = HashMap::new();
    // Map Sequence -> (StopID, Platform)
    let mut platform_v2_updates: HashMap<u32, (CompactString, CompactString)> = HashMap::new();

    for loc in &ts.locations {
        // Check if tiploc exists
        if let Some(tiploc) = &loc.tiploc {
            // Map TIPLOC -> Stop ID
            let stop_id_opt = state.gtfs.get_stop_id(tiploc);

            if let Some(stop_id) = stop_id_opt {
                // Find matching sequence (Forward greedy match)
                let mut found_seq = None;
                if !trip_stops.is_empty() {
                    for i in current_static_idx..trip_stops.len() {
                        if trip_stops[i].0 == stop_id {
                            found_seq = Some(trip_stops[i].1);
                            current_static_idx = i + 1; // Advance
                            break;
                        }
                    }
                }

                // Platform Logic
                if let Some(plat) = &loc.platform {
                    // Suppression Check:
                    // 1. platsup="true" (Platform attr) -> Hide
                    // 2. suppr="true" (Location elem) -> Hide
                    // 3. cisPlatsup and conf are informational.
                    let is_suppressed = plat.platsup.unwrap_or(false) || loc.suppr.unwrap_or(false);

                    if !is_suppressed {
                        // Only update if not suppressed
                        if let Some(num) = &plat.number {
                            // V1 (Legacy - Broken for loops)
                            platform_updates.insert(stop_id.clone(), num.clone());

                            // V2 (Sequence based)
                            if let Some(seq) = found_seq {
                                platform_v2_updates.insert(seq, (stop_id.clone(), num.clone()));
                            }
                        }
                    }
                }

                // Delay / Time Logic
                if has_time_data(loc) {
                    let stu = build_stop_time_update(
                        loc,
                        &stop_id,
                        &ts.ssd,
                        found_seq,
                        trip_start_secs,
                    );

                    // Update Logic: Prefer sequence match if available
                    if let Some(seq) = found_seq {
                        if let Some(idx) = trip_update
                            .stop_time_update
                            .iter()
                            .position(|u| u.stop_sequence == Some(seq))
                        {
                            trip_update.stop_time_update[idx] = stu;
                        } else {
                            trip_update.stop_time_update.push(stu);
                        }
                    } else {
                        // Fallback to stop_id match
                        if let Some(existing_idx) = trip_update
                            .stop_time_update
                            .iter()
                            .position(|u| u.stop_id.as_deref() == Some(stop_id.as_str()))
                        {
                            trip_update.stop_time_update[existing_idx] = stu;
                        } else {
                            trip_update.stop_time_update.push(stu);
                        }
                    }
                }
            }
        }
    }

    // Sort updates by sequence
    trip_update
        .stop_time_update
        .sort_by_key(|u| u.stop_sequence.unwrap_or(0));

    // Update Platform Maps
    if !platform_v2_updates.is_empty() {
        use crate::state::PlatformInfo;
        let mut platforms_entry = state.platforms_v2.entry(trip_id.clone()).or_default();

        for (seq, (stop_id, plat)) in platform_v2_updates {
            // Check if we already have an entry for this sequence
            if let Some(existing) = platforms_entry.iter_mut().find(|p| p.sequence == seq) {
                existing.platform = plat;
                existing.stop_id = stop_id;
            } else {
                platforms_entry.push(PlatformInfo {
                    stop_id,
                    sequence: seq,
                    platform: plat,
                });
            }
        }
        // Keep sorted by sequence
        platforms_entry.sort_by_key(|p| p.sequence);
    }
}

fn update_trip_from_order(to: &TrainOrder, state: &AppState) {
    if let Some(set) = &to.set {
        let items = vec![&set.first, &set.second, &set.third];

        for (idx, item_opt) in items.iter().enumerate() {
            if let Some(item) = item_opt {
                if let Some(rid_data) = &item.rid {
                    if let Some(trip_id) = state.rid_to_trip_id.get(&rid_data.value) {
                        // 1. Update Platform (existing logic) REMOVED
                        if let Some(_stop_id) = state.gtfs.get_stop_id(&to.tiploc) {
                            if let Some(_platform) = &to.platform {
                                // state
                                //     .platforms
                                //     .entry(trip_id.clone())
                                //     .or_default()
                                //     .insert(stop_id.clone(), platform.clone());
                            }
                        }

                        // 2. Update VehiclePosition for Consist
                        let vp_key = CompactString::from(format!("{}_VP", trip_id.as_str()));
                        let mut entity =
                            state.trip_updates.entry(vp_key.clone()).or_insert_with(|| {
                                let mut fe = FeedEntity::default();
                                fe.id = vp_key.to_string();
                                let mut vp = VehiclePosition::default();

                                // Library TripUpdate has TripDescriptor direct, but VehiclePosition?
                                // VehiclePosition typically has optional TripDescriptor.
                                // Let's check typical usage. Usually `vp.trip = Some(...)`
                                // I'll assume standard usage.
                                let mut td = gtfs_realtime::TripDescriptor::default();
                                td.trip_id = Some(trip_id.to_string());
                                vp.trip = Some(td);

                                fe.vehicle = Some(vp);
                                fe
                            });

                        let vp = entity.vehicle.as_mut().unwrap();

                        if let Some(stop_id) = state.gtfs.get_stop_id(&to.tiploc) {
                            vp.stop_id = Some(stop_id.to_string());
                        }

                        // Populate CarriageDetails
                        // Reuse existing entry if sequence matches, else create new.
                        let seq = (idx + 1) as u32;
                        let mut cd = gtfs_realtime::vehicle_position::CarriageDetails::default();
                        cd.id = Some(rid_data.value.to_string());
                        cd.label = item.train_id.clone().map(|s| s.into_string());
                        cd.carriage_sequence = Some(seq);

                        if let Some(existing) = vp
                            .multi_carriage_details
                            .iter_mut()
                            .find(|c| c.carriage_sequence == Some(seq))
                        {
                            *existing = cd;
                        } else {
                            vp.multi_carriage_details.push(cd);
                        }
                        // Keep sorted
                        vp.multi_carriage_details
                            .sort_by_key(|c| c.carriage_sequence.unwrap_or(0));

                        println!("Updated VP Consist for Trip {}", trip_id.as_str());
                    }
                }
            }
        }
    }
}

fn process_station_message(msg: &StationMessage, state: &AppState) {
    let full_msg = format!("{}: {}", msg.category, msg.message);
    state.station_messages.insert(msg.id.clone(), full_msg);
    println!("Processed StationMessage: {} ({})", msg.id, msg.category);
}

fn process_loading(load: &Loading, state: &AppState) {
    // Need Loading fields to be useful.
    if let Some(_trip_id) = state.rid_to_trip_id.get(&load.rid) {
        // TODO: Update OccupancyStatus if Loading struct has fields.
        // For now, valid placeholder.
        println!("Processed Loading for RID: {}", load.rid);
    }
}

fn has_time_data(loc: &Location) -> bool {
    // Check arr, dep, pass for 'et' or 'at'
    check_forecast(&loc.arr) || check_forecast(&loc.dep) || check_forecast(&loc.pass)
}

fn check_forecast(f: &Option<crate::darwin_types::Forecast>) -> bool {
    if let Some(f) = f {
        f.et.is_some() || f.at.is_some()
    } else {
        false
    }
}

fn build_stop_time_update(
    loc: &Location,
    stop_id: &str,
    ssd: &str,
    seq: Option<u32>,
    trip_start_secs: u32,
) -> StopTimeUpdate {
    let mut stu = StopTimeUpdate::default();
    stu.stop_id = Some(stop_id.to_string());
    stu.stop_sequence = seq;

    if let Some(arr) = &loc.arr {
        stu.arrival = parse_time(arr, ssd, loc.wta.as_deref(), trip_start_secs);
    }
    if let Some(dep) = &loc.dep {
        let scheduled_departure = loc.wtd.as_deref().or(loc.ptd.as_deref());
        stu.departure = parse_time(dep, ssd, scheduled_departure, trip_start_secs);
    } else if let Some(_pass) = &loc.pass {
        // Ignored
    }

    stu
}

fn parse_hm(time_str: &str) -> Option<(u32, u32)> {
    let mut parts = time_str.split(':');
    let hour: u32 = parts.next()?.parse().ok()?;
    let min: u32 = parts.next()?.parse().ok()?;

    if hour > 23 || min > 59 {
        return None;
    }

    Some((hour, min))
}

fn parse_time(
    f: &crate::darwin_types::Forecast,
    ssd: &str,
    scheduled_time: Option<&str>,
    trip_start_secs: u32,
) -> Option<StopTimeEvent> {
    let time_str = f.at.as_ref().or(f.et.as_ref())?;
    let (hour, min) = parse_hm(time_str)?;

    // SSD is the GTFS service date. Darwin realtime times contain only HH:MM, so
    // recover the civil-day offset using the static trip start plus the scheduled
    // time at this location. A >12h backwards jump means the clock crossed midnight.
    let mut day_offset = trip_start_secs / 86_400;
    let trip_start_minute = (trip_start_secs % 86_400) / 60;
    let forecast_minute = hour * 60 + min;

    if let Some((scheduled_hour, scheduled_min)) = scheduled_time.and_then(parse_hm) {
        let scheduled_minute = scheduled_hour * 60 + scheduled_min;

        if scheduled_minute + 12 * 60 < trip_start_minute {
            day_offset += 1;
        }
        if forecast_minute + 12 * 60 < scheduled_minute {
            day_offset += 1;
        }
    } else if forecast_minute + 12 * 60 < trip_start_minute {
        day_offset += 1;
    }

    let date = NaiveDate::parse_from_str(ssd, "%Y-%m-%d")
        .ok()?
        .checked_add_signed(Duration::days(day_offset as i64))?;
    let dt = date.and_hms_opt(hour, min, 0)?;

    // Darwin times are UK local time (Europe/London), not UTC.
    // Convert local wall-clock time to a Unix timestamp with DST awareness.
    let ts = match London.from_local_datetime(&dt) {
        chrono::LocalResult::Single(local_dt) => local_dt.timestamp(),
        chrono::LocalResult::Ambiguous(earliest, _) => earliest.timestamp(),
        chrono::LocalResult::None => {
            // Spring-forward DST gap: move forward one hour to the next valid local instant.
            let shifted = dt + Duration::hours(1);
            London.from_local_datetime(&shifted).earliest()?.timestamp()
        }
    };

    let mut event = StopTimeEvent::default();
    event.time = Some(ts);
    // Remove uncertainty for now
    Some(event)
}

#[cfg(test)]
mod tests {
    use super::parse_time;
    use crate::darwin_types::Forecast;
    use chrono::{TimeZone, Utc};

    #[test]
    fn parse_time_uses_london_timezone_in_winter() {
        let f = Forecast {
            et: Some("12:00".into()),
            at: None,
        };

        let event = parse_time(&f, "2026-01-15", None, 0).expect("expected parsed event");
        let expected = Utc
            .with_ymd_and_hms(2026, 1, 15, 12, 0, 0)
            .single()
            .expect("valid UTC datetime")
            .timestamp();

        assert_eq!(event.time, Some(expected));
    }

    #[test]
    fn parse_time_uses_london_timezone_in_summer_bst() {
        let f = Forecast {
            et: Some("12:00".into()),
            at: None,
        };

        let event = parse_time(&f, "2026-07-15", None, 0).expect("expected parsed event");
        // 12:00 in London during BST is 11:00 UTC.
        let expected = Utc
            .with_ymd_and_hms(2026, 7, 15, 11, 0, 0)
            .single()
            .expect("valid UTC datetime")
            .timestamp();

        assert_eq!(event.time, Some(expected));
    }
    #[test]
    fn parse_time_rolls_forecast_over_midnight() {
        let f = Forecast {
            et: Some("00:12".into()),
            at: None,
        };

        // A 23:11 scheduled stop reported as 00:12 is 61 minutes late on the next day,
        // not 22h59 early on the service date.
        let event = parse_time(&f, "2026-08-07", Some("23:11"), 21 * 60 * 60)
            .expect("expected parsed event");
        let expected = Utc
            .with_ymd_and_hms(2026, 8, 7, 23, 12, 0)
            .single()
            .expect("valid UTC datetime")
            .timestamp();

        // 00:12 Europe/London on Aug 8 is 23:12 UTC on Aug 7 during BST.
        assert_eq!(event.time, Some(expected));
    }

    #[test]
    fn parse_time_respects_static_after_midnight_trip_start() {
        let f = Forecast {
            et: Some("00:12".into()),
            at: None,
        };

        // 24:05 in static GTFS means 00:05 on the civil day after the service date.
        let event = parse_time(&f, "2026-08-07", Some("00:10"), 24 * 60 * 60 + 5 * 60)
            .expect("expected parsed event");
        let expected = Utc
            .with_ymd_and_hms(2026, 8, 7, 23, 12, 0)
            .single()
            .expect("valid UTC datetime")
            .timestamp();

        assert_eq!(event.time, Some(expected));
    }

}
