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

static TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]*>|[^<]+").unwrap());
static TAG_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^</?\s*([a-zA-Z0-9]+)").unwrap());
static ATTACHMENT_ID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)data-attachment-id="([^"]+)"|attachments/get/([A-Za-z0-9-]+)"#).unwrap()
});
static IMAGE_SRC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\bsrc\s*=\s*"([^"]*)""#).unwrap());
static MARKDOWN_IMAGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"!\[[^\]]*\]\(\s*index\.php\?/attachments/get/([A-Za-z0-9-]+)[^)]*\)").unwrap()
});
static ENTITY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"&(#[xX][0-9a-fA-F]+|#[0-9]+|[a-zA-Z]+);").unwrap());
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
static BLANK_LINES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());

/// How an image reads in plain text. Attachments are named by ID, so they can be downloaded.
fn image_text(tag: &str) -> String {
    if let Some(caps) = ATTACHMENT_ID.captures(tag) {
        let id = caps.get(1).or(caps.get(2)).map_or("", |m| m.as_str());
        return format!("[image: attachment {id}]");
    }
    match IMAGE_SRC.captures(tag) {
        Some(caps) => format!("[image: {}]", &caps[1]),
        None => "[image]".to_string(),
    }
}

/// Text with HTML entities decoded. Unknown names are left as they are.
fn decode_entities(text: &str) -> String {
    ENTITY
        .replace_all(text, |caps: &Captures| {
            let e = &caps[1];
            let c = if let Some(hex) = e.strip_prefix("#x").or(e.strip_prefix("#X")) {
                u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
            } else if let Some(dec) = e.strip_prefix('#') {
                dec.parse().ok().and_then(char::from_u32)
            } else {
                match e {
                    "nbsp" => Some(' '),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "amp" => Some('&'),
                    "hellip" => Some('…'),
                    "mdash" => Some('—'),
                    "ndash" => Some('–'),
                    "lsquo" => Some('‘'),
                    "rsquo" => Some('’'),
                    "ldquo" => Some('“'),
                    "rdquo" => Some('”'),
                    "rarr" => Some('→'),
                    "larr" => Some('←'),
                    "times" => Some('×'),
                    _ => None,
                }
            };
            c.map_or_else(|| caps[0].to_string(), String::from)
        })
        .into_owned()
}

/// Plain text built up from HTML, one block per line.
struct Flattener {
    out: String,
    /// Open lists, innermost last: the next number for an ordered one, None for bullets.
    lists: Vec<Option<usize>>,
    /// Depth of `<pre>`, inside which whitespace is kept.
    pre: usize,
}

impl Flattener {
    fn at_line_start(&self) -> bool {
        self.out.is_empty() || self.out.ends_with('\n')
    }

    fn line(&mut self) {
        if !self.at_line_start() {
            self.out.push('\n');
        }
    }

    fn inline(&mut self, text: &str) {
        if !self.at_line_start() && !self.out.ends_with(' ') && !text.starts_with(' ') {
            self.out.push(' ');
        }
        self.out.push_str(text);
    }

    fn text(&mut self, raw: &str) {
        let decoded = decode_entities(raw);
        if self.pre > 0 {
            self.out.push_str(&decoded);
            return;
        }
        let collapsed = SPACES.replace_all(&decoded, " ");
        let t = if self.at_line_start() || self.out.ends_with(' ') {
            collapsed.trim_start()
        } else {
            &collapsed
        };
        self.out.push_str(t);
    }

    fn tag(&mut self, tag: &str) {
        let Some(caps) = TAG_NAME.captures(tag) else {
            return;
        };
        let name = caps[1].to_ascii_lowercase();
        let closing = tag.starts_with("</");
        match name.as_str() {
            "br" => self.out.push('\n'),
            "p" | "div" | "tr" | "blockquote" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.line()
            }
            "pre" => {
                self.line();
                if closing {
                    self.pre = self.pre.saturating_sub(1);
                } else {
                    self.pre += 1;
                }
            }
            "ul" | "ol" => {
                self.line();
                if closing {
                    self.lists.pop();
                } else {
                    self.lists.push((name == "ol").then_some(1));
                }
            }
            "li" => {
                self.line();
                if closing {
                    return;
                }
                let depth = self.lists.len().saturating_sub(1);
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        *n += 1;
                        format!("{}. ", *n - 1)
                    }
                    _ => "- ".to_string(),
                };
                self.out.push_str(&"  ".repeat(depth));
                self.out.push_str(&marker);
            }
            "td" | "th" if !closing && !self.at_line_start() => self.out.push_str(" | "),
            "img" => self.inline(&image_text(tag)),
            _ => {}
        }
    }
}

/// Rich text as plain text for the terminal: lines kept, lists numbered or bulleted by depth, and
/// images named by attachment ID.
pub fn to_text(text: &str) -> String {
    if !text.contains('<') {
        return MARKDOWN_IMAGE
            .replace_all(text.trim(), "[image: attachment $1]")
            .into_owned();
    }
    let mut f = Flattener {
        out: String::new(),
        lists: Vec::new(),
        pre: 0,
    };
    for token in TOKEN.find_iter(text) {
        let t = token.as_str();
        if t.starts_with('<') {
            f.tag(t);
        } else {
            f.text(t);
        }
    }
    let lines: Vec<&str> = f.out.lines().map(str::trim_end).collect();
    BLANK_LINES
        .replace_all(&lines.join("\n"), "\n\n")
        .trim()
        .to_string()
}

/// Whether plain text from [`to_text`] names an attachment image.
pub fn has_attachment_image(text: &str) -> bool {
    text.contains("[image: attachment ")
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
            to_text("<p>Step &amp; check&nbsp;&#8594;</p><blockquote>Shown<br/>twice</blockquote>"),
            "Step & check →\nShown\ntwice"
        );
    }

    #[test]
    fn lists_keep_their_numbers_and_nesting() {
        let html = "<ol><li>Open the page</li><li>Sign in shows<ul><li>Email and password\
                    <img src=\"index.php?/attachments/get/451#_t=17\" data-attachment-id=\"451\"></li>\
                    </ul></li><li>Done</li></ol><p><img src=\"index.php?/attachments/get/618\"></p>";
        assert_eq!(
            to_text(html),
            "1. Open the page\n2. Sign in shows\n  - Email and password [image: attachment 451]\n\
             3. Done\n[image: attachment 618]"
        );
    }

    #[test]
    fn images_name_their_attachment() {
        assert_eq!(
            to_text("See ![](index.php?/attachments/get/9#_t=1) here"),
            "See [image: attachment 9] here"
        );
        assert_eq!(
            to_text("<img src=\"https://x/y.png\">"),
            "[image: https://x/y.png]"
        );
        assert!(has_attachment_image("a [image: attachment 9]"));
        assert!(!has_attachment_image("a [image: https://x]"));
    }

    #[test]
    fn preformatted_text_keeps_its_lines() {
        assert_eq!(
            to_text("<p>Log:</p><pre>a  b\nc</pre><p>after</p>"),
            "Log:\na  b\nc\nafter"
        );
    }
}
