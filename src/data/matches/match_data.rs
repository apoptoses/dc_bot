
use serde::{Serialize, Deserialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex as AsyncMutex;

static FILE_LOCKS: OnceLock<AsyncMutex<HashMap<String, Arc<AsyncMutex<()>>>>> = OnceLock::new();

fn file_locks() -> &'static AsyncMutex<HashMap<String, Arc<AsyncMutex<()>>>> {
    FILE_LOCKS.get_or_init(|| AsyncMutex::new(HashMap::new()))
}

async fn get_lock_for_path(path: &Path) -> Arc<AsyncMutex<()>> {
    let key = path.to_string_lossy().to_string();
    let map_mutex = file_locks();
    let mut map = map_mutex.lock().await;
    if let Some(l) = map.get(&key) {
        return l.clone();
    }
    let l = Arc::new(AsyncMutex::new(()));
    map.insert(key, l.clone());
    l
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Victim {
    pub puuid: String,
    pub name: String,
    pub tag: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Killer {
    pub puuid: String,
    pub name: String,
    pub tag: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Blue {
    pub has_won: bool,
    pub rounds_won: i32,
    pub rounds_lost: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Red {
    pub has_won: bool,
    pub rounds_won: i32,
    pub rounds_lost: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Spent {
    pub overall: i32,
    pub average: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Economy {
    pub spent: Spent,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Damage {
    pub dealt: i32,
    pub received: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Stats {
    pub score: i32,
    pub kills: i32,
    pub deaths: i32,
    pub assists: i32,
    pub headshots: i32,
    pub legshots: i32,
    pub bodyshots: i32,
    pub damage: Damage,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Agent {
    pub name: String,
}
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Map {
    pub name: String,
}


// this is for individual 1v1 data
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Kills {
    pub killer: Killer,
    pub victim: Victim,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Teams {
    pub red: Red,
    pub blue: Blue,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Players {
    pub puuid: String,
    pub name: String,
    pub tag: String,
    pub rank: Option<String>,
    pub team_id: String,
    pub agent: Agent,
    pub stats: Stats,
    pub economy: Economy,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Queue {
    pub id: String,
    pub name: String,
    pub mode_type: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Metadata {
    pub match_id: String,
    pub map: Map,
}
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct MatchData {
    pub metadata: Metadata,
    pub game_length_in_ms: i32,
    pub started_at: String,
    pub queue: Queue,
    pub players: Vec<Players>,
    pub teams: Teams,
    pub kills: Vec<Kills>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ApiResponse {
    pub data: Vec<MatchData>,
}

fn as_i32(v: &Value) -> i32 { v.as_i64().unwrap_or(0) as i32 }
fn s(v: Option<&Value>) -> String { v.and_then(|x| x.as_str()).unwrap_or("").to_string() }

impl MatchData {
    pub fn from_match_json(root: &Value) -> Option<MatchData> {
        let data = root.get("data").unwrap_or(root);
        let metadata = {
            let mm = data.get("metadata").and_then(|v| v.as_object())?;
            let match_id = mm.get("match_id").and_then(|v| v.as_str())?.to_string();
            let map_name = mm.get("map").and_then(|m| m.get("name")).and_then(|v| v.as_str()).unwrap_or("").to_string();
            Metadata { match_id, map: Map { name: map_name } }
        };
        
        let game_len_ms = data
            .get("metadata").and_then(|m| m.get("game_length_in_ms")).map(as_i32)
            .or_else(|| data.get("game_length_in_ms").map(as_i32))
            .unwrap_or(0);
        let started_at_str = data
            .get("metadata").and_then(|m| m.get("started_at")).and_then(|v| v.as_str())
            .or_else(|| data.get("started_at").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string();
        let queue_v = data
            .get("metadata").and_then(|m| m.get("queue"))
            .or_else(|| data.get("queue"))
            .unwrap_or(&Value::Null);
        let queue = Queue {
            id: s(queue_v.get("id")),
            name: s(queue_v.get("name")),
            mode_type: s(queue_v.get("mode_type")),
        };

        let mut players: Vec<Players> = Vec::new();
        if let Some(p_arr) = data.get("players").and_then(|v| v.as_array()) {
            for p in p_arr {
                let puuid = s(p.get("puuid"));
                let name = s(p.get("name"));
                let tag = s(p.get("tag"));
                let team_id = s(p.get("team_id"));
                let agent_name = p.get("agent").and_then(|a| a.get("name")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let stats_v = p.get("stats").unwrap_or(&Value::Null);
                let stats = Stats {
                    score: stats_v.get("score").map(as_i32).unwrap_or(0),
                    kills: stats_v.get("kills").map(as_i32).unwrap_or(0),
                    deaths: stats_v.get("deaths").map(as_i32).unwrap_or(0),
                    assists: stats_v.get("assists").map(as_i32).unwrap_or(0),
                    headshots: stats_v.get("headshots").map(as_i32).unwrap_or(0),
                    legshots: stats_v.get("legshots").map(as_i32).unwrap_or(0),
                    bodyshots: stats_v.get("bodyshots").map(as_i32).unwrap_or(0),
                    damage: Damage {
                        dealt: stats_v.get("damage").and_then(|d| d.get("dealt")).map(as_i32).unwrap_or(0),
                        received: stats_v.get("damage").and_then(|d| d.get("received")).map(as_i32).unwrap_or(0),
                    },
                };
                let economy_v = p.get("economy").unwrap_or(&Value::Null);
                let spent_v = economy_v.get("spent").unwrap_or(&Value::Null);
                let economy = Economy {
                    spent: Spent {
                        overall: spent_v.get("overall").map(as_i32).unwrap_or(0),
                        average: spent_v.get("average").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                    },
                };
                let rank = p.get("rank").and_then(|v| v.as_str()).map(|s| s.to_string());
                players.push(Players { puuid, name, tag, team_id, agent: Agent { name: agent_name }, stats, economy, rank });
            }
        }

        let mut red = Red::default();
        let mut blue = Blue::default();
        if let Some(teams_arr) = data.get("teams").and_then(|v| v.as_array()) {
            for t in teams_arr {
                let team_id = t.get("team_id").and_then(|v| v.as_str()).unwrap_or("");
                let won = t.get("won").and_then(|v| v.as_bool()).unwrap_or(false);
                let rounds = t.get("rounds").and_then(|v| v.as_object());
                let rounds_won = rounds.and_then(|r| r.get("won")).map(as_i32).unwrap_or(0);
                let rounds_lost = rounds.and_then(|r| r.get("lost")).map(as_i32).unwrap_or(0);
                match team_id {
                    "Red" | "red" => {
                        red = Red { has_won: won, rounds_won, rounds_lost };
                    }
                    "Blue" | "blue" => {
                        blue = Blue { has_won: won, rounds_won, rounds_lost };
                    }
                    _ => {}
                }
            }
        } else if let Some(obj) = data.get("teams").and_then(|v| v.as_object()) {
            if let Some(r) = obj.get("red").and_then(|v| v.as_object()) {
                red = Red {
                    has_won: r.get("has_won").and_then(|v| v.as_bool()).unwrap_or(false),
                    rounds_won: r.get("rounds_won").map(as_i32).unwrap_or(0),
                    rounds_lost: r.get("rounds_lost").map(as_i32).unwrap_or(0),
                };
            }
            if let Some(b) = obj.get("blue").and_then(|v| v.as_object()) {
                blue = Blue {
                    has_won: b.get("has_won").and_then(|v| v.as_bool()).unwrap_or(false),
                    rounds_won: b.get("rounds_won").map(as_i32).unwrap_or(0),
                    rounds_lost: b.get("rounds_lost").map(as_i32).unwrap_or(0),
                };
            }
        }
        let teams = Teams { red, blue };

        let mut kills: Vec<Kills> = Vec::new();
        if let Some(k_arr) = data.get("kills").and_then(|v| v.as_array()) {
            for k in k_arr {
                let killer = Killer {
                    puuid: s(k.get("killer").and_then(|o| o.get("puuid"))),
                    name: s(k.get("killer").and_then(|o| o.get("name"))),
                    tag: s(k.get("killer").and_then(|o| o.get("tag"))),
                };
                let victim = Victim {
                    puuid: s(k.get("victim").and_then(|o| o.get("puuid"))),
                    name: s(k.get("victim").and_then(|o| o.get("name"))),
                    tag: s(k.get("victim").and_then(|o| o.get("tag"))),
                };
                kills.push(Kills {
                    killer, 
                    victim,
                });
            }
        }


        Some(MatchData { 
            metadata, 
            game_length_in_ms: game_len_ms, 
            started_at: started_at_str, 
            queue, 
            players, 
            teams,
            kills,
        })
    }

    pub async fn save_to_disk(&self, path: &Path) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let lock = get_lock_for_path(path).await;
        let _guard = lock.lock().await;

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

pub fn from_match_json(root: &Value) -> Option<MatchData> {
    MatchData::from_match_json(root)
}

pub async fn save_match_to_disk(base_dir: &Path, data: &MatchData) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    let file = base_dir.join(format!("{}.json", data.metadata.match_id));
    data.save_to_disk(&file).await?;
    Ok(file)
}
