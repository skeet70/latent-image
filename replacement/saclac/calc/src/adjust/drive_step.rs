//! The lens step a contrast search moves by, the rate it is driven at, and how far the lens
//! travels in one frame of a scan.

use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::MeasurementSelection;

use crate::adjust::configuration::{Configuration, ControllerFlag, FrameInterval};
use crate::adjust::focus_profile::LensResources;
use crate::adjust::lens::{LensSteps, Step, StepRequest};
use crate::adjust::motion::drive::{DriveKind, DriveMode};
use crate::exposure::ports::{Lens, ProfileAccess, Properties};
use crate::exposure::settings;
use crate::stock::adjust as stock;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepKind {
    /// The direction search.
    Direction,
    Scan,
    /// The return and the approach.
    Return,
    /// A scan scaled by the lens's movement (0xfaa990).
    MonitoredScan,
    /// The continuous search's start (0xf2751c), peak search (0xf287e8) and tracking (0xf27fcc).
    ContinuousStart,
    ContinuousPeak,
    ContinuousTrack,
}

/// The drive mode the contrast search's steps and drives are read for.
pub const fn search_mode(configuration: &Configuration) -> DriveMode {
    let fine = configuration.controller_flag(ControllerFlag::First);
    DriveMode { direction: fine as u32, step: if fine { 10 } else { 9 } }
}

fn selected(ports: &mut impl ProfileAccess, record: RecordHandle) -> bool { ports.profile().measurement_selection(record) == MeasurementSelection::Record }

/// In lens steps (0xfab390).
fn base_step(properties: &mut impl Properties, kind: StepKind, selected: bool) -> u16 { settings::tenths(properties, stock::base_step(kind, selected)) }

fn low_contrast(properties: &mut impl Properties, contrast: u32) -> bool { contrast < properties.property(stock::PROBE_CONTRAST).wrapping_mul(10) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StepQuery {
    pub kind: StepKind,
    /// The lens channel whose calibration index the table is read for.
    pub channel: u32,
    pub contrast: u32,
    /// The pipeline timing word whose frame interval the step rate is read for.
    pub timing: u32,
    pub drive_kind: DriveKind,
}

/// The step for a search phase and the rate the lens table gives it (0xfaab3c).
pub fn step(ports: &mut (impl Properties + ProfileAccess + Lens + LensResources + LensSteps + FrameInterval), configuration: &Configuration, record: RecordHandle, query: StepQuery) -> Step {
    let resource = ports.resource().unwrap_or(0);
    let probes = matches!(query.kind, StepKind::Scan | StepKind::ContinuousPeak);
    let base = if probes && low_contrast(ports, query.contrast) {
        settings::tenths(ports, stock::LOW_CONTRAST_STEP)
    } else {
        let selected = selected(ports, record);
        base_step(ports, query.kind, selected)
    };
    let index = ports.calibration_index(query.channel);
    let interval = ports.frame_interval(query.timing).unwrap_or(0);
    let shift = ports.property(stock::STEP_PERIOD_SHIFT) as u8;
    let period = interval.checked_shr(u32::from(shift)).unwrap_or(0) / 100;
    ports.step(StepRequest { resource, index, base, period, kind: query.drive_kind, mode: search_mode(configuration) })
}

/// The lens's movement a monitored scan scales its step by.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Monitor {
    /// Selects the scale table for a lens that moved.
    pub moved: bool,
    /// Grows the step by its size in thousandths, either sign.
    pub mark: i16,
}

/// `size` grown by the mark and scaled by the resource's monitor scale in hundredths (0xfaa928).
pub fn monitored_step(properties: &mut impl Properties, resource: u32, monitor: Monitor, size: i16) -> i16 {
    let scale = properties.property(stock::monitor_scale(monitor.moved, resource)) as i32;
    let grown = (i32::from(monitor.mark).abs() + 1000).wrapping_mul(i32::from(size)) / 1000;
    (scale.wrapping_mul(grown) / 100) as i16
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanStepQuery {
    /// Present for a scan that monitors the lens's movement.
    pub monitor: Option<Monitor>,
    /// The lens's calibration index and resource as the search took them in.
    pub index: u32,
    pub resource: u32,
    /// The frame interval in microseconds.
    pub interval: u32,
    pub contrast: u32,
    pub drive_kind: DriveKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanStep {
    pub base: u16,
    pub step: Step,
    /// Lens steps the drive covers in one frame interval at the step's rate.
    pub travel: u16,
    /// The lens table's distance for that travel.
    pub distance: u16,
}

/// The scan's step, its rate and its travel per frame (0xfaa990).
pub fn scan_step(ports: &mut (impl Properties + ProfileAccess + LensSteps), configuration: &Configuration, record: RecordHandle, query: ScanStepQuery) -> ScanStep {
    let selected = selected(ports, record);
    let base = if low_contrast(ports, query.contrast) {
        settings::tenths(ports, if selected { stock::SELECTED_LOW_CONTRAST_STEP } else { stock::LOW_CONTRAST_STEP })
    } else {
        let kind = if query.monitor.is_some() { StepKind::MonitoredScan } else { StepKind::Scan };
        base_step(ports, kind, selected)
    };
    let request = StepRequest { resource: query.resource, index: query.index, base, period: query.interval / 100, kind: query.drive_kind, mode: search_mode(configuration) };
    let mut step = ports.step(request);
    if let Some(monitor) = query.monitor {
        step.size = monitored_step(ports, query.resource, monitor, step.size as i16) as u16;
    }
    let tenths = query.interval.wrapping_mul(step.rate) / 100_000;
    let distance = ports.travel_distance(query.resource, query.index, tenths as i16);
    ScanStep { base, step, travel: (tenths / 10) as u16, distance }
}
