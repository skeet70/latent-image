#![forbid(unsafe_code)]

use xm1_saclac_calc::exposure::ports::{ImageReport, StockMemory};
use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_shared::stock::{Argument::{Buffer, Word}, Machine};
use crate::{services::Services, stock::acquisition::Aligned};

impl<M: Machine, R, E, P> StockMemory for Services<M, R, E, P> {
    fn read_u8(&mut self, at: SharedAddress) -> u8 { self.machine.read_u8(at.0) }
    fn read_u16(&mut self, at: SharedAddress) -> u16 { self.machine.read_u16(at.0) }
    fn read_u32(&mut self, at: SharedAddress) -> u32 { self.machine.read_u32(at.0) }
    fn write_u8(&mut self, at: SharedAddress, value: u8) { self.machine.write_u8(at.0, value); }
    fn write_u16(&mut self, at: SharedAddress, value: u16) { self.machine.write_u16(at.0, value); }
    fn write_u32(&mut self, at: SharedAddress, value: u32) { self.machine.write_u32(at.0, value); }
    fn fill(&mut self, at: SharedAddress, bytes: usize, value: u8) {
        for offset in 0..bytes { self.machine.write_u8(at.0.wrapping_add(offset as u32), value); }
    }
}

pub(crate) struct MachineMemory<'a, M>(pub(crate) &'a mut M);

impl<M: Machine> StockMemory for MachineMemory<'_, M> {
    fn read_u8(&mut self, at: SharedAddress) -> u8 { self.0.read_u8(at.0) }
    fn read_u16(&mut self, at: SharedAddress) -> u16 { self.0.read_u16(at.0) }
    fn read_u32(&mut self, at: SharedAddress) -> u32 { self.0.read_u32(at.0) }
    fn write_u8(&mut self, at: SharedAddress, value: u8) { self.0.write_u8(at.0, value); }
    fn write_u16(&mut self, at: SharedAddress, value: u16) { self.0.write_u16(at.0, value); }
    fn write_u32(&mut self, at: SharedAddress, value: u32) { self.0.write_u32(at.0, value); }
    fn fill(&mut self, at: SharedAddress, bytes: usize, value: u8) {
        for offset in 0..bytes { self.0.write_u8(at.0.wrapping_add(offset as u32), value); }
    }
}

impl<M: Machine, R, E, P> ImageReport for Services<M, R, E, P> {
    fn second_report_word(&mut self) -> u32 {
        let mut output = Aligned([0; 4]);
        self.machine.enter_with(0x74_0888, &[Word(0), Buffer(0), Word(0), Word(0)], &mut output.0);
        u32::from_le_bytes(output.0)
    }
    fn fourth_report_word(&mut self) -> u32 {
        let mut output = Aligned([0; 4]);
        self.machine.enter_with(0x74_0888, &[Word(0), Word(0), Word(0), Buffer(0)], &mut output.0);
        u32::from_le_bytes(output.0)
    }
}
