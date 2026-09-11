# gedlint

GEDCOM linter in Rust: fast, single binary, streaming for large files, dual 5.5.1 (legacy, MyHeritage exports) + 7.0 (formal spec) support.

Three ways to run the same engine: the CLI, a GitHub Action, and a **[web viewer](https://pmontp19.github.io/gedlint)** that compiles the engine to WebAssembly and runs entirely in the browser. Rule explanations come from one registry, so `--explain` and the web page cannot disagree.

Full spec: pmontp19/gedcom-family-tree issue #2 (revising #1).

## Install

- Web: <https://pmontp19.github.io/gedlint> (see "Web viewer" below).
- Prebuilt binaries (linux x86_64/arm64, macOS ARM, Windows x86_64), with a `.sha256` next to each archive: GitHub Releases. Intel Macs: build from source.
- From source: `cargo install --git github.com/pmontp19/gedlint` or `cargo build --release`.

## Status

`cargo test` green across the unit tests and eight integration suites (rule fixtures, CLI, `--fix` end-to-end, config, engine fixes, baseline, golden corpora, registry), `cargo clippy --all-targets -- -D warnings` clean, `cargo fmt --check` clean, coverage above the regions floor (`cargo llvm-cov`), `cargo check --target wasm32-unknown-unknown` OK. The toolchain is pinned in `rust-toolchain.toml` so those gates give the same answer locally and in CI.

Rule set audited against the 5.5.1 and 7.0 registries, with hand-rolled mutation testing and a regression test for every survivor. Official spec corpora vendored and pinned in `tests/golden.rs`: the FamilySearch 7.0 reference files and the GEDCOM Committee TGC551 pair lint without false positives. A registry test fails the build if a rule code exists that the table does not document, or vice versa.

Validated against a real 544-individual MyHeritage tree kept outside this repo. Under the default configuration it reports 1014 diagnostics, of which 516 are `U502` vendor tags and 140 are `E005`: `CONT` lines nested under a `CONC`, which neither the previous validator nor gedlint before [#8](https://github.com/pmontp19/gedlint/issues/8) could see, because each level step is legal on its own and only the structural check catches them. Enabling both opt-in rulesets adds 146 `W601`, 49 `W602`, 15 `W701` and 14 `W702`. `--fix` under the default configuration touches none of those: it only trims trailing whitespace, because opt-in repairs are gated on configuration ([#44](https://github.com/pmontp19/gedlint/issues/44)).

## Usage

```
gedlint [--fix [--only CODE] [--unsafe]] [--config PATH | --no-config] [--baseline FILE | --write-baseline FILE] [--format text|json] [--severity error|warning|info] [--max N] [--verbose] [--no-color] [--quiet] <file.ged>
gedlint --explain [CODE]
```

Exit codes: 0 clean, 1 warnings, 2 errors.

The default text output groups diagnostics by rule code (worst severity and most occurrences first, one line per code, `N occurrences (--verbose to list all)`) and ends with a `Categories:`/`Rules:` footer; `--verbose` lists every occurrence in the classic per-line format. The same grouping lives in the engine (`Report::grouped()`) and is shared with the GitHub Action's job summary and the web viewer, so the three cannot drift.

`--explain W202` prints what the rule is about, what breaks in other genealogy programs when a file violates it and what to do instead; `--explain` alone lists every rule grouped by ruleset. A rule is addressable by code (`W202`) or by `<ruleset>/<name>` (`core/asymmetric-famc-chil`), the same two spellings the configuration accepts. Unknown rule: exit 2.

Configuration is a `gedlint.toml` looked up next to the linted file and in every parent directory: it sets presets and per-rule severities, addressed by code or `<ruleset>/<name>`. An unknown rule, preset or spelling is an error, never a silent no-op. `--config PATH` names the file explicitly; `--no-config` skips discovery and runs the built-ins only.

For adopting gedlint on a legacy tree: `--write-baseline FILE` records every current finding (keyed by rule code plus a line-independent message fingerprint, with counts) and exits 0; `--baseline FILE` then fails only on findings beyond the recorded counts. Findings fixed later show up as ratchet progress, and re-running `--write-baseline` prunes them.

`--fix` only applies safe repairs (E001 orphan lines get a CONT prefix, E005 nested CONT/CONC re-leveled to sit beside the run, E101 split CONC rejoined, trailing whitespace trimmed, classic Mac CR line endings normalized to LF) and always writes a `.bak` copy. Repairs are gated by the configuration exactly as diagnostics are: a rule that is off (or a ruleset not enabled) never edits a line, and `--only` narrows that set, never widens it. `--max N` caps the text output (rule groups in the default view, individual diagnostics under `--verbose`; JSON is always complete); `--severity` sets the minimum level shown.

Each repair is one `Edit` over a line range, carrying an `Applicability` (`Safe` or `MaybeIncorrect`), so a subset can be applied: `--only CODE` (repeatable) restricts `--fix` to those repair codes, and `--unsafe` also applies the `MaybeIncorrect` ones, which a bare `--fix` never does. Line-ending normalization is whole-file preprocessing rather than a rule, so `--only` does not switch it off.

## Web viewer

<https://pmontp19.github.io/gedlint> is the same engine compiled to `wasm32-unknown-unknown` and driven through a minimal C ABI (`src/wasm.rs`, no wasm-bindgen), running in a Web Worker so a large file never freezes the tab. It shows the health summary, a searchable findings explorer with the offending span highlighted and the registry's `title`/`why`/`remedy` on every card, per-finding interactive repair via `compute_edits`/`apply_edits` (choose the repairs, download the fixed file, see the file re-checked), and the opt-in rulesets as toggles. The page ships a Content-Security-Policy with `connect-src 'none'`: the tree is processed in the tab and the page is technically unable to send it anywhere (the `.wasm` is embedded as a same-origin script, not fetched). Static files live in `web/`, no framework and no npm; `scripts/build-web.sh` regenerates the embedded engine from `cargo build --release --target wasm32-unknown-unknown --lib` (the `--lib` matters: the bin target writes the same `.wasm` filename), and `.github/workflows/deploy-pages.yml` builds, optimizes with `wasm-opt -Oz` when available and deploys to Pages.

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
    config: gedlint.toml     # explicit config; omit for discovery, 'none' to disable
    baseline: gedlint.baseline.json   # only findings absent from it affect the outcome
    require-checksum: true   # set false only to pin a release before v0.6.0, which ship no .sha256
    version: v1        # moving major tag; pin an exact tag (v0.5.0) to freeze the binary
```

Outputs: `errors`, `warnings`, `infos`, `files`, `exit-code` (0 clean, 1 warnings, 2 errors, worst across all files).

A gate that stops running is worse than no gate, so the step fails if the action itself crashes: only a run that reaches the end can report success, and a green step always means the tree was actually linted.

```yaml
- run: echo "${{ steps.gedlint.outputs.errors }} errors in ${{ steps.gedlint.outputs.files }} files"
```

GitHub renders at most 10 annotations per severity per step; the job summary always lists every rule and count. Prebuilt runners: linux x86_64/arm64, macOS ARM, Windows x86_64. The binary is downloaded once per job, verified against the release `.sha256`, and cached in `RUNNER_TEMP`. Releases before v0.6.0 publish no checksum: pinning `version` to one of those needs `require-checksum: false`. Rendering annotations and the summary needs `node` on `PATH` (present on all GitHub-hosted runners).

## Design

- **Linter, not just a validator**: categories (correctness / suspicious / style / upgrade), configurable severities, `--fix`. clippy/eslint model.
- **Engine layout**: `src/lib.rs` is only the public API surface: re-exports plus the thin `lint_str` / `lint_bytes` / `lint_reader` entry points over the modules beside it (`diag` for the report, grouping and JSON, `parse` for the line grammar, `registry` for rule metadata, `rules` for the one streaming pass driving one module per domain, `fix` for structured safe repairs, `config` for `gedlint.toml`, `baseline` for the ratchet file). Nothing under `src/` except `main.rs` touches fs, process, env or net, so the engine compiles to WASM unchanged.
- **`src/main.rs`**: thin CLI layer (hand-rolled args, zero dependencies, manual ANSI colors).
- **`src/registry.rs`**: `RULES`, one `RuleMeta` per rule (code, name, ruleset, category, default severity, `fixable: Option<Applicability>`, title, why, remedy). Single source of truth for `--explain`, and for the config validator, the generated docs and the web viewer's finding card as they land. `tests/registry.rs` fails the build if a code the engine emits has no entry, or an entry names a code the engine never emits.
- **Version**: detected via `HEAD.GEDC.VERS`; the rule set applies per version. `U5xx` rules flag the 5.5.1 to 7.0 upgrade path (see https://gedcom.io/migrate/).
- **No global diagnostic cap**: every diagnostic is collected (a real file with 516 `_UPD` infos once hid errors behind a 200-item cap); output limiting is opt-in via `--max`.
- **Tests**: one integration suite per area, no shared state: `tests/rules.rs` (minimal fixture per rule plus version behavior), `tests/cli.rs` and `tests/cli_fix.rs` (end-to-end: formats, exit codes, `--fix`/`.bak`, `--severity`, `--max`), `tests/config.rs`, `tests/fixes.rs`, `tests/baseline.rs`, `tests/registry.rs` and `tests/golden.rs` (vendored official corpora pinned: minimal/maximal 7.0, remarriage, same-sex, escapes, TGC551 CR/LF twins; provenance in `tests/fixtures/golden/README.md`).

## Rules

The rule code's number block says what domain a rule belongs to, and the block is stable API (RFC 014):

| Block | Domain |
|---|---|
| `E0xx` | structural: levels, HEAD/TRLR placement, duplicate xrefs, required records |
| `E1xx` / `W1xx` | encoding: UTF-8, split CONC, BOM, line endings, control characters |
| `E2xx` / `W2xx` | referential: broken pointers, FAMC/CHIL symmetry |
| `W3xx` | semantic: impossible dates, duplicates, parent ages, SEX, enum values, conflicting events |
| `W4xx` | style and exporter quirks (PLAC URLs, NAME/DATE style, NOTE HTML) |
| `U5xx` | 5.5.1 to 7.0 upgrade path |

The registry (`RULES` in `src/registry.rs`) is the single source of truth for what each rule does, why it matters to consumer software and how to fix a finding; the tests keep the engine and the registry from drifting apart, and this README no longer duplicates them.

Full entry for any code, in plain language: `gedlint --explain <CODE>` (`--explain` alone lists every rule by ruleset).

## State of the art (research summary, Sep 2026)

Existing validators: Chronoplex GEDCOM Validator (Windows/.NET, closed), GED-inline (Java MIT, 5.5/5.5.1/7.0, reference + ged-inline.org), gedcomtools Python (MIT, G5+G7+GX, `validate7`), js-gedcom (full 7 validation), ArmidaleSoftware C#, go-gedcom (ABNF). Linters with `--fix`: sashaperigo/gedcom-tools (Ancestry, Python), zupulint (26 checks, privacy). Rust: pirtleshell/rust-gedcom (abandoned 2021, parse-only 5.5.1), `ged_io` crate (read/write). Gap: no Rust linter with a clippy model (categories/severities/config/--fix), streaming+WASM and dual 5/7 with genealogical semantics. That is where gedlint fits.

## Real fixtures

A private 520-person MyHeritage GEDCOM (not in this repo) with split UTF-8, PLAC URLs, SEX U, real duplicates, impossible ages, a 112-year entry. Never paste private tree data into public issues; discuss counts and rule codes only.
