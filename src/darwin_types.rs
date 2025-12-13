use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename = "Pport")]
pub struct Pport {
    #[serde(rename = "uR")]
    pub update_record: Option<UpdateRecord>,
    #[serde(rename = "sR")]
    pub schedule_record: Option<ScheduleRecord>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRecord {
    #[serde(rename = "@updateOrigin")]
    pub update_origin: Option<String>,
    #[serde(rename = "TS", default)]
    pub train_status: Vec<TrainStatus>,
    #[serde(rename = "TO", default)]
    pub train_order: Vec<TrainOrder>,
    #[serde(rename = "OW", default)]
    pub station_message: Vec<StationMessage>,
    #[serde(rename = "loading", default)]
    pub loading: Vec<Loading>, // Note: Check if capital 'L' or lowercase. Usually strict XML is sensitive.
    // Based on user request/Darwin schema, Loading might be new v16 topic but typically LO?
    // User message said "Loading LO". I will add alias.
    #[serde(alias = "LO", default)]
    pub loading_alias: Vec<Loading>,

    #[serde(rename = "association", default)]
    pub association: Vec<Association>,
    #[serde(rename = "formation", default)]
    pub formation: Vec<Formation>,
    #[serde(rename = "trainAlert", default)]
    pub train_alert: Vec<TrainAlert>,
    #[serde(rename = "trackingId", default)]
    pub tracking_id: Vec<TrackingId>,
    #[serde(rename = "alarm", default)]
    pub rtti_alarm: Vec<RTTIAlarm>,
}

#[derive(Debug, Deserialize)]
pub struct Formation {
    #[serde(rename = "@rid")]
    pub rid: String,
    #[serde(rename = "coach", default)] // Assuming 'coach' is the element name
    pub coaches: Vec<Coach>,
}

#[derive(Debug, Deserialize)]
pub struct Coach {
    #[serde(rename = "@number")]
    pub number: String,
    #[serde(rename = "@coachClass")]
    pub class: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ScheduleRecord {
    #[serde(rename = "schedule")]
    pub schedule: Option<Vec<Schedule>>,
}

#[derive(Debug, Deserialize)]
pub struct Schedule {
    #[serde(rename = "@rid")]
    pub rid: String,
    #[serde(rename = "@uid")]
    pub uid: String,
    #[serde(rename = "@ssd")]
    pub ssd: String,
}

#[derive(Debug, Deserialize)]
pub struct TrainStatus {
    #[serde(rename = "@rid")]
    pub rid: String,
    #[serde(rename = "@uid")]
    pub uid: String,
    #[serde(rename = "@ssd")]
    pub ssd: String,
    #[serde(rename = "@isActive")]
    pub is_active: Option<bool>,
    #[serde(rename = "LateReason")]
    pub late_reason: Option<LateReason>,
    #[serde(rename = "Location", default)]
    pub locations: Vec<Location>,
}

#[derive(Debug, Deserialize)]
pub struct LateReason {
    #[serde(rename = "$value")]
    pub value: Option<String>, // Reason Text
}

#[derive(Debug, Deserialize)]
pub struct Location {
    #[serde(rename = "@tpl")]
    pub tiploc: Option<String>,
    #[serde(rename = "@wta")]
    pub wta: Option<String>,
    #[serde(rename = "@wtp")]
    pub wtp: Option<String>,
    #[serde(rename = "@wtd")]
    pub wtd: Option<String>,
    #[serde(rename = "@ptd")]
    pub ptd: Option<String>,
    #[serde(rename = "plat")]
    pub platform: Option<Platform>,
    #[serde(rename = "suppr")]
    pub suppr: Option<bool>, // Add suppr flag
    #[serde(rename = "arr")]
    pub arr: Option<Forecast>,
    #[serde(rename = "dep")]
    pub dep: Option<Forecast>,
    #[serde(rename = "pass")]
    pub pass: Option<Forecast>,
    #[serde(rename = "length")]
    pub length: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Platform {
    #[serde(rename = "$value")]
    pub number: Option<String>,
    #[serde(rename = "@cisPlatsup")]
    pub cis_suppressed: Option<bool>,
    #[serde(rename = "@platsup")]
    pub platsup: Option<bool>, // Suppressed from public view
    #[serde(rename = "@conf")]
    pub conf: Option<bool>, // Confirmed
    #[serde(rename = "@platsrc")]
    pub platsrc: Option<String>, // Source (A = Automatic, M = Manual)
    #[serde(rename = "cisPlatsup")]
    pub cis_platsup_elem: Option<bool>, // Sometimes element? (Schema says attr, but user note had element example?) No, user example had attr.
                                        // User example: <fc:plat platsup="true" cisPlatsup="true">2</fc:plat> -> attributes.
}

#[derive(Debug, Deserialize)]
pub struct Forecast {
    #[serde(rename = "@et")]
    pub et: Option<String>,
    #[serde(rename = "@at")]
    pub at: Option<String>,
}

// New Types

#[derive(Debug, Deserialize)]
pub struct TrainOrder {
    #[serde(rename = "@tiploc")]
    pub tiploc: String,
    #[serde(rename = "@crs")]
    pub crs: String,
    #[serde(rename = "@platform")]
    pub platform: Option<String>,
    #[serde(rename = "set")]
    pub set: Option<TrainOrderSet>,
    #[serde(rename = "clear")]
    pub clear: Option<TrainOrderClear>,
}

#[derive(Debug, Deserialize)]
pub struct TrainOrderSet {
    #[serde(rename = "first")]
    pub first: Option<TrainOrderItem>,
    #[serde(rename = "second")]
    pub second: Option<TrainOrderItem>,
    #[serde(rename = "third")]
    pub third: Option<TrainOrderItem>,
}

#[derive(Debug, Deserialize)]
pub struct TrainOrderClear {
    // Empty element usually
}

#[derive(Debug, Deserialize)]
pub struct TrainOrderItem {
    #[serde(rename = "rid")]
    pub rid: Option<TrainOrderRid>,
    #[serde(rename = "trainID")]
    pub train_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TrainOrderRid {
    #[serde(rename = "$value")]
    pub value: String, // The RID
    #[serde(rename = "@wta")]
    pub wta: Option<String>,
    #[serde(rename = "@wtd")]
    pub wtd: Option<String>,
    #[serde(rename = "@pta")]
    pub pta: Option<String>,
    #[serde(rename = "@ptd")]
    pub ptd: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StationMessage {
    #[serde(rename = "@id")]
    pub id: String,
    #[serde(rename = "@cat")]
    pub category: String,
    #[serde(rename = "Msg")]
    pub message: String, // Msg is an element
    #[serde(rename = "Station")]
    pub stations: Option<Vec<StationMessageStation>>,
}

#[derive(Debug, Deserialize)]
pub struct StationMessageStation {
    #[serde(rename = "@crs")]
    pub crs: String,
}

#[derive(Debug, Deserialize)]
pub struct Loading {
    #[serde(rename = "@rid")]
    pub rid: String,
    // Add loading fields as needed
}

// Completed Structs
#[derive(Debug, Deserialize)]
pub struct Association {
    #[serde(rename = "@tiploc")]
    pub tiploc: String,
    #[serde(rename = "@category")]
    pub category: String, // JJ=Join, VV=Divide, NP=NextWorking, etc.
    #[serde(rename = "main")]
    pub main: AssociationService,
    #[serde(rename = "assoc")]
    pub assoc: AssociationService,
}

#[derive(Debug, Deserialize)]
pub struct AssociationService {
    #[serde(rename = "@rid")]
    pub rid: String,
    #[serde(rename = "@pta")]
    pub pta: Option<String>,
    #[serde(rename = "@ptd")]
    pub ptd: Option<String>,
    #[serde(rename = "@wta")]
    pub wta: Option<String>,
    #[serde(rename = "@wtd")]
    pub wtd: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TrainAlert {
    #[serde(rename = "@id")]
    pub id: String,
    // Alert logic is complex, often involves referencing other alerts or messages.
    // Placeholder for now sufficient until specific alert logic requested?
    // User requested "Fill in the other placeholder types".
    // Basic structure:
    #[serde(rename = "AlertWithdrawn")]
    pub withdrawn: Option<String>, // Element
    #[serde(rename = "AlertService")]
    pub service: Option<Vec<AlertService>>,
}

#[derive(Debug, Deserialize)]
pub struct AlertService {
    #[serde(rename = "@rid")]
    pub rid: String,
    #[serde(rename = "Location")]
    pub location: Option<Vec<String>>, // Tpl
}

#[derive(Debug, Deserialize)]
pub struct TrackingId {
    #[serde(rename = "@id")]
    pub id: String,
    #[serde(rename = "@correct")]
    pub correct: bool, // "true" or "false"
}

#[derive(Debug, Deserialize)]
pub struct RTTIAlarm {
    #[serde(rename = "@id")]
    pub id: String,
    #[serde(rename = "set")]
    pub set: Option<RTTIAlarmSet>,
    #[serde(rename = "clear")]
    pub clear: Option<String>, // Element value is ID usually? Or empty? Wiki says "clear element and the same unique identifier".
}

#[derive(Debug, Deserialize)]
pub struct RTTIAlarmSet {
    #[serde(rename = "@tdAreaFail")]
    pub td_area_fail: Option<bool>,
    #[serde(rename = "@tyrell")]
    pub tyrell: Option<bool>,
    // and others
}

#[cfg(test)]
mod tests {
    use super::*;
    use quick_xml::de::from_str;

    #[test]
    fn test_reproduce_ts_error() {
        // 1. Simplest - Empty TS
        let xml_empty = r#"<Pport ts="T" version="16.0"><uR updateOrigin="TD"><TS rid="1" uid="U" ssd="D"></TS></uR></Pport>"#;
        let res: Result<Pport, _> = from_str(xml_empty);
        assert!(res.is_ok(), "Failed empty TS: {:?}", res.err());

        // 2. TS with Location (Simple)
        let xml_loc = r#"<Pport ts="T" version="16.0"><uR updateOrigin="TD"><TS rid="1" uid="U" ssd="D"><Location tpl="L1"></Location></TS></uR></Pport>"#;
        let res: Result<Pport, _> = from_str(xml_loc);
        assert!(res.is_ok(), "Failed simple Location: {:?}", res.err());

        // 3. TS with Location (With children)
        let xml_full = r#"<Pport ts="T" version="16.0"><uR updateOrigin="TD"><TS rid="1" uid="U" ssd="D"><Location tpl="L1"><length>4</length></Location></TS></uR></Pport>"#;
        let res: Result<Pport, _> = from_str(xml_full);
        match res {
            Ok(_) => println!("Successfully parsed Full Pport"),
            Err(e) => panic!("Failed full parsing: {:?}", e),
        }
    }
}
