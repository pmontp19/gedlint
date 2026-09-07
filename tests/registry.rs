//! The registry must describe exactly the rules the engine has, no more and
//! no fewer. An orphan entry or an undocumented code fails here rather than
//! reaching the config validator, the generated docs or the web viewer.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use gedlint::{rule, rule_by_name, rulesets, Category, RULES};

/// Every `"E001"`-shaped literal in the engine sources. Codes are always
/// written out at the `Diag::new` call site (nothing builds one at runtime),
/// so this is the set of codes the engine can actually emit.
fn engine_codes() -> BTreeSet<String> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = BTreeSet::new();
    for f in rs_files(&src) {
        // registry.rs is the table under test; main.rs only renders it.
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        if name == "registry.rs" || name == "main.rs" {
            continue;
        }
        collect_codes(&fs::read_to_string(&f).unwrap(), &mut out);
    }
    assert!(!out.is_empty(), "no rule codes found under src/: the scanner is broken, not the registry");
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

fn collect_codes(text: &str, out: &mut BTreeSet<String>) {
    let b = text.as_bytes();
    for i in 0..b.len().saturating_sub(5) {
        let is_code = b[i] == b'"'
            && matches!(b[i + 1], b'E' | b'W' | b'U')
            && b[i + 2..i + 5].iter().all(u8::is_ascii_digit)
            && b[i + 5] == b'"';
        if is_code {
            out.insert(text[i + 1..i + 5].to_string());
        }
    }
}

#[test]
fn registry_and_engine_agree_on_the_set_of_codes() {
    let engine = engine_codes();
    let registry: BTreeSet<String> = RULES.iter().map(|r| r.code.to_string()).collect();
    let undocumented: Vec<&String> = engine.difference(&registry).collect();
    let orphans: Vec<&String> = registry.difference(&engine).collect();
    assert!(undocumented.is_empty(), "codes the engine emits with no RULES entry: {:?}", undocumented);
    assert!(orphans.is_empty(), "RULES entries the engine never emits: {:?}", orphans);
}

#[test]
fn codes_are_unique_and_sorted() {
    let codes: Vec<&str> = RULES.iter().map(|r| r.code).collect();
    let mut sorted = codes.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(codes, sorted, "RULES must be sorted by code, with no duplicates");
}

#[test]
fn names_are_unique_kebab_case_inside_their_ruleset() {
    let mut seen: BTreeSet<(&str, &str)> = BTreeSet::new();
    for r in RULES {
        assert!(seen.insert((r.ruleset, r.name)), "duplicate name {}/{}", r.ruleset, r.name);
        assert!(!r.name.is_empty() && !r.name.starts_with('-') && !r.name.ends_with('-'), "bad name: {}", r.name);
        assert!(
            r.name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
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
    assert_eq!(sets, vec!["core"], "everything shipped today is core (RFC 014 section 0.4)");
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
        assert!(!r.title.ends_with('.'), "{}: the title is a heading, not a sentence", r.code);
        // Long enough to name what breaks and what to do: a stub fails here.
        assert!(r.why.len() > 120, "{}: why must say what breaks in consumer software", r.code);
        assert!(r.remedy.len() > 80, "{}: remedy must say what to do instead", r.code);
        assert!(r.why.ends_with('.'), "{}: why must be prose", r.code);
        assert!(r.remedy.ends_with('.'), "{}: remedy must be prose", r.code);
        // Written for a genealogist: the jargon of the tool stays out of it.
        for (field, text) in [("why", r.why), ("remedy", r.remedy)] {
            assert!(!text.contains("linter"), "{}: {} should not mention the linter", r.code, field);
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
        format!("{head}0 @F1@ FAM\n1 CHIL @I1@\n0 @I1@ INDI\n1 NAME A /B/\n1 NOTE x<br>y\n0 TRLR\n"),
    ];
    let mut checked = 0;
    for g in &corpus {
        for d in gedlint::lint_str(g).diags {
            let meta = rule(d.code).unwrap_or_else(|| panic!("{} is not in the registry", d.code));
            assert_eq!(meta.category, d.category, "{}: category drifted from the registry", d.code);
            checked += 1;
        }
    }
    assert!(checked >= 10, "the corpus must exercise several rules, got {}", checked);
    // Sanity: the four categories are all represented in the table.
    for c in [Category::Correctness, Category::Suspicious, Category::Style, Category::Upgrade] {
        assert!(RULES.iter().any(|r| r.category == c), "no rule in category {}", c.as_str());
    }
}

#[test]
fn fixable_marks_exactly_what_fix_bytes_repairs() {
    // A file with both repairs: an orphan line (E001) and a UTF-8 character
    // split across CONC lines (E101).
    let mut src: Vec<u8> = b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @S1@ SOUR\n1 DATA\n2 TEXT caf".to_vec();
    src.push(0xC3);
    src.extend_from_slice(b"\n2 CONC ");
    src.push(0xA9);
    src.extend_from_slice(b"\norphan line\n0 TRLR\n");
    let (_, applied) = gedlint::fix_bytes(&src);
    let repaired: BTreeSet<&str> = applied
        .iter()
        .filter_map(|note| note.split_once(':'))
        .map(|(code, _)| code)
        .filter(|code| rule(code).is_some())
        .collect();
    let marked: BTreeSet<&str> = RULES.iter().filter(|r| r.fixable).map(|r| r.code).collect();
    assert_eq!(repaired, marked, "fixable in RULES must match what --fix actually repairs: {:?}", applied);
}
