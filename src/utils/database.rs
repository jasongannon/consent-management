use sqlx::{postgres::PgPoolOptions, Pool, Postgres};
use uuid::Uuid;
use serde_json::Value;
use chrono::{DateTime, Utc};

pub struct Database {
    pool: Pool<Postgres>,
}

impl Database {
    pub async fn new(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;

        Ok(Self { pool })
    }

    pub async fn create_user(&self, username: &str, email: &str, password_hash: &str) -> Result<Uuid, sqlx::Error> {
        let id: Uuid = sqlx::query!(
            "INSERT INTO users (username, email, password_hash) VALUES ($1, $2, $3) RETURNING id",
            username,
            email,
            password_hash
        )
        .fetch_one(&self.pool)
        .await?
        .id;

        Ok(id)
    }

    pub async fn create_consent(&self, user_id: Uuid, scope: &str, expires_at: DateTime<Utc>) -> Result<Uuid, sqlx::Error> {
        let id: Uuid = sqlx::query!(
            "INSERT INTO consents (user_id, scope, expires_at, status) VALUES ($1, $2, $3, 'active') RETURNING id",
            user_id,
            scope,
            expires_at
        )
        .fetch_one(&self.pool)
        .await?
        .id;

        Ok(id)
    }

    pub async fn log_audit_event(&self, event_type: &str, user_id: Option<Uuid>, event_details: Value, previous_hash: &str) -> Result<(), sqlx::Error> {
        let current_hash = format!("{:x}", sha2::Sha256::digest(format!("{}{:?}{}{}", event_type, user_id, event_details, previous_hash).as_bytes()));

        sqlx::query!(
            "INSERT INTO audit_logs (event_type, user_id, event_details, previous_hash, current_hash) VALUES ($1, $2, $3, $4, $5)",
            event_type,
            user_id,
            event_details,
            previous_hash,
            current_hash
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_user_consents(&self, user_id: Uuid) -> Result<Vec<Consent>, sqlx::Error> {
        let consents = sqlx::query_as!(
            Consent,
            "SELECT id, scope, created_at, expires_at, status FROM consents WHERE user_id = $1",
            user_id
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(consents)
    }

    pub async fn query_audit_logs(&self, start_time: DateTime<Utc>, end_time: DateTime<Utc>, event_type: Option<&str>) -> Result<Vec<AuditLog>, sqlx::Error> {
        let audit_logs = sqlx::query_as!(
            AuditLog,
            "SELECT id, event_time, event_type, user_id, event_details, previous_hash, current_hash 
            FROM audit_logs 
            WHERE event_time BETWEEN $1 AND $2 
            AND ($3::text IS NULL OR event_type = $3)
            ORDER BY event_time",
            start_time,
            end_time,
            event_type
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(audit_logs)
    }
}

#[derive(Debug)]
pub struct Consent {
    pub id: Uuid,
    pub scope: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub status: String,
}

#[derive(Debug)]
pub struct AuditLog {
    pub id: Uuid,
    pub event_time: DateTime<Utc>,
    pub event_type: String,
    pub user_id: Option<Uuid>,
    pub event_details: Value,
    pub previous_hash: String,
    pub current_hash: String,
}
