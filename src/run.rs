use anyhow::Result;

use crate::api::statuses::Statuses;
use crate::api::{self, Page, with_params};
use crate::cli::{RunCommand, RunListArgs};
use crate::context::Ctx;
use crate::ids::{self, Kind};
use crate::output::{
    date, mark_completed, note_more, print_json, print_rich, print_table, state_row, table, text,
};
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
            let more = api::cut(&mut runs, args.limit.get());
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
    let statuses = Statuses::fetch(&ctx.client)?;
    let rows: Vec<Vec<String>> = page
        .items
        .iter()
        .map(|r| {
            vec![
                format!("R{}", text(&r["id"])),
                statuses.tally(r),
                mark_completed(plan::run_name(r), r),
            ]
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
    let statuses = Statuses::fetch(&ctx.client)?;
    let mut rows = vec![
        state_row(&r),
        vec!["Tests".to_string(), statuses.tally(&r)],
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
    println!("R{id}  {}", plan::run_name(&r));
    println!("{}", ctx.page_url(Kind::Run, id));
    println!("{}", table(&rows, "  "));
    print_rich(&r["description"]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::testing::{cli, get, page, statuses};
    use mockito::Server;
    use serde_json::json;

    #[test]
    fn runs_list_by_project_and_by_plan_and_view() {
        let mut s = Server::new();
        statuses(&mut s);
        get(
            &mut s,
            "get_runs/2&is_completed=0&limit=50&offset=0",
            page(
                "runs",
                json!([{"id": 90, "name": "Codex", "untested_count": 124},
                       {"id": 84, "name": "Installer", "passed_count": 11, "is_completed": true}]),
            ),
        );
        get(
            &mut s,
            "get_runs/3&limit=50&offset=0",
            page("runs", json!([])),
        );
        get(
            &mut s,
            "get_plan/91",
            json!({"entries": [{"id": "e", "runs": [
                {"id": 92, "name": "Win", "config": "Windows", "is_completed": false},
                {"id": 93, "name": "Mac", "config": null, "is_completed": true}]}]}),
        );
        get(
            &mut s,
            "get_run/92",
            json!({"id": 92, "name": "Win", "config": "Windows", "is_completed": true,
                   "plan_id": 91, "milestone_id": null, "suite_id": 6,
                   "created_on": 1762000000, "description": "<p>Notes</p>", "passed_count": 2}),
        );
        cli(&s, &["run", "list", "-p", "2", "--active"]).unwrap();
        cli(&s, &["run", "list", "-p", "3"]).unwrap();
        cli(&s, &["run", "list", "--plan", "91", "--completed"]).unwrap();
        cli(&s, &["run", "list", "--plan", "91", "--limit", "1"]).unwrap();
        cli(&s, &["--json", "run", "list", "--plan", "91"]).unwrap();
        cli(&s, &["run", "view", "R92"]).unwrap();
        cli(&s, &["--json", "run", "view", "92"]).unwrap();
    }
}
