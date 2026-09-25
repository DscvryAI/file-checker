"""Ground truth by parsing: a file's label is whatever a real parser can open it as.

Never looks at the extension (it is only recorded, to measure how often it lies).
Parsers: olefile, zipfile + python-docx/openpyxl/python-pptx, PyMuPDF, Pillow, gzip, json, xml.
Files no parser opens get label null ("unverified"); text-ish files without a parser (html,
csv, plain text, rtf, eml) are unverified too, and are scored as agreement only.

usage: python tools/label.py <dir> <out.json> [manifest.json]
  With a manifest (constructed set), also prints agreement between parser and construction.
"""
import gzip
import io
import json
import os
import sys
import warnings
import xml.etree.ElementTree as ET
import zipfile

import fitz
import olefile
import openpyxl
from docx import Document
from PIL import Image
from pptx import Presentation

warnings.filterwarnings("ignore")
fitz.TOOLS.mupdf_display_errors(False)
PIL_MAP = {"JPEG": "jpeg", "PNG": "png", "GIF": "gif", "TIFF": "tiff", "BMP": "bmp", "WEBP": "webp"}
ODF = {"application/vnd.oasis.opendocument.text": "odt",
       "application/vnd.oasis.opendocument.spreadsheet": "ods",
       "application/vnd.oasis.opendocument.presentation": "odp",
       "application/epub+zip": "epub"}


def ole(path):
    with olefile.OleFileIO(path) as o:
        streams = {"/".join(s) for s in o.listdir(streams=True, storages=False)}
    top = {s.split("/")[0] for s in streams}
    if {"EncryptedPackage", "EncryptionInfo"} <= top:
        return "office-encrypted", "olefile"
    if "WordDocument" in top:
        return "doc", "olefile"
    if top & {"Workbook", "Book"}:
        return "xls", "olefile"
    if "PowerPoint Document" in top:
        return "ppt", "olefile"
    if any(s.startswith("__substg1.0_") for s in top):
        return "msg", "olefile"
    return "other", "olefile"


def zipped(path):
    with zipfile.ZipFile(path) as z:
        if z.testzip() is not None:
            return None
        names = z.namelist()
        if names and names[0] == "mimetype":
            mime = z.read("mimetype").decode("ascii", "replace").strip()
            if mime in ODF:
                return ODF[mime], "zipfile-mimetype"
    # Openers get bytes, not the path: openpyxl otherwise rejects a workbook by its extension.
    data = open(path, "rb").read()
    for label, opener in (("docx", Document), ("xlsx", lambda f: openpyxl.load_workbook(f, read_only=True)),
                          ("pptx", Presentation)):
        try:
            opener(io.BytesIO(data))
            return label, "python-" + label
        except Exception:
            pass
    # An OPC package (macro-enabled, template, xlsb, xps, theme, or odd) that none of the
    # libraries opens is not verified as anything; calling it a plain zip would be wrong.
    if any(n.lower() == "[content_types].xml" for n in names):
        return None
    return "zip", "zipfile"


def pdf(path):
    doc = fitz.open(path, filetype="pdf")
    try:
        if doc.page_count > 0:
            flags = ["repaired"] if doc.is_repaired else []
            return "pdf", "pymupdf", flags
    finally:
        doc.close()
    return None


def image(path):
    with Image.open(path) as im:
        fmt = im.format
        im.verify()
    return PIL_MAP.get(fmt, "other"), "pillow:" + fmt


def textish(path):
    data = open(path, "rb").read()
    if b"\x00" in data[:8192]:
        return None
    try:
        text = data.decode("utf-8-sig")
    except UnicodeDecodeError:
        return None
    try:
        v = json.loads(text)
        if isinstance(v, (dict, list)):
            return "json", "json"
    except ValueError:
        pass
    try:
        root = ET.fromstring(data)
        tag = root.tag.rsplit("}", 1)[-1].lower()
        return ("html", "xml-parser:xhtml") if tag == "html" else ("xml", "xml-parser")
    except ET.ParseError:
        pass
    return None


def label(path):
    """Return (label or None, via, flags). The first parser that succeeds wins."""
    if olefile.isOleFile(path):
        try:
            return (*ole(path), [])
        except Exception:
            pass
    if zipfile.is_zipfile(path):
        try:
            r = zipped(path)
            if r:
                return (*r, [])
        except Exception:
            pass
    for fn in (pdf, image):
        try:
            r = fn(path)
            if r:
                return r if len(r) == 3 else (*r, [])
        except Exception:
            pass
    try:
        with gzip.open(path) as g:
            while g.read(1 << 20):
                pass
        return "gzip", "gzip", []
    except Exception:
        pass
    r = textish(path)
    if r:
        return (*r, [])
    return None, "none", []


def main():
    d, out, *rest = sys.argv[1:]
    files = sorted(f for f in os.listdir(d) if f != "manifest.json")
    result = {}
    for f in files:
        lab, via, flags = label(os.path.join(d, f))
        result[f] = {"label": lab, "via": via, "flags": flags, "ext": f.rsplit(".", 1)[-1].lower()}
    with open(out, "w", encoding="utf-8") as fh:
        json.dump(result, fh, indent=1)
    counts = {}
    for r in result.values():
        counts[r["label"] or "unverified"] = counts.get(r["label"] or "unverified", 0) + 1
    print(len(result), "files", json.dumps(dict(sorted(counts.items(), key=lambda kv: -kv[1]))))
    if rest:
        man = {e["file"]: e for e in json.load(open(rest[0], encoding="utf-8"))}
        agree = [f for f in man if result[f]["label"] == man[f]["label"]]
        print(f"agrees with construction: {len(agree)}/{len(man)}")
        for f in man:
            if result[f]["label"] != man[f]["label"]:
                print(f"  {f}: built {man[f]['label']} {man[f]['flags']} ({man[f]['origin']}), "
                      f"parser says {result[f]['label']} via {result[f]['via']}")


if __name__ == "__main__":
    main()
