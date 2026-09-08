//! C ABI for the WebAssembly build (issue 15, RFC 014 section 9).
//!
//! No wasm-bindgen: plain `extern "C"` exports keep the engine usable from
//! vanilla JS with zero dependencies, which is the repo's rule. The module
//! is pure memory arithmetic over the existing engine entry points; it
//! touches no fs, process, env or net, so `src/lib.rs` stays WASM-clean.
//!
//! The contract, in full:
//!
//! * Buffers cross the boundary as `(ptr, len)` pairs. Input pointers stay
//!   valid for the duration of the call only; the caller keeps ownership
//!   and frees them itself.
//! * A caller-side buffer comes from [`gedlint_alloc`] (exactly `len`
//!   bytes, zeroed) and goes back with [`gedlint_dealloc`] using the
//!   **same** `len` it was allocated with.
//! * Every entry point returns a pointer to a 16-byte heap header:
//!   `[u64 LE ptr][u64 LE len]`. The payload is a second heap buffer of
//!   exactly `len` bytes. The caller frees the payload with
//!   `dealloc(ptr, len)` and then the header with `dealloc(header, 16)`.
//!   Returning the length next to the pointer means JS never has to scan
//!   for a terminator.
//! * [`gedlint_apply`] is the one payload that is not JSON: it is
//!   `[u64 LE json_len][summary JSON][repaired file bytes]`, because the
//!   repaired file may not be valid UTF-8 (the E101 rejoin exists exactly
//!   for such files) and must not ride inside a JSON string.
//! * Every entry point that lints or repairs takes the configuration as a
//!   second `(ptr, len)` buffer of `gedlint.toml` text (empty = default)
//!   and reports a broken one through `{"error": "..."}`. `gedlint_edits`
//!   and `gedlint_apply` must be handed the **same** config the lint used:
//!   `gedlint_apply` matches its mask against the edit list by index, so
//!   two configurations that produce different lists would silently select
//!   the wrong repairs (the mask-length check below catches a length
//!   mismatch, and both lists come from `compute_edits_with` on the same
//!   bytes and config, so they cannot diverge).
//! * The repair selection is a **mask**, one byte per edit in the order
//!   [`gedlint_edits`] returned for the same input and configuration,
//!   rather than serialized edits: round-tripping `Edit.replacement`
//!   through JSON could not carry non-UTF-8 bytes without a base64 layer,
//!   and a mask cannot desynchronize.

use crate::config::{parse_config, Config};
use crate::diag::escape_json;
use crate::fix::{apply_edits, compute_edits_with, normalize_endings, Edit};
use crate::lint_bytes_with;
use crate::registry::rules_to_json;

/// Size of the `[ptr][len]` header every entry point returns.
const HEADER_BYTES: usize = 16;

// ---------------------------------------------------------------------------
// Memory management
// ---------------------------------------------------------------------------

/// Allocate exactly `len` zeroed bytes. `len` 0 returns a non-null dangling
/// pointer (freeing it is a no-op). The caller frees with
/// [`gedlint_dealloc`] and the same `len`.
#[no_mangle]
pub extern "C" fn gedlint_alloc(len: usize) -> *mut u8 {
    // into_boxed_slice guarantees capacity == len, so dealloc can rebuild
    // the Vec from (ptr, len) alone.
    Box::leak(vec![0u8; len].into_boxed_slice()).as_mut_ptr()
}

/// Free a buffer from [`gedlint_alloc`] (or a returned payload), passing the
/// same length it was allocated with. `len` 0 is a no-op.
///
/// # Safety
/// `ptr` must come from [`gedlint_alloc`] or be the payload/header pointer of
/// a previous return value, and `len` must be the size that buffer was
/// created with.
#[no_mangle]
pub unsafe extern "C" fn gedlint_dealloc(ptr: *mut u8, len: usize) {
    if len == 0 || ptr.is_null() {
        return;
    }
    drop(unsafe { Vec::from_raw_parts(ptr, len, len) });
}

/// Borrow `(ptr, len)` as a slice for the duration of the call.
///
/// # Safety
/// `ptr..ptr+len` must be valid for reads and free of mutable aliases for
/// the whole call, which is how the JS host invokes these entry points
/// (it blocks on the synchronous call).
unsafe fn input(ptr: *const u8, len: usize) -> &'static [u8] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }
}

/// Leak `bytes` (capacity pinned to its length) and return the 16-byte
/// header describing it.
fn ret(bytes: Vec<u8>) -> *mut u8 {
    let leaked = Box::leak(bytes.into_boxed_slice());
    let mut head = Vec::with_capacity(HEADER_BYTES);
    head.extend_from_slice(&(leaked.as_ptr() as usize as u64).to_le_bytes());
    head.extend_from_slice(&(leaked.len() as u64).to_le_bytes());
    Box::leak(head.into_boxed_slice()).as_mut_ptr()
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// Full report JSON for a GEDCOM buffer under a config given as `gedlint.toml`
/// text. An empty config is the built-in default; a broken one returns
/// `{"error": "..."}` instead of a report.
///
/// # Safety
/// `data..data+data_len` and `cfg..cfg+cfg_len` must be valid for reads for
/// the duration of the call (the JS host blocks on it synchronously).
#[no_mangle]
pub unsafe extern "C" fn gedlint_lint(data: *const u8, data_len: usize, cfg: *const u8, cfg_len: usize) -> *mut u8 {
    let data = unsafe { input(data, data_len) };
    let cfg = unsafe { input(cfg, cfg_len) };
    ret(lint_entry(data, cfg).into_bytes())
}

/// [`crate::registry::RULES`] as JSON: code, name, ruleset, category,
/// severities, fixability and the `title`/`why`/`remedy` documentation the
/// finding card renders.
#[no_mangle]
pub extern "C" fn gedlint_registry() -> *mut u8 {
    ret(rules_to_json().into_bytes())
}

/// Every candidate repair as JSON, in the engine's repair-priority order,
/// gated at production by the configuration (#44): an edit belonging to a
/// rule the configuration disables is never offered, so the panel cannot
/// offer to rewrite a file by a convention the user never enabled.
/// `normalized_endings` says whether classic Mac CR endings were rewritten,
/// which is preprocessing every repair assumes, not an edit.
///
/// # Safety
/// `data..data+data_len` and `cfg..cfg+cfg_len` must be valid for reads for
/// the duration of the call (the JS host blocks on it synchronously).
#[no_mangle]
pub unsafe extern "C" fn gedlint_edits(data: *const u8, data_len: usize, cfg: *const u8, cfg_len: usize) -> *mut u8 {
    let data = unsafe { input(data, data_len) };
    let cfg = unsafe { input(cfg, cfg_len) };
    ret(edits_entry(data, cfg).into_bytes())
}

/// Apply the selected subset (see the module docs for the mask layout) and
/// return the repaired file behind a JSON summary. The mask indexes the
/// list [`gedlint_edits`] returned for the **same input and configuration**;
/// a different config can produce a different list and select the wrong
/// repairs, so callers must pass the config unchanged.
///
/// # Safety
/// `data..data+data_len`, `mask..mask+mask_len` and `cfg..cfg+cfg_len` must
/// be valid for reads for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn gedlint_apply(data: *const u8, data_len: usize, mask: *const u8, mask_len: usize, cfg: *const u8, cfg_len: usize) -> *mut u8 {
    let data = unsafe { input(data, data_len) };
    let mask = unsafe { input(mask, mask_len) };
    let cfg = unsafe { input(cfg, cfg_len) };
    ret(apply_entry(data, mask, cfg))
}

// ---------------------------------------------------------------------------
// Pure bodies (unit-testable without pointer plumbing)
// ---------------------------------------------------------------------------

/// Parse a config buffer shared by every entry point: empty is the default,
/// anything else must be `gedlint.toml` text. The error comes back as the
/// message `error_json` will wrap, so all three entry points report config
/// problems identically.
fn parse_cfg(cfg: &[u8]) -> Result<Config, String> {
    if cfg.is_empty() {
        return Ok(Config::default());
    }
    match std::str::from_utf8(cfg) {
        Err(_) => Err("the configuration is not valid UTF-8".to_string()),
        Ok(text) => parse_config(text).map_err(|e| e.to_string()),
    }
}

fn lint_entry(data: &[u8], cfg: &[u8]) -> String {
    match parse_cfg(cfg) {
        Err(e) => error_json(&e),
        Ok(c) => lint_bytes_with(data, &c).to_json(),
    }
}

fn edits_entry(data: &[u8], cfg: &[u8]) -> String {
    let c = match parse_cfg(cfg) {
        Err(e) => return error_json(&e),
        Ok(c) => c,
    };
    let (norm, lone_cr) = normalize_endings(data);
    let edits = compute_edits_with(&norm, &c);
    let mut out = String::with_capacity(edits.len() * 96 + 48);
    out.push_str("{\"normalized_endings\":");
    out.push_str(if lone_cr { "true" } else { "false" });
    out.push_str(",\"edits\":[");
    for (i, e) in edits.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"code\":\"");
        out.push_str(e.code);
        out.push_str("\",\"start\":");
        out.push_str(&e.lines.0.to_string());
        out.push_str(",\"end\":");
        out.push_str(&e.lines.1.to_string());
        out.push_str(",\"applicability\":\"");
        out.push_str(e.applicability.as_str());
        out.push_str("\",\"note\":\"");
        out.push_str(&escape_json(&e.note));
        out.push_str("\",\"replacement_preview\":\"");
        out.push_str(&replacement_preview(e));
        out.push_str("\"}");
    }
    out.push_str("]}");
    out
}

/// `[u64 LE json_len][error JSON]`: the payload shape `gedlint_apply` uses
/// for errors, where no file bytes follow the JSON.
fn apply_error(msg: &str) -> Vec<u8> {
    let json = error_json(msg);
    let mut out = Vec::with_capacity(8 + json.len());
    out.extend_from_slice(&(json.len() as u64).to_le_bytes());
    out.extend_from_slice(json.as_bytes());
    out
}

fn apply_entry(data: &[u8], mask: &[u8], cfg: &[u8]) -> Vec<u8> {
    let c = match parse_cfg(cfg) {
        Err(e) => return apply_error(&e),
        Ok(c) => c,
    };
    // The same computation edits_entry ran for the caller's config: the
    // mask indexes that list, so the config must be the one the repairs
    // were listed under.
    let (norm, lone_cr) = normalize_endings(data);
    let edits = compute_edits_with(&norm, &c);
    if mask.len() != edits.len() {
        return apply_error("the selection does not match the current repair list; compute repairs again and retry");
    }
    let chosen: Vec<Edit> = edits.iter().zip(mask.iter()).filter(|(_, &m)| m != 0).map(|(e, _)| e.clone()).collect();
    let (file, dropped) = apply_edits(&norm, &chosen);

    let mut json = String::with_capacity(64 + (chosen.len() + dropped.len()) * 64);
    json.push_str("{\"applied\":[");
    write_edit_list(&mut json, &chosen);
    json.push_str("],\"postponed\":[");
    write_edit_list(&mut json, &dropped);
    json.push_str("],\"normalized_endings\":");
    json.push_str(if lone_cr { "true" } else { "false" });
    json.push_str(",\"bytes\":");
    json.push_str(&file.len().to_string());
    json.push('}');

    let mut out = Vec::with_capacity(8 + json.len() + file.len());
    out.extend_from_slice(&(json.len() as u64).to_le_bytes());
    out.extend_from_slice(json.as_bytes());
    out.extend_from_slice(&file);
    out
}

/// One `{code,start,end,note}` object per edit, comma-separated, for the
/// applied/postponed lists of the apply summary.
fn write_edit_list(out: &mut String, edits: &[Edit]) {
    for (i, e) in edits.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"code\":\"");
        out.push_str(e.code);
        out.push_str("\",\"start\":");
        out.push_str(&e.lines.0.to_string());
        out.push_str(",\"end\":");
        out.push_str(&e.lines.1.to_string());
        out.push_str(",\"note\":\"");
        out.push_str(&escape_json(&e.note));
        out.push_str("\"}");
    }
}


/// First replacement line, lossily decoded and truncated, so the UI can
/// preview the result without holding raw bytes. Empty for deletions.
fn replacement_preview(e: &Edit) -> String {
    let first = e.replacement.first().map(|l| l.as_slice()).unwrap_or(&[]);
    let text = String::from_utf8_lossy(first);
    let text = text.strip_suffix('\r').unwrap_or(&text);
    let mut short: String = text.chars().take(96).collect();
    if short.chars().count() < text.chars().count() {
        short.push_str("...");
    }
    escape_json(&short)
}

fn error_json(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len() + 16);
    out.push_str("{\"error\":\"");
    out.push_str(&escape_json(msg));
    out.push_str("\"}");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_preview_truncates_and_escapes() {
        let e = Edit {
            code: "E001",
            lines: (2, 2),
            replacement: vec![b"a <long> line".to_vec()],
            applicability: crate::fix::Applicability::Safe,
            note: String::new(),
        };
        assert_eq!(replacement_preview(&e), "a <long> line");
        let quoted = Edit {
            code: "E001",
            lines: (2, 2),
            replacement: vec![b"say \"hi\"".to_vec()],
            applicability: crate::fix::Applicability::Safe,
            note: String::new(),
        };
        assert_eq!(replacement_preview(&quoted), "say \\\"hi\\\"");
        let long: Vec<u8> = vec![b'x'; 300];
        let e2 = Edit { replacement: vec![long], lines: (2, 2), ..e };
        let p = replacement_preview(&e2);
        assert!(p.ends_with("...") && p.len() < 110, "{}", p);
        // A deletion has nothing to preview.
        let e3 = Edit { replacement: vec![], lines: (2, 2), ..e2 };
        assert_eq!(replacement_preview(&e3), "");
    }

    #[test]
    fn apply_entry_mask_mismatch_returns_an_error_payload() {
        let out = apply_entry(b"0 HEAD\n", &[1, 2, 3], b"");
        let total = out.len();
        let n = u64::from_le_bytes(out[..8].try_into().unwrap()) as usize;
        let json = String::from_utf8(out[8..].to_vec()).unwrap();
        assert!(json.contains("\"error\""), "{}", json);
        assert_eq!(total, 8 + n, "json length header must describe the rest of the payload");
        // A broken config is an error payload too, not a default-config run.
        let bad = apply_entry(b"0 HEAD\n", b"", b"[lints]\npresets = [\"nope\"]\n");
        let json = String::from_utf8(bad[8..].to_vec()).unwrap();
        assert!(json.contains("\"error\""), "{}", json);
    }
}
