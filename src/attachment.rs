use anyhow::{Context, Result, bail};
use regex::Regex;
use reqwest::header::CONTENT_DISPOSITION;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::api::{self, Client, id_text};
use crate::cli::{
    AttachmentAddArgs, AttachmentCommand, AttachmentDeleteArgs, AttachmentDownloadArgs, HolderArgs,
};
use crate::context::Ctx;
use crate::ids::{self, Kind};
use crate::markup::attachment_src;
use crate::output::{date, plural, print_json, print_table, size, table, text};
use crate::prompt;

pub fn run(ctx: &Ctx, cmd: AttachmentCommand) -> Result<()> {
    match cmd {
        AttachmentCommand::List(args) => list(ctx, &args.holder),
        AttachmentCommand::Add(args) => add(ctx, &args),
        AttachmentCommand::Download(args) => download(ctx, &args),
        AttachmentCommand::Delete(args) => delete(ctx, &args),
    }
}

/// What holds attachments: the endpoint suffix for it, as in `case/39`, and its name, as in C39.
struct Holder {
    path: String,
    name: String,
}

fn holder(args: &HolderArgs) -> Result<Option<Holder>> {
    let found = if let Some(c) = &args.case {
        let id = ids::parse(c, Kind::Case)?;
        Some((format!("case/{id}"), format!("C{id}")))
    } else if let Some(r) = &args.run {
        let id = ids::parse(r, Kind::Run)?;
        Some((format!("run/{id}"), format!("R{id}")))
    } else if let Some(t) = &args.test {
        let id = ids::parse(t, Kind::Test)?;
        Some((format!("test/{id}"), format!("T{id}")))
    } else if let Some(p) = &args.plan {
        let id = ids::parse(p, Kind::Plan)?;
        Some((format!("plan/{id}"), format!("plan {id}")))
    } else {
        None
    };
    Ok(found.map(|(path, name)| Holder { path, name }))
}

/// The attachments `holder` carries.
fn attachments(client: &Client, holder: &Holder) -> Result<Vec<Value>> {
    let endpoint = format!("get_attachments_for_{}", holder.path);
    Ok(api::list(client, &endpoint, "attachments", None)?.items)
}

/// An attachment's ID, which newer servers send as `id` and older ones as `attachment_id`.
fn attachment_id(v: &Value) -> String {
    let id = id_text(&v["id"]);
    if id.is_empty() {
        id_text(&v["attachment_id"])
    } else {
        id
    }
}

fn list(ctx: &Ctx, args: &HolderArgs) -> Result<()> {
    let holder = holder(args)?.context("Pass --case, --run, --test, or --plan")?;
    let items = attachments(&ctx.client, &holder)?;
    if ctx.json {
        return print_json(&items);
    }
    let rows: Vec<Vec<String>> = items
        .iter()
        .map(|a| {
            vec![
                attachment_id(a),
                a["size"].as_u64().map(size).unwrap_or_default(),
                date(&a["created_on"]),
                text(&a["name"]),
            ]
        })
        .collect();
    print_table(&rows, &format!("No attachments on {}.", holder.name));
    Ok(())
}

/// Upload `file` to the endpoint, returning the attachment's ID.
pub fn upload(client: &Client, endpoint: &str, file: &Path) -> Result<String> {
    let v = client.upload(endpoint, file)?;
    let id = id_text(&v["attachment_id"]);
    if id.is_empty() {
        bail!("TestRail did not return an ID for {}", file.display());
    }
    Ok(id)
}

fn add(ctx: &Ctx, args: &AttachmentAddArgs) -> Result<()> {
    let (endpoint, name) = if let Some(r) = &args.run {
        let id = ids::parse(r, Kind::Run)?;
        (format!("add_attachment_to_run/{id}"), format!("R{id}"))
    } else if let Some(r) = &args.result {
        let id = ids::parse(r, Kind::Result)?;
        (
            format!("add_attachment_to_result/{id}"),
            format!("result {id}"),
        )
    } else if let Some(c) = &args.case {
        let id = ids::parse(c, Kind::Case)?;
        (format!("add_attachment_to_case/{id}"), format!("C{id}"))
    } else if let Some(p) = &args.plan {
        let id = ids::parse(p, Kind::Plan)?;
        (format!("add_attachment_to_plan/{id}"), format!("plan {id}"))
    } else {
        unreachable!("clap requires a target")
    };
    let mut added = Vec::new();
    for file in &args.files {
        let id = upload(&ctx.client, &endpoint, file)?;
        added.push(json!({
            "file": file.display().to_string(),
            "attachment_id": id,
            "src": attachment_src(&id),
        }));
    }
    if ctx.json {
        return print_json(&added);
    }
    println!("✓ Attached {} to {name}", plural(added.len(), "file"));
    let rows: Vec<Vec<String>> = added
        .iter()
        .map(|a| vec![text(&a["attachment_id"]), text(&a["file"]), text(&a["src"])])
        .collect();
    println!("{}", table(&rows, "  "));
    Ok(())
}

fn download(ctx: &Ctx, args: &AttachmentDownloadArgs) -> Result<()> {
    let holder = holder(&args.holder)?;
    let (wanted, folder): (Vec<(String, Option<String>)>, String) =
        match (&holder, args.ids.is_empty()) {
            (Some(_), false) => {
                bail!("Pass attachment IDs or a --case, --run, --test, or --plan, not both")
            }
            (None, true) => {
                bail!("Pass attachment IDs, or --case, --run, --test, or --plan for all of theirs")
            }
            (None, false) => (
                args.ids.iter().map(|id| (id.clone(), None)).collect(),
                "attachments".to_string(),
            ),
            (Some(h), true) => (
                attachments(&ctx.client, h)?
                    .iter()
                    .map(|a| (attachment_id(a), a["name"].as_str().map(str::to_string)))
                    .collect(),
                h.name.replace(' ', "-"),
            ),
        };
    let dir = args
        .dir
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join("testrail-arriba").join(folder));
    std::fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    let mut taken = HashSet::new();
    let mut saved = Vec::new();
    for (id, name) in wanted {
        let (path, bytes) = fetch(&ctx.client, &id, name.as_deref(), &dir, &mut taken)?;
        saved.push(json!({"id": id, "path": path.display().to_string(), "size": bytes}));
    }
    if ctx.json {
        return print_json(&saved);
    }
    if saved.is_empty() {
        println!("No attachments to download.");
        return Ok(());
    }
    println!(
        "✓ Downloaded {} to {}",
        plural(saved.len(), "file"),
        dir.display()
    );
    let rows: Vec<Vec<String>> = saved
        .iter()
        .map(|s| {
            vec![
                text(&s["id"]),
                size(s["size"].as_u64().unwrap_or(0)),
                text(&s["path"]),
            ]
        })
        .collect();
    println!("{}", table(&rows, "  "));
    Ok(())
}

/// Save attachment `id` into `dir` under `name`, the server's name for it, or its ID, kept apart
/// from the names already `taken`.
fn fetch(
    client: &Client,
    id: &str,
    name: Option<&str>,
    dir: &Path,
    taken: &mut HashSet<String>,
) -> Result<(PathBuf, u64)> {
    let mut resp = client.fetch(&format!("get_attachment/{id}"))?;
    let served = resp
        .headers()
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(disposition_name);
    let name = name.map(str::to_string).or(served);
    // Pasted screenshots are all named image.png, so the ID leads to tell them apart.
    let mut file = format!("{id}-{}", safe_name(name.as_deref().unwrap_or(""), id));
    if !taken.insert(file.clone()) {
        file = with_suffix(&file, id);
        taken.insert(file.clone());
    }
    let path = dir.join(&file);
    let mut out = std::fs::File::create(&path)
        .with_context(|| format!("Failed to create {}", path.display()))?;
    let bytes = std::io::copy(&mut resp, &mut out)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok((path, bytes))
}

static DISPOSITION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)filename\*\s*=\s*(?:UTF-8'[^']*')?([^;]+)|filename\s*=\s*"?([^";]+)"?"#)
        .unwrap()
});

/// The file name in a Content-Disposition header, preferring the encoded `filename*` form.
fn disposition_name(header: &str) -> Option<String> {
    let mut plain = None;
    for caps in DISPOSITION.captures_iter(header) {
        if let Some(encoded) = caps.get(1) {
            return Some(percent_decode(encoded.as_str().trim().trim_matches('"')));
        }
        if plain.is_none() {
            plain = caps.get(2).map(|m| m.as_str().trim().to_string());
        }
    }
    plain
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = text.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A name that stays inside the download folder, or one made from the ID.
fn safe_name(name: &str, id: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = base
        .chars()
        .map(|c| if c.is_control() || c == ':' { '_' } else { c })
        .collect();
    let clean = clean.trim().trim_start_matches('.').to_string();
    if clean.is_empty() {
        format!("attachment-{id}")
    } else {
        clean
    }
}

/// `shot.png` as `shot-55.png`.
fn with_suffix(name: &str, id: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem}-{id}.{ext}"),
        _ => format!("{name}-{id}"),
    }
}

fn delete(ctx: &Ctx, args: &AttachmentDeleteArgs) -> Result<()> {
    let question = format!("Delete {}?", plural(args.ids.len(), "attachment"));
    if !prompt::confirm(&question, ctx.yes)? {
        bail!("Cancelled");
    }
    for id in &args.ids {
        ctx.client
            .post(&format!("delete_attachment/{id}"), &json!({}))?;
    }
    if ctx.json {
        return print_json(&json!({"deleted": args.ids}));
    }
    println!("✓ Deleted {}", plural(args.ids.len(), "attachment"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[test]
    fn disposition_names_parse() {
        assert_eq!(
            disposition_name(r#"attachment; filename="shot 1.png""#).as_deref(),
            Some("shot 1.png")
        );
        assert_eq!(
            disposition_name("attachment; filename=a.log").as_deref(),
            Some("a.log")
        );
        assert_eq!(
            disposition_name(r#"attachment; filename="a.png"; filename*=UTF-8''caf%C3%A9.png"#)
                .as_deref(),
            Some("café.png")
        );
        assert!(disposition_name("inline").is_none());
    }

    #[test]
    fn names_stay_in_the_folder() {
        assert_eq!(safe_name("../../etc/passwd", "5"), "passwd");
        assert_eq!(safe_name("C:\\x\\a:b.png", "5"), "a_b.png");
        assert_eq!(safe_name("", "5"), "attachment-5");
        assert_eq!(safe_name("..", "5"), "attachment-5");
        assert_eq!(with_suffix("shot.png", "55"), "shot-55.png");
        assert_eq!(with_suffix("README", "55"), "README-55");
    }

    #[test]
    fn holders_download_every_attachment() {
        let mut server = Server::new();
        server
            .mock("GET", "/index.php?/api/v2/get_attachments_for_run/16&limit=250&offset=0")
            .with_body(
                r#"{"attachments": [{"id": "a1", "name": "shot.png"}, {"id": 2, "name": "shot.png"}], "_links": {"next": null}}"#,
            )
            .create();
        server
            .mock("GET", "/index.php?/api/v2/get_attachment/a1")
            .with_body("one")
            .create();
        server
            .mock("GET", "/index.php?/api/v2/get_attachment/2")
            .with_body("two")
            .create();
        let ctx = Ctx::for_tests(&server.url());
        let dir = tempfile::tempdir().unwrap();
        download(
            &ctx,
            &AttachmentDownloadArgs {
                ids: Vec::new(),
                holder: HolderArgs {
                    case: None,
                    run: Some("R16".into()),
                    test: None,
                    plan: None,
                },
                dir: Some(dir.path().to_path_buf()),
            },
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a1-shot.png")).unwrap(),
            "one"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("2-shot.png")).unwrap(),
            "two"
        );
    }

    #[test]
    fn ids_download_under_the_served_name() {
        let mut server = Server::new();
        server
            .mock("GET", "/index.php?/api/v2/get_attachment/9")
            .with_header("content-disposition", r#"attachment; filename="run.log""#)
            .with_body("log")
            .create();
        let client = Client::for_tests(&server.url());
        let dir = tempfile::tempdir().unwrap();
        let (path, bytes) = fetch(&client, "9", None, dir.path(), &mut HashSet::new()).unwrap();
        assert_eq!(path, dir.path().join("9-run.log"));
        assert_eq!(bytes, 3);
    }

    #[test]
    fn uploads_return_the_attachment_id() {
        let mut server = Server::new();
        server
            .mock("POST", "/index.php?/api/v2/add_attachment_to_run/16")
            .with_body(r#"{"attachment_id": "2ec2-a"}"#)
            .create();
        server
            .mock("POST", "/index.php?/api/v2/add_attachment_to_run/17")
            .with_body("{}")
            .create();
        let client = Client::for_tests(&server.url());
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.png");
        std::fs::write(&file, "png").unwrap();
        assert_eq!(
            upload(&client, "add_attachment_to_run/16", &file).unwrap(),
            "2ec2-a"
        );
        assert!(upload(&client, "add_attachment_to_run/17", &file).is_err());
    }
}
