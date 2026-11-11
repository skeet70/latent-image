use super::*;
use crate::record_address_space::{ByteCount, CallbackHandler, CallbackKey, CallbackRegistry, GuestDomain, GuestStack, HostAddressSpace, MemoryError, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};
use crate::record_hardware_requests::{CaptureMessage, CaptureQueue, CaptureTask};
use crate::shutdown::KernelStatus;

const PAYLOAD: usize = 44;

struct Transport {
    command: [u16; 2],
    payload: [[u8; PAYLOAD]; 2],
    length: [u32; 2],
    submitted: usize,
}

impl Default for Transport {
    fn default() -> Self { Self { command: [0; 2], payload: [[0; PAYLOAD]; 2], length: [0; 2], submitted: 0 } }
}

impl<M: GuestAddressSpace> CaptureQueue<M> for Transport {
    fn current_task(&mut self, _memory: &mut M) -> Result<CaptureTask, MemoryError> { Ok(CaptureTask::new(1)) }
    fn submit(&mut self, memory: &mut M, message: CaptureMessage<'_>) -> KernelStatus {
        let index = self.submitted.min(1);
        self.command[index] = message.command.get();
        self.length[index] = message.length.0;
        for offset in 0..message.length.0 as usize {
            self.payload[index][offset] = memory.read_u8(message.payload.checked_add(offset as u32).unwrap()).unwrap();
        }
        self.submitted += 1;
        KernelStatus(0)
    }
}

impl Transport {
    fn word(&self, message: usize, offset: usize) -> u32 {
        let bytes = &self.payload[message][offset..offset + 4];
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }
    fn half(&self, message: usize, offset: usize) -> u16 {
        let bytes = &self.payload[message][offset..offset + 2];
        u16::from_le_bytes([bytes[0], bytes[1]])
    }
}

fn callback(registry: &mut CallbackRegistry<(), 1>) -> CallbackId {
    registry.register(CallbackKey::new(1).unwrap(), CallbackHandler::Status(|_, _| Ok(()))).unwrap()
}

#[test]
fn peak_submission_captures_the_input_before_the_caller_changes_it() {
    let mut input = [0u8; 36];
    let mut stack = [0u8; 256];
    let mut destination = [0u8; 4];
    let mut regions = [
        MemoryRegion::new(RegionDescriptor::new(GuestAddress::new(0x1000), ByteCount(36), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap(), &mut input).unwrap(),
        MemoryRegion::new(RegionDescriptor::new(GuestAddress::new(0x2000), ByteCount(256), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap(), &mut stack).unwrap(),
        MemoryRegion::new(RegionDescriptor::new(GuestAddress::new(0x016f_0aa4), ByteCount(4), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap(), &mut destination).unwrap(),
    ];
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
    let mut context = RecordContext::with_stack(memory, Transport::default(), GuestStack::new(GuestAddress::new(0x2000), ByteCount(256)).unwrap());
    let base = GuestAddress::new(0x1000);
    context.write_u16(base.checked_add(8).unwrap(), 640).unwrap();
    context.write_u16(base.checked_add(12).unwrap(), (-3i16) as u16).unwrap();
    context.write_u32(base.checked_add(16).unwrap(), 29).unwrap();
    context.write_u32(base.checked_add(32).unwrap(), 53).unwrap();
    let mut registry = CallbackRegistry::<(), 1>::new();
    helper_fea634(&mut context, callback(&mut registry), base).unwrap();
    context.write_u32(base.checked_add(16).unwrap(), 97).unwrap();
    context.write_u16(base.checked_add(8).unwrap(), 320).unwrap();
    let transport = &context.effects;
    assert_eq!(transport.command[0], 0x3f2e);
    assert_eq!(transport.length[0], 44);
    assert_eq!(transport.word(0, 0), 1);
    assert_eq!(transport.half(0, 12), 640);
    assert_eq!(transport.half(0, 16), (-3i16) as u16);
    assert_eq!(transport.word(0, 20), 29);
    assert_eq!(transport.word(0, 36), 53);
}

#[test]
fn a_second_submission_keeps_the_first_request_intact() {
    let mut stack = [0u8; 256];
    let mut destination = [0u8; 4];
    let mut regions = [
        MemoryRegion::new(RegionDescriptor::new(GuestAddress::new(0x2000), ByteCount(256), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap(), &mut stack).unwrap(),
        MemoryRegion::new(RegionDescriptor::new(GuestAddress::new(0x016f_0aa4), ByteCount(4), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap(), &mut destination).unwrap(),
    ];
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
    let mut context = RecordContext::with_stack(memory, Transport::default(), GuestStack::new(GuestAddress::new(0x2000), ByteCount(256)).unwrap());
    let mut registry = CallbackRegistry::<(), 1>::new();
    helper_fea468(&mut context, callback(&mut registry), 7, 11, 13).unwrap();
    helper_fea6ac(&mut context).unwrap();
    let transport = &context.effects;
    assert_eq!(transport.submitted, 2);
    assert_eq!(transport.command[0], 0x3f16);
    assert_eq!(transport.length[0], 16);
    assert_eq!(transport.word(0, 4), 7);
    assert_eq!(transport.word(0, 8), 11);
    assert_eq!(transport.word(0, 12), 13);
    assert_eq!(transport.command[1], 0x3f2f);
    assert_eq!(transport.length[1], 0);
}
