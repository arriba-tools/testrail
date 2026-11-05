use anyhow::Result;
use serde_json::Value;

use crate::api::{self, with_params};
use crate::cli::{MilestoneCommand, MilestoneListArgs};
use crate::context::Ctx;
use crate::ids::{self, Kind};
use crate::output::{
    day, mark_completed, print_json, print_rich, print_table, state_row, table, text,
};
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
    let name = mark_completed(format!("{indent}{}", text(&m["name"])), m);
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
    let mut rows = vec![state_row(&m)];
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
    println!("{id}  {}", text(&m["name"]));
    println!("{}", ctx.page_url(Kind::Milestone, id));
    println!("{}", table(&rows, "  "));
    print_rich(&m["description"]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::testing::{cli, get, page};
    use mockito::Server;
    use serde_json::json;

    #[test]
    fn milestones_list_with_sub_milestones_and_view() {
        let mut s = Server::new();
        get(
            &mut s,
            "get_milestones/2&is_completed=1&limit=250&offset=0",
            page(
                "milestones",
                json!([{"id": 9, "name": "2.0", "due_on": 1772000000, "is_completed": true,
                        "milestones": [{"id": 10, "name": "2.0 beta", "is_completed": true}]}]),
            ),
        );
        get(
            &mut s,
            "get_milestone/9",
            json!({"id": 9, "name": "2.0", "is_completed": false, "start_on": 1771000000,
                   "due_on": 1772000000, "refs": "PROJ-1", "description": "<p>Ship it</p>"}),
        );
        cli(&s, &["milestone", "list", "-p", "2", "--completed"]).unwrap();
        cli(
            &s,
            &["--json", "milestone", "list", "-p", "2", "--completed"],
        )
        .unwrap();
        cli(&s, &["milestone", "view", "M9"]).unwrap();
        cli(&s, &["--json", "milestone", "view", "9"]).unwrap();
    }
}
