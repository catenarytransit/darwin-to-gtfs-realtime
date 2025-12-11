use crate::state::{AppState, PlatformMap};
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

    // 2. Save Platforms (JSON)
    let platforms_path = format!("{}/platforms.json", dir);
    // Collect DashMap to HashMap for serde
    let mut platform_data: std::collections::HashMap<String, PlatformMap> =
        std::collections::HashMap::new();
    for r in state.platforms.iter() {
        platform_data.insert(r.key().clone(), r.value().clone());
    }

    let f = File::create(platforms_path)?;
    serde_json::to_writer(f, &platform_data)?;

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
                state.trip_updates.insert(entity.id.clone(), entity);
            }
            println!("Loaded {} trips from disk.", state.trip_updates.len());
        }
    }

    let platforms_path = format!("{}/platforms.json", dir);
    if Path::new(&platforms_path).exists() {
        let f = File::open(platforms_path)?;
        let platform_data: std::collections::HashMap<String, PlatformMap> =
            serde_json::from_reader(f)?;
        for (trip_id, map) in platform_data {
            state.platforms.insert(trip_id, map);
        }
        println!(
            "Loaded platforms for {} trips from disk.",
            state.platforms.len()
        );
    }

    Ok(())
}
