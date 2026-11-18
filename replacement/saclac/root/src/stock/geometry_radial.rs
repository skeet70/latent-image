use xm1_saclac_profile_geometry::ports::{RadialBuffer, RadialGeometry, RadialModel};
use xm1_saclac_shared::stock::Machine;
use crate::geometry::GeometryBindings;

#[repr(align(4))]
struct RadialRecord([u8; 140]);
impl RadialRecord {
    fn from_table(table: &RadialBuffer) -> Self {
        let mut record = Self([0; 140]);
        for (index, point) in table.points.iter().enumerate() { record.0[4 * index..4 * index + 4].copy_from_slice(&point.to_le_bytes()); }
        let halves = [table.scales[0], table.scales[1], table.bounds[0], table.bounds[1], table.gains[0], table.gains[1], table.shift];
        for (index, half) in halves.iter().enumerate() { record.0[0x7c + 2 * index..0x7e + 2 * index].copy_from_slice(&half.to_le_bytes()); }
        record
    }
    fn copy_to(&self, table: &mut RadialBuffer) {
        for (index, point) in table.points.iter_mut().enumerate() { *point = u32::from_le_bytes(self.0[4 * index..4 * index + 4].try_into().unwrap()); }
        let half = |at| u16::from_le_bytes(self.0[at..at + 2].try_into().unwrap());
        table.scales = [half(0x7c), half(0x7e)]; table.bounds = [half(0x80), half(0x82)]; table.gains = [half(0x84), half(0x86)]; table.shift = half(0x88);
    }
}
impl<M: Machine, P, I> RadialGeometry for GeometryBindings<M, P, I> {
    fn radial_model(&mut self, points: &[u32; 31]) -> RadialModel {
        let mut model = RadialModel::default();
        self.stages.machine.enter(0x6e_3b0c, &[points.as_ptr() as usize, model.0.as_mut_ptr() as usize]);
        model
    }
    fn fit_compact(&mut self, clocks: (u32, u32), model: &RadialModel, table: &mut RadialBuffer) -> bool {
        let mut record = RadialRecord::from_table(table);
        let result = self.stages.machine.enter(0x6e_3744, &[clocks.0 as usize, clocks.1 as usize, model.0.as_ptr() as usize, 1, record.0.as_mut_ptr() as usize]);
        record.copy_to(table);
        result != 0
    }
    fn radial_bound(&mut self, clocks: (u32, u32)) -> u16 {
        let mut value = 0u16;
        self.stages.machine.enter(0x6e_3488, &[clocks.0 as usize, clocks.1 as usize, &mut value as *mut u16 as usize]); value
    }
    fn radial_centre(&mut self, model: &mut RadialModel) -> u32 { self.stages.machine.enter(0x6e_3400, &[model.0.as_ptr() as usize]) }
    fn radial_extent(&mut self, clocks: (u32, u32), centre: u32, bound: u16, gain: u16) -> u16 {
        let mut value = 0u16;
        self.stages.machine.enter(0x6e_35f4, &[clocks.0 as usize, clocks.1 as usize, centre as usize, bound as usize, gain as usize, &mut value as *mut u16 as usize]); value
    }
    fn radial_table(&mut self, clocks: (u32, u32), pair: [u32; 2], model: &RadialModel, extent: u16, scales: [u16; 2], gain: u16) -> u16 {
        let mut value = 0u16;
        self.stages.machine.enter(0x6e_384c, &[clocks.0 as usize, clocks.1 as usize, pair[0] as usize, pair[1] as usize, model.0.as_ptr() as usize, extent as usize, scales[0] as usize, scales[1] as usize, gain as usize, &mut value as *mut u16 as usize]); value
    }
}
