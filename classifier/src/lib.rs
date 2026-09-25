//! What is this upload really? A structural file-type check for document pipelines.
//!
//! Reads the first bytes, and when the format needs it the zip central directory or the OLE
//! (Compound File) directory. Never decompresses, never parses content. No dependencies.

use std::borrow::Cow;
use std::cell::Cell;

/// Random access to the file's bytes. Every read is counted, so we can report bytes read.
pub trait Source {
    fn size(&self) -> u64;
    /// Up to `len` bytes at `off` (fewer at end of file); None if `off` is past the end.
    fn read_at(&self, off: u64, len: usize) -> Option<Cow<'_, [u8]>>;
}

/// A file already in memory.
pub struct Bytes<'a>(pub &'a [u8]);

impl Source for Bytes<'_> {
    fn size(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&self, off: u64, len: usize) -> Option<Cow<'_, [u8]>> {
        let b = self.0;
        let off = usize::try_from(off).ok()?;
        if off > b.len() {
            return None;
        }
        Some(Cow::Borrowed(&b[off..b.len().min(off.saturating_add(len))]))
    }
}

struct Src<'a> {
    inner: &'a dyn Source,
    read: Cell<u64>,
}

impl<'a> Src<'a> {
    fn size(&self) -> u64 {
        self.inner.size()
    }
    fn get(&self, off: u64, len: usize) -> Option<Cow<'a, [u8]>> {
        let r = self.inner.read_at(off, len)?;
        self.read.set(self.read.get() + r.len() as u64);
        Some(r)
    }
    /// Exactly `len` bytes, or None (short read = truncated structure).
    fn exact(&self, off: u64, len: usize) -> Option<Cow<'a, [u8]>> {
        self.get(off, len).filter(|r| r.len() == len)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// One of the label set, or "unknown" for anything outside it.
    pub label: &'static str,
    /// A finer name when useful ("postscript", "xlsb", "ole-empty"...). Not scored.
    pub detail: &'static str,
    pub truncated: bool,
    pub encrypted: bool,
    pub macros: bool,
    pub extension_mismatch: bool,
    pub bytes_read: u64,
}

impl Verdict {
    fn new(label: &'static str) -> Self {
        Verdict { label, detail: "", truncated: false, encrypted: false, macros: false,
                  extension_mismatch: false, bytes_read: 0 }
    }
    fn detail(mut self, d: &'static str) -> Self {
        self.detail = d;
        self
    }
    pub fn flags(&self) -> Vec<&'static str> {
        let mut f = Vec::new();
        if self.encrypted { f.push("encrypted"); }
        if self.extension_mismatch { f.push("extension-mismatch"); }
        if self.macros { f.push("macros"); }
        if self.truncated { f.push("truncated"); }
        f
    }
}

const HEAD: usize = 8192;

/// Classify from content alone.
pub fn classify(source: &dyn Source) -> Verdict {
    let src = Src { inner: source, read: Cell::new(0) };
    let mut v = sniff(&src);
    v.bytes_read = src.read.get();
    v
}

/// Classify, then compare with the filename's extension.
pub fn check(source: &dyn Source, name: &str) -> Verdict {
    let mut v = classify(source);
    if let Some(expected) = name.rsplit_once('.').and_then(|(_, e)| label_for_extension(e)) {
        v.extension_mismatch = mismatch(expected, v.label);
    }
    v
}

pub fn label_for_extension(ext: &str) -> Option<&'static str> {
    Some(match ext.to_ascii_lowercase().as_str() {
        "pdf" => "pdf",
        "docx" | "docm" | "dotx" | "dotm" => "docx",
        "xlsx" | "xlsm" | "xltx" | "xltm" | "xlsb" => "xlsx",
        "pptx" | "pptm" | "potx" | "ppsx" | "ppsm" => "pptx",
        "doc" | "dot" => "doc",
        "xls" | "xlt" => "xls",
        "ppt" | "pps" | "pot" => "ppt",
        "msg" => "msg",
        "rtf" => "rtf",
        "odt" => "odt",
        "ods" => "ods",
        "odp" => "odp",
        "epub" => "epub",
        "jpg" | "jpeg" | "jpe" | "jfif" => "jpeg",
        "png" => "png",
        "gif" => "gif",
        "tif" | "tiff" => "tiff",
        "bmp" => "bmp",
        "webp" => "webp",
        "zip" => "zip",
        "gz" | "tgz" => "gzip",
        "7z" => "7z",
        "rar" => "rar",
        "html" | "htm" | "xhtml" => "html",
        "xml" => "xml",
        "json" => "json",
        "eml" => "eml",
        "csv" | "tsv" => "csv",
        "txt" | "text" | "log" => "text",
        _ => return None,
    })
}

const TEXTISH: [&str; 6] = ["html", "xml", "json", "eml", "csv", "text"];

fn mismatch(expected: &str, got: &str) -> bool {
    if expected == got || got == "unknown" {
        return false;
    }
    // A password-protected .docx/.xlsx/.pptx is correctly named: Office opens it as such.
    if got == "office-encrypted" && matches!(expected, "docx" | "xlsx" | "pptx") {
        return false;
    }
    // Among text formats the line is blurry (a .txt that happens to be CSV); only flag a
    // text extension on binary content, or a binary extension on text.
    !(TEXTISH.contains(&expected) && TEXTISH.contains(&got))
}

fn sniff(src: &Src) -> Verdict {
    let Some(head) = src.get(0, HEAD) else { return Verdict::new("unknown").detail("empty") };
    let h: &[u8] = &head;
    if h.is_empty() {
        return Verdict::new("unknown").detail("empty");
    }
    if h.starts_with(b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1") {
        return ole(src);
    }
    if h.starts_with(b"PK\x03\x04") || h.starts_with(b"PK\x05\x06") {
        return zip(src, h);
    }
    // PDF: the header may follow a little junk (the spec's readers allow it within 1 KB).
    if find(&h[..h.len().min(1024)], b"%PDF-").is_some() {
        let mut v = Verdict::new("pdf");
        v.truncated = !tail_contains(src, 1024, b"%%EOF");
        return v;
    }
    if h.starts_with(b"\xFF\xD8\xFF") {
        let mut v = Verdict::new("jpeg");
        v.truncated = !tail_contains(src, 64, b"\xFF\xD9");
        return v;
    }
    if h.starts_with(b"\x89PNG\r\n\x1A\n") {
        let mut v = Verdict::new("png");
        v.truncated = !tail_contains(src, 12, b"IEND");
        return v;
    }
    if h.starts_with(b"GIF87a") || h.starts_with(b"GIF89a") {
        return Verdict::new("gif");
    }
    if h.starts_with(b"II*\0") || h.starts_with(b"MM\0*") {
        return Verdict::new("tiff");
    }
    if h.len() >= 18 && h.starts_with(b"BM") {
        let dib = u32le(h, 14);
        if matches!(dib, 12 | 40 | 52 | 56 | 64 | 108 | 124) {
            return Verdict::new("bmp");
        }
    }
    if h.len() >= 12 && h.starts_with(b"RIFF") && &h[8..12] == b"WEBP" {
        return Verdict::new("webp");
    }
    if h.starts_with(b"\x1F\x8B\x08") {
        return Verdict::new("gzip");
    }
    if h.starts_with(b"7z\xBC\xAF\x27\x1C") {
        return Verdict::new("7z");
    }
    if h.starts_with(b"Rar!\x1A\x07") {
        return Verdict::new("rar");
    }
    if h.starts_with(b"{\\rtf") {
        return Verdict::new("rtf");
    }
    if h.starts_with(b"%!PS") || h.starts_with(b"\xC5\xD0\xD3\xC6") {
        return Verdict::new("unknown").detail("postscript");
    }
    text(src, h)
}

// ---------- OLE / Compound File Binary ----------

const ENDOFCHAIN: u32 = 0xFFFF_FFFE;

struct Ole<'s, 'a> {
    src: &'s Src<'a>,
    ss: u64,
    header: Cow<'a, [u8]>,
    truncated: Cell<bool>,
}

impl Ole<'_, '_> {
    fn sector_off(&self, s: u32) -> u64 {
        (s as u64 + 1) * self.ss
    }
    fn u32_at(&self, off: u64) -> Option<u32> {
        match self.src.exact(off, 4) {
            Some(b) => Some(u32le(&b, 0)),
            None => {
                self.truncated.set(true);
                None
            }
        }
    }
    /// The i-th FAT sector id, from the header's 109 DIFAT slots and then the DIFAT chain.
    fn difat(&self, i: u64) -> Option<u32> {
        if i < 109 {
            return Some(u32le(&self.header, 0x4C + 4 * i as usize));
        }
        let per = self.ss / 4 - 1;
        let mut sec = u32le(&self.header, 0x44);
        let mut j = i - 109;
        let mut steps = 0;
        while j >= per {
            sec = self.u32_at(self.sector_off(sec) + per * 4)?;
            j -= per;
            steps += 1;
            if sec >= ENDOFCHAIN || steps > 100_000 {
                return None;
            }
        }
        self.u32_at(self.sector_off(sec) + j * 4)
    }
    fn next(&self, s: u32) -> Option<u32> {
        let per = self.ss / 4;
        let fat_sec = self.difat(s as u64 / per)?;
        if fat_sec >= ENDOFCHAIN {
            return None;
        }
        self.u32_at(self.sector_off(fat_sec) + (s as u64 % per) * 4)
    }
}

fn ole(src: &Src) -> Verdict {
    let unknown = |t: bool, d: &'static str| {
        let mut v = Verdict::new("unknown").detail(d);
        v.truncated = t;
        v
    };
    let Some(header) = src.exact(0, 512) else { return unknown(true, "ole") };
    let shift = u16le(&header, 0x1E);
    if shift != 9 && shift != 12 {
        return unknown(false, "ole-bad-header");
    }
    let o = Ole { src, ss: 1 << shift, header, truncated: Cell::new(false) };
    let max_sectors = src.size() / o.ss + 2;

    // Directory chain: follow it lazily, one sector at a time.
    let mut chain = vec![u32le(&o.header, 0x30)];
    let per_sec = o.ss / 128;
    let entry = |i: u32, chain: &mut Vec<u32>| -> Option<Cow<[u8]>> {
        let k = (i as u64 / per_sec) as usize;
        while chain.len() <= k {
            let last = *chain.last()?;
            if last >= ENDOFCHAIN || chain.len() as u64 > max_sectors {
                return None;
            }
            chain.push(o.next(last)?);
        }
        let sec = chain[k];
        if sec >= ENDOFCHAIN {
            return None;
        }
        let e = src.exact(o.sector_off(sec) + (i as u64 % per_sec) * 128, 128);
        if e.is_none() {
            o.truncated.set(true);
        }
        e
    };

    // The root's direct children form a red-black tree over left/right sibling links.
    let mut names: Vec<String> = Vec::new();
    let root = entry(0, &mut chain);
    let mut stack = match root.as_deref() {
        Some(r) if r[0x42] == 5 => vec![u32le(r, 0x4C)],
        _ => return unknown(o.truncated.get(), "ole-no-root"),
    };
    let mut seen = std::collections::HashSet::new();
    while let Some(i) = stack.pop() {
        if i >= 0xFFFF_FFFA || !seen.insert(i) || seen.len() > 50_000 {
            continue;
        }
        let Some(e) = entry(i, &mut chain) else { continue };
        let n = (u16le(&e, 0x40) as usize).min(64);
        let name: String = char::decode_utf16(
            e[..n.saturating_sub(2)].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])),
        )
        .map(|c| c.unwrap_or('?'))
        .collect();
        names.push(name);
        stack.push(u32le(&e, 0x44));
        stack.push(u32le(&e, 0x48));
    }
    let has = |n: &str| names.iter().any(|x| x == n);
    let mut v = if has("EncryptedPackage") && has("EncryptionInfo") {
        let mut v = Verdict::new("office-encrypted");
        v.encrypted = true;
        v
    } else if has("WordDocument") {
        Verdict::new("doc")
    } else if has("Workbook") || has("Book") {
        Verdict::new("xls")
    } else if has("PowerPoint Document") {
        Verdict::new("ppt")
    } else if names.iter().any(|x| x.starts_with("__substg1.0_")) {
        Verdict::new("msg")
    } else if names.is_empty() {
        Verdict::new("unknown").detail("ole-empty")
    } else {
        Verdict::new("unknown").detail("ole-other")
    };
    if v.label != "unknown" && v.label != "office-encrypted" {
        v.macros = has("Macros") || has("_VBA_PROJECT_CUR") || has("VBA");
    }
    v.truncated = o.truncated.get();
    v
}

// ---------- ZIP family: OOXML, ODF, EPUB, plain zip ----------

fn zip(src: &Src, head: &[u8]) -> Verdict {
    let (names, truncated) = match central_directory(src) {
        Some(n) => (n, false),
        None => (local_headers(src), true),
    };
    let has = |n: &str| names.iter().any(|x| x == n);
    let any = |p: &str| names.iter().any(|x| x.starts_with(p));
    // Without a central directory we only see the entries before the cut: the part folder
    // alone has to count as evidence.
    let types = has("[Content_Types].xml") || truncated;
    let mut v = if head.len() >= 38 && &head[30..38] == b"mimetype" && u16le(head, 8) == 0 {
        // ODF/EPUB: a stored "mimetype" entry first, its content right after the local header.
        let data_at = 30 + u16le(head, 26) as usize + u16le(head, 28) as usize;
        let len = (u32le(head, 18) as usize).min(100);
        let mime = head.get(data_at..data_at + len).unwrap_or(b"");
        match mime {
            b"application/vnd.oasis.opendocument.text" => Verdict::new("odt"),
            b"application/vnd.oasis.opendocument.spreadsheet" => Verdict::new("ods"),
            b"application/vnd.oasis.opendocument.presentation" => Verdict::new("odp"),
            b"application/epub+zip" => Verdict::new("epub"),
            _ => Verdict::new("unknown").detail("zip-mimetype-other"),
        }
    // OOXML: [Content_Types].xml plus the part folder, or the main part by its exact name. The
    // second matters when the directory is missing and writers put [Content_Types].xml last.
    } else if has("word/document.xml") || types && any("word/") {
        Verdict::new("docx")
    } else if has("xl/workbook.xml") || has("xl/workbook.bin") || types && any("xl/") {
        Verdict::new("xlsx").detail(if has("xl/workbook.bin") { "xlsb" } else { "" })
    } else if has("ppt/presentation.xml") || types && any("ppt/") {
        Verdict::new("pptx")
    } else {
        Verdict::new("zip")
    };
    v.macros = names.iter().any(|x| x.ends_with("vbaProject.bin"));
    v.truncated = truncated;
    v
}

fn central_directory(src: &Src) -> Option<Vec<String>> {
    let size = src.size();
    let span = size.min(22 + 65535);
    let tail = src.get(size - span, span as usize)?;
    let at = rfind(&tail, b"PK\x05\x06")?;
    let eocd = &tail[at..];
    if eocd.len() < 22 {
        return None;
    }
    let mut cd_size = u32le(eocd, 12) as u64;
    let mut cd_off = u32le(eocd, 16) as u64;
    if cd_off == 0xFFFF_FFFF || cd_size == 0xFFFF_FFFF {
        // Zip64: the locator sits just before the classic end record.
        let loc_pos = (size - span) + at as u64;
        let loc = src.exact(loc_pos.checked_sub(20)?, 20)?;
        if !loc.starts_with(b"PK\x06\x07") {
            return None;
        }
        let rec = src.exact(u64le(&loc, 8), 56)?;
        if !rec.starts_with(b"PK\x06\x06") {
            return None;
        }
        cd_size = u64le(&rec, 40);
        cd_off = u64le(&rec, 48);
    }
    if cd_off.checked_add(cd_size)? > size || cd_size > 64 << 20 {
        return None;
    }
    let cd = src.exact(cd_off, cd_size as usize)?;
    let mut names = Vec::new();
    let mut p = 0;
    while p + 46 <= cd.len() && &cd[p..p + 4] == b"PK\x01\x02" {
        let (n, e, c) = (u16le(&cd, p + 28) as usize, u16le(&cd, p + 30) as usize, u16le(&cd, p + 32) as usize);
        let name = cd.get(p + 46..p + 46 + n)?;
        names.push(String::from_utf8_lossy(name).into_owned());
        p += 46 + n + e + c;
    }
    Some(names)
}

/// No central directory (truncated): walk local file headers from the start.
fn local_headers(src: &Src) -> Vec<String> {
    let mut names = Vec::new();
    let mut off = 0u64;
    while names.len() < 10_000 {
        let Some(h) = src.exact(off, 30) else { break };
        if !h.starts_with(b"PK\x03\x04") {
            break;
        }
        let (flags, csize) = (u16le(&h, 6), u32le(&h, 18) as u64);
        let (n, e) = (u16le(&h, 26) as u64, u16le(&h, 28) as u64);
        let Some(name) = src.exact(off + 30, n as usize) else { break };
        names.push(String::from_utf8_lossy(&name).into_owned());
        if flags & 8 != 0 && csize == 0 {
            break; // size only in a trailing data descriptor; can't skip without inflating
        }
        off += 30 + n + e + csize;
    }
    names
}

// ---------- Text formats ----------

fn text(src: &Src, h: &[u8]) -> Verdict {
    if h.starts_with(b"\xFF\xFE") || h.starts_with(b"\xFE\xFF") {
        return Verdict::new("text").detail("utf-16");
    }
    if h.contains(&0) {
        return Verdict::new("unknown");
    }
    let controls = h.iter().filter(|&&b| b < 0x09 || (0x0E..0x20).contains(&b) && b != 0x1B).count();
    let valid_utf8 = match std::str::from_utf8(h) {
        Ok(_) => true,
        Err(e) => e.error_len().is_none(), // only a character cut at the end of the head
    };
    if !valid_utf8 && controls * 100 > h.len() || controls * 20 > h.len() {
        return Verdict::new("unknown");
    }
    let t = strip_bom(h).trim_ascii_start();
    let lower: Vec<u8> = t[..t.len().min(4096)].to_ascii_lowercase();
    let starts = |p: &[u8]| lower.starts_with(p);

    if starts(b"<?xml") {
        let html = find(&lower, b"<html").is_some() || find(&lower, b"<!doctype html").is_some();
        return Verdict::new(if html { "html" } else { "xml" });
    }
    if starts(b"<!doctype html") || starts(b"<html") {
        return Verdict::new("html");
    }
    if lower.first() == Some(&b'<') {
        // Markup: HTML if the first element is an HTML one (or <html>/<body> appears), else XML.
        // A <title> alone is not enough: SGML/XML corpora use it too.
        const HTML_FIRST: [&[u8]; 14] = [b"<!--", b"<head", b"<body", b"<title", b"<meta", b"<table",
            b"<div", b"<p>", b"<p ", b"<br", b"<a ", b"<h1", b"<script", b"<link"];
        if HTML_FIRST.iter().any(|tag| starts(tag))
            || find(&lower, b"<html").is_some() {
            return Verdict::new("html");
        }
        if find(&lower, b"</").is_some() || find(&lower, b"/>").is_some() {
            return Verdict::new("xml");
        }
    }
    if matches!(t.first(), Some(b'{') | Some(b'[')) && looks_like_json(src, t) {
        return Verdict::new("json");
    }
    if is_email(t) {
        return Verdict::new("eml");
    }
    if is_delimited(t) {
        return Verdict::new("csv");
    }
    // Loose HTML: tags that appear later in the head, as in fragments saved as .html.
    if [&b"<html"[..], b"<body", b"<table", b"<!doctype html"].iter().any(|tag| find(&lower, tag).is_some()) {
        return Verdict::new("html");
    }
    Verdict::new("text")
}

fn looks_like_json(src: &Src, t: &[u8]) -> bool {
    let open = t[0];
    let close = if open == b'{' { b'}' } else { b']' };
    let rest = t[1..].trim_ascii_start();
    let second_ok = match open {
        b'{' => matches!(rest.first(), Some(b'"') | Some(b'}')),
        _ => matches!(rest.first(), Some(b'{' | b'[' | b'"' | b']' | b'-' | b'0'..=b'9' | b't' | b'f' | b'n')),
    };
    if !second_ok {
        return false;
    }
    let size = src.size();
    let tail = src.get(size.saturating_sub(64), 64).unwrap_or_default();
    tail.trim_ascii_end().last() == Some(&close)
}

fn is_email(t: &[u8]) -> bool {
    let mut from = false;
    let mut other = false;
    for line in t.split(|&b| b == b'\n').take(60) {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            break;
        }
        if line[0] == b' ' || line[0] == b'\t' {
            continue; // folded header
        }
        let Some(colon) = line.iter().position(|&b| b == b':') else { return false };
        let name = &line[..colon];
        if name.is_empty() || !name.iter().all(|&b| b.is_ascii_alphanumeric() || b == b'-') {
            return false;
        }
        let name = name.to_ascii_lowercase();
        from |= name == b"from";
        other |= matches!(&name[..], b"date" | b"subject" | b"to" | b"message-id" | b"received" | b"mime-version");
    }
    from && other
}

fn is_delimited(t: &[u8]) -> bool {
    let lines: Vec<&[u8]> = t.split(|&b| b == b'\n').take(40).collect();
    // Drop the last line: the head may cut it.
    let lines: Vec<&[u8]> = lines[..lines.len().saturating_sub(1)].iter().copied()
        .filter(|l| !l.trim_ascii().is_empty()).collect();
    if lines.len() < 2 {
        return false;
    }
    for d in [b',', b'\t', b';', b'|'] {
        let counts: Vec<usize> = lines.iter().map(|l| fields(l, d)).collect();
        if counts[0] >= 2 && counts.iter().all(|&c| c == counts[0]) {
            return true;
        }
    }
    false
}

/// Fields on one line, respecting double quotes.
fn fields(line: &[u8], d: u8) -> usize {
    let mut n = 1;
    let mut quoted = false;
    for &b in line {
        if b == b'"' {
            quoted = !quoted;
        } else if b == d && !quoted {
            n += 1;
        }
    }
    n
}

// ---------- Helpers ----------

fn tail_contains(src: &Src, n: u64, needle: &[u8]) -> bool {
    let size = src.size();
    let start = size.saturating_sub(n);
    src.get(start, (size - start) as usize).map_or(false, |t| find(&t, needle).is_some())
}

fn strip_bom(h: &[u8]) -> &[u8] {
    h.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(h)
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

fn rfind(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).rposition(|w| w == n)
}

fn u16le(b: &[u8], o: usize) -> u16 {
    b.get(o..o + 2).map_or(0, |s| u16::from_le_bytes([s[0], s[1]]))
}

fn u32le(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4).map_or(0, |s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn u64le(b: &[u8], o: usize) -> u64 {
    b.get(o..o + 8).map_or(0, |s| u64::from_le_bytes(s.try_into().unwrap()))
}
