//! The frame query: the features the scene detectors read from the frame record and the lens,
//! and the scene the detectors name for them.

use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::stock::shared::group_27;

use super::detectors::{frame_record, switch, SceneDetector};
use super::history::majority;
use super::ports::{LensServices, SubjectQuery};
use super::{Features, LevelBounds, Mode, Recognition, SceneRecognition};
use crate::adjust::configuration::MotionRecord;
use crate::adjust::lens::{BiasTable, LensConfiguration};
use crate::adjust::motion::ports::LensTables;
use crate::adjust::profile_adjustment::{LensSteps, StepTable};
use crate::exposure::ports::{Lens, ProfileAccess, Properties, Property};
use crate::exposure::settings;
use crate::stock::scene_recognition as stock;

pub trait FramePorts: Properties + ProfileAccess + SubjectQuery + Lens + LensSteps + LensConfiguration + LensTables + LensServices {}

impl<T: Properties + ProfileAccess + SubjectQuery + Lens + LensSteps + LensConfiguration + LensTables + LensServices> FramePorts for T {}

/// A response curve point's value and response, configured in hundreds.
fn curve_point(ports: &mut impl Properties, [value, response]: [Property; 2]) -> (u32, i32) {
    (u32::from((ports.property(value) as u16).wrapping_mul(100)), i32::from((ports.property(response) as u16).wrapping_mul(100)))
}

/// The lens response for `lens` at `step`: the response at the lens's primary value, read on the
/// line through the two configured points and held at their ends (0xf4b190).
pub(crate) fn response(ports: &mut (impl Properties + LensServices), lens: u32, step: u32) -> i32 {
    let value = ports.primary_tenths(lens);
    let (low_value, low_response) = curve_point(ports, stock::RESPONSE_LOW);
    let point = if value <= low_value {
        low_response
    } else {
        let (high_value, high_response) = curve_point(ports, stock::RESPONSE_HIGH);
        if value >= high_value {
            high_response
        } else {
            let along = (value - low_value) as i32;
            low_response + along * (high_response - low_response) / (high_value - low_value) as i32
        }
    };
    let reference = ports.lens_reference();
    ports.response(point as u16, lens, step, reference)
}

/// The lens position `lens` targets at `step` (0xf4e5dc).
pub(crate) fn target(ports: &mut (impl LensTables + LensServices), lens: u32, step: u32) -> i16 {
    let reference = ports.lens_reference();
    ports.channel_position(lens, step, reference, 0)
}

/// The closeup distance for `lens` at `step`: the lens target plus the larger of the configured
/// minimum and the reach the lens's spans and response give (0xf4e7a4).
fn distance(ports: &mut (impl Properties + LensTables + LensServices), lens: u32, step: u32) -> i16 {
    let band = (0..3).find(|&band| ports.property(stock::distance_band_limit(band)) as i32 >= (lens as i32).wrapping_add(1)).unwrap_or(3);
    let factor = i32::from((ports.property(stock::distance_factor(band)) as u16).wrapping_mul(10));
    let target = target(ports, lens, step);
    let response = response(ports, lens, step);
    let [first_span, second_span] = stock::distance_spans(lens);
    let span = i32::from(settings::word(ports, first_span)) + i32::from(settings::word(ports, second_span));
    let reach = factor.wrapping_mul(span).wrapping_add(response.wrapping_sub(i32::from(target)).wrapping_mul(1000)) / 1000;
    let scaled = i32::from(settings::word(ports, stock::distance_minimum(lens))).wrapping_mul(ports.property(stock::DISTANCE_SCALE) as i32);
    let minimum = scaled / 10 + i32::from(scaled % 10 > 4);
    (minimum.max(reach) as u16).wrapping_add(target as u16) as i16
}

/// Which record kind's level bounds to read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BoundsKind {
    Kind0,
    Kind11,
}

/// The level bounds for `lens`: from the lens's level against its class's reference, a slope and
/// offset per bound held between a floor and a cap, moved to `base` plus `offset` less a margin
/// (0xf4bf88).
pub(crate) fn bounds(ports: &mut (impl Properties + LensServices), lens: u32, base: i16, offset: i16, kind: BoundsKind) -> LevelBounds {
    let class = ports.level_class(lens);
    let setting = ports.property(stock::BOUNDS_SETTING);
    let reference = settings::signed_tenths(ports, stock::class_reference(class));
    let difference = ports.class_level(lens, setting).wrapping_sub(i32::from(reference));
    let origin = base.wrapping_add(offset);
    let [upper, lower] = stock::BOUNDS[kind as usize];
    let mut bound = |settings_of: stock::BoundSettings| {
        let sloped = ((ports.property(settings_of.slope) as i32).wrapping_mul(difference) / 100) as i16;
        let level = sloped.wrapping_add(settings::signed_tenths(ports, settings_of.offset));
        let cap = settings::signed_tenths(ports, settings_of.cap);
        let held = if cap > level { level.max(settings::signed_tenths(ports, settings_of.floor)) } else { cap };
        origin.wrapping_sub(settings::word(ports, settings_of.margin) as i16).wrapping_add(held)
    };
    LevelBounds { high: bound(upper), low: bound(lower) }
}

impl SceneRecognition {
    /// Reads the frame's features and asks the scene detectors: the one naming the majority's
    /// scene first, then those the configuration lists, until one names a scene or the
    /// majority carries flags (0xf4f1e8).
    pub fn query_frame(&mut self, ports: &mut impl FramePorts, handle: RecordHandle, mode: Mode) -> Recognition {
        let keep_stationary = switch(ports, handle, stock::KEEP_STATIONARY) != 0;
        let stationary = self.evaluation.features.stationary;
        self.evaluation.features = Features::ZERO;
        if keep_stationary {
            self.evaluation.features.stationary = stationary;
        }
        let subjects = self.query_subjects(ports, handle);
        let at_005d = ports.profile().read(handle, &group_00::AT_005D);
        let frame = frame_record(ports, handle);
        let mut features = Features { at_005d: u32::from(at_005d), subjects, ..self.evaluation.features };
        match mode {
            Mode::Capture => {
                let step = settings::level_step(ports, 2);
                let capture = frame.capture;
                features.primary_level = capture.primary_level.wrapping_add(step);
                features.alternate_level = capture.alternate_level.wrapping_add(step);
                features.level = capture.level.wrapping_add(step);
                features.upper_level = features.level;
                features.difference_answer = capture.difference;
                features.status = capture.status;
                features.reference = capture.reference;
                features.state = capture.state;
                features.lens_step = self.lens_bias(ports, capture.reference);
                features.position = capture.position;
                features.selected = capture.selected;
            }
            Mode::Live => {
                let live = frame.live;
                features.reference = live.reference;
                features.state = live.state;
                self.level_offset = settings::level_step(ports, stock::LIVE_LEVEL_STEP);
                self.lens_offset = ports.profile().read_shared(handle, &group_27::AT_0712);
                let converted = ports.converted_difference(2, u32::from(stock::LIVE_LEVEL_STEP));
                let lens_offset = ports.reference_offset(live.reference);
                let base = converted.wrapping_add(lens_offset);
                features.primary_level = base.wrapping_add(live.primary_level);
                features.alternate_level = base.wrapping_add(live.alternate_level);
                features.third_level = converted.wrapping_add(live.third_level);
                features.upper_level = converted.wrapping_add(live.upper_level);
                features.lower_level = converted.wrapping_add(live.lower_level);
                features.level = features.upper_level.wrapping_add(lens_offset);
                features.at_38 = live.at_38;
                features.at_3c = live.at_3c;
                features.lens_step = self.lens_bias(ports, live.reference);
                features.position = live.position;
                features.at_4c = live.at_4c;
            }
        }
        features.response = response(ports, features.reference, features.state) as i16;
        features.distance = distance(ports, features.reference, features.state);
        let step = ports.near_step(features.reference, 0xffff, StepTable::Standard);
        let base = ports.base_level(step);
        let offset = ports.reference_offset(features.reference);
        features.bounds = [bounds(ports, features.reference, base, offset, BoundsKind::Kind0), bounds(ports, features.reference, base, offset, BoundsKind::Kind11)];
        self.evaluation.features = features;

        let alternate = ports.profile().read(handle, &group_00::AT_0065) != 0;
        let mut answer = Recognition::AUTO;
        let mut asked = 0;
        let mut first = None;
        if ports.property(stock::QUERY_SKIP[usize::from(alternate)]) == 0 {
            let window = (ports.property(stock::MAJORITY_WINDOW[usize::from(mode == Mode::Capture)]) as usize).clamp(1, crate::adjust::configuration::HISTORY);
            answer = majority(self.primary_codes.entries(), window);
            if let Some(detector) = SceneDetector::naming(answer.scene) {
                first = Some(detector);
                answer.scene = self.ask(ports, handle, mode, detector, 0);
                if answer != Recognition::AUTO {
                    return self.answer(answer);
                }
                asked = 1;
            }
        }
        self.evaluation.features.motion = MotionRecord::decode(ports.profile().read_shared(handle, &group_27::AT_07EC));
        for index in 0..stock::FALLBACK_DETECTORS {
            let Some(detector) = SceneDetector::from_index(ports.property(stock::fallback_detector(alternate, index))) else { continue };
            if Some(detector) == first {
                continue;
            }
            answer.scene = self.ask(ports, handle, mode, detector, asked);
            if answer != Recognition::AUTO {
                return self.answer(answer);
            }
            asked += 1;
        }
        self.answer(answer)
    }

    fn lens_bias(&self, ports: &mut impl LensConfiguration, lens: u32) -> i16 { ports.bias(lens as i32, BiasTable::Area) }

    fn ask(&mut self, ports: &mut impl FramePorts, handle: RecordHandle, mode: Mode, detector: SceneDetector, asked: usize) -> super::Scene {
        self.evaluation.features.tried[asked] = detector.index();
        let features = self.evaluation.features;
        detector.ask(ports, handle, &features, mode)
    }

    fn answer(&mut self, answer: Recognition) -> Recognition {
        self.evaluation.features.selection = answer;
        answer
    }
}
