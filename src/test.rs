use anyhow::Result;
use serde_json::Value;

use crate::api::statuses::Statuses;
use crate::api::{self, with_params};
use crate::case;
use crate::cli::{TestCommand, TestListArgs, TestViewArgs};
use crate::context::Ctx;
use crate::ids::{self, Kind};
use crate::markup;
use crate::output::{date, first_line, note_more, print_json, print_table, table, text};
use crate::plan;

pub fn run(ctx: &Ctx, cmd: TestCommand) -> Result<()> {
    match cmd {
        TestCommand::List(args) => list(ctx, &args),
        TestCommand::View(args) => view(ctx, &args),
    }
}

fn list(ctx: &Ctx, args: &TestListArgs) -> Result<()> {
    let run = ids::parse(&args.run, Kind::Run)?;
    let statuses = Statuses::fetch(&ctx.client)?;
    let mut params = Vec::new();
    if !args.status.is_empty() {
        let wanted = args
            .status
            .iter()
            .map(|s| statuses.id(s).map(|id| id.to_string()))
            .collect::<Result<Vec<_>>>()?;
        params.push(("status_id", wanted.join(",")));
    }
    let endpoint = with_params(&format!("get_tests/{run}"), &params);
    let page = api::list(&ctx.client, &endpoint, "tests", args.limit.get())?;
    if ctx.json {
        return print_json(&page.items);
    }
    let rows: Vec<Vec<String>> = page
        .items
        .iter()
        .map(|t| {
            vec![
                format!("T{}", text(&t["id"])),
                statuses.label(&t["status_id"]),
                format!("C{}", text(&t["case_id"])),
                text(&t["title"]),
            ]
        })
        .collect();
    print_table(&rows, "No tests found.");
    note_more(page.more, page.items.len());
    Ok(())
}

fn view(ctx: &Ctx, args: &TestViewArgs) -> Result<()> {
    let id = ids::parse(&args.test, Kind::Test)?;
    let mut t = ctx.client.get(&format!("get_test/{id}"))?;
    // The run names the platform through its configuration, which the test lacks.
    let run = ctx.client.get(&format!("get_run/{}", text(&t["run_id"])))?;
    let results = api::list(
        &ctx.client,
        &format!("get_results/{id}"),
        "results",
        Some(args.results),
    )?;
    if ctx.json {
        t["run"] = run;
        t["results"] = Value::Array(results.items);
        return print_json(&t);
    }
    let statuses = Statuses::fetch(&ctx.client)?;
    println!("T{id}  {}", text(&t["title"]));
    println!("{}", ctx.web_url(&format!("tests/view/{id}")));
    let rows = vec![
        vec!["Status".to_string(), statuses.label(&t["status_id"])],
        vec![
            "Run".to_string(),
            format!("R{}  {}", text(&t["run_id"]), plan::run_name(&run)),
        ],
        vec!["Case".to_string(), format!("C{}", text(&t["case_id"]))],
    ];
    println!("{}", table(&rows, "  "));
    let mut out = case::body(&t);
    if !results.items.is_empty() {
        out.push_str("\nResults\n");
        for r in &results.items {
            out.push_str(&result_text(r, &statuses));
        }
    }
    print!("{out}");
    if markup::has_attachment_image(&out) {
        println!("\n{}", case::IMAGE_HINT);
    }
    Ok(())
}

/// A result with its comment and step outcomes, as a test's history shows it.
fn result_text(r: &Value, statuses: &Statuses) -> String {
    let mut head = vec![
        format!("#{}", text(&r["id"])),
        statuses.label(&r["status_id"]),
        date(&r["created_on"]),
    ];
    for (label, key) in [
        ("version", "version"),
        ("elapsed", "elapsed"),
        ("defects", "defects"),
    ] {
        let value = text(&r[key]);
        if !value.is_empty() {
            head.push(format!("{label} {value}"));
        }
    }
    head.retain(|h| !h.is_empty());
    let mut out = format!("  {}\n", head.join("  "));
    let comment = markup::to_text(&text(&r["comment"]));
    if !comment.is_empty() {
        out.push_str(&case::indent(&comment, "    ", "    "));
        out.push('\n');
    }
    for (i, step) in r["custom_step_results"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let first = format!("    {}. ", i + 1);
        let rest = " ".repeat(first.len());
        let content = first_line(&markup::to_text(&text(&step["content"])), 70);
        out.push_str(&format!(
            "{first}{}  {content}\n",
            statuses.label(&step["status_id"])
        ));
        let actual = markup::to_text(&text(&step["actual"]));
        if !actual.is_empty() {
            out.push_str(&case::indent(&format!("Actual: {actual}"), &rest, &rest));
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn results_show_version_comment_and_step_outcomes() {
        let r = json!({
            "id": 2064, "status_id": 5, "created_on": null, "version": "2.4.11",
            "elapsed": "2m", "defects": null, "comment": "<p>Fails at step 2</p>",
            "custom_step_results": [
                {"content": "<p>Log in</p><p>Open settings</p>", "actual": "", "status_id": "1"},
                {"content": "<p>Save</p>", "actual": "<p>Nothing saved</p>", "status_id": "5"}
            ]
        });
        assert_eq!(
            result_text(&r, &Statuses::builtin()),
            "  #2064  Failed  version 2.4.11  elapsed 2m\n    Fails at step 2\n    1. Passed  Log in\n    2. Failed  Save\n       Actual: Nothing saved\n"
        );
    }
}
