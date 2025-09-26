use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use serde_json::Value;
use std::sync::{Arc, OnceLock};
use tokio::sync::Semaphore;
use crate::commands::valorant::stats::extract_win_for_player;
use crate::data::matches::store::{MatchStore, Scope};

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

    // Open scoped sled store and try cache-first latest match by Riot ID
    let mode_type_dir = if mode_type.eq_ignore_ascii_case("deathmatch") { "deathmatch" } else { "standard" };
    let store = MatchStore::open(Scope {
        guild_id,
        platform,
        region,
        mode: "custom",
        mode_type: Some(mode_type_dir),
    })?;
    let riot_key = format!("{}#{}", name, tag);
    if let Some(local) = store.get_latest_for_player(&riot_key)? {
        return Ok(Some(local));
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
        // On API failure (e.g., 429), fallback to local store before returning an error
        if let Some(local) = store.get_latest_for_player(&riot_key)? { return Ok(Some(local)); }
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

    // If no IDs returned by API, try local store for latest
    if match_ids.is_empty() {
        if let Some(local) = store.get_latest_for_player(&riot_key)? {
            return Ok(Some(local));
        }
    }

    let mut first_return: Option<Value> = None;

    for mid in match_ids.iter() {
        let mut match_body: Value;
        {
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
            match_body = serde_json::from_str(&body_text2)?;
        }

        let mut idx_puuids: Vec<(usize, String, &'static str)> = Vec::new();
        if let Some(players) = match_body
            .get("data")
            .and_then(|d| d.get("players"))
            .and_then(|v| v.as_array())
        {
            for (idx, p) in players.iter().enumerate() {
                if let Some(puuid) = p.get("puuid").and_then(|v| v.as_str()) {
                    idx_puuids.push((idx, puuid.to_string(), "flat"));
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
                    idx_puuids.push((idx, puuid.to_string(), "all"));
                }
            }
        } else if let Some(obj) = match_body
            .get("data")
            .and_then(|d| d.get("players"))
            .and_then(|v| v.as_object())
        {
            if let Some(arr) = obj.get("red").and_then(|v| v.as_array()) {
                for (idx, p) in arr.iter().enumerate() {
                    if let Some(puuid) = p.get("puuid").and_then(|v| v.as_str()) {
                        idx_puuids.push((idx, puuid.to_string(), "red"));
                    }
                }
            }
            if let Some(arr) = obj.get("blue").and_then(|v| v.as_array()) {
                for (idx, p) in arr.iter().enumerate() {
                    if let Some(puuid) = p.get("puuid").and_then(|v| v.as_str()) {
                        idx_puuids.push((idx, puuid.to_string(), "blue"));
                    }
                }
            }
        }

        if !idx_puuids.is_empty() {
            let sem = Arc::new(Semaphore::new(5));
            let mut handles = Vec::with_capacity(idx_puuids.len());
            for (idx, puuid, label) in idx_puuids.clone() {
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
                    (idx, rank.unwrap_or_else(|| "Unrated".to_string()), label)
                }));
            }

            let mut results_with_label: Vec<(usize, String, &'static str)> = Vec::new();
            for h in handles {
                if let Ok(res) = h.await { results_with_label.push(res); }
            }

            // Write back ranks into the appropriate structure
            if let Some(players_arr) = match_body
                .get_mut("data").and_then(|d| d.get_mut("players")).and_then(|v| v.as_array_mut()) {
                for (idx, rank, label) in results_with_label.iter() {
                    if *label == "flat" {
                        if let Some(Value::Object(po)) = players_arr.get_mut(*idx) {
                            po.insert("rank".to_string(), Value::String(rank.clone()));
                        }
                    }
                }
            }
            if let Some(players_all) = match_body
                .get_mut("data").and_then(|d| d.get_mut("players")).and_then(|v| v.get_mut("all")).and_then(|v| v.as_array_mut()) {
                for (idx, rank, label) in results_with_label.iter() {
                    if *label == "all" {
                        if let Some(Value::Object(po)) = players_all.get_mut(*idx) {
                            po.insert("rank".to_string(), Value::String(rank.clone()));
                        }
                    }
                }
            }
            if let Some(players_obj) = match_body
                .get_mut("data").and_then(|d| d.get_mut("players")).and_then(|v| v.as_object_mut()) {
                if let Some(red_arr) = players_obj.get_mut("red").and_then(|v| v.as_array_mut()) {
                    for (idx, rank, label) in results_with_label.iter() {
                        if *label == "red" {
                            if let Some(Value::Object(po)) = red_arr.get_mut(*idx) {
                                po.insert("rank".to_string(), Value::String(rank.clone()));
                            }
                        }
                    }
                }
                if let Some(blue_arr) = players_obj.get_mut("blue").and_then(|v| v.as_array_mut()) {
                    for (idx, rank, label) in results_with_label.iter() {
                        if *label == "blue" {
                            if let Some(Value::Object(po)) = blue_arr.get_mut(*idx) {
                                po.insert("rank".to_string(), Value::String(rank.clone()));
                            }
                        }
                    }
                }
            }
        }
        // Persist the enriched match into the sled store
        store.upsert_match(&match_body)?;
        if first_return.is_none() { first_return = Some(match_body); }
    }

    Ok(first_return)
}

#[poise::command(slash_command, prefix_command)]
pub async fn custom_match(
    ctx: poise::Context<'_, crate::Data, crate::Error>,
    #[description = "Riot ID (e.g., Name#Tag)"] riot_id: String,
    #[description = "Region (eu, na, latam, br, ap, kr) Defaults to na"] region: Option<String>,
    #[description = "Platform (pc, console) Defaults to pc"] platform: Option<String>,
    #[description = "Mode type (Standard or Deathmatch). Defaults to Standard"] mode_type: Option<String>,
    #[description = "Pagination start (default 0)"] start: Option<u32>,
    #[description = "Matches query size (1-10, default 10)"] query_size: Option<u8>,
    #[description = "Store multiple matches (1-10, default 1). When 2+, requests are rate-limited to 30/min"] store_matches: Option<u8>,
    #[description = "Optional player filter (Name or Name#Tag) to display only that player's stats"] player_filter: Option<String>,
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
    let qs: u8 = query_size.unwrap_or(10).clamp(1, 10);
    let sm: u8 = store_matches.unwrap_or(1).clamp(1, 10);

    let region_lc = region.unwrap_or_else(|| "na".to_string()).trim().to_lowercase();
    let platform_lc = platform.unwrap_or_else(|| "pc".to_string()).trim().to_lowercase();
    let mode_type_input = mode_type.unwrap_or_else(|| "Standard".to_string());
    let mode_type_dir = if mode_type_input.eq_ignore_ascii_case("deathmatch") { "deathmatch".to_string() } else { "standard".to_string() };

    let (name, tag) = match riot_id.split_once('#') {
        Some((n, t)) => (n.trim().to_string(), t.trim().to_string()),
        None => { ctx.say("Please provide a valid Riot ID in the format Name#Tag.").await?; return Ok(()); }
    };

    match fetch_custom_match_data(&token, &region_lc, &platform_lc, &name, &tag, &guild_id_str, &mode_type_dir, start, qs, sm).await {
        Ok(Some(json)) => {
            use poise::serenity_prelude as serenity;

            let data = json.get("data").unwrap_or(&json);
            let map_name = data.get("metadata").and_then(|m| m.get("map")).and_then(|m| m.get("name")).and_then(|v| v.as_str()).unwrap_or("?");
            let (blue_won, red_won, blue_rw, red_rw) = if let Some(obj) = data.get("teams").and_then(|v| v.as_object()) {
                let red = obj.get("red").and_then(|v| v.as_object());
                let blue = obj.get("blue").and_then(|v| v.as_object());
                (
                    blue.and_then(|b| b.get("has_won")).and_then(|v| v.as_bool()).unwrap_or(false),
                    red.and_then(|r| r.get("has_won")).and_then(|v| v.as_bool()).unwrap_or(false),
                    blue.and_then(|b| b.get("rounds_won")).and_then(|v| v.as_i64()).unwrap_or(0),
                    red.and_then(|r| r.get("rounds_won")).and_then(|v| v.as_i64()).unwrap_or(0),
                )
            } else if let Some(arr) = data.get("teams").and_then(|v| v.as_array()) {
                let mut b_won = false; let mut r_won = false; let mut b_rw: i64 = 0; let mut r_rw: i64 = 0;
                for t in arr {
                    let team_id = t.get("team_id").and_then(|v| v.as_str()).unwrap_or("");
                    let won_b = t.get("won").and_then(|v| v.as_bool()).unwrap_or(false);
                    let rounds = t.get("rounds").and_then(|v| v.as_object());
                    let rw = rounds.and_then(|r| r.get("won")).and_then(|v| v.as_i64()).unwrap_or(0);
                    if team_id.eq_ignore_ascii_case("blue") { b_won = won_b; b_rw = rw; }
                    if team_id.eq_ignore_ascii_case("red") { r_won = won_b; r_rw = rw; }
                }
                (b_won, r_won, b_rw, r_rw)
            } else { (false, false, 0, 0) };
            let winner = if red_won { "RED" } else if blue_won { "BLUE" } else { "TIE" };
            let color = if red_won { 0xFF0000 } else if blue_won { 0x3B82F6 } else { 0x808080 };

            let total_ms = data.get("metadata").and_then(|m| m.get("game_length_in_ms")).and_then(|v| v.as_i64()).unwrap_or(0);
            let total_secs = total_ms / 1000;
            let mins = total_secs / 60;
            let secs = total_secs % 60;
            let time_str = format!("{}m, {}s", mins, secs);
            let started_at = data.get("metadata").and_then(|m| m.get("started_at")).and_then(|v| v.as_str()).unwrap_or("");
            let formatted_date = match chrono::DateTime::parse_from_rfc3339(started_at) {
                Ok(dt) => format!("<t:{}:R>", dt.timestamp()),
                Err(_) => started_at.to_string(),
            };
            let footer_line = format!("{} | Match Length - {}", formatted_date, time_str);
            let score_str = format!("{}:{}", blue_rw.max(0) as i64, red_rw.max(0) as i64);
            let title = format!("**{} {} // {} WON**", map_name, score_str, winner);
            
            let data_players: Option<Vec<Value>> =
                if let Some(arr) = data.get("players").and_then(|v| v.as_array()) {
                    Some(arr.clone())
                } else if let Some(arr) = data.get("players").and_then(|v| v.get("all")).and_then(|v| v.as_array()) {
                    Some(arr.clone())
                } else if let Some(obj) = data.get("players").and_then(|v| v.as_object()) {
                    let mut merged: Vec<Value> = Vec::new();
                    if let Some(arr) = obj.get("red").and_then(|v| v.as_array()) {
                        merged.extend(arr.iter().cloned());
                    }
                    if let Some(arr) = obj.get("blue").and_then(|v| v.as_array()) {
                        merged.extend(arr.iter().cloned());
                    }
                    if merged.is_empty() { None } else { Some(merged) }
                } else { None };

            // If a player filter was provided, try to show only that player's stats
            if let Some(filter) = player_filter {
                let f = filter.trim().to_lowercase();
                let mut found_player: Option<Value> = None;
                if let Some(ref players) = data_players {
                    for p in players.iter() {
                        let pname = p.get("name").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                        let ptag = p.get("tag").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                        let full = if ptag.is_empty() { pname.clone() } else { format!("{}#{}", pname, ptag) };
                        if full == f || pname == f { found_player = Some(p.clone()); break; }
                    }
                }

                if let Some(p) = found_player {
                    let pname = p.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                    let ptag = p.get("tag").and_then(|v| v.as_str()).unwrap_or("");
                    let agent_name = p.get("agent").and_then(|a| a.get("name")).and_then(|v| v.as_str()).unwrap_or("");
                    let stats = p.get("stats").unwrap_or(&Value::Null);
                    let score = stats.get("score").and_then(|v| v.as_i64()).unwrap_or(0);
                    let kills = stats.get("kills").and_then(|v| v.as_i64()).unwrap_or(0);
                    let deaths = stats.get("deaths").and_then(|v| v.as_i64()).unwrap_or(0);
                    let assists = stats.get("assists").and_then(|v| v.as_i64()).unwrap_or(0);
                    let hs = stats.get("headshots").and_then(|v| v.as_i64()).unwrap_or(0);
                    let body = stats.get("bodyshots").and_then(|v| v.as_i64()).unwrap_or(0);
                    let leg = stats.get("legshots").and_then(|v| v.as_i64()).unwrap_or(0);
                    let total_shots = (hs + body + leg).max(1) as f64;
                    let hs_pct = (hs as f64) * 100.0 / total_shots;
                    let kd = if deaths == 0 { kills as f64 } else { (kills as f64) / (deaths as f64) };
                    let team = p.get("team_id").and_then(|v| v.as_str()).unwrap_or("");
                    let won = extract_win_for_player(data, &p.get("puuid").and_then(|v| v.as_str()).unwrap_or(""));

                    let desc = format!(
                        "Player: {}#{}\nAgent: {}\nTeam: {}\nScore: {}\nKills: {}\nDeaths: {}\nAssists: {}\nK/D: {:.2}\nHS%: {:.1}%\nWon: {}",
                        pname,
                        ptag,
                        agent_name,
                        team,
                        score,
                        kills,
                        deaths,
                        assists,
                        kd,
                        hs_pct,
                        won.unwrap_or(false)
                    );

                    let embed = serenity::CreateEmbed::default()
                        .title(format!("Match player stats: {}#{}", pname, ptag))
                        .description(desc)
                        .color(color);

                    ctx.send(poise::CreateReply::default().embed(embed)).await?;
                    return Ok(());
                } else {
                    ctx.say(format!("Player '{}' not found in the match.", filter)).await?;
                    return Ok(());
                }
            }

            let (blue_rw_i, red_rw_i, blue_rl_i, red_rl_i) = if let Some(obj) = data.get("teams").and_then(|v| v.as_object()) {
                let red = obj.get("red").and_then(|v| v.as_object());
                let blue = obj.get("blue").and_then(|v| v.as_object());
                (
                    blue.and_then(|b| b.get("rounds_won")).and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                    red.and_then(|r| r.get("rounds_won")).and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                    blue.and_then(|b| b.get("rounds_lost")).and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                    red.and_then(|r| r.get("rounds_lost")).and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                )
            } else if let Some(arr) = data.get("teams").and_then(|v| v.as_array()) {
                let mut b_w = 0; let mut r_w = 0; let mut b_l = 0; let mut r_l = 0;
                for t in arr {
                    let team_id = t.get("team_id").and_then(|v| v.as_str()).unwrap_or("");
                    let rounds = t.get("rounds").and_then(|v| v.as_object());
                    let won = rounds.and_then(|r| r.get("won")).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let lost = rounds.and_then(|r| r.get("lost")).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    if team_id.eq_ignore_ascii_case("blue") { b_w = won; b_l = lost; }
                    if team_id.eq_ignore_ascii_case("red") { r_w = won; r_l = lost; }
                }
                (b_w, r_w, b_l, r_l)
            } else { (0, 0, 0, 0) };
            let total_rounds = std::cmp::max(1, std::cmp::max(blue_rw_i + blue_rl_i, red_rw_i + red_rl_i));

            let mut lines: Vec<(f32, (String, String))> = Vec::new();
            if let Some(arr) = data_players {
                for p in arr.iter() {
                    let team_id = p.get("team_id").and_then(|v| v.as_str()).unwrap_or("");
                    let team_sq = if team_id.eq_ignore_ascii_case("blue") { "🟦" } else { "🟥" };
                    let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                    let tag = p.get("tag").and_then(|v| v.as_str()).unwrap_or("");
                    let agent_name = p.get("agent").and_then(|a| a.get("name")).and_then(|v| v.as_str()).unwrap_or("");
                    let stats = p.get("stats").unwrap_or(&Value::Null);
                    let score = stats.get("score").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let kills = stats.get("kills").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let deaths = stats.get("deaths").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let assists = stats.get("assists").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let hs = stats.get("headshots").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let body = stats.get("bodyshots").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let leg = stats.get("legshots").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let total_shots = (hs + body + leg).max(1) as f32;
                    let hs_pct = (hs as f32 / total_shots) * 100.0;
                    let kd_ratio = if deaths == 0 { kills as f32 } else { kills as f32 / deaths as f32 };
                    let acs = (score as f32 / total_rounds as f32).round();
                    let rank_text = p.get("rank").and_then(|v| v.as_str()).unwrap_or("");
                    let rank_emoji = if rank_text.is_empty() { None } else { get_rank_emoji(rank_text) };
                    let agent_emoji = get_agent_emoji(agent_name);
                    let mut icon_parts: Vec<String> = Vec::new();
                    if let Some(e) = rank_emoji { if !e.is_empty() { icon_parts.push(e); } }
                    if let Some(e) = agent_emoji { if !e.is_empty() { icon_parts.push(e); } }
                    let icons = if icon_parts.is_empty() { String::new() } else { format!("{} ", icon_parts.join(" ")) };
                    let name_body = format!("{} {}{}#{}", team_sq, icons, name, tag);
                    let stats_line = format!("`{}ACS | {}/{}/{} | {:.2}K/D | {:.1}%HS`", acs as i32, kills, deaths, assists, kd_ratio, hs_pct);
                    lines.push((acs as f32, (name_body, stats_line)));
                }
            }
            lines.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            let mut desc = String::new();
            if !lines.is_empty() {
                desc.push_str("\n\n**Scoreboard**");
                for (idx, (_, (name_body, stats_line))) in lines.into_iter().enumerate() {
                    let name_line = format!("\n#{} {}", idx + 1, name_body);
                    let entry = format!("{}\n{}", name_line, stats_line);
                    desc.push_str(&entry);
                }
            }
            if !desc.is_empty() { desc.push_str("\n\n"); }
            desc.push_str(&footer_line);

            let embed = serenity::CreateEmbed::default()
                .title(title)
                .description(desc)
                .color(color);
            ctx.send(poise::CreateReply::default().embed(embed)).await?;
            Ok(())
        }
        Ok(None) => {
            ctx.say("No matches found.").await?;
            Ok(())
        }
        Err(e) => {
            ctx.say(format!("Failed to fetch match: {}", e)).await?;
            Ok(())
        }
    }
}
