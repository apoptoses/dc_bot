use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use serde_json::Value;
use std::sync::{Arc, OnceLock};
use tokio::sync::Semaphore;

static RANK_EMOJI_MAP: OnceLock<std::collections::HashMap<String, String>> = OnceLock::new();
static AGENT_EMOJI_MAP: OnceLock<std::collections::HashMap<String, String>> = OnceLock::new();

fn normalize_rank_key(rank: &str) -> String {
    let r = rank.trim().to_lowercase();
    if r.is_empty() { return "unranked".to_string(); }
    let mut parts = r.split_whitespace();
    let base = parts.next().unwrap_or("");
    let num = parts.next();
    let base_norm = match base {
        "unrated" | "unranked" => "unranked".to_string(),
        _ => base.to_string(),
    };
    if let Some(n) = num {
        if n == "1" { return base_norm; }
        return format!("{}{}", base_norm, n);
    }
    base_norm
}

fn load_rank_emoji_map() -> &'static std::collections::HashMap<String, String> {
    RANK_EMOJI_MAP.get_or_init(|| {
        let mut map = std::collections::HashMap::new();
        let data = include_str!("../../../assets/valorant/ranks/valorant_ranks_emojis.rs");
        for line in data.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { continue; }
            let mut parts = line.split_whitespace();
            let key = parts.next().unwrap_or("").to_lowercase();
            let emoji = parts.next().unwrap_or("");
            if !key.is_empty() && !emoji.is_empty() {
                map.insert(key, emoji.to_string());
            }
        }
        map
    })
}

fn get_rank_emoji(rank_text: &str) -> Option<String> {
    let key = normalize_rank_key(rank_text);
    let map = load_rank_emoji_map();
    map.get(&key).cloned()
}

fn load_agent_emoji_map() -> &'static std::collections::HashMap<String, String> {
    AGENT_EMOJI_MAP.get_or_init(|| {
        let mut map = std::collections::HashMap::new();
        let data = include_str!("../../../assets/valorant/agents/valorant_agents_emojis.rs");
        for line in data.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { continue; }
            let mut parts = line.split_whitespace();
            let key = parts.next().unwrap_or("").to_lowercase();
            let emoji = parts.next().unwrap_or("");
            if !key.is_empty() && !emoji.is_empty() {
                map.insert(key, emoji.to_string());
            }
        }
        map
    })
}

fn get_agent_emoji(agent_name: &str) -> Option<String> {
    let key = agent_name.trim().to_lowercase();
    let map = load_agent_emoji_map();
    map.get(&key).cloned()
}

pub async fn fetch_custom_match_data(
    auth: &str,
    region: &str,
    platform: &str,
    name: &str,
    tag: &str,
    guild_id: &str,
    mode_type: &str,
    start: u32,
    skip_stored: bool,
    query_size: u8,
    store_matches: u8,
) -> Result<Option<Value>, Box<dyn std::error::Error + Send + Sync>> {
    const REGIONS: &[&str] = &["eu", "na", "latam", "br", "ap", "kr"];
    if !REGIONS.contains(&region) {
        return Err(format!("invalid region '{}'. Allowed: eu, na, latam, br, ap, kr", region).into());
    }
    const PLATFORMS: &[&str] = &["pc", "console"];
    if !PLATFORMS.contains(&platform) {
        return Err(format!("invalid platform '{}'. Allowed: pc, console", platform).into());
    }

    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_str(auth)?);
    headers.insert(USER_AGENT, HeaderValue::from_static("dc_bot/0.0.1 (+https://github.com)"));

    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()?;
    
    let rate_active = store_matches >= 2;
    let rate_lock = Arc::new(tokio::sync::Mutex::new(()));

    let matches_url = format!(
        "https://api.henrikdev.xyz/valorant/v4/matches/{}/{}/{}/{}",
        region, platform, urlencoding::encode(name), urlencoding::encode(tag)
    );
    
    let res = {
        let req = client
            .get(&matches_url)
            .query(&[("mode", "custom"), ("size", &query_size.to_string()), ("start", &start.to_string())]);
        if rate_active {
            let _g = rate_lock.lock().await;
            let resp = req.send().await?;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            resp
        } else {
            req.send().await?
        }
    };

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        return Err(format!("matches request failed: {} - {}", status, text).into());
    }

    let body_text = res.text().await?;
    let body: Value = serde_json::from_str(&body_text)?;

    let data_arr = body.get("data").and_then(|v| v.as_array()).ok_or_else(|| {
        "unexpected response: 'data' is not an array".to_string()
    })?;
    
    let mut match_ids: Vec<String> = Vec::new();
    for item in data_arr {
        let players_len = item
            .get("players")
            .and_then(|v| v.as_array())
            .map(|arr| arr.len())
            .or_else(|| {
                item.get("players")
                    .and_then(|pl| pl.get("all"))
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.len())
            })
            .unwrap_or(0);

        let match_id = item
            .get("metadata")
            .and_then(|m| m.get("match_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| item.get("match_id").and_then(|v| v.as_str()).map(|s| s.to_string()));

        let item_mode_type = item
            .get("metadata").and_then(|m| m.get("queue")).and_then(|q| q.get("mode_type")).and_then(|v| v.as_str())
            .or_else(|| item.get("queue").and_then(|q| q.get("mode_type")).and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_lowercase();
        let is_deathmatch = mode_type.eq_ignore_ascii_case("deathmatch");
        let allow = if is_deathmatch {
            item_mode_type == "deathmatch"
        } else {
            players_len == 10 && item_mode_type != "deathmatch"
        };

        if allow {
            if let Some(mid) = match_id {
                match_ids.push(mid);
                if match_ids.len() as u8 >= store_matches { break; }
            }
        }
    }

    if match_ids.is_empty() { return Ok(None); }

    let mut first_return: Option<Value> = None;
    for mid in match_ids {
        let local_path = format!("src\\data\\matches\\{}\\{}\\{}\\custom\\{}\\{}.json", guild_id, platform, region, mode_type, mid);
        if tokio::fs::try_exists(&local_path).await.unwrap_or(false) {
            if let Ok(bytes) = tokio::fs::read(&local_path).await {
                if let Ok(local_json) = serde_json::from_slice::<Value>(&bytes) {
                    let is_incomplete = local_json.get("started_at").and_then(|v| v.as_str()).map(|s| s.is_empty()).unwrap_or(true)
                        || local_json.get("game_length_in_ms").and_then(|v| v.as_i64()).unwrap_or(0) == 0;
                    if !is_incomplete {
                        if skip_stored { continue; }
                        if first_return.is_none() {
                            if let Value::Object(mut map) = local_json {
                                map.insert("__source".to_string(), Value::String("cache".to_string()));
                                first_return = Some(Value::Object(map));
                            } else {
                                first_return = Some(local_json);
                            }
                        }
                        continue;
                    }
                }
            }
        }

        let match_url = format!(
            "https://api.henrikdev.xyz/valorant/v4/match/{}/{}",
            region, mid
        );
        let res2 = {
            let req = client.get(&match_url);
            if rate_active {
                let _g = rate_lock.lock().await;
                let resp = req.send().await?;
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                resp
            } else {
                req.send().await?
            }
        };
        if !res2.status().is_success() {
            let status = res2.status();
            let text = res2.text().await.unwrap_or_default();
            return Err(format!("match request failed: {} - {}", status, text).into());
        }
        let body_text2 = res2.text().await?;
        let mut match_body: Value = serde_json::from_str(&body_text2)?;

        let mut idx_puuids: Vec<(usize, String)> = Vec::new();
        if let Some(players) = match_body
            .get("data")
            .and_then(|d| d.get("players"))
            .and_then(|v| v.as_array())
        {
            for (idx, p) in players.iter().enumerate() {
                if let Some(puuid) = p.get("puuid").and_then(|v| v.as_str()) {
                    idx_puuids.push((idx, puuid.to_string()));
                }
            }
        } else if let Some(players) = match_body
            .get("data")
            .and_then(|d| d.get("players"))
            .and_then(|v| v.get("all"))
            .and_then(|v| v.as_array())
        {
            for (idx, p) in players.iter().enumerate() {
                if let Some(puuid) = p.get("puuid").and_then(|v| v.as_str()) {
                    idx_puuids.push((idx, puuid.to_string()));
                }
            }
        }

        if !idx_puuids.is_empty() {
            let sem = Arc::new(Semaphore::new(5));
            let mut handles = Vec::with_capacity(idx_puuids.len());
            for (idx, puuid) in idx_puuids {
                let client_cl = client.clone();
                let reg = region.to_string();
                let plat = platform.to_string();
                let sem_cl = sem.clone();
                let rate_lock_cl = rate_lock.clone();
                let rate_active_cl = rate_active;
                handles.push(tokio::spawn(async move {
                    let _permit = sem_cl.acquire_owned().await.ok();
                    let url = format!(
                        "https://api.henrikdev.xyz/valorant/v3/by-puuid/mmr/{}/{}/{}",
                        reg, plat, puuid
                    );
                    let res = {
                        let req = client_cl.get(&url);
                        if rate_active_cl {
                            let _g = rate_lock_cl.lock().await;
                            let resp = req.send().await;
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            resp
                        } else {
                            req.send().await
                        }
                    };
                    let mut rank: Option<String> = None;
                    if let Ok(resp) = res {
                        if resp.status().is_success() {
                            if let Ok(text) = resp.text().await {
                                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                                    rank = v
                                        .get("data")
                                        .and_then(|d| d.get("current"))
                                        .and_then(|c| c.get("tier"))
                                        .and_then(|t| t.get("name"))
                                        .and_then(|n| n.as_str())
                                        .map(|s| s.to_string())
                                        .or_else(|| {
                                            v.get("data")
                                                .and_then(|d| d.get("tier"))
                                                .and_then(|t| t.get("name"))
                                                .and_then(|n| n.as_str())
                                                .map(|s| s.to_string())
                                        })
                                        .or_else(|| {
                                            v.get("data")
                                                .and_then(|d| d.get("peak"))
                                                .and_then(|p| p.get("tier"))
                                                .and_then(|t| t.get("name"))
                                                .and_then(|n| n.as_str())
                                                .map(|s| s.to_string())
                                        });
                                }
                            }
                        }
                    }
                    (idx, rank.unwrap_or_else(|| "Unrated".to_string()))
                }));
            }

            let mut results: Vec<(usize, String)> = Vec::new();
            for h in handles {
                if let Ok(res) = h.await { results.push(res); }
            }
            if let Some(players_arr) = match_body.get_mut("data").and_then(|d| d.get_mut("players")).and_then(|v| v.as_array_mut()) {
                for (idx, rank) in results {
                    if let Some(Value::Object(po)) = players_arr.get_mut(idx) {
                        po.insert("rank".to_string(), Value::String(rank));
                    }
                }
            } else if let Some(players_arr) = match_body
                .get_mut("data")
                .and_then(|d| d.get_mut("players"))
                .and_then(|v| v.get_mut("all"))
                .and_then(|v| v.as_array_mut())
            {
                for (idx, rank) in results {
                    if let Some(Value::Object(po)) = players_arr.get_mut(idx) {
                        po.insert("rank".to_string(), Value::String(rank));
                    }
                }
            }
        }

        if let Some(md) = crate::data::matches::match_data::from_match_json(&match_body) {
            let base_dir = std::path::PathBuf::from(format!("src\\data\\matches\\{}\\{}\\{}\\custom\\{}", guild_id, platform, region, mode_type));
            let _ = crate::data::matches::save_match_to_disk(&base_dir, &md).await;
        }
        if first_return.is_none() { first_return = Some(match_body); }
    }

    Ok(first_return)
}

#[poise::command(slash_command, prefix_command)]
pub async fn custom_match(
    ctx: poise::Context<'_, crate::Data, crate::Error>,
    #[description = "Region (eu, na, latam, br, ap, kr)"] region: String,
    #[description = "Platform (pc, console)"] platform: String,
    #[description = "Riot name (without #)"] name: String,
    #[description = "Riot tag (without #)"] tag: String,
    #[description = "Mode type (Standard or Deathmatch). Defaults to Standard"] mode_type: Option<String>,
    #[description = "Pagination start (default 0)"] start: Option<u32>,
    #[description = "Skip already stored match ids (useful when paging)"] skip_stored: Option<bool>,
    #[description = "Matches query size (1-10, default 10)"] query_size: Option<u8>,
    #[description = "Store multiple matches (1-10, default 1). When 2+, requests are rate-limited to 30/min"] store_matches: Option<u8>,
) -> Result<(), crate::Error> {
    let _ = dotenvy::dotenv();
    let token = match std::env::var("API_TOKEN") {
        Ok(v) => v,
        Err(_) => {
            ctx.say("API token not configured. Please set API_TOKEN in .env").await?;
            return Ok(());
        }
    };
    
    ctx.defer().await?;

    let start = start.unwrap_or(0);
    let guild_id_str = ctx.guild_id().map(|g| g.get().to_string()).unwrap_or_else(|| "dm".to_string());
    let skip = skip_stored.unwrap_or(false);
    let qs: u8 = query_size.unwrap_or(10).clamp(1, 10);
    let sm: u8 = store_matches.unwrap_or(1).clamp(1, 10);

    let region_lc = region.trim().to_lowercase();
    let platform_lc = platform.trim().to_lowercase();
    let mode_type_input = mode_type.unwrap_or_else(|| "Standard".to_string());
    let mode_type_dir = if mode_type_input.eq_ignore_ascii_case("deathmatch") { "deathmatch".to_string() } else { "standard".to_string() };

    match fetch_custom_match_data(&token, &region_lc, &platform_lc, &name, &tag, &guild_id_str, &mode_type_dir, start, skip, qs, sm).await {
        Ok(Some(json)) => {
            use poise::serenity_prelude as serenity;
            if let Some(md) = crate::data::matches::match_data::from_match_json(&json) {
                let base_dir = std::path::PathBuf::from(format!(
                    "src\\data\\matches\\{}\\{}\\{}\\custom\\{}",
                    guild_id_str,
                    platform_lc,
                    region_lc,
                    mode_type_dir
                ));
                let _ = crate::data::matches::save_match_to_disk(&base_dir, &md).await;

                let red_sum = md.teams.red.rounds_won + md.teams.red.rounds_lost;
                let blue_sum = md.teams.blue.rounds_won + md.teams.blue.rounds_lost;
                let total_rounds = std::cmp::max(1, std::cmp::max(red_sum, blue_sum));
                let winner = if md.teams.red.has_won { "RED" } else if md.teams.blue.has_won { "BLUE" } else { "TIE" };
                let color = if md.teams.red.has_won { 0xFF0000 } else if md.teams.blue.has_won { 0x3B82F6 } else { 0x808080 };
                let mut players = md.players.clone();
                for (i, p) in players.iter().enumerate() {

                desc.push_str("\n\n**Scoreboard**");

    // Step 1: Resolve puuid
    let puuid = match resolve_puuid(&name, &tag).await {
        Ok(p) => p,
        Err(e) => {
            ctx.say(format!("Failed to resolve puuid: {}", e)).await?;
            return Ok(());
        }
    };
                    let rank_emoji = p.rank.as_ref().and_then(|r| get_rank_emoji(r)).unwrap_or_default();
    // Step 2: Fetch and cache all matches for puuid
    let matches = match fetch_and_cache_matches_by_puuid(&region_lc, &platform_lc, &puuid).await {
        Ok(m) if !m.is_empty() => m,
        Ok(_) => {
            ctx.say("No matches found for this player.").await?;
            return Ok(());
        },
        Err(e) => {
            ctx.say(format!("Failed to fetch matches: {}", e)).await?;
            return Ok(());
        }
    };

    // Step 3: Display summary for the first match
    use poise::serenity_prelude as serenity;
    let md = &matches[0];
                        format!("**{}** 🏆 MVP", name_tag)
    let red_sum = md.teams.red.rounds_won + md.teams.red.rounds_lost;
    let blue_sum = md.teams.blue.rounds_won + md.teams.blue.rounds_lost;
    let total_rounds = std::cmp::max(1, std::cmp::max(red_sum, blue_sum));
    let winner = if md.teams.red.has_won { "RED" } else if md.teams.blue.has_won { "BLUE" } else { "TIE" };
    let color = if md.teams.red.has_won { 0xFF0000 } else if md.teams.blue.has_won { 0x3B82F6 } else { 0x808080 };
    let score_str = format!("{}:{}", md.teams.blue.rounds_won.max(0), md.teams.red.rounds_won.max(0));
                        format!("#{} {} {} {}", i + 1, team_sq, icon_parts, display_name)
    let mut players = md.players.clone();
    players.sort_by(|a, b| {
        let acs_a = a.stats.score as f32 / total_rounds as f32;
        let acs_b = b.stats.score as f32 / total_rounds as f32;
        acs_b.partial_cmp(&acs_a).unwrap_or(std::cmp::Ordering::Equal)
    });
                    let stats_line = format!(
    let mut desc = String::new();
    desc.push_str("\n\n**Scoreboard**");
    for (i, p) in players.iter().enumerate() {
        let team_sq = if p.team_id.eq_ignore_ascii_case("Blue") { "🟦" } else { "🟥" };
        let rank_emoji = p.rank.as_ref().and_then(|r| get_rank_emoji(r)).unwrap_or_default();
        let agent_emoji = get_agent_emoji(&p.agent.name).unwrap_or_default();
        let mut icon_parts: Vec<String> = Vec::new();
        if !rank_emoji.is_empty() { icon_parts.push(rank_emoji); }
        if !agent_emoji.is_empty() { icon_parts.push(agent_emoji); }
        let icon_parts = icon_parts.join(" ");
                let formatted_date = match chrono::DateTime::parse_from_rfc3339(&md.started_at) {
        let name_tag = format!("{}#{}", p.name, p.tag);
        let name_line = if icon_parts.is_empty() {
            format!("#{} {} {}", i + 1, team_sq, name_tag)
        } else {
            format!("#{} {} {} {}", i + 1, team_sq, icon_parts, name_tag)
        };
            }
        let total_shots = (p.stats.headshots + p.stats.bodyshots + p.stats.legshots).max(1) as f32;
        let hs_pct = (p.stats.headshots as f32 / total_shots) * 100.0;
        let kd_ratio = if p.stats.deaths == 0 { p.stats.kills as f32 } else { p.stats.kills as f32 / p.stats.deaths as f32 };
        let acs = (p.stats.score as f32 / total_rounds as f32).round() as i32;
        let stats_line = format!(
            "`{}ACS | {}/{}/{} | {:.2}K/D | {:.1}%HS`",
            acs,
            p.stats.kills,
            p.stats.deaths,
            p.stats.assists,
            kd_ratio,
            hs_pct
        );

        if !desc.is_empty() { desc.push('\n'); }
        desc.push_str(&name_line);
        desc.push('\n');
        desc.push_str(&stats_line);
    }

    let total_ms = md.game_length_in_ms.max(0) as i64;
    let total_secs = total_ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    let time_str = format!("{}m, {}s", mins, secs);

    let formatted_date = match chrono::DateTime::parse_from_rfc3339(&md.started_at) {
        Ok(dt) => format!("<t:{}:R>", dt.timestamp()),
        Err(_) => md.started_at.clone(),
    };
    let footer_line = format!("{} | Match Length - {}", formatted_date, time_str);

    let title = format!("**{} {} // {} WON**", md.metadata.map.name, score_str, winner);

    let embed = serenity::CreateEmbed::default()
        .title(title)
        .description(format!("{}\n\n{}", desc, footer_line))
        .color(color);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}
