use bincode::{Decode, Encode};
// save unique puuid player data to disk in location under guild id, platform, region, mode, and mode type.


#[derive(Encode, Decode, PartialEq, Debug)]
pub struct PlayerData {
    pub player: Vec<Player>,
}

#[derive(Encode, Decode, PartialEq, Debug)]
pub struct Player {
    pub puuid: String,
    pub name: String, 
    pub tag: String,
    pub discord_id: Option<String>, // this is optional, add when discord user id is inputted by user by using link command
    pub player_stats: PlayerStats,
    pub last_updated: i64, // unix timestamp
}

#[derive(Encode, Decode, PartialEq, Debug)]
pub struct PlayerStats {
    pub time_played: i64, // convert to seconds before saving
    pub total_kills: i32,
    pub total_deaths: i32,
    pub total_assists: i32,
    pub total_aces: i32,
    pub total_headshots: i32,
    pub total_bodyshots: i32,
    pub total_legshots: i32,
    pub total_score: i32,
    pub total_damage_dealt: i32,
    pub total_damage_received: i32,
    pub total_wins: i32,
    pub total_losses: i32,
    pub total_rounds_won: i32,
    pub total_rounds_lost: i32,
    pub total_matches_played: i32,
    pub total_matches_won: i32, // this will have to be determined in the storing process by checking player
    pub total_matches_lost: i32, // team id and seeing if their team has won or lost
    pub match_player_stats: MatchPlayerStats,
    pub weapon_player_stats: WeaponPlayerStats,
    pub player_versus_player_stats: PlayerVsPlayerStats,
    pub account_level: i32,
    pub session_playtime_in_ms: i64,
    pub behavior: PlayerBehavior,
    pub economy: PlayerEconomy,
    pub ability_casts: AbilityCasts,
}

#[derive(Encode, Decode, PartialEq, Debug)]
pub struct PlayerBehavior {
    pub afk_rounds: i32,
    pub friendly_fire: FriendlyFire,
    pub rounds_in_spawn: i32,
}

#[derive(Encode, Decode, PartialEq, Debug)]
pub struct FriendlyFire {
    pub incoming: i32,
    pub outgoing: i32,
}

#[derive(Encode, Decode, PartialEq, Debug)]
pub struct PlayerEconomy {
    pub spent: EconomySpent,
    pub loadout_value: LoadoutValue,
}

#[derive(Encode, Decode, PartialEq, Debug)]
pub struct EconomySpent {
    pub overall: i32,
    pub average: i32,
}

#[derive(Encode, Decode, PartialEq, Debug)]
pub struct LoadoutValue {
    pub overall: i32,
    pub average: i32,
}

#[derive(Encode, Decode, PartialEq, Debug)]
pub struct AbilityCasts {
    pub grenade: i32,
    pub ability_1: i32,
    pub ability_2: i32,
    pub ultimate: i32,
}

#[derive(Encode, Decode, PartialEq, Debug)]
pub struct MatchPlayerStats {
    pub total_rounds_won: i32,
    pub total_rounds_lost: i32,
    pub total_matches_played: i32,
    pub total_matches_won: i32, // this will have to be determined in the storing process by checking player
    pub total_matches_lost: i32, // team id and seeing if their team has won or lost
}

#[derive(Encode, Decode, PartialEq, Debug)]
pub struct WeaponPlayerStats {
    pub weapon_stats: Vec<WeaponStats>,
}

// we will be using this later to make a command that retrieves how many times this player has killed x player
// and how many times x player has killed this player and listing them.
#[derive(Encode, Decode, PartialEq, Debug)]
pub struct PlayerVsPlayerStats {
    pub total_killed_victims: String, // this will be storing the puuids of the victims
    pub total_deaths_by_killers: String, // this will be storing the puuids of who killed this player
}

#[derive(Encode, Decode, PartialEq, Debug)]
pub struct WeaponStats {
    pub weapon_id: String,
    pub weapon_name: String,
    pub total_weapon_kills: i32,
    pub total_weapon_headshots: i32,
    pub total_weapon_bodyshots:  i32,
    pub total_weapon_legshots: i32,
    pub total_damage: i32,
}