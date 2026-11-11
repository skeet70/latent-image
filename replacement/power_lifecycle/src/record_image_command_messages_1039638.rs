//! Image-5 image-command senders, bodies 0x0103_9638-0x0103_b0b8.
//!
//! Stock builds `{u16 operation, u8 sender, u8 queue, u32 length, u32 payload}`
//! and enters `dispatch_send_message` at 0x0072_503c.
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::shutdown::{QueueId, RequestId, ServiceMessage, ShutdownRtos};

const COMMAND_QUEUE: u8 = 0x30;
const ACCEPTED: u32 = 0;
const QUEUE_ERROR: u32 = 0x86;
const RESOURCE_WORD: u32 = 0x016f_ec34 + 0x4ec8;

fn required(source: GuestAddress) -> Option<GuestAddress> {
    if source.get() == 0 { None } else { Some(source) }
}

fn copy<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, payload: &mut [u8], at: usize, source: GuestAddress, length: u32) -> Result<(), MemoryError> {
    for index in 0..length { payload[at + index as usize] = context.read_u8(source.checked_add(index)?)?; }
    Ok(())
}

fn word(payload: &mut [u8], at: usize, value: u32) {
    payload[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn post<R: ShutdownRtos>(rtos: &mut R, operation: u16, payload: &[u8]) -> u32 {
    let message = ServiceMessage { operation: RequestId(operation), sender: rtos.current_task(), destination: QueueId(COMMAND_QUEUE), payload };
    match rtos.post(message) { Ok(()) => ACCEPTED, Err(_) => QUEUE_ERROR }
}

/// The image-5 body at 0x0103_9638.
pub fn helper_1039638<M: GuestAddressSpace, E, R: ShutdownRtos>(
    context: &mut RecordContext<M, E>, rtos: &mut R,
    descriptor: GuestAddress, dimensions: GuestAddress, extended: GuestAddress, mode: GuestAddress,
    snapshot: GuestAddress, channel: GuestAddress, callback: GuestAddress,
    enabled: u32, value: u32, completion: u32,
) -> Result<u32, MemoryError> {
    let (Some(descriptor), Some(dimensions), Some(extended), Some(mode), Some(snapshot), Some(channel)) = (
        required(descriptor), required(dimensions), required(extended), required(mode), required(snapshot), required(channel),
    ) else {
        return Ok(QUEUE_ERROR);
    };
    let mut payload = [0u8; 208];
    copy(context, &mut payload, 0, descriptor, 16)?;
    copy(context, &mut payload, 16, dimensions, 24)?;
    copy(context, &mut payload, 40, extended, 16)?;
    copy(context, &mut payload, 56, mode, 16)?;
    copy(context, &mut payload, 72, snapshot, 56)?;
    copy(context, &mut payload, 128, channel, 56)?;
    if let Some(callback) = required(callback) { copy(context, &mut payload, 184, callback, 12)?; }
    word(&mut payload, 196, completion);
    word(&mut payload, 200, enabled);
    word(&mut payload, 204, value);
    Ok(post(rtos, 0x4002, &payload))
}

/// The image-5 body at 0x0103_aff0.
pub fn helper_103aff0<M: GuestAddressSpace, E, R: ShutdownRtos>(
    _context: &mut RecordContext<M, E>, rtos: &mut R, record: u32, byte: u32, mode: u32,
) -> Result<u32, MemoryError> {
    let mut payload = [0u8; 12];
    if record != 0 {
        word(&mut payload, 0, record);
        payload[4] = byte as u8;
        word(&mut payload, 8, mode);
    }
    Ok(post(rtos, 0x4041, &payload))
}

/// The image-5 body at 0x0103_b064.
pub fn helper_103b064<M: GuestAddressSpace, E, R: ShutdownRtos>(
    context: &mut RecordContext<M, E>, rtos: &mut R,
) -> Result<u32, MemoryError> {
    if context.read_u32(GuestAddress::new(RESOURCE_WORD))? != 0 { return Ok(0); }
    Ok(post(rtos, 0x4043, &[]))
}

#[cfg(target_arch = "arm")]
mod native {
    use super::*;

    /// Runs `body` with the one native context and the installed RTOS.
    fn with_rtos<R>(body: impl FnOnce(&mut RecordContext<crate::record_address_space::NativeAddressSpace<'_>, ()>, &mut crate::shutdown_rtos::NativeShutdownRtos) -> Result<R, MemoryError>) -> R {
        unsafe { crate::record_native_boundary::with_native_context::<(), _>(|context| {
            let mut rtos = crate::shutdown_rtos::NativeShutdownRtos;
            match body(context, &mut rtos) {
                Ok(value) => value,
                Err(_) => crate::record_native_boundary::fault(),
            }
        }) }
    }

    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_record_helper_1039638(
        descriptor: u32, dimensions: u32, extended: u32, mode: u32, snapshot: u32, channel: u32, callback: u32,
        enabled: u32, value: u32, completion: u32,
    ) -> u32 {
        with_rtos(|context, rtos| helper_1039638(
            context, rtos,
            GuestAddress::new(descriptor), GuestAddress::new(dimensions), GuestAddress::new(extended), GuestAddress::new(mode),
            GuestAddress::new(snapshot), GuestAddress::new(channel), GuestAddress::new(callback),
            enabled, value, completion,
        ))
    }

    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_record_helper_103aff0(value: u32, byte: u32, mode: u32) {
        let _ = with_rtos(|context, rtos| helper_103aff0(context, rtos, value, byte, mode));
    }

    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_record_helper_103b064() {
        let _ = with_rtos(|context, rtos| helper_103b064(context, rtos));
    }
}

#[cfg(test)]
mod tests {
    //! Expected payloads are the image-5 stores at 0x0103_9638-0x0103_b0b8.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};
    use crate::shutdown::{KernelStatus, TaskId};

    const DESCRIPTOR: u32 = 0x0010_0000;
    const DIMENSIONS: u32 = 0x0010_1000;
    const EXTENDED: u32 = 0x0010_2000;
    const MODE: u32 = 0x0010_3000;
    const SNAPSHOT: u32 = 0x0010_4000;
    const CHANNEL: u32 = 0x0010_5000;
    const CALLBACK: u32 = 0x0010_6000;

    #[derive(Default)]
    struct Rtos { task: u8, messages: [Option<(u16, u8, u8, Vec<u8>)>; 4], count: usize, fail: bool }

    impl ShutdownRtos for Rtos {
        fn current_task(&mut self) -> TaskId { TaskId(self.task) }
        fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
            if self.fail { return Err(KernelStatus(-1)); }
            self.messages[self.count] = Some((message.operation.0, message.sender.0, message.destination.0, message.payload.to_vec()));
            self.count += 1;
            Ok(())
        }
        fn set_event_flag(&mut self, _flag: u32, _bits: u32) -> Result<(), KernelStatus> { Ok(()) }
    }

    struct Memory {
        descriptor: [u8; 16],
        dimensions: [u8; 24],
        extended: [u8; 16],
        mode: [u8; 16],
        snapshot: [u8; 56],
        channel: [u8; 56],
        callback: [u8; 12],
        resource: [u8; 4],
    }

    impl Memory {
        fn new() -> Self {
            let mut memory = Memory { descriptor: [0; 16], dimensions: [0; 24], extended: [0; 16], mode: [0; 16], snapshot: [0; 56], channel: [0; 56], callback: [0; 12], resource: [0; 4] };
            for index in 0..16 { memory.descriptor[index] = 0x10 + index as u8; memory.extended[index] = 0x20 + index as u8; memory.mode[index] = 0x30 + index as u8; }
            for index in 0..24 { memory.dimensions[index] = 0x40 + index as u8; }
            for index in 0..56 { memory.snapshot[index] = 0x50 + index as u8; memory.channel[index] = 0x60 + index as u8; }
            for index in 0..12 { memory.callback[index] = 0x70 + index as u8; }
            memory
        }

        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
            let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
            let mut regions = [
                MemoryRegion::new(descriptor(DESCRIPTOR, 16), &mut self.descriptor).unwrap(),
                MemoryRegion::new(descriptor(DIMENSIONS, 24), &mut self.dimensions).unwrap(),
                MemoryRegion::new(descriptor(EXTENDED, 16), &mut self.extended).unwrap(),
                MemoryRegion::new(descriptor(MODE, 16), &mut self.mode).unwrap(),
                MemoryRegion::new(descriptor(SNAPSHOT, 56), &mut self.snapshot).unwrap(),
                MemoryRegion::new(descriptor(CHANNEL, 56), &mut self.channel).unwrap(),
                MemoryRegion::new(descriptor(CALLBACK, 12), &mut self.callback).unwrap(),
                MemoryRegion::new(descriptor(RESOURCE_WORD, 4), &mut self.resource).unwrap(),
            ];
            let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
            body(&mut RecordContext::new(space, ()))
        }
    }

    fn addresses() -> (GuestAddress, GuestAddress, GuestAddress, GuestAddress, GuestAddress, GuestAddress, GuestAddress) {
        (
            GuestAddress::new(DESCRIPTOR), GuestAddress::new(DIMENSIONS), GuestAddress::new(EXTENDED), GuestAddress::new(MODE),
            GuestAddress::new(SNAPSHOT), GuestAddress::new(CHANNEL), GuestAddress::new(CALLBACK),
        )
    }

    #[test]
    fn the_ten_argument_sender_copies_every_record_into_its_payload() {
        let mut memory = Memory::new();
        let mut rtos = Rtos { task: 7, ..Rtos::default() };
        let (descriptor, dimensions, extended, mode, snapshot, channel, callback) = addresses();
        memory.with_context(|context| {
            assert_eq!(helper_1039638(context, &mut rtos, descriptor, dimensions, extended, mode, snapshot, channel, callback, 0x11, 0x22, 0x33), Ok(0));
        });
        let (operation, sender, destination, payload) = rtos.messages[0].clone().unwrap();
        assert_eq!(operation, 0x4002);
        assert_eq!(sender, 7);
        assert_eq!(destination, 0x30);
        assert_eq!(payload.len(), 208);
        assert_eq!(&payload[..16], &memory.descriptor[..]);
        assert_eq!(&payload[16..40], &memory.dimensions[..]);
        assert_eq!(&payload[40..56], &memory.extended[..]);
        assert_eq!(&payload[56..72], &memory.mode[..]);
        assert_eq!(&payload[72..128], &memory.snapshot[..]);
        assert_eq!(&payload[128..184], &memory.channel[..]);
        assert_eq!(&payload[184..196], &memory.callback[..]);
        assert_eq!(&payload[196..200], &0x33u32.to_le_bytes());
        assert_eq!(&payload[200..204], &0x11u32.to_le_bytes());
        assert_eq!(&payload[204..208], &0x22u32.to_le_bytes());
    }

    #[test]
    fn the_ten_argument_sender_rejects_a_null_record_before_it_posts() {
        let mut memory = Memory::new();
        let mut rtos = Rtos::default();
        let (descriptor, dimensions, extended, mode, snapshot, channel, callback) = addresses();
        memory.with_context(|context| {
            assert_eq!(helper_1039638(context, &mut rtos, descriptor, dimensions, extended, mode, snapshot, GuestAddress::new(0), callback, 0, 0, 0), Ok(QUEUE_ERROR));
        });
        assert_eq!(rtos.count, 0);
    }

    #[test]
    fn the_three_word_sender_posts_the_record_bytes() {
        let mut memory = Memory::new();
        let mut rtos = Rtos::default();
        memory.with_context(|context| assert_eq!(helper_103aff0(context, &mut rtos, 0x1122_3344, 0x1ff, 0x5566_7788), Ok(0)));
        let (operation, _, destination, payload) = rtos.messages[0].clone().unwrap();
        assert_eq!(operation, 0x4041);
        assert_eq!(destination, 0x30);
        assert_eq!(&payload[..4], &0x1122_3344u32.to_le_bytes());
        assert_eq!(payload[4], 0xff);
        assert_eq!(&payload[8..12], &0x5566_7788u32.to_le_bytes());
    }

    #[test]
    fn the_three_word_sender_posts_zeros_for_a_null_record() {
        let mut memory = Memory::new();
        let mut rtos = Rtos::default();
        memory.with_context(|context| assert_eq!(helper_103aff0(context, &mut rtos, 0, 0x55, 0x66), Ok(0)));
        let (_, _, _, payload) = rtos.messages[0].clone().unwrap();
        assert_eq!(payload, vec![0u8; 12]);
    }

    #[test]
    fn the_empty_sender_posts_only_while_the_resource_word_is_clear() {
        let mut memory = Memory::new();
        let mut rtos = Rtos::default();
        memory.with_context(|context| assert_eq!(helper_103b064(context, &mut rtos), Ok(0)));
        assert_eq!(rtos.count, 1);
        assert_eq!(rtos.messages[0].clone().unwrap().0, 0x4043);
        assert!(rtos.messages[0].clone().unwrap().3.is_empty());
        memory.resource.copy_from_slice(&1u32.to_le_bytes());
        memory.with_context(|context| assert_eq!(helper_103b064(context, &mut rtos), Ok(0)));
        assert_eq!(rtos.count, 1);
    }
}
