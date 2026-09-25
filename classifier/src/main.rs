//! usage:
//!   file-checker <file>...                          label and flags, one line per file
//!   file-checker --bench <list.txt> <out.jsonl> [reps]
//!     For each file: one end-to-end check from disk (open + only the reads it needs), then
//!     `reps` in-memory runs on the whole file; writes the median of those.

use std::borrow::Cow;
use std::cell::RefCell;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::time::Instant;

use file_checker::{check, Bytes, Source};

struct FileSource {
    f: RefCell<File>,
    size: u64,
}

impl Source for FileSource {
    fn size(&self) -> u64 {
        self.size
    }
    fn read_at(&self, off: u64, len: usize) -> Option<Cow<'_, [u8]>> {
        if off > self.size {
            return None;
        }
        let len = len.min((self.size - off) as usize);
        let mut buf = vec![0; len];
        let mut f = self.f.borrow_mut();
        f.seek(SeekFrom::Start(off)).ok()?;
        f.read_exact(&mut buf).ok()?;
        Some(Cow::Owned(buf))
    }
}

fn open(path: &str) -> std::io::Result<FileSource> {
    let f = File::open(path)?;
    let size = f.metadata()?.len();
    Ok(FileSource { f: RefCell::new(f), size })
}

fn name_of(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--bench") {
        let list = std::fs::read_to_string(&args[1]).expect("list");
        let mut out = std::io::BufWriter::new(File::create(&args[2]).expect("out"));
        let reps: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(9);
        let total = Instant::now();
        for path in list.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let t = Instant::now();
            let v = match open(path) {
                Ok(src) => check(&src, name_of(path)),
                Err(e) => {
                    eprintln!("{path}: {e}");
                    continue;
                }
            };
            let disk_us = t.elapsed().as_secs_f64() * 1e6;
            // reps = 0: disk-only batch run (for wall-clock comparison with other tools).
            let mem_us = if reps == 0 {
                "null".to_string()
            } else {
                let bytes = std::fs::read(path).unwrap();
                let mut times: Vec<f64> = (0..reps)
                    .map(|_| {
                        let t = Instant::now();
                        std::hint::black_box(check(&Bytes(std::hint::black_box(&bytes[..])), name_of(path)));
                        t.elapsed().as_secs_f64() * 1e6
                    })
                    .collect();
                times.sort_by(|a, b| a.partial_cmp(b).unwrap());
                format!("{:.2}", times[times.len() / 2])
            };
            let size = std::fs::metadata(path).map_or(0, |m| m.len());
            let flags: Vec<String> = v.flags().iter().map(|f| json_str(f)).collect();
            writeln!(out, "{{\"file\":{},\"label\":{},\"detail\":{},\"flags\":[{}],\"bytes_read\":{},\"size\":{},\"disk_us\":{:.1},\"mem_us\":{}}}",
                json_str(name_of(path)), json_str(v.label), json_str(v.detail), flags.join(","),
                v.bytes_read, size, disk_us, mem_us).unwrap();
        }
        eprintln!("done in {:.2}s", total.elapsed().as_secs_f64());
        return;
    }
    for path in &args {
        match open(path) {
            Ok(src) => {
                let v = check(&src, name_of(path));
                let d = if v.detail.is_empty() { String::new() } else { format!(" ({})", v.detail) };
                println!("{path}: {}{d} {:?} read {} of {} bytes", v.label, v.flags(), v.bytes_read, src.size);
            }
            Err(e) => println!("{path}: error {e}"),
        }
    }
}
