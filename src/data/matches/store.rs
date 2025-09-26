use serde_json::Value;
use std::path::PathBuf;

pub struct MatchStore {
    db: sled::Db,
    matches: sled::Tree,
    by_puuid: sled::Tree,
    riot_to_puuid: sled::Tree,
    latest_by_player: sled::Tree,
}

pub struct Scope<'a> {
    pub guild_id: &'a str,
    pub platform: &'a str,
    pub region: &'a str,
    pub mode: &'a str,            // e.g., "custom"
    pub mode_type: Option<&'a str>, // Some("standard"|"deathmatch") when mode == "custom"
}

impl MatchStore {
    pub fn open(scope: Scope<'_>) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Storage root strictly under matches\... per requirements
        let db_path = {
            let mut p = PathBuf::from("matches");
            p.push(scope.guild_id);
            p.push(scope.platform);
            p.push(scope.region);
            p.push(scope.mode);
            if let Some(mt) = scope.mode_type { p.push(mt); }
            p.push("db");
            p
        };

        let db = sled::open(&db_path)?;
        Ok(Self {
            matches: db.open_tree("matches")?,
            by_puuid: db.open_tree("by_puuid")?,
            riot_to_puuid: db.open_tree("riot_to_puuid")?,
            latest_by_player: db.open_tree("latest_by_player")?,
            db,
        })
    }

    pub fn get_latest_for_player(&self, riot_id: &str) -> Result<Option<Value>, Box<dyn std::error::Error + Send + Sync>> {
        let key = riot_id.trim().to_lowercase();
        if let Some(v) = self.latest_by_player.get(key.as_bytes())? {
            let ((_, mid), _) = bincode::serde::decode_from_slice::<(i64, String), _>(&v, bincode::config::standard())?;
            if let Some(m) = self.matches.get(mid.as_bytes())? {
                let bytes = zstd::stream::decode_all(std::io::Cursor::new(m.to_vec()))?;
                let json: Value = serde_json::from_slice(&bytes)?;
                return Ok(Some(json));
            }
        }
        Ok(None)
    }

    pub fn get_puuid_for_riot(&self, riot_id: &str) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
        let key = riot_id.trim().to_lowercase();
        if let Some(v) = self.riot_to_puuid.get(key.as_bytes())? {
            let (puuid, _) = bincode::serde::decode_from_slice::<String, _>(&v, bincode::config::standard())?;
            return Ok(Some(puuid));
        }
        Ok(None)
    }

    pub fn get_page_by_puuid(&self, puuid: &str, start: usize, size: usize) -> Result<Vec<Value>, Box<dyn std::error::Error + Send + Sync>> {
        let mut out = Vec::new();
        if let Some(v) = self.by_puuid.get(puuid.as_bytes())? {
            let (vec_ts_mids, _) = bincode::serde::decode_from_slice::<Vec<(i64, String)>, _>(&v, bincode::config::standard())?;
            if start >= vec_ts_mids.len() { return Ok(out); }
            let end = (start + size).min(vec_ts_mids.len());
            for (_, mid) in &vec_ts_mids[start..end] {
                if let Some(m) = self.matches.get(mid.as_bytes())? {
                    let bytes = zstd::stream::decode_all(std::io::Cursor::new(m.to_vec()))?;
                    let json: Value = serde_json::from_slice(&bytes)?;
                    out.push(json);
                }
            }
        }
        Ok(out)
    }

    pub fn upsert_match(&self, raw: &Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let root = normalize_to_wrapped(raw);
        let data = root.get("data").unwrap_or(&root);

        let match_id = data.get("metadata")
            .and_then(|m| m.get("match_id")).and_then(|v| v.as_str())
            .or_else(|| data.get("match_id").and_then(|v| v.as_str()))
            .ok_or("missing match_id")?
            .to_string();

        let ts = extract_ts_ms(data);
        let players = collect_players(data);

        let json_bytes = serde_json::to_vec(&root)?;
        let compressed = zstd::stream::encode_all(std::io::Cursor::new(json_bytes), 3)?;
        self.matches.insert(match_id.as_bytes(), compressed)?;

        for p in players {
            if let (Some(puuid), Some(name), Some(tag)) = (p.puuid, p.name, p.tag) {
                let riot_key = format!("{}#{}", name.to_lowercase(), tag.to_lowercase());

                self.riot_to_puuid.insert(riot_key.as_bytes(), bincode::serde::encode_to_vec(&puuid, bincode::config::standard())?)?;

                // latest_by_player
                let write_latest = if let Some(prev) = self.latest_by_player.get(riot_key.as_bytes())? {
                    let ((prev_ts, _prev_mid), _) = bincode::serde::decode_from_slice::<(i64, String), _>(&prev, bincode::config::standard())?;
                    ts >= prev_ts
                } else { true };
                if write_latest {
                    self.latest_by_player.insert(riot_key.as_bytes(), bincode::serde::encode_to_vec(&(ts, match_id.clone()), bincode::config::standard())?)?;
                }

                // by_puuid (prepend newest; dedupe)
                let mut vec_ts_mid: Vec<(i64, String)> = if let Some(v) = self.by_puuid.get(puuid.as_bytes())? {
                    bincode::serde::decode_from_slice::<Vec<(i64, String)>, _>(&v, bincode::config::standard())?.0
                } else { Vec::new() };
                if !vec_ts_mid.iter().any(|(_, mid)| mid == &match_id) {
                    vec_ts_mid.insert(0, (ts, match_id.clone()));
                    if vec_ts_mid.len() > 1000 { vec_ts_mid.truncate(1000); }
                    self.by_puuid.insert(puuid.as_bytes(), bincode::serde::encode_to_vec(&vec_ts_mid, bincode::config::standard())?)?;
                }
            }
        }

        self.db.flush()?;
        Ok(())
    }
}

fn normalize_to_wrapped(raw: &Value) -> Value {
    if raw.get("data").is_some() {
        raw.clone()
    } else if raw.get("metadata").is_some() || raw.get("players").is_some() {
        let mut m = serde_json::Map::new();
        m.insert("data".to_string(), raw.clone());
        Value::Object(m)
    } else {
        raw.clone()
    }
}

fn extract_ts_ms(data: &Value) -> i64 {
    if let Some(s) = data.get("metadata").and_then(|m| m.get("started_at")).and_then(|v| v.as_str()) {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) { return dt.timestamp_millis(); }
    }
    if let Some(ms) = data.get("metadata").and_then(|m| m.get("game_start")).and_then(|v| v.as_i64()) { return ms; }
    if let Some(ms) = data.get("metadata").and_then(|m| m.get("game_length_in_ms")).and_then(|v| v.as_i64()) { return ms; }
    chrono::Utc::now().timestamp_millis()
}

struct PlayerEntry { puuid: Option<String>, name: Option<String>, tag: Option<String> }

fn collect_players(data: &Value) -> Vec<PlayerEntry> {
    if let Some(arr) = data.get("players").and_then(|v| v.as_array()) { return arr.iter().map(extract_p).collect(); }
    if let Some(arr) = data.get("players").and_then(|v| v.get("all")).and_then(|v| v.as_array()) { return arr.iter().map(extract_p).collect(); }
    if let Some(obj) = data.get("players").and_then(|v| v.as_object()) {
        let mut out = Vec::new();
        for k in ["red", "blue"] { if let Some(arr) = obj.get(k).and_then(|v| v.as_array()) { out.extend(arr.iter().map(extract_p)); } }
        return out;
    }
    Vec::new()
}

fn extract_p(p: &Value) -> PlayerEntry {
    PlayerEntry {
        puuid: p.get("puuid").and_then(|v| v.as_str()).map(|s| s.to_string()),
        name: p.get("name").and_then(|v| v.as_str()).map(|s| s.to_string()),
        tag: p.get("tag").and_then(|v| v.as_str()).map(|s| s.to_string()),
    }
}
