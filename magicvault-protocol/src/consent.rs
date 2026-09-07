//! Read/revoke projections only. An agent cannot submit a human allow decision.
use crate::FillField;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConsentBrowser {
    Extension {
        extension_id: String,
        profile_id: Uuid,
    },
    /// CDP and trusted custom adapters: valid only for this live connection.
    Session { browser_handle: Uuid },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConsentScope {
    /// Profiles are immutable and bind their executable digest/destination.
    Delivery { profile_id: Uuid },
    Browser {
        browser: ConsentBrowser,
        top_origin: String,
        frame_origin: String,
        is_main_frame: bool,
        fields: Vec<FillField>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConsentGrantInfo {
    pub grant_id: Uuid,
    pub label: String,
    pub scope: ConsentScope,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsentQuery {
    pub grant_id: Uuid,
}
