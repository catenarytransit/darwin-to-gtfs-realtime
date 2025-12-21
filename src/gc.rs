use crate::state::AppState;
use chrono::Utc;
use compact_str::CompactString;
use std::time::Duration;

pub fn cleanup_old_trips(state: &AppState, threshold: Duration) {
    let now = Utc::now().timestamp();
    let threshold_secs = threshold.as_secs() as i64;

    // We want to find trips where the LAST stop time update + threshold < now.
    // Or if there are no stop time updates, maybe use start_date?
    // Let's iterate over trip_updates.

    let mut trips_to_remove: Vec<CompactString> = Vec::new();

    for r in state.trip_updates.iter() {
        let trip_id = r.key();
        let entity = r.value();

        let mut max_time: Option<i64> = None;

        if let Some(tu) = &entity.trip_update {
            for stu in &tu.stop_time_update {
                if let Some(arrival) = &stu.arrival {
                    if let Some(t) = arrival.time {
                        max_time = Some(max_time.map_or(t, |m| m.max(t)));
                    }
                }
                if let Some(departure) = &stu.departure {
                    if let Some(t) = departure.time {
                        max_time = Some(max_time.map_or(t, |m| m.max(t)));
                    }
                }
            }
        }

        // Check if expired
        if let Some(last_activity) = max_time {
            if last_activity + threshold_secs < now {
                trips_to_remove.push(trip_id.clone());
            }
        } else {
            // If no time data, maybe it's very old? Or very new?
            // Safest is to keep it unless we can prove it's old.
            // But if we have start_date, we could use that.
            // For now, let's only remove if we have explicit time updates that are old.
            // Actually, if a trip has NO updates, it might be zombie.
            // But let's stick to the "ended" logic (has times, and they are in the past).
        }
    }

    let count = trips_to_remove.len();
    if count > 0 {
        println!("GC: Found {} expired trips. Cleaning up...", count);

        // Remove from trip_updates
        for trip_id in &trips_to_remove {
            state.trip_updates.remove(trip_id);
            state.platforms_v2.remove(trip_id);
        }

        // Clean up rid_to_trip_id
        // This is a reverse lookup. We need to scan it.
        // DashMap doesn't support easy retain or remove_by_value without scanning?
        // We can scan and collect keys to remove.
        let mut rids_to_remove: Vec<CompactString> = Vec::new();
        for r in state.rid_to_trip_id.iter() {
            if trips_to_remove.contains(r.value()) {
                rids_to_remove.push(r.key().clone());
            }
        }

        for rid in rids_to_remove {
            state.rid_to_trip_id.remove(&rid);
        }

        println!("GC: Cleanup complete.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use gtfs_realtime::{
        FeedEntity, TripUpdate,
        trip_update::{StopTimeEvent, StopTimeUpdate},
    };

    #[test]
    fn test_cleanup_old_trips() {
        // Mock state
        // creating AppState requires a GTFS URL, but we can pass a dummy one if it doesn't try to load immediately?
        // AppState::new does generic init. GTFSManager might try something?
        // GTFSManager::new just sets up structure.

        let state = AppState::new("http://localhost".to_string());

        let now = Utc::now().timestamp();

        // 1. Create Active Trip (Current time)
        let active_trip_id = CompactString::from("trip_active");
        let mut active_fe = FeedEntity::default();
        active_fe.id = active_trip_id.to_string();
        let mut active_tu = TripUpdate::default();
        let mut active_stu = StopTimeUpdate::default();
        active_stu.departure = Some(StopTimeEvent {
            time: Some(now),
            delay: None,
            uncertainty: None,
            scheduled_time: None,
        });
        active_tu.stop_time_update.push(active_stu);
        active_fe.trip_update = Some(active_tu);

        state.trip_updates.insert(active_trip_id.clone(), active_fe);

        // 2. Create Old Trip (2 hours ago)
        let old_trip_id = CompactString::from("trip_old");
        let mut old_fe = FeedEntity::default();
        old_fe.id = old_trip_id.to_string();
        let mut old_tu = TripUpdate::default();
        let mut old_stu = StopTimeUpdate::default();
        old_stu.departure = Some(StopTimeEvent {
            time: Some(now - 7200),
            delay: None,
            uncertainty: None,
            scheduled_time: None,
        }); // 2 hours ago
        old_tu.stop_time_update.push(old_stu);
        old_fe.trip_update = Some(old_tu);

        state.trip_updates.insert(old_trip_id.clone(), old_fe);

        // 3. RID mappings
        state
            .rid_to_trip_id
            .insert("rid_active".into(), active_trip_id.clone());
        state
            .rid_to_trip_id
            .insert("rid_old".into(), old_trip_id.clone());

        // Run GC with 1 hour threshold
        cleanup_old_trips(&state, Duration::from_secs(3600));

        // Assertions
        assert!(
            state.trip_updates.contains_key(&active_trip_id),
            "Active trip should remain"
        );
        assert!(
            !state.trip_updates.contains_key(&old_trip_id),
            "Old trip should be removed"
        );

        assert!(
            state.rid_to_trip_id.contains_key("rid_active"),
            "Active RID should remain"
        );
        assert!(
            !state.rid_to_trip_id.contains_key("rid_old"),
            "Old RID should be removed"
        );
    }
}
