use tokio;
use sqlx::{Pool, Postgres};
use lettre::message::MessageBuilder;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{SmtpTransport, AsyncTransport};
use serde::{Deserialize, Serialize};
use lapin::{Connection, ConnectionProperties, Channel, options::*, types::FieldTable};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
struct NotificationEvent {
    user_id: Uuid,
    notification_type: String,
    content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct UserPreferences {
    email_enabled: bool,
    sms_enabled: bool,
    push_enabled: bool,
    phone_number: Option<String>,
    push_token: Option<String>,
}

struct NotificationService {
    db_pool: Pool<Postgres>,
    email_client: SmtpTransport,
    sms_client: reqwest::Client,
    push_client: reqwest::Client,
    amqp_channel: Channel,
}

impl NotificationService {
    async fn new(db_pool: Pool<Postgres>, smtp_credentials: Credentials, amqp_addr: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let email_client = SmtpTransport::relay("smtp.gmail.com")?
            .credentials(smtp_credentials)
            .build();

        let sms_client = reqwest::Client::new();
        let push_client = reqwest::Client::new();

        let amqp_conn = Connection::connect(amqp_addr, ConnectionProperties::default()).await?;
        let amqp_channel = amqp_conn.create_channel().await?;

        Ok(Self {
            db_pool,
            email_client,
            sms_client,
            push_client,
            amqp_channel,
        })
    }

    async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.amqp_channel
            .queue_declare(
                "notifications",
                QueueDeclareOptions::default(),
                FieldTable::default(),
            )
            .await?;

        let mut consumer = self.amqp_channel
            .basic_consume(
                "notifications",
                "notification_consumer",
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await?;

        println!("Notification service started. Waiting for messages...");

        while let Some(delivery) = consumer.next().await {
            let delivery = delivery?;
            let notification: NotificationEvent = serde_json::from_slice(&delivery.data)?;
            
            self.process_notification(notification).await?;

            self.amqp_channel
                .basic_ack(delivery.delivery_tag, BasicAckOptions::default())
                .await?;
        }

        Ok(())
    }

    async fn process_notification(&self, notification: NotificationEvent) -> Result<(), Box<dyn std::error::Error>> {
        let preferences = self.get_user_preferences(notification.user_id).await?;

        if preferences.email_enabled {
            self.send_email(&notification).await?;
        }

        if preferences.sms_enabled && preferences.phone_number.is_some() {
            self.send_sms(&notification, preferences.phone_number.unwrap()).await?;
        }

        if preferences.push_enabled && preferences.push_token.is_some() {
            self.send_push(&notification, preferences.push_token.unwrap()).await?;
        }

        self.log_notification(&notification, "sent").await?;

        Ok(())
    }

    async fn get_user_preferences(&self, user_id: Uuid) -> Result<UserPreferences, sqlx::Error> {
        sqlx::query_as!(
            UserPreferences,
            "SELECT email_enabled, sms_enabled, push_enabled, phone_number, push_token
            FROM user_notification_preferences
            WHERE user_id = $1",
            user_id
        )
        .fetch_one(&self.db_pool)
        .await
    }

    async fn send_email(&self, notification: &NotificationEvent) -> Result<(), Box<dyn std::error::Error>> {
        let email = MessageBuilder::new()
            .to("user@example.com".parse()?)
            .from("noreply@yourapp.com".parse()?)
            .subject(&notification.notification_type)
            .body(notification.content.clone())?;

        self.email_client.send(email).await?;
        Ok(())
    }

    async fn send_sms(&self, notification: &NotificationEvent, phone_number: String) -> Result<(), Box<dyn std::error::Error>> {
        // Implement SMS sending logic here
        // This is a placeholder and should be replaced with actual SMS gateway integration
        println!("Sending SMS to {}: {}", phone_number, notification.content);
        Ok(())
    }

    async fn send_push(&self, notification: &NotificationEvent, push_token: String) -> Result<(), Box<dyn std::error::Error>> {
        // Implement push notification logic here
        // This is a placeholder and should be replaced with actual push notification service integration
        println!("Sending push notification to {}: {}", push_token, notification.content);
        Ok(())
    }

    async fn log_notification(&self, notification: &NotificationEvent, status: &str) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "INSERT INTO notification_history (user_id, notification_type, channel, content, status)
            VALUES ($1, $2, $3, $4, $5)",
            notification.user_id,
            notification.notification_type,
            "multi-channel", // This should be more specific in a real implementation
            notification.content,
            status
        )
        .execute(&self.db_pool)
        .await?;

        Ok(())
    }

    async fn retry_failed_notifications(&self) -> Result<(), Box<dyn std::error::Error>> {
        let failed_notifications = sqlx::query!(
            "SELECT id, user_id, notification_type, content
            FROM notification_history
            WHERE status = 'failed'
            AND created_at > NOW() - INTERVAL '24 hours'"
        )
        .fetch_all(&self.db_pool)
        .await?;

        for notification in failed_notifications {
            let event = NotificationEvent {
                user_id: notification.user_id,
                notification_type: notification.notification_type,
                content: notification.content,
            };

            self.process_notification(event).await?;

            sqlx::query!(
                "UPDATE notification_history SET status = 'retried', updated_at = NOW() WHERE id = $1",
                notification.id
            )
            .execute(&self.db_pool)
            .await?;
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db_pool = sqlx::PgPool::connect("postgres://username:password@localhost/database").await?;
    let smtp_credentials = Credentials::new("your-email@gmail.com".to_string(), "your-password".to_string());
    let amqp_addr = "amqp://guest:guest@localhost:5672/%2f";

    let notification_service = NotificationService::new(db_pool, smtp_credentials, amqp_addr).await?;

    // Start a task to periodically retry failed notifications
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await; // Retry every hour
            if let Err(e) = notification_service.retry_failed_notifications().await {
                eprintln!("Error retrying failed notifications: {}", e);
            }
        }
    });

    notification_service.start().await?;

    Ok(())
}
