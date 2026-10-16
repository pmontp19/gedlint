# Product Marketing Context

*Last updated: 2026-10-16*

## Product Overview
**One-liner:** gedlint is a GEDCOM linter in Rust: fast, single binary, dual 5.5.1 + 7.0 support, with a GitHub Action and an in-browser web viewer.
**What it does:** Reads a GEDCOM export (the interchange format genealogy software uses), finds structural, encoding, referential and semantic problems that make the file break or lose data in other programs, explains each finding in plain language, and safely repairs the structural ones. The same engine ships as a CLI, a GitHub Action and a browser-based viewer compiled to WebAssembly.
**Product category:** Developer/linter tool for genealogists ("the clippy of GEDCOM"); sits on the shelf next to GEDCOM validators.
**Product type:** Open-source (MIT), free, zero-dependency Rust project. No pricing, no accounts, no telemetry.
**Business model:** None — community-driven open source. "Payment" is feedback, bug reports and trees from other exporters.
**Origin story (owner-confirmed 2026-10-16, use in community posts):** The author exported the family tree from MyHeritage as GEDCOM and wanted to move it elsewhere without losing names, notes or dates. Existing validators buried real errors under noise (516 identical vendor-tag infos against a 200-item cap) or could not see structural damage at all (`CONT` nested under `CONC` is legal per line, broken as a whole); the rest were closed-source, abandoned or Java-only. So the author built the linter he wanted: Rust, single binary, clippy model — categories, severities, config, safe `--fix`, baseline ratchet. Same engine as CLI, GitHub Action and an in-browser viewer that cannot upload the file. It is also his **first Rust project** (feedback on idioms is welcome), and it was born partly for **agentic genealogy** (term not yet established, introduced naturally): the author researches his family tree with AI agents, and an agent that edits a GEDCOM needs guardrails — the tree lives in git, the GitHub Action lints every push, and the committed baseline means only *new* findings fail the build, so an agent must prove its edits did not regress anything before the diff gets reviewed. Proof it works: official FamilySearch 7.0 corpora and the GEDCOM Committee's TGC551 pair lint without false positives; the real 544-person tree produced 1014 findings, 140 of them structural breakage no earlier validator could see. The ask: run your tree (any exporter) in the browser, zero install, and report rule codes and counts — never data.

## Target Audience
**Target companies:** Not companies — individuals and communities: family historians, genealogy hobbyists and professionals, maintainers of genealogy software, and people who keep their tree files in git.
**Decision-makers:** The tree owner themselves. Secondary: genealogy software maintainers who want clean imports.
**Primary use case:** "I exported my tree (MyHeritage, Ancestry, Gramps, Legacy, RootsMagic, FamilySearch) and I want to bring it somewhere else / share it / archive it without names, notes or dates silently disappearing."
**Jobs to be done:**
- Before importing an export into new software: find what is broken so nothing is silently dropped or garbled.
- Before sharing a GEDCOM with family or uploading to a service: check it is well-formed and portable.
- While cleaning up a legacy tree over months: ratchet progress without re-flagging old findings (baseline).
- When AI agents edit or import into the tree file: hard guardrails (Action + committed baseline) so the agent cannot silently drift or damage data — new findings fail the build.
**Use cases:**
- One-off check of an export in the browser, nothing installed, nothing uploaded.
- Recurring lint of tree files kept in a git repository via the GitHub Action.
- Batch clean-up of a large legacy tree with `--fix` and a committed baseline.

## Personas
(B2C/community project — no B2B buying committee.)
| Persona | Cares about | Challenge | Value we promise |
|---------|-------------|-----------|------------------|
| Family historian (main) | Their data surviving the trip between programs; privacy of living people | Validators drown them in noise or miss structural breakage | Plain-language findings (`--explain`), privacy-by-design viewer, safe repairs with `.bak` |
| Genealogy software maintainer | Clean input files; spec conformance | Users import malformed GEDCOMs and file bugs against them | Formal 7.0 spec support, official corpora pinned, exit codes for scripting |
| Rust/dev hobbyist | Tool quality, CI integration | No maintained Rust GEDCOM tooling (last one abandoned 2021) | Zero-dependency single binary, GitHub Action, public registry tests |
| AI-assisted genealogist (agent operator) | Agents that don't corrupt decades of research | Autonomous edits to a GEDCOM are hard to review line by line | Action + baseline ratchet: new findings fail CI, so the agent proves non-regression |

## Problems & Pain Points
**Core problem:** GEDCOM exports are full of quiet breakage — split UTF-8, `CONT` nested under `CONC`, broken pointers, impossible dates — that other genealogy programs silently mangle or drop, and existing validators either bury real errors under noise or cannot see structural damage at all.
**Why alternatives fall short:**
- Chronoplex GEDCOM Validator: closed source, Windows/.NET only.
- GED-inline: solid reference but Java, one-off style, no categories/severities/fix/baseline.
- sashaperigo/gedcom-tools, zupulint: Python, narrow (Ancestry-specific / 26 fixed checks).
- pirtleshell/rust-gedcom: abandoned 2021, parse-only.
- Validator output caps: a real file with 516 identical `_UPD` infos hid errors behind a 200-item cap — real damage became invisible.
- No tool checked structure across lines (e.g. `CONT` under `CONC` is legal per line, broken as a whole).
**What it costs them:** Lost or garbled names, notes, places and dates when switching software; hours of manual diffing; distrust of their own archive.
**Emotional tension:** "Is my family's research safe? Did I just lose grandma's notes by moving software?" Genealogists guard decades of work and the privacy of living relatives.

## Competitive Landscape
**Direct:** GED-inline, Chronoplex Validator, gedcomtools/validate7 — fall short on the clippy model (categories, severities, config, `--fix`, baseline), streaming performance, privacy-first web viewer, and maintained Rust.
**Secondary:** Editor-side validation (Domorium for VS Code/JetBrains/Obsidian/Web) — tied to an editor/product ecosystem, not a standalone linter with exit codes and CI use.
**Indirect:** "My genealogy software validates on import" — importers accept silently, repair nothing, and differ per program; gedlint checks against the spec, not one program's tolerance.
**Gap we fill:** No Rust linter with a clippy model, streaming + WASM, and dual 5.5.1/7.0 support with genealogical semantics. That is where gedlint fits (README, state of the art, Sep 2026).

## Differentiation
**Key differentiators:**
- Same engine, three surfaces: CLI, GitHub Action (inline annotations + job summary), browser viewer (WASM, worker, no framework).
- Privacy by construction: viewer ships CSP with `connect-src 'none'`; baselines carry digests, never tree content; the project refuses tree data in issues.
- One registry (`src/registry.rs`) feeds `--explain`, config validation, generated docs and the web cards — they cannot disagree, like ShellCheck's wiki.
- No global diagnostic cap (opt-in `--max` only), so noise never hides errors again.
- Ratchet baseline for adopting the linter on legacy trees; findings beyond baseline fail, fixed ones show as progress.
- Extensible by convention: opt-in rulesets encode conventions, not the spec — `hispanic-naming` (W6xx) is the proven template (146 W601 / 49 W602 on the real tree) and the pattern for future regional rulesets.
- Dual spec: 5.5.1 (vendor reality) + 7.0 (formal spec), rule set audited against both registries; official corpora vendored and pinned.
**How we do it differently:** clippy/eslint model applied to genealogy data; safe-only, config-gated repairs with `.bak`, never touching semantics.
**Why that's better:** Trust. The tool is verifiable (tests, pinned corpora, mutation-tested rules), local, and explains itself.
**Why customers choose us:** It found what their previous validator missed (e.g. 140 `E005` nested-`CONT` breakages invisible line by line), and it never asks them to upload their family.

## Objections
| Objection | Response |
|-----------|----------|
| "GED-inline already validates." | Different model: categories/severities/config, `--fix` with `.bak`, baseline ratchet, GitHub Action, privacy viewer — and one registry so docs cannot drift from the engine. |
| "Will it mangle my file?" | `--fix` only applies safe, invertible, config-gated repairs and always writes a `.bak`; it never edits dates, links or merges — by rule, not by promise. |
| "My tree is private." | Nothing leaves your machine: CLI is local; the viewer cannot network (`connect-src 'none'`); baselines contain digests only. Bug reports ask for counts and rule codes, never data. |
| "My file is huge." | Streaming single pass, designed for large files; no memory blowups; tested with real 500+ person trees and official maximal corpora. |
**Anti-persona:** People wanting automatic semantic conversion (5.5.1→7.0 migration, merging duplicates, rewriting links) — deliberately out of scope; `U5xx` flags the path but does not perform it. Developers wanting a plugin runtime (deliberately none, RFC 014 §0.2).

## Switching Dynamics
**Push:** Exports that break silently in the next program; validators that cap output or emit noise walls; abandoned tools; closed Windows-only options.
**Pull:** In-browser zero-trust checking, plain-language explanations, safe fixes, CI gate for git-hosted trees.
**Habit:** "My software validates on import"; eyeballing the file; one-off GED-inline runs when something already broke.
**Anxiety:** False positives on a 10,000-person tree; a fixer corrupting the file; a hobby project that disappears. Countered by: pinned official corpora without false positives, `.bak` always, MIT + public history (15 releases).

## Customer Language
**How they describe the problem:**
- "MyHeritage export breaks when I import it into [other program]" (observed pattern; collect verbatims from first community posts)
- "Notes appear cut / names truncated after moving my tree"
- "The validator says OK but the file is broken"
**How they describe us:** (to be collected — none yet)
**Words to use:** export, linter, findings, broken, silently, safe repair, baseline, in your browser, nothing uploaded, plain language, single binary.
**Words to avoid:** "scanner", "cleaner" (overpromises), "AI", "migration/converter" (explicitly out of scope), marketing superlatives.
**Glossary:**
| Term | Meaning |
|------|---------|
| GEDCOM | Genealogical Data Communication: the interchange format between genealogy software (5.5.1 legacy, 7.0 current) |
| Ruleset | core / consistency / hygiene / upgrade — configuration groups of rules |
| Baseline | Committed file recording current findings so only new ones fail the build (ratchet) |
| `U5xx` | Rule block for the 5.5.1→7.0 upgrade path |
| xref | Cross-reference identifier linking records (pointers) |

## Brand Voice
**Tone:** Honest, plain, technical where it earns it; privacy-first; zero hype. "Show, don't promise": counts, corpora, tests.
**Style:** Short declarative sentences; code-font flags (`--fix`, `--explain`); the README's own voice is the brand voice.
**Personality:** Careful, transparent, meticulous, quietly proud, useful.

## Proof Points
**Metrics:** `cargo test` green across unit + 8 integration suites; clippy `-D warnings` clean; coverage floor enforced in CI; `wasm32` build checked in CI; pinned toolchain for reproducible gates.
**Customers:** No logos (B2C). Real-world validation: a 544-individual MyHeritage tree (kept private) — 1014 findings under default config, incl. 516 vendor-tag infos and 140 `E005` structural breakage the previous validator missed; `--fix` touches none of it unless configured (gated repairs).
**Testimonials:** None yet — this is what the community outreach should generate.
> "" — [who]
**Value themes:**
| Theme | Proof |
|-------|-------|
| Finds what others miss | Official-validator comparison closed gaps (E201/E010/E009/W306/W404/W405); nested-`CONT` detection (#8); registry test keeps engine and docs identical |
| No false positives on spec files | FamilySearch 7.0 reference files + GEDCOM Committee TGC551 pair vendored, pinned, lint clean |
| Privacy by construction | CSP `connect-src 'none'`; digest-only baselines; no-tree-data policy |
| Engineered, not vibes | Zero dependencies; mutation testing with regression test per survivor; streaming single pass |

## Goals
**Business goal:** Adoption by real genealogists with real trees from other exporters, and feedback that hardens the rule set beyond the MyHeritage context it was born in. Owner-confirmed (2026-10-16): the aim is to make gedlint useful for *everyone*, not only MyHeritage/Hispanic trees.
**Extensibility goal:** Show, and discover, how far the ruleset model reaches. `hispanic-naming` started as the author's own annoyance and became an opt-in ruleset; outreach should surface which other regional/cultural conventions deserve the same treatment (invite users to describe their region's naming/placement conventions — rule codes and conventions, never tree data).
**Conversion action:** Open https://pmontp19.github.io/gedlint, drop a `.ged`, read the findings — zero install. (Developers: `cargo install gedlint` or the Action.)
**Current metrics:** 0 stars (metadata just added), 15 releases since Aug 2026, crate published to crates.io (v0.11.0), awesome-gedcom PR open. Baseline for measuring outreach.
