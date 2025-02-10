// src/db.rs

use sqlx::{sqlite::SqliteConnectOptions, Row, SqlitePool};
use std::env;
use std::error::Error;
use std::path::PathBuf; // For OS-specific config directory

/// Define our own Result type for convenience.
pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

/// A simplified file info record used for wiki-link completions.
#[derive(Debug, Clone)]
pub struct FileInfo {
    /// The virtual path that will be used for the wiki-link.
    pub virtual_path: String,
    /// The title that will be used as an alias.
    pub title: String,
    pub path: String,
}

/// The Database struct holds an optional connection pool.
/// If the database isn’t available, `pool` will be `None` and API methods will return empty results.
pub struct Database {
    pub(crate) pool: Option<SqlitePool>,
}

/// Returns the path to the database file.
/// First, check if the environment variable `MARKDOWN_LSP_DB_PATH` is set.
/// Otherwise, use the OS config directory, and within it a folder named `gnosis` where the database
/// file is expected to be named `gnosis_db.sqlite`.
fn get_db_path() -> PathBuf {
    if let Ok(path) = env::var("MARKDOWN_LSP_DB_PATH") {
        return PathBuf::from(path);
    }

    if let Some(mut config_dir) = dirs::config_dir() {
        config_dir.push("gnosis");
        config_dir.push("gnosis_db");
        config_dir.push("gnosis_db.sqlite");
        config_dir
    } else {
        // Fallback to current directory.
        PathBuf::from("./gnosis_db.sqlite")
    }
}

impl Database {
    /// Creates a new Database instance.
    /// If the database file does not exist, logs a non-intrusive warning and returns a Database
    /// with no connection pool (queries will return empty results).
    pub async fn new() -> Self {
        let db_path = get_db_path();

        if !db_path.exists() {
            log::warn!(
                "Database file {} does not exist. Wiki-link completions will be empty.",
                db_path.display()
            );
            return Self { pool: None };
        }

        let options = SqliteConnectOptions::new()
            .filename(db_path.to_str().unwrap())
            .create_if_missing(false)
            .to_owned();

        match SqlitePool::connect_with(options).await {
            Ok(pool) => Self { pool: Some(pool) },
            Err(e) => {
                log::error!(
                    "Failed to connect to database: {}. Wiki-link completions will be empty.",
                    e
                );
                Self { pool: None }
            }
        }
    }

    /// Retrieves all file infos from the "files" table.
    /// The query assumes that the "files" table contains the columns:
    /// `virtual_path` (the wiki-link path) and `title` (the file title).
    /// If the database is not available, a warning is logged and an empty vector is returned.
    pub async fn get_all_file_infos(&self) -> Result<Vec<FileInfo>> {
        if let Some(ref pool) = self.pool {
            let rows = sqlx::query("SELECT virtual_path, title, path FROM files")
                .fetch_all(pool)
                .await?;

            let mut infos = Vec::new();
            for row in rows {
                let virtual_path: String = row.try_get("virtual_path")?;
                let path: String = row.try_get("path")?;
                let title: String = row.try_get("title")?;
                infos.push(FileInfo {
                    virtual_path,
                    title,
                    path,
                });
            }
            Ok(infos)
        } else {
            log::warn!("Database is not available. Returning empty completions.");
            Ok(Vec::new())
        }
    }

    /// (Test helper) Creates a Database instance from an existing SqlitePool.
    /// Only used in tests.
    #[cfg(test)]
    pub fn from_pool(pool: SqlitePool) -> Self {
        Self { pool: Some(pool) }
    }
}
