use poise::serenity_prelude as serenity;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

pub struct CommandStatus {
    pub name: String,
    pub status: String,
}

pub struct Data {
    pub started_at: Instant,
    pub commands_check_duration: Duration,
    pub command_statuses: Vec<CommandStatus>,
    pub youtube_schema: Arc<RwLock<crate::data::youtube_schema::YoutubeSchema>>,
    pub youtube_schema_path: PathBuf,
    pub youtube_poller_started: Arc<std::sync::atomic::AtomicBool>,
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;

mod handlers;
pub mod commands;
pub mod data;

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    let program_started = Instant::now();

    let token = std::env::var("TOKEN").expect("missing TOKEN");
    let intents = serenity::GatewayIntents::privileged();

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: handlers::command_handler::commands(),
            event_handler: |ctx, event, framework, data| {
                Box::pin(async move {
                    handlers::event_handler::handle_event(ctx, event, framework, data).await
                })
            },
            ..Default::default()
        })
        .setup(move |ctx, _ready, framework| {
            let program_started = program_started;
            Box::pin(async move {
                let mut statuses: Vec<CommandStatus> = framework
                    .options()
                    .commands
                    .iter()
                    .map(|c| CommandStatus {
                        name: c.name.to_string(),
                        status: "Loaded".to_string(),
                    })
                    .collect();
                
                let check_started = Instant::now();
                let reg_result = poise::builtins::register_globally(ctx, &framework.options().commands).await;
                let commands_check_duration = check_started.elapsed();
                
                match reg_result {
                    Ok(()) => {
                        for s in &mut statuses {
                            s.status = "Registered".to_string();
                        }
                    }
                    Err(e) => {
                        let msg = format!("Reg err: {}", e);
                        for s in &mut statuses {
                            s.status = msg.clone();
                        }
                    }
                }

                let youtube_schema_path = PathBuf::from("src\\data\\youtube_subscriptions.json");
                let initial_youtube_schema = crate::data::youtube_schema::YoutubeSchema::load_from_disk(&youtube_schema_path).await.unwrap_or_else(|e| {
                    eprintln!("Failed to load YouTube schema: {}", e);
                    crate::data::youtube_schema::YoutubeSchema::default()
                });

                Ok(Data {
                    started_at: program_started,
                    commands_check_duration,
                    command_statuses: statuses,
                    youtube_schema: Arc::new(RwLock::new(initial_youtube_schema)),
                    youtube_schema_path,
                    youtube_poller_started: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                })
            })
        })
        .build();

    let client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await;
    client.unwrap().start().await.unwrap();
}