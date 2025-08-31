use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct YouTubeSubscription {
    pub youtube_channel: String,       // raw identifier provided by user
    pub youtube_key: String,           // normalized key for map lookups (lowercased/trimmed)
    pub notify_channel_id: u64,        // Discord channel ID to notify
    pub videos: bool,
    pub shorts: bool,
    pub streams: bool,
    pub podcasts: bool,
    pub playlists: bool,
    pub store: bool,
    pub posts: bool,
    pub releases: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct YoutubeSchema {
    // guild_id -> (youtube_key -> subscription)
    pub guilds: HashMap<u64, HashMap<String, YouTubeSubscription>>,
    #[serde(default)]
    // guild_id -> (youtube_key -> last notified item identifier (e.g., video link))
    pub last_notified: HashMap<u64, HashMap<String, String>>,
}

impl YoutubeSchema {
    pub fn upsert_subscription(&mut self, guild_id: u64, sub: YouTubeSubscription) {
        let g = self.guilds.entry(guild_id).or_default();
        g.insert(sub.youtube_key.clone(), sub);
    }

    pub fn remove_subscription(&mut self, guild_id: u64, youtube_key: &str) -> bool {
        if let Some(g) = self.guilds.get_mut(&guild_id) {
            return g.remove(youtube_key).is_some();
        }
        false
    }

    pub fn list_guild(&self, guild_id: u64) -> Option<&HashMap<String, YouTubeSubscription>> {
        self.guilds.get(&guild_id)
    }

    pub async fn load_from_disk(path: &Path) -> Result<YoutubeSchema, Box<dyn std::error::Error + Send + Sync>> {
        if !path.exists() {
            // Ensure parent directory exists
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await.ok();
            }
            return Ok(YoutubeSchema::default());
        }
        let data = tokio::fs::read(path).await?;
        if data.is_empty() {
            return Ok(YoutubeSchema::default());
        }
        let parsed: YoutubeSchema = serde_json::from_slice(&data)?;
        Ok(parsed)
    }

    pub async fn save_to_disk(&self, path: &Path) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        let json = serde_json::to_vec_pretty(self)?;
        let tmp_path: PathBuf = path.with_extension("json.tmp");
        tokio::fs::write(&tmp_path, &json).await?;
        match tokio::fs::rename(&tmp_path, path).await {
            Ok(()) => {}
            Err(_) => {
                tokio::fs::write(path, &json).await?;
                let _ = tokio::fs::remove_file(&tmp_path).await;
            }
        }
        Ok(())
    }
}

pub fn normalize_youtube_id(input: &str) -> String {
    input.trim().to_ascii_lowercase()
}
