use crate::data::matches::store::{MatchStore, Scope};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};

async fn resolve_puuid(name: &str, tag: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let token = std::env::var("API_TOKEN").ok();
    let mut headers = HeaderMap::new();
    if let Some(t) = token { headers.insert(AUTHORIZATION, HeaderValue::from_str(&t)?); }
    headers.insert(USER_AGENT, HeaderValue::from_static("dc_bot/0.0.1 (+https://github.com)"));
    let client = reqwest::Client::builder().default_headers(headers).build()?;
    let url = format!(
        "https://api.henrikdev.xyz/valorant/v2/account/{}/{}",
        urlencoding::encode(name), urlencoding::encode(tag)
    );
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("resolve_puuid failed: {} - {}", status, text).into());
    }
    let txt = resp.text().await?;
    let v: serde_json::Value = serde_json::from_str(&txt)?;
    let puuid = v.get("data").and_then(|d| d.get("puuid")).and_then(|s| s.as_str()).ok_or("missing puuid in response")?;
    Ok(puuid.to_string())
}


fn extract_player_entry<'a>(root: &'a serde_json::Value, puuid: &str) -> Option<&'a serde_json::Value> {
    let data = root.get("data").unwrap_or(root);

    if let Some(arr) = data.get("players").and_then(|v| v.as_array()) {
        for p in arr { if p.get("puuid").and_then(|x| x.as_str()) == Some(puuid) { return Some(p); } }
    }

    if let Some(arr) = data.get("players").and_then(|v| v.get("all")).and_then(|v| v.as_array()) {
        for p in arr { if p.get("puuid").and_then(|x| x.as_str()) == Some(puuid) { return Some(p); } }
    }

    if let Some(obj) = data.get("players").and_then(|v| v.as_object()) {
        for team_key in ["red", "blue"] {
            if let Some(arr) = obj.get(team_key).and_then(|v| v.as_array()) {
                for p in arr {
                    if p.get("puuid").and_then(|x| x.as_str()) == Some(puuid) { return Some(p); }
                }
            }
        }
    }

    None
}

fn extract_player_team_id(root: &serde_json::Value, puuid: &str) -> Option<String> {
    extract_player_entry(root, puuid)
        .and_then(|p| p.get("team_id").and_then(|v| v.as_str()).map(|s| s.to_string()))
}

fn extract_match_length_ms(root: &serde_json::Value) -> i64 {
    let data = root.get("data").unwrap_or(root);
    data.get("metadata").and_then(|m| m.get("game_length_in_ms")).and_then(|v| v.as_i64())
        .or_else(|| data.get("game_length_in_ms").and_then(|v| v.as_i64()))
        .unwrap_or(0)
}

fn extract_player_stats(root: &serde_json::Value, puuid: &str) -> (i64, i64, i64, i64, i64) {
    if let Some(p) = extract_player_entry(root, puuid) {
        let stats = p.get("stats").unwrap_or(&serde_json::Value::Null);
        let kills = stats.get("kills").and_then(|v| v.as_i64()).unwrap_or(0);
        let deaths = stats.get("deaths").and_then(|v| v.as_i64()).unwrap_or(0);
        let hs = stats.get("headshots").and_then(|v| v.as_i64()).unwrap_or(0);
        let body = stats.get("bodyshots").and_then(|v| v.as_i64()).unwrap_or(0);
        let leg = stats.get("legshots").and_then(|v| v.as_i64()).unwrap_or(0);
        return (kills, deaths, hs, body, leg);
    }
    (0, 0, 0, 0, 0)
}

pub(crate) fn extract_win_for_player(root: &serde_json::Value, puuid: &str) -> Option<bool> {
    let team_id = extract_player_team_id(root, puuid)?;
    let data = root.get("data").unwrap_or(root);
    if let Some(arr) = data.get("teams").and_then(|v| v.as_array()) {
        for t in arr {
            if t.get("team_id").and_then(|v| v.as_str()).map(|s| s.eq_ignore_ascii_case(&team_id)).unwrap_or(false) {
                return t.get("won").and_then(|v| v.as_bool());
            }
        }
        None
    } else if let Some(obj) = data.get("teams").and_then(|v| v.as_object()) {
        if team_id.eq_ignore_ascii_case("red") { obj.get("red").and_then(|r| r.get("has_won")).and_then(|v| v.as_bool()) }
        else if team_id.eq_ignore_ascii_case("blue") { obj.get("blue").and_then(|b| b.get("has_won")).and_then(|v| v.as_bool()) }
        else { None }
    } else { None }
}

#[poise::command(slash_command, prefix_command)]
pub async fn stats(
    ctx: poise::Context<'_, crate::Data, crate::Error>,
    #[description = "Riot ID (e.g., Player#Tag)"] riot_id: String,
    #[description = "Mode (Custom, Competitive, Deathmatch, or TDM). Defaults to custom"] mode: Option<String>,
    #[description = "Region (eu, na, latam, br, ap, kr) Defaults to na"] region: Option<String>,
    #[description = "Platform (pc, console) Defaults to pc"] platform: Option<String>,
    #[description = "Mode type (Standard or Deathmatch). Defaults to Standard"] mode_type: Option<String>,
) -> Result<(), crate::Error> {
    ctx.defer().await?;
    let guild_id = match ctx.guild_id() {
        Some(g) => g.get().to_string(),
        None => { ctx.say("This command must be used in a server.").await?; return Ok(()); }
    };
    let (name, tag) = match riot_id.split_once('#') {
        Some((n, t)) => (n.trim(), t.trim()),
        None => { ctx.say("Please provide a valid Riot ID in the format Name#Tag.").await?; return Ok(()); }
    };

    let mode_lc = mode.unwrap_or_else(|| "custom".to_string()).trim().to_lowercase();
    let region_lc = region.unwrap_or_else(|| "na".to_string()).trim().to_lowercase();
    let platform_lc = platform.unwrap_or_else(|| "pc".to_string()).trim().to_lowercase();
    let mode_type_dir = match mode_type.unwrap_or_else(|| "Standard".to_string()).to_lowercase().as_str() {
        "deathmatch" => "deathmatch".to_string(),
        _ => "standard".to_string(),
    };

    // Open the sled-backed store for this scope
    let scope = Scope {
        guild_id: &guild_id,
        platform: &platform_lc,
        region: &region_lc,
        mode: &mode_lc,
        mode_type: if mode_lc == "custom" { Some(mode_type_dir.as_str()) } else { None },
    };
    let store = MatchStore::open(scope).map_err(|e| format!("failed to open local store: {}", e))?;

    // Resolve PUUID from local store first; fallback to network resolver
    let riot_key = format!("{}#{}", name.to_lowercase(), tag.to_lowercase());
    let puuid = if let Some(p) = store.get_puuid_for_riot(&riot_key).map_err(|e| format!("store error: {}", e))? {
        p
    } else {
        match resolve_puuid(name, tag).await {
            Ok(p) => p,
            Err(e) => { ctx.say(format!("Failed to resolve puuid: {}", e)).await?; return Ok(()); }
        }
    };

    // Retrieve a page of matches from the store for this player
    let match_values: Vec<serde_json::Value> = store
        .get_page_by_puuid(&puuid, 0, 1000)
        .map_err(|e| format!("store error: {}", e))?;

    if match_values.is_empty() { ctx.say("No cached matches found for this player.").await?; return Ok(()); }

    let mut total_ms: i64 = 0;
    let mut total_kills: i64 = 0;
    let mut total_deaths: i64 = 0;
    let mut total_hs: i64 = 0;
    let mut total_body: i64 = 0;
    let mut total_leg: i64 = 0;
    let mut wins: i64 = 0;
    let mut considered_matches: i64 = 0;

    for val in match_values {
        // normalize: if loaded match is a wrapper with 'data' being the match, unwrap
        let candidates: Vec<serde_json::Value> = if val.get("data").and_then(|v| v.as_array()).is_some() {
            // If val.data is an array, it's the aggregate file — but we should have handled that earlier. Still, handle defensively.
            val.get("data").and_then(|v| v.as_array()).unwrap().clone()
        } else if val.get("data").is_some() && val.get("data").unwrap().is_object() {
            vec![val.get("data").unwrap().clone()]
        } else {
            vec![val]
        };

        for m in candidates {
            if extract_player_entry(&m, &puuid).is_none() { continue; }

            considered_matches += 1;
            total_ms += extract_match_length_ms(&m);
            let (k, d, hs, body, leg) = extract_player_stats(&m, &puuid);
            total_kills += k;
            total_deaths += d;
            total_hs += hs;
            total_body += body;
            total_leg += leg;
            if extract_win_for_player(&m, &puuid).unwrap_or(false) { wins += 1; }
        }
    }

    let hours = (total_ms as f64) / 3_600_000.0;
    let kd = if total_deaths == 0 { total_kills as f64 } else { (total_kills as f64) / (total_deaths as f64) };
    let total_shots = (total_hs + total_body + total_leg).max(1) as f64;
    let hs_pct = (total_hs as f64) * 100.0 / total_shots;
    let winrate = if considered_matches == 0 { 0.0 } else { (wins as f64) * 100.0 / (considered_matches as f64) };

    use poise::serenity_prelude as serenity;

    let mut embed = serenity::CreateEmbed::default();
    let footer_text = if mode_lc == "custom" {
        format!(
            "Mode: {} ({}) | Region: {} | Platform: {}",
            mode_lc, mode_type_dir, region_lc, platform_lc
        )
    } else {
        format!(
            "Mode: {} | Region: {} | Platform: {}",
            mode_lc, region_lc, platform_lc
        )
    };

    embed = embed
        .title(format!("Overall stats for {}#{} (cached)", name, tag))
        .color(0x3B82F6)
        .field("K/D", format!("{:.2}", kd), true)
        .field("HS%", format!("{:.1}%", hs_pct), true)
        .field("Winrate", format!("{:.1}%", winrate), true)
        .field("Total Kills", total_kills.to_string(), true)
    // Try to find linked Discord profile
        .field("Matches", considered_matches.to_string(), true)
        .field("Playtime", format!("{:.2}h", hours), true)
        .footer(serenity::CreateEmbedFooter::new(footer_text));

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}
