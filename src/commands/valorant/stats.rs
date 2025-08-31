use crate::data::matches::match_data::MatchData;
use crate::commands::valorant::custom::custom_match::{get_match_cache, resolve_puuid};

#[poise::command(slash_command, prefix_command)]
pub async fn stats(
    ctx: poise::Context<'_, crate::Data, crate::Error>,
    #[description = "Riot ID (e.g., Player#EUW1)"] riot_id: String,
) -> Result<(), crate::Error> {
    ctx.defer().await?;
    let (name, tag) = match riot_id.split_once('#') {
        Some((n, t)) => (n, t),
        None => {
            ctx.say("Please provide a valid Riot ID in the format Name#Tag.").await?;
            return Ok(());
        }
    };
    let puuid = match resolve_puuid(name, tag).await {
        Ok(p) => p,
        Err(e) => {
            ctx.say(format!("Failed to resolve puuid: {}", e)).await?;
            return Ok(());
        }
    };
    let cache = get_match_cache();
    let map = cache.read().await;
    let mut found = Vec::new();
    for (match_id, md) in map.iter() {
        for player in &md.players {
            if player.puuid == puuid {
                found.push((match_id.clone(), player.clone(), md));
            }
        }
    }
    if found.is_empty() {
        ctx.say("No cached matches found for this player.").await?;
        return Ok(());
    }
    use poise::serenity_prelude as serenity;
    let mut desc = String::new();
    for (match_id, player, md) in found.iter() {
        desc.push_str(&format!(
            "Match `{}` on **{}**: Agent: **{}**, Team: **{}**, K/D/A: **{}/{}/{}**, Score: **{}**, HS: **{}**, Body: **{}**, Leg: **{}**\n",
            match_id,
            md.metadata.map.name,
            player.agent.name,
            player.team_id,
            player.stats.kills,
            player.stats.deaths,
            player.stats.assists,
            player.stats.score,
            player.stats.headshots,
            player.stats.bodyshots,
