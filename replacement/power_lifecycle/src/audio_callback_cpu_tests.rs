use super::*;
use crate::audio_release::{AudioDevice, CodecStatus, CodecWord, SerialHandle};
use crate::audio_release::stream_construction::{Address, ConstructionIo, ConstructionMemory, SemaphoreHandle};
use crate::primitives::ServiceStatus;
use crate::shutdown::{RequestId, TaskId};
use std::collections::BTreeMap;

#[derive(Default)]
struct Memory { bytes: BTreeMap<u32, u8> }
impl ConstructionMemory for Memory {
    fn word(&self, address: Address) -> u32 {
        u32::from_le_bytes(core::array::from_fn(|index| self.byte(address.offset(index as u32))))
    }
    fn put(&mut self, address: Address, value: u32) {
        for (index, byte) in value.to_le_bytes().into_iter().enumerate() { self.put_byte(address.offset(index as u32), byte); }
    }
    fn byte(&self, address: Address) -> u8 { self.bytes.get(&address.0).copied().unwrap_or(0) }
    fn put_byte(&mut self, address: Address, value: u8) { self.bytes.insert(address.0, value); }
}
#[derive(Default)]
struct Device { unavailable: bool, words: std::vec::Vec<CodecWord>, published: Option<BankNotificationContext> }
impl AudioDevice for Device {
    fn shadow(&self, _: crate::audio_release::CodecRegister) -> CodecWord { panic!("callback CPU reads canonical memory") }
    fn store_shadow(&mut self, _: crate::audio_release::CodecRegister, _: CodecWord) { panic!("callback CPU writes canonical memory") }
    fn open(&mut self) -> Result<SerialHandle, ServiceStatus> { if self.unavailable { Err(ServiceStatus(-20)) } else { Ok(SerialHandle(1)) } }
    fn configure(&mut self, _: SerialHandle) -> ServiceStatus { ServiceStatus(0) }
    fn write(&mut self, _: SerialHandle, word: CodecWord) -> ServiceStatus { self.words.push(word); ServiceStatus(0) }
    fn status(&mut self, _: SerialHandle, status: &mut CodecStatus) -> ServiceStatus { status.receive = 1; status.transmit = 1; ServiceStatus(0) }
    fn stop(&mut self, _: SerialHandle) -> ServiceStatus { ServiceStatus(0) }
    fn close(&mut self, _: SerialHandle) -> ServiceStatus { ServiceStatus(0) }
    fn lock_codec(&mut self) -> ServiceStatus { ServiceStatus(0) }
    fn unlock_codec(&mut self) -> ServiceStatus { ServiceStatus(0) }
}
impl ConstructionIo for Device {
    type Guard = ();
    fn enter(&mut self) {}
    fn leave(&mut self, _: ()) {}
    fn wait(&mut self, _: SemaphoreHandle) -> ServiceStatus { ServiceStatus(0) }
}
impl PhysicalIo for Device {
    fn post(&mut self, _: &mut impl ConstructionMemory, _: RequestId) { panic!("unexpected notification") }
    fn panel(&mut self, _: &mut impl ConstructionMemory, _: PanelOperation) -> Result<(), CallbackAddress> { panic!("unexpected panel action") }
    fn acquire_resource(&mut self, _: &mut impl ConstructionMemory, _: ClockPort, _: Address) -> ServiceStatus { panic!("unexpected resource acquisition") }
    fn release_resource(&mut self, _: &mut impl ConstructionMemory, _: ResourceHandle) -> ServiceStatus { panic!("unexpected resource release") }
    fn callback(&mut self, _: &mut impl ConstructionMemory, _: CallbackAddress) -> Result<(), CallbackAddress> { panic!("unexpected callback") }
    fn signal(&mut self, _: &mut impl ConstructionMemory, _: TaskId, _: u32) { panic!("unexpected event flag") }
    fn publish_bank(&mut self, memory: &mut impl ConstructionMemory, publication: BankPublication) {
        let BankPublication::Explicit(context) = publication else { panic!("unexpected unchanged context"); };
        self.published = Some(context);
        memory.put(Address(0x8fc778), 8);
    }
}
fn cached(memory: &Memory, register: u32) -> u16 {
    let address = Address(0x82916c + register * 2);
    u16::from_le_bytes([memory.byte(address), memory.byte(address.offset(1))])
}
fn set_cache(memory: &mut Memory, register: u32, value: u16) {
    let address = Address(0x82916c + register * 2);
    for (index, byte) in value.to_le_bytes().into_iter().enumerate() { memory.put_byte(address.offset(index as u32), byte); }
}

#[test]
fn selected_format_preserves_upper_codec_bits_and_transmits_result() {
    let mut memory = Memory::default();
    memory.put(Address(0x8fc760), 2);
    set_cache(&mut memory, 6, 0xabff);
    let mut device = Device::default();
    assert_eq!(invoke(&mut memory, &mut device, AudioAction::SelectFormat), Ok(()));
    assert_eq!(cached(&memory, 6), 0xabf2);
    assert_eq!(device.words, [CodecWord(0xabf2)]);
}
#[test]
fn unavailable_codec_keeps_the_updated_canonical_cache() {
    let mut memory = Memory::default();
    memory.put(Address(0x96ca50), 0x10000);
    memory.put_byte(Address(0x10309), 0x5a);
    set_cache(&mut memory, 0x17, 0xc3ff);
    let mut device = Device { unavailable: true, ..Device::default() };
    assert_eq!(invoke(&mut memory, &mut device, AudioAction::Output0309), Ok(()));
    assert_eq!(cached(&memory, 0x17), 0xc35a);
    assert!(device.words.is_empty());
}
#[test]
fn bank_terminal_restores_phase_saved_by_the_publication_effect() {
    let mut memory = Memory::default();
    memory.put(Address(0x8fc774), 0x1234);
    memory.put(Address(0x8fc778), 4);
    memory.put(Address(0x8faef0), 13);
    let mut device = Device::default();
    assert_eq!(invoke(&mut memory, &mut device, AudioAction::BankComplete), Ok(()));
    assert_eq!(device.published, Some(BankNotificationContext(0x1234)));
    assert_eq!(memory.word(Address(0x8faef0)), 8);
}
