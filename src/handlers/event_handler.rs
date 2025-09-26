use poise::serenity_prelude as serenity;
use std::time::Duration;

pub async fn handle_event<'a>(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'a, crate::Data, crate::Error>,
    data: &crate::Data,
) -> Result<(), crate::Error> {
    match event {
        serenity::FullEvent::Ready { data_about_bot, .. } => {
            let startup_duration = data.started_at.elapsed();
            let commands_check = data.commands_check_duration;

            fn fmt_dur(d: Duration) -> String {
                if d.as_secs() >= 1 {
                    format!("{:.3}s", d.as_secs_f64())
                } else {
                    format!("{:.3}ms", d.as_secs_f64() * 1000.0)
                }
            }
            
            let mut name_w = "Name".len();
            let mut status_w = "Status".len();
            for s in &data.command_statuses {
                name_w = name_w.max(s.name.len());
                status_w = status_w.max(s.status.len());
            }
            let total_commands = data.command_statuses.len();

            let title = format!("Bot Ready: {}", data_about_bot.user.name);
            let meta_left = format!(
                "Startup time: {}",
                fmt_dur(startup_duration)
            );
            let meta_right = format!(
                "Commands check: {}",
                fmt_dur(commands_check)
            );
            let meta_w = meta_left.len().max(meta_right.len());

            let table_width = 2 + name_w + 3 + status_w + 2; // | name | status |
            let header_width = title.len().max(meta_w).max(table_width).max(30);
            let hline = format!("+{}+", "=".repeat(header_width));
            let sline = format!("+{}+", "-".repeat(header_width));

            println!("{}", hline);
            println!("|{:<width$}|", title, width = header_width);
            println!("{}", sline);
            println!("|{:<width$}|", meta_left, width = header_width);
            println!("|{:<width$}|", meta_right, width = header_width);
            println!("|{:<width$}|", format!("Commands loaded: {}", total_commands), width = header_width);
            println!("{}", sline);
            
            let table_hline = format!(
                "+-{}-+-{}-+",
                "-".repeat(name_w),
                "-".repeat(status_w)
            );
            println!("{}", table_hline);
            println!(
                "| {:<name_w$} | {:<status_w$} |",
                "Name",
                "Status",
                name_w = name_w,
                status_w = status_w
            );
            println!("{}", table_hline);
            if total_commands == 0 {
                println!(
                    "| {:<name_w$} | {:<status_w$} |",
                    "(no commands)",
                    "-",
                    name_w = name_w,
                    status_w = status_w
                );
            } else {
                for s in &data.command_statuses {
                    println!(
                        "| {:<name_w$} | {:<status_w$} |",
                        s.name,
                        s.status,
                        name_w = name_w,
                        status_w = status_w
                    );
                }
            }
            println!("{}", table_hline);
            println!("{}", hline);

        }
        _ => {}
    }
    Ok(())
}
