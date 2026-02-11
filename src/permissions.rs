use crate::{db::Db, errors::ApiError};

pub async fn require_admin(db: &Db, user_id: &str) -> Result<(), ApiError> {
    let row = sqlx::query(
        "SELECT 1 FROM user_roles ur INNER JOIN roles r ON ur.role_id = r.id WHERE ur.user_id = ? AND r.name = 'admin' LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&db.0)
    .await?;

    if row.is_some() {
        Ok(())
    } else {
        Err(ApiError::Forbidden)
    }
}
