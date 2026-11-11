//! Aggregate buffers remain published until every packet lease is dropped.

use core::cell::{Cell, UnsafeCell};
use super::ReceiveRead;
use super::super::vendor_wireless_station_driver::receive::{FirmwareEventKind, Packet,
    PeerIndex, ReceiveError, ReceiveSignal};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AggregateIdentity { slot: usize, generation: u64 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PoolError { Empty, GenerationExhausted, Stale, Receive(ReceiveError), InvalidSpan }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PacketMetadata {
    Ethernet { header_length: u8, peer: PeerIndex, signal: ReceiveSignal, status: u32 },
    Management { header_length: u8, peer: PeerIndex, signal: ReceiveSignal, status: u32 },
    Event { kind: FirmwareEventKind },
    Loopback,
}
struct Aggregate<'memory> {
    bytes: UnsafeCell<&'memory mut [u8]>,
    generation: Cell<u64>,
    packets: Cell<u8>,
}

pub struct ReceivePool<'memory, const A: usize> {
    aggregates: [Aggregate<'memory>; A],
    free: [Cell<Option<usize>>; A],
    read: Cell<usize>,
    write: Cell<usize>,
    available: Cell<usize>,
}
impl<'memory, const A: usize> ReceivePool<'memory, A> {
    pub fn new(buffers: [&'memory mut [u8]; A]) -> Self {
        Self { aggregates: buffers.map(|bytes| Aggregate { bytes: UnsafeCell::new(bytes),
            generation: Cell::new(0), packets: Cell::new(0) }),
            free: core::array::from_fn(|index| Cell::new(Some(index))),
            read: Cell::new(0), write: Cell::new(0), available: Cell::new(A) }
    }
    pub fn available(&self) -> usize { self.available.get() }
    pub fn take(&self) -> Result<AggregateWriteLease<'_, 'memory, A>, PoolError> {
        if self.available.get() == 0 { return Err(PoolError::Empty); }
        let index = self.read.get();
        let Some(slot) = self.free[index].take() else { return Err(PoolError::Stale); };
        self.read.set((index + 1) % A);
        self.available.set(self.available.get() - 1);
        let aggregate = &self.aggregates[slot];
        let Some(generation) = aggregate.generation.get().checked_add(1) else {
            return Err(PoolError::GenerationExhausted);
        };
        aggregate.generation.set(generation);
        Ok(AggregateWriteLease { pool: self, identity: AggregateIdentity { slot, generation }, published: false })
    }
    fn recycle(&self, identity: AggregateIdentity) {
        let index = self.write.get();
        self.free[index].set(Some(identity.slot));
        self.write.set((index + 1) % A);
        self.available.set(self.available.get() + 1);
    }
}

pub struct AggregateWriteLease<'pool, 'memory, const A: usize> {
    pool: &'pool ReceivePool<'memory, A>,
    identity: AggregateIdentity,
    published: bool,
}
#[derive(Clone, Copy)]
struct Description { wire: Span, payload: Span, metadata: PacketMetadata }
#[derive(Clone, Copy)]
struct Span { start: usize, end: usize }
impl Span {
    fn of(bytes: &[u8], base: usize, length: usize) -> Result<Self, PoolError> {
        let start = (bytes.as_ptr() as usize).checked_sub(base).ok_or(PoolError::InvalidSpan)?;
        let end = start.checked_add(bytes.len()).filter(|end| *end <= length).ok_or(PoolError::InvalidSpan)?;
        Ok(Self { start, end })
    }
}
impl<'pool, 'memory, const A: usize> AggregateWriteLease<'pool, 'memory, A> {
    pub const fn identity(&self) -> AggregateIdentity { self.identity }
    pub(crate) fn buffer_mut(&mut self) -> &mut [u8] {
        // A writing aggregate has one non-clone lease and no published packets.
        unsafe { &mut **self.pool.aggregates[self.identity.slot].bytes.get() }
    }
    pub fn capacity(&self) -> usize {
        unsafe { (&**self.pool.aggregates[self.identity.slot].bytes.get()).len() }
    }
    pub fn publish(mut self, receipt: ReceiveRead) -> Result<PacketBatch<'pool, 'memory, A>, (PoolError, Self)> {
        if receipt.aggregate != self.identity { return Err((PoolError::Stale, self)); }
        if receipt.cancelled { return Err((PoolError::Receive(ReceiveError::Cancelled), self)); }
        let descriptions = match decode(receipt.plan, self.buffer_mut()) {
            Ok(descriptions) => descriptions,
            Err(error) => return Err((error, self)),
        };
        let count = descriptions.iter().flatten().count();
        if count == 0 { return Ok(PacketBatch { packets: core::array::from_fn(|_| None), next: 0 }); }
        let aggregate = &self.pool.aggregates[self.identity.slot];
        aggregate.packets.set(count as u8);
        self.published = true;
        Ok(PacketBatch { packets: descriptions.map(|description| description.map(|description| PacketLease {
            pool: self.pool, identity: self.identity, description })), next: 0 })
    }
}
fn decode(plan: super::super::vendor_wireless_station_driver::receive::ReceivePlan,
    buffer: &mut [u8]) -> Result<[Option<Description>; 16], PoolError> {
    let base = buffer.as_ptr() as usize;
    let length = buffer.len();
    let mut descriptions = [None; 16];
    for (index, description) in descriptions.iter_mut().enumerate().take(plan.packet_count()) {
        let extent = plan.packet_extent(index).ok_or(PoolError::Receive(ReceiveError::InvalidLength))?;
        let wire = buffer.get(extent.range()).ok_or(PoolError::Receive(ReceiveError::Truncated))?;
        let header = wire.get(..2).ok_or(PoolError::Receive(ReceiveError::Truncated))?;
        if usize::from(u16::from_le_bytes([header[0], header[1]])) != extent.length() {
            return Err(PoolError::Receive(ReceiveError::InvalidLength));
        }
        *description = Some(describe(Packet::parse(wire).map_err(PoolError::Receive)?, base, length)?);
    }
    Ok(descriptions)
}

fn describe(packet: Packet<'_>, base: usize, length: usize) -> Result<Description, PoolError> {
    let (wire, payload, metadata) = match packet {
        Packet::Ethernet(frame) => (frame.wire(), frame.bytes(), PacketMetadata::Ethernet {
            header_length: frame.header_length(), peer: frame.peer(), signal: frame.signal(), status: frame.status() }),
        Packet::Management(frame) => (frame.wire(), frame.bytes(), PacketMetadata::Management {
            header_length: frame.header_length(), peer: frame.peer(), signal: frame.signal(), status: frame.status() }),
        Packet::Event(event) => (event.wire(), event.body(), PacketMetadata::Event { kind: event.kind() }),
        Packet::Loopback(bytes) => (bytes, bytes, PacketMetadata::Loopback),
    };
    Ok(Description { wire: Span::of(wire, base, length)?, payload: Span::of(payload, base, length)?, metadata })
}
impl<const A: usize> Drop for AggregateWriteLease<'_, '_, A> {
    fn drop(&mut self) { if !self.published { self.pool.recycle(self.identity); } }
}

pub struct PacketLease<'pool, 'memory, const A: usize> {
    pool: &'pool ReceivePool<'memory, A>,
    identity: AggregateIdentity,
    description: Description,
}
impl<const A: usize> PacketLease<'_, '_, A> {
    pub const fn aggregate(&self) -> AggregateIdentity { self.identity }
    pub const fn metadata(&self) -> PacketMetadata { self.description.metadata }
    pub fn wire(&self) -> &[u8] {
        let span = self.description.wire;
        unsafe { &(&**self.pool.aggregates[self.identity.slot].bytes.get())[span.start..span.end] }
    }
    pub fn payload(&self) -> &[u8] {
        let span = self.description.payload;
        unsafe { &(&**self.pool.aggregates[self.identity.slot].bytes.get())[span.start..span.end] }
    }
    pub fn packet(&self) -> Result<Packet<'_>, ReceiveError> { Packet::parse(self.wire()) }
}
impl<const A: usize> Drop for PacketLease<'_, '_, A> {
    fn drop(&mut self) {
        let aggregate = &self.pool.aggregates[self.identity.slot];
        let remaining = aggregate.packets.get() - 1;
        aggregate.packets.set(remaining);
        if remaining == 0 { self.pool.recycle(self.identity); }
    }
}
pub struct PacketBatch<'pool, 'memory, const A: usize> {
    packets: [Option<PacketLease<'pool, 'memory, A>>; 16],
    next: usize,
}
impl<'pool, 'memory, const A: usize> Iterator for PacketBatch<'pool, 'memory, A> {
    type Item = PacketLease<'pool, 'memory, A>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.next == 16 { return None; }
        let packet = self.packets[self.next].take();
        self.next += 1;
        packet
    }
}

#[cfg(test)]
#[path = "vendor_wireless_service_host_pool_tests.rs"]
mod tests;
