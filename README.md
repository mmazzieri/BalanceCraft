# BalanceCraft

BalanceCraft keeps your life in balance, one task at a time. You split your activities into weighted areas (work, family, health…) and branches, and BalanceCraft builds today's list: tasks from the areas that got less attention than their weight come first, then the bigger tasks and the closer deadlines.

It is a local desktop app written in Rust. Everything you enter lives in one file, `*.balancecraft`, which you can keep in a folder synced by OneDrive, Google Drive, Dropbox or Syncthing and open from several computers. When two computers edit the same file, BalanceCraft merges the changes field by field instead of overwriting them.

> **Status:** early development. Nothing is usable yet: the plan is in [docs/plan.md](docs/plan.md) (in Italian).

## Relation with Cerqua

BalanceCraft is the local, open-source sibling of [Cerqua](https://www.mauromazzieri.it/cerqua), a mobile and web app with cloud sync by the same author. The data model, the priority algorithm and the merge come from Cerqua; the code is a new implementation in Rust. Cerqua can import a `.balancecraft` file. The two apps share nothing else: no account, no server.

## File format

A `.balancecraft` file is UTF-8 JSON with a versioned envelope:

```json
{
  "format": "balancecraft",
  "formatVersion": 1,
  "generator": "BalanceCraft 0.1.0",
  "documentId": "…",
  "savedAt": "2026-11-23T21:30:00Z",
  "data": { "areas": [], "history": [], "areaActions": [], "settings": {} },
  "extensions": {}
}
```

The JSON Schema will be published in [`schema/`](schema/), with example files in [`fixtures/`](fixtures/).

## Build from source

You need a recent stable Rust (1.90 or newer).

```sh
cargo run --release -p balancecraft   # the desktop app
cargo xtask ci                         # fmt, clippy and tests
```

## License

BalanceCraft is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
Copyright (c) 2026 Mauro Mazzieri and the BalanceCraft contributors.

Unless you explicitly state otherwise, any contribution you intentionally submit for inclusion in BalanceCraft, as defined in the Apache-2.0 license, is dual licensed as above, without any additional terms or conditions.
