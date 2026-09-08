//! Native end-to-end tests of the C ABI the web viewer drives (#15): the
//! exact functions the Web Worker calls, header decoding and the
//! alloc/dealloc symmetry included, so the browser contract cannot rot
//! silently when the engine changes.

use gedlint::wasm::{
    gedlint_alloc, gedlint_apply, gedlint_dealloc, gedlint_edits, gedlint_lint, gedlint_registry,
};
use gedlint::RULES;
use gedlint::{compute_edits, normalize_endings};

/// Decode the 16-byte return header, free it, copy the payload out, free
/// that too: exactly what worker.js does.
fn take(ret: *mut u8) -> Vec<u8> {
    assert!(!ret.is_null(), "entry points always return a header");
    let head = unsafe { std::slice::from_raw_parts(ret, 16) };
    let ptr = u64::from_le_bytes(head[0..8].try_into().unwrap()) as usize;
    let len = u64::from_le_bytes(head[8..16].try_into().unwrap()) as usize;
    unsafe {
        gedlint_dealloc(ret, 16);
        let payload = std::slice::from_raw_parts(ptr as *const u8, len).to_vec();
        gedlint_dealloc(ptr as *mut u8, len);
        payload
    }
}

/// Copy `bytes` into a caller-side buffer the way JS does (alloc, write,
/// call, free with the allocated size). An empty buffer still allocates one
/// byte so the empty-config case is exercised.
fn with_bufs<T>(
    data: &[u8],
    cfg: &[u8],
    f: unsafe extern "C" fn(*const u8, usize, *const u8, usize) -> T,
) -> T {
    let da = data.len().max(1);
    let ca = cfg.len().max(1);
    let dp = gedlint_alloc(da);
    unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), dp, data.len()) };
    let cp = gedlint_alloc(ca);
    unsafe { std::ptr::copy_nonoverlapping(cfg.as_ptr(), cp, cfg.len()) };
    let out = unsafe { f(dp, data.len(), cp, cfg.len()) };
    unsafe {
        gedlint_dealloc(dp, da);
        gedlint_dealloc(cp, ca);
    }
    out
}

/// Same as `with_bufs` for `gedlint_apply`, whose signature also carries
/// the selection mask between the data and config buffers.
fn apply_bufs(data: &[u8], mask: &[u8], cfg: &[u8]) -> *mut u8 {
    let da = data.len().max(1);
    let ma = mask.len().max(1);
    let ca = cfg.len().max(1);
    let dp = gedlint_alloc(da);
    unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), dp, data.len()) };
    let mp = gedlint_alloc(ma);
    unsafe { std::ptr::copy_nonoverlapping(mask.as_ptr(), mp, mask.len()) };
    let cp = gedlint_alloc(ca);
    unsafe { std::ptr::copy_nonoverlapping(cfg.as_ptr(), cp, cfg.len()) };
    let out = unsafe { gedlint_apply(dp, data.len(), mp, mask.len(), cp, cfg.len()) };
    unsafe {
        gedlint_dealloc(dp, da);
        gedlint_dealloc(mp, ma);
        gedlint_dealloc(cp, ca);
    }
    out
}

fn check(data: &[u8], cfg: &[u8]) -> String {
    let ret = with_bufs(data, cfg, gedlint_lint);
    String::from_utf8(take(ret)).unwrap()
}

fn edits(data: &[u8], cfg: &[u8]) -> String {
    let da = data.len().max(1);
    let ca = cfg.len().max(1);
    let dp = gedlint_alloc(da);
    unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), dp, data.len()) };
    let cp = gedlint_alloc(ca);
    unsafe { std::ptr::copy_nonoverlapping(cfg.as_ptr(), cp, cfg.len()) };
    let ret = unsafe { gedlint_edits(dp, data.len(), cp, cfg.len()) };
    unsafe {
        gedlint_dealloc(dp, da);
        gedlint_dealloc(cp, ca);
    }
    String::from_utf8(take(ret)).unwrap()
}

/// The MyHeritage split-character construct the whole E101 path exists for:
/// "José" cut after the first byte of the é.
fn split_conc_file() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME Jos");
    data.push(0xC3);
    data.extend_from_slice(b"\n2 CONC ");
    data.push(0xA9);
    data.extend_from_slice(b" /Oso/\n0 TRLR\n");
    data
}

#[test]
fn lint_returns_the_report_json() {
    let j = check(
        b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME A /B/\n1 SEX Q\n0 TRLR\n",
        b"",
    );
    assert!(j.contains("\"code\":\"W305\""), "{}", j);
    assert!(j.contains("\"summary\":{"), "{}", j);
    // Opt-in ruleset stays off without a config.
    assert!(!j.contains("W70"), "{}", j);
}

#[test]
fn lint_accepts_a_preset_config_and_reports_config_errors() {
    let data = b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME A /MARTI/\n0 TRLR\n";
    let cfg = b"[lints]\npresets = [\"recommended\", \"hygiene\"]\n";
    let j = check(data, cfg);
    assert!(
        j.contains("W702"),
        "hygiene preset must reach the engine: {}",
        j
    );

    let bad = check(data, b"[lints]\npresets = [\"nope\"]\n");
    assert!(bad.contains("\"error\""), "{}", bad);
    assert!(bad.contains("line 2"), "{}", bad);

    let not_utf8 = check(data, &[0xFF, 0xFE]);
    assert!(not_utf8.contains("not valid UTF-8"), "{}", not_utf8);
}

#[test]
fn registry_json_carries_every_rule_and_its_documentation() {
    let j = String::from_utf8(take(gedlint_registry())).unwrap();
    let entries = j.matches("{\"code\":").count();
    assert_eq!(entries, RULES.len(), "one JSON object per registry entry");
    for r in RULES {
        assert!(
            j.contains(&format!("\"code\":\"{}\"", r.code)),
            "{} missing",
            r.code
        );
    }
    // The finding-card payload, and the two fixability spellings.
    assert!(j.contains("\"title\":\""), "{}", j);
    assert!(j.contains("\"why\":\""), "{}", j);
    assert!(j.contains("\"remedy\":\""), "{}", j);
    assert!(j.contains("\"fixable\":\"safe\""), "{}", j);
    assert!(j.contains("\"fixable\":\"maybe-incorrect\""), "{}", j);
    assert!(j.contains("\"fixable\":null"), "{}", j);
    assert!(j.contains("\"ruleset\":\"hispanic-naming\""), "{}", j);
}

#[test]
fn edits_json_matches_compute_edits() {
    let data = split_conc_file();
    let j = edits(&data, b"");
    let expect = compute_edits(&normalize_endings(&data).0);
    assert_eq!(j.matches("\"code\":").count(), expect.len(), "{}", j);
    let e = expect.iter().find(|e| e.code == "E101").unwrap();
    assert!(
        j.contains(&format!(
            "\"code\":\"E101\",\"start\":{},\"end\":{}",
            e.lines.0, e.lines.1
        )),
        "{}",
        j
    );
    assert!(j.contains("\"applicability\":\"safe\""), "{}", j);
    assert!(j.contains("\"normalized_endings\":false"), "{}", j);
    // A broken config is an error, never a default-config repair list.
    let bad = edits(&data, b"[lints]\npresets = [\"nope\"]\n");
    assert!(bad.contains("\"error\""), "{}", bad);
}

/// The repair ABI gates on the configuration (#44): the same input must
/// yield different edit lists under the default config and under the
/// opt-in `hispanic-naming` preset, and `gedlint_apply` must apply the
/// config it is handed, not the default.
#[test]
fn edits_and_apply_honour_the_config() {
    // A comma-joined surname: exactly one W601 repair candidate, and no
    // other edit, so the two lists below differ only by the preset.
    let data = b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME Maria /Rovira, Font/\n0 TRLR\n";
    let hispanic = b"[lints]\npresets = [\"recommended\", \"hispanic-naming\"]\n";

    let def = edits(data, b"");
    let on = edits(data, hispanic);
    assert!(
        !def.contains("W601"),
        "the default config must not offer the opt-in repair: {}",
        def
    );
    assert!(
        on.contains("\"code\":\"W601\""),
        "the preset must reach the repair engine: {}",
        on
    );
    assert_eq!(
        on.matches("\"code\":").count(),
        def.matches("\"code\":").count() + 1
    );

    // Apply through the ABI under each config: default rewrites nothing,
    // the preset removes the comma. Same bytes, same all-ones mask shape.
    let payload = take(apply_bufs(data, &[], b""));
    let json = String::from_utf8(payload[8..].to_vec()).unwrap();
    assert!(json.contains("\"applied\":[]"), "{}", json);
    assert!(
        json.contains("Rovira, Font"),
        "the file must pass through unchanged"
    );

    let payload = take(apply_bufs(data, &[1], hispanic));
    let n = u64::from_le_bytes(payload[..8].try_into().unwrap()) as usize;
    let json = String::from_utf8(payload[8..8 + n].to_vec()).unwrap();
    let file = &payload[8 + n..];
    assert!(json.contains("\"code\":\"W601\""), "{}", json);
    assert!(
        file.windows(b"1 NAME Maria /Rovira Font/".len())
            .any(|w| w == b"1 NAME Maria /Rovira Font/"),
        "{}",
        String::from_utf8_lossy(file)
    );
    assert!(!file.windows(13).any(|w| w == b"Rovira, Font"));
}

#[test]
fn apply_returns_summary_plus_file_bytes() {
    let data = split_conc_file();
    let all = compute_edits(&normalize_endings(&data).0);
    let mask: Vec<u8> = vec![1; all.len()];
    let payload = take(apply_bufs(&data, &mask, b""));
    let n = u64::from_le_bytes(payload[..8].try_into().unwrap()) as usize;
    let json = String::from_utf8(payload[8..8 + n].to_vec()).unwrap();
    let file = &payload[8 + n..];

    assert!(json.contains("\"applied\":[{\"code\":\"E101\""), "{}", json);
    assert!(json.contains("\"postponed\":[]"), "{}", json);
    // The joined name is whole again and there is no CONC left to rejoin.
    let joined: &[u8] = b"Jos\xC3\xA9";
    assert!(file.windows(joined.len()).any(|w| w == joined));
    assert!(!file.windows(5).any(|w| w == b"CONC "));
    // `bytes` in the summary must describe the file that follows it.
    let bytes_at: usize = json.find("\"bytes\":").map(|i| i + 8).unwrap();
    let n_str: String = json[bytes_at..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    assert_eq!(n_str.parse::<usize>().unwrap(), file.len());
}

#[test]
fn apply_a_subset_and_an_empty_selection() {
    let mut data = Vec::new();
    data.extend_from_slice(b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME Jos");
    data.push(0xC3);
    data.extend_from_slice(b"\n2 CONC ");
    data.push(0xA9);
    data.extend_from_slice(
        b" /Oso/\n0 @I2@ INDI\n1 NOTE wrapped text\nlanded without a CONT\n0 TRLR\n",
    );
    let all = compute_edits(&normalize_endings(&data).0);
    assert!(all.len() >= 2, "the fixture must carry two repairs");

    // Only the E001 edit is selected.
    let mask: Vec<u8> = all
        .iter()
        .map(|e| if e.code == "E001" { 1 } else { 0 })
        .collect();
    let payload = take(apply_bufs(&data, &mask, b""));
    let n = u64::from_le_bytes(payload[..8].try_into().unwrap()) as usize;
    let json = String::from_utf8(payload[8..8 + n].to_vec()).unwrap();
    let file = &payload[8 + n..];
    assert!(json.contains("\"code\":\"E001\""), "{}", json);
    assert!(
        !json.contains("\"applied\":[{\"code\":\"E101\""),
        "{}",
        json
    );
    assert!(
        file.windows(13).any(|w| w == b"2 CONT landed"),
        "{}",
        String::from_utf8_lossy(file)
    );
    // The unselected split CONC survives untouched, bytes and all.
    let untouched = b"Jos\xC3\n2 CONC \xA9 /Oso/";
    assert!(
        file.windows(untouched.len()).any(|w| w == untouched),
        "{}",
        String::from_utf8_lossy(file)
    );

    // An empty mask changes nothing but still normalizes line endings.
    let zero = vec![0u8; all.len()];
    let payload = take(apply_bufs(&data, &zero, b""));
    let n = u64::from_le_bytes(payload[..8].try_into().unwrap()) as usize;
    let json = String::from_utf8(payload[8..8 + n].to_vec()).unwrap();
    assert!(json.contains("\"applied\":[]"), "{}", json);
    assert!(json.contains("\"postponed\":[]"), "{}", json);
}

#[test]
fn apply_reports_postponed_overlaps() {
    // A line that is both an orphan and carries trailing whitespace: two
    // edits on the same line, so the lower-priority one must be postponed
    // even when both are selected.
    let data = b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NOTE x\norphan  \n0 TRLR\n";
    let all = compute_edits(&normalize_endings(data).0);
    let both: Vec<u8> = vec![1; all.len()];
    let payload = take(apply_bufs(data, &both, b""));
    let n = u64::from_le_bytes(payload[..8].try_into().unwrap()) as usize;
    let json = String::from_utf8(payload[8..8 + n].to_vec()).unwrap();
    assert!(json.contains("\"code\":\"E001\""), "{}", json);
    assert!(
        json.contains("\"postponed\":[{\"code\":\"style\""),
        "{}",
        json
    );
}

#[test]
fn alloc_dealloc_round_trips_repeatedly() {
    for len in [1usize, 2, 16, 4096] {
        for _ in 0..3 {
            let p = gedlint_alloc(len);
            assert!(!p.is_null());
            unsafe {
                std::slice::from_raw_parts_mut(p, len).fill(0xAB);
                gedlint_dealloc(p, len);
            }
        }
    }
    // A zero-length allocation frees as a no-op.
    let p = gedlint_alloc(0);
    unsafe { gedlint_dealloc(p, 0) };
}
