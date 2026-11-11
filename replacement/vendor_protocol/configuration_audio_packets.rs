use xm1_power_lifecycle::audio_release::{packet_ring::{self, PacketIndex, RingMemory, Stride, Tag}, stream};
const PACKET_COUNT: usize = packet_ring::CAPACITY;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PcmAddress(u32);
impl PcmAddress {
    pub const fn from_abi(address: u32) -> Self { Self(address) }
    pub const fn get(self) -> u32 { self.0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PcmByteLength(u32);
impl PcmByteLength {
    pub const fn from_abi(length: u32) -> Self { Self(length) }
    pub const fn get(self) -> u32 { self.0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Packet {
    position: PcmAddress,
    length: PcmByteLength,
    first: bool,
}
impl Packet {
    pub const fn position(self) -> PcmAddress { self.position }
    pub const fn length(self) -> PcmByteLength { self.length }
    pub const fn first(self) -> bool { self.first }
}

#[derive(Clone, Copy)]
enum Slot { Free, Reserved, Queued(Packet) }

pub struct ServicePacketQueue {
    slots: [Slot; PACKET_COUNT],
    write: usize,
    read: usize,
}
impl ServicePacketQueue {
    pub const fn new() -> Self { Self { slots: [Slot::Free; PACKET_COUNT], write: 0, read: 0 } }

    pub fn submit(&mut self, position: PcmAddress, length: PcmByteLength) {
        packet_ring::enqueue(self, stream::PcmAddress(position.get()), stream::ByteLength(length.get()), Stride(1));
    }
    pub fn submit_second(&mut self, position: PcmAddress) {
        self.submit(position, PcmByteLength::from_abi(super::PCM_BYTES_PER_SECOND as u32));
    }
    pub fn clear(&mut self) {
        self.slots.fill(Slot::Free);
        self.read = 0;
        self.write = 0;
    }

    pub fn front(&self) -> Option<Packet> {
        packet_ring::front(self).map(|index| {
            let packet = RingMemory::packet(self, index);
            Packet { position: PcmAddress(packet.position.0), length: PcmByteLength(packet.length.0), first: packet.first }
        })
    }

    pub fn release_front(&mut self) -> Option<Packet> {
        let index = packet_ring::front(self)?;
        let packet = self.front();
        packet_ring::release(self, Some(index));
        packet
    }
}
impl Default for ServicePacketQueue { fn default() -> Self { Self::new() } }

impl RingMemory for ServicePacketQueue {
    fn read_cursor(&self) -> PacketIndex { PacketIndex::from_abi(self.read as u32).unwrap() }
    fn write_cursor(&self) -> PacketIndex { PacketIndex::from_abi(self.write as u32).unwrap() }
    fn set_read_cursor(&mut self, index: PacketIndex) { self.read = index.get(); }
    fn set_write_cursor(&mut self, index: PacketIndex) { self.write = index.get(); }
    fn tag(&self, index: PacketIndex) -> Tag {
        match self.slots[index.get()] { Slot::Free => Tag::Free, Slot::Reserved => Tag::Reserved, Slot::Queued(_) => Tag::Ready }
    }
    fn set_tag(&mut self, index: PacketIndex, tag: Tag) {
        self.slots[index.get()] = match tag { Tag::Free => Slot::Free, Tag::Reserved => Slot::Reserved, _ => self.slots[index.get()] };
    }
    fn packet(&self, index: PacketIndex) -> stream::Packet {
        match self.slots[index.get()] {
            Slot::Queued(packet) => stream::Packet { position: stream::PcmAddress(packet.position.get()), length: stream::ByteLength(packet.length.get()), first: packet.first },
            _ => unreachable!(),
        }
    }
    fn publish(&mut self, index: PacketIndex, packet: stream::Packet, _: Stride) {
        self.slots[index.get()] = Slot::Queued(Packet { position: PcmAddress(packet.position.0), length: PcmByteLength(packet.length.0), first: packet.first });
    }
}

#[cfg(test)]
mod tests {
    use super::{PcmAddress, ServicePacketQueue};

    #[test]
    fn second_packets_cover_the_source_in_order() {
        let mut queue = ServicePacketQueue::new();
        queue.submit_second(PcmAddress::from_abi(0x42e40010));
        let mut position = 0x42e40010;
        let mut bytes = 0;
        let mut first = true;
        while let Some(packet) = queue.release_front() {
            assert_eq!(packet.position().get(), position);
            assert_eq!(packet.first(), first);
            position += packet.length().get();
            bytes += packet.length().get();
            first = false;
        }
        assert_eq!(bytes, 192_000);
    }

    #[test]
    fn full_ring_retains_already_queued_audio() {
        let mut queue = ServicePacketQueue::new();
        for second in 0..46 { queue.submit_second(PcmAddress::from_abi(second * 192_000)); }
        let mut bytes = 0;
        while let Some(packet) = queue.release_front() { bytes += packet.length().get(); }
        assert_eq!(bytes, 45 * 192_000);
        queue.submit_second(PcmAddress::from_abi(0x43014c10));
        assert_eq!(queue.front().unwrap().position().get(), 0x43014c10);
    }
}
