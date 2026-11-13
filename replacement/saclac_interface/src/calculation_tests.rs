use crate::calculation::{ReferenceLimits, ReferenceRequest};

fn request() -> ReferenceRequest {
    let mut bytes = [0u8; ReferenceRequest::BYTES];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = index as u8;
    }
    ReferenceRequest::from_bytes(bytes)
}

#[test]
fn a_shift_moves_the_ten_positions_and_nothing_else() {
    let before = request();
    let mut after = before;
    after.shift_positions(-3);
    assert_eq!(after.position(), before.position().wrapping_sub(3));
    for (moved, original) in after.area_positions().iter().zip(before.area_positions()) {
        assert_eq!(*moved, original.wrapping_sub(3));
    }
    for index in (0..ReferenceRequest::BYTES).filter(|index| !(0x38..0x3a).contains(index) && !(0x104..0x116).contains(index)) {
        assert_eq!(after.bytes()[index], before.bytes()[index], "byte {index:#x}");
    }
}

#[test]
fn limits_round_trip_and_keep_the_references() {
    let mut block = request();
    let limits = ReferenceLimits { first: -2, second: 0x1234, option: 7 };
    let reference = block.reference();
    block.set_limits(limits);
    assert_eq!(block.limits(), limits);
    assert_eq!(block.reference(), reference);
    assert_eq!(&block.bytes()[0x80..0x82], &(-2i16).to_le_bytes());
    assert_eq!(&block.bytes()[0x8c..0x90], &7u32.to_le_bytes());
}
