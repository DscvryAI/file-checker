// Run file-type over the file list; write jsonl in the evaluate.py format.
// file-type detects binary signatures only (by design it does not classify text formats).
// usage: node tools/node/filetype.mjs <files.txt> <out.jsonl>
import { readFileSync, writeFileSync } from 'node:fs';
import { fileTypeFromFile } from 'file-type';

const MAP = {
  pdf: 'pdf', docx: 'docx', docm: 'docx', xlsx: 'xlsx', xlsm: 'xlsx', pptx: 'pptx', pptm: 'pptx',
  rtf: 'rtf', odt: 'odt', ods: 'ods', odp: 'odp', epub: 'epub',
  jpg: 'jpeg', png: 'png', apng: 'png', gif: 'gif', tif: 'tiff', bmp: 'bmp', webp: 'webp',
  zip: 'zip', gz: 'gzip', '7z': '7z', rar: 'rar', xml: 'xml',
};

const [list, out] = process.argv.slice(2);
const paths = readFileSync(list, 'utf8').split(/\r?\n/).filter(Boolean);
const run = async () => {
  const rows = [];
  const t = performance.now();
  for (const p of paths) {
    const r = await fileTypeFromFile(p);
    rows.push({ file: p.split('/').pop(), label: MAP[r?.ext] ?? 'unknown', raw: r?.ext ?? null });
  }
  return [rows, (performance.now() - t) / 1000];
};
await run();
const [rows, secs] = await run();
writeFileSync(out, rows.map((r) => JSON.stringify(r)).join('\n') + '\n');
const version = JSON.parse(readFileSync(new URL('./node_modules/file-type/package.json', import.meta.url))).version;
console.log('file-type', JSON.stringify({ version, files: rows.length, batch_s: +secs.toFixed(3),
  per_file_ms: +(1000 * secs / rows.length).toFixed(3) }));
