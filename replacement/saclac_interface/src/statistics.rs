#![forbid(unsafe_code)]

//! Records statistics requests carry.

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
