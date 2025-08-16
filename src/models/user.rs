use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct User {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    pub avatar_file_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct PublicUser {
    pub id: String,
    pub username: String,
    pub avatar_file_id: Option<String>,
}

impl From<User> for PublicUser {
    fn from(u: User) -> Self {
        Self { id: u.id, username: u.username, avatar_file_id: u.avatar_file_id }
    }
}