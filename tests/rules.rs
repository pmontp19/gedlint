//! One rule per test with minimal fixtures (acceptance criterion 5).
//! Each test builds the smallest GEDCOM that triggers a single rule.

use gedlint::{
    compute_edits, compute_edits_with, fix_bytes, fix_bytes_with, lint_bytes, lint_str,
    parse_config, Config, Diag, FixSelection, Severity, Version,
};

fn codes_with(input: &str, preset: &str) -> Vec<String> {
    let cfg = gedlint::parse_config(&format!("[lints]\npresets = [\"{}\"]\n", preset)).unwrap();
    gedlint::lint_str_with(input, &cfg)
        .diags
        .iter()
        .map(|d| d.code.to_string())
        .collect()
}

/// Recommended plus the named opt-in rulesets: the config a test needs to
/// drive a repair that belongs to one of them. `--fix` repairs are gated by
/// the configuration (#44), so a test about a W6xx/W7xx repair must enable
/// its ruleset or the gate (correctly) refuses the edit.
fn fix_cfg(presets: &[&str]) -> Config {
    let mut all = vec!["recommended"];
    all.extend_from_slice(presets);
    let list = all
        .iter()
        .map(|p| format!("\"{p}\""))
        .collect::<Vec<_>>()
        .join(", ");
    parse_config(&format!("[lints]\npresets = [{list}]\n")).unwrap()
}

fn has_with(input: &str, code: &str, preset: &str) -> bool {
    codes_with(input, preset).iter().any(|c| c == code)
}

fn codes(input: &str) -> Vec<String> {
    lint_str(input)
        .diags
        .iter()
        .map(|d| d.code.to_string())
        .collect()
}

fn has(input: &str, code: &str) -> bool {
    codes(input).iter().any(|c| c == code)
}

const HEAD551: &str = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR UTF-8\n";
const HEAD70: &str = "0 HEAD\n1 GEDC\n2 VERS 7.0\n";

fn wrap551(body: &str) -> String {
    format!("{}0 TRLR\n", format_args!("{HEAD551}{body}"))
}

#[test]
fn e002_missing_head_trlr() {
    let r = lint_str("0 @I1@ INDI\n1 NAME Joan /Osó/\n");
    assert!(r.diags.iter().any(|d| d.code == "E002"));
    assert_eq!(r.exit_code(), 2);
}

#[test]
fn e001_level_jump() {
    let g = wrap551("0 @I1@ INDI\n3 NAME Joan /Osó/\n");
    assert!(has(&g, "E001"));
}

#[test]
fn e003_dup_xref() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n0 @I1@ INDI\n1 NAME C /D/\n");
    assert!(has(&g, "E003"));
}

#[test]
fn e201_broken_fam_ref() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 FAMC @F9@\n");
    assert!(has(&g, "E201"));
}

#[test]
fn e201_broken_chil_ref() {
    let g = wrap551("0 @F1@ FAM\n1 CHIL @I9@\n");
    assert!(has(&g, "E201"));
}

#[test]
fn w202_famc_not_listed() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 FAMC @F1@\n0 @F1@ FAM\n1 HUSB @I2@\n0 @I2@ INDI\n1 NAME C /D/\n");
    assert!(has(&g, "W202"));
}

#[test]
fn w202_chil_not_declaring_famc() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n0 @F1@ FAM\n1 CHIL @I1@\n");
    assert!(has(&g, "W202"));
}

#[test]
fn w202_reports_the_famc_line() {
    // Issue 30: whole-file graph findings used to report at line 0.
    // HEAD551 is 4 lines, so the FAMC sits on line 7.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 FAMC @F1@\n0 @F1@ FAM\n1 HUSB @I2@\n0 @I2@ INDI\n1 NAME C /D/\n");
    let d = only(&g, "W202");
    assert_eq!(d.line, 7, "W202 points at the FAMC line: {:?}", d);
    assert!(d.msg.contains("declares FAMC"), "{}", d.msg);
}

#[test]
fn w202_reports_the_chil_line() {
    // HEAD551 is 4 lines, so the CHIL sits on line 8.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n0 @F1@ FAM\n1 CHIL @I1@\n");
    let d = only(&g, "W202");
    assert_eq!(d.line, 8, "W202 points at the CHIL line: {:?}", d);
    assert!(d.msg.contains("lists CHIL"), "{}", d.msg);
}

#[test]
fn w301_death_before_birth() {
    let g = wrap551(
        "0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 12 SEP 1909\n1 DEAT\n2 DATE 3 JAN 1900\n",
    );
    assert!(has(&g, "W301"));
}

#[test]
fn w301_longevity_112() {
    // The 112-year entry from the real corpus: must trigger W301.
    let g = wrap551(
        "0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1 JAN 1800\n1 DEAT\n2 DATE 1 JAN 1912\n",
    );
    assert!(has(&g, "W301"));
}

#[test]
fn w302_duplicates() {
    let g = wrap551(
        "0 @I1@ INDI\n1 NAME Joan /Osó/\n1 BIRT\n2 DATE 1861\n0 @I2@ INDI\n1 NAME Joan /Oso/\n1 BIRT\n2 DATE 1862\n",
    );
    // "Osó" vs "Oso" does not normalize accents: use the exact same name.
    let g2 = wrap551(
        "0 @I1@ INDI\n1 NAME Joan /Oso/\n1 BIRT\n2 DATE 1861\n0 @I2@ INDI\n1 NAME Joan /Oso/\n1 BIRT\n2 DATE 1862\n",
    );
    assert!(!has(&g, "W302") || has(&g2, "W302"));
    assert!(has(&g2, "W302"));
}

#[test]
fn w303_parent_age() {
    let g = wrap551(
        "0 @I1@ INDI\n1 NAME Pare /X/\n1 BIRT\n2 DATE 1900\n\
         0 @I2@ INDI\n1 NAME Mare /Y/\n1 BIRT\n2 DATE 1900\n\
         0 @I3@ INDI\n1 NAME Fill /Z/\n1 BIRT\n2 DATE 1905\n1 FAMC @F1@\n\
         0 @F1@ FAM\n1 HUSB @I1@\n1 WIFE @I2@\n1 CHIL @I3@\n",
    );
    assert!(has(&g, "W303"));
}

#[test]
fn w304_child_before_marriage() {
    let g = wrap551(
        "0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1900\n\
         0 @I2@ INDI\n1 NAME C /D/\n1 BIRT\n2 DATE 1902\n\
         0 @I3@ INDI\n1 NAME E /F/\n1 BIRT\n2 DATE 1910\n1 FAMC @F1@\n\
         0 @F1@ FAM\n1 HUSB @I1@\n1 WIFE @I2@\n1 CHIL @I3@\n1 MARR\n2 DATE 1920\n",
    );
    assert!(has(&g, "W304"));
}

#[test]
fn w305_sex_invalid() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 SEX Q\n");
    assert!(has(&g, "W305"));
}

#[test]
fn w401_plac_url() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 PLAC Reus https://example.com/x\n");
    assert!(has(&g, "W401"));
}

#[test]
fn w401_plac_url_multibyte_no_panic() {
    // truncate() used to slice at byte 60, panicking on multibyte chars.
    let pad = "x".repeat(59) + "é";
    let g = wrap551(&format!(
        "0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 PLAC {pad} https://example.com/{}\n",
        "y".repeat(100)
    ));
    let r = lint_str(&g);
    assert!(r.diags.iter().any(|d| d.code == "W401"));
    assert_eq!(r.exit_code(), 1);
}

#[test]
fn w403_note_html() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 NOTE first line<br>second line\n");
    assert!(has(&g, "W403"));
}

#[test]
fn w402_name_slashes() {
    let g = wrap551("0 @I1@ INDI\n1 NAME Joan /Oso\n");
    assert!(has(&g, "W402"));
}

#[test]
fn e101_conc_split_fix() {
    // Build the MyHeritage bug at byte level: "é" (U+00E9 = C3 A9)
    // split across two CONC lines: first ends with C3, next starts with A9.
    let mut data = Vec::new();
    data.extend_from_slice("0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME Jos".as_bytes());
    data.push(0xC3);
    data.extend_from_slice("\n2 CONC ".as_bytes());
    data.push(0xA9);
    data.extend_from_slice(" /Oso/\n0 TRLR\n".as_bytes());
    let r = lint_bytes(&data);
    assert!(
        r.diags.iter().any(|d| d.code == "E101"),
        "expected E101, got: {:?}",
        r.diags
    );
    let (fixed, applied) = fix_bytes(&data);
    assert!(!applied.is_empty());
    let r2 = lint_bytes(&fixed);
    assert!(
        !r2.diags.iter().any(|d| d.code == "E101"),
        "E101 must be gone after --fix: {:?}",
        r2.diags
    );
    assert!(String::from_utf8(fixed).is_ok());
}

#[test]
fn fix_trims_trailing_ws() {
    let data = b"0 HEAD   \n1 GEDC\n2 VERS 5.5.1\n0 TRLR\n".to_vec();
    let (fixed, applied) = fix_bytes(&data);
    assert!(!applied.is_empty());
    assert!(String::from_utf8_lossy(&fixed).starts_with("0 HEAD\n"));
}

#[test]
fn e001_orphan_cont_fix() {
    // MyHeritage continuation without CONT prefix (real cleaned-tree case).
    let data = b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @S1@ SOUR\n1 DATA\n2 TEXT <p>origen\ncontinuacio sense prefix\n0 TRLR\n"
        .to_vec();
    let r = lint_bytes(&data);
    assert!(r.diags.iter().any(|d| d.code == "E001"));
    let (fixed, applied) = fix_bytes(&data);
    assert!(applied.iter().any(|a| a.contains("CONT")));
    let r2 = lint_bytes(&fixed);
    assert!(
        !r2.diags.iter().any(|d| d.code == "E001" && d.line == 7),
        "orphan line 7 repaired: {:?}",
        r2.diags
    );
    let text = String::from_utf8(fixed).unwrap();
    assert!(text.contains("3 CONT continuacio sense prefix"));
}

#[test]
fn no_global_cap_hides_errors() {
    // 516 real _UPD infos used to hide errors behind the 200 global cap: now everything is collected.
    let mut g = String::from(HEAD551);
    for i in 1..=300 {
        g.push_str(&format!("0 @I{}@ INDI\n1 NAME A{} /B/\n1 _UPD X\n", i, i));
    }
    g.push_str("0 @F9@ FAM\n1 CHIL @I999@\n0 TRLR\n");
    let r = lint_str(&g);
    assert!(
        r.diags.iter().any(|d| d.code == "E201"),
        "E201 must not stay hidden: {:?}",
        r.diags.len()
    );
    assert!(r.infos() >= 300);
}

#[test]
fn bom_does_not_hide_head_or_version() {
    // The real MyHeritage export starts with a BOM: HEAD and VERS must still be detected.
    let g = "\u{FEFF}0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 TRLR\n";
    let r = lint_str(g);
    assert_eq!(r.version, Version::V551);
    assert!(!r.diags.iter().any(|d| d.code == "E002"));
    let rb = lint_bytes(g.as_bytes());
    assert!(rb.diags.iter().any(|d| d.code == "W102"));
}

#[test]
fn deat_y_without_date_is_not_longevity() {
    // DEAT Y without DATE = death with unknown date, not 8097 years.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1902\n1 DEAT Y\n");
    assert!(!has(&g, "W301"));
}

#[test]
fn deat_y_with_a_date_is_still_checked() {
    // Issue 31: the old `indi_died_unknown` set (written, never read) looked
    // like it was meant to exclude every DEAT Y from W301. It never did, and
    // must not: a DEAT Y carrying a subordinate DATE is a real death year,
    // so an impossible one still warns.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1902\n1 DEAT Y\n2 DATE 1850\n");
    assert!(has(&g, "W301"), "{:?}", codes(&g));
}

#[test]
fn w302_no_false_positive_distant_births() {
    // Same name but 10 years apart: must NOT be a duplicate (kills ±200 mutant).
    let g = wrap551(
        "0 @I1@ INDI\n1 NAME Joan /Oso/\n1 BIRT\n2 DATE 1861\n0 @I2@ INDI\n1 NAME Joan /Oso/\n1 BIRT\n2 DATE 1871\n",
    );
    assert!(!has(&g, "W302"));
}

#[test]
fn w302_case_insensitive_duplicates() {
    // Same name in different case: still a duplicate (kills no-lowercase mutant).
    let g = wrap551(
        "0 @I1@ INDI\n1 NAME JOAN /OSO/\n1 BIRT\n2 DATE 1861\n0 @I2@ INDI\n1 NAME Joan /Oso/\n1 BIRT\n2 DATE 1862\n",
    );
    assert!(has(&g, "W302"));
}

#[test]
fn w303_old_mother_60() {
    // Mother aged 60 (>50) fires; father aged 60 (<=70) does not matter here.
    let g = wrap551(
        "0 @I1@ INDI\n1 NAME Pare /X/\n1 BIRT\n2 DATE 1870\n\
         0 @I2@ INDI\n1 NAME Mare /Y/\n1 BIRT\n2 DATE 1870\n\
         0 @I3@ INDI\n1 NAME Fill /Z/\n1 BIRT\n2 DATE 1930\n1 FAMC @F1@\n\
         0 @F1@ FAM\n1 HUSB @I1@\n1 WIFE @I2@\n1 CHIL @I3@\n",
    );
    assert!(has(&g, "W303"));
}

#[test]
fn date_year_3000_ignored() {
    // Year 3000 is outside 100..=2100: no birth year, no W303 (kills 9999 mutant).
    let g = wrap551(
        "0 @I1@ INDI\n1 NAME Pare /X/\n1 BIRT\n2 DATE 1900\n\
         0 @I2@ INDI\n1 NAME Mare /Y/\n1 BIRT\n2 DATE 1900\n\
         0 @I3@ INDI\n1 NAME Fill /Z/\n1 BIRT\n2 DATE 3000\n1 FAMC @F1@\n\
         0 @F1@ FAM\n1 HUSB @I1@\n1 WIFE @I2@\n1 CHIL @I3@\n",
    );
    assert!(!has(&g, "W303"));
}

#[test]
fn e004_malformed_xref() {
    // Unclosed xref is malformed.
    let g = wrap551("0 @I1 INDI\n1 NAME A /B/\n");
    assert!(has(&g, "E004"));
}

#[test]
fn e005_orphan_cont() {
    // CONT as the first line has no parent.
    assert!(has("2 CONT orphan\n", "E005"));
}

#[test]
fn e005_level_zero_cont_mid_file() {
    // Issue 8: a level-0 CONT mid-file has no parent (nothing is at
    // level -1), regardless of how many non-blank lines came before it.
    let g = wrap551("0 CONT orfe\n");
    assert!(has(&g, "E005"));
}

#[test]
fn e005_no_fire_on_empty_value_parent() {
    // A parent with an empty value is still a structural parent: E005 is
    // not value-based. `2 TEXT` carries no value but is a valid parent
    // for the `3 CONT` that continues it.
    let g = wrap551("0 @S1@ SOUR\n1 DATA\n2 TEXT\n3 CONT foo\n");
    assert!(!has(&g, "E005"));
}

#[test]
fn e005_cont_under_conc() {
    // CONT/CONC are pseudo-substructures of the value-bearing line and
    // never nest: a CONT hanging off a CONC is malformed.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n2 CONC foo\n3 CONT bar\n");
    assert!(has(&g, "E005"));
}

#[test]
fn e005_cont_under_cont() {
    // Same rule, the other CONT/CONC branch: a CONT hanging off a CONT
    // (not just a CONC) is also malformed. Kills a mutant that drops the
    // `parent_tag == "CONT"` half of the E005 OR (the CONC-only test
    // above still passes under that mutation).
    let g = wrap551("0 @I1@ INDI\n1 NOTE A\n2 CONT B\n3 CONT C\n");
    assert!(has(&g, "E005"));
}

#[test]
fn e005_no_fire_on_sibling_conts() {
    // Consecutive CONT siblings under the same parent are legal.
    let g = wrap551("0 @I1@ INDI\n1 NAME A\n2 CONT B\n2 CONT C\n");
    assert!(!has(&g, "E005"));
}

#[test]
fn e005_no_fire_across_blank_line() {
    // A blank line does not touch the parent stack, so a CONT after one
    // still sees the last real line as its parent.
    let g = wrap551("0 @I1@ INDI\n1 NAME A\n\n2 CONT B\n");
    assert!(!has(&g, "E005"));
}

#[test]
fn u502_vendor_tag() {
    // MyHeritage _UPD is a vendor tag: upgrade info, never an error.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 _UPD 20240101\n");
    let r = lint_str(&g);
    assert!(r
        .diags
        .iter()
        .any(|d| d.code == "U502" && d.severity == Severity::Info));
}

#[test]
fn u501_lowercase_pedi() {
    // 5.5.1 lowercase PEDI values must be uppercase in 7.0.
    let g =
        wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 FAMC @F1@\n2 PEDI birth\n0 @F1@ FAM\n1 CHIL @I1@\n");
    assert!(has(&g, "U501"));
}

#[test]
fn u501_rela_reads_identically_at_both_levels() {
    // Issue 29: the sublevel variant of U501 used to emit its message in
    // Catalan while the level-1 variant was English. Both must read alike.
    let l1 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 RELA ret\n");
    let sub =
        wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 ASSO @I2@\n2 RELA ret\n0 @I2@ INDI\n1 NAME C /D/\n");
    let d1 = only(&l1, "U501");
    let dsub = only(&sub, "U501");
    assert!(d1.msg.contains("RELA removed in 7.0"), "{}", d1.msg);
    assert_eq!(
        d1.msg, dsub.msg,
        "both U501 RELA variants must emit the same wording"
    );
}

#[test]
fn e201_non_pointer_value_message() {
    // Issue 29: the E201 branch for a non-pointer FAMS/FAMC value used to
    // emit its message in Catalan.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 FAMS not-a-pointer\n");
    let d = only(&g, "E201");
    assert!(d.msg.contains("with a non-pointer value"), "{}", d.msg);
}

#[test]
fn w402_date_approximations_and_months() {
    // Lowercase "about" and non-English months violate DATE style.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE about 1900\n");
    assert!(has(&g, "W402"));
    let g2 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 18 gener 1861\n");
    assert!(has(&g2, "W402"));
    let ok = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE ABT 1900\n");
    assert!(!has(&ok, "W402"));
}

#[test]
fn w102_control_char_and_mixed_endings() {
    let g = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME A\x01 /B/\n0 TRLR\n";
    assert!(has(g, "W102"));
    let g2 = "0 HEAD\r\n1 GEDC\n2 VERS 5.5.1\n0 TRLR\n";
    assert!(has(g2, "W102"));
}

#[test]
fn e201_level2_pointer() {
    // Broken pointer below level 1 (e.g. event SOUR) is still E201.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 SOUR @S9@\n");
    assert!(has(&g, "E201"));
}

#[test]
fn e008_duplicate_singletons() {
    // INDI.SEX, FAM.HUSB, HEAD.GEDC, GEDC.VERS are all {0:1}/{1:1}.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 SEX M\n1 SEX F\n");
    assert!(has(&g, "E008"));
    let g2 = wrap551("0 @F1@ FAM\n1 HUSB @I1@\n1 HUSB @I2@\n0 @I1@ INDI\n1 NAME A /B/\n0 @I2@ INDI\n1 NAME C /D/\n");
    assert!(has(&g2, "E008"));
    let g3 = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 GEDC\n2 VERS 5.5.1\n0 TRLR\n";
    assert!(has(g3, "E008"));
    // Singletons present exactly once: clean.
    let ok = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 SEX M\n");
    assert!(!has(&ok, "E008"));
}

#[test]
fn e008_vers_scoped_by_parent() {
    // Issue #5: HEAD.GEDC.VERS (GEDCOM version) and HEAD.SOUR.VERS (product
    // version) are different singletons; every MyHeritage 5.5.1 export has both.
    let g = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n2 FORM LINEAGE-LINKED\n1 SOUR MYHERITAGE\n2 NAME MyHeritage Family Tree Builder\n2 VERS 5.5.1\n0 TRLR\n";
    assert!(!has(g, "E008"), "{:?}", codes(g));
    // HEAD.CHAR.VERS is a third slot (5.5.1 only).
    let chr = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR UTF-8\n2 VERS 1.0\n0 TRLR\n";
    assert!(!has(chr, "E008"), "{:?}", codes(chr));
    // Two VERS inside the SAME block are still duplicates.
    let dup_gedc = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n2 VERS 5.5.1\n0 TRLR\n";
    assert!(has(dup_gedc, "E008"));
    let dup_sour = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 SOUR X\n2 VERS 1\n2 VERS 2\n0 TRLR\n";
    assert!(has(dup_sour, "E008"));
}

#[test]
fn e009_gedc_vers_not_satisfied_by_sour_vers() {
    // A SOUR.VERS must not stand in for the required GEDC.VERS.
    let g = "0 HEAD\n1 GEDC\n1 SOUR X\n2 VERS 9\n0 TRLR\n";
    assert!(has(g, "E009"), "{:?}", codes(g));
}

#[test]
fn e001_blank_lines_are_not_malformed() {
    // Issue #5: trailing blank lines after TRLR are not malformed lines.
    let g = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 TRLR\n\n\n";
    assert!(!has(g, "E001"), "{:?}", codes(g));
    assert!(!has(g, "E002"), "{:?}", codes(g));
    assert_eq!(lint_str(g).exit_code(), 0);
    // A blank line must not mask the level jump that follows it.
    let jump = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n\n3 NAME A /B/\n0 TRLR\n";
    assert!(has(jump, "E001"), "{:?}", codes(jump));
}

#[test]
fn e001_year_leading_orphan_is_repaired() {
    // Issue #5: a biography continuation starting with a year read as level
    // 1936 ("level jump"), so --fix left it behind. Levels stop at 99.
    let data = b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @S1@ SOUR\n1 DATA\n2 TEXT Primera part\n1936 va ser un any dur\n0 TRLR\n".to_vec();
    let r = lint_bytes(&data);
    assert!(
        r.diags.iter().any(|d| d.code == "E001" && d.line == 7),
        "{:?}",
        r.diags
    );
    let (fixed, applied) = fix_bytes(&data);
    assert!(applied.iter().any(|a| a.contains("CONT")));
    let text = String::from_utf8(fixed).unwrap();
    assert!(text.contains("3 CONT 1936 va ser un any dur"), "{}", text);
    assert!(
        lint_str(&text).diags.is_empty(),
        "{:?}",
        lint_str(&text).diags
    );
}

#[test]
fn e001_orphan_run_stays_flat() {
    // A run of orphan lines is a run of siblings under the anchor, not a
    // staircase: nested CONTs would hang the 2nd/3rd paragraph off the 1st
    // CONT instead of off TEXT, silently losing them for a strict reader.
    let data = b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @S1@ SOUR\n1 DATA\n2 TEXT Primera part\n1936 un any dur\n1937 un altre\n1938 un altre mes\n0 TRLR\n".to_vec();
    let (fixed, applied) = fix_bytes(&data);
    assert!(
        applied.iter().any(|a| a.contains("3 orphan lines")),
        "{:?}",
        applied
    );
    let text = String::from_utf8(fixed).unwrap();
    assert!(text.contains("3 CONT 1936 un any dur"), "{}", text);
    assert!(text.contains("3 CONT 1937 un altre"), "{}", text);
    assert!(text.contains("3 CONT 1938 un altre mes"), "{}", text);
    assert!(
        lint_str(&text).diags.is_empty(),
        "{:?}",
        lint_str(&text).diags
    );
    // The anchor resets on the next line that really has a level.
    let mixed = b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @S1@ SOUR\n1 DATA\n2 TEXT a\norfe u\norfe dos\n1 NOTE x\ndespres\n0 TRLR\n".to_vec();
    let t2 = String::from_utf8(fix_bytes(&mixed).0).unwrap();
    assert!(
        t2.contains("3 CONT orfe u") && t2.contains("3 CONT orfe dos"),
        "{}",
        t2
    );
    assert!(t2.contains("2 CONT despres"), "{}", t2);
}

#[test]
fn fix_leaves_whitespace_only_lines_alone() {
    // lint ignores a whitespace-only line, so --fix must not turn it into an
    // empty CONT; the trailing-whitespace pass trims it instead.
    let data =
        b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @S1@ SOUR\n1 DATA\n2 TEXT part\n   \n0 TRLR\n".to_vec();
    let (fixed, applied) = fix_bytes(&data);
    let text = String::from_utf8(fixed).unwrap();
    assert!(
        !applied.iter().any(|a| a.contains("orphan")),
        "{:?}",
        applied
    );
    assert!(!text.contains("CONT"), "{}", text);
    assert!(text.contains("2 TEXT part\n\n0 TRLR"), "{}", text);
}

#[test]
fn e009_missing_required() {
    // HEAD.GEDC and GEDC.VERS are {1:1}.
    assert!(has("0 HEAD\n1 CHAR UTF-8\n0 TRLR\n", "E009"));
    assert!(has("0 HEAD\n1 GEDC\n1 CHAR UTF-8\n0 TRLR\n", "E009"));
    assert!(!has("0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 TRLR\n", "E009"));
}

#[test]
fn w306_enum_values() {
    // Invalid ROLE / QUAY / RESN values fire W306.
    let g = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 ASSO @I2@\n2 ROLE ALIEN\n0 @I2@ INDI\n1 NAME C /D/\n0 TRLR\n", HEAD70);
    assert!(has(&g, "W306"));
    let ok = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 ASSO @I2@\n2 ROLE FRIEND\n0 @I2@ INDI\n1 NAME C /D/\n0 TRLR\n", HEAD70);
    assert!(!has(&ok, "W306"));
    let q = wrap551(
        "0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 SOUR @S1@\n3 QUAY 9\n0 @S1@ SOUR\n1 TITL T\n",
    );
    assert!(has(&q, "W306"));
    let r = format!(
        "{}0 @I1@ INDI\n1 NAME A /B/\n1 RESN locked\n0 TRLR\n",
        HEAD70
    );
    assert!(has(&r, "W306"));
    let r2 = format!(
        "{}0 @I1@ INDI\n1 NAME A /B/\n1 RESN LOCKED\n0 TRLR\n",
        HEAD70
    );
    assert!(!has(&r2, "W306"));
}

#[test]
fn w306_other_wants_phrase() {
    // ROLE OTHER without a sibling PHRASE: info; with PHRASE: silent.
    let g = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 ASSO @I2@\n2 ROLE OTHER\n0 @I2@ INDI\n1 NAME C /D/\n0 TRLR\n", HEAD70);
    let r = lint_str(&g);
    assert!(r
        .diags
        .iter()
        .any(|d| d.code == "W306" && d.severity == Severity::Info));
    let ok = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 ASSO @I2@\n2 ROLE OTHER\n2 PHRASE Teacher\n0 @I2@ INDI\n1 NAME C /D/\n0 TRLR\n", HEAD70);
    assert!(!lint_str(&ok).diags.iter().any(|d| d.code == "W306"));
}

#[test]
fn w302_3digit_years() {
    // 3-digit years are valid: same name + 1 year apart still duplicates.
    let g = wrap551(
        "0 @I1@ INDI\n1 NAME Joan /Puig/\n1 BIRT\n2 DATE 1 JAN 950\n0 @I2@ INDI\n1 NAME Joan /Puig/\n1 BIRT\n2 DATE 1 JAN 951\n",
    );
    assert!(has(&g, "W302"));
}

#[test]
fn e101_ansel_exempt_conc_split() {
    // Declared ANSEL: a high byte starting a CONC payload is legal, no E101.
    let mut data =
        b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR ANSEL\n0 @I1@ INDI\n1 NOTE ab\n2 CONC ".to_vec();
    data.push(0x83);
    data.extend_from_slice(b"\n0 TRLR\n");
    let r = lint_bytes(&data);
    assert!(
        !r.diags.iter().any(|d| d.code == "E101"),
        "ANSEL: {:?}",
        r.diags
    );
}

#[test]
fn w306_pedi_per_version() {
    // PEDI case follows the version: uppercase in 7.0, lowercase in 5.5.1.
    let fam70 = "0 @F1@ FAM\n1 CHIL @I1@\n";
    let g = format!(
        "{}0 @I1@ INDI\n1 NAME A /B/\n1 FAMC @F1@\n2 PEDI ADOPTED\n{}0 TRLR\n",
        HEAD70, fam70
    );
    assert!(!has(&g, "W306"));
    let g2 = format!(
        "{}0 @I1@ INDI\n1 NAME A /B/\n1 FAMC @F1@\n2 PEDI adopted\n{}0 TRLR\n",
        HEAD70, fam70
    );
    assert!(has(&g2, "W306"));
    let g3 = wrap551(
        "0 @I1@ INDI\n1 NAME A /B/\n1 FAMC @F1@\n2 PEDI adopted\n0 @F1@ FAM\n1 CHIL @I1@\n",
    );
    assert!(!has(&g3, "W306"));
    // Issue 9 (finding 4): 5.5.1 enum checks are case-insensitive; commercial
    // exporters capitalize ("ADOPTED"). 7.0 stays strict (registry case).
    let g4 = wrap551(
        "0 @I1@ INDI\n1 NAME A /B/\n1 FAMC @F1@\n2 PEDI ADOPTED\n0 @F1@ FAM\n1 CHIL @I1@\n",
    );
    assert!(!has(&g4, "W306"), "{:?}", codes(&g4));
}

#[test]
fn w306_551_enum_case_insensitive() {
    // Issue 9 (The Kennedy Family.ged): `2 TYPE Birth` must not warn in 5.5.1.
    let t = wrap551("0 @I1@ INDI\n1 NAME A /B/\n2 TYPE Birth\n");
    assert!(!has(&t, "W306"), "{:?}", codes(&t));
    let t2 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n2 TYPE MARRIED\n");
    assert!(!has(&t2, "W306"));
    // 7.0 registry spellings are uppercase: "Birth" is still flagged there.
    let t3 = format!(
        "{}0 @I1@ INDI\n1 NAME A /B/\n2 TYPE Birth\n0 TRLR\n",
        HEAD70
    );
    assert!(has(&t3, "W306"));
    // MEDI under 5.5.1 accepts capitalized spellings.
    let m = wrap551("0 @O1@ OBJE\n1 FILE\n2 FORM jpeg\n3 MEDI Photo\n");
    assert!(!has(&m, "W306"));
    // 5.5.1 has no ASSO.ROLE; source-citation ROLE has its own small set.
    let r5 =
        wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 ASSO @I2@\n2 ROLE HUSB\n0 @I2@ INDI\n1 NAME C /D/\n");
    assert!(!has(&r5, "W306"));
    let r5b = wrap551(
        "0 @I1@ INDI\n1 NAME A /B/\n1 ASSO @I2@\n2 ROLE CLERGY\n0 @I2@ INDI\n1 NAME C /D/\n",
    );
    assert!(has(&r5b, "W306"));
}

#[test]
fn w306_resn_list_70() {
    // Issue 9 (maximal70.ged): 7.0 RESN is type-List#Enum (comma-separated).
    let ok = format!(
        "{}0 @I1@ INDI\n1 NAME A /B/\n1 RESN CONFIDENTIAL, LOCKED\n0 TRLR\n",
        HEAD70
    );
    assert!(!has(&ok, "W306"), "{:?}", codes(&ok));
    let ok2 = format!(
        "{}0 @I1@ INDI\n1 NAME A /B/\n1 RESN CONFIDENTIAL, LOCKED, PRIVACY\n0 TRLR\n",
        HEAD70
    );
    assert!(!has(&ok2, "W306"));
    let bad = format!(
        "{}0 @I1@ INDI\n1 NAME A /B/\n1 RESN CONFIDENTIAL, BOGUS\n0 TRLR\n",
        HEAD70
    );
    assert!(has(&bad, "W306"));
    // Trailing/empty tokens are tolerated (exporter quirk).
    let tail = format!(
        "{}0 @I1@ INDI\n1 NAME A /B/\n1 RESN CONFIDENTIAL,\n0 TRLR\n",
        HEAD70
    );
    assert!(!has(&tail, "W306"));
    // 5.5.1 RESN is a single enum (no lists); case-insensitive.
    let ok3 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 RESN Locked\n");
    assert!(!has(&ok3, "W306"));
    let bad3 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 RESN confidential, locked\n");
    assert!(has(&bad3, "W306"));
}

#[test]
fn w306_data_even_list_70() {
    // Issue 9 (maximal70.ged line 676): DATA.EVEN is a List#Enum of
    // event/attribute tags: "BIRT, DEAT" and "MARR" are valid.
    let ok = format!(
        "{}0 @S1@ SOUR\n1 TITL T\n1 DATA\n2 EVEN BIRT, DEAT\n0 TRLR\n",
        HEAD70
    );
    assert!(!has(&ok, "W306"), "{:?}", codes(&ok));
    let ok2 = format!(
        "{}0 @S1@ SOUR\n1 TITL T\n1 DATA\n2 EVEN MARR\n0 TRLR\n",
        HEAD70
    );
    assert!(!has(&ok2, "W306"));
    let bad = format!(
        "{}0 @S1@ SOUR\n1 TITL T\n1 DATA\n2 EVEN BIRT, BOGUS\n0 TRLR\n",
        HEAD70
    );
    assert!(has(&bad, "W306"));
}

#[test]
fn e201_message_grammar() {
    // Issue 9 (Queen.ged): "points to nonexistent a FAM" double article.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 FAMC @F9@\n");
    let r = lint_str(&g);
    let d = r.diags.iter().find(|d| d.code == "E201").unwrap();
    assert!(d.msg.contains("points to a nonexistent FAM"), "{}", d.msg);
    assert!(!d.msg.contains("nonexistent a"), "{}", d.msg);
}

#[test]
fn w307_remarriage_after_div_is_not_a_conflict() {
    // Issue 9 (remarriage1.ged): MARR 1911, DIV 1912, MARR 1914 in one FAM.
    let g = wrap551(
        "0 @F1@ FAM\n1 HUSB @I1@\n1 MARR\n2 DATE 1911\n1 DIV\n2 DATE 1912\n1 MARR\n2 DATE 1914\n0 @I1@ INDI\n1 NAME A /B/\n",
    );
    assert!(!has(&g, "W307"), "{:?}", codes(&g));
    // The DIV need not carry a date: the event alone separates the marriages.
    let g3 = wrap551("0 @F1@ FAM\n1 MARR\n2 DATE 1911\n1 DIV\n1 MARR\n2 DATE 1914\n");
    assert!(!has(&g3, "W307"));
    // Without an intervening DIV it is still a conflict.
    let g2 = wrap551("0 @F1@ FAM\n1 MARR\n2 DATE 1911\n1 MARR\n2 DATE 1914\n");
    assert!(has(&g2, "W307"));
}

#[test]
fn w307_serial_marriage_divorce_ok() {
    // MARR/DIV/MARR/DIV in one FAM is spec-legal (7.0 allows multiple
    // MARR/DIV): the DIVs must not conflict with each other either.
    let g = wrap551(
        "0 @F1@ FAM\n1 MARR\n2 DATE 1910\n1 DIV\n2 DATE 1912\n1 MARR\n2 DATE 1914\n1 DIV\n2 DATE 1920\n",
    );
    assert!(!has(&g, "W307"), "{:?}", codes(&g));
    // Two DIVs in the same marriage epoch (no MARR between) still conflict.
    let g2 = wrap551("0 @F1@ FAM\n1 MARR\n2 DATE 1910\n1 DIV\n2 DATE 1912\n1 DIV\n2 DATE 1915\n");
    assert!(has(&g2, "W307"));
}

#[test]
fn w307_repeatable_events_exempt() {
    // Issue 9 (TGC551LF): two OCCU/RESI/CENS with different dates is normal.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 OCCU Baker\n2 DATE 31 DEC 1997\n1 OCCU Miller\n2 DATE 31 DEC 1998\n");
    assert!(!has(&g, "W307"), "{:?}", codes(&g));
    let g2 = wrap551(
        "0 @I1@ INDI\n1 NAME A /B/\n1 RESI\n2 DATE 1900\n1 RESI\n2 DATE 1910\n1 CENS\n2 DATE 1901\n1 CENS\n2 DATE 1911\n",
    );
    assert!(!has(&g2, "W307"));
    // Semantically single events remain checked.
    let g3 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1867\n1 BIRT\n2 DATE 1870\n");
    assert!(has(&g3, "W307"));
}

#[test]
fn cr_line_endings_parse() {
    // Issue 9 (TGC551.ged): bare CR is a legal line terminator (5.5.1 s.1).
    let g = "0 HEAD\r1 GEDC\r2 VERS 5.5.1\r1 CHAR ANSEL\r0 @I1@ INDI\r1 NAME A /B/\r0 TRLR\r";
    let r = lint_bytes(g.as_bytes());
    assert_eq!(r.version, Version::V551);
    assert_eq!(r.lines, 7);
    for code in ["E002", "E101", "E009", "W102", "E001"] {
        assert!(
            !r.diags.iter().any(|d| d.code == code),
            "{}: {:?}",
            code,
            r.diags
        );
    }
    // CRLF still counts as one terminator.
    let crlf = "0 HEAD\r\n1 GEDC\r\n2 VERS 7.0\r\n0 TRLR\r\n";
    let r2 = lint_bytes(crlf.as_bytes());
    assert_eq!(r2.lines, 4);
    assert_eq!(r2.version, Version::V70);
}

#[test]
fn w306_other_phrase_child_ok() {
    // Issue 9 (maximal70.ged): PHRASE may hang under the OTHER-valued
    // structure itself (3 ROLE OTHER / 4 PHRASE), not only beside it.
    let g = format!(
        "{}0 @I1@ INDI\n1 NAME A /B/\n1 ASSO @I2@\n2 ROLE OTHER\n3 PHRASE Teacher\n0 @I2@ INDI\n1 NAME C /D/\n0 TRLR\n",
        HEAD70
    );
    assert!(
        !lint_str(&g).diags.iter().any(|d| d.code == "W306"),
        "{:?}",
        codes(&g)
    );
    // Sibling PHRASE keeps working.
    let ok = format!(
        "{}0 @I1@ INDI\n1 NAME A /B/\n1 ASSO @I2@\n2 ROLE OTHER\n2 PHRASE Teacher\n0 @I2@ INDI\n1 NAME C /D/\n0 TRLR\n",
        HEAD70
    );
    assert!(!lint_str(&ok).diags.iter().any(|d| d.code == "W306"));
    // And OTHER without any PHRASE still reports.
    let g2 = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 ASSO @I2@\n2 ROLE OTHER\n0 @I2@ INDI\n1 NAME C /D/\n0 TRLR\n", HEAD70);
    assert!(lint_str(&g2).diags.iter().any(|d| d.code == "W306"));
}

#[test]
fn w306_medi_ignored_in_70() {
    // MEDI is a 5.5.1 tag: never validated under 7.0.
    let g = format!(
        "{}0 @O1@ OBJE\n1 FILE\n2 FORM image/jpeg\n3 MEDI PHOTO\n0 TRLR\n",
        HEAD70
    );
    assert!(!has(&g, "W306"));
}

#[test]
fn w402_date_period_phrase_calendar() {
    // Unbalanced parens, bad calendar escape: all W402.
    let g2 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE (seen on stone\n");
    assert!(has(&g2, "W402"));
    let g3 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE @#MARS@ 1900\n");
    assert!(has(&g3, "W402"));
    let ok3 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE @#DJULIAN@ 1900\n");
    // DJULIAN is not a registry calendar: flagged. GREGORIAN passes.
    assert!(has(&ok3, "W402"));
    let ok4 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE @#GREGORIAN@ 1900\n");
    assert!(!has(&ok4, "W402"));
}

#[test]
fn w402_one_sided_date_period_is_valid() {
    // Issue 9 (TGC551LF BASM/ADOP): DATE_PERIOD allows each half alone
    // (5.5.1 p.43 and 7.0 DATE_PERIOD both accept one-sided FROM/TO).
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BASM\n2 DATE FROM 31 DEC 1997\n");
    assert!(!has(&g, "W402"), "{:?}", codes(&g));
    let g2 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 ADOP Y\n2 DATE TO 31 DEC 1997\n");
    assert!(!has(&g2, "W402"), "{:?}", codes(&g2));
    let ok = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE FROM 1900 TO 1910\n");
    assert!(!has(&ok, "W402"));
}

#[test]
fn w402_name_slashes_across_conc() {
    // Issue 9 (Long26CC.ged): a surname split across CONC makes each line
    // unbalanced but the whole NAME balanced. Only the whole value counts.
    let g = wrap551("0 @I1@ INDI\n1 NAME /VeryLongSurnameThatKeeps\n2 CONC Going/\n");
    assert!(!has(&g, "W402"), "{:?}", codes(&g));
    let g2 = wrap551("0 @I1@ INDI\n1 NAME Very /Long/\n2 CONC Name\n");
    assert!(!has(&g2, "W402"));
    // CONT also continues the value.
    let g3 = wrap551("0 @I1@ INDI\n1 NAME /Surname\n2 CONT more/\n");
    assert!(!has(&g3, "W402"));
    // Genuinely odd totals across CONC still fire.
    let g4 = wrap551("0 @I1@ INDI\n1 NAME Joan /Oso\n2 CONC broken\n");
    assert!(has(&g4, "W402"));
    // Without continuation, unbalanced still fires (existing behavior).
    assert!(has(&wrap551("0 @I1@ INDI\n1 NAME Joan /Oso\n"), "W402"));
}

#[test]
fn w402_conc_under_substructure_not_absorbed() {
    // A CONC under NAME's SOUR citation continues the source line, not the
    // NAME: the run must close at the intervening substructure.
    let g = wrap551(
        "0 @I1@ INDI\n1 NAME Joan /Garcia/\n2 SOUR @S1@\n3 PAGE married /the/ year\n4 CONC 1850 /with/ notes\n0 @S1@ SOUR\n1 TITL T\n",
    );
    assert!(!has(&g, "W402"), "{:?}", codes(&g));
    // A non-continuation line at level 2 also closes the run.
    let g2 = wrap551("0 @I1@ INDI\n1 NAME Joan /Oso\n2 SEX M\n2 CONC broken\n");
    assert!(has(&g2, "W402"));
}

#[test]
fn e002_head_first_trlr_last() {
    // TRLR must end the file; HEAD must open it.
    assert!(has(
        "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 TRLR\n0 @I1@ INDI\n1 NAME A /B/\n",
        "E002"
    ));
    assert!(has(
        "0 @I1@ INDI\n1 NAME A /B/\n0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 TRLR\n",
        "E002"
    ));
    assert!(!has("0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 TRLR\n", "E002"));
}

#[test]
fn char_rules() {
    // 5.5.1 CHAR has 4 legal values; 7.0 drops CHAR entirely.
    let g = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR ANSI\n0 TRLR\n";
    assert!(has(g, "W306"));
    let ok = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR UTF-8\n0 TRLR\n";
    assert!(!has(ok, "W306"));
    let g70 = format!("{}1 CHAR UTF-8\n0 TRLR\n", HEAD70);
    assert!(has(&g70, "U501"));
}

#[test]
fn e008_two_birt_blocks_ok() {
    // BIRT is {0:M} (real @I131@ case): one DATE per block is legal.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1867\n1 BIRT\n2 DATE ABT 1870\n");
    assert!(!has(&g, "E008"));
}

#[test]
fn e008_event_detail_singleton() {
    // Two DATEs under one BIRT: E008.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1900\n2 DATE 1901\n");
    assert!(has(&g, "E008"));
    let ok = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1900\n2 PLAC Reus\n");
    assert!(!has(&ok, "E008"));
}

#[test]
fn w306_lds_stat() {
    // LDS ordinance STAT is enumerated (accepts PRE and PRE_1970).
    let g = format!(
        "{}0 @I1@ INDI\n1 NAME A /B/\n1 BAPL\n2 STAT BOGUS\n0 TRLR\n",
        HEAD70
    );
    assert!(has(&g, "W306"));
    let ok = format!(
        "{}0 @I1@ INDI\n1 NAME A /B/\n1 BAPL\n2 STAT COMPLETED\n0 TRLR\n",
        HEAD70
    );
    assert!(!has(&ok, "W306"));
    let ok2 = format!(
        "{}0 @I1@ INDI\n1 NAME A /B/\n1 BAPL\n2 STAT PRE_1970\n0 TRLR\n",
        HEAD70
    );
    assert!(!has(&ok2, "W306"));
}

#[test]
fn w307_conflicting_duplicate_events() {
    // Real @I131@ pattern: two BIRT blocks, different DATEs.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1867\n1 BIRT\n2 DATE ABT 1870\n");
    assert!(has(&g, "W307"));
    let same = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1867\n1 BIRT\n2 DATE 1867\n");
    assert!(!has(&same, "W307"));
    let single = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1867\n");
    assert!(!has(&single, "W307"));
}

#[test]
fn e009_even_fact_type_70_only() {
    // EVEN/FACT need TYPE in 7.0; 5.5.1 leaves it optional.
    let g = format!(
        "{}0 @I1@ INDI\n1 NAME A /B/\n1 EVEN\n2 DATE 1900\n0 TRLR\n",
        HEAD70
    );
    assert!(has(&g, "E009"));
    let ok = format!(
        "{}0 @I1@ INDI\n1 NAME A /B/\n1 EVEN\n2 TYPE Military service\n2 DATE 1900\n0 TRLR\n",
        HEAD70
    );
    assert!(!has(&ok, "E009"));
    let g551 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 EVEN\n2 DATE 1900\n");
    assert!(!has(&g551, "E009"));
}

#[test]
fn e009_lds_stat_date_70_only() {
    // LDS STAT needs a DATE under 7.0.
    let g = format!(
        "{}0 @I1@ INDI\n1 NAME A /B/\n1 BAPL\n2 STAT COMPLETED\n0 TRLR\n",
        HEAD70
    );
    assert!(has(&g, "E009"));
    let ok = format!(
        "{}0 @I1@ INDI\n1 NAME A /B/\n1 BAPL\n2 STAT COMPLETED\n3 DATE 1900\n0 TRLR\n",
        HEAD70
    );
    assert!(!has(&ok, "E009"));
}

#[test]
fn w306_data_even_and_form() {
    // DATA.EVEN payload and FILE.FORM media type under 7.0.
    let g = format!(
        "{}0 @S1@ SOUR\n1 TITL T\n1 DATA\n2 EVEN BIRTHS\n0 TRLR\n",
        HEAD70
    );
    assert!(has(&g, "W306"));
    let f = format!("{}0 @O1@ OBJE\n1 FILE\n2 FORM textplain\n0 TRLR\n", HEAD70);
    assert!(has(&f, "W306"));
    let ok = format!("{}0 @O1@ OBJE\n1 FILE\n2 FORM image/jpeg\n0 TRLR\n", HEAD70);
    assert!(!has(&ok, "W306"));
}

#[test]
fn void_pointer_is_valid() {
    // @VOID@ is the 7.0 null pointer (voidptr.ged): never E201.
    let g = format!(
        "{}0 @I1@ INDI\n1 NAME A /B/\n1 SOUR @VOID@\n0 TRLR\n",
        HEAD70
    );
    assert!(!has(&g, "E201"));
}

#[test]
fn conc_illegal_in_70() {
    // CONC is a reserved tag in 7.0 (spec 1.3): E007.
    let g = format!("{}0 @I1@ INDI\n1 NAME A /B/\n2 CONC more\n0 TRLR\n", HEAD70);
    assert!(has(&g, "E007"));
    let g551 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n2 CONC more\n");
    assert!(!has(&g551, "E007"));
}

#[test]
fn bet_without_and_flagged_in_551() {
    // 5.5.1 DATE_RANGE mandates BET x AND y: W402.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE BET 1900\n");
    assert!(has(&g, "W402"));
}

#[test]
fn bet_range_order() {
    // Out-of-order range: U501 info suggesting a swap.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE BET 1920 AND 1900\n");
    assert!(has(&g, "U501"));
    let ok = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE BET 1900 AND 1920\n");
    assert!(!has(&ok, "U501"));
}

#[test]
fn ansel_skips_utf8_checks() {
    // Declared ANSEL: high bytes are legal, E101 must stay silent.
    let mut data = b"0 HEAD\n1 CHAR ANSEL\n0 @I1@ INDI\n1 NAME Jos\xe9 /Oso/\n0 TRLR\n".to_vec();
    let r = lint_bytes(&data);
    assert!(
        !r.diags.iter().any(|d| d.code == "E101"),
        "ANSEL: {:?}",
        r.diags
    );
    let _ = &mut data;
}

#[test]
fn bom_silent_in_70() {
    // GEDCOM 7 recommends the BOM (spec 1.1): no W102 for it.
    let g = "\u{FEFF}0 HEAD\n1 GEDC\n2 VERS 7.0\n0 TRLR\n";
    let r = lint_bytes(g.as_bytes());
    assert!(!r.diags.iter().any(|d| d.code == "W102" && d.line == 1));
}

#[test]
fn version_detect_70_and_upgrade_rules() {
    // RELA under 5.5.1 triggers U501; SEX X is valid in 7.0 but not in 5.5.1.
    let g551 = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 ASSO @I2@\n2 RELA germà\n0 @I2@ INDI\n1 NAME C /D/\n0 TRLR\n", HEAD551);
    assert!(has(&g551, "U501"));
    let g70 = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 SEX X\n0 TRLR\n", HEAD70);
    let r70 = lint_str(&g70);
    assert_eq!(r70.version, Version::V70);
    assert!(!r70.diags.iter().any(|d| d.code == "W305"));
    let g551x = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 SEX X\n");
    assert!(has(&g551x, "W305"));
}

#[test]
fn gedcom70_minimal_clean() {
    // Own 7.0 fixture (criterion 5): gedcom.io minimal70 reduced to HEAD+SOUR+TRLR.
    let g = "0 HEAD\n1 GEDC\n2 VERS 7.0\n1 SOUR gedlint\n0 TRLR\n";
    let r = lint_str(g);
    assert_eq!(r.version, Version::V70);
    assert!(
        !r.diags.iter().any(|d| d.severity == Severity::Error),
        "errors: {:?}",
        r.diags
    );
}

#[test]
fn exit_codes_and_json() {
    let clean = wrap551("0 @I1@ INDI\n1 NAME A /B/\n");
    let r = lint_str(&clean);
    assert_eq!(r.exit_code(), 0);
    let j = r.to_json();
    assert!(j.contains("\"version\":\"5.5.1\""));
    let warn = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 SEX Q\n");
    assert_eq!(lint_str(&warn).exit_code(), 1);
    let err = "0 @I1@ INDI\n".to_string();
    assert_eq!(lint_str(&err).exit_code(), 2);
}

#[test]
fn severity_filter() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 SEX Q\n");
    let r = lint_str(&g);
    assert!(r.filtered(Severity::Error).is_empty());
    assert!(!r.filtered(Severity::Warning).is_empty());
}

// ---------------------------------------------------------------------------
// Byte spans and the ruleset axis (issue 18, RFC 014 section 1).
// ---------------------------------------------------------------------------

/// Exactly what a viewer highlights: the raw line's bytes sliced at
/// `col..col + len`, then decoded. Panics if the span is not on char
/// boundaries, which is the point of the assertion.
fn span_of<'a>(src: &'a str, d: &Diag) -> &'a str {
    let raw = src.lines().nth(d.line - 1).expect("diagnostic line exists");
    let (col, len) = (d.col as usize, d.len as usize);
    // Byte slice then decode, exactly what the JS side does with TextDecoder.
    let bytes: &[u8] = raw.as_bytes();
    std::str::from_utf8(&bytes[col..col + len]).expect("span is valid UTF-8")
}

fn only(src: &str, code: &str) -> Diag {
    let r = lint_str(src);
    let mut hits = r.diags.into_iter().filter(|d| d.code == code);
    let d = hits
        .next()
        .unwrap_or_else(|| panic!("no {} in {:?}", code, lint_str(src).diags));
    assert!(hits.next().is_none(), "expected a single {}", code);
    d
}

#[test]
fn spanless_diags_default_to_core_and_no_span() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 FAMC @F9@\n");
    let d = only(&g, "E201");
    assert_eq!(d.ruleset, "core");
    assert_eq!(
        (d.col, d.len),
        (0, 0),
        "len 0 means: highlight the whole line"
    );
}

#[test]
fn e004_span_covers_the_xref_token() {
    let g = wrap551("0 @I1 INDI\n1 NAME A /B/\n");
    let d = only(&g, "E004");
    // "0 @I1 INDI": the token starts at byte 2 and is 3 bytes long.
    assert_eq!((d.col, d.len), (2, 3));
    assert_eq!(span_of(&g, &d), "@I1");
}

#[test]
fn w401_span_is_byte_based_not_char_based() {
    // Multi-byte UTF-8 before the span: "Lòria" is 6 bytes but 5 chars, so a
    // char offset would point one byte short of the URL.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 PLAC Lòria https://example.com/x\n");
    let d = only(&g, "W401");
    let raw = g.lines().nth(d.line - 1).unwrap();
    assert_eq!((d.col, d.len), (14, 21));
    assert_eq!(
        raw.chars().take_while(|c| *c != 'h').count(),
        13,
        "the char offset differs"
    );
    assert_eq!(span_of(&g, &d), "https://example.com/x");
}

#[test]
fn w401_record_level_span_covers_the_url() {
    // The level-1 entry point (PLAC directly under the record), whose message
    // differs from the nested one.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 PLAC Lòria http://example.com\n");
    let d = only(&g, "W401");
    assert!(d.msg.contains("move it to NOTE"), "{}", d.msg);
    assert_eq!(span_of(&g, &d), "http://example.com");
}

#[test]
fn w401_span_runs_to_the_end_of_the_line() {
    // No whitespace after the URL: the span ends at the end of the line.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 PLAC https://example.com/x\n");
    let d = only(&g, "W401");
    assert_eq!((d.col, d.len), (7, 21));
    assert_eq!(span_of(&g, &d), "https://example.com/x");
}

#[test]
fn e101_span_covers_the_orphan_continuation_bytes() {
    // Same MyHeritage bug as e101_conc_split_fix: "é" (C3 A9) cut in half, so
    // the A9 tail opens the CONC line. "2 CONC " is 7 bytes.
    let mut data = Vec::new();
    data.extend_from_slice("0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME Jos".as_bytes());
    data.push(0xC3);
    data.extend_from_slice("\n2 CONC ".as_bytes());
    data.push(0xA9);
    data.extend_from_slice(" /Oso/\n0 TRLR\n".as_bytes());
    let r = lint_bytes(&data);
    let d = r.diags.iter().find(|d| d.code == "E101").expect("E101");
    assert_eq!(d.line, 6);
    assert_eq!((d.col, d.len), (7, 1));
    let raw: &[u8] = data.split(|&b| b == b'\n').nth(d.line - 1).unwrap();
    assert_eq!(&raw[d.col as usize..(d.col + d.len) as usize], &[0xA9]);
}

#[test]
fn e101_whole_file_diag_has_no_span() {
    // Invalid UTF-8 that is not a CONC split: reported against the file
    // (line 0), where a per-line span would be meaningless.
    let mut data = Vec::new();
    data.extend_from_slice(
        "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR UTF-8\n0 @I1@ INDI\n1 NAME A".as_bytes(),
    );
    data.push(0xFF);
    data.extend_from_slice("\n0 TRLR\n".as_bytes());
    let r = lint_bytes(&data);
    let d = r.diags.iter().find(|d| d.code == "E101").expect("E101");
    assert_eq!(d.line, 0);
    assert_eq!((d.col, d.len), (0, 0));
}

#[test]
fn json_carries_the_additive_span_keys() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 PLAC Lòria https://example.com/x\n");
    let j = lint_str(&g).to_json();
    assert!(j.contains("\"ruleset\":\"core\""), "{}", j);
    assert!(j.contains("\"col\":14,\"len\":21"), "{}", j);
    // The keys gh-report.js reads keep their name, type and meaning.
    assert!(j.contains("\"code\":\"W401\""), "{}", j);
    assert!(j.contains("\"category\":\"style\""), "{}", j);
    assert!(j.contains("\"severity\":\"WARN\""), "{}", j);
}

/// The bytes a consumer really has: the on-disk line, sliced at the span.
/// Nothing is stripped first, so a BOM counts as 3 bytes of line 1.
fn on_disk_span<'a>(data: &'a [u8], d: &Diag) -> &'a [u8] {
    let line = data
        .split(|&b| b == b'\n')
        .nth(d.line - 1)
        .expect("diagnostic line exists");
    &line[d.col as usize..(d.col + d.len) as usize]
}

#[test]
fn e004_span_is_relative_to_the_on_disk_line_with_a_bom() {
    // GEDCOM 7 recommends a BOM. The parser strips it, the file still has it,
    // and col must address the file: the xref starts at byte 5, not byte 2.
    let mut data = vec![0xEF, 0xBB, 0xBF];
    data.extend_from_slice(b"0 @I1 INDI\n1 NAME A /B/\n0 TRLR\n");
    let r = lint_bytes(&data);
    let d = r.diags.iter().find(|d| d.code == "E004").expect("E004");
    assert_eq!((d.line, d.col, d.len), (1, 5, 3));
    assert_eq!(std::str::from_utf8(on_disk_span(&data, d)).unwrap(), "@I1");
}

#[test]
fn w401_span_is_relative_to_the_on_disk_line_with_a_bom() {
    let mut data = vec![0xEF, 0xBB, 0xBF];
    data.extend_from_slice(b"2 PLAC http://example.com\n");
    let r = lint_bytes(&data);
    let d = r.diags.iter().find(|d| d.code == "W401").expect("W401");
    assert_eq!((d.line, d.col), (1, 10));
    assert_eq!(
        std::str::from_utf8(on_disk_span(&data, d)).unwrap(),
        "http://example.com"
    );
}

#[test]
fn spans_share_one_byte_base_across_rule_families() {
    // E004 comes from the line pass (BOM stripped internally), E101 from the
    // byte pass (BOM never stripped). Both must address the same bytes.
    let mut data = vec![0xEF, 0xBB, 0xBF];
    data.extend_from_slice("0 @I1 INDI\n1 NAME Jos".as_bytes());
    data.push(0xC3);
    data.extend_from_slice("\n2 CONC ".as_bytes());
    data.push(0xA9);
    data.extend_from_slice(" /Oso/\n0 TRLR\n".as_bytes());
    let r = lint_bytes(&data);
    let e004 = r.diags.iter().find(|d| d.code == "E004").expect("E004");
    let e101 = r.diags.iter().find(|d| d.code == "E101").expect("E101");
    assert_eq!(
        std::str::from_utf8(on_disk_span(&data, e004)).unwrap(),
        "@I1"
    );
    assert_eq!(
        (e101.line, e101.col, e101.len),
        (3, 7, 1),
        "line 3 has no BOM, so no base"
    );
    assert_eq!(on_disk_span(&data, e101), &[0xA9]);
}

#[test]
fn w401_span_skips_a_decoy_http_substring() {
    // "chttpx" contains "http" but does not start a token: the span belongs to
    // the real URL 12 bytes further along.
    let g =
        wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 PLAC Lòria chttpx http://example.com/x\n");
    let d = only(&g, "W401");
    assert_eq!(span_of(&g, &d), "http://example.com/x");
}

#[test]
fn w401_without_a_url_token_has_no_span() {
    // The rule fires on a bare "http" substring, so a value can trip it with
    // no URL in it at all. Then there is nothing to point at: len 0.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 PLAC Reus chttpx\n");
    let d = only(&g, "W401");
    assert_eq!((d.col, d.len), (0, 0));
}

#[test]
fn diag_line_and_edit_lines_use_the_same_numbering() {
    // Cross-check with the structured fixes of #19: a span addresses bytes
    // inside a line, an Edit addresses a range of lines, and both count lines
    // the same way (1-based, after line-ending normalization). If these two
    // ever disagreed, a viewer could not put a finding and its repair on the
    // same row. Same E101 CONC split as the span test above.
    let mut data = Vec::new();
    data.extend_from_slice("0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME Jos".as_bytes());
    data.push(0xC3);
    data.extend_from_slice("\n2 CONC ".as_bytes());
    data.push(0xA9);
    data.extend_from_slice(" /Oso/\n0 TRLR\n".as_bytes());
    let d = lint_bytes(&data)
        .diags
        .into_iter()
        .find(|d| d.code == "E101")
        .expect("E101");
    let e = compute_edits(&data)
        .into_iter()
        .find(|e| e.code == "E101")
        .expect("E101 edit");
    // The diagnostic points at the CONC line; the repair replaces the pair.
    assert_eq!(d.line, 6);
    assert_eq!(e.lines, (5, 6));
    assert!(
        e.lines.0 <= d.line && d.line <= e.lines.1,
        "diag line inside the edit range: {:?}",
        e.lines
    );
}

#[test]
fn in_ruleset_moves_a_diag_out_of_core() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 FAMC @F9@\n");
    let d = only(&g, "E201").in_ruleset("hygiene");
    assert_eq!(d.ruleset, "hygiene");
    assert_eq!(d.code, "E201", "only the ruleset changes");
}

// ---------------------------------------------------------------------------
// Determinism and real lines for the graph rules (issue 30).
// ---------------------------------------------------------------------------

/// Fixture that triggers W202 (both directions, twice each), W302 (two
/// pairs), W303 (young father, old mother) and W304. Several occurrences
/// per rule are what makes HashMap iteration order observable.
fn graph_rules_fixture() -> String {
    wrap551(
        // Two W302 pairs.
        "0 @I1@ INDI\n1 NAME Joan /Oso/\n1 BIRT\n2 DATE 1861\n\
         0 @I2@ INDI\n1 NAME Joan /Oso/\n1 BIRT\n2 DATE 1862\n\
         0 @I3@ INDI\n1 NAME Anna /Puig/\n1 BIRT\n2 DATE 1900\n\
         0 @I4@ INDI\n1 NAME Anna /Puig/\n1 BIRT\n2 DATE 1901\n\
         \
         0 @I5@ INDI\n1 NAME Father /Vell/\n1 BIRT\n2 DATE 1990\n\
         0 @I6@ INDI\n1 NAME Mother /Vell/\n1 BIRT\n2 DATE 1930\n\
         0 @I7@ INDI\n1 NAME Child /Vell/\n1 BIRT\n2 DATE 1995\n1 FAMC @F3@\n\
         0 @F3@ FAM\n1 HUSB @I5@\n1 WIFE @I6@\n1 CHIL @I7@\n1 MARR\n2 DATE 1999\n\
         \
         0 @I8@ INDI\n1 NAME One /A/\n1 FAMC @F1@\n\
         0 @I9@ INDI\n1 NAME Two /A/\n1 FAMC @F2@\n\
         0 @F1@ FAM\n1 HUSB @I1@\n\
         0 @F2@ FAM\n1 HUSB @I2@\n\
         \
         0 @F4@ FAM\n1 CHIL @I10@\n\
         0 @F5@ FAM\n1 CHIL @I11@\n\
         0 @I10@ INDI\n1 NAME Ten /B/\n\
         0 @I11@ INDI\n1 NAME Eleven /B/\n",
    )
}

#[test]
fn graph_rules_fire_all_four() {
    let r = lint_str(&graph_rules_fixture());
    for code in ["W202", "W302", "W303", "W304"] {
        assert!(
            r.diags.iter().any(|d| d.code == code),
            "{} missing: {:?}",
            code,
            r.diags
        );
    }
}

#[test]
fn graph_rules_report_real_lines_not_zero() {
    // Issue 30: every graph-rule diagnostic must point at a meaningful line.
    let r = lint_str(&graph_rules_fixture());
    for code in ["W202", "W302", "W303", "W304"] {
        assert!(
            r.diags
                .iter()
                .filter(|d| d.code == code)
                .all(|d| d.line > 0),
            "{} still reports at line 0: {:?}",
            code,
            r.diags
                .iter()
                .filter(|d| d.code == code)
                .collect::<Vec<_>>()
        );
    }
    // Spot checks on the fixture's known lines (HEAD551 is 4 lines):
    // @I2@'s record opens on line 9 (W302 reports the second record)...
    let w302 = only(
        &wrap551("0 @I1@ INDI\n1 NAME Joan /Oso/\n1 BIRT\n2 DATE 1861\n0 @I2@ INDI\n1 NAME Joan /Oso/\n1 BIRT\n2 DATE 1862\n"),
        "W302",
    );
    assert_eq!(
        w302.line, 9,
        "W302 points at the second duplicate's record line"
    );
    assert_eq!(
        w302.msg, "possible duplicate: @I1@ (b. 1861) vs @I2@ (b. 1862)",
        "pair order is deterministic"
    );
}

#[test]
fn w303_w304_point_at_the_child_record() {
    // Same shape as the w303/w304 fixtures above: @I3@ (the child) opens on
    // line 13 after the 4-line header.
    let base = |marr: &str| {
        wrap551(&format!(
            "0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1900\n\
             0 @I2@ INDI\n1 NAME C /D/\n1 BIRT\n2 DATE 1902\n\
             0 @I3@ INDI\n1 NAME E /F/\n1 BIRT\n2 DATE 1910\n1 FAMC @F1@\n\
             0 @F1@ FAM\n1 HUSB @I1@\n1 WIFE @I2@\n1 CHIL @I3@\n{}",
            marr
        ))
    };
    let g303 = base("");
    let r303 = lint_str(&g303);
    for d in r303.diags.iter().filter(|d| d.code == "W303") {
        assert_eq!(
            d.line, 13,
            "W303 points at the child's record line: {:?}",
            d
        );
    }
    let g304 = base("1 MARR\n2 DATE 1920\n");
    let d304 = only(&g304, "W304");
    assert_eq!(
        d304.line, 13,
        "W304 points at the child's record line: {:?}",
        d304
    );
}

#[test]
fn graph_rules_order_is_identical_across_threads() {
    // The real assertion behind "run it twice": RandomState is re-seeded per
    // thread just like per process, so identical output across 8 independent
    // threads means the order no longer depends on HashMap iteration.
    let g = std::sync::Arc::new(graph_rules_fixture());
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let g = g.clone();
            std::thread::spawn(move || {
                let r = lint_str(&g);
                r.diags
                    .iter()
                    .map(|d| {
                        format!(
                            "{} {} {} {}",
                            d.severity.tag().trim(),
                            d.code,
                            d.line,
                            d.msg
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        })
        .collect();
    let outs: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    for o in &outs {
        assert!(*o == outs[0], "graph-rule output differs between threads");
    }
}

// ---------------------------------------------------------------------------
// Grouped output (issue 20 / RFC 014 section 7).
// ---------------------------------------------------------------------------

use gedlint::Report;

#[test]
fn grouped_collapses_by_code_severity_then_count() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 SEX Q\n1 _UPD X\n1 _UPD Y\n");
    let r = lint_str(&g);
    let groups = r.grouped();
    let u502 = groups
        .iter()
        .find(|g| g.code == "U502")
        .expect("U502 group");
    assert_eq!(
        (u502.count, u502.severity, u502.line),
        (2, Severity::Info, 8),
        "first line of the run"
    );
    assert_eq!(
        u502.example,
        "vendor tag _UPD: kept as an undocumented extension in 7.0 (add a SCHMA TAG definition)"
    );
    // Severity descending: the W305 group sorts before the U502 group.
    let w305 = groups
        .iter()
        .find(|g| g.code == "W305")
        .expect("W305 group");
    assert_eq!((w305.count, w305.severity), (1, Severity::Warning));
    assert!(
        groups.iter().position(|g| g.code == "W305").unwrap()
            < groups.iter().position(|g| g.code == "U502").unwrap()
    );
}

#[test]
fn grouped_takes_the_worst_severity_of_a_code() {
    // W306 both warns (bad RESN value) and informs (OTHER without PHRASE).
    let g = format!(
        "{}0 @I1@ INDI\n1 NAME A /B/\n1 RESN BOGUS\n1 ASSO @I2@\n2 ROLE OTHER\n0 @I2@ INDI\n1 NAME C /D/\n0 TRLR\n",
        HEAD70
    );
    let r = lint_str(&g);
    let w306 = r
        .grouped()
        .into_iter()
        .find(|g| g.code == "W306")
        .expect("W306 group");
    assert_eq!(
        (w306.count, w306.severity),
        (2, Severity::Warning),
        "worst severity wins"
    );
}

#[test]
fn group_diags_over_a_filtered_slice() {
    // The CLI groups the severity-filtered subset, not the whole report.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 SEX Q\n1 _UPD X\n");
    let r = lint_str(&g);
    let warnings: Vec<&Diag> = r.filtered(Severity::Warning);
    let groups = Report::group_diags(&warnings);
    assert_eq!(
        groups.len(),
        1,
        "only W305 survives the filter: {:?}",
        groups
    );
    assert_eq!(groups[0].code, "W305");
    assert_eq!(r.grouped().len(), 2, "unfiltered keeps both");
}

#[test]
fn json_carries_the_engine_groups() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 SEX Q\n1 _UPD X\n1 _UPD Y\n");
    let j = lint_str(&g).to_json();
    assert!(
        j.contains("\"groups\":[{\"code\":\"W305\",\"category\":\"suspicious\",\"severity\":\"WARN\",\"count\":1,"),
        "{}",
        j
    );
    assert!(
        j.contains("\"code\":\"U502\",\"category\":\"upgrade\",\"severity\":\"INFO\",\"count\":2,\"line\":8,\"example\":\"vendor tag _UPD"),
        "{}",
        j
    );
}

#[test]
fn w601_no_comma_in_surname() {
    // Assert exactly two tokens inside the surname slashes!
    let g2 = wrap551("0 @I1@ INDI\n1 NAME A /Cognom1, Cognom2/\n");
    assert!(has_with(&g2, "W601", "hispanic-naming"));
    let ok = wrap551("0 @I1@ INDI\n1 NAME A /Cognom1 Cognom2/\n");
    assert!(!has_with(&ok, "W601", "hispanic-naming"));
}

#[test]
fn w601_fires_on_surn_subtag_alone() {
    // The defect lives ONLY in the structured subtag; the NAME value is clean.
    let g = wrap551("0 @I1@ INDI\n1 NAME Maria /Montpeo Osso/\n2 SURN Montpeo, Osso\n");
    assert!(has_with(&g, "W601", "hispanic-naming"));
    let ok = wrap551("0 @I1@ INDI\n1 NAME Maria /Montpeo Osso/\n2 SURN Montpeo Osso\n");
    assert!(!has_with(&ok, "W601", "hispanic-naming"));
}

#[test]
fn w601_name_and_surn_defect_is_one_diagnostic() {
    // Same defect in both places: one defect, one diagnostic (why says so).
    let g = wrap551("0 @I1@ INDI\n1 NAME Maria /Montpeo, Osso/\n2 SURN Montpeo, Osso\n");
    let n = codes_with(&g, "hispanic-naming")
        .iter()
        .filter(|c| c.as_str() == "W601")
        .count();
    assert_eq!(
        n,
        1,
        "exactly one W601 per record: {:?}",
        codes_with(&g, "hispanic-naming")
    );
}

#[test]
fn w601_repair_ignores_three_tokens() {
    let cfg = fix_cfg(&["hispanic-naming"]);
    let data = b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME A /Cognom1, Cognom2, Cognom3/\n0 TRLR\n".to_vec();
    let (_fixed, applied) = fix_bytes_with(&data, &FixSelection::default(), &cfg);
    assert!(!applied.iter().any(|a| a.contains("W601")), "{:?}", applied);

    // Repair works for exactly two tokens
    let data2 = b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME A /Cognom1, Cognom2/\n0 TRLR\n"
        .to_vec();
    let (fixed2, applied2) = fix_bytes_with(&data2, &FixSelection::default(), &cfg);
    assert!(applied2.iter().any(|a| a.contains("W601")));
    assert!(String::from_utf8_lossy(&fixed2).contains("/Cognom1 Cognom2/"));
}

#[test]
fn w601_repair_covers_surn_same_conservative_shape() {
    let cfg = fix_cfg(&["hispanic-naming"]);
    // Two tokens in 2 SURN: safe repair.
    let data = b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME Maria /Montpeo Osso/\n2 SURN Montpeo, Osso\n0 TRLR\n".to_vec();
    let (fixed, applied) = fix_bytes_with(&data, &FixSelection::default(), &cfg);
    assert!(applied.iter().any(|a| a.contains("W601")));
    assert!(String::from_utf8_lossy(&fixed).contains("2 SURN Montpeo Osso"));

    // Three tokens in 2 SURN: reported by the rule, never repaired.
    let data3 =
        b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME A /B/\n2 SURN Un, Dos, Tres\n0 TRLR\n"
            .to_vec();
    let (_f3, applied3) = fix_bytes_with(&data3, &FixSelection::default(), &cfg);
    assert!(!applied3.iter().any(|a| a.contains("W601")));

    // A NOTE value that contains the tag text is not mistaken for the line.
    let note = b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME A /B/\n1 NOTE his SURN was Un, Dos\n0 TRLR\n".to_vec();
    let (f2, applied2) = fix_bytes_with(&note, &FixSelection::default(), &cfg);
    assert!(!applied2.iter().any(|a| a.contains("W601")));
    assert!(String::from_utf8_lossy(&f2).contains("NOTE his SURN was Un, Dos"));

    // A SURN under some other level-1 tag (here _MARNM) is out of scope:
    // the diagnostic does not fire there, so neither may the repair. Both
    // rulesets on and --unsafe, so the only thing that can stop the repair
    // is the scope guard itself.
    let stray = b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME A /Prat/\n1 _MARNM Un, Dos\n2 SURN Un, Dos\n0 TRLR\n".to_vec();
    let sel = FixSelection {
        allow_unsafe: true,
        ..Default::default()
    };
    let (f3, applied3) = fix_bytes_with(&stray, &sel, &fix_cfg(&["hispanic-naming", "hygiene"]));
    assert!(!applied3
        .iter()
        .any(|a| a.contains("W601") || a.contains("W702")));
    assert!(String::from_utf8_lossy(&f3).contains("2 SURN Un, Dos"));
}

#[test]
fn w602_no_married_name() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n2 _MARNM C\n");
    assert!(has_with(&g, "W602", "hispanic-naming"));

    // W602 produces no edit, even with the ruleset enabled: deleting the
    // tag would destroy data, so there is nothing to select.
    let data =
        b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME A /B/\n2 _MARNM C\n0 TRLR\n".to_vec();
    let edits = compute_edits_with(&data, &fix_cfg(&["hispanic-naming"]));
    assert!(!edits.iter().any(|e| e.code == "W602"));
}

#[test]
fn w603_no_abbreviated_given_name() {
    let g = wrap551("0 @I1@ INDI\n1 NAME Fco. /Perez/\n");
    assert!(has_with(&g, "W603", "hispanic-naming"));
}

#[test]
fn w603_fires_on_givn_subtag_alone() {
    // The abbreviation lives ONLY in 2 GIVN; the NAME value is clean.
    let g = wrap551("0 @I1@ INDI\n1 NAME Fco /Perez/\n2 GIVN Fco.\n");
    assert!(has_with(&g, "W603", "hispanic-naming"));
    let ok = wrap551("0 @I1@ INDI\n1 NAME Fco /Perez/\n2 GIVN Francesc\n");
    assert!(!has_with(&ok, "W603", "hispanic-naming"));

    // Same abbreviation in NAME and GIVN: one diagnostic.
    let both = wrap551("0 @I1@ INDI\n1 NAME Fco. /Perez/\n2 GIVN Fco.\n");
    let n = codes_with(&both, "hispanic-naming")
        .iter()
        .filter(|c| c.as_str() == "W603")
        .count();
    assert_eq!(n, 1);
}

#[test]
fn w701_polluted_name() {
    let g = wrap551("0 @I1@ INDI\n1 NAME Joan (b. 1850) /Perez/\n");
    assert!(has_with(&g, "W701", "hygiene"));
    let g2 = wrap551("0 @I1@ INDI\n1 NAME Joan /Perez (twin)/\n");
    assert!(has_with(&g2, "W701", "hygiene"));

    // legitimate names it must not flag, including Catalan house name
    let ok = wrap551("0 @I1@ INDI\n1 NAME Joan /Perez (cal Ferrer)/\n");
    assert!(!has_with(&ok, "W701", "hygiene"));
    let ok2 = wrap551("0 @I1@ INDI\n1 NAME Joan /Perez (can X)/\n");
    assert!(!has_with(&ok2, "W701", "hygiene"));
    let ok3 = wrap551("0 @I1@ INDI\n1 NAME Joan /Perez (Mas Y)/\n");
    assert!(!has_with(&ok3, "W701", "hygiene"));
}

#[test]
fn w701_fires_on_surn_subtag_alone() {
    // The pollution lives ONLY in 2 SURN; the NAME value is clean.
    let g = wrap551("0 @I1@ INDI\n1 NAME Pere /Valles/\n2 SURN Valles (moliner)\n");
    assert!(has_with(&g, "W701", "hygiene"));
    let ok = wrap551("0 @I1@ INDI\n1 NAME Pere /Valles/\n2 SURN Valles\n");
    assert!(!has_with(&ok, "W701", "hygiene"));

    // Same pollution in NAME and SURN: one diagnostic.
    let both = wrap551("0 @I1@ INDI\n1 NAME Pere /Valles (moliner)/\n2 SURN Valles (moliner)\n");
    let n = codes_with(&both, "hygiene")
        .iter()
        .filter(|c| c.as_str() == "W701")
        .count();
    assert_eq!(n, 1);
}

#[test]
fn w702_all_caps_name() {
    let g = wrap551("0 @I1@ INDI\n1 NAME Joan /PEREZ/\n");
    assert!(has_with(&g, "W702", "hygiene"));
    let ok = wrap551("0 @I1@ INDI\n1 NAME Joan /Perez/\n");
    assert!(!has_with(&ok, "W702", "hygiene"));
}

#[test]
fn w702_fires_on_surn_subtag_alone() {
    // The all-caps surname lives ONLY in 2 SURN; the NAME value is clean.
    let g = wrap551("0 @I1@ INDI\n1 NAME Anna /Puig/\n2 SURN PUIG SOLE\n");
    assert!(has_with(&g, "W702", "hygiene"));
    let ok = wrap551("0 @I1@ INDI\n1 NAME Anna /Puig/\n2 SURN Puig Sole\n");
    assert!(!has_with(&ok, "W702", "hygiene"));

    // All-caps in both NAME slot and SURN: one diagnostic.
    let both = wrap551("0 @I1@ INDI\n1 NAME Anna /PUIG SOLE/\n2 SURN PUIG SOLE\n");
    let n = codes_with(&both, "hygiene")
        .iter()
        .filter(|c| c.as_str() == "W702")
        .count();
    assert_eq!(n, 1);
}

#[test]
fn w702_surn_repair_is_maybe_incorrect_only() {
    let cfg = fix_cfg(&["hygiene"]);
    // With the ruleset enabled, a bare --fix (Safe only) must still not
    // touch the SURN value: this is the applicability gate, not the config
    // one, doing the refusing.
    let data = b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME Anna /Puig/\n2 SURN PUIG SOLE\n0 TRLR\n".to_vec();
    let (f1, applied) = fix_bytes_with(&data, &FixSelection::default(), &cfg);
    assert!(!applied.iter().any(|a| a.contains("W702")));
    assert!(String::from_utf8_lossy(&f1).contains("2 SURN PUIG SOLE"));

    // --unsafe reaches it under the same config.
    let sel = FixSelection {
        allow_unsafe: true,
        ..Default::default()
    };
    let (fixed, applied2) = fix_bytes_with(&data, &sel, &cfg);
    assert!(applied2.iter().any(|a| a.contains("W702")));
    assert!(String::from_utf8_lossy(&fixed).contains("2 SURN Puig Sole"));
}

#[test]
fn w703_malformed_place() {
    let g = wrap551("0 @I1@ INDI\n1 BIRT\n2 PLAC Alcover, , Tarragona\n");
    assert!(has_with(&g, "W703", "hygiene"));
    let g2 = wrap551("0 @I1@ INDI\n1 BIRT\n2 PLAC Alcover,,Tarragona\n");
    assert!(has_with(&g2, "W703", "hygiene"));

    // No W703 double-reporting on URL (W401 already reports it)
    let url = wrap551("0 @I1@ INDI\n1 BIRT\n2 PLAC Reus https://example.com/x\n");
    assert!(has(&url, "W401"));
    assert!(!has_with(&url, "W703", "hygiene"));

    // Doubled commas repair is Safe, and applies once hygiene is enabled
    let data =
        b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 BIRT\n2 PLAC Alcover, , Tarragona\n0 TRLR\n"
            .to_vec();
    let (fixed, applied) = fix_bytes_with(&data, &FixSelection::default(), &fix_cfg(&["hygiene"]));
    assert!(applied.iter().any(|a| a.contains("W703")));
    assert!(String::from_utf8_lossy(&fixed).contains("PLAC Alcover, Tarragona"));
}

#[test]
fn hispanic_naming_rules_are_off_by_default() {
    let g = wrap551("0 @I1@ INDI\n1 NAME Fco. /A, B/\n2 GIVN Fco.\n2 SURN A, B\n2 _MARNM C\n");
    assert!(!has(&g, "W601"));
    assert!(!has(&g, "W602"));
    assert!(!has(&g, "W603"));
}

#[test]
fn hygiene_rules_are_off_by_default() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A (1) /B/\n2 SURN B (2)\n0 @I2@ INDI\n1 NAME A /CAPS/\n2 SURN CAPS\n1 BIRT\n2 PLAC X, , Y\n");
    assert!(!has(&g, "W701"));
    assert!(!has(&g, "W702"));
    assert!(!has(&g, "W703"));
}
