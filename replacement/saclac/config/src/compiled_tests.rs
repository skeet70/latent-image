use xm1_saclac_shared::image_registers::{RegisterGroup, RegisterSnapshot, WindowLimits};

use crate::compiled::*;
use crate::registers::{PipelineHardware, RESTORE_CLIENT};

#[derive(Default)]
struct Scan {
    nine: Option<Optioned<9>>,
    four: Option<Optioned<4>>,
    flags: u32,
}

impl CompiledScan for Scan {
    type Block = ();
    fn fill_settings(&mut self, _: &mut ()) {}
    fn fill_geometry(&mut self, _: &mut ()) {}
    fn set_long_table(&mut self, _: &[u16; 22]) {}
    fn set_wide_table(&mut self, _: &[u16; 28]) {}
    fn nine(&mut self) -> Optioned<9> { Optioned { option: 7, values: [1, 2, 3, 4, 5, 6, 7, 8, 9] } }
    fn set_nine(&mut self, table: Optioned<9>) { self.nine = Some(table); }
    fn four(&mut self) -> Optioned<4> { Optioned { option: 9, values: [4, 3, 2, 1] } }
    fn set_four(&mut self, table: Optioned<4>) { self.four = Some(table); }
    fn options(&self) -> [u8; 2] { [7, 9] }
    fn set_output_kind(&mut self, _: u8) {}
    fn stage_flags(&self) -> u32 { self.flags }
}

#[test]
fn reads_give_the_values_and_stores_keep_each_tables_option() {
    let mut scan = Scan::default();
    assert_eq!(nine_values(&mut scan), [1, 2, 3, 4, 5, 6, 7, 8, 9]);
    assert_eq!(four_values(&mut scan), [4, 3, 2, 1]);
    store_nine(&mut scan, [9; 9]);
    store_four(&mut scan, [5; 4]);
    assert_eq!(scan.nine, Some(Optioned { option: 7, values: [9; 9] }));
    assert_eq!(scan.four, Some(Optioned { option: 9, values: [5; 4] }));
}

#[derive(Default)]
struct Snapshot(Vec<(char, usize)>);

impl RegisterSnapshot for Snapshot {
    type Configuration = ();
    fn restore(&mut self, group: RegisterGroup) { self.0.push(('r', group.index())); }
    fn window_limits(&self, _: ()) -> WindowLimits { WindowLimits([1, 2, 3]) }
    fn window_words(&self) -> [u32; 2] { [0, 0] }
    fn set_window_words(&mut self, _: [u32; 2]) { self.0.push(('w', 2)); }
    fn pack_retained(&mut self, group: RegisterGroup, _: ()) { self.0.push(('p', group.index())); }
}

#[derive(Default)]
struct Hardware(Vec<(u32, u32)>);

impl PipelineHardware for Hardware {
    fn acquire(&mut self, client: u32) -> u32 {
        self.0.push((client, 0));
        42
    }
    fn release(&mut self, client: u32, grant: u32) { self.0.push((client, grant)); }
}

#[test]
fn attaching_and_restoring_follow_the_configurations_stage_flags() {
    let scan = Scan { flags: 1 << 17 | 1 << 19, ..Scan::default() };
    let (mut snapshot, mut hardware) = (Snapshot::default(), Hardware::default());
    attach(&scan, (), &mut snapshot);
    restore(&scan, &mut snapshot, &mut hardware);
    assert_eq!(snapshot.0, [('p', 0), ('p', 1), ('p', 3), ('r', 0), ('r', 1), ('r', 3)]);
    assert_eq!(hardware.0, [(RESTORE_CLIENT, 0), (RESTORE_CLIENT, 42)]);
}

#[test]
fn releases_pass_their_grant_words() {
    let mut hardware = Hardware::default();
    release_configuration(&mut hardware, 3);
    release_capture(&mut hardware);
    assert_eq!(hardware.0, [(3, 0), (0x1000, 2)]);
}

#[derive(Debug, Eq, PartialEq)]
enum Step {
    Tag([u8; 4]),
    Read,
    Pack(u32),
    Select(u8),
    Options([u32; 2]),
    Apply(u8),
}

#[derive(Default)]
struct Stream(Vec<Step>);

impl StreamSnapshot for Stream {
    type Registers = u32;
    type Source = u8;
    fn write_tag(&mut self, tag: [u8; 4]) { self.0.push(Step::Tag(tag)); }
    fn read_registers(&mut self) -> u32 {
        self.0.push(Step::Read);
        0x55
    }
    fn pack(&mut self, registers: &u32) { self.0.push(Step::Pack(*registers)); }
    fn select(&mut self, source: u8) { self.0.push(Step::Select(source)); }
    fn set_options(&mut self, options: [u32; 2]) { self.0.push(Step::Options(options)); }
    fn apply_options(&mut self, source: u8) { self.0.push(Step::Apply(source)); }
}

#[test]
fn a_stream_snapshot_is_tagged_and_packed_while_the_hardware_is_held() {
    let (mut stream, mut hardware) = (Stream::default(), Hardware::default());
    snapshot_options(&mut stream, &mut hardware, 6);
    assert_eq!(
        stream.0,
        [Step::Select(6), Step::Options([0x1000, 16]), Step::Apply(6), Step::Tag(*b"S-MV"), Step::Read, Step::Pack(0x55)]
    );
    assert_eq!(hardware.0, [(0x1000, 0), (0x1000, 42)]);
}
