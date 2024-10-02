use actix_web::{web, App, HttpServer, Responder, HttpResponse};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use sha2::{Sha256, Digest};
use sqlx::PgPool;
use futures::StreamExt;
use rdkafka::consumer::{StreamConsumer, Consumer};
use rdkafka::config::ClientConfig;

#[derive(Serialize, Deserialize)]
struct AuditEvent {
    id: uuid::Uuid,
    timestamp: DateTime<Utc>,
    event_type: String,
    user_id: uuid::Uuid,
    details: serde_json::Value,
    previous_hash: String,
    current_hash: String,
}

struct AuditLogService {
    db_pool: PgPool,
    kafka_consumer: StreamConsumer,
}

impl AuditLogService {
    async fn new(database_url: &str, kafka_brokers: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let db_pool = PgPool::connect(database_url).await?;
        
        let consumer: StreamConsumer = ClientConfig::new()
            .set("group.id", "audit_log_group")
            .set("bootstrap.servers", kafka_brokers)
            .set("enable.auto.commit", "true")
            .create()?;

        Ok(Self { db_pool, kafka_consumer })
    }

    async fn process_events(&self) {
        self.kafka_consumer.subscribe(&["audit_events"]).unwrap();

        while let Some(message) = self.kafka_consumer.stream().next().await {
            match message {
                Ok(msg) => {
                    if let Some(payload) = msg.payload() {
                        let event: AuditEvent = serde_json::from_slice(payload).unwrap();
                        self.store_event(event).await.unwrap();
                    }
                }
                Err(e) => eprintln!("Error while receiving message: {:?}", e),
            }
        }
    }

    async fn store_event(&self, mut event: AuditEvent) -> Result<(), sqlx::Error> {
        let previous_hash = self.get_last_hash().await?;
        event.previous_hash = previous_hash;
        event.current_hash = self.calculate_hash(&event);

        sqlx::query!(
            "INSERT INTO audit_logs (id, timestamp, event_type, user_id, details, previous_hash, current_hash) 
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            event.id,
            event.timestamp,
            event.event_type,
            event.user_id,
            event.details,
            event.previous_hash,
            event.current_hash
        )
        .execute(&self.db_pool)
        .await?;

        Ok(())
    }

    async fn get_last_hash(&self) -> Result<String, sqlx::Error> {
        let result = sqlx::query!("SELECT current_hash FROM audit_logs ORDER BY timestamp DESC LIMIT 1")
            .fetch_optional(&self.db_pool)
            .await?;

        Ok(result.map(|r| r.current_hash).unwrap_or_else(|| "0".repeat(64)))
    }

    fn calculate_hash(&self, event: &AuditEvent) -> String {
        let mut hasher = Sha256::new();
        hasher.update(event.id.to_string());
        hasher.update(event.timestamp.to_rfc3339());
        hasher.update(&event.event_type);
        hasher.update(event.user_id.to_string());
        hasher.update(event.details.to_string());
        hasher.update(&event.previous_hash);
        format!("{:x}", hasher.finalize())
    }

    async fn query_logs(&self, start_date: DateTime<Utc>, end_date: DateTime<Utc>, event_type: Option<String>) -> Result<Vec<AuditEvent>, sqlx::Error> {
        let query = sqlx::query_as!(
            AuditEvent,
            "SELECT * FROM audit_logs WHERE timestamp BETWEEN $1 AND $2 AND ($3::text IS NULL OR event_type = $3) ORDER BY timestamp",
            start_date,
            end_date,
            event_type
        )
        .fetch_all(&self.db_pool)
        .await?;

        Ok(query)
    }

    async fn verify_integrity(&self, start_id: uuid::Uuid, end_id: uuid::Uuid) -> Result<bool, sqlx::Error> {
        let logs = sqlx::query_as!(
            AuditEvent,
            "SELECT * FROM audit_logs WHERE id BETWEEN $1 AND $2 ORDER BY timestamp",
            start_id,
            end_id
        )
        .fetch_all(&self.db_pool)
        .await?;

        for window in logs.windows(2) {
            let current = &window[0];
            let next = &window[1];
            if current.current_hash != next.previous_hash || self.calculate_hash(current) != current.current_hash {
                return Ok(false);
            }
        }

        Ok(true)
    }
}

async fn query_logs(
    audit_service: web::Data<AuditLogService>,
    query: web::Query<LogQuery>,
) -> impl Responder {
    match audit_service.query_logs(query.start_date, query.end_date, query.event_type.clone()).await {
        Ok(logs) => HttpResponse::Ok().json(logs),
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}

async fn verify_integrity(
    audit_service: web::Data<AuditLogService>,
    params: web::Path<(uuid::Uuid, uuid::Uuid)>,
) -> impl Responder {
    match audit_service.verify_integrity(params.0, params.1).await {
        Ok(is_valid) => HttpResponse::Ok().json(is_valid),
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let audit_service = AuditLogService::new("postgres://user:password@localhost/audit_db", "localhost:9092")
        .await
        .expect("Failed to create audit log service");
    
    let audit_service = web::Data::new(audit_service);
    
    let service_clone = audit_service.clone();
    tokio::spawn(async move {
        service_clone.process_events().await;
    });

    HttpServer::new(move || {
        App::new()
            .app_data(audit_service.clone())
            .route("/query", web::get().to(query_logs))
            .route("/verify/{start_id}/{end_id}", web::get().to(verify_integrity))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}

