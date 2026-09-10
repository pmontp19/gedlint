//! The registry must describe exactly the rules the engine has, no more and
//! no fewer. An orphan entry or an undocumented code fails here rather than
//! reaching the config validator, the generated docs or the web viewer.
//!
//! The set of codes the engine can emit is read out of its sources. That is
//! sound only because `every_diagnostic_carries_a_literal_code` forbids a
//! computed code at the call site, and because comments are stripped before
//! anything is collected, so a code quoted in prose cannot invent one.
//!
//! What no static check here can see: a rule whose code literal is present
//! but whose emission path is dead. `every_core_rule_has_a_fixture` is the
//! partial guard, partial because it only asserts the code is named in
//! `tests/rules.rs`, not that the fixture still triggers it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use gedlint::{rule, rule_by_name, rulesets, Applicability, Category, RULES};

/// The constructors that can put a diagnostic into a report. `with_span` is
/// added by #18 and has no call sites yet; naming it now means the invariant
/// covers it the day it does.
const DIAG_CTORS: &[&str] = &["Diag::new(", "Diag::with_span("];

/// Every rule code the engine can emit: the first argument of every
/// diagnostic constructor call in the engine sources.
fn engine_codes() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (_, arg) in diag_call_args() {
        if let Some(c) = code_literal(&arg) {
            out.insert(c);
        }
    }
    assert!(
        !out.is_empty(),
        "no rule codes found under src/: the scanner is broken, not the registry"
    );
    out
}

/// `(file, the first argument as written)` for every diagnostic constructor
/// call in the engine, comments already stripped.
fn diag_call_args() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for f in engine_files() {
        let file = f.file_name().unwrap().to_string_lossy().into_owned();
        let text = strip_comments(&fs::read_to_string(&f).unwrap());
        for ctor in DIAG_CTORS {
            let mut from = 0;
            while let Some(p) = text[from..].find(ctor) {
                let at = from + p + ctor.len();
                // Enough to see a code literal; the call is often multi-line.
                out.push((
                    file.clone(),
                    text[at..].trim_start().chars().take(24).collect(),
                ));
                from = at;
            }
        }
    }
    out
}

/// The code in `"E001", ...`, or None when the argument is anything else.
fn code_literal(arg: &str) -> Option<String> {
    let b = arg.as_bytes();
    let ok = b.len() >= 6
        && b[0] == b'"'
        && matches!(b[1], b'E' | b'W' | b'U')
        && b[2..5].iter().all(u8::is_ascii_digit)
        && b[5] == b'"';
    ok.then(|| arg[1..5].to_string())
}

/// The engine sources. `registry.rs` is the table under test and `main.rs`
/// only renders it; neither can emit a diagnostic.
fn engine_files() -> Vec<PathBuf> {
    let mut out = rs_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));
    out.retain(|f| {
        !matches!(
            f.file_name().unwrap().to_string_lossy().as_ref(),
            "registry.rs" | "main.rs"
        )
    });
    out.sort();
    out
}

fn rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(rs_files(&p));
        } else if p.extension().map(|x| x == "rs").unwrap_or(false) {
            out.push(p);
        }
    }
    out
}

/// Rust source with line and block comments removed. String and char
/// literals are copied verbatim: `'"'` in `src/diag.rs` would otherwise open
/// a string that swallows real code.
fn strip_comments(text: &str) -> String {
    let b = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\'' {
            let n = char_literal_len(b, i);
            out.extend_from_slice(&b[i..i + n]);
            i += n;
        } else if b[i] == b'"' {
            out.push(b'"');
            i += 1;
            while i < b.len() && b[i] != b'"' {
                let n = if b[i] == b'\\' && i + 1 < b.len() {
                    2
                } else {
                    1
                };
                out.extend_from_slice(&b[i..i + n]);
                i += n;
            }
            if i < b.len() {
                out.push(b'"');
                i += 1;
            }
        } else if b[i] == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
            let mut depth = 1;
            i += 2;
            while i < b.len() && depth > 0 {
                if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                    depth += 1;
                    i += 2;
                } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            out.push(b' ');
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).expect("only ASCII delimiters are ever sliced on")
}

/// Length of the char literal at `i`, or 1 for a lifetime such as `'static`.
fn char_literal_len(b: &[u8], i: usize) -> usize {
    if b.get(i + 1) == Some(&b'\\') {
        // '\n', '\\', '\'', '\u{7f}': past the escaped char, then the quote.
        let mut j = i + 3;
        while j < b.len() && b[j] != b'\'' {
            j += 1;
        }
        if j < b.len() {
            j - i + 1
        } else {
            1
        }
    } else if b.get(i + 2) == Some(&b'\'') {
        3
    } else {
        1
    }
}

#[test]
fn every_diagnostic_carries_a_literal_code() {
    // The completeness check below reads codes out of the sources, so a code
    // assembled at runtime would ship a rule that no static check can see.
    let sites = diag_call_args();
    assert!(
        sites.len() >= 40,
        "only {} diagnostic constructor calls found: the scanner is broken",
        sites.len()
    );
    for (file, arg) in &sites {
        assert!(
            code_literal(arg).is_some(),
            "{}: a diagnostic code must be a bare \"E001\"-shaped literal at the call site, \
             or the registry cannot be checked against it; found: {}",
            file,
            arg
        );
    }
}

#[test]
fn registry_and_engine_agree_on_the_set_of_codes() {
    let engine = engine_codes();
    let registry: BTreeSet<String> = RULES.iter().map(|r| r.code.to_string()).collect();
    let undocumented: Vec<&String> = engine.difference(&registry).collect();
    let orphans: Vec<&String> = registry.difference(&engine).collect();
    assert!(
        undocumented.is_empty(),
        "codes the engine emits with no RULES entry: {:?}",
        undocumented
    );
    assert!(
        orphans.is_empty(),
        "RULES entries the engine never emits: {:?}",
        orphans
    );
}

#[test]
fn every_core_rule_has_a_fixture() {
    // Deliberately weak: it proves the code is named in the per-rule fixture
    // file, not that the fixture still triggers it. It is the only thing
    // standing between a rule whose emission path goes dead and a green run,
    // because the code literal stays in the sources either way.
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("rules.rs");
    let fixtures = fs::read_to_string(path).unwrap();
    for r in RULES.iter().filter(|r| r.ruleset == "core") {
        assert!(
            fixtures.contains(r.code),
            "{} has no fixture in tests/rules.rs",
            r.code
        );
    }
}

#[test]
fn codes_are_unique_and_sorted() {
    let codes: Vec<&str> = RULES.iter().map(|r| r.code).collect();
    let mut sorted = codes.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        codes, sorted,
        "RULES must be sorted by code, with no duplicates"
    );
}

#[test]
fn names_are_unique_kebab_case_inside_their_ruleset() {
    let mut seen: BTreeSet<(&str, &str)> = BTreeSet::new();
    for r in RULES {
        assert!(
            seen.insert((r.ruleset, r.name)),
            "duplicate name {}/{}",
            r.ruleset,
            r.name
        );
        assert!(
            !r.name.is_empty() && !r.name.starts_with('-') && !r.name.ends_with('-'),
            "bad name: {}",
            r.name
        );
        assert!(
            r.name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "name must be kebab-case: {}",
            r.name
        );
        assert!(!r.ruleset.is_empty(), "{} has no ruleset", r.code);
    }
}

#[test]
fn lookup_by_code_and_by_ruleset_name() {
    let w202 = rule("W202").expect("W202 is in the registry");
    assert_eq!(w202.name, "asymmetric-famc-chil");
    assert_eq!(w202.ruleset, "core");
    assert_eq!(rule_by_name("core", "asymmetric-famc-chil"), Some(w202));
    // Both lookups are exact: no case folding, no ruleset guessing.
    assert_eq!(rule("w202"), None);
    assert_eq!(rule("W999"), None);
    assert_eq!(rule_by_name("hygiene", "asymmetric-famc-chil"), None);
    assert_eq!(rule_by_name("core", "no-such-rule"), None);
    for r in RULES {
        assert_eq!(rule(r.code), Some(r));
        assert_eq!(rule_by_name(r.ruleset, r.name), Some(r));
    }
}

#[test]
fn rulesets_lists_every_ruleset_once() {
    let sets = rulesets();
    assert_eq!(
        sets,
        vec!["core", "hispanic-naming", "hygiene"],
        "all expected rulesets are listed (RFC 014 section 0.4)"
    );
    for r in RULES {
        assert!(sets.contains(&r.ruleset));
    }
}

#[test]
fn only_core_is_enabled_by_default() {
    for r in RULES {
        assert_eq!(
            r.default_enabled,
            r.ruleset == "core",
            "{}: a non-core ruleset is opt-in (RFC 014 section 0.2)",
            r.code
        );
    }
}

#[test]
fn every_rule_carries_usable_prose() {
    for r in RULES {
        assert!(r.title.len() > 15, "{}: title too short", r.code);
        assert!(
            !r.title.ends_with('.'),
            "{}: the title is a heading, not a sentence",
            r.code
        );
        // Long enough to name what breaks and what to do: a stub fails here.
        assert!(
            r.why.len() > 120,
            "{}: why must say what breaks in consumer software",
            r.code
        );
        assert!(
            r.remedy.len() > 80,
            "{}: remedy must say what to do instead",
            r.code
        );
        assert!(r.why.ends_with('.'), "{}: why must be prose", r.code);
        assert!(r.remedy.ends_with('.'), "{}: remedy must be prose", r.code);
        // Written for a genealogist: the jargon of the tool stays out of it.
        for (field, text) in [("why", r.why), ("remedy", r.remedy)] {
            assert!(
                !text.contains("linter"),
                "{}: {} should not mention the linter",
                r.code,
                field
            );
        }
    }
}

#[test]
fn category_matches_what_the_engine_emits() {
    // The registry's category is public API (JSON, scripts/gh-report.js): it
    // must be the same one the diagnostic carries.
    let head = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR UTF-8\n";
    let corpus = [
        format!("{head}0 @I1@ INDI\n3 NAME A /B/\n0 TRLR\n"),
        format!("{head}0 @I1@ INDI\n1 NAME A /B/\n1 FAMC @F9@\n0 TRLR\n"),
        format!("{head}0 @I1@ INDI\n1 NAME A /B/\n1 SEX Q\n1 PLAC http://x\n0 TRLR\n"),
        format!("{head}0 @I1@ INDI\n1 NAME A /B\n1 BIRT\n2 DATE about 1900\n0 TRLR\n"),
        format!("{head}0 @I1@ INDI\n1 NAME A /B/\n1 _UPD X\n1 ASSO @I1@\n2 RELA cosi\n0 TRLR\n"),
        format!(
            "{head}0 @F1@ FAM\n1 CHIL @I1@\n0 @I1@ INDI\n1 NAME A /B/\n1 NOTE x<br>y\n0 TRLR\n"
        ),
    ];
    let mut checked = 0;
    for g in &corpus {
        for d in gedlint::lint_str(g).diags {
            let meta = rule(d.code).unwrap_or_else(|| panic!("{} is not in the registry", d.code));
            assert_eq!(
                meta.category, d.category,
                "{}: category drifted from the registry",
                d.code
            );
            checked += 1;
        }
    }
    assert!(
        checked >= 10,
        "the corpus must exercise several rules, got {}",
        checked
    );
    // Sanity: the four categories are all represented in the table.
    for c in [
        Category::Correctness,
        Category::Suspicious,
        Category::Style,
        Category::Upgrade,
    ] {
        assert!(
            RULES.iter().any(|r| r.category == c),
            "no rule in category {}",
            c.as_str()
        );
    }
}

#[test]
fn fixable_matches_the_repairs_fix_really_carries() {
    // A file with both repairs: an orphan line (E001) and a UTF-8 character
    // split across CONC lines (E101). Kept dynamic on purpose: a rule that
    // gains a repair the fixture does not trigger fails here, which is the
    // prompt to extend the fixture rather than to hardcode a list.
    let mut src: Vec<u8> =
        b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @S1@ SOUR\n1 DATA\n2 TEXT caf".to_vec();
    src.push(0xC3);
    src.extend_from_slice(b"\n2 CONC ");
    src.push(0xA9);
    src.extend_from_slice(
        b"\norphan line\n0 @I1@ INDI\n1 NAME Joan /CRUZ, LOPEZ/\n0 @I2@ INDI\n1 NAME A, /DE LA O/\n1 BIRT\n2 PLAC Reus,, Spain\n",
    );
    // E005: a CONT nested under a CONT (#51).
    src.extend_from_slice(b"0 @I3@ INDI\n1 NOTE some text\n2 CONT first\n3 CONT second\n0 TRLR\n");

    // Repairs are config-gated (#44), so this table check runs with every
    // ruleset enabled: the default config would (correctly) propose none of
    // the opt-in ruleset repairs.
    let all_on = gedlint::parse_config(
        "[lints]\npresets = [\"recommended\", \"hispanic-naming\", \"hygiene\"]\n",
    )
    .unwrap();

    // What the engine proposes, with the applicability it proposes it at.
    // `compute_edits` also emits the "style" pseudo-code for trailing
    // whitespace, which is whole-file cosmetics with no rule behind it.
    let (normalized, _) = gedlint::normalize_endings(&src);
    let mut proposed: BTreeMap<&'static str, Applicability> = BTreeMap::new();
    for e in gedlint::compute_edits_with(&normalized, &all_on) {
        if rule(e.code).is_none() {
            continue;
        }
        if let Some(prev) = proposed.insert(e.code, e.applicability) {
            assert_eq!(
                prev, e.applicability,
                "{}: two applicabilities, RuleMeta.fixable records one",
                e.code
            );
        }
    }
    let marked: BTreeMap<&'static str, Applicability> = RULES
        .iter()
        .filter_map(|r| r.fixable.map(|a| (r.code, a)))
        .collect();
    assert_eq!(
        proposed, marked,
        "RuleMeta.fixable must match the edits compute_edits carries"
    );

    // And end to end: a bare --fix under the same config applies the Safe
    // ones and says so.
    let (_, applied) = gedlint::fix_bytes_with(&src, &gedlint::FixSelection::default(), &all_on);
    let reported: BTreeSet<&str> = applied
        .iter()
        .filter_map(|note| note.split_once(':'))
        .map(|(code, _)| code)
        .filter(|code| rule(code).is_some())
        .collect();
    let safe: BTreeSet<&str> = RULES
        .iter()
        .filter(|r| r.fixable == Some(Applicability::Safe))
        .map(|r| r.code)
        .collect();
    assert_eq!(
        reported, safe,
        "a bare --fix applies exactly the Safe repairs: {:?}",
        applied
    );
}
