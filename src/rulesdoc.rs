//! The static rules-reference page (docs-rules-page): `RULES` rendered as a
//! standalone HTML document for the GitHub Pages site.
//!
//! Pure string building, like every other engine module: the CLI writes the
//! result out, the deploy pipeline regenerates it on each Pages build, and
//! no rule prose is written anywhere but `src/registry.rs` - the page reads
//! the same records `--explain` prints, so the two cannot drift.
//!
//! Three renderings of the same registry, one `rules.css` stylesheet:
//! `reference` collapses each rule into a `details` entry (a dense lookup),
//! `handbook` prints everything expanded behind a table of contents, and
//! `cards` lays the rules out as a badge-first grid. None of them need
//! JavaScript, so the page ships the same strict CSP as the viewer.
//!
//! The page also carries a short "Using gedlint" section: a reader who
//! lands on a rule from a terminal link needs the run/fix/explain commands
//! one click away. That prose is page chrome, not rule documentation, so it
//! lives here.

use crate::registry::{rulesets, RuleMeta, RULES};

/// Layout of the generated page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocStyle {
    /// Collapsed `details` per rule: a dense lookup table.
    Reference,
    /// Everything expanded, table of contents first: a handbook to read.
    Handbook,
    /// Badge-first card grid, prose behind a `details` per card.
    Cards,
}

impl DocStyle {
    /// Parse the CLI spelling of a style. Unknown values are a usage error,
    /// never a silent fallback.
    pub fn parse(s: &str) -> Option<DocStyle> {
        match s {
            "reference" => Some(DocStyle::Reference),
            "handbook" => Some(DocStyle::Handbook),
            "cards" => Some(DocStyle::Cards),
            _ => None,
        }
    }

    /// Class on `<body>` the stylesheet keys the layout off.
    fn marker(self) -> &'static str {
        match self {
            DocStyle::Reference => "style-reference",
            DocStyle::Handbook => "style-handbook",
            DocStyle::Cards => "style-cards",
        }
    }
}

/// Text into HTML: `&` first, then the angle brackets and quotes.
fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Registry prose into HTML. The registry marks inline code with backticks
/// (`gedlint --fix`), which become `<code>` here; an unbalanced backtick
/// would leak the rest of the paragraph into one element, so a test pins
/// every record to an even count.
fn prose(s: &str) -> String {
    let escaped = escape_html(s);
    let mut out = String::with_capacity(escaped.len());
    for (i, part) in escaped.split('`').enumerate() {
        if i > 0 {
            out.push_str(if i % 2 == 1 { "<code>" } else { "</code>" });
        }
        out.push_str(part);
    }
    out
}

/// One rule as a page entry. `in_details` folds the prose into a `details`
/// element (reference and cards keep the page scannable; the handbook shows
/// everything). The `id` is the rule code, so `rules.html#W401` is a stable
/// link a terminal can print.
fn article(r: &RuleMeta, in_details: bool) -> String {
    let mut out = String::with_capacity(2048);
    out.push_str("<article class=\"rule\" id=\"");
    out.push_str(r.code);
    out.push_str("\">\n<h3><a class=\"anchor\" href=\"#");
    out.push_str(r.code);
    out.push_str("\"><code>");
    out.push_str(r.code);
    out.push_str("</code></a> <span class=\"rname\">");
    out.push_str(r.name);
    out.push_str("</span></h3>\n<p class=\"rtitle\">");
    out.push_str(&prose(r.title));
    out.push_str("</p>\n<ul class=\"badges\">\n");
    for (class, text) in badges(r) {
        out.push_str("<li class=\"b ");
        out.push_str(class);
        out.push_str("\">");
        out.push_str(&text);
        out.push_str("</li>\n");
    }
    out.push_str("</ul>\n");
    let body = body(r);
    if in_details {
        out.push_str("<details>\n<summary>Why it matters and how to fix it</summary>\n");
        out.push_str(&body);
        out.push_str("</details>\n");
    } else {
        out.push_str(&body);
    }
    out.push_str("</article>\n");
    out
}

/// The badge row: severity, category, fixability, default state. Same words
/// the CLI prints, lowercased for the page.
fn badges(r: &RuleMeta) -> Vec<(&'static str, String)> {
    let sev = match r.default_severity {
        crate::diag::Severity::Error => ("sev-error", "error"),
        crate::diag::Severity::Warning => ("sev-warning", "warning"),
        crate::diag::Severity::Info => ("sev-info", "info"),
    };
    let fix = match r.fixable {
        Some(crate::fix::Applicability::Safe) => ("fix-safe", "fixable by --fix"),
        Some(crate::fix::Applicability::MaybeIncorrect) => {
            ("fix-maybe", "needs review (--fix --unsafe)")
        }
        None => ("fix-none", "no automatic fix"),
    };
    vec![
        (sev.0, sev.1.to_string()),
        ("cat", r.category.as_str().to_string()),
        (fix.0, fix.1.to_string()),
        (
            if r.default_enabled { "on" } else { "off" },
            if r.default_enabled {
                "on by default".to_string()
            } else {
                "off by default".to_string()
            },
        ),
    ]
}

/// Why + remedy + optional before/after example.
fn body(r: &RuleMeta) -> String {
    let mut out = String::with_capacity(1600);
    out.push_str("<h4>Why it matters</h4>\n<p>");
    out.push_str(&prose(r.why));
    out.push_str("</p>\n<h4>How to fix it</h4>\n<p>");
    out.push_str(&prose(r.remedy));
    out.push_str("</p>\n");
    if let Some((before, after)) = r.example {
        out.push_str("<div class=\"example\">\n<h4>Example</h4>\n<div class=\"ex\">\n");
        out.push_str("<pre class=\"ex-before\" aria-label=\"Before\">");
        out.push_str(&escape_html(before));
        out.push_str("</pre>\n<pre class=\"ex-after\" aria-label=\"After\">");
        out.push_str(&escape_html(after));
        out.push_str("</pre>\n</div>\n</div>\n");
    }
    out
}

/// Ruleset section header, with the counts generated rather than written so
/// they cannot go stale. The cards layout wraps its entries in a grid.
fn section(rs: &str, rules: &[RuleMeta], in_details: bool, grid: bool) -> String {
    let on = if rules.iter().all(|r| r.default_enabled) {
        "on by default"
    } else {
        "off by default"
    };
    let mut out = String::with_capacity(rules.len() * 2048);
    out.push_str("<section class=\"ruleset\" id=\"rs-");
    out.push_str(rs);
    out.push_str("\">\n<h2>");
    out.push_str(rs);
    out.push_str(" <span class=\"rs-meta\">");
    out.push_str(&rules.len().to_string());
    out.push_str(" rules, ");
    out.push_str(on);
    out.push_str("</span></h2>\n");
    if grid {
        out.push_str("<div class=\"grid\">\n");
    }
    for r in rules {
        out.push_str(&article(r, in_details));
    }
    if grid {
        out.push_str("</div>\n");
    }
    out.push_str("</section>\n");
    out
}

/// Navigation: the handbook gets a full table of contents (every rule), the
/// other two a one-line jump row over the rulesets.
fn toc(style: DocStyle, rules: &[RuleMeta]) -> String {
    let mut out = String::from("<nav class=\"toc\" aria-label=\"Rules\">\n");
    if style == DocStyle::Handbook {
        out.push_str("<h2>Contents</h2>\n");
        for rs in rulesets() {
            let in_rs: Vec<&RuleMeta> = rules.iter().filter(|r| r.ruleset == rs).collect();
            out.push_str("<section><h3><a href=\"#rs-");
            out.push_str(rs);
            out.push_str("\">");
            out.push_str(rs);
            out.push_str("</a></h3>\n<ul>\n");
            for r in in_rs {
                out.push_str("<li><a href=\"#");
                out.push_str(r.code);
                out.push_str("\"><code>");
                out.push_str(r.code);
                out.push_str("</code> ");
                out.push_str(r.name);
                out.push_str("</a></li>\n");
            }
            out.push_str("</ul>\n</section>\n");
        }
    } else {
        out.push_str("<p>");
        for (i, rs) in rulesets().into_iter().enumerate() {
            if i > 0 {
                out.push_str(" &middot; ");
            }
            let n = rules.iter().filter(|r| r.ruleset == rs).count();
            out.push_str("<a href=\"#rs-");
            out.push_str(rs);
            out.push_str("\">");
            out.push_str(rs);
            out.push_str(" (");
            out.push_str(&n.to_string());
            out.push_str(")</a>");
        }
        out.push_str("</p>\n");
    }
    out.push_str("</nav>\n");
    out
}

/// The "Using gedlint" section: run, read, fix, explain. Terminal commands
/// in a single copyable block, page links same-origin.
fn usage() -> String {
    let mut out = String::from("<section class=\"usage\" id=\"usage\">\n<h2>Using gedlint</h2>\n");
    out.push_str(
        "<p>You can check a file <a href=\"index.html\">right here in the browser</a> \
(nothing is uploaded), or with the command-line tool:</p>\n",
    );
    out.push_str(
        "<pre><code>gedlint tree.ged                # report: what will break on import\n\
gedlint --explain W401          # why one rule exists, how to satisfy it\n\
gedlint --fix tree.ged          # apply the provably safe repairs (.bak copy)\n\
gedlint --fix --unsafe tree.ged # also format guesses, marked needs review\n\
gedlint --format json tree.ged  # machine-readable, complete</code></pre>\n",
    );
    out.push_str("<p>Each finding reads <code>severity [code:category] line N: message</code>; \
the code links to its entry below. Rule groups can be switched and re-leveled per project with a \
<code>gedlint.toml</code> file, and every pull request can be checked with the \
<a href=\"https://github.com/pmontp19/gedlint#github-action\" rel=\"noopener\">GitHub Action</a>.</p>\n");
    out.push_str("</section>\n");
    out
}

/// The whole page over a given registry slice (tests render modified copies
/// without touching the static `RULES`). Byte-for-byte deterministic: no
/// timestamp, so two builds of the same registry produce identical output.
fn rules_html_for(style: DocStyle, rules: &[RuleMeta]) -> String {
    let in_details = style != DocStyle::Handbook;
    let mut out = String::with_capacity(rules.len() * 2600);
    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'self'; img-src data:; base-uri 'none'; form-action 'none'\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("<meta name=\"color-scheme\" content=\"light dark\">\n");
    out.push_str("<meta name=\"description\" content=\"Every gedlint rule: what breaks in Gramps, webtrees, RootsMagic and other genealogy software, and how to fix it.\">\n");
    out.push_str("<title>gedlint rules reference</title>\n");
    out.push_str("<link rel=\"stylesheet\" href=\"rules.css\">\n</head>\n");
    out.push_str("<body class=\"");
    out.push_str(style.marker());
    out.push_str("\" id=\"top\">\n");
    out.push_str("<header class=\"site-head\"><div class=\"wrap head-row\">");
    out.push_str("<span class=\"brand\">gedlint</span>");
    out.push_str("<nav class=\"head-nav\"><a href=\"index.html\">Linter</a>");
    out.push_str("<a href=\"https://github.com/pmontp19/gedlint\" rel=\"noopener\">Source</a>");
    out.push_str("</nav></div></header>\n");
    out.push_str("<main class=\"wrap\">\n<h1>Rule reference</h1>\n");
    out.push_str("<p class=\"lede\">");
    out.push_str(&rules.len().to_string());
    out.push_str(" rules check a GEDCOM file for the defects that break imports in Gramps, webtrees, RootsMagic and elsewhere. The same explanations print in the terminal: <code>gedlint --explain CODE</code>.</p>\n");
    out.push_str(&usage());
    out.push_str(&toc(style, rules));
    let mut i = 0;
    for rs in rulesets() {
        let n = rules[i..].iter().take_while(|r| r.ruleset == rs).count();
        if n > 0 {
            out.push_str(&section(
                rs,
                &rules[i..i + n],
                in_details,
                style == DocStyle::Cards,
            ));
        }
        i += n;
        if i >= rules.len() {
            break;
        }
    }
    out.push_str("<footer class=\"site-foot\"><p>Generated from the same rule registry the CLI prints with <code>--explain</code>, so the two cannot drift. &middot; <a href=\"#top\">Back to top</a></p></footer>\n");
    out.push_str("</main>\n</body>\n</html>\n");
    out
}

/// The page as shipped: the whole static registry.
pub fn rules_html(style: DocStyle) -> String {
    rules_html_for(style, RULES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_html_escapes_everything() {
        assert_eq!(escape_html("a<&>\"'b"), "a&lt;&amp;&gt;&quot;&#39;b");
        assert_eq!(escape_html("plain"), "plain");
    }

    #[test]
    fn prose_wraps_backticked_code() {
        assert_eq!(
            prose("run `gedlint --fix` on it"),
            "run <code>gedlint --fix</code> on it"
        );
        assert_eq!(prose("no code"), "no code");
        // Escaping still applies inside and outside code spans.
        assert_eq!(prose("`a&b`"), "<code>a&amp;b</code>");
    }

    #[test]
    fn registry_prose_has_balanced_backticks() {
        // The renderer assumes pairs; this pins the whole registry to it.
        for r in RULES {
            for t in [r.title, r.why, r.remedy] {
                assert_eq!(
                    t.matches('`').count() % 2,
                    0,
                    "{}: unbalanced backticks in {:?}",
                    r.code,
                    t
                );
            }
        }
    }

    #[test]
    fn style_parse_and_marker() {
        assert_eq!(DocStyle::parse("reference"), Some(DocStyle::Reference));
        assert_eq!(DocStyle::parse("handbook"), Some(DocStyle::Handbook));
        assert_eq!(DocStyle::parse("cards"), Some(DocStyle::Cards));
        assert_eq!(DocStyle::parse("nope"), None);
        assert_eq!(DocStyle::Cards.marker(), "style-cards");
    }

    #[test]
    fn page_anchors_every_rule() {
        for style in [DocStyle::Reference, DocStyle::Handbook, DocStyle::Cards] {
            let html = rules_html(style);
            assert!(html.starts_with("<!DOCTYPE html>"));
            for r in RULES {
                assert!(
                    html.contains(&format!("id=\"{}\"", r.code)),
                    "{} missing",
                    r.code
                );
                assert!(html.contains(&format!("href=\"#{}\"", r.code)));
                assert!(html.contains(r.name));
                assert!(html.contains(&prose(r.title)));
                assert!(html.contains(&prose(r.remedy)));
            }
            for rs in rulesets() {
                assert!(html.contains(&format!("id=\"rs-{}\"", rs)));
            }
            assert!(html.contains("id=\"usage\""));
            assert!(html.contains("gedlint --fix tree.ged"));
        }
    }

    #[test]
    fn styles_differ_structurally() {
        let reference = rules_html(DocStyle::Reference);
        let handbook = rules_html(DocStyle::Handbook);
        let cards = rules_html(DocStyle::Cards);
        assert!(reference.contains("class=\"style-reference\""));
        assert!(reference.contains("<details>"));
        assert!(!reference.contains("<h2>Contents</h2>"));
        assert!(handbook.contains("class=\"style-handbook\""));
        assert!(handbook.contains("<h2>Contents</h2>"));
        assert!(!handbook.contains("<details>"));
        assert!(cards.contains("class=\"style-cards\""));
        assert!(cards.contains("<details>"));
    }

    #[test]
    fn example_block_renders_when_present() {
        // All registry entries are None today; render a modified copy to pin
        // the markup the content PRs will fill in.
        let mut rules = RULES.to_vec();
        rules[0].example = Some((
            "1 NOTE first part\nbecause it lost the prefix",
            "1 NOTE first part\n2 CONT because it lost the prefix",
        ));
        let html = rules_html_for(DocStyle::Reference, &rules);
        assert!(html.contains("class=\"example\""));
        assert!(html.contains("ex-before"));
        assert!(html.contains("2 CONT because it lost the prefix"));
        // And the shipped page has none yet.
        assert!(!rules_html(DocStyle::Reference).contains("class=\"example\""));
    }
}
