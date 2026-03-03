use std::fs;

use stuffchat::bridge::load_or_create_bridge_secret;

#[test]
fn creates_bridge_secret_when_missing() {
    let root =
        std::env::temp_dir().join(format!("stuffchat-bridge-config-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("temp dir");
    let path = root.join("bridge_secret");

    let secret = load_or_create_bridge_secret(&path).expect("create secret");

    assert!(!secret.is_empty());
    assert_eq!(
        fs::read_to_string(&path).expect("read secret").trim(),
        secret
    );
}

#[test]
fn reuses_existing_bridge_secret() {
    let root =
        std::env::temp_dir().join(format!("stuffchat-bridge-config-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("temp dir");
    let path = root.join("bridge_secret");
    fs::write(&path, "existing-secret\n").expect("write secret");

    let secret = load_or_create_bridge_secret(&path).expect("load secret");

    assert_eq!(secret, "existing-secret");
}
