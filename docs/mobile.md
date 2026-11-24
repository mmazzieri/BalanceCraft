# The phone apps

`mobile/android` (Kotlin, Jetpack Compose) and `mobile/ios` (SwiftUI) are thin apps on the desktop's own commands, file format and merge. Their first version opens or makes a document, shows Today and the tree of areas, and adds, edits, completes, puts on hold and deletes tasks. Not yet: history, area log, prerequisites, moving tasks, settings, undoing a completion on iOS.

A phone reaches the shared document only through the system, so it keeps a **mirror** of it in the app's own storage. The engine works on the mirror with a replica, as on a computer, and the phone carries the mirror to the document: it reads the document again just before writing, and merges first if it changed. A change is in the replica before anything is written, so it survives a document that cannot be reached. Conflict copies that a sync app makes are merged by the computers.

- **Android**: the folder of the document is chosen with the system's picker (the access to a single file is lost when a sync app replaces it), and the document is written in place, without an atomic rename. What is being written is kept until it is read back; a document left damaged by a write that stopped is written again.
- **iOS**: reads and writes go through file coordination, and the document is replaced atomically.

## The bridge

`crates/ffi` builds a shared library for Android (`Core.call` through JNI) and a static one for iOS (`balancecraft_call`, `balancecraft_free`). One request in JSON, one answer: `{"ok": …}` or `{"error": "…"}`.

| `op` | Fields | Answers |
|---|---|---|
| `check` | `text` | `{document}`, or an error when the text is not a document |
| `new` | `mirror`, `state` | makes an empty document |
| `run` | `mirror`, `state`, `command`: one of `balancecraft_commands::Command` | the command's answer |

## Building

Android, with the SDK and NDK: `rustup target add aarch64-linux-android x86_64-linux-android`, `cargo install cargo-ndk`, then `cd mobile/android && ./gradlew assembleDebug`.

iOS, on a Mac with Xcode: `rustup target add aarch64-apple-ios aarch64-apple-ios-sim`, `brew install xcodegen`, then `cd mobile/ios && xcodegen generate && open BalanceCraft.xcodeproj`.

The `Mobile` workflow builds both; the Android build is kept as an artifact.
