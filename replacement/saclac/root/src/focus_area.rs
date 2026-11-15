use xm1_saclac_calc::adjust::focus_profile::{AreaConfiguration, AreaSetup, LensResources, ReferencedArea};
use xm1_saclac_config::channel;
use xm1_saclac_shared::{settings::Channel, stock::{Argument::{Buffer, Word}, Machine, Stock}};
use crate::stock::acquisition::Aligned;

pub(crate) struct Resources<'a, M>(pub(crate) &'a M, pub(crate) &'a Stock<M>);
impl<M: Machine + Clone> LensResources for Resources<'_, M> {
    fn resource(&mut self) -> Option<u32> {
        let resource = self.0.enter(0x76_d9c8, &[]);
        ((resource as i32) >= 0).then_some(resource)
    }
    fn reference(&mut self) -> u32 { self.0.enter(0x76_dbb4, &[]) }
    fn flags(&mut self) -> u32 { crate::calibration_inputs::lens_position_flags(self.0) }
    fn channel_bias(&mut self, resource: u32) -> i32 { i32::from(channel::bias(self.1, Channel(resource as i32), 0)) }
}

pub(crate) struct Areas<'a, M>(pub(crate) &'a M);
impl<M: Machine> Areas<'_, M> {
    fn program(&self, area: &AreaSetup, reference: Option<u32>) -> [i16; 2] {
        let mut output = Aligned([0; 4]);
        for (bytes, value) in output.0.chunks_exact_mut(2).zip(area.values) { bytes.copy_from_slice(&value.to_le_bytes()); }
        let mut args = [Word(0); 14];
        args[0] = Word(area.resource);
        args[2] = Word(area.scene);
        args[3] = Word(u32::from(area.kind));
        args[4] = Word(u32::from(area.class));
        let (entry, count) = if let Some(reference) = reference {
            args[5] = Word(reference);
            args[6] = Buffer(0);
            args[7] = Buffer(2);
            args[8] = Word(1);
            args[10] = Word(area.flags);
            args[12] = Word(u32::from(area.auxiliary));
            args[13] = Word(area.last);
            (0x74_c568, 14)
        } else {
            args[5] = Buffer(0);
            args[6] = Buffer(2);
            args[7] = Word(1);
            args[9] = Word(area.flags);
            args[10] = Word(u32::from(area.auxiliary));
            args[11] = Word(area.last);
            (0x74_bd64, 12)
        };
        self.0.enter_with(entry, &args[..count], &mut output.0);
        [i16::from_le_bytes([output[0], output[1]]), i16::from_le_bytes([output[2], output[3]])]
    }
}
impl<M: Machine> AreaConfiguration for Areas<'_, M> {
    fn configure(&mut self, area: &AreaSetup) -> [i16; 2] { self.program(area, None) }
    fn configure_referenced(&mut self, area: &ReferencedArea) -> [i16; 2] { self.program(&area.area, Some(area.reference)) }
}
