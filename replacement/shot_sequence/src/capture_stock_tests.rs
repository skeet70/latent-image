use crate::boundary::ClientRef;
use crate::capture_boundary::{CaptureOptions, SlotSelection};
use crate::capture_stock;

#[test]
fn capture_block_fields() {
    let mut block = [0u8; capture_stock::CAPTURE_BLOCK_BYTES];
    for (offset, value) in [(0x4, 0x40u32), (0x8, 3), (0xc, 0x1000), (0x10, 77), (0x14, 0x2000), (0x38, 0x3000)] {
        block[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let parameters = capture_stock::capture_parameters(&block).unwrap();
    assert_eq!(parameters.options, CaptureOptions(0x40));
    assert!(parameters.reduced());
    assert_eq!((parameters.sources, parameters.reference, parameters.slots, parameters.selection), (ClientRef(0x1000), 77, ClientRef(0x2000), ClientRef(0x3000)));
    assert_eq!(capture_stock::capture_parameters(&block[..59]), None);
}

#[test]
fn the_selection_bytes_are_the_requested_slot_then_the_count() {
    assert_eq!(capture_stock::slot_selection([2, 3]), SlotSelection { requested: 2, count: 3 });
}
