mod common;

use actix_web::{App, http::StatusCode, test, web};
use common::{auth_token, grant_role, insert_user, test_context};
use serde_json::Value;
use sqlx::Row;
use std::io::Cursor;
use std::path::Path;
use std::process::Command;
use stuffchat::avatar::{AVATAR_MIME_TYPE, AVATAR_ORIGINAL_NAME, AVATAR_SIZE};
use stuffchat::models::role::PERM_ADMIN_ALL;

const MULTIPART_BOUNDARY: &str = "----stuffchat-avatar-boundary";
macro_rules! test_app {
    ($ctx:expr) => {
        test::init_service(
            App::new()
                .app_data(web::Data::new($ctx.cfg.clone()))
                .app_data(web::Data::new($ctx.db.clone()))
                .app_data(web::Data::new($ctx.chat_server.clone()))
                .app_data(web::Data::new(None::<stuffchat::push::PushRelayRuntime>))
                .configure(|cfg| stuffchat::app::configure(cfg, true)),
        )
        .await
    };
}

fn small_png_bytes() -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([200, 40, 80, 255]));
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut out, image::ImageFormat::Png)
        .expect("encode png fixture");
    out.into_inner()
}

fn ffprobe_dimensions(path: &Path) -> Option<(u32, u32)> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-show_entries")
        .arg("stream=width,height")
        .arg("-of")
        .arg("csv=p=0")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut parts = text.trim().split(',');
    let width = parts.next()?.parse::<u32>().ok()?;
    let height = parts.next()?.parse::<u32>().ok()?;
    Some((width, height))
}

fn multipart_file(filename: &str, content_type: &str, content: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{MULTIPART_BOUNDARY}\r\n").as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(content);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{MULTIPART_BOUNDARY}--\r\n").as_bytes());
    body
}

async fn grant_admin(db: &stuffchat::db::Db, user_id: &str) {
    sqlx::query("INSERT INTO roles(id, name, permissions, created_at) VALUES (?, ?, ?, ?)")
        .bind("admin-role")
        .bind("admin-role")
        .bind(PERM_ADMIN_ALL)
        .bind(chrono::Utc::now())
        .execute(&db.0)
        .await
        .expect("insert admin role");
    grant_role(db, user_id, "admin-role").await;
}

#[actix_web::test]
async fn user_avatar_upload_stores_1024_avif() {
    let ctx = test_context().await;
    std::fs::create_dir_all(&ctx.cfg.uploads_dir).expect("create uploads dir");
    insert_user(&ctx.db, "avatar-user", "avataruser").await;
    let token = auth_token(&ctx.cfg, "avatar-user");
    let app = test_app!(ctx);
    let png = small_png_bytes();

    let response: Value = test::call_and_read_body_json(
        &app,
        test::TestRequest::put()
            .uri("/api/users/me/avatar")
            .insert_header(("Authorization", format!("Bearer {token}")))
            .insert_header((
                "Content-Type",
                format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}"),
            ))
            .set_payload(multipart_file("avatar.png", "image/png", &png))
            .to_request(),
    )
    .await;
    let file_id = response["avatar_file_id"].as_str().expect("avatar file id");

    let row = sqlx::query(
        "SELECT original_name, stored_name, mime_type, size_bytes FROM files WHERE id = ?",
    )
    .bind(file_id)
    .fetch_one(&ctx.db.0)
    .await
    .expect("avatar file row");
    assert_eq!(row.get::<String, _>("original_name"), AVATAR_ORIGINAL_NAME);
    assert_eq!(
        row.get::<Option<String>, _>("mime_type").as_deref(),
        Some(AVATAR_MIME_TYPE)
    );
    let stored_name: String = row.get("stored_name");
    assert!(stored_name.ends_with(".avif"));
    assert!(row.get::<i64, _>("size_bytes") > 0);

    let path = Path::new(&ctx.cfg.uploads_dir).join(&stored_name);
    assert_eq!(
        ffprobe_dimensions(&path).expect("avif dimensions"),
        (AVATAR_SIZE, AVATAR_SIZE)
    );

    let served = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&format!("/files/{file_id}/avatar"))
            .to_request(),
    )
    .await;
    assert_eq!(served.status(), StatusCode::OK);
    assert_eq!(
        served
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some(AVATAR_MIME_TYPE)
    );
}

#[actix_web::test]
async fn admin_avatar_upload_uses_same_avif_storage() {
    let ctx = test_context().await;
    std::fs::create_dir_all(&ctx.cfg.uploads_dir).expect("create uploads dir");
    insert_user(&ctx.db, "admin", "admin").await;
    insert_user(&ctx.db, "target", "target").await;
    grant_admin(&ctx.db, "admin").await;
    let token = auth_token(&ctx.cfg, "admin");
    let app = test_app!(ctx);
    let png = small_png_bytes();

    let response: Value = test::call_and_read_body_json(
        &app,
        test::TestRequest::put()
            .uri("/api/admin/users/target/avatar")
            .insert_header(("Authorization", format!("Bearer {token}")))
            .insert_header((
                "Content-Type",
                format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}"),
            ))
            .set_payload(multipart_file("avatar.png", "image/png", &png))
            .to_request(),
    )
    .await;
    let file_id = response["avatar_file_id"].as_str().expect("avatar file id");

    let row = sqlx::query("SELECT avatar_file_id FROM users WHERE id = ?")
        .bind("target")
        .fetch_one(&ctx.db.0)
        .await
        .expect("target user");
    assert_eq!(
        row.get::<Option<String>, _>("avatar_file_id").as_deref(),
        Some(file_id)
    );

    let file_row =
        sqlx::query("SELECT original_name, stored_name, mime_type FROM files WHERE id = ?")
            .bind(file_id)
            .fetch_one(&ctx.db.0)
            .await
            .expect("avatar file row");
    assert_eq!(
        file_row.get::<String, _>("original_name"),
        AVATAR_ORIGINAL_NAME
    );
    assert!(file_row.get::<String, _>("stored_name").ends_with(".avif"));
    assert_eq!(
        file_row.get::<Option<String>, _>("mime_type").as_deref(),
        Some(AVATAR_MIME_TYPE)
    );
}

#[actix_web::test]
async fn invalid_avatar_upload_does_not_update_user() {
    let ctx = test_context().await;
    std::fs::create_dir_all(&ctx.cfg.uploads_dir).expect("create uploads dir");
    insert_user(&ctx.db, "bad-avatar", "badavatar").await;
    let token = auth_token(&ctx.cfg, "bad-avatar");
    let app = test_app!(ctx);

    let response = test::call_service(
        &app,
        test::TestRequest::put()
            .uri("/api/users/me/avatar")
            .insert_header(("Authorization", format!("Bearer {token}")))
            .insert_header((
                "Content-Type",
                format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}"),
            ))
            .set_payload(multipart_file("avatar.txt", "text/plain", b"not an image"))
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let row = sqlx::query("SELECT avatar_file_id FROM users WHERE id = ?")
        .bind("bad-avatar")
        .fetch_one(&ctx.db.0)
        .await
        .expect("user row");
    assert_eq!(row.get::<Option<String>, _>("avatar_file_id"), None);
}

#[actix_web::test]
async fn animated_gif_avatar_upload_preserves_animation_when_ffmpeg_exists() {
    if !tool_exists("ffmpeg")
        || !tool_exists("ffprobe")
        || !tool_exists("avifenc")
        || !tool_exists("avifdec")
    {
        return;
    }

    let ctx = test_context().await;
    std::fs::create_dir_all(&ctx.cfg.uploads_dir).expect("create uploads dir");
    insert_user(&ctx.db, "anim-user", "animuser").await;
    let token = auth_token(&ctx.cfg, "anim-user");
    let app = test_app!(ctx);

    let gif_path = ctx.root_dir.join("animated.gif");
    let status = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-f")
        .arg("lavfi")
        .arg("-i")
        .arg("testsrc=size=16x16:rate=2")
        .arg("-frames:v")
        .arg("2")
        .arg(&gif_path)
        .status()
        .expect("run ffmpeg test gif");
    if !status.success() {
        return;
    }
    let gif = std::fs::read(&gif_path).expect("read test gif");

    let response: Value = test::call_and_read_body_json(
        &app,
        test::TestRequest::put()
            .uri("/api/users/me/avatar")
            .insert_header(("Authorization", format!("Bearer {token}")))
            .insert_header((
                "Content-Type",
                format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}"),
            ))
            .set_payload(multipart_file("animated.gif", "image/gif", &gif))
            .to_request(),
    )
    .await;
    let file_id = response["avatar_file_id"].as_str().expect("avatar file id");

    let row = sqlx::query("SELECT stored_name FROM files WHERE id = ?")
        .bind(file_id)
        .fetch_one(&ctx.db.0)
        .await
        .expect("avatar file row");
    let stored_name: String = row.get("stored_name");
    let avif_path = Path::new(&ctx.cfg.uploads_dir).join(stored_name);
    let avif_bytes = std::fs::read(&avif_path).expect("read animated avif");
    assert_eq!(avif_bytes.get(4..12), Some(&b"ftypavis"[..]));
    assert!(avif_bytes.windows(4).any(|window| window == b"meta"));
    assert!(!avif_bytes.windows(4).any(|window| window == b"mp41"));

    assert!(avifdec_frame_count(&avif_path).expect("decode animated avif") > 1);
}

fn avifdec_frame_count(path: &Path) -> Option<u32> {
    let output = Command::new("avifdec")
        .arg("--info")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .find_map(|line| line.trim().strip_prefix("* Image Sequence Frames: ("))
        .and_then(|line| line.split_whitespace().next())
        .and_then(|frames| frames.parse::<u32>().ok())
}

fn tool_exists(tool: &str) -> bool {
    ["-version", "--version", "-V"].iter().any(|arg| {
        Command::new(tool)
            .arg(arg)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    })
}
