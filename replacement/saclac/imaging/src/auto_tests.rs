use crate::auto::*;
use crate::driver::{acquire, Client, Event, Fault, Isp, Operation, Record, Refusal, Register, SessionId, Unit};
use crate::fake::{FakeIsp, IDLE};

const fn r(offset: u16) -> Register { Register::Auto(offset) }

const CALLER: Caller = Caller { client: Client::S2, session: Some(SessionId(1)) };

fn held() -> FakeIsp {
    let mut isp = FakeIsp::new();
    acquire(&mut isp, Unit::Auto, Client::S2).unwrap();
    isp.take();
    isp
}

fn parameter(operation: u32) -> Event {
    Event::Refused { client: Client::S2, unit: Unit::Auto, operation: Operation(operation), fault: Fault::Parameter }
}

const fn span(start: u16, blocks: u8, size: u16) -> Span { Span { start, blocks, size } }

const fn segment(position: u16, length: u16) -> Segment { Segment { position, length } }

#[test]
fn another_session_is_logged_and_the_operation_proceeds() {
    let mut isp = held();
    let caller = Caller { session: Some(SessionId(2)), ..CALLER };
    set_selectors(&mut isp, caller, Selectors { fields: [0; 4], source: Source::First, source_enabled: false });
    assert_eq!(isp.logs(), [Event::Refused { client: Client::S2, unit: Unit::Auto, operation: Operation(6), fault: Fault::ResourceId }]);
    assert_eq!(isp.writes().len(), 3);
}

#[test]
fn selectors_keep_the_registers_other_bits() {
    let mut isp = held();
    for offset in [0x00c, 0x600] { isp.preset(r(offset), 0xffff_ffff); }
    isp.preset(r(0x418), 1);
    set_selectors(&mut isp, CALLER, Selectors { fields: [0xf, 0, 3, 0], source: Source::Third, source_enabled: false });
    assert_eq!(isp.writes(), [(r(0x00c), 0xffff_cfc7), (r(0x600), 0xffff_fffe), (r(0x418), 0)]);
    assert!(isp.logs().is_empty());
}

#[test]
fn the_second_grid_programs_its_second_start_one_higher() {
    let mut isp = held();
    let grid = Grid { spans: [span(0x10, 16, 512), span(0x1ffe, 1, 1)], gaps: [0x1ff, 0] };
    assert_eq!(set_grid(&mut isp, CALLER, GridId::Second, grid), Ok(()));
    assert_eq!(isp.writes(), [(r(0x200), 0x0010_f1ff), (r(0x204), 0x1fff_0000), (r(0x258), 0x01ff_0000)]);
    let grid = Grid { spans: [span(0x10, 16, 512), span(0x1fff, 1, 1)], gaps: [0, 0] };
    assert_eq!(set_grid(&mut isp, CALLER, GridId::Second, grid), Err(Refusal::Parameter));
    assert_eq!(isp.logs(), [parameter(0x14)]);
}

#[test]
fn the_third_grid_programs_its_second_start_six_higher() {
    let mut isp = held();
    let grid = Grid { spans: [span(0, 1, 1), span(0x10, 2, 3)], gaps: [1, 2] };
    assert_eq!(set_grid(&mut isp, CALLER, GridId::Third, grid), Ok(()));
    assert_eq!(isp.writes(), [(r(0x300), 0), (r(0x304), 0x0016_1002), (r(0x30c), 0x0001_0002)]);
    let grid = Grid { spans: [span(0, 17, 1), span(0, 1, 1)], gaps: [0, 0] };
    assert_eq!(set_grid(&mut isp, CALLER, GridId::Third, grid), Err(Refusal::Parameter));
}

#[test]
fn the_first_grid_scales_its_second_span() {
    let mut isp = held();
    isp.preset(r(0x15c), 0x0401_0100);
    let grid = Grid { spans: [span(0x20, 2, 8), span(10, 3, 9)], gaps: [5, 0x1ff] };
    assert_eq!(set_grid(&mut isp, CALLER, GridId::First, grid), Ok(()));
    assert_eq!(isp.writes(), [(r(0x100), 0x0020_1007), (r(0x104), 0x000a_2003), (r(0x108), 0x0005_00ff)]);
}

#[test]
fn a_start_inside_the_margin_is_logged_against_the_holder() {
    let mut isp = FakeIsp::new();
    acquire(&mut isp, Unit::Auto, Client::TG).unwrap();
    isp.preset(r(0x15c), 0x1401_0100);
    let caller = Caller { client: Client::S1, session: Some(SessionId(1)) };
    let grid = Grid { spans: [span(0, 1, 1), span(0, 1, 1)], gaps: [0, 0] };
    assert_eq!(set_grid(&mut isp, caller, GridId::First, grid), Ok(()));
    assert_eq!(isp.logs(), [Event::Refused { client: Client::TG, unit: Unit::Auto, operation: Operation(10), fault: Fault::Parameter }]);
    assert_eq!(isp.value(r(0x104)), Some(0x0006_0000));
}

#[test]
fn other_scaling_modes_lower_the_start_by_two_more_or_leave_it() {
    let mut isp = held();
    let grid = Grid { spans: [span(0, 1, 1), span(4, 1, 1)], gaps: [0, 0] };
    isp.preset(r(0x15c), 0x300);
    set_grid(&mut isp, CALLER, GridId::First, grid).unwrap();
    assert_eq!(isp.value(r(0x104)), Some(0x000c_0000));
    isp.preset(r(0x15c), 0x500);
    set_grid(&mut isp, CALLER, GridId::First, grid).unwrap();
    assert_eq!(isp.value(r(0x104)), Some(0x000a_0000));
}

#[test]
fn a_scaled_size_never_reaches_zero() {
    let mut isp = held();
    isp.preset(r(0x15c), 0x0001_0100);
    let grid = Grid { spans: [span(0, 1, 1), span(0, 1, 1)], gaps: [0, 0] };
    set_grid(&mut isp, CALLER, GridId::First, grid).unwrap();
    assert_eq!(isp.value(r(0x104)), Some(0x0003_0000));
}

#[test]
fn a_refused_bank_pair_leaves_the_earlier_pairs_programmed() {
    let mut isp = held();
    let mut pairs = [[segment(1, 2), segment(3, 4)]; 10];
    pairs[2][0].length = 0;
    assert_eq!(set_first_bank(&mut isp, CALLER, &pairs), Err(Refusal::Parameter));
    assert_eq!(isp.writes(), [
        (r(0x10c), 0x0001_0001), (r(0x110), 0x0009_0003),
        (r(0x114), 0x0001_0001), (r(0x118), 0x0009_0003),
    ]);
    assert_eq!(isp.logs(), [parameter(12)]);
}

#[test]
fn the_second_bank_takes_wider_first_positions_and_shifts_the_second() {
    let mut isp = held();
    let pairs = [[segment(0x3fff, 0x2000), segment(0x1ffe, 1)]; 10];
    assert_eq!(set_second_bank(&mut isp, CALLER, &pairs), Ok(()));
    assert_eq!(isp.value(r(0x208)), Some(0x3fff_1fff));
    assert_eq!(isp.value(r(0x20c + 8 * 9)), Some(0x1fff_0000));
    let pairs = [[segment(0, 1), segment(0x1fff, 1)]; 10];
    assert_eq!(set_second_bank(&mut isp, CALLER, &pairs), Err(Refusal::Parameter));
}

#[test]
fn windows() {
    let mut isp = held();
    assert_eq!(set_window(&mut isp, CALLER, Window::First([segment(0x3fff, 1), segment(2, 3)])), Ok(()));
    assert_eq!(set_window(&mut isp, CALLER, Window::Second(span(0x3000, 4, 5), segment(6, 7))), Ok(()));
    assert_eq!(set_window(&mut isp, CALLER, Window::Third(segment(1, 2), span(3, 4, 5))), Ok(()));
    assert_eq!(set_window(&mut isp, CALLER, Window::Fourth([segment(1, 2), segment(3, 4)])), Ok(()));
    assert_eq!(isp.writes(), [
        (r(0x400), 0x3fff_0000), (r(0x404), 0x0002_0002),
        (r(0x408), 0x3000_3004), (r(0x40c), 0x0006_0006),
        (r(0x410), 0x0001_0001), (r(0x414), 0x0003_3004),
        (r(0x604), 0x0001_0001), (r(0x608), 0x0004_0003),
    ]);
    assert_eq!(set_window(&mut isp, CALLER, Window::Fourth([segment(0x2000, 1), segment(0, 1)])), Err(Refusal::Parameter));
    assert_eq!(isp.logs(), [parameter(0x31)]);
}

#[test]
fn auto_sessions_use_the_shared_lock() {
    let mut isp = FakeIsp::new();
    isp.put_record(Unit::Auto, Record { active: Some(SessionId(3)), holder: Client::REC, ..IDLE });
    isp.note(Unit::Auto, Operation(0x18));
    assert_eq!(acquire(&mut isp, Unit::Auto, Client::S1), Err(Refusal::Busy));
    assert_eq!(isp.logs(), [Event::Busy { holder: Client::REC, requester: Client::S1, unit: Unit::Auto, operation: Operation(0x18) }]);
}
