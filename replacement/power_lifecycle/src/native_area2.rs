use crate::area2::{self, Area2Record, FirmwareVersion, SaveSelector};
use crate::mirror::{BACKING_BYTES, BackupDiagnostic, BackupKey, BackupSemaphore, BackupValue, BackupWriteMode, DeviceStatus, HEADER_BYTES, HeaderBackupDriver, RequestDirection, RequestStatus, SharedBacking};

#[repr(C)]
struct RecordWrapper { _opaque: [u8; 0] }
extern "C" {
    fn record_store_append(wrapper: *mut RecordWrapper, seed: u32, source: *const u8) -> i32;
    fn record_store_close(wrapper: *mut RecordWrapper);
    fn xm1_power_current_task() -> u32;
    fn xm1_power_semaphore_wait(semaphore: i32, timeout: i32) -> i32;
    fn xm1_power_semaphore_signal(semaphore: i32) -> i32;
    fn xm1_power_command_diagnostic(class: u32, code: u32) -> i32;
    fn xm1_power_backup_write_field(index: u32, value: u32, mode: u32) -> i32;
    fn xm1_power_frontend_failure();
}

struct NativeBackupDriver;
impl HeaderBackupDriver for NativeBackupDriver {
    type Task = ();
    fn task(&mut self) -> Option<()> {
        if unsafe { xm1_power_current_task() } == 0 { None } else { Some(()) }
    }
    fn acquire(&mut self, _: &(), semaphore: BackupSemaphore) {
        let id = match semaphore { BackupSemaphore::Settings => 0x25 };
        unsafe { let _ = xm1_power_semaphore_wait(id, -1); }
    }
    fn release(&mut self, _: &(), semaphore: BackupSemaphore) {
        let id = match semaphore { BackupSemaphore::Settings => 0x25 };
        unsafe { let _ = xm1_power_semaphore_signal(id); }
    }
    fn diagnostic(&mut self, diagnostic: BackupDiagnostic) {
        let (class, code) = diagnostic.legacy_arguments();
        unsafe { let _ = xm1_power_command_diagnostic(class, code); }
    }
    fn admit(&mut self, _: RequestDirection) -> Result<(), DeviceStatus> { Ok(()) }
    fn write_backup(&mut self, key: BackupKey, value: BackupValue, mode: BackupWriteMode) -> DeviceStatus {
        DeviceStatus(unsafe { xm1_power_backup_write_field(u32::from(key.0), u32::from(value.0), mode.legacy_argument()) })
    }
    fn store_request_status(&mut self, status: RequestStatus) {
        unsafe { (0x89edc4 as *mut u32).write_volatile(status.legacy_code() as u32); }
    }
    fn report_failure(&mut self, _: RequestStatus) {
        unsafe { xm1_power_frontend_failure(); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_area2_save(selector: u32) -> i32 {
    let keys = core::array::from_fn(|index| BackupKey((0x0f + index) as u16));
    let live = &mut *(0x409c281c as *mut [u8; BACKING_BYTES]);
    let shadow = &mut *(0x409c29b8 as *mut [u8; HEADER_BYTES]);
    let mut backing = SharedBacking::without_export_residue(live, shadow, &keys, NativeBackupDriver);
    let working = &mut *(0x409c2b54 as *mut Area2Record);
    let snapshot = &mut *(0x409c2d54 as *mut Area2Record);
    let version = FirmwareVersion {
        major: (0x7cb0e1 as *const u8).read_volatile(),
        minor: (0x7cb0e7 as *const u8).read_volatile(),
    };
    area2::prepare(working, &mut backing, version);
    let area2 = (0x89f830 as *const *mut RecordWrapper).read_volatile();
    if working.changed_from(snapshot) {
        let _ = record_store_append(area2, 0, working.0.as_ptr());
    }
    match SaveSelector::from_abi(selector) {
        SaveSelector::Snapshot => snapshot.0.copy_from_slice(&working.0),
        SaveSelector::Close => {
            record_store_close(area2);
            record_store_close((0x89f86c as *const *mut RecordWrapper).read_volatile());
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_area2_seal(record: *mut Area2Record, protected_bytes: *const u32) {
    assert_eq!(protected_bytes.read(), 0x1ec);
    crate::area2_codec::seal(&mut *record);
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_area2_check(record: *mut Area2Record, protected_bytes: *const u32) -> i32 {
    if protected_bytes.read() != 0x1ec { return 1; }
    match crate::area2_codec::validate(&mut *record) {
        crate::area2_codec::Validation::Rejected => 1,
        _ => 0,
    }
}
