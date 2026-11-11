use super::*;

fn callback(value: u32) -> CallbackAddress { CallbackAddress::from_snapshot(value) }

#[test]
fn capture_preserves_descriptor_and_stock_argument_order() {
    let request = Request::Capture {
        callbacks: CaptureCallbacks {
            first: callback(0x0102_0304),
            second: callback(0x1112_1314),
            third: callback(0x2122_2324),
            fourth: callback(0x3132_3334),
            fifth: callback(0x4142_4344),
        },
        argument: CaptureArgument(0x5152_5354),
        descriptor: &CaptureDescriptor::from_snapshot([0xa7; 0x98]),
    };
    let mut output = [0xed; 0xb4];
    assert_eq!(request.event().opcode(), 0x3f49);
    let payload = request.encode(&mut output).unwrap();
    assert_eq!(payload.len(), 0xb0);
    assert_eq!(&payload[..24], &[
        4, 3, 2, 1, 0x24, 0x23, 0x22, 0x21,
        0x14, 0x13, 0x12, 0x11, 0x54, 0x53, 0x52, 0x51,
        0x34, 0x33, 0x32, 0x31, 0x44, 0x43, 0x42, 0x41,
    ]);
    assert_eq!(&payload[24..], &[0xa7; 0x98]);
    assert_eq!(&output[0xb0..], &[0xed; 4]);
}

#[test]
fn small_request_appends_callback_after_descriptor() {
    let request = Request::Small {
        callback: callback(0x0102_0304),
        descriptor: &SmallCaptureDescriptor::from_snapshot([0xa6; 0x40]),
    };
    let mut output = [0; 0x44];
    assert_eq!(request.event().opcode(), 0x3f56);
    let payload = request.encode(&mut output).unwrap();
    assert_eq!(&payload[..0x40], &[0xa6; 0x40]);
    assert_eq!(&payload[0x40..], &[4, 3, 2, 1]);
}

#[test]
fn position_encodes_halfword_and_callback_at_distinct_offsets() {
    let request = Request::Position {
        callback: callback(0x0102_0304),
        first: CaptureArgument(0x1112_1314),
        second: CaptureArgument(0x2122_2324),
        position: PositionArgument(0x8182),
        final_argument: CaptureArgument(0x3132_3334),
    };
    let mut output = [0xf5; 20];
    assert_eq!(request.event().opcode(), 0x3f57);
    assert_eq!(request.encode(&mut output).unwrap(), &[
        0x14, 0x13, 0x12, 0x11, 0x24, 0x23, 0x22, 0x21,
        0x82, 0x81, 0, 0, 0x34, 0x33, 0x32, 0x31, 4, 3, 2, 1,
    ]);
}

#[test]
fn insufficient_output_is_unchanged() {
    let request = Request::Small {
        callback: callback(0),
        descriptor: &SmallCaptureDescriptor::from_snapshot([0xa6; 0x40]),
    };
    let mut output = [0xf7; 0x43];
    assert_eq!(request.encode(&mut output), Err(OutputTooSmall { required: 0x44 }));
    assert_eq!(output, [0xf7; 0x43]);
}

use xm1_power_lifecycle::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

#[test]
fn statistics_reset_publishes_stock_offsets_and_idle_state() {
    let mut bytes = [0x7d; 36];
    let descriptor = RegionDescriptor::new(GuestAddress::new(0x016f_812c), ByteCount(36), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
    let mut regions = [MemoryRegion::new(descriptor, &mut bytes).unwrap()];
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
    let mut context = RecordContext::new(memory, ());
    helper_ffc1ec(&mut context).unwrap();
    let mut published = [0; 9];
    for (index, word) in published.iter_mut().enumerate() {
        *word = context.read_u32(GuestAddress::new(0x016f_812c + index as u32 * 4)).unwrap();
    }
    assert_eq!(published, [2128, 3152, 4176, 3152, 8592, 8596, 8600, 8604, 0]);
}

#[test]
fn statistics_reset_preserves_earlier_writes_on_memory_failure() {
    let mut bytes = [0x7d; 4];
    let descriptor = RegionDescriptor::new(GuestAddress::new(0x016f_814c), ByteCount(4), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
    let mut regions = [MemoryRegion::new(descriptor, &mut bytes).unwrap()];
    let memory = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
    let mut context = RecordContext::new(memory, ());
    assert_eq!(helper_ffc1ec(&mut context), Err(MemoryError::Unmapped(GuestAddress::new(0x016f_812c))));
    assert_eq!(context.read_u32(GuestAddress::new(0x016f_814c)), Ok(0));
}
