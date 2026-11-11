use super::{after, TcpSequence, TransportError, TransportTick};
use super::super::packet::PacketError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SegmentSize(u16);
impl SegmentSize {
    pub fn parse(value: u16) -> Option<Self> { (value != 0).then_some(Self(value)) }
    pub fn get(self) -> usize { usize::from(self.0) }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowShift(u8);
impl WindowShift {
    pub fn parse(value: u8) -> Option<Self> { (value <= 14).then_some(Self(value)) }
    pub fn get(self) -> u8 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TcpTimestamp(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimestampPair { pub value: TcpTimestamp, pub echo: TcpTimestamp }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SackRange { pub start: TcpSequence, pub end: TcpSequence }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TcpOptions {
    pub maximum_segment: Option<SegmentSize>,
    pub window_shift: Option<WindowShift>,
    pub selective_ack: bool,
    pub timestamp: Option<TimestampPair>,
    pub sacks: [Option<SackRange>; 4],
}
impl TcpOptions {
    pub const fn empty() -> Self {
        Self { maximum_segment: None, window_shift: None, selective_ack: false, timestamp: None, sacks: [None; 4] }
    }
    pub fn parse(bytes: &[u8]) -> Result<Self, TransportError> {
        let mut options = Self::empty();
        let mut offset = 0;
        while offset < bytes.len() {
            let kind = bytes[offset];
            if kind == 0 { break; }
            if kind == 1 { offset += 1; continue; }
            let length = bytes.get(offset + 1).copied().map(usize::from).ok_or(TransportError::Packet(PacketError::Length))?;
            if length < 2 || offset + length > bytes.len() { return Err(TransportError::Packet(PacketError::Length)); }
            let value = &bytes[offset..offset + length];
            match kind {
                2 => {
                    if length != 4 { return Err(TransportError::Packet(PacketError::Length)); }
                    options.maximum_segment = Some(SegmentSize::parse(u16::from_be_bytes([value[2], value[3]]))
                        .ok_or(TransportError::Packet(PacketError::Length))?);
                }
                3 => {
                    if length != 3 { return Err(TransportError::Packet(PacketError::Length)); }
                    options.window_shift = Some(WindowShift(value[2].min(14)));
                }
                4 => {
                    if length != 2 { return Err(TransportError::Packet(PacketError::Length)); }
                    options.selective_ack = true;
                }
                5 => {
                    if length < 10 || (length - 2) % 8 != 0 || length > 34 { return Err(TransportError::Packet(PacketError::Length)); }
                    for (index, pair) in value[2..].chunks_exact(8).enumerate() {
                        let start = word(&pair[..4]);
                        let end = word(&pair[4..]);
                        if !after(end, start) { return Err(TransportError::Packet(PacketError::Length)); }
                        options.sacks[index] = Some(SackRange { start: TcpSequence(start), end: TcpSequence(end) });
                    }
                }
                8 => {
                    if length != 10 { return Err(TransportError::Packet(PacketError::Length)); }
                    options.timestamp = Some(TimestampPair { value: TcpTimestamp(word(&value[2..6])), echo: TcpTimestamp(word(&value[6..10])) });
                }
                _ => {}
            }
            offset += length;
        }
        Ok(options)
    }
    pub fn encode(self, output: &mut [u8; 40]) -> usize {
        let mut length = 0;
        if let Some(size) = self.maximum_segment {
            output[..4].copy_from_slice(&[2, 4, (size.0 >> 8) as u8, size.0 as u8]);
            length = 4;
        }
        if let Some(shift) = self.window_shift {
            output[length..length + 4].copy_from_slice(&[1, 3, 3, shift.0]);
            length += 4;
        }
        if self.selective_ack {
            output[length..length + 4].copy_from_slice(&[1, 1, 4, 2]);
            length += 4;
        }
        if let Some(timestamp) = self.timestamp {
            output[length..length + 4].copy_from_slice(&[1, 1, 8, 10]);
            output[length + 4..length + 8].copy_from_slice(&timestamp.value.0.to_be_bytes());
            output[length + 8..length + 12].copy_from_slice(&timestamp.echo.0.to_be_bytes());
            length += 12;
        }
        let count = self.sacks.iter().take_while(|range| range.is_some()).count().min((40 - length).saturating_sub(2) / 8);
        if count != 0 {
            output[length] = 5;
            output[length + 1] = (2 + count * 8) as u8;
            length += 2;
            for range in self.sacks[..count].iter().flatten() {
                output[length..length + 4].copy_from_slice(&range.start.0.to_be_bytes());
                output[length + 4..length + 8].copy_from_slice(&range.end.0.to_be_bytes());
                length += 8;
            }
        }
        while length % 4 != 0 { output[length] = 1; length += 1; }
        length
    }
}
fn word(bytes: &[u8]) -> u32 { u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransportTiming {
    pub smoothed_milliseconds: u32,
    pub deviation_milliseconds: u32,
    pub retransmit_milliseconds: u32,
    pub congestion_bytes: usize,
    pub slow_start_threshold: usize,
}
#[derive(Clone, Copy)]
pub(super) struct Recovery {
    scaled_rtt: u32,
    deviation: u32,
    pub timeout: u32,
    pub congestion: usize,
    pub threshold: usize,
    credit: usize,
    duplicate_acks: u8,
    recovering_until: Option<u32>,
}
impl Recovery {
    pub const fn new() -> Self {
        Self { scaled_rtt: 0, deviation: 0, timeout: 1000, congestion: 536, threshold: 65535,
            credit: 0, duplicate_acks: 0, recovering_until: None }
    }
    pub fn initialize(&mut self, segment: usize) { self.congestion = segment.max(1); }
    pub fn sample(&mut self, sample: u32) {
        let sample = sample.clamp(1, 60_000);
        if self.scaled_rtt == 0 {
            self.scaled_rtt = sample * 8;
            self.deviation = sample * 2;
        } else {
            let delta = i64::from(sample) - i64::from(self.scaled_rtt >> 3);
            self.scaled_rtt = (i64::from(self.scaled_rtt) + delta).max(1) as u32;
            self.deviation = (i64::from(self.deviation) + delta.abs() - i64::from(self.deviation >> 2)).max(1) as u32;
        }
        self.timeout = ((self.scaled_rtt >> 3) + self.deviation).clamp(200, 60_000);
    }
    pub fn acknowledgment(&mut self, bytes: usize, sequence: u32, segment: usize) {
        self.duplicate_acks = 0;
        if let Some(end) = self.recovering_until {
            if after(end, sequence) { return; }
            self.recovering_until = None;
            self.congestion = self.threshold;
        }
        if self.congestion < self.threshold { self.congestion = self.congestion.saturating_add(bytes.min(segment)); }
        else {
            self.credit = self.credit.saturating_add(bytes);
            if self.credit >= self.congestion {
                self.credit -= self.congestion;
                self.congestion = self.congestion.saturating_add(segment);
            }
        }
    }
    pub fn duplicate(&mut self, flight: usize, segment: usize, high: u32) -> bool {
        self.duplicate_acks = self.duplicate_acks.saturating_add(1);
        if self.duplicate_acks == 3 {
            self.threshold = (flight / 2).max(segment.saturating_mul(2));
            self.congestion = self.threshold.saturating_add(segment.saturating_mul(3));
            self.recovering_until = Some(high);
            true
        } else {
            if self.duplicate_acks > 3 { self.congestion = self.congestion.saturating_add(segment); }
            false
        }
    }
    pub fn timeout(&mut self, flight: usize, segment: usize) {
        self.threshold = (flight / 2).max(segment.saturating_mul(2));
        self.congestion = segment.max(1);
        self.credit = 0;
        self.duplicate_acks = 0;
        self.recovering_until = None;
    }
    pub fn status(self) -> TransportTiming {
        TransportTiming { smoothed_milliseconds: self.scaled_rtt >> 3, deviation_milliseconds: self.deviation,
            retransmit_milliseconds: self.timeout, congestion_bytes: self.congestion, slow_start_threshold: self.threshold }
    }
}
pub(super) const fn local_shift(capacity: usize) -> WindowShift {
    let mut shift = 0;
    while shift < 14 && capacity >> shift > 65535 { shift += 1; }
    WindowShift(shift)
}
pub(super) fn timestamp(now: TransportTick, echo: TcpTimestamp) -> TimestampPair {
    TimestampPair { value: TcpTimestamp(now.0), echo }
}
