use crate::children::MeasurementUpdate;
use crate::layers::*;
use crate::live_statistics::LiveStatistics;
use crate::live_statistics_profile::{Layers, Leaf};
use crate::live_view::{Signal, StatisticsCall, StatisticsControl as Ctl, StatisticsOutcome as O};
use crate::replies::{Replies, ReplyCommand, Values};
use std::{vec, vec::Vec};
use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_interface::statistics::{Source, RESULTS_BYTES};
use xm1_saclac_interface::{
    stock, ClientCallback, Command, CommandKind as K, Completed, Completion as C, CompletionStatus, RecordHandle,
    Statistics, StatisticsOperation as Op, StatisticsRequest,
};

#[derive(Clone, Debug, Eq, PartialEq)]
enum Seen {
    Post(Op, Vec<u8>, Vec<C>),
    Reply(K, i32, Vec<u8>),
    Write(Field, u32),
    Feature(bool),
    RowData(u32),
    Debug(u32),
    Delivered(u32),
    ResetSelection,
    Fault,
    /// A profile operation the configuration never makes.
    Unexpected(&'static str),
}

struct Fake {
    fields: Vec<(Field, u32)>,
    window: [u32; 4],
    changes: Vec<Change>,
    parameters: Vec<(K, Vec<u32>)>,
    feature: bool,
    exports: DiagnosticExports,
    capture_windows: bool,
    first_dispatch: u32,
    source: Source,
    results: [u8; RESULTS_BYTES],
    seen: Vec<Seen>,
}

impl Fake {
    fn new(fields: &[(Field, u32)]) -> Self {
        Self {
            fields: fields.to_vec(),
            window: [0x21, 0x22, 0x23, 0x24],
            changes: Vec::new(),
            parameters: Vec::new(),
            feature: false,
            exports: DiagnosticExports::default(),
            capture_windows: true,
            first_dispatch: 0,
            source: Source { word: 0x0bad_0208, byte: 0x29 },
            results: core::array::from_fn(|index| (index % 253) as u8),
            seen: Vec::new(),
        }
    }
    fn field(&self, field: Field) -> u32 {
        self.fields.iter().rev().find(|(known, _)| *known == field).map_or(0, |(_, value)| *value)
    }
    fn take(&mut self) -> Vec<Seen> { core::mem::take(&mut self.seen) }
    fn posts(&mut self) -> Vec<(Op, Vec<u8>, Vec<C>)> {
        self.take().into_iter().filter_map(|seen| match seen {
            Seen::Post(op, bytes, completions) => Some((op, bytes, completions)),
            _ => None,
        }).collect()
    }
}

impl ProfileRecord for Fake {
    fn read(&mut self, record: RecordHandle, field: Field) -> u32 { assert_eq!(record, self::record()); self.field(field) }
    fn write(&mut self, _: RecordHandle, field: Field, value: u32) {
        self.fields.push((field, value));
        self.seen.push(Seen::Write(field, value));
    }
    fn changed(&mut self, _: RecordHandle, change: Change) -> bool { self.changes.contains(&change) }
    fn read_block(&mut self, _: RecordHandle, block: Block, out: &mut [u32]) {
        assert_eq!(block, Block::AuxiliaryWindow);
        out.copy_from_slice(&self.window[..out.len()]);
    }
    fn write_block(&mut self, _: RecordHandle, _: Block, _: &[u32]) { self.seen.push(Seen::Unexpected("write_block")) }
    fn clear_selection(&mut self, _: RecordHandle) { self.seen.push(Seen::Unexpected("clear_selection")) }
    fn select_descriptor(&mut self, _: RecordHandle, _: u32) {}
    fn set_descriptor_pair(&mut self, _: RecordHandle, _: u32, _: u32) {}
    fn notify_selection_owner(&mut self, _: RecordHandle) {}
}

impl ProfileShared for Fake {
    fn measured_window(&mut self) -> ([u16; 3], u32) { ([0x201, 0x202, 0x203], 0x205) }
    fn source(&mut self) -> Source { self.source }
}

impl StatisticsGeometry for Fake {
    fn correct_window(&mut self, window: [u16; 3], _: u32, _: u32, _: u32) -> [u16; 3] { window }
    fn linearize(&mut self, value: u32) -> u32 { value }
    fn reset_default_selection(&mut self) { self.seen.push(Seen::ResetSelection) }
}

impl Adjustment for Fake {
    fn mode_position(&mut self, _: RecordHandle) -> u32 { 0x33 }
    fn refresh_dispatch(&mut self, _: RecordHandle) -> u32 { 0x44 }
    fn first_dispatch(&mut self, _: RecordHandle) -> u32 { self.first_dispatch }
}

impl StatisticsHelpers for Fake {
    fn scale_capture_window(&mut self, mode: u32, _: u32) -> Option<[u32; 2]> {
        self.capture_windows.then_some([0x100 + mode, 0x200 + mode])
    }
    fn selection_ratio(&mut self, _: RecordHandle) -> [u32; 2] { [3, 3] }
    fn lens_offset(&mut self, _: u32) -> u32 { 0 }
}

impl StatisticsSettings for Fake {
    fn parameter_feature(&mut self) -> bool { self.feature }
    fn set_parameter_feature(&mut self, on: bool) { self.seen.push(Seen::Feature(on)) }
    fn diagnostic_exports(&mut self) -> DiagnosticExports { self.exports }
    fn export_row_data(&mut self, buffer: u32) { self.seen.push(Seen::RowData(buffer)) }
    fn export_debug(&mut self, buffer: u32) { self.seen.push(Seen::Debug(buffer)) }
}

impl CommandParameters for Fake {
    fn word(&mut self, command: K, index: usize) -> u32 {
        self.parameters.iter().find(|(kind, _)| *kind == command).and_then(|(_, words)| words.get(index).copied()).unwrap_or(0)
    }
}

impl Results for Fake {
    fn address(&mut self) -> SharedAddress { SharedAddress(0x016c_0728) }
    fn read(&mut self, out: &mut [u8; RESULTS_BYTES]) { *out = self.results }
    fn deliver(&mut self, callback: ClientCallback) { self.seen.push(Seen::Delivered(stock::callback_word(callback))) }
}

impl Replies for Fake {
    fn reply(&mut self, command: ReplyCommand, status: CompletionStatus, values: Values<'_>) {
        let bytes = match values {
            Values::Words(words) => words.iter().flat_map(|word| word.to_le_bytes()).collect(),
            Values::Record(bytes) => bytes.to_vec(),
        };
        self.seen.push(Seen::Reply(command.kind(), status.0, bytes));
    }
}

impl Statistics for Fake {
    type Error = ();
    fn request(&mut self, request: StatisticsRequest<'_>) -> Result<(), ()> {
        self.seen.push(Seen::Post(request.operation, request.parameters.bytes().to_vec(), request.completions.to_vec()));
        Ok(())
    }
}

impl Layers for Fake {
    type Profile = Self;
    type Shared = Self;
    type Geometry = Self;
    type Adjustment = Self;
    type Helpers = Self;
    type Settings = Self;
    type Parameters = Self;
    type Results = Self;
    type Replies = Self;
    type Statistics = Self;
    fn profile(&mut self) -> &mut Self { self }
    fn shared(&mut self) -> &mut Self { self }
    fn geometry(&mut self) -> &mut Self { self }
    fn adjustment(&mut self) -> &mut Self { self }
    fn helpers(&mut self) -> &mut Self { self }
    fn settings(&mut self) -> &mut Self { self }
    fn parameters(&mut self) -> &mut Self { self }
    fn results(&mut self) -> &mut Self { self }
    fn replies(&mut self) -> &mut Self { self }
    fn statistics(&mut self) -> &mut Self { self }
    fn primary_applied(&mut self) -> bool { true }
    fn fault(&mut self) { self.seen.push(Seen::Fault) }
}

fn record() -> RecordHandle { stock::record_handle_from_a2(0x16a0_0490) }

fn control(statistics: &mut LiveStatistics, fake: &mut Fake, control: Ctl) -> O {
    statistics.send(StatisticsCall::Control(control), record(), &mut Leaf(fake))
}

fn command(statistics: &mut LiveStatistics, fake: &mut Fake, kind: K) -> O {
    let call = StatisticsCall::Forward(Signal::Command(Command { kind, parameters: stock::parameters(&[]) }));
    statistics.send(call, record(), &mut Leaf(fake))
}

fn completed(statistics: &mut LiveStatistics, fake: &mut Fake, completion: C, payload: &[u8]) -> O {
    let call = StatisticsCall::Forward(Signal::Completed(Completed { completion, parameters: stock::parameters(payload) }));
    statistics.send(call, record(), &mut Leaf(fake))
}

fn word(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) }
fn words(bytes: &[u8], offset: usize, count: usize) -> Vec<u32> { (0..count).map(|index| word(bytes, offset + 4 * index)).collect() }
fn half(bytes: &[u8], offset: usize) -> u16 { u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap()) }

/// A profile whose view mode needs the statistics started.
fn profile(extra: &[(Field, u32)]) -> Fake {
    let mut fields = vec![
        (Field::Descriptor, 0x11),
        (Field::DescriptorFirst, 0x12),
        (Field::DescriptorSecond, 0x13),
        (Field::FormatRatio, 0x0004_0003),
        (Field::FormatValue, 0x15),
        (Field::FormatMode, 7),
        (Field::ViewMode, 2),
        (Field::OutputFirst, 0x31),
        (Field::OutputThird, 0x33),
        (Field::OutputFourth, 0x34),
        (Field::SelectionPending, 0x41),
        (Field::StartHeader, 0x99),
        (Field::CaptureView, 0x77),
        (Field::ExposureSelected, 0x55),
    ];
    fields.extend_from_slice(extra);
    Fake::new(&fields)
}

fn running(fake: &mut Fake) -> LiveStatistics {
    let mut statistics = LiveStatistics::new();
    assert_eq!(control(&mut statistics, fake, Ctl::Begin), O::Pending);
    assert_eq!(completed(&mut statistics, fake, C::Statistics69, &[]), O::Complete);
    fake.take();
    statistics
}

fn captured(fake: &mut Fake) -> LiveStatistics {
    let mut statistics = LiveStatistics::new();
    assert_eq!(control(&mut statistics, fake, Ctl::Capture), O::Pending);
    assert_eq!(completed(&mut statistics, fake, C::Statistics6c, &[]), O::Complete);
    fake.take();
    statistics
}

#[test]
fn an_unselected_start_carries_the_profile_blocks_without_a_format_mode() {
    let mut fake = profile(&[]);
    fake.first_dispatch = 2;
    assert_eq!(control(&mut LiveStatistics::new(), &mut fake, Ctl::Begin), O::Pending);
    assert!(fake.seen.contains(&Seen::Write(Field::StartMode, 3)));
    let posts = fake.posts();
    assert_eq!(posts.len(), 1);
    let (op, bytes, completions) = &posts[0];
    assert_eq!((*op, bytes.len(), completions.as_slice()), (Op::Request02, 208, &[C::Statistics69, C::Statistics7f][..]));
    assert_eq!(words(bytes, 0, 4), [0x11, 0x12, 0x13, 0x11de_784a]);
    assert_eq!(words(bytes, 16, 2), [240, 640]);
    assert_eq!((half(bytes, 24), half(bytes, 26)), (3, 4));
    assert_eq!(words(bytes, 28, 3), [0x15, 1, 0], "format value, class and no mode");
    assert_eq!(words(bytes, 40, 4), [0x21, 0x22, 0x23, 0x24]);
    assert_eq!(words(bytes, 56, 4), [0x31, 2, 0x33, 0x34]);
    assert_eq!(words(bytes, 72, 8), [0, 0, 0, 0x44, 1, 0, 0, 0], "the fifth exposure setting is 1 without a selection");
    assert_eq!(words(bytes, 184, 3), [0x41, 0x016c_0728, 0x00f3_a794]);
    assert_eq!(words(bytes, 196, 3), [0x00f3_aa18, 1, 0x99]);
}

#[test]
fn the_parameter_feature_decides_the_start_mode_the_profile_records() {
    let mut fake = profile(&[]);
    fake.feature = true;
    control(&mut LiveStatistics::new(), &mut fake, Ctl::Begin);
    assert!(fake.seen.contains(&Seen::Write(Field::StartMode, 4)));
}

#[test]
fn a_selected_start_carries_the_format_mode_its_class_and_capture_window() {
    for (mode, class) in [(6, 9), (7, 9), (8, 8), (1, 8)] {
        let mut fake = profile(&[(Field::Selection, 1), (Field::FormatMode, mode)]);
        control(&mut LiveStatistics::new(), &mut fake, Ctl::Begin);
        assert!(!fake.seen.iter().any(|seen| matches!(seen, Seen::Write(Field::StartMode, _))));
        let posts = fake.posts();
        let (op, bytes, _) = &posts[0];
        assert_eq!((*op, bytes.len()), (Op::Request06, 204));
        assert_eq!(words(bytes, 16, 2), [0x100 + mode, 0x200 + mode]);
        assert_eq!(words(bytes, 32, 2), [class, mode]);
        assert_eq!(word(bytes, 72 + 16), 0x55, "the fifth exposure setting is the profile's with a selection");
        assert_eq!(words(bytes, 196, 2), [1, 0x00f3_aa18]);
    }
}

#[test]
fn a_selected_format_without_a_capture_window_carries_zero_dimensions() {
    let mut fake = profile(&[(Field::Selection, 1)]);
    fake.capture_windows = false;
    control(&mut LiveStatistics::new(), &mut fake, Ctl::Begin);
    assert_eq!(words(&fake.posts()[0].1, 16, 2), [0, 0]);
}

#[test]
fn the_view_mode_decides_whether_a_begin_starts_the_statistics() {
    for (mode, output, locked, starts) in [(0, 0, 0, false), (3, 0, 1, false), (3, 0, 0, true), (0, 1, 0, true), (2, 0, 1, true)] {
        let mut fake = profile(&[(Field::ViewMode, mode), (Field::OutputFirst, output), (Field::ViewLocked, locked)]);
        let outcome = control(&mut LiveStatistics::new(), &mut fake, Ctl::Begin);
        assert_eq!((outcome, fake.posts().len()), if starts { (O::Pending, 1) } else { (O::Complete, 0) }, "view mode {mode}");
    }
}

#[test]
fn the_capture_configuration_forwards_command09s_words_and_names_three_completions() {
    let mut fake = profile(&[]);
    fake.parameters.push((K::Command09, vec![0xc0, 0x91, 0x92, 0x93]));
    assert_eq!(control(&mut LiveStatistics::new(), &mut fake, Ctl::Capture), O::Pending);
    let posts = fake.posts();
    let (op, bytes, completions) = &posts[0];
    assert_eq!((*op, bytes.len()), (Op::Request08, 248));
    assert_eq!(completions.as_slice(), &[C::Statistics6c, C::Statistics78, C::Statistics79, C::Statistics7f]);
    assert_eq!(words(bytes, 196, 3), [1, 0, 0x77]);
    assert_eq!(words(bytes, 224, 6), [0x91, 0x92, 0x93, 0x00f3_a9c8, 0x00f3_a9a0, 0x00f3_aa40]);
}

#[test]
fn a_stop_reports_the_shot_state_and_restores_the_parameter_feature_the_profile_names() {
    for (stop_mode, restored) in [(3, Some(false)), (4, Some(true)), (0, None), (5, None)] {
        let mut fake = profile(&[(Field::StopMode, stop_mode)]);
        let mut statistics = running(&mut fake);
        statistics.set_shot_idle(false);
        assert_eq!(control(&mut statistics, &mut fake, Ctl::Pause), O::Pending);
        let seen = fake.take();
        assert_eq!(seen[0], Seen::Post(Op::Request07, [1u32, 0, 0x00f3_a9f0].iter().flat_map(|word| word.to_le_bytes()).collect(), vec![C::Statistics6b]));
        assert_eq!(seen.get(1).cloned(), restored.map(Seen::Feature), "stop mode {stop_mode}");
    }
}

#[test]
fn the_capture_results_reach_command09s_client_as_one_record() {
    let mut fake = profile(&[(Field::FormatMode, 5)]);
    let mut statistics = LiveStatistics::new();
    control(&mut statistics, &mut fake, Ctl::Capture);
    fake.take();
    let mut payload: [u8; 64] = core::array::from_fn(|index| 0x80 + index as u8);
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(completed(&mut statistics, &mut fake, C::Statistics79, &payload), O::Pending);
    let seen = fake.take();
    assert_eq!(seen[0], Seen::ResetSelection);
    let Seen::Reply(K::Command09, 17, record) = &seen[1] else { panic!("no capture report: {seen:?}") };
    assert_eq!(record.len(), 64 + RESULTS_BYTES);
    assert_eq!(&record[..12], &payload[4..16]);
    assert_eq!(words(record, 12, 2), [2 * 0x205, 0x105], "the capture window's second dimension doubled, then its first");
    assert_eq!(&record[20..24], &payload[16..20]);
    assert_eq!(&record[24..64], &payload[20..60]);
    assert_eq!(&record[64..], &fake.results[..]);
}

#[test]
fn capture_results_without_a_capture_window_or_selection_reset_carry_zero_dimensions() {
    let mut fake = profile(&[]);
    fake.capture_windows = false;
    let mut statistics = captured(&mut fake);
    completed(&mut statistics, &mut fake, C::Statistics79, &[0; 64]);
    let seen = fake.take();
    let [Seen::Reply(K::Command09, 17, record)] = seen.as_slice() else { panic!("{seen:?}") };
    assert_eq!(words(record, 12, 2), [0, 0]);
}

#[test]
fn the_results_go_to_the_callback_the_profile_names() {
    let mut fake = profile(&[(Field::ResultsCallback, 0x00e6_c578)]);
    let mut statistics = running(&mut fake);
    assert_eq!(completed(&mut statistics, &mut fake, C::Statistics7f, &[]), O::Pending);
    assert_eq!(fake.take(), [Seen::Delivered(0x00e6_c578)]);
    let mut fake = profile(&[]);
    let mut statistics = running(&mut fake);
    completed(&mut statistics, &mut fake, C::Statistics7f, &[]);
    assert_eq!(fake.take(), []);
}

#[test]
fn finishing_a_capture_release_exports_the_diagnostics_the_settings_enable() {
    for (buffer, exports, expected) in [
        (0x5000, DiagnosticExports { row_data: true, debug: true }, vec![Seen::RowData(0x5000), Seen::Debug(0x5000)]),
        (0x5000, DiagnosticExports { row_data: false, debug: true }, vec![Seen::Debug(0x5000)]),
        (0, DiagnosticExports { row_data: true, debug: true }, vec![]),
    ] {
        let mut fake = profile(&[(Field::DiagnosticBuffer, buffer)]);
        fake.exports = exports;
        let mut statistics = captured(&mut fake);
        assert_eq!(control(&mut statistics, &mut fake, Ctl::Release), O::Pending);
        let seen = fake.take();
        assert_eq!(seen, [
            Seen::Post(Op::Request09, [0u32, 0x00f3_a978].iter().flat_map(|word| word.to_le_bytes()).collect(), vec![C::Statistics6d]),
            Seen::Reply(K::Command0a, 20, vec![]),
        ]);
        assert_eq!(completed(&mut statistics, &mut fake, C::Statistics6d, &[]), O::Complete);
        assert_eq!(fake.take(), expected);
    }
}

fn autofocus_posts(selection: u32, changes: &[Change]) -> Vec<Op> {
    let mut fake = profile(&[(Field::Selection, selection)]);
    let mut statistics = running(&mut fake);
    fake.changes = changes.to_vec();
    let mut posts = Vec::new();
    let mut outcome = command(&mut statistics, &mut fake, K::Autofocus);
    while outcome == O::Pending {
        let [(op, _, completions)] = fake.posts().try_into().unwrap();
        posts.push(op);
        outcome = completed(&mut statistics, &mut fake, completions[0], &[]);
    }
    assert_eq!(outcome, O::Complete);
    posts
}

#[test]
fn an_autofocus_change_retunes_the_groups_its_changes_touch() {
    assert_eq!(autofocus_posts(0, &[]), []);
    assert_eq!(autofocus_posts(0, &[Change::Detail]), [Op::Request12]);
    assert_eq!(autofocus_posts(0, &[Change::Class, Change::FactorFirst]), [Op::Request10, Op::Request12]);
    assert_eq!(autofocus_posts(0, &[Change::CaptureRatio, Change::Offset]), [Op::Request2c, Op::Request12]);
    assert_eq!(autofocus_posts(0, &[Change::Resource, Change::Range]), []);
}

#[test]
fn with_selection_one_the_resource_and_range_decide_the_geometry_and_measurement() {
    assert_eq!(autofocus_posts(1, &[Change::Resource, Change::Range]), [Op::Request2c, Op::Request10]);
    assert_eq!(autofocus_posts(1, &[Change::Class, Change::Retune]), []);
    assert_eq!(autofocus_posts(2, &[Change::Class]), [Op::Request10], "only selection 1 switches the groups");
}

#[test]
fn a_measured_source_without_a_source_word_still_carries_its_byte_and_position() {
    let mut fake = profile(&[]);
    fake.source = Source { word: 0, byte: 0x5a };
    let mut statistics = LiveStatistics::new();
    assert_eq!(control(&mut statistics, &mut fake, Ctl::Measured(MeasurementUpdate::Source)), O::Pending);
    assert_eq!(fake.posts(), [(Op::Request41, vec![0, 0, 0, 0, 0x5a, 0, 0, 0, 0x33, 0, 0, 0], vec![])]);
}
