use anyhow::Result;

use crate::api;
use crate::cli::SuiteCommand;
use crate::context::Ctx;
use crate::output::{mark_completed, print_json, print_table, text};
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
                    vec![format!("S{}", text(&s["id"])), mark_completed(name, s)]
                })
                .collect();
            print_table(&rows, "No suites found.");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{cli, get, page};
    use mockito::Server;
    use serde_json::json;

    #[test]
    fn suites_list_with_their_flags() {
        let mut s = Server::new();
        get(
            &mut s,
            "get_suites/2&limit=250&offset=0",
            page(
                "suites",
                json!([
                    {"id": 6, "name": "Master", "is_baseline": false, "is_completed": false},
                    {"id": 7, "name": "Base", "is_baseline": true, "is_completed": true}
                ]),
            ),
        );
        cli(&s, &["suite", "list", "-p", "2"]).unwrap();
        cli(&s, &["--json", "suite", "list", "-p", "2"]).unwrap();
    }
}
