use crate::{
    admin_log,
    auth::AuthUser,
    db::Db,
    errors::ApiError,
    models::role::PERM_POST_MESSAGES,
    permissions,
    push::{PushActorInfo, PushChannelInfo, PushMessageInfo, PushRelayRuntime},
    ws::server::{Broadcast, GetChannelActiveUsers},
};
use actix_web::{HttpResponse, web};
use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, sqlite::SqliteRow};

#[derive(Deserialize)]
pub struct ListQuery {
    pub before: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
    pub tz: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Deserialize)]
pub struct ContextQuery {
    pub before: Option<i64>,
    pub after: Option<i64>,
}

#[derive(Serialize, Deserialize)]
struct SearchCursor {
    created_at: DateTime<Utc>,
    id: String,
}

#[derive(Default, Debug)]
struct SearchFilters {
    terms: Vec<String>,
    from_username: Option<String>,
    before: Option<DateTime<Utc>>,
    after: Option<DateTime<Utc>>,
    channel_name: Option<String>,
    has_attachment: Option<bool>,
}

fn decode_cursor(encoded: &str) -> Result<SearchCursor, ApiError> {
    let decoded =
        urlencoding::decode(encoded).map_err(|_| ApiError::BadRequest("invalid cursor".into()))?;
    serde_json::from_str::<SearchCursor>(&decoded)
        .map_err(|_| ApiError::BadRequest("invalid cursor".into()))
}

fn encode_cursor(c: &SearchCursor) -> Result<String, ApiError> {
    let raw = serde_json::to_string(c).map_err(|_| ApiError::Internal)?;
    Ok(urlencoding::encode(&raw).into_owned())
}

fn tokenize_query(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in input.chars() {
        match ch {
            '"' => {
                if in_quotes {
                    if !current.is_empty() {
                        out.push(std::mem::take(&mut current));
                    }
                    in_quotes = false;
                } else {
                    if !current.trim().is_empty() {
                        out.push(std::mem::take(&mut current));
                    }
                    in_quotes = true;
                }
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        out.push(current);
    }

    out
}

fn parse_has_attachment_value(v: &str) -> Result<bool, ApiError> {
    match v.trim().to_ascii_lowercase().as_str() {
        "attachment" | "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        _ => Err(ApiError::BadRequest("invalid has:attachment value".into())),
    }
}

fn parse_fixed_offset(value: &str) -> Result<FixedOffset, ApiError> {
    let trimmed = value.trim();

    if let Ok(minutes_east) = trimmed.parse::<i32>() {
        return FixedOffset::east_opt(minutes_east * 60)
            .ok_or_else(|| ApiError::BadRequest("invalid timezone offset".into()));
    }

    if trimmed.len() == 6 {
        let sign = &trimmed[0..1];
        let hh = trimmed[1..3]
            .parse::<i32>()
            .map_err(|_| ApiError::BadRequest("invalid timezone offset".into()))?;
        let mm = trimmed[4..6]
            .parse::<i32>()
            .map_err(|_| ApiError::BadRequest("invalid timezone offset".into()))?;
        if &trimmed[3..4] != ":" || hh > 23 || mm > 59 {
            return Err(ApiError::BadRequest("invalid timezone offset".into()));
        }
        let seconds = (hh * 3600) + (mm * 60);
        return match sign {
            "+" => FixedOffset::east_opt(seconds),
            "-" => FixedOffset::west_opt(seconds),
            _ => None,
        }
        .ok_or_else(|| ApiError::BadRequest("invalid timezone offset".into()));
    }

    Err(ApiError::BadRequest("invalid timezone offset".into()))
}

fn parse_modifier_datetime(value: &str, tz_name: Option<&str>) -> Result<DateTime<Utc>, ApiError> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        return Ok(dt.with_timezone(&Utc));
    }

    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| {
        ApiError::BadRequest("invalid date (expected RFC3339 or YYYY-MM-DD)".into())
    })?;
    let tz_name =
        tz_name.ok_or_else(|| ApiError::BadRequest("tz required for date-only filters".into()))?;
    let tz = parse_fixed_offset(tz_name)?;

    let local_dt = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| ApiError::BadRequest("invalid date".into()))?;
    let zoned = tz
        .from_local_datetime(&local_dt)
        .single()
        .ok_or_else(|| ApiError::BadRequest("invalid local datetime".into()))?;

    Ok(zoned.with_timezone(&Utc))
}

fn parse_search_filters(raw: &str, tz_name: Option<&str>) -> Result<SearchFilters, ApiError> {
    if raw.trim().is_empty() {
        return Err(ApiError::BadRequest("q required".into()));
    }
    if raw.len() > 512 {
        return Err(ApiError::BadRequest("query too long".into()));
    }

    let tokens = tokenize_query(raw);
    if tokens.len() > 12 {
        return Err(ApiError::BadRequest("too many query tokens".into()));
    }

    let mut f = SearchFilters::default();
    for token in tokens {
        let lower = token.to_ascii_lowercase();
        if lower.starts_with("from:") {
            let v = token[5..].trim();
            if v.is_empty() {
                return Err(ApiError::BadRequest("from: requires a username".into()));
            }
            f.from_username = Some(v.to_string());
            continue;
        }
        if lower.starts_with("before:") {
            let v = token[7..].trim();
            if v.is_empty() {
                return Err(ApiError::BadRequest("before: requires a value".into()));
            }
            f.before = Some(parse_modifier_datetime(v, tz_name)?);
            continue;
        }
        if lower.starts_with("after:") {
            let v = token[6..].trim();
            if v.is_empty() {
                return Err(ApiError::BadRequest("after: requires a value".into()));
            }
            f.after = Some(parse_modifier_datetime(v, tz_name)?);
            continue;
        }
        if lower.starts_with("in:") {
            let v = token[3..].trim().trim_start_matches('#');
            if v.is_empty() {
                return Err(ApiError::BadRequest("in: requires a channel".into()));
            }
            f.channel_name = Some(v.to_string());
            continue;
        }

        if lower == "has:attachment" {
            f.has_attachment = Some(true);
            continue;
        }
        if lower.starts_with("has:attachment=") {
            f.has_attachment = Some(parse_has_attachment_value(
                &token["has:attachment=".len()..],
            )?);
            continue;
        }
        if lower.starts_with("has:attachment:") {
            f.has_attachment = Some(parse_has_attachment_value(
                &token["has:attachment:".len()..],
            )?);
            continue;
        }

        f.terms.push(token);
    }

    if let (Some(after), Some(before)) = (f.after.as_ref(), f.before.as_ref()) {
        if after >= before {
            return Err(ApiError::BadRequest(
                "after: must be earlier than before:".into(),
            ));
        }
    }

    Ok(f)
}

fn build_fts_match_query(terms: &[String]) -> String {
    terms
        .iter()
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}

async fn collect_reactions(
    db: &Db,
    msg_ids: &[String],
) -> Result<std::collections::HashMap<String, Vec<(String, Vec<String>)>>, ApiError> {
    if msg_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let placeholders: String = msg_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let query_str = format!(
        "SELECT message_id, emoji, user_id FROM message_reactions WHERE message_id IN ({}) ORDER BY created_at ASC",
        placeholders
    );
    let mut q = sqlx::query(&query_str);
    for mid in msg_ids {
        q = q.bind(mid);
    }
    let reaction_rows = q.fetch_all(&db.0).await?;

    let mut rmap: std::collections::HashMap<String, Vec<(String, Vec<String>)>> =
        std::collections::HashMap::new();
    let mut intermediate: std::collections::HashMap<
        String,
        (Vec<String>, std::collections::HashMap<String, Vec<String>>),
    > = std::collections::HashMap::new();

    for r in reaction_rows {
        let mid: String = r.get("message_id");
        let emoji: String = r.get("emoji");
        let uid: String = r.get("user_id");
        let entry = intermediate
            .entry(mid)
            .or_insert_with(|| (Vec::new(), std::collections::HashMap::new()));
        if !entry.1.contains_key(&emoji) {
            entry.0.push(emoji.clone());
        }
        entry.1.entry(emoji).or_default().push(uid);
    }
    for (mid, (order, mut emap)) in intermediate {
        let grouped: Vec<(String, Vec<String>)> = order
            .into_iter()
            .map(|e| {
                let users = emap.remove(&e).unwrap_or_default();
                (e, users)
            })
            .collect();
        rmap.insert(mid, grouped);
    }

    Ok(rmap)
}

fn serialize_message_rows(
    rows: Vec<SqliteRow>,
    reactions_map: &std::collections::HashMap<String, Vec<(String, Vec<String>)>>,
) -> Vec<serde_json::Value> {
    rows.into_iter()
        .map(|r| {
            let id: String = r.get("id");
            let file_id: Option<String> = r.get("file_id");
            let original_name: Option<String> = r.get("original_name");
            let size_bytes: Option<i64> = r.get("size_bytes");
            let file_url = match (file_id.as_deref(), original_name.as_deref()) {
                (Some(fid), Some(name)) => Some(format!("/files/{}/{}", fid, name)),
                _ => None,
            };
            let reactions: Vec<serde_json::Value> = reactions_map
                .get(&id)
                .map(|grouped| {
                    grouped
                        .iter()
                        .map(|(emoji, users)| {
                            serde_json::json!({
                                "emoji": emoji,
                                "users": users,
                                "count": users.len(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();

            serde_json::json!({
                "id": id,
                "channel_id": r.get::<String,_>("channel_id"),
                "user_id": r.get::<String,_>("user_id"),
                "content": r.get::<Option<String>,_>("content"),
                "file_url": file_url,
                "filename": original_name,
                "file_size": size_bytes,
                "created_at": r.get::<DateTime<Utc>,_>("created_at"),
                "edited_at": r.get::<Option<DateTime<Utc>>,_>("edited_at"),
                "replying_to": r.get::<Option<String>,_>("replying_to"),
                "reactions": reactions,
            })
        })
        .collect()
}

fn build_content_preview(content: Option<&str>) -> String {
    let text = content.unwrap_or("").trim();
    if text.is_empty() {
        return String::new();
    }
    const MAX: usize = 180;
    if text.chars().count() <= MAX {
        return text.to_string();
    }
    let truncated: String = text.chars().take(MAX).collect();
    format!("{truncated}…")
}

pub async fn list_messages(
    db: web::Data<Db>,
    user: AuthUser,
    path: web::Path<String>,
    q: web::Query<ListQuery>,
) -> Result<HttpResponse, ApiError> {
    let channel_id = path.into_inner();
    let m =
        sqlx::query("SELECT can_read FROM channel_members WHERE channel_id = ? AND user_id = ?")
            .bind(&channel_id)
            .bind(&user.user_id)
            .fetch_optional(&db.0)
            .await?;
    let m = m.ok_or(ApiError::Forbidden)?;
    if m.get::<i64, _>("can_read") == 0 {
        return Err(ApiError::Forbidden);
    }

    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let rows = if let Some(before_id) = &q.before {
        // Get created_at of before_id for pagination
        let ref_row =
            sqlx::query("SELECT created_at, id FROM messages WHERE id = ? AND channel_id = ?")
                .bind(before_id)
                .bind(&channel_id)
                .fetch_optional(&db.0)
                .await?;
        let (ts, ref_id): (DateTime<Utc>, String) = ref_row
            .map(|r| (r.get("created_at"), r.get("id")))
            .unwrap_or_else(|| (Utc::now(), before_id.clone()));
        sqlx::query(
            "SELECT m.id, m.channel_id, m.user_id, m.content, m.file_id, m.created_at, m.edited_at, m.replying_to, f.original_name, f.size_bytes
             FROM messages m
             LEFT JOIN files f ON f.id = m.file_id
             WHERE m.channel_id = ? AND m.deleted_at IS NULL
             AND (m.created_at < ? OR (m.created_at = ? AND m.id < ?))
             ORDER BY m.created_at DESC, m.id DESC LIMIT ?"
        )
            .bind(&channel_id)
            .bind(ts)
            .bind(ts)
            .bind(ref_id)
            .bind(limit)
            .fetch_all(&db.0)
            .await?
    } else {
        sqlx::query(
            "SELECT m.id, m.channel_id, m.user_id, m.content, m.file_id, m.created_at, m.edited_at, m.replying_to, f.original_name, f.size_bytes
             FROM messages m
             LEFT JOIN files f ON f.id = m.file_id
             WHERE m.channel_id = ? AND m.deleted_at IS NULL
             ORDER BY m.created_at DESC, m.id DESC LIMIT ?"
        )
            .bind(&channel_id)
            .bind(limit)
            .fetch_all(&db.0)
            .await?
    };

    let msg_ids: Vec<String> = rows.iter().map(|r| r.get::<String, _>("id")).collect();
    let reactions_map = collect_reactions(&db, &msg_ids).await?;
    let msgs = serialize_message_rows(rows, &reactions_map);

    Ok(HttpResponse::Ok().json(msgs))
}

pub async fn search_messages(
    db: web::Data<Db>,
    user: AuthUser,
    q: web::Query<SearchQuery>,
) -> Result<HttpResponse, ApiError> {
    let filters = parse_search_filters(&q.q, q.tz.as_deref())?;
    let limit = q.limit.unwrap_or(25).clamp(1, 50);
    let cursor = q.cursor.as_deref().map(decode_cursor).transpose()?;
    let has_terms = !filters.terms.is_empty();
    let fts_query = if has_terms {
        Some(build_fts_match_query(&filters.terms))
    } else {
        None
    };

    let mut sql = String::from(
        "SELECT m.id, m.channel_id, c.name AS channel_name, m.user_id, u.username, m.content, m.file_id, m.created_at, m.replying_to
         FROM messages m
         INNER JOIN channels c ON c.id = m.channel_id
         INNER JOIN channel_members cm ON cm.channel_id = m.channel_id
         INNER JOIN users u ON u.id = m.user_id",
    );
    if has_terms {
        sql.push_str(" INNER JOIN messages_fts mf ON mf.rowid = m.rowid");
    }
    sql.push_str(
        " WHERE cm.user_id = ? AND cm.can_read = 1
          AND m.deleted_at IS NULL
          AND c.deleted_at IS NULL",
    );

    if has_terms {
        sql.push_str(" AND mf.content MATCH ?");
    }
    if filters.from_username.is_some() {
        sql.push_str(" AND u.username = ? COLLATE NOCASE");
    }
    if filters.before.is_some() {
        sql.push_str(" AND m.created_at < ?");
    }
    if filters.after.is_some() {
        sql.push_str(" AND m.created_at >= ?");
    }
    if filters.channel_name.is_some() {
        sql.push_str(" AND c.name = ? COLLATE NOCASE");
    }
    if filters.has_attachment.is_some() {
        sql.push_str(" AND m.file_id IS ");
        if filters.has_attachment == Some(false) {
            sql.push_str("NULL");
        } else {
            sql.push_str("NOT NULL");
        }
    }
    if cursor.is_some() {
        sql.push_str(" AND (m.created_at < ? OR (m.created_at = ? AND m.id < ?))");
    }
    sql.push_str(" ORDER BY m.created_at DESC, m.id DESC LIMIT ?");

    let mut query = sqlx::query(&sql).bind(&user.user_id);
    if let Some(fts) = &fts_query {
        query = query.bind(fts);
    }
    if let Some(from) = &filters.from_username {
        query = query.bind(from);
    }
    if let Some(before) = filters.before.as_ref() {
        query = query.bind(before.clone());
    }
    if let Some(after) = filters.after.as_ref() {
        query = query.bind(after.clone());
    }
    if let Some(ch_name) = &filters.channel_name {
        query = query.bind(ch_name);
    }
    if let Some(c) = &cursor {
        query = query.bind(c.created_at).bind(c.created_at).bind(&c.id);
    }
    query = query.bind(limit);

    let rows = query.fetch_all(&db.0).await?;
    let next_cursor = if rows.len() == limit as usize {
        let last = rows.last().ok_or(ApiError::Internal)?;
        let c = SearchCursor {
            created_at: last.get("created_at"),
            id: last.get("id"),
        };
        Some(encode_cursor(&c)?)
    } else {
        None
    };

    let results: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|r| {
            let content: Option<String> = r.get("content");
            let preview = build_content_preview(content.as_deref());
            serde_json::json!({
                "id": r.get::<String,_>("id"),
                "channel_id": r.get::<String,_>("channel_id"),
                "channel_name": r.get::<String,_>("channel_name"),
                "user_id": r.get::<String,_>("user_id"),
                "username": r.get::<String,_>("username"),
                "content_preview": preview,
                "has_attachment": r.get::<Option<String>,_>("file_id").is_some(),
                "created_at": r.get::<DateTime<Utc>,_>("created_at"),
                "replying_to": r.get::<Option<String>,_>("replying_to"),
            })
        })
        .collect();

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "results": results,
        "next_cursor": next_cursor,
    })))
}

pub async fn get_message_context(
    db: web::Data<Db>,
    user: AuthUser,
    path: web::Path<String>,
    q: web::Query<ContextQuery>,
) -> Result<HttpResponse, ApiError> {
    let anchor_id = path.into_inner();
    let before_limit = q.before.unwrap_or(30).clamp(1, 100);
    let after_limit = q.after.unwrap_or(20).clamp(1, 100);

    let anchor = sqlx::query(
        "SELECT m.id, m.channel_id, m.user_id, m.content, m.file_id, m.created_at, m.edited_at, m.replying_to, f.original_name, f.size_bytes
         FROM messages m
         LEFT JOIN files f ON f.id = m.file_id
         WHERE m.id = ? AND m.deleted_at IS NULL",
    )
    .bind(&anchor_id)
    .fetch_optional(&db.0)
    .await?;
    let anchor = anchor.ok_or(ApiError::NotFound)?;
    let channel_id: String = anchor.get("channel_id");
    let anchor_created_at: DateTime<Utc> = anchor.get("created_at");
    let anchor_msg_id: String = anchor.get("id");

    let m =
        sqlx::query("SELECT can_read FROM channel_members WHERE channel_id = ? AND user_id = ?")
            .bind(&channel_id)
            .bind(&user.user_id)
            .fetch_optional(&db.0)
            .await?;
    let m = m.ok_or(ApiError::Forbidden)?;
    if m.get::<i64, _>("can_read") == 0 {
        return Err(ApiError::Forbidden);
    }

    let mut before_rows = sqlx::query(
        "SELECT m.id, m.channel_id, m.user_id, m.content, m.file_id, m.created_at, m.edited_at, m.replying_to, f.original_name, f.size_bytes
         FROM messages m
         LEFT JOIN files f ON f.id = m.file_id
         WHERE m.channel_id = ? AND m.deleted_at IS NULL
           AND (m.created_at < ? OR (m.created_at = ? AND m.id < ?))
         ORDER BY m.created_at DESC, m.id DESC
         LIMIT ?",
    )
    .bind(&channel_id)
    .bind(anchor_created_at)
    .bind(anchor_created_at)
    .bind(&anchor_msg_id)
    .bind(before_limit)
    .fetch_all(&db.0)
    .await?;
    before_rows.reverse();

    let after_rows = sqlx::query(
        "SELECT m.id, m.channel_id, m.user_id, m.content, m.file_id, m.created_at, m.edited_at, m.replying_to, f.original_name, f.size_bytes
         FROM messages m
         LEFT JOIN files f ON f.id = m.file_id
         WHERE m.channel_id = ? AND m.deleted_at IS NULL
           AND (m.created_at > ? OR (m.created_at = ? AND m.id > ?))
         ORDER BY m.created_at ASC, m.id ASC
         LIMIT ?",
    )
    .bind(&channel_id)
    .bind(anchor_created_at)
    .bind(anchor_created_at)
    .bind(&anchor_msg_id)
    .bind(after_limit)
    .fetch_all(&db.0)
    .await?;

    let mut rows = before_rows;
    rows.push(anchor);
    rows.extend(after_rows);

    let msg_ids: Vec<String> = rows.iter().map(|r| r.get::<String, _>("id")).collect();
    let reactions_map = collect_reactions(&db, &msg_ids).await?;
    let messages = serialize_message_rows(rows, &reactions_map);

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "channel_id": channel_id,
        "anchor_message_id": anchor_msg_id,
        "messages": messages,
    })))
}

pub async fn get_message(
    db: web::Data<Db>,
    user: AuthUser,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    let row = sqlx::query(
        "SELECT m.id, m.channel_id, m.user_id, m.content, m.file_id, m.created_at, m.edited_at, m.replying_to, f.original_name, f.size_bytes
         FROM messages m
         LEFT JOIN files f ON f.id = m.file_id
         WHERE m.id = ? AND m.deleted_at IS NULL",
    )
    .bind(&id)
    .fetch_optional(&db.0)
    .await?;

    let row = row.ok_or(ApiError::NotFound)?;
    let channel_id: String = row.get("channel_id");

    let m =
        sqlx::query("SELECT can_read FROM channel_members WHERE channel_id = ? AND user_id = ?")
            .bind(&channel_id)
            .bind(&user.user_id)
            .fetch_optional(&db.0)
            .await?;
    let m = m.ok_or(ApiError::Forbidden)?;
    if m.get::<i64, _>("can_read") == 0 {
        return Err(ApiError::Forbidden);
    }

    let msg_ids = vec![id.clone()];
    let reactions_map = collect_reactions(&db, &msg_ids).await?;
    let mut messages = serialize_message_rows(vec![row], &reactions_map);

    if let Some(msg) = messages.pop() {
        Ok(HttpResponse::Ok().json(msg))
    } else {
        Err(ApiError::NotFound)
    }
}

#[derive(Deserialize)]
pub struct PostMessageReq {
    pub content: Option<String>,
    pub file_id: Option<String>,
    pub replying_to: Option<String>,
}

pub async fn post_message(
    db: web::Data<Db>,
    chat: web::Data<actix::Addr<crate::ws::server::ChatServer>>,
    push_runtime: web::Data<Option<PushRelayRuntime>>,
    user: AuthUser,
    path: web::Path<String>,
    body: web::Json<PostMessageReq>,
) -> Result<HttpResponse, ApiError> {
    permissions::require_permission(&db, &user.user_id, PERM_POST_MESSAGES).await?;
    let channel_id = path.into_inner();
    let m =
        sqlx::query("SELECT can_write FROM channel_members WHERE channel_id = ? AND user_id = ?")
            .bind(&channel_id)
            .bind(&user.user_id)
            .fetch_optional(&db.0)
            .await?;
    let m = m.ok_or(ApiError::Forbidden)?;
    if m.get::<i64, _>("can_write") == 0 {
        return Err(ApiError::Forbidden);
    }
    let channel_row =
        sqlx::query("SELECT name, is_voice FROM channels WHERE id = ? AND deleted_at IS NULL")
            .bind(&channel_id)
            .fetch_optional(&db.0)
            .await?;
    let channel_row = channel_row.ok_or(ApiError::NotFound)?;
    let channel_name: String = channel_row.get("name");
    let is_voice = channel_row.get::<i64, _>("is_voice") != 0;
    let user_row = sqlx::query("SELECT username FROM users WHERE id = ?")
        .bind(&user.user_id)
        .fetch_optional(&db.0)
        .await?;
    let user_row = user_row.ok_or(ApiError::Unauthorized)?;
    let actor_username: String = user_row.get("username");

    if body
        .content
        .as_deref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
        && body.file_id.is_none()
    {
        return Err(ApiError::BadRequest(
            "message must have content or file".into(),
        ));
    }

    // Resolve original filename for broadcast (if a file is attached)
    let (file_url, filename, file_size) = if let Some(fid) = &body.file_id {
        let row = sqlx::query("SELECT original_name, size_bytes FROM files WHERE id = ?")
            .bind(fid)
            .fetch_optional(&db.0)
            .await?;
        if let Some(r) = row {
            let original: String = r.get("original_name");
            let size: i64 = r.get("size_bytes");
            (
                Some(format!("/files/{}/{}", fid, original)),
                Some(original),
                Some(size),
            )
        } else {
            (None, None, None)
        }
    } else {
        (None, None, None)
    };

    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now();
    sqlx::query("INSERT INTO messages(id, channel_id, user_id, content, file_id, created_at, replying_to) VALUES (?, ?, ?, ?, ?, ?, ?)")
        .bind(&id).bind(&channel_id).bind(&user.user_id).bind(&body.content).bind(&body.file_id).bind(now).bind(&body.replying_to)
        .execute(&db.0).await?;

    // Broadcast to WS
    let payload = serde_json::json!({
        "type": "message_created",
        "id": id,
        "channel_id": channel_id,
        "user_id": user.user_id,
        "content": body.content,
        "file_url": file_url,
        "filename": filename,
        "file_size": file_size,
        "created_at": now,
        "replying_to": body.replying_to,
    })
    .to_string();
    chat.do_send(Broadcast {
        channel_id: channel_id.clone(),
        payload: payload.clone(),
    });

    // Notify other members (skipping those in the channel room)
    let member_rows = sqlx::query("SELECT user_id FROM channel_members WHERE channel_id = ?")
        .bind(&channel_id)
        .fetch_all(&db.0)
        .await?;
    let member_ids: Vec<String> = member_rows
        .into_iter()
        .map(|r| r.get("user_id"))
        .filter(|uid| uid != &user.user_id)
        .collect();

    if !member_ids.is_empty() {
        chat.do_send(crate::ws::server::NotifyUsers {
            user_ids: member_ids.clone(),
            payload: payload.clone(),
            skip_channel: Some(channel_id.clone()),
        });
    }

    if let Some(push_runtime) = push_runtime.get_ref().as_ref() {
        let active_user_ids = chat
            .send(GetChannelActiveUsers {
                channel_id: channel_id.clone(),
                include_voice: false,
            })
            .await
            .map_err(|_| ApiError::Internal)?
            .map_err(|_| ApiError::Internal)?;
        let active_user_ids: std::collections::HashSet<String> =
            active_user_ids.into_iter().collect();
        let recipient_user_ids: Vec<String> = member_ids
            .into_iter()
            .filter(|user_id| !active_user_ids.contains(user_id))
            .collect();
        if !recipient_user_ids.is_empty() {
            let preview = {
                let text = build_content_preview(body.content.as_deref());
                if !text.is_empty() {
                    text
                } else if body.file_id.is_some() {
                    "Sent an attachment".to_string()
                } else {
                    String::new()
                }
            };
            let _ = push_runtime
                .enqueue_message_event(
                    PushChannelInfo {
                        id: channel_id.clone(),
                        name: channel_name,
                        is_voice,
                    },
                    PushActorInfo {
                        id: user.user_id.clone(),
                        username: actor_username,
                    },
                    PushMessageInfo { id: id.clone(), preview },
                    recipient_user_ids,
                    now,
                )
                .await;
        }
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({ "id": id })))
}

#[derive(Deserialize)]
pub struct EditMessageReq {
    pub content: String,
}

pub async fn edit_message(
    db: web::Data<Db>,
    chat: web::Data<actix::Addr<crate::ws::server::ChatServer>>,
    user: AuthUser,
    path: web::Path<String>,
    body: web::Json<EditMessageReq>,
) -> Result<HttpResponse, ApiError> {
    if body.content.trim().is_empty() {
        return Err(ApiError::BadRequest("content required".into()));
    }

    let id = path.into_inner();
    // Load message with channel and author
    let row =
        sqlx::query("SELECT channel_id, user_id FROM messages WHERE id = ? AND deleted_at IS NULL")
            .bind(&id)
            .fetch_optional(&db.0)
            .await?;
    let row = row.ok_or(ApiError::NotFound)?;
    let channel_id: String = row.get("channel_id");
    let author_id: String = row.get("user_id");

    // Permission: author or channel manager
    let can_manage = permissions::can_manage_channel(&db, &user.user_id, &channel_id).await?;
    if user.user_id != author_id && !can_manage {
        return Err(ApiError::Forbidden);
    }

    let now = Utc::now();
    sqlx::query("UPDATE messages SET content = ?, edited_at = ? WHERE id = ?")
        .bind(&body.content)
        .bind(now)
        .bind(&id)
        .execute(&db.0)
        .await?;

    // Broadcast update
    let payload = serde_json::json!({
        "type": "message_edited",
        "id": id,
        "channel_id": channel_id,
        "content": body.content,
        "edited_at": now,
    })
    .to_string();
    chat.do_send(Broadcast {
        channel_id: channel_id.clone(),
        payload,
    });

    Ok(HttpResponse::Ok().finish())
}

pub async fn delete_message(
    db: web::Data<Db>,
    chat: web::Data<actix::Addr<crate::ws::server::ChatServer>>,
    user: AuthUser,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    let row =
        sqlx::query("SELECT channel_id, user_id FROM messages WHERE id = ? AND deleted_at IS NULL")
            .bind(&id)
            .fetch_optional(&db.0)
            .await?;
    let row = row.ok_or(ApiError::NotFound)?;
    let channel_id: String = row.get("channel_id");
    let author_id: String = row.get("user_id");

    // Permission: author or channel manager
    let can_manage = permissions::can_manage_channel(&db, &user.user_id, &channel_id).await?;
    if user.user_id != author_id && !can_manage {
        return Err(ApiError::Forbidden);
    }

    let now = Utc::now();
    sqlx::query("UPDATE messages SET deleted_at = ? WHERE id = ?")
        .bind(now)
        .bind(&id)
        .execute(&db.0)
        .await?;

    // Broadcast deletion
    let payload = serde_json::json!({
        "type": "message_deleted",
        "id": id,
        "channel_id": channel_id,
        "deleted_at": now,
    })
    .to_string();
    chat.do_send(Broadcast {
        channel_id: channel_id.clone(),
        payload,
    });

    Ok(HttpResponse::Ok().finish())
}

pub async fn flag_message(
    db: web::Data<Db>,
    user: AuthUser,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    let row = sqlx::query(
        r#"
        SELECT m.channel_id, m.user_id AS sender_user_id, m.content, u.username AS sender_username
        FROM messages m
        INNER JOIN users u ON u.id = m.user_id
        WHERE m.id = ? AND m.deleted_at IS NULL
        "#,
    )
    .bind(&id)
    .fetch_optional(&db.0)
    .await?;
    let row = row.ok_or(ApiError::NotFound)?;
    let channel_id: String = row.get("channel_id");

    let membership =
        sqlx::query("SELECT can_read FROM channel_members WHERE channel_id = ? AND user_id = ?")
            .bind(&channel_id)
            .bind(&user.user_id)
            .fetch_optional(&db.0)
            .await?;
    let membership = membership.ok_or(ApiError::Forbidden)?;
    if membership.get::<i64, _>("can_read") == 0 {
        return Err(ApiError::Forbidden);
    }

    let sender_user_id: String = row.get("sender_user_id");
    let sender_username: String = row.get("sender_username");
    let message_content: Option<String> = row.get("content");

    admin_log::record_admin_log(
        &db.0,
        &user.user_id,
        "message.flagged",
        &serde_json::json!({
            "message_id": id,
            "channel_id": channel_id,
            "message_content": message_content,
            "message_sender": {
                "id": sender_user_id,
                "username": sender_username,
            },
        }),
    )
    .await?;

    Ok(HttpResponse::Ok().finish())
}

#[cfg(test)]
mod tests {
    use super::{parse_search_filters, tokenize_query};
    use chrono::{TimeZone, Utc};

    #[test]
    fn tokenizes_with_quotes() {
        let tokens = tokenize_query(r#"hello from:alice "exact phrase" has:attachment"#);
        assert_eq!(
            tokens,
            vec![
                "hello".to_string(),
                "from:alice".to_string(),
                "exact phrase".to_string(),
                "has:attachment".to_string()
            ]
        );
    }

    #[test]
    fn parses_supported_modifiers() {
        let f = parse_search_filters(
            r#"from:alice before:2026-02-01 after:2026-01-01 in:#general has:attachment "hello there""#,
            Some("-08:00"),
        )
        .expect("filters");

        assert_eq!(f.from_username.as_deref(), Some("alice"));
        assert_eq!(f.channel_name.as_deref(), Some("general"));
        assert_eq!(f.has_attachment, Some(true));
        assert_eq!(f.terms, vec!["hello there".to_string()]);
        assert!(f.before.is_some());
        assert!(f.after.is_some());
    }

    #[test]
    fn parses_has_attachment_boolean_aliases() {
        let f_true = parse_search_filters("has:attachment=yes", None).expect("true");
        assert_eq!(f_true.has_attachment, Some(true));

        let f_false = parse_search_filters("has:attachment:0", None).expect("false");
        assert_eq!(f_false.has_attachment, Some(false));
    }

    #[test]
    fn rejects_invalid_has_attachment_value() {
        let err = parse_search_filters("has:attachment=maybe", None).expect_err("expected error");
        assert!(format!("{err}").contains("invalid has:attachment value"));
    }

    #[test]
    fn date_only_uses_timezone() {
        let f = parse_search_filters("before:2026-02-01", Some("-08:00")).expect("filters");
        let expected = Utc
            .with_ymd_and_hms(2026, 2, 1, 8, 0, 0)
            .single()
            .expect("expected utc datetime");
        assert_eq!(f.before, Some(expected));
    }

    #[test]
    fn invalid_date_returns_error() {
        let err = parse_search_filters("before:nope", Some("-08:00")).expect_err("expected error");
        assert!(format!("{err}").contains("invalid date"));
    }
}
