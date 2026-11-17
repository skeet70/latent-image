use core::{mem::ManuallyDrop, num::NonZeroI32, task::Poll};

use super::{PhysicalBlock, BLOCK_BYTES};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Instant(u64);
impl Instant {
    pub const fn new(count: u64) -> Self { Self(count) }
    pub const fn count(self) -> u64 { self.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deadline(Instant);
impl Deadline {
    pub const fn new(instant: Instant) -> Option<Self> {
        if instant.0 == u64::MAX { None } else { Some(Self(instant)) }
    }
    pub const fn instant(self) -> Instant { self.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteCount(usize);
impl ByteCount {
    pub const fn new(bytes: usize) -> Self { Self(bytes) }
    pub const fn get(self) -> usize { self.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LowerStatus(NonZeroI32);
impl LowerStatus {
    pub const fn new(code: i32) -> Option<Self> {
        match NonZeroI32::new(code) { Some(code) => Some(Self(code)), None => None }
    }
    pub const fn code(self) -> i32 { self.0.get() }
    pub const fn aggregate_code(self) -> i32 {
        match self.code() { 4 => 6, 3 => 7, 7 => 3, 10 => 4, other => other }
    }
    fn continues(self) -> bool { matches!(self.code(), 7 | 10) }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    DriverAcquire,
    ChannelAcquire,
    Classify,
    Start,
    Controller,
    Stop,
    Quiescence,
    FinishTransfer,
    ChannelRelease,
    DriverRelease,
    SkipResource,
    SkipProgress,
    Decoder,
    Publication,
}
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionError {
    Retained,
    BlockRange,
    BufferLength,
    SkipBudget,
    MissingSegment,
    Descriptor,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimaryError { Lower(LowerStatus), Deadline(Phase), Admission(AdmissionError) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupError { Lower { phase: Phase, status: LowerStatus }, Deadline(Phase) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure { Primary { primary: PrimaryError, cleanup: Option<CleanupError> }, Cleanup(CleanupError) }
impl Failure {
    fn combine(primary: Option<PrimaryError>, cleanup: Option<CleanupError>) -> Option<Self> {
        match (primary, cleanup) {
            (Some(primary), cleanup) => Some(Self::Primary { primary, cleanup }),
            (None, Some(cleanup)) => Some(Self::Cleanup(cleanup)),
            (None, None) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockCount(u16);
impl BlockCount {
    pub const fn new(blocks: u32) -> Option<Self> {
        if blocks <= 1024 { Some(Self(blocks as u16)) } else { None }
    }
    pub const fn get(self) -> u32 { self.0 as u32 }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipBudget { Unlimited, Limited(BlockCount) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadPlan { first: PhysicalBlock, blocks: BlockCount, bad_budget: SkipBudget }
impl ReadPlan {
    /// The block count is the required usable output count. Bad table entries advance
    /// the physical cursor without advancing the destination.
    pub fn new(first: PhysicalBlock, blocks: BlockCount, bad_budget: SkipBudget) -> Result<Self, AdmissionError> {
        if first.index() + blocks.get() > 1024 { return Err(AdmissionError::BlockRange); }
        Ok(Self { first, blocks, bad_budget })
    }
    pub const fn capacity_bytes(self) -> usize { self.blocks.get() as usize * BLOCK_BYTES as usize }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockDisposition { Usable, Bad }

/// Quiescence success excludes further destination writes. Failed release retains its lease.
/// All methods are nonblocking; a failed start may have activated DMA.
///
/// # Safety
/// Implementations must not report quiescence while the device can still access the
/// destination. Handles returned by acquisition remain valid until their release succeeds.
pub unsafe trait ReadPorts {
    type Driver;
    type Channel;
    fn now(&mut self) -> Instant;
    fn try_driver(&mut self) -> Poll<Result<Self::Driver, LowerStatus>>;
    fn try_channel(&mut self, driver: &mut Self::Driver) -> Poll<Result<Self::Channel, LowerStatus>>;
    fn classify(&mut self, block: PhysicalBlock) -> Result<BlockDisposition, LowerStatus>;
    fn start(&mut self, channel: &mut Self::Channel, block: PhysicalBlock, destination: &mut [u8]) -> Result<(), LowerStatus>;
    fn poll_controller(&mut self, channel: &mut Self::Channel) -> Poll<Result<(), LowerStatus>>;
    fn request_stop(&mut self, channel: &mut Self::Channel) -> Result<(), LowerStatus>;
    fn poll_quiescent(&mut self, channel: &mut Self::Channel) -> Poll<Result<(), LowerStatus>>;
    fn finish_transfer(&mut self, channel: &mut Self::Channel) -> Result<(), LowerStatus>;
    fn close_channel(&mut self, channel: &mut Self::Channel) -> Result<(), LowerStatus>;
    fn unlock_driver(&mut self, driver: &mut Self::Driver) -> Result<(), LowerStatus>;
    /// Reports a proved completed prefix, not aggregate success or permission to reuse backing.
    fn progress(&mut self, bytes: ByteCount) -> Result<(), LowerStatus>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability { Available, Retained }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadReport { pub bytes: ByteCount, pub bad_blocks: BlockCount }
#[derive(Debug)]
pub enum ReadOutcome<B> {
    Success { destination: B, report: ReadReport },
    ReleasedFailure { destination: B, report: ReadReport, failure: Failure },
    HeldQuiescent { report: ReadReport, failure: Failure },
    UnprovedDma { report: ReadReport, failure: Failure },
}

/// # Safety
/// The backing address and extent remain fixed across moves and mutable reborrows until drop.
/// The destination exclusively owns its backing for the entire operation.
pub unsafe trait DmaDestination: AsMut<[u8]> + 'static {}

struct Operation<D, C, B> {
    destination: ManuallyDrop<B>,
    driver: Option<ManuallyDrop<D>>,
    channel: Option<ManuallyDrop<C>>,
    active: bool,
}

pub struct DmaReadOwner<P: ReadPorts, B: DmaDestination> {
    ports: ManuallyDrop<P>,
    retained: Option<Operation<P::Driver, P::Channel, B>>,
}

fn check<P: ReadPorts>(ports: &mut P, deadline: Deadline, phase: Phase) -> Result<(), PrimaryError> {
    if ports.now() >= deadline.instant() { Err(PrimaryError::Deadline(phase)) } else { Ok(()) }
}
fn wait<P: ReadPorts, T>(ports: &mut P, deadline: Deadline, phase: Phase, mut poll: impl FnMut(&mut P) -> Poll<Result<T, LowerStatus>>) -> Result<T, PrimaryError> {
    loop {
        check(ports, deadline, phase)?;
        match poll(ports) {
            Poll::Pending => {},
            Poll::Ready(result) => return result.map_err(PrimaryError::Lower),
        }
    }
}
fn cleanup_deadline<P: ReadPorts>(ports: &mut P, deadline: Deadline, phase: Phase) -> Result<(), CleanupError> {
    check(ports, deadline, phase).map_err(|_| CleanupError::Deadline(phase))
}
fn stop<P: ReadPorts>(ports: &mut P, channel: &mut P::Channel, deadline: Deadline) -> Result<(), CleanupError> {
    cleanup_deadline(ports, deadline, Phase::Stop)?;
    ports.request_stop(channel).map_err(|status| CleanupError::Lower { phase: Phase::Stop, status })?;
    loop {
        cleanup_deadline(ports, deadline, Phase::Quiescence)?;
        match ports.poll_quiescent(channel) {
            Poll::Pending => {},
            Poll::Ready(result) => return result.map_err(|status| CleanupError::Lower { phase: Phase::Quiescence, status }),
        }
    }
}

impl<P: ReadPorts, B: DmaDestination> DmaReadOwner<P, B> {
    pub fn new(ports: P) -> Self { Self { ports: ManuallyDrop::new(ports), retained: None } }
    pub fn availability(&self) -> Availability {
        if self.retained.is_some() { Availability::Retained } else { Availability::Available }
    }
    pub fn read(&mut self, plan: ReadPlan, mut destination: B, deadline: Deadline) -> ReadOutcome<B> {
        let mut report = ReadReport { bytes: ByteCount::new(0), bad_blocks: BlockCount(0) };
        let admission = if self.retained.is_some() { Some(AdmissionError::Retained) }
            else if destination.as_mut().len() < plan.capacity_bytes() { Some(AdmissionError::BufferLength) }
            else { None };
        if let Some(error) = admission {
            return ReadOutcome::ReleasedFailure { destination, report, failure: Failure::Primary { primary: PrimaryError::Admission(error), cleanup: None } };
        }
        let ports = &mut *self.ports;
        let mut operation = Operation { destination: ManuallyDrop::new(destination), driver: None, channel: None, active: false };
        let mut primary = None;
        let mut cleanup = None;
        let execution = (|| -> Result<(), PrimaryError> {
            operation.driver = Some(ManuallyDrop::new(wait(ports, deadline, Phase::DriverAcquire, |p| p.try_driver())?));
            operation.channel = Some(ManuallyDrop::new(wait(ports, deadline, Phase::ChannelAcquire, |p| p.try_channel(operation.driver.as_mut().unwrap()))?));
            let mut skipped = 0;
            let mut accumulated = None;
            let mut physical = plan.first.index();
            while report.bytes.get() < plan.capacity_bytes() {
                check(ports, deadline, Phase::Classify)?;
                let block = PhysicalBlock::new(physical)
                    .map_err(|_| PrimaryError::Admission(AdmissionError::BlockRange))?;
                physical = physical.checked_add(1)
                    .ok_or(PrimaryError::Admission(AdmissionError::BlockRange))?;
                match ports.classify(block).map_err(PrimaryError::Lower)? {
                    BlockDisposition::Bad => {
                        if let SkipBudget::Limited(limit) = plan.bad_budget {
                            if skipped == limit.get() {
                                return Err(PrimaryError::Admission(AdmissionError::SkipBudget));
                            }
                        }
                        skipped += 1;
                        report.bad_blocks = BlockCount(skipped as u16);
                        continue;
                    }
                    BlockDisposition::Usable => {},
                }
                check(ports, deadline, Phase::Start)?;
                let channel = operation.channel.as_mut().unwrap();
                let begin = report.bytes.get();
                operation.active = true;
                ports.start(channel, block, &mut operation.destination.as_mut()[begin..begin + BLOCK_BYTES as usize])
                    .map_err(PrimaryError::Lower)?;
                let controller = wait(ports, deadline, Phase::Controller, |p| p.poll_controller(channel));
                if let Err(error) = controller {
                    if !matches!(error, PrimaryError::Lower(status) if status.continues()) {
                        return Err(error);
                    }
                    if accumulated.is_none() { accumulated = Some(error); }
                }
                match stop(ports, channel, deadline) {
                    Ok(()) => operation.active = false,
                    Err(error) => { cleanup = Some(error); return controller; },
                }
                check(ports, deadline, Phase::FinishTransfer)?;
                ports.finish_transfer(channel).map_err(PrimaryError::Lower)?;
                report.bytes = ByteCount::new(begin + BLOCK_BYTES as usize);
                ports.progress(report.bytes).map_err(PrimaryError::Lower)?;
            }
            match accumulated { Some(error) => Err(error), None => Ok(()) }
        })();
        if let Err(error) = execution { primary = Some(error); }
        if operation.active && cleanup.is_none() {
            match stop(ports, operation.channel.as_mut().unwrap(), deadline) {
                Ok(()) => operation.active = false,
                Err(error) => cleanup = Some(error),
            }
        }
        if !operation.active {
            if let Some(channel) = operation.channel.as_mut() {
                match cleanup_deadline(ports, deadline, Phase::ChannelRelease) {
                    Err(error) => { cleanup.get_or_insert(error); }
                    Ok(()) => match ports.close_channel(channel) {
                        Ok(()) => { operation.channel.take(); },
                        Err(status) => cleanup = Some(CleanupError::Lower { phase: Phase::ChannelRelease, status }),
                    },
                }
            }
            if operation.channel.is_none() {
                if let Some(driver) = operation.driver.as_mut() {
                    match cleanup_deadline(ports, deadline, Phase::DriverRelease) {
                        Err(error) => { cleanup.get_or_insert(error); }
                        Ok(()) => match ports.unlock_driver(driver) {
                            Ok(()) => { operation.driver.take(); },
                            Err(status) => cleanup = Some(CleanupError::Lower { phase: Phase::DriverRelease, status }),
                        },
                    }
                }
            }
        }
        let failure = Failure::combine(primary, cleanup);
        if operation.active || operation.driver.is_some() || operation.channel.is_some() {
            let active = operation.active;
            self.retained = Some(operation);
            let failure = failure.expect("retention has a failure");
            if active { ReadOutcome::UnprovedDma { report, failure } }
            else { ReadOutcome::HeldQuiescent { report, failure } }
        } else {
            let destination = ManuallyDrop::into_inner(operation.destination);
            match failure {
                Some(failure) => ReadOutcome::ReleasedFailure { destination, report, failure },
                None => ReadOutcome::Success { destination, report },
            }
        }
    }
}

#[cfg(test)]
#[path = "dma_read_tests.rs"]
mod tests;
