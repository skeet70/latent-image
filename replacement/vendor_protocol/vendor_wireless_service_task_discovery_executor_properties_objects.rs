use super::schema::{ObjectFormatId, ObjectPropertyId};
use super::super::executor_objects::ObjectHandle;
use super::super::super::interface_modes::catalog::{ObjectMetadata, ObjectFormat};
use crate::filesystem_catalog::{Catalog, CatalogError};
use crate::filesystem_file_cpu::FileCpu;
use crate::filesystem_runtime::SectorIo;
use crate::media_parser::{self, MediaError, MediaFormat, MediaMetadata};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectPropertyFailure { InvalidObject, UnsupportedProperty, Busy, InvalidPath, Filesystem(CatalogError), Media(MediaError), OutputCapacity }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectIdentity(pub [u8; 16]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimestampPrecision { Minutes, Seconds }
#[derive(Clone, Copy)]
struct CachedMetadata { handle: ObjectHandle, metadata: MediaMetadata }
pub struct ObjectPropertyCpu { cached: Option<CachedMetadata>, identity: Option<(ObjectHandle, ObjectIdentity)> }
impl ObjectPropertyCpu {
    pub const fn new() -> Self { Self { cached: None, identity: None } }
    pub fn invalidate(&mut self, handle: Option<ObjectHandle>) {
        if handle.is_none() || self.cached.as_ref().is_some_and(|entry| Some(entry.handle) == handle) {
            self.cached = None;
            self.identity = None;
        }
    }
    pub fn metadata<I: SectorIo>(&mut self, object: &ObjectMetadata, fs: &mut Catalog<I>, files: &mut FileCpu, scratch: &mut [u8]) -> Result<MediaMetadata, ObjectPropertyFailure> {
        if let Some(entry) = self.cached { if entry.handle == object.handle { return Ok(entry.metadata); } }
        let format = MediaFormat::from_ptp(object.format.wire() as u16).map_err(ObjectPropertyFailure::Media)?;
        let entry = fs.resolve(&object.path).map_err(ObjectPropertyFailure::Filesystem)?;
        let metadata = media_parser::metadata(files, fs, entry, format, scratch).map_err(ObjectPropertyFailure::Media)?;
        self.cached = Some(CachedMetadata { handle: object.handle, metadata });
        self.identity = None;
        Ok(metadata)
    }
    pub fn read<I: SectorIo>(&mut self, object: &ObjectMetadata, property: ObjectPropertyId, precision: TimestampPrecision, fs: &mut Catalog<I>, files: &mut FileCpu, scratch: &mut [u8], output: &mut [u8]) -> Result<usize, ObjectPropertyFailure> {
        let format = ObjectFormatId::parse(object.format.wire() as u16).ok_or(ObjectPropertyFailure::UnsupportedProperty)?;
        if !format.properties().contains(&property.wire()) { return Err(ObjectPropertyFailure::UnsupportedProperty); }
        use ObjectPropertyId::*;
        match property {
            Storage => write(output, &(0x10000001u32 + u32::from(object.handle.wire() >> 31 != 0)).to_le_bytes()),
            Format => write(output, &(object.format.wire() as u16).to_le_bytes()),
            Protection => write(output, &u16::from(object.protected).to_le_bytes()),
            Size => write(output, &(object.size.0 as u32 as u64).to_le_bytes()),
            Parent => write(output, &object.parent.map_or(0, |parent| u32::from(parent.ordinal())).to_le_bytes()),
            Name | DisplayName => {
                let mut path = [0; 28];
                let length = stock_path(object, &mut path)?;
                let start = path[..length].iter().rposition(|byte| *byte == b'\\').map_or(0, |index| index + 1);
                string(output, &path[start..length], 1)
            }
            Modified => {
                let timestamp = timestamp(object, precision);
                let mut bytes = [0; 15];
                bytes[..8].copy_from_slice(&timestamp[..8]);
                bytes[8] = b'T';
                bytes[9..].copy_from_slice(&timestamp[8..]);
                string(output, &bytes, 2)
            }
            PersistentId => {
                if let Some((handle, identity)) = self.identity { if handle == object.handle { return write(output, &identity.0); } }
                let mut path = [0; 28];
                let length = stock_path(object, &mut path)?;
                let timestamp = timestamp(object, precision);
                let identity = identity_from_parts(&path[..length], &timestamp);
                self.identity = Some((object.handle, identity));
                write(output, &identity.0)
            }
            DcD3 => write(output, &0u32.to_le_bytes()),
            _ => {
                let metadata = self.metadata(object, fs, files, scratch)?;
                let value = match property {
                    Dc87 => metadata.secondary_width, Dc88 => metadata.secondary_height,
                    Dc89 => metadata.average_size, De93 => metadata.duration,
                    De94 => return write(output, &metadata.sample_format.to_le_bytes()),
                    De97 => return write(output, &(match object.format { ObjectFormat::Movie => metadata.sample_count, _ => metadata.components as u16 }).to_le_bytes()),
                    De9a => metadata.sample_rate,
                    De9b => match object.format { ObjectFormat::Movie => metadata.reserved, _ => u32::from(metadata.orientation) },
                    De9c => metadata.stream_flags,
                    _ => return Err(ObjectPropertyFailure::UnsupportedProperty),
                };
                write(output, &value.to_le_bytes())
            }
        }
    }
}
impl Default for ObjectPropertyCpu { fn default() -> Self { Self::new() } }

pub fn identity_from_parts(path: &[u8], timestamp: &[u8; 14]) -> ObjectIdentity {
    let mut input = [0; 47];
    let mut at = 0;
    for part in [path.get(..path.len().min(28)).unwrap_or(path), &path[..path.len().min(4)], timestamp.as_slice()] {
        let length = part.iter().position(|byte| *byte == 0).unwrap_or(part.len());
        input[at..at + length].copy_from_slice(&part[..length]);
        at += length;
    }
    ObjectIdentity(crate::wireless_association::station::crypto::legacy::md5(&[&input]))
}
fn timestamp(object: &ObjectMetadata, precision: TimestampPrecision) -> [u8; 14] {
    let mut value = object.modified.digits();
    if precision == TimestampPrecision::Minutes { value[12..].fill(0); }
    value
}
fn stock_path(object: &ObjectMetadata, output: &mut [u8; 28]) -> Result<usize, ObjectPropertyFailure> {
    let units = object.path.units();
    if units.len() + 1 >= output.len() { return Err(ObjectPropertyFailure::InvalidPath); }
    output[0] = b'\\';
    for (target, unit) in output[1..].iter_mut().zip(units) {
        *target = match *unit { 47 => b'\\', value if value <= 255 => value as u8, _ => return Err(ObjectPropertyFailure::InvalidPath) };
    }
    Ok(units.len() + 1)
}
fn write(output: &mut [u8], bytes: &[u8]) -> Result<usize, ObjectPropertyFailure> {
    output.get_mut(..bytes.len()).ok_or(ObjectPropertyFailure::OutputCapacity)?.copy_from_slice(bytes);
    Ok(bytes.len())
}
fn string(output: &mut [u8], bytes: &[u8], terminators: usize) -> Result<usize, ObjectPropertyFailure> {
    let count = bytes.len() + terminators;
    let length = 1 + count * 2;
    let output = output.get_mut(..length).ok_or(ObjectPropertyFailure::OutputCapacity)?;
    output.fill(0);
    output[0] = count as u8;
    for (target, byte) in output[1..].chunks_exact_mut(2).zip(bytes) { target[0] = *byte; }
    Ok(length)
}
