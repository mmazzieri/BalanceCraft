# BalanceCraft

BalanceCraft keeps your life in balance, one task at a time. You split your activities into weighted areas (work, family, health…) and branches, and BalanceCraft builds today's list: tasks from the areas that got less attention than their weight come first, then the bigger tasks and the closer deadlines.

It is a local desktop app written in Rust. Everything you enter lives in one file, `*.balancecraft`, which you can keep in a folder synced by OneDrive, Google Drive, Dropbox or Syncthing and open from several computers. When two computers edit the same file, BalanceCraft merges the changes field by field instead of overwriting them.

> **Status:** 0.2, in development. Today, Tree, Areas, Waiting and History work, and every release comes with installers for Windows, macOS and Linux. The plan is in [docs/plan.md](docs/plan.md) (in Italian).

## Install

Each [release](https://github.com/mmazzieri/BalanceCraft/releases) has these files, listed with their checksums in `SHA256SUMS.txt`:

| System | File |
|---|---|
| Windows 10 and 11, x64 | `balancecraft-<version>-windows-x64.msi` (installer) or `-windows-x64-portable.zip` |
| macOS 11 or newer, Intel and Apple silicon | `balancecraft-<version>-macos-universal.dmg` |
| Debian, Ubuntu and derivatives | `balancecraft-<version>-linux-<arch>.deb` |
| Fedora, openSUSE, RHEL | `balancecraft-<version>-linux-<arch>.rpm` |
| Any Linux with glibc 2.35 or newer | `balancecraft-<version>-linux-<arch>.tar.gz` |

`<arch>` is `x86_64` or `aarch64`. The MSI installs for the current user or for every user of the computer, adds BalanceCraft to the Start menu and opens `.balancecraft` files with a double click; so do the Linux packages, through the MIME type `application/vnd.balancecraft+json`. The Windows files are not signed yet, so SmartScreen may ask for a confirmation the first time.

On macOS, drag BalanceCraft to Applications. Until a release is signed with a Developer ID and notarized, macOS blocks it the first time because it cannot check it. On macOS 15 or newer, open it once, close the warning, then go to **System Settings › Privacy & Security** and choose **Open Anyway** next to BalanceCraft; on older versions, right-click it, choose **Open** and confirm. From Terminal, `xattr -dr com.apple.quarantine /Applications/BalanceCraft.app` does the same. Double-clicking a `.balancecraft` file in Finder does not open it yet; use **File › Open…** or the recent files.

## Using it

- **File › New document…** creates a `.balancecraft` file with five default areas. Put it in a synced folder to use it from several computers; **File › Open…** opens it on the others. A file name on the command line (`balancecraft path/to/file.balancecraft`) opens that file.
- **Today** lists the tasks to do now, best first, with the deadline light (red: overdue or due today; yellow: within the warning, in Italian working days; green: later). Each task can be marked done (with undo), put on hold, edited or given a subtask.
- **Tree** shows areas and branches. The `…` menu of each row adds, edits, moves (**Move to…** or up and down), completes, puts on hold and deletes. A task can also be dragged above or below another task, or onto an area name to put it at the top of that area; areas are reordered by dragging their names.
- **Areas** lists the areas, the most neglected first: days since the last action, status, last action and next action. Clicking an area opens its page, where the status, the next action (optionally linked to a task) and the log of actions are written and edited.
- **Waiting** lists the tasks on hold with the reason, to resume or complete them. **History** lists the completed tasks, newest first; a completion can be undone from there.
- The task form has the **prerequisites**: tasks that must be done first. Until they are, the task stays out of Today.
- **Settings** has the language (Italian or English) and the default deadline warning.
- **File › Import Cerqua backup…** turns a JSON backup exported from Cerqua into a new `.balancecraft` file.

Edits are saved on this computer at once and written to the shared file a second later. The app checks the shared file every few seconds and merges what other computers wrote. Copies made by the sync client after a conflict (`name-PC.balancecraft`, `name (1).balancecraft`) are merged and moved to `.balancecraft-merged/`; copies made by hand are listed in the status bar and left alone.

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
packaging/linux/package.sh            # Linux packages in dist/release (needs nfpm for deb and rpm)
```

The packages for all three systems are built by `.github/workflows/release.yml`: on every pull request that touches `packaging/`, and on a `v*` tag, which also creates a draft release with the files and `SHA256SUMS.txt`.

## License

BalanceCraft is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
Copyright (c) 2026 Mauro Mazzieri and the BalanceCraft contributors.

Unless you explicitly state otherwise, any contribution you intentionally submit for inclusion in BalanceCraft, as defined in the Apache-2.0 license, is dual licensed as above, without any additional terms or conditions.
