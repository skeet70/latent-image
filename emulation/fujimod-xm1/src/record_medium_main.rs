//! Composes a committed NAND image whose record-area blocks the RAM-push
//! bridge's `--record-nand` programs, from 128 KiB block files.

use fujimod_xm1::bridge::{emit, read, sha256, write};
use fujimod_xm1::nand_image::CommittedImage;
use fujimod_xm1::record_medium::{LogicalBlock, PhysicalBlock, RecordMedium, compose_pages};
use fujimod_xm1::run::{self, RunDocument, ValidatedUpdate};
use serde_json::json;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "Usage: fujimod-xm1-record-medium --run RUN.json --installed-dat DAT \
--block LOGICAL:PHYSICAL:FILE [--block ...] --output NAND\n\
LOGICAL is a pool-relative record-area block (0x52..0x59, 0x68, 0x69), PHYSICAL a free pool block, FILE 131072 bytes.\n";

fn number(text: &str) -> Result<u32, String> {
    let digits = text.strip_prefix("0x").ok_or_else(|| format!("{text} is not 0x-prefixed hex"))?;
    u32::from_str_radix(digits, 16).map_err(|_| format!("{text} is not hex"))
}

fn drive() -> Result<(), String> {
    let mut run_path = None;
    let mut dat_path = None;
    let mut output = None;
    let mut blocks = Vec::new();
    let mut sources = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args.next().ok_or(USAGE)?;
        let slot = match flag.as_str() {
            "--run" => &mut run_path,
            "--installed-dat" => &mut dat_path,
            "--output" => &mut output,
            "--block" => {
                let mut parts = value.splitn(3, ':');
                let (Some(logical), Some(physical), Some(file)) = (parts.next(), parts.next(), parts.next()) else {
                    return Err(USAGE.into());
                };
                let logical = u16::try_from(number(logical)?).map_err(|_| "logical block exceeds 16 bits")?;
                let data = read(file.as_ref())?;
                sources.push(json!({"logical": format!("{logical:#x}"), "file": file, "sha256": sha256(&data)}));
                blocks.push((LogicalBlock(logical), PhysicalBlock(number(physical)?), data));
                continue;
            }
            _ => return Err(USAGE.into()),
        };
        if slot.replace(PathBuf::from(value)).is_some() {
            return Err(USAGE.into());
        }
    }
    let (Some(run_path), Some(dat_path), Some(output)) = (run_path, dat_path, output) else {
        return Err(USAGE.into());
    };
    if blocks.is_empty() {
        return Err(USAGE.into());
    }
    if output.exists() {
        return Err(format!("{} exists", output.display()));
    }
    let document = RunDocument::parse(&read(&run_path)?)?;
    let base = run::nand_base(&document, &ValidatedUpdate(read(&dat_path)?))?;
    let image = CommittedImage::compose(base.as_ref(), &compose_pages(&blocks)?)?;
    let medium = RecordMedium::parse(&image)?;
    write(&output, &image)?;
    emit(&json!({"output": output, "sha256": sha256(&image), "sources": sources, "medium": medium.summary()}))
}

fn main() -> ExitCode {
    match drive() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
