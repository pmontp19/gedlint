//! Golden-set regression tests (issue 9 external dataset audit).
//! Official public corpora must lint without false positives.
//! Fixtures live in tests/fixtures/golden (see the README there for provenance).

use gedlint::{lint_bytes, Version};

fn read(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/golden/{}",
        env!("CARGO_MANIFEST_DIR"),
        name
    ))
    .unwrap()
}

fn codes(data: &[u8]) -> Vec<String> {
    lint_bytes(data)
        .diags
        .iter()
        .map(|d| d.code.to_string())
        .collect()
}

fn assert_clean(name: &str) {
    let r = lint_bytes(&read(name));
    assert!(r.diags.is_empty(), "{}: {:?}", name, r.diags);
    assert_eq!(r.exit_code(), 0, "{}", name);
}

#[test]
fn minimal70_is_clean() {
    let r = lint_bytes(&read("minimal70.ged"));
    assert_eq!(r.version, Version::V70);
    assert_eq!(r.lines, 4);
    assert_clean("minimal70.ged");
}

#[test]
fn maximal70_is_clean() {
    // The ultimate 7.0 reference file: exercises every standard tag. After
    // the issue-9 fixes (RESN/DATA.EVEN lists, child PHRASE) it must be 0/0/0.
    let r = lint_bytes(&read("maximal70.ged"));
    assert_eq!(r.version, Version::V70);
    assert!(r.diags.is_empty(), "{:?}", r.diags);
}

#[test]
fn spec70_extras_are_clean() {
    assert_clean("same-sex-marriage.ged");
    assert_clean("escapes.ged");
}

#[test]
fn remarriage_files_have_no_w307() {
    for n in ["remarriage1.ged", "remarriage2.ged"] {
        let r = lint_bytes(&read(n));
        assert_eq!(r.version, Version::V70);
        assert!(
            !r.diags.iter().any(|d| d.code == "W307"),
            "{}: {:?}",
            n,
            r.diags
        );
        assert_eq!(r.errors(), 0, "{}", n);
        assert_eq!(r.warnings(), 0, "{}: {:?}", n, r.diags);
    }
}

#[test]
fn tgc551_lf_no_structural_errors() {
    // The GEDCOM Committee 5.5.1 coverage file contains deliberately odd
    // sample values (placeholders, a bogus longevity, a duplicate CHR), so
    // some W/U diagnostics are expected; but nothing structural and none of
    // the issue-9 false positives.
    let r = lint_bytes(&read("TGC551LF.ged"));
    assert_eq!(r.version, Version::V551);
    assert!(
        !r.diags.iter().any(|d| d.code.starts_with('E')),
        "no E-codes expected: {:?}",
        r.diags
    );
    assert!(
        !r.diags.iter().any(|d| d.code == "W402"),
        "one-sided FROM/TO is valid: {:?}",
        r.diags
    );
    let w307: Vec<_> = r.diags.iter().filter(|d| d.code == "W307").collect();
    assert_eq!(
        w307.len(),
        1,
        "only the deliberate CAL-vs-EST CHR: {:?}",
        r.diags
    );
    // Placeholder MEDI/ROLE payloads are genuine W306s.
    let w306: Vec<usize> = r
        .diags
        .iter()
        .filter(|d| d.code == "W306")
        .map(|d| d.line)
        .collect();
    assert_eq!(w306, vec![1398, 1591], "{:?}", r.diags);
}

#[test]
fn tgc551_cr_matches_lf() {
    // Issue 9: the original Committee file uses classic Mac CR endings and
    // must lint exactly like its LF twin.
    let lf = codes(&read("TGC551LF.ged"));
    let cr = codes(&read("TGC551.ged"));
    assert_eq!(
        cr, lf,
        "CR file must produce the same diagnostics as its LF twin"
    );
    let r = lint_bytes(&read("TGC551.ged"));
    assert_eq!(r.version, Version::V551);
    assert_eq!(r.lines, 2161);
    assert!(
        !r.diags.iter().any(|d| d.code.starts_with('E')),
        "{:?}",
        r.diags
    );
}
