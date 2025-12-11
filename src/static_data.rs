use anyhow::{Context, Result};
use gtfs_structures::Gtfs;
use std::collections::HashMap;
use std::io::copy;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;
use tempfile::Builder;
use zip::ZipArchive;

pub struct GTFSManager {
    url: String,
    // Use Arc<RwLock> to allow safe sharing between the updater thread and the main application
    tiploc_map: Arc<RwLock<HashMap<String, String>>>,
}

impl GTFSManager {
    pub fn new(url: String) -> Self {
        Self {
            url,
            tiploc_map: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn start_updater(&self) {
        let map_clone = self.tiploc_map.clone();
        let url = self.url.clone();

        thread::spawn(move || {
            loop {
                log_info("Updating GTFS data...");
                match Self::download_and_load(&url) {
                    Ok(new_gtfs) => {
                        let new_map = Self::build_tiploc_map(&new_gtfs);
                        {
                            let mut m = map_clone.write().unwrap();
                            *m = new_map;
                        }
                        log_info("GTFS data updated successfully.");
                    }
                    Err(e) => {
                        eprintln!("Failed to update GTFS data: {:?}", e);
                    }
                }
                // Update every hour
                thread::sleep(Duration::from_secs(3600));
            }
        });
    }

    // Try to load immediately (blocking), returns error if fails
    pub fn load_initial(&self) -> Result<()> {
        log_info("Performing initial GTFS load...");
        let new_gtfs = Self::download_and_load(&self.url)?;
        let new_map = Self::build_tiploc_map(&new_gtfs);

        {
            let mut m = self.tiploc_map.write().unwrap();
            *m = new_map;
        }
        log_info("Initial GTFS load complete.");
        Ok(())
    }

    pub fn get_stop_id(&self, tiploc: &str) -> Option<String> {
        let map = self.tiploc_map.read().unwrap();
        // Try exact match first
        if let Some(id) = map.get(tiploc) {
            return Some(id.clone());
        }
        None
    }

    pub fn has_data(&self) -> bool {
        !self.tiploc_map.read().unwrap().is_empty()
    }

    fn download_and_load(url: &str) -> Result<Gtfs> {
        // 1. Download to temp file
        let gtfs = Gtfs::new(url).map_err(|e| anyhow::anyhow!("Gtfs error: {:?}", e))?;
        Ok(gtfs)
    }

    fn build_tiploc_map(gtfs: &Gtfs) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for (id, stop) in &gtfs.stops {
            // Priority 1: stop_code is a TIPLOC?
            map.insert(id.clone(), id.clone());

            if let Some(code) = &stop.code {
                map.insert(code.clone(), id.clone());
            }
        }
        println!("Built TIPLOC map with {} entries", map.len());
        map
    }
}

fn log_info(msg: &str) {
    println!("[{}] {}", chrono::Utc::now().to_rfc3339(), msg);
}
