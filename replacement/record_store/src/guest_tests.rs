use super::*;
use std::{sync::Mutex, vec, vec::Vec};

#[export_name = "record_store_initialized"]
static mut INITIALIZED: u32 = 0;
#[export_name = "record_store_wrappers"]
static mut WRAPPERS: [Wrapper; 5] = [const { Wrapper { active: 0, banks: 0, slot: ptr::null_mut() } }; 5];
#[repr(C, align(8))]
struct State([u8; 360]);
#[export_name = "record_store_state"]
static mut STATE: State = State([0; 360]);
#[export_name = "record_store_block"]
static mut BLOCK: [u8; BLOCK_BYTES] = [0; BLOCK_BYTES];
#[export_name = "record_store_staging_0"]
static mut STAGING0: [u8; 0x20000] = [0; 0x20000];
#[export_name = "record_store_staging_1"]
static mut STAGING1: [u8; 0x20000] = [0; 0x20000];
#[export_name = "record_store_staging_2"]
static mut STAGING2: [u8; 0x20000] = [0; 0x20000];
#[export_name = "record_store_staging_3"]
static mut STAGING3: [u8; 0x20000] = [0; 0x20000];
#[export_name = "record_store_staging_4"]
static mut STAGING4: [u8; 0x20000] = [0; 0x20000];
struct Backend { banks: [Vec<u8>; 2], held: bool, reads: usize, writes: usize }
static BACKEND: Mutex<Option<Backend>> = Mutex::new(None);
#[export_name = "record_raw_region_read"]
unsafe extern "C" fn read(address: u32, buffer: *mut u8) -> i32 {
    let mut backend = BACKEND.lock().unwrap();
    let backend = backend.as_mut().unwrap();
    assert!(backend.held);
    backend.reads += 1;
    let bank = ((address >> 17) & 1) as usize;
    slice::from_raw_parts_mut(buffer, BLOCK_BYTES).copy_from_slice(&backend.banks[bank]);
    0
}
unsafe extern "C" fn write(buffer: *const u8, bank: u32, offset: u32, length: u32) -> i32 {
    let mut backend = BACKEND.lock().unwrap();
    let backend = backend.as_mut().unwrap();
    assert!(!backend.held);
    backend.writes += 1;
    for (dest, source) in backend.banks[bank as usize][offset as usize..].iter_mut().zip(slice::from_raw_parts(buffer, length as usize)) { *dest &= source; }
    0
}
unsafe extern "C" fn erase(bank: u32) -> i32 {
    let mut backend = BACKEND.lock().unwrap();
    let backend = backend.as_mut().unwrap();
    assert!(!backend.held);
    backend.banks[bank as usize].fill(0xff);
    0
}
macro_rules! write_import { ($name:ident, $symbol:literal) => { #[export_name = $symbol] unsafe extern "C" fn $name(buffer: *const u8, bank: u32, offset: u32, length: u32) -> i32 { write(buffer, bank, offset, length) } }; }
macro_rules! erase_import { ($name:ident, $symbol:literal) => { #[export_name = $symbol] unsafe extern "C" fn $name(bank: u32) -> i32 { erase(bank) } }; }
write_import!(write0, "record_raw_write_0"); write_import!(write1, "record_raw_write_1"); write_import!(write2, "record_raw_write_2"); write_import!(write3, "record_raw_write_3"); write_import!(write4, "record_raw_write_4");
erase_import!(erase0, "record_raw_erase_0"); erase_import!(erase1, "record_raw_erase_1"); erase_import!(erase2, "record_raw_erase_2"); erase_import!(erase3, "record_raw_erase_3"); erase_import!(erase4, "record_raw_erase_4");
#[export_name = "record_semaphore_take"]
unsafe extern "C" fn take(id: u32, timeout: i32) -> i32 { assert_eq!((id, timeout), (0x4b, -1)); let mut backend = BACKEND.lock().unwrap(); let backend = backend.as_mut().unwrap(); assert!(!backend.held); backend.held = true; 0 }
#[export_name = "record_semaphore_give"]
unsafe extern "C" fn give(id: u32) -> i32 { assert_eq!(id, 0x4b); let mut backend = BACKEND.lock().unwrap(); let backend = backend.as_mut().unwrap(); assert!(backend.held); backend.held = false; 0 }

#[test]
fn rsc_nine_guest_entries_status_outputs_and_read_only_semaphore() {
    unsafe {
        INITIALIZED = 0;
        *BACKEND.lock().unwrap() = Some(Backend { banks: [vec![0xff; BLOCK_BYTES], vec![0xff; BLOCK_BYTES]], held: false, reads: 0, writes: 0 });
        let register = |client: *mut *mut Wrapper| record_store_register(0, 0x60, 6, 0, None, ptr::null_mut(), None, ptr::null_mut(), client);
        let mut client = 0x1234usize as *mut Wrapper;
        assert_eq!(register(&mut client), -14);
        assert!(client.is_null());
        assert_eq!(record_store_open(0), 0);
        assert_eq!(record_store_open(0), -14);
        assert_eq!(register(&mut client), 0);
        assert_eq!(client, ptr::addr_of_mut!(WRAPPERS).cast::<Wrapper>());
        let mut output = 0x1234usize as *const u8;
        let mut skipped = 0x9876;
        assert_eq!(record_store_read_latest(client, &mut skipped, &mut output), -255);
        assert_eq!(output as usize, 0x1234);
        assert_eq!(skipped, 0x9876);
        assert_eq!(record_store_append(client, 0x9234, [0xa5u8; 0x60].as_ptr()), 0);
        assert_eq!(record_store_read_latest(client, &mut skipped, &mut output), 0);
        assert_eq!(output, ptr::addr_of!(STAGING0).cast::<u8>());
        assert_eq!(slice::from_raw_parts(output, 3), &[0x34, 0x92, 0xa5]);
        let mut collected = [0x71; 0x240];
        let mut count = 0;
        assert_eq!(record_store_collect(client, collected.as_mut_ptr(), &mut count), 0);
        assert_eq!(count, 0x60);
        let mut bytes = [0x71; 4];
        assert_eq!(record_store_read_at(0, 0x40000, 4, bytes.as_mut_ptr()), -30);
        assert_eq!(bytes, [0, 0x71, 0x71, 0x71]);
        bytes.fill(0x71);
        assert_eq!(record_store_read_at(0, 0x40000, 0, bytes.as_mut_ptr()), -30);
        assert_eq!(bytes, [0, 0x71, 0x71, 0x71]);
        assert_eq!(record_store_erase_all(client), 0);
        record_store_close(client);
        assert!(!client.is_null());
        assert_eq!(record_store_read_latest(client, ptr::null_mut(), &mut output), -14);
        assert!(output.is_null());
        let sentinel = client;
        assert_eq!(record_store_register(0, 1, 6, 0, None, ptr::null_mut(), None, ptr::null_mut(), &mut client), -30);
        assert_eq!(client, sentinel);
        assert_eq!(register(&mut client), 0);
        assert_eq!(client, sentinel);
        assert_eq!(record_store_zero(), 0);
        let backend = BACKEND.lock().unwrap();
        let backend = backend.as_ref().unwrap();
        assert!(backend.reads > 0);
        assert_eq!(backend.writes, 1);
        assert!(!backend.held);
    }
}
