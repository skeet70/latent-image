use super::*;
use crate::configuration_audio_actors::packets::ServicePacketQueue;
use xm1_power_lifecycle::{audio_release::executor::{self, Action, DelayTicks, ExecutorDiagnostic, ExecutorMode, NodeAddress, OperationHandle, OperationNode, OperationSlot, SlotState}, audio_release::stream_pipeline::{Bank, RequestField}};
use xm1_power_services::primitives::ServiceStatus;

struct Memory { mode: ExecutorMode, slots: [OperationSlot; 10], generations: [OperationHandle; 10], counter: OperationHandle }
impl Memory {
    fn new() -> Self { Self { mode: ExecutorMode::Idle, slots: [OperationSlot { state: SlotState::Free, remaining: DelayTicks(0), node: NodeAddress(0) }; 10], generations: [OperationHandle(0); 10], counter: OperationHandle(0) } }
}
impl ExecutorMemory for Memory {
    fn mode(&self) -> ExecutorMode { self.mode }
    fn set_mode(&mut self, mode: ExecutorMode) { self.mode = mode; }
    fn slot(&self, index: usize) -> OperationSlot { self.slots[index] }
    fn set_slot(&mut self, index: usize, slot: OperationSlot) { self.slots[index] = slot; }
    fn generation(&self, index: usize) -> OperationHandle { self.generations[index] }
    fn set_generation(&mut self, index: usize, handle: OperationHandle) { self.generations[index] = handle; }
    fn generation_counter(&self) -> OperationHandle { self.counter }
    fn set_generation_counter(&mut self, handle: OperationHandle) { self.counter = handle; }
    fn node(&self, _: NodeAddress) -> OperationNode { OperationNode { delay: DelayTicks(1), action: Action::Noop, next: None } }
}
struct Io;
impl ExecutorIo for Io {
    type SavedIrq = ();
    fn save_irq(&mut self) {}
    fn restore_irq(&mut self, _: ()) {}
    fn start_tick_timer(&mut self) -> ServiceStatus { ServiceStatus(0) }
    fn stop_tick_timer(&mut self) -> ServiceStatus { ServiceStatus(0) }
    fn diagnostic(&mut self, _: ExecutorDiagnostic) {}
    fn invoke(&mut self, _: Action) -> Result<(), Action> { Ok(()) }
}
struct Device { fields: [u32; 24], transmit: bool, enabled: bool, destination: Option<PcmAddress> }
impl Device {
    fn new(transmit: bool) -> Self { let mut fields = [0; 24]; fields[Field::Event as usize / 4] = 1; fields[Field::Phase as usize / 4] = 1; Self { fields, transmit, enabled: false, destination: None } }
}
impl Pipeline for Device {
    fn get(&self, field: Field) -> u32 { self.fields[field as usize / 4] }
    fn set(&mut self, field: Field, value: u32) { self.fields[field as usize / 4] = value; }
    fn request(&self, field: RequestField) -> u32 { match field { RequestField::Direction => u32::from(self.transmit), _ => 0 } }
    fn set_error(&mut self, _: u32) {}
    fn selected_bank(&self) -> Bank { Bank::First }
    fn channel_index(&mut self) -> u32 { 1 }
    fn program(&mut self) -> ServiceStatus { self.destination = Some(PcmAddress(self.get(Field::DmaDestination))); ServiceStatus(0) }
    fn start_dma(&mut self) -> ServiceStatus { ServiceStatus(0) }
    fn dma_pending(&mut self) -> bool { true }
    fn close_dma(&mut self) -> ServiceStatus { ServiceStatus(0) }
    fn start_resource(&mut self) -> ServiceStatus { self.enabled = true; ServiceStatus(0) }
    fn ready(&mut self) {}
    fn schedule_ready(&mut self) {}
    fn fill(&mut self, _: PcmAddress, _: ByteLength) {}
}
impl PipelineLifecycle for Device {
    type Guard = ();
    fn stop(&mut self) -> ServiceStatus { self.enabled = false; ServiceStatus(0) }
    fn release(&mut self) -> ServiceStatus { ServiceStatus(0) }
    fn finished(&mut self) {}
    fn enter(&mut self) {}
    fn leave(&mut self, _: ()) {}
}
fn timeout() -> Timeout { Timeout::new(100).unwrap() }

#[test]
fn capture_requires_codec_ticks_stream_completion_dma_drain_and_codec_stop() {
    let (mut runtime, mut memory, mut io, mut worker) = (VendorAudioRuntime::new(), Memory::new(), Io, crate::audio_zoom_actor::InstalledWorker { memory: Image::default() });
    let (mut device, mut ring) = (Device::new(false), ServicePacketQueue::new());
    let mut bytes = [0xff; 4];
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    let key = ConfigurationKey::from_abi(2);
    runtime.begin(Direction::Capture, RecordingSeconds::parse(1).unwrap(), PcmAddress(0x42e40010), key, &mut memory, &mut io, &mut worker, Tick(0), timeout()).unwrap();
    assert_eq!(runtime.poll(&mut memory, &mut io, &mut worker, &mut device, &mut ring, &mut configuration, Tick(1), timeout()), Ok(Progress::Waiting));
    assert!(!device.enabled);
    executor::tick(&mut memory, &mut io).unwrap();
    runtime.poll(&mut memory, &mut io, &mut worker, &mut device, &mut ring, &mut configuration, Tick(1), timeout()).unwrap();
    assert_eq!(worker.memory.word(Address(0x8faef0)), 3);
    executor::tick(&mut memory, &mut io).unwrap();
    runtime.poll(&mut memory, &mut io, &mut worker, &mut device, &mut ring, &mut configuration, Tick(2), timeout()).unwrap();
    assert_eq!(worker.memory.word(Address(0x8faef0)), 5);
    assert!(device.enabled);
    assert_eq!(device.destination, Some(PcmAddress(0x42e40010)));
    assert_eq!(configuration.byte(2), Some(0xff));
    while ring.release_front().is_some() {}
    runtime.event(StreamEvent::Empty, &mut device, &mut ring).unwrap();
    assert!(runtime.ready_to_stop());
    assert_eq!(runtime.poll(&mut memory, &mut io, &mut worker, &mut device, &mut ring, &mut configuration, Tick(3), timeout()), Ok(Progress::Waiting));
    assert_eq!(configuration.byte(2), Some(0xff));
    stream_pipeline::complete(&mut device, stream_pipeline::Completion::Receive);
    assert!(device.enabled);
    stream_pipeline::complete(&mut device, stream_pipeline::Completion::Receive);
    assert!(!device.enabled);
    runtime.event(StreamEvent::Finished, &mut device, &mut ring).unwrap();
    runtime.transfer_finished(&mut memory, &mut io, &mut worker, Tick(3), timeout()).unwrap();
    assert_eq!(runtime.poll(&mut memory, &mut io, &mut worker, &mut device, &mut ring, &mut configuration, Tick(3), timeout()), Ok(Progress::Waiting));
    executor::tick(&mut memory, &mut io).unwrap();
    assert_eq!(runtime.poll(&mut memory, &mut io, &mut worker, &mut device, &mut ring, &mut configuration, Tick(4), timeout()), Ok(Progress::Waiting));
    assert_eq!(worker.memory.word(Address(0x8faef0)), 10);
    executor::tick(&mut memory, &mut io).unwrap();
    assert_eq!(runtime.poll(&mut memory, &mut io, &mut worker, &mut device, &mut ring, &mut configuration, Tick(5), timeout()), Ok(Progress::Complete(ActionCompletion::service_success())));
    assert_eq!(configuration.byte(2), Some(0));
    assert!(!runtime.busy());
    assert_eq!(worker.memory.word(Address(0x8faef0)), 8);
    assert_eq!(worker.memory.word(Address(0x8fc758)), 0);
    assert_eq!(worker.memory.word(Address(0x8fc760)), 4);
    assert_eq!(worker.memory.word(Address(0x8fc764)), 2);
}

#[test]
fn stream_publications_reject_other_slots_commands_and_short_payloads() {
    let slot = StreamSlot::from_abi(3).unwrap();
    let mut message = [0; 16];
    message[..2].copy_from_slice(&0x2e07u16.to_le_bytes());
    message[2] = 3;
    assert_eq!(decode_stream_event(slot, &message), Some(StreamEvent::Finished));
    assert_eq!(decode_stream_event(slot, &message[..15]), None);
    message[2] = 4;
    assert_eq!(decode_stream_event(slot, &message), None);
    message[2] = 3;
    message[0] = 8;
    assert_eq!(decode_stream_event(slot, &message), None);
}

#[test]
fn playback_uses_owned_recording_and_leaves_the_other_channel_silent() {
    let mut recording = [0; PCM_BYTES_PER_SECOND];
    recording[..8].copy_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]);
    let mut output = [0xff; PCM_BYTES_PER_SECOND];
    let playback = prepare_playback(2, RecordingSeconds::parse(1).unwrap(), &recording, &mut output).unwrap();
    assert_eq!(&playback[..8], &[0, 0, 0x33, 0x44, 0, 0, 0x77, 0x88]);
    assert_eq!(&recording[..8], &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]);
}

#[test]
fn paired_sound_end_continues_the_existing_stream_before_physical_drain() {
    use crate::configuration_audio_actors::{packets::{PcmAddress as AssetAddress, PcmByteLength}, sound::{SoundClip, SoundLevel}};
    let (mut runtime, mut memory, mut io, mut worker) = (VendorAudioRuntime::new(), Memory::new(), Io, crate::audio_zoom_actor::InstalledWorker { memory: Image::default() });
    let (mut device, mut ring) = (Device::new(true), ServicePacketQueue::new());
    let mut bytes = [0xaa; 4];
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    let begin = SoundPlayback { mode: SoundMode::Track, level: SoundLevel::from_sample(0),
        clip: SoundClip::from_asset(AssetAddress::from_abi(0x1000), PcmByteLength::from_abi(200)), configure: true, stop_after: false };
    runtime.begin_sound(begin, &mut memory, &mut io, &mut worker, Tick(0), timeout()).unwrap();
    executor::tick(&mut memory, &mut io).unwrap();
    runtime.poll(&mut memory, &mut io, &mut worker, &mut device, &mut ring, &mut configuration, Tick(1), timeout()).unwrap();
    executor::tick(&mut memory, &mut io).unwrap();
    runtime.poll(&mut memory, &mut io, &mut worker, &mut device, &mut ring, &mut configuration, Tick(2), timeout()).unwrap();
    assert_eq!(ring.release_front().unwrap().length().get(), 200);
    runtime.event(StreamEvent::Empty, &mut device, &mut ring).unwrap();
    assert_eq!(runtime.poll(&mut memory, &mut io, &mut worker, &mut device, &mut ring, &mut configuration, Tick(3), timeout()), Ok(Progress::SegmentStopped));
    assert!(device.enabled);
    let end = SoundPlayback { clip: SoundClip::from_asset(AssetAddress::from_abi(0x2000), PcmByteLength::from_abi(100)), configure: false, stop_after: true, ..begin };
    runtime.continue_sound(end, &mut device, &mut ring).unwrap();
    assert_eq!(ring.release_front().unwrap().position().get(), 0x2000);
    runtime.event(StreamEvent::Empty, &mut device, &mut ring).unwrap();
    runtime.poll(&mut memory, &mut io, &mut worker, &mut device, &mut ring, &mut configuration, Tick(4), timeout()).unwrap();
    stream_pipeline::complete(&mut device, stream_pipeline::Completion::Send);
    stream_pipeline::complete(&mut device, stream_pipeline::Completion::Send);
    assert!(!device.enabled);
    runtime.event(StreamEvent::Finished, &mut device, &mut ring).unwrap();
    runtime.transfer_finished(&mut memory, &mut io, &mut worker, Tick(5), timeout()).unwrap();
    executor::tick(&mut memory, &mut io).unwrap();
    assert_eq!(runtime.poll(&mut memory, &mut io, &mut worker, &mut device, &mut ring, &mut configuration, Tick(6), timeout()), Ok(Progress::Complete(ActionCompletion::service_success())));
    assert_eq!(configuration.byte(2), Some(0xaa));
}

#[derive(Default)]
struct Image(std::collections::BTreeMap<u32, u8>);
impl ConstructionMemory for Image {
    fn word(&self, address: Address) -> u32 { u32::from_le_bytes(core::array::from_fn(|index| self.byte(address.offset(index as u32)))) }
    fn put(&mut self, address: Address, value: u32) { for (index, byte) in value.to_le_bytes().into_iter().enumerate() { self.put_byte(address.offset(index as u32), byte); } }
    fn byte(&self, address: Address) -> u8 { self.0.get(&address.0).copied().unwrap_or(0) }
    fn put_byte(&mut self, address: Address, value: u8) { self.0.insert(address.0, value); }
}
struct Semaphore { available: bool }
impl ConstructionIo for Semaphore {
    type Guard = ();
    fn enter(&mut self) {}
    fn leave(&mut self, _: ()) {}
    fn wait(&mut self, _: stream_construction::SemaphoreHandle) -> ServiceStatus {
        if self.available { self.available = false; ServiceStatus(0) } else { ServiceStatus(-255) }
    }
}

#[test]
fn host_preparation_acquires_the_canonical_installed_controller_and_dma_tables() {
    let mut image = Image::default();
    image.put(Address(0x7c86bc + 8), 1);
    let mut semaphore = Semaphore { available: true };
    let slot = register_stream(&mut image).unwrap();
    let event = prepare_transfer(&mut image, &mut semaphore, Direction::Capture, None).unwrap();
    assert_ne!(event, EventHandle(0));
    assert_eq!(image.word(Address(0xfffa0008)) & 0x8003, 2);
    assert_eq!(image.word(Address(0xfffa0100)) & 3, 0);
    let request_handle = image.word(Address(0x956734));
    assert_ne!(request_handle, 0);
    assert_eq!(image.word(Address(0x918b98 + 64 + 4)), request_handle);
    assert_eq!(prepare_transfer(&mut image, &mut semaphore, Direction::Playback, None), Err(Error::Device(-20)));
    let mut message = [0; 16];
    message[..2].copy_from_slice(&0x2e06u16.to_le_bytes());
    message[2] = image.word(Address(0x9566c4)) as u8;
    assert_eq!(decode_stream_event(slot, &message), Some(StreamEvent::Empty));
}
