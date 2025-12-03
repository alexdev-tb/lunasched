use common::{NotificationChannel, Job};
use anyhow::Result;

pub struct Notifier;

impl Notifier {
    pub fn new() -> Self {
        Self
    }
    
    /// Send notifications for a job event
    pub async fn notify(&self, job: &Job, event: &str, message: &str, channels: &[NotificationChannel]) {
        for channel in channels {
            if let Err(e) = self.send_notification(job, event, message, channel).await {
                log::error!("Failed to send notification via {:?}: {}", channel, e);
            }
        }
    }
    
    async fn send_notification(
        &self,
        job: &Job,
        event: &str,
        message: &str,
        channel: &NotificationChannel,
    ) -> Result<()> {
        match channel {
            NotificationChannel::Email { to, subject } => {
                self.send_email(job, event, message, to, subject.as_deref()).await
            }
            NotificationChannel::Webhook { url, headers } => {
                self.send_webhook(job, event, message, url, headers.as_ref()).await
            }
            NotificationChannel::Discord { webhook_url } => {
                self.send_discord(job, event, message, webhook_url).await
            }
            NotificationChannel::Slack { webhook_url } => {
                self.send_slack(job, event, message, webhook_url).await
            }
        }
    }
    
    async fn send_email(
        &self,
        job: &Job,
        event: &str,
        message: &str,
        to: &str,
        subject: Option<&str>,
    ) -> Result<()> {
        use lettre::message::header::ContentType;
        use lettre::transport::smtp::authentication::Credentials;
        use lettre::{Message, SmtpTransport, Transport};
        
        let subject_str = subject.map(|s| s.to_string())
            .unwrap_or_else(|| format!("Lunasched: Job {} - {}", job.name, event));
        let from = std::env::var("LUNASCHED_EMAIL_FROM")
            .unwrap_or_else(|_| "lunasched@localhost".to_string());
        
        let email = Message::builder()
            .from(from.parse()?)
            .to(to.parse()?)
            .subject(&subject_str)
            .header(ContentType::TEXT_PLAIN)
            .body(format!(
                "Job: {}\nEvent: {}\nOwner: {}\nSchedule: {:?}\n\n{}",
                job.name, event, job.owner, job.schedule, message
            ))?;
        
        // Only try to send if SMTP credentials are configured
        if let (Ok(smtp_server), Ok(smtp_username), Ok(smtp_password)) = (
            std::env::var("LUNASCHED_SMTP_SERVER"),
            std::env::var("LUNASCHED_SMTP_USERNAME"),
            std::env::var("LUNASCHED_SMTP_PASSWORD"),
        ) {
            let creds = Credentials::new(smtp_username, smtp_password);
            let mailer = SmtpTransport::relay(&smtp_server)?
                .credentials(creds)
                .build();
            
            mailer.send(&email)?;
            log::info!("Email notification sent to {} for job {}", to, job.name);
        } else {
            log::warn!("SMTP not configured, skipping email notification");
        }
        
        Ok(())
    }
    
    async fn send_webhook(
        &self,
        job: &Job,
        event: &str,
        message: &str,
        url: &str,
        headers: Option<&std::collections::HashMap<String, String>>,
    ) -> Result<()> {
        let client = reqwest::Client::new();
        let mut request = client.post(url);
        
        if let Some(headers_map) = headers {
            for (key, value) in headers_map {
                request = request.header(key, value);
            }
        }
        
        let payload = serde_json::json!({
            "job_id": job.id.0,
            "job_name": job.name,
            "event": event,
            "message": message,
            "owner": job.owner,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        
        let response = request.json(&payload).send().await?;
        
        if response.status().is_success() {
            log::info!("Webhook notification sent to {} for job {}", url, job.name);
        } else {
            log::error!("Webhook failed with status: {}", response.status());
        }
        
        Ok(())
    }
    
    async fn send_discord(
        &self,
        job: &Job,
        event: &str,
        message: &str,
        webhook_url: &str,
    ) -> Result<()> {
        let client = reqwest::Client::new();
        
        let color = match event {
            "success" => 0x00ff00, // Green
            "failure" => 0xff0000, // Red
            "start" => 0x0000ff,   // Blue
            _ => 0x808080,         // Gray
        };
        
        let payload = serde_json::json!({
            "embeds": [{
                "title": format!("Job {} - {}", job.name, event),
                "description": message,
                "color": color,
                "fields": [
                    {"name": "Job ID", "value": job.id.0, "inline": true},
                    {"name": "Owner", "value": job.owner, "inline": true},
                ],
                "timestamp": chrono::Utc::now().to_rfc3339(),
            }]
        });
        
        let response = client.post(webhook_url).json(&payload).send().await?;
        
        if response.status().is_success() {
            log::info!("Discord notification sent for job {}", job.name);
        } else {
            log::error!("Discord webhook failed with status: {}", response.status());
        }
        
        Ok(())
    }
    
    async fn send_slack(
        &self,
        job: &Job,
        event: &str,
        message: &str,
        webhook_url: &str,
    ) -> Result<()> {
        let client = reqwest::Client::new();
        
        let emoji = match event {
            "success" => ":white_check_mark:",
            "failure" => ":x:",
            "start" => ":rocket:",
            _ => ":grey_question:",
        };
        
        let payload = serde_json::json!({
            "text": format!("{} Job {} - {}", emoji, job.name, event),
            "blocks": [{
                "type": "section",
                "text": {
                    "type": "mrkdwn",
                    "text": format!(
                        "*Job:* {}\n*Event:* {}\n*Owner:* {}\n\n{}",
                        job.name, event, job.owner, message
                    )
                }
            }]
        });
        
        let response = client.post(webhook_url).json(&payload).send().await?;
        
        if response.status().is_success() {
            log::info!("Slack notification sent for job {}", job.name);
        } else {
            log::error!("Slack webhook failed with status: {}", response.status());
        }
        
        Ok(())
    }
}
