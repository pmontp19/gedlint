//! W306: enumerated payloads checked against the spec registries, plus the
//! OTHER/PHRASE pairing flushed at the end of the run.

use std::collections::HashSet;

use crate::diag::{Category, Diag, Severity};
use crate::parse::Version;
use crate::rules::events::LDS_EVENTS;

// ---------------------------------------------------------------------------
// GEDCOM 7 enumeration sets (exact spellings from the spec registries).
// Lowercase variants are the 5.5.1 spellings where they differ.
const ROLE: &[&str] = &[
    "CHIL",
    "CLERGY",
    "FATH",
    "FRIEND",
    "GODP",
    "HUSB",
    "MOTH",
    "MULTIPLE",
    "NGHBR",
    "OFFICIATOR",
    "PARENT",
    "SPOU",
    "WIFE",
    "WITN",
    "OTHER",
];
// 5.5.1 has no ASSO.ROLE; its ROLE belongs to the source citation
// (SOUR.EVEN.ROLE) and has a different, smaller set.
const ROLE551: &[&str] = &["chil", "husb", "wife", "moth", "fath", "spou"];
const PEDI70: &[&str] = &["ADOPTED", "BIRTH", "FOSTER", "SEALING", "OTHER"];
const PEDI551: &[&str] = &["adopted", "birth", "foster", "sealing", "other"];
const QUAY: &[&str] = &["0", "1", "2", "3"];
const RESN70: &[&str] = &["CONFIDENTIAL", "LOCKED", "PRIVACY"];
const RESN551: &[&str] = &["confidential", "locked", "privacy"];
const FAMC_STAT: &[&str] = &["CHALLENGED", "DISPROVEN", "PROVEN"];
const NAME_TYPE70: &[&str] = &[
    "AKA",
    "BIRTH",
    "IMMIGRANT",
    "MAIDEN",
    "MARRIED",
    "OTHER",
    "PROFESSIONAL",
];
const NAME_TYPE551: &[&str] = &[
    "aka",
    "birth",
    "immigrant",
    "maiden",
    "married",
    "other",
    "professional",
];
const MEDI551: &[&str] = &[
    "AUDIO",
    "BOOK",
    "CARD",
    "ELECTRONIC",
    "FICHE",
    "FILM",
    "MAGAZINE",
    "MANUSCRIPT",
    "MAP",
    "NEWSPAPER",
    "OTHER",
    "PHOTO",
    "TOMBSTONE",
    "VIDEO",
];
const ORD_STAT: &[&str] = &[
    "BIC",
    "CANCELED",
    "CHILD",
    "COMPLETED",
    "DNS",
    "DNS_CAN",
    "EXCLUDED",
    "INFANT",
    "PRE",
    "PRE_1970",
    "STILLBORN",
    "SUBMITTED",
    "UNCLEARED",
];
// SOUR.DATA.EVEN payloads (7.0): type-List#Enum of event/attribute tags
// (spec: "a parish register of births, deaths, and marriages would be
// BIRT, DEAT, MARR").
const EVENATTR: &[&str] = &[
    "ADOP", "ANUL", "BAPM", "BARM", "BASM", "BIRT", "BLES", "BURI", "CAST", "CENS", "CHR", "CHRA",
    "CONF", "CREM", "DEAT", "DIV", "DIVF", "DSCR", "EDUC", "EMIG", "ENGA", "EVEN", "FACT", "FCOM",
    "GRAD", "IDNO", "IMMI", "MARB", "MARC", "MARL", "MARR", "MARS", "NATI", "NATU", "NCHI", "OCCU",
    "ORDN", "PROB", "PROP", "RELI", "RESI", "RETI", "SSN", "TITL", "WILL",
];

/// OTHER values awaiting a sibling PHRASE, and the slots a PHRASE was seen on.
#[derive(Default)]
pub(crate) struct EnumState {
    // (record, parent_tag, parent_line, tag, line).
    pub(crate) pending_other: Vec<(String, String, usize, String, usize)>,
    pub(crate) phrased: HashSet<(String, String, usize)>,
}

/// W306: enumerated values (ROLE/PEDI/QUAY/RESN/STAT/TYPE/MEDI) against the
/// 7.0 registry spellings (lowercase variants under 5.5.1). OTHER values
/// are registered for the sibling-PHRASE check flushed at end of run.
#[allow(clippy::too_many_arguments)]
pub(crate) fn check_enum(
    diags: &mut Vec<Diag>,
    tag: &str,
    value: &str,
    parent_tag: &str,
    record: &str,
    parent_key: Option<(String, usize)>,
    pending_other: &mut Vec<(String, String, usize, String, usize)>,
    version: Version,
    line: usize,
) {
    let v = value.trim();
    if v.is_empty() {
        return;
    }
    // OBJE.FILE.FORM under 7.0 is a media type, not an enum.
    if tag == "FORM" && parent_tag == "FILE" && version == Version::V70 {
        if !v.contains('/') || v.chars().any(char::is_whitespace) {
            let who = if record.is_empty() {
                format!("line {}", line)
            } else {
                record.to_string()
            };
            diags.push(Diag::new(
                "W306",
                Category::Suspicious,
                Severity::Warning,
                line,
                format!("{}: invalid media type {:?} (use image/jpeg etc.)", who, v),
            ));
        }
        return;
    }
    let set: Option<(&[&str], &str)> = match tag {
        "ROLE" => {
            if version == Version::V70 {
                Some((ROLE, "ASSO.ROLE"))
            } else {
                Some((ROLE551, "ROLE"))
            }
        }
        "PEDI" => Some((
            if version == Version::V70 {
                PEDI70
            } else {
                PEDI551
            },
            "FAMC.PEDI",
        )),
        "QUAY" => Some((QUAY, "SOUR.QUAY")),
        "RESN" => Some((
            if version == Version::V70 {
                RESN70
            } else {
                RESN551
            },
            "RESN",
        )),
        "STAT" if parent_tag == "FAMC" && version != Version::V551 => {
            Some((FAMC_STAT, "FAMC.STAT"))
        }
        "STAT" if LDS_EVENTS.contains(&parent_tag) && version != Version::V551 => {
            Some((ORD_STAT, "LDS.STAT"))
        }
        "EVEN" if parent_tag == "DATA" && version == Version::V70 => Some((EVENATTR, "DATA.EVEN")),
        "TYPE" if parent_tag == "NAME" => Some((
            if version == Version::V70 {
                NAME_TYPE70
            } else {
                NAME_TYPE551
            },
            "NAME.TYPE",
        )),
        "MEDI" if version != Version::V70 => Some((MEDI551, "FILE.FORM.MEDI")),
        _ => None,
    };
    let Some((allowed, what)) = set else { return };
    // 7.0 RESN and SOUR.DATA.EVEN payloads are type-List#Enum: a
    // comma-separated list of enum values, each valid on its own
    // (maximal70.ged: "RESN CONFIDENTIAL, LOCKED", "EVEN BIRT, DEAT").
    if version == Version::V70 && (tag == "RESN" || (tag == "EVEN" && parent_tag == "DATA")) {
        let all_ok = v.split(',').all(|t| {
            // Empty tokens (trailing comma, "A, ,B") are tolerated:
            // exporter quirk, the meaningful tokens still get validated.
            let t = t.trim();
            t.is_empty() || allowed.contains(&t)
        });
        if all_ok {
            return;
        }
        let who = if record.is_empty() {
            format!("line {}", line)
        } else {
            record.to_string()
        };
        diags.push(Diag::new(
            "W306",
            Category::Suspicious,
            Severity::Warning,
            line,
            format!(
                "{}: invalid {} value {:?} (expected: {})",
                who,
                what,
                v,
                allowed.join("|")
            ),
        ));
        return;
    }
    // 5.5.1 enum checks accept any case: the spec spells them lowercase but
    // commercial exporters capitalize ("TYPE Birth", "PEDI ADOPTED").
    let ci = version == Version::V551;
    let hit = if ci {
        allowed.iter().any(|s| s.eq_ignore_ascii_case(v))
    } else {
        allowed.contains(&v)
    };
    if hit {
        let is_other = if ci {
            v.eq_ignore_ascii_case("other")
        } else {
            v == "OTHER"
        };
        if (tag == "ROLE" || tag == "PEDI" || tag == "TYPE") && is_other {
            if let Some((ptag, pline)) = parent_key {
                pending_other.push((record.to_string(), ptag, pline, tag.to_string(), line));
            }
        }
        return;
    }
    let who = if record.is_empty() {
        format!("line {}", line)
    } else {
        record.to_string()
    };
    diags.push(Diag::new(
        "W306",
        Category::Suspicious,
        Severity::Warning,
        line,
        format!(
            "{}: invalid {} value {:?} (expected: {})",
            who,
            what,
            v,
            allowed.join("|")
        ),
    ));
}

/// End of run: OTHER enum values want a sibling PHRASE with the free text.
pub(crate) fn finish(diags: &mut Vec<Diag>, st: &EnumState) {
    // W306: OTHER enum values want a sibling PHRASE with the free text.
    for (rec, ptag, pline, tag, line) in &st.pending_other {
        if !st.phrased.contains(&(rec.clone(), ptag.clone(), *pline)) {
            let who = if rec.is_empty() {
                format!("line {}", line)
            } else {
                rec.clone()
            };
            diags.push(Diag::new(
                "W306",
                Category::Suspicious,
                Severity::Info,
                *line,
                format!(
                    "{}: {} OTHER without a sibling PHRASE (add the free-text phrase)",
                    who, tag
                ),
            ));
        }
    }
}
