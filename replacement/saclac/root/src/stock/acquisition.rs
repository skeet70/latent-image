#![forbid(unsafe_code)]

//! X-M1 1.50 layouts of the shared-block records the measurement round's acquisition reads and
//! writes, the configuration keys it reads, the memory it shares with calc1, imgdc and the shot,
//! and the retained entries it enters.

use xm1_focus_measurement_round::acquisition::ports::{
    AcquisitionBlock, AcquisitionSettings, Branch, ExposureRecordUpdate, GridInputs, PositionInputs,
    ProgramChoice, RateNotice, RateRecord, ReleaseSettings, SelectionNotice, SelectionPosition,
    SelectionRecord, StatisticsBuffer,
};
use xm1_focus_measurement_round::auxiliary::ResourceIndex;
use xm1_focus_measurement_round::pipeline::{HistoryStatistics, PipelineWord, Statistics, TimingWord};
use xm1_focus_measurement_round::profile::GridCell;
use xm1_focus_measurement_round::record::TraceMark;
use xm1_focus_measurement_round::stock::{SNAPSHOT_BYTES, S1_SAMPLE_BYTES};
use xm1_saclac_interface::calc::{AcquisitionGrid, SharedAddress};
use xm1_saclac_interface::stock::round_acquisition::{SelectionRegion, LIVE_REGION, S1_REGION};
use xm1_saclac_interface::stock::StockCallbackWord;
use xm1_saclac_interface::{Completion, RecordHandle};
use xm1_saclac_shared::stock::{Argument, Machine};

/// Retained entries may load words through their buffer arguments.
#[repr(align(4))]
pub(crate) struct Aligned<const N: usize>(pub(crate) [u8; N]);

impl<const N: usize> core::ops::Deref for Aligned<N> {
    type Target = [u8];
    fn deref(&self) -> &[u8] { &self.0 }
}

impl<const N: usize> core::ops::DerefMut for Aligned<N> {
    fn deref_mut(&mut self) -> &mut [u8] { &mut self.0 }
}

/// A value at a byte offset of a record, little-endian.
trait Packed: Sized {
    fn get(bytes: &[u8], at: usize) -> Self;
    fn put(&self, bytes: &mut [u8], at: usize);
}

impl Packed for u8 {
    fn get(bytes: &[u8], at: usize) -> Self { bytes[at] }
    fn put(&self, bytes: &mut [u8], at: usize) { bytes[at] = *self }
}

impl Packed for u16 {
    fn get(bytes: &[u8], at: usize) -> Self { u16::from_le_bytes([bytes[at], bytes[at + 1]]) }
    fn put(&self, bytes: &mut [u8], at: usize) { bytes[at..at + 2].copy_from_slice(&self.to_le_bytes()) }
}

impl Packed for u32 {
    fn get(bytes: &[u8], at: usize) -> Self { u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) }
    fn put(&self, bytes: &mut [u8], at: usize) { bytes[at..at + 4].copy_from_slice(&self.to_le_bytes()) }
}

/// A flag kept as a word.
impl Packed for bool {
    fn get(bytes: &[u8], at: usize) -> Self { u32::get(bytes, at) != 0 }
    fn put(&self, bytes: &mut [u8], at: usize) { u32::from(*self).put(bytes, at) }
}

macro_rules! packed_word {
    ($($type:ident),*) => {$(
        impl Packed for $type {
            fn get(bytes: &[u8], at: usize) -> Self { Self(u32::get(bytes, at)) }
            fn put(&self, bytes: &mut [u8], at: usize) { self.0.put(bytes, at) }
        }
    )*};
}

packed_word!(ResourceIndex, PipelineWord, StatisticsBuffer, TimingWord);

/// Where a field sits: one offset, or one offset per element.
trait At<T> {
    fn get(self, bytes: &[u8]) -> T;
    fn put(self, bytes: &mut [u8], value: &T);
}

impl<T: Packed> At<T> for usize {
    fn get(self, bytes: &[u8]) -> T { T::get(bytes, self) }
    fn put(self, bytes: &mut [u8], value: &T) { value.put(bytes, self) }
}

impl<T: Packed, const N: usize> At<[T; N]> for [usize; N] {
    fn get(self, bytes: &[u8]) -> [T; N] { self.map(|at| T::get(bytes, at)) }
    fn put(self, bytes: &mut [u8], value: &[T; N]) {
        for (at, element) in self.into_iter().zip(value) {
            element.put(bytes, at);
        }
    }
}

/// A record's decoder and its encoder over the same field offsets; the encoder leaves the
/// other bytes as they are.
macro_rules! record {
    ($(#[$meta:meta])* $decode:ident, $encode:ident, $record:ident, $bytes:literal, { $($field:ident: $at:expr),* $(,)? }) => {
        $(#[$meta])*
        pub fn $decode(bytes: &[u8; $bytes]) -> $record { $record { $($field: At::get($at, bytes),)* } }

        pub fn $encode(bytes: &mut [u8; $bytes], record: &$record) { $(At::put($at, bytes, &record.$field);)* }
    };
}

record!(
    /// The focus result 0x756734 copies out and 0x756754 back. Offsets from 0xf3382c, 0xf34c98,
    /// 0xf3515c, 0xf3557c, 0xf363f8 and 0xf2457c.
    acquisition_block, put_acquisition_block, AcquisitionBlock, 132, {
    lens: 0x00usize,
    collected: 0x04usize,
    exposure: 0x08usize,
    level: 0x0cusize,
    gain_offset: 0x0eusize,
    offset: 0x10usize,
    aspect: 0x12usize,
    reference: 0x14usize,
    halfword: 0x16usize,
    levels: [0x20, 0x22, 0x24, 0x26],
    reused: 0x28usize,
    offsets: 0x2cusize,
    extra: 0x30usize,
    release_halfword: 0x34usize,
    classes: 0x38usize,
    weights: [0x3c, 0x3e, 0x40, 0x42],
    release_offset: 0x44usize,
    copy: 0x48usize,
    selected: 0x60usize,
    selection_reference: 0x64usize,
    selection_byte: 0x66usize,
    eligible: 0x68usize,
    setting_ready: 0x6cusize,
    setting_position: 0x70usize,
    setting_halfword: 0x74usize,
    grid_word: 0x78usize,
    shot_span: 0x7cusize,
    setting_shot_word: 0x80usize,
});

record!(
    /// The record 0x75a4ec reads and 0x75a52c writes; 0xf2476c fills it from a selection result.
    selection_record, put_selection_record, SelectionRecord, 44, {
    position: 0x00usize,
    word: 0x04usize,
    level: 0x08usize,
    levels: [0x0a, 0x0e, 0x10, 0x12],
    target: 0x0cusize,
    limit_level: 0x14usize,
    reference: 0x16usize,
    flags: [0x18, 0x1c, 0x20, 0x24],
    limit: 0x28usize,
    tracked: 0x29usize,
});

/// 0xf36324..0xf36354 over the 24-byte record 0x758068 reads: the flag byte at +4 set, the
/// word before it kept.
pub fn put_program_choice(bytes: &mut [u8; 24], choice: &ProgramChoice) {
    1u8.put(bytes, 0x04);
    choice.word.put(bytes, 0x08);
    choice.level.put(bytes, 0x0c);
    choice.timing.put(bytes, 0x10);
    choice.code.put(bytes, 0x14);
    choice.selected.put(bytes, 0x16);
}

/// The 40-byte rate record 0x7582c0 reads: the halfword and word 0xf521d8 takes (0xf35340).
pub fn rate_record(bytes: &[u8; 40]) -> RateRecord { RateRecord { word: u32::get(bytes, 0x10), level: u16::get(bytes, 0x14) } }

/// 0xf35294 and 0xf2457c set the record's word at +0x24 and write it back with 0x758300.
pub fn put_rate_selection(bytes: &mut [u8; 40], selected: u32) { selected.put(bytes, 0x24) }

/// The 76-byte exposure record 0x7588e8 reads and 0x75890c writes: the gain step (0xf332b8),
/// the lens and its column (0xf34548..0xf34554), the primary pair (0xf3380c..0xf33814) and
/// the selection tail with its last halfword (0xf3369c..0xf336b4).
pub fn update_exposure_record(bytes: &mut [u8; 76], update: ExposureRecordUpdate) {
    match update {
        ExposureRecordUpdate::GainStep(step) => step.0.put(bytes, 0x34),
        ExposureRecordUpdate::Context { lens, index } => {
            lens.put(bytes, 0x38);
            index.put(bytes, 0x3c);
        }
        ExposureRecordUpdate::PrimaryPair(pair) => [0x28, 0x2a].put(bytes, &pair),
        ExposureRecordUpdate::SelectionTail { tail, last } => {
            [0x2c, 0x2e, 0x30].put(bytes, &tail);
            last.put(bytes, 0x4a);
        }
    }
}

/// The lens snapshot 0x756840 reads, 48 bytes: the reference 0xf36760 subtracts at +0x28 and
/// the word 0xf33ef4 takes at +0x2c.
pub fn lens_snapshot_reference(bytes: &[u8; 48]) -> (u16, u32) { (u16::get(bytes, 0x28), u32::get(bytes, 0x2c)) }

/// Words of a record field, in order.
pub fn words<const N: usize, const BYTES: usize>(bytes: &[u8; BYTES]) -> [u32; N] { core::array::from_fn(|index| u32::get(bytes, 4 * index)) }

pub fn put_words<const N: usize, const BYTES: usize>(bytes: &mut [u8; BYTES], words: [u32; N]) {
    for (index, word) in words.into_iter().enumerate() {
        word.put(bytes, 4 * index);
    }
}

/// Configuration keys the acquisition reads through 0x69f8fc (0xf34c98, 0xf3515c, 0xf2457c,
/// 0xf332d0, 0xf367a4, 0xf3579c).
const SNAPSHOT_GRID: u32 = 0x4a07;
const RATE: u32 = 0x5256;
const S1_RATE: u32 = 0x5257;
const RATE_STATUS: u32 = 0x5258;
const DRAWS_RATE: u32 = 0xa8;
const GATE: u32 = 0x65a6;
const RATE_DIVISOR: u32 = 0x657d;
/// The low byte of the halfword 0x69fec8 reads at 232 (0xf353a0).
const SUPPRESSES_ELIGIBLE: u32 = 0xe8;
const TOP_STEPS: u32 = 0x24f;
const TRACES: u32 = 0x24e;
/// The release's signed tens of the bounds, class weights and signed offsets (0xf34c98).
const RELEASE_BOUNDS: [u32; 4] = [0x6484, 0x6485, 0x6486, 0x6487];
const RELEASE_WEIGHTS: [[u32; 2]; 4] = [[0x648c, 0x648d], [0x6488, 0x6489], [0x6494, 0x6495], [0x6492, 0x6493]];
const RELEASE_OFFSETS: [u32; 3] = [0x64a0, 0x64a1, 0x64a2];

/// The acquisition's settings from the configuration bytes `byte` reads.
pub fn acquisition_settings(byte: impl Fn(u32) -> u8) -> AcquisitionSettings {
    AcquisitionSettings {
        snapshot_grid: byte(SNAPSHOT_GRID) != 0,
        rate: byte(RATE),
        s1_rate: byte(S1_RATE),
        rate_status: byte(RATE_STATUS) & 1 != 0,
        draws_rate: byte(DRAWS_RATE) & 0x80 != 0,
        gate: byte(GATE) & 1 != 0,
        rate_divisor: byte(RATE_DIVISOR) != 0,
        suppresses_eligible: byte(SUPPRESSES_ELIGIBLE) & 8 != 0,
        top_steps: byte(TOP_STEPS),
        traces: byte(TRACES) == 1,
        release: ReleaseSettings {
            bounds: RELEASE_BOUNDS.map(|key| byte(key) as i8),
            weights: RELEASE_WEIGHTS.map(|pair| pair.map(&byte)),
            offsets: RELEASE_OFFSETS.map(|key| byte(key) as i8),
        },
    }
}


/// A halfword the entry takes sign-extended.
fn signed(value: i16) -> Argument { Argument::Word(value as i32 as u32) }

fn word(value: u32) -> Argument { Argument::Word(value) }

fn enter(machine: &impl Machine, address: u32, arguments: &[Argument]) -> u32 { machine.enter_with(address, arguments, &mut []) }

fn read_bytes<const N: usize>(machine: &impl Machine, address: u32) -> [u8; N] {
    core::array::from_fn(|index| {
        let at = address.wrapping_add(index as u32);
        machine.read_u8(at)
    })
}

fn write_bytes(machine: &mut impl Machine, address: u32, bytes: &[u8]) {
    for (offset, &byte) in bytes.iter().enumerate() {
        machine.write_u8(address.wrapping_add(offset as u32), byte);
    }
}

/// 0xfa7bb8 over the five input words and the grid.
pub fn fill_grid(machine: &impl Machine, inputs: &GridInputs, grid: &mut AcquisitionGrid) {
    xm1_saclac_interface::stock::round_acquisition::set_grid_inputs(grid, inputs.timing.0, inputs.lens_word, inputs.positions, inputs.window);
    let mut buffer = Aligned(grid.0);
    machine.enter_with(0xfa_7bb8, &[Argument::Buffer(0x418), Argument::Buffer(0)], &mut buffer);
    grid.0 = buffer.0;
}


/// 0xf625cc.
pub fn five_parameter_adjustment(machine: &impl Machine, arguments: [u32; 5]) -> u32 { enter(machine, 0xf6_25cc, &arguments.map(word)) }

/// 0xfa6934 with its last two arguments 0.
pub fn rate_divisor(machine: &impl Machine, updated_position: u32, change_category: u32, lens: ResourceIndex, grid_halfword: u16) -> u32 {
    let arguments = [word(updated_position), word(change_category), word(lens.0), word(u32::from(grid_halfword)), word(0), word(0)];
    enter(machine, 0xfa_6934, &arguments)
}

/// 0xf5f368.
pub fn clamp_selection_difference(machine: &impl Machine, mode: u32, lens: ResourceIndex, level: i16, lens_halfword: u16, lens_reference: u32, target: i16) -> i16 {
    let arguments = [word(mode), word(lens.0), signed(level), signed(lens_halfword as i16), word(lens_reference), signed(target)];
    enter(machine, 0xf5_f368, &arguments) as i16
}

/// 0xf624a8 writes the setting from +0x1c, in place where calc1 reads it; the window takes back
/// the signed level at +0x1e and the flag word at +0x28.
pub fn capture_selection_position(machine: &impl Machine, setting: SharedAddress, branch: Branch, inputs: &PositionInputs) -> SelectionPosition {
    let arguments = [
        word(setting.0 + 0x1c), word(inputs.word), signed(inputs.level), word(inputs.updated_position),
        word(u32::from(inputs.position_halfword)), word(inputs.mode_word), word(u32::from(inputs.byte)), word(u32::from(inputs.halfword)),
        word(match branch { Branch::Live => 0, Branch::S1 => 1 }),
    ];
    enter(machine, 0xf6_24a8, &arguments);
    SelectionPosition { level: machine.read_u16(setting.0 + 0x1e), flag: machine.read_u32(setting.0 + 0x28) }
}


/// The owned rate/status renderer formerly at0xf521d8.
pub fn draw_rate_status(machine: &impl Machine, level: u16, status: u32, flag: u32) {
    crate::shot_absorb_diagnostics::draw_rate_status(machine, crate::shot_absorb_diagnostics::RateTenths(level), xm1_saclac_calc::diagnostics::RateStatus::from_code(status), flag != 0);
}



/// 256 statistics halfwords from the buffer's address.
pub fn statistics_values(machine: &impl Machine, buffer: StatisticsBuffer) -> [u16; 256] {
    core::array::from_fn(|index| machine.read_u16(buffer.0 + 2 * index as u32))
}

/// Where the release takes the extra statistics from (0xf350c8).
const EXTRA_STATISTICS: u32 = 0x5ffc_6340;

pub fn extra_statistics(machine: &impl Machine, values: &mut [u16]) {
    for (index, value) in values.iter_mut().enumerate() {
        *value = machine.read_u16(EXTRA_STATISTICS + 2 * index as u32);
    }
}

/// The pipeline reference halfword at 0x170380a.
pub fn set_pipeline_reference(machine: &mut impl Machine, level: i16) { machine.write_u16(0x0170_380a, level as u16); }

/// The pipeline slot of each capture, which 0xfea79c captures and 0xfea7ac copies out.
const fn slot(statistics: Statistics) -> u32 {
    match statistics {
        Statistics::Primary => 0,
        Statistics::Search => 1,
        Statistics::Auxiliary => 2,
        Statistics::History => 4,
    }
}

pub fn capture(machine: &impl Machine, statistics: Statistics) { enter(machine, 0xfe_a79c, &[word(slot(statistics))]); }

pub fn snapshot(machine: &impl Machine, statistics: Statistics) -> [u8; SNAPSHOT_BYTES] {
    let mut buffer = Aligned([0; SNAPSHOT_BYTES]);
    machine.enter_with(0xfe_a7ac, &[word(slot(statistics)), Argument::Buffer(0)], &mut buffer);
    buffer.0
}

/// 0xfeaacc(0) prepares slot 0's S1 sample; 0xfeaadc(0, out) copies it.
pub fn prepare_s1_sample(machine: &impl Machine) { enter(machine, 0xfe_aacc, &[word(0)]); }

pub fn s1_sample(machine: &impl Machine) -> [u8; S1_SAMPLE_BYTES] {
    let mut buffer = Aligned([0; S1_SAMPLE_BYTES]);
    machine.enter_with(0xfe_aadc, &[word(0), Argument::Buffer(0)], &mut buffer);
    buffer.0
}

/// An entry that writes one word through its last argument: what it returns and the word.
fn word_out(machine: &impl Machine, address: u32, lead: Option<Argument>) -> (u32, u32) {
    let mut buffer = Aligned([0; 4]);
    let returned = match lead {
        Some(lead) => machine.enter_with(address, &[lead, Argument::Buffer(0)], &mut buffer),
        None => machine.enter_with(address, &[Argument::Buffer(0)], &mut buffer),
    };
    (returned, u32::get(&buffer, 0))
}

/// 0xfea7bc.
pub fn timing(machine: &impl Machine) -> TimingWord { TimingWord(word_out(machine, 0xfe_a7bc, None).1) }

/// 0xfa95a8: a nonzero return leaves the interval unknown.
pub fn frame_interval(machine: &impl Machine, timing: TimingWord) -> Option<u32> {
    match word_out(machine, 0xfa_95a8, Some(word(timing.0))) {
        (0, interval) => Some(interval),
        _ => None,
    }
}

/// 0x103b064.
pub fn request_mode_six_statistics(machine: &impl Machine) { enter(machine, 0x103_b064, &[]); }

/// 0xfea7cc writes 1 when the pipeline raised the window refresh (0xf2fec4).
pub fn take_window_refresh(machine: &impl Machine) -> bool { word_out(machine, 0xfe_a7cc, None).1 == 1 }

/// The history capture's banks (0x5ffee314, 0x5ffee414), its word (0x5ffee514) and the two
/// halfword banks from 0x5ffee518, eight cells a row (0xf32838..0xf32878).
pub fn history_statistics(machine: &impl Machine, cell: GridCell) -> HistoryStatistics {
    let bank = |base: u32| core::array::from_fn(|index| machine.read_u32(base + 4 * index as u32));
    let at = 0x5ffe_e518 + 2 * (cell.column + 8 * cell.row);
    HistoryStatistics {
        banks: [bank(0x5ffe_e314), bank(0x5ffe_e414)],
        word: PipelineWord(machine.read_u32(0x5ffe_e514)),
        cell_levels: [machine.read_u16(at), machine.read_u16(at + 128)],
    }
}

/// Where 0xfea5c0 copies the selection table and 0xfeab3c the focus table before posting them.
pub const SELECTION_TABLE: u32 = 0x16f_4488;
pub const FOCUS_TABLE: u32 = 0x16f_224c;

pub fn place(machine: &mut impl Machine, address: u32, bytes: &[u8]) { write_bytes(machine, address, bytes) }

/// The shot's reference level (0xf36778) and selection word (0xf367f4).
pub fn shot_reference_level(machine: &impl Machine) -> u16 { machine.read_u16(0x16b_3d38) }

pub fn shot_selection_word(machine: &impl Machine) -> u32 { machine.read_u32(0x16a_d738) }


pub const fn region(branch: Branch) -> SelectionRegion {
    match branch {
        Branch::Live => LIVE_REGION,
        Branch::S1 => S1_REGION,
    }
}

/// Where each branch keeps the grid its window names (0xf36e74 with 0x16bf370, 0xf250e8 with
/// 0x16b6030).
pub const fn grid_address(branch: Branch) -> SharedAddress {
    match branch {
        Branch::Live => SharedAddress(0x16b_f370),
        Branch::S1 => SharedAddress(0x16b_6030),
    }
}


/// The saved setting prefix 0xf1fbe8 copies from the shot record.
pub fn write_saved_settings(machine: &mut impl Machine, record: SharedAddress) {
    for offset in 0..0x9c {
        let byte = machine.read_u8(record.0 + offset);
        machine.write_u8(0x16b_510c + offset, byte);
    }
}


/// 0x7407b0.
pub fn refresh_hardware_state(machine: &impl Machine) { enter(machine, 0x74_07b0, &[]); }

/// 0x74081c.
pub fn hardware_ready(machine: &impl Machine) -> bool { enter(machine, 0x74_081c, &[]) != 0 }

/// The second output of 0x740888 reads the current hardware clock word.
pub fn clock_word(machine: &impl Machine) -> u32 { machine.read_u32(0x0091_4df0) }

pub fn write_record(machine: &mut impl Machine, at: SharedAddress, offset: usize, bytes: &[u8]) {
    write_bytes(machine, at.0.wrapping_add(offset as u32), bytes)
}

pub fn read_record<const N: usize>(machine: &impl Machine, at: SharedAddress) -> [u8; N] { read_bytes(machine, at.0) }

/// The callback calc1 enters for each branch's 0x3d00 and the completion it posts back.
pub const fn acquisition_callback(branch: Branch) -> (StockCallbackWord, Completion) {
    match branch {
        Branch::Live => (StockCallbackWord(0xf3_3568), Completion::Calculation87),
        Branch::S1 => (StockCallbackWord(0xf2_41a8), Completion::Calculation9b),
    }
}

/// The callback calc1 enters for each branch's 0x3d01 and the completion it posts back.
pub const fn selection_callback(branch: Branch) -> (StockCallbackWord, Completion) {
    match branch {
        Branch::Live => (StockCallbackWord(0xf3_3520), Completion::Calculation88),
        Branch::S1 => (StockCallbackWord(0xf2_41d4), Completion::Calculation9c),
    }
}

/// 0x740e3c cleans the copied grid from the data cache before calc1 reads it (0xf4c584).
pub fn clean(machine: &impl Machine, address: u32, bytes: usize) { enter(machine, 0x74_0e3c, &[word(address), word(bytes as u32)]); }

/// 0x740e74 hands a record calc1 filled back to it.
pub fn release(machine: &impl Machine, record: SharedAddress, bytes: usize) { enter(machine, 0x74_0e74, &[word(record.0), word(bytes as u32)]); }

/// The completion's second parameter word names the result record (0xf35168, 0xf24704).
pub fn result_address(parameters: &[u8]) -> Option<SharedAddress> {
    let bytes = parameters.get(4..8)?;
    match u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) {
        0 => None,
        address => Some(SharedAddress(address)),
    }
}

/// 0x72872c's trace point for each request (0xf3a714, 0xf38584, 0xf39750, 0xf35cd4, 0xf377ec).
pub const fn trace_point(mark: TraceMark) -> u32 {
    match mark {
        TraceMark::WindowRequest => 0x0110_7000,
        TraceMark::AuxiliaryRequest => 0x0110_4000,
        TraceMark::SourceRequest => 0x0110_5000,
        TraceMark::AcquisitionRequest => 0x0110_1000,
        TraceMark::SelectionRequest => 0x0110_2000,
    }
}

pub fn trace(machine: &impl Machine, point: u32) { enter(machine, 0x72_872c, &[word(point)]); }

/// The selection hook 0x756b9c names takes 13 arguments with 0 seventh (0xf3664c, 0xf34704); the
/// rate hook 0x756d04 names takes 5 (0xf24734).
pub fn enter_selection_hook(machine: &impl Machine, hook: StockCallbackWord, notice: &SelectionNotice) {
    if hook.0 == 0 { return; }
    let [first, last] = notice.flags;
    let [l0, l1, l2, l3] = notice.levels.map(signed);
    let arguments = [
        word(notice.kind), signed(notice.level), word(notice.word), signed(notice.limit_level), word(first), word(last), word(0),
        word(notice.recording), l0, l1, l2, l3, word(notice.position),
    ];
    enter(machine, hook.0, &arguments);
}

pub fn enter_rate_hook(machine: &impl Machine, hook: StockCallbackWord, notice: &RateNotice) {
    if hook.0 == 0 { return; }
    let arguments = [word(notice.position), word(u32::from(notice.feature)), signed(notice.levels[0]), signed(notice.levels[1]), word(notice.word)];
    enter(machine, hook.0, &arguments);
}

/// The three grid setters copy the window's first seven words over the cells' first seven.
pub fn grid_records(grid: &AcquisitionGrid) -> ([u8; 1044], [u8; 40], [u8; 80]) {
    let publication = xm1_saclac_interface::stock::round_acquisition::grid_publication(grid);
    fn packed<const N: usize>((lead, body): ([u32; 3], &[u8])) -> [u8; N] {
        let mut bytes = [0; N];
        [0, 4, 8].put(&mut bytes, &lead);
        bytes[12..].copy_from_slice(body);
        bytes
    }
    let mut cells = packed::<1044>(publication.cells);
    let window = packed::<40>(publication.window);
    cells[12..40].copy_from_slice(&window[12..40]);
    (cells, window, packed(publication.entries))
}

pub fn statistics_in_use_word(value: xm1_saclac_profile_records::StatisticsInUse) -> u32 {
    value.primary as u32 | (value.search as u32) << 1 | (value.auxiliary as u32) << 2
}

pub fn statistics_in_use(word: u32) -> xm1_saclac_profile_records::StatisticsInUse {
    xm1_saclac_profile_records::StatisticsInUse { primary: word & 1 != 0, search: word & 2 != 0, auxiliary: word & 4 != 0 }
}

pub fn position_word(value: xm1_saclac_profile_records::WindowPosition) -> u32 {
    u32::from(value.0[0]) | u32::from(value.0[1]) << 16
}

pub fn controller_record(machine: &impl Machine) -> RecordHandle {
    xm1_saclac_interface::stock::record_handle_from_a2(machine.read_u32(0x16b_e01c))
}

pub fn current_lens(machine: &impl Machine) -> u32 { enter(machine, super::RESOURCE_INDEX, &[]) }

/// The capture profile selects one of two signed level offsets.
pub fn timing_offset(machine: &impl Machine, timing: TimingWord) -> u16 {
    let row = 0x00ef_6de4u32.wrapping_add(timing.0.wrapping_mul(20));
    let selector = u32::from(machine.read_u8(row.wrapping_add(14)));
    machine.read_u16(0x00ef_66d0 + if selector > 1 { 0 } else { selector * 2 })
}

/// The grid 0xf4c584 places before posting calc1 0x3d00.
pub const CALC1_GRID: SharedAddress = SharedAddress(0x16d_4280);
pub const SHOT_RECORD_BYTES: usize = 0x7c;
pub const SAVED_RECORD_BYTES: usize = 0x63ec;

/// The classes, four offset banks, and optional extra and primary-statistics copy (0xf34c98).
pub fn store_release(machine: &mut impl Machine, release: &xm1_focus_measurement_round::acquisition::ports::Release<'_>) {
    use xm1_focus_measurement_round::stock as round;
    write_bytes(machine, round::RELEASE_CLASSES.0, release.classes);
    for (bank, values) in release.offsets.iter().enumerate() {
        let base = round::RELEASE_OFFSETS.0 + 512 * bank as u32;
        for (index, value) in values.iter().enumerate() { write_bytes(machine, base + 2 * index as u32, &value.to_le_bytes()); }
    }
    if let Some(snapshot) = release.snapshot {
        for (index, value) in snapshot.extra.iter().enumerate() { write_bytes(machine, round::RELEASE_EXTRA.0 + 2 * index as u32, &value.to_le_bytes()); }
        for offset in (0..0x1150).step_by(4) {
            machine.write_u32(snapshot.copy.0 + offset, machine.read_u32(0x5ffe_c1b4 + offset));
        }
    }
}

pub fn fault(machine: &impl Machine) { enter(machine, super::FAULT, &[]); }
