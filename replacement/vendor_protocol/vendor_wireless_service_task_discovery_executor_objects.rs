//! X-M1 1.50 object-operation CPU and wire transaction ownership.
//! Instruction provenance is recorded in the adjacent evidence file.

use super::executor::{OperationReply, OperationRequest, PtpError, PtpOutput, PtpPhase, PtpState, ResponseCode};
use super::packet::CommandPacket;
use super::super::interface_modes::catalog::{CatalogGeneration, CatalogRuntime, ObjectMetadata};
use crate::filesystem_catalog::Catalog;
use crate::filesystem_file_cpu::{FileCpu, FileHandle, OpenMode, SeekFrom};
use crate::filesystem_runtime::SectorIo;
use crate::media_parser::{self, MediaFormat, MediaMetadata};
use super::super::interface_modes::{InterfaceMode, InterfaceModes};

#[path = "vendor_wireless_service_task_discovery_executor_objects_memory.rs"]
mod memory;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageSelector { Default, All, Primary, Secondary }
impl StorageSelector {
    pub fn parse(raw: u32) -> Result<Self, ObjectResponse> {
        match raw { 0 => Ok(Self::Default), u32::MAX => Ok(Self::All), 0x10000001 => Ok(Self::Primary), 0x10000002 => Ok(Self::Secondary), _ => Err(ObjectResponse::InvalidStorage) }
    }
    pub const fn contains(self, volume: ObjectVolume) -> bool {
        match self { Self::Default | Self::All => true, Self::Primary => matches!(volume, ObjectVolume::Primary), Self::Secondary => matches!(volume, ObjectVolume::Secondary) }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectVolume { Primary, Secondary }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectHandle(u32);
impl ObjectHandle {
    pub const fn from_wire(value: u32) -> Self { Self(value) }
    pub const fn wire(self) -> u32 { self.0 }
    pub const fn volume(self) -> ObjectVolume { if self.0 & 0x80000000 == 0 { ObjectVolume::Primary } else { ObjectVolume::Secondary } }
    pub const fn ordinal(self) -> u16 { self.0 as u16 }
    pub const fn from_ordinal(volume: ObjectVolume, ordinal: u16) -> Self { Self((match volume { ObjectVolume::Primary => 0, ObjectVolume::Secondary => 0x80000000 }) | ordinal as u32) }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectResponse {
    Ok, GeneralError, SessionNotOpen, ParameterNotSupported, InvalidStorage,
    InvalidObject, InvalidFormat, StoreFull, ObjectWriteProtected, AccessDenied,
    StoreNotAvailable, InvalidParent, InvalidParameter, PartialUnsupported,
}
impl ObjectResponse {
    pub const fn wire(self) -> u16 {
        match self {
            Self::Ok => 0x2001, Self::GeneralError => 0x2002, Self::SessionNotOpen => 0x2003,
            Self::ParameterNotSupported => 0x2006, Self::InvalidStorage => 0x2008,
            Self::InvalidObject => 0x2009, Self::InvalidFormat => 0x200b, Self::StoreFull => 0x200c,
            Self::ObjectWriteProtected => 0x200d, Self::AccessDenied => 0x200f,
            Self::StoreNotAvailable => 0x2013, Self::InvalidParent => 0x201a,
            Self::InvalidParameter => 0x201d, Self::PartialUnsupported => 0x201f,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormatFilter { Any, Images, Exact(u32) }
impl FormatFilter {
    pub const fn from_wire(value: u32) -> Self { match value { 0 => Self::Any, u32::MAX => Self::Images, value => Self::Exact(value) } }
    pub const fn supported(self) -> bool {
        match self { Self::Any | Self::Images => true, Self::Exact(value) => matches!(value as u16, 0x3001 | 0x3002 | 0x3006 | 0x3008 | 0x300a | 0x300d | 0xb103 | 0x3800 | 0x3801 | 0x3808 | 0xb802 | 0xb003) }
    }
    pub const fn matches(self, format: u32, count_only: bool) -> bool {
        match self { Self::Any => if count_only { format as u16 != 0 } else { format != 0 }, Self::Images => format & 0x800 != 0, Self::Exact(value) => value == format as u16 as u32 }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParentFilter { Any, Root, Object(ObjectHandle) }
impl ParentFilter {
    pub const fn from_wire(value: u32) -> Self { match value { 0 => Self::Any, u32::MAX => Self::Root, value => Self::Object(ObjectHandle::from_wire(value)) } }
    pub const fn matches(self, parent: u32) -> bool { match self { Self::Any => true, Self::Root => parent == 0, Self::Object(handle) => parent == handle.ordinal() as u32 } }
}

pub fn unused_parameters_zero(parameters: &[u32; 5], first_unused: usize) -> bool {
    parameters.get(first_unused..).is_none_or(|unused| unused.iter().all(|value| *value == 0))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectOffset(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectLength(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadMode { Start, Continue, Seek }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadWindow { pub offset: ObjectOffset, pub length: ObjectLength, pub mode: ReadMode, pub reaches_end: bool }
impl ReadWindow {
    pub fn partial(size: ObjectLength, offset: ObjectOffset, requested: ObjectLength, previous_end: ObjectOffset, unsupported: bool) -> Result<Self, ObjectResponse> {
        if offset.0 >= size.0 || requested.0 == 0 { return Err(ObjectResponse::InvalidParameter); }
        if unsupported { return Err(ObjectResponse::PartialUnsupported); }
        let requested_end = offset.0.wrapping_add(requested.0);
        let length = if requested_end > size.0 { size.0 - offset.0 } else { requested.0 };
        let mode = if offset.0 == 0 { ReadMode::Start } else if offset.0 >= previous_end.0 { ReadMode::Seek } else { ReadMode::Continue };
        Ok(Self { offset, length: ObjectLength(length), mode, reaches_end: requested_end >= size.0 })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeleteFailure { MissingObject, Protected, Busy, Other }
impl DeleteFailure {
    pub const fn from_device(status: u32) -> Option<Self> {
        match status { 0 => None, 0x80000100 => Some(Self::MissingObject), 0x80000200 => Some(Self::Protected), 0x80000002 => Some(Self::Busy), _ => Some(Self::Other) }
    }
    pub const fn response(self) -> u16 { match self { Self::MissingObject => 0x2009, Self::Protected => 0x200d, Self::Busy => 0x2019, Self::Other => 0x2002 } }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectInfoHeader {
    pub storage: StorageSelector,
    pub format: u16,
    pub protection: u16,
    pub size: ObjectLength,
    pub thumbnail_format: u16,
    pub thumbnail_size: ObjectLength,
    pub thumbnail_width: u32,
    pub thumbnail_height: u32,
    pub image_width: u32,
    pub image_height: u32,
    pub bit_depth: u32,
    pub parent: ParentFilter,
    pub association_type: u16,
    pub association_description: u32,
    pub sequence: u32,
}
impl ObjectInfoHeader {
    pub const WIRE_LENGTH: usize = 52;
    pub fn decode(bytes: &[u8]) -> Result<Self, ObjectResponse> {
        if bytes.len() < Self::WIRE_LENGTH { return Err(ObjectResponse::InvalidParameter); }
        Ok(Self {
            storage: StorageSelector::parse(word(bytes, 0))?,
            format: half(bytes, 4), protection: half(bytes, 6), size: ObjectLength(word(bytes, 8)),
            thumbnail_format: half(bytes, 12), thumbnail_size: ObjectLength(word(bytes, 14)),
            thumbnail_width: word(bytes, 18), thumbnail_height: word(bytes, 22),
            image_width: word(bytes, 26), image_height: word(bytes, 30), bit_depth: word(bytes, 34),
            parent: ParentFilter::from_wire(word(bytes, 38)), association_type: half(bytes, 42),
            association_description: word(bytes, 44), sequence: word(bytes, 48),
        })
    }
    pub fn encode(self) -> [u8; Self::WIRE_LENGTH] {
        let mut bytes = [0; Self::WIRE_LENGTH];
        let storage = match self.storage { StorageSelector::Default => 0, StorageSelector::All => u32::MAX, StorageSelector::Primary => 0x10000001, StorageSelector::Secondary => 0x10000002 };
        let parent = match self.parent { ParentFilter::Any => 0, ParentFilter::Root => u32::MAX, ParentFilter::Object(handle) => handle.wire() };
        for (offset, value) in [(0, storage), (8, self.size.0), (14, self.thumbnail_size.0), (18, self.thumbnail_width), (22, self.thumbnail_height), (26, self.image_width), (30, self.image_height), (34, self.bit_depth), (38, parent), (44, self.association_description), (48, self.sequence)] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        for (offset, value) in [(4, self.format), (6, self.protection), (12, self.thumbnail_format), (42, self.association_type)] {
            bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }
}
fn word(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]) }
fn half(bytes: &[u8], offset: usize) -> u16 { u16::from_le_bytes([bytes[offset], bytes[offset + 1]]) }

#[derive(Clone, Copy)]
struct ObjectSelection { storage: StorageSelector, format: FormatFilter, parent: ParentFilter }
impl ObjectSelection {
    fn includes(self, object: &ObjectMetadata, count_only: bool) -> bool {
        self.storage.contains(object.handle.volume()) && self.format.matches(object.format.wire(), count_only && matches!(self.parent, ParentFilter::Any)) && self.parent.matches(object.parent.map_or(0, |parent| u32::from(parent.ordinal())))
    }
}
enum PendingOperation {
    Idle,
    Handles { command: CommandPacket, selection: ObjectSelection, next: usize, generation: CatalogGeneration },
    Read { command: CommandPacket, file: FileHandle, remaining: ObjectLength, total: ObjectLength },
    Response { command: CommandPacket, reply: OperationReply },
    UploadInfo { command: CommandPacket },
    MemoryRead { command: CommandPacket, mode: super::super::interface_modes::InterfaceMode, generation: super::super::interface_modes::ConfigurationGeneration, offset: u32, remaining: u32, total: u32, alternate: bool },
}
pub struct ConcreteRemoteObjectOperations {
    pending: PendingOperation,
    previous_end: ObjectOffset,
}
impl Default for ConcreteRemoteObjectOperations { fn default() -> Self { Self::new() } }
impl ConcreteRemoteObjectOperations {
    pub const fn new() -> Self { Self { pending: PendingOperation::Idle, previous_end: ObjectOffset(0) } }

    fn selection(command: CommandPacket, catalog: &CatalogRuntime<'_>) -> Result<ObjectSelection, ObjectResponse> {
        let storage = StorageSelector::parse(command.parameters[0])?;
        if matches!(storage, StorageSelector::Default) { return Err(ObjectResponse::InvalidStorage); }
        if matches!(storage, StorageSelector::Secondary) { return Err(ObjectResponse::StoreNotAvailable); }
        let format = FormatFilter::from_wire(command.parameters[1]);
        if !format.supported() { return Err(ObjectResponse::InvalidFormat); }
        let parent = ParentFilter::from_wire(command.parameters[2]);
        if let ParentFilter::Object(handle) = parent {
            let object = catalog.object(handle).ok_or(ObjectResponse::InvalidObject)?;
            if object.format.wire() as u16 != 0x3001 { return Err(ObjectResponse::InvalidParent); }
        }
        if !unused_parameters_zero(&command.parameters, 3) { return Err(ObjectResponse::ParameterNotSupported); }
        Ok(ObjectSelection { storage, format, parent })
    }

    fn queue_response(&mut self, command: CommandPacket, response: u16, parameters: &[u32]) -> OperationReply {
        let mut reply = OperationReply::new(ResponseCode(response));
        reply.parameters[..parameters.len()].copy_from_slice(parameters);
        reply.parameter_count = parameters.len() as u8;
        self.pending = PendingOperation::Response { command, reply };
        reply
    }

    pub fn execute<I: SectorIo>(&mut self, request: OperationRequest<'_>, state: &mut PtpState, output: &mut PtpOutput, catalog: &mut CatalogRuntime<'_>, fs: &mut Catalog<'_, I>, files: &mut FileCpu, scratch: &mut [u8], interfaces: &InterfaceModes, mode: InterfaceMode) -> Result<OperationReply, PtpError> {
        let command = request.command;
        if request.phase == PtpPhase::Cancel {
            self.cancel(fs, files, output)?;
            return self.respond(command, state.response.0, output);
        }
        if request.phase == PtpPhase::Data {
            if let PendingOperation::UploadInfo { command: original } = self.pending {
                if original.transaction != command.transaction || original.operation != command.operation { return Err(PtpError::InvalidState); }
                let parent = upload_parent(command, request.data).and_then(|handle| catalog.object(handle)).map(|object| UploadParent { handle: object.handle, format: object.format.wire() });
                let response = upload_info_response(command, request.data, parent);
                let reply = self.queue_response(command, response, &[]);
                output.send_response(reply, command.transaction)?;
                self.pending = PendingOperation::Idle;
                return Ok(reply);
            }
            let original = match &self.pending {
                PendingOperation::Handles { command, .. } | PendingOperation::Read { command, .. } | PendingOperation::Response { command, .. } => *command,
                _ => return Err(PtpError::InvalidState),
            };
            if original.transaction != command.transaction || original.operation != command.operation { return Err(PtpError::InvalidState); }
            return self.continue_output(output, catalog, fs, files);
        }
        if !matches!(self.pending, PendingOperation::Idle) || !output.is_empty() { return Err(PtpError::OutputFull); }
        if state.session_id.is_none() {
            let reply = self.queue_response(command, 0x2003, &[]);
            output.send_response(reply, command.transaction)?;
            self.pending = PendingOperation::Idle;
            return Ok(reply);
        }
        let response = match command.operation.0 {
            0x1006 | 0x1007 => {
                match Self::selection(command, catalog) {
                    Err(response) => response.wire(),
                    Ok(selection) => {
                        let count = catalog.objects().filter(|object| selection.includes(object, command.operation.0 == 0x1006)).count() as u32;
                        if command.operation.0 == 0x1006 {
                            let reply = self.queue_response(command, 0x2001, &[count]);
                            output.send_response(reply, command.transaction)?;
                            self.pending = PendingOperation::Idle;
                            return Ok(reply);
                        }
                        output.begin_data(command.operation, command.transaction, count.checked_add(1).and_then(|count| count.checked_mul(4)).ok_or(PtpError::InvalidLength)?)?;
                        output.write_data(&count.to_le_bytes())?;
                        self.pending = PendingOperation::Handles { command, selection, next: 0, generation: catalog.publication_generation() };
                        return Ok(OperationReply::new(ResponseCode(0x2001)));
                    }
                }
            }
            0x1008 | 0x100a => {
                let handle = ObjectHandle::from_wire(command.parameters[0]);
                let Some(object) = catalog.object(handle) else { return self.respond(command, 0x2009, output); };
                if !unused_parameters_zero(&command.parameters, 1) { return self.respond(command, 0x2006, output); }
                let entry = match fs.resolve(&object.path) { Ok(entry) => entry, Err(_) => return self.respond(command, 0x2009, output) };
                let format = media_format(object.format.wire() as u16);
                let metadata = match format {
                    Some(format) => match media_parser::metadata(files, fs, entry, format, scratch) {
                        Ok(metadata) => Some(metadata),
                        Err(_) => return self.respond(command, if command.operation.0 == 0x100a { 0x2010 } else { 0x200f }, output),
                    },
                    None => None,
                };
                if command.operation.0 == 0x100a {
                    let Some(thumbnail) = metadata.and_then(|metadata| metadata.thumbnail) else { return self.respond(command, 0x2010, output); };
                    if thumbnail.bytes == 0 { return self.respond(command, 0x2010, output); }
                    let file = match files.open(fs.io(), entry, OpenMode::Read) { Ok(file) => file, Err(_) => return self.respond(command, 0x200f, output) };
                    if files.seek(fs.io(), file, SeekFrom::Start(u64::from(thumbnail.offset))).is_err() {
                        let _ = files.close(fs.io(), file);
                        return self.respond(command, 0x200f, output);
                    }
                    return self.start_file_output(command, file, ObjectLength(thumbnail.bytes), fs, files, output);
                }
                let mut bytes = [0; 640];
                let length = encode_object_info(object, metadata.as_ref(), entry.name.units(), &mut bytes)?;
                output.send_data(command.operation, command.transaction, &bytes[..length])?;
                self.queue_response(command, 0x2001, &[]);
                return Ok(OperationReply::new(ResponseCode(0x2001)));
            }
            0x1009 | 0x101b => {
                let handle = ObjectHandle::from_wire(command.parameters[0]);
                let Some(object) = catalog.object(handle) else {
                    let reply = self.queue_response(command, 0x2009, &[]);
                    output.send_response(reply, command.transaction)?;
                    self.pending = PendingOperation::Idle;
                    return Ok(reply);
                };
                if object.format.wire() == 0 { 0x2009 }
                else if command.operation.0 == 0x101b && (command.parameters[1] >= object.size.0 as u32 || command.parameters[2] == 0) { 0x201d }
                else if handle.volume() == ObjectVolume::Secondary { 0x2013 }
                else if !unused_parameters_zero(&command.parameters, if command.operation.0 == 0x101b { 3 } else { 1 }) { 0x2006 }
                else {
                    let size = ObjectLength(object.size.0 as u32);
                    let window = if command.operation.0 == 0x101b {
                        ReadWindow::partial(size, ObjectOffset(command.parameters[1]), ObjectLength(command.parameters[2]), self.previous_end, interfaces.partial_reads_unsupported(mode))
                    } else { Ok(ReadWindow { offset: ObjectOffset(0), length: size, mode: ReadMode::Start, reaches_end: true }) };
                    match window {
                        Err(response) => response.wire(),
                        Ok(window) => {
                            let entry = match fs.resolve(&object.path) { Ok(entry) => entry, Err(_) => return self.respond(command, 0x200f, output) };
                            let file = match files.open(fs.io(), entry, OpenMode::Read) { Ok(file) => file, Err(_) => return self.respond(command, 0x200f, output) };
                            if files.seek(fs.io(), file, SeekFrom::Start(u64::from(window.offset.0))).is_err() {
                                let _ = files.close(fs.io(), file);
                                0x200f
                            } else {
                                self.previous_end = ObjectOffset(window.offset.0.wrapping_add(window.length.0));
                                return self.start_file_output(command, file, window.length, fs, files, output);
                            }
                        }
                    }
                }
            }
            0x100b => {
                let handle = ObjectHandle::from_wire(command.parameters[0]);
                if catalog.object(handle).is_some_and(|object| object.protected) { 0x200d }
                else if !unused_parameters_zero(&command.parameters, 1) { 0x2006 }
                else if command.parameters[0] != u32::MAX && catalog.object(handle).is_none() { 0x2009 }
                else { 0x2002 }
            }
            0x100c => {
                if !unused_parameters_zero(&command.parameters, 2) { 0x2006 }
                else if command.parameters[0] == 0 && command.parameters[1] != 0 { 0x201d }
                else {
                    if command.parameters[0] != 0 {
                        if StorageSelector::parse(command.parameters[0]).is_err() { return self.respond(command, 0x2008, output); }
                        if command.parameters[0] == 0x10000002 { return self.respond(command, 0x2013, output); }
                    }
                    if command.parameters[1] != 0 && command.parameters[1] != u32::MAX {
                        let parent = catalog.object(ObjectHandle::from_wire(command.parameters[1]));
                        if parent.is_none_or(|object| object.format.wire() as u16 != 0x3001) { return self.respond(command, 0x201a, output); }
                    }
                    self.pending = PendingOperation::UploadInfo { command };
                    return Ok(OperationReply::new(ResponseCode(0x2001)));
                }
            }
            0x100d => 0x2015,
            _ => return Err(PtpError::InvalidState),
        };
        self.respond(command, response, output)
    }

    fn respond(&mut self, command: CommandPacket, response: u16, output: &mut PtpOutput) -> Result<OperationReply, PtpError> {
        let reply = self.queue_response(command, response, &[]);
        output.send_response(reply, command.transaction)?;
        self.pending = PendingOperation::Idle;
        Ok(reply)
    }

    fn start_file_output<I: SectorIo>(&mut self, command: CommandPacket, file: FileHandle, total: ObjectLength, fs: &mut Catalog<'_, I>, files: &mut FileCpu, output: &mut PtpOutput) -> Result<OperationReply, PtpError> {
        let mut bytes = [0; 2036];
        let wanted = (total.0 as usize).min(bytes.len());
        let count = if wanted == 0 { 0 } else {
            match files.read(fs.io(), file, &mut bytes[..wanted]) {
                Ok(count) if count != 0 => count,
                _ => { let _ = files.close(fs.io(), file); return self.respond(command, 0x200f, output); }
            }
        };
        if let Err(error) = output.begin_data(command.operation, command.transaction, total.0) {
            let _ = files.close(fs.io(), file);
            return Err(error);
        }
        output.write_data(&bytes[..count])?;
        let remaining = ObjectLength(total.0 - count as u32);
        if remaining.0 == 0 {
            let response = if files.close(fs.io(), file).is_ok() { 0x2001 } else { 0x200f };
            if command.operation.0 == 0x101b { self.queue_response(command, response, &[total.0]); }
            else { self.queue_response(command, response, &[]); }
        } else { self.pending = PendingOperation::Read { command, file, remaining, total }; }
        Ok(OperationReply::new(ResponseCode(0x2001)))
    }

    pub fn continue_output<I: SectorIo>(&mut self, output: &mut PtpOutput, catalog: &CatalogRuntime<'_>, fs: &mut Catalog<'_, I>, files: &mut FileCpu) -> Result<OperationReply, PtpError> {
        if !output.is_empty() { return Err(PtpError::OutputFull); }
        let pending = core::mem::replace(&mut self.pending, PendingOperation::Idle);
        match pending {
            PendingOperation::Response { command, reply } => {
                output.send_response(reply, command.transaction)?;
                Ok(reply)
            }
            PendingOperation::Handles { command, selection, mut next, generation } => {
                if generation != catalog.publication_generation() {
                    output.discard();
                    return self.respond(command, 0x2002, output);
                }
                for object in catalog.objects().filter(|object| selection.includes(object, false)).skip(next) {
                    if output.write_data(&object.handle.wire().to_le_bytes())? != 4 {
                        return Err(PtpError::InvalidLength);
                    }
                    next += 1;
                    if output.queued_bytes().len() == 2048 { break; }
                }
                if output.data_remaining() == 0 { self.queue_response(command, 0x2001, &[]); }
                else { self.pending = PendingOperation::Handles { command, selection, next, generation }; }
                Ok(OperationReply::new(ResponseCode(0x2001)))
            }
            PendingOperation::Read { command, file, remaining, total } => {
                let mut buffer = [0; 2048];
                let wanted = usize::try_from(remaining.0).unwrap_or(usize::MAX).min(buffer.len());
                let count = match files.read(fs.io(), file, &mut buffer[..wanted]) {
                    Ok(count) if count != 0 || remaining.0 == 0 => count,
                    _ => {
                        let _ = files.close(fs.io(), file);
                        output.discard();
                        self.queue_response(command, 0x200f, &[]);
                        return self.continue_output(output, catalog, fs, files);
                    }
                };
                output.write_data(&buffer[..count])?;
                let left = ObjectLength(remaining.0.wrapping_sub(count as u32));
                if left.0 == 0 {
                    let closed = files.close(fs.io(), file).is_ok();
                    if command.operation.0 == 0x101b { self.queue_response(command, if closed { 0x2001 } else { 0x200f }, &[total.0]); }
                    else { self.queue_response(command, if closed { 0x2001 } else { 0x200f }, &[]); }
                } else { self.pending = PendingOperation::Read { command, file, remaining: left, total }; }
                Ok(OperationReply::new(ResponseCode(0x2001)))
            }
            other => { self.pending = other; Err(PtpError::InvalidState) }
        }
    }

    pub fn cancel<I: SectorIo>(&mut self, fs: &mut Catalog<'_, I>, files: &mut FileCpu, output: &mut PtpOutput) -> Result<(), PtpError> {
        let pending = core::mem::replace(&mut self.pending, PendingOperation::Idle);
        output.discard();
        if let PendingOperation::Read { file, .. } = pending { files.close(fs.io(), file).map_err(|_| PtpError::InvalidState)?; }
        self.previous_end = ObjectOffset(0);
        Ok(())
    }
}

#[cfg(test)]
#[path = "vendor_wireless_service_task_discovery_executor_objects_tests.rs"]
mod tests;

fn media_format(format: u16) -> Option<MediaFormat> {
    match format { 0x3801 => Some(MediaFormat::Jpeg), 0x3808 => Some(MediaFormat::ExifJpeg), 0x3800 => Some(MediaFormat::Raf), 0xb103 => Some(MediaFormat::FujiRaf), 0x300d => Some(MediaFormat::Movie), _ => None }
}

fn encode_object_info(object: &ObjectMetadata, media: Option<&MediaMetadata>, name: &[u16], bytes: &mut [u8; 640]) -> Result<usize, PtpError> {
    if name.len() >= 255 { return Err(PtpError::InvalidLength); }
    let header = ObjectInfoHeader {
        storage: match object.handle.volume() { ObjectVolume::Primary => StorageSelector::Primary, ObjectVolume::Secondary => StorageSelector::Secondary },
        format: object.format.wire() as u16, protection: u16::from(object.protected), size: ObjectLength(object.size.0 as u32),
        thumbnail_format: if media.and_then(|media| media.thumbnail).is_some() { 0x3808 } else { 0 },
        thumbnail_size: ObjectLength(media.and_then(|media| media.thumbnail).map_or(0, |thumbnail| thumbnail.bytes)),
        thumbnail_width: media.map_or(0, |media| media.primary_width),
        thumbnail_height: media.map_or(0, |media| media.primary_height),
        image_width: media.map_or(0, |media| media.secondary_width),
        image_height: media.map_or(0, |media| media.secondary_height),
        bit_depth: media.map_or(0, |media| media.bit_depth),
        parent: object.parent.map_or(ParentFilter::Any, ParentFilter::Object),
        association_type: if object.format.wire() as u16 == 0x3001 { if object.format.wire() == 0x80003001 { 7 } else { 1 } } else { 0 },
        association_description: 0, sequence: 0,
    };
    bytes[..52].copy_from_slice(&header.encode());
    let mut position = 52;
    bytes[position] = (name.len() + 1) as u8;
    position += 1;
    for unit in name.iter().copied().chain(core::iter::once(0)) {
        bytes[position..position + 2].copy_from_slice(&unit.to_le_bytes());
        position += 2;
    }
    bytes[position] = 16;
    position += 1;
    let packed = object.created.0;
    let year = 1980 + (packed >> 25);
    let month = (packed >> 21) & 15;
    let day = (packed >> 16) & 31;
    let hour = (packed >> 11) & 31;
    let minute = (packed >> 5) & 63;
    let second = (packed & 31) * 2;
    let digits = [
        (year / 1000) % 10, (year / 100) % 10, (year / 10) % 10, year % 10,
        month / 10, month % 10, day / 10, day % 10,
        36, hour / 10, hour % 10, minute / 10, minute % 10, second / 10, second % 10,
    ];
    for digit in digits {
        bytes[position..position + 2].copy_from_slice(&(u16::from(b'0') + digit as u16).to_le_bytes());
        position += 2;
    }
    bytes[position..position + 4].fill(0);
    Ok(position + 4)
}

#[derive(Clone, Copy)]
struct UploadParent { handle: ObjectHandle, format: u32 }
fn upload_parent(command: CommandPacket, bytes: &[u8]) -> Option<ObjectHandle> {
    let raw = if command.parameters[0] != 0 && command.parameters[1] != 0 { command.parameters[1] } else if bytes.len() >= 42 { word(bytes, 38) } else { 0 };
    if raw == 0 || raw == u32::MAX { None } else { Some(ObjectHandle::from_wire(raw)) }
}
fn upload_info_response(command: CommandPacket, bytes: &[u8], parent: Option<UploadParent>) -> u16 {
    if bytes.len() < 53 { return 0x201d; }
    let storage = if command.parameters[0] != 0 { command.parameters[0] } else { word(bytes, 0) };
    if StorageSelector::parse(storage).is_err() { return 0x2008; }
    let format = half(bytes, 4);
    if !matches!(format, 0xb003 | 0xb802 | 0x3002) { return 0x200e; }
    if storage == 0x10000002 { return 0x2013; }
    if let Some(handle) = upload_parent(command, bytes) {
        let Some(parent) = parent else { return 0x201a; };
        if parent.handle != handle || parent.format as u16 != 0x3001 { return 0x201a; }
        if storage != 0 && (storage == 0x10000001) != (handle.volume() == ObjectVolume::Primary) { return 0x201a; }
    }
    let units = usize::from(bytes[52]);
    if units == 0 || units + 1 > 28 || bytes.len() < 53 + units * 2 { return 0x2014; }
    0x2020
}
