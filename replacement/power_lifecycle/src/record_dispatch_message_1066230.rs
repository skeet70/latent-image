//! Kernel message posts, image-4 entries 0x0076d8f8-0x0076d9a0 and image-5 entries 0x01066230-0x01066358.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::shutdown::{KernelStatus, QueueId, RequestId, ServiceMessage, ShutdownRtos, TaskId};

const SELECTION_QUEUE: u8 = 50;
const REQUEST_QUEUE: u8 = 55;
const GEOMETRY_OPERATION: u16 = 0x4703;
const GEOMETRY_PAYLOAD: u32 = 16;
const GEOMETRY_FAILURE: u32 = 0xffff_ff02;

fn sender<E: ShutdownRtos>(effects: &mut E) -> TaskId {
    let task = effects.current_task();
    TaskId(if task.0 == 0 { 0xff } else { task.0 })
}

fn post<E: ShutdownRtos>(effects: &mut E, operation: u16, destination: u8, payload: &[u8]) -> u32 {
    let sender = sender(effects);
    match effects.post(ServiceMessage { operation: RequestId(operation), sender, destination: QueueId(destination), payload }) {
        Ok(()) => 0,
        Err(KernelStatus(value)) => value as u32,
    }
}

pub fn helper_76d8f8<M: GuestAddressSpace, E: ShutdownRtos>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    post(&mut context.effects, 0x4200, SELECTION_QUEUE, &[]);
    crate::record_leaf_forwards_767724::helper_767724(context)?;
    Ok(())
}

pub fn helper_76d950<M: GuestAddressSpace, E: ShutdownRtos>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    post(&mut context.effects, 0x4201, SELECTION_QUEUE, &[]);
    Ok(())
}

pub fn helper_1066230<M: GuestAddressSpace, E: ShutdownRtos>(context: &mut RecordContext<M, E>, operation: u32, payload: &[u8]) -> Result<u32, MemoryError> {
    Ok(post(&mut context.effects, operation as u16, REQUEST_QUEUE, payload))
}

pub fn helper_106631c<M: GuestAddressSpace, E: ShutdownRtos>(context: &mut RecordContext<M, E>, source: GuestAddress, completion: u32) -> Result<u32, MemoryError> {
    let mut payload = [0u8; GEOMETRY_PAYLOAD as usize];
    payload[..4].copy_from_slice(&completion.to_le_bytes());
    for index in 0..3u32 {
        let word = context.read_u32(source.checked_add(index * 4)?)?;
        payload[4 + index as usize * 4..8 + index as usize * 4].copy_from_slice(&word.to_le_bytes());
    }
    Ok(if helper_1066230(context, u32::from(GEOMETRY_OPERATION), &payload)? != 0 { GEOMETRY_FAILURE } else { 0 })
}

#[cfg(target_arch = "arm")]
#[derive(Default)]
pub struct NativeQueue;

#[cfg(target_arch = "arm")]
impl ShutdownRtos for NativeQueue {
    fn current_task(&mut self) -> TaskId { crate::shutdown_rtos::NativeShutdownRtos.current_task() }
    fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> { crate::shutdown_rtos::NativeShutdownRtos.post(message) }
    fn set_event_flag(&mut self, flag: u32, bits: u32) -> Result<(), KernelStatus> { crate::shutdown_rtos::NativeShutdownRtos.set_event_flag(flag, bits) }
}

mod exports {
    use super::*;
    use crate::record_native_exports::stock_export;

    stock_export!(
        xm1_power_record_helper_76d8f8,
        helper_76d8f8_abi,
        NativeQueue,
        () -> (),
        (),
        (),
        crate::record_dispatch_message_1066230::helper_76d8f8,
        |_| (),
        bounds ShutdownRtos
    );
    stock_export!(
        xm1_power_record_helper_76d950,
        helper_76d950_abi,
        NativeQueue,
        () -> (),
        (),
        (),
        crate::record_dispatch_message_1066230::helper_76d950,
        |_| (),
        bounds ShutdownRtos
    );
    stock_export!(
        xm1_power_record_helper_106631c,
        helper_106631c_abi,
        NativeQueue,
        (source: u32, completion: u32) -> u32,
        (source: GuestAddress, completion: u32),
        (GuestAddress::new(source), completion),
        crate::record_dispatch_message_1066230::helper_106631c,
        |value| value,
        bounds ShutdownRtos
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};
    use std::vec::Vec;

    const STACK: u32 = 0x0010_0000;
    const SOURCE: u32 = 0x0080_0000;

    struct Queue {
        task: TaskId,
        posts: Vec<(RequestId, TaskId, QueueId, Vec<u8>)>,
        failure: Option<i32>,
    }

    impl Queue {
        fn new(task: u8) -> Self { Self { task: TaskId(task), posts: Vec::new(), failure: None } }
    }

    impl ShutdownRtos for Queue {
        fn current_task(&mut self) -> TaskId { self.task }
        fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
            self.posts.push((message.operation, message.sender, message.destination, message.payload.to_vec()));
            match self.failure { Some(value) => Err(KernelStatus(value)), None => Ok(()) }
        }
        fn set_event_flag(&mut self, _flag: u32, _bits: u32) -> Result<(), KernelStatus> { Ok(()) }
    }

    struct Memory {
        selection: [u8; 0x70],
        globals: [u8; 0x100],
        source: [u8; 12],
        stack: [u8; 0x100],
    }

    impl Memory {
        fn with_context<R>(&mut self, effects: Queue, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, Queue>) -> R) -> R {
            let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
            let mut regions = [
                MemoryRegion::new(descriptor(0x016b_7864, 0x70), &mut self.selection).unwrap(),
                MemoryRegion::new(descriptor(0x0094_2ab0, 0x100), &mut self.globals).unwrap(),
                MemoryRegion::new(descriptor(SOURCE, 12), &mut self.source).unwrap(),
                MemoryRegion::new(descriptor(STACK, 0x100), &mut self.stack).unwrap(),
            ];
            let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
            let stack = GuestStack::new(GuestAddress::new(STACK), ByteCount(0x100)).unwrap();
            body(&mut RecordContext::with_stack(space, effects, stack))
        }
    }

    fn memory() -> Memory { Memory { selection: [0; 0x70], globals: [0; 0x100], source: [0; 12], stack: [0; 0x100] } }

    #[test]
    fn the_selection_posts_use_queue_fifty_and_a_zero_length_payload() {
        let mut memory = memory();
        let mut effects = Queue::new(7);
        memory.with_context(effects, |context| {
            assert_eq!(helper_76d8f8(context), Ok(()));
            assert_eq!(helper_76d950(context), Ok(()));
            assert_eq!(context.effects.posts.len(), 2);
            assert_eq!(context.effects.posts[0], (RequestId(0x4200), TaskId(7), QueueId(50), Vec::new()));
            assert_eq!(context.effects.posts[1], (RequestId(0x4201), TaskId(7), QueueId(50), Vec::new()));
        });
        effects = Queue::new(0);
        memory.with_context(effects, |context| {
            assert_eq!(helper_76d950(context), Ok(()));
            assert_eq!(context.effects.posts[0].1, TaskId(0xff));
        });
    }

    #[test]
    fn the_geometry_request_posts_the_completion_then_the_three_source_words() {
        let mut memory = memory();
        memory.source[..4].copy_from_slice(&0x1111_1111u32.to_le_bytes());
        memory.source[4..8].copy_from_slice(&0x2222_2222u32.to_le_bytes());
        memory.source[8..].copy_from_slice(&0x3333_3333u32.to_le_bytes());
        let effects = Queue::new(9);
        memory.with_context(effects, |context| {
            assert_eq!(helper_106631c(context, GuestAddress::new(SOURCE), 0x0a0b_0c0d), Ok(0));
            assert_eq!(context.effects.posts.len(), 1);
            let post = &context.effects.posts[0];
            assert_eq!(post.0, RequestId(GEOMETRY_OPERATION));
            assert_eq!(post.2, QueueId(55));
            assert_eq!(post.3, std::vec![0x0d, 0x0c, 0x0b, 0x0a, 0x11, 0x11, 0x11, 0x11, 0x22, 0x22, 0x22, 0x22, 0x33, 0x33, 0x33, 0x33]);
        });
    }

    #[test]
    fn a_failed_geometry_post_returns_the_minus_two_fifty_four_status() {
        let mut memory = memory();
        let mut effects = Queue::new(9);
        effects.failure = Some(-1);
        memory.with_context(effects, |context| assert_eq!(helper_106631c(context, GuestAddress::new(SOURCE), 0), Ok(GEOMETRY_FAILURE)));
    }
}
