use super::*;
use core::{ptr, slice};

#[cfg(test)]
#[path = "guest_tests.rs"]
mod tests;

const _: () = assert!(core::mem::size_of::<Service<GuestCallbacks>>() <= 0x168);

extern "C" {
    static mut record_store_initialized: u32;
    static mut record_store_wrappers: [Wrapper; 5];
    static mut record_store_state: u32;
    static mut record_store_block: [u8; BLOCK_BYTES];
    static mut record_store_second_block: [u8; BLOCK_BYTES];
    static mut record_store_cache_tags: [u32; 2];
    static mut record_store_cache_timestamp: u32;
    static mut record_store_cache_ages: [u32; 2];
    static mut record_store_staging_0: u8;
    static mut record_store_staging_1: u8;
    static mut record_store_staging_2: u8;
    static mut record_store_staging_3: u8;
    static mut record_store_staging_4: u8;
    fn record_raw_region_read(address: u32, buffer: *mut u8) -> i32;
    fn record_raw_write_0(buffer: *const u8, bank: u32, offset: u32, length: u32) -> i32;
    fn record_raw_write_1(buffer: *const u8, bank: u32, offset: u32, length: u32) -> i32;
    fn record_raw_write_2(buffer: *const u8, bank: u32, offset: u32, length: u32) -> i32;
    fn record_raw_write_3(buffer: *const u8, bank: u32, offset: u32, length: u32) -> i32;
    fn record_raw_write_4(buffer: *const u8, bank: u32, offset: u32, length: u32) -> i32;
    fn record_raw_erase_0(bank: u32) -> i32;
    fn record_raw_erase_1(bank: u32) -> i32;
    fn record_raw_erase_2(bank: u32) -> i32;
    fn record_raw_erase_3(bank: u32) -> i32;
    fn record_raw_erase_4(bank: u32) -> i32;
    fn record_semaphore_take(id: u32, timeout: i32) -> i32;
    fn record_semaphore_give(id: u32) -> i32;
}
#[repr(C)]
pub(crate) struct Wrapper { active: u32, banks: u32, slot: *mut Slot }
type Seal = unsafe extern "C" fn(*mut u8, *mut u8);
type Check = unsafe extern "C" fn(*mut u8, *mut u8) -> i32;
type Slot = Registration<GuestCallbacks>;
pub struct GuestStaging(());
impl Staging for GuestStaging {
    fn bytes(&mut self, kind: RecordKind) -> &mut [u8] {
        unsafe {
            let pointer = match kind { RecordKind::Kind0 => ptr::addr_of_mut!(record_store_staging_0), RecordKind::Kind1 => ptr::addr_of_mut!(record_store_staging_1), RecordKind::Kind2 => ptr::addr_of_mut!(record_store_staging_2), RecordKind::Kind3 => ptr::addr_of_mut!(record_store_staging_3), RecordKind::Kind4 => ptr::addr_of_mut!(record_store_staging_4) };
            slice::from_raw_parts_mut(pointer, kind.program_bytes().0 as usize)
        }
    }
    fn read_bytes(&mut self, kind: RecordKind, length: usize) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.bytes(kind).as_mut_ptr(), length) }
    }
}
pub struct RawMedium(());
fn status(result: i32) -> Result<(), MediumError> { if result == 0 { Ok(()) } else { Err(MediumError(result)) } }
impl RecordMedium for RawMedium {
    fn read(&mut self, kind: RecordKind, bank: Bank, offset: BytePosition, output: &mut [u8]) -> Result<(), MediumError> {
        unsafe { status(record_raw_region_read(CacheTag::region(kind, bank).0 + offset.0, output.as_mut_ptr())) }
    }
    fn program(&mut self, kind: RecordKind, bank: Bank, offset: BytePosition, bytes: &[u8]) -> Result<(), MediumError> {
        let write = match kind { RecordKind::Kind0 => record_raw_write_0, RecordKind::Kind1 => record_raw_write_1, RecordKind::Kind2 => record_raw_write_2, RecordKind::Kind3 => record_raw_write_3, RecordKind::Kind4 => record_raw_write_4 };
        unsafe { status(write(bytes.as_ptr(), bank as u32, offset.0, bytes.len() as u32)) }
    }
    fn erase(&mut self, kind: RecordKind, bank: Bank) -> Result<(), MediumError> {
        let erase = match kind { RecordKind::Kind0 => record_raw_erase_0, RecordKind::Kind1 => record_raw_erase_1, RecordKind::Kind2 => record_raw_erase_2, RecordKind::Kind3 => record_raw_erase_3, RecordKind::Kind4 => record_raw_erase_4 };
        unsafe { status(erase(bank as u32)) }
    }
}
pub struct SharedBlock(());
impl BlockBuffer<RawMedium> for SharedBlock {
    fn copy_into(&mut self, medium: &mut RawMedium, kind: RecordKind, bank: Bank, offset: usize, output: &mut [u8]) {
        unsafe {
            record_semaphore_take(0x4b, -1);
            // Cache metadata and blocks are borrowed only while semaphore 0x4b is held.
            let tags = &mut *ptr::addr_of_mut!(record_store_cache_tags);
            let ages = &mut *ptr::addr_of_mut!(record_store_cache_ages);
            let timestamp = &mut *ptr::addr_of_mut!(record_store_cache_timestamp);
            let (slot, fill) = select_cache(tags, ages, CacheTag::region(kind, bank));
            let block = if slot == 0 { &mut *ptr::addr_of_mut!(record_store_block) } else { &mut *ptr::addr_of_mut!(record_store_second_block) };
            if fill { let _ = medium.read(kind, bank, BytePosition(0), block); }
            access_cache(ages, timestamp, slot);
            let length = output.len();
            if cfg!(feature = "control-unlock-before-copy") { record_semaphore_give(0x4b); }
            copy(output, part(block, offset, length));
            if !cfg!(feature = "control-unlock-before-copy") { record_semaphore_give(0x4b); }
        }
    }
    fn invalidate(&mut self) {
        unsafe {
            record_semaphore_take(0x4b, -1);
            record_store_cache_tags = [u32::MAX; 2];
            record_semaphore_give(0x4b);
        }
    }
}
#[derive(Clone, Copy)]
#[repr(C)]
struct GuestCallbacks { prepare: Option<Seal>, prepare_context: *mut u8, check: Option<Check>, check_context: *mut u8 }
impl From<Slot> for GuestCallbacks { fn from(slot: Slot) -> Self { slot.callbacks } }
unsafe fn service<'a>() -> &'a mut Service<GuestCallbacks> { &mut *ptr::addr_of_mut!(record_store_state).cast::<Service<GuestCallbacks>>() }
impl Callbacks for GuestCallbacks {
    fn check(&mut self, bytes: &mut [u8]) -> CheckVerdict { if let Some(check) = self.check { if unsafe { check(bytes.as_mut_ptr(), self.check_context) } != 0 { return CheckVerdict::Reject; } } CheckVerdict::Accept }
    fn seal(&mut self, bytes: &mut [u8]) { if let Some(prepare) = self.prepare { unsafe { prepare(bytes.as_mut_ptr(), self.prepare_context); } } }
}
unsafe fn slot<'a>(wrapper: *mut Wrapper) -> Option<&'a mut Slot> {
    if record_store_initialized == 0 || (*wrapper).active == 0 || (*wrapper).slot.is_null() { None } else { Some(&mut *(*wrapper).slot) }
}
fn entry<'a>(medium: &'a mut RawMedium, staging: &'a mut GuestStaging) -> Entry<'a, RawMedium, GuestStaging, SharedBlock> { Entry::with_buffer(medium, staging, SharedBlock(())) }

/// # Safety
/// The caller must exclusively own each area and staging kind used by the operation.
pub unsafe fn with_guest_entry<R>(operation: impl FnOnce(&mut Entry<'_, RawMedium, GuestStaging, SharedBlock>) -> R) -> R {
    operation(&mut entry(&mut RawMedium(()), &mut GuestStaging(())))
}

#[no_mangle]
pub unsafe extern "C" fn record_store_open(_: u32) -> i32 {
    if record_store_initialized != 0 { return -14; }
    for wrapper in &mut *ptr::addr_of_mut!(record_store_wrappers) { wrapper.active = 0; wrapper.slot = ptr::null_mut(); }
    Service::initialize_at(ptr::addr_of_mut!(record_store_state).cast::<Service<GuestCallbacks>>());
    record_store_cache_tags = [u32::MAX; 2];
    record_store_cache_ages = [0; 2];
    record_store_cache_timestamp = 0;
    if service().open(&mut GuestStaging(())).is_err() { return -14; }
    record_store_initialized = 1;
    0
}
#[no_mangle]
pub unsafe extern "C" fn record_store_register(kind: u32, size: u32, total: u32, q: u32, prepare: Option<Seal>, prepare_context: *mut u8, check: Option<Check>, check_context: *mut u8, client: *mut *mut Wrapper) -> i32 {
    if record_store_initialized == 0 { *client = ptr::null_mut(); return -14; }
    let Some(kind) = RecordKind::parse(kind) else { return -30; };
    if size > u16::MAX as u32 || q > u16::MAX as u32 || total > 0x7ffe { return -30; }
    let callbacks = GuestCallbacks { prepare, prepare_context, check, check_context };
    let Ok(handle) = service().register(kind, RecordBytes(size as u16), RecordCount(total as u16), SequenceOffset(q as u16), callbacks) else { return -30; };
    let Some(state) = service().get_mut(handle) else { contract_violation() };
    let wrapper = ptr::addr_of_mut!(record_store_wrappers).cast::<Wrapper>().add(handle.index());
    wrapper.write(Wrapper { active: 1, banks: 2, slot: state });
    *client = wrapper;
    0
}
#[no_mangle]
pub unsafe extern "C" fn record_store_close(wrapper: *mut Wrapper) {
    if slot(wrapper).is_some() {
        let base = ptr::addr_of_mut!(record_store_wrappers).cast::<Wrapper>();
        for index in 0..5 {
            if wrapper == base.add(index) {
                if let Some(handle) = Handle::from_index(index) { service().close(handle); }
                break;
            }
        }
        (*wrapper).active = 0;
        (*wrapper).slot = ptr::null_mut();
    }
}
#[no_mangle]
pub unsafe extern "C" fn record_store_read_latest(wrapper: *mut Wrapper, skipped: *mut u16, output: *mut *const u8) -> i32 {
    let Some(slot) = slot(wrapper) else { *output = ptr::null(); return -14; };
    let mut callbacks = GuestCallbacks::from(*slot);
    let mut medium = RawMedium(());
    let mut staging = GuestStaging(());
    match entry(&mut medium, &mut staging).read_latest(&mut slot.area, &mut callbacks, skipped.as_mut()) {
        Latest::Found { record, .. } => { *output = record.as_ptr(); 0 },
        Latest::Empty => -255,
        Latest::NoneValid { output: effect } => { if effect == FailureOutput::Null { *output = ptr::null(); } -255 },
    }
}
#[no_mangle]
pub unsafe extern "C" fn record_store_append(wrapper: *mut Wrapper, seed: u32, source: *const u8) -> i32 {
    let Some(slot) = slot(wrapper) else { return -14; };
    let mut callbacks = GuestCallbacks::from(*slot);
    let mut medium = RawMedium(());
    let mut staging = GuestStaging(());
    let same_staging = source == staging.bytes(slot.area.kind).as_ptr();
    let source = if same_staging { RecordSource::Staging } else { RecordSource::Caller(slice::from_raw_parts(source, slot.area.geometry.bytes.0 as usize)) };
    match entry(&mut medium, &mut staging).append_source(&mut slot.area, &mut callbacks, Seed(seed as u16), source) { Ok(_) => 0, Err(_) => -255 }
}
#[no_mangle]
pub unsafe extern "C" fn record_store_erase_all(wrapper: *mut Wrapper) -> i32 {
    let Some(slot) = slot(wrapper) else { return -14; };
    let mut medium = RawMedium(());
    let mut staging = GuestStaging(());
    entry(&mut medium, &mut staging).erase_all(&mut slot.area);
    0
}
#[no_mangle]
pub unsafe extern "C" fn record_store_collect(wrapper: *mut Wrapper, output: *mut u8, count: *mut u32) -> i32 {
    let Some(slot) = slot(wrapper) else { return -14; };
    let mut medium = RawMedium(());
    let mut staging = GuestStaging(());
    let output = slice::from_raw_parts_mut(output, slot.area.geometry.bytes.0 as usize * slot.area.geometry.count.0 as usize * 2);
    let bytes = entry(&mut medium, &mut staging).collect(&mut slot.area, output);
    if !count.is_null() { *count = bytes as u32; if bytes == 0 { return -255; } }
    0
}
#[no_mangle]
pub unsafe extern "C" fn record_store_read_at(kind: u32, position: u32, length: u32, output: *mut u8) -> i32 {
    let output = slice::from_raw_parts_mut(output, length as usize);
    if record_store_initialized == 0 { fill(output, 0); return -14; }
    let Some(kind) = RecordKind::parse(kind) else { fill(output, 0); return -30; };
    if position >= 0x40000 { *output.as_mut_ptr() = 0; return -30; }
    let mut medium = RawMedium(());
    let mut staging = GuestStaging(());
    match entry(&mut medium, &mut staging).read_at(kind, BytePosition(position), output) { Ok(()) => 0, Err(_) => -30 }
}
#[no_mangle]
pub extern "C" fn record_store_zero() -> i32 { 0 }

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! { loop { core::hint::spin_loop(); } }
