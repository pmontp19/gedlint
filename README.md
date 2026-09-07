# gedlint

GEDCOM linter in Rust: fast, single binary, streaming for large files, dual 5.5.1 (legacy, MyHeritage exports) + 7.0 (formal spec) support. Compilable to WASM to reuse the engine in a web viewer.

Full spec: pmontp19/gedcom-family-tree issue #2 (revising #1).

## Install

- Prebuilt binaries (linux x86_64/arm64, macOS ARM, Windows x86_64), with a `.sha256` next to each archive: GitHub Releases. Intel Macs: build from source.
- From source: `cargo install --git github.com/pmontp19/gedlint` or `cargo build --release`.

## Status

Working MVP: `cargo test` (97 tests: 2 unit + 79 rule + 10 CLI + 6 golden), `cargo clippy` clean, `cargo llvm-cov` 91.4% regions (lib 92.4%, main 79.4%), release validated at 4.5MB in 0.13s, `cargo check --target wasm32-unknown-unknown` OK. Prebuilt binaries (linux/macOS/Windows) attached to releases. Validated against a real 520-person MyHeritage tree (found 175 strict-grammar errors the previous validator missed: HTML continuations without CONT, plus encoding quirks). Rule set audited against the 5.5.1 and 7.0 specs (E007/E008/E009/W306 from the registries); two rounds of hand-rolled mutation testing (15/19), all survivors covered with regression tests. External dataset audit (issue #9): official spec corpora vendored and pinned in `tests/golden.rs` — the FamilySearch 7.0 reference files and the GEDCOM Committee TGC551 pair lint without false positives.

## Usage

```
gedlint [--fix] [--format text|json] [--severity error|warning|info] [--max N] [--no-color] [--quiet] <file.ged>
```

Exit codes: 0 clean, 1 warnings, 2 errors.

`--fix` only applies safe repairs (E001 orphan lines get a CONT prefix, E101 split CONC rejoined, trailing whitespace trimmed, classic Mac CR line endings normalized to LF) and always writes a `.bak` copy. `--max N` caps text output (JSON is always complete); `--severity` sets the minimum level shown.

## GitHub Action

```yaml
- uses: pmontp19/gedlint@v1
  with:
    path: tree.ged
```

Findings land as inline annotations on the diff, plus a job summary grouped by rule code. Full set of inputs:

```yaml
- uses: pmontp19/gedlint@v1
  id: gedlint
  with:
    path: |            # one path or glob per line; every pattern must match
      trees/*.ged
      archive/legacy.ged
    fail-on: error     # or 'warning'
    format: text       # log rendering; 'json' prints the raw report instead
    annotations: true  # inline file annotations
    max-annotations: 50   # worst first; '0' lifts the cap
    summary: true      # job summary grouped by rule code
    require-checksum: true   # set false only to pin a release before v0.6.0, which ship no .sha256
    version: v1        # moving major tag; pin an exact tag (v0.5.0) to freeze the binary
```

Outputs: `errors`, `warnings`, `infos`, `files`, `exit-code` (0 clean, 1 warnings, 2 errors, worst across all files).

```yaml
- run: echo "${{ steps.gedlint.outputs.errors }} errors in ${{ steps.gedlint.outputs.files }} files"
```

GitHub renders at most 10 annotations per severity per step; the job summary always lists every rule and count. Prebuilt runners: linux x86_64/arm64, macOS ARM, Windows x86_64. The binary is downloaded once per job, verified against the release `.sha256`, and cached in `RUNNER_TEMP`. Releases before v0.6.0 publish no checksum: pinning `version` to one of those needs `require-checksum: false`. Rendering annotations and the summary needs `node` on `PATH` (present on all GitHub-hosted runners).

## Design

- **Linter, not just a validator**: categories (correctness / suspicious / style / upgrade), configurable severities, `--fix`. clippy/eslint model.
- **`src/lib.rs`**: pure engine (`lint_str`, `lint_bytes`, `lint_reader`, `fix_bytes`, `Report::to_json`). No fs or process usage: compiles to WASM unchanged. Streaming line-by-line parsing (`BufRead`).
- **`src/main.rs`**: thin CLI layer (hand-rolled args, zero dependencies, manual ANSI colors).
- **Version**: detected via `HEAD.GEDC.VERS`; the rule set applies per version. `U5xx` rules flag the 5.5.1 to 7.0 upgrade path (see https://gedcom.io/migrate/).
- **No global diagnostic cap**: every diagnostic is collected (a real file with 516 `_UPD` infos once hid errors behind a 200-item cap); output limiting is opt-in via `--max`.
- **Tests**: `tests/rules.rs` (one minimal fixture per rule plus an own 7.0 fixture), `tests/cli.rs` (end-to-end: formats, exit codes, `--fix`/`.bak`, `--severity`, `--max`) and `tests/golden.rs` (vendored official corpora pinned: minimal/maximal 7.0, remarriage, same-sex, escapes, TGC551 CR/LF twins; provenance in `tests/fixtures/golden/README.md`).

## Rules

Structural: E001 level (levels stop at 99, so a text line starting with a year is an orphan, not a jump; blank lines are ignored), E002 HEAD/TRLR (incl. HEAD-first, nothing after TRLR), E003 duplicate xref, E004 malformed xref, E005 orphan CONT/CONC, E007 CONC in 7.0 (reserved tag, spec 1.3), E008 duplicate singleton (SEX/HUSB/WIFE/GEDC, VERS scoped by its HEAD parent so GEDC.VERS and SOUR.VERS coexist, one detail substructure per event block), E009 missing required (HEAD.GEDC, GEDC.VERS).
Encoding: E101 invalid UTF-8 / split CONC (MyHeritage bug) with `--fix` (skipped for declared ANSEL/ASCII), W102 BOM (silent in 7.0, which recommends it) / mixed line endings / control chars. Bare CR (classic Mac, TGC551) is a legal terminator: parsed like LF, normalized by `--fix`.
Referential: E201 broken refs incl. level-2+ pointers (@VOID@ exempt), W202 FAMC/CHIL mismatch.
Semantic: W301 death before birth / longevity >105 (DEAT Y without a date is excluded), W302 duplicates (name + birth ±2 years), W303 parent age, W304 child before marriage, W305 SEX (X valid only in 7.0), W306 enum values (ROLE/PEDI/QUAY/RESN/FAMC-STAT/NAME-TYPE/MEDI/LDS-STAT/DATA-EVEN from the registries; 7.0 exact, 5.5.1 case-insensitive; RESN and DATA.EVEN accept comma-separated lists; HEAD.CHAR validated; FILE.FORM media type; OTHER wants a PHRASE beside or under the value), W307 duplicate single-instance events with conflicting dates (repeatable OCCU/RESI/CENS exempt; a MARR/DIV pair separated by the counterpart is a serial marriage, not a conflict).
Style/MyHeritage quirks: W401 URL inside PLAC, W402 non-standard NAME/DATE (NAME slash balance across CONC continuations; BET without AND and range order; parens; calendar escapes; one-sided FROM/TO periods are valid), W403 NOTE with HTML.
Upgrade: U501 RELA/PEDI/BET/CHAR (7.0 changes, incl. out-of-order ranges), U502 vendor tags `_MARNM`/`_UPD`/Ancestry (kept as undocumented extensions, SCHMA recommended). E009 also covers EVEN/FACT without TYPE and LDS STAT without DATE (7.0 only).

## State of the art (research summary, Sep 2026)

Existing validators: Chronoplex GEDCOM Validator (Windows/.NET, closed), GED-inline (Java MIT, 5.5/5.5.1/7.0, reference + ged-inline.org), gedcomtools Python (MIT, G5+G7+GX, `validate7`), js-gedcom (full 7 validation), ArmidaleSoftware C#, go-gedcom (ABNF). Linters with `--fix`: sashaperigo/gedcom-tools (Ancestry, Python), zupulint (26 checks, privacy). Rust: pirtleshell/rust-gedcom (abandoned 2021, parse-only 5.5.1), `ged_io` crate (read/write). Gap: no Rust linter with a clippy model (categories/severities/config/--fix), streaming+WASM and dual 5/7 with genealogical semantics. That is where gedlint fits.

## Real fixtures

A private 520-person MyHeritage GEDCOM (not in this repo) with split UTF-8, PLAC URLs, SEX U, real duplicates, impossible ages, a 112-year entry. Never paste private tree data into public issues; discuss counts and rule codes only.
