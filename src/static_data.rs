use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate};
use compact_str::CompactString;
use gtfs_structures::{Calendar, CalendarDate, Exception, Gtfs, Trip};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

pub struct GtfsData {
    pub tiploc_map: HashMap<CompactString, CompactString>,
    pub uid_index: HashMap<CompactString, Vec<String>>, // UID -> List of TripIDs
    pub trips: HashMap<CompactString, Trip>,
    pub calendar: HashMap<CompactString, Calendar>,
    pub calendar_dates: HashMap<CompactString, Vec<CalendarDate>>,
    pub trip_start_times: HashMap<CompactString, u32>,
}

impl Default for GtfsData {
    fn default() -> Self {
        Self {
            tiploc_map: HashMap::new(),
            uid_index: HashMap::new(),
            trips: HashMap::new(),
            calendar: HashMap::new(),
            calendar_dates: HashMap::new(),
            trip_start_times: HashMap::new(),
        }
    }
}

pub struct GTFSManager {
    url: String,
    // Use Arc<RwLock> to allow safe sharing between the updater thread and the main application
    data: Arc<RwLock<GtfsData>>,
}

impl GTFSManager {
    pub fn new(url: String) -> Self {
        Self {
            url,
            data: Arc::new(RwLock::new(GtfsData::default())),
        }
    }

    pub fn start_updater(&self) {
        let data_clone = self.data.clone();
        let url = self.url.clone();

        thread::spawn(move || {
            loop {
                log_info("Updating GTFS data...");
                match Self::download_and_load(&url) {
                    Ok(new_gtfs) => {
                        let new_data = Self::build_indices(&new_gtfs);
                        {
                            let mut d = data_clone.write().unwrap();
                            *d = new_data;
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

        println!("Building indices...");
        let new_data = Self::build_indices(&new_gtfs);

        {
            let mut d = self.data.write().unwrap();
            *d = new_data;
        }
        log_info("Initial GTFS load complete.");
        Ok(())
    }

    pub fn get_stop_id(&self, tiploc: &str) -> Option<CompactString> {
        let data = self.data.read().unwrap();
        // Try exact match first
        if let Some(id) = data.tiploc_map.get(tiploc) {
            return Some(id.clone());
        }
        None
    }

    pub fn unwrap_stop_id(&self, tiploc: &str) -> CompactString {
        self.get_stop_id(tiploc)
            .unwrap_or_else(|| CompactString::from(tiploc))
    }

    pub fn find_trip_id(&self, uid: &str, date: NaiveDate) -> Option<CompactString> {
        let data = self.data.read().unwrap();

        // 1. Look up candidates by UID
        if let Some(candidates) = data.uid_index.get(uid) {
            for trip_id in candidates {
                // 2. Check service calendar
                // trip_id in uid_index is String (Vec<String>), need to handle lookup
                if let Some(trip) = data.trips.get(trip_id.as_str()) {
                    if self.service_runs_on_date(&data, &trip.service_id, date) {
                        return Some(CompactString::from(trip_id));
                    }
                }
            }
        }
        None
    }

    pub fn get_trip_start_time(&self, trip_id: &str) -> Option<u32> {
        self.data
            .read()
            .unwrap()
            .trip_start_times
            .get(trip_id)
            .cloned()
    }

    pub fn get_trip_stops(&self, trip_id: &str) -> Option<Vec<(CompactString, u32)>> {
        let data = self.data.read().unwrap();
        if let Some(trip) = data.trips.get(trip_id) {
            Some(
                trip.stop_times
                    .iter()
                    .map(|st| (CompactString::from(st.stop.id.clone()), st.stop_sequence))
                    .collect(),
            )
        } else {
            None
        }
    }

    fn service_runs_on_date(&self, data: &GtfsData, service_id: &str, date: NaiveDate) -> bool {
        // Check CalendarDates (Exceptions) first
        if let Some(exceptions) = data.calendar_dates.get(service_id) {
            for exception in exceptions {
                if exception.date == date {
                    if exception.exception_type == Exception::Added {
                        return true;
                    } else if exception.exception_type == Exception::Deleted {
                        return false;
                    }
                }
            }
        }

        // Check Calendar
        if let Some(cal) = data.calendar.get(service_id) {
            if date >= cal.start_date && date <= cal.end_date {
                let runs = match date.weekday() {
                    chrono::Weekday::Mon => cal.monday,
                    chrono::Weekday::Tue => cal.tuesday,
                    chrono::Weekday::Wed => cal.wednesday,
                    chrono::Weekday::Thu => cal.thursday,
                    chrono::Weekday::Fri => cal.friday,
                    chrono::Weekday::Sat => cal.saturday,
                    chrono::Weekday::Sun => cal.sunday,
                };

                if runs {
                    return true;
                }
            }
        }

        false
    }

    pub fn has_data(&self) -> bool {
        !self.data.read().unwrap().tiploc_map.is_empty()
    }

    fn download_and_load(url: &str) -> Result<Gtfs> {
        // 1. Download to temp file
        let gtfs = Gtfs::new(url).map_err(|e| anyhow::anyhow!("Gtfs error: {:?}", e))?;
        println!("Downloaded GTFS");
        Ok(gtfs)
    }

    fn build_indices(gtfs: &Gtfs) -> GtfsData {
        let mut data = GtfsData::default();

        // TIPLOC Map
        for (id, stop) in &gtfs.stops {
            data.tiploc_map
                .insert(CompactString::from(id), CompactString::from(id));
            if let Some(code) = &stop.code {
                data.tiploc_map
                    .insert(CompactString::from(code), CompactString::from(id));
            }
        }

        log_info("Built TIPLOC map");

        // UID Index & Trips
        for (trip_id, trip) in &gtfs.trips {
            data.trips
                .insert(CompactString::from(trip_id), trip.clone());

            // Assume Trip ID format matches UID_...
            if let Some(uid_part) = trip_id.split('_').next() {
                data.uid_index
                    .entry(CompactString::from(uid_part))
                    .or_default()
                    .push(trip_id.clone());
            }
        }
        log_info("Built UID index");

        // Calendar
        for (service_id, cal) in &gtfs.calendar {
            data.calendar
                .insert(CompactString::from(service_id), cal.clone());
        }
        log_info("Built Calendar");

        // Calendar Dates
        for (service_id, dates) in &gtfs.calendar_dates {
            data.calendar_dates
                .insert(CompactString::from(service_id), dates.clone());
        }
        log_info("Built Calendar Dates");

        // Trip Start Times
        for (trip_id, trip) in &gtfs.trips {
            // trip.stop_times is a Vec<StopTime>
            for st in &trip.stop_times {
                if let Some(time) = st.departure_time {
                    data.trip_start_times
                        .entry(CompactString::from(trip_id))
                        .and_modify(|t| {
                            if time < *t {
                                *t = time;
                            }
                        })
                        .or_insert(time);
                }
            }
        }
        log_info("Built Trip Start Times");

        println!(
            "GTFS Indices built: {} stops, {} trips, {} services",
            data.tiploc_map.len(),
            data.trips.len(),
            data.calendar.len()
        );

        data
    }
}

fn log_info(msg: &str) {
    println!("[{}] {}", chrono::Utc::now().to_rfc3339(), msg);
}
