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
    let t = ctx.client.get(&format!("get_test/{id}"))?;
    let results = api::list(
        &ctx.client,
        &format!("get_results/{id}"),
        "results",
        Some(args.results),
    )?;
    if ctx.json {
        let mut out = t.clone();
        out["results"] = Value::Array(results.items);
        return print_json(&out);
    }
    let statuses = Statuses::fetch(&ctx.client)?;
    println!("T{id}  {}", text(&t["title"]));
    println!("{}", ctx.web_url(&format!("tests/view/{id}")));
    let rows = vec![
        vec!["Status".to_string(), statuses.label(&t["status_id"])],
        vec!["Run".to_string(), format!("R{}", text(&t["run_id"]))],
        vec!["Case".to_string(), format!("C{}", text(&t["case_id"]))],
    ];
    println!("{}", table(&rows, "  "));
    let body = case::body(&t);
    print!("{body}");
    if !results.items.is_empty() {
        println!("\nResults");
        for r in &results.items {
            print_result(r, &statuses);
        }
    }
    if markup::has_attachment_image(&body) {
        println!("\n{}", case::IMAGE_HINT);
    }
    Ok(())
}

/// A result with its comment and step outcomes, as a test's history shows it.
fn print_result(r: &Value, statuses: &Statuses) {
    let mut head = vec![
        format!("#{}", text(&r["id"])),
        statuses.label(&r["status_id"]),
        date(&r["created_on"]),
    ];
    let elapsed = text(&r["elapsed"]);
    if !elapsed.is_empty() {
        head.push(elapsed);
    }
    println!("  {}", head.join("  "));
    let comment = markup::to_text(&text(&r["comment"]));
    if !comment.is_empty() {
        println!("{}", case::indent(&comment, "    ", "    "));
    }
    for (i, step) in r["custom_step_results"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let actual = first_line(&markup::to_text(&text(&step["actual"])), 80);
        println!(
            "    {}. {}  {actual}",
            i + 1,
            statuses.label(&step["status_id"])
        );
    }
}
