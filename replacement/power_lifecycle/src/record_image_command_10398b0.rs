//! The image-command sender at 0x0103_98b0.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::shutdown::{QueueId, RequestId, ServiceMessage, ShutdownRtos};

pub const COMMAND_QUEUE: u8 = 0x30;
pub const ACCEPTED: u32 = 0;
pub const QUEUE_ERROR: u32 = 0x86;

fn required(source: GuestAddress) -> Option<GuestAddress> {
    if source.get() == 0 { None } else { Some(source) }
}

fn word(payload: &mut [u8], at: usize, value: u32) {
    payload[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn copy<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, payload: &mut [u8], at: usize, source: GuestAddress, length: u32) -> Result<(), MemoryError> {
    for index in 0..length {
        payload[at + index as usize] = context.read_u8(source.checked_add(index)?)?;
    }
    Ok(())
}

fn post<R: ShutdownRtos>(rtos: &mut R, operation: u16, payload: &[u8]) -> u32 {
    let message = ServiceMessage { operation: RequestId(operation), sender: rtos.current_task(), destination: QueueId(COMMAND_QUEUE), payload };
    match rtos.post(message) {
        Ok(()) => ACCEPTED,
        Err(_) => QUEUE_ERROR,
    }
}

/// 0x010398b0
pub fn helper_10398b0<M: GuestAddressSpace, E, R: ShutdownRtos>(
    context: &mut RecordContext<M, E>,
    rtos: &mut R,
    first: GuestAddress,
    second: GuestAddress,
    third: GuestAddress,
    fourth: GuestAddress,
    fifth: GuestAddress,
    sixth: GuestAddress,
    seventh: u32,
    optional: GuestAddress,
    ninth: u32,
    tenth: u32,
) -> Result<u32, MemoryError> {
    let extended = context.read_u32(sixth.checked_add(4)?)? != 0;
    let (Some(first), Some(second), Some(third), Some(fourth), Some(fifth), Some(sixth)) =
        (required(first), required(second), required(third), required(fourth), required(fifth), required(sixth))
    else {
        return Ok(QUEUE_ERROR);
    };
    if extended {
        let mut payload = [0u8; 204];
        copy(context, &mut payload, 0, first, 16)?;
        copy(context, &mut payload, 16, second, 24)?;
        copy(context, &mut payload, 40, third, 16)?;
        copy(context, &mut payload, 56, fourth, 16)?;
        copy(context, &mut payload, 72, fifth, 56)?;
        copy(context, &mut payload, 128, sixth, 56)?;
        if let Some(optional) = required(optional) {
            copy(context, &mut payload, 184, optional, 12)?;
        }
        word(&mut payload, 196, seventh);
        word(&mut payload, 200, tenth);
        Ok(post(rtos, 0x4006, &payload))
    } else {
        let mut payload = [0u8; 208];
        copy(context, &mut payload, 0, first, 16)?;
        copy(context, &mut payload, 16, second, 24)?;
        copy(context, &mut payload, 40, third, 16)?;
        copy(context, &mut payload, 56, fourth, 16)?;
        copy(context, &mut payload, 72, fifth, 56)?;
        copy(context, &mut payload, 128, sixth, 56)?;
        if let Some(optional) = required(optional) {
            copy(context, &mut payload, 188, optional, 12)?;
        }
        word(&mut payload, 184, seventh);
        word(&mut payload, 200, ninth);
        word(&mut payload, 204, tenth);
        Ok(post(rtos, 0x4004, &payload))
    }
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 body at 0x0103_98b0.
    use super::*;
    use crate::record_address_space::{
        ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission,
    };
    use crate::shutdown::{KernelStatus, TaskId};

    const FIRST: u32 = 0x0010_0000;
    const SECOND: u32 = 0x0010_0100;
    const THIRD: u32 = 0x0010_0200;
    const FOURTH: u32 = 0x0010_0300;
    const FIFTH: u32 = 0x0010_0400;
    const SIXTH: u32 = 0x0010_0500;
    const OPTIONAL: u32 = 0x0010_0600;

    struct Recorder {
        fail: bool,
        posts: Vec<(u16, u8, u8, Vec<u8>)>,
    }

    impl Recorder {
        fn new() -> Self { Self { fail: false, posts: Vec::new() } }
    }

    impl ShutdownRtos for Recorder {
        fn current_task(&mut self) -> TaskId { TaskId(7) }
        fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
            if self.fail { return Err(KernelStatus(-1)); }
            self.posts.push((message.operation.0, message.sender.0, message.destination.0, message.payload.to_vec()));
            Ok(())
        }
        fn set_event_flag(&mut self, _flag: u32, _bits: u32) -> Result<(), KernelStatus> { Ok(()) }
    }

    struct Memory {
        low: [u8; 4],
        first: [u8; 16],
        second: [u8; 24],
        third: [u8; 16],
        fourth: [u8; 16],
        fifth: [u8; 56],
        sixth: [u8; 56],
        optional: [u8; 12],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(0x0000_0004, 4), &mut memory.low).unwrap(),
            MemoryRegion::new(descriptor(FIRST, 16), &mut memory.first).unwrap(),
            MemoryRegion::new(descriptor(SECOND, 24), &mut memory.second).unwrap(),
            MemoryRegion::new(descriptor(THIRD, 16), &mut memory.third).unwrap(),
            MemoryRegion::new(descriptor(FOURTH, 16), &mut memory.fourth).unwrap(),
            MemoryRegion::new(descriptor(FIFTH, 56), &mut memory.fifth).unwrap(),
            MemoryRegion::new(descriptor(SIXTH, 56), &mut memory.sixth).unwrap(),
            MemoryRegion::new(descriptor(OPTIONAL, 12), &mut memory.optional).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn filled(byte: u8) -> Memory {
        Memory {
            low: [0; 4],
            first: [byte; 16],
            second: [byte.wrapping_add(1); 24],
            third: [byte.wrapping_add(2); 16],
            fourth: [byte.wrapping_add(3); 16],
            fifth: [byte.wrapping_add(4); 56],
            sixth: [byte.wrapping_add(5); 56],
            optional: [byte.wrapping_add(6); 12],
        }
    }

    fn send(memory: &mut Memory, extended: bool, optional: GuestAddress, fail: bool) -> (u32, Option<(u16, u8, u8, Vec<u8>)>) {
        memory.sixth[4..8].copy_from_slice(&u32::from(extended).to_le_bytes());
        with_memory(memory, |context| {
            let mut rtos = Recorder::new();
            rtos.fail = fail;
            let result = helper_10398b0(
                context,
                &mut rtos,
                GuestAddress::new(FIRST),
                GuestAddress::new(SECOND),
                GuestAddress::new(THIRD),
                GuestAddress::new(FOURTH),
                GuestAddress::new(FIFTH),
                GuestAddress::new(SIXTH),
                0x1111_2222,
                optional,
                0x3333_4444,
                0x5555_6666,
            );
            (result.unwrap(), rtos.posts.pop())
        })
    }

    #[test]
    fn the_short_variant_posts_operation_4004_with_208_bytes() {
        let mut memory = filled(0x40);
        let (status, post) = send(&mut memory, false, GuestAddress::new(OPTIONAL), false);
        assert_eq!(status, ACCEPTED);
        let (operation, sender, destination, payload) = post.unwrap();
        assert_eq!((operation, sender, destination), (0x4004, 7, COMMAND_QUEUE));
        assert_eq!(payload.len(), 208);
        assert_eq!(&payload[..16], &memory.first[..]);
        assert_eq!(&payload[16..40], &memory.second[..]);
        assert_eq!(&payload[40..56], &memory.third[..]);
        assert_eq!(&payload[56..72], &memory.fourth[..]);
        assert_eq!(&payload[72..128], &memory.fifth[..]);
        assert_eq!(&payload[128..184], &memory.sixth[..]);
        assert_eq!(&payload[184..188], &0x1111_2222u32.to_le_bytes());
        assert_eq!(&payload[188..200], &memory.optional[..]);
        assert_eq!(&payload[200..204], &0x3333_4444u32.to_le_bytes());
        assert_eq!(&payload[204..208], &0x5555_6666u32.to_le_bytes());
    }

    #[test]
    fn the_extended_variant_posts_operation_4006_with_204_bytes() {
        let mut memory = filled(0x40);
        let (status, post) = send(&mut memory, true, GuestAddress::new(OPTIONAL), false);
        assert_eq!(status, ACCEPTED);
        let (operation, _, _, payload) = post.unwrap();
        assert_eq!(operation, 0x4006);
        assert_eq!(payload.len(), 204);
        assert_eq!(&payload[128..184], &memory.sixth[..]);
        assert_eq!(&payload[184..196], &memory.optional[..]);
        assert_eq!(&payload[196..200], &0x1111_2222u32.to_le_bytes());
        assert_eq!(&payload[200..204], &0x5555_6666u32.to_le_bytes());
    }

    #[test]
    fn an_absent_optional_record_leaves_its_bytes_zero() {
        let mut memory = filled(0x40);
        let (status, post) = send(&mut memory, true, GuestAddress::new(0), false);
        assert_eq!(status, ACCEPTED);
        assert_eq!(&post.unwrap().3[184..196], &[0u8; 12][..]);
        let mut memory = filled(0x40);
        let (status, post) = send(&mut memory, false, GuestAddress::new(0), false);
        assert_eq!(status, ACCEPTED);
        assert_eq!(&post.unwrap().3[188..200], &[0u8; 12][..]);
    }

    #[test]
    fn a_null_record_or_a_failed_post_reports_the_queue_error() {
        let mut memory = filled(0x40);
        memory.sixth[4..8].copy_from_slice(&0u32.to_le_bytes());
        let status = with_memory(&mut memory, |context| {
            helper_10398b0(
                context,
                &mut Recorder::new(),
                GuestAddress::new(0),
                GuestAddress::new(SECOND),
                GuestAddress::new(THIRD),
                GuestAddress::new(FOURTH),
                GuestAddress::new(FIFTH),
                GuestAddress::new(SIXTH),
                0,
                GuestAddress::new(0),
                0,
                0,
            )
        });
        assert_eq!(status, Ok(QUEUE_ERROR));

        let mut memory = filled(0x40);
        let (status, post) = send(&mut memory, false, GuestAddress::new(OPTIONAL), true);
        assert_eq!(status, QUEUE_ERROR);
        assert!(post.is_none());
    }
}
