//! The vote: the recognition live view and the shot publish to the record. Live view holds a
//! recognition while its change detectors find the frame unchanged from the reference frame,
//! records frames unchanged from the snapshot, and settles on the majority of the recording; the
//! shot keeps the published scene group unless its change detectors find the frame changed.

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::stock::shared::group_27;

use super::detectors::{enabled, filter, frame_record, on_tripod, record_selected, switch, with_landscape, with_person, with_shake_reduction, CaptureChange, CaptureDetector, ChangeDetector, SceneGroup};
use super::frame::FramePorts;
use super::history::{leading, majority, FrameNote};
use super::ports::{Overlay, SceneEvents, SceneTrace, Steadiness};
use super::{read_grid, LensDetectors, Mode, Recognition, Scene, SceneFlags, SceneRecognition, Tally, CELLS};
use crate::adjust::configuration::{History, ProfileStore, HISTORY};
use crate::exposure::ports::{Property, StockMemory};
use crate::exposure::settings;
use crate::stock::scene_recognition as stock;

pub trait VotePorts: FramePorts + StockMemory + ProfileStore + Steadiness + Overlay + SceneEvents + SceneTrace {}

impl<T: FramePorts + StockMemory + ProfileStore + Steadiness + Overlay + SceneEvents + SceneTrace> VotePorts for T {}

/// The scenes with a person in them.
const PEOPLE: [u32; 18] = [1, 4, 8, 9, 14, 15, 19, 20, 22, 23, 26, 27, 35, 36, 38, 39, 41, 42];

fn window(ports: &mut impl crate::exposure::ports::Properties, property: Property) -> usize { (ports.property(property) as usize).clamp(1, HISTORY) }

impl SceneRecognition {
    /// Recognises the frame for `mode` and publishes the recognition to the record (0xf4f838).
    pub fn vote(&mut self, ports: &mut impl VotePorts, handle: RecordHandle, mode: Mode) -> Recognition {
        let answer = match mode {
            Mode::Capture => self.capture_vote(ports, handle),
            Mode::Live => self.live_vote(ports, handle),
        };
        self.report(ports, handle, answer);
        answer
    }

    fn majority_for(&self, ports: &mut impl VotePorts, mode: Mode) -> Recognition {
        majority(self.primary_codes.entries(), window(ports, stock::MAJORITY_WINDOW[usize::from(mode == Mode::Capture)]))
    }

    fn capture_vote(&mut self, ports: &mut impl VotePorts, handle: RecordHandle) -> Recognition {
        let (query, flags, option, candidate, group, filtered, selection);
        if ports.property(stock::CAPTURE_QUERY) == 0 {
            let settled = self.majority_for(ports, Mode::Capture);
            (query, option, candidate, group, filtered) = (Scene::AUTO, 0, Scene::AUTO, Scene::AUTO, Scene::AUTO);
            selection = settled.scene;
            flags = settled.flags;
        } else {
            let found = self.query_frame(ports, handle, Mode::Capture);
            query = found.scene;
            flags = found.flags;
            option = switch(ports, handle, stock::PROMOTION_SWITCH[1]);
            let features = self.evaluation.features;
            candidate = if option != 0 { with_landscape(ports, handle, &features, query) } else { query };
            let published_group = SceneGroup::of(self.published.scene);
            let considered = switch(ports, handle, stock::PROMOTION) != 0 && enabled(ports, candidate);
            match published_group.filter(|_| considered) {
                Some(published_group) if !self.capture_changed(ports, handle) => {
                    group = published_group.base();
                    self.group_hint = group;
                    filtered = filter(ports, &features, group, candidate).0;
                    selection = with_person(candidate, filtered);
                }
                _ => {
                    (group, filtered) = (Scene::AUTO, Scene::AUTO);
                    selection = candidate;
                }
            }
        }
        let (selection, flags) = self.finish_selection(ports, handle, selection, flags, |trace, promoted| trace.capture_trace.promoted = promoted);
        self.evaluation.features.selection = Recognition { scene: selection, flags };
        let trace = &mut self.capture_trace;
        trace.query = query;
        trace.option = option;
        trace.candidate = candidate;
        trace.ready = self.ready;
        trace.group = group;
        trace.hint = self.group_hint;
        trace.filtered = filtered;
        trace.selection = Recognition { scene: selection, flags };

        let mut settled = Recognition { scene: selection, flags };
        if ports.property(stock::CAPTURE_RECORD) != 0 {
            self.record_code(settled);
            settled = self.majority_for(ports, Mode::Capture);
        }
        self.settled = settled;
        let choices = &mut self.evaluation.choices;
        choices.recording = false;
        choices.holding = settled != Recognition::AUTO;
        self.evaluation.against_reference.reference.note.subject = PEOPLE.contains(&settled.scene.0);
        self.capture = settled;
        let published = self.overridden(ports, handle, Mode::Capture, settled);
        self.published = published;
        self.saved_features = self.evaluation.features;
        self.saved_capture = self.capture;
        published
    }

    /// Whether the shot's change detectors find the frame changed from the reference frame,
    /// keeping what the lens detectors found in both traces.
    fn capture_changed(&mut self, ports: &mut impl VotePorts, handle: RecordHandle) -> bool {
        let mut found = LensDetectors::ZERO;
        let mut weight = 0u8;
        for key in stock::CAPTURE_DETECTORS {
            let Some(detector) = CaptureDetector::from_slot(ports.property(key)) else { continue };
            let change = self.capture_change(ports, handle, detector);
            if change == CaptureChange::None {
                continue;
            }
            weight = weight.wrapping_add(ports.property(stock::detector_weight(key)) as u8);
            match (detector, change) {
                (CaptureDetector::GridMatch, CaptureChange::Toward(toward)) => (found.matched, found.match_direction) = (true, u32::from(toward)),
                (CaptureDetector::LensMoved, CaptureChange::Toward(toward)) => (found.lens_moved, found.lens_direction) = (true, u32::from(toward)),
                (CaptureDetector::Reference, _) => found.lens_changed = true,
                _ => {}
            }
        }
        self.live_trace.detectors = found;
        self.capture_trace.detectors = found;
        if u32::from(weight) < ports.property(stock::CAPTURE_THRESHOLD) {
            return false;
        }
        if found.lens_changed {
            return true;
        }
        if found.matched && (found.match_direction == 0 || ports.property(stock::CAPTURE_MATCH_TOWARD) == 0) {
            return true;
        }
        found.lens_moved && (found.lens_direction == 0 || ports.property(stock::CAPTURE_LENS_TOWARD) == 0)
    }

    /// The tripod promotion, the moving flag a dominant class-1 motion adds, and shake reduction;
    /// `promoted` keeps the selection before shake reduction.
    fn finish_selection(&mut self, ports: &mut impl VotePorts, handle: RecordHandle, selection: Scene, mut flags: SceneFlags, promoted: impl FnOnce(&mut Self, Recognition)) -> (Scene, SceneFlags) {
        let features = self.evaluation.features;
        let selection = if switch(ports, handle, stock::KEEP_STATIONARY) != 0 { on_tripod(ports, &features, selection) } else { selection };
        if switch(ports, handle, stock::SHAKE_CHECK) != 0 && features.motion.dominant == 1 {
            flags = flags.with(SceneFlags::MOVING);
        }
        promoted(self, Recognition { scene: selection, flags });
        (selection, with_shake_reduction(ports, handle, &features, selection, flags))
    }

    /// The configured scene and flag overrides, then the kind a record with the record
    /// measurement selection is given.
    fn overridden(&self, ports: &mut impl VotePorts, handle: RecordHandle, mode: Mode, mut recognition: Recognition) -> Recognition {
        let index = usize::from(mode == Mode::Capture);
        let scene = ports.property(stock::SCENE_OVERRIDE[index]);
        if scene != 0xff {
            recognition.scene = Scene(scene);
            let shift = stock::FLAG_OVERRIDE_SHIFT[index];
            let overrides = ports.property(stock::FLAG_OVERRIDES);
            if overrides >> shift & 3 != 0 {
                recognition.flags = SceneFlags((overrides >> shift & 3) as u16);
            }
        }
        let kind = ports.property(stock::KIND_OVERRIDE);
        if kind != 0 && record_selected(ports, handle) {
            recognition.scene = stock::translated_kind(kind);
        }
        recognition
    }

    fn live_vote(&mut self, ports: &mut impl VotePorts, handle: RecordHandle) -> Recognition {
        if ports.property(stock::LIVE_VOTE) == 0 {
            let found = self.query_frame(ports, handle, Mode::Live);
            self.record_code(found);
            let settled = self.majority_for(ports, Mode::Live);
            self.settled = settled;
            self.evaluation.choices.history = settled;
            return self.publish_live(ports, handle, settled);
        }
        if self.evaluation.choices.holding {
            self.hold(ports, handle);
        } else {
            self.compare_with_snapshot(ports, handle);
            if ports.property(stock::UNCHANGED_ONLY) == 0 || self.evaluation.choices.unchanged {
                self.record(ports, handle);
            }
        }
        if ports.property(stock::SNAPSHOT_VOTE) != 0 {
            self.note_snapshot(ports, handle);
        }
        let settled = self.settled;
        self.publish_live(ports, handle, settled)
    }

    /// Releases the held recognition after the configured frames, or when enough of the
    /// latest votes of the reference table found the frame changed.
    fn hold(&mut self, ports: &mut impl VotePorts, handle: RecordHandle) {
        if ports.property(stock::HOLD_BY_COUNT) != 0 {
            let choices = &mut self.evaluation.choices;
            choices.held = choices.held.wrapping_add(1);
            if u32::from(self.evaluation.choices.held) >= ports.property(stock::HOLD_FRAMES) {
                self.evaluation.choices.held = 0;
                self.release();
            }
            return;
        }
        self.evaluation.choices.released = false;
        let changed = self.tally(ports, stock::REFERENCE_DETECTORS, stock::REFERENCE_VOTE_THRESHOLD, |state, ports, detector| state.changed_from_reference(ports, handle, detector));
        self.push_comparison();
        self.changes.record(changed);
        if !changed {
            return;
        }
        self.evaluation.choices.released = true;
        let window = window(ports, stock::RELEASE_WINDOW);
        let entries = &self.changes.entries()[..window];
        let changes = if entries[window - 1].is_some() { entries.iter().filter(|entry| **entry == Some(true)).count() } else { 0 };
        if changes as u32 >= ports.property(stock::RELEASE_CHANGES) {
            self.changes = History::empty();
            self.evaluation.choices.tally = Tally::ZERO;
            self.release();
        }
    }

    fn release(&mut self) {
        let choices = &mut self.evaluation.choices;
        choices.holding = false;
        choices.unchanged = false;
        self.adjustments_changed = true;
        self.group_hint = Scene::NO_HINT;
    }

    /// Asks the detectors `keys` name until their summed weight reaches `threshold`, keeping each
    /// answer in the tally; whether it was reached.
    fn tally<P: VotePorts>(&mut self, ports: &mut P, keys: [Property; 6], threshold: Property, mut changed: impl FnMut(&mut Self, &mut P, ChangeDetector) -> bool) -> bool {
        self.evaluation.choices.tally = Tally::ZERO;
        let mut weight = 0u8;
        for (index, key) in keys.into_iter().enumerate() {
            let Some(detector) = ChangeDetector::from_slot(ports.property(key)) else { continue };
            let answer = changed(self, ports, detector);
            let detector_weight = ports.property(stock::detector_weight(key)) as u8;
            let tally = &mut self.evaluation.choices.tally;
            tally.detectors[index] = detector.slot();
            tally.weights[index] = detector_weight;
            tally.answers[index] = u32::from(answer);
            if answer {
                weight = weight.wrapping_add(detector_weight);
                tally.weight = weight;
                if u32::from(weight) >= ports.property(threshold) {
                    tally.decision = 1;
                    return true;
                }
            }
        }
        false
    }

    fn compare_with_snapshot(&mut self, ports: &mut impl VotePorts, handle: RecordHandle) {
        if ports.property(stock::SNAPSHOT_VOTE) == 0 {
            return;
        }
        let changed = self.tally(ports, stock::SNAPSHOT_DETECTORS, stock::SNAPSHOT_VOTE_THRESHOLD, |state, ports, detector| state.changed_from_snapshot(ports, handle, detector));
        if !changed {
            self.evaluation.choices.unchanged = true;
        }
    }

    /// Recognises the frame, notes it in the recording, and settles on the recording's majority
    /// once it holds the configured count of frames.
    fn record(&mut self, ports: &mut impl VotePorts, handle: RecordHandle) {
        if !self.evaluation.choices.recording {
            self.evaluation.choices.recorded = 0;
            self.empty_references();
            self.primary_codes = History::empty();
        }
        if switch(ports, handle, stock::KEEP_STATIONARY) != 0 {
            let counted = !self.evaluation.features.index_recorded;
            self.evaluation.features.stationary = self.decide_stationary(ports, counted);
            if counted {
                super::history::push_front(&mut self.stationary_history, self.evaluation.against_reference.stationary);
            }
            self.evaluation.features.index_recorded = false;
        }
        let found = self.query_frame(ports, handle, Mode::Live);
        let option = switch(ports, handle, stock::PROMOTION_SWITCH[0]);
        let features = self.evaluation.features;
        let candidate = if option != 0 { with_landscape(ports, handle, &features, found.scene) } else { found.scene };
        let style: u8 = ports.profile().read(handle, &group_00::AT_003C);
        let mut filtering = switch(ports, handle, stock::PROMOTION) != 0 && !matches!(style, 4 | 5) && enabled(ports, candidate);
        if filtering {
            self.ready = true;
            filtering = ports.property(stock::FILTER_UNCHANGED_ONLY) == 0 || self.evaluation.choices.unchanged;
        }
        self.live_trace.earlier_candidate = self.candidate;
        self.live_trace.earlier_filtered = self.filtered;
        self.candidate = candidate;
        let mut flags = found.flags;
        let chosen = if filtering {
            let (mut filtered, outcome) = filter(ports, &features, self.group_hint, candidate);
            self.live_trace.filter = outcome;
            if ports.property(stock::GROUP_SWITCHES) != 0 && !self.evaluation.choices.unchanged {
                if let Some(group) = SceneGroup::of(filtered) {
                    if ports.property(stock::GROUP_SWITCH[group as usize]) == 0 {
                        filtered = candidate;
                    }
                }
            }
            self.filtered = filtered;
            let selection = with_person(candidate, filtered);
            self.evaluation.features.selection = Recognition { scene: selection, flags };
            self.live_trace.filtered = filtered;
            self.live_trace.selection = selection;
            selection
        } else {
            self.filtered = candidate;
            candidate
        };
        self.live_trace.query = found.scene;
        self.live_trace.option = option;
        self.live_trace.candidate = candidate;
        self.live_trace.ready = self.ready;
        let selection;
        (selection, flags) = self.finish_selection(ports, handle, chosen, flags, |state, promoted| state.live_trace.promoted = promoted);
        if !filtering {
            self.evaluation.features.selection = Recognition { scene: selection, flags };
            self.live_trace.filtered = candidate;
            self.live_trace.selection = selection;
        }
        self.live_trace.flags = flags;

        let skip = !record_selected(ports, handle) && ((self.ready && self.group_hint == Scene::NO_HINT) || self.adjustments_changed);
        if skip {
            if self.evaluation.choices.recorded != 0 {
                self.evaluation.choices.recorded = 0;
                self.empty_references();
            }
        } else {
            self.note_reference(ports, Recognition { scene: selection, flags });
            self.evaluation.choices.recorded = self.evaluation.choices.recorded.wrapping_add(1);
        }
        if u32::from(self.evaluation.choices.recorded) < ports.property(stock::MAJORITY_WINDOW[0]) {
            self.evaluation.choices.recording = true;
            return;
        }
        let settled = match leading(&self.references.map(|record| record.code).entries()[..], window(ports, stock::MAJORITY_WINDOW[0])) {
            Some((index, code)) => {
                if let Some(reference) = self.references.entries()[index] {
                    self.evaluation.against_reference.reference = reference;
                }
                code.recognition()
            }
            None => Recognition::AUTO,
        };
        self.evaluation.choices.majority = settled;
        self.evaluation.choices.recording = false;
        self.settled = settled;
        self.evaluation.choices.holding = settled != Recognition::AUTO;
        if self.evaluation.choices.holding {
            ports.snapshot_adjustments();
        }
    }

    /// Empties the references and the hints recorded with them (0xf4dbac).
    fn empty_references(&mut self) {
        self.references = History::empty();
        self.live_trace.hints = History::empty();
    }

    fn note_snapshot(&mut self, ports: &mut impl VotePorts, handle: RecordHandle) {
        let reference = frame_record(ports, handle).live.reference;
        let comparison = &self.evaluation.against_reference;
        let grid = comparison.source.map_or([0; CELLS], |source| read_grid(ports, source));
        let comparison = &self.evaluation.against_reference;
        self.evaluation.snapshot = FrameNote {
            grid,
            position: comparison.position,
            subject: comparison.subject_present,
            reference,
            motion: comparison.motion,
            stationary: comparison.stationary.stationary,
        };
    }

    fn publish_live(&mut self, ports: &mut impl VotePorts, handle: RecordHandle, settled: Recognition) -> Recognition {
        let published = self.overridden(ports, handle, Mode::Live, settled);
        self.saved = self.evaluation;
        if self.published != published {
            ports.scene_changed(handle, published);
            self.published = published;
        }
        published
    }

    /// Writes the recognition to the record, then the log entry, the trace lines and the
    /// on-screen text each switch asks for.
    fn report(&mut self, ports: &mut impl VotePorts, handle: RecordHandle, answer: Recognition) {
        let records = ports.records();
        let _refused = records.write_shared(handle, &group_27::AT_073C, answer.scene.0);
        let _refused = records.write_shared(handle, &group_27::AT_0740, answer.flags.0);
        let (log, log_bit) = stock::LOG_SWITCH;
        if ports.property(log) & log_bit != 0 && record_selected(ports, handle) {
            self.log(ports, handle);
        }
        let (trace, trace_bit) = stock::TRACE_SWITCH;
        if settings::word(ports, trace) & trace_bit != 0 {
            self.trace(ports, handle);
        }
        if ports.property(stock::OVERLAY) != 0 {
            draw(ports, answer);
        }
    }

    /// One entry of the work-memory log; the index wraps after the last entry.
    fn log(&mut self, ports: &mut impl VotePorts, handle: RecordHandle) {
        let live = frame_record(ports, handle).live;
        let converted = i32::from(ports.converted_difference(2, u32::from(stock::LIVE_LEVEL_STEP)));
        let offset = i32::from(ports.reference_offset(live.reference));
        let saved = &self.saved.features;
        let upper = converted + i32::from(live.upper_level);
        let words = [
            self.published.scene.0,
            u32::from(self.published.flags.0),
            u32::from(self.saved.choices.holding),
            live.reference,
            saved.state,
            saved.at_3c,
            i32::from(saved.position) as u32,
            i32::from(saved.response) as u32,
            i32::from(saved.distance) as u32,
            upper as u32,
            (converted + i32::from(live.lower_level)) as u32,
            (upper + offset) as u32,
            (converted + i32::from(live.third_level)) as u32,
            i32::from(live.at_38) as u32,
            u32::from(saved.subjects != 0),
        ];
        let entry = stock::log_entry(self.log_index);
        for (index, word) in words.into_iter().enumerate() {
            ports.write_u32(SharedAddress(entry.0 + 4 * index as u32), word);
        }
        self.log_index = if self.log_index >= stock::LOG_LAST { 0 } else { self.log_index + 1 };
    }

    /// The published scene's name and flags, then the detectors' inputs; a negative response is
    /// kept as its magnitude.
    fn trace(&mut self, ports: &mut impl VotePorts, handle: RecordHandle) {
        let published = self.published;
        ports.scene_name(published.scene);
        ports.scene_flags(published.flags);
        let thresholds = [stock::LANDSCAPE_LEVEL, stock::NIGHT_LEVEL, stock::CLOSEUP_LEVEL, stock::BELOW_LIMIT, stock::DIFFERENCE_LIMIT];
        let selected = usize::from(record_selected(ports, handle));
        let [landscape, night, closeup, below, difference] = thresholds.map(|pair| i32::from(settings::signed_tenths(ports, pair[selected])));
        let features = &mut self.evaluation.features;
        features.response = features.response.wrapping_abs();
        let (level, bounds) = if published.scene == Scene::HANDHELD_NIGHT { (features.alternate_level, features.bounds[1]) } else { (features.primary_level, features.bounds[0]) };
        let values = [
            features.subjects as i32,
            features.reference as i32,
            features.state as i32,
            i32::from(features.level),
            landscape,
            night,
            closeup,
            below,
            i32::from(features.upper_level),
            i32::from(features.lower_level),
            difference,
            i32::from(features.position),
            i32::from(features.distance),
            i32::from(features.response),
            i32::from(level),
            i32::from(bounds.low.wrapping_abs()),
            i32::from(bounds.high),
            features.motion.dominant as i32,
            i32::from(features.stationary),
            published.scene.0 as i32,
            i32::from(published.flags.0),
        ];
        ports.measurements(&values);
    }
}

/// The on-screen text: the scene's line, then its flags' line (0xf4e058).
fn draw(ports: &mut impl Overlay, recognition: Recognition) {
    ports.text(stock::scene_label(recognition.scene), 2, 8, 2, 9);
    let mut line = [0u8; 16];
    let mut length = 0;
    let mut append = |text: &[u8]| {
        line[length..length + text.len()].copy_from_slice(text);
        length += text.len();
    };
    let flags = recognition.flags;
    if flags.0 & SceneFlags::MOVING.0 != 0 {
        append(stock::MOVING_TEXT);
    }
    if flags.0 & SceneFlags::SHAKE_REDUCTION.0 != 0 {
        append(stock::SHAKE_REDUCTION_TEXT);
    }
    if flags.0 == 0 {
        append(stock::NO_FLAGS_TEXT);
    }
    ports.text(&line[..length], 2, 9, 2, 9);
}
