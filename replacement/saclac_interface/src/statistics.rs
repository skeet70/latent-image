#![forbid(unsafe_code)]

//! Payloads of the requests saclac's live statistics configuration posts to the statistics task
//! (queue 48). The blocks carry profile values whose meaning stays with the statistics task.

use crate::calc::SharedAddress;
use crate::stock::StockCallbackWord;

/// The last word of the setup block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SetupTail {
    Default,
    /// Command11's fifth parameter word.
    Adjustment(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Setup { pub descriptor: u32, pub pair: [u32; 2], pub tail: SetupTail }

/// The view format the statistics measure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Format {
    pub dimensions: [u32; 2],
    pub ratio: [u16; 2],
    pub value: u32,
    pub class: u32,
    /// Zero when nothing is selected.
    pub mode: u32,
}

/// The measurement window after the profile's correction factors, and its level.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MeasuredWindow { pub corrected: [u16; 3], pub level: u16, pub extent: u32 }

/// The profile's shared measurement source.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Source { pub word: u32, pub byte: u8 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Exposure {
    pub settings: [u32; 8],
    pub window: MeasuredWindow,
    pub adjusted: u16,
    pub linear: u16,
    pub source: Source,
    pub class: u8,
    pub detail: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capture {
    pub position: u32,
    pub selection: u32,
    pub settings: [u32; 2],
    /// The profile's retune word, or 1 to resume and 0 to suspend.
    pub retune: u32,
    pub options: [u8; 5],
    pub spans: [u32; 2],
    pub dimensions: [u32; 2],
    pub policy: u16,
}

/// Where the statistics task writes its results, and the callback it enters when they are there.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Notice { pub pending: u32, pub results: SharedAddress, pub callback: StockCallbackWord }

/// The blocks every start and capture configuration carries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Configuration {
    pub setup: Setup,
    pub format: Format,
    pub window: [u32; 4],
    pub output: [u32; 4],
    pub exposure: Exposure,
    pub capture: Capture,
    pub notice: Notice,
    /// The profile has a pending selection or an active view.
    pub flagged: bool,
}

/// Command09's parameter words the capture configuration forwards.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureCommand { pub second: u32, pub third: u32, pub fourth: u32 }

/// Command24's parameter words and the profile's selection ratio.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Command24 { pub second: u32, pub third: u32, pub fourth: u32, pub fifth: u32, pub ratio: [u32; 2] }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatisticsPayload {
    Start { configuration: Configuration, feature: u32, callback: StockCallbackWord },
    StartSelected { configuration: Configuration, callback: StockCallbackWord },
    Configure { configuration: Configuration, view: u32, command: CaptureCommand, callbacks: [StockCallbackWord; 3] },
    Stop { pause: bool, shot_idle: bool, callback: StockCallbackWord },
    Release { callback: StockCallbackWord },
    MeasuredWindow(MeasuredWindow),
    MeasuredSource { source: Source, position: u32 },
    Adjust { setup: Setup, callbacks: [StockCallbackWord; 2] },
    Command13,
    Prepare { selection: [u32; 3], range: [u32; 4], exposure: Exposure, callback: StockCallbackWord },
    Output { output: [u32; 4], callback: StockCallbackWord },
    Window { window: [u32; 4], callback: StockCallbackWord },
    Format { format: Format, callback: StockCallbackWord },
    Exposure { exposure: Exposure, callback: StockCallbackWord },
    Capture { capture: Capture, callback: StockCallbackWord },
    Selection { pending: u32, results: SharedAddress, callbacks: [StockCallbackWord; 2], flagged: bool },
    Command24 { command: Command24, callback: StockCallbackWord },
}

/// The bytes the statistics task writes at a `Notice`'s results address.
pub const RESULTS_BYTES: usize = 0x414;

/// The capture's statistics Command09's client receives with status 17.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureReport<'a> {
    /// Zero when the format has no capture window.
    pub dimensions: [u32; 2],
    /// The completion's 64-byte payload.
    pub completion: &'a [u8],
    pub results: &'a [u8; RESULTS_BYTES],
}
