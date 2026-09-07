//! Exporter-quirk style rules: control characters (W102), URLs inside PLAC
//! (W401) and HTML inside NOTE (W403).

use crate::diag::{push_capped, Category, Diag, Severity};
use crate::parse::{truncate, Line};

/// W102: ASCII controls (other than \t).
pub(crate) fn check_control_chars(diags: &mut Vec<Diag>, l: &Line) {
    if l.raw.chars().any(|c| c.is_control() && c != '\t') {
        push_capped(
            diags,
            vec![Diag::new(
                "W102",
                Category::Style,
                Severity::Warning,
                l.no,
                "control character inside line".into(),
            )],
        );
    }
}

/// W401 at level 1: URL inside PLAC (MyHeritage quirk).
pub(crate) fn check_plac_url_record(diags: &mut Vec<Diag>, l: &Line) {
    if l.tag == "PLAC" && l.value.contains("http") {
        push_capped(
            diags,
            vec![Diag::new(
                "W401",
                Category::Style,
                Severity::Warning,
                l.no,
                format!("PLAC with URL (MyHeritage quirk): move it to NOTE: {}", truncate(&l.value, 60)),
            )],
        );
    }
}

/// W403: HTML notes inside NOTE.
pub(crate) fn check_note_html(diags: &mut Vec<Diag>, l: &Line) {
    if l.tag == "NOTE" && (l.value.contains("<br") || l.value.contains("<notexml") || l.value.contains("&nbsp")) {
        push_capped(
            diags,
            vec![Diag::new(
                "W403",
                Category::Style,
                Severity::Warning,
                l.no,
                format!("NOTE with HTML (exporter quirk): {}", truncate(&l.value, 60)),
            )],
        );
    }
}

/// W401 below level 1: nested PLAC with URL (MyHeritage quirk).
pub(crate) fn check_plac_url(diags: &mut Vec<Diag>, l: &Line) {
    if l.tag == "PLAC" && l.value.contains("http") {
        push_capped(
            diags,
            vec![Diag::new(
                "W401",
                Category::Style,
                Severity::Warning,
                l.no,
                format!("PLAC with URL (MyHeritage quirk): {}", truncate(&l.value, 60)),
            )],
        );
    }
}
