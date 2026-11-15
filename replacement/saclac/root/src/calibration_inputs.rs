#![forbid(unsafe_code)]

use xm1_saclac_config::{dump::DumpDerivations, ports::{Derivations, ExportQuery}, stock::GuestMemory, table::{self, Row, Step}};
use xm1_calibration_core::{Calibration, ChannelIndex, CurveIndex, configuration::{ConfigAddress, Configuration, ConfigurationError, ConfigurationMemory}};
use xm1_power_lifecycle::{record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, GuestDomain, MemoryError, RecordContext}, record_profile_globals, record_profile_mapping};
use xm1_vendor_protocol::{metadata_calibration::MetadataCalibration, metadata_math::{CalibrationValue, transform_mapped_value}};
use xm1_saclac_shared::{settings::Channel, stock::{Argument::{Buffer, Word}, Machine}};

pub(crate) struct Native<'a, M>(pub(crate) &'a M, pub(crate) &'a xm1_saclac_shared::stock::Stock<M>);

impl<M: Machine + Clone> GuestMemory for Native<'_, M> {
    fn byte(&self, at: u32) -> u8 { self.0.read_u8(at) }
    fn halfword(&self, at: u32) -> u16 { self.0.read_u16(at) }
    fn word(&self, at: u32) -> u32 { self.0.read_u32(at) }
    fn set_byte(&self, at: u32, value: u8) { self.0.clone().write_u8(at, value); }
    fn set_halfword(&self, at: u32, value: u16) {
        let mut machine = self.0.clone();
        for (offset, byte) in value.to_le_bytes().into_iter().enumerate() { machine.write_u8(at.wrapping_add(offset as u32), byte); }
    }
    fn set_word(&self, at: u32, value: u32) { self.0.clone().write_u32(at, value); }
    fn fill(&self, at: u32, value: u8, count: u32) { self.0.enter(0x7221c8, &[at as usize, usize::from(value), count as usize]); }
}

impl<M: Machine + Clone> Derivations for Native<'_, M> {
    fn export_word(&mut self, channel: Channel, query: ExportQuery) -> i16 {
        let mut word = crate::stock::acquisition::Aligned([0; 2]);
        self.0.enter_with(0x7498e0, &[Word(channel.0 as u32), Word(query.selector), Word(query.first), Word(query.second), Buffer(0), Word(0)], &mut word);
        i16::from_le_bytes(word.0)
    }
    fn adjust_direction(&mut self, word: i16, direction: u32, mode: u32) -> i16 {
        record_profile_mapping::helper_747aac(&mut self.record(), word as i32 as u32, direction, mode).expect("lens direction configuration") as i16
    }
    fn scaled_limit(&mut self, channel: Channel, first: u32, scale: u32) -> u16 { self.0.enter(0x749254, &[channel.0 as u32 as usize, first as usize, scale as usize]) as u16 }
    fn channel_scale(&mut self, channel: Channel) -> u16 {
        self.calibration().scaled_channel_curve(ChannelIndex(channel.0)).expect("channel calibration").0
    }
    fn table_scale(&mut self, step: u32, channel: Channel) -> u16 {
        self.metadata().scaled_mapped_word(CurveIndex(step), ChannelIndex(channel.0)).expect("mapped calibration").0
    }
    fn curve_position(&mut self, level: i16, channel: Channel) -> u32 { self.0.enter(0x746120, &[level as i32 as u32 as usize, channel.0 as u32 as usize]) }
    fn curve_value(&mut self, position: u32, channel: Channel) -> i16 { self.0.enter(0x7461a8, &[position as usize, channel.0 as u32 as usize]) as i16 }
    fn channel_word(&mut self, channel: Channel, selector: u32, table: u32) -> i16 { self.0.enter(0x747408, &[channel.0 as u32 as usize, selector as usize, table as usize]) as i16 }
    fn reference_word(&mut self, mode: u32, channel: Channel) -> i16 {
        let mut word = crate::stock::acquisition::Aligned([0; 2]);
        self.0.enter_with(0x747584, &[Word(0), Word(mode), Word(channel.0 as u32), Word(0), Word(0), Word(0), Buffer(0)], &mut word);
        i16::from_le_bytes(word.0)
    }
    fn apply_reference(&mut self, word: i16, channel: Channel) { self.0.enter(0x749be4, &[word as i32 as u32 as usize, channel.0 as u32 as usize, 0, u32::MAX as usize, 1]); }
    fn lens_table(&mut self, selector: u32, first: u32, second: u32) -> u32 { self.0.enter(0x7462e8, &[selector as usize, first as usize, second as usize]) }
    fn lens_limits(&mut self, table: u32, words: [u32; 11]) -> u32 {
        let mut arguments = [0; 12];
        arguments[0] = table as usize;
        for (argument, word) in arguments[1..].iter_mut().zip(words) { *argument = word as usize; }
        self.0.enter(0x74b05c, &arguments)
    }
}

impl<M: Machine + Clone> DumpDerivations for Native<'_, M> {
    fn transformed_value(&mut self, step: Step, row: Row) -> u16 {
        transform_mapped_value(CalibrationValue(table::value(self.1, step, row))) as u16
    }
    fn transformed_word(&mut self, step: Step, row: Row) -> u16 {
        self.calibration().transform_mapped_word(CurveIndex(step.0), ChannelIndex(row.0 as i32)).expect("mapped calibration").0
    }
    fn lens_position_flags(&mut self) -> u32 {
        lens_position_flags(self.0)
    }
    fn selection_word(&mut self, channel: Channel, mode: u32, flags: u32, word: i16) -> u32 {
        self.0.enter(0x748398, &[channel.0 as u32 as usize, mode as usize, flags as usize, word as i32 as u32 as usize])
    }
}

// ConfigurationMemory addresses are offsets into the canonical adjustment area.
struct Area<'a, A>(&'a A);
impl<A: xm1_saclac_shared::settings::Adjustments> ConfigurationMemory for Area<'_, A> {
    type Error = core::convert::Infallible;
    fn read_byte(&mut self, at: ConfigAddress) -> Result<u8, ConfigurationError<Self::Error>> { Ok(xm1_saclac_config::adjustment::byte(self.0, at.0)) }
    fn read_half(&mut self, at: ConfigAddress) -> Result<u16, ConfigurationError<Self::Error>> { Ok(xm1_saclac_config::adjustment::half(self.0, at.0)) }
    fn read_word(&mut self, at: ConfigAddress) -> Result<u32, ConfigurationError<Self::Error>> { Ok(xm1_saclac_config::adjustment::word(self.0, at.0)) }
}

impl<M: Machine + Clone> Native<'_, M> {
    fn calibration(&self) -> Calibration<'_, Area<'_, xm1_saclac_shared::stock::Stock<M>>> {
        Calibration::new(Configuration::new(Area(self.1), ConfigAddress(0)))
    }
    fn metadata(&self) -> MetadataCalibration<'_, Area<'_, xm1_saclac_shared::stock::Stock<M>>> {
        MetadataCalibration::new(Configuration::new(Area(self.1), ConfigAddress(0)))
    }
    fn record(&self) -> RecordContext<RecordMemory<'_, M>, ()> { runtime(self.0) }
}

pub(crate) struct RecordMemory<'a, M>(&'a M);
impl<M: Machine + Clone> GuestAddressSpace for RecordMemory<'_, M> {
    fn domain(&self) -> GuestDomain { GuestDomain::A }
    fn read_u8(&mut self, at: GuestAddress) -> Result<u8, MemoryError> { Ok(self.0.read_u8(at.get())) }
    fn read_u16(&mut self, at: GuestAddress) -> Result<u16, MemoryError> { Ok(self.0.read_u16(at.get())) }
    fn read_u32(&mut self, at: GuestAddress) -> Result<u32, MemoryError> { Ok(self.0.read_u32(at.get())) }
    fn write_u8(&mut self, at: GuestAddress, value: u8) -> Result<(), MemoryError> { self.0.clone().write_u8(at.get(), value); Ok(()) }
    fn write_u16(&mut self, at: GuestAddress, value: u16) -> Result<(), MemoryError> {
        let mut machine = self.0.clone();
        for (offset, byte) in value.to_le_bytes().into_iter().enumerate() { machine.write_u8(at.get().wrapping_add(offset as u32), byte); }
        Ok(())
    }
    fn write_u32(&mut self, at: GuestAddress, value: u32) -> Result<(), MemoryError> { self.0.clone().write_u32(at.get(), value); Ok(()) }
    fn copy_bytes(&mut self, destination: GuestAddress, source: GuestAddress, length: ByteCount) -> Result<(), MemoryError> {
        let mut machine = self.0.clone();
        for offset in 0..length.0 { machine.write_u8(destination.checked_add(offset)?.get(), machine.read_u8(source.checked_add(offset)?.get())); }
        Ok(())
    }
}

pub(crate) fn runtime<M: Machine + Clone>(machine: &M) -> RecordContext<RecordMemory<'_, M>, ()> {
    RecordContext::new(RecordMemory(machine), ())
}

pub(crate) fn lens_position<M: Machine + Clone>(machine: &M) -> u32 {
    record_profile_globals::helper_76da84(&mut runtime(machine)).expect("lens position read")
}

pub(crate) fn lens_direction<M: Machine + Clone>(machine: &M) -> u32 {
    record_profile_globals::helper_76dad4(&mut runtime(machine)).expect("lens direction read")
}

pub(crate) fn lens_position_flags<M: Machine + Clone>(machine: &M) -> u32 {
    record_profile_globals::helper_76f340(&mut runtime(machine)).expect("lens runtime read")
}

/// Signed comparison and halfword outputs of 0x100585c.
pub(crate) fn split_distance(machine: &impl Machine, distance: i16) -> xm1_shot_sequence::capture_boundary::DistanceSplit {
    use xm1_shot_sequence::capture_boundary::DistanceSplit;
    let crate::stock::ControlDistanceLimit(limit) = crate::stock::control_distance_limit(machine);
    if i32::from(distance) > limit {
        DistanceSplit { kept: limit as i16, excess: distance.wrapping_sub(limit as i16) }
    } else {
        DistanceSplit { kept: distance, excess: 0 }
    }
}
