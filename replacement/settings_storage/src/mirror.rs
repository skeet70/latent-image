pub const HEADER_BYTES: usize = 16;
pub const BODY_BYTES: usize = 0x118;
pub const CUSTOM_BYTES: usize = 40;
pub const BACKING_BYTES: usize = 0x19a;
pub const INITIALIZED_EXPORT_BYTES: usize = 0x168;
pub const EXPORT_RESIDUE_BYTES: usize = BACKING_BYTES - INITIALIZED_EXPORT_BYTES;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackupKey(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackupValue(pub u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceStatus(pub i32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackingStatus {
    Success,
    OutOfRange,
    Refused(DeviceStatus),
    AddressUnavailable,
}

impl BackingStatus {
    pub const fn legacy_code(self) -> i32 {
        match self {
            Self::Success => 0,
            Self::OutOfRange => -30,
            Self::Refused(status) => status.0,
            Self::AddressUnavailable => -30,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestStatus {
    Success,
    Failed,
}

impl RequestStatus {
    pub const fn legacy_code(self) -> i32 {
        match self {
            Self::Success => 0,
            Self::Failed => -63,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestOutcome {
    pub underlying: BackingStatus,
    pub mapped: RequestStatus,
    pub reported: bool,
}

impl RequestOutcome {
    pub const fn success() -> Self {
        Self { underlying: BackingStatus::Success, mapped: RequestStatus::Success, reported: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeaderPosition(u8);

impl HeaderPosition {
    pub const fn new(value: u8) -> Option<Self> {
        if value < HEADER_BYTES as u8 { Some(Self(value)) } else { None }
    }

    pub const fn index(self) -> usize { self.0 as usize }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverObservation {
    pub position: HeaderPosition,
    pub key: BackupKey,
    pub value: BackupValue,
    pub status: DeviceStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MirrorOutcome {
    pub request: RequestOutcome,
    pub synchronized: [Option<DriverObservation>; HEADER_BYTES],
}

impl MirrorOutcome {
    pub const fn unchanged() -> Self {
        Self { request: RequestOutcome::success(), synchronized: [None; HEADER_BYTES] }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BodyOutcome {
    pub request: RequestOutcome,
    pub synchronized: [Option<DriverObservation>; HEADER_BYTES],
}

impl BodyOutcome {
    pub const fn unchanged() -> Self {
        Self { request: RequestOutcome::success(), synchronized: [None; HEADER_BYTES] }
    }
}

pub trait HeaderBackend {
    fn read_header(&mut self, destination: &mut [u8; HEADER_BYTES]) -> RequestOutcome;
    fn write_header(&mut self, source: &[u8; HEADER_BYTES]) -> MirrorOutcome;
}
pub trait SettingsBackend: HeaderBackend {
    fn read_body(&mut self, destination: &mut [u8; BODY_BYTES]) -> RequestOutcome;
    fn write_body(&mut self, source: &[u8; BODY_BYTES]) -> BodyOutcome;
    fn read_custom(&mut self, destination: &mut [u8; CUSTOM_BYTES]) -> RequestOutcome;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestDirection { Read, Write }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionContext { Task, OutsideTask }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupSemaphore { Settings }

impl BackupSemaphore {
    pub const fn legacy_id(self) -> u32 { 0x25 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupDiagnostic { OutsideTask }

impl BackupDiagnostic {
    pub const fn legacy_arguments(self) -> (u32, u32) { (2, 0x3b) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupWriteMode { Immediate }

impl BackupWriteMode {
    pub const fn legacy_argument(self) -> u32 { 0 }
}

pub trait HeaderBackupDriver {
    fn context(&mut self) -> ExecutionContext;
    fn acquire(&mut self, semaphore: BackupSemaphore);
    fn release(&mut self, semaphore: BackupSemaphore);
    fn diagnostic(&mut self, diagnostic: BackupDiagnostic);
    fn admit(&mut self, direction: RequestDirection) -> Result<(), DeviceStatus>;
    fn write_backup(&mut self, key: BackupKey, value: BackupValue, mode: BackupWriteMode) -> DeviceStatus;
    fn store_request_status(&mut self, status: RequestStatus);
    fn report_failure(&mut self, status: RequestStatus);
}
pub trait BackupDriver: HeaderBackupDriver {
    fn refresh_custom(&mut self, custom: &[u8; CUSTOM_BYTES]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackingRange {
    offset: u32,
    length: u32,
}

impl BackingRange {
    #[inline(always)]
    pub const fn new(offset: u32, length: u32) -> Self { Self { offset, length } }
    pub const fn offset(self) -> u32 { self.offset }
    pub const fn length(self) -> u32 { self.length }
    pub const fn end(self) -> u32 { self.offset.wrapping_add(self.length) }
    pub const fn admitted(self) -> bool { self.end() <= BACKING_BYTES as u32 }

    #[inline(always)]
    fn projection(self, buffer_length: usize) -> Result<core::ops::Range<usize>, BackingStatus> {
        if !self.admitted() { return Err(BackingStatus::OutOfRange); }
        if self.length == 0 { return Ok(0..0); }
        let start = self.offset as usize;
        let end = start.checked_add(self.length as usize).ok_or(BackingStatus::AddressUnavailable)?;
        if end > BACKING_BYTES || self.length as usize > buffer_length {
            return Err(BackingStatus::AddressUnavailable);
        }
        Ok(start..end)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestDescriptor(BackingRange);

impl RequestDescriptor {
    #[inline(always)]
    pub const fn from_abi(length: u32, offset: u32) -> Self {
        Self(BackingRange::new(offset as u16 as u32, length as u16 as u32))
    }
    #[inline(always)]
    pub const fn range(self) -> BackingRange { self.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackingWriteOutcome {
    pub status: BackingStatus,
    pub synchronized: [Option<DriverObservation>; HEADER_BYTES],
    pub custom_refreshed: bool,
}

pub struct SharedBacking<'a, D> {
    live: &'a mut [u8; BACKING_BYTES],
    shadow: &'a mut [u8; HEADER_BYTES],
    keys: &'a [BackupKey; HEADER_BYTES],
    residue: Option<&'a [u8; EXPORT_RESIDUE_BYTES]>,
    driver: D,
}

impl<'a, D: HeaderBackupDriver> SharedBacking<'a, D> {
    pub fn new(
        live: &'a mut [u8; BACKING_BYTES],
        shadow: &'a mut [u8; HEADER_BYTES],
        keys: &'a [BackupKey; HEADER_BYTES],
        residue: &'a [u8; EXPORT_RESIDUE_BYTES],
        driver: D,
    ) -> Self {
        Self { live, shadow, keys, residue: Some(residue), driver }
    }

    #[inline(always)]
    pub fn without_export_residue(
        live: &'a mut [u8; BACKING_BYTES],
        shadow: &'a mut [u8; HEADER_BYTES],
        keys: &'a [BackupKey; HEADER_BYTES],
        driver: D,
    ) -> Self {
        Self { live, shadow, keys, residue: None, driver }
    }

    pub fn live(&self) -> &[u8; BACKING_BYTES] { self.live }
    pub fn live_mut(&mut self) -> &mut [u8; BACKING_BYTES] { self.live }
    pub fn shadow(&self) -> &[u8; HEADER_BYTES] { self.shadow }
    pub fn driver(&self) -> &D { &self.driver }
    pub fn driver_mut(&mut self) -> &mut D { &mut self.driver }
    pub fn auxiliary(&self) -> &[u8] { &self.live[0x128..0x140] }
    pub fn auxiliary_mut(&mut self) -> &mut [u8] { &mut self.live[0x128..0x140] }

    fn enter(&mut self) {
        match self.driver.context() {
            ExecutionContext::Task => self.driver.acquire(BackupSemaphore::Settings),
            ExecutionContext::OutsideTask => self.driver.diagnostic(BackupDiagnostic::OutsideTask),
        }
    }

    fn leave(&mut self) {
        if self.driver.context() == ExecutionContext::Task {
            self.driver.release(BackupSemaphore::Settings);
        }
    }

    #[inline(always)]
    fn export_source(&self, index: usize) -> (&[u8], usize) {
        let source: &[u8] = if index < HEADER_BYTES { &*self.shadow } else { &*self.live };
        (source, index)
    }

    #[inline(always)]
    fn export_byte(&self, index: usize) -> u8 {
        let (source, offset) = self.export_source(index);
        crate::memory::read_byte(source, offset)
    }

    #[inline(always)]
    fn export_halfword(&self, index: usize) -> u16 {
        let (source, offset) = self.export_source(index);
        crate::memory::read_halfword(source, offset)
    }

    /// Stock leaves the residue uninitialized; without a residue window the caller's bytes stay untouched.
    #[inline(always)]
    pub fn export(&mut self, range: BackingRange, destination: &mut [u8]) -> BackingStatus {
        self.enter();
        let status = match self.driver.admit(RequestDirection::Read) {
            Err(status) => BackingStatus::Refused(status),
            Ok(()) => match range.projection(destination.len()) {
                Err(status) => status,
                Ok(projection) => {
                    let destination = destination.get_mut(..projection.len()).unwrap();
                    let initialized = projection.end.min(INITIALIZED_EXPORT_BYTES).saturating_sub(projection.start);
                    let mut written = 0;
                    if (destination.as_ptr() as usize | projection.start) & 1 == 0 {
                        let paired = initialized & !1;
                        while written < paired {
                            let value = self.export_halfword(projection.start + written);
                            crate::memory::write_output_halfword(destination, written, value);
                            written += 2;
                        }
                    }
                    while written < initialized {
                        let value = self.export_byte(projection.start + written);
                        crate::memory::write_output_byte(destination, written, value);
                        written += 1;
                    }
                    if projection.end > INITIALIZED_EXPORT_BYTES {
                        let start = projection.start.max(INITIALIZED_EXPORT_BYTES);
                        let base = start - INITIALIZED_EXPORT_BYTES;
                        let count = projection.end - start;
                        let offset = start - projection.start;
                        #[cfg(storage_defect_full_export_tail_zero_invented)]
                        destination[offset..offset + count].fill(0);
                        #[cfg(not(storage_defect_full_export_tail_zero_invented))]
                        if let Some(residue) = self.residue.as_ref() {
                            destination[offset..offset + count].copy_from_slice(&residue[base..base + count]);
                        }
                    }
                    BackingStatus::Success
                }
            },
        };
        self.leave();
        status
    }

    #[inline(always)]
    pub fn synchronize(&mut self) -> [Option<DriverObservation>; HEADER_BYTES] {
        let checksum = self.live[..15].iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte)).wrapping_neg();
        crate::memory::write_byte(&mut self.live[15], checksum);
        let mut observations = [None; HEADER_BYTES];
        for (index, (key, observation)) in self.keys.iter().zip(observations.iter_mut()).enumerate() {
            if crate::memory::read_byte(&*self.live, index) != crate::memory::read_byte(&*self.shadow, index) {
                let key = *key;
                let value = BackupValue(crate::memory::read_byte(&*self.live, index));
                let status = self.driver.write_backup(key, value, BackupWriteMode::Immediate);
                crate::memory::write_byte_at(&mut *self.shadow, index, crate::memory::read_byte(&*self.live, index));
                *observation = Some(DriverObservation { position: HeaderPosition(index as u8), key, value, status });
            }
        }
        observations
    }

    #[inline(always)]
    fn import_before_leave(&mut self, range: BackingRange, source: &[u8]) -> BackingWriteOutcome {
        self.enter();
        let mut outcome = BackingWriteOutcome { status: BackingStatus::Success, synchronized: [None; HEADER_BYTES], custom_refreshed: false };
        match self.driver.admit(RequestDirection::Write) {
            Err(status) => outcome.status = BackingStatus::Refused(status),
            Ok(()) => match range.projection(source.len()) {
                Err(status) => outcome.status = status,
                Ok(projection) => {
                    crate::memory::copy_bytes(self.live.get_mut(projection.clone()).unwrap(), source.get(..projection.len()).unwrap());
                    outcome.synchronized = self.synchronize();
                }
            },
        }
        outcome
    }

    #[inline(always)]
    fn report(&mut self, underlying: BackingStatus) -> RequestOutcome {
        let mapped = if underlying == BackingStatus::Success { RequestStatus::Success } else { RequestStatus::Failed };
        self.driver.store_request_status(mapped);
        let reported = mapped == RequestStatus::Failed;
        if reported { self.driver.report_failure(mapped); }
        RequestOutcome { underlying, mapped, reported }
    }

    #[inline(always)]
    pub fn read_request(&mut self, descriptor: RequestDescriptor, destination: &mut [u8]) -> RequestOutcome {
        let status = self.export(descriptor.range(), destination);
        self.report(status)
    }

}

impl<D: BackupDriver> SharedBacking<'_, D> {
    #[inline(always)]
    pub fn import(&mut self, range: BackingRange, source: &[u8]) -> BackingWriteOutcome {
        let mut outcome = self.import_before_leave(range, source);
        if outcome.status == BackingStatus::Success && range.end() > 0x140 {
            let custom: &[u8; CUSTOM_BYTES] = self.live[0x140..0x168].try_into().ok().unwrap();
            self.driver.refresh_custom(custom);
            outcome.custom_refreshed = true;
        }
        self.leave();
        outcome
    }

    #[inline(always)]
    pub fn write_request(&mut self, descriptor: RequestDescriptor, source: &[u8]) -> MirrorOutcome {
        let outcome = self.import(descriptor.range(), source);
        MirrorOutcome { request: self.report(outcome.status), synchronized: outcome.synchronized }
    }
}

impl<D: HeaderBackupDriver> HeaderBackend for SharedBacking<'_, D> {
    #[inline(always)]
    fn read_header(&mut self, destination: &mut [u8; HEADER_BYTES]) -> RequestOutcome {
        self.read_request(RequestDescriptor::from_abi(HEADER_BYTES as u32, 0), destination)
    }
    #[inline(always)]
    fn write_header(&mut self, source: &[u8; HEADER_BYTES]) -> MirrorOutcome {
        let outcome = self.import_before_leave(BackingRange::new(0, HEADER_BYTES as u32), source);
        self.leave();
        MirrorOutcome { request: self.report(outcome.status), synchronized: outcome.synchronized }
    }
}
impl<D: BackupDriver> SettingsBackend for SharedBacking<'_, D> {
    fn read_body(&mut self, destination: &mut [u8; BODY_BYTES]) -> RequestOutcome {
        self.read_request(RequestDescriptor::from_abi(BODY_BYTES as u32, 0x10), destination)
    }
    fn write_body(&mut self, source: &[u8; BODY_BYTES]) -> BodyOutcome {
        let outcome = self.write_request(RequestDescriptor::from_abi(BODY_BYTES as u32, 0x10), source);
        BodyOutcome { request: outcome.request, synchronized: outcome.synchronized }
    }
    fn read_custom(&mut self, destination: &mut [u8; CUSTOM_BYTES]) -> RequestOutcome {
        self.read_request(RequestDescriptor::from_abi(CUSTOM_BYTES as u32, 0x140), destination)
    }
}

#[cfg(test)]
#[path = "mirror_tests.rs"]
mod tests;
