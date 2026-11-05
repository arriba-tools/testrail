use anyhow::{Result, bail};
use serde_json::Value;

use crate::api::{self, Client, with_params};
use crate::cli::{ProjectCommand, StateArgs};
use crate::context::Ctx;
use crate::ids::{self, Kind};
use crate::output::{mark_completed, print_json, print_rich, print_table, state_row, table, text};

pub fn run(ctx: &Ctx, cmd: ProjectCommand) -> Result<()> {
    match cmd {
        ProjectCommand::List(state) => list(ctx, &state),
        ProjectCommand::View(args) => view(ctx, &args.project),
    }
}

/// A completion filter as the query parameter TestRail takes.
pub fn completed_param(state: &StateArgs) -> Vec<(&'static str, String)> {
    state
        .is_completed()
        .map(|c| vec![("is_completed", u8::from(c).to_string())])
        .unwrap_or_default()
}

fn list(ctx: &Ctx, state: &StateArgs) -> Result<()> {
    let endpoint = with_params("get_projects", &completed_param(state));
    let projects = api::list(&ctx.client, &endpoint, "projects", None)?.items;
    if ctx.json {
        return print_json(&projects);
    }
    let rows: Vec<Vec<String>> = projects
        .iter()
        .map(|p| {
            vec![
                text(&p["id"]),
                suite_mode(&p["suite_mode"]).to_string(),
                mark_completed(text(&p["name"]), p),
            ]
        })
        .collect();
    print_table(&rows, "No projects found.");
    Ok(())
}

fn suite_mode(v: &Value) -> &'static str {
    match v.as_i64() {
        Some(1) => "single suite",
        Some(2) => "baselines",
        Some(3) => "multiple suites",
        _ => "",
    }
}

fn view(ctx: &Ctx, project: &str) -> Result<()> {
    let id = resolve(&ctx.client, project)?;
    let p = ctx.client.get(&format!("get_project/{id}"))?;
    if ctx.json {
        return print_json(&p);
    }
    let rows = vec![
        vec![
            "Suites".to_string(),
            suite_mode(&p["suite_mode"]).to_string(),
        ],
        state_row(&p),
    ];
    println!("{id}  {}", text(&p["name"]));
    println!("{}", ctx.web_url(&format!("projects/overview/{id}")));
    println!("{}", table(&rows, "  "));
    print_rich(&p["announcement"]);
    Ok(())
}

/// A project's ID from its ID, its URL, or its name, matched whole and then in part, ignoring case.
pub fn resolve(client: &Client, text: &str) -> Result<u64> {
    if let Ok(id) = ids::parse(text, Kind::Project) {
        return Ok(id);
    }
    let projects = api::list(client, "get_projects", "projects", None)?.items;
    pick(&projects, text)
}

fn pick(projects: &[Value], typed: &str) -> Result<u64> {
    let typed = typed.trim();
    let wanted = typed.to_lowercase();
    let name = |p: &Value| p["name"].as_str().unwrap_or("").to_lowercase();
    let id = |p: &Value| p["id"].as_u64().unwrap_or(0);
    if let Some(p) = projects.iter().find(|p| name(p) == wanted) {
        return Ok(id(p));
    }
    let partial: Vec<&Value> = projects
        .iter()
        .filter(|p| name(p).contains(&wanted))
        .collect();
    match partial.as_slice() {
        [one] => Ok(id(one)),
        [] => {
            bail!("No project is named {typed:?}. Run `testrail-arriba project list` to see them.")
        }
        many => bail!(
            "Several projects match {typed:?}: {}",
            many.iter()
                .map(|p| format!("{} ({})", text(&p["name"]), id(p)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;
    use serde_json::json;

    #[test]
    fn names_match_whole_then_in_part() {
        let projects = vec![
            json!({"id": 1, "name": "Acme"}),
            json!({"id": 2, "name": "Acme Mobile"}),
            json!({"id": 3, "name": "Web"}),
        ];
        assert_eq!(pick(&projects, "acme").unwrap(), 1);
        assert_eq!(pick(&projects, "mobile").unwrap(), 2);
        let err = pick(&projects, "acm").unwrap_err().to_string();
        assert!(err.contains("Acme (1), Acme Mobile (2)"), "{err}");
        assert!(pick(&projects, "nope").is_err());
    }

    #[test]
    fn ids_skip_the_lookup_and_names_use_it() {
        let mut server = Server::new();
        let mock = server
            .mock("GET", "/index.php?/api/v2/get_projects&limit=250&offset=0")
            .with_body(
                json!({"projects": [{"id": 2, "name": "Acme"}], "_links": {"next": null}})
                    .to_string(),
            )
            .expect(1)
            .create();
        let client = Client::for_tests(&server.url());
        assert_eq!(resolve(&client, "7").unwrap(), 7);
        assert_eq!(resolve(&client, "Acme").unwrap(), 2);
        mock.assert();
    }

    #[test]
    fn completion_filters_become_parameters() {
        let state = StateArgs {
            active: true,
            completed: false,
        };
        assert_eq!(
            completed_param(&state),
            vec![("is_completed", "0".to_string())]
        );
    }

    #[test]
    fn projects_list_and_view_through_the_cli() {
        use crate::testing::{cli, get, page};
        let mut s = Server::new();
        get(
            &mut s,
            "get_projects&is_completed=0&limit=250&offset=0",
            page(
                "projects",
                json!([
                    {"id": 2, "name": "Acme", "suite_mode": 1, "is_completed": false},
                    {"id": 3, "name": "Old", "suite_mode": 3, "is_completed": true}
                ]),
            ),
        );
        get(
            &mut s,
            "get_projects&limit=250&offset=0",
            page("projects", json!([])),
        );
        get(
            &mut s,
            "get_project/2",
            json!({"id": 2, "name": "Acme", "suite_mode": 2, "is_completed": true,
                   "announcement": "<p>Read me</p>"}),
        );
        cli(&s, &["project", "list", "--active"]).unwrap();
        cli(&s, &["project", "list"]).unwrap();
        cli(&s, &["--json", "project", "list"]).unwrap();
        cli(&s, &["project", "view", "2"]).unwrap();
        cli(&s, &["--json", "project", "view", "2"]).unwrap();
        let err = cli(&s, &["project", "view", "Nope"])
            .unwrap_err()
            .to_string();
        assert!(err.starts_with("No project is named \"Nope\""), "{err}");
    }
}
