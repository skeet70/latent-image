use crate::adjust::lens::{LensConfiguration, LensControl, LensSteps, Step};
use crate::adjust::motion::drive::{DriveKind, DriveLimits, DriveSpeed};
use crate::adjust::motion::ports::{FrameGeometry, LensBounds, LensCalibration, LensTables};
use crate::adjust::motion::preparation::{CaptureFrames, LensStatus};
use crate::adjust::timing::LevelScale;
use crate::exposure::ports::{ProfileAccess, Properties, StockMemory};
use crate::scene_recognition::ports::Overlay;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlanSpeedRequest {
    pub lens: u32,
    pub step: u32,
    pub depth: u16,
    pub period: u32,
    pub maximum_step: i16,
    pub minimum_step: i16,
    pub drive_kind: DriveKind,
    pub selector: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PositionRemap {
    pub position: i16,
    pub lens: u32,
    pub calibration: u32,
    pub step: u32,
    pub side: u32,
}

pub trait PlanServices {
    /// 0x7492ac, with direction 0 and drive step 8.
    fn plan_speed(&mut self, request: PlanSpeedRequest) -> Step;
    /// 0x74947c, with its fifth and sixth arguments zero.
    fn remap_position(&mut self, request: PositionRemap) -> i16;
    /// 0x747af8.
    fn minimum_speed(&mut self) -> u32;
    /// 0x76dad4.
    fn current_side(&mut self) -> u32;
    /// 0x76e4b0 equals 1.
    fn checks_drive_speed(&mut self) -> bool;
    /// 0x742088.
    fn load_register(&mut self, register: u8) -> u8;
    /// 0x76fc4c.
    fn snapshot_mode(&mut self) -> u32;
    /// 0x10538a4, followed by 0x10538c4 only when the former returns 1.
    fn cached_focus(&mut self) -> Option<[u32; 4]>;
    /// 0x1110a84.
    fn release_aux_text(&mut self);
}

pub trait PlanPorts:
    Properties
    + StockMemory
    + ProfileAccess
    + LensConfiguration
    + LensControl
    + LensSteps
    + LensTables
    + LensCalibration
    + LensBounds
    + DriveLimits
    + DriveSpeed
    + FrameGeometry
    + CaptureFrames
    + LensStatus
    + LevelScale
    + Overlay
    + PlanServices
{
}
impl<
    T: Properties
        + StockMemory
        + ProfileAccess
        + LensConfiguration
        + LensControl
        + LensSteps
        + LensTables
        + LensCalibration
        + LensBounds
        + DriveLimits
        + DriveSpeed
        + FrameGeometry
        + CaptureFrames
        + LensStatus
        + LevelScale
        + Overlay
        + PlanServices,
> PlanPorts for T
{
}
