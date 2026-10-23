use std::path::Path;
use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

use crate::error::Result;

/// Opens the hub database and applies the embedded migrations.
///
/// A memory pool uses one connection: each extra connection would be a different
/// in-memory database.
pub async fn connect(path: &Path) -> Result<SqlitePool> {
    let uri = format!("sqlite://{}", path.display());
    let options = SqliteConnectOptions::from_str(&uri)
        .map_err(|error| crate::error::HubError::config(format!("hub database url: {error}")))?
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await?;
    migrate(&pool).await?;
    Ok(pool)
}

/// Opens an existing database without applying migrations. Import uses this so a
/// live omo-gateway file is never rewritten by the hub's own schema.
pub async fn connect_readonly(path: &Path) -> Result<SqlitePool> {
    let uri = format!("sqlite://{}", path.display());
    let options = SqliteConnectOptions::from_str(&uri)
        .map_err(|error| crate::error::HubError::config(format!("hub database url: {error}")))?
        .read_only(true)
        .foreign_keys(false);
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(crate::error::HubError::from)
}

pub async fn memory() -> Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str("sqlite::memory:")
        .map_err(|error| crate::error::HubError::config(error.to_string()))?
        .foreign_keys(true);
    // The database lives only as long as its one connection, so that connection is never
    // retired for idling or age (a long dry-run import would otherwise lose its tables).
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .min_connections(1)
        .idle_timeout(None)
        .max_lifetime(None)
        .connect_with(options)
        .await?;
    migrate(&pool).await?;
    Ok(pool)
}

async fn migrate(pool: &SqlitePool) -> Result<()> {
    // Embedded at build time: an installed binary has no source tree to read them from.
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}

pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}
