#![forbid(unsafe_code)]

//! What the lens children read and request outside their own state.

use xm1_saclac_interface::lens::{LensMode, LensPayload, PositionStep, ProfileSlotReference};
use xm1_saclac_interface::{lens_effect, LensEffect, ParameterValue, PeerOperation, RecordHandle};

/// A lensdc request with the lens effect the interface classifies its operation as.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensRequest {
    payload: LensPayload,
    effect: LensEffect,
}

impl LensRequest {
    pub const fn new(payload: LensPayload) -> Self {
        Self { payload, effect: lens_effect(PeerOperation::LensDc(payload.operation())) }
    }

    pub const fn payload(&self) -> LensPayload { self.payload }

    pub const fn effect(&self) -> LensEffect { self.effect }
}

/// Lens control did not post the request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Refused;

/// One of lens control's three status probes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Probe { Clear, Set, Forced }

/// The three probes, named by the reply-flag bit each forced probe sets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Probes { pub bit0: Probe, pub bit1: Probe, pub bit2: Probe }

/// Lens control's resource index, which the bring-up applies to the configuration lookup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceIndex(pub u32);

pub trait LensControl {
    fn send(&mut self, request: LensRequest) -> Result<(), Refused>;
    fn probes(&mut self) -> Probes;
    fn resource_index(&mut self) -> ResourceIndex;
}

/// A reply's status as the client sees it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Done,
    /// A drive's intermediate report.
    Progress,
    /// The request was taken and its end is reported later.
    Accepted,
    /// Nothing was in progress for the request to act on.
    Unneeded,
    /// Lens control did not post the request.
    Refused,
    /// The status lens control's completion carried.
    Lens(i32),
}

/// The LensDrive command's words 1 to 4.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriveWords {
    pub value: ParameterValue,
    pub second: ParameterValue,
    /// The word the root's cancellation replaces.
    pub third: ParameterValue,
    pub fourth: DriveFourth,
}

/// LensDrive's fourth word: one, or the word the command carried.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriveFourth { One, Carried(ParameterValue) }

impl DriveWords {
    pub const fn with_fourth(self, fourth: DriveFourth) -> Self {
        Self { fourth, ..self }
    }

    pub const fn parameters(self) -> [ParameterValue; 4] {
        let fourth = match self.fourth {
            DriveFourth::One => ParameterValue(1),
            DriveFourth::Carried(value) => value,
        };
        [self.value, self.second, self.third, fourth]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriveDetail { Lens(Probes), Reported(ParameterValue) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriveReply {
    pub status: Status,
    pub value: ParameterValue,
    pub fourth: DriveFourth,
    pub detail: DriveDetail,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reply {
    /// To the LensDrive command's client.
    Drive(DriveReply),
    /// To the Cancel command's client.
    Cancel(Status),
    /// To Command10's client.
    Stop(Status),
    /// To notification 0x1b's client.
    Step { status: Status, probes: Probes },
    /// To Command2c's client.
    Select { status: Status, probes: Probes, value: ParameterValue },
    /// To Command2d's client.
    Release(Status),
    /// To the LensBringUp command's client.
    BringUp { status: Status, values: [ParameterValue; 3] },
    /// To the LensStop command's client.
    LensStop { status: Status, value: ParameterValue },
}

pub trait Replies {
    fn reply(&mut self, reply: Reply);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModeSource { Primary, Secondary, Autofocus }

/// The command parameter blocks intake fills; word 0 of each holds the client callback.
pub trait CommandBlocks {
    /// LensDrive's words 1..4.
    fn drive(&mut self) -> DriveWords;
    /// Replaces LensDrive's word 3.
    fn set_drive_third(&mut self, value: ParameterValue);
    /// Cancel's word 1.
    fn cancel(&mut self) -> ParameterValue;
    /// Command02's word 5, Command03's word 4, Autofocus's word 38.
    fn mode(&mut self, source: ModeSource) -> LensMode;
    /// The halfword at byte 4 of notification 0x1b's block.
    fn step(&mut self) -> PositionStep;
    /// Command2c's word 1.
    fn selection(&mut self) -> ParameterValue;
    /// LensBringUp's words 1..5.
    fn bring_up(&mut self) -> [ParameterValue; 5];
    /// LensStop's words 1 and 2.
    fn lens_stop(&mut self) -> [ParameterValue; 2];
}

/// The record's resource byte the drive policy compares with 0 and 1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceSelection { Zero, One, Other }

pub trait Records {
    fn profile_slot(&mut self, record: RecordHandle) -> ProfileSlotReference;
    fn resource_selection(&mut self, record: RecordHandle) -> ResourceSelection;
    /// Bit 1 of the shared profile word at +0x13c.
    fn shared_drive_bit(&mut self) -> bool;
    /// Zeroes word 8 of the shared twelve-word profile block and sets profile option 0x26 to 1,
    /// each where the record layer admits the record.
    fn prepare_selection(&mut self, record: RecordHandle);
}

/// Configuration byte 0x88ef's drive-correction flags, read by `stock::drive_policy`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DrivePolicy {
    /// Bit 3: every drive is corrected.
    pub always: bool,
    /// Bit 1: a drive is corrected when the record's resource byte is 1.
    pub resource_one: bool,
    /// Bit 2: a drive is corrected when the record's resource byte is 0.
    pub resource_zero: bool,
}

pub trait Configuration {
    fn drive_policy(&mut self) -> DrivePolicy;
    /// The adjustment configuration's first parameter is nonzero.
    fn adjustment_configured(&mut self) -> bool;
    fn select_lookup(&mut self, index: ResourceIndex);
}

pub trait DriveResults {
    /// The result the live router recorded from the last Lens34.
    fn live_result(&mut self) -> ParameterValue;
    /// The word the S1 drive's retained callback stored with its last post.
    fn s1_report(&mut self) -> ParameterValue;
}

pub trait Faults {
    fn fault(&mut self);
}
