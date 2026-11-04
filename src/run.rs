use anyhow::Result;

use crate::api::{self, Page, with_params};
use crate::cli::{RunCommand, RunListArgs};
use crate::context::Ctx;
use crate::ids::{self, Kind};
use crate::markup;
use crate::output::{counts, date, note_more, print_json, print_table, table, text};
use crate::plan;
use crate::project::{self, completed_param};

pub fn run(ctx: &Ctx, cmd: RunCommand) -> Result<()> {
    match cmd {
        RunCommand::List(args) => list(ctx, &args),
        RunCommand::View(args) => view(ctx, &args.run),
    }
}

fn list(ctx: &Ctx, args: &RunListArgs) -> Result<()> {
    let page = match (&args.plan, &args.project) {
        (Some(p), _) => {
            let id = ids::parse(p, Kind::Plan)?;
            let mut runs = plan::runs(&ctx.client.get(&format!("get_plan/{id}"))?);
            if let Some(completed) = args.state.is_completed() {
                runs.retain(|r| r["is_completed"].as_bool() == Some(completed));
            }
            let mut more = false;
            if let Some(l) = args.limit.get() {
                more = runs.len() > l;
                runs.truncate(l);
            }
            Page { items: runs, more }
        }
        (None, Some(p)) => {
            let project = project::resolve(&ctx.client, p)?;
            let endpoint = with_params(
                &format!("get_runs/{project}"),
                &completed_param(&args.state),
            );
            api::list(&ctx.client, &endpoint, "runs", args.limit.get())?
        }
        (None, None) => unreachable!("clap requires --project or --plan"),
    };
    if ctx.json {
        return print_json(&page.items);
    }
    let rows: Vec<Vec<String>> = page
        .items
        .iter()
        .map(|r| {
            let mut name = plan::run_name(r);
            if r["is_completed"].as_bool() == Some(true) {
                name.push_str(" (completed)");
            }
            vec![format!("R{}", text(&r["id"])), counts(r), name]
        })
        .collect();
    let empty = if args.plan.is_some() {
        "No runs found."
    } else {
        "No runs found outside plans. Pass --plan for the runs inside one."
    };
    print_table(&rows, empty);
    note_more(page.more, page.items.len());
    Ok(())
}

fn view(ctx: &Ctx, run: &str) -> Result<()> {
    let id = ids::parse(run, Kind::Run)?;
    let r = ctx.client.get(&format!("get_run/{id}"))?;
    if ctx.json {
        return print_json(&r);
    }
    println!("R{id}  {}", plan::run_name(&r));
    println!("{}", ctx.web_url(&format!("runs/view/{id}")));
    let state = if r["is_completed"].as_bool() == Some(true) {
        "completed"
    } else {
        "active"
    };
    let mut rows = vec![
        vec!["State".to_string(), state.to_string()],
        vec!["Tests".to_string(), counts(&r)],
        vec!["Created".to_string(), date(&r["created_on"])],
    ];
    for (label, key) in [
        ("Plan", "plan_id"),
        ("Milestone", "milestone_id"),
        ("Suite", "suite_id"),
    ] {
        if !r[key].is_null() {
            rows.push(vec![label.to_string(), text(&r[key])]);
        }
    }
    println!("{}", table(&rows, "  "));
    let description = markup::to_text(&text(&r["description"]));
    if !description.is_empty() {
        println!("\n{description}");
    }
    Ok(())
}
