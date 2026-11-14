#![forbid(unsafe_code)]

use xm1_saclac_profile_records::field::{RecordField, RecordRead, SharedField, SharedRead, Value};
use xm1_saclac_profile_records::{Memory, ProfileRecords, StockEntries};
use xm1_saclac_interface::stock::record_handle_from_a2;
use xm1_saclac_interface::RecordHandle;
use xm1_saclac_shared::commands::Commands;
use xm1_saclac_shared::stock::{Machine, Stock};

/// The all-ones word, which names no slot; reads of ungated shared fields take it.
pub(crate) const NO_HANDLE: RecordHandle = record_handle_from_a2(u32::MAX);

/// The stock and owned services the bound modules reach; `machine` serves the rows in `stock`
/// and `peers` posts to saclac's peer queues.
pub struct Services<M, R, E, P> {
    pub(crate) machine: M,
    pub(crate) stock: Stock<M>,
    pub(crate) commands: Commands<Stock<M>>,
    pub(crate) records: ProfileRecords<R>,
    pub(crate) entries: E,
    pub(crate) peers: P,
    pub(crate) acquisition: crate::acquisition::AcquisitionState,
    pub(crate) configuration: xm1_saclac_calc::adjust::configuration::Configuration,
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Services<M, R, E, P> {
    pub fn new(machine: M, records: R, entries: E, peers: P) -> Self {
        Self {
            stock: Stock::new(machine.clone()),
            commands: Commands::new(Stock::new(machine.clone())),
            machine,
            records: ProfileRecords::new(records),
            entries,
            peers,
            acquisition: crate::acquisition::AcquisitionState::default(),
            configuration: xm1_saclac_calc::adjust::configuration::Configuration::default(),
        }
    }

    pub fn records(&self) -> &ProfileRecords<R> { &self.records }

    /// A record field, or zero when the registry selects neither record.
    pub(crate) fn field<T: Value + Default, W>(&self, record: RecordHandle, field: &RecordField<T, RecordRead, W>) -> T {
        self.records.read(record, field).unwrap_or_default()
    }

    /// A shared field, or zero when `record` may not read it.
    pub(crate) fn shared<T: Value + Default, W>(&self, record: RecordHandle, field: &SharedField<T, SharedRead, W>) -> T {
        self.records.read_shared(record, field).unwrap_or_default()
    }

    /// A shared byte block, or zeros when `record` may not read it.
    pub(crate) fn shared_bytes<const N: usize, W>(&self, record: RecordHandle, field: &SharedField<[u8; N], SharedRead, W>) -> [u8; N] {
        self.records.read_shared(record, field).unwrap_or([0; N])
    }
}
