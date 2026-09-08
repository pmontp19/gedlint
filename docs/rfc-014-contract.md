# RFC 014: engine contracts for rulesets, spans, structured fixes and config

Status: accepted, in implementation.
Supersedes the single-issue form of [#14](https://github.com/pmontp19/gedlint/issues/14).
Consumed by: the CLI (`src/main.rs`), the GitHub Action (`action.yml` + `scripts/gh-report.js`) and the web viewer ([#15](https://github.com/pmontp19/gedlint/issues/15)).

This document is the **binding contract**. Parallel work streams agree on the type
signatures and vocabulary here without talking to each other. Change it by editing
this file and saying so in the issue, never by drifting in an implementation.

---

## 0. Decisions that shape everything else

### 0.1 No CST, no per-rule query engine

Biome's Rowan CST and Oxlint's visitor engine exist to serve nested-syntax languages
with incremental reparse and IDE integration. GEDCOM is a line-oriented format with an
explicit level column. The single streaming pass in `lint_lines` is what makes gedlint
viable on large files, WASM-portable and dependency-free, and it is not being replaced.

What is taken from Biome: **rule metadata as data**, **rule groups**, **fix
applicability**, and **configuration by rule name**. What is not taken: the syntax tree,
the query engine, and one pass per rule.

The legitimate architecture item is that `lint_lines` is a single 1086-line function
carrying about 30 interleaved pieces of state. That is addressed by a mechanical module
split (section 6), not by a new rule abstraction.

### 0.2 Rulesets are built in and gated by config, not loadable plugins

A loadable plugin system would need dynamic linking (impossible on
`wasm32-unknown-unknown`) or an embedded scripting VM (a dependency, plus binary size,
plus a slow rule path). Both violate the repo's constraints, and in Biome and Oxlint the
plugin layer is the newest and least stable surface.

Everything the plugin idea was wanted for (opt-in rules, off by default, grouped and
addressable) is delivered by **built-in rulesets gated by configuration**, at a small
fraction of the cost.

Forward compatibility is kept cheaply: every rule declares a `ruleset` string and config
addresses rules as `<ruleset>/<rule-name>`. If external rules ever ship, they slot into
the same namespace with no config migration.

### 0.3 GEDCOM has no comment syntax, so config is the only suppression mechanism

There is no `// gedlint-ignore` and there never can be: the format has no comments and
an injected pseudo-comment line would be a grammar error. This is a real divergence from
every linter gedlint is modelled on, and it raises the priority of two things:

- `gedlint.toml` is the only way a user can silence a rule (section 4).
- A **baseline file** is the only way a user can adopt gedlint on a legacy tree without
  either fixing 1000 findings or muting whole rules (section 5).

### 0.4 Two orthogonal axes, both preserved

| Axis | Values | Meaning | Stability |
|---|---|---|---|
| `category` | `correctness`, `suspicious`, `style`, `upgrade` | Why the rule exists (intent) | **Public API.** Serialized in JSON, consumed by `scripts/gh-report.js:108,143,205`. Do not extend or rename. |
| `ruleset` | `core`, `hygiene`, `hispanic-naming`, ... | What domain the rule belongs to and whether it is on by default | New, additive |

RFC #14 proposed `cultural`, `hygiene` and `graph` as categories. They are domains, not
intents, and `category` is already the intent axis and already public. They become
rulesets instead.

### 0.5 Rule code namespace

The letter tracks severity, the number block tracks domain. Existing blocks:

| Block | Domain |
|---|---|
| `E0xx` | structural |
| `E1xx` / `W1xx` | encoding |
| `E2xx` / `W2xx` | referential |
| `W3xx` | semantic |
| `W4xx` | style and vendor quirks |
| `U5xx` | 5.5.1 to 7.0 upgrade path |

New blocks, chosen so that the code itself signals an opt-in ruleset:

| Block | Ruleset | Default |
|---|---|---|
| `W6xx` | `hispanic-naming` (and any future regional ruleset) | **off** |
| `W7xx` | `hygiene` | **off** |

RFC #14's proposed `W405`-`W410` are therefore renumbered into `W6xx` / `W7xx`. Nothing
opt-in lands inside the `W4xx` block, so a reader never has to look up whether a `W4xx`
code is normative.

---

## 1. Diagnostic type

`Diag` gains two additive fields. Both default so that existing construction sites are
untouched and rules adopt spans one at a time.

```rust
pub struct Diag {
    pub code: &'static str,
    pub category: Category,       // unchanged, public API
    pub ruleset: &'static str,    // NEW: "core" for every existing rule
    pub severity: Severity,
    pub line: usize,              // 1-based, 0 = whole file
    pub col: u32,                 // NEW: 0-based BYTE offset into the raw line
    pub len: u32,                 // NEW: span length in BYTES; 0 = no span, whole line
    pub msg: String,
}
```

**Offsets are byte offsets into the raw line, not char or UTF-16 offsets.** Bytes are
the natural and zero-cost unit in Rust, and they are unambiguous. The JS side slices the
line's bytes at `col` and `col + len` and runs each of the three pieces through
`TextDecoder`, which is both correct and fast. Do not convert to char offsets in Rust.

**The normative base is the on-disk line**, that is: split the file on line terminators
(`CR`, `LF` or `CRLF`), drop the terminator, and count bytes from the start of what is
left. Nothing is stripped first. In particular the UTF-8 BOM that GEDCOM 7 recommends is
the first three bytes of line 1, even though the engine skips it before parsing, so a
span on line 1 of a BOM'd file starts at 3 or more. This is a single base for every rule:
a consumer slices the line it read from the file without having to know which rule
produced the diagnostic, and the byte-level rules (`E101`) and the line-level rules
(`E004`, `W401`) cannot disagree about where a line begins.

This is the same decision section 3 makes for `Edit.replacement`, and it is the same
reason: a span has to address lines that are not valid UTF-8, since `E101` fires exactly
when an exporter cut a character in two, and a char offset has nothing to count there.
The two sections agree by construction and must stay that way. Spans address bytes,
repairs carry bytes, and both number lines identically, 1-based after line-ending
normalization, so a `Diag.line` and the `Edit.lines` range that repairs it land on the
same rows. A reader arriving at either section should find the other agreeing.

Constructors:

```rust
impl Diag {
    // existing signature, unchanged: sets ruleset = "core", col = 0, len = 0
    fn new(code, category, severity, line, msg) -> Diag;
    // new
    fn with_span(code, category, severity, line, col, len, msg) -> Diag;
    fn in_ruleset(self, ruleset: &'static str) -> Diag;
}
```

JSON gains `"ruleset"`, `"col"` and `"len"` on every diagnostic. Additive only: no
existing key changes name, type or meaning, so `scripts/gh-report.js` keeps working
unmodified.

---

## 2. Rule registry

A single static table is the source of truth for the CLI's `--explain`, the config
validator, generated documentation and the web viewer's finding card. It is the keystone
of this RFC: it lands before anything that consumes it.

```rust
pub struct RuleMeta {
    pub code: &'static str,          // "W202"
    pub name: &'static str,          // "asymmetric-famc-chil"
    pub ruleset: &'static str,       // "core"
    pub category: Category,
    pub default_severity: Severity,
    pub default_enabled: bool,       // false for every non-core ruleset
    pub fixable: Option<Applicability>, // None = no automatic repair
    pub title: &'static str,         // one line, imperative
    pub why: &'static str,           // what breaks in consumer software, concretely
    pub remedy: &'static str,        // what the user should do instead
}

pub const RULES: &[RuleMeta] = &[ /* every rule, sorted by code */ ];

pub fn rule(code: &str) -> Option<&'static RuleMeta>;
pub fn rule_by_name(ruleset: &str, name: &str) -> Option<&'static RuleMeta>;
```

`why` and `remedy` are the user-facing payload. Write them for a genealogist who has
never heard the word "linter": name the software that breaks and how the breakage looks
("Gramps drops the second surname on import"), not the clause of the specification.

A test asserts that every code emitted anywhere in the engine exists in `RULES`, and
that `RULES` has no entry that the engine never emits.

---

## 3. Structured fixes

`fix_bytes` is currently all or nothing. The web viewer needs per-finding selection and
the CLI wants `--fix --only <CODE>`, so repairs become data.

```rust
pub enum Applicability {
    /// Provably meaning-preserving. Applied by a bare `--fix`.
    Safe,
    /// Probably right, needs human eyes. Never applied without opt-in.
    MaybeIncorrect,
}

pub struct Edit {
    pub code: &'static str,
    /// Inclusive 1-based line range this edit replaces.
    pub lines: (usize, usize),
    /// Replacement lines, without terminators. Empty vec = delete the range.
    pub replacement: Vec<Vec<u8>>,
    pub applicability: Applicability,
    /// One line for the UI: "rejoin CONC split inside a UTF-8 sequence".
    pub note: String,
}

/// Which repairs a caller wants. `Default` is the `--fix` contract.
pub struct FixSelection {
    pub only: Vec<String>,   // rule codes, ASCII case-insensitive; empty = all
    pub allow_unsafe: bool,  // also apply MaybeIncorrect
}

/// Compute every candidate repair without applying any. Returned in repair
/// priority order (see below), not line order.
pub fn compute_edits(data: &[u8]) -> Vec<Edit>;

/// Apply a chosen subset. Edits must not overlap; overlapping ranges are
/// resolved by keeping the first and dropping the rest, and the dropped ones
/// are returned so the caller can re-run.
pub fn apply_edits(data: &[u8], edits: &[Edit]) -> (Vec<u8>, Vec<Edit>);

/// Whole-file preprocessing (below), and `fix_bytes` with a selection.
pub fn normalize_endings(data: &[u8]) -> (Cow<'_, [u8]>, bool);
pub fn fix_bytes_with(data: &[u8], sel: &FixSelection) -> (Vec<u8>, Vec<String>);
```

**`replacement` is bytes, not `String`.** Amended from `Vec<String>` during
implementation, for the same reason `col`/`len` in section 1 are byte offsets: the E101
repair rejoins a UTF-8 sequence the exporter cut in two, so the input lines are by
definition not valid UTF-8, and neither is the joined result when the split is
pathological (`Jos<C3>` + `2 CONC <A9><A9> x`). `Vec<String>` cannot hold those bytes,
and a lossy conversion would corrupt exactly the files the repair exists for. Rules that
work on text build one with `line.into_bytes()`.

`compute_edits` returns edits in **repair priority order** (`E001`, then `style`, then
`E101`), because `apply_edits` keeps the first of two overlapping edits and the pass
that runs first decides what a later pass on the same lines reads. `E001` precedes
`E101` because the `CONT` prefix is what the rejoin reads; `style` precedes `E101`
because trailing whitespace must be gone before the rejoin absorbs the line, or a pad
lands between the two halves of the cut UTF-8 sequence and the repaired file is still
invalid (#36). A dropped edit still reserves its
lines, so a lower-priority edit cannot slip inside the range of a repair that is merely
postponed. Trailing whitespace has no rule code (the linter does not report it), so its
edits carry the pseudo-code `style`, which is what the `--fix` report line has always
printed.

A line-range model, not an intra-line span model, because the existing repairs are not
all intra-line: rejoining a split `CONC` replaces two lines with one, and prefixing an
orphan line with `CONT` rewrites one line in place. Line ranges express both, plus pure
deletion.

Line-ending normalization (classic Mac `CR`) stays a whole-file preprocessing step with
its own flag. It is not a per-rule edit and must not be modelled as one. It is also not
selectable: `compute_edits` and `apply_edits` both split lines on `\n`, so every repair
assumes it has run.

`fix_bytes` survives unchanged as a thin wrapper, so `tests/cli.rs` and the `--fix`
contract in `README.md` keep holding:

```rust
pub fn fix_bytes(data: &[u8]) -> (Vec<u8>, Vec<String>) {
    // normalize line endings, then apply every Safe edit
}
```

It applies **one pass per rule code, in priority order**, and deliberately not a
fixpoint. One pass per code because two edits on one line (an orphan that also has
trailing whitespace) overlap by construction, so a single `apply_edits` call cannot land
both. Not a fixpoint because re-running a code repairs things `--fix` never reported:
rejoining a `CONC` can turn a blank line into a levelless one, and a second `E001` pass
would prefix it with `CONT`, inventing a record the linter never asked about. The next
`--fix` picks up whatever the previous one exposed. Adding a repair therefore means
naming its code in the order list, which a debug assertion enforces.

**`--fix` policy is unchanged and non-negotiable** (`AGENTS.md`): only safe, invertible
repairs, always with a `.bak`, never a semantic change. Concretely, for the new rulesets
this means no automatic deletion of `_MARNM`, and no automatic case conversion of names.

---

## 4. Configuration

```toml
# gedlint.toml
[lints]
presets = ["recommended", "hispanic-naming"]

[lints.rules]
"hispanic-naming/no-comma-in-surname" = "error"
"hygiene/polluted-name" = "warn"
"U502" = "off"
```

- A rule is addressable **either** by `<ruleset>/<name>` **or** by bare code. Both
  resolve through the registry; an unknown key is a hard configuration error, not a
  silent no-op.
- Values: `"off" | "info" | "warn" | "error"`.
- Presets: `recommended` (every `core` rule at its default severity) plus one name per
  opt-in ruleset. Explicit `[lints.rules]` entries win over presets.
- Parsing is a hand-rolled minimal TOML subset in `src/config.rs`. Sections, string
  values, and string arrays only. No dependency.

Layering:

```rust
// pure, lives in the engine, no fs
pub fn parse_config(text: &str) -> Result<Config, ConfigError>;
pub fn lint_str_with(input: &str, cfg: &Config) -> Report;
pub fn lint_bytes_with(data: &[u8], cfg: &Config) -> Report;
```

`lint_str` / `lint_bytes` / `lint_reader` keep their signatures and behave as
`*_with(Config::default())`.

File discovery (walking up from the linted file's directory to find `gedlint.toml`,
plus a `--config <path>` override and a `--no-config` escape hatch) lives in
`src/main.rs`, because `src/lib.rs` must stay free of fs.

The WASM entry point takes the config as a second buffer of TOML text, so the browser
can offer the same presets without a second config implementation in JavaScript.

---

## 5. Baseline

The reported motivating case is 1036 diagnostics on a real tree, with `W202` and `W302`
buried under 516 `U502` and 171 `E001`. Grouping the output makes that readable;
it does not make the file adoptable in CI, because the job still fails on day one.

```
gedlint --baseline gedlint.baseline.json tree.ged   # fail only on NEW findings
gedlint --write-baseline gedlint.baseline.json tree.ged
```

Baseline entries are keyed by `(code, a normalized fingerprint of the message)` and a
count, **never by line number**, so that inserting a line at the top of the file does not
invalidate the whole baseline. A finding matches the baseline while the count for its key
has not been exhausted.

This is the mechanism that lets a genealogist adopt gedlint on an existing tree and
ratchet down, which no amount of output formatting achieves.

---

## 6. Module split (foundation)

`src/lib.rs` is 2074 lines and every work stream below would edit it, which serializes
everything. It is split first, mechanically, with **zero behaviour change**:

```
src/lib.rs      public API surface and re-exports
src/diag.rs     Severity, Category, Diag, Report, JSON serialization
src/parse.rs    Line, parse_line, newline normalization, head scan
src/registry.rs RuleMeta table and lookups
src/rules/      one module per domain, matching the code blocks
src/fix.rs      Applicability, Edit, compute_edits, apply_edits, fix_bytes
src/config.rs   Config, parse_config
```

Acceptance for the split: the full test suite passes unchanged, `cargo clippy
--all-targets` is clean, coverage does not drop, and the public API is byte-identical.
No rule logic is edited in that change.

---

## 7. Output ergonomics

`scripts/gh-report.js:161-175` already groups diagnostics by rule code for the Action's
job summary. That logic moves into the engine as `Report::grouped()` so the CLI, the
Action and the web viewer share one implementation and cannot drift.

Default text output: sort by severity descending, then by count descending, then by code;
collapse runs of one code into a single line with a count, and list every occurrence
under `--verbose`. `--max` and `--severity` keep their current meaning.

---

## 8. Rulesets shipped by this RFC

Both are `default_enabled: false`. Enabling either is an explicit choice, because both
encode conventions rather than the specification.

### `hispanic-naming` (`W6xx`)

| Code | Name | Severity | Fix |
|---|---|---|---|
| `W601` | `no-comma-in-surname` | warn | `Safe`, only for the exact two-token shape `/A, B/` |
| `W602` | `no-married-name` | warn | **none** |
| `W603` | `no-abbreviated-given-name` | info | none |

`W602` has no automatic repair. Deleting `_MARNM` destroys data that is frequently the
only record of a married name, and `AGENTS.md` forbids semantic repairs. The rule warns
and explains; the user decides.

`W601`'s repair only fires when the surname field is exactly two tokens separated by a
comma. Anything more elaborate is reported and left alone.

### `hygiene` (`W7xx`)

| Code | Name | Severity | Fix |
|---|---|---|---|
| `W701` | `polluted-name` | info | none |
| `W702` | `all-caps-name` | info | `MaybeIncorrect`, opt-in only |
| `W703` | `malformed-place` | info | `Safe` for doubled commas only |

`W702` is deliberately `info` and deliberately off. Fully capitalized surnames are a
long-standing genealogical convention rather than a defect, and title casing is lossy
for `MCDONALD`, `O'BRIEN`, `DE LA O` and most Iberian particles. It is never applied by
a bare `--fix`.

`W701` is heuristic (parentheses, asterisks, digits, ordinal markers inside a name
field) and there is no regex engine available, so it stays at `info` with a conservative
matcher. False positives are worse than misses here: a parenthetical can be a legitimate
house name.

---

## 9. What the web viewer needs from the engine

Recorded here so [#15](https://github.com/pmontp19/gedlint/issues/15) is not blocked by
a late engine change:

1. `Edit` + `Applicability` + `apply_edits` (section 3), for per-finding repair selection.
2. `col` / `len` on `Diag` (section 1), to highlight the offending span rather than the
   whole line.
3. `RuleMeta.title` / `why` / `remedy` (section 2), which is exactly the finding card.
4. A WASM entry point that accepts a config buffer (section 4).

Plus two delivery constraints that belong with the contract:

- The WASM module runs in a **Web Worker**. A large GEDCOM must never block the main
  thread.
- The page ships a Content Security Policy with `connect-src 'none'`. It turns the
  privacy claim into something a user can verify in devtools in five seconds, which is
  the strongest argument the project has for an audience that will not read the source.
