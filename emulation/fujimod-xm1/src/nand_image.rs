//! The retained engine's committed NAND image (`emulator/nand_image.py`): a
//! gzip of one JSON document naming the flash base by digest, the installed
//! data overlays and spare inputs, and every page programmed or erased since.
//! Export writes the same bytes as `export_image`: sorted keys, no spaces,
//! ASCII escapes, then gzip with mtime 0 and zlib level 9.

use crate::run::NodeName;
use fujimod_emulator::session::Session;
use fujimod_emulator_devices::parts::nand::{Nand, Overlay};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;

const KIND: &str = "type1-nand-committed-image";
const PAGE_BYTES: usize = 2048;
const SPARE_BYTES: usize = 512;
const PAGES_PER_BLOCK: u32 = 64;
const BLOCKS: u32 = 1024;

#[derive(Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Geometry {
    blocks: u32,
    page_bytes: usize,
    pages_per_block: u32,
    spare_bytes: usize,
}

#[derive(Deserialize)]
enum ImageKind {
    #[serde(rename = "type1-nand-committed-image")]
    Type1Committed,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema: u32,
    #[allow(dead_code)]
    kind: ImageKind,
    geometry: Geometry,
    base: Base,
    data_overlays: Vec<Value>,
    spare_pages: Vec<(u32, Value)>,
    committed_data_pages: Vec<(u32, String)>,
    committed_spare_pages: Vec<(u32, String)>,
}

#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Base {
    sha256: String,
    size: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OverlayRecord {
    offset: u64,
    data: String,
    sha256: String,
    #[allow(dead_code)]
    provenance: Value,
}

/// A committed image: its immutable inputs as recorded, and its committed pages.
pub struct CommittedImage {
    base: Base,
    data_overlays: Vec<Value>,
    spare_pages: Vec<(u32, Value)>,
    pages: BTreeMap<u32, (Vec<u8>, Vec<u8>)>,
}

fn hex(text: &str, bytes: usize) -> Result<Vec<u8>, String> {
    if text.len() != 2 * bytes {
        return Err("committed NAND bytes have the wrong length".into());
    }
    (0..bytes)
        .map(|index| u8::from_str_radix(&text[2 * index..2 * index + 2], 16).map_err(|_| "committed NAND bytes are not hex".into()))
        .collect()
}

fn hex_any(text: &str) -> Result<Vec<u8>, String> {
    hex(text, text.len() / 2)
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The `{"data": HEX, ...}` record of a spare input.
fn spare_input(record: &Value) -> Result<Vec<u8>, String> {
    let data = record.get("data").and_then(Value::as_str).ok_or("NAND spare input lacks data")?;
    hex_any(data)
}

impl CommittedImage {
    pub fn parse(gzip: &[u8]) -> Result<Self, String> {
        let mut json = Vec::new();
        flate2::read::GzDecoder::new(gzip).read_to_end(&mut json).map_err(|e| format!("committed NAND: {e}"))?;
        let document: Document = serde_json::from_slice(&json).map_err(|e| format!("committed NAND: {e}"))?;
        let geometry = Geometry { blocks: BLOCKS, page_bytes: PAGE_BYTES, pages_per_block: PAGES_PER_BLOCK, spare_bytes: SPARE_BYTES };
        if document.schema != 1 || document.geometry != geometry {
            return Err("committed NAND is not a schema-1 type1 committed image".into());
        }
        let data: BTreeMap<u32, &String> = document.committed_data_pages.iter().map(|(row, text)| (*row, text)).collect();
        let spare: BTreeMap<u32, &String> = document.committed_spare_pages.iter().map(|(row, text)| (*row, text)).collect();
        if data.len() != document.committed_data_pages.len()
            || spare.len() != document.committed_spare_pages.len()
            || !data.keys().eq(spare.keys())
            || data.keys().any(|row| *row >= BLOCKS * PAGES_PER_BLOCK)
        {
            return Err("committed NAND data and spare pages must be unique, paired and in range".into());
        }
        let pages = data
            .iter()
            .map(|(row, text)| Ok((*row, (hex(text, PAGE_BYTES)?, hex(spare[row], SPARE_BYTES)?))))
            .collect::<Result<_, String>>()?;
        Ok(Self { base: document.base, data_overlays: document.data_overlays, spare_pages: document.spare_pages, pages })
    }

    /// Replaces the unexecuted session's NAND `medium` with this image. The
    /// session's flash base must be the image's base; every committed block
    /// is erased and its pages programmed, which records them as written.
    pub fn apply(&self, session: &mut Session, medium: &NodeName, flash_base: &[u8]) -> Result<(), String> {
        if flash_base.len() != self.base.size || format!("{:x}", Sha256::digest(flash_base)) != self.base.sha256 {
            return Err("committed NAND base differs from the session's flash base".into());
        }
        let mut overlays = Vec::new();
        for record in &self.data_overlays {
            let record: OverlayRecord = serde_json::from_value(record.clone()).map_err(|e| format!("NAND overlay: {e}"))?;
            let bytes = hex_any(&record.data)?;
            if format!("{:x}", Sha256::digest(&bytes)) != record.sha256 {
                return Err("NAND overlay digest differs from its data".into());
            }
            overlays.push(Overlay { offset: record.offset, bytes: bytes.into() });
        }
        let spare = self
            .spare_pages
            .iter()
            .map(|(row, record)| Ok((*row, spare_input(record)?)))
            .collect::<Result<BTreeMap<u32, Vec<u8>>, String>>()?;
        let mut nand = session.nand_medium(&medium.0)?.with_inputs(overlays, spare)?;
        let blocks: std::collections::BTreeSet<u32> = self.pages.keys().map(|row| row / PAGES_PER_BLOCK).collect();
        for block in &blocks {
            if (block * PAGES_PER_BLOCK..(block + 1) * PAGES_PER_BLOCK).any(|row| !self.pages.contains_key(&row)) {
                return Err(format!("committed NAND block {block:#x} is only partly committed"));
            }
            nand.erase(block * PAGES_PER_BLOCK)?;
        }
        for (row, (data, spare)) in &self.pages {
            nand.program(*row, data, spare)?;
        }
        session.retain_nand(&medium.0, nand)
    }

    /// The data and spare bytes of a committed `row`.
    pub fn page(&self, row: u32) -> Option<&(Vec<u8>, Vec<u8>)> {
        self.pages.get(&row)
    }

    /// The blocks whose committed page 0 spare starts with `tag`.
    pub fn blocks_tagged(&self, tag: &[u8]) -> Vec<u32> {
        self.pages
            .iter()
            .filter(|(row, (_, spare))| *row % PAGES_PER_BLOCK == 0 && spare.starts_with(tag))
            .map(|(row, _)| row / PAGES_PER_BLOCK)
            .collect()
    }

    /// The committed image of `nand`, keeping this image's recorded inputs.
    pub fn export(&self, nand: &Nand) -> Result<Vec<u8>, String> {
        #[derive(Deserialize)]
        struct Changes {
            #[allow(dead_code)]
            identity: Value,
            pages: BTreeMap<u32, Vec<u8>>,
            written_spare: BTreeMap<u32, Vec<u8>>,
        }
        let changes: Changes =
            serde_json::to_vec(&nand.changes()).and_then(|bytes| serde_json::from_slice(&bytes)).map_err(|e| e.to_string())?;
        if !changes.pages.keys().eq(changes.written_spare.keys()) {
            return Err("NAND export requires a spare page for every written page".into());
        }
        let rows = |pages: &BTreeMap<u32, Vec<u8>>| -> Value {
            Value::Array(pages.iter().map(|(row, bytes)| Value::Array(vec![(*row).into(), encode_hex(bytes).into()])).collect())
        };
        let mut document = serde_json::Map::new();
        document.insert("schema".into(), 1.into());
        document.insert("kind".into(), KIND.into());
        document.insert(
            "geometry".into(),
            serde_json::json!({"blocks": BLOCKS, "page_bytes": PAGE_BYTES, "pages_per_block": PAGES_PER_BLOCK, "spare_bytes": SPARE_BYTES}),
        );
        document.insert("base".into(), serde_json::json!({"sha256": self.base.sha256, "size": self.base.size}));
        document.insert("data_overlays".into(), Value::Array(self.data_overlays.clone()));
        document.insert(
            "spare_pages".into(),
            Value::Array(self.spare_pages.iter().map(|(row, record)| Value::Array(vec![(*row).into(), record.clone()])).collect()),
        );
        document.insert("committed_data_pages".into(), rows(&changes.pages));
        document.insert("committed_spare_pages".into(), rows(&changes.written_spare));
        let mut json = String::new();
        canonical(&Value::Object(document), &mut json);
        gzip(json.as_bytes())
    }
}

/// `json.dumps(value, sort_keys=True, separators=(',', ':'))` for integers, strings, arrays and objects.
fn canonical(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => {
            out.push('"');
            for unit in text.encode_utf16() {
                match unit {
                    0x22 => out.push_str("\\\""),
                    0x5c => out.push_str("\\\\"),
                    0x0a => out.push_str("\\n"),
                    0x0d => out.push_str("\\r"),
                    0x09 => out.push_str("\\t"),
                    0x08 => out.push_str("\\b"),
                    0x0c => out.push_str("\\f"),
                    0x20..=0x7e => out.push(unit as u8 as char),
                    _ => out.push_str(&format!("\\u{unit:04x}")),
                }
            }
            out.push('"');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                canonical(&Value::String(key.clone()), out);
                out.push(':');
                canonical(&map[key], out);
            }
            out.push('}');
        }
    }
}

#[link(name = "z")]
unsafe extern "C" {
    fn compressBound(source_len: std::ffi::c_ulong) -> std::ffi::c_ulong;
    fn compress2(
        dest: *mut u8,
        dest_len: *mut std::ffi::c_ulong,
        source: *const u8,
        source_len: std::ffi::c_ulong,
        level: std::ffi::c_int,
    ) -> std::ffi::c_int;
}

/// `gzip.compress(data, mtime=0)`: Python's header with OS 255, zlib's level-9 deflate stream.
fn gzip(data: &[u8]) -> Result<Vec<u8>, String> {
    let length = std::ffi::c_ulong::try_from(data.len()).map_err(|_| "gzip input exceeds zlib's length")?;
    let mut wrapped = vec![0u8; unsafe { compressBound(length) } as usize];
    let mut written = wrapped.len() as std::ffi::c_ulong;
    let status = unsafe { compress2(wrapped.as_mut_ptr(), &mut written, data.as_ptr(), length, 9) };
    if status != 0 || written < 6 {
        return Err(format!("zlib compress2 failed with status {status}"));
    }
    let deflate = &wrapped[2..written as usize - 4];
    let mut crc = flate2::Crc::new();
    crc.update(data);
    let mut out = Vec::with_capacity(deflate.len() + 18);
    out.extend_from_slice(&[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 2, 0xff]);
    out.extend_from_slice(deflate);
    out.extend_from_slice(&crc.sum().to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_json_sorts_keys_and_escapes_like_python() {
        let value = serde_json::json!({"b": [1, "x"], "a": {"d": null, "c": "é\n\"/"}});
        let mut out = String::new();
        canonical(&value, &mut out);
        assert_eq!(out, r#"{"a":{"c":"\u00e9\n\"/","d":null},"b":[1,"x"]}"#);
    }

    #[test]
    fn gzip_round_trips_with_a_fixed_header() {
        let data = b"committed".repeat(100);
        let packed = gzip(&data).unwrap();
        assert_eq!(&packed[..10], &[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 2, 0xff]);
        let mut unpacked = Vec::new();
        flate2::read::GzDecoder::new(&packed[..]).read_to_end(&mut unpacked).unwrap();
        assert_eq!(unpacked, data);
    }
}
