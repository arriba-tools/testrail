use anyhow::Result;

use crate::api::statuses::Statuses;
use crate::cli::StatusCommand;
use crate::context::Ctx;
use crate::output::{print_json, print_table, text};

pub fn run(ctx: &Ctx, cmd: StatusCommand) -> Result<()> {
    match cmd {
        StatusCommand::List => {
            let statuses = Statuses::fetch(&ctx.client)?;
            if ctx.json {
                return print_json(statuses.all());
            }
            let rows: Vec<Vec<String>> = statuses
                .all()
                .iter()
                .map(|s| {
                    let mut flags = Vec::new();
                    if s["is_untested"].as_bool() == Some(true) {
                        flags.push("untested");
                    }
                    if s["is_final"].as_bool() == Some(true) {
                        flags.push("final");
                    }
                    vec![
                        text(&s["id"]),
                        text(&s["name"]),
                        text(&s["label"]),
                        flags.join(", "),
                    ]
                })
                .collect();
            print_table(&rows, "No statuses found.");
            Ok(())
        }
    }
}
