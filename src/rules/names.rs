//! NAME accumulation across CONC/CONT and the W402 slash-balance check.
//!
//! W402 must see the whole NAME value: a surname split across CONC lines
//! (Long26CC) is balanced as a whole but odd per line.

use std::collections::HashMap;

use crate::diag::{Category, Diag, Severity};
use crate::parse::{truncate, Line};

/// The NAME run currently open: (xref, line, accumulated value).
#[derive(Default)]
pub(crate) struct Names {
    pub(crate) name_buf: Option<(String, usize, String)>,
}

/// W402 check on the accumulated NAME value (NAME + CONC/CONT run).
pub(crate) fn flush(
    diags: &mut Vec<Diag>,
    buf: &mut Option<(String, usize, String)>,
    names: &mut HashMap<String, String>,
) {
    if let Some((xref, line, val)) = buf.take() {
        names.insert(xref.clone(), val.clone());
        if val.matches('/').count() % 2 != 0 {
            diags.push(Diag::new(
                "W402",
                Category::Style,
                Severity::Warning,
                line,
                format!("{}: NAME with unbalanced slashes: {}", xref, truncate(&val, 50))
            ));
        }
    }
}

/// A level-1 INDI.NAME opens a run; the value may continue via CONC/CONT, so
/// W402 runs on the whole accumulated value at flush time.
pub(crate) fn open(st: &mut Names, l: &Line, xref: &str) {
    st.name_buf = Some((xref.to_string(), l.no, l.value.clone()));
}

/// Level >= 2: append CONC verbatim and CONT after a space. The run is only
/// open while CONC/CONT directly follow the NAME at level 2: any other line
/// (a SOUR between, or a CONC deeper down that belongs to a substructure like
/// SOUR.PAGE) closes it first so foreign values are never absorbed.
pub(crate) fn continue_value(
    diags: &mut Vec<Diag>,
    st: &mut Names,
    names: &mut HashMap<String, String>,
    l: &Line,
    lvl: u32,
) {
    if st.name_buf.is_some() {
        if lvl == 2 && (l.tag == "CONC" || l.tag == "CONT") {
            if let Some((_, _, val)) = &mut st.name_buf {
                if l.tag == "CONC" {
                    val.push_str(&l.value);
                } else {
                    val.push(' ');
                    val.push_str(&l.value);
                }
            }
        } else {
            flush(diags, &mut st.name_buf, names);
        }
    }
}
