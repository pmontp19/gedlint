//! Upgrade path 5.5.1 -> 7.0: tags removed from the standard (U501) and
//! vendor extensions that survive as undocumented ones (U502).

use crate::diag::{Category, Diag, Severity};
use crate::parse::{Line, Version};

/// U501 at level 1: RELA was removed in 7.0.
pub(crate) fn check_rela_record(diags: &mut Vec<Diag>, l: &Line, version: Version) {
    if version == Version::V551 && l.tag == "RELA" {
        diags.push(Diag::new(
            "U501",
            Category::Upgrade,
            Severity::Info,
            l.no,
            "RELA removed in 7.0: use enumerated ROLE (see gedcom.io/migrate)".into(),
        ));
    }
}

/// U502: vendor tags kept as undocumented extensions in 7.0.
pub(crate) fn check_vendor_tag(diags: &mut Vec<Diag>, l: &Line, version: Version) {
    if version == Version::V551
        && l.tag.starts_with("_")
        && matches!(l.tag.as_str(), "_MARNM" | "_UPD" | "_APID" | "_OID")
    {
        diags.push(Diag::new(
            "U502",
            Category::Upgrade,
            Severity::Info,
            l.no,
            format!("vendor tag {}: kept as an undocumented extension in 7.0 (add a SCHMA TAG definition)", l.tag)
        ));
    }
}

/// U501 below level 1: RELA also at sublevels (e.g. ASSO.RELA).
pub(crate) fn check_rela_sub(diags: &mut Vec<Diag>, l: &Line, version: Version) {
    if version == Version::V551 && l.tag == "RELA" {
        diags.push(Diag::new(
            "U501",
            Category::Upgrade,
            Severity::Info,
            l.no,
            "RELA removed in 7.0: use enumerated ROLE (see gedcom.io/migrate)".into(),
        ));
    }
}

/// U501: lowercase PEDI, which 7.0 requires in uppercase.
pub(crate) fn check_pedi_case(diags: &mut Vec<Diag>, l: &Line, version: Version) {
    if l.tag == "PEDI" && version == Version::V551 {
        let v = l.value.trim();
        if v != v.to_uppercase() {
            diags.push(Diag::new(
                "U501",
                Category::Upgrade,
                Severity::Info,
                l.no,
                format!("lowercase PEDI ({}): 7.0 requires uppercase", v),
            ));
        }
    }
}
