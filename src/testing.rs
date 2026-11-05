//! Helpers for tests that drive whole commands against a mock TestRail.

use anyhow::Result;
use clap::Parser;
use mockito::{Mock, Server};
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::context::Ctx;

/// Run a command line against `server`, as `main` does once it has a login.
pub fn cli(server: &Server, args: &[&str]) -> Result<()> {
    let cli = Cli::try_parse_from(std::iter::once("testrail-arriba").chain(args.iter().copied()))?;
    let mut ctx = Ctx::for_tests(&server.url());
    ctx.json = cli.json;
    crate::dispatch(&ctx, cli.command)
}

/// Answer one GET of `endpoint` with `body`.
pub fn get(server: &mut Server, endpoint: &str, body: Value) -> Mock {
    answer(server, "GET", endpoint, body)
}

/// Answer one POST to `endpoint` with `body`.
pub fn post(server: &mut Server, endpoint: &str, body: Value) -> Mock {
    answer(server, "POST", endpoint, body)
}

fn answer(server: &mut Server, method: &str, endpoint: &str, body: Value) -> Mock {
    server
        .mock(method, format!("/index.php?/api/v2/{endpoint}").as_str())
        .with_body(body.to_string())
        .create()
}

/// A bulk endpoint's only page.
pub fn page(key: &str, items: Value) -> Value {
    json!({ key: items, "_links": {"next": null} })
}

/// The system statuses and one custom status, as `get_statuses` lists them.
pub fn statuses(server: &mut Server) -> Mock {
    get(
        server,
        "get_statuses",
        json!([
            {"id": 1, "name": "passed", "label": "Passed", "is_final": true},
            {"id": 2, "name": "blocked", "label": "Blocked", "is_final": true},
            {"id": 3, "name": "untested", "label": "Untested", "is_untested": true},
            {"id": 4, "name": "retest", "label": "Retest"},
            {"id": 5, "name": "failed", "label": "Failed", "is_final": true},
            {"id": 6, "name": "not_applicable", "label": "Not Applicable"}
        ]),
    )
}
