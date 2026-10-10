//! Message text: the same reading as the web chat (links, e-mail addresses,
//! `**bold**`, file-service links as cards), turned into the markdown subset
//! Slint's `StyledText` draws.

use regex_lite::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Text(String),
    Bold(String),
    Url(String),
    Email(String),
}

static URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:https?|ftp)://[-A-Z0-9+&@#/%?=~_|!:,.;]*[-A-Z0-9+&@#/%=~_|]").unwrap());
static EMAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b").unwrap());
static BOLD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*\*((?s:.+?))\*\*").unwrap());

fn cut(text: &str, pattern: &Regex, hit: impl Fn(&regex_lite::Captures) -> Part, between: &dyn Fn(&str, &mut Vec<Part>), out: &mut Vec<Part>) {
    let mut last = 0;
    for found in pattern.captures_iter(text) {
        let whole = found.get(0).unwrap();
        if whole.start() > last {
            between(&text[last..whole.start()], out);
        }
        out.push(hit(&found));
        last = whole.end();
    }
    if last < text.len() {
        between(&text[last..], out);
    }
}

/// Links first, e-mail addresses between them, bold only in what is left:
/// a link never loses its stars.
pub fn parts(text: &str, with_bold: bool) -> Vec<Part> {
    let plain = |t: &str, out: &mut Vec<Part>| out.push(Part::Text(t.to_string()));
    let bold = |t: &str, out: &mut Vec<Part>| {
        if with_bold {
            cut(t, &BOLD, |c| Part::Bold(c[1].to_string()), &plain, out)
        } else {
            plain(t, out)
        }
    };
    let emails = |t: &str, out: &mut Vec<Part>| cut(t, &EMAIL, |c| Part::Email(c[0].to_string()), &bold, out);
    let mut out = Vec::new();
    cut(text, &URL, |c| Part::Url(c[0].to_string()), &emails, &mut out);
    out
}

/// A link to a file somebody uploaded elsewhere: drawn as a card.
#[derive(Debug, Clone, PartialEq)]
pub struct FileLink {
    pub service: &'static str,
    pub label: String,
    pub url: String,
}

struct Form {
    host: &'static str,
    path: &'static str,
    query: Option<&'static str>,
}

struct Service {
    name: &'static str,
    forms: &'static [Form],
    /// Whether a query or a fragment may follow.
    rest: bool,
}

const fn form(host: &'static str, path: &'static str) -> Form {
    Form { host, path, query: None }
}

// The same list as frontend/src/utils/chatFileLink.js.
static SERVICES: &[Service] = &[
    Service {
        name: "SwissTransfer",
        forms: &[form(
            r"^(?:[a-z0-9-]+\.)*swisstransfer\.com$|^swisstransfer\.infomaniak\.com$",
            r"^(?:/[a-z]{2})?/(?:d|dl)/[^/]+$",
        )],
        rest: false,
    },
    Service {
        name: "Dropbox",
        forms: &[form(r"^(?:www\.)?dropbox\.com$", r"^/(?:s|sh|scl/fi|scl/fo|t)/[\w-]+(?:/[^/]*)*$")],
        rest: true,
    },
    Service {
        name: "Google Drive",
        forms: &[
            form(r"^drive\.google\.com$", r"^/file/d/[\w-]{10,}(?:/[\w-]*)?$"),
            form(r"^drive\.google\.com$", r"^/drive/(?:u/\d+/)?folders/[\w-]{10,}/?$"),
            Form {
                host: r"^drive\.google\.com$",
                path: r"^/(?:open|uc)$",
                query: Some(r"(?:^|&)id=[\w-]{10,}(?:&|$)"),
            },
            form(r"^docs\.google\.com$", r"^/(?:document|spreadsheets|presentation)/d/[\w-]{10,}(?:/[\w-]*)?$"),
        ],
        rest: true,
    },
    Service {
        name: "OneDrive",
        forms: &[
            form(r"^1drv\.ms$", r"(?i)^/[a-z]/(?:s![\w-]+|c/[0-9a-f]+/[\w-]+)/?$"),
            Form {
                host: r"^onedrive\.live\.com$",
                path: r"^/(?:redir|redir\.aspx|download|embed)?$",
                query: Some(r"(?:^|&)(?:res)?id=[^&]+"),
            },
        ],
        rest: true,
    },
    Service {
        name: "WeTransfer",
        forms: &[
            form(r"^we\.tl$", r"(?i)^/(?:t-)?[a-z0-9]+/?$"),
            form(r"^(?:[a-z0-9-]+\.)?wetransfer\.com$", r"(?i)^/downloads/[a-z0-9]+(?:/[a-z0-9]+){1,2}/?$"),
        ],
        rest: true,
    },
];

static ADDRESS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^https://([a-z0-9.-]+)(/[^?#]*)?(?:\?([^#]*))?(#.*)?$").unwrap());
static HEAD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[^:/?#]+://[^/?#]*").unwrap());

fn is(pattern: &str, text: &str) -> bool {
    Regex::new(pattern).map(|r| r.is_match(text)).unwrap_or(false)
}

pub fn file_link(url: &str) -> Option<FileLink> {
    if url.chars().any(char::is_whitespace) {
        return None;
    }
    let head = HEAD.find(url)?;
    let lowered = format!("{}{}", head.as_str().to_lowercase(), &url[head.end()..]);
    let address = ADDRESS.captures(&lowered)?;
    let host = address.get(1)?.as_str();
    let path = address.get(2).map_or("", |m| m.as_str());
    let query = address.get(3).map(|m| m.as_str());
    let hash = address.get(4);
    let service = SERVICES.iter().find(|s| {
        s.forms
            .iter()
            .any(|f| is(f.host, host) && is(f.path, path) && f.query.is_none_or(|q| is(q, query.unwrap_or(""))))
    })?;
    if !service.rest && (query.is_some() || hash.is_some()) {
        return None;
    }
    Some(FileLink {
        service: service.name,
        label: format!("{}{}", host.strip_prefix("www.").unwrap_or(host), path),
        url: url.to_string(),
    })
}

/// A video call link of the web chat carries its topic and times behind the
/// `#`: shown without, opened with.
pub fn without_video_topic(url: &str) -> &str {
    match url.find("#config.subject=") {
        Some(at) => &url[..at],
        None => url,
    }
}

/// Every ASCII punctuation mark may be escaped in CommonMark, so all are:
/// what a member wrote is never read as markup.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        if c.is_ascii_punctuation() {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn link_target(url: &str) -> String {
    // Inside <...> only these three end or break a link destination.
    url.replace('<', "%3C").replace('>', "%3E").replace(' ', "%20")
}

/// What one message becomes on the screen.
#[derive(Debug, Default, PartialEq)]
pub struct Rendered {
    /// One markdown string per line of the message; an empty one is a blank line.
    pub lines: Vec<String>,
    pub files: Vec<FileLink>,
}

/// With a `mark_color`, what matches the search `needle` is painted in it.
pub fn render(text: &str, with_bold: bool, mark_color: Option<&str>, needle: &str) -> Rendered {
    let mut out = Rendered::default();
    let mut line = String::new();
    let push_text = |line: &mut String, lines: &mut Vec<String>, value: &str, wrap: (&str, &str)| {
        for (i, piece) in value.split('\n').enumerate() {
            if i > 0 {
                lines.push(std::mem::take(line));
            }
            if piece.is_empty() {
                continue;
            }
            // Leading blanks would start a code block.
            let lead = piece.len() - piece.trim_start_matches(' ').len();
            let lead = if line.is_empty() { lead } else { 0 };
            line.push_str(&"\u{a0}".repeat(lead));
            let body = marked(&piece[lead..], mark_color, needle);
            // Stars must touch the word, or CommonMark does not read them as bold.
            if wrap.0.is_empty() || body.trim().is_empty() {
                line.push_str(&body);
            } else {
                let core = piece[lead..].trim();
                let (before, after) = piece[lead..].split_once(core).unwrap_or(("", ""));
                line.push_str(before);
                line.push_str(wrap.0);
                line.push_str(&marked(core, mark_color, needle));
                line.push_str(wrap.1);
                line.push_str(after);
            }
        }
    };
    for part in parts(text, with_bold) {
        match part {
            Part::Text(value) => push_text(&mut line, &mut out.lines, &value, ("", "")),
            Part::Bold(value) => push_text(&mut line, &mut out.lines, &value, ("**", "**")),
            Part::Email(value) => line.push_str(&format!("[{}](<mailto:{}>)", escape(&value), link_target(&value))),
            Part::Url(value) => match file_link(&value) {
                Some(file) => {
                    out.files.push(file);
                    // The card stands for the link: no blank left where it was.
                    let trimmed = line.trim_end().len();
                    line.truncate(trimmed);
                }
                None => line.push_str(&format!("[{}](<{}>)", escape(without_video_topic(&value)), link_target(&value))),
            },
        }
    }
    out.lines.push(line);
    while out.lines.len() > 1 && out.lines.last().is_some_and(|l| l.trim().is_empty()) {
        out.lines.pop();
    }
    out
}

fn marked(text: &str, color: Option<&str>, needle: &str) -> String {
    let Some(color) = color else { return escape(text) };
    let hits = super::search::ranges(text, needle);
    if hits.is_empty() {
        return escape(text);
    }
    let mut out = String::new();
    let mut last = 0;
    for (from, to) in hits {
        out.push_str(&escape(&text[last..from]));
        out.push_str(&format!("<font color=\"{color}\"><u>{}</u></font>", escape(&text[from..to])));
        last = to;
    }
    out.push_str(&escape(&text[last..]));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_addresses_and_bold_are_told_apart() {
        let got = parts("see https://a.example/x?y=1, write to me@a.example **now**", true);
        assert_eq!(
            got,
            vec![
                Part::Text("see ".into()),
                Part::Url("https://a.example/x?y=1".into()),
                Part::Text(", write to ".into()),
                Part::Email("me@a.example".into()),
                Part::Text(" ".into()),
                Part::Bold("now".into()),
            ]
        );
    }

    #[test]
    fn bold_never_reaches_across_a_link() {
        assert_eq!(
            parts("**see https://a.example now**", true),
            vec![Part::Text("**see ".into()), Part::Url("https://a.example".into()), Part::Text(" now**".into())]
        );
    }

    #[test]
    fn what_a_member_wrote_is_never_markup() {
        let got = render("# no heading\n1. no list\n    no code [x](y) <b>", true, None, "");
        assert_eq!(got.lines[0], "\\# no heading");
        assert_eq!(got.lines[1], "1\\. no list");
        assert!(got.lines[2].starts_with("\u{a0}\u{a0}\u{a0}\u{a0}no code \\[x\\]\\(y\\) \\<b\\>"));
    }

    #[test]
    fn a_file_link_becomes_a_card_and_leaves_the_text() {
        let got = render("here: https://www.swisstransfer.com/d/abc-123", true, None, "");
        assert_eq!(got.lines, vec!["here\\:".to_string()]);
        assert_eq!(got.files[0].service, "SwissTransfer");
        assert_eq!(got.files[0].label, "swisstransfer.com/d/abc-123");
    }

    #[test]
    fn swisstransfer_with_a_query_is_an_ordinary_link() {
        assert!(file_link("https://www.swisstransfer.com/d/abc?x=1").is_none());
        assert!(file_link("https://drive.google.com/file/d/0123456789abc/view?usp=sharing").is_some());
        assert!(file_link("http://www.swisstransfer.com/d/abc").is_none());
    }

    #[test]
    fn a_video_link_shows_without_its_topic() {
        let got = render("https://meet.example/gdd-abc#config.subject=%22x%22", true, None, "");
        assert_eq!(
            got.lines[0],
            "[https\\:\\/\\/meet\\.example\\/gdd\\-abc](<https://meet.example/gdd-abc#config.subject=%22x%22>)"
        );
    }

    #[test]
    fn blank_lines_stay_and_trailing_ones_go() {
        assert_eq!(render("a\n\nb\n\n", false, None, "").lines, vec!["a", "", "b"]);
    }
}
