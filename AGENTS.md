# AGENTS.md: working rules for this repo (loaded every session, keep small)

## Non-obvious facts

- Working language is English (code, messages, docs). The owner's family research (fixtures) is Catalan; test *data* may be Catalan, test *names/comments* must not.
- Private real fixtures live OUTSIDE this repo (machine-local Orca checkout): `/Users/pere/orca/workspaces/Genealogia-Montpeo/gedlint/arbre/Montpeo_arbre_netejat.ged` (520 INDI, MyHeritage) and `arbre/originals/original_myheritage_20260905.ged`. Never copy tree data into this repo or public issues; discuss counts and rule codes only.
- Spec source of truth was pmontp19/gedcom-family-tree#2 (closed on MVP). Both spec audits (5.5.1, 7.0) are done; findings not yet implemented are roadmap, not bugs.
- `cargo-llvm-cov` is installed; `cargo-mutants` is not (offline at the time). Mutation testing was manual in a /tmp copy; survivors all have regression tests.

## Constraints (not derivable from code)

- Zero dependencies (measured base speed matters; WASM reuse matters). No clap/serde/wasm-bindgen.
- `src/lib.rs` must stay free of fs/process/env/net so it compiles to `wasm32-unknown-unknown` unchanged. All I/O lives in `src/main.rs`.
- `--fix` only gets safe, invertible repairs, always with `.bak`. Never "fix" semantics (dates, links, merges).
- No global diagnostic cap (a real file has 516 `_UPD` infos); limit output only via `--max`.
- This repo is private. Commit and push after every work unit.

## Quality gates (run all three)

- `cargo test` (gates: rule fixtures in tests/rules.rs, CLI in tests/cli.rs)
- `cargo clippy --all-targets` (must be clean)
- `cargo llvm-cov --summary-only --all-targets` (floor: 84% regions total; raise it when you add code)

## Deferred roadmap (audited, consciously postponed)

- Conflicting duplicate events (two BIRT blocks with different DATEs): suspicious, needs per-record event comparison.
- EVEN payload validation and full LDS coverage beyond STAT.
