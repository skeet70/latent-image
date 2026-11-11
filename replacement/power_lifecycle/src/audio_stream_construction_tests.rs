use super::*;
use std::collections::BTreeMap;

#[derive(Default)]
struct Image { bytes: BTreeMap<Address, u8>, auto_pending: bool }
impl ConstructionMemory for Image {
    fn word(&self, address: Address) -> u32 {
        if self.auto_pending && address == Address(0xfff02064) && self.word(Address(0xfff02068)) & 1 != 0 { return 4; }
        u32::from_le_bytes(core::array::from_fn(|index| self.byte(address.offset(index as u32))))
    }
    fn put(&mut self, address: Address, value: u32) { for (index, byte) in value.to_le_bytes().into_iter().enumerate() { self.put_byte(address.offset(index as u32), byte); } }
    fn byte(&self, address: Address) -> u8 { self.bytes.get(&address).copied().unwrap_or(0) }
    fn put_byte(&mut self, address: Address, value: u8) { self.bytes.insert(address, value); }
}
struct Rtos { masked: bool, wait_status: ServiceStatus, waited: Option<SemaphoreHandle> }
impl Rtos { fn new() -> Self { Self { masked: false, wait_status: ServiceStatus(0), waited: None } } }
impl ConstructionIo for Rtos {
    type Guard = bool;
    fn enter(&mut self) -> bool { let previous = self.masked; self.masked = true; previous }
    fn leave(&mut self, guard: bool) { self.masked = guard; }
    fn wait(&mut self, semaphore: SemaphoreHandle) -> ServiceStatus { self.waited = Some(semaphore); self.wait_status }
}

#[test]
fn controller_generation_wrap_skips_zero_and_the_other_live_handle() {
    let mut image = Image::default();
    let mut rtos = Rtos::new();
    image.put(Address(0x409c2804), u32::MAX);
    image.put(Address(0x942d58), 1);
    assert_eq!(controller_acquire(&mut image, &mut rtos, Bank::First, Address(0x8000)), ServiceStatus(0));
    assert_eq!(image.word(Address(0x8000)), 2);
    assert_eq!(image.word(Address(0x942d54)), 2);
    assert_eq!(image.word(Address(0x942d58)), 1);
    assert!(!rtos.masked);
}

#[test]
fn invalid_configuration_keeps_the_acquired_controller_and_other_valid_fields() {
    let mut image = Image::default();
    let mut rtos = Rtos::new();
    image.put(Address(0x9566bc), 0x8000);
    image.put(Address(0x8010), 3);
    image.put(Address(0x9000), 2);
    image.put(Address(0x900c), 0);
    image.put(Address(0x9010), 1);
    image.put(Address(0x973b10), 77);
    assert_eq!(resource_acquire(&mut image, &mut rtos, Address(0x9000)), ServiceStatus(-30));
    assert_eq!(image.word(Address(0x942d58)), 1);
    assert_eq!(image.word(Address(0x973b0c)), 1);
    assert_eq!(image.word(Address(0x973adc)) & 2, 2);
    assert_eq!(image.word(Address(0x973b10)), 77);
    assert_eq!(image.word(Address(0x973b14)), 0);
    assert_eq!(image.word(Address(0x973b18)), 1);
    assert_eq!(image.word(Address(0x973b1c)), 0);
}

#[test]
fn semaphore_failure_keeps_the_event_handle_and_does_not_publish_output() {
    let mut image = Image::default();
    let mut rtos = Rtos::new();
    rtos.wait_status = ServiceStatus(-5);
    image.put(Address(0x7c86c4), 1);
    image.put(Address(0x8000), 0xdead);
    assert_eq!(pipeline_acquire(&mut image, &mut rtos, Address(0x9000), Address(0x8000)), ServiceStatus(-255));
    assert_eq!(image.word(Address(0x93b608)), 1);
    assert_eq!(image.word(Address(0x93b600)), 1);
    assert_eq!(image.word(Address(0x93b5fc)), 8);
    assert_eq!(image.word(Address(0x9020)), 2);
    assert_eq!(image.word(Address(0x8000)), 0xdead);
    assert_eq!(image.word(Address(0x918bdc)), 0);
}

#[test]
fn pipeline_acquisition_publishes_real_shared_generations_and_channel_ownership() {
    let mut image = Image::default();
    let mut rtos = Rtos::new();
    image.put(Address(0x7c86c4), 1);
    image.put(Address(0x93e720), 7);
    assert_eq!(pipeline_acquire(&mut image, &mut rtos, Address(0x9000), Address(0x8000)), ServiceStatus(0));
    assert_eq!(image.word(Address(0x8000)), 1);
    assert_eq!(image.word(Address(0x9014)), 2);
    assert_eq!(image.word(Address(0x918bdc)), 2);
    assert_eq!(image.word(Address(0x918b9c)), 1);
    assert_eq!(image.word(Address(0x918bbc)), 2);
    assert_eq!(image.word(Address(0x93b600)), 3);
    assert_eq!(image.word(Address(0x9020)), 0);
    assert_eq!(rtos.waited, Some(SemaphoreHandle(0x7000c)));
}

#[test]
fn resource_configuration_programs_the_selected_bank_and_retains_unrelated_bits() {
    let mut image = Image::default();
    image.put(Address(0x942d58), 9);
    image.put(Address(0x973b0c), 9);
    image.put(Address(0x973b14), 1);
    image.put(Address(0x973b18), 0);
    image.put(Address(0xfffa100c), u32::MAX);
    image.put(Address(0xfffa1008), u32::MAX);
    assert_eq!(resource_configure(&mut image), ServiceStatus(0));
    assert_eq!(image.word(Address(0xfffa100c)) & 0x10001, 1);
    assert_eq!(image.word(Address(0xfffa100c)) & 2, 2);
    assert_eq!(image.word(Address(0xfffa1008)) & 0x8003, 2);
    assert_eq!(image.word(Address(0xfffa1100)), 0);
    assert_eq!(image.word(Address(0x973ae0)) & 2, 2);
    assert_eq!(resource_release(&mut image), ServiceStatus(0));
    assert_eq!(image.word(Address(0x942d58)), 0);
    assert_eq!(image.word(Address(0x973adc)) & 2, 0);
}

use super::super::stream_installed::{CallbackAddress, InstalledIo, InstalledPipeline, IrqHandle, IrqSource};
use super::super::stream_pipeline::{self, Completion, EventHandle, Field, Pipeline, Submission};
use super::super::stream::{ByteLength, PcmAddress};
struct Physical { rtos: Rtos, attach_status: ServiceStatus, signaled: Option<SemaphoreHandle> }
impl Physical { fn new() -> Self { Self { rtos: Rtos::new(), attach_status: ServiceStatus(0), signaled: None } } }
impl ConstructionIo for Physical {
    type Guard = bool;
    fn enter(&mut self) -> bool { self.rtos.enter() }
    fn leave(&mut self, guard: bool) { self.rtos.leave(guard); }
    fn wait(&mut self, semaphore: SemaphoreHandle) -> ServiceStatus { self.rtos.wait(semaphore) }
}
impl InstalledIo for Physical {
    type IrqMask = bool;
    fn save_irq(&mut self) -> bool { self.rtos.enter() }
    fn restore_irq(&mut self, saved: bool) { self.rtos.leave(saved); }
    fn attach_irq(&mut self, memory: &mut impl ConstructionMemory, _source: IrqSource, _completion: Completion, output: Address) -> ServiceStatus {
        if self.attach_status.0 == 0 { memory.put(output, 77); }
        self.attach_status
    }
    fn release_irq(&mut self, _handle: IrqHandle) -> ServiceStatus { ServiceStatus(0) }
    fn clean_data_cache(&mut self) {}
    fn signal(&mut self, semaphore: SemaphoreHandle) -> ServiceStatus { self.signaled = Some(semaphore); ServiceStatus(0) }
    fn invoke(&mut self, _memory: &mut impl ConstructionMemory, _callback: CallbackAddress) {}
    fn schedule(&mut self, _memory: &mut impl ConstructionMemory, _node: super::super::executor::NodeAddress, _output: Address) -> ServiceStatus { ServiceStatus(0) }
}

#[test]
fn installed_receive_provider_programs_and_retires_the_actual_channel_image() {
    let mut image = Image::default();
    image.auto_pending = true;
    let mut physical = Physical::new();
    image.put(Address(0x7c86c4), 1);
    image.put(Address(0x9566bc), 0xa000);
    assert_eq!(resource_acquire(&mut image, &mut physical, Address(0x9000)), ServiceStatus(0));
    assert_eq!(resource_configure(&mut image), ServiceStatus(0));
    assert_eq!(pipeline_acquire(&mut image, &mut physical, Address(0x9000), Address(0x8000)), ServiceStatus(0));
    let event = EventHandle(image.word(Address(0x8000)));
    let mut pipeline = InstalledPipeline { memory: &mut image, io: &mut physical, channel_index_residue: 0 };
    assert_eq!(stream_pipeline::submit(&mut pipeline, event, PcmAddress(0x42000000), ByteLength(9600), Submission::First), ServiceStatus(0));
    assert_eq!(pipeline.memory.word(Address(0xfff02040)), 0xfffa0020);
    assert_eq!(pipeline.memory.word(Address(0xfff02044)), 0x42000000);
    assert_eq!(pipeline.memory.word(Address(0xfff02048)), 9600);
    assert_eq!(pipeline.memory.word(Address(0xfff0206c)), 0x50122011);
    assert_eq!(pipeline.memory.word(Address(0xfff02068)), 1);
    assert_eq!(pipeline.memory.word(Address(0xfffa0008)) & 0x30000, 0x10000);
    assert_eq!(pipeline.memory.word(Address(0x918bfc)), 77);
    assert_eq!(stream_pipeline::cancel(&mut pipeline, event), ServiceStatus(0));
    stream_pipeline::complete(&mut pipeline, Completion::Receive);
    assert_ne!(pipeline.get(Field::Event), 0);
    stream_pipeline::complete(&mut pipeline, Completion::Receive);
    assert_eq!(pipeline.get(Field::Event), 0);
    assert_eq!(pipeline.get(Field::Phase), 0);
    assert_eq!(pipeline.memory.word(Address(0x918bdc)), 0);
    assert_eq!(pipeline.memory.word(Address(0x918bfc)), 0);
    assert_eq!(pipeline.memory.word(Address(0xfffa0008)) & 0x30000, 0);
    assert_eq!(pipeline.io.signaled, Some(SemaphoreHandle(12)));
}

#[test]
fn installed_irq_failure_preserves_open_channel_without_starting_dma() {
    let mut image = Image::default();
    let mut physical = Physical::new();
    physical.attach_status = ServiceStatus(-20);
    image.put(Address(0x93b60c), 9);
    image.put(Address(0x918bdc), 9);
    image.put(Address(0x918b9c), 1);
    image.put(Address(0x93b624), 38);
    image.put(Address(0x93b628), 0x6e5298);
    let mut pipeline = InstalledPipeline { memory: &mut image, io: &mut physical, channel_index_residue: 0 };
    assert_eq!(pipeline.start_dma(), ServiceStatus(-20));
    assert_eq!(pipeline.memory.word(Address(0x918b9c)), 1);
    assert_eq!(pipeline.memory.word(Address(0xfff02068)), 0);
    assert_eq!(pipeline.memory.word(Address(0x918bdc)), 9);
}
