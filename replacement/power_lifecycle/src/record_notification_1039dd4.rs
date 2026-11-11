//! The 0x4007 record-image notification: image-5 bodies 0x0103_9dd4,
//! 0x00f2_32f4 and 0x00f2_3a64.

use crate::record_address_space::{GuestAddressSpace, MemoryError, RecordContext};
use crate::shutdown::{KernelStatus, QueueId, RequestId, ServiceMessage, ShutdownRtos};

/// The task 43 queue and operation the three bodies send.
pub const DESTINATION: u8 = 48;
pub const OPERATION: u16 = 0x4007;
/// The callback the two fixed senders name.
pub const CALLBACK: u32 = 0x00f2_27dc;

/// The image-5 body at 0x0103_9dd4.
pub fn helper_1039dd4<M: GuestAddressSpace, E: ShutdownRtos>(context: &mut RecordContext<M, E>, stop: u32, pending: u32, callback: u32) -> Result<u32, MemoryError> {
    let mut payload = [0u8; 12];
    payload[..4].copy_from_slice(&stop.to_le_bytes());
    payload[4..8].copy_from_slice(&pending.to_le_bytes());
    payload[8..].copy_from_slice(&callback.to_le_bytes());
    let sender = context.effects.current_task();
    let result = context.effects.post(ServiceMessage { operation: RequestId(OPERATION), sender, destination: QueueId(DESTINATION), payload: &payload });
    Ok(match result { Ok(()) => 0, Err(_) => 134 })
}

/// The image-5 body at 0x00f2_32f4.
pub fn helper_f232f4<M: GuestAddressSpace, E: ShutdownRtos>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    helper_1039dd4(context, 0, 1, CALLBACK).map(|_| ())
}

/// The image-5 body at 0x00f2_3a64.
pub fn helper_f23a64<M: GuestAddressSpace, E: ShutdownRtos>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    helper_1039dd4(context, 1, 1, CALLBACK).map(|_| ())
}

mod exports {
    use super::*;
    use crate::record_native_exports::stock_export;
    use crate::shutdown::{KernelStatus, ServiceMessage, ShutdownRtos, TaskId};

    /// The native RTOS the exports of this family post through.
    #[cfg(target_arch = "arm")]
    #[derive(Default)]
    pub struct NativeNotificationRtos;

    #[cfg(target_arch = "arm")]
    impl ShutdownRtos for NativeNotificationRtos {
        fn current_task(&mut self) -> TaskId { crate::shutdown_rtos::NativeShutdownRtos.current_task() }
        fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> { crate::shutdown_rtos::NativeShutdownRtos.post(message) }
        fn set_event_flag(&mut self, flag: u32, bits: u32) -> Result<(), KernelStatus> { crate::shutdown_rtos::NativeShutdownRtos.set_event_flag(flag, bits) }
    }

    stock_export!(
        xm1_power_record_helper_f232f4, helper_f232f4_abi, NativeNotificationRtos,
        () -> (),
        (),
        (),
        crate::record_notification_1039dd4::helper_f232f4,
        |_| (),
        bounds ShutdownRtos
    );
    stock_export!(
        xm1_power_record_helper_f23a64, helper_f23a64_abi, NativeNotificationRtos,
        () -> (),
        (),
        (),
        crate::record_notification_1039dd4::helper_f23a64,
        |_| (),
        bounds ShutdownRtos
    );
    stock_export!(
        xm1_power_record_helper_1039dd4, helper_1039dd4_abi, NativeNotificationRtos,
        (stop: u32, pending: u32, callback: u32) -> u32,
        (stop: u32, pending: u32, callback: u32),
        (stop, pending, callback),
        crate::record_notification_1039dd4::helper_1039dd4,
        |value| value,
        bounds ShutdownRtos
    );
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00f2_32f4-0x0103_9dd4.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestAddress, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};
    use crate::shutdown::TaskId;

    #[derive(Default)]
    struct Queue {
        status: i32,
        operations: [u16; 4],
        senders: [u8; 4],
        destinations: [u8; 4],
        payloads: [[u8; 12]; 4],
        sent: u32,
    }

    impl ShutdownRtos for Queue {
        fn current_task(&mut self) -> TaskId { TaskId(43) }
        fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
            let index = self.sent as usize;
            self.operations[index] = message.operation.0;
            self.senders[index] = message.sender.0;
            self.destinations[index] = message.destination.0;
            self.payloads[index][..message.payload.len()].copy_from_slice(message.payload);
            self.sent += 1;
            if self.status == 0 { Ok(()) } else { Err(KernelStatus(self.status)) }
        }
        fn set_event_flag(&mut self, _flag: u32, _bits: u32) -> Result<(), KernelStatus> { Ok(()) }
    }

    fn with_context<R>(queue: &mut Queue, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, Queue>) -> R) -> R {
        let mut region = [0u8; 4];
        let descriptor = RegionDescriptor::new(GuestAddress::new(0x0010_0000), ByteCount(4), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [MemoryRegion::new(descriptor, &mut region).unwrap()];
        let mut context = RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), core::mem::take(queue));
        let result = body(&mut context);
        *queue = core::mem::take(&mut context.effects);
        result
    }

    #[test]
    fn the_notification_carries_the_three_words() {
        let mut queue = Queue::default();
        with_context(&mut queue, |context| assert_eq!(helper_1039dd4(context, 7, 0, 0x1234_5678), Ok(0)));
        assert_eq!(queue.sent, 1);
        assert_eq!(queue.operations[0], OPERATION);
        assert_eq!(queue.senders[0], 43);
        assert_eq!(queue.destinations[0], DESTINATION);
        let mut expected = [0u8; 12];
        expected[..4].copy_from_slice(&7u32.to_le_bytes());
        expected[4..8].copy_from_slice(&0u32.to_le_bytes());
        expected[8..].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        assert_eq!(queue.payloads[0], expected);
    }

    #[test]
    fn a_queue_error_answers_one_hundred_thirty_four() {
        let mut queue = Queue { status: -18, ..Queue::default() };
        with_context(&mut queue, |context| assert_eq!(helper_1039dd4(context, 0, 0, 0), Ok(134)));
        assert_eq!(queue.sent, 1);
    }

    #[test]
    fn the_two_fixed_senders_pass_their_stop_and_pending_words() {
        let mut queue = Queue::default();
        with_context(&mut queue, |context| assert_eq!(helper_f232f4(context), Ok(())));
        with_context(&mut queue, |context| assert_eq!(helper_f23a64(context), Ok(())));
        assert_eq!(queue.sent, 2);
        assert_eq!(queue.payloads[0], [0, 0, 0, 0, 1, 0, 0, 0, 0xdc, 0x27, 0xf2, 0]);
        assert_eq!(queue.payloads[1], [1, 0, 0, 0, 1, 0, 0, 0, 0xdc, 0x27, 0xf2, 0]);
    }
}
