//! Browser build: one call, bytes and filename in, a JSON verdict out.

use file_checker::{check, Bytes};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn check_json(bytes: &[u8], name: &str) -> String {
    let v = check(&Bytes(bytes), name);
    let flags: Vec<String> = v.flags().iter().map(|f| format!("\"{f}\"")).collect();
    format!(
        "{{\"label\":\"{}\",\"detail\":\"{}\",\"flags\":[{}],\"bytes_read\":{},\"size\":{}}}",
        v.label, v.detail, flags.join(","), v.bytes_read, bytes.len()
    )
}
