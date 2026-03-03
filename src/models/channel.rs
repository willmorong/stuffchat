use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub is_voice: bool,
    pub is_private: bool,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}
