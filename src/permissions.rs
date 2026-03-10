use crate::{db::Db, errors::ApiError, models::role::*};
use chrono::Utc;

const DEFAULT_ADMIN_ROLE: &str = "admin";
const DEFAULT_MEMBER_ROLE: &str = "member";

pub async fn role_permission_mask(db: &Db, user_id: &str) -> Result<i64, ApiError> {
    let mask = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT COALESCE(SUM(r.permissions), 0) AS permissions
         FROM user_roles ur
         INNER JOIN roles r ON r.id = ur.role_id
         WHERE ur.user_id = ?",
    )
    .bind(user_id)
    .fetch_one(&db.0)
    .await?;

    Ok(mask.unwrap_or(0))
}

pub async fn has_permission(db: &Db, user_id: &str, permission: i64) -> Result<bool, ApiError> {
    let mask = role_permission_mask(db, user_id).await?;
    Ok((mask & permission) == permission)
}

pub async fn require_permission(db: &Db, user_id: &str, permission: i64) -> Result<(), ApiError> {
    if has_permission(db, user_id, permission).await? {
        Ok(())
    } else {
        Err(ApiError::Forbidden)
    }
}

pub async fn require_admin(db: &Db, user_id: &str) -> Result<(), ApiError> {
    require_permission(db, user_id, PERM_ADMIN_ALL).await
}

pub async fn is_channel_owner(db: &Db, user_id: &str, channel_id: &str) -> Result<bool, ApiError> {
    let row = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT 1 FROM channels WHERE id = ? AND created_by = ? AND deleted_at IS NULL",
    )
    .bind(channel_id)
    .bind(user_id)
    .fetch_optional(&db.0)
    .await?;
    Ok(row.is_some())
}

pub async fn can_manage_channel(
    db: &Db,
    user_id: &str,
    channel_id: &str,
) -> Result<bool, ApiError> {
    if is_channel_owner(db, user_id, channel_id).await? {
        return Ok(true);
    }

    if has_permission(db, user_id, PERM_ADMIN_ALL).await?
        || has_permission(db, user_id, PERM_MANAGE_CHANNELS).await?
    {
        return Ok(true);
    }

    let can_manage = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(can_manage, 0) FROM channel_members WHERE channel_id = ? AND user_id = ?",
    )
    .bind(channel_id)
    .bind(user_id)
    .fetch_optional(&db.0)
    .await?
    .unwrap_or(0);

    Ok(can_manage != 0)
}

async fn ensure_role_with_permissions(
    db: &Db,
    name: &str,
    permissions: i64,
) -> Result<String, ApiError> {
    let existing: Option<String> =
        sqlx::query_scalar::<_, String>("SELECT id FROM roles WHERE name = ?")
            .bind(name)
            .fetch_optional(&db.0)
            .await?;

    if let Some(role_id) = existing {
        sqlx::query("UPDATE roles SET permissions = permissions | ? WHERE id = ?")
            .bind(permissions)
            .bind(&role_id)
            .execute(&db.0)
            .await?;
        return Ok(role_id);
    }

    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO roles(id, name, permissions, created_at) VALUES (?, ?, ?, ?)")
        .bind(&id)
        .bind(name)
        .bind(permissions)
        .bind(Utc::now())
        .execute(&db.0)
        .await?;
    Ok(id)
}

pub async fn seed_default_roles(db: &Db) -> Result<(), ApiError> {
    ensure_role_with_permissions(db, DEFAULT_ADMIN_ROLE, PERM_ADMIN_ALL).await?;
    let member_role_id =
        ensure_role_with_permissions(db, DEFAULT_MEMBER_ROLE, DEFAULT_MEMBER_PERMISSIONS).await?;

    // Assign everyone a member role unless they already have any roles.
    sqlx::query(
        "INSERT OR IGNORE INTO user_roles (user_id, role_id)
         SELECT id, ? FROM users
         WHERE NOT EXISTS (SELECT 1 FROM user_roles ur WHERE ur.user_id = users.id)",
    )
    .bind(&member_role_id)
    .execute(&db.0)
    .await?;

    Ok(())
}

pub async fn seed_admin_role(db: &Db) -> Result<String, ApiError> {
    ensure_role_with_permissions(db, DEFAULT_ADMIN_ROLE, PERM_ADMIN_ALL).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    async fn setup_db() -> Db {
        let path = std::env::temp_dir().join(format!(
            "stuffchat-perms-test-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        Db::connect_and_migrate(path.to_str().expect("temp db path"))
            .await
            .expect("init db")
    }

    async fn insert_user(db: &Db, id: &str, username: &str) {
        sqlx::query(
            "INSERT INTO users(id, username, email, password_hash, created_at, updated_at)
             VALUES (?, ?, ?, 'legacy-hash', ?, ?)",
        )
        .bind(id)
        .bind(username)
        .bind(format!("{username}@example.com"))
        .bind(chrono::Utc::now())
        .bind(chrono::Utc::now())
        .execute(&db.0)
        .await
        .expect("insert user");
    }

    async fn insert_channel(db: &Db, id: &str, created_by: &str) {
        sqlx::query(
            "INSERT INTO channels(id, name, is_voice, is_private, created_by, created_at)
             VALUES (?, 'seed-channel', 0, 0, ?, ?)",
        )
        .bind(id)
        .bind(created_by)
        .bind(chrono::Utc::now())
        .execute(&db.0)
        .await
        .expect("insert channel");
    }

    #[tokio::test]
    async fn seed_default_roles_creates_baseline_roles_and_member_assignments() {
        let db = setup_db().await;
        insert_user(&db, "user-a", "alice").await;
        insert_user(&db, "user-b", "bob").await;

        seed_default_roles(&db).await.expect("seed roles");

        let admin_permissions: i64 =
            sqlx::query_scalar("SELECT permissions FROM roles WHERE name = 'admin' LIMIT 1")
                .fetch_one(&db.0)
                .await
                .expect("admin role");
        assert!((admin_permissions & PERM_ADMIN_ALL) == PERM_ADMIN_ALL);

        let member_permissions: i64 =
            sqlx::query_scalar("SELECT permissions FROM roles WHERE name = 'member' LIMIT 1")
                .fetch_one(&db.0)
                .await
                .expect("member role");
        assert_eq!(
            member_permissions & DEFAULT_MEMBER_PERMISSIONS,
            DEFAULT_MEMBER_PERMISSIONS
        );

        let user_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM user_roles ur
             JOIN roles r ON ur.role_id = r.id
             WHERE ur.user_id IN ('user-a', 'user-b') AND r.name = 'member'",
        )
        .fetch_one(&db.0)
        .await
        .expect("member user_roles count");
        assert_eq!(user_count, 2);
    }

    #[tokio::test]
    async fn seed_admin_role_updates_existing_admin_role_bits() {
        let db = setup_db().await;
        sqlx::query("UPDATE roles SET permissions = 0 WHERE name = 'admin'")
            .execute(&db.0)
            .await
            .expect("clear admin role permissions");

        let role_id = seed_admin_role(&db).await.expect("seed admin");
        let permissions: i64 =
            sqlx::query_scalar("SELECT permissions FROM roles WHERE id = ? LIMIT 1")
                .bind(role_id)
                .fetch_one(&db.0)
                .await
                .expect("admin role after seed");
        assert!((permissions & PERM_ADMIN_ALL) == PERM_ADMIN_ALL);
    }

    #[tokio::test]
    async fn can_manage_channel_supports_owner_admin_and_channel_manager_permissions() {
        let db = setup_db().await;

        insert_user(&db, "owner", "owner").await;
        insert_user(&db, "manager", "manager").await;
        insert_user(&db, "admin", "admin").await;
        insert_user(&db, "outsider", "outsider").await;

        insert_channel(&db, "channel-1", "owner").await;

        let manager_role = uuid::Uuid::new_v4().to_string();
        grant_user_role(&db, "admin", &manager_role, PERM_ADMIN_ALL).await;

        sqlx::query(
            "INSERT INTO channel_members(channel_id, user_id, can_read, can_write, can_manage)
             VALUES ('channel-1', 'manager', 1, 1, 1)",
        )
        .execute(&db.0)
        .await
        .expect("insert manager membership");

        assert!(
            can_manage_channel(&db, "owner", "channel-1")
                .await
                .expect("owner manage")
        );
        assert!(
            can_manage_channel(&db, "manager", "channel-1")
                .await
                .expect("manager manage")
        );
        assert!(
            can_manage_channel(&db, "admin", "channel-1")
                .await
                .expect("admin manage")
        );
        assert!(
            !can_manage_channel(&db, "outsider", "channel-1")
                .await
                .expect("outsider manage")
        );
    }

    #[tokio::test]
    async fn seed_default_roles_does_not_override_existing_user_roles() {
        let db = setup_db().await;
        insert_user(&db, "unassigned", "unassigned").await;
        insert_user(&db, "already-assigned", "assigned").await;
        grant_user_role(&db, "already-assigned", "custom-role", 0).await;

        seed_default_roles(&db).await.expect("seed roles");

        let unassigned_member_roles: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM user_roles ur JOIN roles r ON ur.role_id = r.id WHERE ur.user_id = 'unassigned' AND r.name = 'member'")
                .fetch_one(&db.0)
                .await
                .expect("count unassigned member role");
        let assigned_member_roles: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM user_roles ur JOIN roles r ON ur.role_id = r.id WHERE ur.user_id = 'already-assigned' AND r.name = 'member'")
                .fetch_one(&db.0)
                .await
                .expect("count assigned member role");

        assert_eq!(unassigned_member_roles, 1);
        assert_eq!(assigned_member_roles, 0);
    }

    #[tokio::test]
    async fn role_permission_mask_aggregates_permissions_across_roles() {
        let db = setup_db().await;
        insert_user(&db, "stacked-user", "stacked").await;

        grant_user_role(&db, "stacked-user", "create-role", PERM_CREATE_CHANNELS).await;
        grant_user_role(&db, "stacked-user", "upload-role", PERM_UPLOAD_FILES).await;

        let mask = role_permission_mask(&db, "stacked-user")
            .await
            .expect("aggregate role mask");
        assert_eq!(mask, PERM_CREATE_CHANNELS | PERM_UPLOAD_FILES);

        assert_eq!(
            has_permission(&db, "stacked-user", PERM_CREATE_CHANNELS)
                .await
                .expect("create permission"),
            true
        );
        assert_eq!(
            has_permission(&db, "stacked-user", PERM_UPLOAD_FILES)
                .await
                .expect("upload permission"),
            true
        );
        assert_eq!(
            has_permission(&db, "stacked-user", PERM_POST_MESSAGES)
                .await
                .expect("post permission"),
            false
        );
    }

    async fn grant_user_role(db: &Db, user_id: &str, role_id: &str, permissions: i64) {
        sqlx::query("INSERT INTO roles(id, name, permissions, created_at) VALUES (?, ?, ?, ?)")
            .bind(role_id)
            .bind(format!("role-{role_id}"))
            .bind(permissions)
            .bind(chrono::Utc::now())
            .execute(&db.0)
            .await
            .expect("insert role");

        sqlx::query("INSERT INTO user_roles(user_id, role_id) VALUES (?, ?)")
            .bind(user_id)
            .bind(role_id)
            .execute(&db.0)
            .await
            .expect("assign role");
    }
}
