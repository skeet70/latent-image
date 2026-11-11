pub use super::scanlines::{Dimension, Row, RowCount};
pub use super::Opcode;

macro_rules! signed_units {
    ($($name:ident),* $(,)?) => { $(
        #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
        pub struct $name(pub i32);
        impl Word for $name {
            fn from_word(value: i32) -> Self { Self(value) }
            fn word(self) -> i32 { self.0 }
        }
    )* };
}
macro_rules! unsigned_units {
    ($($name:ident),* $(,)?) => { $(
        #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
        pub struct $name(pub u32);
        impl Word for $name {
            fn from_word(value: i32) -> Self { Self(value as u32) }
            fn word(self) -> i32 { self.0 as i32 }
        }
    )* };
}
trait Word: Copy {
    fn from_word(value: i32) -> Self;
    fn word(self) -> i32;
}
signed_units!(GridCoordinate, ScaleValue, PointCount, ValueCount, Remaining, ProgramOffset, OutputOffset, GridShift, GridUnit, GridMask);
unsigned_units!(ScaleIndex, PointIndex, RasterMode);
impl Word for bool {
    fn from_word(value: i32) -> Self { value != 0 }
    fn word(self) -> i32 { i32::from(self) }
}
impl Word for Opcode {
    fn from_word(value: i32) -> Self { Self::new(value as u8) }
    fn word(self) -> i32 { i32::from(self.get()) }
}
macro_rules! shared_unit {
    ($($name:ident),* $(,)?) => { $(
        impl Word for $name {
            fn from_word(value: i32) -> Self { Self(value) }
            fn word(self) -> i32 { self.0 }
        }
    )* };
}
shared_unit!(Dimension, Row, RowCount);
impl RasterMode {
    pub const CONTOUR: Self = Self(0);
    pub const COORDINATES: Self = Self(1);
    pub const SCANLINES: Self = Self(2);
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BytePoint { pub x: u8, pub y: u8 }
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RowPair { pub first: i16, pub second: i16 }
impl RowPair {
    pub fn from_packed(value: u32) -> Self { Self { first: value as i16, second: (value >> 16) as i16 } }
    pub fn packed(self) -> u32 { u32::from(self.first as u16) | (u32::from(self.second as u16) << 16) }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Axis { X, Y }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Side { Minimum, Maximum }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoefficientBank { Values, X, Y }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scale { Small, Large }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecuteStatus { Complete, InvalidOpcode, RunCapacity }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlushStatus { Complete, RunCapacity }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum InstructionSignal { Continue = 0, Finished = 0x8003, Flush = 0x8004 }

macro_rules! state {
    ($($variant:ident => $field:ident, $setter:ident: $kind:ty),*; $($extra:ident, $extra_setter:ident: $extra_kind:ty),* $(,)?) => {
        #[derive(Debug)]
        pub struct VmState { $(pub $field: $kind,)* $(pub $extra: $extra_kind,)* }
        impl Default for VmState {
            fn default() -> Self { Self { $($field: <$kind as Word>::from_word(0),)* $($extra: <$extra_kind as Word>::from_word(0),)* } }
        }
        pub trait State {
            $(fn $field(&self) -> $kind;
            fn $setter(&mut self, value: $kind);)*
            $(fn $extra(&self) -> $extra_kind;
            fn $extra_setter(&mut self, value: $extra_kind);)*
        }
        impl State for VmState {
            $(fn $field(&self) -> $kind { self.$field }
            fn $setter(&mut self, value: $kind) { self.$field = value; })*
            $(fn $extra(&self) -> $extra_kind { self.$extra }
            fn $extra_setter(&mut self, value: $extra_kind) { self.$extra = value; })*
        }
        #[derive(Clone, Copy)]
        enum Field { $($variant,)* }
        impl<T: Storage> Vm<'_, T> {
            #[inline(always)]
            fn get(&self, field: Field) -> i32 {
                match field { $(Field::$variant => self.storage.state().$field().word(),)* }
            }
            #[inline(always)]
            fn set(&mut self, field: Field, value: i32) {
                match field { $(Field::$variant => self.storage.state_mut().$setter(<$kind as Word>::from_word(value)),)* }
            }
        }
    };
}
state! {
    PointCount => point_count, set_point_count: PointCount,
    ValueCount => value_count, set_value_count: ValueCount,
    Remaining => remaining, set_remaining: Remaining,
    Cursor => cursor, set_cursor: ProgramOffset,
    Width => width, set_width: Dimension,
    Height => height, set_height: Dimension,
    Oversample => oversample, set_oversample: bool,
    ScaleX => scale_x, set_scale_x: ScaleValue,
    ScaleY => scale_y, set_scale_y: ScaleValue,
    Multiplier => multiplier, set_multiplier: ScaleValue,
    Mode => mode, set_mode: RasterMode,
    RowCount => row_count, set_row_count: RowCount,
    SourceX => source_x, set_source_x: PointIndex,
    DestinationX => destination_x, set_destination_x: PointIndex,
    SourceY => source_y, set_source_y: PointIndex,
    DestinationY => destination_y, set_destination_y: PointIndex,
    FirstX => first_x, set_first_x: GridCoordinate,
    FirstY => first_y, set_first_y: GridCoordinate,
    SecondX => second_x, set_second_x: GridCoordinate,
    SecondY => second_y, set_second_y: GridCoordinate,
    ExtentX => extent_x, set_extent_x: GridCoordinate,
    ExtentY => extent_y, set_extent_y: GridCoordinate,
    StartX => start_x, set_start_x: GridCoordinate,
    StartY => start_y, set_start_y: GridCoordinate,
    ControlX => control_x, set_control_x: GridCoordinate,
    ControlY => control_y, set_control_y: GridCoordinate,
    EndX => end_x, set_end_x: GridCoordinate,
    EndY => end_y, set_end_y: GridCoordinate,
    FinishX => finish_x, set_finish_x: GridCoordinate,
    FinishY => finish_y, set_finish_y: GridCoordinate,
    LeftState => left_state, set_left_state: bool,
    RightState => right_state, set_right_state: bool,
    Unit => unit, set_unit: GridUnit,
    HalfUnit => half_unit, set_half_unit: GridUnit,
    Mask => mask, set_mask: GridMask,
    Shift => shift, set_shift: GridShift,
    HalfShift => half_shift, set_half_shift: GridShift,
    FirstRow => first_row, set_first_row: Row,
    HasControl => has_control, set_has_control: bool,
    EdgeSide => edge_side, set_edge_side: bool,
    SmallScale => small_scale, set_small_scale: bool,
    LeftCursor => left_cursor, set_left_cursor: OutputOffset,
    RightCursor => right_cursor, set_right_cursor: OutputOffset,
    MinimumRow => minimum_row, set_minimum_row: Row,
    LeftMaximumRow => left_maximum_row, set_left_maximum_row: Row,
    RightMaximumRow => right_maximum_row, set_right_maximum_row: Row,
    ScanHalf => scan_half, set_scan_half: GridUnit,
    ScanStep => scan_step, set_scan_step: GridUnit;
    left_y, set_left_y: GridCoordinate,
    right_y, set_right_y: GridCoordinate,
    operand, set_operand: Opcode,
}

pub trait Storage {
    type Error;
    type State: State;
    fn state(&self) -> &Self::State;
    fn state_mut(&mut self) -> &mut Self::State;
    fn read_byte(&mut self) -> Result<u8, Self::Error>;
    fn read_short(&mut self) -> Result<i16, Self::Error>;
    fn point(&self, axis: Axis, index: PointIndex) -> Result<GridCoordinate, Self::Error>;
    fn set_point(&mut self, axis: Axis, index: PointIndex, value: GridCoordinate) -> Result<(), Self::Error>;
    fn coefficient(&self, bank: CoefficientBank, index: ScaleIndex) -> Result<ScaleValue, Self::Error>;
    fn set_coefficient(&mut self, bank: CoefficientBank, index: ScaleIndex, value: ScaleValue) -> Result<(), Self::Error>;
    fn initial_half(&self, index: ScaleIndex) -> Result<u16, Self::Error>;
    fn initial_value(&self, index: ScaleIndex) -> Result<u8, Self::Error>;
    fn initial_point(&self, index: PointIndex) -> Result<BytePoint, Self::Error>;
    fn output_half(&self, side: Side, offset: OutputOffset) -> Result<i16, Self::Error>;
    fn set_output_half(&mut self, side: Side, offset: OutputOffset, value: i16) -> Result<(), Self::Error>;
    fn set_output_pair(&mut self, side: Side, offset: OutputOffset, value: RowPair) -> Result<(), Self::Error>;
    fn contour_rows(&mut self) -> Result<RowCount, Self::Error>;
    fn select_scale(&mut self, scale: Scale);
    fn draw_curve(&mut self) -> Result<(), Self::Error>;
    fn flush_scanlines(&mut self) -> Result<FlushStatus, Self::Error>;
}

#[derive(Clone, Copy)]
struct Point { x: GridCoordinate, y: GridCoordinate }
#[derive(Clone, Copy)]
enum Encoding { Short, Byte }
#[derive(Clone, Copy)]
enum Basis { Axis, Rotated }
#[derive(Clone, Copy)]
enum CurveEnd { Stored, Read, ReadStart }
struct Vm<'a, T: Storage> { storage: &'a mut T }

fn lsl(value: i32, shift: i32) -> i32 {
    let shift = shift as u32 & 255;
    if shift >= 32 { 0 } else { value.wrapping_shl(shift) }
}
fn asr(value: i32, shift: i32) -> i32 {
    let shift = shift as u32 & 255;
    if shift >= 32 { value >> 31 } else { value >> shift }
}
fn div256(value: i32) -> i32 { value / 256 }

impl<T: Storage> Vm<'_, T> {
    fn read_byte(&mut self) -> Result<u8, T::Error> { self.storage.read_byte() }
    fn read_short(&mut self) -> Result<i32, T::Error> { self.storage.read_short().map(i32::from) }
    fn operand(&self) -> u8 { self.storage.state().operand().get() }
    fn index(&self, field: Field) -> PointIndex { PointIndex(self.get(field) as u32) }
    fn coordinate(&self, axis: Axis, index: PointIndex) -> Result<GridCoordinate, T::Error> { self.storage.point(axis, index) }
    fn set_coordinate(&mut self, axis: Axis, index: PointIndex, value: GridCoordinate) -> Result<(), T::Error> { self.storage.set_point(axis, index, value) }
    fn indexed_point(&self, x: Field, y: Field) -> Result<Point, T::Error> {
        Ok(Point { x: self.coordinate(Axis::X, self.index(x))?, y: self.coordinate(Axis::Y, self.index(y))? })
    }
    fn save_point(&mut self, x: Field, y: Field, point: Point) { self.set(x, point.x.0); self.set(y, point.y.0); }
    fn source(&self) -> Result<Point, T::Error> { self.indexed_point(Field::SourceX, Field::SourceY) }
    fn destination(&self) -> Result<Point, T::Error> { self.indexed_point(Field::DestinationX, Field::DestinationY) }
    fn amount(&mut self, encoding: Encoding) -> Result<i32, T::Error> {
        match encoding {
            Encoding::Short => self.read_short(),
            Encoding::Byte => { let value = self.read_byte()?; Ok(if value == 255 { 256 } else { i32::from(value) }) }
        }
    }
    fn read_point(&mut self, encoding: Encoding, basis: Basis) -> Result<Point, T::Error> {
        let first = self.amount(encoding)?;
        let second = self.amount(encoding)?;
        let ex = self.get(Field::ExtentX);
        let ey = self.get(Field::ExtentY);
        let (dx, dy) = match basis {
            Basis::Axis => (ex.wrapping_mul(first), ey.wrapping_mul(second)),
            Basis::Rotated => (ex.wrapping_mul(first).wrapping_sub(ey.wrapping_mul(second)), ey.wrapping_mul(first).wrapping_add(ex.wrapping_mul(second))),
        };
        Ok(Point { x: GridCoordinate(self.get(Field::FirstX).wrapping_add(div256(dx))), y: GridCoordinate(self.get(Field::FirstY).wrapping_add(div256(dy))) })
    }
    fn update_point(&mut self, field: Field, point: Point) -> Result<(), T::Error> {
        let index = self.index(field);
        self.set_coordinate(Axis::X, index, point.x)?;
        self.set_coordinate(Axis::Y, index, point.y)
    }
    fn capture_basis(&mut self, crossed: bool) -> Result<(), T::Error> {
        let first = self.indexed_point(Field::SourceX, if crossed { Field::DestinationY } else { Field::SourceY })?;
        let second = self.indexed_point(Field::DestinationX, if crossed { Field::SourceY } else { Field::DestinationY })?;
        self.set(Field::ExtentX, second.x.0.wrapping_sub(first.x.0));
        self.set(Field::ExtentY, second.y.0.wrapping_sub(first.y.0));
        self.save_point(Field::FirstX, Field::FirstY, first);
        self.save_point(Field::SecondX, Field::SecondY, second);
        Ok(())
    }
    fn append_finish(&mut self) -> Result<(), T::Error> {
        if self.get(Field::Mode) == 0 {
            let shift = self.get(Field::Shift);
            let left = self.get(Field::LeftCursor);
            let right = self.get(Field::RightCursor);
            let left_value = asr(self.get(Field::FinishX), shift) as i16;
            let right_value = asr(self.get(Field::FinishY), shift) as i16;
            self.storage.set_output_half(Side::Minimum, OutputOffset(left), left_value)?;
            self.storage.set_output_half(Side::Maximum, OutputOffset(right), right_value)?;
            self.set(Field::LeftCursor, left.wrapping_add(2));
            self.set(Field::RightCursor, right.wrapping_add(2));
        }
        Ok(())
    }
}

pub fn set_scale<T: Storage>(storage: &mut T, index: ScaleIndex, value: ScaleValue, scale: Scale) -> Result<(), T::Error> {
    let shifted = lsl(value.0, storage.state().shift().0);
    let (x, y) = match scale {
        Scale::Small => (div256(storage.state().scale_y().0.wrapping_mul(shifted).wrapping_add(128)), div256(storage.state().scale_x().0.wrapping_mul(shifted).wrapping_add(128))),
        Scale::Large => (shifted, shifted),
    };
    storage.set_coefficient(CoefficientBank::Values, index, value)?;
    storage.set_coefficient(CoefficientBank::X, index, ScaleValue(x))?;
    storage.set_coefficient(CoefficientBank::Y, index, ScaleValue(y))
}

pub fn refresh_scales<T: Storage>(storage: &mut T) -> Result<(), T::Error> {
    let shift = storage.state().shift().0;
    if storage.state().small_scale() {
        let scale_x = storage.state().scale_x().0;
        let scale_y = storage.state().scale_y().0;
        for index in 0..8 {
            let value = lsl(storage.coefficient(CoefficientBank::Values, ScaleIndex(index))?.0, shift);
            storage.set_coefficient(CoefficientBank::X, ScaleIndex(index), ScaleValue(div256(scale_y.wrapping_mul(value))))?;
            storage.set_coefficient(CoefficientBank::Y, ScaleIndex(index), ScaleValue(div256(scale_x.wrapping_mul(value))))?;
        }
    } else {
        for index in 0..8 {
            let value = ScaleValue(lsl(storage.coefficient(CoefficientBank::Values, ScaleIndex(index))?.0, shift));
            storage.set_coefficient(CoefficientBank::X, ScaleIndex(index), value)?;
            storage.set_coefficient(CoefficientBank::Y, ScaleIndex(index), value)?;
        }
    }
    Ok(())
}

fn curve<T: Storage>(vm: &mut Vm<'_, T>, encoding: Encoding, basis: Basis, end: CurveEnd, side_opcode: u8) -> Result<InstructionSignal, T::Error> {
    vm.set(Field::EdgeSide, i32::from(vm.operand() == side_opcode));
    let mut remaining = match encoding { Encoding::Short => vm.read_short()?, Encoding::Byte => i32::from(vm.read_byte()?) };
    if matches!(end, CurveEnd::ReadStart) {
        let point = vm.read_point(encoding, basis)?;
        vm.update_point(Field::SourceX, point)?;
        remaining = remaining.wrapping_sub(1);
    }
    vm.save_point(Field::StartX, Field::StartY, vm.source()?);
    vm.set(Field::HasControl, 1);
    loop {
        remaining = remaining.wrapping_sub(2);
        let control = vm.read_point(encoding, basis)?;
        vm.save_point(Field::ControlX, Field::ControlY, control);
        if matches!(end, CurveEnd::Stored) && remaining <= 0 { break; }
        let point = vm.read_point(encoding, basis)?;
        vm.save_point(Field::EndX, Field::EndY, point);
        vm.storage.draw_curve()?;
        vm.save_point(Field::StartX, Field::StartY, point);
        vm.save_point(Field::EndX, Field::EndY, point);
        if !matches!(end, CurveEnd::Stored) && remaining <= 0 {
            vm.set_coordinate(Axis::X, vm.index(Field::DestinationX), point.x)?;
            vm.set_coordinate(Axis::Y, vm.index(Field::DestinationY), point.y)?;
            return Ok(InstructionSignal::Continue);
        }
    }
    vm.save_point(Field::EndX, Field::EndY, vm.destination()?);
    vm.storage.draw_curve()?;
    Ok(InstructionSignal::Continue)
}

fn line<T: Storage>(vm: &mut Vm<'_, T>, encoding: Encoding, basis: Basis, segment: bool, side_opcode: u8, same_y_index: bool) -> Result<InstructionSignal, T::Error> {
    vm.set(Field::EdgeSide, i32::from(vm.operand() == side_opcode));
    let start = if segment {
        let point = vm.read_point(encoding, basis)?;
        vm.update_point(Field::SourceX, point)?;
        let start = vm.source()?;
        let point = vm.read_point(encoding, basis)?;
        vm.update_point(Field::DestinationX, point)?;
        start
    } else {
        if side_opcode != 3 { let point = vm.read_point(encoding, basis)?; vm.update_point(Field::DestinationX, point)?; }
        vm.source()?
    };
    let end = vm.indexed_point(Field::DestinationX, if same_y_index { Field::DestinationX } else { Field::DestinationY })?;
    vm.save_point(Field::StartX, Field::StartY, start);
    vm.save_point(Field::ControlX, Field::ControlY, start);
    vm.save_point(Field::EndX, Field::EndY, end);
    vm.set(Field::HasControl, 0);
    vm.storage.draw_curve()?;
    Ok(InstructionSignal::Continue)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Instruction {
    RotatedSegment, RotatedLineTo, StoredLine, ShortRotatedStart, RotatedCurveTo, RotatedCurve,
    ShortSegment, ShortLineTo, ShortAxisStart, ShortCurveTo, ShortCurve,
    ScaleAddX, ScaleSubtractX, CaptureBounds, CaptureCrossedBounds, CopyX, CopyY, RotateShort, TranslateShort, NoOp,
    ScaleAddY, ScaleSubtractY, ScaledAddX, ScaledSubtractX, ScaledAddY, ScaledSubtractY,
    Finish4, Finish3, ByteRotatedCloseNormal, ByteAxisCloseNormal, ByteRotatedCloseCross, ByteAxisCloseCross,
    ByteRotatedSegment, ByteRotatedLineTo, ByteRotatedStart, RotatedByteCurveTo, ByteRotatedClose,
    ByteSegment, ByteLineTo, ByteAxisStart, ByteCurveTo, ByteCurve,
    AddCrossX, SubtractX, RotateByte, TranslateByte, AddY, SubtractY,
    AddScaledX, SubtractScaledX, AddScaledY, SubtractScaledY, FinishCurve,
}
impl Instruction {
    pub fn decode(opcode: Opcode) -> Option<Self> {
        Some(match opcode.get() {
            0x00 | 0x04 => Self::RotatedSegment, 0x02 | 0x06 => Self::RotatedLineTo, 0x03 | 0x07 => Self::StoredLine,
            0x08 | 0x0c => Self::ShortRotatedStart, 0x0a | 0x0e => Self::RotatedCurveTo, 0x0b | 0x0f => Self::RotatedCurve,
            0x10 | 0x14 => Self::ShortSegment, 0x12 | 0x16 => Self::ShortLineTo, 0x18 | 0x1c => Self::ShortAxisStart,
            0x1a | 0x1e => Self::ShortCurveTo, 0x1b | 0x1f => Self::ShortCurve,
            0x20..=0x27 => Self::ScaleAddX, 0x28..=0x2f => Self::ScaleSubtractX,
            0x30 => Self::CaptureBounds, 0x31 => Self::CaptureCrossedBounds, 0x32 => Self::CopyX, 0x33 => Self::CopyY,
            0x34 => Self::RotateShort, 0x35 => Self::TranslateShort, 0x36 | 0x37 => Self::NoOp,
            0x40..=0x47 => Self::ScaleAddY, 0x48..=0x4f => Self::ScaleSubtractY,
            0x50..=0x57 => Self::ScaledAddX, 0x58..=0x5f => Self::ScaledSubtractX,
            0x60..=0x67 => Self::ScaledAddY, 0x68..=0x6f => Self::ScaledSubtractY,
            0x70 => Self::Finish4, 0x71 => Self::Finish3,
            0x78 | 0x79 => Self::ByteRotatedCloseNormal, 0x7a | 0x7b => Self::ByteAxisCloseNormal,
            0x7c | 0x7d => Self::ByteRotatedCloseCross, 0x7e | 0x7f => Self::ByteAxisCloseCross,
            0x80 | 0x84 => Self::ByteRotatedSegment, 0x82 | 0x86 => Self::ByteRotatedLineTo,
            0x88 | 0x8c => Self::ByteRotatedStart, 0x8a | 0x8e => Self::RotatedByteCurveTo, 0x8b | 0x8f => Self::ByteRotatedClose,
            0x90 | 0x94 => Self::ByteSegment, 0x92 | 0x96 => Self::ByteLineTo, 0x98 | 0x9c => Self::ByteAxisStart,
            0x9a | 0x9e => Self::ByteCurveTo, 0x9b | 0x9f => Self::ByteCurve,
            0xa0..=0xa7 => Self::AddCrossX, 0xa8..=0xaf => Self::SubtractX,
            0xb4 => Self::RotateByte, 0xb5 => Self::TranslateByte,
            0xc0..=0xc7 => Self::AddY, 0xc8..=0xcf => Self::SubtractY,
            0xd0..=0xd7 => Self::AddScaledX, 0xd8..=0xdf => Self::SubtractScaledX,
            0xe0..=0xe7 => Self::AddScaledY, 0xe8..=0xef => Self::SubtractScaledY,
            0xf0 => Self::FinishCurve, _ => return None,
        })
    }
}

pub fn instruction<T: Storage>(storage: &mut T, instruction: Instruction) -> Result<InstructionSignal, T::Error> {
    execute_instruction(&mut Vm { storage }, instruction)
}
fn execute_instruction<T: Storage>(vm: &mut Vm<'_, T>, instruction: Instruction) -> Result<InstructionSignal, T::Error> {
    use Instruction::*;
    match instruction {
        RotatedSegment => return line(vm, Encoding::Short, Basis::Rotated, true, 0x00, false),
        RotatedLineTo => return line(vm, Encoding::Short, Basis::Rotated, false, 0x02, false),
        StoredLine => return line(vm, Encoding::Short, Basis::Axis, false, 0x03, false),
        ShortSegment => return line(vm, Encoding::Short, Basis::Axis, true, 0x10, false),
        ShortLineTo => return line(vm, Encoding::Short, Basis::Axis, false, 0x12, true),
        ByteRotatedSegment => return line(vm, Encoding::Byte, Basis::Rotated, true, 0x80, false),
        ByteRotatedLineTo => return line(vm, Encoding::Byte, Basis::Rotated, false, 0x82, false),
        ByteSegment => return line(vm, Encoding::Byte, Basis::Axis, true, 0x90, false),
        ByteLineTo => return line(vm, Encoding::Byte, Basis::Axis, false, 0x92, true),
        ShortCurve => return curve(vm, Encoding::Short, Basis::Axis, CurveEnd::Stored, 0x1b),
        ShortCurveTo => return curve(vm, Encoding::Short, Basis::Axis, CurveEnd::Read, 0x1a),
        RotatedCurve => return curve(vm, Encoding::Short, Basis::Rotated, CurveEnd::Stored, 0x0b),
        RotatedCurveTo => return curve(vm, Encoding::Short, Basis::Rotated, CurveEnd::Read, 0x0a),
        ShortAxisStart => return curve(vm, Encoding::Short, Basis::Axis, CurveEnd::ReadStart, 0x18),
        ShortRotatedStart => return curve(vm, Encoding::Short, Basis::Rotated, CurveEnd::ReadStart, 0x08),
        ByteCurve => return curve(vm, Encoding::Byte, Basis::Axis, CurveEnd::Stored, 0x9b),
        ByteCurveTo => return curve(vm, Encoding::Byte, Basis::Axis, CurveEnd::Read, 0x9a),
        ByteRotatedClose => return curve(vm, Encoding::Byte, Basis::Rotated, CurveEnd::Stored, 0x8b),
        RotatedByteCurveTo => return curve(vm, Encoding::Byte, Basis::Rotated, CurveEnd::Read, 0x8a),
        ByteAxisStart => return curve(vm, Encoding::Byte, Basis::Axis, CurveEnd::ReadStart, 0x98),
        ByteRotatedStart => return curve(vm, Encoding::Byte, Basis::Rotated, CurveEnd::ReadStart, 0x88),
        ByteAxisCloseNormal | ByteAxisCloseCross | ByteRotatedCloseNormal | ByteRotatedCloseCross => {
            let (basis, crossed, side) = match instruction {
                ByteAxisCloseNormal => (Basis::Axis, false, 0x7a), ByteAxisCloseCross => (Basis::Axis, true, 0x7e),
                ByteRotatedCloseNormal => (Basis::Rotated, false, 0x78), _ => (Basis::Rotated, true, 0x7c),
            };
            vm.capture_basis(crossed)?;
            return curve(vm, Encoding::Byte, basis, CurveEnd::Stored, side);
        }
        CaptureBounds | CaptureCrossedBounds => vm.capture_basis(matches!(instruction, CaptureCrossedBounds))?,
        CopyX | CopyY => {
            let (axis, source, destination) = if matches!(instruction, CopyX) { (Axis::X, Field::SourceX, Field::DestinationX) } else { (Axis::Y, Field::SourceY, Field::DestinationY) };
            vm.set_coordinate(axis, vm.index(destination), vm.coordinate(axis, vm.index(source))?)?;
        }
        RotateShort | TranslateShort | RotateByte | TranslateByte => {
            let encoding = if matches!(instruction, RotateShort | TranslateShort) { Encoding::Short } else { Encoding::Byte };
            let basis = if matches!(instruction, RotateShort | RotateByte) { Basis::Rotated } else { Basis::Axis };
            let point = vm.read_point(encoding, basis)?;
            vm.update_point(Field::SourceX, point)?;
        }
        Finish3 | Finish4 => { vm.append_finish()?; return Ok(if matches!(instruction, Finish3) { InstructionSignal::Finished } else { InstructionSignal::Flush }); }
        FinishCurve => {
            vm.set(Field::Remaining, vm.get(Field::Remaining).wrapping_add(1));
            vm.set(Field::StartX, -1);
            vm.set(Field::Cursor, vm.get(Field::Cursor).wrapping_sub(1));
            vm.storage.draw_curve()?;
            return Ok(InstructionSignal::Flush);
        }
        NoOp => (),
        _ => {
            let opcode = u32::from(vm.operand());
            let (axis, bank, first_opcode, subtract, read_count, scaled) = match instruction {
                ScaleAddX => (Axis::X, CoefficientBank::X, 0x20, false, 2, true),
                ScaleSubtractX => (Axis::X, CoefficientBank::X, 0x28, true, 2, true),
                ScaleAddY => (Axis::Y, CoefficientBank::Y, 0x40, false, 2, true),
                ScaleSubtractY => (Axis::Y, CoefficientBank::Y, 0x48, true, 2, true),
                ScaledAddX => (Axis::X, CoefficientBank::X, 0x50, false, 1, true),
                ScaledSubtractX => (Axis::X, CoefficientBank::X, 0x58, true, 1, true),
                ScaledAddY => (Axis::Y, CoefficientBank::Y, 0x60, false, 1, true),
                ScaledSubtractY => (Axis::Y, CoefficientBank::Y, 0x68, true, 1, true),
                AddCrossX => (Axis::X, CoefficientBank::Y, 0xa0, false, 1, false),
                SubtractX => (Axis::X, CoefficientBank::X, 0xa8, true, 1, false),
                AddY => (Axis::Y, CoefficientBank::Y, 0xc0, false, 1, false),
                SubtractY => (Axis::Y, CoefficientBank::Y, 0xc8, true, 1, false),
                AddScaledX => (Axis::X, CoefficientBank::X, 0xd0, false, 0, false),
                SubtractScaledX => (Axis::X, CoefficientBank::X, 0xd8, true, 0, false),
                AddScaledY => (Axis::Y, CoefficientBank::Y, 0xe0, false, 0, false),
                SubtractScaledY => (Axis::Y, CoefficientBank::Y, 0xe8, true, 0, false),
                _ => unreachable!(),
            };
            let first = if read_count != 0 { vm.read_short()? } else { 0 };
            if read_count == 2 { vm.read_short()?; }
            let coefficient = vm.storage.coefficient(bank, ScaleIndex(opcode.wrapping_sub(first_opcode)))?.0;
            let amount = if scaled { div256(coefficient.wrapping_mul(first)) } else { coefficient };
            let (source, destination) = if matches!(axis, Axis::Y) && !matches!(instruction, ScaledAddY | ScaledSubtractY | AddScaledY | SubtractScaledY) {
                (Field::SourceY, Field::DestinationY)
            } else { (Field::SourceX, Field::DestinationX) };
            let origin = vm.coordinate(axis, vm.index(source))?;
            vm.set_coordinate(axis, vm.index(destination), GridCoordinate(if subtract { origin.0.wrapping_sub(amount) } else { origin.0.wrapping_add(amount) }))?;
        }
    }
    Ok(InstructionSignal::Continue)
}

fn reset_scan_rows<T: Storage>(vm: &mut Vm<'_, T>) -> Result<(), T::Error> {
    let height = vm.get(Field::Height);
    let width = vm.get(Field::Width);
    let left = vm.get(Field::LeftCursor);
    let right = vm.get(Field::RightCursor);
    let pair = RowPair::from_packed((width as u32) | (width as u32).wrapping_shl(16));
    for row in 0..height / 2 {
        let offset = row.wrapping_mul(4);
        vm.storage.set_output_pair(Side::Minimum, OutputOffset(left.wrapping_add(offset)), pair)?;
        vm.storage.set_output_pair(Side::Maximum, OutputOffset(right.wrapping_add(offset)), RowPair::default())?;
    }
    if height & 2 != 0 {
        let offset = height.wrapping_sub(1).wrapping_mul(2);
        vm.storage.set_output_half(Side::Minimum, OutputOffset(left.wrapping_add(offset)), width as i16)?;
        vm.storage.set_output_half(Side::Maximum, OutputOffset(right.wrapping_add(offset)), 0)?;
    }
    vm.set(Field::MinimumRow, height);
    vm.set(Field::LeftMaximumRow, 0);
    vm.set(Field::RightMaximumRow, 0);
    Ok(())
}

fn normalize_rows<T: Storage>(vm: &mut Vm<'_, T>) -> Result<(), T::Error> {
    for row in 1..vm.get(Field::RowCount) {
        let offset = row.wrapping_mul(2);
        let previous_right = vm.storage.output_half(Side::Maximum, OutputOffset(offset.wrapping_sub(2)))?;
        if vm.storage.output_half(Side::Minimum, OutputOffset(offset))? > previous_right {
            vm.storage.set_output_half(Side::Minimum, OutputOffset(offset), previous_right)?;
        } else {
            let previous_left = vm.storage.output_half(Side::Minimum, OutputOffset(offset.wrapping_sub(2)))?;
            if vm.storage.output_half(Side::Maximum, OutputOffset(offset))? < previous_left {
                vm.storage.set_output_half(Side::Maximum, OutputOffset(offset), previous_left)?;
            }
        }
    }
    Ok(())
}

pub fn execute<T: Storage>(storage: &mut T, mode: RasterMode) -> Result<ExecuteStatus, T::Error> {
    execute_vm(&mut Vm { storage }, mode)
}
fn execute_vm<T: Storage>(vm: &mut Vm<'_, T>, mode: RasterMode) -> Result<ExecuteStatus, T::Error> {
    vm.set(Field::Mode, mode.0 as i32);
    let multiplier = if vm.get(Field::Oversample) == 0 { 1 } else { 8 };
    vm.set(Field::ScaleX, vm.get(Field::Width).wrapping_mul(multiplier));
    vm.set(Field::ScaleY, vm.get(Field::Height).wrapping_mul(multiplier));
    vm.set(Field::Multiplier, multiplier);
    let maximum = vm.get(Field::ScaleX).max(vm.get(Field::ScaleY));
    if mode.0 == 2 {
        let shift = 8i32.wrapping_sub(maximum >> 12);
        vm.set(Field::ScanStep, lsl(1, shift.wrapping_sub(3)).max(2));
        vm.set(Field::ScanHalf, lsl(1, shift.wrapping_sub(1)));
        vm.set(Field::HalfShift, shift.wrapping_sub(1));
        vm.set(Field::Shift, shift);
        vm.set(Field::Unit, lsl(1, shift));
        vm.set(Field::Mask, lsl(1, shift).wrapping_sub(1));
    } else {
        let shift = if maximum <= 127 { 8 } else { 3 };
        vm.set(Field::Unit, lsl(1, shift));
        vm.set(Field::HalfUnit, lsl(1, shift - 1));
        vm.set(Field::Shift, shift);
        vm.set(Field::HalfShift, shift - 1);
        vm.set(Field::Mask, lsl(1, shift).wrapping_sub(1));
    }
    let scale = if vm.get(Field::ScaleX) <= 127 && vm.get(Field::ScaleY) <= 127 { Scale::Small } else { Scale::Large };
    vm.set(Field::SmallScale, i32::from(matches!(scale, Scale::Small)));
    vm.storage.select_scale(scale);
    if vm.get(Field::PointCount) == 0 {
        vm.set_coordinate(Axis::X, PointIndex(0), GridCoordinate(0))?;
        vm.set_coordinate(Axis::Y, PointIndex(0), GridCoordinate(0))?;
        let shift = vm.get(Field::Shift);
        let (x, y) = match scale {
            Scale::Small => (lsl(vm.get(Field::ScaleY), shift).wrapping_sub(1), lsl(vm.get(Field::ScaleX), shift).wrapping_sub(1)),
            Scale::Large => (!lsl(-256, shift), !lsl(-256, shift)),
        };
        vm.set_coordinate(Axis::X, PointIndex(1), GridCoordinate(x))?;
        vm.set_coordinate(Axis::Y, PointIndex(1), GridCoordinate(y))?;
        vm.set(Field::ExtentX, x);
        vm.set(Field::ExtentY, y);
        vm.set(Field::FirstX, 0);
        vm.set(Field::FirstY, 0);
        vm.set(Field::SecondX, x);
        vm.set(Field::SecondY, y);
    } else {
        for index in (0..8).rev() {
            let value = i32::from(vm.storage.initial_half(ScaleIndex(index))?);
            if value != 0 { set_scale(vm.storage, ScaleIndex(index), ScaleValue(value), scale)?; }
        }
        for index in 0..vm.get(Field::PointCount) {
            let shift = vm.get(Field::Shift);
            let point = vm.storage.initial_point(PointIndex(index as u32))?;
            let x = lsl(i32::from(point.x), shift);
            let y = lsl(i32::from(point.y), shift);
            let (x, y) = match scale {
                Scale::Small => (div256(vm.get(Field::ScaleY).wrapping_mul(x).wrapping_add(128)), div256(vm.get(Field::ScaleX).wrapping_mul(y).wrapping_add(128))),
                Scale::Large => (x, y),
            };
            vm.set_coordinate(Axis::X, PointIndex(index as u32), GridCoordinate(x))?;
            vm.set_coordinate(Axis::Y, PointIndex(index as u32), GridCoordinate(y))?;
        }
    }
    for index in 0..vm.get(Field::ValueCount) {
        let value = ScaleValue(i32::from(vm.storage.initial_value(ScaleIndex(index as u32))?));
        set_scale(vm.storage, ScaleIndex(index as u32), value, scale)?;
    }
    vm.set(Field::LeftCursor, 0);
    vm.set(Field::RightCursor, 0);
    if mode.0 == 0 {
        vm.set(Field::FirstRow, -1);
        vm.set(Field::LeftState, 0);
        vm.set(Field::RightState, 0);
    } else if mode.0 == 2 { reset_scan_rows(vm)?; }
    while vm.get(Field::Remaining) > 0 {
        let signal = loop {
            let opcode = Opcode::new(vm.read_byte()?);
            vm.storage.state_mut().set_operand(opcode);
            let indices = vm.read_byte()?;
            vm.set(Field::SourceX, i32::from(indices >> 4));
            vm.set(Field::DestinationX, i32::from(indices & 15));
            vm.set(Field::SourceY, i32::from(indices >> 4));
            vm.set(Field::DestinationY, i32::from(indices & 15));
            let Some(instruction) = Instruction::decode(opcode) else { return Ok(ExecuteStatus::InvalidOpcode); };
            match execute_instruction(vm, instruction)? {
                InstructionSignal::Continue => {
                    if vm.get(Field::Remaining) <= 0 { vm.append_finish()?; break InstructionSignal::Finished; }
                }
                signal => break signal,
            }
        };
        if mode.0 == 0 {
            let count = vm.storage.contour_rows()?;
            vm.storage.state_mut().set_row_count(count);
            if count.0 > 0 {
                normalize_rows(vm)?;
                if vm.storage.flush_scanlines()? == FlushStatus::RunCapacity { return Ok(ExecuteStatus::RunCapacity); }
            }
            vm.set(Field::FirstRow, -1);
            vm.set(Field::LeftState, 0);
            vm.set(Field::RightState, 0);
            vm.set(Field::LeftCursor, 0);
            vm.set(Field::RightCursor, 0);
            if matches!(signal, InstructionSignal::Finished) { return Ok(ExecuteStatus::Complete); }
        } else if mode.0 == 2 {
            let first = vm.get(Field::MinimumRow);
            let count = vm.get(Field::LeftMaximumRow).min(vm.get(Field::RightMaximumRow)).wrapping_sub(first).wrapping_add(1);
            vm.set(Field::RowCount, count);
            if count > 0 {
                vm.set(Field::FirstRow, first);
                normalize_rows(vm)?;
                if vm.storage.flush_scanlines()? == FlushStatus::RunCapacity { return Ok(ExecuteStatus::RunCapacity); }
                if matches!(signal, InstructionSignal::Finished) { return Ok(ExecuteStatus::Complete); }
                reset_scan_rows(vm)?;
            } else if matches!(signal, InstructionSignal::Finished) { return Ok(ExecuteStatus::Complete); }
        }
    }
    Ok(ExecuteStatus::Complete)
}
