//! X-M1 1.50 framing derived from image 4 at 0x0061c400.
//! Command checksum and byte order: 0x006bf724, 0x006bec08, 0x006beca0.
//! Response decisions: 0x006bdf44 and 0x006be368. Page transfers: 0x006bf50c,
//! 0x006bf5fc, 0x006bdacc and 0x006bdb64. These are static byte-derived semantics.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandWord(u32);

impl CommandWord {
    pub const fn new(raw: u32) -> Self { Self(raw) }
    pub const fn raw(self) -> u32 { self.0 }
    pub const fn command_id(self) -> u16 { ((self.0 >> 6) & 0x1ff) as u16 }
    pub const fn wire_bytes(self) -> [u8; 4] { self.0.to_be_bytes() }
    pub const fn checksum(self) -> u8 {
        (((self.0 >> 27) + ((self.0 >> 22) & 31) + ((self.0 >> 17) & 31)
            + ((self.0 >> 12) & 31) + ((self.0 >> 7) & 31)
            + ((self.0 >> 6) & 1)) & 31) as u8
    }
    pub const fn with_checksum(self) -> Self {
        Self((self.0 & !0x3f) | ((self.checksum() as u32) << 1))
    }
    pub const fn checksum_valid(self) -> bool { ((self.0 >> 1) & 31) == self.checksum() as u32 }
    fn from_bytes(bytes: &[u8]) -> Result<Self, FrameError> {
        let word = bytes.get(..4).ok_or(FrameError::ShortFrame)?;
        Ok(Self(u32::from_be_bytes([word[0], word[1], word[2], word[3]])))
    }
}

pub const MAX_FRAME_LENGTH: usize = 0x820;
pub const MAX_PAYLOAD_LENGTH: usize = MAX_FRAME_LENGTH - 4;

pub const fn fallback_command(previous: CommandWord) -> CommandWord {
    CommandWord::new((15 << 6) | ((previous.command_id() as u32) << 16)).with_checksum()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResetResponse { Accepted, Recover }
pub fn classify_reset(bytes: &[u8]) -> Result<ResetResponse, FrameError> {
    Ok(if CommandWord::from_bytes(bytes)?.raw() == 0xc33c_a55a {
        ResetResponse::Accepted
    } else {
        ResetResponse::Recover
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Sequence(u8);
impl Sequence {
    pub const fn new(value: u8) -> Result<Self, FrameError> {
        if value < 8 { Ok(Self(value)) } else { Err(FrameError::InvalidSequence) }
    }
    pub const fn get(self) -> u8 { self.0 }
    pub const fn next(self) -> Self { Self((self.0 + 1) & 7) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueuedCommand {
    pub command: CommandWord,
    pub acknowledgement: CommandWord,
}
impl QueuedCommand {
    pub const fn new(command: CommandWord, sequence: Sequence) -> Self {
        Self { command: command.with_checksum(), acknowledgement: CommandWord(0x0800_8000 | ((sequence.0 as u32) << 24)) }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageId(u16);
impl PageId {
    pub const fn new(value: u16) -> Self { Self(value) }
    pub const fn get(self) -> u16 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PayloadLength(u16);
impl PayloadLength {
    pub const fn new(value: usize) -> Result<Self, FrameError> {
        if value <= MAX_PAYLOAD_LENGTH { Ok(Self(value as u16)) } else { Err(FrameError::LengthOverflow) }
    }
    pub const fn get(self) -> usize { self.0 as usize }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameSpan { payload: PayloadLength }
impl FrameSpan {
    pub const fn new(payload: PayloadLength, capacity: usize) -> Result<Self, FrameError> {
        match payload.get().checked_add(4) {
            Some(total) if total <= capacity => Ok(Self { payload }),
            _ => Err(FrameError::LengthOverflow),
        }
    }
    pub const fn wire_len(self) -> usize { self.payload.get() + 4 }
    pub const fn payload_length(self) -> PayloadLength { self.payload }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChecksumMode { Disabled, Enabled }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadMarker { Disabled, Enabled }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameError { ShortFrame, LengthOverflow, InvalidSequence, PageMismatch, ChecksumMismatch }

pub struct OwnedFrame<const N: usize> { bytes: [u8; N], length: usize }
impl<const N: usize> Default for OwnedFrame<N> { fn default() -> Self { Self::new() } }
impl<const N: usize> OwnedFrame<N> {
    pub const fn new() -> Self { Self { bytes: [0; N], length: 0 } }
    pub fn build_write(&mut self, page: PageId, payload: &[u8], checksum: ChecksumMode) -> Result<FrameSpan, FrameError> {
        let span = FrameSpan::new(PayloadLength::new(payload.len())?, N)?;
        self.bytes[..2].copy_from_slice(&page.0.to_be_bytes());
        self.bytes[2..2 + payload.len()].copy_from_slice(payload);
        if checksum == ChecksumMode::Enabled {
            self.bytes[2 + payload.len()] = byte_checksum(&self.bytes[..2 + payload.len()]);
        }
        self.length = span.wire_len();
        Ok(span)
    }
    pub fn build_read(&mut self, page: PageId, length: PayloadLength, marker: ReadMarker) -> Result<FrameSpan, FrameError> {
        let span = FrameSpan::new(length, N)?;
        self.bytes[..span.wire_len()].fill(0);
        self.bytes[..2].copy_from_slice(&page.0.to_be_bytes());
        if marker == ReadMarker::Enabled { self.bytes[length.get() + 2] = 0x5a; }
        self.length = span.wire_len();
        Ok(span)
    }
    pub fn as_bytes(&self) -> &[u8] { &self.bytes[..self.length] }
}

pub fn byte_checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageResponse<'a> { pub page: PageId, pub payload: &'a [u8] }
pub fn parse_page_response(bytes: &[u8], expected: PageId, length: PayloadLength, checksum: ChecksumMode) -> Result<PageResponse<'_>, FrameError> {
    let span = FrameSpan::new(length, bytes.len()).map_err(|_| FrameError::ShortFrame)?;
    let bytes = &bytes[..span.wire_len()];
    let page = PageId(u16::from_be_bytes([bytes[0], bytes[1]]));
    if page != expected { return Err(FrameError::PageMismatch); }
    if checksum == ChecksumMode::Enabled && byte_checksum(&bytes[..length.get() + 2]) != bytes[length.get() + 2] {
        return Err(FrameError::ChecksumMismatch);
    }
    Ok(PageResponse { page, payload: &bytes[2..length.get() + 2] })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageWriteCompletion { pub page: PageId, pub marker_valid: bool }
pub fn classify_page_write(bytes: &[u8], length: PayloadLength, marker: ReadMarker) -> Result<PageWriteCompletion, FrameError> {
    if bytes.len() < 2 { return Err(FrameError::ShortFrame); }
    let marker_valid = match marker {
        ReadMarker::Disabled => true,
        ReadMarker::Enabled => *bytes.get(length.get() + 2).ok_or(FrameError::ShortFrame)? == 0x5a,
    };
    Ok(PageWriteCompletion { page: PageId(u16::from_be_bytes([bytes[0], bytes[1]])), marker_valid })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryMode { Normal, Awaiting, Restart }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryCount(u32);
impl RetryCount {
    pub const fn new(value: u32) -> Self { Self(value) }
    pub const fn get(self) -> u32 { self.0 }
    pub const fn increment(self) -> Self { Self(self.0.wrapping_add(1)) }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryLimit(u32);
impl RetryLimit {
    pub const fn new(value: u32) -> Self { Self(value) }
    pub const fn get(self) -> u32 { self.0 }
    pub const fn exceeded(self, count: RetryCount) -> bool { count.0 > self.0 }
    pub const fn reached(self, count: RetryCount) -> bool { count.0 >= self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidResponse { InvalidMagic, ReservedBit, Checksum, MissingAcknowledgement, InvalidHeader, CommandMismatch }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResponseOutcome { Accepted, Busy, RetryExhausted, Recover, Invalid(InvalidResponse) }
impl ResponseOutcome {
    pub const fn stock_code(self, recovery: RecoveryMode) -> u32 {
        match self {
            Self::Accepted => 0, Self::Busy => 6, Self::Recover => 9,
            Self::RetryExhausted => 10,
            Self::Invalid(_) => if matches!(recovery, RecoveryMode::Awaiting) { 10 } else { 8 },
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResponseContext {
    pub current_command: CommandWord,
    pub previous_command: CommandWord,
    pub last_response: Option<CommandWord>,
    pub expected_sequence: Sequence,
    pub recovery: RecoveryMode,
    pub busy_count: RetryCount,
    pub busy_limit: RetryLimit,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AckClassification {
    pub response: CommandWord,
    pub outcome: ResponseOutcome,
    pub recovery: RecoveryMode,
    pub expected_sequence: Sequence,
    pub busy_count: RetryCount,
    pub acknowledges_previous: bool,
    pub reset_retry_count: bool,
}

pub fn classify_ack(bytes: &[u8], context: ResponseContext) -> Result<AckClassification, FrameError> {
    let response = CommandWord::from_bytes(bytes)?;
    let word = response.0;
    let mut result = AckClassification {
        response, outcome: ResponseOutcome::Accepted, recovery: context.recovery,
        expected_sequence: context.expected_sequence, busy_count: context.busy_count,
        acknowledges_previous: false, reset_retry_count: false,
    };
    let invalid = if word == 0xc33c_a55a {
        None
    } else if word == 0xa55a_3cc3 { Some(InvalidResponse::InvalidMagic)
    } else if word & 1 != 0 { Some(InvalidResponse::ReservedBit)
    } else if !response.checksum_valid() { Some(InvalidResponse::Checksum)
    } else if word & 0x8000 == 0 { Some(InvalidResponse::MissingAcknowledgement)
    } else if word >> 24 & 0xf8 != 8 { Some(InvalidResponse::InvalidHeader)
    } else if word & 0x000f_0000 != 0 { None
    } else if response.command_id() != context.current_command.command_id() { Some(InvalidResponse::CommandMismatch)
    } else { None };
    let recover = word == 0xc33c_a55a || (invalid == Some(InvalidResponse::MissingAcknowledgement) && word & 0x7fc0 == 0x3c0)
        || (invalid.is_none() && word & 0x000f_0000 != 0);
    if recover {
        result.outcome = ResponseOutcome::Recover;
        if result.recovery == RecoveryMode::Awaiting { result.recovery = RecoveryMode::Restart; }
        return Ok(result);
    }
    if let Some(reason) = invalid { result.outcome = ResponseOutcome::Invalid(reason); return Ok(result); }
    if result.recovery == RecoveryMode::Awaiting { result.recovery = RecoveryMode::Normal; }
    if context.last_response.is_some_and(|last| last.0 != 0 && last.0 != 0xa55a_3cc3)
        && ((word >> 24) & 7) == context.expected_sequence.0 as u32 {
        result.acknowledges_previous = true;
        result.reset_retry_count = true;
        result.expected_sequence = context.expected_sequence.next();
    }
    if word & 0x0010_0000 != 0 {
        result.busy_count = context.busy_count.increment();
        result.outcome = if context.busy_limit.reached(result.busy_count) { ResponseOutcome::RetryExhausted } else { ResponseOutcome::Busy };
    } else { result.busy_count = RetryCount(0); }
    Ok(result)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandClassification {
    pub response: CommandWord,
    pub acknowledgement: CommandWord,
    pub outcome: ResponseOutcome,
    pub recovery: RecoveryMode,
}
pub fn classify_command(bytes: &[u8], previous_command: CommandWord, ack_template: CommandWord, recovery: RecoveryMode, allow_invalid_magic: bool) -> Result<CommandClassification, FrameError> {
    let response = CommandWord::from_bytes(bytes)?;
    let word = response.0;
    let mut result = CommandClassification {
        response, acknowledgement: CommandWord(0x0800_8000 | (ack_template.0 & 0x0700_0000)),
        outcome: ResponseOutcome::Accepted, recovery,
    };
    let failure = if word == 0xc33c_a55a { Some((ResponseOutcome::Recover, 0))
    } else if word == 0xa55a_3cc3 {
        if allow_invalid_magic { result.acknowledgement.0 |= 0x0010_0000; return Ok(result); }
        Some((ResponseOutcome::Invalid(InvalidResponse::InvalidMagic), 0x0001_0000))
    } else if word & 1 != 0 { Some((ResponseOutcome::Invalid(InvalidResponse::ReservedBit), 0x0001_0000))
    } else if !response.checksum_valid() { Some((ResponseOutcome::Invalid(InvalidResponse::Checksum), 0x0002_0000))
    } else if word & 0x8000 != 0 {
        if word >> 24 & 0xf8 == 8 && word & 0x000f_0000 != 0 { Some((ResponseOutcome::Recover, 0)) }
        else { Some((ResponseOutcome::Invalid(InvalidResponse::InvalidHeader), 0x0004_0000)) }
    } else if word & 0x7fc0 == 0x3c0 { Some((ResponseOutcome::Recover, 0))
    } else if response.command_id() != previous_command.command_id() { Some((ResponseOutcome::Invalid(InvalidResponse::CommandMismatch), 0x0004_0000))
    } else { None };
    if let Some((outcome, flags)) = failure {
        result.outcome = outcome;
        if outcome == ResponseOutcome::Recover && recovery == RecoveryMode::Awaiting { result.recovery = RecoveryMode::Restart; }
        if recovery != RecoveryMode::Awaiting { result.acknowledgement.0 |= flags; }
    } else {
        if recovery == RecoveryMode::Awaiting { result.recovery = RecoveryMode::Normal; }
        result.acknowledgement.0 |= word & 0x7fc0;
        if word == 0 { result.acknowledgement.0 |= 0x0010_0000; }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> ResponseContext {
        ResponseContext {
            current_command: CommandWord::new(0x400), previous_command: CommandWord::new(0x400),
            last_response: Some(CommandWord::new(0x400)), expected_sequence: Sequence::new(3).unwrap(),
            recovery: RecoveryMode::Normal, busy_count: RetryCount::new(0), busy_limit: RetryLimit::new(2),
        }
    }

    #[test]
    fn valid_response_advances_expected_sequence() {
        let reply = [0x0b, 0x00, 0x84, 0x3a];
        let result = classify_ack(&reply, context()).unwrap();
        assert_eq!(result.outcome, ResponseOutcome::Accepted);
        assert!(result.acknowledges_previous);
        assert_eq!(result.expected_sequence.get(), 4);
        assert!(result.reset_retry_count);
    }

    #[test]
    fn checksum_corruption_is_rejected() {
        let result = classify_ack(&[0x0b, 0x00, 0x84, 0x38], context()).unwrap();
        assert_eq!(result.outcome, ResponseOutcome::Invalid(InvalidResponse::Checksum));
        assert!(!result.acknowledges_previous);
    }

    #[test]
    fn short_and_overflow_lengths_are_rejected() {
        assert_eq!(classify_ack(&[0, 0, 0], context()), Err(FrameError::ShortFrame));
        assert_eq!(PayloadLength::new(65536), Err(FrameError::LengthOverflow));
        let mut frame = OwnedFrame::<7>::new();
        assert_eq!(frame.build_read(PageId::new(0x1234), PayloadLength::new(4).unwrap(), ReadMarker::Enabled), Err(FrameError::LengthOverflow));
        assert_eq!(parse_page_response(&[0x12, 0x34, 0], PageId::new(0x1234), PayloadLength::new(0).unwrap(), ChecksumMode::Enabled), Err(FrameError::ShortFrame));
    }

    #[test]
    fn response_command_mismatch_is_rejected() {
        let result = classify_ack(&[0x0b, 0x00, 0x88, 0x0a], context()).unwrap();
        assert_eq!(result.outcome, ResponseOutcome::Invalid(InvalidResponse::CommandMismatch));
    }

    #[test]
    fn page_frame_carries_big_endian_page_and_byte_sum() {
        let mut frame = OwnedFrame::<8>::new();
        frame.build_write(PageId::new(0x1234), &[0xfe, 1], ChecksumMode::Enabled).unwrap();
        assert_eq!(frame.as_bytes(), &[0x12, 0x34, 0xfe, 1, 0x45, 0]);
        let response = parse_page_response(frame.as_bytes(), PageId::new(0x1234), PayloadLength::new(2).unwrap(), ChecksumMode::Enabled).unwrap();
        assert_eq!(response.payload, &[0xfe, 1]);
        assert_eq!(parse_page_response(frame.as_bytes(), PageId::new(0x1235), PayloadLength::new(2).unwrap(), ChecksumMode::Enabled), Err(FrameError::PageMismatch));
        assert_eq!(parse_page_response(&[0x12, 0x34, 0xfe, 1, 0x44, 0], PageId::new(0x1234), PayloadLength::new(2).unwrap(), ChecksumMode::Enabled), Err(FrameError::ChecksumMismatch));
    }

    #[test]
    fn busy_limit_and_recovery_follow_configured_state() {
        let mut state = context();
        let busy = CommandWord::new(0x0b10_8400).with_checksum().wire_bytes();
        let first = classify_ack(&busy, state).unwrap();
        assert_eq!(first.outcome, ResponseOutcome::Busy);
        state.busy_count = first.busy_count;
        assert_eq!(classify_ack(&busy, state).unwrap().outcome, ResponseOutcome::RetryExhausted);
        state.recovery = RecoveryMode::Awaiting;
        let result = classify_ack(&[0xc3, 0x3c, 0xa5, 0x5a], state).unwrap();
        assert_eq!(result.outcome, ResponseOutcome::Recover);
        assert_eq!(result.recovery, RecoveryMode::Restart);
    }

    #[test]
    fn incoming_command_builds_acknowledgement_and_detects_mismatch() {
        let ack = CommandWord::new(0x0b00_8000);
        let result = classify_command(&[0, 0, 4, 0x10], CommandWord::new(0x400), ack, RecoveryMode::Normal, false).unwrap();
        assert_eq!(result.outcome, ResponseOutcome::Accepted);
        assert_eq!(result.acknowledgement.with_checksum().wire_bytes(), [0x0b, 0, 0x84, 0x3a]);
        let mismatch = classify_command(&[0, 0, 8, 0x20], CommandWord::new(0x400), ack, RecoveryMode::Normal, false).unwrap();
        assert_eq!(mismatch.outcome, ResponseOutcome::Invalid(InvalidResponse::CommandMismatch));
    }

    #[test]
    fn physical_frame_boundary_and_reset_magic_are_exact() {
        assert_eq!(PayloadLength::new(2077), Err(FrameError::LengthOverflow));
        let mut frame = OwnedFrame::<2080>::new();
        frame.build_read(PageId::new(0xabcd), PayloadLength::new(2076).unwrap(), ReadMarker::Enabled).unwrap();
        assert_eq!(frame.as_bytes().len(), 2080);
        assert_eq!(&frame.as_bytes()[..2], &[0xab, 0xcd]);
        assert_eq!(&frame.as_bytes()[2078..], &[0x5a, 0]);
        assert_eq!(classify_reset(&[0xc3, 0x3c, 0xa5, 0x5a]), Ok(ResetResponse::Accepted));
        assert_eq!(classify_reset(&[0xa5, 0x5a, 0x3c, 0xc3]), Ok(ResetResponse::Recover));
    }

    #[test]
    fn command_queue_preserves_upper_word_and_rolls_sequence() {
        let queued = QueuedCommand::new(CommandWord::new(0x1234_5601), Sequence::new(7).unwrap());
        assert_eq!(queued.command.raw() & !0x3f, 0x1234_5600);
        assert_eq!(queued.acknowledgement.raw(), 0x0f00_8000);
        assert_eq!(Sequence::new(7).unwrap().next().get(), 0);
        assert_eq!(fallback_command(CommandWord::new(0x400)).raw() & !0x3f, 0x0010_03c0);
    }

    #[test]
    fn write_preserves_unwritten_trailer_bytes() {
        let mut frame = OwnedFrame::<8>::new();
        frame.build_write(PageId::new(0x1234), &[0x77, 0x88, 0x99], ChecksumMode::Disabled).unwrap();
        frame.build_write(PageId::new(0x1234), &[1], ChecksumMode::Disabled).unwrap();
        assert_eq!(frame.as_bytes(), &[0x12, 0x34, 1, 0x88, 0x99]);
    }
}
