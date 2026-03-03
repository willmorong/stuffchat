use chrono::Utc;
use serde::Serialize;
use serde_json::Value;
use sqlx::{Executor, Row, Sqlite};

use crate::db::Db;

#[derive(Debug, Serialize)]
pub struct AdminLogActor {
    pub id: String,
    pub username: String,
}

#[derive(Debug, Serialize)]
pub struct AdminLogEntry {
    pub id: String,
    pub actor: AdminLogActor,
    pub action_type: String,
    pub action_info: Value,
    pub created_at: chrono::DateTime<Utc>,
}

pub async fn record_admin_log<'e, E>(
    executor: E,
    actor_user_id: &str,
    action_type: &str,
    action_info: &Value,
) -> Result<(), sqlx::Error>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "INSERT INTO admin_log(id, actor_user_id, action_type, action_info, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(actor_user_id)
    .bind(action_type)
    .bind(action_info.to_string())
    .bind(Utc::now())
    .execute(executor)
    .await?;

    Ok(())
}

pub async fn list_admin_logs(db: &Db, limit: i64) -> Result<Vec<AdminLogEntry>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT al.id, al.action_type, al.action_info, al.created_at,
               u.id AS actor_id, u.username AS actor_username
        FROM admin_log al
        INNER JOIN users u ON u.id = al.actor_user_id
        ORDER BY al.created_at DESC, al.id DESC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(&db.0)
    .await?;

    rows.into_iter()
        .map(|row| {
            let action_info: String = row.get("action_info");
            let action_info =
                serde_json::from_str(&action_info).unwrap_or(Value::String(action_info));

            Ok(AdminLogEntry {
                id: row.get("id"),
                actor: AdminLogActor {
                    id: row.get("actor_id"),
                    username: row.get("actor_username"),
                },
                action_type: row.get("action_type"),
                action_info,
                created_at: row.get("created_at"),
            })
        })
        .collect()
}
