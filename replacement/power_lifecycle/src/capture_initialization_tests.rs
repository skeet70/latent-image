use crate::capture_initialization::{copy_cached_settings, initialize_groups};
use crate::capture_initialization_state::{fill_bytes, read_byte, read_half, read_word, write_word};

#[cfg(target_os = "linux")]
#[test]
fn capture_group_reset_and_snapshot_copy_preserve_unowned_bytes_and_storage() {
    unsafe extern "C" {
        fn mmap(address: *mut core::ffi::c_void, length: usize, protection: i32, flags: i32, fd: i32, offset: isize) -> *mut core::ffi::c_void;
        fn munmap(address: *mut core::ffi::c_void, length: usize) -> i32;
    }
    const BASE: usize = 0x0094_3000;
    const LENGTH: usize = 0x20000;
    unsafe {
        let mapped = mmap(BASE as *mut _, LENGTH, 3, 0x100022, -1, 0);
        assert_eq!(mapped as usize, BASE);
        struct Mapping;
        impl Drop for Mapping {
            fn drop(&mut self) { unsafe { munmap(BASE as *mut _, LENGTH); } }
        }
        let _mapping = Mapping;
        const GROUPS: usize = 0x0095_4000;
        const COPIED_GROUPS: usize = 0x0095_5000;
        const CURRENT: usize = 0x0095_6838;
        const SNAPSHOT: usize = 0x0095_a6b4;
        fill_bytes(GROUPS, 0x55, 0x790);
        write_word(0x0094_32a0 + 0x948, 0x409b_0b10);
        initialize_groups(GROUPS);
        assert_eq!(read_half(GROUPS), 0);
        assert_eq!(read_word(GROUPS + 4), 0x409b_0b10);
        assert_eq!(read_word(GROUPS + 8), 0);
        assert_eq!(read_half(GROUPS + 12), 0);
        assert_eq!(read_half(GROUPS + 14), 0x5555);
        assert_eq!(read_word(GROUPS + 16), u32::MAX);
        assert_eq!(read_word(GROUPS + 52), u32::MAX);
        assert_eq!(read_word(GROUPS + 56), 0);
        assert_eq!(read_half(GROUPS + 98), 0x5555);
        assert_eq!(read_word(GROUPS + 0x778), 0);
        assert_eq!(read_byte(GROUPS + 0x788), 0x55);
        fill_bytes(CURRENT, 0xa7, 1000);
        fill_bytes(SNAPSHOT, 0x13, 1000);
        fill_bytes(COPIED_GROUPS, 0x22, 0x790);
        write_word(CURRENT + 0x174, GROUPS as u32);
        write_word(SNAPSHOT + 0x174, COPIED_GROUPS as u32);
        copy_cached_settings(SNAPSHOT, CURRENT);
        assert_eq!(read_word(CURRENT + 0x174), GROUPS as u32);
        assert_eq!(read_word(SNAPSHOT + 0x174), COPIED_GROUPS as u32);
        assert_eq!(read_word(SNAPSHOT), 0xa7a7_a7a7);
        assert_eq!(read_word(SNAPSHOT + 996), 0xa7a7_a7a7);
        assert_eq!(read_word(COPIED_GROUPS + 4), 0x409b_0b10);
        assert_eq!(read_half(COPIED_GROUPS + 14), 0x5555);
        assert_eq!(read_word(COPIED_GROUPS + 16), u32::MAX);
        assert_eq!(read_word(COPIED_GROUPS + 0x784), 0);
        assert_eq!(read_byte(COPIED_GROUPS + 0x788), 0x22);
    }
}
