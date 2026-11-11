//! Shared byte and sector-backed media metadata parsing for X-M1 1.50.

use crate::filesystem_catalog::{Catalog, Entry};
use crate::filesystem_file_cpu::{FileCpu, FileError, FileHandle, OpenMode, SeekFrom};
use crate::filesystem_runtime::SectorIo;

#[path = "media_parser_jpeg.rs"]
mod jpeg;
#[path = "media_parser_raf.rs"]
mod raf;
#[path = "media_parser_movie.rs"]
mod movie;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaFormat { Jpeg, ExifJpeg, Raf, FujiRaf, Movie }
impl MediaFormat {
    pub fn from_ptp(value: u16) -> Result<Self, MediaError> {
        match value {
            0x3801 => Ok(Self::Jpeg), 0x3808 => Ok(Self::ExifJpeg),
            0x3800 => Ok(Self::Raf), 0xb103 => Ok(Self::FujiRaf),
            0x300d => Ok(Self::Movie), _ => Err(MediaError::UnsupportedFormat),
        }
    }
    pub const fn ptp(self) -> u16 {
        match self { Self::Jpeg => 0x3801, Self::ExifJpeg => 0x3808,
            Self::Raf => 0x3800, Self::FujiRaf => 0xb103, Self::Movie => 0x300d }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaError {
    UnsupportedFormat, InvalidSignature, Malformed, Truncated, MissingDimensions,
    SizeOverflow, ScratchTooSmall, File(FileError),
}
impl From<FileError> for MediaError { fn from(error: FileError) -> Self { Self::File(error) } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThumbRange { pub offset: u32, pub bytes: u32 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageDimensions {
    pub width: u32, pub height: u32, pub precision: u8, pub components: u8,
    pub sampling: u8,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MediaMetadata {
    pub format: MediaFormat,
    pub primary_width: u32, pub primary_height: u32,
    pub secondary_width: u32, pub secondary_height: u32,
    pub image_width: u32, pub image_height: u32, pub orientation: u16,
    pub data_size: u32, pub secondary_size: u32,
    pub thumbnail: Option<ThumbRange>, pub bit_depth: u32, pub components: u32,
    pub embedded_image: Option<ThumbRange>,
    pub duration: u32, pub average_size: u32, pub sample_count: u16,
    pub sample_format: u16, pub sample_rate: u32, pub reserved: u32, pub stream_flags: u32,
}
impl MediaMetadata {
    fn image(format: MediaFormat, dimensions: ImageDimensions) -> Self {
        Self { format, primary_width: 0, primary_height: 0,
            secondary_width: 0, secondary_height: 0, image_width: dimensions.width,
            image_height: dimensions.height, orientation: 0, data_size: 0, secondary_size: 0,
            thumbnail: None, embedded_image: None, bit_depth: u32::from(dimensions.precision) * u32::from(dimensions.components),
            components: u32::from(dimensions.components) * u32::from(dimensions.sampling),
            duration: 0, average_size: 0, sample_count: 0, sample_format: 0,
            sample_rate: 0, reserved: 0, stream_flags: 0 }
    }
}

trait Source {
    fn len(&self) -> u32;
    fn read(&mut self, offset: u32, output: &mut [u8]) -> Result<(), MediaError>;
}
struct Bytes<'a>(&'a [u8]);
impl Source for Bytes<'_> {
    fn len(&self) -> u32 { self.0.len() as u32 }
    fn read(&mut self, offset: u32, output: &mut [u8]) -> Result<(), MediaError> {
        let end = (offset as usize).checked_add(output.len()).ok_or(MediaError::SizeOverflow)?;
        output.copy_from_slice(self.0.get(offset as usize..end).ok_or(MediaError::Truncated)?);
        Ok(())
    }
}
fn bytes<const N: usize>(source: &mut impl Source, at: u32) -> Result<[u8; N], MediaError> {
    let mut output = [0; N]; source.read(at, &mut output)?; Ok(output)
}
fn add(left: u32, right: u32) -> Result<u32, MediaError> {
    left.checked_add(right).ok_or(MediaError::SizeOverflow)
}
fn range(source: &impl Source, start: u32, size: u32) -> Result<u32, MediaError> {
    let end = add(start, size)?;
    if end > source.len() { return Err(MediaError::Truncated); }
    Ok(end)
}
fn be32(source: &mut impl Source, at: u32) -> Result<u32, MediaError> {
    Ok(u32::from_be_bytes(bytes(source, at)?))
}
fn parse_source(source: &mut impl Source, format: MediaFormat) -> Result<MediaMetadata, MediaError> {
    let length = source.len();
    match format {
        MediaFormat::Jpeg | MediaFormat::ExifJpeg => jpeg::parse(source, 0, length, format),
        MediaFormat::Raf | MediaFormat::FujiRaf => raf::parse(source, format),
        MediaFormat::Movie => movie::parse(source),
    }
}
pub fn parse(input: &[u8], format: MediaFormat) -> Result<MediaMetadata, MediaError> {
    u32::try_from(input.len()).map_err(|_| MediaError::SizeOverflow)?;
    parse_source(&mut Bytes(input), format)
}
pub fn parse_thumbnail(input: &[u8]) -> Result<ImageDimensions, MediaError> {
    let length = u32::try_from(input.len()).map_err(|_| MediaError::SizeOverflow)?;
    jpeg::dimensions(&mut Bytes(input), 0, length, false)
}

struct FileSource<'a, I> {
    files: &'a mut FileCpu, io: &'a mut I, handle: FileHandle, length: u32,
    scratch: &'a mut [u8], window_start: u32, window_bytes: usize,
}
impl<I: SectorIo> Source for FileSource<'_, I> {
    fn len(&self) -> u32 { self.length }
    fn read(&mut self, offset: u32, output: &mut [u8]) -> Result<(), MediaError> {
        let end = range(self, offset, u32::try_from(output.len()).map_err(|_| MediaError::SizeOverflow)?)?;
        let mut at = offset;
        while at < end {
            if at < self.window_start || u64::from(at) >= u64::from(self.window_start) + self.window_bytes as u64 {
                self.files.seek(self.io, self.handle, SeekFrom::Start(u64::from(at)))?;
                self.window_start = at;
                self.window_bytes = self.files.read(self.io, self.handle, self.scratch)?;
                if self.window_bytes == 0 { return Err(MediaError::Truncated); }
            }
            let start = (at - self.window_start) as usize;
            let count = (end - at) as usize;
            let count = count.min(self.window_bytes - start);
            let destination = (at - offset) as usize;
            output[destination..destination + count].copy_from_slice(&self.scratch[start..start + count]);
            at += count as u32;
        }
        Ok(())
    }
}
pub fn metadata<I: SectorIo>(files: &mut FileCpu, catalog: &mut Catalog<I>, entry: Entry,
    format: MediaFormat, scratch: &mut [u8]) -> Result<MediaMetadata, MediaError> {
    if scratch.len() < 32 { return Err(MediaError::ScratchTooSmall); }
    let handle = files.open(catalog.io(), entry, OpenMode::Read)?;
    let result = (|| {
        let length = u32::try_from(files.stat(handle)?.bytes).map_err(|_| MediaError::SizeOverflow)?;
        let mut source = FileSource { files, io: catalog.io(), handle, length,
            scratch, window_start: 0, window_bytes: 0 };
        parse_source(&mut source, format)
    })();
    let closed = files.close(catalog.io(), handle);
    match result { Err(error) => Err(error), Ok(value) => { closed?; Ok(value) } }
}

#[cfg(test)]
#[path = "media_parser_tests.rs"]
mod tests;
