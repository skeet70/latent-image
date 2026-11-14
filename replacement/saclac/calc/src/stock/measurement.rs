//! X-M1 1.50 measurement configuration and the seven retained pipeline records (0xf9a6ac).

#![forbid(unsafe_code)]

use xm1_saclac_interface::calc::SharedAddress;
use crate::adjust::motion::measurement::{KindSettings, LevelChecks, MeasurementSettings, PlainSettings};
use crate::adjust::motion::search::SearchMode;
use crate::adjust::motion::state::MotionState;
use crate::exposure::ports::{Properties, Property, StockMemory};
use crate::exposure::settings;
pub use crate::stock::motion::{LOAD_TEST_AREA, LOAD_TEST_PLAIN, MASKED_SCENE, PLAIN_SEARCH};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LimitCheck { pub enable: Property, pub mask: u8, pub margin: Option<Property> }
const fn limit(enable: u32, mask: u8, margin: Option<u32>) -> LimitCheck {
    LimitCheck { enable: Property(enable), mask, margin: match margin { Some(id) => Some(Property(id)), None => None } }
}
pub const LIMIT_CHECKS: [LimitCheck; 11] = [
    limit(0x9412, 1, Some(0x9420)), limit(0x9412, 2, Some(0x9422)), limit(0x9412, 4, Some(0x9422)),
    limit(0x9412, 8, Some(0x9421)), limit(0x9412, 0x10, Some(0x9420)),
    limit(0x941b, 1, Some(0x9420)), limit(0x941b, 2, Some(0x9420)), limit(0x941b, 8, Some(0x94d1)),
    limit(0x941b, 4, Some(0x9423)), limit(0x941b, 0x10, None), limit(0x941b, 0x20, None),
];

pub const LEVEL_CHECK_BITS: Property = Property(0x94c4);
pub const LEVEL_CHECK_MARGIN: Property = Property(0x94c5);
pub const SECOND_LEVEL_CHECK: Property = Property(0x96e1);
pub const SECOND_LEVEL_MARGIN: Property = Property(0x96e2);
pub const CHANNEL_BITS: [Property; 2] = [Property(0x9db0), Property(0x9db1)];
pub const CHANNEL_TIMES: [Property; 2] = [Property(0x9de0), Property(0x9de2)];
pub const CHANNEL_CUTOFF: Property = Property(0x9de1);
pub const CHANNEL_KEPT: Property = Property(0x9de3);
pub const ROW_GAIN_LEVELS: [Property; 4] = [Property(0x9641), Property(0x9642), Property(0x9643), Property(0x9644)];
pub const ROW_GAINS: [Property; 4] = [Property(0x9645), Property(0x9646), Property(0x9647), Property(0x9648)];
pub const ROW_WEIGHTS: [Property; 12] = [Property(0x962b), Property(0x9637), Property(0x9631), Property(0x963d), Property(0x9629), Property(0x9635), Property(0x962f), Property(0x963b), Property(0x962d), Property(0x9639), Property(0x9633), Property(0x963f)];
pub const ROW_OFFSETS: [Property; 12] = [Property(0x9658), Property(0x965e), Property(0x965b), Property(0x9661), Property(0x9657), Property(0x965d), Property(0x965a), Property(0x9660), Property(0x9659), Property(0x965f), Property(0x965c), Property(0x9662)];
pub const ROW_NEGATIVE_OFFSETS: [Property; 12] = [Property(0x964b), Property(0x9651), Property(0x964e), Property(0x9654), Property(0x964a), Property(0x9650), Property(0x964d), Property(0x9653), Property(0x964c), Property(0x9652), Property(0x964f), Property(0x9655)];
pub const LEVEL_BAND: [Property; 2] = [Property(0x94c7), Property(0x94c6)];
pub const CHANNEL_DEPTH: Property = Property(0x9518);
pub const CHANNEL_DRIVE_BASE: Property = Property(0x951b);
pub const CHANNEL_DRIVE_FACTOR: Property = Property(0x951d);
pub const CHANNEL_DRIVE_LIMIT: Property = Property(0x951e);
pub const CHANNEL_TRAVEL_SCALE: Property = Property(0x9285);
pub const CHANNEL_TRAVEL_PERCENT: Property = Property(0x9520);
pub const PLAIN_TRAVEL: [Property; 3] = [Property(0x92a5), Property(0x92a6), Property(0x92a7)];
pub const SEARCH_DEPTH: Property = Property(0x92db);
pub const LAST_DEPTH: Property = Property(0x92d3);
pub const FIRST_CHECK: Property = Property(0x94b1);
pub const FIRST_CHECK_KIND: Property = Property(0x94b2);
pub const LEVEL_CHECKS: Property = Property(0x94b5);
pub const CHANNEL_SETS: Property = Property(0x9db2);
pub const KIND_FLAG: Property = Property(0x94ca);
pub const KIND_11_ORDERS: [Property; 2] = [Property(0x92bc), Property(0x92bb)];
pub const ORDERS: [Property; 2] = [Property(0x946e), Property(0x946e)];
pub const LEVEL_WINDOW: [Property; 2] = [Property(0x9430), Property(0x9432)];
pub const LEVEL_LIMIT: [Property; 2] = [Property(0x9462), Property(0x9464)];
pub const PLAIN_LEVEL: Property = Property(0x92d6);
pub const ALTERNATE_KIND: Property = Property(0x94d9);
pub const MASKED_DEPTHS: [Property; 4] = [Property(0x4902), Property(0x4908), Property(0x4909), Property(0x4904)];
pub const RANGED_FACTORS: [Property; 2] = [Property(0x929c), Property(0x929e)];
pub const DRIVE_KIND_SETTINGS: [Property; 2] = [Property(0x94e3), Property(0x94e4)];
pub const DRIVE_DEPTH: [Property; 2] = [Property(0x3e144d), Property(0x96ee)];

/// Addresses are supplied by the retained caller; owned callers keep `MeasurementSettings`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MeasurementRecords {
    pub map: SharedAddress, pub checks: SharedAddress, pub selection: SharedAddress,
    pub peak: SharedAddress, pub threshold: SharedAddress, pub weights: SharedAddress, pub search: SharedAddress,
}
impl MeasurementRecords {
    pub const fn addresses(self) -> [SharedAddress; 7] { [self.map, self.checks, self.selection, self.peak, self.threshold, self.weights, self.search] }
}

const fn at(base: SharedAddress, offset: u32) -> SharedAddress { SharedAddress(base.0.wrapping_add(offset)) }

/// A null outer request returns 0x84 without accessing it.
pub fn read_request(request: SharedAddress, memory: &mut impl StockMemory) -> Option<MeasurementRecords> {
    if request.0 == 0 { return None; }
    let [map, checks, selection, peak, threshold, weights, search] = core::array::from_fn(|index| SharedAddress(memory.read_u32(at(request, 8 + 4 * index as u32))));
    Some(MeasurementRecords { map, checks, selection, peak, threshold, weights, search })
}

pub fn write_request_header(request: SharedAddress, section: SharedAddress, memory: &mut impl StockMemory) {
    memory.write_u32(request, section.0);
    memory.write_u32(at(request, 4), 0x1ffe_c90c);
}

fn common<P: Properties + StockMemory>(base: SharedAddress, ports: &mut P) {
    let first = settings::word(ports, Property(0x9458));
    ports.write_u16(base, first);
    let enabled = ports.property(Property(0x944b)) != 0;
    ports.write_u32(at(base, 4), u32::from(enabled));
    let second = settings::word(ports, Property(0x944f));
    ports.write_u16(at(base, 8), second);
    let byte = ports.property(Property(0x92d2)) as u8;
    ports.write_u8(at(base, 10), byte);
}

fn write_level_checks(base: SharedAddress, checks: LevelChecks, memory: &mut impl StockMemory) {
    memory.write_u32(base, u32::from(checks.first));
    memory.write_u32(at(base, 4), u32::from(checks.second));
}

fn four_way(mode: SearchMode, ports: &mut impl Properties) -> u32 {
    if mode == SearchMode::Area { return 0; }
    match ports.property(Property(0x92ce)) { value @ 1..=4 => value, _ => 0 }
}

fn write_plain<P: Properties + StockMemory>(base: SharedAddress, plain: &PlainSettings, limit: [u32; 2], ports: &mut P) {
    ports.write_u32(at(base, 0x11c), u32::from(plain.level_reached));
    for (index, value) in plain.steps.iter().enumerate() { ports.write_u32(at(base, 0x120 + 4 * index as u32), *value); }
    ports.write_u32(at(base, 0x134), limit[0]);
    ports.write_u32(at(base, 0x138), limit[1]);
    let Some(search) = plain.plain else { return; };
    ports.write_u32(at(base, 0x13c), 1);
    ports.write_u32(at(base, 0x140), u32::from(search.first));
    let bits = ports.property(Property(0x92b1));
    ports.write_u32(at(base, 0x144), u32::from(bits & 1 == 0));
    let value = ports.property(Property(0x92a8));
    ports.write_u32(at(base, 0x148), value);
    ports.write_u32(at(base, 0x14c), u32::from(search.second));
    for (offset, id) in [(0x150, 0x92aa), (0x151, 0x92ac), (0x154, 0x92a9), (0x155, 0x92ab), (0x15c, 0x92a4)] {
        let value = ports.property(Property(id)) as u8;
        ports.write_u8(at(base, offset), value);
    }
    for (offset, id) in [(0x152, 0x92af), (0x156, 0x92ad)] {
        let value = settings::word(ports, Property(id));
        ports.write_u16(at(base, offset), value);
    }
    ports.write_u32(at(base, 0x158), u32::from(bits & 2 == 0));
    ports.write_u16(at(base, 0x15e), search.travel);
    ports.write_u32(at(base, 0x160), plain.alternate);
}

fn clear_records(records: MeasurementRecords, memory: &mut impl StockMemory) {
    for (address, bytes) in records.addresses().into_iter().zip([0x98, 0xc4, 0x6c, 0x18, 4, 0x90, 0x218]) {
        memory.fill(address, bytes, 0);
    }
}

pub fn configure_request<P: crate::adjust::motion::measurement::MeasurementPorts + StockMemory>(
    request: SharedAddress, state: &MotionState, ports: &mut P,
) -> u32 {
    let Some(records) = read_request(request, ports) else { return 0x84 };
    clear_records(records, ports);
    write_request_header(request, state.section, ports);
    let settings = state.measurement_settings(ports);
    write_fields(records, state, &settings, ports);
    0
}

pub fn write_measurement<P: Properties + StockMemory>(records: MeasurementRecords, state: &MotionState, settings: &MeasurementSettings, ports: &mut P) {
    clear_records(records, ports);
    write_fields(records, state, settings, ports);
}

fn write_fields<P: Properties + StockMemory>(records: MeasurementRecords, state: &MotionState, settings: &MeasurementSettings, ports: &mut P) {
    let [a, b, c, d, e, f, g] = records.addresses();
    macro_rules! w { ($base:expr, $offset:expr, $value:expr) => {{ let value = $value; ports.write_u32(at($base, $offset), value as u32); }}; }
    macro_rules! h { ($base:expr, $offset:expr, $value:expr) => {{ let value = $value; ports.write_u16(at($base, $offset), value as u16); }}; }
    macro_rules! byte { ($base:expr, $offset:expr, $value:expr) => {{ let value = $value; ports.write_u8(at($base, $offset), value as u8); }}; }
    macro_rules! pb { ($base:expr, $offset:expr, $id:expr) => { byte!($base, $offset, ports.property(Property($id))); }; }
    macro_rules! pw { ($base:expr, $offset:expr, $id:expr) => { w!($base, $offset, ports.property(Property($id))); }; }
    macro_rules! ph { ($base:expr, $offset:expr, $id:expr) => { h!($base, $offset, settings::word(ports, Property($id))); }; }
    let mode = state.search.mode;
    let layout = state.plan.layout.code();
    let level = state.exposures.second.level;
    let thresholds = state.thresholds.words();
    let limit = settings.search_limit.words();
    for (row, cells) in settings.map.0.iter().enumerate() {
        for (column, value) in cells.iter().enumerate() { byte!(a, (row * 8 + column) as u32, *value); }
    }
    w!(a, 0x40, 1); w!(a, 0x44, layout);
    byte!(a, 0x48, settings.secondary_areas); byte!(a, 0x49, settings.level_band);
    let buffers = crate::stock::motion::IMAGE_BUFFERS;
    for (offset, index) in [(0x4c, 0), (0x6c, 1), (0x50, 2), (0x54, 5), (0x80, 3), (0x88, 4)] { w!(a, offset, buffers[index].0); }
    w!(a, 0x58, mode == SearchMode::Channel && settings.search_limit.selected());
    let value = ports.property(Property(0x9621)); w!(a, 0x5c, if value <= 2 { value } else { 0 });
    for (offset, id) in [(0x60, 0x9624), (0x62, 0x9622), (0x64, 0x9626)] { ph!(a, offset, id); }
    w!(a, 0x68, state.layout.tracking.is_none());
    w!(a, 0x70, state.layout.cell_rate); h!(a, 0x74, state.exposures.second.at_06);
    for (index, value) in state.layout.weights.iter().enumerate() { h!(a, 0x76 + 2 * index as u32, *value); }
    w!(a, 0x7c, 1); w!(a, 0x84, 1);
    w!(a, 0x8c, state.layout.grids[0].cell_height); w!(a, 0x90, state.layout.grids[0].cell_width);
    for index in 0..3 { pb!(a, 0x94 + index, 0x9417 + index); }
    w!(b, 0, limit[0]); w!(b, 4, limit[1]); w!(b, 8, if mode == SearchMode::Treated { 0 } else { 3 });
    pb!(b, 0xc, 0x92c0); h!(b, 0xe, ports.property(Property(0x94bb)).wrapping_mul(10));
    let selected = if mode == SearchMode::Treated { &thresholds[12..18] } else { &thresholds[..6] };
    for (index, value) in selected.iter().enumerate() { w!(b, 0x10 + 4 * index as u32, *value); }
    w!(b, 0x28, 2); w!(b, 0x2c, 1); w!(b, 0x30, i32::from(settings.depth));
    w!(b, 0x34, settings::word(ports, Property(0x944f))); w!(b, 0x38, settings::word(ports, Property(0x945c)));
    pb!(b, 0x3c, 0x944c); pw!(b, 0x40, 0x944b);
    byte!(b, 0x44, settings.measured_cell.unwrap_or(0)); pb!(b, 0x45, 0x9457); ph!(b, 0x46, 0x9458);
    w!(b, 0x48, settings.first_check.is_some());
    if let Some(check) = settings.first_check {
        w!(b, 0x4c, check.kind);
        for (offset, id) in [(0x50, 0x94b0), (0x52, 0x94af), (0x54, if check.alternate_level { 0x94dc } else { 0x94b0 })] { h!(b, offset, ports.property(Property(id)).wrapping_mul(10)); }
        let pair = check.limit.words(); w!(b, 0x58, pair[0]); w!(b, 0x5c, pair[1]); w!(b, 0x60, check.above_margin);
    }
    w!(b, 0x64, settings.level_checks.is_some());
    if let Some(checks) = settings.level_checks {
        h!(b, 0x68, ports.property(Property(0x94c9)).wrapping_mul(10)); h!(b, 0x6a, ports.property(Property(0x94c8)).wrapping_mul(10));
        write_level_checks(at(b, 0x6c), checks, ports);
    }
    w!(b, 0x74, settings.channels.is_some());
    if let Some(channels) = settings.channels {
        for (index, value) in channels.channels.iter().enumerate() { byte!(b, 0x78 + index as u32, *value); byte!(b, 0x86 + index as u32, *value && channels.kept); }
        pw!(b, 0x94, 0x9dc2); pw!(b, 0x98, 0x9dc3);
    }
    w!(b, 0x9c, settings.kind_flag); pw!(b, 0xa0, 0x94cb); w!(b, 0xa4, layout == 2);
    for (index, value) in thresholds[12..].iter().enumerate() { w!(b, 0xa8 + 4 * index as u32, *value); }
    pb!(b, 0xc0, 0x94d8);
    for (offset, id) in [(0, 0x92bf), (1, 0x92bf), (2, 0x9476), (3, 0x92c0), (4, 0x92c0), (5, 0x9475)] { pb!(c, offset, id); }
    h!(c, 6, ports.property(Property(0x94bb)).wrapping_mul(10));
    for (offset, value) in [(8, 49), (0xc, 0), (0x10, settings.selection_limit as u32), (0x14, 49), (0x18, 1), (0x1c, settings.selection_limit as u32), (0x20, 62), (0x24, 0), (0x28, settings.selection_limit as u32)] { w!(c, offset, value); }
    for (base, values) in [(0x2c, &thresholds[..3]), (0x38, &thresholds[3..6]), (0x44, &thresholds[12..15])] {
        for (index, value) in values.iter().enumerate() { w!(c, base + 4 * index as u32, *value); }
    }
    w!(c, 0x50, settings.period_long);
    for (base, order) in [(0x60, settings.step_orders[0]), (0x54, settings.step_orders[1])] {
        for (index, value) in order.iter().enumerate() { w!(c, base + 4 * index as u32, *value); }
    }
    pb!(d, 0, 0x92cf); pb!(d, 1, 0x92d0); w!(d, 4, four_way(mode, ports)); w!(d, 8, settings.level_limit); common(at(d, 0xc), ports);
    pb!(e, 0, 0x94d0); pb!(e, 1, 0x94d0); ph!(e, 2, 0x944f);
    for (index, row) in settings.row_weights.iter().flatten().enumerate() {
        let base = index as u32 * 12; w!(f, base, row.weight); w!(f, base + 4, row.offset); w!(f, base + 8, row.negative_offset);
    }
    w!(g, 0, settings.kind_code);
    for (index, value) in thresholds.iter().enumerate() { w!(g, 4 + 4 * index as u32, *value); }
    w!(g, 0x4c, settings.period_long); w!(g, 0x50, 1); w!(g, 0x54, i32::from(settings.depth)); w!(g, 0x58, i32::from(settings.last_depth));
    common(at(g, 0x5c), ports); byte!(g, 0x66, 0); ph!(g, 0x68, 0x945e);
    w!(g, 0x6c, ports.property(Property(0x944b)) != 0);
    h!(g, 0x70, u32::from(state.plan.frame_depth).wrapping_mul(u32::from(settings::word(ports, Property(0x945a)))) / 100);
    pb!(g, 0x72, 0x9454); w!(g, 0x74, settings.level_limit); w!(g, 0x78, mode == SearchMode::Plain && ports.property(Property(0x9466)) == 1);
    w!(g, 0x7c, match ports.property(Property(0x9451)) { value @ 11..=14 => value - 10, _ => 0 });
    let pair = settings.kind_limits[0].words(); w!(g, 0x80, pair[0]); w!(g, 0x84, pair[1]); w!(g, 0x88, settings.level_window);
    w!(g, 0x8c, ports.property(Property(0x9461)) == 1 && level <= settings::tenths(ports, Property(0x9463)) as i16);
    w!(g, 0x90, ports.property(Property(0x9465)) == 1);
    let pair = settings.kind_limits[1].words(); w!(g, 0x94, pair[0]); w!(g, 0x98, pair[1]);
    let depth = state.depth_settings;
    byte!(g, 0x9c, depth.bytes[0]); byte!(g, 0x9d, depth.bytes[1]); h!(g, 0x9e, depth.halves[0]);
    w!(g, 0xa0, settings::word(ports, Property(0x9434)));
    byte!(g, 0xa4, depth.bytes[2]); h!(g, 0xa6, depth.halves[1]); h!(g, 0xa8, depth.halves[2]);
    w!(g, 0xac, four_way(mode, ports)); w!(g, 0xb0, state.readings.scene == MASKED_SCENE); w!(g, 0xb4, settings.load_test_plain);
    w!(g, 0xb8, matches!(state.plan.kind.0, 17 | 18)); h!(g, 0xbc, state.plan.profile.end);
    for offset in (0..28).step_by(4) { w!(g, 0xc0 + offset, ports.read_u32(at(b, 0x48 + offset))); }
    w!(g, 0xdc, settings.second_level_checks.is_some());
    if let Some(checks) = settings.second_level_checks {
        h!(g, 0xe0, ports.property(Property(0x94b4)).wrapping_mul(10)); h!(g, 0xe2, ports.property(Property(0x94b3)).wrapping_mul(10));
        write_level_checks(at(g, 0xe4), checks, ports);
    }
    w!(g, 0xec, settings.second_channels.is_some());
    if let Some(channels) = settings.second_channels {
        for (index, value) in channels.iter().enumerate() { byte!(g, 0xf0 + index as u32, *value); byte!(g, 0xfe + index as u32, *value); }
        pw!(g, 0x10c, 0x9de4); pw!(g, 0x110, 0x9dc3);
    }
    w!(g, 0x114, settings.kind_flag); pw!(g, 0x118, 0x94cb);
    match settings.kind {
        KindSettings::Treated(treated) => {
            w!(g, 0x1a0, limit[0]); w!(g, 0x1a4, limit[1]); w!(g, 0x1a8, 0); w!(g, 0x1ac, 4); w!(g, 0x1b0, treated.window); byte!(g, 0x1b4, treated.setting);
        }
        KindSettings::Plain(plain) => write_plain(g, &plain, limit, ports),
        KindSettings::Channel { drive, plain } => {
            let first = if state.readings.locked == 3 { 0x9507 } else { 0x94f7 };
            for index in 0..16 { pb!(g, 0x164 + index, first + index); }
            h!(g, 0x174, drive.depth); pb!(g, 0x176, 0x94f4); h!(g, 0x178, drive.positions[0]); h!(g, 0x17a, drive.positions[1]);
            h!(g, 0x17c, drive.curve); h!(g, 0x17e, drive.value); h!(g, 0x180, 0xa7);
            for (offset, id) in [(0x182, 0x94ef), (0x183, 0x94ee), (0x184, 0x94f5), (0x185, 0x94f6)] { pb!(g, offset, id); }
            w!(g, 0x18c, drive.frames); w!(g, 0x188, drive.step_frames); w!(g, 0x190, 255); byte!(g, 0x194, 0x30); byte!(g, 0x195, drive.slope); h!(g, 0x198, drive.travel);
            for (offset, id) in [(0x19a, 0x94f3), (0x19b, 0x94f2), (0x19c, 0x94f1)] { pb!(g, offset, id); }
            write_plain(g, &plain, limit, ports);
        }
    }
    if let Some(masked) = settings.masked {
        for (offset, id) in [(0x1b8, 0x4900), (0x1b9, 0x4901), (0x1bc, 0x4905), (0x1bd, 0x4906)] { pb!(g, offset, id); }
        for (offset, value) in [0x1c0, 0x1c4, 0x1c8, 0x1d0].into_iter().zip(masked.depths) { w!(g, offset, i32::from(value)); }
        w!(g, 0x1d4, i32::from(masked.position)); h!(g, 0x1ba, masked.cell);
    }
    if let Some(ranged) = settings.ranged { w!(g, 0x1d8, 49); w!(g, 0x1dc, 1); w!(g, 0x1e0, 0); w!(g, 0x1e4, ranged[0]); w!(g, 0x1e8, ranged[1]); }
    w!(g, 0x1ec, settings.bounds.is_some()); w!(g, 0x1f0, settings.bounds.is_some());
    if let Some(bounds) = settings.bounds {
        h!(g, 0x1f4, bounds.low); h!(g, 0x1f6, bounds.high); h!(g, 0x1f8, bounds.low_limit);
        w!(g, 0x204, bounds.drive_kind); pb!(g, 0x200, 0x927e); w!(g, 0x1fc, layout == 3);
        w!(g, 0x208, bounds.depth.is_some()); h!(g, 0x20c, bounds.depth.unwrap_or(0x7fff));
    }
    let trace = state.section;
    w!(trace, 0x44b98, ports.read_u8(at(b, 0xc))); w!(trace, 0x44b9c, ports.read_u16(at(g, 0x15e)));
    w!(trace, 0x44ba0, settings.search_limit.selected()); w!(trace, 0x44ba4, limit[0]);
}
