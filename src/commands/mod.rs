pub mod general;
pub mod social_notifications;
pub mod valorant;
pub mod moderation;

pub fn commands() -> Vec<poise::Command<crate::Data, crate::Error>> {
    vec![
        // General
        // Social Notifications - YouTube
        social_notifications::youtube::sub(),
        social_notifications::youtube::unsub(),
        // Social Notifications - Twitch
        social_notifications::twitch::follow(),
        social_notifications::twitch::unfollow(),
        // Valorant
        valorant::custom::custom_match::custom_match(),
        valorant::custom::custom_match::player_matches(),
        // Moderation
        valorant::prune_match::prune_match(),
    ]
}