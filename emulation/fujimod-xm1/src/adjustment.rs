//! The modeled adjustment block set of a retained-engine committed NAND image
//! (`emulator/nand_image.py`): every page of physical blocks 0x381..0x3a1.

use crate::run::NodeName;
use fujimod_emulator::session::Session;
use serde::Deserialize;
use std::{collections::BTreeMap, io::Read};

const PAGE_BYTES: usize = 2048;
const SPARE_BYTES: usize = 512;
const PAGES_PER_BLOCK: u32 = 64;
const FIRST_BLOCK: u32 = 0x381;
const END_BLOCK: u32 = 0x3a2;

#[derive(Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Geometry {
    page_bytes: usize,
    spare_bytes: usize,
    pages_per_block: u32,
    blocks: u32,
}

#[derive(Deserialize)]
enum ImageKind {
    #[serde(rename = "type1-nand-committed-image")]
    Type1Committed,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct CommittedImage {
    schema: u32,
    kind: ImageKind,
    geometry: Geometry,
    base: serde_json::Value,
    data_overlays: serde_json::Value,
    spare_pages: serde_json::Value,
    committed_data_pages: Vec<(u32, String)>,
    committed_spare_pages: Vec<(u32, String)>,
}

/// Data and spare bytes by NAND row.
pub struct AdjustmentNand(BTreeMap<u32, (Vec<u8>, Vec<u8>)>);

fn hex(text: &str, bytes: usize) -> Result<Vec<u8>, String> {
    if text.len() != 2 * bytes {
        return Err("adjustment page has the wrong length".into());
    }
    (0..bytes)
        .map(|index| u8::from_str_radix(&text[2 * index..2 * index + 2], 16).map_err(|_| "adjustment page is not hex".into()))
        .collect()
}

impl AdjustmentNand {
    pub fn parse(gzip: &[u8]) -> Result<Self, String> {
        let mut json = Vec::new();
        flate2::read::GzDecoder::new(gzip).read_to_end(&mut json).map_err(|e| format!("adjustment NAND: {e}"))?;
        let image: CommittedImage = serde_json::from_slice(&json).map_err(|e| format!("adjustment NAND: {e}"))?;
        let geometry = Geometry { page_bytes: PAGE_BYTES, spare_bytes: SPARE_BYTES, pages_per_block: PAGES_PER_BLOCK, blocks: 1024 };
        if image.schema != 1 || image.geometry != geometry {
            return Err("adjustment NAND is not a schema-1 type1 committed image".into());
        }
        let rows = FIRST_BLOCK * PAGES_PER_BLOCK..END_BLOCK * PAGES_PER_BLOCK;
        let data: BTreeMap<u32, &String> =
            image.committed_data_pages.iter().filter(|(row, _)| rows.contains(row)).map(|(row, text)| (*row, text)).collect();
        let spare: BTreeMap<u32, &String> =
            image.committed_spare_pages.iter().filter(|(row, _)| rows.contains(row)).map(|(row, text)| (*row, text)).collect();
        if data.len() != rows.len() || spare.len() != rows.len() {
            return Err("adjustment NAND must commit every page of blocks 0x381..0x3a1".into());
        }
        let pages = rows
            .map(|row| Ok((row, (hex(data[&row], PAGE_BYTES)?, hex(spare[&row], SPARE_BYTES)?))))
            .collect::<Result<_, String>>()?;
        Ok(Self(pages))
    }

    /// Erases the adjustment blocks of the unexecuted session's NAND `medium` and programs the pages.
    pub fn apply(&self, session: &mut Session, medium: &NodeName) -> Result<(), String> {
        let mut nand = session.nand_medium(&medium.0)?;
        for block in FIRST_BLOCK..END_BLOCK {
            nand.erase(block * PAGES_PER_BLOCK)?;
        }
        for (row, (data, spare)) in &self.0 {
            nand.program(*row, data, spare)?;
        }
        session.retain_nand(&medium.0, nand)
    }
}
