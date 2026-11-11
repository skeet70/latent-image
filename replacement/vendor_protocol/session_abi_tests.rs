use crate::session_abi::{vendor_session_context_alignment, vendor_session_context_size, vendor_session_control, vendor_session_initialize, ControlObservation};
use std::alloc::{alloc, dealloc, handle_alloc_error, Layout};

fn frame(opcode: u8, length: u32, token: u16) -> [u8; 12] {
    let mut bytes = [0; 12];
    bytes[0] = opcode;
    bytes[6..10].copy_from_slice(&length.to_be_bytes());
    bytes[10..12].copy_from_slice(&token.to_be_bytes());
    bytes
}

#[test]
fn externally_allocated_session_keeps_failed_validation_separate_from_session_activity() {
    let layout = Layout::from_size_align(vendor_session_context_size(), vendor_session_context_alignment()).unwrap();
    let storage = unsafe { alloc(layout) };
    if storage.is_null() { handle_alloc_error(layout); }
    let mut buffer = [0; 256];
    let mut observation = ControlObservation::new();
    unsafe {
        vendor_session_initialize(storage.cast(), &mut buffer);
        let rejected = frame(0xc2, 2, 0);
        assert_eq!(vendor_session_control(storage.cast(), rejected.as_ptr(), 0, 0, 0, &mut observation), 0);
        assert_eq!((observation.status, observation.active, observation.reply_length), (0x24, 0, 2));
        assert_eq!(&observation.reply[..2], &[0, 0]);
        let open = frame(0xc0, 8, 0);
        assert_eq!(vendor_session_control(storage.cast(), open.as_ptr(), 0x1fff, 0, 0, &mut observation), 0);
        assert_eq!((observation.status, observation.active, observation.sid), (0, 1, 1));
        assert_eq!(&observation.reply[..8], &[1, 0, 0, 0, 0, 0, 1, 0]);
        let close = frame(0xc4, 2, 1);
        assert_eq!(vendor_session_control(storage.cast(), close.as_ptr(), 0, 0, 0, &mut observation), 0);
        assert_eq!((observation.status, observation.active), (0, 0));
        dealloc(storage, layout);
    }
}
