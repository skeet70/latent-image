//! Image-5 capture request 0x3f43, bodies 0x00fe_aaa4-0x00f2_3084.
use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};
use crate::record_hardware_requests::{CaptureCommand, CaptureQueue};
use crate::shutdown::KernelStatus;

/// The image-5 body at 0x00fe_aaa4.
pub fn helper_feaaa4<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, value: u32) -> Result<KernelStatus, MemoryError> {
    context.with_guest_frame(ByteCount(4), StackAlignment::Word, |context, frame| {
        context.write_u32(frame.base(), value)?;
        crate::record_hardware_requests::helper_fe9580(context, CaptureCommand::new(0x3f43), frame.base(), frame.length(), &[])
    })
}

/// The image-5 body at 0x00f2_3070.
pub fn helper_f23070<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>) -> Result<KernelStatus, MemoryError> {
    helper_feaaa4(context, 0x00f2_26cc)
}

#[cfg(test)]
mod tests {
    //! Expected command, length and payload are the image-5 instructions at 0x00fe_aaa4.
    use super::*;
    use crate::record_address_space::{GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};
    use crate::record_hardware_requests::{CaptureMessage, CaptureTask};

    #[derive(Default)]
    struct Transport { command: u16, length: u32, payload: [u8; 4], submitted: u32 }

    impl<M: GuestAddressSpace> CaptureQueue<M> for Transport {
        fn current_task(&mut self, _memory: &mut M) -> Result<CaptureTask, MemoryError> { Ok(CaptureTask::new(3)) }
        fn submit(&mut self, memory: &mut M, message: CaptureMessage<'_>) -> KernelStatus {
            self.command = message.command.get();
            self.length = message.length.0;
            for offset in 0..message.length.0 as usize {
                self.payload[offset] = memory.read_u8(message.payload.checked_add(offset as u32).unwrap()).unwrap();
            }
            self.submitted += 1;
            KernelStatus(0)
        }
    }

    #[test]
    fn the_request_sends_its_one_word_to_the_capture_queue() {
        let mut destination = [0u8; 4];
        let mut stack = [0u8; 0x40];
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(0x016f_0aa4, 4), &mut destination).unwrap(),
            MemoryRegion::new(descriptor(0x0010_0000, 0x40), &mut stack).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let guest_stack = GuestStack::new(GuestAddress::new(0x0010_0000), ByteCount(0x40)).unwrap();
        let mut context = RecordContext::with_stack(space, Transport::default(), guest_stack);
        assert_eq!(helper_f23070(&mut context), Ok(KernelStatus(0)));
        assert_eq!(context.effects.command, 0x3f43);
        assert_eq!(context.effects.length, 4);
        assert_eq!(&context.effects.payload[..], &0x00f2_26ccu32.to_le_bytes());
        assert_eq!(context.effects.submitted, 1);
    }
}
