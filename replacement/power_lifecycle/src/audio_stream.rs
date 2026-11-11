use crate::primitives::ServiceStatus;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Continuation { Input, Output, Finished }
impl Continuation {
    pub const fn from_abi(address: u32) -> Option<Self> {
        match address { 0x6ec1bc => Some(Self::Input), 0x6ec418 => Some(Self::Output), 0x6ec18c => Some(Self::Finished), _ => None }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamEvent { Packet, Empty, Finished }
impl StreamEvent {
    pub const fn command(self) -> u16 { match self { Self::Packet => 0x2e05, Self::Empty => 0x2e06, Self::Finished => 0x2e07 } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PcmAddress(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteLength(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Packet { pub position: PcmAddress, pub length: ByteLength, pub first: bool }
pub trait StreamMemory {
    fn release_front(&mut self);
    fn front(&self) -> Option<Packet>;
    fn immediate_notification(&self) -> bool;
    fn deferred_notification(&self) -> bool;
    fn set_deferred_notification(&mut self, value: bool);
    fn stopping(&self) -> bool;
    fn continue_across_commands(&self) -> bool;
}
pub trait StreamIo {
    fn post(&mut self, event: StreamEvent);
    fn submit(&mut self, position: PcmAddress, length: ByteLength) -> ServiceStatus;
}

pub fn advance(memory: &mut impl StreamMemory, io: &mut impl StreamIo, continuation: Continuation) {
    if continuation == Continuation::Finished { io.post(StreamEvent::Finished); return; }
    let immediate = continuation == Continuation::Input && memory.immediate_notification();
    if continuation == Continuation::Input && !immediate && memory.deferred_notification() {
        io.post(StreamEvent::Packet);
        memory.set_deferred_notification(false);
    }
    memory.release_front();
    let Some(packet) = memory.front() else { io.post(StreamEvent::Empty); return; };
    if packet.first && memory.stopping() && (continuation == Continuation::Output || !memory.continue_across_commands()) {
        io.post(StreamEvent::Empty);
        return;
    }
    let _ = io.submit(packet.position, packet.length);
    if packet.first {
        if continuation == Continuation::Output || immediate { io.post(StreamEvent::Packet); }
        else { memory.set_deferred_notification(true); }
    }
}

#[cfg(target_arch = "arm")]
pub mod native {
    use super::*;
    use crate::shutdown::{QueueId, RequestId, ServiceMessage, ShutdownRtos};
    use crate::shutdown_rtos::NativeShutdownRtos;
    use super::super::stream_pipeline::native::NativePipeline;
    pub unsafe fn word(address: u32) -> u32 { core::ptr::read_volatile(address as *const u32) }
    pub unsafe fn put(address: u32, value: u32) { core::ptr::write_volatile(address as *mut u32, value); }
    use super::super::packet_ring::{self, native::InstalledRing, RingMemory};
    pub struct InstalledStream;
    impl StreamMemory for InstalledStream {
        fn release_front(&mut self) {
            let index = packet_ring::front(&InstalledRing);
            packet_ring::release(&mut InstalledRing, index);
        }
        fn front(&self) -> Option<Packet> { packet_ring::front(&InstalledRing).map(|index| InstalledRing.packet(index)) }
        fn immediate_notification(&self) -> bool { unsafe { word(0x956748) == 1 } }
        fn deferred_notification(&self) -> bool { unsafe { word(0x95674c) == 1 } }
        fn set_deferred_notification(&mut self, value: bool) { unsafe { put(0x95674c, u32::from(value)); } }
        fn stopping(&self) -> bool { unsafe { core::ptr::read_volatile(0x9566f0 as *const u16) == 0x2e01 } }
        fn continue_across_commands(&self) -> bool { unsafe { word(0x9566f4) == 2 } }
    }
    pub struct InstalledStreamIo;
    impl StreamIo for InstalledStreamIo {
        fn post(&mut self, event: StreamEvent) { unsafe { send_request(0x9566c0, event.command(), [0; 3]); } }
        fn submit(&mut self, position: PcmAddress, length: ByteLength) -> ServiceStatus {
            use super::super::stream_pipeline::{EventHandle, Submission};
            super::super::stream_pipeline::submit(&mut NativePipeline::continuation(), EventHandle(unsafe { word(0x956744) }), position, length, Submission::Continuation)
        }
    }
    pub unsafe fn send_request(request: u32, command: u16, arguments: [u32; 3]) {
        let registry = core::ptr::read_volatile(request as *const u8);
        let destination = core::ptr::read_volatile((0x7d088c + u32::from(registry)) as *const u8);
        let request_sender = core::ptr::read_volatile((request + 4) as *const u16) as u8;
        let mut payload = [0u8; 16];
        payload[..2].copy_from_slice(&command.to_le_bytes());
        payload[2] = request_sender;
        payload[3] = destination;
        for (index, argument) in arguments.iter().enumerate() { payload[4 + index * 4..8 + index * 4].copy_from_slice(&argument.to_le_bytes()); }
        let mut rtos = NativeShutdownRtos;
        let sender = rtos.current_task();
        let _ = rtos.post(ServiceMessage { operation: RequestId(command), sender, destination: QueueId(destination), payload: &payload });
    }
    pub fn invoke(continuation: Continuation) { advance(&mut InstalledStream, &mut InstalledStreamIo, continuation); }
}

#[cfg(test)]
#[path = "audio_stream_tests.rs"]
mod tests;
