use super::{endpoint, mapping_tag, mark_bad_pages, ApplicationNand, BlockCount, ByteOffset, Error, LogicalBlock, MediaEndpoint, PageReadBytes, PhysicalBlock, PhysicalPage, ReadStatus, Timeout, Transfer, Verification, BLOCK_BYTES, PAGE_BYTES, PAGES_PER_BLOCK, PROFILE_BASE, PROFILE_PHYSICAL_BLOCKS};
use super::physical::{PhysicalBlockCount, SpareLayout};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageDigest(pub [u8; 32]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageProvenance { Captured(ImageDigest), Synthetic }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostConfiguration { G1StrictRejectBeforeMutation, LegacyAndCommitFailedStatus }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LawProvenance { PinnedMediumPart, FujiModControllerModelSynthetic }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceGeneration { SharedProfileEndpointV1 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimingLaw { Synthetic512BytesPerRequestTick }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureProgressLaw { SyntheticProgramPrefix1024EraseBeforeMutation }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancellationLaw { PreserveCompletedPrefix }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageError { Layout, Cancelled, Finished, AccountingOverflow, JournalFull, AlreadyStarted }

impl SourceGeneration {
    pub const fn label(self) -> &'static str { match self { Self::SharedProfileEndpointV1 => "shared-profile-endpoint-v1" } }
}
impl HostConfiguration {
    pub const fn label(self) -> &'static str {
        match self {
            Self::G1StrictRejectBeforeMutation => "g1-strict-reject-before-mutation",
            Self::LegacyAndCommitFailedStatus => "legacy-and-commit-failed-status",
        }
    }
    pub const fn provenance(self) -> LawProvenance {
        match self {
            Self::G1StrictRejectBeforeMutation => LawProvenance::PinnedMediumPart,
            Self::LegacyAndCommitFailedStatus => LawProvenance::FujiModControllerModelSynthetic,
        }
    }
}
impl LawProvenance {
    pub const fn label(self) -> &'static str {
        match self {
            Self::PinnedMediumPart => "pinned-medium-part",
            Self::FujiModControllerModelSynthetic => "fujimod-controller-model-synthetic",
        }
    }
}
impl TimingLaw {
    pub const fn label(self) -> &'static str { match self { Self::Synthetic512BytesPerRequestTick => "synthetic-512-bytes-per-request-tick" } }
}
impl FailureProgressLaw {
    pub const fn label(self) -> &'static str { match self { Self::SyntheticProgramPrefix1024EraseBeforeMutation => "synthetic-program-prefix-1024-erase-before-mutation" } }
}
impl CancellationLaw {
    pub const fn label(self) -> &'static str { match self { Self::PreserveCompletedPrefix => "preserve-completed-prefix" } }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LawIdentity {
    pub generation: SourceGeneration,
    pub configuration: HostConfiguration,
    pub configuration_provenance: LawProvenance,
    pub timing: TimingLaw,
    pub injected_failure: FailureProgressLaw,
    pub cancellation: CancellationLaw,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FailureCount(u16);
impl FailureCount {
    pub const fn new(count: u16) -> Self { Self(count) }
    pub const fn get(self) -> u16 { self.0 }
    fn consume(&mut self) -> bool {
        if self.0 == 0 { false } else { self.0 -= 1; true }
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BlockFailures {
    pub read: FailureCount,
    pub program: FailureCount,
    pub erase: FailureCount,
    pub mark_bad: FailureCount,
    pub read_cancel: FailureCount,
    pub program_cancel: FailureCount,
    pub erase_cancel: FailureCount,
    pub mark_bad_cancel: FailureCount,
    pub tagged_read: FailureCount,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OperationCount(u64);
impl OperationCount { pub const fn get(self) -> u64 { self.0 } }
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProcessedBytes(u64);
impl ProcessedBytes { pub const fn get(self) -> u64 { self.0 } }
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WorkBytes(u32);
impl WorkBytes { pub const fn get(self) -> u32 { self.0 } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StepBytes(u16);
impl StepBytes {
    pub const SECTOR: Self = Self(512);
    pub fn new(bytes: u16) -> Result<Self, Error> {
        if bytes == 0 { Err(Error::Range) } else { Ok(Self(bytes)) }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation { ReadPage(PhysicalPage), ReadSpare(PhysicalPage), ProgramPage(PhysicalPage), EraseBlock(PhysicalBlock) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationOutcome { Pending, Complete, TaggedFault(WorkBytes), Failed(Error), Cancelled }
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OperationProgress { pub data: WorkBytes, pub spare: WorkBytes }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationRecord {
    pub sequence: OperationCount,
    pub operation: Operation,
    pub progress: OperationProgress,
    pub total: OperationProgress,
    pub outcome: OperationOutcome,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModelObservation {
    pub generation: SourceGeneration,
    pub configuration: HostConfiguration,
    pub provenance: ImageProvenance,
    pub timing: TimingLaw,
    pub injected_failure: FailureProgressLaw,
    pub cancellation: CancellationLaw,
    pub submitted: OperationCount,
    pub completed: OperationCount,
    pub failed: OperationCount,
    pub cancelled: OperationCount,
    pub processed_data: ProcessedBytes,
    pub processed_spare: ProcessedBytes,
    pub first_failure: Option<OperationRecord>,
    pub last: Option<OperationRecord>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ProgramPurpose { Data, BadMarker }
struct ProgramMetadata { bytes: [u8; 16], purpose: ProgramPurpose }

pub struct MemoryNand<'image> {
    base: PhysicalBlock,
    data: &'image mut [u8],
    spare: &'image mut [[u8; 16]],
    failures: &'image mut [BlockFailures],
    metadata: ProgramMetadata,
    observation: ModelObservation,
    journal: Option<&'image mut [Option<OperationRecord>]>,
    journal_used: usize,
}

impl<'image> MemoryNand<'image> {
    pub fn adopt(base: PhysicalBlock, data: &'image mut [u8], spare: &'image mut [[u8; 16]], failures: &'image mut [BlockFailures], provenance: ImageProvenance, configuration: HostConfiguration) -> Result<Self, Error> {
        let blocks = data.len() / BLOCK_BYTES as usize;
        if blocks == 0 || data.len() % BLOCK_BYTES as usize != 0 || spare.len() != data.len() / PAGE_BYTES || failures.len() != blocks || blocks > (1024 - base.index()) as usize {
            return Err(Error::Image(ImageError::Layout));
        }
        Ok(Self {
            base, data, spare, failures,
            metadata: ProgramMetadata { bytes: [0xff; 16], purpose: ProgramPurpose::Data },
            observation: ModelObservation {
                generation: SourceGeneration::SharedProfileEndpointV1, configuration, provenance,
                timing: TimingLaw::Synthetic512BytesPerRequestTick,
                injected_failure: FailureProgressLaw::SyntheticProgramPrefix1024EraseBeforeMutation,
                cancellation: CancellationLaw::PreserveCompletedPrefix,
                submitted: OperationCount(0), completed: OperationCount(0), failed: OperationCount(0), cancelled: OperationCount(0),
                processed_data: ProcessedBytes(0), processed_spare: ProcessedBytes(0), first_failure: None, last: None,
            },
            journal: None, journal_used: 0,
        })
    }

    pub const fn provenance(&self) -> ImageProvenance { self.observation.provenance }
    pub const fn configuration(&self) -> HostConfiguration { self.observation.configuration }
    pub fn bytes(&self) -> &[u8] { self.data }
    pub fn spare_records(&self) -> &[[u8; 16]] { self.spare }
    pub fn failures_mut(&mut self) -> &mut [BlockFailures] { self.failures }
    pub fn observation(&self) -> &ModelObservation { &self.observation }
    pub const fn laws(&self) -> LawIdentity {
        LawIdentity {
            generation: self.observation.generation,
            configuration: self.observation.configuration,
            configuration_provenance: self.observation.configuration.provenance(),
            timing: self.observation.timing,
            injected_failure: self.observation.injected_failure,
            cancellation: self.observation.cancellation,
        }
    }
    pub fn journal(&self) -> Option<&[Option<OperationRecord>]> {
        self.journal.as_deref().map(|journal| &journal[..self.journal_used])
    }
    pub fn attach_journal(&mut self, journal: &'image mut [Option<OperationRecord>]) -> Result<(), Error> {
        if self.observation.submitted.0 != 0 || self.journal.is_some() { return Err(Error::Image(ImageError::AlreadyStarted)); }
        self.journal = Some(journal);
        Ok(())
    }

    fn page_index(&self, page: PhysicalPage) -> Result<usize, Error> {
        let relative = page.index().checked_sub(self.base.index() * PAGES_PER_BLOCK).ok_or(Error::Range)? as usize;
        if relative < self.spare.len() { Ok(relative) } else { Err(Error::Range) }
    }
    fn block_index(&self, block: PhysicalBlock) -> Result<usize, Error> { self.page_index(block.page(0)) }

    fn start<'operation>(&'operation mut self, request: Request<'operation>) -> Result<MemoryOperation<'operation, 'image>, Error> {
        let (operation, index, total) = match &request {
            Request::ReadPage { page, .. } => (Operation::ReadPage(*page), self.page_index(*page)?, OperationProgress { data: WorkBytes(PAGE_BYTES as u32), spare: WorkBytes(0) }),
            Request::ReadSpare { page, .. } => (Operation::ReadSpare(*page), self.page_index(*page)?, OperationProgress { data: WorkBytes(0), spare: WorkBytes(16) }),
            Request::Program { page, .. } => (Operation::ProgramPage(*page), self.page_index(*page)?, OperationProgress { data: WorkBytes(PAGE_BYTES as u32), spare: WorkBytes(16) }),
            Request::Erase { block } => (Operation::EraseBlock(*block), self.block_index(*block)?, OperationProgress { data: WorkBytes(BLOCK_BYTES), spare: WorkBytes(PAGES_PER_BLOCK * 16) }),
        };
        if self.journal.as_ref().is_some_and(|journal| self.journal_used == journal.len()) { return Err(Error::Image(ImageError::JournalFull)); }
        self.observation.submitted.0 = self.observation.submitted.0.checked_add(1).ok_or(Error::Image(ImageError::AccountingOverflow))?;
        let record = OperationRecord { sequence: self.observation.submitted, operation, progress: OperationProgress::default(), total, outcome: OperationOutcome::Pending };
        let journal_slot = self.journal_used;
        if let Some(journal) = self.journal.as_deref_mut() { journal[journal_slot] = Some(record); self.journal_used += 1; }
        self.observation.last = Some(record);
        Ok(MemoryOperation { nand: self, request, index, record, journal_slot, plan: None, illegal_bits: false })
    }

    pub fn begin_read_page<'operation>(&'operation mut self, page: PhysicalPage, output: &'operation mut [u8; PAGE_BYTES]) -> Result<MemoryOperation<'operation, 'image>, Error> {
        self.start(Request::ReadPage { page, output, transfer: Transfer::Tagged })
    }
    pub fn begin_read_spare<'operation>(&'operation mut self, page: PhysicalPage, output: &'operation mut [u8; 16]) -> Result<MemoryOperation<'operation, 'image>, Error> {
        self.start(Request::ReadSpare { page, output })
    }
    pub fn begin_program_page<'operation>(&'operation mut self, page: PhysicalPage, data: &'operation [u8; PAGE_BYTES], spare: &'operation [u8; 16]) -> Result<MemoryOperation<'operation, 'image>, Error> {
        self.start(Request::Program { page, data, spare: SpareInput::Supplied(spare) })
    }
    pub fn begin_erase_block(&mut self, block: PhysicalBlock) -> Result<MemoryOperation<'_, 'image>, Error> { self.start(Request::Erase { block }) }

    pub fn read_spare(&mut self, page: PhysicalPage) -> Result<[u8; 16], Error> {
        let mut spare = [0; 16];
        self.begin_read_spare(page, &mut spare)?.finish()?;
        Ok(spare)
    }
    pub fn read_page(&mut self, page: PhysicalPage, output: &mut [u8; PAGE_BYTES]) -> Result<(), Error> {
        clean(self.begin_read_page(page, output)?.finish()?)
    }
    pub fn read_block(&mut self, block: PhysicalBlock, output: &mut [u8]) -> Result<(), Error> {
        self.block_index(block)?;
        if output.len() != BLOCK_BYTES as usize { return Err(Error::Range); }
        for (page, output) in output.chunks_exact_mut(PAGE_BYTES).enumerate() {
            self.read_page(block.page(page as u32), output.try_into().map_err(|_| Error::Range)?)?;
        }
        Ok(())
    }
    pub fn program_page(&mut self, page: PhysicalPage, data: &[u8; PAGE_BYTES], spare: &[u8; 16]) -> Result<(), Error> {
        self.begin_program_page(page, data, spare)?.finish().map(|_| ())
    }
    pub fn erase_block(&mut self, block: PhysicalBlock) -> Result<(), Error> { self.begin_erase_block(block)?.finish().map(|_| ()) }
    pub fn mark_bad(&mut self, block: PhysicalBlock, timeout: Timeout) -> Result<(), Error> {
        self.block_index(block)?;
        static ERASED: [u8; PAGE_BYTES] = [0xff; PAGE_BYTES];
        unsafe { mark_bad_pages(self, block, &ERASED, Transfer::Tagged, timeout) }
    }
    pub fn read_with_spare(&mut self, first: PhysicalBlock, count: PhysicalBlockCount, layout: SpareLayout, output: &mut [u8]) -> Result<(), Error> {
        if first.index() + count.blocks() > 1024 { return Err(Error::Range); }
        let stride = if layout == SpareLayout::Interleaved { PAGE_BYTES + 64 } else { PAGE_BYTES };
        let pages = count.blocks() as usize * PAGES_PER_BLOCK as usize;
        if output.len() != pages * stride { return Err(Error::Range); }
        let index = self.block_index(first)?;
        if index + pages > self.spare.len() { return Err(Error::Range); }
        for (page, output) in output.chunks_exact_mut(stride).enumerate() {
            let row = first.page(page as u32);
            let data = (&mut output[..PAGE_BYTES]).try_into().map_err(|_| Error::Range)?;
            self.start(Request::ReadPage { page: row, output: data, transfer: Transfer::Masked })?.finish()?;
            if layout == SpareLayout::Interleaved { output[PAGE_BYTES..PAGE_BYTES + 16].copy_from_slice(&self.read_spare(row)?); }
        }
        Ok(())
    }
}

fn clean(record: OperationRecord) -> Result<(), Error> {
    match record.outcome { OperationOutcome::TaggedFault(_) => Err(Error::ReadFault), OperationOutcome::Complete => Ok(()), OperationOutcome::Failed(error) => Err(error), OperationOutcome::Cancelled => Err(Error::Image(ImageError::Cancelled)), OperationOutcome::Pending => Err(Error::Image(ImageError::Finished)) }
}

enum SpareInput<'operation> { Supplied(&'operation [u8; 16]), Staged }
enum Request<'operation> {
    ReadPage { page: PhysicalPage, output: &'operation mut [u8; PAGE_BYTES], transfer: Transfer },
    ReadSpare { page: PhysicalPage, output: &'operation mut [u8; 16] },
    Program { page: PhysicalPage, data: &'operation [u8; PAGE_BYTES], spare: SpareInput<'operation> },
    Erase { block: PhysicalBlock },
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum Plan { Complete, ReadFailure, ProgramPrefix, EraseFailure, CancelPrefix, TaggedPrefix }

pub struct MemoryOperation<'operation, 'image> {
    nand: &'operation mut MemoryNand<'image>,
    request: Request<'operation>,
    index: usize,
    record: OperationRecord,
    journal_slot: usize,
    plan: Option<Plan>,
    illegal_bits: bool,
}

impl MemoryOperation<'_, '_> {
    pub fn observation(&self) -> OperationRecord { self.record }

    fn publish(&mut self) {
        self.nand.observation.last = Some(self.record);
        if let Some(journal) = self.nand.journal.as_deref_mut() { journal[self.journal_slot] = Some(self.record); }
    }
    fn terminal(&mut self, outcome: OperationOutcome) {
        self.record.outcome = outcome;
        match outcome {
            OperationOutcome::Complete | OperationOutcome::TaggedFault(_) => self.nand.observation.completed.0 += 1,
            OperationOutcome::Failed(_) => {
                self.nand.observation.failed.0 += 1;
                if self.nand.observation.first_failure.is_none() { self.nand.observation.first_failure = Some(self.record); }
            },
            OperationOutcome::Cancelled => self.nand.observation.cancelled.0 += 1,
            OperationOutcome::Pending => {},
        }
        self.publish();
    }
    fn fail(&mut self, error: Error) -> Result<OperationRecord, Error> { self.terminal(OperationOutcome::Failed(error)); Err(error) }

    fn strict_violation(&self) -> bool {
        let Request::Program { data, spare, .. } = &self.request else { return false; };
        let spare = match spare { SpareInput::Supplied(bytes) => *bytes, SpareInput::Staged => &self.nand.metadata.bytes };
        let start = self.index * PAGE_BYTES;
        self.nand.data[start..start + PAGE_BYTES].iter().zip(data.iter()).any(|(&old, &new)| old & new != new)
            || self.nand.spare[self.index].iter().zip(spare).any(|(&old, &new)| old & new != new)
    }
    fn select_plan(&mut self) -> Plan {
        let failure = &mut self.nand.failures[self.index / PAGES_PER_BLOCK as usize];
        match &self.request {
            Request::ReadPage { transfer, .. } => {
                if failure.read_cancel.consume() { Plan::CancelPrefix }
                else if failure.read.consume() { Plan::ReadFailure }
                else if *transfer == Transfer::Tagged && failure.tagged_read.consume() { Plan::TaggedPrefix }
                else { Plan::Complete }
            },
            Request::ReadSpare { .. } => {
                if failure.read_cancel.consume() { Plan::CancelPrefix }
                else if failure.read.consume() { Plan::ReadFailure }
                else { Plan::Complete }
            },
            Request::Program { spare, .. } => {
                let marker = matches!(spare, SpareInput::Staged) && self.nand.metadata.purpose == ProgramPurpose::BadMarker;
                let (cancel, fail) = if marker { (&mut failure.mark_bad_cancel, &mut failure.mark_bad) } else { (&mut failure.program_cancel, &mut failure.program) };
                if cancel.consume() { Plan::CancelPrefix } else if fail.consume() { Plan::ProgramPrefix } else { Plan::Complete }
            },
            Request::Erase { .. } => {
                if failure.erase_cancel.consume() { Plan::CancelPrefix }
                else if failure.erase.consume() { Plan::EraseFailure }
                else { Plan::Complete }
            },
        }
    }

    fn transfer(&mut self, data_end: u32, spare_end: u32) {
        let data_start = self.record.progress.data.0 as usize;
        let data_end = data_end as usize;
        let spare_start = self.record.progress.spare.0 as usize;
        let spare_end = spare_end as usize;
        let base = self.index * PAGE_BYTES;
        match &mut self.request {
            Request::ReadPage { output, .. } => output[data_start..data_end].copy_from_slice(&self.nand.data[base + data_start..base + data_end]),
            Request::ReadSpare { output, .. } => output[spare_start..spare_end].copy_from_slice(&self.nand.spare[self.index][spare_start..spare_end]),
            Request::Program { data, spare, .. } => {
                let spare = match spare { SpareInput::Supplied(bytes) => *bytes, SpareInput::Staged => &self.nand.metadata.bytes };
                match self.nand.observation.configuration {
                    HostConfiguration::G1StrictRejectBeforeMutation => {
                        self.nand.data[base + data_start..base + data_end].copy_from_slice(&data[data_start..data_end]);
                        self.nand.spare[self.index][spare_start..spare_end].copy_from_slice(&spare[spare_start..spare_end]);
                    },
                    HostConfiguration::LegacyAndCommitFailedStatus => {
                        for (old, &new) in self.nand.data[base + data_start..base + data_end].iter_mut().zip(&data[data_start..data_end]) { self.illegal_bits |= *old & new != new; *old &= new; }
                        for (old, &new) in self.nand.spare[self.index][spare_start..spare_end].iter_mut().zip(&spare[spare_start..spare_end]) { self.illegal_bits |= *old & new != new; *old &= new; }
                    },
                }
            },
            Request::Erase { .. } => {
                self.nand.data[base + data_start..base + data_end].fill(0xff);
                self.nand.spare[self.index..self.index + PAGES_PER_BLOCK as usize].as_flattened_mut()[spare_start..spare_end].fill(0xff);
            },
        }
    }

    pub fn advance(&mut self, budget: StepBytes) -> Result<OperationRecord, Error> {
        if self.record.outcome != OperationOutcome::Pending { return Err(Error::Image(ImageError::Finished)); }
        if self.plan.is_none() {
            if self.nand.observation.configuration == HostConfiguration::G1StrictRejectBeforeMutation && self.strict_violation() { return self.fail(Error::ProgramFailed); }
            self.plan = Some(self.select_plan());
        }
        let plan = self.plan.unwrap_or(Plan::Complete);
        if plan == Plan::ReadFailure { return self.fail(Error::ReadFault); }
        if plan == Plan::EraseFailure { return self.fail(Error::ProgramFailed); }
        let total = self.record.total.data.0 + self.record.total.spare.0;
        let limit = match plan { Plan::ProgramPrefix => 1024, Plan::CancelPrefix => total.min(512), Plan::TaggedPrefix => 512, _ => total };
        let cursor = self.record.progress.data.0 + self.record.progress.spare.0;
        let end = (cursor + u32::from(budget.0)).min(limit);
        let data_end = end.min(self.record.total.data.0);
        let spare_end = end.saturating_sub(self.record.total.data.0);
        let data_count = u64::from(data_end - self.record.progress.data.0);
        let spare_count = u64::from(spare_end - self.record.progress.spare.0);
        let Some(processed_data) = self.nand.observation.processed_data.0.checked_add(data_count) else { return self.fail(Error::Image(ImageError::AccountingOverflow)); };
        let Some(processed_spare) = self.nand.observation.processed_spare.0.checked_add(spare_count) else { return self.fail(Error::Image(ImageError::AccountingOverflow)); };
        self.transfer(data_end, spare_end);
        self.nand.observation.processed_data.0 = processed_data;
        self.nand.observation.processed_spare.0 = processed_spare;
        self.record.progress = OperationProgress { data: WorkBytes(data_end), spare: WorkBytes(spare_end) };
        if end == limit {
            match plan {
                Plan::ProgramPrefix => return self.fail(Error::ProgramFailed),
                Plan::CancelPrefix => { self.terminal(OperationOutcome::Cancelled); return Err(Error::Image(ImageError::Cancelled)); },
                Plan::TaggedPrefix => self.terminal(OperationOutcome::TaggedFault(WorkBytes(512))),
                _ if self.illegal_bits => return self.fail(Error::ProgramFailed),
                _ => self.terminal(OperationOutcome::Complete),
            }
        } else { self.publish(); }
        Ok(self.record)
    }
    pub fn finish(mut self) -> Result<OperationRecord, Error> {
        while self.record.outcome == OperationOutcome::Pending { self.advance(StepBytes::SECTOR)?; }
        Ok(self.record)
    }
    fn finish_with_timeout(mut self, timeout: Timeout) -> Result<OperationRecord, Error> {
        let mut remaining = timeout.0.max(1);
        while self.record.outcome == OperationOutcome::Pending {
            if remaining == 0 { return self.fail(Error::Timeout); }
            remaining -= 1;
            self.advance(StepBytes::SECTOR)?;
        }
        Ok(self.record)
    }
    pub fn cancel(mut self) -> OperationRecord {
        if self.record.outcome == OperationOutcome::Pending { self.terminal(OperationOutcome::Cancelled); }
        self.record
    }
}
impl Drop for MemoryOperation<'_, '_> {
    fn drop(&mut self) { if self.record.outcome == OperationOutcome::Pending { self.terminal(OperationOutcome::Cancelled); } }
}

impl endpoint::Sealed for MemoryNand<'_> {}
impl MediaEndpoint for MemoryNand<'_> {
    fn require_configured(&self) -> Result<(), Error> { Ok(()) }
    unsafe fn spare(&mut self, page: PhysicalPage, timeout: Timeout) -> Result<[u8; 16], Error> {
        let mut output = [0; 16];
        self.begin_read_spare(page, &mut output)?.finish_with_timeout(timeout)?;
        Ok(output)
    }
    unsafe fn erase(&mut self, block: PhysicalBlock, timeout: Timeout) -> Result<(), Error> { self.begin_erase_block(block)?.finish_with_timeout(timeout).map(|_| ()) }
    unsafe fn read_page(&mut self, page: PhysicalPage, output: &mut [u8; PAGE_BYTES], transfer: Transfer, timeout: Timeout) -> Result<ReadStatus, Error> {
        let record = self.start(Request::ReadPage { page, output, transfer })?.finish_with_timeout(timeout)?;
        match record.outcome { OperationOutcome::TaggedFault(bytes) => Ok(ReadStatus::TaggedFault(PageReadBytes(bytes.0 as u16))), _ => Ok(ReadStatus::Clean) }
    }
    unsafe fn set_tag(&mut self, block: Option<LogicalBlock>, bad: bool) {
        let bytes = mapping_tag(block).0.to_be_bytes();
        self.metadata.bytes = [bytes[0], bytes[1], bytes[0], bytes[1], 0xff, if bad { 0xe0 } else { 0xff }, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
        self.metadata.purpose = if bad { ProgramPurpose::BadMarker } else { ProgramPurpose::Data };
    }
    unsafe fn program_page(&mut self, page: PhysicalPage, data: &[u8; PAGE_BYTES], _transfer: Transfer, timeout: Timeout) -> Result<(), Error> {
        self.start(Request::Program { page, data, spare: SpareInput::Staged })?.finish_with_timeout(timeout).map(|_| ())
    }
}

pub struct MemoryProfile<'image> { engine: ApplicationNand<MemoryNand<'image>> }
impl<'image> MemoryProfile<'image> {
    pub fn mount(nand: MemoryNand<'image>, timeout: Timeout) -> Result<Self, Error> {
        nand.block_index(PhysicalBlock(PROFILE_BASE))?;
        nand.block_index(PhysicalBlock(PROFILE_BASE + PROFILE_PHYSICAL_BLOCKS as u16 - 1))?;
        let mut engine = ApplicationNand::from_endpoint(nand);
        unsafe { engine.build_profile_map(timeout)?; }
        Ok(Self { engine })
    }
    pub fn is_mounted(&self) -> bool { self.engine.is_mounted() }
    pub fn media(&self) -> &MemoryNand<'image> { &self.engine.controller }
    pub fn failures_mut(&mut self) -> &mut [BlockFailures] { self.engine.controller.failures_mut() }
    pub fn into_media(self) -> MemoryNand<'image> { self.engine.controller }
    pub fn write_blocks(&mut self, first: LogicalBlock, data: &[u8], verification: Verification, timeout: Timeout) -> Result<(), Error> {
        unsafe { self.engine.write_blocks(first, data, verification, timeout) }
    }
    pub fn read_blocks(&mut self, first: LogicalBlock, output: &mut [u8], timeout: Timeout) -> Result<(), Error> {
        unsafe { self.engine.read_blocks(first, output, timeout) }
    }
    pub fn read_bytes(&mut self, first: LogicalBlock, offset: ByteOffset, output: &mut [u8], timeout: Timeout) -> Result<(), Error> {
        unsafe { self.engine.read_bytes(first, offset, output, timeout) }
    }
    pub fn write_bytes(&mut self, first: LogicalBlock, offset: ByteOffset, data: &[u8], verification: Verification, timeout: Timeout) -> Result<(), Error> {
        unsafe { self.engine.write_bytes(first, offset, data, verification, timeout) }
    }
    pub fn erase_blocks(&mut self, first: LogicalBlock, count: BlockCount, timeout: Timeout) -> Result<(), Error> {
        unsafe { self.engine.erase_blocks(first, count, timeout) }
    }
}

#[cfg(all(test, xm1_application_nand_host_tests))]
#[path = "memory_tests.rs"]
mod tests;
