use crate::custom::*;
use crate::contract_violation;
use core::{ptr, slice};
use record_store::{self as store, Callbacks};

#[derive(Clone, Copy)]
struct CustomAddress(u32);
const PREVIOUS: CustomAddress = CustomAddress(0x89f838);
const PREVIOUS_PAYLOAD: CustomAddress = CustomAddress(0x89f83c);
const WORKING: CustomAddress = CustomAddress(0x89f874);
const WORKING_PAYLOAD: CustomAddress = CustomAddress(0x89f878);
const LIVE: CustomAddress = CustomAddress(0x409c295c);
const RESET_FLAG: CustomAddress = CustomAddress(0x89f834);
const WRAPPER: CustomAddress = CustomAddress(0x89f86c);

#[repr(C)]
struct Wrapper { active: u32, banks: u32, slot: *mut store::Area }
type Prepare = unsafe extern "C" fn(*mut u8, *const u32);
type Check = unsafe extern "C" fn(*mut u8, *const u32) -> i32;
extern "C" {
    fn record_store_register(kind: u32, size: u32, total: u32, sequence: u32, prepare: Option<Prepare>, prepare_context: *const u32, check: Option<Check>, check_context: *const u32, client: *mut *mut Wrapper) -> i32;
    fn settings_custom_config_read(offset: u32) -> u8;
    fn settings_custom_reset_modes(mode: u32) -> i32;
    fn settings_custom_store8(address: u32, value: u32);
    fn settings_custom_store16(address: u32, value: u32);
    fn settings_custom_store32(address: u32, value: u32);
}
enum InitialRead { Boot, Retained }
// Native lifecycle entry points materialize working payloads before change detection.
struct Backend<'a> { area: &'a mut store::Area, initial: InitialRead }
impl Backend<'_> {
    fn read_with_recovery(&mut self, recovery: Option<&mut u16>) -> ReloadOutcome {
        unsafe { store::with_guest_entry(|entry| match entry.read_latest(self.area, &mut CustomCodec, recovery) {
            store::Latest::Found { candidate, record } => ReloadOutcome::Latest { candidate, record: CustomSettingsRecord::from_working_wire(record.try_into().unwrap_or_else(|_| contract_violation())) },
            store::Latest::Empty => ReloadOutcome::Empty,
            store::Latest::NoneValid { output } => ReloadOutcome::NoneValid { output },
        }) }
    }
}
impl CustomRecordBackend for Backend<'_> {
    fn read_latest(&mut self) -> ReloadOutcome {
        let initial = core::mem::replace(&mut self.initial, InitialRead::Retained);
        let mut recovery = 0;
        let outcome = self.read_with_recovery(if matches!(initial, InitialRead::Boot) { Some(&mut recovery) } else { None });
        unsafe {
            match outcome {
                ReloadOutcome::Latest { record, .. } => store_halfwords(PREVIOUS, record.wire()),
                _ if matches!(initial, InitialRead::Boot) => {
                    for offset in (0..CUSTOM_RECORD_BYTES).step_by(4) { settings_custom_store32(PREVIOUS.0 + offset as u32, u32::MAX); }
                },
                _ => {}
            }
            if matches!(initial, InitialRead::Retained) && !cfg!(custom_control_omit_sequence_reload) {
                copy_halfwords::<2>(WORKING, PREVIOUS);
            }
        }
        outcome
    }
    fn append(&mut self, _record: &CustomSettingsRecord) -> Result<store::Appended, store::NotAppended> {
        if cfg!(custom_control_record_append_omitted) {
            return Err(store::NotAppended::DestinationFull { erase: None });
        }
        unsafe {
            settings_custom_append(self.area, &*(WORKING.0 as *const CustomSettingsRecord))
        }
    }
}
#[no_mangle]
#[inline(never)]
fn settings_custom_append(area: &mut store::Area, record: &CustomSettingsRecord) -> Result<store::Appended, store::NotAppended> {
    unsafe { store::with_guest_entry(|entry| entry.append(area, &mut CustomCodec, store::Seed(0), record.wire())) }
}
unsafe fn backend<'a>() -> Option<Backend<'a>> {
    let wrapper = (WRAPPER.0 as *const *mut Wrapper).read();
    if wrapper.is_null() || (*wrapper).active == 0 || (*wrapper).slot.is_null() { None }
    else { Some(Backend { area: &mut *(*wrapper).slot, initial: InitialRead::Retained }) }
}
unsafe fn store_halfwords<const N: usize>(address: CustomAddress, bytes: &[u8; N]) {
    const { assert!(N % 2 == 0); }
    for (index, pair) in bytes.chunks_exact(2).enumerate() {
        let [low, high] = pair else { contract_violation() };
        settings_custom_store16(address.0 + (index * 2) as u32, u32::from(u16::from_le_bytes([*low, *high])));
    }
}
unsafe fn copy_halfwords<const N: usize>(destination: CustomAddress, source: CustomAddress) {
    const { assert!(N % 2 == 0); }
    for offset in (0..N).step_by(2) {
        let value = ((source.0 + offset as u32) as *const u16).read_volatile();
        settings_custom_store16(destination.0 + offset as u32, u32::from(value));
    }
}
struct Images { previous: CustomSettingsRecord, working: CustomSettingsRecord, live: CustomPayload }
impl Images {
    unsafe fn read() -> Self {
        Self { previous: (PREVIOUS.0 as *const CustomSettingsRecord).read(), working: (WORKING.0 as *const CustomSettingsRecord).read(), live: (LIVE.0 as *const CustomPayload).read() }
    }
    fn lifecycle(&mut self) -> CustomSettingsLifecycle<'_> { CustomSettingsLifecycle::adopt(&mut self.previous, &mut self.working, &mut self.live) }
}
unsafe fn defaults() -> CustomPayload {
    store_halfwords(LIVE, crate::SlotDefaultsWindow::slot(crate::ProfileSelection::Record0).bytes());
    for field in CustomDefaultField::ORDER {
        let offset = field.configuration_offset();
        let first = settings_custom_config_read(offset);
        let input = if matches!(field, CustomDefaultField::FirstChoice | CustomDefaultField::SecondChoice | CustomDefaultField::ThirdChoice)
            && !matches!(first, 0..=10 | 0x85..=0x88) {
            if settings_custom_config_read(offset) & 0x80 != 0 { 0x80 } else { 11 }
        } else { first };
        if !cfg!(custom_control_static_defaults) {
            let address = LIVE.0 + field.byte_offset() as u32;
            let previous = (address as *const u8).read_volatile();
            settings_custom_store8(address, u32::from(field.decode(input).apply(previous)));
        }
    }
    (LIVE.0 as *const CustomPayload).read()
}

#[no_mangle]
pub unsafe extern "C" fn settings_custom_prepare(record: *mut u8, _: *const u32) { CustomCodec.seal(slice::from_raw_parts_mut(record, 52)); }
#[no_mangle]
pub unsafe extern "C" fn settings_custom_check(record: *mut u8, _: *const u32) -> i32 {
    settings_custom_codec_check(record, CUSTOM_RECORD_BYTES as u32)
}
#[no_mangle]
pub unsafe extern "C" fn settings_custom_defaults() -> i32 { defaults(); 0 }
#[no_mangle]
pub unsafe extern "C" fn settings_custom_read(output: *mut u8, recovery: *mut u16) -> i32 {
    let Some(mut backend) = backend() else { return -14 };
    let recovery = if cfg!(custom_control_drop_recovery) { None } else { recovery.as_mut() };
    match backend.read_with_recovery(recovery) {
        ReloadOutcome::Latest { record, .. } => { ptr::copy_nonoverlapping(record.wire().as_ptr(), output, 52); 0 },
        _ => -255,
    }
}
#[no_mangle]
pub unsafe extern "C" fn settings_custom_refresh() -> i32 {
    let Some(mut backend) = backend() else { return -14 };
    copy_halfwords::<CUSTOM_PAYLOAD_BYTES>(WORKING_PAYLOAD, LIVE);
    let mut images = Images::read();
    let outcome = images.lifecycle().commit(&mut backend);
    outcome.retained_abi_status()
}

pub unsafe fn boot(prerequisites: &mut StartupPrerequisites) -> Result<CustomBootOutcome, store::RegistrationError> {
    let mut wrapper = ptr::null_mut();
    let registered = record_store_register(3, 52, 254, 0, Some(settings_custom_prepare), 0x828570 as *const u32, Some(settings_custom_check), 0x828570 as *const u32, &mut wrapper);
    settings_custom_store32(WRAPPER.0, wrapper as u32);
    if registered != 0 { return Err(store::RegistrationError::Closed); }
    let Some(mut backend) = backend() else { return Err(store::RegistrationError::Closed) };
    backend.initial = InitialRead::Boot;
    let mut images = Images::read();
    let outcome = images.lifecycle().boot_with_defaults(prerequisites, || {
        let payload = defaults();
        copy_halfwords::<CUSTOM_PAYLOAD_BYTES>(WORKING_PAYLOAD, LIVE);
        payload
    }, &mut backend, || { settings_custom_reset_modes(0); });
    if prerequisites.custom_reset == CustomReset::Preserve && (RESET_FLAG.0 as *const u32).read() == 1 { settings_custom_store32(RESET_FLAG.0, 0); }
    copy_halfwords::<CUSTOM_RECORD_BYTES>(WORKING, PREVIOUS);
    copy_halfwords::<CUSTOM_PAYLOAD_BYTES>(LIVE, PREVIOUS_PAYLOAD);
    Ok(outcome)
}

#[no_mangle]
pub unsafe extern "C" fn settings_custom_boot() -> i32 {
    let mut prerequisites = StartupPrerequisites {
        area2: Area2Startup::CompletedByRetainedStartup,
        header: HeaderStartup::CompletedByRetainedStartup,
        custom_reset: if (0x89f834 as *const u32).read() == 1 { CustomReset::Reset } else { CustomReset::Preserve },
    };
    match boot(&mut prerequisites) { Ok(_) => 0, Err(_) => -14 }
}
