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
        for formation in ur.formation {
            process_formation(&formation, state);
        }
    }
    // We currently ignore schedule_record (sR) as we rely on static GTFS for basic schedule
    // and uR for updates.
}

fn process_formation(formation: &crate::darwin_types::Formation, state: &AppState) {
    if let Some(trip_id) = state.rid_to_trip_id.get(&formation.rid) {
        let label = formation
            .coaches
            .iter()
            .map(|c| c.number.clone())
            .collect::<Vec<_>>()
            .join("-");

        println!(
            "Processed Formation for RID: {}, Label: {}",
            formation.rid, label
        );

        // Update TripUpdate
        if let Some(mut entity) = state.trip_updates.get_mut(trip_id.value()) {
            if let Some(tu) = entity.trip_update.as_mut() {
                let mut vehicle = tu.vehicle.clone().unwrap_or_default();
                vehicle.label = Some(label.clone());
                tu.vehicle = Some(vehicle);
            }
        }

        // Update VehiclePosition
        let vp_key = CompactString::from(format!("{}_VP", trip_id.as_str()));
        let mut entity = state.trip_updates.entry(vp_key.clone()).or_insert_with(|| {
            let mut fe = FeedEntity::default();
            fe.id = vp_key.to_string();
            let mut vp = VehiclePosition::default();
            let mut td = gtfs_realtime::TripDescriptor::default();
            td.trip_id = Some(trip_id.to_string());
            vp.trip = Some(td);
            fe.vehicle = Some(vp);
            fe
        });

        if let Some(vp) = entity.vehicle.as_mut() {
            let mut descriptor = vp.vehicle.clone().unwrap_or_default();
            descriptor.label = Some(label);
            vp.vehicle = Some(descriptor);
        }
    }
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

    // 2. Prepare GTFS-RT Entity
    let mut entity = state
        .trip_updates
        .entry(trip_id.clone())
        .or_insert_with(|| {
            let mut fe = FeedEntity::default();
            fe.id = trip_id.to_string();
            let mut tu = TripUpdate::default();
            tu.trip.trip_id = Some(trip_id.to_string());

            // Correct Start Date Calculation
            // 1. Get trip start time from static GTFS (seconds from midnight)
            let start_secs = state.gtfs.get_trip_start_time(&trip_id).unwrap_or(0);

            // 2. Construct naive datetime (Local/London time) based on SSD + StartTime
            // SSD is the "Schedule Date", adding start_secs gives the actual Start Time in UK Local.
            let initial_dt = date_parsed.and_hms_opt(0, 0, 0).unwrap_or_default()
                + Duration::seconds(start_secs as i64);

            // 3. Convert/Ensure it's London Time (mostly for correctness of date boundary)
            // Using latest() to handle ambiguity; fallback to naive date if invalid (gap).
            let correct_date_str = London
                .from_local_datetime(&initial_dt)
                .latest()
                .map(|dt| dt.date_naive())
                .unwrap_or_else(|| initial_dt.date())
                .format("%Y%m%d")
                .to_string();

            tu.trip.start_date = Some(correct_date_str);
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
                    let stu = build_stop_time_update(loc, &stop_id, &ts.ssd, found_seq);

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
) -> StopTimeUpdate {
    let mut stu = StopTimeUpdate::default();
    stu.stop_id = Some(stop_id.to_string());
    stu.stop_sequence = seq;

    if let Some(arr) = &loc.arr {
        stu.arrival = parse_time(arr, ssd);
    }
    if let Some(dep) = &loc.dep {
        stu.departure = parse_time(dep, ssd);
    } else if let Some(_pass) = &loc.pass {
        // Ignored
    }

    stu
}

fn parse_time(f: &crate::darwin_types::Forecast, ssd: &str) -> Option<StopTimeEvent> {
    let time_str = f.at.as_ref().or(f.et.as_ref())?;

    // format HH:MM
    let hm: Vec<&str> = time_str.split(':').collect();
    if hm.len() != 2 {
        return None;
    }
    let hour: u32 = hm[0].parse().ok()?;
    let min: u32 = hm[1].parse().ok()?;

    // We need complete date.
    // SSD is YYYY-MM-DD
    let date = NaiveDate::parse_from_str(ssd, "%Y-%m-%d").ok()?;
    let dt = date.and_hms_opt(hour, min, 0)?;

    // Handle day rollover?
    // If dt < ssd (time-wise) it's likely +1 day.
    // But ssd is just date.

    // We convert to UTC timestamp
    // Assuming Utc for now to compile, but note this gap.
    let ts = dt.and_utc().timestamp();

    let mut event = StopTimeEvent::default();
    event.time = Some(ts);
    // Remove uncertainty for now
    Some(event)
}
