use crate::darwin_types::{Loading, Location, Pport, StationMessage, TrainOrder, TrainStatus};
use crate::state::AppState;
use anyhow::Result;
use chrono::{NaiveDate, Timelike, Utc};

use gtfs_realtime::{
    FeedEntity, TripUpdate,
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
    }
    // We currently ignore schedule_record (sR) as we rely on static GTFS for basic schedule
    // and uR for updates.
}

fn update_trip(ts: &TrainStatus, state: &AppState) {
    // 1. Construct Trip ID: {uid}_{ssd (YYYYMMDD)}
    let clean_date = ts.ssd.replace("-", "");
    let trip_id = format!("{}_{}", ts.uid, clean_date); // e.g., C00140_250519

    // Update RID mapping
    state.rid_to_trip_id.insert(ts.rid.clone(), trip_id.clone());

    println!(
        "Processed TrainStatus for RID: {}, Trip: {}",
        ts.rid, trip_id
    );

    // 2. Prepare GTFS-RT Entity
    let mut entity = state
        .trip_updates
        .entry(trip_id.clone())
        .or_insert_with(|| {
            let mut fe = FeedEntity::default();
            fe.id = trip_id.clone();
            let mut tu = TripUpdate::default();
            tu.trip.trip_id = Some(trip_id.clone());
            tu.trip.start_date = Some(clean_date.clone());
            // route_id? We don't have it easily. gtfs-rt spec says optional if trip_id is unique.
            fe.trip_update = Some(tu);
            fe
        });

    let trip_update = entity.trip_update.as_mut().unwrap();

    // 3. Process Locations
    let mut platform_updates = HashMap::new();

    for loc in &ts.locations {
        // Check if tiploc exists
        if let Some(tiploc) = &loc.tiploc {
            // Map TIPLOC -> Stop ID
            let stop_id_opt = state.gtfs.get_stop_id(tiploc);

            if let Some(stop_id) = stop_id_opt {
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
                            platform_updates.insert(stop_id.clone(), num.clone());
                        }
                    }
                }

                // Delay / Time Logic
                if has_time_data(loc) {
                    let stu = build_stop_time_update(loc, &stop_id, &ts.ssd);

                    // Check if exists
                    if let Some(existing_idx) = trip_update
                        .stop_time_update
                        .iter()
                        .position(|u| u.stop_id.as_deref() == Some(&stop_id))
                    {
                        trip_update.stop_time_update[existing_idx] = stu;
                    } else {
                        trip_update.stop_time_update.push(stu);
                    }
                }
            }
        }
    }

    // Update Platform Map
    if !platform_updates.is_empty() {
        let mut platforms_entry = state.platforms.entry(trip_id.clone()).or_default();
        for (stop_id, plat) in platform_updates {
            platforms_entry.insert(stop_id, plat);
        }
    }
}

fn update_trip_from_order(to: &TrainOrder, state: &AppState) {
    if let Some(stop_id) = state.gtfs.get_stop_id(&to.tiploc) {
        if let Some(platform) = &to.platform {
            // New Hierarchy: set -> first/second/third -> item -> rid|trainID
            if let Some(set) = &to.set {
                let items = vec![&set.first, &set.second, &set.third];

                for item_opt in items {
                    if let Some(item) = item_opt {
                        // Check for RID
                        if let Some(rid_data) = &item.rid {
                            if let Some(trip_id) = state.rid_to_trip_id.get(&rid_data.value) {
                                println!(
                                    "Processed TrainOrder for RID: {} at {}",
                                    rid_data.value, to.tiploc
                                );
                                // Update Platform for this Trip at this Stop
                                // Note: TrainOrder doesn't explicitly have 'platsup'.
                                // Assuming visible if provided in TrainOrder? (Or maybe inherit?)
                                // For now, proceed.
                                let mut platforms_entry =
                                    state.platforms.entry(trip_id.clone()).or_default();
                                platforms_entry.insert(stop_id.clone(), platform.clone());
                            }
                        }
                        // Ignore trainID for now (no mapping to RID/TripID available easily without more state)
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
    if let Some(trip_id) = state.rid_to_trip_id.get(&load.rid) {
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

fn build_stop_time_update(loc: &Location, stop_id: &str, ssd: &str) -> StopTimeUpdate {
    let mut stu = StopTimeUpdate::default();
    stu.stop_id = Some(stop_id.to_string());

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
