# AGENTS.md

Notes for people and coding agents working on BalanceCraft.

- Plan and decisions: [docs/plan.md](docs/plan.md) (Italian). Read it before changing the file format, the merge or the sync behaviour.
- Layout: `crates/model` (serde types, pure), `crates/engine` (algorithm, store commands, merge; pure, no I/O, clock passed in, must build for wasm32), `crates/file` (file format and synced-folder storage), `crates/ui-egui` (interface), `apps/balancecraft` (desktop shell), `xtask/`.
- Checks: `cargo xtask ci` (fmt, clippy with `-D warnings`, tests). `unsafe` is forbidden; `unwrap`, `expect` and `panic` are denied outside tests.
- Licenses: dependencies and assets only under the license allowlist of the Craft apps (MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0, BSL-1.0, 0BSD, Unlicense, CC0-1.0, OFL-1.1, …), checked by `cargo deny check`. No GPL, LGPL or AGPL code, not even read. Record every addition in `ATTRIBUTION.md`.
- Clean room: no code, text, icons or assets from proprietary products. Do not name other products in the interface.
- File format: never remove or rename a field of format v1. Unknown fields must survive a read-write cycle and take part in the merge (see the plan).
- Conformance: `fixtures/conformance/` holds vectors generated from Cerqua. A change that makes them fail is a behaviour change and needs a decision, not a fixture update.
