#![forbid(unsafe_code)]

//! The compiled scan configuration the profile layer fills and hands the pipeline (image 5,
//! 0xd5d268..0xd5e240, 0xd6b394, 0xd6b460): its register tables, its output kind, the register
//! groups of the configuration's applied parts, and the stream snapshot taken with it.

use xm1_saclac_shared::image_registers::RegisterSnapshot;

use crate::registers::{pack_groups, restore_groups, PipelineHardware, RESTORE_CLIENT};
use crate::scan::AppliedParts;

/// A table the scan library keeps beside an option byte.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Optioned<const N: usize> {
    pub option: u8,
    pub values: [u16; N],
}

/// The scan library's entries over one compiled configuration, which stay stock with the
/// library.
pub trait CompiledScan {
    /// A caller's block the library fills, in the library's layout.
    type Block;

    /// 0xd64840.
    fn fill_settings(&mut self, block: &mut Self::Block);
    /// 0xd649e8, whose two scratch outputs are dropped.
    fn fill_geometry(&mut self, block: &mut Self::Block);
    /// 0xd64850.
    fn set_long_table(&mut self, values: &[u16; 22]);
    /// 0xd649f8 with its halfwords alone.
    fn set_wide_table(&mut self, values: &[u16; 28]);
    /// 0xd64940.
    fn nine(&mut self) -> Optioned<9>;
    /// 0xd64950.
    fn set_nine(&mut self, table: Optioned<9>);
    /// 0xd64e2c.
    fn four(&mut self) -> Optioned<4>;
    /// 0xd6537c.
    fn set_four(&mut self, table: Optioned<4>);
    /// The option bytes the library keeps beside the nine- and four-entry tables.
    fn options(&self) -> [u8; 2];
    /// 0xd5e240.
    fn set_output_kind(&mut self, kind: u8);
}

/// 0xd5dfec.
pub fn nine_values(scan: &mut impl CompiledScan) -> [u16; 9] { scan.nine().values }

/// Replaces the nine-entry table, keeping its option byte (0xd5e05c).
pub fn store_nine(scan: &mut impl CompiledScan, values: [u16; 9]) {
    let [option, _] = scan.options();
    scan.set_nine(Optioned { option, values });
}

/// 0xd5e120.
pub fn four_values(scan: &mut impl CompiledScan) -> [u16; 4] { scan.four().values }

/// Replaces the four-entry table, keeping its option byte (0xd5e168).
pub fn store_four(scan: &mut impl CompiledScan, values: [u16; 4]) {
    let [_, option] = scan.options();
    scan.set_four(Optioned { option, values });
}

/// Packs the register groups of `applied` from `configuration`, the compiled scan's library
/// configuration (0xd5e1d4).
pub fn attach<S: RegisterSnapshot>(applied: AppliedParts, configuration: S::Configuration, snapshot: &mut S) {
    pack_groups(applied, configuration, snapshot);
}

/// Restores the register groups of `applied` (0xd5e1f4).
pub fn restore(applied: AppliedParts, snapshot: &mut impl RegisterSnapshot, hardware: &mut impl PipelineHardware) {
    restore_groups(applied, snapshot, hardware);
}

/// Releases `client`'s configuration hold (0xd5d268).
pub fn release_configuration(hardware: &mut impl PipelineHardware, client: u32) { hardware.release(client, 0) }

/// Releases the capture hold (0xd5d27c).
pub fn release_capture(hardware: &mut impl PipelineHardware) { hardware.release(RESTORE_CLIENT, 2) }

pub const STREAM_TAG: [u8; 4] = *b"S-MV";

/// A stream's register snapshot and the stream services around it, which stay stock.
pub trait StreamSnapshot {
    /// The image registers as 0x693dcc reads them.
    type Registers;
    /// A stream the snapshot options name.
    type Source: Copy;

    fn write_tag(&mut self, tag: [u8; 4]);
    /// 0x693dcc.
    fn read_registers(&mut self) -> Self::Registers;
    /// 0x73ed94 over the snapshot's 96 descriptors.
    fn pack(&mut self, registers: &Self::Registers);
    /// 0xd6ae18 with no second source.
    fn select(&mut self, source: Self::Source);
    /// The two option words 0xd6afe4 reads.
    fn set_options(&mut self, options: [u32; 2]);
    /// 0xd6afe4.
    fn apply_options(&mut self, source: Self::Source);
}

/// Tags the stream snapshot and packs the registers into it while holding the hardware
/// (0xd6b394).
pub fn snapshot_stream<S: StreamSnapshot>(stream: &mut S, hardware: &mut impl PipelineHardware) {
    let grant = hardware.acquire(RESTORE_CLIENT);
    stream.write_tag(STREAM_TAG);
    let registers = stream.read_registers();
    stream.pack(&registers);
    hardware.release(RESTORE_CLIENT, grant);
}

/// Selects `source`, applies options 0x1000 and 16, then takes the stream snapshot (0xd6b460).
pub fn snapshot_options<S: StreamSnapshot>(stream: &mut S, hardware: &mut impl PipelineHardware, source: S::Source) {
    stream.select(source);
    stream.set_options([0x1000, 16]);
    stream.apply_options(source);
    snapshot_stream(stream, hardware);
}
