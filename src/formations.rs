use compact_str::CompactString;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ScheduleFormations {
    #[serde(rename = "@rid")]
    pub rid: CompactString,
    #[serde(rename = "formation", default)]
    pub formations: Vec<Formation>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Formation {
    #[serde(rename = "@fid")]
    pub fid: CompactString,
    #[serde(rename = "@src")]
    pub src: Option<CompactString>,
    #[serde(rename = "@srcInst")]
    pub src_inst: Option<CompactString>,
    #[serde(rename = "coaches")]
    pub coaches: CoachList,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CoachList {
    #[serde(rename = "coach", default)]
    pub coaches: Vec<CoachData>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CoachData {
    #[serde(rename = "@coachNumber")]
    pub coach_number: CompactString,
    #[serde(rename = "@coachClass")]
    pub coach_class: Option<CompactString>,
    #[serde(rename = "toilet", default)]
    pub toilet: Option<ToiletAvailabilityType>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ToiletAvailabilityType {
    // If it's a simple type like string or attribute $status. We'll map value if simple.
    #[serde(rename = "$value", default)]
    pub status: Option<CompactString>,

    // In case it has attributes
    #[serde(rename = "@status", default)]
    pub status_attr: Option<CompactString>,
}
