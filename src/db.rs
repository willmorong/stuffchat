use sqlx::{SqlitePool, sqlite::SqlitePoolOptions, sqlite::SqliteConnectOptions};
use std::str::FromStr;
use std::time::Duration;

#[derive(Clone)]
pub struct Db(pub SqlitePool);
impl Db {
    pub async fn connect_and_migrate(path: &str) -> anyhow::Result<Self> {
        let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", path))?
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal);

        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .acquire_timeout(Duration::from_secs(10))
            .connect_with(opts).await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Db(pool))
    }
}