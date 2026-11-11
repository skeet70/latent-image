//! X-M1 1.50 command roots from x70view pin 55436fc85c0560dd252b0410af6f2427fe198cd3.
//! The provider entry is 0xd806c8; the ramp generator is 0xd80844.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteCount(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebugAddress(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PayloadSelector(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigNamespace { Cached, Encoded }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendStatus { Complete, Pending, Unavailable, Invalid, Failed }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebugQuery {
    pub status: BackendStatus,
    pub actual: ByteCount,
    pub overflow: u32,
}

/// Memory requests are restricted to caller-admitted regions.
/// `Complete` requires the operation to have executed.
pub trait Backend {
    fn configuration_capacity(&self) -> u32;
    fn identity_byte(&self, index: u32) -> Option<u8>;
    fn config_read(&mut self, namespace: ConfigNamespace, offset: u32, output: &mut [u8]) -> BackendStatus;
    fn config_write(&mut self, namespace: ConfigNamespace, offset: u32, input: &[u8]) -> BackendStatus;
    fn debug_query(&mut self, output: &mut [u8]) -> DebugQuery;
    fn debug_read(&mut self, address: DebugAddress, output: &mut [u8]) -> BackendStatus;
    fn debug_write(&mut self, address: DebugAddress, input: &[u8]) -> BackendStatus;
    fn debug_execute(&mut self, input: &[u8]) -> BackendStatus;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Command {
    DeviceInfo = 0x150000,
    Acknowledge = 0x1a0000,
    UsbSpeed = 0x1a0001,
    Configuration = 0x1a0002,
    Debug = 0x200001,
    Continuation = 0x3fffff,
    ProviderPayload = 0x450006,
    GeneratedPayload = 0x550001,
    Capabilities = 0x580000,
    Upload = 0x660002,
    PrepareUpload = 0x760001,
}

impl Command {
    pub fn parse(value: u32) -> Option<Self> {
        Some(match value {
            0x150000 => Self::DeviceInfo, 0x1a0000 => Self::Acknowledge,
            0x1a0001 => Self::UsbSpeed, 0x1a0002 => Self::Configuration,
            0x200001 => Self::Debug, 0x3fffff => Self::Continuation,
            0x450006 => Self::ProviderPayload, 0x550001 => Self::GeneratedPayload,
            0x580000 => Self::Capabilities, 0x660002 => Self::Upload,
            0x760001 => Self::PrepareUpload, _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandError {
    RequestTooLarge,
    UsbStatusUnavailable,
    PayloadBufferTooSmall { required: ByteCount },
    IdentityUnavailable { index: u32 },
    ProviderPayloadRequired,
    NoPendingOperation,
    UploadLengthMismatch { expected: ByteCount, received: ByteCount },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PayloadRegion { CallerBuffer, Capabilities }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferEffect {
    None,
    Upload { length: ByteCount },
    Download { region: PayloadRegion, length: ByteCount },
    ProviderRequest { selector: PayloadSelector },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Dispatch {
    /// `None` preserves the preceding available length for opcode 0x3fffff.
    pub reply_length: Option<usize>,
    pub ready: bool,
    pub continuation: bool,
    pub transfer: TransferEffect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceState { Normal, Processing, Success, Failed { code: u32 } }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsbLinkStatus { Unavailable, Bit14Clear, Bit14Set }

#[derive(Clone, Copy, Eq, PartialEq)]
enum PendingOperation { None, Backend, Configuration, Provider, Upload }

pub struct Commands {
    pub service_state: ServiceState,
    pub usb_status: UsbLinkStatus,
    service_failure_code: ServiceFailureCode,
    saved_payload_length: ByteCount,
    upload_length: ByteCount,
    uploaded_length: ByteCount,
    upload_command: Option<Command>,
    pending: PendingOperation,
}

impl Default for Commands {
    fn default() -> Self { Self::new() }
}

impl Commands {
    pub const fn new() -> Self {
        Self { service_state: ServiceState::Normal, usb_status: UsbLinkStatus::Unavailable,
            service_failure_code: ServiceFailureCode(0),
            saved_payload_length: ByteCount(0), upload_length: ByteCount(0), uploaded_length: ByteCount(0),
            upload_command: None, pending: PendingOperation::None }
    }

    pub const fn saved_payload_length(&self) -> ByteCount { self.saved_payload_length }
    pub const fn upload_length(&self) -> ByteCount { self.upload_length }
    pub const fn uploaded_length(&self) -> ByteCount { self.uploaded_length }

    /// Stock 0x00d7fd64: clears 0x01469cc0, 0x01469ca0, 0x01469cac, 0x01469cb0 and 0x01469cbc.
    pub fn open(&mut self) {
        self.pending = PendingOperation::None;
        self.upload_command = None;
        self.service_state = ServiceState::Normal;
        self.saved_payload_length = ByteCount(0);
        self.service_failure_code = ServiceFailureCode(0);
    }

    /// Stock 0x00d7f2cc zeroes the upload length and progress words at 0x01469888 and 0x0146988c.
    pub fn clear_upload(&mut self) {
        self.upload_length = ByteCount(0);
        self.uploaded_length = ByteCount(0);
    }

    pub fn dispatch<B: Backend>(
        &mut self, request: &mut [u8; 512], received: usize,
        reply: &mut [u8; 512], payload: &mut [u8], backend: &mut B,
    ) -> Result<Dispatch, CommandError> {
        if received > request.len() { return Err(CommandError::RequestTooLarge); }
        let opcode = load(request, 0);
        reply.fill(0);
        store(reply, 0, opcode);
        request[received..].fill(0);
        self.pending = PendingOperation::None;
        self.upload_command = None;
        let mut result = Dispatch { reply_length: Some(32), ready: true,
            continuation: false, transfer: TransferEffect::None };
        if received < 32 { status(reply, 9, 1); return Ok(result); }
        let Some(command) = Command::parse(opcode) else {
            status(reply, 0xffff, 0); return Ok(result);
        };
        match command {
            Command::Continuation => {
                result.reply_length = None;
                result.continuation = true;
            }
            Command::Acknowledge => {}
            Command::UsbSpeed => reply[20] = match self.usb_status {
                UsbLinkStatus::Bit14Set => 2, UsbLinkStatus::Bit14Clear => 1,
                UsbLinkStatus::Unavailable => return Err(CommandError::UsbStatusUnavailable),
            },
            Command::DeviceInfo => {
                result.reply_length = Some(512);
                self.device_info(load(request, 16), reply, backend)?;
            }
            Command::Configuration => {
                result.reply_length = Some(0x124);
                self.configuration(request, reply, backend);
            }
            Command::Debug => {
                result.reply_length = Some(0x11c);
                self.debug(request, reply, backend);
            }
            Command::Upload | Command::PrepareUpload => {
                let length = ByteCount(load(request, 20));
                self.upload_length = length;
                if payload.is_empty() { status(reply, 1, 0xffff); }
                else if length.0 as usize > payload.len() { status(reply, 9, 3); }
                else {
                    self.upload_command = Some(command);
                    self.uploaded_length = ByteCount(0);
                    result.transfer = TransferEffect::Upload { length };
                    if command == Command::PrepareUpload {
                        match load(request, 24) {
                            1..=255 => {
                                let end = payload.len() & !3;
                                payload[..end].fill(0xff);
                            }
                            256 => self.saved_payload_length = length,
                            _ => {}
                        }
                    }
                }
            }
            Command::GeneratedPayload => {
                let length = match load(request, 16) {
                    0 => {
                        if payload.len() < 0x10000 {
                            return Err(CommandError::PayloadBufferTooSmall { required: ByteCount(0x10000) });
                        }
                        for (index, byte) in payload[..0x10000].iter_mut().enumerate() { *byte = index as u8; }
                        ByteCount(0x10000)
                    }
                    256 => self.saved_payload_length,
                    _ => { status(reply, 9, 3); ByteCount(0) }
                };
                if length.0 as usize > payload.len() {
                    return Err(CommandError::PayloadBufferTooSmall { required: length });
                }
                store(reply, 20, length.0);
                result.transfer = TransferEffect::Download { region: PayloadRegion::CallerBuffer, length };
            }
            Command::Capabilities => {
                store(reply, 20, 48);
                result.transfer = TransferEffect::Download { region: PayloadRegion::Capabilities, length: ByteCount(48) };
            }
            Command::ProviderPayload => {
                self.pending = PendingOperation::Provider;
                result.transfer = TransferEffect::ProviderRequest { selector: PayloadSelector(load(request, 16)) };
            }
        }
        result.ready = self.pending == PendingOperation::None && !result.continuation;
        Ok(result)
    }

    fn device_info<B: Backend>(&mut self, selector: u32, reply: &mut [u8; 512], backend: &B) -> Result<(), CommandError> {
        match selector {
            1 => identity_text(reply, backend, 0x16f, 11, 12)?,
            2 => wide(reply, b"FUJIFILM"),
            4 => {
                let mut product = [0; 20];
                copy_product(backend, &mut product)?;
                wide(reply, &product);
            }
            8 => {
                let major = identity(backend, 0x204)?;
                let minor = identity(backend, 0x205)?;
                let mut at = 16;
                if major >= 16 { reply[at] = (major >> 4).wrapping_add(b'0'); at += 2; }
                reply[at] = (major & 15).wrapping_add(b'0');
                reply[at + 2] = b'.';
                reply[at + 4] = (minor >> 4).wrapping_add(b'0');
                reply[at + 6] = (minor & 15).wrapping_add(b'0');
            }
            0x10 => wide(reply, b"CAMERA"),
            0x20 => {
                for index in 0..30 { reply[16 + index * 2] = identity(backend, 0x11b + index as u32)?; }
            }
            0x40 => wide(reply, b"1.00"),
            0xf0001 => match self.service_state {
                ServiceState::Processing => wide(reply, b"WRITING"),
                ServiceState::Success => wide(reply, b"SUCCESS"),
                ServiceState::Failed { code } => {
                    wide(reply, b"FAIL");
                    reply[24] = (code / 10).wrapping_add(48) as u8;
                    reply[26] = (code % 10).wrapping_add(48) as u8;
                }
                ServiceState::Normal => wide(reply, b"NORMAL"),
            },
            0xf0002 => { self.service_state = ServiceState::Normal; wide(reply, b"NORMAL"); }
            _ => status(reply, 9, 3),
        }
        Ok(())
    }

    fn configuration<B: Backend>(&mut self, request: &[u8; 512], reply: &mut [u8; 512], backend: &mut B) {
        let function = load(request, 16);
        let namespace = load(request, 20);
        store(reply, 16, function);
        store(reply, 20, namespace);
        if !(1..=3).contains(&function) || namespace > 1 { status(reply, 9, 3); return; }
        if function == 1 {
            store(reply, 28, backend.configuration_capacity());
            store(reply, 32, 256);
            return;
        }
        if backend.configuration_capacity() == 0 { status(reply, 1, 0xffff); return; }
        let page = load(request, 24);
        let offset = load(request, 28);
        let length = load(request, 32);
        store(reply, 24, page);
        store(reply, 28, offset);
        store(reply, 32, length);
        if length > 256 { status(reply, 9, 3); return; }
        let Some(start) = page.checked_mul(256).and_then(|page| page.checked_add(offset)) else {
            status(reply, 9, 3); return;
        };
        let namespace = if namespace == 0 { ConfigNamespace::Cached } else { ConfigNamespace::Encoded };
        let outcome = if function == 2 {
            backend.config_read(namespace, start, &mut reply[36..36 + length as usize])
        } else {
            backend.config_write(namespace, start, &request[36..36 + length as usize])
        };
        self.apply(if outcome == BackendStatus::Failed { BackendStatus::Unavailable } else { outcome }, reply);
        if self.pending == PendingOperation::Backend { self.pending = PendingOperation::Configuration; }
    }

    fn debug<B: Backend>(&mut self, request: &[u8; 512], reply: &mut [u8; 512], backend: &mut B) {
        let function = load(request, 16);
        store(reply, 16, function);
        let length = if function == 1 || function == 4 { load(request, 20) } else { load(request, 32) };
        if function > 5 || length.wrapping_sub(1) > 255 { status(reply, 9, 3); return; }
        let count = length as usize;
        let outcome = match function {
            1 | 4 => {
                let query = backend.debug_query(&mut reply[28..28 + count]);
                store(reply, 20, query.overflow);
                store(reply, 24, query.actual.0);
                query.status
            }
            2 => {
                let address = DebugAddress(load(request, 28));
                store(reply, 28, address.0);
                store(reply, 32, length);
                backend.debug_read(address, &mut reply[36..36 + count])
            }
            3 => {
                let address = DebugAddress(load(request, 28));
                store(reply, 28, address.0);
                store(reply, 32, length);
                backend.debug_write(address, &request[36..36 + count])
            }
            5 => {
                store(reply, 32, length);
                backend.debug_execute(&request[36..36 + count])
            }
            _ => return,
        };
        self.apply(outcome, reply);
    }

    fn apply(&mut self, outcome: BackendStatus, reply: &mut [u8; 512]) {
        match outcome {
            BackendStatus::Complete => {}
            BackendStatus::Pending => self.pending = PendingOperation::Backend,
            BackendStatus::Unavailable => status(reply, 1, 0xffff),
            BackendStatus::Invalid => status(reply, 9, 3),
            BackendStatus::Failed => status(reply, 1, 3),
        }
    }

    pub(crate) fn complete(&mut self, outcome: BackendStatus, reply: &mut [u8; 512]) -> Result<bool, CommandError> {
        if self.pending == PendingOperation::None { return Err(CommandError::NoPendingOperation); }
        if self.pending == PendingOperation::Configuration {
            if outcome == BackendStatus::Pending { return Ok(false); }
            self.pending = PendingOperation::None;
            if outcome != BackendStatus::Complete { status(reply, 1, 0xffff); }
            return Ok(true);
        }
        if self.pending == PendingOperation::Provider {
            if outcome == BackendStatus::Complete { return Err(CommandError::ProviderPayloadRequired); }
            if outcome == BackendStatus::Pending { return Ok(false); }
        }
        if self.pending == PendingOperation::Upload {
            if outcome == BackendStatus::Pending { return Ok(false); }
            self.pending = PendingOperation::None;
            if outcome == BackendStatus::Complete { self.service_state = ServiceState::Success; }
            else {
                store(reply, 20, u32::MAX);
                self.service_state = ServiceState::Failed { code: self.service_failure_code.0 };
            }
            return Ok(true);
        }
        let outcome = if self.pending == PendingOperation::Provider { BackendStatus::Unavailable } else { outcome };
        self.pending = PendingOperation::None;
        self.apply(outcome, reply);
        Ok(self.pending == PendingOperation::None)
    }

    pub fn publish_provider_payload(&mut self, payload: &PreparedPayload<'_>, reply: &mut [u8; 512]) -> Result<(), CommandError> {
        if self.pending != PendingOperation::Provider { return Err(CommandError::NoPendingOperation); }
        store(reply, 20, payload.length().0);
        self.pending = PendingOperation::None;
        Ok(())
    }
}

pub fn copy_product<B: Backend>(backend: &B, output: &mut [u8; 20]) -> Result<(), CommandError> {
    output.fill(0);
    for (index, byte) in output[..19].iter_mut().enumerate() {
        let value = identity(backend, 0x15c + index as u32)?;
        if value == 0 { break; }
        *byte = value;
    }
    Ok(())
}

fn identity<B: Backend>(backend: &B, index: u32) -> Result<u8, CommandError> {
    backend.identity_byte(index).ok_or(CommandError::IdentityUnavailable { index })
}

fn identity_text<B: Backend>(reply: &mut [u8; 512], backend: &B, start: u32, maximum: usize, width: usize) -> Result<(), CommandError> {
    let mut ended = false;
    for index in 0..width {
        if index < maximum && !ended {
            let byte = identity(backend, start + index as u32)?;
            reply[16 + index * 2] = byte;
            ended = byte == 0;
        }
    }
    Ok(())
}

fn wide(reply: &mut [u8; 512], bytes: &[u8]) {
    for (index, byte) in bytes.iter().enumerate() { reply[16 + index * 2] = *byte; }
}

fn load(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn store(bytes: &mut [u8], at: usize, value: u32) { bytes[at..at + 4].copy_from_slice(&value.to_le_bytes()); }
fn status(reply: &mut [u8; 512], primary: u32, secondary: u32) { store(reply, 4, primary); store(reply, 8, secondary); }

pub const fn capabilities() -> &'static [u8; 48] {
    &[
        4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        1, 0, 0x55, 0, 0xe8, 3, 0, 0, 1, 0, 0x76, 0, 0xe8, 3, 0, 0,
        6, 0, 0x45, 0, 0xe8, 3, 0, 0, 2, 0, 0x66, 0, 0x60, 0xea, 0, 0,
    ]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderError { InvalidSelector, Unavailable, Capacity, InvalidDescriptor, WrongPhase, Device(i32) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogKind { Primary, Secondary }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderRoute {
    Log(LogKind),
    PublishedImage,
    Diagnostic,
    Calibration,
    DescriptorImage { descriptor: ImageDescriptor, packing: ImagePacking },
    DeviceInfo,
    Empty,
    Capture,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageDescriptor(u8);

impl ImageDescriptor {
    pub fn new(value: u32) -> Result<Self, ProviderError> {
        if value < 36 { Ok(Self(value as u8)) } else { Err(ProviderError::InvalidDescriptor) }
    }
    pub const fn index(self) -> u8 { self.0 }
}

pub fn provider_route<B: Backend>(selector: PayloadSelector, current: ImageDescriptor, backend: &B) -> Result<ProviderRoute, ProviderError> {
    Ok(match selector.0 {
        0x10 | 0x12c => ProviderRoute::Log(LogKind::Primary),
        0x11 | 0x12d => ProviderRoute::Log(LogKind::Secondary),
        0x12 | 0x258 | 0x25f | 0x260 | 0x263 => ProviderRoute::PublishedImage,
        0x80 => ProviderRoute::Diagnostic,
        0x133 => ProviderRoute::Calibration,
        0x259 => {
            let mode = backend.identity_byte(0xac).ok_or(ProviderError::Unavailable)?;
            let descriptor = match mode { 4 => 30, 5 => 31, 6 => 32, 7 => 33, 8 => 34, 9 => 35, 10 => 11, _ => 29 };
            ProviderRoute::DescriptorImage { descriptor: ImageDescriptor(descriptor), packing: ImagePacking::Unpacked }
        }
        0x25c => ProviderRoute::DescriptorImage { descriptor: current, packing: ImagePacking::Unpacked },
        0x25e => ProviderRoute::DescriptorImage { descriptor: current, packing: ImagePacking::Configured },
        0x2b2 => ProviderRoute::DeviceInfo,
        0x2b3 => ProviderRoute::Empty,
        0x2b4 => ProviderRoute::Capture,
        _ => return Err(ProviderError::InvalidSelector),
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageExtent { pub bytes: ByteCount, pub stride: ByteCount, pub columns: u32, pub rows: u32 }

pub fn image_extent<B: Backend>(descriptor: ImageDescriptor, packing: ImagePacking, backend: &B) -> Result<ImageExtent, ProviderError> {
    let index = descriptor.0 as usize;
    // Image-5 tables: descriptors 0xef6de4, dimensions 0xef6aa4, crop keys 0xef6994.
    const FORMATS: [usize; 36] = [8, 0, 0, 0, 0, 0, 0, 0, 6, 6, 1, 1, 3, 9, 10, 11, 12, 13, 14, 15, 3, 3, 4, 4, 4, 4, 4, 4, 4, 2, 3, 4, 5, 6, 7, 8];
    let format = FORMATS[index];
    const NATIVE_ROWS: [u32; 16] = [3348, 716, 1668, 840, 426, 249, 219, 131, 116, 444, 444, 444, 444, 444, 444, 444];
    let mut columns = 4992u32;
    let mut rows = NATIVE_ROWS[format];
    let mode = if packing == ImagePacking::Unpacked && index != 1 {
        backend.identity_byte(0xc6).ok_or(ProviderError::Unavailable)?
    } else { 1 };
    if packing == ImagePacking::Unpacked && index != 1 && mode != 1 {
        const CROP_KEYS: [[u32; 4]; 16] = [
            [13568u32, 13572, 13574, 13570], [13650, 13654, 13656, 13652],
            [13594, 13598, 13600, 13596], [13602, 13606, 13608, 13604],
            [13610, 13614, 13616, 13612], [13618, 13622, 13624, 13620],
            [13626, 13630, 13632, 13628], [13634, 13638, 13640, 13636],
            [13642, 13646, 13648, 13644], [13664, 13684, 13686, 13682],
            [13666, 13684, 13686, 13682], [13668, 13684, 13686, 13682],
            [13670, 13684, 13686, 13682], [13672, 13684, 13686, 13682],
            [13674, 13684, 13686, 13682], [13676, 13684, 13686, 13682],
        ];
        let keys = CROP_KEYS[format];
        let word = |key: u32| -> Result<u32, ProviderError> {
            let low = backend.identity_byte(key).ok_or(ProviderError::Unavailable)?;
            let high = backend.identity_byte(key + 1).ok_or(ProviderError::Unavailable)?;
            Ok(u16::from_le_bytes([low, high]) as u32)
        };
        let first = word(keys[0])? >> 1;
        let fourth = word(keys[3])? >> 1;
        if mode == 0 { columns = fourth.checked_add(first).ok_or(ProviderError::InvalidDescriptor)?; }
        if mode == 2 {
            rows = if (9..16).contains(&format) { word(keys[2])?.checked_sub(1).ok_or(ProviderError::InvalidDescriptor)? } else { rows.saturating_sub(1) };
            columns = fourth;
        }
    }
    let packed = packing == ImagePacking::Configured && backend.identity_byte(0xec).ok_or(ProviderError::Unavailable)? == 1;
    let aligned = columns.checked_add(if packed { 255 } else { 63 }).ok_or(ProviderError::InvalidDescriptor)?
        & if packed { !255 } else { !63 };
    if aligned > if packed { 0xff00 } else { 0xffc0 } { return Err(ProviderError::InvalidDescriptor); }
    let stride = if packed { aligned.checked_mul(3).ok_or(ProviderError::InvalidDescriptor)? >> 1 }
        else { aligned.checked_mul(2).ok_or(ProviderError::InvalidDescriptor)? };
    let bytes = rows.checked_mul(stride).ok_or(ProviderError::InvalidDescriptor)?;
    Ok(ImageExtent { bytes: ByteCount(bytes), stride: ByteCount(stride), columns, rows })
}

pub fn collect_log<'a, M, S, B>(
    kind: LogKind, entry: &mut record_store::Entry<'_, M, S, B>,
    area: &mut record_store::Area, output: &'a mut [u8],
) -> Result<PreparedPayload<'a>, ProviderError>
where M: record_store::RecordMedium, S: record_store::Staging, B: record_store::BlockBuffer<M> {
    let required = area.geometry.record_bytes().0 as usize * area.geometry.records_per_bank().0 as usize * 2;
    if output.len() < required { return Err(ProviderError::Capacity); }
    let written = entry.collect(area, output);
    let limit = match kind { LogKind::Primary => 0x10000, LogKind::Secondary => 0x5f40 };
    let start = written.saturating_sub(limit);
    let address = 0x42e40000u32.checked_add(start as u32).ok_or(ProviderError::Capacity)?;
    Ok(PreparedPayload::from_region(PublishedRegion::adopt(address, &output[start..written])?))
}

#[derive(Clone, Copy)]
pub struct PublishedRegion<'a> { address: u32, bytes: &'a [u8] }
impl<'a> PublishedRegion<'a> {
    pub fn adopt(address: u32, bytes: &'a [u8]) -> Result<Self, ProviderError> {
        let length = u32::try_from(bytes.len()).map_err(|_| ProviderError::Capacity)?;
        address.checked_add(length).ok_or(ProviderError::Capacity)?;
        Ok(Self { address, bytes })
    }
    pub fn read(&self, address: u32, length: ByteCount) -> Result<&'a [u8], ProviderError> {
        let start = address.checked_sub(self.address).ok_or(ProviderError::Capacity)? as usize;
        let end = start.checked_add(length.0 as usize).ok_or(ProviderError::Capacity)?;
        self.bytes.get(start..end).ok_or(ProviderError::Capacity)
    }
    pub const fn address(self) -> u32 { self.address }
    pub const fn bytes(self) -> &'a [u8] { self.bytes }
    pub const fn length(self) -> ByteCount { ByteCount(self.bytes.len() as u32) }
    pub fn subregion(self, address: u32, length: ByteCount) -> Result<Self, ProviderError> {
        Self::adopt(address, self.read(address, length)?)
    }
}

pub struct ProviderRegions<'a> {
    pub primary_log: Option<PreparedPayload<'a>>,
    pub secondary_log: Option<PreparedPayload<'a>>,
    pub image: Option<PublishedImage<'a>>,
    pub diagnostic: Option<PublishedRegion<'a>>,
    pub descriptor_image: Option<PublishedRegion<'a>>,
    pub device_info: Option<PreparedPayload<'a>>,
    pub diagnostic_layout: DiagnosticLayout,
}

pub fn provider_bytes<'a, B: Backend>(route: ProviderRoute, regions: &ProviderRegions<'a>, backend: &B) -> Result<PreparedPayload<'a>, ProviderError> {
    let region = match route {
        ProviderRoute::Log(kind) => return match kind {
            LogKind::Primary => regions.primary_log,
            LogKind::Secondary => regions.secondary_log,
        }.ok_or(ProviderError::Unavailable),
        ProviderRoute::PublishedImage => {
            let image = regions.image.as_ref().ok_or(ProviderError::Unavailable)?;
            image.region.subregion(image.publication.address(), image.publication.length())?
        }
        ProviderRoute::Diagnostic => regions.diagnostic.ok_or(ProviderError::Unavailable)?
            .subregion(0x4b2ece00, regions.diagnostic_layout.length())?,
        ProviderRoute::DescriptorImage { descriptor, packing } => regions.descriptor_image.ok_or(ProviderError::Unavailable)?
            .subregion(0x437cee00, image_extent(descriptor, packing, backend)?.bytes)?,
        ProviderRoute::DeviceInfo => {
            let region = regions.device_info.ok_or(ProviderError::Unavailable)?.region();
            region.subregion(region.address(), ByteCount(0xe297c))?
        }
        ProviderRoute::Empty => PublishedRegion::adopt(0, &[])?,
        ProviderRoute::Calibration | ProviderRoute::Capture => return Err(ProviderError::WrongPhase),
    };
    Ok(PreparedPayload::from_region(region))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureEffect {
    RegisterObserverAndWait,
    Initialize { operation: u16, mode: u32, enabled: u32 },
    ReadPage { page: u16, mode: u32, first: u32, second: u32 },
    UnregisterObserver,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureResult { Payload(ByteCount), Unchanged, Failed(i32) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CapturePhase { AwaitReady, AwaitInitialization, AwaitPage { page: u16, index: usize }, Finished }

pub struct Capture { phase: CapturePhase, result: Option<CaptureResult> }

impl Capture {
    pub const fn begin() -> (Self, CaptureEffect) {
        (Self { phase: CapturePhase::AwaitReady, result: None }, CaptureEffect::RegisterObserverAndWait)
    }

    pub fn ready(&mut self, completion: Result<(), i32>) -> Result<Option<CaptureEffect>, ProviderError> {
        if self.phase != CapturePhase::AwaitReady { return Err(ProviderError::WrongPhase); }
        if let Err(error) = completion {
            self.phase = CapturePhase::Finished;
            self.result = Some(CaptureResult::Failed(error));
            return Ok(None);
        }
        self.phase = CapturePhase::AwaitInitialization;
        Ok(Some(CaptureEffect::Initialize { operation: 6, mode: 11, enabled: 1 }))
    }

    pub fn initialized(&mut self, completion: Result<(), i32>) -> Result<CaptureEffect, ProviderError> {
        if self.phase != CapturePhase::AwaitInitialization { return Err(ProviderError::WrongPhase); }
        if let Err(error) = completion { return Ok(self.finish(CaptureResult::Failed(error))); }
        self.phase = CapturePhase::AwaitPage { page: 1, index: 0 };
        Ok(CaptureEffect::ReadPage { page: 1, mode: 11, first: 1, second: 1 })
    }

    pub fn page(&mut self, completion: Result<&[u8], i32>, output: &mut [u8]) -> Result<CaptureEffect, ProviderError> {
        let CapturePhase::AwaitPage { page, index } = self.phase else { return Err(ProviderError::WrongPhase); };
        let bytes = match completion { Ok(bytes) => bytes, Err(error) => return Ok(self.finish(CaptureResult::Failed(error))) };
        if page == 0xff00 {
            let header = output.get(0x2800..0x2805).ok_or(ProviderError::Capacity)?;
            let result = if header[0] == 0xff && header[1] == 0xf0 {
                let length = header[2] as u32 * 0x800 + header[3] as u32 + header[4] as u32 * 256;
                if length as usize > output.len() { return Err(ProviderError::Capacity); }
                CaptureResult::Payload(ByteCount(length))
            } else { CaptureResult::Unchanged };
            return Ok(self.finish(result));
        }
        let source = bytes.get(..0x800).ok_or(ProviderError::Capacity)?;
        let start = index * 0x800;
        output.get_mut(start..start + 0x800).ok_or(ProviderError::Capacity)?.copy_from_slice(source);
        let next = match page { 1..=4 => page + 1, 5 => 0xffff, 0xffff => 0xff00, _ => return Err(ProviderError::WrongPhase) };
        self.phase = CapturePhase::AwaitPage { page: next, index: index + 1 };
        Ok(CaptureEffect::ReadPage { page: next, mode: 11, first: 1, second: 1 })
    }

    fn finish(&mut self, result: CaptureResult) -> CaptureEffect {
        self.phase = CapturePhase::Finished;
        self.result = Some(result);
        CaptureEffect::UnregisterObserver
    }

    pub const fn result(&self) -> Option<CaptureResult> { self.result }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImagePacking { Unpacked, Configured }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalibrationRead {
    pub first_block: u16,
    pub blocks: u16,
    pub timeout: u32,
}

pub trait CalibrationMedium {
    fn read(&mut self, request: CalibrationRead, output: &mut [u8]) -> Result<(), i32>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalibrationOutcome { NotInitialized, Read, Failed(i32) }

pub fn prepare_calibration<'a>(
    initialized: bool, workspace: &'a mut [u8], medium: &mut impl CalibrationMedium,
) -> Result<(PreparedPayload<'a>, CalibrationOutcome), ProviderError> {
    let workspace = workspace.get_mut(..0x80020).ok_or(ProviderError::Capacity)?;
    let outcome = if initialized {
        match medium.read(CalibrationRead { first_block: 0x5a, blocks: 4, timeout: 1000 }, &mut workspace[32..]) {
            Ok(()) => CalibrationOutcome::Read,
            Err(error) => CalibrationOutcome::Failed(error),
        }
    } else { CalibrationOutcome::NotInitialized };
    Ok((PreparedPayload::from_region(PublishedRegion::adopt(0x42e40000, workspace)?), outcome))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImagePublicationMode { Raw, Intermediate, Alternate, Pool }

impl ImagePublicationMode {
    pub fn parse(value: u32) -> Result<Self, ProviderError> {
        match value {
            0xa5 | 0xa7 => Ok(Self::Raw), 0xa6 => Ok(Self::Intermediate),
            0xa8 => Ok(Self::Alternate), 0xa9 => Ok(Self::Pool),
            _ => Err(ProviderError::InvalidSelector),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImagePublication { address: u32, length: ByteCount }

impl ImagePublication {
    pub fn from_frame(mode: ImagePublicationMode, frame: &[u8]) -> Result<Self, ProviderError> {
        let read = |at: usize| -> Result<u32, ProviderError> {
            let bytes = frame.get(at..at + 4).ok_or(ProviderError::Capacity)?;
            Ok(u32::from_le_bytes(bytes.try_into().map_err(|_| ProviderError::Capacity)?))
        };
        let (address, length) = match mode {
            ImagePublicationMode::Raw => {
                let first = read(0x190)?;
                let second = read(0x198)?;
                let factor = if *frame.get(0x19b4).ok_or(ProviderError::Capacity)? == 0 { 2 } else { 1 };
                let length = first.checked_mul(second).and_then(|bytes| bytes.checked_mul(factor)).ok_or(ProviderError::Capacity)?;
                (read(0x188)?, length)
            }
            ImagePublicationMode::Intermediate => (read(0x8c)?, read(0x90)?),
            ImagePublicationMode::Alternate => (read(0x2928)?, read(0x292c)?),
            ImagePublicationMode::Pool => return Err(ProviderError::WrongPhase),
        };
        Self::new(address, length)
    }

    pub fn from_pool(pool: &[u8], index: u32) -> Result<Self, ProviderError> {
        if index >= 17 { return Err(ProviderError::InvalidDescriptor); }
        let start = index as usize * 0x28 + 0xc;
        let descriptor = pool.get(start..start + 12).ok_or(ProviderError::Capacity)?;
        Self::new(load(descriptor, 4), load(descriptor, 8))
    }

    fn new(address: u32, length: u32) -> Result<Self, ProviderError> {
        address.checked_add(length).ok_or(ProviderError::Capacity)?;
        Ok(Self { address, length: ByteCount(length) })
    }

    pub const fn clear() -> Self { Self { address: 0, length: ByteCount(0) } }
    pub const fn length(self) -> ByteCount { self.length }
    pub const fn address(self) -> u32 { self.address }
    pub fn bytes<'a>(self, region: &PublishedRegion<'a>) -> Result<&'a [u8], ProviderError> {
        if self.address == 0 || self.length.0 == 0 { return Err(ProviderError::Unavailable); }
        region.read(self.address, self.length)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticLayout { Ordinary, Extended }

impl DiagnosticLayout {
    pub fn from_native_configuration(configuration: &[u8]) -> Result<Self, ProviderError> {
        let value = configuration.get(12..16).ok_or(ProviderError::Capacity)?;
        Ok(if u32::from_le_bytes(value.try_into().map_err(|_| ProviderError::Capacity)?) == 1 { Self::Extended } else { Self::Ordinary })
    }
    pub const fn length(self) -> ByteCount {
        ByteCount(match self { Self::Ordinary => 0x100000, Self::Extended => 0x108000 })
    }
}

#[derive(Clone, Copy)]
pub struct PreparedPayload<'a> { region: PublishedRegion<'a> }

impl<'a> PreparedPayload<'a> {
    pub(crate) const fn from_region(region: PublishedRegion<'a>) -> Self { Self { region } }
    pub const fn region(self) -> PublishedRegion<'a> { self.region }
    pub const fn address(self) -> u32 { self.region.address }
    pub const fn bytes(self) -> &'a [u8] { self.region.bytes }
    pub const fn length(self) -> ByteCount { self.region.length() }
}

pub struct PublishedImage<'a> { publication: ImagePublication, region: PublishedRegion<'a> }

impl<'a> PublishedImage<'a> {
    pub fn new(publication: ImagePublication, region: PublishedRegion<'a>) -> Result<Self, ProviderError> {
        publication.bytes(&region)?;
        Ok(Self { publication, region })
    }
}

impl Capture {
    pub fn prepared<'a>(&self, output: PublishedRegion<'a>) -> Result<PreparedPayload<'a>, ProviderError> {
        match self.result {
            Some(CaptureResult::Payload(length)) => Ok(PreparedPayload::from_region(output.subregion(0x014e6b08, length)?)),
            Some(CaptureResult::Unchanged) => Err(ProviderError::Unavailable),
            Some(CaptureResult::Failed(error)) => Err(ProviderError::Device(error)),
            None => Err(ProviderError::WrongPhase),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadProvider { Absent, Configuration }

pub struct ConfigurationUpload<'a> {
    pub source: u8,
    pub destination: u8,
    pub opcode: u16,
    pub payload: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UploadPending {
    pub destination: u8,
    pub opcode: u16,
    pub kind: u32,
}

pub trait UploadMessages {
    fn configuration(&mut self, message: ConfigurationUpload<'_>) -> Result<(), i32>;
    fn pending_transport(&mut self, message: UploadPending) -> Result<(), i32>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UploadCompletion {
    pub reply_length: usize,
    pub ready: bool,
    pub clear_receive_state: bool,
    pub transport_notification: Option<Result<(), i32>>,
}

impl Commands {
    pub fn complete_upload(
        &mut self, received: &[u8], provider: UploadProvider,
        reply: &mut [u8; 512], messages: &mut impl UploadMessages,
    ) -> Result<UploadCompletion, CommandError> {
        let Some(command) = self.upload_command else { return Err(CommandError::NoPendingOperation); };
        let count = u32::try_from(received.len()).map(ByteCount).map_err(|_| CommandError::RequestTooLarge)?;
        if count != self.upload_length {
            return Err(CommandError::UploadLengthMismatch { expected: self.upload_length, received: count });
        }
        self.upload_command = None;
        self.uploaded_length = count;
        reply[..128].fill(0);
        store(reply, 0, Command::Continuation as u32);
        let mut completion = UploadCompletion { reply_length: 32, ready: true,
            clear_receive_state: false, transport_notification: None };
        if command != Command::Upload { return Ok(completion); }
        if provider == UploadProvider::Absent {
            store(reply, 20, u32::MAX);
            self.service_state = ServiceState::Normal;
            return Ok(completion);
        }
        match messages.configuration(ConfigurationUpload { source: 0xff, destination: 7, opcode: 0x1708, payload: received }) {
            Ok(()) => {
                self.pending = PendingOperation::Upload;
                self.service_state = ServiceState::Processing;
                completion.ready = false;
                completion.clear_receive_state = true;
                completion.transport_notification = Some(messages.pending_transport(UploadPending { destination: 0x24, opcode: 0x3407, kind: 2 }));
            }
            Err(_) => {
                store(reply, 20, u32::MAX);
                self.service_state = ServiceState::Failed { code: self.service_failure_code.0 };
            }
        }
        Ok(completion)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureQueue { Property, Observer }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureCallback { Ready, Initialized, Page, None }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapturePayload {
    Observer,
    Initialize { control: u16, operation: u8, flags: u8, transfer: u16, reply_kind: u8 },
    Page { page: u16, mode: u32, first: u32, second: u32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureMessage {
    pub queue: CaptureQueue,
    pub opcode: u16,
    pub payload_bytes: ByteCount,
    pub callback: CaptureCallback,
    pub payload: CapturePayload,
}

impl CaptureQueue {
    pub const fn id(self) -> u8 { match self { Self::Property => 0x62, Self::Observer => 0x63 } }
}

impl CaptureEffect {
    pub const fn message(self) -> CaptureMessage {
        match self {
            Self::RegisterObserverAndWait => CaptureMessage { queue: CaptureQueue::Observer, opcode: 0x7303,
                payload_bytes: ByteCount(0x18), callback: CaptureCallback::Ready, payload: CapturePayload::Observer },
            Self::UnregisterObserver => CaptureMessage { queue: CaptureQueue::Observer, opcode: 0x7304,
                payload_bytes: ByteCount(0x18), callback: CaptureCallback::None, payload: CapturePayload::Observer },
            Self::Initialize { operation, mode, enabled } => CaptureMessage { queue: CaptureQueue::Property, opcode: 0x723f,
                payload_bytes: ByteCount(0x30), callback: CaptureCallback::Initialized,
                payload: CapturePayload::Initialize { control: 0x600, operation: (operation & 0x7ff) as u8,
                    flags: ((operation & 0x7ff) >> 8) as u8 | ((mode & 15) << 3) as u8 | ((enabled & 1) << 7) as u8,
                    transfer: 0x4940, reply_kind: 2 } },
            Self::ReadPage { page, mode, first, second } => CaptureMessage { queue: CaptureQueue::Property, opcode: 0x7254,
                payload_bytes: ByteCount(0x14), callback: CaptureCallback::Page,
                payload: CapturePayload::Page { page, mode, first, second } },
        }
    }
}

pub const fn capabilities_region() -> PublishedRegion<'static> {
    PublishedRegion { address: 0x00d80d14, bytes: capabilities() }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletionCode(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceFailureCode(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PendingKind { ConfigurationWrite, ConfigurationRead, Upload, Provider }

/// 0x3408 kinds stock 0x00d819fc passes two payload words: slots +0x42c, +0x430, +0x434.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum PairCompletion { Pair = 6, Read = 7, Write = 8 }

/// 0x3408 kinds stock 0x00d819fc passes one payload word: slots +0x438..+0x468.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum SingleCompletion {
    Control = 9, Argument = 10, Argument12 = 12, Argument13 = 13, Argument14 = 14, Path = 16,
    Argument17 = 17, Directory = 18, Argument19 = 19, Argument20 = 20, Argument21 = 21,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolCompletion {
    Pair { kind: PairCompletion, first: u32, second: u32 },
    Single { kind: SingleCompletion, value: u32 },
}
impl ProtocolCompletion {
    fn parse(kind: u32, first: u32, second: u32) -> Option<Self> {
        let single = |kind| Some(Self::Single { kind, value: first });
        match kind {
            6 => Some(Self::Pair { kind: PairCompletion::Pair, first, second }),
            7 => Some(Self::Pair { kind: PairCompletion::Read, first, second }),
            8 => Some(Self::Pair { kind: PairCompletion::Write, first, second }),
            9 => single(SingleCompletion::Control), 10 => single(SingleCompletion::Argument),
            12 => single(SingleCompletion::Argument12), 13 => single(SingleCompletion::Argument13),
            14 => single(SingleCompletion::Argument14), 16 => single(SingleCompletion::Path),
            17 => single(SingleCompletion::Argument17), 18 => single(SingleCompletion::Directory),
            19 => single(SingleCompletion::Argument19), 20 => single(SingleCompletion::Argument20),
            21 => single(SingleCompletion::Argument21),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VendorCompletion {
    Command { status: CompletionCode },
    ServiceCode { indicator: u32, code: ServiceFailureCode },
    Protocol(ProtocolCompletion),
    Ignored { kind: u32 },
}
impl VendorCompletion {
    pub fn parse(payload: &[u8; 24]) -> Self {
        match load(payload, 0) {
            4 => Self::Command { status: CompletionCode(load(payload, 4)) },
            5 => Self::ServiceCode { indicator: load(payload, 4), code: ServiceFailureCode(load(payload, 8)) },
            kind => match ProtocolCompletion::parse(kind, load(payload, 4), load(payload, 8)) {
                Some(completion) => Self::Protocol(completion),
                None => Self::Ignored { kind },
            },
        }
    }
    /// Stock 0x00d819fc in phases 8 to 11: kinds 6 and 10 rearm the USB callbacks only for a nonzero first word.
    pub const fn rearms_transport(self) -> bool {
        match self {
            Self::Protocol(ProtocolCompletion::Pair { kind: PairCompletion::Pair, first, .. }) => first != 0,
            Self::Protocol(ProtocolCompletion::Single { kind: SingleCompletion::Argument, value }) => value != 0,
            _ => true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionMessage { Pending(PendingKind), Completed(VendorCompletion) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionError { Opcode, Payload, Kind, Command(CommandError) }

impl CompletionMessage {
    pub fn parse(opcode: u16, payload: &[u8]) -> Result<Self, CompletionError> {
        let payload: &[u8; 24] = payload.try_into().map_err(|_| CompletionError::Payload)?;
        match opcode {
            0x3407 => Ok(Self::Pending(match load(payload, 0) {
                0 => PendingKind::ConfigurationWrite, 1 => PendingKind::ConfigurationRead,
                2 => PendingKind::Upload, 3 => PendingKind::Provider,
                _ => return Err(CompletionError::Kind),
            })),
            0x3408 => Ok(Self::Completed(VendorCompletion::parse(payload))),
            _ => Err(CompletionError::Opcode),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ControllerState {
    Stopped, Starting, State2, State3, State4, State5, State6, State7, State8, State9,
    ActiveVendor, PendingVendor,
}

impl ControllerState {
    pub fn parse(value: u32) -> Result<Self, CompletionError> {
        Ok(match value {
            0 => Self::Stopped, 1 => Self::Starting, 2 => Self::State2, 3 => Self::State3,
            4 => Self::State4, 5 => Self::State5, 6 => Self::State6, 7 => Self::State7,
            8 => Self::State8, 9 => Self::State9, 10 => Self::ActiveVendor, 11 => Self::PendingVendor,
            _ => return Err(CompletionError::Kind),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionDisposition { Ignored, Deferred, Pending, Command { ready: bool }, ServiceCode, Protocol(ProtocolCompletion) }

impl Commands {
    pub(crate) fn consume_vendor_completion(
        &mut self, completion: VendorCompletion, reply: &mut [u8; 512],
        prepared: Option<&PreparedPayload<'_>>,
    ) -> Result<CompletionDisposition, CompletionError> {
        match completion {
            VendorCompletion::Command { status: CompletionCode(status) } => {
                if self.pending == PendingOperation::None { return Ok(CompletionDisposition::Ignored); }
                let ready = if self.pending == PendingOperation::Provider && status == 0 {
                    let payload = prepared.ok_or(CompletionError::Command(CommandError::ProviderPayloadRequired))?;
                    self.publish_provider_payload(payload, reply).map_err(CompletionError::Command)?;
                    true
                } else {
                    self.complete(if status == 0 { BackendStatus::Complete } else { BackendStatus::Failed }, reply).map_err(CompletionError::Command)?
                };
                Ok(CompletionDisposition::Command { ready })
            }
            VendorCompletion::ServiceCode { indicator, code } => {
                if indicator == 1 {
                    self.service_failure_code = code;
                    if matches!(self.service_state, ServiceState::Failed { .. }) {
                        self.service_state = ServiceState::Failed { code: code.0 };
                    }
                }
                Ok(CompletionDisposition::ServiceCode)
            }
            VendorCompletion::Protocol(completion) => Ok(CompletionDisposition::Protocol(completion)),
            VendorCompletion::Ignored { .. } => Ok(CompletionDisposition::Ignored),
        }
    }
}
