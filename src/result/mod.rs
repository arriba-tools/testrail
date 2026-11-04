mod add;

use anyhow::Result;

use crate::api::{self, statuses::Statuses};
use crate::cli::{ResultCommand, ResultListArgs};
use crate::context::Ctx;
use crate::ids::{self, Kind};
use crate::markup;
use crate::output::{date, first_line, note_more, print_json, print_table, text};

pub fn run(ctx: &Ctx, cmd: ResultCommand) -> Result<()> {
    match cmd {
        ResultCommand::List(args) => list(ctx, &args),
        ResultCommand::Add(args) => add::run(ctx, &args),
    }
}

fn list(ctx: &Ctx, args: &ResultListArgs) -> Result<()> {
    let (endpoint, whole_run) = match (&args.test, &args.run, &args.case) {
        (Some(t), _, _) => (format!("get_results/{}", ids::parse(t, Kind::Test)?), false),
        (None, Some(r), Some(c)) => (
            format!(
                "get_results_for_case/{}/{}",
                ids::parse(r, Kind::Run)?,
                ids::parse(c, Kind::Case)?
            ),
            false,
        ),
        (None, Some(r), None) => (
            format!("get_results_for_run/{}", ids::parse(r, Kind::Run)?),
            true,
        ),
        (None, None, _) => unreachable!("clap requires --test or --run"),
    };
    let page = api::list(&ctx.client, &endpoint, "results", args.limit.get())?;
    if ctx.json {
        return print_json(&page.items);
    }
    let statuses = Statuses::fetch(&ctx.client)?;
    let rows: Vec<Vec<String>> = page
        .items
        .iter()
        .map(|r| {
            let mut row = vec![
                format!("#{}", text(&r["id"])),
                statuses.label(&r["status_id"]),
                date(&r["created_on"]),
            ];
            if whole_run {
                row.push(format!("T{}", text(&r["test_id"])));
            }
            row.push(text(&r["version"]));
            row.push(text(&r["elapsed"]));
            row.push(first_line(&markup::to_text(&text(&r["comment"])), 60));
            row
        })
        .collect();
    print_table(&rows, "No results found.");
    note_more(page.more, page.items.len());
    Ok(())
}
