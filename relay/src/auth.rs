use crate::db::Db;
use actix_web::HttpRequest;
use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use sqlx::Row;

type HmacSha256 = Hmac<Sha256>;

const MAX_CLOCK_SKEW_SECS: i64 = 300;

pub async fn verify_request(req: &HttpRequest, body: &[u8], db: &Db) -> Result<String, String> {
    let server_id = header_value(req, "X-Stuffchat-Relay-Server")?;
    let timestamp = header_value(req, "X-Stuffchat-Relay-Timestamp")?;
    let nonce = header_value(req, "X-Stuffchat-Relay-Nonce")?;
    let signature = header_value(req, "X-Stuffchat-Relay-Signature")?;

    let timestamp_value = timestamp
        .parse::<i64>()
        .map_err(|_| "invalid timestamp".to_string())?;
    if (Utc::now().timestamp() - timestamp_value).abs() > MAX_CLOCK_SKEW_SECS {
        return Err("timestamp outside allowed skew".to_string());
    }

    let row = sqlx::query("SELECT secret FROM relay_servers WHERE server_id = ? AND revoked_at IS NULL")
        .bind(&server_id)
        .fetch_optional(&db.0)
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "unknown relay server".to_string())?;
    let secret: String = row.get("secret");

    let expected_signature = build_signature(
        req.method().as_str(),
        req.uri().path(),
        &timestamp,
        &nonce,
        body,
        &secret,
    )?;

    if expected_signature != signature {
        return Err("invalid signature".to_string());
    }

    let now = Utc::now();
    sqlx::query("INSERT INTO request_nonces(server_id, nonce, created_at) VALUES (?, ?, ?)")
        .bind(&server_id)
        .bind(&nonce)
        .bind(now)
        .execute(&db.0)
        .await
        .map_err(|_| "nonce replay detected".to_string())?;

    sqlx::query("DELETE FROM request_nonces WHERE created_at < ?")
        .bind(now - chrono::Duration::hours(1))
        .execute(&db.0)
        .await
        .map_err(|err| err.to_string())?;

    Ok(server_id)
}

pub fn build_signature(
    method: &str,
    path: &str,
    timestamp: &str,
    nonce: &str,
    body: &[u8],
    secret: &str,
) -> Result<String, String> {
    let body_hash = hex::encode(Sha256::digest(body));
    let signing_input = format!("{method}\n{path}\n{timestamp}\n{nonce}\n{body_hash}");
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).map_err(|err| err.to_string())?;
    mac.update(signing_input.as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}

fn header_value(req: &HttpRequest, name: &str) -> Result<String, String> {
    req.headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("missing header {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_matches_server_side_contract() {
        let signature = build_signature(
            "POST",
            "/v1/push/batches",
            "1700000000",
            "nonce",
            br#"{"x":1}"#,
            "secret",
        )
        .expect("signature");

        assert_eq!(
            signature,
            "075ef05a715f9ca742d018fc9d35ae22b8d55e2e47bf707cf3290f09e2d5aeaa"
        );
    }
}
