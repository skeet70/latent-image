//! The date overlay token and its queue-33 publication: image-5 bodies
//! 0x00db_6464, 0x00db_6608, 0x00db_6628 and 0x00db_6688, and the empty
//! notification at image-6 0x0111_1c30.

use crate::osal::{delay_us, DelayTimer, Microseconds};
use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};
use crate::shutdown::{KernelStatus, QueueId, RequestId, ServiceMessage, ShutdownRtos};

/// The current token, the token the overlay last published and the code it
/// published under.
pub const TOKEN: u32 = 0x014e_32bc;
pub const PUBLISHED: u32 = 0x014e_32b8;
pub const PUBLISHED_CODE: u32 = 0x014e_32ba;
pub const OBSERVED_TOKEN: u32 = 0x014e_3338;
/// The overlay's code word.
pub const OVERLAY_CODE: u16 = 0x4174;
/// The queue the overlay publication goes to, and its code word.
pub const DATE_QUEUE: u8 = 33;
pub const DATE_CODE: u16 = 0x3100;

/// The image-5 body at 0x00db_6608.
pub fn helper_db6608<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    context.read_u8(GuestAddress::new(TOKEN)).map(u32::from)
}

/// The image-5 body at 0x00db_6628.
pub fn helper_db6628<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mode: u32) -> Result<u32, MemoryError> {
    match mode {
        1 => {
            let current = context.read_u8(GuestAddress::new(TOKEN))?;
            let next = if current == 0xff { 1 } else { current + 1 };
            context.write_u8(GuestAddress::new(TOKEN), next)?;
            Ok(u32::from(next))
        }
        2 => helper_db6608(context),
        _ => Ok(0),
    }
}

/// The image-5 body at 0x00db_6464.
pub fn helper_db6464<M: GuestAddressSpace, E: ShutdownRtos + DelayTimer>(context: &mut RecordContext<M, E>, payload: GuestAddress) -> Result<(), MemoryError> {
    let mut bytes = [0u8; 12];
    for offset in 0..12 {
        bytes[offset as usize] = context.read_u8(payload.checked_add(offset)?)?;
    }
    let sender = context.effects.current_task();
    let mut remaining = 10;
    loop {
        let result = context.effects.post(ServiceMessage { operation: RequestId(DATE_CODE), sender, destination: QueueId(DATE_QUEUE), payload: &bytes });
        if !matches!(result, Err(KernelStatus(-18)) | Err(KernelStatus(-62))) { break; }
        delay_us(&mut context.effects, Microseconds(100));
        remaining -= 1;
        if remaining == 0 { break; }
    }
    let code = u32::from(u16::from_le_bytes([bytes[0], bytes[1]]));
    helper_1111c30(context, code, 5)
}

/// The image-5 body at 0x00db_6688.
pub fn helper_db6688<M: GuestAddressSpace, E: ShutdownRtos + DelayTimer>(context: &mut RecordContext<M, E>, mode: u32) -> Result<(), MemoryError> {
    if mode == 1 {
        if context.read_u16(GuestAddress::new(PUBLISHED))? == OVERLAY_CODE {
            if context.read_u8(GuestAddress::new(TOKEN))? == context.read_u8(GuestAddress::new(OBSERVED_TOKEN))? { return Ok(()); }
        } else {
            context.write_u16(GuestAddress::new(PUBLISHED), OVERLAY_CODE)?;
        }
    } else {
        context.write_u16(GuestAddress::new(PUBLISHED), 0)?;
    }
    let value = helper_db6628(context, mode)?;
    context.with_guest_frame(ByteCount(12), StackAlignment::Word, |context, frame| {
        for offset in 0..12 {
            context.write_u8(frame.base().checked_add(offset)?, 0)?;
        }
        context.write_u16(frame.base(), OVERLAY_CODE)?;
        context.write_u8(frame.base().checked_add(2)?, value as u8)?;
        context.write_u16(GuestAddress::new(PUBLISHED_CODE), OVERLAY_CODE)?;
        helper_db6464(context, frame.base())
    })
}

/// The image-6 body at 0x0111_1c30.
pub fn helper_1111c30<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, _code: u32, _status: u32) -> Result<(), MemoryError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00db_6464-0x00db_6688.
    use super::*;
    use crate::osal::TimerReload;
    use crate::record_address_space::{GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const STACK: u32 = 0x0010_0000;
    const PAYLOAD: u32 = 0x0010_1000;

    #[derive(Default)]
    struct Kernel {
        statuses: [i32; 12],
        sent: u32,
        sends: [[u8; 12]; 12],
        delays: u32,
        tasks: u32,
    }

    impl ShutdownRtos for Kernel {
        fn current_task(&mut self) -> crate::shutdown::TaskId { self.tasks += 1; crate::shutdown::TaskId(41) }
        fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
            let index = self.sent as usize;
            self.sends[index][..message.payload.len().min(12)].copy_from_slice(&message.payload[..message.payload.len().min(12)]);
            self.sends[index][2] = message.sender.0;
            self.sends[index][3] = message.destination.0;
            self.sent += 1;
            let status = self.statuses[index];
            if status == 0 { Ok(()) } else { Err(KernelStatus(status)) }
        }
        fn set_event_flag(&mut self, _flag: u32, _bits: u32) -> Result<(), KernelStatus> { Ok(()) }
    }

    impl DelayTimer for Kernel {
        fn reload(&mut self) -> u32 { self.delays += 1; 1 }
        fn current(&mut self) -> u32 { 0 }
    }

    struct Memory {
        globals: [u8; 0x100],
        payload: [u8; 12],
        stack: [u8; 0x100],
    }

    fn with_context<R>(memory: &mut Memory, kernel: &mut Kernel, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, Kernel>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(0x014e_32b8, 0x100), &mut memory.globals).unwrap(),
            MemoryRegion::new(descriptor(PAYLOAD, 12), &mut memory.payload).unwrap(),
            MemoryRegion::new(descriptor(STACK, 0x100), &mut memory.stack).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let stack = GuestStack::new(GuestAddress::new(STACK), ByteCount(0x100)).unwrap();
        let mut context = RecordContext::with_stack(space, core::mem::take(kernel), stack);
        let result = body(&mut context);
        *kernel = core::mem::take(&mut context.effects);
        result
    }

    const GLOBALS: u32 = 0x014e_32b8;

    fn at(address: u32) -> usize { (address - GLOBALS) as usize }

    #[test]
    fn the_token_advance_wraps_from_ff_to_one() {
        let mut memory = Memory { globals: [0; 0x100], payload: [0; 12], stack: [0; 0x100] };
        let mut kernel = Kernel::default();
        memory.globals[at(TOKEN)] = 0x40;
        with_context(&mut memory, &mut kernel, |context| assert_eq!(helper_db6628(context, 1), Ok(0x41)));
        assert_eq!(memory.globals[at(TOKEN)], 0x41);
        memory.globals[at(TOKEN)] = 0xff;
        with_context(&mut memory, &mut kernel, |context| assert_eq!(helper_db6628(context, 1), Ok(1)));
        assert_eq!(memory.globals[at(TOKEN)], 1);
        memory.globals[at(TOKEN)] = 7;
        with_context(&mut memory, &mut kernel, |context| assert_eq!(helper_db6628(context, 2), Ok(7)));
        with_context(&mut memory, &mut kernel, |context| assert_eq!(helper_db6628(context, 0), Ok(0)));
        with_context(&mut memory, &mut kernel, |context| assert_eq!(helper_db6608(context), Ok(7)));
    }

    #[test]
    fn the_publication_posts_one_packet_and_keeps_the_payload() {
        let mut memory = Memory { globals: [0; 0x100], payload: [0; 12], stack: [0; 0x100] };
        let mut kernel = Kernel::default();
        memory.payload[..2].copy_from_slice(&OVERLAY_CODE.to_le_bytes());
        with_context(&mut memory, &mut kernel, |context| assert_eq!(helper_db6464(context, GuestAddress::new(PAYLOAD)), Ok(())));
        assert_eq!(kernel.sent, 1);
        assert_eq!(&kernel.sends[0][..2], &OVERLAY_CODE.to_le_bytes());
        assert_eq!(kernel.sends[0][2], 41);
        assert_eq!(kernel.sends[0][3], 33);
        assert_eq!(kernel.tasks, 1);
        assert_eq!(kernel.delays, 0);
    }

    #[test]
    fn the_publication_retries_ten_times_on_the_two_queue_errors() {
        let mut memory = Memory { globals: [0; 0x100], payload: [0; 12], stack: [0; 0x100] };
        let mut kernel = Kernel { statuses: [-18; 12], ..Kernel::default() };
        with_context(&mut memory, &mut kernel, |context| assert_eq!(helper_db6464(context, GuestAddress::new(PAYLOAD)), Ok(())));
        assert_eq!(kernel.sent, 10);
        assert!(kernel.delays >= 10);
        let mut kernel = Kernel { statuses: [-62, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], ..Kernel::default() };
        with_context(&mut memory, &mut kernel, |context| assert_eq!(helper_db6464(context, GuestAddress::new(PAYLOAD)), Ok(())));
        assert_eq!(kernel.sent, 2);
        assert!(kernel.delays >= 1);
        let mut kernel = Kernel { statuses: [-1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], ..Kernel::default() };
        with_context(&mut memory, &mut kernel, |context| assert_eq!(helper_db6464(context, GuestAddress::new(PAYLOAD)), Ok(())));
        assert_eq!(kernel.sent, 1);
        assert_eq!(kernel.delays, 0);
    }

    #[test]
    fn the_mode_two_publication_sends_the_observed_token_and_marks_the_code() {
        let mut memory = Memory { globals: [0; 0x100], payload: [0; 12], stack: [0; 0x100] };
        let mut kernel = Kernel::default();
        memory.globals[at(TOKEN)] = 0x22;
        with_context(&mut memory, &mut kernel, |context| assert_eq!(helper_db6688(context, 2), Ok(())));
        assert_eq!(kernel.sent, 1);
        assert_eq!(&memory.globals[at(PUBLISHED_CODE)..at(PUBLISHED_CODE) + 2], &OVERLAY_CODE.to_le_bytes());
        assert_eq!(memory.globals[at(TOKEN)], 0x22);
        assert_eq!(&memory.globals[at(PUBLISHED)..at(PUBLISHED) + 2], &0u16.to_le_bytes());
    }

    #[test]
    fn mode_one_publishes_once_per_new_token() {
        let mut memory = Memory { globals: [0; 0x100], payload: [0; 12], stack: [0; 0x100] };
        let mut kernel = Kernel::default();
        memory.globals[at(TOKEN)] = 0x10;
        memory.globals[at(OBSERVED_TOKEN)] = 0x10;
        with_context(&mut memory, &mut kernel, |context| assert_eq!(helper_db6688(context, 1), Ok(())));
        assert_eq!(kernel.sent, 1);
        assert_eq!(memory.globals[at(TOKEN)], 0x11);
        assert_eq!(&memory.globals[at(PUBLISHED)..at(PUBLISHED) + 2], &OVERLAY_CODE.to_le_bytes());
        memory.globals[at(PUBLISHED)..at(PUBLISHED) + 2].copy_from_slice(&OVERLAY_CODE.to_le_bytes());
        with_context(&mut memory, &mut kernel, |context| assert_eq!(helper_db6688(context, 1), Ok(())));
        assert_eq!(kernel.sent, 2);
        memory.globals[at(OBSERVED_TOKEN)] = memory.globals[at(TOKEN)];
        with_context(&mut memory, &mut kernel, |context| assert_eq!(helper_db6688(context, 1), Ok(())));
        assert_eq!(kernel.sent, 2);
    }

    #[test]
    fn the_notification_has_no_effect() {
        let mut memory = Memory { globals: [0; 0x100], payload: [0; 12], stack: [0; 0x100] };
        let mut kernel = Kernel::default();
        with_context(&mut memory, &mut kernel, |context| assert_eq!(helper_1111c30(context, 0x4174, 5), Ok(())));
        assert_eq!(kernel.sent, 0);
    }
}
