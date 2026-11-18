#![forbid(unsafe_code)]

//! The diagnostic copy of the controller's parameters, kept only while the diagnostic block is
//! enabled.

use crate::controller::{ProfileGeometry, Tables};
use crate::memory::Memory;
use crate::numeric::centi_log2;
use crate::ports::{ConfigOffset, Ports};
use crate::profile::{ProfileHandle, ProfileKind};
use crate::stock::layout::{diagnostics as field, parameters, work};

/// The feature with no label in the description.
const UNLABELLED_FEATURE: u8 = 11;

/// Writes `value` in decimal, zero-padded to `width` digits.
fn push_padded(text: &mut Text, value: u16, width: usize) {
    let mut digits = [b'0'; 5];
    let mut rest = value;
    for digit in digits.iter_mut().rev() {
        *digit = b'0' + (rest % 10) as u8;
        rest /= 10;
    }
    text.push(&digits[digits.len() - width..]);
}

/// A description bounded to its field, which always ends in a zero byte.
struct Text {
    bytes: [u8; field::DESCRIPTION_BYTES],
    len: usize,
}

impl Text {
    const fn new() -> Self { Self { bytes: [0; field::DESCRIPTION_BYTES], len: 0 } }

    fn push(&mut self, bytes: &[u8]) {
        for &byte in bytes.iter().take_while(|&&byte| byte != 0) {
            if self.len + 1 < self.bytes.len() {
                self.bytes[self.len] = byte;
                self.len += 1;
            }
        }
    }

    fn push_label(&mut self, write: impl FnOnce(&mut [u8]) -> usize) {
        let mut label = [0; field::DESCRIPTION_BYTES];
        let len = write(&mut label).min(label.len());
        self.push(&label[..len]);
    }
}

impl<M: Memory, P: Ports, T: Tables> ProfileGeometry<M, P, T> {
    fn diagnostics_enabled(&self) -> bool { self.memory.read_u32(field::ENABLED) != 0 }

    /// Writes the profile's frame rate and labels into the description (0xeea3b4).
    pub(crate) fn describe_profile(&mut self, profile: ProfileHandle) {
        if self.ports.byte(ConfigOffset(0x98d1)) == 0 && self.ports.byte(ConfigOffset(0xb6)) == 1 {
            return;
        }
        if !self.diagnostics_enabled() {
            return;
        }
        let mut text = Text::new();
        if self.memory.read_u8(field::FIXED) != 0 {
            text.push_label(|into| self.ports.fixed_label(into));
        } else {
            let entry = self.profiles.entry(profile);
            let rate = match entry.kind {
                ProfileKind(2 | 3) => self.ports.configured_rate(),
                _ => (self.profiles.selected_threshold(profile) as u32 * 100) as u16,
            };
            if rate != 0 {
                push_padded(&mut text, rate / 100, 3);
                text.push_label(|into| self.ports.rate_separator(into));
                push_padded(&mut text, rate % 100, 2);
                text.push_label(|into| self.ports.rate_suffix(into));
            }
            text.push_label(|into| self.ports.profile_label(profile, into));
            if entry.feature != 0 && entry.feature != UNLABELLED_FEATURE {
                text.push_label(|into| self.ports.feature_label(entry.feature, into));
            }
        }
        self.memory.write_bytes(field::DESCRIPTION, &text.bytes);
    }

    /// 0xeea5a4.
    pub(crate) fn set_diagnostic_index(&mut self, scale: u32, index: u32) {
        if !self.diagnostics_enabled() {
            return;
        }
        self.memory.write_u8(field::INDEX, index as u8);
        let value = self.ports.transform(index, scale);
        self.memory.write_u16(field::INDEX_VALUE, value);
    }

    /// 0xeea5e4.
    pub(crate) fn set_diagnostic_value(&mut self, value: u32) {
        if self.diagnostics_enabled() {
            self.memory.write_u32(field::VALUE, value);
        }
    }

    /// 0xeea614.
    pub(crate) fn set_diagnostic_limit(&mut self, limit: i16) {
        if self.diagnostics_enabled() {
            self.memory.write_u16(field::LIMIT, limit as u16);
        }
    }

    /// 0xeea644.
    pub(crate) fn set_diagnostic_secondary_limit(&mut self, limit: i16) {
        if self.diagnostics_enabled() {
            self.memory.write_u16(field::SECONDARY_LIMIT, limit as u16);
        }
    }

    /// 0xeea6a8.
    pub(crate) fn set_diagnostic_white_balance(&mut self, values: [u16; 3]) {
        if !self.diagnostics_enabled() {
            return;
        }
        let [first, second, third] = values.map(u32::from);
        self.memory.write_u32(field::WHITE_BALANCE[0], first | second << 16);
        self.memory.write_u32(field::WHITE_BALANCE[1], first | third << 16);
    }

    /// log2 of `rate / 100` in hundredths, zero below 100 (0xeea6f8).
    pub(crate) fn set_diagnostic_rate(&mut self, rate: u32) {
        if self.diagnostics_enabled() {
            let log = centi_log2(self.profiles.tables(), rate / 100).unwrap_or(0);
            self.memory.write_u16(field::RATE, log);
        }
    }

    /// 0xeea748.
    pub(crate) fn set_diagnostic_gains(&mut self, gains: [u16; 4]) {
        if !self.diagnostics_enabled() {
            return;
        }
        let [first, second, third, fourth] = gains.map(u32::from);
        self.memory.write_u32(field::GAINS[0], first | second << 16);
        self.memory.write_u32(field::GAINS[1], third | fourth << 16);
    }

    /// The quad selection's gains in the order the diagnostic block keeps them.
    pub(crate) fn quad_gains(&self) -> [u16; 4] {
        let [first, second, _, fourth] = work::QUAD_GAINS.map(|at| self.memory.read_u16(at));
        [first, second, fourth, second]
    }

    /// The parameter fields the diagnostic block mirrors, as 0xed87b0 copies them.
    pub(crate) fn publish_parameter_diagnostics(&mut self) {
        let profile = ProfileHandle(self.memory.read_u32(parameters::PROFILE));
        self.describe_profile(profile);
        let scale = self.memory.read_u32(parameters::INDEX_SCALE);
        let index = self.memory.read_u32(parameters::INDEX);
        self.set_diagnostic_index(scale, index);
        self.set_diagnostic_limit(self.memory.read_i16(parameters::LIMIT));
        self.set_diagnostic_secondary_limit(self.memory.read_i16(parameters::SECONDARY_LIMIT));
        self.set_diagnostic_white_balance(self.white_balance());
        self.set_diagnostic_value(self.memory.read_u32(parameters::AREA));
    }

    pub(crate) fn white_balance(&self) -> [u16; 3] {
        work::WHITE_BALANCE.map(|at| self.memory.read_u16(at))
    }
}
