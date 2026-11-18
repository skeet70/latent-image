#![forbid(unsafe_code)]

//! X-M1 1.50 command and result data for machine 16, x70view 55436fc85c.
//! Native entries: configure f2e774/f2e7bc, stores f2ebc4/f2ed10/f2edc0/f2eea8,
//! publications efa968/efa9f0/efaa34/efaa80/efaacc.

use xm1_saclac_interface::{capture_results::{FirstResult, SecondResult, ThirdResult, FourthResult}, CommandKind, RecordHandle};
use xm1_saclac_profile_records::{field::Index, stock::{record as r, shared as s}, Memory, StockEntries};
use xm1_saclac_shared::{settings::{signed_byte, AdjustmentKey, Adjustments}, stock::Machine};
use xm1_shot_sequence::{calculations_work::{BinnedPlanes, CaptureCommand, CalculationsRecords}, ports::CalculationsJob, stock::calculations as layout};
use crate::{controller_replies::parameter_word, shot_services::Libraries};

fn command(machine: &impl Machine, offset: usize) -> u32 { parameter_word(machine, CommandKind::Command07, offset / 4) }
fn word(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("calculation result word")) }
fn half(bytes: &[u8], offset: usize) -> u16 { u16::from_le_bytes(bytes[offset..offset + 2].try_into().expect("calculation result halfword")) }
fn block<const N: usize>(bytes: &[u8], offset: usize) -> [u8; N] { bytes[offset..offset + N].try_into().expect("calculation result block") }
fn publish(machine: &mut impl Machine, offset: usize, bytes: &[u8], optional: bool) {
    let destination = command(machine, offset);
    if optional && destination == 0 { return; }
    assert_ne!(destination, 0, "missing calculation client destination");
    for (index, &byte) in bytes.iter().enumerate() { machine.write_u8(destination.wrapping_add(index as u32), byte); }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> CaptureCommand for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn jobs(&mut self) -> u32 { command(&self.services.borrow().machine, 4) }
    fn detail(&mut self) -> u32 {
        let services = self.services.borrow();
        services.machine.read_u32(command(&services.machine, 0xc).wrapping_add(0x50))
    }
    fn processing_word(&mut self) -> u32 {
        let services = self.services.borrow();
        services.machine.read_u32(command(&services.machine, 0xc).wrapping_add(0x48))
    }
    fn fourth_target(&mut self) -> bool { command(&self.services.borrow().machine, 0x20) != 0 }
    fn publish_readout(&mut self, mode: u32, value: u32) {
        let mut machine = self.services.borrow().machine.clone();
        let destination = command(&machine, 0x38);
        assert_ne!(destination, 0, "missing calculation readout destination");
        machine.write_u32(destination.wrapping_add(0x18), mode);
        let destination = command(&machine, 0x38);
        assert_ne!(destination, 0, "missing calculation readout destination");
        machine.write_u32(destination.wrapping_add(0x1c), value);
    }
    fn publish_planes(&mut self, planes: &BinnedPlanes, extent: [u16; 2]) {
        let mut machine = self.services.borrow().machine.clone();
        let destination = command(&machine, 0x38);
        assert_ne!(destination, 0, "missing calculation planes destination");
        for (index, words) in planes.iter().enumerate() {
            for (cell, &value) in words.iter().enumerate() { machine.write_u32(layout::BINNED_PLANES[index] + 4 * cell as u32, value); }
        }
        for (index, address) in layout::BINNED_PLANES.into_iter().enumerate() { machine.write_u32(destination.wrapping_add(0x28 + 4 * index as u32), address); }
        for (index, value) in layout::BINNED_DIMENSIONS.into_iter().enumerate() { machine.write_u8(destination.wrapping_add(0x38 + index as u32), value); }
        machine.write_u16(destination.wrapping_add(0x3a), extent[0]);
        machine.write_u16(destination.wrapping_add(0x3c), extent[1]);
    }
    fn publish(&mut self, job: CalculationsJob, record: RecordHandle) {
        let services = self.services.borrow();
        let mut machine = services.machine.clone();
        match job {
            CalculationsJob::First => {
                let bytes = services.records.read(record, &r::group_03::AT_0144_28).expect("calculation first publication source");
                publish(&mut machine, 0x18, &bytes, false);
            }
            CalculationsJob::Second => {
                let first = services.records.read(record, &r::group_03::AT_0144_28).expect("calculation first publication source");
                let second = services.records.read(record, &r::group_11::AT_0364_28).expect("calculation second publication source");
                publish(&mut machine, 0x18, &first, false);
                publish(&mut machine, 0x1c, &second, false);
                let second = services.records.read(record, &r::group_11::AT_0364_28).expect("calculation second publication source");
                publish(&mut machine, 0x1c, &second, false);
            }
            CalculationsJob::Third | CalculationsJob::Fourth => {
                let bytes = services.records.read(record, &r::group_12::AT_0380_12).expect("calculation final publication source");
                publish(&mut machine, 0x20, &bytes, job == CalculationsJob::Fourth);
            }
        }
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> CalculationsRecords for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn configuration(&mut self) -> u32 { u32::from(self.services.borrow().stock.byte(AdjustmentKey(layout::DETAIL_CONFIGURATION))) }
    fn offset_unit(&mut self) -> u16 { i16::from(signed_byte(&self.services.borrow().stock, AdjustmentKey(layout::OFFSET_UNIT))) as u16 }
    fn paired_mode(&mut self, record: RecordHandle) -> u32 { self.services.borrow().records.read_element(record, &r::group_07::AT_01C0, Index::new(0).expect("first calculation slot")).expect("calculation paired mode source") }
    fn profile_kind(&mut self, record: RecordHandle) -> u32 { u32::from(self.services.borrow().records.read(record, &r::group_00::AT_0043).expect("calculation profile kind source")) }
    fn slot(&mut self, record: RecordHandle) -> [u8; 12] { block(&self.services.borrow().records.read_shared(record, &s::group_22::AT_0180).expect("calculation slot source"), 0) }
    fn store_first(&mut self, record: RecordHandle, result: &FirstResult) {
        let mut services = self.services.borrow_mut();
        let _ = services.records.write(record, &r::group_03::AT_014E, block(&result.0, 0));
        let _ = services.records.write(record, &r::group_03::AT_0154, result.last_word());
    }
    fn store_second(&mut self, record: RecordHandle, result: &SecondResult) {
        let mut services = self.services.borrow_mut();
        let bytes = &result.0;
        let _ = services.records.write(record, &r::group_03::AT_0144_4, word(bytes, 0));
        let _ = services.records.write(record, &r::group_03::AT_0148, block(bytes, 4));
        let _ = services.records.write(record, &r::group_11::AT_0364_4, word(bytes, 0xc));
        let _ = services.records.write(record, &r::group_11::AT_0368, block(bytes, 0x10));
        let _ = services.records.write(record, &r::group_11::AT_036E, block(bytes, 0x16));
        let _ = services.records.write(record, &r::group_11::AT_0374, block(bytes, 0x1c));
        let _ = services.records.write(record, &r::group_03::AT_0158, block(bytes, 0x28));
        let _ = services.records.write(record, &r::group_11::AT_037A, half(bytes, 0x22));
        let _ = services.records.write(record, &r::group_11::AT_037C, half(bytes, 0x24));
        let _ = services.records.write(record, &r::group_11::AT_037E, half(bytes, 0x26));
    }
    fn store_third(&mut self, record: RecordHandle, result: ThirdResult) { let _ = self.services.borrow_mut().records.write(record, &r::group_12::AT_0380_4, result.0); }
    fn store_fourth(&mut self, record: RecordHandle, result: FourthResult) {
        let mut services = self.services.borrow_mut();
        let _ = services.records.write(record, &r::group_12::AT_0384, result.first);
        let _ = services.records.write(record, &r::group_12::AT_0386, result.second);
        let _ = services.records.write(record, &r::group_12::AT_0388, result.third);
    }
}
