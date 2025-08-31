
#[poise::command(slash_command, prefix_command)]
pub async fn follow(
    ctx: poise::Context<'_, crate::Data, crate::Error>,
    #[description = "Twitch channel name or URL"] twitch_channel: String,
    #[description = "Destination Discord channel (e.g., #channel or ID)"] notify_channel: String,
) -> Result<(), crate::Error> {
    ctx
        .say(format!(
            "[stub] Following Twitch '{}' with notifications to '{}'.",
            twitch_channel, notify_channel
        ))
        .await?;
    Ok(())
}

#[poise::command(slash_command, prefix_command)]
pub async fn unfollow(
    ctx: poise::Context<'_, crate::Data, crate::Error>,
    #[description = "Twitch channel name or URL"] twitch_channel: String,
) -> Result<(), crate::Error> {
    ctx
        .say(format!(
            "[stub] Unfollowed Twitch channel '{}'.",
            twitch_channel
        ))
        .await?;
    Ok(())
}
