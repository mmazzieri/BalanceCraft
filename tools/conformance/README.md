# Conformance vectors from Cerqua

`fixtures/conformance/*.jsonl` hold inputs and outputs of Cerqua's pure functions (`computeShares`, `computeFlatList`, `mergeAppData`, `cleanupTombstones`, `reorderSiblings`, `migrateLegacyData`), recorded while Cerqua's own test suite runs. BalanceCraft's tests replay them: a Rust function must give the same result as Cerqua's.

Each line is `{"fn", "test", "tz", "args", "result"}`: `test` is the Cerqua test that made the call, `tz` its time zone (`""` is UTC). The clock is in `args` (`nowMs`, `nowDT`). `_savedAt` is removed from merge results, because Cerqua fills it with `Date.now()`.

## Regenerating them

Cerqua is closed source, so this needs a Cerqua checkout (version noted below) with `npm ci` done in `mobile/`:

1. Copy `record.ts` to `mobile/conformance.record.ts`, and write `mobile/conformance.vitest.config.ts` with the same aliases as `mobile/vitest.config.ts`, `setupFiles: ['./conformance.record.ts']`, and `include` set to `src/lib/algorithm.test.ts`, `src/lib/merge.test.ts`, `src/lib/meta.test.ts`, `src/store/useStore.sync.test.ts`, `src/store/useStore.test.ts`.
2. `CONFORMANCE_OUT=/tmp/out.jsonl npx vitest run --config conformance.vitest.config.ts`, then delete the two files.
3. Deduplicate by function and arguments. Keep every merge from named tests and 200 of the randomized property runs (seed 20261123).

The paths in `record.ts` point to `/home/user/cerqua`: change them to your checkout.

Current vectors: Cerqua 1.46.0 (commit of 23/11/2026), 313 tests, recorded on 23/11/2026.

The vectors are synthetic test data written by Cerqua's author and released with BalanceCraft under MIT OR Apache-2.0.
