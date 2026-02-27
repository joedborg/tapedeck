use anyhow::Context;
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use tracing::info;

use crate::config::AppConfig;
use crate::models::User;

pub type Db = SqlitePool;

pub async fn connect(config: &AppConfig) -> anyhow::Result<Db> {
    // Ensure the data directory exists
    if let Some(parent) = std::path::Path::new(&config.database_url)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("create DB dir {parent:?}"))?;
    }

    let url = format!("sqlite://{}?mode=rwc", config.database_url);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .with_context(|| format!("connect to SQLite at {}", config.database_url))?;

    // Run embedded migrations
    run_migrations(&pool).await?;

    info!("Database ready at {}", config.database_url);
    Ok(pool)
}

async fn run_migrations(pool: &Db) -> anyhow::Result<()> {
    // Enable WAL mode for better concurrency
    sqlx::query("PRAGMA journal_mode=WAL;")
        .execute(pool)
        .await
        .context("set WAL mode")?;
    sqlx::query("PRAGMA foreign_keys=ON;")
        .execute(pool)
        .await
        .context("enable foreign keys")?;

    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .context("run migrations")?;

    Ok(())
}

/// Ensure the initial admin user exists, creating it if the users table is empty.
pub async fn seed_admin(pool: &Db, config: &AppConfig) -> anyhow::Result<()> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;

    if count.0 == 0 {
        let id = User::new_id();
        let hash = crate::auth::hash_password(&config.admin_password)?;
        sqlx::query(
            "INSERT INTO users (id, username, password) VALUES (?, ?, ?)",
        )
        .bind(&id)
        .bind(&config.admin_username)
        .bind(&hash)
        .execute(pool)
        .await?;

        tracing::warn!(
            "Created initial admin user '{}'. Change the password immediately.",
            config.admin_username
        );
    }

    Ok(())
}
