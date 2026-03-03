use actix_multipart::Multipart;
use actix_web::{HttpResponse, web};
use futures_util::TryStreamExt as _;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;

use crate::{
    admin_log,
    auth::{self, AuthUser},
    config::Config,
    db::Db,
    errors::ApiError,
    permissions::require_admin,
    ws::server::{BroadcastAll, ChatServer},
};

#[derive(Serialize)]
struct RoleInfo {
    id: String,
    name: String,
    permissions: i64,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize)]
struct UserRole {
    id: String,
    name: String,
}

#[derive(Serialize)]
struct AdminUser {
    id: String,
    username: String,
    email: Option<String>,
    avatar_file_id: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    roles: Vec<UserRole>,
}

pub async fn list_users(db: web::Data<Db>, user: AuthUser) -> Result<HttpResponse, ApiError> {
    require_admin(&db, &user.user_id).await?;

    let rows = sqlx::query(
        r#"
        SELECT u.id, u.username, u.email, u.avatar_file_id, u.created_at, u.updated_at,
               r.id as role_id, r.name as role_name
        FROM users u
        LEFT JOIN user_roles ur ON ur.user_id = u.id
        LEFT JOIN roles r ON r.id = ur.role_id
        ORDER BY u.username ASC
        "#,
    )
    .fetch_all(&db.0)
    .await?;

    let mut by_user: std::collections::BTreeMap<String, AdminUser> =
        std::collections::BTreeMap::new();
    for row in rows {
        let user_id: String = row.get("id");
        let entry = by_user.entry(user_id.clone()).or_insert_with(|| AdminUser {
            id: user_id.clone(),
            username: row.get("username"),
            email: row.get("email"),
            avatar_file_id: row.get("avatar_file_id"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            roles: Vec::new(),
        });
        let role_id: Option<String> = row.get("role_id");
        let role_name: Option<String> = row.get("role_name");
        if let (Some(id), Some(name)) = (role_id, role_name) {
            entry.roles.push(UserRole { id, name });
        }
    }

    Ok(HttpResponse::Ok().json(by_user.into_values().collect::<Vec<_>>()))
}

#[derive(Deserialize)]
pub struct UpdateUserReq {
    pub username: Option<String>,
    pub email: Option<String>,
}

pub async fn update_user(
    db: web::Data<Db>,
    chat: web::Data<actix::Addr<ChatServer>>,
    user: AuthUser,
    path: web::Path<String>,
    body: web::Json<UpdateUserReq>,
) -> Result<HttpResponse, ApiError> {
    require_admin(&db, &user.user_id).await?;
    let target_id = path.into_inner();
    let username = body.username.as_ref().map(|value| value.trim().to_string());
    if username.as_deref().is_some_and(|u| u.len() < 3) {
        return Err(ApiError::BadRequest("username too short".into()));
    }
    let email = body.email.as_ref().map(|value| value.trim().to_string());

    let existing = sqlx::query("SELECT username, email FROM users WHERE id = ?")
        .bind(&target_id)
        .fetch_optional(&db.0)
        .await?;
    let existing = existing.ok_or(ApiError::NotFound)?;
    let current_username: String = existing.get("username");
    let current_email: Option<String> = existing.get("email");

    let mut changes = serde_json::Map::new();
    if let Some(value) = username.as_ref().filter(|value| value.as_str() != current_username) {
        changes.insert("username".into(), json!(value));
    }
    if email != current_email {
        changes.insert("email".into(), json!(email));
    }

    sqlx::query("UPDATE users SET username = COALESCE(?, username), email = COALESCE(?, email), updated_at = ? WHERE id = ?")
        .bind(&username)
        .bind(&email)
        .bind(chrono::Utc::now())
        .bind(&target_id)
        .execute(&db.0)
        .await?;

    if !changes.is_empty() {
        admin_log::record_admin_log(
            &db.0,
            &user.user_id,
            "user.updated",
            &json!({
                "target_user_id": target_id,
                "changes": changes,
            }),
        )
        .await?;
    }

    // Broadcast profile update
    chat.do_send(BroadcastAll {
        payload: serde_json::json!({
            "type": "user_updated",
            "user_id": target_id,
        })
        .to_string(),
    });

    log::info!(
        "AdminAction: update_user admin_id={} target_id={} username={:?} email={:?}",
        user.user_id,
        target_id,
        username,
        email
    );

    Ok(HttpResponse::Ok().finish())
}

#[derive(Deserialize)]
pub struct AdminPasswordReq {
    pub new_password: String,
}

pub async fn set_user_password(
    db: web::Data<Db>,
    user: AuthUser,
    path: web::Path<String>,
    body: web::Json<AdminPasswordReq>,
) -> Result<HttpResponse, ApiError> {
    require_admin(&db, &user.user_id).await?;
    if body.new_password.len() < 8 {
        return Err(ApiError::BadRequest("new password too short".into()));
    }
    let target_id = path.into_inner();
    let new_hash = auth::hash_password(&body.new_password)?;
    sqlx::query("UPDATE users SET password_hash = ?, updated_at = ? WHERE id = ?")
        .bind(new_hash)
        .bind(chrono::Utc::now())
        .bind(&target_id)
        .execute(&db.0)
        .await?;
    admin_log::record_admin_log(
        &db.0,
        &user.user_id,
        "user.password_set",
        &json!({
            "target_user_id": target_id,
        }),
    )
    .await?;
    log::info!(
        "AdminAction: set_user_password admin_id={} target_id={}",
        user.user_id,
        target_id
    );
    Ok(HttpResponse::Ok().finish())
}

pub async fn upload_user_avatar(
    cfg: web::Data<Config>,
    db: web::Data<Db>,
    chat: web::Data<actix::Addr<ChatServer>>,
    user: AuthUser,
    path: web::Path<String>,
    mut payload: Multipart,
) -> Result<HttpResponse, ApiError> {
    use crate::routes::files::{SavedFile, save_multipart_file};
    require_admin(&db, &user.user_id).await?;
    let target_id = path.into_inner();

    let mut saved: Option<SavedFile> = None;
    while let Some(item) = payload
        .try_next()
        .await
        .map_err(|_| ApiError::BadRequest("invalid multipart".into()))?
    {
        let field = item;
        let s = save_multipart_file(&cfg, &db, &target_id, field).await?;
        saved = Some(s);
        break;
    }
    let saved = saved.ok_or(ApiError::BadRequest("no file".into()))?;

    sqlx::query("UPDATE users SET avatar_file_id = ?, updated_at = ? WHERE id = ?")
        .bind(&saved.file_id)
        .bind(chrono::Utc::now())
        .bind(&target_id)
        .execute(&db.0)
        .await?;
    admin_log::record_admin_log(
        &db.0,
        &user.user_id,
        "user.avatar_updated",
        &json!({
            "target_user_id": target_id,
            "avatar_file_id": saved.file_id,
        }),
    )
    .await?;

    // Broadcast profile update
    chat.do_send(BroadcastAll {
        payload: serde_json::json!({
            "type": "user_updated",
            "user_id": target_id,
        })
        .to_string(),
    });

    log::info!(
        "AdminAction: upload_user_avatar admin_id={} target_id={} avatar_file_id={}",
        user.user_id,
        target_id,
        saved.file_id
    );

    Ok(HttpResponse::Ok().json(serde_json::json!({"avatar_file_id": saved.file_id})))
}

pub async fn list_roles(db: web::Data<Db>, user: AuthUser) -> Result<HttpResponse, ApiError> {
    require_admin(&db, &user.user_id).await?;
    let rows = sqlx::query("SELECT id, name, permissions, created_at FROM roles ORDER BY name ASC")
        .fetch_all(&db.0)
        .await?;
    let roles: Vec<RoleInfo> = rows
        .into_iter()
        .map(|r| RoleInfo {
            id: r.get("id"),
            name: r.get("name"),
            permissions: r.get("permissions"),
            created_at: r.get("created_at"),
        })
        .collect();
    Ok(HttpResponse::Ok().json(roles))
}

#[derive(Deserialize)]
pub struct ListAdminLogsQuery {
    pub limit: Option<i64>,
}

pub async fn list_admin_logs(
    db: web::Data<Db>,
    user: AuthUser,
    query: web::Query<ListAdminLogsQuery>,
) -> Result<HttpResponse, ApiError> {
    require_admin(&db, &user.user_id).await?;
    let limit = query.limit.unwrap_or(100).clamp(1, 200);
    let logs = admin_log::list_admin_logs(&db, limit).await?;
    Ok(HttpResponse::Ok().json(logs))
}

#[derive(Deserialize)]
pub struct CreateRoleReq {
    pub name: String,
    pub permissions: Option<i64>,
}

pub async fn create_role(
    db: web::Data<Db>,
    user: AuthUser,
    body: web::Json<CreateRoleReq>,
) -> Result<HttpResponse, ApiError> {
    require_admin(&db, &user.user_id).await?;
    let name = body.name.trim();
    if name.len() < 2 {
        return Err(ApiError::BadRequest("role name too short".into()));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now();
    let permissions = body.permissions.unwrap_or(0);

    let res =
        sqlx::query("INSERT INTO roles(id, name, permissions, created_at) VALUES (?, ?, ?, ?)")
            .bind(&id)
            .bind(name)
            .bind(permissions)
            .bind(created_at)
            .execute(&db.0)
            .await;

    match res {
        Ok(_) => {
            admin_log::record_admin_log(
                &db.0,
                &user.user_id,
                "role.created",
                &json!({
                    "role_id": id,
                    "role_name": name,
                    "permissions": permissions,
                }),
            )
            .await?;
            log::info!(
                "AdminAction: create_role admin_id={} role_id={} name={}",
                user.user_id,
                id,
                name
            );
            Ok(HttpResponse::Ok().json(RoleInfo {
                id,
                name: name.to_string(),
                permissions,
                created_at,
            }))
        }
        Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
            Err(ApiError::Conflict("role name already exists".into()))
        }
        Err(e) => Err(ApiError::from(e)),
    }
}

pub async fn delete_role(
    db: web::Data<Db>,
    user: AuthUser,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    require_admin(&db, &user.user_id).await?;
    let role_id = path.into_inner();
    let existing = sqlx::query("SELECT name FROM roles WHERE id = ?")
        .bind(&role_id)
        .fetch_optional(&db.0)
        .await?;
    let existing = existing.ok_or(ApiError::NotFound)?;
    let role_name: String = existing.get("name");
    sqlx::query("DELETE FROM roles WHERE id = ?")
        .bind(&role_id)
        .execute(&db.0)
        .await?;
    admin_log::record_admin_log(
        &db.0,
        &user.user_id,
        "role.deleted",
        &json!({
            "role_id": role_id,
            "role_name": role_name,
        }),
    )
    .await?;
    log::info!(
        "AdminAction: delete_role admin_id={} role_id={}",
        user.user_id,
        role_id
    );
    Ok(HttpResponse::Ok().finish())
}

#[derive(Deserialize)]
pub struct UpdateUserRolesReq {
    pub role_ids: Vec<String>,
}

pub async fn update_user_roles(
    db: web::Data<Db>,
    chat: web::Data<actix::Addr<ChatServer>>,
    user: AuthUser,
    path: web::Path<String>,
    body: web::Json<UpdateUserRolesReq>,
) -> Result<HttpResponse, ApiError> {
    require_admin(&db, &user.user_id).await?;
    let target_id = path.into_inner();

    let mut unique_roles: Vec<String> = body
        .role_ids
        .iter()
        .map(|r| r.trim())
        .filter(|r| !r.is_empty())
        .map(|r| r.to_string())
        .collect();
    unique_roles.sort();
    unique_roles.dedup();

    if !unique_roles.is_empty() {
        let placeholders = std::iter::repeat("?")
            .take(unique_roles.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("SELECT id FROM roles WHERE id IN ({})", placeholders);
        let mut query = sqlx::query(&sql);
        for rid in &unique_roles {
            query = query.bind(rid);
        }
        let rows = query.fetch_all(&db.0).await?;
        if rows.len() != unique_roles.len() {
            return Err(ApiError::BadRequest("unknown role id".into()));
        }
    }

    let role_rows = sqlx::query(
        r#"
        SELECT r.id, r.name
        FROM user_roles ur
        INNER JOIN roles r ON r.id = ur.role_id
        WHERE ur.user_id = ?
        ORDER BY r.name ASC
        "#,
    )
    .bind(&target_id)
    .fetch_all(&db.0)
    .await?;
    let previous_roles: Vec<serde_json::Value> = role_rows
        .into_iter()
        .map(|row| {
            json!({
                "id": row.get::<String, _>("id"),
                "name": row.get::<String, _>("name"),
            })
        })
        .collect();

    let next_role_rows = if unique_roles.is_empty() {
        Vec::new()
    } else {
        let placeholders = std::iter::repeat("?")
            .take(unique_roles.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT id, name FROM roles WHERE id IN ({}) ORDER BY name ASC",
            placeholders
        );
        let mut query = sqlx::query(&sql);
        for rid in &unique_roles {
            query = query.bind(rid);
        }
        query.fetch_all(&db.0).await?
    };
    let next_roles: Vec<serde_json::Value> = next_role_rows
        .into_iter()
        .map(|row| {
            json!({
                "id": row.get::<String, _>("id"),
                "name": row.get::<String, _>("name"),
            })
        })
        .collect();

    let mut tx = db.0.begin().await?;
    sqlx::query("DELETE FROM user_roles WHERE user_id = ?")
        .bind(&target_id)
        .execute(&mut *tx)
        .await?;

    for rid in &unique_roles {
        sqlx::query("INSERT INTO user_roles(user_id, role_id) VALUES (?, ?)")
            .bind(target_id.clone())
            .bind(rid)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;

    admin_log::record_admin_log(
        &db.0,
        &user.user_id,
        "user.roles_updated",
        &json!({
            "target_user_id": target_id,
            "previous_roles": previous_roles,
            "new_roles": next_roles,
        }),
    )
    .await?;

    // Broadcast profile update
    chat.do_send(BroadcastAll {
        payload: serde_json::json!({
            "type": "user_updated",
            "user_id": target_id,
        })
        .to_string(),
    });

    log::info!(
        "AdminAction: update_user_roles admin_id={} target_id={} role_ids={:?}",
        user.user_id,
        target_id,
        unique_roles
    );
    Ok(HttpResponse::Ok().finish())
}
