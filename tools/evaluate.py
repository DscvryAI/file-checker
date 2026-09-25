"""Score one tool's output (jsonl of {file, label, flags?}) against the truth.

Truth:
  - constructed set: the manifest (labels known by construction);
  - govdocs1: the parser labeller; files no parser opened are "unverified" and only described.
Anything outside the label set counts as "unknown" on both sides, so a tool is right on an
out-of-set file if it does not claim an in-set label.

usage: python tools/evaluate.py <name> <results.jsonl> [--verbose] [--heldout]
"""
import collections
import json
import sys

TEXTISH = {"html", "xml", "json", "eml", "csv", "text"}
LABELS = {"pdf", "docx", "xlsx", "pptx", "doc", "xls", "ppt", "msg", "office-encrypted", "rtf", "odt",
          "ods", "odp", "epub", "jpeg", "png", "gif", "tiff", "bmp", "webp", "zip", "gzip", "7z",
          "rar"} | TEXTISH


def norm(label):
    return label if label in LABELS else "unknown"


def truth_heldout():
    """Held-out: govdocs1 thread 002 (g-prefixed) and Apache POI test-data (poi-prefixed)."""
    t = {}
    for f, e in json.load(open("data/labels-heldout.json", encoding="utf-8")).items():
        src = "poi" if f.startswith("poi-") else "govdocs"
        if e["label"] is None:
            t[f] = {"label": None, "flags": set(), "group": f"heldout-{src}-unverified", "ext": e["ext"]}
        else:
            t[f] = {"label": norm(e["label"]), "flags": set(), "group": f"heldout-{src}", "ext": e["ext"]}
    return t


def truth():
    t = {}
    for e in json.load(open("data/constructed/manifest.json", encoding="utf-8")):
        flags = set(e["flags"])
        group = ("constructed-truncated" if "truncated" in flags else
                 "constructed-text" if e["label"] in TEXTISH else "constructed")
        t[e["file"]] = {"label": e["label"], "flags": flags, "group": group}
    for f, e in json.load(open("data/labels-govdocs.json", encoding="utf-8")).items():
        if e["label"] is None:
            t[f] = {"label": None, "flags": set(), "group": "govdocs-unverified", "ext": e["ext"]}
        else:
            t[f] = {"label": norm(e["label"]), "flags": set(), "group": "govdocs", "ext": e["ext"]}
    return t


def main():
    name, path, *opt = sys.argv[1:]
    verbose = "--verbose" in opt
    heldout = "--heldout" in opt
    res = {}
    for line in open(path, encoding="utf-8"):
        r = json.loads(line)
        res[r["file"]] = r
    t = truth_heldout() if heldout else truth()
    missing = [f for f in t if f not in res]
    print(f"== {name}: {len(res)} results, {len(missing)} missing")

    groups = collections.defaultdict(lambda: [0, 0])
    misses = collections.Counter()
    for f, tr in t.items():
        if tr["label"] is None or f not in res:
            continue
        got = norm(res[f]["label"])
        ok = got == tr["label"]
        groups[tr["group"]][0] += ok
        groups[tr["group"]][1] += 1
        if not ok:
            misses[(tr["group"], tr["label"], got)] += 1
            if verbose:
                print(f"   miss {f}: truth {tr['label']}, got {res[f]['label']}")
    shown = (("heldout-govdocs", "heldout-poi") if heldout else
             ("govdocs", "constructed", "constructed-text", "constructed-truncated"))
    scored = shown if heldout else ("govdocs", "constructed")
    for g in shown:
        ok, n = groups[g]
        print(f"  {g:24s} {ok:4d} / {n:<4d}")
    ok = sum(groups[g][0] for g in scored)
    n = sum(groups[g][1] for g in scored)
    print(f"  {'STRUCTURAL (scored)':24s} {ok:4d} / {n:<4d}")
    for (g, want, got), c in sorted(misses.items(), key=lambda kv: -kv[1]):
        print(f"    {c:3d} x {g}: {want} -> {got}")

    # Flags, where the tool reports them.
    if any("flags" in r for r in res.values()):
        for flag in ("extension-mismatch", "truncated", "encrypted"):
            tp = fp = fn = 0
            for f, tr in t.items():
                if f not in res or tr["group"].endswith("unverified"):
                    continue
                has, want = flag in res[f].get("flags", []), flag in tr["flags"]
                tp += has and want
                fp += has and not want
                fn += want and not has
            print(f"  flag {flag:20s} caught {tp}, missed {fn}, false alarms {fp}")

    # Unverified govdocs files: what the tool calls them, by extension (agreement only).
    by_ext = collections.defaultdict(collections.Counter)
    for f, tr in t.items():
        if tr["group"].endswith("unverified") and f in res:
            by_ext[tr["ext"]][res[f]["label"]] += 1
    print("  unverified govdocs, tool label by extension:")
    for ext, c in sorted(by_ext.items(), key=lambda kv: -sum(kv[1].values())):
        print(f"    .{ext:7s} " + ", ".join(f"{k} {v}" for k, v in c.most_common()))


if __name__ == "__main__":
    main()
