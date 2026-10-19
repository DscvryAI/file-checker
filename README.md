# file-checker

What is this upload really, and can a document pipeline take it?

This is a structural file-type check in Rust with no dependencies. It reads the first 8 KB and, when the format needs it, the zip central directory or the OLE directory. It never decompresses or parses content. Along with the label, it reports three flags: `encrypted`, `truncated` and `extension-mismatch`.

The plan and the label set are in [PLAN.md](PLAN.md). The write-up is at https://dscvryai.com/insights/what-is-this-file/.

## Layout

- `classifier/`: the library (`src/lib.rs`) and the CLI (`src/main.rs`).
- `wasm/`: the browser build, a separate crate so the classifier keeps zero dependencies and the frozen `lib.rs` is untouched. It's one call, `check_json(bytes, name)`. The output in `wasm-pkg/` is 39 KB, 18 KB gzipped.
  - `tools/wasm-bench.mjs` confirms it gives the same answer as native on all 3,765 files, at a median of 4.7–8.6 µs in Node.
  - `tools/samples.py` makes the four made-up demo files in `samples/`.
- `tools/construct.py`: the constructed set. Its labels are known by construction, and its names are neutral (`c0001.ext`).
- `tools/label.py`: ground truth by parsing. A file's label is whatever a real parser can open it as (olefile; python-docx, openpyxl and python-pptx; PyMuPDF and Pillow; gzip, json and xml). The extension is never an input.
- `tools/competitors.py`: libmagic and Magika. `tools/node/filetype.mjs`: file-type. Each tool's mapping into the label set is a fixed table in its runner.
- `tools/evaluate.py`: scoring. A file outside the label set counts as `unknown` on both sides.

## Data

The rules were tuned on the dev set: govdocs1 thread 001 (976 files, 564 of them parser-verified) and 270 constructed files.

The held-out set was only run after the rules were frozen, and `results/frozen-lib.sha256` is checked before the run. It's govdocs1 thread 002 (990 files, 518 parser-verified) and Apache POI's `test-data` at commit 942d95d (1,529 files, 1,399 parser-verified). The POI files are real-world, bug-report and fuzzer-minimised Office, OLE and msg files.

govdocs1 is public domain, and POI's test data is Apache-2.0. Neither is redistributed.

## Results

Run on 2026-09-25 on a Windows laptop. Held-out, routing correct on parser-verified files:

| | govdocs1 002 | Apache POI | Total |
|---|---|---|---|
| file-checker | 517 / 518 | 1,397 / 1,399 | 1,914 / 1,917 (99.8%) |
| trust the extension | 516 / 518 | 1,358 / 1,399 | 1,874 / 1,917 (97.8%) |
| Magika 1.0.3 | 515 / 518 | 1,356 / 1,399 | 1,871 / 1,917 (97.6%) |
| libmagic (`file` 5.45) | 512 / 518 | 1,345 / 1,399 | 1,857 / 1,917 (96.9%) |
| file-type 22.1.1 | 370 / 518 | 633 / 1,399 | 1,003 / 1,917 (52.3%) |

On dev, which the rules were tuned on (so only for completeness): file-checker 759/759, libmagic 755, Magika 724, file-type 524, extension 707.

### The differences

Encrypted OOXML (47 files: 30 constructed, 17 in POI). A password-protected .docx is an OLE file holding an `EncryptedPackage`. file-checker got 47/47 and libmagic 46/47. Magika got none: it calls them `thumbsdb` (25), `doc` (16), `xls` (2) or unknown (4). file-type calls them all a generic `cfb`, and the extension says docx, xlsx or pptx for files that won't open without the password.

Legacy Office and Outlook (doc, xls, ppt, msg) are all `cfb` to file-type, so it misses 885 held-out files.

file-checker flagged every lying extension, 20/20 on dev and 9/9 held-out, with no false alarms.

For truncation it caught 15/15 on dev with no false alarms. On the held-out set it flagged 35 files: 31 are POI fuzzer or crash-test files (not checked one by one), and I checked the other 4 by hand. They're all broken: three zips with no end record, which Python's `zipfile` refuses, and one PDF that PyMuPDF had to repair. A truncated file's label still names what it was meant to be.

Three PDFs in govdocs1 001 have bytes before `%PDF`. Two are MacBinary-wrapped (a 128-byte classic Mac header carrying the filename) and one starts with a stray CRLF; readers accept up to 1 KB of leading bytes. file-type misses all 3, Magika the 2 MacBinary files and libmagic the CRLF one.

file-checker's three held-out misses are left in. Two are fuzzer-minimised OLE files it calls ppt and xls, where olefile finds no Office stream, and one is a `.txt` that Pillow reads as BUFR, a meteorological binary format, which it calls text.

### Speed and size

| | Per file (batch, warm cache) | Footprint |
|---|---|---|
| file-checker (native CLI, from disk) | 0.04–0.09 ms; in-memory median 1.4–2.6 µs | 219 KB executable, no dependencies |
| file-type (Node) | 0.57–0.72 ms | 580 KB node_modules |
| Magika (Python API) | 9.5–19.5 ms, after a 0.1 s model load | 3.2 MB model + 46 MB onnxruntime |
| libmagic (`file -f`, Git for Windows) | 15–36 ms | the magic database |

The timings cross languages and runtimes, so treat them as orders of magnitude; they also drift about two times between runs on this laptop. The check reads a median of 8–9 KB per file, 12–56 KB at p95 and 182 KB at most. In total that's 1.8% (dev) and 6.4% (held-out) of the bytes on disk.

## Limits

The held-out accuracy comes from two independent sources, and the lead is in the cases a pipeline pays for: encrypted Office, the OLE formats, and renamed or truncated files. On ordinary files, trusting the extension scores almost the same (97.8%).

Text formats aren't proven. CSV against text, and HTML against text, have no parser truth. The constructed text files are easy (file-checker 60/60, libmagic 54, Magika 52), and the real ones are reported only as agreement between tools. Magika is built for these, and for source code, which is outside this label set. It knows 200+ types, and this compares only the 30 labels here.

The corpora aren't a modern upload mix either: govdocs1 is from the 2000s, and POI is weighted to odd files.

And a sniffer isn't a validator. Opening cleanly in one Python library doesn't mean the pipeline can process a file, and polyglot and adversarial files are out of scope.

## Iterations

All on dev, before the freeze.

1. The first run got 758/759. Its misses and false alarms led to three rule fixes. An encrypted .docx is correctly named, so it isn't a mismatch. Without a central directory, the OOXML part folder counts as evidence (openpyxl writes `[Content_Types].xml` last). And `<title>`/`<body>` alone don't make markup HTML, as a TREC SGML document showed.
2. 759/759, and then the rules were frozen.
3. Harness fixes, none in the classifier. The file list was written with CRLF, which made libmagic fail every file. And the labeller used to call any zip the Python libraries refuse a plain `zip`; an Office package they can't open (docm, dotx, xlsb, xps) is now `unverified`. This was fixed before any tool's held-out output was looked at.

## Commands

```
python tools/construct.py <contract-nli raw dir> data/constructed
python tools/label.py data/govdocs/001 data/labels-govdocs.json
python tools/label.py data/constructed data/labels-constructed.json data/constructed/manifest.json
python tools/label.py data/heldout/flat data/labels-heldout.json
cargo build --release --manifest-path classifier/Cargo.toml
classifier/target/release/file-checker --bench data/files.txt results/ours.jsonl 9
.venv/Scripts/python tools/competitors.py data/files.txt results
node tools/node/filetype.mjs data/files.txt results/filetype.jsonl
python tools/evaluate.py ours results/ours.jsonl [--heldout] [--verbose]
(cd wasm && wasm-pack build --release --target web --out-dir ../wasm-pkg)
node tools/wasm-bench.mjs data/files.txt results/ours.jsonl results/wasm.jsonl 9
python tools/samples.py samples
```
