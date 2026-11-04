//! Adding a result in one call: step outcomes written in Markdown, screenshots uploaded to the run
//! and shown inline, and files attached to the result once it exists.
//!
//! TestRail cannot edit a result after adding it, so images go to the run first. Their IDs are then
//! known when the step results are written, and the result is added once.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::api::Client;
use crate::api::statuses::{self, Statuses};
use crate::attachment;
use crate::case;
use crate::cli::ResultAddArgs;
use crate::context::Ctx;
use crate::ids::{self, Kind};
use crate::markup;
use crate::output::{plural, print_json, text};
use crate::prompt;

/// One step's outcome as the steps file gives it. Content and expected results left out are taken
/// from the case.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct StepInput {
    content: Option<String>,
    expected: Option<String>,
    actual: Option<String>,
    /// Status by name or ID.
    status: Option<Value>,
    status_id: Option<i64>,
}

/// Where the result goes.
struct Target {
    run: u64,
    case: u64,
    test: Option<u64>,
    /// The test, when given, which carries the case's steps.
    test_value: Option<Value>,
}

/// A result ready to send.
struct Prepared {
    endpoint: String,
    payload: Value,
    run: u64,
    test: Option<u64>,
    images: Vec<Value>,
}

pub fn run(ctx: &Ctx, args: &ResultAddArgs) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to read the current directory")?;
    let prepared = prepare(&ctx.client, args, &cwd)?;
    if args.dry_run {
        eprintln!("Dry run: nothing was uploaded or added. Local images show as paths.");
        return print_json(&json!({"endpoint": prepared.endpoint, "result": prepared.payload}));
    }
    let result = ctx.client.post(&prepared.endpoint, &prepared.payload)?;
    let result_id = text(&result["id"]);
    let mut attached = Vec::new();
    for file in &args.attach {
        let id = attachment::upload(
            &ctx.client,
            &format!("add_attachment_to_result/{result_id}"),
            &cwd.join(file),
        )
        .with_context(|| {
            format!(
                "Added result {result_id}, but attaching {} failed",
                file.display()
            )
        })?;
        attached.push(json!({"file": file.display().to_string(), "attachment_id": id}));
    }
    if ctx.json {
        return print_json(&json!({
            "result": result,
            "images": prepared.images,
            "attachments": attached,
        }));
    }
    let test = result["test_id"]
        .as_u64()
        .or(prepared.test)
        .map(|t| t.to_string());
    let status = Statuses::builtin().label(&result["status_id"]);
    match &test {
        Some(t) => {
            println!("✓ Added result {result_id} ({status}) to T{t}");
            println!("  {}", ctx.web_url(&format!("tests/view/{t}")));
        }
        None => println!("✓ Added result {result_id} ({status})"),
    }
    let mut notes = Vec::new();
    if !prepared.images.is_empty() {
        notes.push(format!(
            "{} uploaded to R{}",
            plural(prepared.images.len(), "image"),
            prepared.run
        ));
    }
    if !attached.is_empty() {
        notes.push(format!("{} attached", plural(attached.len(), "file")));
    }
    if !notes.is_empty() {
        println!("  {}", notes.join(", "));
    }
    Ok(())
}

/// Build the result: resolve the target and statuses, read the steps, and upload the images their
/// text names unless this is a dry run. Relative paths resolve against `cwd`, or for images in the
/// steps file, against its folder.
fn prepare(client: &Client, args: &ResultAddArgs, cwd: &Path) -> Result<Prepared> {
    let target = target(client, args)?;
    let mut statuses = StatusCache::new(client);
    let status_id = statuses::resolve(client, &args.status)?;
    let mut images = Images {
        client,
        run: target.run,
        dry_run: args.dry_run,
        uploaded: HashMap::new(),
        log: Vec::new(),
    };

    let mut payload = Map::new();
    payload.insert("status_id".into(), json!(status_id));

    let comment = match (&args.comment, &args.comment_file) {
        (Some(c), _) => Some((c.clone(), cwd.to_path_buf())),
        (None, Some(f)) => {
            let path = cwd.join(f);
            Some((prompt::read_file(&path)?, folder(&path, cwd)))
        }
        (None, None) => None,
    };
    if let Some((comment, base)) = comment {
        let html = convert(&comment, args.raw, &mut |src| images.src(&base, src))?;
        payload.insert("comment".into(), json!(html));
    }
    if let Some(elapsed) = &args.elapsed {
        payload.insert("elapsed".into(), json!(elapsed));
    }
    if let Some(version) = &args.version {
        payload.insert("version".into(), json!(version));
    }
    if !args.defects.is_empty() {
        payload.insert("defects".into(), json!(args.defects.join(",")));
    }

    if let Some(path) = &args.steps {
        let (inputs, base) = load_steps(path, cwd)?;
        let extra = step_images(&args.step_images, inputs.len(), cwd)?;
        let case_steps = if inputs
            .iter()
            .any(|s| s.content.is_none() || s.expected.is_none())
        {
            let source = match &target.test_value {
                Some(t) => t.clone(),
                None => client.get(&format!("get_case/{}", target.case))?,
            };
            separated_steps(&source)
        } else {
            Vec::new()
        };
        let mut steps = Vec::new();
        for (i, input) in inputs.into_iter().enumerate() {
            let from_case = case_steps.get(i);
            let mut keep = |src: &str| Ok(src.to_string());
            let content = match &input.content {
                Some(t) => convert(t, args.raw, &mut |src| images.src(&base, src))?,
                None => match from_case {
                    Some((c, _)) => convert(c, args.raw, &mut keep)?,
                    None => bail!(
                        "Step {} has no content, and C{} has {}. Give each step's content in the steps file.",
                        i + 1,
                        target.case,
                        plural(case_steps.len(), "separated step")
                    ),
                },
            };
            let expected = match &input.expected {
                Some(t) => convert(t, args.raw, &mut |src| images.src(&base, src))?,
                None => match from_case {
                    Some((_, e)) => convert(e, args.raw, &mut keep)?,
                    None => String::new(),
                },
            };
            let expected = if args.raw {
                expected
            } else {
                markup::quote(&expected)
            };
            let mut actual = match &input.actual {
                Some(t) => convert(t, args.raw, &mut |src| images.src(&base, src))?,
                None => String::new(),
            };
            for file in extra.get(&(i + 1)).into_iter().flatten() {
                let src = images.src(cwd, &file.to_string_lossy())?;
                actual.push_str(&format!("<img src=\"{src}\" />"));
            }
            let mut step = json!({"content": content, "expected": expected, "actual": actual});
            if let Some(id) = statuses.step(&input, i + 1)? {
                step["status_id"] = json!(id);
            }
            steps.push(step);
        }
        payload.insert("custom_step_results".into(), Value::Array(steps));
    }

    for field in &args.fields {
        let (name, value) = field
            .split_once('=')
            .with_context(|| format!("--field {field} is not NAME=VALUE"))?;
        let value = serde_json::from_str(value).unwrap_or_else(|_| json!(value));
        payload.insert(name.trim().to_string(), value);
    }

    let endpoint = match target.test {
        Some(t) => format!("add_result/{t}"),
        None => format!("add_result_for_case/{}/{}", target.run, target.case),
    };
    Ok(Prepared {
        endpoint,
        payload: Value::Object(payload),
        run: target.run,
        test: target.test,
        images: images.log,
    })
}

fn target(client: &Client, args: &ResultAddArgs) -> Result<Target> {
    match (&args.test, &args.run, &args.case) {
        (Some(t), _, _) => {
            let id = ids::parse(t, Kind::Test)?;
            let test = client.get(&format!("get_test/{id}"))?;
            Ok(Target {
                run: test["run_id"].as_u64().context("The test has no run_id")?,
                case: test["case_id"]
                    .as_u64()
                    .context("The test has no case_id")?,
                test: Some(id),
                test_value: Some(test),
            })
        }
        (None, Some(r), Some(c)) => Ok(Target {
            run: ids::parse(r, Kind::Run)?,
            case: ids::parse(c, Kind::Case)?,
            test: None,
            test_value: None,
        }),
        _ => unreachable!("clap requires --test, or --run with --case"),
    }
}

/// The folder a file's relative image paths resolve against.
fn folder(path: &Path, cwd: &Path) -> PathBuf {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => cwd.to_path_buf(),
    }
}

/// The steps file's entries, and the folder its image paths resolve against.
fn load_steps(path: &Path, cwd: &Path) -> Result<(Vec<StepInput>, PathBuf)> {
    let (text, base) = if path.as_os_str() == "-" {
        (prompt::read_stdin()?, cwd.to_path_buf())
    } else {
        let full = cwd.join(path);
        (prompt::read_file(&full)?, folder(&full, cwd))
    };
    let steps: Vec<StepInput> = serde_json::from_str(&text).with_context(|| {
        format!(
            "{} is not a JSON array of steps with content, expected, actual, and status",
            path.display()
        )
    })?;
    if steps.is_empty() {
        bail!("{} has no steps", path.display());
    }
    Ok((steps, base))
}

/// `--step-image N=FILE` values, by step number, with paths made absolute.
fn step_images(
    values: &[String],
    steps: usize,
    cwd: &Path,
) -> Result<HashMap<usize, Vec<PathBuf>>> {
    let mut out: HashMap<usize, Vec<PathBuf>> = HashMap::new();
    for value in values {
        let (n, file) = value
            .split_once('=')
            .with_context(|| format!("--step-image {value} is not N=FILE"))?;
        let n: usize = n
            .trim()
            .parse()
            .with_context(|| format!("--step-image {value} does not start with a step number"))?;
        if n == 0 || n > steps {
            bail!(
                "--step-image {value} names step {n}, but the steps file has {}",
                plural(steps, "step")
            );
        }
        out.entry(n).or_default().push(cwd.join(file.trim()));
    }
    Ok(out)
}

/// A case's separated steps. Text-template cases have none to fill from.
fn separated_steps(source: &Value) -> Vec<(String, String)> {
    if source["custom_steps_separated"].is_array() {
        case::steps(source)
    } else {
        Vec::new()
    }
}

/// Text as TestRail's HTML, or as given with `raw`.
fn convert(text: &str, raw: bool, image: &mut dyn FnMut(&str) -> Result<String>) -> Result<String> {
    if raw {
        Ok(text.to_string())
    } else {
        markup::to_html(text, image)
    }
}

/// Uploads each local image once, to the run, and remembers where TestRail serves it.
struct Images<'a> {
    client: &'a Client,
    run: u64,
    dry_run: bool,
    uploaded: HashMap<PathBuf, String>,
    log: Vec<Value>,
}

impl Images<'_> {
    fn src(&mut self, base: &Path, src: &str) -> Result<String> {
        let path = base.join(src);
        if let Some(done) = self.uploaded.get(&path) {
            return Ok(done.clone());
        }
        if !path.is_file() {
            bail!("Image {} does not exist", path.display());
        }
        let served = if self.dry_run {
            path.display().to_string()
        } else {
            let id = attachment::upload(
                self.client,
                &format!("add_attachment_to_run/{}", self.run),
                &path,
            )?;
            self.log
                .push(json!({"file": path.display().to_string(), "attachment_id": id}));
            markup::attachment_src(&id)
        };
        self.uploaded.insert(path, served.clone());
        Ok(served)
    }
}

/// Step statuses, looking up the instance's only for names that are not system ones.
struct StatusCache<'a> {
    client: &'a Client,
    fetched: Option<Statuses>,
}

impl<'a> StatusCache<'a> {
    fn new(client: &'a Client) -> Self {
        Self {
            client,
            fetched: None,
        }
    }

    fn step(&mut self, input: &StepInput, n: usize) -> Result<Option<i64>> {
        match (&input.status, input.status_id) {
            (Some(_), Some(_)) => bail!("Step {n} has both status and status_id"),
            (None, Some(id)) => Ok(Some(id)),
            (None, None) => Ok(None),
            (Some(Value::Number(id)), None) => id
                .as_i64()
                .map(Some)
                .with_context(|| format!("Step {n} has status {id}, which is not an ID")),
            (Some(Value::String(name)), None) => {
                if let Ok(id) = Statuses::builtin().id(name) {
                    return Ok(Some(id));
                }
                if self.fetched.is_none() {
                    self.fetched = Some(Statuses::fetch(self.client)?);
                }
                let statuses = self.fetched.as_ref().expect("fetched above");
                statuses
                    .id(name)
                    .map(Some)
                    .with_context(|| format!("Step {n}"))
            }
            (Some(other), None) => bail!("Step {n} has status {other}, which is not a name or ID"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Matcher, Server};

    fn args() -> ResultAddArgs {
        ResultAddArgs {
            test: None,
            run: Some("R16".into()),
            case: Some("C78".into()),
            status: "passed".into(),
            comment: None,
            comment_file: None,
            elapsed: None,
            version: None,
            defects: Vec::new(),
            steps: None,
            step_images: Vec::new(),
            attach: Vec::new(),
            fields: Vec::new(),
            raw: false,
            dry_run: false,
        }
    }

    #[test]
    fn plain_results_need_no_lookups() {
        let server = Server::new();
        let client = Client::for_tests(&server.url());
        let mut a = args();
        a.comment = Some("All steps **verified**".into());
        a.elapsed = Some("1m 30s".into());
        a.version = Some("2.1.0".into());
        a.defects = vec!["BUG-1".into(), "BUG-2".into()];
        a.fields = vec!["custom_env=staging".into(), "custom_count=3".into()];
        let p = prepare(&client, &a, Path::new("/")).unwrap();
        assert_eq!(p.endpoint, "add_result_for_case/16/78");
        assert_eq!(
            p.payload,
            json!({
                "status_id": 1,
                "comment": "<p>All steps <strong>verified</strong></p>",
                "elapsed": "1m 30s",
                "version": "2.1.0",
                "defects": "BUG-1,BUG-2",
                "custom_env": "staging",
                "custom_count": 3
            })
        );
    }

    #[test]
    fn steps_fill_from_the_case_and_upload_images_once() {
        let mut server = Server::new();
        server
            .mock("GET", "/index.php?/api/v2/get_case/78")
            .with_body(
                json!({"custom_steps_separated": [
                    {"content": "Open the login page", "expected": "Fields show"},
                    {"content": "<p>Click Submit</p>", "expected": "A dialog appears"}
                ]})
                .to_string(),
            )
            .create();
        let upload = server
            .mock("POST", "/index.php?/api/v2/add_attachment_to_run/16")
            .match_body(Matcher::Regex("filename=\"s1.png\"".into()))
            .with_body(r#"{"attachment_id": 55}"#)
            .expect(1)
            .create();
        server
            .mock("POST", "/index.php?/api/v2/add_attachment_to_run/16")
            .match_body(Matcher::Regex("filename=\"s2.png\"".into()))
            .with_body(r#"{"attachment_id": 56}"#)
            .expect(1)
            .create();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("shots")).unwrap();
        std::fs::write(dir.path().join("shots/s1.png"), "1").unwrap();
        std::fs::write(dir.path().join("s2.png"), "2").unwrap();
        std::fs::write(
            dir.path().join("shots/steps.json"),
            json!([
                {"status": "passed", "actual": "Fields showed.\n\n![](s1.png)"},
                {"status": 5, "actual": "No dialog.\n\n![](s1.png)", "expected": "A *dialog*"}
            ])
            .to_string(),
        )
        .unwrap();

        let client = Client::for_tests(&server.url());
        let mut a = args();
        a.status = "failed".into();
        a.steps = Some("shots/steps.json".into());
        a.step_images = vec!["2=s2.png".into()];
        let p = prepare(&client, &a, dir.path()).unwrap();
        upload.assert();
        assert_eq!(p.images.len(), 2);
        assert_eq!(
            p.payload["custom_step_results"],
            json!([
                {
                    "content": "<p>Open the login page</p>",
                    "expected": "<blockquote><p>Fields show</p></blockquote>",
                    "actual": "<p>Fields showed.</p><img src=\"index.php?/attachments/get/55\" alt=\"\" />",
                    "status_id": 1
                },
                {
                    "content": "<p>Click Submit</p>",
                    "expected": "<blockquote><p>A <em>dialog</em></p></blockquote>",
                    "actual": "<p>No dialog.</p><img src=\"index.php?/attachments/get/55\" alt=\"\" /><img src=\"index.php?/attachments/get/56\" />",
                    "status_id": 5
                }
            ])
        );
        assert_eq!(p.payload["status_id"], 5);
    }

    #[test]
    fn tests_route_to_add_result_and_carry_their_steps() {
        let mut server = Server::new();
        let get_test = server
            .mock("GET", "/index.php?/api/v2/get_test/1001")
            .with_body(
                json!({"id": 1001, "run_id": 16, "case_id": 78,
                       "custom_steps_separated": [{"content": "Open", "expected": ""}]})
                .to_string(),
            )
            .expect(1)
            .create();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("steps.json"), r#"[{"status": "passed"}]"#).unwrap();
        let client = Client::for_tests(&server.url());
        let mut a = args();
        a.run = None;
        a.case = None;
        a.test = Some("T1001".into());
        a.steps = Some("steps.json".into());
        let p = prepare(&client, &a, dir.path()).unwrap();
        get_test.assert();
        assert_eq!(p.endpoint, "add_result/1001");
        assert_eq!(
            p.payload["custom_step_results"],
            json!([{"content": "<p>Open</p>", "expected": "", "actual": "", "status_id": 1}])
        );
    }

    #[test]
    fn dry_runs_upload_nothing() {
        let server = Server::new();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("s1.png"), "1").unwrap();
        std::fs::write(
            dir.path().join("steps.json"),
            json!([{"content": "Open", "expected": "Loads", "actual": "![](s1.png)", "status": "passed"}])
                .to_string(),
        )
        .unwrap();
        let client = Client::for_tests(&server.url());
        let mut a = args();
        a.dry_run = true;
        a.steps = Some("steps.json".into());
        let p = prepare(&client, &a, dir.path()).unwrap();
        let actual = text(&p.payload["custom_step_results"][0]["actual"]);
        assert!(
            actual.contains(&dir.path().join("s1.png").display().to_string()),
            "{actual}"
        );
        assert!(p.images.is_empty());
    }

    #[test]
    fn raw_text_is_sent_as_given() {
        let server = Server::new();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("steps.json"),
            r#"[{"content": "**Open**", "expected": "Loads", "actual": "ok", "status_id": 1}]"#,
        )
        .unwrap();
        let client = Client::for_tests(&server.url());
        let mut a = args();
        a.raw = true;
        a.comment = Some("**as is**".into());
        a.steps = Some("steps.json".into());
        let p = prepare(&client, &a, dir.path()).unwrap();
        assert_eq!(p.payload["comment"], "**as is**");
        assert_eq!(
            p.payload["custom_step_results"],
            json!([{"content": "**Open**", "expected": "Loads", "actual": "ok", "status_id": 1}])
        );
    }

    #[test]
    fn mistakes_in_steps_are_reported() {
        let mut server = Server::new();
        server
            .mock("GET", "/index.php?/api/v2/get_case/78")
            .with_body(r#"{"custom_steps": "one block"}"#)
            .create();
        let dir = tempfile::tempdir().unwrap();
        let client = Client::for_tests(&server.url());
        let attempt = |steps: &str, images: Vec<String>| {
            std::fs::write(dir.path().join("steps.json"), steps).unwrap();
            let mut a = args();
            a.steps = Some("steps.json".into());
            a.step_images = images;
            prepare(&client, &a, dir.path()).err().unwrap().to_string()
        };
        assert!(attempt(r#"[{"actual_result": "x"}]"#, vec![]).contains("is not a JSON array"));
        assert_eq!(attempt("[]", vec![]), "steps.json has no steps");
        assert_eq!(
            attempt(r#"[{"status": "passed"}]"#, vec![]),
            "Step 1 has no content, and C78 has 0 separated steps. Give each step's content in the steps file."
        );
        let full = r#"[{"content": "a", "expected": "b", "status": "passed", "status_id": 1}]"#;
        assert_eq!(
            attempt(full, vec![]),
            "Step 1 has both status and status_id"
        );
        let ok = r#"[{"content": "a", "expected": "b", "actual": "![](missing.png)"}]"#;
        assert!(attempt(ok, vec![]).starts_with("Image "), "missing image");
        let ok = r#"[{"content": "a", "expected": "b"}]"#;
        assert_eq!(
            attempt(ok, vec!["2=x.png".into()]),
            "--step-image 2=x.png names step 2, but the steps file has 1 step"
        );
    }
}
