use crate::mirror::{
    BackupDiagnostic, BackupDriver, HeaderBackupDriver, HeaderBackend, BackupKey, BackupSemaphore, BackupValue,
    BackupWriteMode, DeviceStatus, ExecutionContext, MirrorOutcome, RequestDirection,
    RequestDescriptor, RequestOutcome, RequestStatus, SettingsBackend, SharedBacking, BackingRange, BACKING_BYTES,
    CUSTOM_BYTES, HEADER_BYTES,
};
#[cfg(settings_storage_firmware)]
extern "C" {
    fn xm1_storage_context() -> u32;
    fn xm1_storage_acquire(semaphore: u32, timeout: u32) -> i32;
    fn xm1_storage_release(semaphore: u32) -> i32;
    fn xm1_storage_diagnostic(class: u32, code: u32) -> i32;
    fn xm1_storage_write(key: u32, value: u32, mode: u32) -> i32;
    fn xm1_storage_report(status: i32);
    fn xm1_storage_status32(value: u32);
    #[cfg(not(settings_storage_header_only))]
    fn xm1_bank_custom_refresh();
}

// X-M1 1.50 image4, 0x7cb7d4..0x7cb7f4.
const REGISTER_IDENTIFIERS: [BackupKey; HEADER_BYTES] = [
    BackupKey(0x0f), BackupKey(0x10), BackupKey(0x11), BackupKey(0x12),
    BackupKey(0x13), BackupKey(0x14), BackupKey(0x15), BackupKey(0x16),
    BackupKey(0x17), BackupKey(0x18), BackupKey(0x19), BackupKey(0x1a),
    BackupKey(0x1b), BackupKey(0x1c), BackupKey(0x1d), BackupKey(0x1e),
];

pub(crate) enum FullBackup {}
enum HeaderOnly {}
pub(crate) struct NativeDriver<Capability>(core::marker::PhantomData<Capability>);
impl<Capability> NativeDriver<Capability> {
    pub(crate) fn new() -> Self { Self(core::marker::PhantomData) }
}
pub(crate) type NativeBackupDriver = NativeDriver<FullBackup>;
type NativeHeaderDriver = NativeDriver<HeaderOnly>;

impl<Capability> HeaderBackupDriver for NativeDriver<Capability> {
    fn context(&mut self) -> ExecutionContext {
        let task = unsafe {
            #[cfg(settings_storage_firmware)]
            { xm1_storage_context() }
            #[cfg(not(settings_storage_firmware))]
            { core::mem::transmute::<usize, unsafe extern "C" fn() -> u32>(0x723cb8)() }
        };
        if task == 0 { ExecutionContext::OutsideTask } else { ExecutionContext::Task }
    }

    fn acquire(&mut self, semaphore: BackupSemaphore) {
        unsafe {
            #[cfg(settings_storage_firmware)]
            { xm1_storage_acquire(semaphore.legacy_id(), u32::MAX); }
            #[cfg(not(settings_storage_firmware))]
            { core::mem::transmute::<usize, unsafe extern "C" fn(u32, u32) -> i32>(0x724b68)(semaphore.legacy_id(), u32::MAX); }
        }
    }

    fn release(&mut self, semaphore: BackupSemaphore) {
        unsafe {
            #[cfg(settings_storage_firmware)]
            { xm1_storage_release(semaphore.legacy_id()); }
            #[cfg(not(settings_storage_firmware))]
            { core::mem::transmute::<usize, unsafe extern "C" fn(u32) -> i32>(0x724c00)(semaphore.legacy_id()); }
        }
    }

    fn diagnostic(&mut self, diagnostic: BackupDiagnostic) {
        let (class, code) = diagnostic.legacy_arguments();
        unsafe {
            #[cfg(settings_storage_firmware)]
            { xm1_storage_diagnostic(class, code); }
            #[cfg(not(settings_storage_firmware))]
            { core::mem::transmute::<usize, unsafe extern "C" fn(u32, u32) -> i32>(0x726c74)(class, code); }
        }
    }

    fn admit(&mut self, _: RequestDirection) -> Result<(), DeviceStatus> {
        #[cfg(storage_fixture_refuse_requests)]
        { Err(DeviceStatus(-30)) }
        #[cfg(not(storage_fixture_refuse_requests))]
        { Ok(()) }
    }

    fn write_backup(&mut self, key: BackupKey, value: BackupValue, mode: BackupWriteMode) -> DeviceStatus {
        DeviceStatus(unsafe {
            #[cfg(settings_storage_firmware)]
            { xm1_storage_write(u32::from(key.0), u32::from(value.0), mode.legacy_argument()) }
            #[cfg(not(settings_storage_firmware))]
            { core::mem::transmute::<usize, unsafe extern "C" fn(u32, u32, u32) -> i32>(0x6a59b8)(u32::from(key.0), u32::from(value.0), mode.legacy_argument()) }
        })
    }


    fn store_request_status(&mut self, status: RequestStatus) {
        #[cfg(settings_storage_firmware)]
        unsafe { xm1_storage_status32(status.legacy_code() as u32); }
        #[cfg(not(settings_storage_firmware))]
        unsafe { core::ptr::write_volatile(0x89edc4 as *mut i32, status.legacy_code()); }
    }

    fn report_failure(&mut self, status: RequestStatus) {
        unsafe {
            #[cfg(settings_storage_firmware)]
            { xm1_storage_report(status.legacy_code()); }
            #[cfg(not(settings_storage_firmware))]
            { core::mem::transmute::<usize, unsafe extern "C" fn(i32)>(0x633594)(status.legacy_code()); }
        }
    }
}

#[cfg(not(settings_storage_header_only))]
impl BackupDriver for NativeBackupDriver {
    fn refresh_custom(&mut self, _: &[u8; CUSTOM_BYTES]) {
        unsafe {
            #[cfg(settings_storage_firmware)]
            { xm1_bank_custom_refresh(); }
            #[cfg(not(settings_storage_firmware))]
            { core::mem::transmute::<usize, unsafe extern "C" fn()>(0x6a86c4)(); }
        }
    }
}

#[inline(always)]
unsafe fn shared_backing<'a, D: HeaderBackupDriver>(driver: D) -> SharedBacking<'a, D> {
    let live = unsafe { &mut *(0x409c281c as *mut [u8; BACKING_BYTES]) };
    let shadow = unsafe { &mut *(0x409c29b8 as *mut [u8; HEADER_BYTES]) };
    SharedBacking::without_export_residue(live, shadow, &REGISTER_IDENTIFIERS, driver)
}

#[cfg(not(settings_storage_header_only))]
#[inline(always)]
pub(crate) unsafe fn with_backend<T>(operation: impl FnOnce(&mut SharedBacking<'_, NativeBackupDriver>) -> T) -> T {
    let mut backing = unsafe { shared_backing(NativeBackupDriver::new()) };
    operation(&mut backing)
}

pub struct NativeHeaderBackend { _exclusive: () }
impl NativeHeaderBackend {
    /// # Safety
    /// The backup windows and callbacks must be mapped and exclusively accessible while this backend exists.
    pub unsafe fn adopt() -> Self { Self { _exclusive: () } }
}
impl HeaderBackend for NativeHeaderBackend {
    #[inline(always)]
    fn read_header(&mut self, destination: &mut [u8; HEADER_BYTES]) -> RequestOutcome {
        #[cfg(storage_defect_mirror_private_copy_diverges)]
        let stale = *destination;
        let outcome = unsafe { shared_backing(NativeHeaderDriver::new()).read_header(destination) };
        #[cfg(storage_defect_mirror_private_copy_diverges)]
        for (index, value) in stale.iter().enumerate() { crate::memory::write_output_byte(destination, index, *value); }
        outcome
    }
    #[inline(always)]
    fn write_header(&mut self, source: &[u8; HEADER_BYTES]) -> MirrorOutcome {
        unsafe { shared_backing(NativeHeaderDriver::new()).write_header(source) }
    }
}

#[cfg(not(settings_storage_header_only))]
pub struct NativeBackend { _exclusive: () }
#[cfg(not(settings_storage_header_only))]
impl NativeBackend {
    /// # Safety
    /// The requirements of `NativeHeaderBackend::adopt` apply.
    pub unsafe fn adopt() -> Self { Self { _exclusive: () } }
}
#[cfg(not(settings_storage_header_only))]
impl HeaderBackend for NativeBackend {
    #[inline(always)]
    fn read_header(&mut self, destination: &mut [u8; HEADER_BYTES]) -> RequestOutcome {
        NativeHeaderBackend { _exclusive: () }.read_header(destination)
    }
    #[inline(always)]
    fn write_header(&mut self, source: &[u8; HEADER_BYTES]) -> MirrorOutcome {
        NativeHeaderBackend { _exclusive: () }.write_header(source)
    }
}
#[cfg(not(settings_storage_header_only))]
impl SettingsBackend for NativeBackend {
    #[inline(always)]
    fn read_body(&mut self, destination: &mut [u8; crate::mirror::BODY_BYTES]) -> RequestOutcome {
        unsafe { shared_backing(NativeBackupDriver::new()).read_body(destination) }
    }
    #[inline(always)]
    fn write_body(&mut self, source: &[u8; crate::mirror::BODY_BYTES]) -> crate::BodyOutcome {
        unsafe { shared_backing(NativeBackupDriver::new()).write_body(source) }
    }
    #[inline(always)]
    fn read_custom(&mut self, destination: &mut [u8; CUSTOM_BYTES]) -> RequestOutcome {
        unsafe { shared_backing(NativeBackupDriver::new()).read_custom(destination) }
    }
}

#[cfg(any(storage_defect_application_appends_bank_as_area3, storage_defect_close_appends_area3))]
pub(crate) unsafe fn append_bank_as_record(source: *const u8) {
    let handle = unsafe { core::ptr::read_volatile(0x89f86c as *const *mut u8) };
    #[cfg(settings_bank_native)]
    unsafe {
        #[repr(transparent)]
        struct RecordStoreHandle(*mut core::ffi::c_void);
        extern "C" { fn record_store_append(handle: RecordStoreHandle, seed: u32, source: *const u8) -> i32; }
        record_store_append(RecordStoreHandle(handle.cast()), 0, source);
    }
    #[cfg(not(settings_bank_native))]
    unsafe {
        extern "C" { fn xm1_bank_record_append(handle: *mut u8, seed: u32, source: *const u8) -> i32; }
        xm1_bank_record_append(handle, 0, source);
    }
}

#[cfg(not(settings_storage_header_only))]
/// # Safety
/// The fixed X-M1 backing and callbacks must be mapped. `header` must name 16 writable bytes disjoint from that backing. Access to both windows must be exclusive during this call.
pub unsafe fn open(header: *mut u8) -> RequestOutcome {
    let header = unsafe { &mut *header.cast::<[u8; HEADER_BYTES]>() };
    #[cfg(storage_defect_mirror_private_copy_diverges)]
    let stale_header = *header;
    let outcome = unsafe { with_backend(|backing| backing.read_header(header)) };
    #[cfg(storage_defect_mirror_private_copy_diverges)]
    { *header = stale_header; }
    outcome
}

#[cfg(not(settings_storage_header_only))]
/// # Safety
/// The fixed X-M1 backing and callbacks must be mapped. `header` must name 16 readable bytes disjoint from that backing. Access to the backing must be exclusive during this call.
pub unsafe fn close(header: *const u8) -> MirrorOutcome {
    let header = unsafe { &*header.cast::<[u8; HEADER_BYTES]>() };
    unsafe { with_backend(|backing| backing.write_header(header)) }
}

#[cfg(not(settings_storage_header_only))]
/// # Safety
/// The requirements of `open` apply.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_storage_open(header: *mut u8) -> i32 {
    unsafe { open(header) }.mapped.legacy_code()
}

#[cfg(not(settings_storage_header_only))]
/// # Safety
/// The requirements of `close` apply.
/// With `storage_defect_checksum_written_into_bank`, the header must also be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_storage_close(header: *const u8) -> i32 {
    let outcome = unsafe { close(header) };
    #[cfg(storage_defect_checksum_written_into_bank)]
    unsafe {
        let checksum = core::ptr::read_volatile(0x409c282b as *const u8);
        core::ptr::write(header.cast_mut().add(15), checksum);
    }
    #[cfg(storage_defect_close_appends_area3)]
    unsafe {
        core::mem::transmute::<usize, unsafe extern "C" fn(u32, u32, u32) -> i32>(0x6a86c4)(0, 0, 0);
    }
    outcome.request.mapped.legacy_code()
}

#[cfg(not(settings_storage_header_only))]
/// # Safety
/// The fixed X-M1 backing and callbacks must be mapped. `buffer` must name `length` writable bytes disjoint from that backing. Access to both windows must be exclusive during this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_storage_export(buffer: *mut u8, length: u32, offset: u32) -> i32 {
    let destination = unsafe { core::slice::from_raw_parts_mut(buffer, length as usize) };
    unsafe { with_backend(|backing| backing.export(BackingRange::new(offset, length), destination)) }.legacy_code()
}

#[cfg(not(settings_storage_header_only))]
/// # Safety
/// The fixed X-M1 backing and callbacks must be mapped. `buffer` must name `length` readable bytes disjoint from that backing. Access to the backing must be exclusive during this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_storage_import(buffer: *const u8, length: u32, offset: u32) -> i32 {
    let source = unsafe { core::slice::from_raw_parts(buffer, length as usize) };
    unsafe { with_backend(|backing| backing.import(BackingRange::new(offset, length), source)) }.status.legacy_code()
}

#[cfg(not(settings_storage_header_only))]
#[repr(C)]
pub struct AbiRequestDescriptor {
    pub buffer: u32,
    pub length: u32,
    pub offset: u32,
}

#[cfg(not(settings_storage_header_only))]
fn short_request_result(underlying: i32) -> i32 {
    let mapped = if underlying == 0 { RequestStatus::Success } else { RequestStatus::Failed };
    mapped.legacy_code()
}

#[cfg(not(settings_storage_header_only))]
/// # Safety
/// `descriptor` must point to a readable ABI descriptor. Its buffer and the fixed backing must satisfy `xm1_storage_export` for the low halfwords of length and offset.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_storage_read_request(descriptor: *const AbiRequestDescriptor) -> i32 {
    let descriptor = unsafe { core::ptr::read(descriptor) };
    let range = RequestDescriptor::from_abi(descriptor.length, descriptor.offset).range();
    short_request_result(unsafe {
        xm1_storage_export(descriptor.buffer as usize as *mut u8, range.length(), range.offset())
    })
}

#[cfg(not(settings_storage_header_only))]
/// # Safety
/// `descriptor` must point to a readable ABI descriptor. Its buffer and the fixed backing must satisfy `xm1_storage_import` for the low halfwords of length and offset.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_storage_write_request(descriptor: *const AbiRequestDescriptor) -> i32 {
    let descriptor = unsafe { core::ptr::read(descriptor) };
    let range = RequestDescriptor::from_abi(descriptor.length, descriptor.offset).range();
    short_request_result(unsafe {
        xm1_storage_import(descriptor.buffer as usize as *const u8, range.length(), range.offset())
    })
}
