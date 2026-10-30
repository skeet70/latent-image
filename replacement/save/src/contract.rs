//! Closed save-request values. The `abi` adapter maps them to X-M1 frame words.

/// The four develop-save kinds plus kind 3, which the facade handles but no
/// reachable dispatcher case supplies.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SaveKind {
    Kind0,
    Kind1,
    Kind2,
    Kind3,
    Kind4,
    Unrecognized(i32),
}

impl SaveKind {
    pub const fn from_abi(word: i32) -> Self {
        match word {
            0 => Self::Kind0,
            1 => Self::Kind1,
            2 => Self::Kind2,
            3 => Self::Kind3,
            4 => Self::Kind4,
            other => Self::Unrecognized(other),
        }
    }

    pub const fn bank(self) -> DescriptorBank {
        match self {
            Self::Kind1 => DescriptorBank::BANK1,
            Self::Kind2 | Self::Kind3 | Self::Kind4 => DescriptorBank::BANK2,
            _ => DescriptorBank::BANK0,
        }
    }

    pub const fn save_mode(self) -> SaveMode {
        match self {
            Self::Kind2 | Self::Kind3 => SaveMode::Three,
            Self::Kind4 => SaveMode::One,
            _ => SaveMode::Zero,
        }
    }

    pub const fn storage_variant(self) -> StorageVariant {
        match self {
            Self::Kind3 => StorageVariant::KindThree,
            _ => StorageVariant::Direct,
        }
    }

}

/// Descriptor bank in the frame record. Bank stride is `0x150`, bank 0 starts
/// at `frame + 0x27d0`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DescriptorBank(u8);

impl DescriptorBank {
    pub const BANK0: Self = Self(0);
    pub const BANK1: Self = Self(1);
    pub const BANK2: Self = Self(2);

    pub const fn index(self) -> u8 {
        self.0
    }
}

/// Descriptor word at `+0x04`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum SaveMode {
    Zero = 0,
    One = 1,
    Three = 3,
}

impl SaveMode {
    pub const fn word(self) -> u32 {
        self as u32
    }
}

/// Storage entry selects descriptor word `+0x14c`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum StorageVariant {
    Direct = 0,
    KindThree = 1,
}

/// Completion callback argument from the storage machine.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CompletionCode(pub i32);

/// The save status word stored by the completion callback.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RawStatus(pub i32);

impl RawStatus {
    pub const SAVED: Self = Self(0);

    pub const fn is_saved(self) -> bool {
        self.0 == 0
    }

    pub const fn from_completion(code: CompletionCode) -> Self {
        match code.0 {
            0 => Self(0),
            -4 => Self(1),
            _ => Self(2),
        }
    }
}

/// The task event mask returned by the phase-one wait.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SaveFlags(u32);

impl SaveFlags {
    /// Mask bit whose presence makes the facade return `0`.
    pub const INTERRUPTION: u32 = 0x20;

    pub const fn from_word(word: u32) -> Self {
        Self(word)
    }

    pub const fn interrupted(self) -> bool {
        self.0 & Self::INTERRUPTION != 0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TaskId(u32);

impl TaskId {
    pub const fn from_word(word: u32) -> Option<Self> {
        if word == 0 {
            None
        } else {
            Some(Self(word))
        }
    }

    pub const fn word(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WaitMask(u32);

impl WaitMask {
    pub const PHASE_ONE: Self = Self(0x30);
    pub const PHASE_TWO: Self = Self(0x10);

    pub const fn word(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SignalMask(u32);

impl SignalMask {
    pub const COMPLETION: Self = Self(0x10);

    pub const fn word(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitTimeout {
    Infinite,
}

impl WaitTimeout {
    pub const fn word(self) -> u32 {
        match self {
            Self::Infinite => u32::MAX,
        }
    }
}

/// The two `0x006fae80` activity points.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StagePoint {
    Start,
    End,
}

impl StagePoint {
    pub const fn word(self) -> u32 {
        match self {
            Self::Start => 8,
            Self::End => 10,
        }
    }
}

/// The two save completion slots in the shared task table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SaveTaskSlot {
    PhaseOne,
    PhaseTwo,
}

/// The selector byte read by the admission test.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConfigByte {
    SaveDispatch,
}

impl ConfigByte {
    pub const fn index(self) -> u32 {
        match self {
            Self::SaveDispatch => 0xc3,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ServiceProfile(pub u32);

impl ServiceProfile {
    pub const fn is_a2(self) -> bool {
        self.0 == 0xa2
    }

    pub const fn is_a4(self) -> bool {
        self.0 == 0xa4
    }

    pub const fn is_a5(self) -> bool {
        self.0 == 0xa5
    }

    pub const fn is_a7(self) -> bool {
        self.0 == 0xa7
    }

    pub const fn is_a8(self) -> bool {
        self.0 == 0xa8
    }

    pub const fn is_a9(self) -> bool {
        self.0 == 0xa9
    }

    pub const fn above_0x82(self) -> bool {
        self.0 > 0x82
    }

    pub const fn below_0x88(self) -> bool {
        self.0 < 0x88
    }
}

/// Argument of the save-metadata preparation call: `1` only for kind 0 with
/// `frame + 0x275c == 1`, otherwise `0`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MetadataPass {
    Selected,
    Alternate,
}

impl MetadataPass {
    pub const fn word(self) -> u32 {
        match self {
            Self::Selected => 1,
            Self::Alternate => 0,
        }
    }
}

/// Frame words the facade reads. The adapter owns their addresses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameField {
    /// `+0x275c`
    FrameState,
    /// `+0x604`
    SubframeCommand,
    /// `+0x5f0`, compared with `1`
    ExtentPresetSource,
    /// `+0x578`, compared with `2`
    ExtentLayout,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExtentPreset {
    One,
    Zero,
}

impl ExtentPreset {
    pub const fn from_source(word: u32) -> Self {
        if word == 1 {
            Self::One
        } else {
            Self::Zero
        }
    }

    pub const fn word(self) -> u32 {
        match self {
            Self::One => 1,
            Self::Zero => 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SaveCompletion(pub unsafe extern "C" fn(i32));

#[derive(Clone, Copy, Debug)]
pub struct ReleaseCompletion(pub unsafe extern "C" fn());

/// The ABI status-out pointer.
#[derive(Clone, Copy, Debug)]
pub struct StatusOut(*mut i32);

impl StatusOut {
    /// # Safety
    /// `pointer` is null or points to one writable `i32` for the duration of
    /// the facade call.
    pub unsafe fn new(pointer: *mut i32) -> Option<Self> {
        if pointer.is_null() {
            None
        } else {
            Some(Self(pointer))
        }
    }

    pub fn write(self, status: RawStatus) {
        unsafe { core::ptr::write_volatile(self.0, status.0) }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SaveReturn {
    /// The ABI returns `1`.
    Success,
    /// The ABI returns `0`.
    Interrupted,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PhaseOne {
    /// The admission shortcut returned before any storage request.
    NotWaited,
    Observed(SaveFlags),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PhaseTwo {
    NotRun,
    Completed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SaveOutcome {
    pub returned: SaveReturn,
    /// The last value written through the status-out pointer.
    pub status: RawStatus,
    pub shortcut: bool,
    pub phase_one: PhaseOne,
    pub phase_two: PhaseTwo,
}
