use anyhow::Result;
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

use crate::api::{self, with_params};
use crate::cli::{CaseCommand, CaseListArgs};
use crate::context::Ctx;
use crate::ids::{self, Kind};
use crate::markup;
use crate::output::{date, note_more, print_json, print_table, table, text};
use crate::project;

pub fn run(ctx: &Ctx, cmd: CaseCommand) -> Result<()> {
    match cmd {
        CaseCommand::List(args) => list(ctx, &args),
        CaseCommand::View(args) => view(ctx, &args.case),
    }
}

fn list(ctx: &Ctx, args: &CaseListArgs) -> Result<()> {
    let project = project::resolve(&ctx.client, &args.project.project)?;
    let mut params = Vec::new();
    if let Some(suite) = &args.suite {
        params.push(("suite_id", ids::parse(suite, Kind::Suite)?.to_string()));
    }
    if let Some(section) = &args.section {
        params.push((
            "section_id",
            ids::parse(section, Kind::Section)?.to_string(),
        ));
    }
    if let Some(filter) = &args.filter {
        params.push(("filter", filter.clone()));
    }
    let endpoint = with_params(&format!("get_cases/{project}"), &params);
    let page = api::list(&ctx.client, &endpoint, "cases", args.limit.get())?;
    if ctx.json {
        return print_json(&page.items);
    }
    let rows: Vec<Vec<String>> = page
        .items
        .iter()
        .map(|c| vec![format!("C{}", text(&c["id"])), text(&c["title"])])
        .collect();
    print_table(&rows, "No cases found.");
    note_more(page.more, page.items.len());
    Ok(())
}

fn view(ctx: &Ctx, case: &str) -> Result<()> {
    let id = ids::parse(case, Kind::Case)?;
    let c = ctx.client.get(&format!("get_case/{id}"))?;
    if ctx.json {
        return print_json(&c);
    }
    println!("C{id}  {}", text(&c["title"]));
    println!("{}", ctx.web_url(&format!("cases/view/{id}")));
    let mut rows = vec![vec!["Section".to_string(), text(&c["section_id"])]];
    for (label, key) in [("Refs", "refs"), ("Estimate", "estimate")] {
        let value = text(&c[key]);
        if !value.is_empty() {
            rows.push(vec![label.to_string(), value]);
        }
    }
    rows.push(vec!["Updated".to_string(), date(&c["updated_on"])]);
    println!("{}", table(&rows, "  "));
    let body = body(&c);
    print!("{body}");
    if markup::has_attachment_image(&body) {
        println!("\n{IMAGE_HINT}");
    }
    Ok(())
}

/// Printed after text that names attachment images, so the reader knows how to see them.
pub const IMAGE_HINT: &str = "Download an image with `testrail-arriba attachment download <ID>`.";

/// Fields shown as the steps, or not shown at all.
const STEP_FIELDS: [&str; 5] = [
    "custom_steps",
    "custom_expected",
    "custom_preconds",
    "custom_steps_separated",
    "custom_step_results",
];

/// Custom text fields other than the steps, such as a background, labeled from their names.
fn text_fields(case: &Value) -> Vec<(String, String)> {
    case.as_object()
        .into_iter()
        .flatten()
        .filter(|(k, _)| k.starts_with("custom_") && !STEP_FIELDS.contains(&k.as_str()))
        .filter_map(|(k, v)| {
            let t = markup::to_text(v.as_str()?);
            (!t.is_empty()).then(|| (field_label(k), t))
        })
        .collect()
}

/// `custom_case_background` as `Case background`.
fn field_label(key: &str) -> String {
    let words = key.trim_start_matches("custom_").replace('_', " ");
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The steps a case keeps, whichever template it uses: separated steps with their expected
/// results, or one block of each.
pub fn steps(case: &Value) -> Vec<(String, String)> {
    if let Some(separated) = case["custom_steps_separated"].as_array() {
        return separated
            .iter()
            .map(|s| (text(&s["content"]), text(&s["expected"])))
            .collect();
    }
    let block = text(&case["custom_steps"]);
    let expected = text(&case["custom_expected"]);
    if block.trim().is_empty() && expected.trim().is_empty() {
        Vec::new()
    } else {
        vec![(block, expected)]
    }
}

/// A case's or test's custom text, preconditions, and steps as plain text, each section opening
/// with a blank line.
pub fn body(case: &Value) -> String {
    let mut out = String::new();
    let mut section = |title: &str, text: &str| {
        out.push_str(&format!("\n{title}\n{}\n", indent(text, "  ", "  ")));
    };
    for (label, value) in text_fields(case) {
        section(&label, &value);
    }
    let preconds = markup::to_text(&text(&case["custom_preconds"]));
    if !preconds.is_empty() {
        section("Preconditions", &preconds);
    }
    let steps = steps(case);
    if !steps.is_empty() {
        out.push_str("\nSteps\n");
        for (i, (content, expected)) in steps.iter().enumerate() {
            let first = format!("  {}. ", i + 1);
            let rest = " ".repeat(first.len());
            let content = markup::to_text(content);
            // A list opens on its own line, so its numbers don't run into the step's.
            if starts_with_list(&content) {
                out.push_str(first.trim_end());
                out.push('\n');
                out.push_str(&indent(&content, &rest, &rest));
            } else {
                out.push_str(&indent(&content, &first, &rest));
            }
            out.push('\n');
            let expected = markup::to_text(expected);
            if !expected.is_empty() {
                let label = if starts_with_list(&expected) {
                    format!("Expected:\n{}", indent(&expected, "  ", "  "))
                } else {
                    format!("Expected: {expected}")
                };
                out.push_str(&indent(&label, &rest, &rest));
                out.push('\n');
            }
        }
    }
    out
}

static LIST_START: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:\d+\.|-) ").unwrap());

fn starts_with_list(text: &str) -> bool {
    LIST_START.is_match(text)
}

/// Text with `first` before its first line and `rest` before the others.
pub fn indent(text: &str, first: &str, rest: &str) -> String {
    text.lines()
        .enumerate()
        .map(|(i, line)| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{}{line}", if i == 0 { first } else { rest })
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn separated_steps_come_with_their_expected_results() {
        let case = json!({"custom_steps_separated": [
            {"content": "Open the page", "expected": "It loads"},
            {"content": "Click Submit", "expected": null}
        ]});
        assert_eq!(
            steps(&case),
            vec![
                ("Open the page".to_string(), "It loads".to_string()),
                ("Click Submit".to_string(), String::new())
            ]
        );
    }

    #[test]
    fn text_templates_read_as_one_step() {
        let case = json!({"custom_steps": "1. Open\n2. Click", "custom_expected": "Works"});
        assert_eq!(steps(&case).len(), 1);
        assert!(steps(&json!({"custom_steps": ""})).is_empty());
    }

    #[test]
    fn body_shows_custom_text_preconditions_and_steps() {
        let case = json!({
            "custom_case_background": "<p>Why this matters</p>",
            "custom_automation_type": 0,
            "custom_preconds": "Signed out",
            "custom_steps_separated": [
                {"content": "<p>Open</p><p>Click</p>", "expected": "<p><img src=\"index.php?/attachments/get/7\"></p>"}
            ]
        });
        let body = body(&case);
        assert_eq!(
            body,
            "\nCase background\n  Why this matters\n\nPreconditions\n  Signed out\n\nSteps\n  1. Open\n     Click\n     Expected: [image: attachment 7]\n"
        );
        assert!(markup::has_attachment_image(&body));
        assert_eq!(field_label("custom_x"), "X");
    }

    #[test]
    fn lists_open_on_their_own_line() {
        let case = json!({"custom_steps_separated": [{
            "content": "<ol><li>Try empty</li><li>Try wrong</li></ol>",
            "expected": "<ul><li>Errors show</li></ul>"
        }]});
        assert_eq!(
            body(&case),
            "\nSteps\n  1.\n     1. Try empty\n     2. Try wrong\n     Expected:\n       - Errors show\n"
        );
    }

    #[test]
    fn indent_marks_the_first_line() {
        assert_eq!(indent("a\n\nb", "  1. ", "     "), "  1. a\n\n     b");
    }
}
