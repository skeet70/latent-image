use core::cell::RefCell;
use xm1_saclac_calc::{exposure::{context::ExposureContext, ports::{InPlace, Placement}}, selection::ROWS};
use xm1_saclac_interface::calc::SharedAddress;

/// Addresses into the canonical context, whose borrowed container cannot move while this lives.
/// Only C-layout measurements, subject lists and plain arrays cross the native boundary.
pub struct ExposurePlacement<'a> {
    context: &'a RefCell<ExposureContext>,
    columns: [SharedAddress; ROWS],
    measurements: [SharedAddress; ROWS],
    subjects: [SharedAddress; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AddressWidth;

fn address<T>(value: &T) -> Result<SharedAddress, AddressWidth> {
    u32::try_from(core::ptr::from_ref(value) as usize).map(SharedAddress).map_err(|_| AddressWidth)
}

impl<'a> ExposurePlacement<'a> {
    /// Construct after placing the context in its persistent guest storage.
    /// A host pointer outside the guest's 32-bit address space is refused, never truncated.
    pub fn new(context: &'a RefCell<ExposureContext>) -> Result<Self, AddressWidth> {
        let exposure = context.borrow();
        let mut columns = [SharedAddress(0); ROWS];
        let mut measurements = [SharedAddress(0); ROWS];
        for index in 0..ROWS {
            columns[index] = address(&exposure.selection.columns[index].regions)?;
            measurements[index] = address(&exposure.selection.rows[index].measurement)?;
        }
        let subjects = [
            address(&exposure.binding.subjects)?,
            address(&exposure.binding.summary)?,
            address(&exposure.snapshot.subjects)?,
            address(&exposure.snapshot.table_words)?,
        ];
        Ok(Self { context, columns, measurements, subjects })
    }

    pub fn context(&self) -> &'a RefCell<ExposureContext> { self.context }
}

impl Placement for ExposurePlacement<'_> {
    fn address(&mut self, block: InPlace) -> SharedAddress {
        match block {
            InPlace::ColumnRegions(row) => self.columns[row.get()],
            InPlace::RowMeasurement(row) => self.measurements[row.get()],
            InPlace::BoundSubjects => self.subjects[0],
            InPlace::BoundSummary => self.subjects[1],
            InPlace::SnapshotSubjects => self.subjects[2],
            InPlace::SnapshotTables => self.subjects[3],
        }
    }
}
