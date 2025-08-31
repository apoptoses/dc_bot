use crate::data::youtube_schema::{normalize_youtube_id, YouTubeSubscription};
use std::io::Cursor;

fn parse_channel_id(input: &str) -> Option<u64> {
    let s = input.trim();
    if let Some(rest) = s.strip_prefix("<#") {
        if rest.ends_with('>') {
            let inner = &rest[..rest.len() - 1];
            if let Ok(id) = inner.parse::<u64>() {
                return Some(id);
            }
        }
    }
    if s.chars().all(|c| c.is_ascii_digit()) {
        return s.parse::<u64>().ok();
    }
    None
}

#[poise::command(slash_command, prefix_command)]
pub async fn sub(
    ctx: poise::Context<'_, crate::Data, crate::Error>,
    #[description = "YouTube channel identifier (URL, handle, or ID)"] youtube_channel: String,
    #[description = "Destination Discord channel (e.g., #channel or ID)"] notify_channel: String,
    #[description = "Notify for regular videos"] videos: Option<bool>,
    #[description = "Notify for shorts"] shorts: Option<bool>,
    #[description = "Notify for live streams"] streams: Option<bool>,
    #[description = "Notify for podcasts"] podcasts: Option<bool>,
    #[description = "Notify for playlists"] playlists: Option<bool>,
    #[description = "Notify for posts"] posts: Option<bool>,
    #[description = "Notify for store updates"] store: Option<bool>,
    #[description = "Notify for releases"] releases: Option<bool>,
) -> Result<(), crate::Error> {
    // Must be used in a guild
    let guild_id = match ctx.guild_id() {
        Some(g) => g.get(),
        None => {
            ctx.say("").await?;
            return Ok(());
        }
    };

    // Parse channel mention or numeric ID
    let Some(channel_id) = parse_channel_id(&notify_channel) else {
        ctx.say("Couldn't parse the notification channel. Please provide a channel mention like #channel or a numeric channel ID.").await?;
        return Ok(());
    };

    // Option defaults: if none specified, enable all. Otherwise, use provided values with false default.
    let any_specified = videos.is_some()
        || shorts.is_some()
        || streams.is_some()
        || podcasts.is_some()
        || playlists.is_some()
        || posts.is_some()
        || store.is_some()
        || releases.is_some();
    let (videos, shorts, streams, podcasts, playlists, posts, store, releases) = if any_specified {
        (
            videos.unwrap_or(false),
            shorts.unwrap_or(false),
            streams.unwrap_or(false),
            podcasts.unwrap_or(false),
            playlists.unwrap_or(false),
            posts.unwrap_or(false),
            store.unwrap_or(false),
            releases.unwrap_or(false),
        )
    } else {
        (true, true, true, true, true, true, true, true)
    };

    let key = normalize_youtube_id(&youtube_channel);
    let sub = YouTubeSubscription {
        youtube_channel: youtube_channel.clone(),
        youtube_key: key.clone(),
        notify_channel_id: channel_id,
        videos,
        shorts,
        streams,
        podcasts,
        playlists,
        posts,
        store,
        releases,
    };

    // Write to store then persist to disk
    let path = ctx.data().youtube_schema_path.clone();
    {
        let mut guard = ctx.data().youtube_schema.write().await;
        guard.upsert_subscription(guild_id, sub);
        // Clone snapshot for saving outside the lock
        let snapshot = guard.clone();
        drop(guard);
        if let Err(e) = snapshot.save_to_disk(&path).await {
            ctx.say(format!(
                "Saved subscription, but failed to write database file: {}",
                e
            ))
            .await?;
            return Ok(());
        }
    }

    ctx.say(format!(
        "Subscribed to '{}' in this server. Notifications -> <#{}> (videos={}, shorts={}, streams={}, podcasts={}, playlists={}, posts={}, store={}, releases={}).",
        youtube_channel, channel_id, videos, shorts, streams, podcasts, playlists, posts, store, releases
    ))
    .await?;

    // Try to show a preview of the latest uploads
    match resolve_feed_url(&youtube_channel).await {
        Some(feed_url) => {
            match fetch_latest_uploads(&feed_url, 3).await {
                Ok(items) if !items.is_empty() => {
                    let mut preview = String::from("Latest uploads preview:\n");
                    for (i, (title, link)) in items.iter().enumerate() {
                        let idx = i + 1;
                        if !link.is_empty() {
                            preview.push_str(&format!("{}. {} - {}\n", idx, title, link));
                        } else {
                            preview.push_str(&format!("{}. {}\n", idx, title));
                        }
                    }
                    let _ = ctx.say(preview).await;
                }
                _ => {
                    let _ = ctx.say("Couldn't fetch latest uploads preview right now. The subscription was saved and notifications will use your settings.").await;
                }
            }
        }
        None => {
            let _ = ctx.say("Couldn't resolve the channel feed for a preview, but your subscription was saved.").await;
        }
    }

    Ok(())
}

#[poise::command(slash_command, prefix_command)]
pub async fn unsub(
    ctx: poise::Context<'_, crate::Data, crate::Error>,
    #[description = "YouTube channel identifier (URL, handle, or ID)"] youtube_channel: String,
) -> Result<(), crate::Error> {
    let guild_id = match ctx.guild_id() {
        Some(g) => g.get(),
        None => {
            ctx.say("This command can only be used in a server (guild).").await?;
            return Ok(());
        }
    };

    let key = normalize_youtube_id(&youtube_channel);
    let path = ctx.data().youtube_schema_path.clone();

    let removed = {
        let mut guard = ctx.data().youtube_schema.write().await;
        let existed = guard.remove_subscription(guild_id, &key);
        let snapshot = guard.clone();
        drop(guard);
        if let Err(e) = snapshot.save_to_disk(&path).await {
            ctx.say(format!(
                "Processed unsubscribe, but failed to write database file: {}",
                e
            ))
            .await?;
        }
        existed
    };

    if removed {
        ctx.say(format!("Unsubscribed from '{}' for this server.", youtube_channel)).await?;
    } else {
        ctx.say(format!("No existing subscription for '{}' in this server.", youtube_channel)).await?;
    }

    Ok(())
}


// --- YouTube feed helpers ---
pub(crate) async fn resolve_feed_url(input: &str) -> Option<String> {
    let s = input.trim();

    // If full URL contains /channel/UC...
    if let Some(idx) = s.to_ascii_lowercase().find("/channel/") {
        let tail = &s[idx + "/channel/".len()..];
        let id: String = tail
            .chars()
            .take_while(|&c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            .collect();
        if id.starts_with("UC") && id.len() >= 6 {
            return Some(format!("https://www.youtube.com/feeds/videos.xml?channel_id={}", id));
        }
    }

    // If looks like a channel id directly (UC...)
    let looks_like_uc = s.starts_with("UC") && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if looks_like_uc {
        return Some(format!("https://www.youtube.com/feeds/videos.xml?channel_id={}", s));
    }

    // For handles or other YT URLs, fetch page HTML and extract channelId
    // Strategy:
    // - If input starts with @ -> try https://www.youtube.com/@handle
    // - Else if input contains youtube.com -> use it directly
    // - Else treat input as a handle-like name: try @name, then /c/name, then /user/name
    let mut candidates: Vec<String> = Vec::new();
    if s.starts_with('@') {
        candidates.push(format!("https://www.youtube.com/{}", s));
    } else if s.to_ascii_lowercase().contains("youtube.com/") {
        candidates.push(s.to_string());
    } else {
        let name = s.trim_matches('/');
        let handle = if name.starts_with('@') { name.to_string() } else { format!("@{}", name) };
        candidates.push(format!("https://www.youtube.com/{}", handle));
        candidates.push(format!("https://www.youtube.com/c/{}", name));
        candidates.push(format!("https://www.youtube.com/user/{}", name));
    }

    for page_url in candidates {
        if let Ok(resp) = reqwest::Client::new()
            .get(&page_url)
            .header("user-agent", "dc_bot/0.0.1 (+https://github.com)")
            .send()
            .await
        {
            if let Ok(text) = resp.text().await {
                if let Some(ch_id) = extract_channel_id_from_html(&text) {
                    return Some(format!("https://www.youtube.com/feeds/videos.xml?channel_id={}", ch_id));
                }
            }
        }
    }

    None
}

fn extract_channel_id_from_html(html: &str) -> Option<String> {
    // Very lightweight search for patterns like "channelId":"UC..." or "channelId=UC..."
    let hay = html;
    let key = "channelId";
    let mut start_idx = 0usize;
    while let Some(pos) = hay[start_idx..].find(key) {
        let i = start_idx + pos;
        // Search forward from here to find the next 'U' that starts with "UC"
        let tail = &hay[i..];
        if let Some(uc_pos) = tail.find("UC") {
            let abs = i + uc_pos;
            // Collect allowed chars following UC
            let mut id = String::new();
            for ch in hay[abs..].chars() {
                if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                    id.push(ch);
                    if id.len() > 64 { break; }
                } else {
                    break;
                }
            }
            if id.starts_with("UC") && id.len() >= 6 {
                return Some(id);
            }
        }
        start_idx = i + key.len();
    }
    None
}

pub(crate) async fn fetch_latest_uploads(feed_url: &str, limit: usize) -> Result<Vec<(String, String)>, crate::Error> {
    let client = reqwest::Client::new();
    let res = client
        .get(feed_url)
        .header("user-agent", "dc_bot/0.0.1 (+https://github.com)")
        .send()
        .await?;

    if !res.status().is_success() {
        return Ok(Vec::new());
    }

    let bytes = res.bytes().await?;
    let cursor = Cursor::new(bytes);
    let feed = match feed_rs::parser::parse(cursor) {
        Ok(f) => f,
        Err(_) => return Ok(Vec::new()),
    };

    let mut out = Vec::new();
    for entry in feed.entries.iter().take(limit) {
        let title = entry
            .title
            .as_ref()
            .map(|t| t.content.clone())
            .unwrap_or_else(|| "(untitled)".to_string());
        let link = entry
            .links
            .iter()
            .find(|l| l.rel.as_deref() == Some("alternate"))
            .map(|l| l.href.clone())
            .or_else(|| entry.links.first().map(|l| l.href.clone()))
            .unwrap_or_else(|| "".to_string());
        out.push((title, link));
    }

    Ok(out)
}
