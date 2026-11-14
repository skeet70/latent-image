
use crate::compiled::*;
use crate::registers::PipelineHardware;

#[derive(Default)]
struct Scan {
    nine: Option<Optioned<9>>,
    four: Option<Optioned<4>>,
}

impl CompiledScan for Scan {
    fn set_long_table(&mut self, _: &[u16; 22]) {}
    fn set_wide_table(&mut self, _: &[u16; 28]) {}
    fn set_nine(&mut self, table: Optioned<9>) { self.nine = Some(table); }
    fn set_four(&mut self, table: Optioned<4>) { self.four = Some(table); }
    fn options(&self) -> [u8; 2] { [7, 9] }
    fn set_output_kind(&mut self, _: u8) {}
}

#[test]
fn stores_keep_each_tables_option() {
    let mut scan = Scan::default();
    store_nine(&mut scan, [9; 9]);
    store_four(&mut scan, [5; 4]);
    assert_eq!(scan.nine, Some(Optioned { option: 7, values: [9; 9] }));
    assert_eq!(scan.four, Some(Optioned { option: 9, values: [5; 4] }));
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
