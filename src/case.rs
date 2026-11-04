use anyhow::Result;
use serde_json::Value;

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
    print_steps(&c);
    Ok(())
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

/// Print a case's or test's preconditions and steps.
pub fn print_steps(case: &Value) {
    let preconds = markup::to_text(&text(&case["custom_preconds"]));
    if !preconds.is_empty() {
        println!("\nPreconditions\n{}", indent(&preconds, "  ", "  "));
    }
    let steps = steps(case);
    if steps.is_empty() {
        return;
    }
    println!("\nSteps");
    for (i, (content, expected)) in steps.iter().enumerate() {
        let first = format!("  {}. ", i + 1);
        let rest = " ".repeat(first.len());
        println!("{}", indent(&markup::to_text(content), &first, &rest));
        let expected = markup::to_text(expected);
        if !expected.is_empty() {
            println!("{}", indent(&format!("Expected: {expected}"), &rest, &rest));
        }
    }
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
    fn indent_marks_the_first_line() {
        assert_eq!(indent("a\n\nb", "  1. ", "     "), "  1. a\n\n     b");
    }
}
