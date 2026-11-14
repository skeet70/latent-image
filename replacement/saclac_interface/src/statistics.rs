#![forbid(unsafe_code)]

//! Payloads of the requests saclac's live statistics configuration posts to the statistics task
//! (queue 48), and the setting record of a statistics 0x4005 request. The blocks carry profile
//! values whose meaning stays with the statistics task.

use crate::calc::SharedAddress;
use crate::stock::StockCallbackWord;

/// The last word of the setup block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SetupTail {
    Default,
    /// Command11's fifth parameter word.
    Adjustment(u32),
    /// The S1 measurement's start carries 1 (0xf2311c).
    S1,
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
    /// The live view's starts carry 1 when the profile has a pending selection or an active
    /// view; the S1 start carries the profile's view-active byte.
    pub flag: u32,
}

/// Command09's parameter words the capture configuration forwards.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureCommand { pub second: u32, pub third: u32, pub fourth: u32 }

/// Command24's parameter words and the profile's selection ratio.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Command24 { pub second: u32, pub third: u32, pub fourth: u32, pub fifth: u32, pub ratio: [u32; 2] }
/// Bytes a requester built and statistics reads whole; its fields are the requester's to type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Record<const BYTES: usize>(pub [u8; BYTES]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatisticsPayload {
    Start { configuration: Configuration, feature: u32, callback: StockCallbackWord },
    StartSelected { configuration: Configuration, callback: StockCallbackWord },
    /// The S1 measurement's start with nothing selected; with a selection the S1 start is
    /// `StartSelected`.
    S1Start { configuration: Configuration, header: u32, callback: StockCallbackWord },
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
    /// X-M1 1.50: 0x103a09c posts 0x400b with 4 bytes from its argument.
    Request0b {
        /// Payload 0, 4 bytes, stored at 0x103a0a8.
        value: u32,
    },
    /// X-M1 1.50: 0x103a13c posts 0x400e with 32 bytes; 0xf3be7c builds the record.
    Request0e {
        /// Payload 0..28, copied at 0x103a164.
        record: Record<28>,
        /// Payload 28, 4 bytes, stored at 0x103a148.
        callback: StockCallbackWord,
    },
    /// X-M1 1.50: 0x103a200 posts 0x400f with 4 bytes from its argument.
    Request0f {
        /// Payload 0, 4 bytes, stored at 0x103a20c.
        callback: StockCallbackWord,
    },
    /// X-M1 1.50: 0x103b064 posts 0x4043 with no bytes when the word at 0x016f3afc is zero.
    Request43,
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


/// The 144-byte record of a statistics 0x4005 request (X-M1 1.50 id5 0x1039b48 copies its four
/// parts, 56, 16, 16 and 56 bytes, ahead of the callback word).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatisticsSetting([u8; StatisticsSetting::BYTES]);

/// The capture-setting command's values a setting request lays over the capture's last record
/// (0xf2a14c..0xf2a1a0).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SettingOverrides {
    /// The word at +4 of the client block the command's word +0x24 names.
    pub selector: u32,
    /// Command words +0x4, +0xc, +0x10, +0x8 and +0x20, in record order.
    pub words: [u32; 5],
    /// Six bytes of the client block the command's word +0x18 names.
    pub window: [u8; 6],
    /// The byte at +4 of the client block the command's word +0x1c names.
    pub option: u8,
}

impl StatisticsSetting {
    pub const BYTES: usize = 144;
    const SELECTOR: usize = 0x20;
    const WORDS: usize = 0x24;
    const EXTRA: usize = 56 + 16 + 16;
    const WINDOW: usize = Self::EXTRA + 0x24;
    const OPTION: usize = Self::EXTRA + 0x34;

    pub const fn from_bytes(bytes: [u8; Self::BYTES]) -> Self { Self(bytes) }

    pub const fn bytes(&self) -> &[u8; Self::BYTES] { &self.0 }

    pub fn apply(&mut self, overrides: &SettingOverrides) {
        self.put_word(Self::SELECTOR, overrides.selector);
        for (index, word) in overrides.words.iter().enumerate() {
            self.put_word(Self::WORDS + 4 * index, *word);
        }
        self.0[Self::WINDOW..Self::WINDOW + 6].copy_from_slice(&overrides.window);
        self.0[Self::OPTION] = overrides.option;
    }

    fn put_word(&mut self, offset: usize, value: u32) { self.0[offset..offset + 4].copy_from_slice(&value.to_le_bytes()); }
}
