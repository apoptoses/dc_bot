// for manual hosting, map picker/voter/randomizer
#[poise::command(slash_command,prefix_command)]
pub async fn host(
    ctx: poise::Context<'_, create::Data, crate::Error>
    #[description = ""] ,
) -> Result<(), crate::Error> {
    let Some(guild_id) = ctx.guild_id() else {
        ctx.say("This command can only be used in a server.").await?;
        return Ok(());
    };

}