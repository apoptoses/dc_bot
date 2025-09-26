pub mod general;
pub mod valorant;
pub mod moderation;

pub fn commands() -> Vec<poise::Command<crate::Data, crate::Error>> {
    vec![
        // Valorant
        valorant::custom::custom_match::custom_match(),
        valorant::stats::stats(),
        valorant::prune_match::prune_match(),
    ]
}