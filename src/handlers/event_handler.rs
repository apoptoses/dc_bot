use poise::serenity_prelude as serenity;
use std::sync::atomic::Ordering;
use std::time::Duration;

pub async fn handle_event<'a>(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'a, crate::Data, crate::Error>,
    data: &crate::Data,
) -> Result<(), crate::Error> {
    match event {
        serenity::FullEvent::Ready { data_about_bot, .. } => {
            let startup_duration = data.started_at.elapsed();
            let commands_check = data.commands_check_duration;

            fn fmt_dur(d: Duration) -> String {
                if d.as_secs() >= 1 {
                    format!("{:.3}s", d.as_secs_f64())
                } else {
                    format!("{:.3}ms", d.as_secs_f64() * 1000.0)
                }
            }
            
            let mut name_w = "Name".len();
            let mut status_w = "Status".len();
            for s in &data.command_statuses {
                name_w = name_w.max(s.name.len());
                status_w = status_w.max(s.status.len());
            }
            let total_commands = data.command_statuses.len();

            let title = format!("Bot Ready: {}", data_about_bot.user.name);
            let meta_left = format!(
                "Startup time: {}",
                fmt_dur(startup_duration)
            );
            let meta_right = format!(
                "Commands check: {}",
                fmt_dur(commands_check)
            );
            let meta_w = meta_left.len().max(meta_right.len());

            let table_width = 2 + name_w + 3 + status_w + 2; // | name | status |
            let header_width = title.len().max(meta_w).max(table_width).max(30);
            let hline = format!("+{}+", "=".repeat(header_width));
            let sline = format!("+{}+", "-".repeat(header_width));

            println!("{}", hline);
            println!("|{:<width$}|", title, width = header_width);
            println!("{}", sline);
            println!("|{:<width$}|", meta_left, width = header_width);
            println!("|{:<width$}|", meta_right, width = header_width);
            println!("|{:<width$}|", format!("Commands loaded: {}", total_commands), width = header_width);
            println!("{}", sline);
            
            let table_hline = format!(
                "+-{}-+-{}-+",
                "-".repeat(name_w),
                "-".repeat(status_w)
            );
            println!("{}", table_hline);
            println!(
                "| {:<name_w$} | {:<status_w$} |",
                "Name",
                "Status",
                name_w = name_w,
                status_w = status_w
            );
            println!("{}", table_hline);
            if total_commands == 0 {
                println!(
                    "| {:<name_w$} | {:<status_w$} |",
                    "(no commands)",
                    "-",
                    name_w = name_w,
                    status_w = status_w
                );
            } else {
                for s in &data.command_statuses {
                    println!(
                        "| {:<name_w$} | {:<status_w$} |",
                        s.name,
                        s.status,
                        name_w = name_w,
                        status_w = status_w
                    );
                }
            }
            println!("{}", table_hline);
            println!("{}", hline);

            // Start YouTube poller once on Ready
            if !data.youtube_poller_started.swap(true, Ordering::SeqCst) {
                let http = ctx.http.clone();
                let schema = data.youtube_schema.clone();
                let path = data.youtube_schema_path.clone();
                tokio::spawn(async move {
                    use tokio::time::{interval, Duration as TokioDuration};
                    let mut ticker = interval(TokioDuration::from_secs(300)); // every 5 minutes
                    loop {
                        ticker.tick().await;
                        // Snapshot subscriptions
                        let snapshot = {
                            let guard = schema.read().await;
                            guard.clone()
                        };
                        for (guild_id, subs) in snapshot.guilds.iter() {
                            for (key, sub) in subs.iter() {
                                // Determine if any notification type is enabled
                                if !(sub.videos || sub.shorts || sub.streams || sub.podcasts || sub.playlists || sub.posts || sub.store || sub.releases) {
                                    continue;
                                }
                                if let Some(feed_url) = crate::commands::social_notifications::youtube::resolve_feed_url(&sub.youtube_channel).await {
                                    if let Ok(items) = crate::commands::social_notifications::youtube::fetch_latest_uploads(&feed_url, 1).await {
                                        if let Some((title, link)) = items.get(0) {
                                            if link.is_empty() { continue; }
                                            let is_short = link.contains("/shorts/");
                                            // Basic filter: shorts vs regular videos
                                            let allowed = if is_short { sub.shorts } else { sub.videos };
                                            if !allowed { continue; }

                                            let already = snapshot
                                                .last_notified
                                                .get(guild_id)
                                                .and_then(|m| m.get(key))
                                                .cloned();
                                            if already.as_deref() != Some(link) {
                                                let content = format!(
                                                    "New {} from {}: {}\n{}",
                                                    if is_short { "short" } else { "video" },
                                                    sub.youtube_channel,
                                                    title,
                                                    link
                                                );
                                                let channel_id = serenity::ChannelId::new(sub.notify_channel_id);
                                                let _ = channel_id.say(&http, content).await;

                                                // Persist last_notified
                                                {
                                                    let mut guard = schema.write().await;
                                                    let map = guard.last_notified.entry(*guild_id).or_default();
                                                    map.insert(key.clone(), link.clone());
                                                    let _ = guard.save_to_disk(&path).await;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                });
            }
        }
        _ => {}
    }
    Ok(())
}
