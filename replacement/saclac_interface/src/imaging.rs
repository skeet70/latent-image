#![forbid(unsafe_code)]

//! Payloads of the requests saclac posts to the imaging task (imgdc). The profile values they
//! carry keep the meaning imgdc gives them.

use crate::stock::StockCallbackWord;

/// The profile values the S1 statistics configuration hands its imaging job, taken at the
/// S1 begin.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct S1Job {
    /// Shared-block halfwords +0x206, +0x208 and +0x238.
    pub levels: [i16; 3],
    pub lens_index: u32,
    /// Shared-block word +0x200.
    pub lens_word: u32,
    pub selection: u32,
    /// Shared-block word +0x230.
    pub step: u32,
    /// Shared-block halfword +0x234 and byte +0x236.
    pub step_limit: u16,
    pub step_mode: u8,
    pub curves: [u32; 2],
    pub output: u32,
    pub view_mode: u32,
    /// The descriptor for the lens's current value, then the descriptor pair.
    pub descriptors: [u32; 3],
    /// The descriptor for any value, then the descriptor pair.
    pub setup: [u32; 3],
    /// The statistics window block's last two words.
    pub window: [u32; 2],
    /// Shared-block word +0x218 and halfword +0x216.
    pub exposure: u32,
    pub exposure_offset: i16,
    pub view_active: u32,
    pub unselected_view: u32,
    pub view_byte: u8,
    pub classes: [u32; 2],
    pub ratio: u32,
    pub span: u32,
    pub class: u32,
    pub unselected: u32,
    pub scene: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImagingPayload {
    /// The callbacks post completions 0x57 (configured), 0x58 (ended) and 0x59 (reported).
    BeginS1 { job: S1Job, callbacks: [StockCallbackWord; 3] },
    EndS1 { callback: StockCallbackWord },
}

/// One of the two identical value groups the view configuration carries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ValueGroup {
    pub first: u32,
    pub second: u16,
    pub third: u8,
    pub fourth: u32,
    pub fifth: u32,
    pub sixth: u16,
    /// Already corrected by the image configuration for `fourth`.
    pub seventh: u16,
    pub eighth: u16,
    pub ninth: u32,
    pub tenth: u16,
    pub eleventh: u32,
}

/// The profile layers' values the view configuration request carries unchanged.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ConfigurationValues {
    pub head: [u32; 3],
    pub windows: [[u32; 3]; 2],
    pub block: [u32; 4],
    pub group: ValueGroup,
    pub converted: [u16; 2],
    pub properties: [u16; 4],
    pub tail: [u32; 3],
    pub restorable: u8,
    pub level: u16,
    pub detail: u8,
    pub classes: [u8; 2],
    pub scale: u8,
    pub capture: u8,
    pub rate: u8,
    pub restarts: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectionClass { Unselected, Selected, Flagged }

/// What the view configuration carries about the preparation before it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Prepared { #[default] None, Gated, Ungated, Refresh, Autofocus }

impl Prepared {
    pub const fn word(self) -> u32 {
        match self { Self::None => 0, Self::Gated => 1, Self::Ungated => 2, Self::Refresh => 3, Self::Autofocus => 4 }
    }
}

/// The view configuration request's content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Configured {
    pub values: ConfigurationValues,
    pub selection: u32,
    pub selection_mode: u32,
    pub selection_class: SelectionClass,
    pub refresh: u32,
    pub offset: u16,
    pub motion: u8,
    pub active: u8,
    pub view_mode: u8,
    pub direct_start: bool,
    pub prepared: Prepared,
}

/// The adjustment request's values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Adjustment { pub selection: u32, pub mode: u32, pub first: u32, pub second: u32 }

/// The secondary profile's options the view restore and stop carry.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RestoreOptions { pub first: bool, pub second: bool, pub negative: bool }

impl RestoreOptions {
    pub const fn any(self) -> bool { self.first || self.second || self.negative }
}

/// The changed-field group of an autofocus or Command22 change, as the view stop's mode codes it (2 to 5).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChangeCode(u8);

impl ChangeCode {
    pub const fn new(code: u8) -> Option<Self> {
        if 2 <= code && code <= 5 { Some(Self(code)) } else { None }
    }
    pub const fn get(self) -> u8 { self.0 }
}

/// The motion-control setting the view stop samples for later view requests.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MotionSetting(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StopMode { Plain, Secondary, SecondaryNegative, Change(ChangeCode) }

/// A live-view request with the values its imgdc payload carries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ViewPayload {
    Start { value: u32, measured: bool },
    Restore(RestoreOptions),
    Refresh { value: u32, motion: MotionSetting },
    Configure(Configured),
    Stop(StopMode),
    Suspend,
    Resume,
    Adjust(Adjustment),
    Window(u32),
}
