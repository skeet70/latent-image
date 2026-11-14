#![forbid(unsafe_code)]

//! The layer's service entries: thin requests to stock services, the profile factor and level
//! pair helpers, the pending display configuration and the dump file writer.

use crate::controller::{ProfileGeometry, Refusal, Status, Tables};
use crate::memory::Memory;
use crate::ports::{AdcReading, ConfigOffset, FileCompletion, Ports, Register, StockCode, StockData};
use crate::profile::ProfileHandle;
use crate::stock::layout::{display, dump};

const WINDOW_ROUTE: u32 = 13;
const ADC_CHANNEL: u32 = 5;
const STATUS_REGISTER: Register = Register(0xfff8_90d4);
const SELECTION: ConfigOffset = ConfigOffset(0xad);
const SELECTION_OFFSET: ConfigOffset = ConfigOffset(0xae);
const OPERATION_SWITCHES: [ConfigOffset; 2] = [ConfigOffset(0x5256), ConfigOffset(0x5257)];
const FILE_WAIT_TICKS: u32 = 3;
const FACTOR_SHIFT: u32 = 20;

/// The output route power the window holds (0xed80a8, 0xed80c4).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowPower {
    Held,
    Released,
}

impl WindowPower {
    const fn level(self) -> u32 {
        match self {
            WindowPower::Held => 8,
            WindowPower::Released => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionRequest {
    pub selection: bool,
    pub offset: bool,
    pub level: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionValues {
    /// The configured selection, zero from the split index on.
    pub selection: Option<u8>,
    pub offset: Option<i16>,
    pub level: Option<u16>,
}

/// A byte pair whose first byte says whether it holds a level and whose second says which.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LevelPair(pub [u8; 2]);

impl LevelPair {
    /// 0xef752c.
    pub const PRIMARY: Self = Self([2, 1]);
    /// 0xef7550.
    pub const SECONDARY: Self = Self([2, 2]);

    /// The pair's profile level class (0xef7380).
    pub const fn encoded_class(self) -> u32 {
        match self.0[0] as u32 + self.0[1] as u32 {
            sum @ 1..=4 => sum + 1,
            _ => 1,
        }
    }

    /// 0 for no level, 1 for a first byte alone, 2 for both bytes (0xef74f8).
    pub const fn class(self) -> u8 {
        match self.0 {
            [0, _] => 0,
            [_, 0] => 1,
            _ => 2,
        }
    }

    /// 0xef75d4.
    pub const fn first_complement(self) -> u8 {
        match self.0[0] {
            first @ 0..=1 => 1 - first,
            _ => 0,
        }
    }
}

/// The conditions under which the controllers' configured operation runs (0xef773c).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationConditions {
    /// Enables the operation while `held` is zero.
    pub requested: u32,
    /// Enables the operation when `requested` is zero and this is 1.
    pub alternate: u32,
    /// 1 refuses; any other non-zero value disables `requested`.
    pub held: u32,
    /// Each refuses at 1.
    pub vetoes: [u32; 3],
    /// Modes 1 and 3 refuse.
    pub mode: u32,
    pub profile: ProfileHandle,
    /// Non-zero refuses.
    pub busy: u32,
    /// 3 refuses.
    pub stage: u32,
}

/// 0xef8b78, 0xef8bc0.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtendedOperations {
    Primary,
    Secondary,
}

impl ExtendedOperations {
    const fn priorities(self) -> [(u32, u32); 5] {
        match self {
            ExtendedOperations::Primary => [(50, 3), (43, 4), (47, 4), (48, 4), (44, 5)],
            ExtendedOperations::Secondary => [(44, 6), (47, 5), (48, 5), (43, 5), (50, 3)],
        }
    }
}

/// The dump a caller writes; each names its file's extension (0xee9770..0xee97a0).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DumpKind {
    Raw,
    Tby,
    Tma,
    Tiy,
    Tcy,
    Sae,
    Binary,
}

impl DumpKind {
    pub const fn from_code(code: u32) -> Self {
        match code {
            0 => DumpKind::Raw,
            2 => DumpKind::Tby,
            3 => DumpKind::Tma,
            4 => DumpKind::Tiy,
            5 => DumpKind::Tcy,
            6 => DumpKind::Sae,
            _ => DumpKind::Binary,
        }
    }

    pub const fn extension(self) -> &'static [u8] {
        match self {
            DumpKind::Raw => b".raw\0",
            DumpKind::Tby => b".tby\0",
            DumpKind::Tma => b".tma\0",
            DumpKind::Tiy => b".tiy\0",
            DumpKind::Tcy => b".tcy\0",
            DumpKind::Sae => b".sae\0",
            DumpKind::Binary => b".bin\0",
        }
    }
}

mod part {
    pub const SELECTED_VALUE: u32 = 0x1;
    pub const LAYOUT: u32 = 0x4;
    pub const SELECTED: u32 = 0x10;
    pub const COLOUR: u32 = 0x40;
}

impl<M: Memory, P: Ports, T: Tables> ProfileGeometry<M, P, T> {
    /// The route's status (0xed80a8, 0xed80c4).
    pub fn set_window_power(&mut self, power: WindowPower) -> i32 { self.ports.set_requested_level(WINDOW_ROUTE, power.level()) }

    /// 0xee8e70.
    pub fn read_adc_channel5(&mut self) -> AdcReading { self.ports.read_adc_byte(ADC_CHANNEL) }

    /// 0xee8bb0, 0xef3350.
    pub fn device_status(&mut self) -> bool { self.ports.register(STATUS_REGISTER) & 0x4 != 0 }

    /// 0xee9708.
    pub fn read_peak_selection(&mut self) { self.ports.read_selection() }

    /// 0xee9718.
    pub fn notify_peak_return(&mut self) { self.ports.notify_return() }

    /// 0xee9728.
    pub fn peak_active(&mut self) -> bool { self.ports.state() & 0x3 == 0x3 }

    /// 0xeea030.
    pub fn selection_values(&mut self, request: SelectionRequest) -> SelectionValues {
        let selection = request.selection.then(|| {
            let selection = self.ports.byte(SELECTION);
            if selection >= self.ports.split_index() { 0 } else { selection }
        });
        let offset = request.offset.then(|| self.ports.signed_halfword(SELECTION_OFFSET));
        let level = request.level.then(|| {
            let limit = self.ports.selection_limit();
            self.ports.lens_value(limit) as u16
        });
        SelectionValues { selection, offset, level }
    }

    /// `values` with its second and third halfwords scaled by the selection's profile factors
    /// and the calibration factors of `first` and `second` (0xef43b8).
    pub fn apply_profile_factors(&mut self, values: [u16; 3], first: u32, second: u32, selection: u32) -> [u16; 3] {
        let profile = self.ports.profile_factors(selection);
        let [calibration_first, calibration_second, rounding] = self.ports.calibration_factors(first, second);
        let scaled = |value: u16, profile: u16, calibration: u16| {
            let product = u64::from(u32::from(profile) * u32::from(value));
            ((product * u64::from(calibration) + u64::from(rounding)) >> FACTOR_SHIFT) as u16
        };
        [values[0], scaled(values[1], profile[0], calibration_first), scaled(values[2], profile[1], calibration_second)]
    }

    /// 0xef72d4.
    pub fn converted_difference(&mut self, first: u32, second: u32) -> i16 {
        let first = self.ports.lens_value(first);
        first.wrapping_sub(self.ports.lens_value(second))
    }

    /// 0xef74d0.
    pub fn record_decade(&mut self, pair: LevelPair) -> i16 { self.ports.configured_decade(pair.0[1]) }

    /// 0xef7570.
    pub fn level_pair(&mut self, kind: u32, level: u32) -> LevelPair {
        let decoded = self.ports.decode_level(level);
        match kind {
            1 => LevelPair([decoded, 0]),
            2 if level == 4 => LevelPair::PRIMARY,
            2 => LevelPair::SECONDARY,
            _ => LevelPair([0, decoded]),
        }
    }

    /// 0xef773c.
    pub fn configured_operation(&mut self, conditions: &OperationConditions) -> bool {
        let flagged = self.profiles.flag14(conditions.profile);
        let refused = conditions.vetoes.contains(&1)
            || conditions.held == 1
            || conditions.busy != 0
            || conditions.stage == 3
            || !flagged;
        let enabled = (conditions.requested != 0 && conditions.held == 0) || (conditions.requested == 0 && conditions.alternate == 1);
        !refused && enabled && !matches!(conditions.mode, 1 | 3)
    }

    /// 0xef7844.
    pub fn configured_operation_available(&mut self, record: u32) -> bool {
        let switched = OPERATION_SWITCHES.iter().any(|&offset| self.ports.byte(offset) & 0x3 != 0);
        switched && self.ports.resource_byte(record) != 0
    }

    /// 0xef8b78, 0xef8bc0.
    pub fn configure_extended_operations(&mut self, operations: ExtendedOperations) {
        for (task, priority) in operations.priorities() {
            self.ports.configure_task(task, priority);
        }
    }

    /// Applies each pending part of the display configuration, clearing its bit, then has the
    /// driver enter `callback` (0xeeb5b4).
    pub fn apply_display_configuration(&mut self, callback: StockCode) -> Status {
        if self.pending_display() == 0 {
            return Err(Refusal::WrongState);
        }
        if self.pending_display() & part::LAYOUT != 0 {
            let applied = self.ports.write_selected_quad(0, display::QUAD)
                && self.ports.write_selected_channel(0, display::CHANNEL)
                && self.ports.apply_scale_layout(display::SCALE_LAYOUT)
                && {
                    let graphic = self.memory.read_u32(display::GRAPHIC);
                    self.ports.graphic_service(graphic)
                }
                && {
                    let bits = self.memory.read_u32(display::CONFIGURATION_BITS);
                    self.ports.update_configuration_bits(bits)
                }
                && self.ports.forward_selection(display::SELECTION);
            self.refuse_unless(applied)?;
            self.clear_display_part(part::LAYOUT);
        }
        if self.pending_display() & part::SELECTED != 0 {
            let selected = self.memory.read_u32(display::SELECTED);
            let applied = self.ports.apply_selected(selected);
            self.refuse_unless(applied)?;
            self.clear_display_part(part::SELECTED);
        }
        if self.pending_display() & part::COLOUR != 0 {
            self.ports.set_selected_colour(self.memory.read_u32(display::COLOUR));
            self.clear_display_part(part::COLOUR);
        }
        if self.pending_display() & part::SELECTED_VALUE != 0 {
            let value = self.memory.read_u32(display::SELECTED_VALUE);
            let applied = self.ports.forward_selected_value(value);
            self.refuse_unless(applied)?;
            self.clear_display_part(part::SELECTED_VALUE);
        }
        let accepted = self.ports.on_selected_event(callback);
        self.refuse_unless(accepted)
    }

    fn pending_display(&self) -> u32 { self.memory.read_u32(display::PENDING) }

    fn clear_display_part(&mut self, bit: u32) { self.memory.write_u32(display::PENDING, self.pending_display() & !bit) }

    fn refuse_unless(&self, accepted: bool) -> Status { if accepted { Ok(()) } else { Err(Refusal::WrongState) } }

    /// Writes `length` bytes at `data` to the next numbered file with `kind`'s extension, then
    /// stores the file's directory and file numbers at `numbers` (0xee92e4).
    ///
    /// Suspected stock defect, kept: when the open completes with a non-zero status, the write
    /// is skipped but its wait is not, so the call waits until something else clears the
    /// pending word (0xee9428..0xee9450).
    pub fn write_dump(&mut self, kind: DumpKind, data: Option<StockData>, length: u32, numbers: Option<StockData>) -> Status {
        let Some(data) = data else {
            return Err(Refusal::BadArgument);
        };
        self.ports.next_file_name(dump::FILE_NAME);
        self.ports.copy_text(dump::FILE_PATH, dump::FILE_NAME);
        self.ports.append_text(dump::FILE_PATH, kind.extension());

        self.memory.write_u32(dump::FILE_PENDING, 1);
        let opened = self.ports.open_file(FileCompletion::Opened, dump::FILE_PATH);
        self.memory.write_u32(dump::FILE_HANDLE, opened);
        self.wait_for_file();

        self.memory.write_u32(dump::FILE_PENDING, 1);
        if self.file_status() == 0 {
            let handle = self.memory.read_u32(dump::FILE_HANDLE);
            self.ports.write_file(FileCompletion::Done, handle, length, data);
        }
        self.wait_for_file();

        self.memory.write_u32(dump::FILE_PENDING, 1);
        let handle = self.memory.read_u32(dump::FILE_HANDLE);
        if self.file_status() == 0 {
            self.ports.close_file(FileCompletion::Done, handle);
        } else {
            self.ports.finish_file(FileCompletion::Done, handle);
        }
        self.wait_for_file();

        if let Some(numbers) = numbers {
            self.ports.read_file_numbers(numbers);
        }
        Ok(())
    }

    fn file_status(&self) -> u32 { self.memory.read_u32(dump::FILE_STATUS) }

    /// 0xee8aec.
    fn wait_for_file(&mut self) {
        loop {
            self.ports.wait_ticks(FILE_WAIT_TICKS);
            if self.memory.read_u32(dump::FILE_PENDING) == 0 {
                break;
            }
        }
    }
}

/// The open file's completion (0xee89c4).
pub fn file_opened(memory: &mut impl Memory, status: u32, handle: u32) {
    memory.write_u32(dump::FILE_HANDLE, handle);
    file_done(memory, status);
}

/// A write or close request's completion (0xee89f0, 0xee8a18).
pub fn file_done(memory: &mut impl Memory, status: u32) {
    memory.write_u32(dump::FILE_STATUS, status);
    memory.write_u32(dump::FILE_PENDING, 0);
}
