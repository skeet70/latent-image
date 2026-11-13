//! What the recognition reaches outside the library.

use xm1_saclac_interface::RecordHandle;

use super::{Recognition, Scene, SceneFlags};
use crate::exposure::ports::{ProfileAccess, Properties, StockMemory};

/// The record words the subject query reads (0xf4f18c).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SubjectRequest {
    /// The record's chosen word (0x752714 through 0xf4f16c).
    pub chosen: u32,
    pub at_0164: u32,
    pub at_0168: u32,
    /// Record word 0: the row scale's numerator and denominator.
    pub row_scale: [u16; 2],
}

/// The capture layer's subject query on a 640 by 480 frame (0xeedc84).
pub trait SubjectQuery {
    /// The number of subjects found, `None` when the query refuses.
    fn subjects(&mut self, request: &SubjectRequest) -> Option<u32>;
}

/// The lens's steadiness report (0xee9728).
pub trait Steadiness {
    fn steady(&mut self) -> bool;
}

/// The debug overlay's text lines (0x1110918).
pub trait Overlay {
    /// `text` up to 24 bytes, at `column` 1..=32 and `row` 1..=10.
    fn text(&mut self, text: &[u8], column: u8, row: u8, foreground: u8, background: u8);
}

/// Image 4's lens configuration services the frame query reads.
pub trait LensServices {
    /// The word image 4's lens driver keeps at 0x942be8 (0x76f340).
    fn lens_reference(&mut self) -> u32;
    /// The lens's primary value in tenths, 0 outside the configured lenses (0x747bf8).
    fn primary_tenths(&mut self, lens: u32) -> u32;
    /// The response at `value` for `lens` at `step` (0x7499a0 with a last argument of 1).
    fn response(&mut self, value: u16, lens: u32, step: u32, reference: u32) -> i32;
    /// 0xef72d4.
    fn converted_difference(&mut self, first: u32, second: u32) -> i16;
    /// The lens's level class (0x747234).
    fn level_class(&mut self, lens: u32) -> u32;
    /// The lens's level at `setting` (0x747c40 with a second argument of 0).
    fn class_level(&mut self, lens: u32, setting: u32) -> i32;
}

/// What the vote reaches outside the library besides the record's fields.
pub trait SceneEvents {
    /// Calls the change callback the record's shared word +0x16c holds, when one is set, with
    /// (scene, flags, 0, 0, 1) (0x756d04, 0xf50cc8).
    fn scene_changed(&mut self, handle: RecordHandle, recognition: Recognition);
    /// Copies the adjustment diagnostics into their second slot 0x16c9164 under lock 0xa8
    /// (0xf50a58..0xf50aa0).
    fn snapshot_adjustments(&mut self);
}

/// The camera's debug trace: log 0xffb104 through 0x64744c.
pub trait SceneTrace {
    /// The scene's name line.
    fn scene_name(&mut self, scene: Scene);
    /// The flags' line: moving, then shake reduction, or neither.
    fn scene_flags(&mut self, flags: SceneFlags);
    /// The measurements line, in the format's order.
    fn measurements(&mut self, values: &[i32; 21]);
}

pub trait DetectorPorts: Properties + StockMemory + ProfileAccess + SubjectQuery + Steadiness + Overlay {}

impl<T: Properties + StockMemory + ProfileAccess + SubjectQuery + Steadiness + Overlay> DetectorPorts for T {}
