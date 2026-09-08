//! `gedlint.toml`: presets and per-rule severity overrides.
//!
//! GEDCOM has no comment syntax, so there is no inline suppression and
//! there never can be: an injected pseudo-comment line is a grammar error.
//! The config file is therefore the only way a user can ever silence a
//! rule, which makes parsing it a correctness feature, not a convenience.
//! Consequences, per RFC 014 section 4:
//!
//! - Anything questionable is a hard error (`ConfigError`), never a silent
//!   no-op: an unknown rule key, an unknown preset, a malformed file.
//! - A rule is addressable by its code (`"U502"`) or by
//!   `<ruleset>/<name>` (`"core/invalid-sex-value"`), both resolved through
//!   the registry, so a typo cannot quietly configure nothing.
//! - The parser is a hand-rolled minimal TOML subset (sections, string
//!   values, string arrays), pure and dependency-free.

use std::collections::BTreeMap;
use std::fmt;

use crate::diag::Severity;
use crate::registry::{rule, rule_by_name, rulesets, RULES};

/// What configuration asks of one rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleLevel {
    /// Emit nothing for the rule; it must not affect the exit code either.
    Off,
    /// Emit at this severity instead of the rule's default one.
    Severity(Severity),
}

/// A parsed `gedlint.toml`. Build one with [`parse_config`]; the zero value
/// is the built-in behaviour (the `recommended` preset, no overrides) and is
/// exactly what the plain `lint_str` / `lint_bytes` entry points apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Presets named in `[lints]`, in file order. Absent key = `recommended`.
    /// An explicit empty list starts from silence: nothing is enabled except
    /// what `[lints.rules]` turns on.
    presets: Vec<String>,
    /// `[lints.rules]` entries, with the key already resolved to the
    /// canonical rule code by the registry.
    overrides: Vec<(&'static str, RuleLevel)>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            presets: vec!["recommended".to_string()],
            overrides: Vec::new(),
        }
    }
}

impl Config {
    /// The configured state of every rule that is not simply "on, emitting
    /// at whatever severity the rule itself produces". A rule **absent**
    /// from the map is on as emitted; present means `Off` (emit nothing) or
    /// `Severity(s)` (re-level to `s`). Only explicit `[lints.rules]`
    /// entries re-level: presets merely enable, and a rule may legitimately
    /// emit at different severities per finding (W306 does).
    pub fn effective(&self) -> BTreeMap<&'static str, RuleLevel> {
        // Start from silence: a rule no preset covers is off until an
        // explicit entry turns it on.
        let mut map: BTreeMap<&'static str, RuleLevel> =
            RULES.iter().map(|r| (r.code, RuleLevel::Off)).collect();
        // Presets decide which rules are on at all. Additive and in order:
        // "recommended" enables every core rule, a preset named after an
        // opt-in ruleset enables that ruleset's rules.
        for preset in &self.presets {
            for r in RULES {
                let on = (preset == "recommended" && r.ruleset == "core")
                    || preset.as_str() == r.ruleset;
                if on {
                    map.remove(r.code);
                }
            }
        }
        // Explicit entries beat presets, both ways: "off" silences a rule a
        // preset enabled, a severity re-levels one (and enables it if the
        // presets left it off).
        for (code, level) in &self.overrides {
            map.insert(code, *level);
        }
        map
    }

    /// Whether the rule `code` emits anything under this configuration.
    /// This is the gate the `--fix` side reads too (#44): a rule that is
    /// "off", or that no preset covers, must produce no edit either, so the
    /// repairs and the diagnostics of one run cannot disagree. A code with
    /// no registry entry (the `style` pseudo-code trailing whitespace
    /// carries) is always enabled: no configuration can name it.
    pub fn enables(&self, code: &str) -> bool {
        !matches!(self.effective().get(code), Some(RuleLevel::Off))
    }
}

/// Why a configuration file was refused. `line` is 1-based.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub line: usize,
    pub msg: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "config error at line {}: {}", self.line, self.msg)
    }
}

impl std::error::Error for ConfigError {}

fn err(line: usize, msg: impl Into<String>) -> ConfigError {
    ConfigError {
        line,
        msg: msg.into(),
    }
}

/// The preset names a config file may legally use: `recommended` plus one
/// per opt-in ruleset (RFC 014 section 4). Core is not one: it is what
/// `recommended` already means.
fn preset_names() -> Vec<String> {
    let mut names = vec!["\"recommended\"".to_string()];
    for rs in rulesets() {
        if rs != "core" {
            names.push(format!("\"{}\"", rs));
        }
    }
    names
}

/// Parse and validate a `gedlint.toml` file's text. Pure: text in, config
/// or a located error out, no filesystem access anywhere.
///
/// Only a minimal TOML subset is understood: `[lints]` and `[lints.rules]`
/// sections, `key = "string"` pairs and one string array (`presets`). Keys
/// may be quoted strings or bare `[A-Za-z0-9_-]` words; `#` starts a comment
/// outside a string. Anything else is a hard error.
pub fn parse_config(text: &str) -> Result<Config, ConfigError> {
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let mut presets: Option<Vec<String>> = None;
    let mut overrides: Vec<(&'static str, RuleLevel)> = Vec::new();
    let mut seen_lints = false;
    let mut seen_rules = false;
    // None = before any section header.
    let mut section: Option<Section> = None;

    for (i, raw) in text.lines().enumerate() {
        let no = i + 1;
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix('[') {
            let Some(close) = rest.find(']') else {
                return Err(err(no, "section header must close with ']'"));
            };
            let name = &rest[..close];
            let after = rest[close + 1..].trim();
            if !after.is_empty() {
                return Err(err(no, "unexpected text after the section header"));
            }
            section = Some(match name {
                "lints" => {
                    if seen_lints {
                        return Err(err(no, "section [lints] appears twice"));
                    }
                    seen_lints = true;
                    Section::Lints
                }
                "lints.rules" => {
                    if seen_rules {
                        return Err(err(no, "section [lints.rules] appears twice"));
                    }
                    seen_rules = true;
                    Section::Rules
                }
                other => {
                    return Err(err(
                        no,
                        format!("unknown section [{other}]: expected [lints] or [lints.rules]"),
                    ));
                }
            });
            continue;
        }

        let Some(section) = section else {
            return Err(err(
                no,
                "key before any [section]: start the file with [lints]",
            ));
        };
        // The separator is the first '=' outside a quoted key, so a key
        // like "my=rule" splits at the right place and reports as an
        // unknown rule instead of a mangled string.
        let Some(eq) = find_separator(line) else {
            return Err(err(no, "expected `key = value`"));
        };
        let key = parse_key(line[..eq].trim(), no)?;
        let value = line[eq + 1..].trim();

        match section {
            Section::Lints => {
                if key != "presets" {
                    return Err(err(
                        no,
                        format!("unknown key \"{key}\" in [lints]: only \"presets\" is allowed"),
                    ));
                }
                if presets.is_some() {
                    return Err(err(no, "presets is set twice"));
                }
                let list = parse_string_array(value, no)?;
                let mut resolved: Vec<String> = Vec::new();
                for p in &list {
                    let known = p == "recommended"
                        || rulesets()
                            .iter()
                            .any(|rs| *rs != "core" && *rs == p.as_str());
                    if !known {
                        return Err(err(
                            no,
                            format!(
                                "unknown preset \"{p}\": valid presets are {}",
                                preset_names().join(", ")
                            ),
                        ));
                    }
                    if resolved.iter().any(|r| r == p) {
                        return Err(err(no, format!("preset \"{p}\" is listed twice")));
                    }
                    resolved.push(p.clone());
                }
                presets = Some(resolved);
            }
            Section::Rules => {
                let meta = if let Some((rs, name)) = key.split_once('/') {
                    rule_by_name(rs, name)
                } else {
                    rule(&key)
                };
                let Some(meta) = meta else {
                    return Err(err(
                        no,
                        format!(
                            "unknown rule \"{key}\": address a rule by code, e.g. \"W305\", or as <ruleset>/<name>, e.g. \"core/invalid-sex-value\""
                        ),
                    ));
                };
                if overrides.iter().any(|(c, _)| *c == meta.code) {
                    return Err(err(no, format!("rule \"{key}\" is configured twice")));
                }
                let inner = parse_quoted(value, no)?;
                let level = match inner {
                    "off" => RuleLevel::Off,
                    "info" => RuleLevel::Severity(Severity::Info),
                    "warn" => RuleLevel::Severity(Severity::Warning),
                    "error" => RuleLevel::Severity(Severity::Error),
                    other => {
                        return Err(err(
                            no,
                            format!("invalid level \"{other}\" for {}: use \"off\", \"info\", \"warn\" or \"error\"", meta.code),
                        ));
                    }
                };
                overrides.push((meta.code, level));
            }
        }
    }

    Ok(Config {
        presets: presets.unwrap_or_else(|| vec!["recommended".to_string()]),
        overrides,
    })
}

#[derive(Clone, Copy)]
enum Section {
    Lints,
    Rules,
}

/// Drop a trailing `#` comment, but only outside a quoted string: inside
/// one, `#` is data. The subset has no escape sequences, so a quote simply
/// toggles.
fn strip_comment(line: &str) -> &str {
    let mut in_string = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_string = !in_string,
            '#' if !in_string => return &line[..i],
            _ => {}
        }
    }
    line
}

/// Byte index of the first `=` outside a quoted string, for the same
/// reason `strip_comment` tracks quotes.
fn find_separator(line: &str) -> Option<usize> {
    let mut in_string = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_string = !in_string,
            '=' if !in_string => return Some(i),
            _ => {}
        }
    }
    None
}

/// A bare word (`[A-Za-z0-9_-]+`) or a double-quoted string, with nothing
/// allowed after it.
fn parse_key(s: &str, no: usize) -> Result<String, ConfigError> {
    if let Some(rest) = s.strip_prefix('"') {
        let Some(end) = rest.find('"') else {
            return Err(err(no, "key string must close with '\"'"));
        };
        if !rest[end + 1..].trim().is_empty() {
            return Err(err(no, "unexpected text after the key"));
        }
        Ok(rest[..end].to_string())
    } else if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        Ok(s.to_string())
    } else {
        Err(err(
            no,
            "key must be a double-quoted string or bare letters, digits, '-' or '_'",
        ))
    }
}

/// One double-quoted string, with nothing allowed after it.
fn parse_quoted(s: &str, no: usize) -> Result<&str, ConfigError> {
    let Some(rest) = s.strip_prefix('"') else {
        return Err(err(
            no,
            "value must be a double-quoted string, e.g. \"off\"",
        ));
    };
    let Some(end) = rest.find('"') else {
        return Err(err(no, "value string must close with '\"'"));
    };
    if !rest[end + 1..].trim().is_empty() {
        return Err(err(no, "unexpected text after the value"));
    }
    Ok(&rest[..end])
}

/// A single-line array of double-quoted strings: `["a", "b"]`, `[]` and a
/// trailing comma are all fine.
fn parse_string_array(s: &str, no: usize) -> Result<Vec<String>, ConfigError> {
    let Some(inner) = s.strip_prefix('[') else {
        return Err(err(
            no,
            "presets must be an array of strings, e.g. [\"recommended\"]",
        ));
    };
    let Some(inner) = inner.strip_suffix(']') else {
        return Err(err(no, "array must open and close on the same line"));
    };
    let mut out = Vec::new();
    let mut rest = inner.trim();
    while !rest.is_empty() {
        let Some(after) = rest.strip_prefix('"') else {
            return Err(err(no, "array elements must be double-quoted strings"));
        };
        let Some(end) = after.find('"') else {
            return Err(err(no, "array element string must close with '\"'"));
        };
        out.push(after[..end].to_string());
        rest = after[end + 1..].trim_start();
        if let Some(next) = rest.strip_prefix(',') {
            rest = next.trim_start();
        } else if !rest.is_empty() {
            return Err(err(no, "expected ',' between array elements"));
        }
    }
    Ok(out)
}
