use anyhow::Result;

use crate::api;
use crate::cli::SuiteCommand;
use crate::context::Ctx;
use crate::output::{print_json, print_table, text};
use crate::project;

pub fn run(ctx: &Ctx, cmd: SuiteCommand) -> Result<()> {
    match cmd {
        SuiteCommand::List(args) => {
            let project = project::resolve(&ctx.client, &args.project)?;
            let suites = api::list(
                &ctx.client,
                &format!("get_suites/{project}"),
                "suites",
                None,
            )?
            .items;
            if ctx.json {
                return print_json(&suites);
            }
            let rows: Vec<Vec<String>> = suites
                .iter()
                .map(|s| {
                    let mut name = text(&s["name"]);
                    if s["is_baseline"].as_bool() == Some(true) {
                        name.push_str(" (baseline)");
                    }
                    if s["is_completed"].as_bool() == Some(true) {
                        name.push_str(" (completed)");
                    }
                    vec![format!("S{}", text(&s["id"])), name]
                })
                .collect();
            print_table(&rows, "No suites found.");
            Ok(())
        }
    }
}
