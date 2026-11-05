use anyhow::Result;
use serde_json::Value;

use crate::api::statuses::Statuses;
use crate::api::{self, with_params};
use crate::cli::{PlanCommand, PlanListArgs};
use crate::context::Ctx;
use crate::ids::{self, Kind};
use crate::markup;
use crate::output::{date, note_more, print_json, print_table, table, text};
use crate::project::{self, completed_param};

pub fn run(ctx: &Ctx, cmd: PlanCommand) -> Result<()> {
    match cmd {
        PlanCommand::List(args) => list(ctx, &args),
        PlanCommand::View(args) => view(ctx, &args.plan),
    }
}

fn list(ctx: &Ctx, args: &PlanListArgs) -> Result<()> {
    let project = project::resolve(&ctx.client, &args.project.project)?;
    let mut params = completed_param(&args.state);
    if let Some(m) = &args.milestone {
        params.push(("milestone_id", ids::parse(m, Kind::Milestone)?.to_string()));
    }
    let endpoint = with_params(&format!("get_plans/{project}"), &params);
    let limit = args.limit.get();
    // A name filter runs here, so it has to see every plan before the limit applies.
    let fetch_limit = if args.name.is_some() { None } else { limit };
    let mut page = api::list(&ctx.client, &endpoint, "plans", fetch_limit)?;
    if let Some(name) = &args.name {
        let wanted = name.to_lowercase();
        page.items
            .retain(|p| text(&p["name"]).to_lowercase().contains(&wanted));
        if let Some(l) = limit {
            page.more = page.items.len() > l;
            page.items.truncate(l);
        }
    }
    if ctx.json {
        return print_json(&page.items);
    }
    let statuses = Statuses::fetch(&ctx.client)?;
    let rows: Vec<Vec<String>> = page
        .items
        .iter()
        .map(|p| {
            let mut name = text(&p["name"]);
            if p["is_completed"].as_bool() == Some(true) {
                name.push_str(" (completed)");
            }
            vec![
                text(&p["id"]),
                date(&p["created_on"]),
                statuses.tally(p),
                name,
            ]
        })
        .collect();
    print_table(&rows, "No plans found.");
    note_more(page.more, page.items.len());
    Ok(())
}

/// Every run in a plan's entries, each carrying its entry's ID.
pub fn runs(plan: &Value) -> Vec<Value> {
    plan["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|entry| {
            entry["runs"].as_array().into_iter().flatten().map(|run| {
                let mut run = run.clone();
                if run.get("entry_id").is_none() {
                    run["entry_id"] = entry["id"].clone();
                }
                run
            })
        })
        .collect()
}

/// A run's name with its configuration, as a plan lists it.
pub fn run_name(run: &Value) -> String {
    let name = text(&run["name"]);
    match run["config"].as_str().filter(|c| !c.is_empty()) {
        Some(config) => format!("{name} ({config})"),
        None => name,
    }
}

fn view(ctx: &Ctx, plan: &str) -> Result<()> {
    let id = ids::parse(plan, Kind::Plan)?;
    let p = ctx.client.get(&format!("get_plan/{id}"))?;
    if ctx.json {
        return print_json(&p);
    }
    let statuses = Statuses::fetch(&ctx.client)?;
    println!("{id}  {}", text(&p["name"]));
    println!("{}", ctx.web_url(&format!("plans/view/{id}")));
    let state = if p["is_completed"].as_bool() == Some(true) {
        "completed"
    } else {
        "active"
    };
    let mut rows = vec![
        vec!["State".to_string(), state.to_string()],
        vec!["Tests".to_string(), statuses.tally(&p)],
        vec!["Created".to_string(), date(&p["created_on"])],
    ];
    if !p["milestone_id"].is_null() {
        rows.push(vec!["Milestone".to_string(), text(&p["milestone_id"])]);
    }
    println!("{}", table(&rows, "  "));
    let description = markup::to_text(&text(&p["description"]));
    if !description.is_empty() {
        println!("\n{description}");
    }
    let runs = runs(&p);
    if !runs.is_empty() {
        println!("\nRuns");
        let rows: Vec<Vec<String>> = runs
            .iter()
            .map(|r| {
                vec![
                    format!("R{}", text(&r["id"])),
                    statuses.tally(r),
                    run_name(r),
                ]
            })
            .collect();
        println!("{}", table(&rows, "  "));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn runs_flatten_out_of_entries() {
        let plan = json!({"entries": [
            {"id": "e1", "runs": [{"id": 201, "name": "Login", "config": "Chrome"}]},
            {"id": "e2", "runs": [{"id": 202, "name": "Signup", "config": null}]}
        ]});
        let runs = runs(&plan);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[1]["entry_id"], "e2");
        assert_eq!(run_name(&runs[0]), "Login (Chrome)");
        assert_eq!(run_name(&runs[1]), "Signup");
        assert!(super::runs(&json!({})).is_empty());
    }

    #[test]
    fn plans_list_by_name_and_view_their_runs() {
        use crate::testing::{cli, get, page, statuses};
        let mut s = mockito::Server::new();
        statuses(&mut s);
        let plans = json!([
            {"id": 91, "name": "2.4 Regression", "created_on": 1762000000,
             "passed_count": 11, "custom_status1_count": 4},
            {"id": 85, "name": "CLI 8.0", "created_on": 1760000000, "is_completed": true,
             "failed_count": 2}
        ]);
        get(
            &mut s,
            "get_plans/2&is_completed=0&milestone_id=15&limit=50&offset=0",
            page("plans", plans.clone()),
        );
        get(
            &mut s,
            "get_plans/2&limit=250&offset=0",
            page("plans", plans),
        );
        get(
            &mut s,
            "get_plan/91",
            json!({"id": 91, "name": "2.4 Regression", "is_completed": false,
                   "milestone_id": 15, "description": "<p>Notes</p>", "passed_count": 1,
                   "entries": [{"id": "e1", "runs": [
                       {"id": 92, "name": "Win", "config": "Windows", "untested_count": 3}]}]}),
        );
        cli(&s, &["plan", "list", "-p", "2", "--active", "-m", "15"]).unwrap();
        cli(
            &s,
            &[
                "plan",
                "list",
                "-p",
                "2",
                "--name",
                "regression",
                "--limit",
                "1",
            ],
        )
        .unwrap();
        cli(&s, &["--json", "plan", "list", "-p", "2", "--name", "cli"]).unwrap();
        cli(&s, &["plan", "view", "91"]).unwrap();
        cli(&s, &["--json", "plan", "view", "91"]).unwrap();
    }
}
