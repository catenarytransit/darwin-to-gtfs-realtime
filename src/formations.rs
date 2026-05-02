pub mod v1 {
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
        #[serde(rename = "@src", skip_serializing_if = "Option::is_none")]
        pub src: Option<CompactString>,
        #[serde(rename = "@srcInst", skip_serializing_if = "Option::is_none")]
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
        #[serde(rename = "@coachClass", skip_serializing_if = "Option::is_none")]
        pub coach_class: Option<CompactString>,
    }
}

pub mod v2 {
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
        #[serde(rename = "@src", skip_serializing_if = "Option::is_none")]
        pub src: Option<CompactString>,
        #[serde(rename = "@srcInst", skip_serializing_if = "Option::is_none")]
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
        #[serde(rename = "@coachClass", skip_serializing_if = "Option::is_none")]
        pub coach_class: Option<CompactString>,
        #[serde(rename = "toilet", default, skip_serializing_if = "Option::is_none")]
        pub toilet: Option<ToiletAvailabilityType>,
    }

    #[derive(Debug, Deserialize, Serialize, Clone)]
    pub struct ToiletAvailabilityType {
        #[serde(rename = "$value", default, skip_serializing_if = "Option::is_none")]
        pub status: Option<CompactString>,

        #[serde(rename = "@status", default, skip_serializing_if = "Option::is_none")]
        pub status_attr: Option<CompactString>,
    }
}

impl From<v2::ScheduleFormations> for v1::ScheduleFormations {
    fn from(v2: v2::ScheduleFormations) -> Self {
        v1::ScheduleFormations {
            rid: v2.rid,
            formations: v2.formations.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<v2::Formation> for v1::Formation {
    fn from(v2: v2::Formation) -> Self {
        v1::Formation {
            fid: v2.fid,
            src: v2.src,
            src_inst: v2.src_inst,
            coaches: v2.coaches.into(),
        }
    }
}

impl From<v2::CoachList> for v1::CoachList {
    fn from(v2: v2::CoachList) -> Self {
        v1::CoachList {
            coaches: v2.coaches.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<v2::CoachData> for v1::CoachData {
    fn from(v2: v2::CoachData) -> Self {
        v1::CoachData {
            coach_number: v2.coach_number,
            coach_class: v2.coach_class,
        }
    }
}
