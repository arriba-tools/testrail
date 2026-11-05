//! The `api` command: any endpoint, with the login and paths filled in.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::path::Path;

use crate::cli::ApiArgs;
use crate::context::Ctx;
use crate::output::print_json;
use crate::prompt;

pub fn run(ctx: &Ctx, args: ApiArgs) -> Result<()> {
    let body = args.data.as_deref().map(read_data).transpose()?;
    let method = match &args.method {
        Some(m) => m.to_uppercase(),
        None if body.is_some() => "POST".to_string(),
        None => "GET".to_string(),
    };
    let value = match method.as_str() {
        "GET" => {
            if body.is_some() {
                bail!("GET requests take no --data");
            }
            ctx.client.get(&args.endpoint)?
        }
        "POST" => ctx.client.post(
            &args.endpoint,
            &body.unwrap_or_else(|| Value::Object(Default::default())),
        )?,
        other => bail!("TestRail's API takes GET and POST, not {other}"),
    };
    print_json(&value)
}

/// A JSON body from text, `@FILE`, or `-` for stdin.
fn read_data(data: &str) -> Result<Value> {
    let text = match data {
        "-" => prompt::read_stdin()?,
        d if d.starts_with('@') => prompt::read_file(Path::new(&d[1..]))?,
        d => d.to_string(),
    };
    serde_json::from_str(&text).context("--data is not valid JSON")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Matcher, Server};
    use serde_json::json;

    #[test]
    fn data_reads_from_text_and_files() {
        assert_eq!(read_data(r#"{"a": 1}"#).unwrap(), json!({"a": 1}));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("body.json");
        std::fs::write(&path, "[1]").unwrap();
        assert_eq!(
            read_data(&format!("@{}", path.display())).unwrap(),
            json!([1])
        );
        assert!(read_data("{nope").is_err());
    }

    #[test]
    fn data_implies_post() {
        let mut server = Server::new();
        let mock = server
            .mock("POST", "/index.php?/api/v2/close_run/5")
            .match_body(Matcher::Json(json!({})))
            .with_body("{}")
            .create();
        let ctx = Ctx::for_tests(&server.url());
        run(
            &ctx,
            ApiArgs {
                endpoint: "close_run/5".into(),
                method: None,
                data: Some("{}".into()),
            },
        )
        .unwrap();
        mock.assert();
        let err = run(
            &ctx,
            ApiArgs {
                endpoint: "x".into(),
                method: Some("delete".into()),
                data: None,
            },
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "TestRail's API takes GET and POST, not DELETE"
        );
    }

    #[test]
    fn gets_print_and_refuse_a_body() {
        use crate::testing::{cli, get};
        let mut s = Server::new();
        let fields = get(
            &mut s,
            "get_case_fields",
            json!([{"system_name": "custom_steps"}]),
        );
        cli(&s, &["api", "get_case_fields"]).unwrap();
        fields.assert();
        let err = cli(&s, &["api", "get_case_fields", "-X", "get", "-d", "{}"]).unwrap_err();
        assert_eq!(err.to_string(), "GET requests take no --data");
    }
}
