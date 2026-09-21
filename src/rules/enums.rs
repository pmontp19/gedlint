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
// 7.0 ships the same MEDI set (enumset-MEDI).
const MEDI70: &[&str] = MEDI551;
// 5.5.1 MULTIMEDIA_FORMAT (the FORM payload under OBJE/FILE). The spec
// ships the three-letter forms and its errata pins {Size=3:3}, but the
// Committee's own TGC551 and ged-inline.org both accept the four-letter
// spellings too, so the check takes both: the point is catching URL, RTF
// and PICT, not relitigating JPG vs JPEG on files every exporter writes.
const FORM551: &[&str] = &[
    "BMP", "GIF", "JPG", "JPEG", "OLE", "PCX", "TIF", "TIFF", "WAV",
];
// 5.5.1 LDS statuses, one set per ordinance kind (spec p.51-52): baptism
// for BAPL/CONL, endowment for ENDL (the errata removed INFANT from it),
// child sealing for SLGC, spouse sealing for SLGS. Hyphens are the spec
// spelling (PRE-1970, DNS/CAN); the 7.0 underscore forms ride along as
// tolerated aliases, not as flags.
const STAT551_BAPTISM: &[&str] = &[
    "CHILD",
    "CLEARED",
    "COMPLETED",
    "INFANT",
    "PRE-1970",
    "PRE_1970",
    "QUALIFIED",
    "STILLBORN",
    "SUBMITTED",
    "UNCLEARED",
];
const STAT551_ENDOWMENT: &[&str] = &[
    "CHILD",
    "CLEARED",
    "COMPLETED",
    "PRE-1970",
    "PRE_1970",
    "QUALIFIED",
    "STILLBORN",
    "SUBMITTED",
    "UNCLEARED",
];
const STAT551_CHILD_SEALING: &[&str] = &[
    "BIC",
    "CLEARED",
    "COMPLETED",
    "DNS",
    "PRE-1970",
    "PRE_1970",
    "QUALIFIED",
    "STILLBORN",
    "SUBMITTED",
    "UNCLEARED",
];
const STAT551_SPOUSE_SEALING: &[&str] = &[
    "BIC",
    "CANCELED",
    "COMPLETED",
    "DNS",
    "DNS/CAN",
    "DNS_CAN",
    "EXCLUDED",
    "PRE-1970",
    "PRE_1970",
    "SUBMITTED",
    "UNCLEARED",
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

/// True when `v` matches the 7.0 extTag production: an underscore followed
/// by uppercase letters, digits or underscores (grammar.abnf: `extTag =
/// underscore 1*tagchar`). Anything else is not a legal extension value and
/// must keep flowing through the enum check.
fn is_ext_tag(v: &str) -> bool {
    let Some(rest) = v.strip_prefix('_') else {
        return false;
    };
    !rest.is_empty()
        && rest
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

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
    // 7.0: an enumeration payload always permits extTag values (spec
    // type-Enum: "Payload values that match production extTag are always
    // permitted"), so a well-formed underscore value is a legal extension,
    // never a defect. The official extensions.ged leans on exactly this
    // (_ENUMVAL under FAMC.PEDI, _CHILD under ASSO.ROLE).
    if version == Version::V70 && is_ext_tag(v) {
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
        "MEDI" => Some((
            if version == Version::V70 {
                MEDI70
            } else {
                MEDI551
            },
            "FILE.FORM.MEDI",
        )),
        // 5.5.1 FORM is the MULTIMEDIA_FORMAT enum (bmp/gif/jpg/...);
        // 7.0 FORM is a media type handled above. Gated on proven 5.5.1:
        // an unknown-version file with "FORM image/jpeg" must not be
        // judged against the 5.5.1 registry.
        "FORM" if version == Version::V551 && matches!(parent_tag, "OBJE" | "FILE") => {
            Some((FORM551, "OBJE.FORM"))
        }
        // 5.5.1 ordinance STAT: one of the four LDS status sets, chosen
        // by the parent ordinance (INIL is a 7.0 tag; it falls back to
        // the baptism set and is suspicious on a 5.5.1 file for that).
        "STAT" if LDS_EVENTS.contains(&parent_tag) && version == Version::V551 => Some((
            match parent_tag {
                "ENDL" | "INIL" => STAT551_ENDOWMENT,
                "SLGC" => STAT551_CHILD_SEALING,
                "SLGS" => STAT551_SPOUSE_SEALING,
                _ => STAT551_BAPTISM,
            },
            "LDS.STAT",
        )),
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
            // A well-formed extTag token is an extension value, always
            // permitted in 7.0.
            let t = t.trim();
            t.is_empty() || is_ext_tag(t) || allowed.contains(&t)
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
