use crate::geometry::GeometryBindings;
use xm1_saclac_shared::stock::Machine;

impl<M: Machine, P, I> xm1_saclac_profile_geometry::numeric::FloatLog for GeometryBindings<M, P, I> {
    fn ln(&mut self, value: f32) -> f32 { f32::from_bits(self.stages.machine.enter(0x89_4778, &[value.to_bits() as usize])) }
}
impl<M: Machine, P, I> xm1_saclac_profile_geometry::numeric::FloatRoot for GeometryBindings<M, P, I> {
    fn sqrt(&mut self, value: f32) -> f32 { f32::from_bits(self.stages.machine.enter(0x89_4df0, &[value.to_bits() as usize])) }
}
