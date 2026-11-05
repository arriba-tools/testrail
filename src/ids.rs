//! IDs as people paste them: bare numbers, TestRail's letter prefixes, or links to the page.

use anyhow::{Result, bail};
use regex::Regex;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Project,
    Suite,
    Section,
    Case,
    Milestone,
    Plan,
    Run,
    Test,
    Result,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Project => "project",
            Kind::Suite => "suite",
            Kind::Section => "section",
            Kind::Case => "case",
            Kind::Milestone => "milestone",
            Kind::Plan => "plan",
            Kind::Run => "run",
            Kind::Test => "test",
            Kind::Result => "result",
        }
    }

    /// The letter TestRail puts before the ID on screen.
    fn prefix(self) -> Option<char> {
        match self {
            Kind::Suite => Some('S'),
            Kind::Case => Some('C'),
            Kind::Milestone => Some('M'),
            Kind::Run => Some('R'),
            Kind::Test => Some('T'),
            Kind::Result => Some('#'),
            _ => None,
        }
    }

    /// The page path a link to one carries, as in `cases/view/39`.
    pub fn page(self) -> Option<&'static str> {
        match self {
            Kind::Project => Some("projects/overview"),
            Kind::Suite => Some("suites/view"),
            Kind::Case => Some("cases/view"),
            Kind::Milestone => Some("milestones/view"),
            Kind::Plan => Some("plans/view"),
            Kind::Run => Some("runs/view"),
            Kind::Test => Some("tests/view"),
            Kind::Section | Kind::Result => None,
        }
    }
}

static PAGE_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[/?]([a-z]+/(?:view|overview))/(\d+)").unwrap());

/// The ID in `text`, which may be a number, a prefixed ID such as C39, or a link to the page.
pub fn parse(text: &str, kind: Kind) -> Result<u64> {
    let t = text.trim();
    if let Ok(id) = t.parse() {
        return Ok(id);
    }
    if let Some(prefix) = kind.prefix() {
        let mut chars = t.chars();
        if chars
            .next()
            .is_some_and(|c| c.eq_ignore_ascii_case(&prefix))
            && let Ok(id) = chars.as_str().parse()
        {
            return Ok(id);
        }
    }
    if let Some(page) = kind.page()
        && let Some(caps) = PAGE_LINK.captures(t)
        && &caps[1] == page
        && let Ok(id) = caps[2].parse()
    {
        return Ok(id);
    }
    let example = match kind.prefix() {
        Some(p) => format!("{p}123"),
        None => "123".to_string(),
    };
    let link = if kind.page().is_some() {
        " or its URL"
    } else {
        ""
    };
    bail!(
        "Expected a {} ID, such as {example}{link}, but got {t:?}",
        kind.name()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_prefixes_and_links_parse() {
        assert_eq!(parse("39", Kind::Case).unwrap(), 39);
        assert_eq!(parse(" C39 ", Kind::Case).unwrap(), 39);
        assert_eq!(parse("c39", Kind::Case).unwrap(), 39);
        assert_eq!(parse("R201", Kind::Run).unwrap(), 201);
        assert_eq!(parse("T1001", Kind::Test).unwrap(), 1001);
        assert_eq!(
            parse("https://x.testrail.io/index.php?/cases/view/39", Kind::Case).unwrap(),
            39
        );
        assert_eq!(
            parse(
                "https://x.testrail.io/index.php?/runs/view/201&group_by=cases:section_id",
                Kind::Run
            )
            .unwrap(),
            201
        );
        assert_eq!(
            parse(
                "https://x.testrail.io/index.php?/projects/overview/2",
                Kind::Project
            )
            .unwrap(),
            2
        );
        assert_eq!(
            parse("https://x.testrail.io/index.php?/plans/view/15", Kind::Plan).unwrap(),
            15
        );
    }

    #[test]
    fn mismatched_ids_are_rejected() {
        let err = parse("R201", Kind::Case).unwrap_err().to_string();
        assert_eq!(
            err,
            "Expected a case ID, such as C123 or its URL, but got \"R201\""
        );
        assert!(parse("https://x.testrail.io/index.php?/runs/view/3", Kind::Case).is_err());
        let err = parse("abc", Kind::Section).unwrap_err().to_string();
        assert_eq!(err, "Expected a section ID, such as 123, but got \"abc\"");
        assert!(parse("Acme", Kind::Project).is_err());
    }

    #[test]
    fn every_kind_names_itself_in_errors() {
        let cases = [
            (Kind::Project, "project ID, such as 123 or its URL"),
            (Kind::Suite, "suite ID, such as S123 or its URL"),
            (Kind::Milestone, "milestone ID, such as M123 or its URL"),
            (Kind::Plan, "plan ID, such as 123 or its URL"),
            (Kind::Run, "run ID, such as R123 or its URL"),
            (Kind::Test, "test ID, such as T123 or its URL"),
            (Kind::Result, "result ID, such as #123,"),
        ];
        for (kind, want) in cases {
            let err = parse("x", kind).unwrap_err().to_string();
            assert!(err.contains(want), "{err}");
        }
    }

    #[test]
    fn links_to_suites_milestones_and_tests_parse() {
        let base = "https://x.testrail.io/index.php?";
        assert_eq!(
            parse(&format!("{base}/suites/view/6"), Kind::Suite).unwrap(),
            6
        );
        assert_eq!(
            parse(&format!("{base}/milestones/view/9"), Kind::Milestone).unwrap(),
            9
        );
        assert_eq!(
            parse(&format!("{base}/tests/view/1782"), Kind::Test).unwrap(),
            1782
        );
        assert_eq!(parse("M9", Kind::Milestone).unwrap(), 9);
        assert_eq!(parse("S6", Kind::Suite).unwrap(), 6);
        assert_eq!(parse("#2064", Kind::Result).unwrap(), 2064);
    }
}
