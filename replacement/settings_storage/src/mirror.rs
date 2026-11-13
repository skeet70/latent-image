pub const HEADER_BYTES: usize = 16;
pub const BODY_BYTES: usize = 0x118;
pub const CUSTOM_BYTES: usize = 40;
pub const BACKING_BYTES: usize = 0x19a;
pub const INITIALIZED_EXPORT_BYTES: usize = 0x168;
pub const EXPORT_RESIDUE_BYTES: usize = BACKING_BYTES - INITIALIZED_EXPORT_BYTES;

/// `Window` writes through the storage window leaves, `Leaves` through the
/// caller-buffer output leaves.
#[derive(Clone, Copy)]
pub enum ExportSink {
    Leaves,
    Window,
}

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
    /// Reads the header through the storage window leaves.
    #[inline(always)]
    fn read_header_window(&mut self, destination: &mut [u8; HEADER_BYTES]) -> RequestOutcome {
        self.read_header(destination)
    }
    fn write_header(&mut self, source: &[u8; HEADER_BYTES]) -> MirrorOutcome;
    /// Writes the header and leaves the report in `report`.
    #[inline(always)]
    fn write_header_into(&mut self, source: &[u8; HEADER_BYTES], report: &mut MirrorOutcome) {
        *report = self.write_header(source);
    }
    /// Writes the header and keeps no report.
    #[inline(always)]
    fn write_header_discarding(&mut self, source: &[u8; HEADER_BYTES]) {
        let mut report = MirrorOutcome::unchanged();
        self.write_header_into(source, &mut report);
    }
}
pub trait SettingsBackend: HeaderBackend {
    fn read_body(&mut self, destination: &mut [u8; BODY_BYTES]) -> RequestOutcome;
    /// Reads the body through the storage window leaves.
    #[inline(always)]
    fn read_body_window(&mut self, destination: &mut [u8; BODY_BYTES]) -> RequestOutcome {
        self.read_body(destination)
    }
    fn write_body(&mut self, source: &[u8; BODY_BYTES]) -> BodyOutcome;
    fn read_custom(&mut self, destination: &mut [u8; CUSTOM_BYTES]) -> RequestOutcome;
    /// Writes the body and leaves the report in `report`.
    #[inline(always)]
    fn write_body_into(&mut self, source: &[u8; BODY_BYTES], report: &mut BodyOutcome) {
        *report = self.write_body(source);
    }
    /// Writes the body and keeps no report.
    #[inline(always)]
    fn write_body_discarding(&mut self, source: &[u8; BODY_BYTES]) {
        let mut report = BodyOutcome::unchanged();
        self.write_body_into(source, &mut report);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestDirection { Read, Write }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupSemaphore { Settings }

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
    type Task;
    fn task(&mut self) -> Option<Self::Task>;
    fn acquire(&mut self, task: &Self::Task, semaphore: BackupSemaphore);
    fn release(&mut self, task: &Self::Task, semaphore: BackupSemaphore);
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

impl BackingWriteOutcome {
    pub const ZERO: Self = Self { status: BackingStatus::Success, synchronized: [None; HEADER_BYTES], custom_refreshed: false };
}

pub struct SharedBacking<'a, D> {
    live: &'a mut [u8; BACKING_BYTES],
    shadow: &'a mut [u8; HEADER_BYTES],
    keys: &'a [BackupKey; HEADER_BYTES],
    residue: Option<&'a [u8; EXPORT_RESIDUE_BYTES]>,
    driver: D,
}

/// Proof that this task holds the settings semaphore.
pub struct SemaphoreHold<'a, 'b, D: HeaderBackupDriver> {
    backing: &'a mut SharedBacking<'b, D>,
    task: D::Task,
}

impl<D: HeaderBackupDriver> Drop for SemaphoreHold<'_, '_, D> {
    fn drop(&mut self) { self.backing.driver.release(&self.task, BackupSemaphore::Settings); }
}

#[inline(always)]
fn request_outcome(underlying: BackingStatus) -> RequestOutcome {
    let mapped = if underlying == BackingStatus::Success { RequestStatus::Success } else { RequestStatus::Failed };
    RequestOutcome { underlying, mapped, reported: mapped == RequestStatus::Failed }
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

    /// The task context, or the outside-task diagnostic when there is none.
    fn task(&mut self) -> Option<D::Task> {
        let task = self.driver.task();
        if task.is_none() { self.driver.diagnostic(BackupDiagnostic::OutsideTask); }
        task
    }

    /// Takes the settings semaphore for `task`.
    fn acquire(&mut self, task: D::Task) -> SemaphoreHold<'_, 'a, D> {
        self.driver.acquire(&task, BackupSemaphore::Settings);
        SemaphoreHold { backing: self, task }
    }

    #[inline(always)]
    fn store_request(&mut self, request: RequestOutcome) {
        self.driver.store_request_status(request.mapped);
        if request.reported { self.driver.report_failure(request.mapped); }
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
    pub fn export(&mut self, range: BackingRange, destination: &mut [u8], sink: ExportSink) -> BackingStatus {
        match self.task() {
            Some(task) => { let hold = self.acquire(task); hold.backing.export_unheld(range, destination, sink) }
            None => self.export_unheld(range, destination, sink),
        }
    }

    #[inline(always)]
    fn export_unheld(&mut self, range: BackingRange, destination: &mut [u8], sink: ExportSink) -> BackingStatus {
        match self.driver.admit(RequestDirection::Read) {
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
                            match sink {
                                ExportSink::Leaves => crate::memory::write_output_halfword(destination, written, value),
                                ExportSink::Window => crate::memory::write_halfword(destination, written, value),
                            }
                            written += 2;
                        }
                    }
                    while written < initialized {
                        let value = self.export_byte(projection.start + written);
                        match sink {
                            ExportSink::Leaves => crate::memory::write_output_byte(destination, written, value),
                            ExportSink::Window => crate::memory::write_byte_at(destination, written, value),
                        }
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
        }
    }

    /// Records each header byte this write synchronised, when the caller keeps them.
    #[inline(always)]
    fn synchronize_observing(&mut self, mut observations: Option<&mut [Option<DriverObservation>; HEADER_BYTES]>) {
        let checksum = self.live[..15].iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte)).wrapping_neg();
        crate::memory::write_byte(&mut self.live[15], checksum);
        for index in 0..HEADER_BYTES {
            if crate::memory::read_byte(&*self.live, index) != crate::memory::read_byte(&*self.shadow, index) {
                let key = self.keys[index];
                let value = BackupValue(crate::memory::read_byte(&*self.live, index));
                let status = self.driver.write_backup(key, value, BackupWriteMode::Immediate);
                crate::memory::write_byte_at(&mut *self.shadow, index, crate::memory::read_byte(&*self.live, index));
                if let Some(observations) = observations.as_mut() {
                    observations[index] = Some(DriverObservation { position: HeaderPosition(index as u8), key, value, status });
                }
            }
        }
    }

    #[inline(always)]
    pub fn synchronize_into(&mut self, observations: &mut [Option<DriverObservation>; HEADER_BYTES]) {
        self.synchronize_observing(Some(observations))
    }

    /// Copies `range` into the live backing, synchronising the header it covers.
    #[inline(always)]
    fn import_copy_into(&mut self, range: BackingRange, source: &[u8], mut observations: Option<&mut [Option<DriverObservation>; HEADER_BYTES]>) -> BackingStatus {
        if let Some(observations) = observations.as_mut() {
            for observation in observations.iter_mut() { *observation = None; }
        }
        match self.driver.admit(RequestDirection::Write) {
            Err(status) => BackingStatus::Refused(status),
            Ok(()) => match range.projection(source.len()) {
                Err(status) => status,
                Ok(projection) => {
                    crate::memory::copy_bytes(self.live.get_mut(projection.clone()).unwrap(), source.get(..projection.len()).unwrap());
                    self.synchronize_observing(observations);
                    BackingStatus::Success
                }
            },
        }
    }

    #[inline(always)]
    pub fn read_request(&mut self, descriptor: RequestDescriptor, destination: &mut [u8]) -> RequestOutcome {
        self.read_request_with(descriptor, destination, ExportSink::Leaves)
    }

    #[inline(always)]
    pub fn read_request_with(&mut self, descriptor: RequestDescriptor, destination: &mut [u8], sink: ExportSink) -> RequestOutcome {
        let request = request_outcome(self.export(descriptor.range(), destination, sink));
        self.store_request(request);
        request
    }

}

impl<D: BackupDriver> SharedBacking<'_, D> {
    #[inline(always)]
    fn import_unheld(&mut self, range: BackingRange, source: &[u8], outcome: &mut BackingWriteOutcome) {
        outcome.custom_refreshed = false;
        outcome.status = self.import_copy_into(range, source, Some(&mut outcome.synchronized));
        if outcome.status == BackingStatus::Success && range.end() > 0x140 {
            let custom: &[u8; CUSTOM_BYTES] = self.live[0x140..0x168].try_into().ok().unwrap();
            self.driver.refresh_custom(custom);
            outcome.custom_refreshed = true;
        }
    }

    #[inline(always)]
    pub fn import_into(&mut self, range: BackingRange, source: &[u8], outcome: &mut BackingWriteOutcome) {
        match self.task() {
            Some(task) => { let hold = self.acquire(task); hold.backing.import_unheld(range, source, outcome) }
            None => self.import_unheld(range, source, outcome),
        }
    }

    #[inline(always)]
    pub fn import(&mut self, range: BackingRange, source: &[u8]) -> BackingWriteOutcome {
        let mut outcome = BackingWriteOutcome::ZERO;
        self.import_into(range, source, &mut outcome);
        outcome
    }

    #[inline(always)]
    pub fn write_request(&mut self, descriptor: RequestDescriptor, source: &[u8]) -> MirrorOutcome {
        let mut outcome = BackingWriteOutcome::ZERO;
        self.import_into(descriptor.range(), source, &mut outcome);
        let request = request_outcome(outcome.status);
        self.store_request(request);
        MirrorOutcome { request, synchronized: outcome.synchronized }
    }

    /// Writes the body and leaves the report in `report`.
    #[inline(always)]
    pub fn write_body_into(&mut self, source: &[u8; BODY_BYTES], report: &mut BodyOutcome) {
        let request = match self.task() {
            Some(task) => { let hold = self.acquire(task); hold.backing.write_body_unheld(source, Some(report)) }
            None => self.write_body_unheld(source, Some(report)),
        };
        self.store_request(request);
    }

    /// Writes the body and keeps no report.
    #[inline(always)]
    pub fn write_body_discarding(&mut self, source: &[u8; BODY_BYTES]) {
        let request = match self.task() {
            Some(task) => { let hold = self.acquire(task); hold.backing.write_body_unheld(source, None) }
            None => self.write_body_unheld(source, None),
        };
        self.store_request(request);
    }

    #[inline(always)]
    fn write_body_unheld(&mut self, source: &[u8; BODY_BYTES], report: Option<&mut BodyOutcome>) -> RequestOutcome {
        let range = RequestDescriptor::from_abi(BODY_BYTES as u32, 0x10).range();
        match report {
            Some(report) => {
                let request = request_outcome(self.import_copy_into(range, source, Some(&mut report.synchronized)));
                report.request = request;
                request
            }
            None => request_outcome(self.import_copy_into(range, source, None)),
        }
    }
}

impl<D: HeaderBackupDriver> SharedBacking<'_, D> {
    /// Writes the header and leaves the report in `report`.
    #[inline(always)]
    pub fn write_header_into(&mut self, source: &[u8; HEADER_BYTES], report: &mut MirrorOutcome) {
        let request = match self.task() {
            Some(task) => { let hold = self.acquire(task); hold.backing.write_header_unheld(source, Some(report)) }
            None => self.write_header_unheld(source, Some(report)),
        };
        self.store_request(request);
    }

    /// Writes the header and keeps no report.
    #[inline(always)]
    pub fn write_header_discarding(&mut self, source: &[u8; HEADER_BYTES]) {
        let request = match self.task() {
            Some(task) => { let hold = self.acquire(task); hold.backing.write_header_unheld(source, None) }
            None => self.write_header_unheld(source, None),
        };
        self.store_request(request);
    }

    #[inline(always)]
    fn write_header_unheld(&mut self, source: &[u8; HEADER_BYTES], report: Option<&mut MirrorOutcome>) -> RequestOutcome {
        let range = BackingRange::new(0, HEADER_BYTES as u32);
        match report {
            Some(report) => {
                let request = request_outcome(self.import_copy_into(range, source, Some(&mut report.synchronized)));
                report.request = request;
                request
            }
            None => request_outcome(self.import_copy_into(range, source, None)),
        }
    }
}
impl<D: HeaderBackupDriver> HeaderBackend for SharedBacking<'_, D> {
    #[inline(always)]
    fn read_header(&mut self, destination: &mut [u8; HEADER_BYTES]) -> RequestOutcome {
        self.read_request(RequestDescriptor::from_abi(HEADER_BYTES as u32, 0), destination)
    }
    #[inline(always)]
    fn read_header_window(&mut self, destination: &mut [u8; HEADER_BYTES]) -> RequestOutcome {
        self.read_request_with(RequestDescriptor::from_abi(HEADER_BYTES as u32, 0), destination, ExportSink::Window)
    }
    #[inline(always)]
    fn write_header(&mut self, source: &[u8; HEADER_BYTES]) -> MirrorOutcome {
        let mut report = MirrorOutcome::unchanged();
        self.write_header_into(source, &mut report);
        report
    }
    #[inline(always)]
    fn write_header_into(&mut self, source: &[u8; HEADER_BYTES], report: &mut MirrorOutcome) {
        SharedBacking::write_header_into(self, source, report)
    }
    #[inline(always)]
    fn write_header_discarding(&mut self, source: &[u8; HEADER_BYTES]) {
        SharedBacking::write_header_discarding(self, source)
    }
}
impl<D: BackupDriver> SettingsBackend for SharedBacking<'_, D> {
    fn read_body(&mut self, destination: &mut [u8; BODY_BYTES]) -> RequestOutcome {
        self.read_request(RequestDescriptor::from_abi(BODY_BYTES as u32, 0x10), destination)
    }
    fn read_body_window(&mut self, destination: &mut [u8; BODY_BYTES]) -> RequestOutcome {
        self.read_request_with(RequestDescriptor::from_abi(BODY_BYTES as u32, 0x10), destination, ExportSink::Window)
    }
    fn write_body(&mut self, source: &[u8; BODY_BYTES]) -> BodyOutcome {
        let outcome = self.write_request(RequestDescriptor::from_abi(BODY_BYTES as u32, 0x10), source);
        BodyOutcome { request: outcome.request, synchronized: outcome.synchronized }
    }
    fn read_custom(&mut self, destination: &mut [u8; CUSTOM_BYTES]) -> RequestOutcome {
        self.read_request(RequestDescriptor::from_abi(CUSTOM_BYTES as u32, 0x140), destination)
    }
    #[inline(always)]
    fn write_body_into(&mut self, source: &[u8; BODY_BYTES], report: &mut BodyOutcome) {
        SharedBacking::write_body_into(self, source, report)
    }
    #[inline(always)]
    fn write_body_discarding(&mut self, source: &[u8; BODY_BYTES]) {
        SharedBacking::write_body_discarding(self, source)
    }
}

#[cfg(test)]
#[path = "mirror_tests.rs"]
mod tests;
