use poise::serenity_prelude as serenity;
use std::time::{Duration, Instant};

pub struct CommandStatus {
    pub name: String,
    pub status: String,
}

pub struct Data {
    pub started_at: Instant,
    pub commands_check_duration: Duration,
    pub command_statuses: Vec<CommandStatus>,
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

                Ok(Data {
                    started_at: program_started,
                    commands_check_duration,
                    command_statuses: statuses,
                })
            })
        })
        .build();

    let client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await;
    client.unwrap().start().await.unwrap();
}