//! Tests per regla amb fixtures mínims (criteri d'acceptació 5).
//! Cada test construeix el GEDCOM més petit que dispara una sola regla.

use gedlint::{Severity, Version, fix_bytes, lint_bytes, lint_str};

fn codes(input: &str) -> Vec<String> {
    lint_str(input).diags.iter().map(|d| d.code.to_string()).collect()
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
fn w301_death_before_birth() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 12 SEP 1909\n1 DEAT\n2 DATE 3 JAN 1900\n");
    assert!(has(&g, "W301"));
}

#[test]
fn w301_longevity_112() {
    // Entrada de 112 anys del corpus real: ha de disparar W301.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1 JAN 1800\n1 DEAT\n2 DATE 1 JAN 1912\n");
    assert!(has(&g, "W301"));
}

#[test]
fn w302_duplicates() {
    let g = wrap551(
        "0 @I1@ INDI\n1 NAME Joan /Osó/\n1 BIRT\n2 DATE 1861\n0 @I2@ INDI\n1 NAME Joan /Oso/\n1 BIRT\n2 DATE 1862\n",
    );
    // "Osó" vs "Oso" no normalitza accents: usem mateix nom exacte.
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
fn w402_name_slashes() {
    let g = wrap551("0 @I1@ INDI\n1 NAME Joan /Oso\n");
    assert!(has(&g, "W402"));
}

#[test]
fn e101_conc_split_fix() {
    // Construeix el bug MyHeritage a nivell byte: "é" (U+00E9 = C3 A9)
    // partit entre dues línies CONC: primera acaba amb C3, següent comença amb A9.
    let mut data = Vec::new();
    data.extend_from_slice("0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME Jos".as_bytes());
    data.push(0xC3);
    data.extend_from_slice("\n2 CONC ".as_bytes());
    data.push(0xA9);
    data.extend_from_slice(" /Oso/\n0 TRLR\n".as_bytes());
    let r = lint_bytes(&data);
    assert!(r.diags.iter().any(|d| d.code == "E101"), "cal E101, trobat: {:?}", r.diags);
    let (fixed, applied) = fix_bytes(&data);
    assert!(!applied.is_empty());
    let r2 = lint_bytes(&fixed);
    assert!(
        !r2.diags.iter().any(|d| d.code == "E101"),
        "E101 ha de desaparèixer després de --fix: {:?}",
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
fn version_detect_70_and_upgrade_rules() {
    // RELA a 5.5.1 dispara U501; SEX X és vàlid a 7.0 però no a 5.5.1.
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
    // Fixture propi 7.0 (criteri 5): minimal70 de gedcom.io redueix a HEAD+SOUR+TRLR.
    let g = "0 HEAD\n1 GEDC\n2 VERS 7.0\n1 SOUR gedlint\n0 TRLR\n";
    let r = lint_str(g);
    assert_eq!(r.version, Version::V70);
    assert!(!r.diags.iter().any(|d| d.severity == Severity::Error), "errors: {:?}", r.diags);
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
