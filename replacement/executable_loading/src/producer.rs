use core::{ptr::NonNull, slice};

use crate::{LoadedProof, Producer, ProducerCompletion, Retention};
use crate::protocol::{CommandKind, Request};
use xm1_application_nand::{PhysicalBlock, BLOCK_BYTES};
use xm1_application_nand::dma_native::{
    ClockBinding, DrainContract, KernelLeafContract, NandReadPorts, PrefixContext, ProgressSink,
};
use xm1_application_nand::dma_read::{
    AdmissionError, BlockCount, ByteCount, CleanupError, DmaDestination, DmaReadOwner, Failure,
    LowerStatus, Phase, PrimaryError, ReadOutcome, ReadPlan, ReadReport, SkipBudget,
};

const IMAGE5_STAGING: u32 = 0x43e4_0000;
const IMAGE5_STAGING_BYTES: usize = 0x2e_0000;
const IMAGE5_DESTINATION: u32 = 0x00bf_1a00;
const IMAGE5_BYTES: u32 = 0x4f_1e60;
const IMAGE5_BSS: u32 = 0x0146_0000;
const IMAGE5_BSS_BYTES: u32 = 0x2b_be40;
const IMAGE5_FLASH_OFFSET: u32 = 0x2c_0000;
const IMAGE5_FLASH_BYTES: u32 = 0x2e_0000;
const PLAIN_DESTINATION: u32 = 0x0110_0000;
const PLAIN_BYTES: usize = 0x36_0000;
const PLAIN_FLASH_OFFSET: u32 = 0x5a_0000;
const PLAIN_FLASH_BYTES: u32 = 0x36_0000;

#[repr(C)]
struct Segment {
    kind: i32,
    destination: u32,
    bytes: u32,
    bss: u32,
    bss_bytes: u32,
    flash_offset: u32,
    flash_bytes: u32,
}

#[repr(C)]
struct Extents { entries: u32, destination: u32, bytes: u32 }

extern "C" {
    fn xm1_loading_segment(kind: u32) -> *const Segment;
    fn xm1_loading_extents() -> *const Extents;
    fn xm1_loading_mode() -> u32;
    fn xm1_loading_plain_progress(bytes: u32);
    fn xm1_loading_take(resource: u32, timeout: u32) -> i32;
    fn xm1_loading_give(resource: u32) -> i32;
    fn xm1_loading_delay(ticks: u32) -> i32;
    fn xm1_loading_cache(start: u32, bytes: u32);
    fn xm1_decoder_init(destination: u32, staging: u32);
    fn xm1_decoder_feed(loaded: u32);
    fn xm1_decoder_finish();

    fn xm1_loading_action1() -> i32;
    fn xm1_loading_action2() -> i32;
    fn xm1_loading_section3() -> i32;
    fn xm1_loading_selected(index: u32) -> i32;
    fn xm1_loading_region(index: u32) -> i32;
    fn xm1_loading_initialize_storage() -> i32;
    fn xm1_loading_register_completion() -> i32;
    fn xm1_loading_auxiliary(selection: u32) -> i32;
}

pub struct NativeDestination { address: u32, bytes: usize }

impl NativeDestination {
    unsafe fn new(address: u32, bytes: usize) -> Result<Self, AdmissionError> {
        if address == 0 || address & 31 != 0 || bytes == 0
            || address.checked_add(u32::try_from(bytes).map_err(|_| AdmissionError::BufferLength)?) .is_none()
        {
            return Err(AdmissionError::BufferLength);
        }
        Ok(Self { address, bytes })
    }
}

impl AsMut<[u8]> for NativeDestination {
    fn as_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.address as *mut u8, self.bytes) }
    }
}

unsafe impl DmaDestination for NativeDestination {}

unsafe fn plain_prefix(_: PrefixContext, completed: ByteCount) -> Result<(), LowerStatus> {
    unsafe { xm1_loading_plain_progress(completed.get() as u32); }
    Ok(())
}

unsafe fn image5_prefix(_: PrefixContext, completed: ByteCount) -> Result<(), LowerStatus> {
    unsafe { xm1_decoder_feed(completed.get() as u32); }
    Ok(())
}

type Owner = DmaReadOwner<NandReadPorts, NativeDestination>;

pub struct NativeProducer {
    clock: &'static ClockBinding,
    plain: Owner,
    image5_feed: Owner,
    image5_skip: Owner,
}

impl NativeProducer {
    /// # Safety
    /// The selected drain contract is valid for this device. The canonical X-M1 1.50 segment
    /// table and fixed staging/backing ranges remain mapped for the lifetime of this producer.
    pub unsafe fn new(clock: &'static ClockBinding, drain: DrainContract) -> Self {
        let context = PrefixContext::new(NonNull::dangling());
        let plain_ports = unsafe { NandReadPorts::new(
            clock, drain, KernelLeafContract::UncontendedRunnableTask,
            ProgressSink::Consumer { context, consume: plain_prefix },
        ) };
        let image5_feed_ports = unsafe { NandReadPorts::new(
            clock, drain, KernelLeafContract::UncontendedRunnableTask,
            ProgressSink::Consumer { context, consume: image5_prefix },
        ) };
        let image5_skip_ports = unsafe { NandReadPorts::new(
            clock, drain, KernelLeafContract::UncontendedRunnableTask, ProgressSink::None,
        ) };
        Self {
            clock,
            plain: DmaReadOwner::new(plain_ports),
            image5_feed: DmaReadOwner::new(image5_feed_ports),
            image5_skip: DmaReadOwner::new(image5_skip_ports),
        }
    }

    fn failed(primary: PrimaryError, report: ReadReport, retention: Retention) -> ProducerCompletion {
        ProducerCompletion::Failed { failure: Failure::Primary { primary, cleanup: None }, report, retention }
    }

    fn report_zero() -> ReadReport {
        ReadReport { bytes: ByteCount::new(0), bad_blocks: BlockCount::new(0).unwrap() }
    }

    fn plan(first: u32, bytes: u32) -> Result<ReadPlan, AdmissionError> {
        if bytes == 0 || bytes & (BLOCK_BYTES - 1) != 0 { return Err(AdmissionError::Descriptor); }
        let first = PhysicalBlock::new(first).map_err(|_| AdmissionError::BlockRange)?;
        let blocks = BlockCount::new(bytes / BLOCK_BYTES).ok_or(AdmissionError::BlockRange)?;
        ReadPlan::new(first, blocks, SkipBudget::Unlimited)
    }

    fn outcome(outcome: ReadOutcome<NativeDestination>, skipped: bool) -> ProducerCompletion {
        match outcome {
            ReadOutcome::Success { report, .. } if skipped => ProducerCompletion::Skipped(report),
            ReadOutcome::Success { report, .. } => ProducerCompletion::Loaded(unsafe { LoadedProof::from_validated_read(report) }),
            ReadOutcome::ReleasedFailure { report, failure, .. } => ProducerCompletion::Failed { failure, report, retention: Retention::Released },
            ReadOutcome::HeldQuiescent { report, failure } => ProducerCompletion::Failed { failure, report, retention: Retention::HeldQuiescent },
            ReadOutcome::UnprovedDma { report, failure } => ProducerCompletion::Failed { failure, report, retention: Retention::Unproved },
        }
    }

    fn plain_read(&mut self, request: Request, segment: &Segment, extents: &Extents) -> ProducerCompletion {
        if segment.flash_offset != PLAIN_FLASH_OFFSET || segment.flash_bytes != PLAIN_FLASH_BYTES
            || extents.destination != PLAIN_DESTINATION || extents.bytes as usize != PLAIN_BYTES
        {
            return Self::failed(PrimaryError::Admission(AdmissionError::Descriptor), Self::report_zero(), Retention::Released);
        }
        let plan = match Self::plan(segment.flash_offset / BLOCK_BYTES, segment.flash_bytes) {
            Ok(plan) => plan,
            Err(error) => return Self::failed(PrimaryError::Admission(error), Self::report_zero(), Retention::Released),
        };
        let destination = match unsafe { NativeDestination::new(extents.destination, PLAIN_BYTES) } {
            Ok(destination) => destination,
            Err(error) => return Self::failed(PrimaryError::Admission(error), Self::report_zero(), Retention::Released),
        };
        let outcome = self.plain.read(plan, destination, request.deadline);
        match outcome {
            ReadOutcome::Success { destination, report } => {
                unsafe { xm1_loading_plain_progress(segment.flash_bytes); }
                Self::outcome(ReadOutcome::Success { destination, report }, false)
            }
            other => Self::outcome(other, false),
        }
    }


    fn plain_skip(&mut self, request: Request, segment: &Segment) -> ProducerCompletion {
        if segment.flash_offset != PLAIN_FLASH_OFFSET || segment.flash_bytes != PLAIN_FLASH_BYTES {
            return Self::failed(PrimaryError::Admission(AdmissionError::Descriptor), Self::report_zero(), Retention::Released);
        }
        let blocks = segment.flash_bytes / BLOCK_BYTES;
        let mut first = false;
        let mut second = false;
        for resource in [0x24, 0x34] {
            loop {
                if self.clock.now() >= request.deadline.instant() {
                    return ProducerCompletion::Failed {
                        failure: Failure::Primary { primary: PrimaryError::Deadline(Phase::SkipResource), cleanup: None },
                        report: Self::report_zero(),
                        retention: if first || second { Retention::HeldQuiescent } else { Retention::Released },
                    };
                }
                match unsafe { xm1_loading_take(resource, 0) } {
                    0 => { if resource == 0x24 { first = true } else { second = true }; break; }
                    -50 => continue,
                    code => return Self::failed(PrimaryError::Lower(LowerStatus::new(code).unwrap()), Self::report_zero(), if first { Retention::HeldQuiescent } else { Retention::Released }),
                }
            }
        }
        let mut report = Self::report_zero();
        for completed in 1..=blocks {
            if self.clock.now() >= request.deadline.instant() {
                return ProducerCompletion::Failed {
                    failure: Failure::Primary { primary: PrimaryError::Deadline(Phase::SkipProgress), cleanup: None },
                    report, retention: Retention::HeldQuiescent,
                };
            }
            let delay = unsafe { xm1_loading_delay(14) };
            if delay != 0 {
                return ProducerCompletion::Failed {
                    failure: Failure::Primary { primary: PrimaryError::Lower(LowerStatus::new(delay).unwrap()), cleanup: None },
                    report, retention: Retention::HeldQuiescent,
                };
            }
            let bytes = completed as usize * BLOCK_BYTES as usize;
            unsafe { xm1_loading_plain_progress(bytes as u32); }
            report.bytes = ByteCount::new(bytes);
        }
        if self.clock.now() >= request.deadline.instant() {
            return ProducerCompletion::Failed {
                failure: Failure::Primary { primary: PrimaryError::Deadline(Phase::SkipResource), cleanup: None },
                report, retention: Retention::HeldQuiescent,
            };
        }
        let release_second = unsafe { xm1_loading_give(0x34) };
        if release_second != 0 {
            return ProducerCompletion::Failed {
                failure: Failure::Cleanup(CleanupError::Lower { phase: Phase::SkipResource, status: LowerStatus::new(release_second).unwrap() }),
                report, retention: Retention::HeldQuiescent,
            };
        }
        second = false;
        if self.clock.now() >= request.deadline.instant() {
            return ProducerCompletion::Failed {
                failure: Failure::Primary { primary: PrimaryError::Deadline(Phase::SkipResource), cleanup: None },
                report, retention: Retention::HeldQuiescent,
            };
        }
        let release_first = unsafe { xm1_loading_give(0x24) };
        if release_first != 0 {
            return ProducerCompletion::Failed {
                failure: Failure::Cleanup(CleanupError::Lower { phase: Phase::SkipResource, status: LowerStatus::new(release_first).unwrap() }),
                report, retention: Retention::HeldQuiescent,
            };
        }
        first = false;
        unsafe { xm1_loading_plain_progress(segment.flash_bytes); }
        let _ = (first, second);
        ProducerCompletion::Skipped(report)
    }

    fn image5(&mut self, request: Request, segment: &Segment) -> ProducerCompletion {
        if segment.destination != IMAGE5_DESTINATION || segment.bytes != IMAGE5_BYTES
            || segment.bss != IMAGE5_BSS || segment.bss_bytes != IMAGE5_BSS_BYTES
            || segment.flash_offset != IMAGE5_FLASH_OFFSET || segment.flash_bytes != IMAGE5_FLASH_BYTES
        {
            return Self::failed(PrimaryError::Admission(AdmissionError::Descriptor), Self::report_zero(), Retention::Released);
        }
        if self.clock.now() >= request.deadline.instant() {
            return Self::failed(PrimaryError::Deadline(Phase::Publication), Self::report_zero(), Retention::Released);
        }
        unsafe {
            core::ptr::write_bytes(segment.bss as *mut u8, 0, segment.bss_bytes as usize);
            xm1_loading_cache(0, 0);
        }
        let skipped = unsafe { xm1_loading_mode() } != 0;
        if !skipped {
            if self.clock.now() >= request.deadline.instant() {
                return Self::failed(PrimaryError::Deadline(Phase::Decoder), Self::report_zero(), Retention::Released);
            }
            unsafe { xm1_decoder_init(segment.destination, IMAGE5_STAGING); }
        }
        let plan = match Self::plan(segment.flash_offset / BLOCK_BYTES, segment.flash_bytes) {
            Ok(plan) => plan,
            Err(error) => return Self::failed(PrimaryError::Admission(error), Self::report_zero(), Retention::Released),
        };
        let destination = match unsafe { NativeDestination::new(IMAGE5_STAGING, IMAGE5_STAGING_BYTES) } {
            Ok(destination) => destination,
            Err(error) => return Self::failed(PrimaryError::Admission(error), Self::report_zero(), Retention::Released),
        };
        let outcome = if skipped {
            self.image5_skip.read(plan, destination, request.deadline)
        } else {
            self.image5_feed.read(plan, destination, request.deadline)
        };
        let (destination, report) = match outcome {
            ReadOutcome::Success { destination, report } => (destination, report),
            other => return Self::outcome(other, skipped),
        };
        if !skipped {
            if self.clock.now() >= request.deadline.instant() {
                return Self::outcome(ReadOutcome::ReleasedFailure {
                    destination, report,
                    failure: Failure::Primary { primary: PrimaryError::Deadline(Phase::Decoder), cleanup: None },
                }, false);
            }
            unsafe { xm1_decoder_finish(); }
        }
        Self::outcome(ReadOutcome::Success { destination, report }, skipped)
    }

    fn delegated(&mut self, request: Request) -> ProducerCompletion {
        if self.clock.now() >= request.deadline.instant() {
            return Self::failed(PrimaryError::Deadline(Phase::Publication), Self::report_zero(), Retention::Released);
        }
        let status = unsafe {
            match request.kind {
                CommandKind::FixedBlock => xm1_loading_action1(),
                CommandKind::Selected => {
                    let first = xm1_loading_action2();
                    if first == 0 { xm1_loading_section3() } else { first }
                }
                CommandKind::Region => xm1_loading_selected(request.argument.0),
                CommandKind::Section3 => xm1_loading_region(request.argument.0),
                CommandKind::Initialize => xm1_loading_initialize_storage(),
                CommandKind::RegisterCompletion => xm1_loading_register_completion(),
                CommandKind::Auxiliary => xm1_loading_auxiliary(0),
                _ => -30,
            }
        };
        if status == 0 { ProducerCompletion::Delegated }
        else { Self::failed(PrimaryError::Lower(LowerStatus::new(status).unwrap()), Self::report_zero(), Retention::Released) }
    }
}

impl Producer for NativeProducer {
    fn execute(&mut self, request: Request) -> ProducerCompletion {
        match request.kind {
            CommandKind::Plain => {
                let segment = unsafe { xm1_loading_segment(CommandKind::Plain as u32).as_ref() };
                let extents = unsafe { xm1_loading_extents().as_ref() };
                match (segment, extents) {
                    (Some(segment), Some(extents)) if unsafe { xm1_loading_mode() } == 0 => self.plain_read(request, segment, extents),
                    (Some(segment), Some(_)) => self.plain_skip(request, segment),
                    _ => Self::failed(PrimaryError::Admission(AdmissionError::MissingSegment), Self::report_zero(), Retention::Released),
                }
            }
            CommandKind::Image5 => match unsafe { xm1_loading_segment(CommandKind::Image5 as u32).as_ref() } {
                Some(segment) => self.image5(request, segment),
                None => Self::failed(PrimaryError::Admission(AdmissionError::MissingSegment), Self::report_zero(), Retention::Released),
            },
            _ => self.delegated(request),
        }
    }
}

