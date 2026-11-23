// Records inputs and outputs of Cerqua's pure functions while Cerqua's own tests run.
import { vi, expect } from 'vitest';
import fs from 'node:fs';



function rec(fn: string, args: unknown[], result: unknown) {
  let test = '';
  try { test = expect.getState().currentTestName ?? ''; } catch { /* outside a test */ }
  fs.appendFileSync(process.env.CONFORMANCE_OUT!, JSON.stringify({ fn, test, tz: process.env.TZ ?? '', args, result }) + '\n');
}
const clone = <T,>(v: T): T => (v === undefined ? v : JSON.parse(JSON.stringify(v)));

vi.mock("/home/user/cerqua/mobile/src/lib/algorithm.ts", async (orig) => {
  const m: any = await orig();
  const withClock = (name: string, f: any) => (areas: any, history: any, clock?: any) => {
    const c = clock ?? m.localClock();
    const a = [clone(areas), clone(history), { nowDT: c.nowDT, nowMs: c.nowMs }];
    const r = f(areas, history, c);
    const out = name === 'computeShares' ? { byId: Object.fromEntries(r.byId), totalSpent: r.totalSpent } : r;
    rec(name, a, clone(out));
    return r;
  };
  return { ...m, computeFlatList: withClock('computeFlatList', m.computeFlatList), computeShares: withClock('computeShares', m.computeShares) };
});

vi.mock("/home/user/cerqua/mobile/src/lib/merge.ts", async (orig) => {
  const m: any = await orig();
  return { ...m, mergeAppData: (l: any, r: any, ...rest: any[]) => {
    const a = [clone(l), clone(r), ...rest.map(clone)];
    const res = m.mergeAppData(l, r, ...rest);
    rec('mergeAppData', a, clone(res));
    return res;
  } };
});

vi.mock("/home/user/cerqua/mobile/src/lib/meta.ts", async (orig) => {
  const m: any = await orig();
  const wrap = (name: string) => (...args: any[]) => {
    if (name === 'cleanupTombstones' && args.length < 2) args = [args[0], Date.now()];
    const a = args.map(clone);
    const res = m[name](...args);
    rec(name, a, clone(res));
    return res;
  };
  return { ...m, cleanupTombstones: wrap('cleanupTombstones'), reorderSiblings: wrap('reorderSiblings'), migrateLegacyData: wrap('migrateLegacyData') };
});
