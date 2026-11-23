# Attribution

## Provenance of the design

The data model, the priority algorithm (area and branch shares, deficit, score) and the field-level last-writer-wins merge are ported from Cerqua, a closed-source app whose sole copyright holder, Mauro Mazzieri, releases them here under MIT OR Apache-2.0. Cerqua's design started from the public idea of balancing weighted areas of life; no internal knowledge, code, text or assets of any other product were used.

## Test vectors

`fixtures/conformance/` is generated from Cerqua's test suite by Cerqua's author, as `tools/conformance/README.md` describes, and released here under MIT OR Apache-2.0.

## Third-party material

Rust crates are listed with their licenses by `cargo deny list`; `cargo deny check licenses` keeps them on the allowlist in `deny.toml`. Direct dependencies:

| Crate | Version | License |
|---|---|---|
| egui, eframe (and egui-wgpu, egui-winit) | 0.37 | MIT OR Apache-2.0 |
| rfd | 0.17.2 | MIT |
| serde, serde_json | 1 | MIT OR Apache-2.0 |
| chrono | 0.4 | MIT OR Apache-2.0 |
| chrono-tz (tests only) | 0.10 | MIT OR Apache-2.0 |
| uuid | 1 | Apache-2.0 OR MIT |
| sha2 | 0.10 | MIT OR Apache-2.0 |
| thiserror | 2 | MIT OR Apache-2.0 |
| tempfile (tests only) | 3 | MIT OR Apache-2.0 |

Fonts bundled in the app through `epaint_default_fonts` 0.37 (egui's `default_fonts` feature):

| Font | License |
|---|---|
| Ubuntu Light | Ubuntu Font Licence 1.0 (`Ubuntu-font-1.0`) |
| Hack Regular | MIT, with Bitstream Vera parts |
| Noto Emoji Regular | SIL Open Font License 1.1 (`OFL-1.1`) |
| emoji-icon-font | MIT |
| egui-icons (subset of emoji-icon-font) | MIT |

The license texts ship with the crate (`fonts/UFL.txt`, `fonts/OFL.txt`, `fonts/Hack-Regular.txt`, `fonts/emoji-icon-font-mit-license.txt`) and go in the release packages with this file.
