use anyhow::Result;
use serde_json::Value;

use crate::api::{self, with_params};
use crate::cli::{MilestoneCommand, MilestoneListArgs};
use crate::context::Ctx;
use crate::ids::{self, Kind};
use crate::markup;
use crate::output::{day, print_json, print_table, table, text};
use crate::project::{self, completed_param};

pub fn run(ctx: &Ctx, cmd: MilestoneCommand) -> Result<()> {
    match cmd {
        MilestoneCommand::List(args) => list(ctx, &args),
        MilestoneCommand::View(args) => view(ctx, &args.milestone),
    }
}

fn list(ctx: &Ctx, args: &MilestoneListArgs) -> Result<()> {
    let project = project::resolve(&ctx.client, &args.project.project)?;
    let endpoint = with_params(
        &format!("get_milestones/{project}"),
        &completed_param(&args.state),
    );
    let milestones = api::list(&ctx.client, &endpoint, "milestones", None)?.items;
    if ctx.json {
        return print_json(&milestones);
    }
    let mut rows = Vec::new();
    for m in &milestones {
        rows.push(row(m, ""));
        // Sub-milestones come nested in their parent.
        for sub in m["milestones"].as_array().into_iter().flatten() {
            rows.push(row(sub, "  "));
        }
    }
    print_table(&rows, "No milestones found.");
    Ok(())
}

fn row(m: &Value, indent: &str) -> Vec<String> {
    let mut name = format!("{indent}{}", text(&m["name"]));
    if m["is_completed"].as_bool() == Some(true) {
        name.push_str(" (completed)");
    }
    let due = day(&m["due_on"]);
    let due = if due.is_empty() {
        due
    } else {
        format!("due {due}")
    };
    vec![text(&m["id"]), due, name]
}

fn view(ctx: &Ctx, milestone: &str) -> Result<()> {
    let id = ids::parse(milestone, Kind::Milestone)?;
    let m = ctx.client.get(&format!("get_milestone/{id}"))?;
    if ctx.json {
        return print_json(&m);
    }
    println!("{id}  {}", text(&m["name"]));
    println!("{}", ctx.web_url(&format!("milestones/view/{id}")));
    let state = if m["is_completed"].as_bool() == Some(true) {
        "completed"
    } else {
        "active"
    };
    let mut rows = vec![vec!["State".to_string(), state.to_string()]];
    for (label, key) in [("Starts", "start_on"), ("Due", "due_on")] {
        let value = day(&m[key]);
        if !value.is_empty() {
            rows.push(vec![label.to_string(), value]);
        }
    }
    let refs = text(&m["refs"]);
    if !refs.is_empty() {
        rows.push(vec!["Refs".to_string(), refs]);
    }
    println!("{}", table(&rows, "  "));
    let description = markup::to_text(&text(&m["description"]));
    if !description.is_empty() {
        println!("\n{description}");
    }
    Ok(())
}
