use std::vec::Vec;
use super::*;
use super::super::packet_ring::{self, PacketIndex, RingMemory, Stride, Tag, CAPACITY};
use super::super::stream_pipeline::{self, Bank, Field, Pipeline, RequestField};

#[derive(Clone, Copy)]
struct Entry { tag: Tag, packet: Packet }
struct Memory { entries: [Entry; CAPACITY], read: PacketIndex, write: PacketIndex, immediate: bool, deferred: bool, stopping: bool, across: bool }
impl Memory {
    fn new() -> Self {
        Self { entries: [Entry { tag: Tag::Free, packet: Packet { position: PcmAddress(0), length: ByteLength(0), first: false } }; CAPACITY], read: PacketIndex::from_abi(0).unwrap(), write: PacketIndex::from_abi(0).unwrap(), immediate: false, deferred: false, stopping: false, across: false }
    }
    fn pop(&mut self) -> Option<Packet> {
        let index = packet_ring::front(self)?;
        let packet = self.packet(index);
        packet_ring::release(self, Some(index));
        Some(packet)
    }
}
impl RingMemory for Memory {
    fn read_cursor(&self) -> PacketIndex { self.read }
    fn write_cursor(&self) -> PacketIndex { self.write }
    fn set_read_cursor(&mut self, index: PacketIndex) { self.read = index; }
    fn set_write_cursor(&mut self, index: PacketIndex) { self.write = index; }
    fn tag(&self, index: PacketIndex) -> Tag { self.entries[index.get()].tag }
    fn set_tag(&mut self, index: PacketIndex, tag: Tag) { self.entries[index.get()].tag = tag; }
    fn packet(&self, index: PacketIndex) -> Packet { self.entries[index.get()].packet }
    fn publish(&mut self, index: PacketIndex, packet: Packet, _stride: Stride) { self.entries[index.get()] = Entry { tag: Tag::Ready, packet }; }
}
impl StreamMemory for Memory {
    fn release_front(&mut self) { let index = packet_ring::front(self); packet_ring::release(self, index); }
    fn front(&self) -> Option<Packet> { packet_ring::front(self).map(|index| self.packet(index)) }
    fn immediate_notification(&self) -> bool { self.immediate }
    fn deferred_notification(&self) -> bool { self.deferred }
    fn set_deferred_notification(&mut self, value: bool) { self.deferred = value; }
    fn stopping(&self) -> bool { self.stopping }
    fn continue_across_commands(&self) -> bool { self.across }
}
struct Output { events: Vec<StreamEvent>, pending: Option<Packet>, result: ServiceStatus }
impl Output { fn new() -> Self { Self { events: Vec::new(), pending: None, result: ServiceStatus(0) } } }
impl StreamIo for Output {
    fn post(&mut self, event: StreamEvent) { self.events.push(event); }
    fn submit(&mut self, position: PcmAddress, length: ByteLength) -> ServiceStatus { self.pending = Some(Packet { position, length, first: false }); self.result }
}

#[test]
fn reverse_stride_keeps_the_short_tail_in_one_packet() {
    let mut memory = Memory::new();
    packet_ring::enqueue(&mut memory, PcmAddress(100_000), ByteLength(50_000), Stride(-2));
    assert_eq!(memory.pop(), Some(Packet { position: PcmAddress(100_000), length: ByteLength(19_200), first: true }));
    assert_eq!(memory.pop(), Some(Packet { position: PcmAddress(80_800), length: ByteLength(30_800), first: false }));
    assert_eq!(memory.pop(), None);
}

#[test]
fn full_ring_drops_new_data_without_overwriting_queued_audio() {
    let mut memory = Memory::new();
    for index in 0..900 { packet_ring::enqueue(&mut memory, PcmAddress(index * 32), ByteLength(8), Stride(1)); }
    packet_ring::enqueue(&mut memory, PcmAddress(0xdead), ByteLength(8), Stride(1));
    for index in 0..900 { assert_eq!(memory.pop().unwrap().position, PcmAddress(index * 32)); }
    assert_eq!(memory.pop(), None);
    packet_ring::enqueue(&mut memory, PcmAddress(0xbeef), ByteLength(8), Stride(1));
    assert_eq!(memory.pop().unwrap().position, PcmAddress(0xbeef));
}

#[test]
fn input_notifies_the_previous_command_before_reporting_empty() {
    let mut memory = Memory::new();
    memory.deferred = true;
    packet_ring::enqueue(&mut memory, PcmAddress(1), ByteLength(8), Stride(1));
    let mut output = Output::new();
    advance(&mut memory, &mut output, Continuation::Input);
    assert_eq!(output.events, [StreamEvent::Packet, StreamEvent::Empty]);
    assert!(!memory.deferred);
    assert!(output.pending.is_none());
}

#[test]
fn input_stop_mode_two_continues_and_defers_notification_even_when_update_fails() {
    let mut memory = Memory::new();
    memory.stopping = true;
    memory.across = true;
    packet_ring::enqueue(&mut memory, PcmAddress(100), ByteLength(8), Stride(1));
    packet_ring::enqueue(&mut memory, PcmAddress(200), ByteLength(8), Stride(1));
    let mut output = Output::new();
    output.result = ServiceStatus(-12);
    advance(&mut memory, &mut output, Continuation::Input);
    assert_eq!(output.pending.unwrap().position, PcmAddress(200));
    assert!(output.events.is_empty());
    assert!(memory.deferred);
}

#[test]
fn output_stop_does_not_cross_a_command_boundary() {
    let mut memory = Memory::new();
    memory.stopping = true;
    memory.across = true;
    packet_ring::enqueue(&mut memory, PcmAddress(100), ByteLength(8), Stride(1));
    packet_ring::enqueue(&mut memory, PcmAddress(200), ByteLength(8), Stride(1));
    let mut output = Output::new();
    advance(&mut memory, &mut output, Continuation::Output);
    assert_eq!(output.events, [StreamEvent::Empty]);
    assert!(output.pending.is_none());
    assert_eq!(memory.front().unwrap().position, PcmAddress(200));
}

struct Device {
    state: [u32; 24], request: [u32; 9], bank: Bank, program_status: ServiceStatus,
    programmed: [u32; 11], padding: Vec<(PcmAddress, ByteLength)>, ready: bool, scheduled: bool, closed: bool,
    stop_status: ServiceStatus, release_status: ServiceStatus, published_phase: Option<u32>, masked: bool,
}
impl Device {
    fn new(direction: u32) -> Self {
        let mut state = [0; 24]; state[Field::Event as usize / 4] = 19; state[Field::Phase as usize / 4] = 1;
        let mut request = [0; 9]; request[0] = direction;
        Self { state, request, bank: Bank::First, program_status: ServiceStatus(0), programmed: [0; 11], padding: Vec::new(), ready: false, scheduled: false, closed: false, stop_status: ServiceStatus(0), release_status: ServiceStatus(0), published_phase: None, masked: false }
    }
}
impl Pipeline for Device {
    fn get(&self, field: Field) -> u32 { self.state[field as usize / 4] }
    fn set(&mut self, field: Field, value: u32) { self.state[field as usize / 4] = value; }
    fn request(&self, field: RequestField) -> u32 { self.request[field.offset() as usize / 4] }
    fn set_error(&mut self, error: u32) { self.request[8] = error; }
    fn selected_bank(&self) -> Bank { self.bank }
    fn channel_index(&mut self) -> u32 { 1 }
    fn program(&mut self) -> ServiceStatus { self.programmed.copy_from_slice(&self.state[13..24]); self.program_status }
    fn start_dma(&mut self) -> ServiceStatus { ServiceStatus(-12) }
    fn dma_pending(&mut self) -> bool { true }
    fn close_dma(&mut self) -> ServiceStatus { self.closed = true; ServiceStatus(-255) }
    fn start_resource(&mut self) -> ServiceStatus { ServiceStatus(0) }
    fn ready(&mut self) { self.ready = true; }
    fn schedule_ready(&mut self) { self.scheduled = true; }
    fn fill(&mut self, address: PcmAddress, length: ByteLength) { self.padding.push((address, length)); }
}
impl stream_pipeline::PipelineLifecycle for Device {
    type Guard = bool;
    fn stop(&mut self) -> ServiceStatus { self.stop_status }
    fn release(&mut self) -> ServiceStatus { self.set(Field::Event, 0); self.release_status }
    fn finished(&mut self) { self.published_phase = Some(self.get(Field::Phase)); }
    fn enter(&mut self) -> bool { let previous = self.masked; self.masked = true; previous }
    fn leave(&mut self, guard: bool) { self.masked = guard; }
}

#[test]
fn receive_start_keeps_the_ignored_dma_start_error_and_programs_second_bank() {
    let mut device = Device::new(0);
    device.bank = Bank::Second;
    assert_eq!(stream_pipeline::submit(&mut device, stream_pipeline::EventHandle(19), PcmAddress(0x4200), ByteLength(9600), stream_pipeline::Submission::First), ServiceStatus(0));
    assert_eq!(device.programmed[3], 0xfffa1020);
    assert_eq!(device.programmed[4], 0x4200);
    assert_eq!(device.programmed[5], 9600);
    assert_eq!(device.programmed[7], 0x93adf8);
    assert_eq!(device.programmed[8], 2048);
    assert_eq!(device.programmed[10], 12);
    assert!(device.ready);
    assert_eq!(device.get(Field::Phase), 2);
}

#[test]
fn transmit_update_adds_the_remainder_instead_of_rounding_up() {
    let mut device = Device::new(1);
    device.set(Field::Phase, 2);
    assert_eq!(stream_pipeline::submit(&mut device, stream_pipeline::EventHandle(19), PcmAddress(0x4200), ByteLength(11), stream_pipeline::Submission::Continuation), ServiceStatus(0));
    assert_eq!(device.padding, [(PcmAddress(0x420b), ByteLength(3))]);
    assert_eq!(device.programmed[6], 0x4200);
    assert_eq!(device.programmed[8], 14);
    assert_eq!(device.get(Field::Phase), 3);
}

#[test]
fn failed_first_transmit_program_closes_dma_and_still_schedules_ready() {
    let mut device = Device::new(1);
    device.program_status = ServiceStatus(-30);
    assert_eq!(stream_pipeline::submit(&mut device, stream_pipeline::EventHandle(19), PcmAddress(0x4200), ByteLength(8), stream_pipeline::Submission::First), ServiceStatus(-30));
    assert!(device.closed);
    assert!(device.scheduled);
    assert_eq!(device.get(Field::Phase), 8);
    assert_eq!(device.request[8], 3);
    assert_eq!(device.get(Field::Source), 0x7d0c74);
    assert_eq!(device.get(Field::Length), 1920);
}

#[test]
fn invalid_generation_sets_error_without_changing_the_active_transfer() {
    let mut device = Device::new(0);
    assert_eq!(stream_pipeline::submit(&mut device, stream_pipeline::EventHandle(0), PcmAddress(0x4200), ByteLength(8), stream_pipeline::Submission::First), ServiceStatus(-20));
    assert_eq!(device.request[8], 2);
    assert_eq!(device.get(Field::Phase), 1);
    assert!(!device.ready && !device.scheduled && !device.closed);
}

#[test]
fn initialize_discards_queued_audio_and_allows_fresh_packets() {
    let mut memory = Memory::new();
    packet_ring::enqueue(&mut memory, PcmAddress(100), ByteLength(20_000), Stride(1));
    let _ = memory.pop();
    packet_ring::initialize(&mut memory);
    assert_eq!(memory.pop(), None);
    packet_ring::enqueue(&mut memory, PcmAddress(400), ByteLength(8), Stride(1));
    assert_eq!(memory.pop().unwrap().position, PcmAddress(400));
    assert_eq!(memory.pop(), None);
}

#[test]
fn receive_cancel_drains_real_completions_before_publishing_finished() {
    use stream_pipeline::{Completion, EventHandle};
    let mut device = Device::new(0);
    device.set(Field::Phase, 3);
    assert_eq!(stream_pipeline::cancel(&mut device, EventHandle(19)), ServiceStatus(0));
    assert!(device.published_phase.is_none());
    stream_pipeline::complete(&mut device, Completion::Receive);
    assert!(device.ready);
    assert!(device.published_phase.is_none());
    stream_pipeline::complete(&mut device, Completion::Receive);
    assert!(device.published_phase.is_none());
    stream_pipeline::complete(&mut device, Completion::Receive);
    assert_eq!(device.get(Field::Event), 0);
    assert_eq!(device.get(Field::Phase), 0);
    assert_eq!(device.published_phase, Some(7));
    assert!(!device.masked);
}

#[test]
fn failed_transmit_cancel_keeps_update_error_and_publishes_after_phase_reset() {
    let mut device = Device::new(1);
    device.set(Field::Phase, 3);
    device.program_status = ServiceStatus(-30);
    device.stop_status = ServiceStatus(-12);
    device.release_status = ServiceStatus(-20);
    assert_eq!(stream_pipeline::cancel(&mut device, stream_pipeline::EventHandle(19)), ServiceStatus(-20));
    assert_eq!(device.request[8], 4);
    assert_eq!(device.get(Field::Phase), 0);
    assert_eq!(device.published_phase, Some(0));
    assert!(!device.masked);
}
