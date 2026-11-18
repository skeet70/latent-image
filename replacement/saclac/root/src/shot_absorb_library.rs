#![forbid(unsafe_code)]

use xm1_saclac_calc::{adjust::focus_profile, exposure::{grid, ports::{Placement, ProfileAccess}}, record_policy::{self, ChannelMapping}};
use xm1_saclac_config::table::{self, Bounds, Row, Step, StepLimit};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{calculation::{AreaAt, AreaIndex, GridCell, ResultPass}, RecordHandle};
use xm1_saclac_profile_geometry::controller::Tables;
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{absorb::{AbsorbLibrary, CompensationRow, CompensationStep}, boundary::LensReference, stock::{calculation, state}};
use crate::{calculation_services::Calculation, services::Services, shot_services::Libraries};

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> ChannelMapping for Services<M, R, E, P> {
    fn channel(&mut self, pair: [u32; 2], bank: u32) -> u32 {
        table::nearest_step(&self.stock, Row(pair[0]), Step(pair[1]), Row(bank)).0
    }
    fn mapped_value(&mut self, channel: u32, bank: u32) -> u32 {
        u32::from(table::value(&self.stock, Step(channel), Row(bank)))
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: xm1_saclac_interface::ImageProcessor, I: Isp, T: Tables, A: Placement, Q>
    AbsorbLibrary for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn subject_source(&mut self, record: RecordHandle, refinement: u32) -> u32 {
        self.services.borrow_mut().profile().selection_kind(record, refinement != 0)
    }
    fn grid_mode(&mut self, record: RecordHandle, mode: u32) -> u32 {
        u32::from(self.services.borrow_mut().profile().style_option(record, mode))
    }
    fn evaluate_distances(&mut self, pass: ResultPass, cell: GridCell, index: AreaIndex) {
        let mut machine = self.services.borrow().machine.clone();
        let row = state::row(pass, cell, index).get();
        let distances = if machine.read_u8(row) == 0 {
            grid::AreaDistances::default()
        } else {
            // f1e1d8..f1e1ec reads the untracked cell even when the stored area is tracked.
            let area = state::area(AreaAt::Cell { pass, cell, index }).get();
            let reading = grid::AreaReading {
                at00: machine.read_u16(area) as i16,
                reference: machine.read_u32(area + 4),
                level: machine.read_u16(area + 0xa) as i16,
                at44: machine.read_u16(area + 0x44) as i16,
                at48: machine.read_u16(area + 0x48) as i16,
            };
            // Native +0x113c is the published input's +8, not Binding::lens_reference.
            let reference = machine.read_u32(calculation::INPUT + 8);
            grid::area_distances(
                &mut Calculation { services: &mut self.services.borrow_mut(), geometry: &mut self.geometry.borrow_mut(), placement: &mut *self.placement.borrow_mut() },
                &self.exposure.context.borrow(), reference, &reading,
            )
        };
        machine.write_u32(row + 4, distances.code());
        for (offset, value) in [(8, distances.metered), (10, distances.lens), (12, distances.metered_limit), (14, distances.lens_limit)] {
            machine.write_u16(row + offset, value as u16);
        }
    }
    fn selection_changed(&mut self, record: RecordHandle, source: u32, reference: u32) -> bool {
        record_policy::channel_changed(&mut *self.services.borrow_mut(), record, source, reference)
    }
    fn compare_reference(&mut self, record: RecordHandle, source: u32) -> (bool, u32) {
        let result = record_policy::compare_reference(&mut *self.services.borrow_mut(), record, source);
        (result.changed, result.channel)
    }
    fn compensated(&mut self, target: CompensationStep, source: CompensationRow) -> i16 {
        table::compensated_word(&self.services.borrow().stock, Step(target.0), Row(source.0))
    }
    fn classify_level(&mut self, value: i16) -> u32 {
        u32::from(focus_profile::level_band(&mut *self.services.borrow_mut(), value))
    }
    fn split_reference(&mut self) -> u32 { table::bank_size(&self.services.borrow().stock) }
    fn lower_reference(&mut self, reference: LensReference) -> u32 {
        table::lower_step(&self.services.borrow().stock, Row(reference.0), StepLimit::Unbounded, Bounds::Primary).0
    }
}
