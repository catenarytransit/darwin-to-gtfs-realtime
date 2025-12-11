use crate::static_data::GTFSManager;
use dashmap::DashMap;
use gtfs_realtime::FeedEntity;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Platform Map: StopID -> Platform Number
pub type PlatformMap = HashMap<String, String>;

pub struct AppState {
    // Map TripID -> GTFS-RT Entity (TripUpdate)
    pub trip_updates: DashMap<String, FeedEntity>,

    // Map TripID -> (StopID -> Platform)
    // We need this for the /platforms endpoint
    pub platforms: DashMap<String, PlatformMap>,

    // Map Station CRS -> List of Messages
    // For simplicity, we might just store all messages, or map by ID.
    // Let's map by Message ID for now to avoid duplications, or by Station CRS.
    // User requirement: "Store".
    // Let's store by ID.
    pub station_messages: DashMap<String, String>, // ID -> Msg Content

    // Map RID -> TripID (for TrainOrder and Loading lookups)
    pub rid_to_trip_id: DashMap<String, String>,

    pub gtfs: GTFSManager,
}

impl AppState {
    pub fn new(gtfs_url: String) -> Self {
        Self {
            trip_updates: DashMap::new(),
            platforms: DashMap::new(),
            station_messages: DashMap::new(),
            rid_to_trip_id: DashMap::new(),
            gtfs: GTFSManager::new(gtfs_url),
        }
    }
}
