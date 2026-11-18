use crate::{calculation_services::Calculation, fake, geometry::GeometryBindings, services::Services, stages::StageDriver, stock::{geometry::Memory, stages::StageMemory}};
use xm1_saclac_calc::adjust::motion::readings::SelectionLists;
use xm1_saclac_profile_geometry::{controller::ProfileGeometry, profile::Profiles, stock::StockTables};
use xm1_saclac_shared::stock::{Argument, Machine};

#[derive(Clone)]
struct Frames;
impl Machine for Frames {
    fn read_u8(&self, _: u32) -> u8 { panic!("frame fixture has no scalar reads") }
    fn read_u16(&self, _: u32) -> u16 { panic!("frame fixture has no scalar reads") }
    fn read_u32(&self, _: u32) -> u32 { panic!("frame fixture has no scalar reads") }
    fn write_u8(&mut self, _: u32, _: u8) { panic!("frame fixture has no scalar writes") }
    fn write_u32(&mut self, _: u32, _: u32) { panic!("frame fixture has no scalar writes") }
    fn enter(&self, _: u32, _: &[usize]) -> u32 { panic!("frame fixture has no scalar calls") }
    fn enter_with(&self, entry: u32, arguments: &[Argument], output: &mut [u8]) -> u32 {
        match (entry, arguments) {
            (0xef5d44, [Argument::Word(mode), Argument::Word(relative), Argument::Buffer(at)]) => {
                let (width, height): (u32, u32) = match (*mode, *relative) {
                    (30, 1) => (800, 600),
                    (34, 1) => (400, 200),
                    (30 | 34, 0) => (1600, 1200),
                    _ => panic!("unexpected capture mode"),
                };
                for (bytes, value) in output[*at..*at + 16].chunks_exact_mut(4).zip([0, 0, width, height]) {
                    bytes.copy_from_slice(&value.to_le_bytes());
                }
                0
            }
            _ => panic!("unexpected frame service"),
        }
    }
}

#[test]
fn prepared_candidate_areas_use_relative_non_square_capture_dimensions() {
    let machine = Frames;
    let mut services = Services::new(machine.clone(), xm1_saclac_profile_records::Bytes::default(), fake::Entries::default(), ());
    let mut geometry = ProfileGeometry::new(Memory(machine.clone()), GeometryBindings {
        stages: StageDriver { isp: (), arguments: StageMemory, machine }, peers: (),
    }, Profiles::new(StockTables));
    let mut placement = ();
    let mut calculation = Calculation { services: &mut services, geometry: &mut geometry, placement: &mut placement };
    let mut candidates = xm1_shot_sequence::subjects::Candidates::default();
    candidates.lists[1].count = 1;
    let subject = &mut candidates.lists[1].entries[0];
    subject.detail.fill(0xa5);
    for (bytes, value) in subject.detail[..0x1c].chunks_exact_mut(4).zip([40u32, 90, 30, 20, 50, 100, 0x12345678]) {
        bytes.copy_from_slice(&value.to_le_bytes());
    }
    subject.id.0 = 0x87654321;
    candidates.lists[1].entries[1].detail[4..8].copy_from_slice(&71u32.to_le_bytes());
    candidates.lists[1].entries[1].detail[12..16].copy_from_slice(&39u32.to_le_bytes());
    let areas = crate::motion_memory::Selection { candidates: &candidates, reference: 0 }.areas();
    let converted = areas.in_mode(&mut calculation, 34);
    assert_eq!(converted.count, 1);
    let area = converted.areas[0];
    assert_eq!((area.x, area.y, area.height, area.width, area.centre_x, area.centre_y), (45, 13, 6, 15, 50, 16));
    let mut expected = [0xa5; 0x44];
    expected[0x38..0x3c].copy_from_slice(&0x87654321u32.to_le_bytes());
    assert_eq!((area.at_18, area.rest), (0x12345678, expected));
    assert_eq!((converted.areas[1].x, converted.areas[1].height), (71, 39));
}
