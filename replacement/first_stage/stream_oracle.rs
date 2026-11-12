mod format;
mod images;
mod media;
mod engine;

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAX_BYTES: usize = 16 * 1024 * 1024;

struct OutputCapacity(u32);

impl OutputCapacity {
    fn parse(value: &str) -> Option<Self> {
        value.parse::<u32>().ok().filter(|&bytes| bytes > 0 && bytes as usize <= MAX_BYTES).map(Self)
    }
}

#[derive(Debug)]
enum Failure {
    Io(std::io::Error),
    InputBounds,
    Capacity,
    Header(images::ImageError),
    Decode(images::DecodeError),
}

impl Failure {
    fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "io",
            Self::InputBounds => "input-bounds",
            Self::Capacity => "output-capacity",
            Self::Header(_) => "header",
            Self::Decode(images::DecodeError::ZeroToken) => "zero-token",
            Self::Decode(images::DecodeError::InvalidMatch) => "invalid-match",
            Self::Decode(images::DecodeError::OutputLimit) => "output-limit",
            Self::Decode(images::DecodeError::Truncated) => "truncated",
            Self::Decode(images::DecodeError::OutputLength) => "output-length",
        }
    }
}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self { Self::Io(error) }
}

fn decode(record: &[u8], capacity: OutputCapacity, output_path: &Path) -> Result<usize, Failure> {
    let stored = u32::try_from(record.len()).map_err(|_| Failure::InputBounds)?;
    let header = images::CompressedHeader::parse(record, stored).map_err(Failure::Header)?;
    if header.record_length as usize != record.len() {
        return Err(Failure::InputBounds);
    }
    if header.decoded_length > capacity.0 {
        return Err(Failure::Capacity);
    }
    let mut output = vec![0; header.decoded_length as usize];
    let mut decoder = images::Method4::default();
    let status = record.chunks(format::UNIT_BYTES as usize).enumerate().try_for_each(|(index, chunk)| {
        decoder.feed(&chunk[if index == 0 { 20 } else { 0 }..], &mut output)
    }).and_then(|_| decoder.finish(header.decoded_length as usize));
    let mut file = OpenOptions::new().write(true).create_new(true).open(output_path)?;
    file.write_all(&output)?;
    status.map_err(Failure::Decode)?;
    Ok(output.len())
}

fn input(path: &Path) -> Result<Vec<u8>, Failure> {
    let file = File::open(path)?;
    let size = usize::try_from(file.metadata()?.len()).map_err(|_| Failure::InputBounds)?;
    if size < 20 || size > MAX_BYTES {
        return Err(Failure::InputBounds);
    }
    let mut bytes = Vec::with_capacity(size);
    file.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BYTES {
        return Err(Failure::InputBounds);
    }
    Ok(bytes)
}

fn main() {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let [record_path, capacity, output_path] = arguments.as_slice() else {
        eprintln!("usage: media-stream-model RECORD OUTPUT_CAPACITY OUTPUT");
        std::process::exit(2);
    };
    let Some(capacity) = capacity.to_str().and_then(OutputCapacity::parse) else {
        eprintln!("invalid output capacity");
        std::process::exit(2);
    };
    let record = input(&PathBuf::from(record_path));
    let record_bytes = record.as_ref().map_or(0, Vec::len);
    let status = record.and_then(|record| decode(&record, capacity, &PathBuf::from(output_path)));
    match status {
        Ok(decoded) => println!("{{\"status\":\"decoded\",\"record_bytes\":{record_bytes},\"decoded_length\":{decoded},\"failure_code\":null}}"),
        Err(error) => {
            println!("{{\"status\":\"refused\",\"record_bytes\":{record_bytes},\"decoded_length\":null,\"failure_code\":\"{}\"}}", error.code());
            eprintln!("{error:?}");
            std::process::exit(1);
        }
    }
}
