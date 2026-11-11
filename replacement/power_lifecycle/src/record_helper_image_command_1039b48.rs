//! Image-5 image-command senders: `ShutdownRtos::post` is `dispatch_send_message`.
//!
//! The stock bodies leave payload bytes that no copy covers as their own frame
//! residue; here they stay zero.
use crate::callback_registry::CallbackWord;
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::shutdown::{QueueId, RequestId, ServiceMessage, ShutdownRtos, TaskId};

pub const COMMAND_QUEUE: u8 = 48;
pub const ACCEPTED: u32 = 0;
pub const QUEUE_ERROR: u32 = 0x86;
/// 0x0103_a694 rejects a null record before posting.
pub const NULL_RECORD: u32 = 0x84;

fn required(source: GuestAddress) -> Option<GuestAddress> {
    if source.get() == 0 { None } else { Some(source) }
}

trait Word { fn value(self) -> u32; }
impl Word for u32 { fn value(self) -> u32 { self } }
impl Word for usize { fn value(self) -> u32 { self as u32 } }

fn word(payload: &mut [u8], at: usize, value: impl Word) {
    payload[at..at + 4].copy_from_slice(&value.value().to_le_bytes());
}

fn half(payload: &mut [u8], at: usize, value: u16) {
    payload[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn copy<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    payload: &mut [u8],
    at: usize,
    source: GuestAddress,
    length: u32,
) -> Result<(), MemoryError> {
    for index in 0..length {
        payload[at + index as usize] = context.read_u8(source.checked_add(index)?)?;
    }
    Ok(())
}

fn post<R: ShutdownRtos>(rtos: &mut R, operation: u16, payload: &[u8]) -> Result<u32, MemoryError> {
    let message = ServiceMessage { operation: RequestId(operation), sender: rtos.current_task(), destination: QueueId(COMMAND_QUEUE), payload };
    Ok(match rtos.post(message) { Ok(()) => ACCEPTED, Err(_) => QUEUE_ERROR })
}

pub fn helper_1039b48<M: GuestAddressSpace, E, R: ShutdownRtos>(
    context: &mut RecordContext<M, E>,
    rtos: &mut R,
    main: GuestAddress,
    first: GuestAddress,
    second: GuestAddress,
    extra: GuestAddress,
    completion: CallbackWord,
) -> Result<u32, MemoryError> {
    let (Some(main), Some(first), Some(second), Some(extra)) = (required(main), required(first), required(second), required(extra)) else {
        return Ok(QUEUE_ERROR);
    };
    let mut payload = [0u8; 148];
    copy(context, &mut payload, 0, main, 56)?;
    copy(context, &mut payload, 56, first, 16)?;
    copy(context, &mut payload, 72, second, 16)?;
    copy(context, &mut payload, 88, extra, 56)?;
    word(&mut payload, 144, completion.0);
    post(rtos, 0x4005, &payload)
}

pub fn helper_1039c7c<M: GuestAddressSpace, E, R: ShutdownRtos>(
    context: &mut RecordContext<M, E>,
    rtos: &mut R,
    descriptor: GuestAddress,
    dimensions: GuestAddress,
    extended: GuestAddress,
    mode: GuestAddress,
    snapshot: GuestAddress,
    channel: GuestAddress,
    optional: GuestAddress,
    enabled: u32,
    completion: CallbackWord,
) -> Result<u32, MemoryError> {
    let (Some(descriptor), Some(dimensions), Some(extended), Some(mode), Some(snapshot), Some(channel)) =
        (required(descriptor), required(dimensions), required(extended), required(mode), required(snapshot), required(channel))
    else {
        return Ok(QUEUE_ERROR);
    };
    let mut payload = [0u8; 204];
    copy(context, &mut payload, 0, descriptor, 16)?;
    copy(context, &mut payload, 16, dimensions, 24)?;
    copy(context, &mut payload, 40, extended, 16)?;
    copy(context, &mut payload, 56, mode, 16)?;
    copy(context, &mut payload, 72, snapshot, 56)?;
    copy(context, &mut payload, 128, channel, 56)?;
    if let Some(optional) = required(optional) {
        copy(context, &mut payload, 184, optional, 12)?;
    }
    word(&mut payload, 196, enabled);
    word(&mut payload, 200, completion.0);
    post(rtos, 0x4006, &payload)
}

pub fn helper_1039e48<M: GuestAddressSpace, E, R: ShutdownRtos>(
    context: &mut RecordContext<M, E>,
    rtos: &mut R,
    record: GuestAddress,
    first: GuestAddress,
    second: GuestAddress,
    third: GuestAddress,
    fourth: GuestAddress,
    fifth: GuestAddress,
    optional: GuestAddress,
    enabled: u32,
    selection: GuestAddress,
    value: u32,
    first_word: u32,
    second_word: u32,
    completion: CallbackWord,
    first_callback: CallbackWord,
    second_callback: CallbackWord,
) -> Result<u32, MemoryError> {
    let (Some(record), Some(first), Some(second), Some(third), Some(fourth), Some(fifth), Some(selection)) =
        (required(record), required(first), required(second), required(third), required(fourth), required(fifth), required(selection))
    else {
        return Ok(QUEUE_ERROR);
    };
    if first_callback.0 == 0 || second_callback.0 == 0 {
        return Ok(QUEUE_ERROR);
    }
    let mut payload = [0u8; 248];
    copy(context, &mut payload, 0, record, 16)?;
    copy(context, &mut payload, 16, first, 24)?;
    copy(context, &mut payload, 40, second, 16)?;
    copy(context, &mut payload, 56, third, 16)?;
    copy(context, &mut payload, 72, fourth, 56)?;
    copy(context, &mut payload, 128, fifth, 56)?;
    if let Some(optional) = required(optional) {
        copy(context, &mut payload, 184, optional, 12)?;
    }
    word(&mut payload, 196, enabled);
    for (index, at) in [200usize, 204, 208].into_iter().enumerate() {
        let selected = context.read_u32(selection.checked_add(index as u32 * 4)?)?;
        if selected != 0 {
            word(&mut payload, at, selected);
        }
    }
    word(&mut payload, 224, value);
    word(&mut payload, 228, first_word);
    word(&mut payload, 232, second_word);
    word(&mut payload, 236, completion.0);
    word(&mut payload, 240, first_callback.0);
    word(&mut payload, 244, second_callback.0);
    post(rtos, 0x4008, &payload)
}

pub fn helper_103a00c<M: GuestAddressSpace, E, R: ShutdownRtos>(
    context: &mut RecordContext<M, E>,
    rtos: &mut R,
    record: GuestAddress,
    callback: CallbackWord,
    pair: CallbackWord,
) -> Result<u32, MemoryError> {
    if pair.0 == 0 {
        return Ok(QUEUE_ERROR);
    }
    let Some(record) = required(record) else { return Ok(QUEUE_ERROR) };
    let mut payload = [0u8; 24];
    copy(context, &mut payload, 0, record, 16)?;
    word(&mut payload, 16, callback.0);
    word(&mut payload, 20, pair.0);
    post(rtos, 0x400a, &payload)
}

pub fn helper_103a09c<M: GuestAddressSpace, E, R: ShutdownRtos>(
    _context: &mut RecordContext<M, E>,
    rtos: &mut R,
    value: u32,
) -> Result<u32, MemoryError> {
    let mut payload = [0u8; 4];
    word(&mut payload, 0, value);
    post(rtos, 0x400b, &payload)
}

pub fn helper_103a0f0<M: GuestAddressSpace, E, R: ShutdownRtos>(
    _context: &mut RecordContext<M, E>,
    rtos: &mut R,
) -> Result<u32, MemoryError> {
    post(rtos, 0x400c, &[])
}

pub fn helper_103a13c<M: GuestAddressSpace, E, R: ShutdownRtos>(
    context: &mut RecordContext<M, E>,
    rtos: &mut R,
    record: GuestAddress,
    callback: CallbackWord,
) -> Result<u32, MemoryError> {
    let Some(record) = required(record) else { return Ok(QUEUE_ERROR) };
    let mut payload = [0u8; 32];
    copy(context, &mut payload, 0, record, 28)?;
    word(&mut payload, 28, callback.0);
    post(rtos, 0x400e, &payload)
}

pub fn helper_103a1a8<M: GuestAddressSpace, E, R: ShutdownRtos>(
    _context: &mut RecordContext<M, E>,
    rtos: &mut R,
    first: u32,
    second: u32,
) -> Result<u32, MemoryError> {
    let mut payload = [0u8; 8];
    word(&mut payload, 0, first);
    word(&mut payload, 4, second);
    post(rtos, 0x4009, &payload)
}

pub fn helper_103a200<M: GuestAddressSpace, E, R: ShutdownRtos>(
    _context: &mut RecordContext<M, E>,
    rtos: &mut R,
    value: u32,
) -> Result<u32, MemoryError> {
    let mut payload = [0u8; 4];
    word(&mut payload, 0, value);
    post(rtos, 0x400f, &payload)
}

pub fn helper_103a254<M: GuestAddressSpace, E, R: ShutdownRtos>(
    context: &mut RecordContext<M, E>,
    rtos: &mut R,
    record: GuestAddress,
    callback: CallbackWord,
) -> Result<u32, MemoryError> {
    let Some(record) = required(record) else { return Ok(QUEUE_ERROR) };
    let mut payload = [0u8; 28];
    copy(context, &mut payload, 0, record, 24)?;
    word(&mut payload, 24, callback.0);
    post(rtos, 0x4010, &payload)
}

pub fn helper_103a2c0<M: GuestAddressSpace, E, R: ShutdownRtos>(
    context: &mut RecordContext<M, E>,
    rtos: &mut R,
    record: GuestAddress,
    callback: CallbackWord,
) -> Result<u32, MemoryError> {
    let Some(record) = required(record) else { return Ok(QUEUE_ERROR) };
    let mut payload = [0u8; 20];
    copy(context, &mut payload, 0, record, 16)?;
    word(&mut payload, 16, callback.0);
    post(rtos, 0x4011, &payload)
}

pub fn helper_103a32c<M: GuestAddressSpace, E, R: ShutdownRtos>(
    context: &mut RecordContext<M, E>,
    rtos: &mut R,
    record: GuestAddress,
    callback: CallbackWord,
) -> Result<u32, MemoryError> {
    let Some(record) = required(record) else { return Ok(QUEUE_ERROR) };
    let mut payload = [0u8; 60];
    copy(context, &mut payload, 0, record, 56)?;
    word(&mut payload, 56, callback.0);
    post(rtos, 0x4012, &payload)
}

pub fn helper_103a398<M: GuestAddressSpace, E, R: ShutdownRtos>(
    context: &mut RecordContext<M, E>,
    rtos: &mut R,
    record: GuestAddress,
    callback: CallbackWord,
) -> Result<u32, MemoryError> {
    let Some(record) = required(record) else { return Ok(QUEUE_ERROR) };
    let mut payload = [0u8; 60];
    copy(context, &mut payload, 0, record, 56)?;
    word(&mut payload, 56, callback.0);
    post(rtos, 0x402c, &payload)
}

pub fn helper_103a404<M: GuestAddressSpace, E, R: ShutdownRtos>(
    context: &mut RecordContext<M, E>,
    rtos: &mut R,
    record: GuestAddress,
    callback: CallbackWord,
) -> Result<u32, MemoryError> {
    let Some(record) = required(record) else { return Ok(QUEUE_ERROR) };
    let mut payload = [0u8; 20];
    copy(context, &mut payload, 0, record, 16)?;
    word(&mut payload, 16, callback.0);
    post(rtos, 0x4013, &payload)
}

pub fn helper_103a470<M: GuestAddressSpace, E, R: ShutdownRtos>(
    context: &mut RecordContext<M, E>,
    rtos: &mut R,
    record: GuestAddress,
    mode: u16,
    callback: CallbackWord,
) -> Result<u32, MemoryError> {
    let Some(record) = required(record) else { return Ok(QUEUE_ERROR) };
    let mut payload = [0u8; 12];
    copy(context, &mut payload, 0, record, 6)?;
    half(&mut payload, 6, mode);
    word(&mut payload, 8, callback.0);
    post(rtos, 0x4014, &payload)
}

pub fn helper_103a594<M: GuestAddressSpace, E, R: ShutdownRtos>(
    context: &mut RecordContext<M, E>,
    rtos: &mut R,
    record: GuestAddress,
    callback: CallbackWord,
    pair: CallbackWord,
    extra: GuestAddress,
) -> Result<u32, MemoryError> {
    if pair.0 == 0 {
        return Ok(QUEUE_ERROR);
    }
    let Some(record) = required(record) else { return Ok(QUEUE_ERROR) };
    let mut payload = [0u8; 92];
    copy(context, &mut payload, 0, record, 28)?;
    word(&mut payload, 28, callback.0);
    word(&mut payload, 32, pair.0);
    if let Some(extra) = required(extra) {
        copy(context, &mut payload, 36, extra, 56)?;
    }
    post(rtos, 0x4017, &payload)
}

pub fn helper_103a694<M: GuestAddressSpace, E, R: ShutdownRtos>(
    _context: &mut RecordContext<M, E>,
    rtos: &mut R,
    value: u32,
    record: GuestAddress,
    first: CallbackWord,
    enabled: u32,
    second: CallbackWord,
) -> Result<u32, MemoryError> {
    if record.get() == 0 {
        return Ok(NULL_RECORD);
    }
    let mut payload = [0u8; 20];
    word(&mut payload, 0, value);
    word(&mut payload, 4, record.get());
    word(&mut payload, 8, first.0);
    word(&mut payload, 12, second.0);
    word(&mut payload, 16, enabled);
    post(rtos, 0x402a, &payload)
}

pub fn helper_103a708<M: GuestAddressSpace, E, R: ShutdownRtos>(
    context: &mut RecordContext<M, E>,
    rtos: &mut R,
    first: CallbackWord,
    second: u32,
    record: GuestAddress,
    ratio: GuestAddress,
    tail: u32,
) -> Result<u32, MemoryError> {
    let Some(record) = required(record) else { return Ok(QUEUE_ERROR) };
    let mut payload = [0u8; 28];
    word(&mut payload, 0, first.0);
    word(&mut payload, 4, second);
    copy(context, &mut payload, 8, record, 8)?;
    let ratio_first = context.read_u32(ratio)?;
    word(&mut payload, 16, ratio_first);
    let ratio_second = context.read_u32(ratio.checked_add(4)?)?;
    word(&mut payload, 20, ratio_second);
    word(&mut payload, 24, tail);
    post(rtos, 0x402e, &payload)
}

#[cfg(target_arch = "arm")]
mod native {
    use super::*;

    macro_rules! command_export {
        ($symbol:ident, ($($cp:ident: $cty:ty),*), ($($conv:expr),*), $body:path) => {
            #[no_mangle]
            pub unsafe extern "C" fn $symbol($($cp: $cty),*) -> u32 {
                crate::record_native_boundary::with_native_context::<(), _>(|context| {
                    match $body(context, &mut crate::shutdown_rtos::NativeShutdownRtos $(, $conv)*) {
                        Ok(value) => value,
                        Err(_) => crate::record_native_boundary::fault(),
                    }
                })
            }
        };
    }

    command_export!(xm1_power_record_helper_1039b48,
        (main: *const u8, first: *const u8, second: *const u8, extra: *const u8, completion: u32),
        (GuestAddress::new(main as u32), GuestAddress::new(first as u32), GuestAddress::new(second as u32), GuestAddress::new(extra as u32), CallbackWord(completion as usize)),
        super::helper_1039b48);
    command_export!(xm1_power_record_helper_1039c7c,
        (descriptor: *const u8, dimensions: *const u8, extended: *const u8, mode: *const u8, snapshot: *const u8, channel: *const u8, optional: *const u8, enabled: u32, completion: u32),
        (GuestAddress::new(descriptor as u32), GuestAddress::new(dimensions as u32), GuestAddress::new(extended as u32), GuestAddress::new(mode as u32), GuestAddress::new(snapshot as u32), GuestAddress::new(channel as u32), GuestAddress::new(optional as u32), enabled, CallbackWord(completion as usize)),
        super::helper_1039c7c);
    command_export!(xm1_power_record_helper_1039e48,
        (record: *const u8, first: *const u8, second: *const u8, third: *const u8, fourth: *const u8, fifth: *const u8, optional: *const u8, enabled: u32, selection: *const u32, value: u32, first_word: u32, second_word: u32, completion: u32, first_callback: u32, second_callback: u32),
        (GuestAddress::new(record as u32), GuestAddress::new(first as u32), GuestAddress::new(second as u32), GuestAddress::new(third as u32), GuestAddress::new(fourth as u32), GuestAddress::new(fifth as u32), GuestAddress::new(optional as u32), enabled, GuestAddress::new(selection as u32), value, first_word, second_word, CallbackWord(completion as usize), CallbackWord(first_callback as usize), CallbackWord(second_callback as usize)),
        super::helper_1039e48);
    command_export!(xm1_power_record_helper_103a00c,
        (record: *const u8, callback: u32, pair: u32),
        (GuestAddress::new(record as u32), CallbackWord(callback as usize), CallbackWord(pair as usize)),
        super::helper_103a00c);
    command_export!(xm1_power_record_helper_103a09c, (value: u32), (value), super::helper_103a09c);
    command_export!(xm1_power_record_helper_103a0f0, (), (), super::helper_103a0f0);
    command_export!(xm1_power_record_helper_103a13c,
        (record: *const u8, callback: u32),
        (GuestAddress::new(record as u32), CallbackWord(callback as usize)),
        super::helper_103a13c);
    command_export!(xm1_power_record_helper_103a1a8, (first: u32, second: u32), (first, second), super::helper_103a1a8);
    command_export!(xm1_power_record_helper_103a200, (value: u32), (value), super::helper_103a200);
    command_export!(xm1_power_record_helper_103a254,
        (record: *const u8, callback: u32),
        (GuestAddress::new(record as u32), CallbackWord(callback as usize)),
        super::helper_103a254);
    command_export!(xm1_power_record_helper_103a2c0,
        (record: *const u8, callback: u32),
        (GuestAddress::new(record as u32), CallbackWord(callback as usize)),
        super::helper_103a2c0);
    command_export!(xm1_power_record_helper_103a32c,
        (record: *const u8, callback: u32),
        (GuestAddress::new(record as u32), CallbackWord(callback as usize)),
        super::helper_103a32c);
    command_export!(xm1_power_record_helper_103a398,
        (record: *const u8, callback: u32),
        (GuestAddress::new(record as u32), CallbackWord(callback as usize)),
        super::helper_103a398);
    command_export!(xm1_power_record_helper_103a404,
        (record: *const u8, callback: u32),
        (GuestAddress::new(record as u32), CallbackWord(callback as usize)),
        super::helper_103a404);
    command_export!(xm1_power_record_helper_103a470,
        (record: *const u8, mode: u32, callback: u32),
        (GuestAddress::new(record as u32), mode as u16, CallbackWord(callback as usize)),
        super::helper_103a470);
    command_export!(xm1_power_record_helper_103a594,
        (record: *const u8, callback: u32, pair: u32, extra: *const u8),
        (GuestAddress::new(record as u32), CallbackWord(callback as usize), CallbackWord(pair as usize), GuestAddress::new(extra as u32)),
        super::helper_103a594);
    command_export!(xm1_power_record_helper_103a694,
        (value: u32, record: *const u8, first: u32, enabled: u32, second: u32),
        (value, GuestAddress::new(record as u32), CallbackWord(first as usize), enabled, CallbackWord(second as usize)),
        super::helper_103a694);
    command_export!(xm1_power_record_helper_103a708,
        (first: u32, second: u32, record: *const u32, ratio: *const u32, tail: u32),
        (CallbackWord(first as usize), second, GuestAddress::new(record as u32), GuestAddress::new(ratio as u32), tail),
        super::helper_103a708);
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x0103_9b48 to
    //! 0x0103_a708.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};
    use crate::shutdown::KernelStatus;

    const RECORD: u32 = 0x0010_0000;
    const SECOND: u32 = 0x0010_1000;

    struct Recorder {
        sender: u8,
        fail: bool,
        posts: Vec<(u16, u8, u8, Vec<u8>)>,
    }

    impl Recorder {
        fn new() -> Self { Self { sender: 7, fail: false, posts: Vec::new() } }
        fn last(&self) -> (u16, u8, u8, Vec<u8>) { self.posts.last().cloned().unwrap() }
    }

    impl ShutdownRtos for Recorder {
        fn current_task(&mut self) -> TaskId { TaskId(self.sender) }
        fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
            if self.fail { return Err(KernelStatus(-1)); }
            self.posts.push((message.operation.0, message.sender.0, message.destination.0, message.payload.to_vec()));
            Ok(())
        }
        fn set_event_flag(&mut self, _flag: u32, _bits: u32) -> Result<(), KernelStatus> { Ok(()) }
    }

    struct Memory {
        record: [u8; 0x200],
        second: [u8; 0x100],
        ratio: [u8; 8],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(RECORD, 0x200), &mut memory.record).unwrap(),
            MemoryRegion::new(descriptor(SECOND, 0x100), &mut memory.second).unwrap(),
            MemoryRegion::new(descriptor(0x0010_2000, 8), &mut memory.ratio).unwrap(),
        ];
        body(&mut RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ()))
    }

    fn filled() -> Memory {
        let mut memory = Memory { record: [0; 0x200], second: [0; 0x100], ratio: [0; 8] };
        for (index, byte) in memory.record.iter_mut().enumerate() { *byte = index as u8; }
        for (index, byte) in memory.second.iter_mut().enumerate() { *byte = (index as u8).wrapping_add(0x40); }
        memory.ratio[..4].copy_from_slice(&0x1122_3344u32.to_le_bytes());
        memory.ratio[4..].copy_from_slice(&0x5566_7788u32.to_le_bytes());
        memory
    }

    fn address(offset: u32) -> GuestAddress { GuestAddress::new(RECORD + offset) }

    #[test]
    fn every_sender_posts_its_operation_payload_and_returns_zero() {
        let mut memory = filled();
        let record = memory.record;
        with_memory(&mut memory, |context| {
            let mut rtos = Recorder::new();
            assert_eq!(helper_1039b48(context, &mut rtos, address(0), address(0x40), address(0x80), address(0xc0), CallbackWord(0xc0de)), Ok(ACCEPTED));
            let (operation, sender, destination, payload) = rtos.last();
            assert_eq!((operation, sender, destination), (0x4005, 7, COMMAND_QUEUE));
            assert_eq!(payload.len(), 148);
            assert_eq!(&payload[..56], &record[..56]);
            assert_eq!(&payload[56..72], &record[0x40..0x50]);
            assert_eq!(&payload[72..88], &record[0x80..0x90]);
            assert_eq!(&payload[88..144], &record[0xc0..0xf8]);
            assert_eq!(&payload[144..], &0xc0deu32.to_le_bytes());

            assert_eq!(helper_1039c7c(context, &mut rtos, address(0), address(0x40), address(0x80), address(0xc0), address(0x100), address(0x140), GuestAddress::new(0), 3, CallbackWord(9)), Ok(ACCEPTED));
            let (operation, _, _, payload) = rtos.last();
            assert_eq!(operation, 0x4006);
            assert_eq!(payload.len(), 204);
            assert_eq!(&payload[16..40], &record[0x40..0x58]);
            assert_eq!(&payload[184..196], &[0; 12]);
            assert_eq!(&payload[196..200], &3u32.to_le_bytes());
            assert_eq!(&payload[200..204], &9u32.to_le_bytes());

            assert_eq!(helper_103a00c(context, &mut rtos, address(0), CallbackWord(1), CallbackWord(2)), Ok(ACCEPTED));
            assert_eq!(rtos.last().0, 0x400a);
            assert_eq!(rtos.last().3.len(), 24);
            assert_eq!(&rtos.last().3[16..], &[1, 0, 0, 0, 2, 0, 0, 0]);
            assert_eq!(helper_103a09c(context, &mut rtos, 0xabcd), Ok(ACCEPTED));
            assert_eq!(rtos.last().0, 0x400b);
            assert_eq!(rtos.last().3, 0xabcdu32.to_le_bytes().to_vec());
            assert_eq!(helper_103a0f0(context, &mut rtos), Ok(ACCEPTED));
            assert_eq!((rtos.last().0, rtos.last().3.len()), (0x400c, 0));
            assert_eq!(helper_103a13c(context, &mut rtos, address(0), CallbackWord(5)), Ok(ACCEPTED));
            assert_eq!((rtos.last().0, rtos.last().3.len()), (0x400e, 32));
            assert_eq!(&rtos.last().3[28..], &5u32.to_le_bytes());
            assert_eq!(helper_103a1a8(context, &mut rtos, 1, 2), Ok(ACCEPTED));
            assert_eq!(rtos.last().3, [1u8, 0, 0, 0, 2, 0, 0, 0].to_vec());
            assert_eq!(helper_103a200(context, &mut rtos, 0x1234), Ok(ACCEPTED));
            assert_eq!(rtos.last().0, 0x400f);
            assert_eq!(helper_103a254(context, &mut rtos, address(0), CallbackWord(5)), Ok(ACCEPTED));
            assert_eq!((rtos.last().0, rtos.last().3.len()), (0x4010, 28));
            assert_eq!(helper_103a2c0(context, &mut rtos, address(0), CallbackWord(5)), Ok(ACCEPTED));
            assert_eq!((rtos.last().0, rtos.last().3.len()), (0x4011, 20));
            assert_eq!(helper_103a32c(context, &mut rtos, address(0), CallbackWord(5)), Ok(ACCEPTED));
            assert_eq!((rtos.last().0, rtos.last().3.len()), (0x4012, 60));
            assert_eq!(helper_103a398(context, &mut rtos, address(0), CallbackWord(5)), Ok(ACCEPTED));
            assert_eq!((rtos.last().0, rtos.last().3.len()), (0x402c, 60));
            assert_eq!(helper_103a404(context, &mut rtos, address(0), CallbackWord(5)), Ok(ACCEPTED));
            assert_eq!((rtos.last().0, rtos.last().3.len()), (0x4013, 20));
            assert_eq!(helper_103a470(context, &mut rtos, address(0), 0x1357, CallbackWord(5)), Ok(ACCEPTED));
            let (operation, _, _, payload) = rtos.last();
            assert_eq!((operation, payload.len()), (0x4014, 12));
            assert_eq!(&payload[..6], &record[..6]);
            assert_eq!(&payload[6..8], &0x1357u16.to_le_bytes());
            assert_eq!(&payload[8..], &5u32.to_le_bytes());
            assert_eq!(helper_103a594(context, &mut rtos, address(0), CallbackWord(1), CallbackWord(2), address(0x40)), Ok(ACCEPTED));
            let (operation, _, _, payload) = rtos.last();
            assert_eq!((operation, payload.len()), (0x4017, 92));
            assert_eq!(&payload[..28], &record[..28]);
            assert_eq!(&payload[28..32], &1u32.to_le_bytes());
            assert_eq!(&payload[32..36], &2u32.to_le_bytes());
            assert_eq!(&payload[36..92], &record[0x40..0x78]);
            assert_eq!(helper_103a694(context, &mut rtos, 0x10, address(0), CallbackWord(1), 0x20, CallbackWord(2)), Ok(ACCEPTED));
            let (operation, _, _, payload) = rtos.last();
            assert_eq!((operation, payload.len()), (0x402a, 20));
            assert_eq!(&payload[..4], &0x10u32.to_le_bytes());
            assert_eq!(&payload[4..8], &RECORD.to_le_bytes());
            assert_eq!(&payload[8..12], &1u32.to_le_bytes());
            assert_eq!(&payload[12..16], &2u32.to_le_bytes());
            assert_eq!(&payload[16..20], &0x20u32.to_le_bytes());
            assert_eq!(helper_103a708(context, &mut rtos, CallbackWord(1), 2, address(0), GuestAddress::new(0x0010_2000), 3), Ok(ACCEPTED));
            let (operation, _, _, payload) = rtos.last();
            assert_eq!((operation, payload.len()), (0x402e, 28));
            assert_eq!(&payload[..8], &[1, 0, 0, 0, 2, 0, 0, 0]);
            assert_eq!(&payload[8..16], &record[..8]);
            assert_eq!(&payload[16..24], &[0x44, 0x33, 0x22, 0x11, 0x88, 0x77, 0x66, 0x55]);
            assert_eq!(&payload[24..], &3u32.to_le_bytes());
        });
    }

    #[test]
    fn a_null_record_returns_the_queue_error_without_posting() {
        let mut memory = filled();
        with_memory(&mut memory, |context| {
            let mut rtos = Recorder::new();
            let null = GuestAddress::new(0);
            assert_eq!(helper_1039b48(context, &mut rtos, null, address(0), address(0), address(0), CallbackWord(1)), Ok(QUEUE_ERROR));
            assert_eq!(helper_1039c7c(context, &mut rtos, address(0), null, address(0), address(0), address(0), address(0), address(0), 0, CallbackWord(1)), Ok(QUEUE_ERROR));
            assert_eq!(helper_103a00c(context, &mut rtos, null, CallbackWord(1), CallbackWord(2)), Ok(QUEUE_ERROR));
            assert_eq!(helper_103a13c(context, &mut rtos, null, CallbackWord(1)), Ok(QUEUE_ERROR));
            assert_eq!(helper_103a254(context, &mut rtos, null, CallbackWord(1)), Ok(QUEUE_ERROR));
            assert_eq!(helper_103a594(context, &mut rtos, null, CallbackWord(1), CallbackWord(2), address(0)), Ok(QUEUE_ERROR));
            assert_eq!(helper_103a708(context, &mut rtos, CallbackWord(1), 0, null, GuestAddress::new(0x0010_2000), 0), Ok(QUEUE_ERROR));
            assert!(rtos.posts.is_empty());
        });
    }

    #[test]
    fn a_null_completion_word_returns_the_queue_error() {
        let mut memory = filled();
        with_memory(&mut memory, |context| {
            let mut rtos = Recorder::new();
            assert_eq!(helper_103a00c(context, &mut rtos, address(0), CallbackWord(1), CallbackWord(0)), Ok(QUEUE_ERROR));
            assert_eq!(helper_103a594(context, &mut rtos, address(0), CallbackWord(1), CallbackWord(0), address(0)), Ok(QUEUE_ERROR));
            assert_eq!(
                helper_1039e48(context, &mut rtos, address(0), address(0), address(0), address(0), address(0), address(0), address(0), 0, address(0), 0, 0, 0, CallbackWord(1), CallbackWord(0), CallbackWord(2)),
                Ok(QUEUE_ERROR)
            );
            assert_eq!(
                helper_1039e48(context, &mut rtos, address(0), address(0), address(0), address(0), address(0), address(0), address(0), 0, address(0), 0, 0, 0, CallbackWord(1), CallbackWord(1), CallbackWord(0)),
                Ok(QUEUE_ERROR)
            );
            assert!(rtos.posts.is_empty());
        });
    }

    #[test]
    fn a_rejected_post_returns_the_queue_error() {
        let mut memory = filled();
        with_memory(&mut memory, |context| {
            let mut rtos = Recorder { sender: 7, fail: true, posts: Vec::new() };
            assert_eq!(helper_103a09c(context, &mut rtos, 1), Ok(QUEUE_ERROR));
            assert_eq!(helper_103a0f0(context, &mut rtos), Ok(QUEUE_ERROR));
        });
    }

    #[test]
    fn a_null_record_argument_is_reported_as_0x84_only_by_103a694() {
        let mut memory = filled();
        with_memory(&mut memory, |context| {
            let mut rtos = Recorder::new();
            assert_eq!(helper_103a694(context, &mut rtos, 0, GuestAddress::new(0), CallbackWord(1), 0, CallbackWord(2)), Ok(NULL_RECORD));
            assert!(rtos.posts.is_empty());
        });
    }

    #[test]
    fn the_optional_record_is_zeroed_when_absent_and_the_selection_words_skip_zero() {
        let mut memory = filled();
        memory.second[..12].copy_from_slice(&[0, 0, 0, 0, 0xaa, 0, 0, 0, 0, 0, 0, 0]);
        with_memory(&mut memory, |context| {
            let mut rtos = Recorder::new();
            assert_eq!(helper_1039c7c(context, &mut rtos, address(0), address(0), address(0), address(0), address(0), address(0), GuestAddress::new(0), 0, CallbackWord(0)), Ok(ACCEPTED));
            assert_eq!(&rtos.last().3[184..196], &[0; 12]);
            assert_eq!(helper_103a594(context, &mut rtos, address(0), CallbackWord(1), CallbackWord(2), GuestAddress::new(0)), Ok(ACCEPTED));
            assert_eq!(&rtos.last().3[36..92], &[0; 56]);
            assert_eq!(
                helper_1039e48(context, &mut rtos, address(0), address(0), address(0), address(0), address(0), address(0), address(0), 0, GuestAddress::new(SECOND), 0, 0, 0, CallbackWord(1), CallbackWord(1), CallbackWord(2)),
                Ok(ACCEPTED)
            );
            let payload = rtos.last().3;
            assert_eq!(&payload[200..212], &[0, 0, 0, 0, 0xaa, 0, 0, 0, 0, 0, 0, 0]);
        });
    }
}
