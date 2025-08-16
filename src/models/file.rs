use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileMeta {
    pub id: String,
    pub user_id: String,
    pub original_name: String,
    pub stored_name: String,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub created_at: DateTime<Utc>,
}