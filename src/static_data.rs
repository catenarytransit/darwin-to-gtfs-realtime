use crate::maple_syrup::{MapleSyrup, MatchedTrip};
use anyhow::Result;
use chrono::{Datelike, NaiveDate};
use compact_str::CompactString;
use gtfs_structures::{Calendar, CalendarDate, Exception, Gtfs};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

/// Immutable static GTFS snapshot. No expanded gtfs_structures::Trip or
/// StopTime arrays survive building the Maple Syrup index.
#[derive(Default)]
struct GtfsData {
    tiploc_map: HashMap<CompactString, CompactString>,
    maple: MapleSyrup,
    calendar: HashMap<CompactString, Calendar>,
    // Sorted by date; binary searched for service exceptions.
    calendar_dates: HashMap<CompactString, Vec<CalendarDate>>,
}

pub struct GTFSManager {
    url: String,
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
                        let new_data = Self::build_indices(new_gtfs);
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
                thread::sleep(Duration::from_secs(3600));
            }
        });
    }

    pub fn load_initial(&self) -> Result<()> {
        log_info("Performing initial GTFS load...");
        let new_gtfs = Self::download_and_load(&self.url)?;
        println!("Building Maple Syrup indices...");
        let new_data = Self::build_indices(new_gtfs);
        {
            let mut d = self.data.write().unwrap();
            *d = new_data;
        }
        log_info("Initial GTFS load complete.");
        Ok(())
    }

    pub fn get_stop_id(&self, tiploc: &str) -> Option<CompactString> {
        self.data.read().unwrap().tiploc_map.get(tiploc).cloned()
    }

    /// Single snapshot lookup for all fields needed by Darwin TrainStatus.
    /// A cloned Arc<StopPattern> outlives a concurrent hourly GTFS refresh.
    pub fn find_matching_trip(&self, uid: &str, date: NaiveDate) -> Option<MatchedTrip> {
        let data = self.data.read().unwrap();
        for &index in data.maple.uid_trip_indices(uid) {
            let Some(trip) = data.maple.trip_by_index(index) else {
                continue;
            };
            if Self::service_runs_on_date(&data, &trip.service_id, date) {
                return Some(data.maple.matched(trip));
            }
        }
        None
    }

    fn service_runs_on_date(data: &GtfsData, service_id: &str, date: NaiveDate) -> bool {
        if let Some(exceptions) = data.calendar_dates.get(service_id) {
            let pos = exceptions.partition_point(|entry| entry.date < date);
            if let Some(exception) = exceptions.get(pos).filter(|entry| entry.date == date) {
                return exception.exception_type == Exception::Added;
            }
        }

        if let Some(cal) = data.calendar.get(service_id) {
            if date >= cal.start_date && date <= cal.end_date {
                return match date.weekday() {
                    chrono::Weekday::Mon => cal.monday,
                    chrono::Weekday::Tue => cal.tuesday,
                    chrono::Weekday::Wed => cal.wednesday,
                    chrono::Weekday::Thu => cal.thursday,
                    chrono::Weekday::Fri => cal.friday,
                    chrono::Weekday::Sat => cal.saturday,
                    chrono::Weekday::Sun => cal.sunday,
                };
            }
        }
        false
    }

    pub fn has_data(&self) -> bool {
        !self.data.read().unwrap().tiploc_map.is_empty()
    }

    fn download_and_load(url: &str) -> Result<Gtfs> {
        let gtfs = Gtfs::new(url).map_err(|e| anyhow::anyhow!("GTFS error: {:?}", e))?;
        println!("Downloaded GTFS");
        Ok(gtfs)
    }

    /// Consume the parser's trip map rather than cloning every Trip and its
    /// expanded StopTime vector into a second giant HashMap.
    fn build_indices(gtfs: Gtfs) -> GtfsData {
        let mut data = GtfsData::default();

        for (id, stop) in &gtfs.stops {
            data.tiploc_map.insert(CompactString::from(id.as_str()), CompactString::from(id.as_str()));
            if let Some(code) = &stop.code {
                data.tiploc_map.insert(CompactString::from(code.as_str()), CompactString::from(id.as_str()));
            }
        }
        log_info("Built TIPLOC map");

        for (service_id, cal) in gtfs.calendar {
            data.calendar.insert(CompactString::from(service_id), cal);
        }

        for (service_id, mut dates) in gtfs.calendar_dates {
            // Stable sorting preserves the original precedence of duplicate
            // same-day exception rows, if the source feed contains them.
            dates.sort_by_key(|entry| entry.date);
            data.calendar_dates.insert(CompactString::from(service_id), dates);
        }

        data.maple = MapleSyrup::from_trips(gtfs.trips);

        let (trips, patterns, expanded) = data.maple.stats();
        println!(
            "Maple Syrup: {} trips, {} unique stop-time patterns, {} original stop_times",
            trips, patterns, expanded
        );
        println!(
            "GTFS indices built: {} TIPLOC mappings, {} services",
            data.tiploc_map.len(), data.calendar.len()
        );
        data
    }
}

fn log_info(msg: &str) {
    println!("[{}] {}", chrono::Utc::now().to_rfc3339(), msg);
}
