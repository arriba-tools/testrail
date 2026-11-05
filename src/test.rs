use anyhow::Result;
use serde_json::Value;

use crate::api::statuses::Statuses;
use crate::api::{self, Client, with_params};
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
    let statuses = Statuses::fetch(&ctx.client)?;
    let (tests, more) = gather(&ctx.client, args, &statuses)?;
    if ctx.json {
        let items: Vec<&Value> = tests.iter().map(|(t, _)| t).collect();
        return print_json(&items);
    }
    let rows: Vec<Vec<String>> = tests
        .iter()
        .map(|(t, run)| {
            let mut row = vec![
                format!("T{}", text(&t["id"])),
                statuses.label(&t["status_id"]),
                format!("C{}", text(&t["case_id"])),
            ];
            if let Some(run) = run {
                row.push(run.clone());
            }
            row.push(text(&t["title"]));
            row
        })
        .collect();
    print_table(&rows, "No tests found.");
    note_more(more, tests.len());
    Ok(())
}

/// A test, with its run's label when the list spans a plan's runs.
type Listed = (Value, Option<String>);

/// The tests `args` asks for, and whether more remained past the limit.
fn gather(
    client: &Client,
    args: &TestListArgs,
    statuses: &Statuses,
) -> Result<(Vec<Listed>, bool)> {
    let mut params = Vec::new();
    if !args.status.is_empty() {
        let wanted = args
            .status
            .iter()
            .map(|s| statuses.id(s).map(|id| id.to_string()))
            .collect::<Result<Vec<_>>>()?;
        params.push(("status_id", wanted.join(",")));
    }
    let case = args
        .case
        .as_deref()
        .map(|c| ids::parse(c, Kind::Case))
        .transpose()?;
    let runs: Vec<(u64, Option<String>)> = match (&args.run, &args.plan) {
        (Some(r), _) => vec![(ids::parse(r, Kind::Run)?, None)],
        (None, Some(p)) => {
            let id = ids::parse(p, Kind::Plan)?;
            plan::runs(&client.get(&format!("get_plan/{id}"))?)
                .iter()
                .filter_map(|r| Some((r["id"].as_u64()?, Some(run_label(r)))))
                .collect()
        }
        (None, None) => unreachable!("clap requires a run or --plan"),
    };
    let limit = args.limit.get();
    // A case filter runs here, and a plan spans runs, so either reads every test before the limit.
    let whole = case.is_some() || args.plan.is_some();
    let mut tests = Vec::new();
    let mut more = false;
    for (run, label) in runs {
        let endpoint = with_params(&format!("get_tests/{run}"), &params);
        let page = api::list(client, &endpoint, "tests", if whole { None } else { limit })?;
        more |= page.more;
        tests.extend(
            page.items
                .into_iter()
                .filter(|t| case.is_none_or(|c| t["case_id"].as_u64() == Some(c)))
                .map(|t| (t, label.clone())),
        );
    }
    if whole {
        more = api::cut(&mut tests, limit);
    }
    Ok((tests, more))
}

/// A run as a plan's test list names it: its ID and, when it has one, its configuration.
fn run_label(run: &Value) -> String {
    match run["config"].as_str().filter(|c| !c.is_empty()) {
        Some(config) => format!("R{} {config}", text(&run["id"])),
        None => format!("R{}", text(&run["id"])),
    }
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
    use mockito::Server;
    use serde_json::json;

    fn list_args(run: Option<&str>, plan: Option<&str>, case: Option<&str>) -> TestListArgs {
        TestListArgs {
            run: run.map(str::to_string),
            plan: plan.map(str::to_string),
            case: case.map(str::to_string),
            status: Vec::new(),
            limit: crate::cli::LimitArgs {
                limit: 50,
                all: false,
            },
        }
    }

    #[test]
    fn plans_find_a_case_across_their_runs() {
        let mut server = Server::new();
        server
            .mock("GET", "/index.php?/api/v2/get_plan/91")
            .with_body(
                json!({"entries": [{"id": "e", "runs": [
                    {"id": 92, "config": "Windows"}, {"id": 93, "config": "macOS ARM"}
                ]}]})
                .to_string(),
            )
            .create();
        for (run, tests) in [
            (
                92,
                json!([{"id": 1782, "case_id": 52}, {"id": 1783, "case_id": 53}]),
            ),
            (93, json!([{"id": 1900, "case_id": 52}])),
        ] {
            server
                .mock(
                    "GET",
                    format!("/index.php?/api/v2/get_tests/{run}&limit=250&offset=0").as_str(),
                )
                .with_body(json!({"tests": tests, "_links": {"next": null}}).to_string())
                .create();
        }
        let client = Client::for_tests(&server.url());
        let statuses = Statuses::builtin();
        let (tests, more) = gather(
            &client,
            &list_args(None, Some("91"), Some("C52")),
            &statuses,
        )
        .unwrap();
        assert!(!more);
        let found: Vec<(u64, String)> = tests
            .iter()
            .map(|(t, run)| (t["id"].as_u64().unwrap(), run.clone().unwrap()))
            .collect();
        assert_eq!(
            found,
            vec![
                (1782, "R92 Windows".to_string()),
                (1900, "R93 macOS ARM".to_string())
            ]
        );
    }

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

    #[test]
    fn tests_list_and_view_through_the_cli() {
        use crate::testing::{cli, get, page, statuses};
        let mut s = Server::new();
        statuses(&mut s);
        get(
            &mut s,
            "get_tests/92&status_id=5,6&limit=50&offset=0",
            page(
                "tests",
                json!([{"id": 1789, "status_id": 5, "case_id": 66, "title": "Sentry"}]),
            ),
        );
        get(
            &mut s,
            "get_test/1782",
            json!({"id": 1782, "run_id": 92, "case_id": 52, "status_id": 3, "title": "Login",
                   "custom_steps_separated": [{"content": "<p>Open</p>",
                       "expected": "<p><img src=\"index.php?/attachments/get/7\"></p>"}]}),
        );
        get(
            &mut s,
            "get_run/92",
            json!({"id": 92, "name": "Win", "config": "Windows"}),
        );
        get(
            &mut s,
            "get_results/1782&limit=5&offset=0",
            page(
                "results",
                json!([{"id": 2064, "status_id": 1, "created_on": 1762000000,
                        "version": "2.4.11", "comment": "ok",
                        "custom_step_results": [{"content": "<p>Open</p>", "status_id": "1"}]}]),
            ),
        );
        cli(
            &s,
            &["test", "list", "R92", "--status", "failed,not_applicable"],
        )
        .unwrap();
        cli(
            &s,
            &["--json", "test", "list", "R92", "-s", "failed", "-s", "6"],
        )
        .unwrap();
        cli(&s, &["test", "view", "T1782"]).unwrap();
        cli(&s, &["--json", "test", "view", "1782"]).unwrap();
        let err = cli(&s, &["test", "list", "R92", "--status", "bogus"]).unwrap_err();
        assert!(
            err.to_string().starts_with("No status named \"bogus\""),
            "{err}"
        );
    }
}
