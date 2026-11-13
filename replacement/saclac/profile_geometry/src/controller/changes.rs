#![forbid(unsafe_code)]

//! The controller's bookkeeping between a parameter change and the pipeline requests that
//! follow it: requested work, stage claims, the stage masks the refresh sends, and the
//! subscriptions stock starters set.

use super::{ProfileGeometry, Refusal, Status, Tables};
use crate::memory::Memory;
use crate::ports::Ports;
use crate::stock::layout::{changes, requests, work};

/// Work a parameter change leaves for the next refresh.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Work {
    /// Cleared by the refresh without other effect.
    Output,
    Crop,
    PendingPair,
    PrimaryPayload,
    SecondaryPayload,
    /// Set with `Output`; no owned step consumes it.
    OutputCompanion,
    /// Set with `PrimaryPayload` on a format subscription; no owned step consumes it.
    FormatCompanion,
    Format,
    QuadSelection,
    Mode,
    ScanConfiguration,
    Tertiary,
    PrimaryRectangles,
    SecondaryRectangles,
    PrimaryScan,
    SecondaryScan,
    /// Set by the fourth subscription; no owned step consumes it.
    FourthCompanion,
    ExtendedRectangles,
    CompactRectangles,
    RectangleSet,
    RecordRectangles,
    IndexedPair,
}

/// A pipeline stage an operation's starter claims; a claimed stage is pending while the
/// pipeline runs it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stage {
    /// Released only when every operation is idle (0xee4a18).
    Base,
    PrimaryPayload,
    SecondaryPayload,
    /// Released with the format group (0xee4ca4).
    Format,
    QuadSelection,
    FirstMode,
    SecondMode,
    PrimaryScan,
    SecondaryScan,
    Tertiary,
}

impl Stage {
    /// The stage's bit in the pipeline's apply, enable and disable masks.
    pub const fn pipeline_bit(self) -> u32 {
        match self {
            Stage::Base => 0x1,
            Stage::PrimaryPayload => 0x4,
            Stage::SecondaryPayload => 0x8,
            Stage::QuadSelection => 0x10,
            Stage::Tertiary => 0x20,
            Stage::FirstMode => 0x40,
            Stage::SecondMode => 0x80,
            Stage::PrimaryScan => 0x100,
            Stage::SecondaryScan => 0x200,
            Stage::Format => 0x400,
        }
    }
}

/// A word a stock starter sets to 1 to keep its operation's pipeline configuration current.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Subscription {
    Output,
    Format,
    PrimaryScan,
    SecondaryScan,
    Fourth,
    ScanConfiguration,
    Grouped(u8),
}

/// The four stage masks the refresh hands the pipeline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mask {
    /// Geometry records sent with the configuration (0xd9fd54).
    Geometry,
    /// Payloads applied to running stages (0xd9f828).
    Apply,
    /// Stages to enable (0xd9f7c0).
    Enable,
    /// Stages to disable (0xd9f7d8).
    Disable,
}

/// A bit of a stage's held and released pair; a payload update skips a held stage until it is
/// released.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Hold {
    PrimaryPayload,
    SecondaryPayload,
    QuadSelection,
    Mode,
    Tertiary,
}

impl<M: Memory, P: Ports, T: Tables> ProfileGeometry<M, P, T> {
    pub(crate) fn requested(&self, work: Work) -> bool {
        let (at, bit) = changes::work(work);
        self.memory.read_u8(at) & bit != 0
    }

    pub(crate) fn request(&mut self, work: Work) {
        let (at, bit) = changes::work(work);
        self.memory.set_bits_u8(at, bit);
    }

    pub(crate) fn clear_request(&mut self, work: Work) {
        let (at, bit) = changes::work(work);
        self.memory.clear_bits_u8(at, bit);
    }

    pub(crate) fn claimed(&self, stage: Stage) -> bool {
        let (at, bit) = changes::claim(stage);
        self.memory.read_u8(at) & bit != 0
    }

    pub(crate) fn scan_claimed(&self) -> bool {
        let (at, bit) = changes::SCAN_CLAIM;
        self.memory.read_u8(at) & bit != 0
    }

    pub(crate) fn pending(&self, stage: Stage) -> bool {
        let (at, bit) = changes::pending(stage);
        self.memory.read_u8(at) & bit != 0
    }

    pub(crate) fn held(&self, hold: Hold) -> bool {
        let (held, released, bit) = changes::hold(hold);
        self.memory.read_u8(held) & bit != 0 && self.memory.read_u8(released) & bit == 0
    }

    pub(crate) fn subscribed(&self, subscription: Subscription) -> bool {
        self.memory.read_u32(requests::subscription(subscription)) == 1
    }

    pub(crate) fn subscription_word(&self, subscription: Subscription) -> u32 {
        self.memory.read_u32(requests::subscription(subscription))
    }

    pub(crate) fn mask(&self, mask: Mask) -> u32 { self.memory.read_u32(work::mask(mask)) }

    pub(crate) fn add_to_mask(&mut self, mask: Mask, bits: u32) { self.memory.set_bits_u32(work::mask(mask), bits) }

    pub(crate) fn clear_mask(&mut self, mask: Mask) { self.memory.write_u32(work::mask(mask), 0) }

    /// Asks the pipeline to enable `stage` and marks it pending.
    pub(crate) fn enable(&mut self, stage: Stage) {
        self.add_to_mask(Mask::Enable, stage.pipeline_bit());
        let (at, bit) = changes::pending(stage);
        self.memory.set_bits_u8(at, bit);
    }

    /// Asks the pipeline to disable `stage` and clears its pending mark.
    pub(crate) fn finish(&mut self, stage: Stage) {
        self.add_to_mask(Mask::Disable, stage.pipeline_bit());
        let (at, bit) = changes::pending(stage);
        self.memory.clear_bits_u8(at, bit);
    }

    pub(crate) fn finish_if_pending(&mut self, stage: Stage) {
        if self.pending(stage) {
            self.finish(stage);
        }
    }

    /// Drops the starter's claim on `stage`, disabling it if it was pending.
    pub(crate) fn release(&mut self, stage: Stage) -> Status {
        if !self.claimed(stage) {
            return Err(Refusal::PipelineRefused);
        }
        let (at, bit) = changes::claim(stage);
        self.memory.clear_bits_u8(at, bit);
        self.finish_if_pending(stage);
        Ok(())
    }

    /// 0xee5230.
    pub(crate) fn release_payloads(&mut self) {
        let _ = self.release(Stage::PrimaryPayload);
        let _ = self.release(Stage::SecondaryPayload);
    }

    /// 0xee5060.
    pub(crate) fn release_modes(&mut self) -> Status {
        let first = self.release(Stage::FirstMode);
        let second = self.release(Stage::SecondMode);
        first.and(second)
    }

    /// Drops the scan configuration claim with its subscription and the configuration the
    /// pipeline holds (0xee5550).
    pub(crate) fn release_scan_claim(&mut self) {
        if !self.scan_claimed() {
            return;
        }
        let (at, bit) = changes::SCAN_CLAIM;
        self.memory.clear_bits_u8(at, bit);
        self.memory.write_u32(requests::subscription(Subscription::ScanConfiguration), 0);
        self.release_policy_configuration();
    }

    /// Clears a subscription that is set (0xee2ca0, 0xee2e94, 0xee3404, 0xee3108).
    pub(crate) fn unsubscribe(&mut self, subscription: Subscription) {
        if self.subscribed(subscription) {
            self.memory.write_u32(requests::subscription(subscription), 0);
        }
    }

    /// 0xee3404.
    pub(crate) fn unsubscribe_scans(&mut self) {
        self.unsubscribe(Subscription::PrimaryScan);
        self.unsubscribe(Subscription::SecondaryScan);
    }

    /// The fourth subscription and, when it was set, the group it leads (0xee3108).
    pub(crate) fn unsubscribe_fourth(&mut self) {
        if !self.subscribed(Subscription::Fourth) {
            return;
        }
        self.memory.write_u32(requests::subscription(Subscription::Fourth), 0);
        for index in 6..=14 {
            self.unsubscribe(Subscription::Grouped(index));
        }
    }
}
