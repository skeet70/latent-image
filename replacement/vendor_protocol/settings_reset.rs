use crate::{backend::{BackendError, ConfigImages}, extra_configuration::ExtraConfiguration};
use xm1_settings_storage_core::{custom::{CustomCommitOutcome, CustomDefaultField, CustomDefaultsConfiguration, CustomRecordBackend, CustomSettingsLifecycle}, mirror::{BackupDiagnostic, BackupSemaphore, DriverObservation, HeaderBackupDriver, SharedBacking}};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResetMode { Ordinary, OrdinaryAndCustom }
pub struct OrdinaryDefaults<'a> { pub header: &'a [u8; 16], pub body: &'a [u8; 0x118] }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SavedCounters { pub image: u16, pub folder: u16 }
impl SavedCounters {
    pub fn from_area2(record: &[u8; 0x200]) -> Self {
        Self { image: u16::from_le_bytes([record[0x26], record[0x27]]), folder: u16::from_le_bytes([record[0x24], record[0x25]]) }
    }
}
pub struct ResetOutcome { pub ordinary: [Option<DriverObservation>; 16], pub custom: Option<CustomCommitOutcome> }

pub fn reset<D: HeaderBackupDriver>(
    mode: ResetMode,
    backing: &mut SharedBacking<'_, D>,
    defaults: OrdinaryDefaults<'_>,
    counters: SavedCounters,
    configuration: &ConfigImages<'_>,
    custom: &mut CustomSettingsLifecycle<'_>,
    custom_backend: &mut impl CustomRecordBackend,
    extra: &mut ExtraConfiguration<'_>,
    scratch: &mut [u8; 0x128],
) -> Result<ResetOutcome, BackendError> {
    let mut inputs = [0; 6];
    for (index, field) in CustomDefaultField::ORDER.iter().enumerate() { inputs[index] = configuration.byte(field.configuration_offset()).ok_or(BackendError::Capacity)?; }
    let model = configuration.byte(0x215).ok_or(BackendError::Capacity)?;
    let option = configuration.byte(0x216).ok_or(BackendError::Capacity)?;
    let language = configuration.byte(0x20b).ok_or(BackendError::Capacity)?;
    let network = configuration.byte(0x9763).ok_or(BackendError::Capacity)?;
    let mut network_name = [0; 32];
    for (index, value) in network_name.iter_mut().enumerate() { *value = configuration.byte(0x9742+index as u32).ok_or(BackendError::Capacity)?; }
    scratch[..16].copy_from_slice(defaults.header);
    scratch[16..].copy_from_slice(defaults.body);
    scratch[0x78] = (scratch[0x78] & 0x9f) | (if model == 1 || model == 2 { model } else { 0 } << 5);
    scratch[0x5e] = (scratch[0x5e] & 0x7f) | (u8::from(option == 1) << 7);
    scratch[0x21] = (scratch[0x21] & 0xc0) | if language > 34 { 1 } else { language };
    scratch[0x64] = (scratch[0x64] & 0x7f) | (u8::from(scratch[0x21] & 63 == 0) << 7);
    for (index, field) in CustomDefaultField::ORDER.iter().enumerate() {
        let offset = match field { CustomDefaultField::Mode => 0x6f, CustomDefaultField::Enabled => 0x46, CustomDefaultField::Style => 0x7d, CustomDefaultField::FirstChoice => 0x58, CustomDefaultField::SecondChoice => 0x57, CustomDefaultField::ThirdChoice => 0x59 };
        scratch[offset] = field.decode(inputs[index]).apply(scratch[offset]);
    }
    scratch[0x29] = (scratch[0x29] & 0xe0) | (network & 31);
    scratch[0xa0..0xc0].copy_from_slice(&network_name);
    scratch[0xc0..0xe8].fill(0);
    scratch[..2].copy_from_slice(&counters.image.to_le_bytes());
    scratch[2..4].copy_from_slice(&counters.folder.to_le_bytes());
    let task = backing.driver_mut().task();
    match &task {
        Some(task) => backing.driver_mut().acquire(task, BackupSemaphore::Settings),
        None => backing.driver_mut().diagnostic(BackupDiagnostic::OutsideTask),
    }
    backing.live_mut()[..0x128].copy_from_slice(scratch);
    let mut ordinary = [None; 16];
    backing.synchronize_into(&mut ordinary);
    let custom = if mode == ResetMode::OrdinaryAndCustom {
        *custom.live_mut() = CustomDefaultsConfiguration::from_abi(inputs).payload();
        Some(custom.commit(custom_backend))
    } else { None };
    extra.reset_all();
    if let Some(task) = &task { backing.driver_mut().release(task, BackupSemaphore::Settings); }
    Ok(ResetOutcome { ordinary, custom })
}

pub fn reset_counters<D: HeaderBackupDriver>(backing: &mut SharedBacking<'_, D>, area2: &mut [u8; 0x200]) -> [Option<DriverObservation>; 16] {
    let task = backing.driver_mut().task();
    match &task {
        Some(task) => backing.driver_mut().acquire(task, BackupSemaphore::Settings),
        None => backing.driver_mut().diagnostic(BackupDiagnostic::OutsideTask),
    }
    let live = backing.live_mut();
    live[0x128..0x140].fill(0);
    live[..2].copy_from_slice(&0u16.to_le_bytes());
    live[2..4].copy_from_slice(&100u16.to_le_bytes());
    live[0x64] = (live[0x64] & 0x7f) | (u8::from(live[0x21] & 63 == 0) << 7);
    let mut observations = [None; 16];
    backing.synchronize_into(&mut observations);
    let live = backing.live();
    area2[0x24..0x26].copy_from_slice(&live[2..4]);
    area2[0x26..0x28].copy_from_slice(&live[..2]);
    for index in 0..4 {
        let source = 0x128 + index * 6;
        let target = 4 + index * 8;
        area2[target..target+6].copy_from_slice(&live[source..source+6]);
    }
    if let Some(task) = &task { backing.driver_mut().release(task, BackupSemaphore::Settings); }
    observations
}
