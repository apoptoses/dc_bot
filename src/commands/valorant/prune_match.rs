#[poise::command(slash_command, prefix_command)]
pub async fn prune_match(
    ctx: poise::Context<'_, crate::Data, crate::Error>,
    #[description = "Platform (pc, console)"] platform: String,
    #[description = "Region (eu, na, latam, br, ap, kr)"] region: String,
    #[description = "Mode folder (e.g., custom). Defaults to custom"] mode: Option<String>,
    #[description = "Mode type (Standard or Deathmatch). Defaults to Standard"] mode_type: Option<String>,
) -> Result<(), crate::Error> {
    let Some(guild_id) = ctx.guild_id() else {
        ctx.say("This command can only be used in a server.").await?;
        return Ok(());
    };
    
    let platform_lc = platform.trim().to_lowercase();
    let region_lc = region.trim().to_lowercase();
    let mode_lc = mode.unwrap_or_else(|| "custom".to_string()).trim().to_lowercase();
    let mode_type_lc = match mode_type {
        Some(s) if s.eq_ignore_ascii_case("deathmatch") => "deathmatch".to_string(),
        _ => "standard".to_string(),
    };

    const PLATFORMS: &[&str] = &["pc", "console"];
    const REGIONS: &[&str] = &["eu", "na", "latam", "br", "ap", "kr"];

    if !PLATFORMS.contains(&platform_lc.as_str()) {
        ctx.say("Invalid platform. Allowed: pc, console").await?;
        return Ok(());
    }
    if !REGIONS.contains(&region_lc.as_str()) {
        ctx.say("Invalid region. Allowed: eu, na, latam, br, ap, kr").await?;
        return Ok(());
    }

    let base_dir = std::path::PathBuf::from(format!(
        "src\\data\\matches\\{}\\{}\\{}\\{}\\{}",
        guild_id.get(),
        platform_lc,
        region_lc,
        mode_lc,
        mode_type_lc
    ));

    if !tokio::fs::try_exists(&base_dir).await.unwrap_or(false) {
        ctx.say(format!(
            "No files found (directory doesn't exist): {}",
            base_dir.display()
        ))
        .await?;
        return Ok(());
    }

    let mut deleted = 0usize;
    let mut failed = 0usize;

    let mut dir = match tokio::fs::read_dir(&base_dir).await {
        Ok(d) => d,
        Err(e) => {
            ctx.say(format!("Failed to read directory: {}", e)).await?;
            return Ok(());
        }
    };

    while let Ok(Some(entry)) = dir.next_entry().await {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                if ext.eq_ignore_ascii_case("json") {
                    match tokio::fs::remove_file(&path).await {
                        Ok(_) => deleted += 1,
                        Err(_) => failed += 1,
                    }
                }
            }
        }
    }

    ctx.say(format!(
        "Deleted {} JSON file(s) under {}.{}",
        deleted,
        base_dir.display(),
        if failed > 0 { format!(" {} file(s) failed.", failed) } else { String::new() }
    ))
    .await?;

    Ok(())
}
