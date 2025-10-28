// dkg/src/config.rs
use dotenvy::dotenv;
use std::env;
use sqlx::{Pool, Postgres, postgres::PgPoolOptions};
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct Config {
    pub redis_url: String,
    pub encryption_key: Option<String>,
}

impl Config {
    pub fn load() -> Self {
        // Load .env file only once per process (safe to call multiple times)
        dotenv().ok();

        Self {
            redis_url: env::var("REDIS_URL").expect("REDIS_URL not set"),
            encryption_key: env::var("ENCRYPTION_KEY").ok(),
        }
    }
}

/// postgres database pool
pub async fn get_db_pool() -> Result<Pool<Postgres>> {
    let database_url = "postgresql://postgres:password@localhost:5433/idmap_db";
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await?;
    Ok(pool)
}


