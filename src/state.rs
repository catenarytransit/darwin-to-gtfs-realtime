use crate::static_data::GTFSManager;
use compact_str::CompactString;
use dashmap::DashMap;
use gtfs_realtime::FeedEntity;

use serde::{Deserialize, Serialize};
// use std::collections::HashMap; REMOVED

// Platform Map: StopID -> Platform Number REMOVED
// pub type PlatformMap = HashMap<String, String>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlatformInfo {
    pub stop_id: CompactString,
    pub sequence: u32,
    pub platform: CompactString,
}

pub struct AppState {
    // Map TripID -> GTFS-RT Entity (TripUpdate)
    pub trip_updates: DashMap<CompactString, FeedEntity>,

    // platforms field REMOVED

    // Map TripID -> List of Platform Info (V2 Schema)
    pub platforms_v2: DashMap<CompactString, Vec<PlatformInfo>>,

    // Map Station CRS -> List of Messages
    // For simplicity, we might just store all messages, or map by ID.
    // Let's map by Message ID for now to avoid duplications, or by Station CRS.
    // User requirement: "Store".
    // Let's store by ID.
    pub station_messages: DashMap<CompactString, String>, // ID -> Msg Content

    // Map RID -> TripID (for TrainOrder and Loading lookups)
    pub rid_to_trip_id: DashMap<CompactString, CompactString>,

    pub gtfs: GTFSManager,
}

impl AppState {
    pub fn new(gtfs_url: String) -> Self {
        Self {
            trip_updates: DashMap::new(),
            // platforms: DashMap::new(), REMOVED
            platforms_v2: DashMap::new(),
            station_messages: DashMap::new(),
            rid_to_trip_id: DashMap::new(),
            gtfs: GTFSManager::new(gtfs_url),
        }
    }
}
