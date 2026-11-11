use super::stream::{ByteLength, Packet, PcmAddress};
pub const CAPACITY: usize = 900;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PacketIndex(usize);
impl PacketIndex {
    pub const fn from_abi(index: u32) -> Option<Self> { if index < CAPACITY as u32 { Some(Self(index as usize)) } else { None } }
    pub const fn get(self) -> usize { self.0 }
    fn next(self) -> Self { Self((self.0 + 1) % CAPACITY) }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tag { Free, Ready, Reserved, Other(i32) }
impl Tag {
    pub const fn from_abi(value: i32) -> Self { match value { -1 => Self::Free, 0 => Self::Ready, 1 => Self::Reserved, other => Self::Other(other) } }
    pub const fn word(self) -> i32 { match self { Self::Free => -1, Self::Ready => 0, Self::Reserved => 1, Self::Other(value) => value } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Stride(pub i32);
pub trait RingMemory {
    fn read_cursor(&self) -> PacketIndex;
    fn write_cursor(&self) -> PacketIndex;
    fn set_read_cursor(&mut self, index: PacketIndex);
    fn set_write_cursor(&mut self, index: PacketIndex);
    fn tag(&self, index: PacketIndex) -> Tag;
    fn set_tag(&mut self, index: PacketIndex, tag: Tag);
    fn packet(&self, index: PacketIndex) -> Packet;
    fn publish(&mut self, index: PacketIndex, packet: Packet, stride: Stride);
}
pub fn initialize(memory: &mut impl RingMemory) {
    for index in 0..CAPACITY { memory.set_tag(PacketIndex(index), Tag::Free); }
    memory.set_write_cursor(PacketIndex(0));
    memory.set_read_cursor(PacketIndex(0));
}
pub fn front(memory: &impl RingMemory) -> Option<PacketIndex> {
    let mut index = memory.read_cursor();
    for _ in 0..CAPACITY { if memory.tag(index) == Tag::Ready { return Some(index); } index = index.next(); }
    None
}
pub fn reserve(memory: &mut impl RingMemory) -> Option<PacketIndex> {
    let mut index = memory.write_cursor();
    for _ in 0..CAPACITY {
        if memory.tag(index) == Tag::Free { memory.set_tag(index, Tag::Reserved); memory.set_write_cursor(index.next()); return Some(index); }
        index = index.next();
    }
    memory.set_write_cursor(index);
    None
}
pub fn release(memory: &mut impl RingMemory, index: Option<PacketIndex>) {
    let Some(index) = index else { return; };
    memory.set_tag(index, Tag::Free);
    if memory.read_cursor() != index { return; }
    let mut cursor = index.next();
    memory.set_read_cursor(cursor);
    let write = memory.write_cursor();
    for _ in 0..CAPACITY - 1 {
        if memory.tag(cursor) == Tag::Ready || cursor == write { break; }
        cursor = cursor.next();
    }
    memory.set_read_cursor(cursor);
}
pub fn enqueue(memory: &mut impl RingMemory, mut position: PcmAddress, mut remaining: ByteLength, stride: Stride) {
    let stride = if stride.0 == 0 { Stride(1) } else { stride };
    let chunk = (stride.0 as u32).wrapping_mul(9600);
    let chunk = if stride.0 < 0 { chunk.wrapping_neg() } else { chunk };
    let mut first = true;
    while remaining.0 != 0 {
        let length = ByteLength(if chunk.wrapping_mul(2) <= remaining.0 { chunk } else { remaining.0 });
        if let Some(index) = reserve(memory) { memory.publish(index, Packet { position, length, first }, stride); }
        position.0 = if stride.0 < 0 { position.0.wrapping_sub(length.0) } else { position.0.wrapping_add(length.0) };
        remaining.0 = remaining.0.wrapping_sub(length.0);
        first = false;
    }
}

#[cfg(target_arch = "arm")]
pub mod native {
    use super::*;
    use super::super::stream::native::{put, word};
    const BASE: u32 = 0x966998;
    fn at(index: PacketIndex) -> u32 { BASE + 12 + index.0 as u32 * 20 }
    unsafe fn cursor(address: u32) -> PacketIndex {
        match PacketIndex::from_abi(word(address)) { Some(index) => index, None => core::arch::asm!("2: b 2b", in("r0") address, options(noreturn, nostack)) }
    }
    pub struct InstalledRing;
    impl RingMemory for InstalledRing {
        fn read_cursor(&self) -> PacketIndex { unsafe { cursor(BASE + 4) } }
        fn write_cursor(&self) -> PacketIndex { unsafe { cursor(BASE) } }
        fn set_read_cursor(&mut self, index: PacketIndex) { unsafe { put(BASE + 4, index.0 as u32); } }
        fn set_write_cursor(&mut self, index: PacketIndex) { unsafe { put(BASE, index.0 as u32); } }
        fn tag(&self, index: PacketIndex) -> Tag { Tag::from_abi(unsafe { word(at(index)) } as i32) }
        fn set_tag(&mut self, index: PacketIndex, tag: Tag) { unsafe { put(at(index), tag.word() as u32); } }
        fn packet(&self, index: PacketIndex) -> Packet { unsafe { let at = at(index); Packet { position: PcmAddress(word(at + 4)), length: ByteLength(word(at + 8)), first: core::ptr::read_volatile((at + 16) as *const u8) == 1 } } }
        fn publish(&mut self, index: PacketIndex, packet: Packet, stride: Stride) { unsafe {
            let at = at(index);
            put(at + 4, packet.position.0); put(at + 8, packet.length.0); put(at + 12, stride.0 as u32);
            core::ptr::write_volatile((at + 16) as *mut u8, u8::from(packet.first));
            put(at, 0);
        } }
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_audio_stream_enqueue_command() {
        let command = word(0x9566d8);
        let operation = core::ptr::read_volatile(command as *const u16);
        if operation.wrapping_sub(0x2e03) < 2 {
            enqueue(&mut InstalledRing, PcmAddress(word(command + 4)), ByteLength(word(command + 8)), Stride(word(command + 12) as i32));
        }
    }
    #[no_mangle]
    pub extern "C" fn xm1_power_audio_stream_queue_initialize() { initialize(&mut InstalledRing); }
}
