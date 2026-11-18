#![forbid(unsafe_code)]

//! The S1 statistics configuration's port over its layers: its children, record mode, replies,
//! the parameter applies and the conditions it branches on. Where stock reads the live view's
//! or the live measurement's record, the leaf reads the record it is entered with.

use crate::children::{AcquisitionControl, ChildEvent, ChildOutcome, ChildRequest, Children, SearchControl};
use crate::layers::{
    clear_words, Block, Change, CommandParameters, Field, LensControl, LensProbe, ProfileRecord, RecordChannel, RecordMode,
    RecordQuery, ResultIndex, Restarting, Results, S1Values, RECORD_QUERY_BYTES,
};
use crate::live_profile::{apply_preparation, preparation_requested, LiveProfile, SelectionWords};
use crate::live_view::PreparationOverride;
use crate::replies::{Replies, ReplyCommand, Values};
use crate::s1_statistics::{Apply, AutofocusChange, Condition, Port, Reply, Request, Snapshot};
use crate::s1_statistics_profile::{block, read, request, take, Layers, Taken};
use xm1_saclac_interface::{stock, CommandKind as K, CompletionStatus, RecordHandle};

/// The status Command17's reply carries when the profile geometry refuses its selection.
const SELECTION_REFUSED: CompletionStatus = CompletionStatus(-96);

const RESULTS_QUERY_MODE: u32 = 8;

/// The S1 statistics configuration's leaf state: the snapshot its requests carry and the last
/// result records its results apply queried.
pub struct S1Profile {
    taken: Taken,
    records: [u8; RECORD_QUERY_BYTES],
}

impl S1Profile {
    pub fn new() -> Self { Self { taken: Taken::default(), records: [0; RECORD_QUERY_BYTES] } }
}

impl Default for S1Profile {
    fn default() -> Self { Self::new() }
}

pub struct Leaf<'a, L> {
    pub layers: &'a mut L,
    pub profile: &'a mut S1Profile,
    pub live: &'a mut LiveProfile,
    pub preparation: &'a mut PreparationOverride,
}

impl<L: Layers> Port for Leaf<'_, L> {
    fn acquisition(&mut self, event: ChildEvent<'_, AcquisitionControl>) -> ChildOutcome {
        self.layers.children().handle(ChildRequest::S1Acquisition(event))
    }

    fn search(&mut self, event: ChildEvent<'_, SearchControl>) -> ChildOutcome {
        self.layers.children().handle(ChildRequest::S1Search(event))
    }

    fn snapshot(&mut self, record: RecordHandle) -> Snapshot {
        self.profile.taken = take(record, self.layers);
        Snapshot { view_active: self.profile.taken.view_active() }
    }
    fn report_results(&mut self, open: bool) { self.layers.report_results(open); }


    fn record_mode(&mut self, held: bool) {
        let mode = self.layers.record_mode();
        if !mode.available(RecordChannel::S1) {
            return;
        }
        if held { mode.begin(RecordChannel::S1) } else { mode.finish() }
    }

    fn request(&mut self, sent: Request, record: RecordHandle) { request(sent, &self.profile.taken, record, self.layers) }

    fn reply(&mut self, reply: Reply, record: RecordHandle) {
        match reply {
            Reply::Autofocus => self.layers.reply_autofocus_lens(),
            Reply::Command02 => profile_reply(ReplyCommand::Command02, record, self.layers),
            Reply::Command03 => profile_reply(ReplyCommand::Command03, record, self.layers),
            Reply::Command1d => respond(self.layers, ReplyCommand::Command1d, CompletionStatus(0), &[]),
        }
    }

    fn apply(&mut self, apply: Apply, record: RecordHandle) {
        match apply {
            Apply::Command17 => self.apply_selection(record),
            Apply::Prepare => self.prepare(record),
            Apply::Command1d => {
                let [first, second] = [1, 2].map(|index| self.layers.parameters().word(K::Command1d, index));
                self.layers.profile().write(record, Field::Command1dFirst, first);
                self.layers.profile().write(record, Field::Command1dSecond, second);
            }
            Apply::Resume => resume(record, self.layers),
            Apply::Results => self.results(record),
        }
    }

    fn condition(&mut self, condition: Condition, record: RecordHandle) -> bool {
        match condition {
            Condition::OutputNeeded => {
                let mode = read(self.layers, record, Field::ViewMode);
                let output = read(self.layers, record, Field::OutputFirst);
                (mode != 0 && mode != 3) || output != 0
            }
            Condition::RestartRequired => self.layers.profile().changed(record, Change::Restarting(Restarting::Fourth)),
        }
    }

    fn autofocus(&mut self, record: RecordHandle) -> AutofocusChange { crate::s1_autofocus::classify(record, self.layers) }

    fn move_lens(&mut self, record: RecordHandle) -> bool { crate::s1_autofocus::move_lens(record, self.layers) }

    fn fault(&mut self) { self.layers.fault() }
}

impl<L: Layers> Leaf<'_, L> {
    /// Applies Command17: its preparation bits, the live view's selection, and clears the focus
    /// blocks of every selection bit left clear. A selection the geometry refuses is answered
    /// with -96 and applied anyway, then answered again with 0 (0xf2371c..0xf23874; suspected
    /// defect).
    fn apply_selection(&mut self, record: RecordHandle) {
        let words = SelectionWords(core::array::from_fn(|index| self.layers.parameters().word(K::Command17, index)));
        let [_, requested, offered, ..] = words.0;
        if !self.layers.values().admits_selection(requested, offered) {
            respond(self.layers, ReplyCommand::Command17, SELECTION_REFUSED, &[]);
        }
        let profile = self.layers.profile();
        if requested & 1 != 0 { profile.write(record, Field::PreparationFirst, 1); }
        if requested & 4 != 0 { profile.write(record, Field::PreparationSecond, 1); }
        if requested & 8 != 0 { profile.write(record, Field::PendingOutput, 0); }
        self.live.apply_selection(&words, record, self.layers.profile());
        let selection = read(self.layers, record, Field::Command17Selection);
        let profile = self.layers.profile();
        if selection & 1 == 0 { clear_words::<33>(profile, record, Block::Release, &[1]); }
        if selection & 2 == 0 { clear_words::<8>(profile, record, Block::FocusConfirmation, &[0]); }
        if selection & 4 == 0 { clear_words::<5>(profile, record, Block::MeasuredWindow, &[3]); }
        if selection == 0 { clear_words::<20>(profile, record, Block::FocusHistory, &[0]); }
        respond(self.layers, ReplyCommand::Command17, CompletionStatus(0), &[]);
    }

    /// Applies Prepare to the profile, leaves the preparation override pending while the
    /// preparation is requested and answers with the preparation flags.
    fn prepare(&mut self, record: RecordHandle) {
        let [enables, values, first, second] = [1, 2, 3, 4].map(|index| self.layers.parameters().word(K::Prepare, index));
        apply_preparation(enables, values, record, self.layers.profile());
        self.layers.profile().write(record, Field::PrepareFirst, first);
        self.layers.profile().write(record, Field::PrepareSecond, second);
        *self.preparation = if preparation_requested(record, self.layers.profile()) {
            PreparationOverride::Pending
        } else {
            PreparationOverride::None
        };
        let flags = read(self.layers, record, Field::SelectionFlags);
        respond(self.layers, ReplyCommand::Prepare, CompletionStatus(0), &[flags]);
    }

    /// Queries the result records into the profile and hands the statistics task's results to
    /// the profile's callback while a selection is pending.
    fn results(&mut self, record: RecordHandle) {
        let layers = &mut *self.layers;
        let callback = read(layers, record, Field::ResultsCallback);
        layers.profile().write(record, Field::ResultsTaken, 1);
        let query = RecordQuery {
            bank: if layers.values().extended_results() { 7 } else { 0 },
            mode: RESULTS_QUERY_MODE,
            descriptor: layers.values().descriptor(record),
            pair: [read(layers, record, Field::DescriptorFirst), read(layers, record, Field::DescriptorSecond)],
            ratio: read(layers, record, Field::FormatRatio),
        };
        let records = &mut self.profile.records;
        layers.values().query_records(&query, records);
        let ticks = layers.values().ticks();
        layers.profile().write(record, Field::ResultsCount, word(records, 0));
        layers.profile().write(record, Field::ResultsTime, ticks);
        let indices = (0..ResultIndex::COUNT as u8).filter_map(ResultIndex::new);
        for (index, entry) in indices.zip(records[4..].chunks_exact(104)) {
            let words: [u32; 26] = core::array::from_fn(|offset| word(entry, offset * 4));
            layers.profile().write_block(record, Block::ResultRecord(index), &words);
        }
        if let Some(callback) = stock::optional_callback(callback) {
            if read(layers, record, Field::SelectionPending) != 0 {
                layers.results().deliver(callback);
            }
        }
    }
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
}

fn respond(layers: &mut impl Layers, command: ReplyCommand, status: CompletionStatus, words: &[u32]) {
    layers.replies().reply(command, status, Values::Words(words));
}

/// Answers Command02 or Command03 with the profile's descriptor, the lens value and the lens
/// probe flags.
fn profile_reply(command: ReplyCommand, record: RecordHandle, layers: &mut impl Layers) {
    let descriptor = read(layers, record, Field::Descriptor);
    let first = u32::from(layers.lens().overridden(LensProbe::First));
    let second = u32::from(layers.lens().overridden(LensProbe::Second));
    let value = layers.lens().value();
    respond(layers, command, CompletionStatus(0), &[descriptor, value, 0, first << 1 | second]);
}

/// The resume clears the selection options' last word and moves the lens target's position to
/// the lens.
fn resume(record: RecordHandle, layers: &mut impl Layers) {
    clear_words::<7>(layers.profile(), record, Block::Options, &[6]);
    let [target]: [u32; 1] = block(layers, record, Block::LensTarget);
    let position = layers.lens().position() & 0xffff;
    layers.profile().write_block(record, Block::LensTarget, &[target & 0xffff_0000 | position]);
}
