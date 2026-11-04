//! TestRail's rich text: Markdown in, the HTML its step results render, and plain text back out.
//!
//! TestRail escapes HTML in a step result unless the text carries `<p>` tags, and it ignores
//! Markdown image syntax there. So text is rendered to HTML, images get their own line after the
//! paragraph, and expected results sit in a blockquote.

use anyhow::Result;
use pulldown_cmark::{CowStr, Event, Options, Parser, Tag};
use regex::{Captures, Regex};
use std::sync::LazyLock;

/// The `src` TestRail serves an attachment from, relative to the instance.
pub fn attachment_src(id: &str) -> String {
    format!("index.php?/attachments/get/{id}")
}

/// Whether text is HTML already, so it passes through untouched.
pub fn is_html(text: &str) -> bool {
    text.trim_start().starts_with('<')
}

/// Whether an image source names a local file rather than something TestRail can already serve.
pub fn is_local(src: &str) -> bool {
    !(src.contains("://") || src.starts_with("index.php?") || src.starts_with("data:"))
}

/// Markdown as HTML, with each local image's source replaced by what `image` returns for it, as
/// when the file is uploaded. HTML and blank text pass through.
pub fn to_html(text: &str, image: &mut dyn FnMut(&str) -> Result<String>) -> Result<String> {
    if is_html(text) || text.trim().is_empty() {
        return Ok(text.to_string());
    }
    let mut events = Vec::new();
    for event in Parser::new_ext(text, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH) {
        events.push(match event {
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) if is_local(&dest_url) => Event::Start(Tag::Image {
                link_type,
                dest_url: CowStr::from(image(&dest_url)?),
                title,
                id,
            }),
            // Line breaks in a step's text are meant, as in a GitHub comment.
            Event::SoftBreak => Event::HardBreak,
            other => other,
        });
    }
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events.into_iter());
    Ok(tidy(&html))
}

static IMAGE_PARAGRAPH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<p>((?:<img [^>]*/>|<br />|\s)+)</p>").unwrap());
static BREAK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<br />\s*").unwrap());

/// Lift paragraphs of nothing but images out of their `<p>`, and drop the newlines between tags.
fn tidy(html: &str) -> String {
    let html = IMAGE_PARAGRAPH.replace_all(html, |caps: &Captures| {
        BREAK.replace_all(caps[1].trim(), "").into_owned()
    });
    html.trim_end()
        .replace("<br />\n", "<br />")
        .replace(">\n<", "><")
}

/// HTML wrapped in a blockquote, as expected results are shown. Blank text and text already in a
/// blockquote are left alone.
pub fn quote(html: &str) -> String {
    let t = html.trim();
    if t.is_empty() || t.starts_with("<blockquote") {
        html.to_string()
    } else {
        format!("<blockquote>{t}</blockquote>")
    }
}

static TAG_IMAGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)<img\b[^>]*?\bsrc="([^"]*)"[^>]*>"#).unwrap());
static TAG_BREAK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<br\s*/?>").unwrap());
static TAG_BLOCK_END: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)</(?:p|div|h[1-6]|li|tr|blockquote|pre)>").unwrap());
static TAG_ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<li\b[^>]*>").unwrap());
static TAG_ANY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
static BLANK_LINES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());

/// Rich text as plain text for the terminal. Images become Markdown references.
pub fn to_text(text: &str) -> String {
    if !text.contains('<') {
        return text.trim().to_string();
    }
    let t = TAG_IMAGE.replace_all(text, "\n\n![]($1)\n\n");
    let t = TAG_BREAK.replace_all(&t, "\n");
    let t = TAG_BLOCK_END.replace_all(&t, "\n");
    let t = TAG_ITEM.replace_all(&t, "- ");
    let t = TAG_ANY.replace_all(&t, "");
    let t = t
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&amp;", "&");
    let lines: Vec<&str> = t.lines().map(str::trim_end).collect();
    BLANK_LINES
        .replace_all(&lines.join("\n"), "\n\n")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_images(src: &str) -> Result<String> {
        panic!("unexpected image {src}")
    }

    #[test]
    fn markdown_renders_as_paragraphs() {
        assert_eq!(
            to_html("Page loaded **fine**.", &mut no_images).unwrap(),
            "<p>Page loaded <strong>fine</strong>.</p>"
        );
        assert_eq!(
            to_html("one\ntwo\n\nthree", &mut no_images).unwrap(),
            "<p>one<br />two</p><p>three</p>"
        );
        assert_eq!(
            to_html("- a\n- b", &mut no_images).unwrap(),
            "<ul><li>a</li><li>b</li></ul>"
        );
    }

    #[test]
    fn html_and_blank_text_pass_through() {
        let html = "<p>Already</p><img src=\"index.php?/attachments/get/5\" />";
        assert_eq!(to_html(html, &mut no_images).unwrap(), html);
        assert_eq!(to_html("  ", &mut no_images).unwrap(), "  ");
    }

    #[test]
    fn local_images_are_replaced_and_lifted_out_of_paragraphs() {
        let mut seen = Vec::new();
        let mut upload = |src: &str| {
            seen.push(src.to_string());
            Ok(attachment_src(&format!("{}", 54 + seen.len())))
        };
        let html = to_html(
            "No dialog appeared.\n\n![step 2](/tmp/s2.png)\n![](</tmp/my shot.png>)\n\n![](https://x/y.png)",
            &mut upload,
        )
        .unwrap();
        assert_eq!(seen, vec!["/tmp/s2.png", "/tmp/my shot.png"]);
        assert_eq!(
            html,
            "<p>No dialog appeared.</p>\
             <img src=\"index.php?/attachments/get/55\" alt=\"step 2\" />\
             <img src=\"index.php?/attachments/get/56\" alt=\"\" />\
             <img src=\"https://x/y.png\" alt=\"\" />"
        );
    }

    #[test]
    fn inline_images_stay_in_their_paragraph() {
        let html = to_html("See ![](index.php?/attachments/get/9) here", &mut no_images).unwrap();
        assert_eq!(
            html,
            "<p>See <img src=\"index.php?/attachments/get/9\" alt=\"\" /> here</p>"
        );
    }

    #[test]
    fn local_sources_are_files() {
        assert!(is_local("shot.png"));
        assert!(is_local("/tmp/shot.png"));
        assert!(is_local("C:\\shots\\a.png"));
        assert!(!is_local("https://x/y.png"));
        assert!(!is_local("index.php?/attachments/get/5"));
        assert!(!is_local("data:image/png;base64,AA"));
    }

    #[test]
    fn expected_results_are_quoted_once() {
        assert_eq!(
            quote("<p>Loads</p>"),
            "<blockquote><p>Loads</p></blockquote>"
        );
        assert_eq!(
            quote("<blockquote>x</blockquote>"),
            "<blockquote>x</blockquote>"
        );
        assert_eq!(quote(""), "");
    }

    #[test]
    fn rich_text_reads_as_plain_text() {
        assert_eq!(to_text("  plain\n"), "plain");
        assert_eq!(
            to_text(
                "<p>Step &amp; check</p><img src=\"index.php?/attachments/get/55\" /><blockquote>Shown<br/>twice</blockquote><ul><li>a</li><li>b</li></ul>"
            ),
            "Step & check\n\n![](index.php?/attachments/get/55)\n\nShown\ntwice\n- a\n- b"
        );
    }
}
