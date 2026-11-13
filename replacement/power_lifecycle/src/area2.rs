use crate::mirror::{BackupDiagnostic, BackupSemaphore, HeaderBackupDriver, SharedBacking, HEADER_BYTES};
use crate::record_store::{self as store, BlockBuffer, Callbacks, Entry, Handle, RecordMedium, Service, Staging};

pub const RECORD_BYTES: usize = 0x200;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveSelector { Snapshot, Close }
impl SaveSelector {
    pub fn from_abi(value: u32) -> Self { if value == 0 { Self::Snapshot } else { Self::Close } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FirmwareVersion { pub major: u8, pub minor: u8 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Area2Record(pub [u8; RECORD_BYTES]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SavedCounters { pub limit: u16, pub count: u16 }
impl Area2Record {
    pub fn saved_counters(&self) -> SavedCounters {
        SavedCounters {
            limit: u16::from_le_bytes([self.0[0x24], self.0[0x25]]),
            count: u16::from_le_bytes([self.0[0x26], self.0[0x27]]),
        }
    }
    pub fn changed_from(&self, previous: &Self) -> bool {
        if cfg!(power_control_append_equal) { return true; }
        let start = if cfg!(power_control_compare_sequence) { 0 } else { 2 };
        self.0[start..] != previous.0[start..]
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppendObservation { Equal, Appended(store::Appended), NotAppended(store::NotAppended), Closed }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SaveObservation { pub append: AppendObservation, pub completion: LegacyCompletion }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LegacyCompletion { Completed }
impl LegacyCompletion { pub const fn abi(self) -> i32 { 0 } }

pub struct Area2Lifecycle<'a> {
    working: &'a mut Area2Record,
    snapshot: &'a mut Area2Record,
    area2: Handle,
    area3: Handle,
}
impl<'a> Area2Lifecycle<'a> {
    pub fn adopt(working: &'a mut Area2Record, snapshot: &'a mut Area2Record, area2: Handle, area3: Handle) -> Self {
        Self { working, snapshot, area2, area3 }
    }
    pub fn save<D: HeaderBackupDriver, C: Callbacks, M: RecordMedium, S: Staging, B: BlockBuffer<M>>(
        &mut self,
        selector: SaveSelector,
        backing: &mut SharedBacking<'_, D>,
        version: FirmwareVersion,
        service: &mut Service<C>,
        entry: &mut Entry<'_, M, S, B>,
    ) -> SaveObservation {
        prepare(self.working, backing, version);
        let append = if !self.working.changed_from(self.snapshot) {
            AppendObservation::Equal
        } else if let Some(registration) = service.get_mut(self.area2) {
            match entry.append(&mut registration.area, &mut registration.callbacks, store::Seed(0), &self.working.0) {
                Ok(appended) => AppendObservation::Appended(appended),
                Err(error) => AppendObservation::NotAppended(error),
            }
        } else {
            AppendObservation::Closed
        };
        match selector {
            SaveSelector::Snapshot => self.snapshot.0.copy_from_slice(&self.working.0),
            SaveSelector::Close => { service.close(self.area2); service.close(self.area3); }
        }
        SaveObservation { append, completion: LegacyCompletion::Completed }
    }
}

pub(crate) fn prepare<D: HeaderBackupDriver>(working: &mut Area2Record, backing: &mut SharedBacking<'_, D>, version: FirmwareVersion) {
    let header: [u8; HEADER_BYTES] = backing.live()[..HEADER_BYTES].try_into().unwrap();
    let counter = u16::from_le_bytes([header[0], header[1]]);
    let limit = u16::from_le_bytes([header[2], header[3]]);
    let read_limit = if limit.wrapping_sub(100) < 900 { limit } else { 100 };
    let read_counter = if counter <= 9999 { counter } else { 0 };
    if read_limit >= 999 && read_counter >= 9999 {
        set_counter(backing, CounterField::Limit, 100);
        set_counter(backing, CounterField::Count, 0);
        let mut observations = [None; HEADER_BYTES];
        backing.synchronize_into(&mut observations);
    }
    let live = backing.live();
    working.0[0x24..0x26].copy_from_slice(&live[2..4]);
    working.0[0x26..0x28].copy_from_slice(&live[0..2]);
    for index in 0..4 {
        let source = 0x128 + index * 6;
        let destination = 4 + index * 8;
        working.0[destination..destination + 6].copy_from_slice(&live[source..source + 6]);
    }
    working.0[0x28..0x140].copy_from_slice(&live[0x10..0x128]);
    working.0[0x1e8] = version.major;
    working.0[0x1e9] = version.minor;
}

enum CounterField { Count, Limit }
fn set_counter<D: HeaderBackupDriver>(backing: &mut SharedBacking<'_, D>, field: CounterField, value: u16) {
    let task = backing.driver_mut().task();
    match &task {
        Some(task) => backing.driver_mut().acquire(task, BackupSemaphore::Settings),
        None => backing.driver_mut().diagnostic(BackupDiagnostic::OutsideTask),
    }
    let offset = match field { CounterField::Count => 0, CounterField::Limit => 2 };
    backing.live_mut()[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    let mut observations = [None; HEADER_BYTES];
    backing.synchronize_into(&mut observations);
    if let Some(task) = &task {
        backing.driver_mut().release(task, BackupSemaphore::Settings);
    }
}

#[cfg(test)]
#[path = "area2_tests.rs"]
mod tests;
