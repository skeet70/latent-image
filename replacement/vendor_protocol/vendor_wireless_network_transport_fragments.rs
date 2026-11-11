use super::super::packet::{InternetChecksum, PacketError};
use super::super::state::Ipv4Address;
use super::{TransportError, TransportTick};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReassemblyOutcome { Buffered, Complete { length: usize } }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FragmentKey { source: Ipv4Address, destination: Ipv4Address, identification: u16, protocol: u8 }

struct Assembly<const CAPACITY: usize> {
    key: Option<FragmentKey>,
    deadline: TransportTick,
    header: [u8; 60],
    header_length: usize,
    data: [u8; CAPACITY],
    present: [bool; CAPACITY],
    end: Option<usize>,
    high: usize,
}
impl<const CAPACITY: usize> Assembly<CAPACITY> {
    const fn new() -> Self {
        Self { key: None, deadline: TransportTick(0), header: [0; 60], header_length: 0,
            data: [0; CAPACITY], present: [false; CAPACITY], end: None, high: 0 }
    }
    fn reset(&mut self) {
        self.key = None;
        self.header_length = 0;
        self.present.fill(false);
        self.end = None;
        self.high = 0;
    }
}

pub struct FragmentReassembler<const ENTRIES: usize, const CAPACITY: usize> {
    entries: [Assembly<CAPACITY>; ENTRIES],
}
impl<const ENTRIES: usize, const CAPACITY: usize> FragmentReassembler<ENTRIES, CAPACITY> {
    pub const fn new() -> Self { Self { entries: [const { Assembly::new() }; ENTRIES] } }
    pub fn clear(&mut self) { for entry in &mut self.entries { entry.reset(); } }
    pub fn expire(&mut self, now: TransportTick) -> usize {
        let mut count = 0;
        for entry in &mut self.entries {
            if entry.key.is_some() && now.reached(entry.deadline) { entry.reset(); count += 1; }
        }
        count
    }
    pub fn expire_ipv4(&mut self, now: TransportTick, output: &mut [u8])
        -> Result<super::TransportOutcome, TransportError> {
        for entry in &mut self.entries {
            if entry.key.is_none() || !now.reached(entry.deadline) { continue; }
            let key = entry.key.ok_or(TransportError::InvalidState)?;
            if entry.header_length == 0 || key.source.0 == [0; 4] || key.source.0[0] >= 224 ||
                key.destination.0[0] >= 224 || key.destination.0 == [255; 4] {
                entry.reset();
                continue;
            }
            let quoted = entry.header_length + entry.high.min(8);
            let length = 28 + quoted;
            if output.len() < length { return Err(TransportError::Capacity); }
            output[..28].fill(0);
            output[0] = 0x45;
            output[2..4].copy_from_slice(&(length as u16).to_be_bytes());
            output[8] = 64;
            output[9] = 1;
            output[12..16].copy_from_slice(&key.destination.0);
            output[16..20].copy_from_slice(&key.source.0);
            output[20] = 11;
            output[21] = 1;
            output[28..28 + entry.header_length].copy_from_slice(&entry.header[..entry.header_length]);
            output[28 + entry.header_length..length].copy_from_slice(&entry.data[..entry.high.min(8)]);
            let mut checksum = InternetChecksum::new();
            checksum.update(&output[20..length]);
            output[22..24].copy_from_slice(&checksum.finish().to_be_bytes());
            let mut checksum = InternetChecksum::new();
            checksum.update(&output[..20]);
            output[10..12].copy_from_slice(&checksum.finish().to_be_bytes());
            entry.reset();
            return Ok(super::TransportOutcome::Packet { length });
        }
        Ok(super::TransportOutcome::Buffered)
    }
    pub fn receive_ipv4(&mut self, packet: &[u8], now: TransportTick, output: &mut [u8])
        -> Result<ReassemblyOutcome, TransportError> {
        let (header, total) = validate_ipv4(packet)?;
        let flags = u16::from_be_bytes([packet[6], packet[7]]);
        if flags & 0x3fff == 0 {
            if output.len() < total { return Err(TransportError::Capacity); }
            output[..total].copy_from_slice(&packet[..total]);
            return Ok(ReassemblyOutcome::Complete { length: total });
        }
        let offset = usize::from(flags & 0x1fff) * 8;
        let more = flags & 0x2000 != 0;
        let payload = &packet[header..total];
        let end = offset.checked_add(payload.len()).ok_or(TransportError::Capacity)?;
        if payload.is_empty() || (more && payload.len() % 8 != 0) || flags & 0xc000 != 0 {
            return Err(TransportError::Packet(PacketError::Fragment));
        }
        if end > CAPACITY || end > 65515 { return Err(TransportError::Capacity); }
        let key = FragmentKey { source: Ipv4Address([packet[12], packet[13], packet[14], packet[15]]),
            destination: Ipv4Address([packet[16], packet[17], packet[18], packet[19]]),
            identification: u16::from_be_bytes([packet[4], packet[5]]), protocol: packet[9] };
        let index = self.entries.iter().position(|entry| entry.key == Some(key))
            .or_else(|| self.entries.iter().position(|entry| entry.key.is_none())).ok_or(TransportError::Full)?;
        let entry = &mut self.entries[index];
        if output.len() < end.max(entry.high).saturating_add(header.max(entry.header_length)) {
            return Err(TransportError::Capacity);
        }
        if entry.key.is_none() {
            entry.key = Some(key);
            entry.deadline = TransportTick(now.0.wrapping_add(60_000));
        }
        if entry.end.is_some_and(|limit| end > limit || (!more && end != limit)) || (!more && entry.high > end) {
            entry.reset();
            return Err(TransportError::Packet(PacketError::Fragment));
        }
        if offset == 0 {
            if end + header > 65535 { entry.reset(); return Err(TransportError::Capacity); }
            entry.header[..header].copy_from_slice(&packet[..header]);
            entry.header_length = header;
        }
        // Stock trims a new fragment against its predecessor and replaces covered successors.
        let prefix = entry.present[offset..end].iter().take_while(|present| **present).count();
        entry.data[offset + prefix..end].copy_from_slice(&payload[prefix..]);
        entry.present[offset + prefix..end].fill(true);
        entry.high = entry.high.max(end);
        if !more { entry.end = Some(end); }
        let Some(length) = entry.end else { return Ok(ReassemblyOutcome::Buffered); };
        if entry.header_length == 0 || entry.present[..length].iter().any(|present| !present) {
            return Ok(ReassemblyOutcome::Buffered);
        }
        let header = entry.header_length;
        let total = header + length;
        if total > 65535 { entry.reset(); return Err(TransportError::Capacity); }
        output[..header].copy_from_slice(&entry.header[..header]);
        output[header..total].copy_from_slice(&entry.data[..length]);
        output[2..4].copy_from_slice(&(total as u16).to_be_bytes());
        output[6] &= 0xc0;
        output[7] = 0;
        output[10..12].fill(0);
        let mut checksum = InternetChecksum::new();
        checksum.update(&output[..header]);
        output[10..12].copy_from_slice(&checksum.finish().to_be_bytes());
        entry.reset();
        Ok(ReassemblyOutcome::Complete { length: total })
    }
}
impl<const ENTRIES: usize, const CAPACITY: usize> Default for FragmentReassembler<ENTRIES, CAPACITY> {
    fn default() -> Self { Self::new() }
}

pub(super) fn validate_ipv4(packet: &[u8]) -> Result<(usize, usize), TransportError> {
    if packet.len() < 20 { return Err(TransportError::Packet(PacketError::Truncated)); }
    if packet[0] >> 4 != 4 { return Err(TransportError::Packet(PacketError::Version)); }
    let header = usize::from(packet[0] & 15) * 4;
    let total = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
    if header < 20 { return Err(TransportError::Packet(PacketError::HeaderLength)); }
    if total < header { return Err(TransportError::Packet(PacketError::Length)); }
    if total > packet.len() || header > packet.len() { return Err(TransportError::Packet(PacketError::Truncated)); }
    let mut checksum = InternetChecksum::new();
    checksum.update(&packet[..header]);
    if checksum.finish() != 0 { return Err(TransportError::Packet(PacketError::Checksum)); }
    Ok((header, total))
}
