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

/// Numeric thresholds for the consistency rules. Every field has a default
/// matching the historical hardcoded value, so a file without a
/// `[lints.thresholds]` section behaves exactly as before. Pure data, no
/// fs/env access, WASM-safe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thresholds {
    /// W301: lifespan over this many years warns.
    pub max_lifespan: i64,
    /// W303: parent younger than this at a child's birth warns.
    pub min_parent_age: i64,
    /// W303: mother older than this at a child's birth warns.
    pub max_mother_age: i64,
    /// W303: father older than this at a child's birth warns.
    pub max_father_age: i64,
    /// W302: birth years this far apart (inclusive) count as duplicates.
    pub duplicate_window: i64,
    /// W704: sibling gaps up to this many days warn (twins excluded).
    pub sibling_max_gap: i64,
    /// W705: no DEAT and birth more than this many years before the file's
    /// latest year warns (the latest year in the file stands in for today,
    /// so the engine needs no clock and stays WASM-safe).
    pub max_alive_years: i64,
    /// W706: spouses' birth years further apart than this warn.
    pub max_spouse_gap: i64,
    /// W707: marriage (or death of a married person) younger than this warns.
    pub min_marriage_age: i64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            max_lifespan: 105,
            min_parent_age: 13,
            max_mother_age: 50,
            max_father_age: 70,
            duplicate_window: 2,
            sibling_max_gap: 240,
            max_alive_years: 110,
            max_spouse_gap: 25,
            min_marriage_age: 16,
        }
    }
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
    /// `[lints.thresholds]` entries. Absent keys keep their defaults.
    thresholds: Thresholds,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            presets: vec!["recommended".to_string()],
            overrides: Vec::new(),
            thresholds: Thresholds::default(),
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

    /// The numeric thresholds the consistency rules compare against.
    pub fn thresholds(&self) -> &Thresholds {
        &self.thresholds
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
/// Only a minimal TOML subset is understood: `[lints]`, `[lints.rules]` and
/// `[lints.thresholds]` sections, `key = "string"` pairs, bare integer
/// values for thresholds, and one string array (`presets`). Keys
/// may be quoted strings or bare `[A-Za-z0-9_-]` words; `#` starts a comment
/// outside a string. Anything else is a hard error.
pub fn parse_config(text: &str) -> Result<Config, ConfigError> {
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let mut presets: Option<Vec<String>> = None;
    let mut overrides: Vec<(&'static str, RuleLevel)> = Vec::new();
    let mut thresholds = Thresholds::default();
    let mut seen_threshold_keys: Vec<String> = Vec::new();
    let mut seen_lints = false;
    let mut seen_rules = false;
    let mut seen_thresholds = false;
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
                "lints.thresholds" => {
                    if seen_thresholds {
                        return Err(err(no, "section [lints.thresholds] appears twice"));
                    }
                    seen_thresholds = true;
                    Section::Thresholds
                }
                other => {
                    return Err(err(
                        no,
                        format!("unknown section [{other}]: expected [lints], [lints.rules] or [lints.thresholds]"),
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
            Section::Thresholds => {
                if seen_threshold_keys.iter().any(|k| k == &key) {
                    return Err(err(no, format!("threshold \"{key}\" is configured twice")));
                }
                let n = parse_threshold_value(value, no)?;
                match key.as_str() {
                    "max-lifespan" => {
                        check_threshold_range(no, &key, n, 50, 150)?;
                        thresholds.max_lifespan = n;
                    }
                    "min-parent-age" => {
                        check_threshold_range(no, &key, n, 0, 30)?;
                        thresholds.min_parent_age = n;
                    }
                    "max-mother-age" => {
                        check_threshold_range(no, &key, n, 0, 100)?;
                        thresholds.max_mother_age = n;
                    }
                    "max-father-age" => {
                        check_threshold_range(no, &key, n, 0, 120)?;
                        thresholds.max_father_age = n;
                    }
                    "duplicate-window" => {
                        check_threshold_range(no, &key, n, 0, 20)?;
                        thresholds.duplicate_window = n;
                    }
                    "sibling-max-gap" => {
                        check_threshold_range(no, &key, n, 0, 1000)?;
                        thresholds.sibling_max_gap = n;
                    }
                    "max-alive-years" => {
                        check_threshold_range(no, &key, n, 50, 300)?;
                        thresholds.max_alive_years = n;
                    }
                    "max-spouse-gap" => {
                        check_threshold_range(no, &key, n, 0, 100)?;
                        thresholds.max_spouse_gap = n;
                    }
                    "min-marriage-age" => {
                        check_threshold_range(no, &key, n, 0, 30)?;
                        thresholds.min_marriage_age = n;
                    }
                    _ => {
                        return Err(err(
                            no,
                            format!(
                                "unknown threshold \"{key}\": valid thresholds are {}",
                                threshold_names().join(", ")
                            ),
                        ));
                    }
                }
                seen_threshold_keys.push(key);
            }
        }
    }

    Ok(Config {
        presets: presets.unwrap_or_else(|| vec!["recommended".to_string()]),
        overrides,
        thresholds,
    })
}

#[derive(Clone, Copy)]
enum Section {
    Lints,
    Rules,
    Thresholds,
}

/// Every threshold key a `[lints.thresholds]` section may set, for the
/// unknown-key error. Quoted in the message the way presets are.
fn threshold_names() -> Vec<String> {
    [
        "max-lifespan",
        "min-parent-age",
        "max-mother-age",
        "max-father-age",
        "duplicate-window",
        "sibling-max-gap",
        "max-alive-years",
        "max-spouse-gap",
        "min-marriage-age",
    ]
    .iter()
    .map(|k| format!("\"{k}\""))
    .collect()
}

/// A bare integer threshold value: no quotes, no decimals, no signs.
fn parse_threshold_value(s: &str, no: usize) -> Result<i64, ConfigError> {
    let v = s.trim();
    if v.starts_with('"') {
        return Err(err(
            no,
            "threshold values must be bare integers, e.g. max-lifespan = 105",
        ));
    }
    if v.is_empty() || !v.chars().all(|c| c.is_ascii_digit()) {
        return Err(err(
            no,
            "threshold values must be bare integers, e.g. max-lifespan = 105",
        ));
    }
    v.parse::<i64>()
        .map_err(|_| err(no, "threshold value is too large"))
}

/// Range guard so a typo (a 1000-year lifespan, a negative age) fails loudly
/// instead of silently disabling the rule it tunes.
fn check_threshold_range(
    no: usize,
    key: &str,
    n: i64,
    lo: i64,
    hi: i64,
) -> Result<(), ConfigError> {
    if !(lo..=hi).contains(&n) {
        return Err(err(
            no,
            format!("threshold \"{key}\" must be between {lo} and {hi}, got {n}"),
        ));
    }
    Ok(())
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
