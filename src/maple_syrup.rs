//! Maple Syrup representation of static GTFS trips.
//!
//! A parsed GTFS Trip owns a large Vec<StopTime>. Many National Rail services
//! share the same ordered stops and relative arrival/departure offsets.
//! Intern those itineraries, then retain only a start time and a pattern index
//! per trip. Only the subset of GTFS metadata consumed by Darwin is retained.
//!
//! Trip IDs and UIDs have sorted, binary-searchable lookup tables. Every
//! pattern also has a sorted posting index for stop IDs, enabling forward
//! stop matching in O(log n) without breaking repeated-station (loop) trips.

use compact_str::CompactString;
use gtfs_structures::Trip;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct StopOffset {
    id: CompactString,
    sequence: u32,
    arrival_offset: Option<u32>,
    departure_offset: Option<u32>,
}

#[derive(Debug)]
pub struct StopPattern {
    stops: Vec<StopOffset>,
    // Positions in 'stops', sorted by stop ID then original position.
    by_stop_id: Vec<u32>,
}

impl StopPattern {
    fn new(stops: Vec<StopOffset>) -> Self {
        let mut by_stop_id: Vec<u32> = (0..stops.len())
            .map(|i| u32::try_from(i).expect("GTFS trip has more than u32::MAX stops"))
            .collect();
        by_stop_id.sort_unstable_by(|left, right| {
            stops[*left as usize].id.cmp(&stops[*right as usize].id)
                .then_with(|| left.cmp(right))
        });
        Self { stops, by_stop_id }
    }

    fn next_stop(&self, id: &str, from: &mut usize) -> Option<u32> {
        // Binary-search the stop-ID postings, then the first original position
        // at/after the cursor. This preserves the original forward-greedy
        // matching semantics for trips visiting the same station twice.
        let low = self.by_stop_id.partition_point(|&index| {
            self.stops[index as usize].id.as_str() < id
        });
        let high = self.by_stop_id.partition_point(|&index| {
            self.stops[index as usize].id.as_str() <= id
        });
        let matching = &self.by_stop_id[low..high];
        let offset = matching.partition_point(|&index| (index as usize) < *from);
        let found = *matching.get(offset)? as usize;
        *from = found + 1;
        Some(self.stops[found].sequence)
    }

    pub fn len(&self) -> usize {
        self.stops.len()
    }
}

#[derive(Debug)]
pub struct CompactTrip {
    id: CompactString,
    pub service_id: CompactString,
    // Earliest GTFS departure, as used by the original Darwin clock rollover
    // conversion; may be >= 24:00:00. No departure means None, not 00:00.
    earliest_departure: Option<u32>,
    origin_seconds: u32,
    pattern_index: u32,
}

#[derive(Debug)]
struct UidEntry {
    uid: CompactString,
    indices: Vec<u32>,
}

#[derive(Debug, Default)]
pub struct MapleSyrup {
    trips: Vec<CompactTrip>, // sorted by trip ID
    patterns: Vec<Arc<StopPattern>>,
    uids: Vec<UidEntry>, // sorted by UID
    original_stop_times: usize,
}

#[derive(Debug, Clone)]
pub struct MatchedTrip {
    pub trip_id: CompactString,
    pub trip_start_seconds: u32,
    pub pattern: Arc<StopPattern>,
}

impl MatchedTrip {
    /// Finds the next matching stop after the preceding Darwin location.
    /// Successful matches advance the index; misses leave it unchanged.
    pub fn next_stop_sequence(&self, id: &str, cursor: &mut usize) -> Option<u32> {
        self.pattern.next_stop(id, cursor)
    }
}

impl MapleSyrup {
    /// Consume parsed trips, dropping expanded StopTime structs immediately.
    pub fn from_trips(trips: HashMap<String, Trip>) -> Self {
        let mut result = Self::default();
        let mut hashed_patterns: HashMap<u64, Vec<u32>> = HashMap::new();

        result.trips.reserve(trips.len());
        for (trip_id, trip) in trips {
            let origin_seconds = trip.stop_times.iter()
                .flat_map(|stop| [stop.arrival_time, stop.departure_time])
                .flatten().min().unwrap_or(0);
            let earliest_departure = trip.stop_times.iter()
                .filter_map(|stop| stop.departure_time).min();
            result.original_stop_times += trip.stop_times.len();

            // The timetable pattern is measured relative to the earliest event,
            // not the first CSV row. This handles out-of-order feeds and GTFS
            // service times >= 24h without losing the original stop order.
            let offsets: Vec<StopOffset> = trip.stop_times.into_iter()
                .map(|stop| StopOffset {
                    id: CompactString::from(stop.stop.id.as_str()),
                    sequence: stop.stop_sequence,
                    arrival_offset: stop.arrival_time.map(|t| t - origin_seconds),
                    departure_offset: stop.departure_time.map(|t| t - origin_seconds),
                }).collect();

            let mut hasher = DefaultHasher::new();
            offsets.hash(&mut hasher);
            let fingerprint = hasher.finish();
            // Never assume a 64-bit hash is collision-free.
            let existing = hashed_patterns.get(&fingerprint).and_then(|indices| {
                indices.iter().copied().find(|&idx| {
                    result.patterns[idx as usize].stops == offsets
                })
            });
            let pattern_index = match existing {
                Some(index) => index,
                None => {
                    let index = u32::try_from(result.patterns.len())
                        .expect("more than u32::MAX GTFS itinerary patterns");
                    result.patterns.push(Arc::new(StopPattern::new(offsets)));
                    hashed_patterns.entry(fingerprint).or_default().push(index);
                    index
                }
            };

            result.trips.push(CompactTrip {
                id: CompactString::from(trip_id),
                service_id: CompactString::from(trip.service_id),
                earliest_departure,
                origin_seconds,
                pattern_index,
            });
        }

        result.trips.sort_unstable_by(|a, b| a.id.cmp(&b.id));

        let mut by_uid: HashMap<CompactString, Vec<u32>> = HashMap::new();
        for (index, trip) in result.trips.iter().enumerate() {
            // Darwin UID is the portion before the first underscore in static
            // GTFS trip IDs; preserve the converter's existing convention.
            let uid = trip.id.split('_').next().unwrap_or(trip.id.as_str());
            by_uid.entry(CompactString::from(uid)).or_default()
                .push(u32::try_from(index).expect("more than u32::MAX trips"));
        }
        result.uids = by_uid.into_iter().map(|(uid, mut indices)| {
            indices.sort_unstable_by(|&left, &right| {
                let a = &result.trips[left as usize];
                let b = &result.trips[right as usize];
                a.earliest_departure.cmp(&b.earliest_departure)
                    .then_with(|| a.id.cmp(&b.id))
            });
            UidEntry { uid, indices }
        }).collect();
        result.uids.sort_unstable_by(|a, b| a.uid.cmp(&b.uid));
        result
    }

    /// Binary-search a UID rather than scanning the trip table.
    pub fn uid_trip_indices(&self, uid: &str) -> &[u32] {
        self.uids.binary_search_by(|entry| entry.uid.as_str().cmp(uid))
            .ok().map(|i| self.uids[i].indices.as_slice())
            .unwrap_or(&[])
    }

    pub fn trip_by_index(&self, index: u32) -> Option<&CompactTrip> {
        self.trips.get(index as usize)
    }

    /// Binary-search an exact GTFS trip ID.
    pub fn trip(&self, id: &str) -> Option<&CompactTrip> {
        let index = self.trips.binary_search_by(|trip| trip.id.as_str().cmp(id)).ok()?;
        self.trips.get(index)
    }

    pub fn matched(&self, trip: &CompactTrip) -> MatchedTrip {
        MatchedTrip {
            trip_id: trip.id.clone(),
            trip_start_seconds: trip.earliest_departure.unwrap_or(0),
            pattern: Arc::clone(&self.patterns[trip.pattern_index as usize]),
        }
    }

    /// Reconstruct scheduled arrival/departure if a future Darwin matcher
    /// needs a specific stop time; no expanded Trip is retained.
    pub fn scheduled_time(&self, trip_id: &str, index: usize)
        -> Option<(Option<u32>, Option<u32>)>
    {
        let trip = self.trip(trip_id)?;
        let stop = self.patterns[trip.pattern_index as usize].stops.get(index)?;
        Some((
            stop.arrival_offset.map(|offset| trip.origin_seconds + offset),
            stop.departure_offset.map(|offset| trip.origin_seconds + offset),
        ))
    }

    pub fn stats(&self) -> (usize, usize, usize) {
        (self.trips.len(), self.patterns.len(), self.original_stop_times)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtfs_structures::{Stop, StopTime};
    use std::sync::Arc;

    fn make_trip(id: &str, start: u32, station: Arc<Stop>) -> Trip {
        Trip {
            id: id.into(),
            service_id: "weekday".into(),
            route_id: "rail".into(),
            stop_times: vec![
                StopTime { stop: station.clone(), stop_sequence: 10,
                    arrival_time: Some(start), departure_time: Some(start + 45),
                    ..Default::default() },
                StopTime { stop: station.clone(), stop_sequence: 20,
                    arrival_time: None, departure_time: Some(start + 120),
                    ..Default::default() },
                StopTime { stop: station, stop_sequence: 30,
                    arrival_time: Some(start + 280), departure_time: None,
                    ..Default::default() },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn compresses_shifted_schedules_and_keeps_overnight_times() {
        let station = Arc::new(Stop { id: "EUS".into(), ..Default::default() });
        let one = make_trip("A123_1", 18 * 3600, station.clone());
        let two = make_trip("A123_2", 25 * 3600, station);
        let mut trips = HashMap::new();
        trips.insert(one.id.clone(), one);
        trips.insert(two.id.clone(), two);

        let maple = MapleSyrup::from_trips(trips);
        assert_eq!(maple.stats(), (2, 1, 6));
        assert_eq!(maple.uid_trip_indices("A123").len(), 2);
        assert!(maple.trip("A123_1").is_some());
        assert_eq!(maple.scheduled_time("A123_2", 0),
            Some((Some(25 * 3600), Some(25 * 3600 + 45))));
        assert_eq!(maple.scheduled_time("A123_2", 1),
            Some((None, Some(25 * 3600 + 120))));
        assert_eq!(maple.matched(maple.trip("A123_2").unwrap()).trip_start_seconds,
            25 * 3600 + 45);
    }

    #[test]
    fn indexed_stop_search_preserves_forward_loop_occurrences() {
        let station = Arc::new(Stop { id: "LOOP".into(), ..Default::default() });
        let mut trips = HashMap::new();
        let trip = make_trip("U_1", 200, station);
        trips.insert(trip.id.clone(), trip);
        let maple = MapleSyrup::from_trips(trips);
        let snapshot = maple.matched(maple.trip("U_1").unwrap());
        let mut cursor = 0;
        assert_eq!(snapshot.next_stop_sequence("LOOP", &mut cursor), Some(10));
        assert_eq!(snapshot.next_stop_sequence("LOOP", &mut cursor), Some(20));
        assert_eq!(snapshot.next_stop_sequence("NOT_FOUND", &mut cursor), None);
        assert_eq!(snapshot.next_stop_sequence("LOOP", &mut cursor), Some(30));
        assert_eq!(snapshot.next_stop_sequence("LOOP", &mut cursor), None);
    }

    #[test]
    fn different_relative_timetables_are_not_interned() {
        let station = Arc::new(Stop { id: "KGX".into(), ..Default::default() });
        let mut one = make_trip("X_1", 200, station.clone());
        let mut two = make_trip("X_2", 300, station);
        two.stop_times[1].departure_time = Some(460);
        let mut trips = HashMap::new();
        trips.insert(one.id.clone(), std::mem::take(&mut one));
        trips.insert(two.id.clone(), two);
        let maple = MapleSyrup::from_trips(trips);
        assert_eq!(maple.stats().1, 2);
    }
}
