use anyhow::Result;
use serde_json::Value;

use crate::api::{self, with_params};
use crate::cli::{SectionCommand, SectionListArgs};
use crate::context::Ctx;
use crate::ids::{self, Kind};
use crate::output::{print_json, print_table, text};
use crate::project;

pub fn run(ctx: &Ctx, cmd: SectionCommand) -> Result<()> {
    match cmd {
        SectionCommand::List(args) => list(ctx, &args),
    }
}

fn list(ctx: &Ctx, args: &SectionListArgs) -> Result<()> {
    let project = project::resolve(&ctx.client, &args.project.project)?;
    let mut params = Vec::new();
    if let Some(suite) = &args.suite {
        params.push(("suite_id", ids::parse(suite, Kind::Suite)?.to_string()));
    }
    let endpoint = with_params(&format!("get_sections/{project}"), &params);
    let sections = api::list(&ctx.client, &endpoint, "sections", None)?.items;
    if ctx.json {
        return print_json(&sections);
    }
    print_table(&rows(&sections, args.tree), "No sections found.");
    Ok(())
}

/// One row per section, indented by depth for a tree.
fn rows(sections: &[Value], tree: bool) -> Vec<Vec<String>> {
    sections
        .iter()
        .map(|s| {
            let depth = if tree {
                s["depth"].as_u64().unwrap_or(0) as usize
            } else {
                0
            };
            vec![
                text(&s["id"]),
                format!("{}{}", "  ".repeat(depth), text(&s["name"])),
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn trees_indent_by_depth() {
        let sections = vec![
            json!({"id": 1, "name": "Login", "depth": 0}),
            json!({"id": 2, "name": "Errors", "depth": 1}),
        ];
        assert_eq!(rows(&sections, true)[1], vec!["2", "  Errors"]);
        assert_eq!(rows(&sections, false)[1], vec!["2", "Errors"]);
    }
}
