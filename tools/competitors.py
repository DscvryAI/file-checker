"""Run libmagic and Magika over the file list; write jsonl in the evaluate.py format.

Each tool runs in its own best batch mode, twice; the second (warm-cache) run is timed.
Mapping tables below are the whole of the translation into our label set: anything a tool
names outside them becomes "unknown".

usage: .venv/Scripts/python tools/competitors.py <files.txt> <outdir>
"""
import json
import os
import subprocess
import sys
import time

FILE_EXE = r"C:\Program Files\Git\usr\bin\file.exe"

LIBMAGIC = {
    "application/pdf": "pdf",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document": "docx",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet": "xlsx",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation": "pptx",
    "application/msword": "doc",
    "application/vnd.ms-excel": "xls",
    "application/vnd.ms-powerpoint": "ppt",
    "application/vnd.ms-outlook": "msg",
    "application/encrypted": "office-encrypted",
    "text/rtf": "rtf",
    "application/vnd.oasis.opendocument.text": "odt",
    "application/vnd.oasis.opendocument.spreadsheet": "ods",
    "application/vnd.oasis.opendocument.presentation": "odp",
    "application/epub+zip": "epub",
    "image/jpeg": "jpeg", "image/png": "png", "image/gif": "gif", "image/tiff": "tiff",
    "image/bmp": "bmp", "image/x-ms-bmp": "bmp", "image/webp": "webp",
    "application/zip": "zip", "application/gzip": "gzip", "application/x-gzip": "gzip",
    "application/x-7z-compressed": "7z", "application/x-rar": "rar", "application/vnd.rar": "rar",
    "text/html": "html", "text/xml": "xml", "application/xml": "xml", "application/json": "json",
    "message/rfc822": "eml", "text/csv": "csv", "text/plain": "text",
}

MAGIKA = {
    "pdf": "pdf", "docx": "docx", "xlsx": "xlsx", "xlsb": "xlsx", "pptx": "pptx",
    "doc": "doc", "xls": "xls", "ppt": "ppt", "outlook": "msg", "rtf": "rtf",
    "odt": "odt", "ods": "ods", "odp": "odp", "epub": "epub",
    "jpeg": "jpeg", "png": "png", "gif": "gif", "tiff": "tiff", "bmp": "bmp", "webp": "webp",
    "zip": "zip", "gzip": "gzip", "sevenzip": "7z", "rar": "rar",
    "html": "html", "xml": "xml", "json": "json", "eml": "eml", "csv": "csv", "tsv": "csv",
    "txt": "text",
}


def name_of(p):
    return p.replace("\\", "/").rsplit("/", 1)[-1]


def libmagic(paths, listfile):
    def run():
        t = time.perf_counter()
        out = subprocess.run([FILE_EXE, "-b", "--mime-type", "-f", listfile],
                             capture_output=True, text=True, check=True).stdout.splitlines()
        return out, time.perf_counter() - t
    run()
    out, secs = run()
    assert len(out) == len(paths), (len(out), len(paths))
    rows = [{"file": name_of(p), "label": LIBMAGIC.get(m.strip(), "unknown"), "raw": m.strip()}
            for p, m in zip(paths, out)]
    return rows, secs, {"version": subprocess.run([FILE_EXE, "--version"], capture_output=True,
                                                  text=True).stdout.splitlines()[0]}


def magika(paths):
    from pathlib import Path
    import magika as mg
    t = time.perf_counter()
    m = mg.Magika()
    load = time.perf_counter() - t
    ps = [Path(p) for p in paths]
    m.identify_paths(ps)
    t = time.perf_counter()
    res = m.identify_paths(ps)
    secs = time.perf_counter() - t
    rows = []
    for p, r in zip(paths, res):
        lab = str(r.output.label) if r.ok else "error"
        rows.append({"file": name_of(p), "label": MAGIKA.get(lab, "unknown"), "raw": lab,
                     "score": round(float(r.score), 4) if r.ok else None})
    return rows, secs, {"version": mg.__version__, "model_load_s": round(load, 3)}


def main():
    listfile, outdir = sys.argv[1:3]
    paths = [l.strip() for l in open(listfile, encoding="utf-8") if l.strip()]
    meta = {}
    for name, fn in (("libmagic", lambda: libmagic(paths, listfile)), ("magika", lambda: magika(paths))):
        rows, secs, info = fn()
        with open(os.path.join(outdir, name + ".jsonl"), "w", encoding="utf-8") as f:
            for r in rows:
                f.write(json.dumps(r) + "\n")
        meta[name] = {**info, "files": len(rows), "batch_s": round(secs, 3),
                      "per_file_ms": round(1000 * secs / len(rows), 3)}
        print(name, meta[name])
    with open(os.path.join(outdir, "competitors-meta.json"), "w", encoding="utf-8") as f:
        json.dump(meta, f, indent=1)


if __name__ == "__main__":
    main()
