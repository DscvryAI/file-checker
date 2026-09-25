# file-checker: plan

**Question:** what is this upload really, and can a document pipeline take it?

It works from the file's structure, not the extension and not a model. It reads the first bytes, and, when the format needs it, the zip central directory or the OLE directory. It never decompresses or parses content.

## Fewest moving parts

- **The classifier:** one Rust crate with **zero dependencies**, a library and a CLI.
  - OOXML is told apart by the entry names in the zip central directory (`word/`, `xl/`, `ppt/`), which needs no inflate.
  - ODF and EPUB are told apart by their stored `mimetype` entry.
  - For OLE, the classifier walks the directory, which is a few sectors.
- **No Worker, no page, no wasm** until the numbers say the experiment is worth it.
- **Competitors,** each run in its own best batch mode:
  - **libmagic** (`file` 5.45, in Git Bash), with `--mime-type` and the description.
  - **Magika** 1.0.3 (Google's deep-learning detector), via its Python API, in a repo-local venv.
  - **file-type** 22.1.1 (npm), via a Node script.
  - **The extension.** Trust the filename; this is the baseline everyone is implicitly using.

## Label set (what the pipeline cares about)

| Group | Labels |
|---|---|
| PDF | `pdf` |
| OOXML | `docx`, `xlsx`, `pptx` (macro-enabled variants noted as a flag) |
| Legacy Office / OLE | `doc`, `xls`, `ppt`, `msg` |
| Encrypted Office | `office-encrypted`: a password-protected OOXML, which is an OLE file holding an `EncryptedPackage` |
| Other documents | `rtf`, `odt`, `ods`, `odp`, `epub` |
| Images | `jpeg`, `png`, `gif`, `tiff`, `bmp`, `webp` |
| Archives | `zip` (plain), `gzip`, `7z`, `rar` |
| Text-ish | `html`, `xml`, `json`, `eml`, `csv`, `text` |
| Otherwise | `unknown` |

Flags, reported separately from the label:
- `truncated`, which catches common cases like a missing `%%EOF` or a missing end-of-central-directory record;
- `extension-mismatch`;
- `encrypted`.

Every competitor's output is mapped to this label set by a fixed, published table. Anything it names outside the set maps to `other`.

## Data

1. **Real:** govdocs1 thread 001, about 1,000 files crawled from US government sites. Public domain, and not redistributed.
2. **Constructed** (labels known by construction), for the cases govdocs1 lacks:
   - encrypted OOXML (msoffcrypto);
   - freshly written docx, xlsx and pptx (python-docx, openpyxl, python-pptx);
   - renamed files with lying extensions;
   - truncated files;
   - ContractNLI PDFs, which are already on disk.

**Ground truth:** labels come from **a parser opening the file**, not from the extension:
- PyMuPDF for PDF;
- python-docx, openpyxl and python-pptx for OOXML;
- olefile streams for doc, xls, ppt, msg and encrypted Office (`WordDocument`, `Workbook`/`Book`, `PowerPoint Document`, `__substg1.0_*`, `EncryptedPackage`);
- Pillow for images;
- zipfile, gzip and tarfile for archives;
- `json.loads` for JSON;
- the XML parser for XML.

A file no parser opens is labelled `unverified`, and it is scored separately.

## What we can prove

- **Accuracy on parser-verified types:** ours against libmagic, Magika, file-type and the extension, on the same files.
- **Specific failure modes:** encrypted OOXML, the docx/xlsx/pptx split, doc/xls/ppt/msg, lying extensions and truncation, each with its own count.
- **Speed and footprint:**
  - Per-file time, in each tool's batch mode. Cross-language timing is indicative, not a race; this is stated on the page.
  - Bytes read per file.
  - Binary size, measured against Magika's model and runtime.

## What we can't prove (stated on the page)

- **The text-ish classes:** `csv` against `text`, and `html` against `text`, have no parser-level truth. We report agreement between tools there, not accuracy. Magika is expected to be stronger on these.
- **The real upload mix.** govdocs1 skews old: heavy on doc, xls, ppt and pdf from the 2000s, and light on modern OOXML.
- **Safety.** A sniffer is not a validator. Opening cleanly in one Python library is not "the pipeline can process it". Polyglots and adversarial files are out of scope.
- **Magika's full range.** It knows 200+ types; we compete only on this label set.

## Chunks

1. This plan, plus the environment check. (Done.)
2. Data: fetch govdocs1 001, build the constructed set, run the ground-truth labeller, and write the label counts.
3. The classifier crate and CLI; run it; iterate against the labels.
4. Competitor runners plus mapping tables; run them; score (`tools/evaluate.py`).
5. README with results, versions, limits. Then decide: page, wasm demo, or stop.
