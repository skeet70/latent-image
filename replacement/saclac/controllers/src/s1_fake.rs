use crate::children::{ChildEvent, ChildOutcome, ChildRequest, Children};
use crate::layers::*;
use crate::replies::{Replies, ReplyCommand, Values};
use crate::s1_statistics_profile::Layers;
use std::vec::Vec;
use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_interface::statistics::Source;
use xm1_saclac_interface::{
    stock, ClientCallback, CommandKind as K, Completion as C, CompletionStatus, ImgDc, ImgDcOperation, ImgDcRequest,
    RecordHandle, Statistics, StatisticsOperation as Op, StatisticsRequest,
};

pub(crate) fn record() -> RecordHandle { stock::record_handle_from_a2(0x17) }
pub(crate) fn live_record() -> RecordHandle { stock::record_handle_from_a2(0x26) }

/// What a child was asked: a control by its name, a forwarded completion by its code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Child {
    Acquisition(&'static str),
    Search(&'static str),
    Completed(C),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Seen {
    Write(Field, u32),
    Block(Block, Vec<u32>),
    Statistics(Op, Vec<u8>, Vec<C>),
    Imaging(ImgDcOperation, Vec<u8>, Vec<C>),
    LensOption,
    Child(Child),
    RecordModeBegun,
    RecordModeFinished,
    Reply(ReplyCommand, i32, Vec<u32>),
    LensReply,
    Bounds(u32, [u32; 3]),
    SelectionChanged,
    Remap(RemapRequest),
    Move(LensMove),
    Query(RecordQuery),
    Delivered(u32),
    Fault,
}

pub(crate) const DESCRIPTOR: u32 = 0xdddd;
pub(crate) const LENS_VALUE: u32 = 0x80;
pub(crate) const LENS_POSITION: u32 = 0x100;
pub(crate) const MODE_INDEX: u32 = 2;
pub(crate) const REFERENCE: u32 = 0x340;
pub(crate) const STEP: u32 = 0xc4c;
pub(crate) const CHANNEL: (u32, u16) = (5, 0x8000);
pub(crate) const REMAPPED: u16 = 0x200;
pub(crate) const TICKS: u32 = 0x7777;

#[derive(Default)]
pub(crate) struct Fake {
    pub fields: Vec<(Field, u32)>,
    pub live: Vec<(Field, u32)>,
    pub blocks: Vec<(Block, Vec<u32>)>,
    pub changes: Vec<Change>,
    pub parameters: Vec<(K, Vec<u32>)>,
    pub overridden: Vec<LensProbe>,
    pub record_mode_available: bool,
    pub refuses_selection: bool,
    pub interpolates: bool,
    pub table_disabled: bool,
    pub unselected: bool,
    pub searched: u32,
    pub extended: bool,
    pub records: Vec<u8>,
    pub child_outcome: Option<ChildOutcome>,
    pub seen: Vec<Seen>,
}

impl Fake {
    pub fn with(fields: &[(Field, u32)]) -> Self { Self { fields: fields.to_vec(), ..Self::default() } }
    pub fn take_seen(&mut self) -> Vec<Seen> { core::mem::take(&mut self.seen) }
    pub fn statistics(&mut self) -> (Op, Vec<u8>, Vec<C>) {
        match self.take_seen().pop() {
            Some(Seen::Statistics(op, bytes, completions)) => (op, bytes, completions),
            other => panic!("expected a statistics post, saw {other:?}"),
        }
    }
    pub fn block_words(&self, block: Block) -> Option<&Vec<u32>> {
        self.blocks.iter().rev().find(|(known, _)| *known == block).map(|(_, words)| words)
    }
}

impl ProfileRecord for Fake {
    fn read(&mut self, record: RecordHandle, field: Field) -> u32 {
        let fields = if record == live_record() { &self.live } else { &self.fields };
        fields.iter().rev().find(|(known, _)| *known == field).map_or(0, |(_, value)| *value)
    }
    fn write(&mut self, record: RecordHandle, field: Field, value: u32) {
        assert_eq!(record, self::record());
        self.fields.push((field, value));
        self.seen.push(Seen::Write(field, value))
    }
    fn changed(&mut self, _: RecordHandle, change: Change) -> bool { self.changes.contains(&change) }
    fn read_block(&mut self, _: RecordHandle, block: Block, out: &mut [u32]) {
        if let Some(words) = self.block_words(block) {
            out.copy_from_slice(&words[..out.len()]);
        }
    }
    fn write_block(&mut self, record: RecordHandle, block: Block, words: &[u32]) {
        assert_eq!(record, self::record());
        self.blocks.push((block, words.to_vec()));
        self.seen.push(Seen::Block(block, words.to_vec()))
    }
    fn clear_selection(&mut self, _: RecordHandle) {}
    fn select_descriptor(&mut self, _: RecordHandle, _: u32) {}
    fn set_descriptor_pair(&mut self, _: RecordHandle, _: u32, _: u32) {}
    fn notify_selection_owner(&mut self, _: RecordHandle) {}
}

impl ProfileShared for Fake {
    fn measured_window(&mut self) -> ([u16; 3], u32) { ([1, 2, 3], 4) }
    fn source(&mut self) -> Source { Source { word: 0x5150, byte: 0x51 } }
}

impl StatisticsHelpers for Fake {
    fn scale_capture_window(&mut self, mode: u32, _: u32) -> Option<[u32; 2]> { (mode != 0).then_some([0x100 + mode, 0x200 + mode]) }
    fn selection_ratio(&mut self, _: RecordHandle) -> [u32; 2] { [4, 5] }
    fn lens_offset(&mut self, _: u32) -> u32 { 0 }
}

impl StatisticsGeometry for Fake {
    fn correct_window(&mut self, window: [u16; 3], _: u32, _: u32, _: u32) -> [u16; 3] { window }
    fn linearize(&mut self, value: u32) -> u32 { value + 1 }
    fn reset_default_selection(&mut self) {}
}

impl S1Values for Fake {
    fn descriptor(&mut self, _: RecordHandle) -> u32 { DESCRIPTOR }
    fn lens_value(&mut self) -> u32 { 0x42 }
    fn refresh(&mut self, _: RecordHandle) -> u32 { 0x78 }
    fn curve(&mut self, inputs: [u32; 11]) -> u32 { inputs.iter().sum() }
    fn stop_curve(&mut self, stop_mode: u32) -> u32 { 0xc000 + stop_mode }
    fn unselected_view(&mut self, _: RecordHandle) -> u32 { 0xb4 }
    fn span(&mut self, _: RecordHandle) -> u32 { 0xd8 }
    fn unselected(&mut self, _: RecordHandle) -> u32 { u32::from(self.unselected) }
    fn corrected_window(&mut self, inputs: &WindowInputs) -> CorrectedWindow {
        let window = inputs.measured.unwrap_or_default();
        CorrectedWindow { window: [window[0] + 10, window[1] + 10, window[2] + 10], extent: 0x54 }
    }
    fn release_steps(&mut self, position: i16, first: u32, second: u32) -> u16 { position as u16 + first as u16 + second as u16 }
    fn trace_snapshot(&mut self) {}
    fn release_channel(&mut self) {}
    fn searched_lens_pair(&mut self, _: RecordHandle) -> u32 { self.searched }
    fn extended_results(&mut self) -> bool { self.extended }
    fn query_records(&mut self, query: &RecordQuery, out: &mut [u8; RECORD_QUERY_BYTES]) {
        self.seen.push(Seen::Query(*query));
        out[..self.records.len()].copy_from_slice(&self.records);
    }
    fn ticks(&mut self) -> u32 { TICKS }
    fn admits_selection(&mut self, _: u32, _: u32) -> bool { !self.refuses_selection }
}

impl Results for Fake {
    fn address(&mut self) -> SharedAddress { SharedAddress(0x016b_5b30) }
    fn read(&mut self, _: &mut [u8; xm1_saclac_interface::statistics::RESULTS_BYTES]) {}
    fn deliver(&mut self, callback: ClientCallback) { self.seen.push(Seen::Delivered(stock::callback_word(callback))) }
}

impl Statistics for Fake {
    type Error = ();
    fn request(&mut self, request: StatisticsRequest<'_>) -> Result<(), ()> {
        self.seen.push(Seen::Statistics(request.operation, request.parameters.bytes().to_vec(), request.completions.to_vec()));
        Ok(())
    }
}

impl ImgDc for Fake {
    type Error = ();
    fn request(&mut self, request: ImgDcRequest<'_>) -> Result<(), ()> {
        self.seen.push(Seen::Imaging(request.operation, request.parameters.bytes().to_vec(), request.completions.to_vec()));
        Ok(())
    }
}

fn child<T: core::fmt::Debug>(event: ChildEvent<'_, T>, named: fn(&'static str) -> Child, names: fn(T) -> &'static str) -> Child {
    match event {
        ChildEvent::Control { control, record, .. } => {
            assert_eq!(record, self::record());
            named(names(control))
        }
        ChildEvent::Completed { completed, .. } => Child::Completed(completed.completion),
        ChildEvent::Command { .. } => panic!("the S1 statistics configuration forwards no command to a child"),
    }
}

impl Children for Fake {
    fn handle(&mut self, request: ChildRequest<'_>) -> ChildOutcome {
        use crate::children::{AcquisitionControl as A, SearchControl as S};
        let seen = match request {
            ChildRequest::S1Acquisition(event) => child(event, Child::Acquisition, |control| match control {
                A::Initialize => "initialize",
                A::Start => "start",
                A::Stop => "stop",
            }),
            ChildRequest::S1Search(event) => child(event, Child::Search, |control| match control {
                S::Initialize => "initialize",
                S::Start => "start",
                S::Stop => "stop",
                S::Conclude => "conclude",
                S::Cancel => "cancel",
                S::Resume => "resume",
            }),
            other => panic!("the S1 statistics configuration has no {other:?} child"),
        };
        self.seen.push(Seen::Child(seen));
        self.child_outcome.unwrap_or(ChildOutcome::Pending)
    }
}

impl RecordMode for Fake {
    fn initialize(&mut self) -> bool { panic!("the S1 record mode does not initialize") }
    fn available(&mut self, channel: RecordChannel) -> bool {
        assert_eq!(channel, RecordChannel::S1);
        self.record_mode_available
    }
    fn begin(&mut self, channel: RecordChannel) {
        assert_eq!(channel, RecordChannel::S1);
        self.seen.push(Seen::RecordModeBegun)
    }
    fn finish(&mut self) { self.seen.push(Seen::RecordModeFinished) }
}

impl Replies for Fake {
    fn reply(&mut self, command: ReplyCommand, status: CompletionStatus, values: Values<'_>) {
        let Values::Words(words) = values else { panic!("the S1 replies carry words") };
        self.seen.push(Seen::Reply(command, status.0, words.to_vec()))
    }
}

impl CommandParameters for Fake {
    fn word(&mut self, command: K, index: usize) -> u32 {
        self.parameters.iter().find(|(known, _)| *known == command).and_then(|(_, words)| words.get(index).copied()).unwrap_or(0)
    }
}

impl LensControl for Fake {
    fn overridden(&mut self, probe: LensProbe) -> bool { self.overridden.contains(&probe) }
    fn value(&mut self) -> u32 { LENS_VALUE }
    fn position(&mut self) -> u32 { LENS_POSITION }
    fn mode_index(&mut self) -> u32 { MODE_INDEX }
    fn reference(&mut self) -> u32 { REFERENCE }
    fn step(&mut self) -> u32 { STEP }
    fn channel(&mut self) -> (u32, u16) { CHANNEL }
}

/// Levels are the steps; positions name their level, channel and the query's range or step;
/// a pair maps to its second word; combining sums the three words.
impl LensCurve for Fake {
    fn level(&mut self, steps: u16, _: u32, reference: u32, mode_index: u32) -> i16 {
        assert_eq!((reference, mode_index), (REFERENCE, MODE_INDEX));
        steps as i16
    }
    fn interpolates(&mut self) -> bool { self.interpolates }
    fn positions(&mut self, level: i16, channel: u32, query: CurveQuery) -> [u32; 3] { [level as u32, channel, query.range] }
    fn interpolated_positions(&mut self, level: i16, channel: u32, fraction: u16, query: CurveQuery) -> Option<[u32; 3]> {
        assert_eq!(fraction, CHANNEL.1);
        (!self.table_disabled).then_some([level as u32, channel, query.step])
    }
    fn mapped_value(&mut self, second: u32, _: u32) -> u32 { second }
    fn combine_lens_pair(&mut self, first: u32, second: u32, third: u32) -> u32 { first + second + third }
}

impl Layers for Fake {
    type Profile = Self;
    type Shared = Self;
    type Helpers = Self;
    type Geometry = Self;
    type Values = Self;
    type Results = Self;
    type Statistics = Self;
    type Imaging = Self;
    type Children = Self;
    type RecordMode = Self;
    type Replies = Self;
    type Parameters = Self;
    type Lens = Self;
    type Curve = Self;
    fn profile(&mut self) -> &mut Self { self }
    fn shared(&mut self) -> &mut Self { self }
    fn helpers(&mut self) -> &mut Self { self }
    fn geometry(&mut self) -> &mut Self { self }
    fn values(&mut self) -> &mut Self { self }
    fn results(&mut self) -> &mut Self { self }
    fn statistics(&mut self) -> &mut Self { self }
    fn imaging(&mut self) -> &mut Self { self }
    fn children(&mut self) -> &mut Self { self }
    fn record_mode(&mut self) -> &mut Self { self }
    fn replies(&mut self) -> &mut Self { self }
    fn parameters(&mut self) -> &mut Self { self }
    fn lens(&mut self) -> &mut Self { self }
    fn curve(&mut self) -> &mut Self { self }
    fn live_statistics_record(&mut self) -> RecordHandle { live_record() }
    fn lens_option(&mut self, record: RecordHandle) {
        assert_eq!(record, self::record());
        self.seen.push(Seen::LensOption);
    }
    fn reply_autofocus_lens(&mut self) { self.seen.push(Seen::LensReply) }
    fn notify_bounds(&mut self, detail: u32, bounds: [u32; 3]) { self.seen.push(Seen::Bounds(detail, bounds)) }
    fn mark_selection_changed(&mut self) { self.seen.push(Seen::SelectionChanged) }
    fn remap_lens_target(&mut self, request: RemapRequest) -> u16 {
        self.seen.push(Seen::Remap(request));
        REMAPPED
    }
    fn move_lens(&mut self, request: LensMove) { self.seen.push(Seen::Move(request)) }
    fn fault(&mut self) { self.seen.push(Seen::Fault) }
}
