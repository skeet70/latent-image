use core::num::NonZeroI32;
use crate::{Completion, CompletionOutcome, Retention};
use crate::protocol::{CommandKind, ProtocolError, RequestId};
use xm1_application_nand::dma_read::{AdmissionError, BlockCount, ByteCount, CleanupError, Deadline, Failure, Instant, LowerStatus, Phase, PrimaryError, ReadReport};

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletionWords {
    pub tag: u32,
    pub request_id: u32,
    pub command_kind: u32,
    pub primary_code: u32,
    pub cleanup_code: u32,
    pub completed_bytes: u32,
    pub bad_blocks: u32,
    pub protocol_code: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandCompletion {
    Loaded { request_id: RequestId, kind: CommandKind, completed_bytes: ByteCount, bad_blocks: BlockCount },
    Delegated { request_id: RequestId, kind: CommandKind },
    Skipped { request_id: RequestId, kind: CommandKind, completed_bytes: ByteCount, bad_blocks: BlockCount },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnerStatus(NonZeroI32);
impl OwnerStatus {
    pub const fn code(self) -> i32 { self.0.get() }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientError {
    NotConfigured,
    Protocol(ProtocolError),
    OwnerStatus(OwnerStatus),
    Failure { request_id: RequestId, kind: CommandKind, failure: Failure, report: ReadReport, retention: Retention },
}

unsafe extern "C" {
    fn xm1_loading_absolute_deadline(kind: u32, result: *mut u64) -> i32;
    fn xm1_loading_request_sync(kind: u32, deadline: u64, result: *mut CompletionWords) -> i32;
}

fn owner_status(code: i32) -> ClientError {
    match code {
        -1 => ClientError::NotConfigured,
        -2 => ClientError::Protocol(ProtocolError::Outstanding),
        -3 => ClientError::Protocol(ProtocolError::Expired),
        code => ClientError::OwnerStatus(OwnerStatus(NonZeroI32::new(code).unwrap())),
    }
}

pub fn configured_deadline(kind: CommandKind) -> Result<Deadline, ClientError> {
    let mut count = 0u64;
    let status = unsafe { xm1_loading_absolute_deadline(kind as u32, &mut count) };
    if status != 0 { return Err(owner_status(status)); }
    Deadline::new(Instant::new(count)).ok_or(ClientError::Protocol(ProtocolError::NonFiniteDeadline))
}

pub fn request_sync(kind: CommandKind, deadline: Deadline) -> Result<CommandCompletion, ClientError> {
    let mut result = core::mem::MaybeUninit::<CompletionWords>::uninit();
    let status = unsafe { xm1_loading_request_sync(kind as u32, deadline.instant().count(), result.as_mut_ptr()) };
    if status == 0 { unsafe { result.assume_init() }.parse(kind) } else { Err(owner_status(status)) }
}

impl CompletionWords {
    pub fn from_completion(completion: Completion) -> Result<Self, ProtocolError> {
        let mut words = Self { tag: 0, request_id: completion.id.word(), command_kind: completion.kind as u32,
            primary_code: 0, cleanup_code: 0, completed_bytes: 0, bad_blocks: 0, protocol_code: 0 };
        let report = match completion.outcome {
            CompletionOutcome::Loaded(report) => Some(report),
            CompletionOutcome::Skipped(report) => { words.protocol_code = 2; Some(report) },
            CompletionOutcome::Delegated => { words.protocol_code = 1; None },
            CompletionOutcome::Rejected(error) => {
                words.tag = 4;
                let (code, detail) = encode_protocol(error);
                words.protocol_code = code;
                words.primary_code = detail;
                None
            }
            CompletionOutcome::Failed { failure, report, retention } => {
                words.tag = match retention { Retention::Released => 1, Retention::HeldQuiescent => 2, Retention::Unproved => 3 };
                let (primary, cleanup) = match failure {
                    Failure::Primary { primary, cleanup } => (Some(primary), cleanup),
                    Failure::Cleanup(cleanup) => (None, Some(cleanup)),
                };
                if let Some(primary) = primary {
                    match primary {
                        PrimaryError::Lower(status) => { words.protocol_code = 1; words.primary_code = status.code() as u32; }
                        PrimaryError::Deadline(phase) => { words.protocol_code = 2 | ((phase as u32) << 4); }
                        PrimaryError::Admission(error) => { words.protocol_code = 3 | ((error as u32) << 4); }
                    }
                }
                if let Some(cleanup) = cleanup {
                    match cleanup {
                        CleanupError::Lower { phase, status } => {
                            words.protocol_code |= 1 << 8 | ((phase as u32) << 12);
                            words.cleanup_code = status.code() as u32;
                        }
                        CleanupError::Deadline(phase) => { words.protocol_code |= 2 << 8 | ((phase as u32) << 12); }
                    }
                }
                Some(report)
            }
        };
        if let Some(report) = report {
            words.completed_bytes = u32::try_from(report.bytes.get()).map_err(|_| ProtocolError::InvalidRequest)?;
            words.bad_blocks = report.bad_blocks.get();
        }
        Ok(words)
    }

    pub fn parse(self, expected: CommandKind) -> Result<CommandCompletion, ClientError> {
        let invalid = ClientError::Protocol(ProtocolError::InvalidRequest);
        let kind = CommandKind::parse(self.command_kind).ok_or(invalid)?;
        if kind != expected { return Err(invalid); }
        let request_id = RequestId::new(self.request_id).ok_or(invalid)?;
        let report = ReadReport { bytes: ByteCount::new(self.completed_bytes as usize), bad_blocks: BlockCount::new(self.bad_blocks).ok_or(invalid)? };
        match self.tag {
            0 if self.primary_code == 0 && self.cleanup_code == 0 => match self.protocol_code {
                0 if kind.owns_executable() => Ok(CommandCompletion::Loaded { request_id, kind, completed_bytes: report.bytes, bad_blocks: report.bad_blocks }),
                1 if !kind.owns_executable() && self.completed_bytes == 0 && self.bad_blocks == 0 => Ok(CommandCompletion::Delegated { request_id, kind }),
                2 if kind.owns_executable() => Ok(CommandCompletion::Skipped { request_id, kind, completed_bytes: report.bytes, bad_blocks: report.bad_blocks }),
                _ => Err(invalid),
            },
            1..=3 => {
                if self.protocol_code & !0xf3f3 != 0 { return Err(invalid); }
                let primary_detail = (self.protocol_code >> 4) & 15;
                let primary = match self.protocol_code & 3 {
                    0 if self.primary_code == 0 && primary_detail == 0 => None,
                    1 if primary_detail == 0 => Some(PrimaryError::Lower(LowerStatus::new(self.primary_code as i32).ok_or(invalid)?)),
                    2 if self.primary_code == 0 => Some(PrimaryError::Deadline(phase(primary_detail).ok_or(invalid)?)),
                    3 if self.primary_code == 0 => Some(PrimaryError::Admission(admission(primary_detail).ok_or(invalid)?)),
                    _ => return Err(invalid),
                };
                let cleanup_detail = (self.protocol_code >> 12) & 15;
                let cleanup = match (self.protocol_code >> 8) & 3 {
                    0 if self.cleanup_code == 0 && cleanup_detail == 0 => None,
                    1 => Some(CleanupError::Lower { phase: phase(cleanup_detail).ok_or(invalid)?, status: LowerStatus::new(self.cleanup_code as i32).ok_or(invalid)? }),
                    2 if self.cleanup_code == 0 => Some(CleanupError::Deadline(phase(cleanup_detail).ok_or(invalid)?)),
                    _ => return Err(invalid),
                };
                let failure = match (primary, cleanup) {
                    (Some(primary), cleanup) => Failure::Primary { primary, cleanup },
                    (None, Some(cleanup)) => Failure::Cleanup(cleanup),
                    _ => return Err(invalid),
                };
                let retention = match self.tag { 1 => Retention::Released, 2 => Retention::HeldQuiescent, _ => Retention::Unproved };
                Err(ClientError::Failure { request_id, kind, failure, report, retention })
            }
            4 if self.cleanup_code == 0 && self.completed_bytes == 0 && self.bad_blocks == 0 => {
                Err(ClientError::Protocol(decode_protocol(self.protocol_code, self.primary_code).ok_or(invalid)?))
            }
            _ => Err(invalid),
        }
    }
}

fn phase(code: u32) -> Option<Phase> {
    [
        Phase::DriverAcquire, Phase::ChannelAcquire, Phase::Classify, Phase::Start,
        Phase::Controller, Phase::Stop, Phase::Quiescence, Phase::FinishTransfer,
        Phase::ChannelRelease, Phase::DriverRelease, Phase::SkipResource,
        Phase::SkipProgress, Phase::Decoder, Phase::Publication,
    ].get(code as usize).copied()
}
fn admission(code: u32) -> Option<AdmissionError> {
    [
        AdmissionError::Retained, AdmissionError::BlockRange, AdmissionError::BufferLength,
        AdmissionError::SkipBudget, AdmissionError::MissingSegment, AdmissionError::Descriptor,
    ].get(code as usize).copied()
}
fn encode_protocol(error: ProtocolError) -> (u32, u32) {
    match error {
        ProtocolError::InvalidCommand => (1, 0), ProtocolError::InvalidRequest => (2, 0),
        ProtocolError::InvalidCallback => (3, 0), ProtocolError::EnvelopeSize => (4, 0),
        ProtocolError::EnvelopeVersion => (5, 0), ProtocolError::NonFiniteDeadline => (7, 0),
        ProtocolError::Expired => (9, 0), ProtocolError::NotConfigured => (10, 0),
        ProtocolError::Outstanding => (11, 0), ProtocolError::RetainedOperation => (12, 0),
        ProtocolError::Kernel(code) => (13, code as u32), ProtocolError::Producer(code) => (14, code as u32),
        ProtocolError::MissingCompletion => (15, 0),
    }
}
fn decode_protocol(code: u32, detail: u32) -> Option<ProtocolError> {
    if code != 13 && code != 14 && detail != 0 { return None; }
    Some(match code {
        1 => ProtocolError::InvalidCommand, 2 => ProtocolError::InvalidRequest,
        3 => ProtocolError::InvalidCallback, 4 => ProtocolError::EnvelopeSize,
        5 => ProtocolError::EnvelopeVersion,
        7 => ProtocolError::NonFiniteDeadline,
        9 => ProtocolError::Expired, 10 => ProtocolError::NotConfigured,
        11 => ProtocolError::Outstanding, 12 => ProtocolError::RetainedOperation,
        13 => ProtocolError::Kernel(detail as i32), 14 => ProtocolError::Producer(detail as i32),
        15 => ProtocolError::MissingCompletion, _ => return None,
    })
}
