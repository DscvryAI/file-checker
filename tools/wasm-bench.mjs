// WebAssembly build under Node (V8): same answers as native? And how fast, in memory?
// usage: node tools/wasm-bench.mjs <files.txt> <native.jsonl> <out.jsonl> [reps]
import { readFileSync, writeFileSync } from 'node:fs';
import { initSync, check_json } from '../wasm-pkg/file_checker_wasm.js';

initSync({ module: readFileSync(new URL('../wasm-pkg/file_checker_wasm_bg.wasm', import.meta.url)) });
const [list, nativePath, out, repsArg] = process.argv.slice(2);
const reps = Number(repsArg ?? 9);
const native = new Map(readFileSync(nativePath, 'utf8').split('\n').filter(Boolean)
  .map((l) => JSON.parse(l)).map((r) => [r.file, r]));
const rows = [];
let differ = 0;
for (const p of readFileSync(list, 'utf8').split('\n').filter(Boolean)) {
  const name = p.split('/').pop();
  const bytes = readFileSync(p);
  const times = [];
  let r;
  for (let i = 0; i < reps; i++) {
    const t = performance.now();
    r = JSON.parse(check_json(bytes, name));
    times.push(performance.now() - t);
  }
  times.sort((a, b) => a - b);
  const n = native.get(name);
  if (!n || n.label !== r.label || n.flags.join() !== r.flags.join()) {
    differ++;
    console.log('differs', name, n?.label, n?.flags, r.label, r.flags);
  }
  rows.push({ file: name, label: r.label, flags: r.flags, us: +(1000 * times[times.length >> 1]).toFixed(2) });
}
writeFileSync(out, rows.map((r) => JSON.stringify(r)).join('\n') + '\n');
const us = rows.map((r) => r.us).sort((a, b) => a - b);
const q = (f) => us[Math.floor(f * (us.length - 1))];
console.log(JSON.stringify({ files: rows.length, differ_from_native: differ, median_us: q(0.5), p95_us: q(0.95), max_us: us.at(-1) }));
