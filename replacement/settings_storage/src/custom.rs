use area3_validator::{self as validator, Outcome};
use record_store::{self as store, Callbacks};
use crate::{contract_violation, part, part_mut};

pub const CUSTOM_RECORD_BYTES: usize = validator::RECORD_BYTES;
pub const CUSTOM_PAYLOAD_BYTES: usize = 40;

fn copy_value(destination: &mut [u8], source: &[u8]) {
    if destination.len() != source.len() { contract_violation(); }
    for (destination, source) in destination.iter_mut().zip(source) { *destination = *source; }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct CustomPayload(pub [u8; CUSTOM_PAYLOAD_BYTES]);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CustomMetadata(pub [u8; 2]);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct CustomSettingsRecord { wire: [u8; CUSTOM_RECORD_BYTES] }
impl CustomSettingsRecord {
    pub const fn from_working_wire(wire: [u8; CUSTOM_RECORD_BYTES]) -> Self { Self { wire } }
    pub fn wire(&self) -> &[u8; CUSTOM_RECORD_BYTES] { &self.wire }
    pub fn sequence(&self) -> store::Sequence { store::Sequence(u16::from_le_bytes([self.wire[0], self.wire[1]])) }
    pub fn metadata(&self) -> CustomMetadata { CustomMetadata([self.wire[2], self.wire[3]]) }
    pub fn set_metadata(&mut self, metadata: CustomMetadata) { copy_value(part_mut(&mut self.wire, 2, 2), &metadata.0); }
    pub fn payload(&self) -> CustomPayload { CustomPayload(part(&self.wire, 4, CUSTOM_PAYLOAD_BYTES).try_into().unwrap_or_else(|_| contract_violation())) }
    pub fn set_payload(&mut self, payload: &CustomPayload) { copy_value(part_mut(&mut self.wire, 4, CUSTOM_PAYLOAD_BYTES), &payload.0); }
    pub fn changed_from(&self, previous: &Self) -> bool {
        let (start, length) = if cfg!(custom_control_compare_sequence) { (0, 44) }
            else if cfg!(custom_control_skip_metadata) { (4, 40) }
            else if cfg!(custom_control_first_payload_only) { (2, 3) }
            else { (2, 42) };
        part(&self.wire, start, length).iter().zip(part(&previous.wire, start, length)).any(|(left, right)| left != right)
    }
    pub fn validate(mut wire: [u8; CUSTOM_RECORD_BYTES]) -> Result<(Self, Outcome), CustomCodecError> {
        match validate_wire(&mut wire)? {
            Outcome::Rejected(syndrome) => Err(CustomCodecError::Rejected(syndrome)),
            outcome => Ok((Self { wire }, outcome)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomCodecError { RowParityHighBits, Rejected(validator::Syndrome) }
fn validate_wire(wire: &mut [u8; CUSTOM_RECORD_BYTES]) -> Result<Outcome, CustomCodecError> {
    let stored_row = u32::from_le_bytes(part(wire, 44, 4).try_into().unwrap_or_else(|_| contract_violation()));
    if !cfg!(custom_control_accept_high_row_bits) && stored_row & !validator::ROW_PARITY_MASK != 0 { return Err(CustomCodecError::RowParityHighBits); }
    Ok(validator::validate(wire))
}

pub struct CustomCodec;
// Runs under the record-store lock that the callback holds.
#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn settings_custom_codec_check(record: *mut u8, length: u32) -> i32 {
    let Ok(wire) = <&mut [u8; CUSTOM_RECORD_BYTES]>::try_from(core::slice::from_raw_parts_mut(record, length as usize)) else { return 1 };
    match validate_wire(wire) {
        Err(_) | Ok(Outcome::Rejected(_)) if !cfg!(custom_control_accept_rejected) => 1,
        _ => 0,
    }
}
impl Callbacks for CustomCodec {
    fn check(&mut self, record: &mut [u8]) -> store::CheckVerdict {
        match unsafe { settings_custom_codec_check(record.as_mut_ptr(), record.len() as u32) } {
            0 => store::CheckVerdict::Accept,
            _ => store::CheckVerdict::Reject,
        }
    }
    fn seal(&mut self, record: &mut [u8]) {
        if cfg!(custom_control_record_crc_omitted) { return; }
        let wire: &mut [u8; CUSTOM_RECORD_BYTES] = record.try_into().unwrap_or_else(|_| contract_violation());
        part_mut(wire, 44, 8).fill(0);
        let sequence = [wire[0], wire[1]];
        if cfg!(custom_control_seal_before_sequence) { part_mut(wire, 0, 2).fill(0); }
        let parity = validator::computed_parity(wire);
        copy_value(part_mut(wire, 44, 4), &(parity.row.bits() as u32).to_le_bytes());
        copy_value(part_mut(wire, 48, 4), &parity.column.bits().to_le_bytes());
        if cfg!(custom_control_seal_before_sequence) { copy_value(part_mut(wire, 0, 2), &sequence); }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReloadOutcome {
    Latest { candidate: store::Candidate, record: CustomSettingsRecord },
    Empty,
    NoneValid { output: store::FailureOutput },
}
pub trait CustomRecordBackend {
    fn read_latest(&mut self) -> ReloadOutcome;
    fn append(&mut self, record: &CustomSettingsRecord) -> Result<store::Appended, store::NotAppended>;
}

pub struct RecordStoreBackend<'a, M: store::RecordMedium, S: store::Staging> {
    medium: &'a mut M,
    staging: &'a mut S,
    blocks: &'a mut [[u8; store::BLOCK_BYTES]; 2],
    cache: &'a mut store::CacheState,
    area: store::Area,
}
impl<'a, M: store::RecordMedium, S: store::Staging> RecordStoreBackend<'a, M, S> {
    pub fn new(medium: &'a mut M, staging: &'a mut S, blocks: &'a mut [[u8; store::BLOCK_BYTES]; 2], cache: &'a mut store::CacheState) -> Self {
        let geometry = store::Geometry::new(store::RecordBytes(52), store::RecordCount(127), store::SequenceOffset(0)).unwrap_or_else(|_| contract_violation());
        Self { medium, staging, blocks, cache, area: store::Area::register(store::RecordKind::Kind3, geometry) }
    }
    pub fn read_with_recovery(&mut self, recovery: Option<&mut u16>) -> ReloadOutcome {
        let mut entry = store::Entry::new(self.medium, self.staging, self.blocks, self.cache);
        let recovery = if cfg!(custom_control_drop_recovery) { None } else { recovery };
        match entry.read_latest(&mut self.area, &mut CustomCodec, recovery) {
            store::Latest::Found { candidate, record } => ReloadOutcome::Latest {
                candidate, record: CustomSettingsRecord::from_working_wire(record.try_into().unwrap_or_else(|_| contract_violation())),
            },
            store::Latest::Empty => ReloadOutcome::Empty,
            store::Latest::NoneValid { output } => ReloadOutcome::NoneValid { output },
        }
    }
}
impl<M: store::RecordMedium, S: store::Staging> CustomRecordBackend for RecordStoreBackend<'_, M, S> {
    fn read_latest(&mut self) -> ReloadOutcome { self.read_with_recovery(None) }
    fn append(&mut self, record: &CustomSettingsRecord) -> Result<store::Appended, store::NotAppended> {
        store::Entry::new(self.medium, self.staging, self.blocks, self.cache).append(&mut self.area, &mut CustomCodec, store::Seed(0), record.wire())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area2Startup { Accepted, Defaults, CompletedByRetainedStartup }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderStartup { AcceptedBackup, AcceptedAlternate, Defaults, CompletedByRetainedStartup }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomReset { Preserve, Reset }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartupPrerequisites { pub area2: Area2Startup, pub header: HeaderStartup, pub custom_reset: CustomReset }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DefaultBits(u8);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomDefaultField { Mode, Enabled, Style, FirstChoice, SecondChoice, ThirdChoice }
impl CustomDefaultField {
    pub const ORDER: [Self; 6] = [Self::Mode, Self::Enabled, Self::Style, Self::FirstChoice, Self::SecondChoice, Self::ThirdChoice];
    pub const fn configuration_offset(self) -> u32 {
        match self { Self::Mode => 0x217, Self::Enabled => 0x218, Self::Style => 0x219, Self::FirstChoice => 0x208, Self::SecondChoice => 0x209, Self::ThirdChoice => 0x20a }
    }
    pub const fn byte_offset(self) -> usize {
        match self { Self::Mode | Self::ThirdChoice => 26, Self::Enabled => 13, Self::Style => 30, Self::FirstChoice => 25, Self::SecondChoice => 24 }
    }
    pub fn decode(self, value: u8) -> CustomDefaultUpdate {
        let bits = match self {
            Self::Mode => match value { 0..=3 | 0x82 => 2, 4..=6 | 0x81 => 1, _ => 0 },
            Self::Enabled => u8::from(value != 1),
            Self::Style => match value { 1..=3 => value, _ => 0 },
            Self::FirstChoice | Self::SecondChoice | Self::ThirdChoice => match value {
                0..=3 => 7, 4 => 8, 5 => 11, 6 => 14, 7 => 17, 8 => 20, 9 => 23, 10 => 25,
                0x85 => 1, 0x86 => 2, 0x87 => 3, 0x88 => 4, 0x80..=0xff => 1, _ => 8,
            },
        };
        CustomDefaultUpdate { field: self, bits: DefaultBits(bits) }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CustomDefaultUpdate { field: CustomDefaultField, bits: DefaultBits }
impl CustomDefaultUpdate {
    pub fn apply(self, previous: u8) -> u8 {
        match self.field {
            CustomDefaultField::Mode | CustomDefaultField::Style => (previous & 0x9f) | (self.bits.0 << 5),
            CustomDefaultField::Enabled => (previous & 0x1f) | (self.bits.0 << 5),
            _ => (previous & 0xe0) | self.bits.0,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CustomDefaultsConfiguration { updates: [CustomDefaultUpdate; 6] }
impl CustomDefaultsConfiguration {
    pub fn from_abi(inputs: [u8; 6]) -> Self {
        Self { updates: core::array::from_fn(|index| {
            let field = *CustomDefaultField::ORDER.get(index).unwrap_or_else(|| contract_violation());
            let value = *inputs.get(index).unwrap_or_else(|| contract_violation());
            field.decode(value)
        }) }
    }
    pub fn payload(&self) -> CustomPayload {
        let mut bytes = *crate::SlotDefaultsWindow::slot(crate::ProfileSelection::Record0).bytes();
        if cfg!(custom_control_static_defaults) { return CustomPayload(bytes); }
        for update in self.updates {
            let byte = bytes.get_mut(update.field.byte_offset()).unwrap_or_else(|| contract_violation());
            *byte = update.apply(*byte);
        }
        CustomPayload(bytes)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomCommitOutcome {
    Unchanged,
    Appended { append: Result<store::Appended, store::NotAppended>, latest: ReloadOutcome },
}
impl CustomCommitOutcome { pub const fn retained_abi_status(&self) -> i32 { 0 } }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CustomBootOutcome {
    pub prerequisites: StartupPrerequisites,
    pub initial: ReloadOutcome,
    pub missing_defaults: Option<CustomCommitOutcome>,
    pub reset_defaults: Option<CustomCommitOutcome>,
    pub modes_reset: bool,
}

pub struct CustomSettingsLifecycle<'a> {
    previous: &'a mut CustomSettingsRecord,
    working: &'a mut CustomSettingsRecord,
    live: &'a mut CustomPayload,
}
impl<'a> CustomSettingsLifecycle<'a> {
    pub fn adopt(previous: &'a mut CustomSettingsRecord, working: &'a mut CustomSettingsRecord, live: &'a mut CustomPayload) -> Self { Self { previous, working, live } }
    pub fn live(&self) -> &CustomPayload { self.live }
    pub fn live_mut(&mut self) -> &mut CustomPayload { self.live }
    pub fn working(&self) -> &CustomSettingsRecord { self.working }
    pub fn set_metadata(&mut self, metadata: CustomMetadata) { self.working.set_metadata(metadata); }
    fn reload(&mut self, backend: &mut impl CustomRecordBackend) -> ReloadOutcome {
        let latest = backend.read_latest();
        if let ReloadOutcome::Latest { record, .. } = latest { *self.previous = record; }
        latest
    }
    pub fn commit(&mut self, backend: &mut impl CustomRecordBackend) -> CustomCommitOutcome {
        self.working.set_payload(self.live);
        if !cfg!(custom_control_append_unchanged) && !self.working.changed_from(self.previous) { return CustomCommitOutcome::Unchanged; }
        let append = backend.append(self.working).map(|mut appended| {
            if cfg!(custom_control_hide_program_error) { appended.persisted = store::Persisted::Confirmed; }
            appended
        });
        let latest = if cfg!(custom_control_skip_reread) { ReloadOutcome::Empty } else { self.reload(backend) };
        if !cfg!(custom_control_omit_sequence_reload) { copy_value(part_mut(&mut self.working.wire, 0, 2), part(&self.previous.wire, 0, 2)); }
        CustomCommitOutcome::Appended { append, latest }
    }
    pub fn boot(&mut self, prerequisites: &mut StartupPrerequisites, defaults: &CustomDefaultsConfiguration, backend: &mut impl CustomRecordBackend, reset_modes: impl FnMut()) -> CustomBootOutcome {
        self.boot_with_defaults(prerequisites, || defaults.payload(), backend, reset_modes)
    }
    pub fn boot_with_defaults(&mut self, prerequisites: &mut StartupPrerequisites, mut defaults: impl FnMut() -> CustomPayload, backend: &mut impl CustomRecordBackend, mut reset_modes: impl FnMut()) -> CustomBootOutcome {
        let observed = *prerequisites;
        let initial = self.reload(backend);
        let missing = !matches!(initial, ReloadOutcome::Latest { .. });
        let missing_defaults = if missing {
            self.previous.wire.fill(0xff);
            *self.live = defaults();
            let committed = self.commit(backend);
            reset_modes();
            Some(committed)
        } else { None };
        let reset_defaults = if prerequisites.custom_reset == CustomReset::Reset {
            *self.live = defaults();
            let committed = self.commit(backend);
            if !cfg!(custom_control_omit_reset_clear) { prerequisites.custom_reset = CustomReset::Preserve; }
            Some(committed)
        } else { None };
        *self.working = *self.previous;
        *self.live = self.working.payload();
        CustomBootOutcome { prerequisites: observed, initial, missing_defaults, reset_defaults, modes_reset: missing }
    }
}

#[cfg(test)]
#[path = "custom_tests.rs"]
mod tests;
