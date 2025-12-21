use crate::state::AppState;
use compact_str::CompactString;

use anyhow::Result;
use gtfs_realtime::{FeedHeader, FeedMessage};

use prost::Message;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

pub fn save_state(state: &AppState, dir: &str) -> Result<()> {
    let _ = std::fs::create_dir_all(dir);

    // 1. Save Trips (Protobuf)
    let trips_path = format!("{}/trips.pb", dir);
    let mut msg = FeedMessage::default();
    msg.header = FeedHeader::default();
    msg.header.gtfs_realtime_version = "2.0".to_string();
    msg.header.timestamp = Some(chrono::Utc::now().timestamp() as u64);

    for r in state.trip_updates.iter() {
        msg.entity.push(r.value().clone());
    }

    let mut buf = Vec::new();
    msg.encode(&mut buf)?;
    let mut f = File::create(trips_path)?;
    f.write_all(&buf)?;

    // 2. Save Platforms V2 (Bincode)
    let platforms_v2_path = format!("{}/platforms_v2.bin", dir);
    // Collect DashMap to HashMap for serialization
    let mut v2_map = std::collections::HashMap::new();
    for r in state.platforms_v2.iter() {
        v2_map.insert(r.key().clone(), r.value().clone());
    }
    let f = File::create(platforms_v2_path)?;
    bincode::serialize_into(f, &v2_map)?;

    Ok(())
}

pub fn load_state(state: &AppState, dir: &str) -> Result<()> {
    let trips_path = format!("{}/trips.pb", dir);
    if Path::new(&trips_path).exists() {
        let mut f = File::open(trips_path)?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;

        if let Ok(msg) = FeedMessage::decode(&buf[..]) {
            for entity in msg.entity {
                state
                    .trip_updates
                    .insert(CompactString::from(entity.id.clone()), entity);
            }
            println!("Loaded {} trips from disk.", state.trip_updates.len());
        }
    }

    // Platform loading removed

    // Load Platforms V2 (Bincode)
    let platforms_v2_path = format!("{}/platforms_v2.bin", dir);
    if Path::new(&platforms_v2_path).exists() {
        let f = File::open(platforms_v2_path)?;
        let v2_map: std::collections::HashMap<CompactString, Vec<crate::state::PlatformInfo>> =
            bincode::deserialize_from(f)?;

        for (trip_id, info) in v2_map {
            state.platforms_v2.insert(trip_id, info);
        }
        println!(
            "Loaded platforms_v2 for {} trips.",
            state.platforms_v2.len()
        );
    }

    Ok(())
}
