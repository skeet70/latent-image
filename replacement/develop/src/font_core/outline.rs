use super::{AdvancePerMille, AssetOffset, FontError, FontPoint, FontUnit, GlyphAdvance, GlyphId, GlyphOutline, Opcode, OutlineCommand, OutlineMetrics, WritingMode};

#[derive(Clone, Copy)]
pub(crate) struct PointIndex(u8);
impl PointIndex { pub(crate) fn get(self) -> usize { self.0 as usize } }
#[derive(Clone, Copy)]
pub(crate) enum Axis { X, Y }
#[derive(Clone, Copy)]
pub(crate) enum ParameterBank { Low, High }
#[derive(Clone, Copy)]
pub(crate) enum Scalar {
    Register(Axis, PointIndex), Parameter(ParameterBank, u8),
    FirstX, FirstY, SecondX, SecondY, ExtentX, ExtentY,
    StartX, StartY, ControlX, ControlY, EndX, EndY,
}
#[derive(Clone, Copy)]
pub(crate) enum Contour { Forward, Reverse }
impl Contour { pub(crate) fn index(self) -> usize { match self { Self::Forward => 0, Self::Reverse => 1 } } }
#[derive(Clone, Copy)]
pub(crate) struct Operands { pub(crate) source: PointIndex, pub(crate) destination: PointIndex }
#[derive(Clone, Copy)]
pub(crate) struct Pair { pub(crate) x: i32, pub(crate) y: i32 }
#[derive(Clone, Copy)]
pub(crate) struct Segment { pub(crate) start: FontPoint, pub(crate) control: Option<FontPoint>, pub(crate) end: FontPoint }
const ZERO_POINT: FontPoint = FontPoint { x: FontUnit::new(0), y: FontUnit::new(0) };
const ZERO_SEGMENT: Segment = Segment { start: ZERO_POINT, control: None, end: ZERO_POINT };

pub(crate) trait VmState {
    fn read(&self, field: Scalar) -> i32;
    fn write(&mut self, field: Scalar, value: i32);
    fn scale(&self) -> i32;
    fn set_scale(&mut self, value: i32);
    fn offsets(&self) -> Pair;
    fn origins(&self) -> Pair;
    fn point_count(&self) -> usize;
    fn point_header_present(&self) -> bool { self.point_count() != 0 }
    fn initial_point(&self, index: usize) -> Pair;
    fn parameter_count(&self) -> usize;
    fn initial_parameter(&self, index: usize) -> u8;
    fn hint(&self, index: usize) -> u16;
    fn set_parameter(&mut self, index: usize, value: i32);
    fn set_opcode(&mut self, opcode: Opcode);
    fn set_operands(&mut self, operands: Operands);
    fn has_control(&self) -> bool;
    fn set_control(&mut self, value: bool);
    fn contour(&self) -> Contour;
    fn set_contour(&mut self, contour: Contour);
}
pub(crate) trait ProgramReader {
    fn byte(&mut self) -> Result<u8, FontError>;
    fn remaining(&self) -> i32;
    fn rewind_sentinel(&mut self) -> Result<(), FontError>;
    fn short(&mut self) -> Result<i16, FontError> { let low = self.byte()? as u16; Ok((low | (self.byte()? as u16) << 8) as i16) }
}
pub(crate) trait ContourStorage {
    fn push(&mut self, contour: Contour, segment: Segment) -> Result<(), FontError>;
    fn len(&self, contour: Contour) -> usize;
    fn segment(&self, contour: Contour, index: usize) -> Segment;
    fn reset(&mut self);
}
pub(crate) trait OutlineOutput {
    fn begin(&mut self) -> Result<(), FontError>;
    fn command(&mut self, command: OutlineCommand) -> Result<(), FontError>;
    fn finish(&mut self) -> Result<(), FontError>;
}

#[derive(Clone, Copy)]
enum Width { Byte, Short }
#[derive(Clone, Copy)]
enum Basis { Axis, Rotated }
#[derive(Clone, Copy)]
enum End { Store, Close }
#[derive(Clone, Copy)]
enum Instruction {
    Line { width: Width, basis: Basis, start: bool, registers: bool, contour: Contour },
    Curve { width: Width, basis: Basis, start: bool, end: End, contour: Contour },
    BasisCurve { basis: Basis, cross: bool, contour: Contour },
    Basis(bool), Copy(Axis), Translate(Width, Basis), Reserved, Finish(bool),
    Arithmetic { bank: ParameterBank, axis: Axis, subtract: bool, scale: bool, discard: bool },
    Sentinel,
}
impl Instruction {
    fn decode(opcode: Opcode) -> Result<Self, FontError> {
        use Basis::{Axis as Straight, Rotated};
        use Width::{Byte, Short};
        let op = opcode.get();
        let contour = |reverse| if reverse { Contour::Reverse } else { Contour::Forward };
        let line = |width, basis, start, registers, reverse| Self::Line { width, basis, start, registers, contour: contour(reverse) };
        let curve = |width, basis, start, end, reverse| Self::Curve { width, basis, start, end, contour: contour(reverse) };
        let arithmetic = |bank, axis, subtract, scale, discard| Self::Arithmetic { bank, axis, subtract, scale, discard };
        Ok(match op {
            0x00 | 0x04 => line(Short, Rotated, true, false, op == 0),
            0x02 | 0x06 => line(Short, Rotated, false, false, op == 2),
            0x03 | 0x07 => line(Short, Straight, false, true, op == 3),
            0x08 | 0x0c => curve(Short, Rotated, true, End::Store, op == 8),
            0x0a | 0x0e => curve(Short, Rotated, false, End::Store, op == 10),
            0x0b | 0x0f => curve(Short, Rotated, false, End::Close, op == 11),
            0x10 | 0x14 => line(Short, Straight, true, false, op == 16),
            0x12 | 0x16 => line(Short, Straight, false, false, op == 18),
            0x18 | 0x1c => curve(Short, Straight, true, End::Store, op == 24),
            0x1a | 0x1e => curve(Short, Straight, false, End::Store, op == 26),
            0x1b | 0x1f => curve(Short, Straight, false, End::Close, op == 27),
            0x20..=0x27 => arithmetic(ParameterBank::Low, Axis::X, false, true, true),
            0x28..=0x2f => arithmetic(ParameterBank::Low, Axis::X, true, true, true),
            0x30 => Self::Basis(false), 0x31 => Self::Basis(true),
            0x32 => Self::Copy(Axis::X), 0x33 => Self::Copy(Axis::Y),
            0x34 => Self::Translate(Short, Rotated), 0x35 => Self::Translate(Short, Straight),
            0x36 | 0x37 => Self::Reserved,
            0x40..=0x47 => arithmetic(ParameterBank::High, Axis::Y, false, true, true),
            0x48..=0x4f => arithmetic(ParameterBank::High, Axis::Y, true, true, true),
            0x50..=0x57 => arithmetic(ParameterBank::Low, Axis::X, false, true, false),
            0x58..=0x5f => arithmetic(ParameterBank::Low, Axis::X, true, true, false),
            0x60..=0x67 => arithmetic(ParameterBank::High, Axis::Y, false, true, false),
            0x68..=0x6f => arithmetic(ParameterBank::High, Axis::Y, true, true, false),
            0x70 => Self::Finish(false), 0x71 => Self::Finish(true),
            0x78..=0x7f => Self::BasisCurve { basis: if op & 2 == 0 { Rotated } else { Straight }, cross: op & 4 != 0, contour: contour(op & 1 == 0) },
            0x80 | 0x84 => line(Byte, Rotated, true, false, op == 128),
            0x82 | 0x86 => line(Byte, Rotated, false, false, op == 130),
            0x88 | 0x8c => curve(Byte, Rotated, true, End::Store, op == 136),
            0x8a | 0x8e => curve(Byte, Rotated, false, End::Store, op == 138),
            0x8b | 0x8f => curve(Byte, Rotated, false, End::Close, op == 139),
            0x90 | 0x94 => line(Byte, Straight, true, false, op == 144),
            0x92 | 0x96 => line(Byte, Straight, false, false, op == 146),
            0x98 | 0x9c => curve(Byte, Straight, true, End::Store, op == 152),
            0x9a | 0x9e => curve(Byte, Straight, false, End::Store, op == 154),
            0x9b | 0x9f => curve(Byte, Straight, false, End::Close, op == 155),
            0xa0..=0xa7 => arithmetic(ParameterBank::High, Axis::X, false, false, true),
            0xa8..=0xaf => arithmetic(ParameterBank::Low, Axis::X, true, false, true),
            0xb4 => Self::Translate(Byte, Rotated), 0xb5 => Self::Translate(Byte, Straight),
            0xc0..=0xc7 => arithmetic(ParameterBank::High, Axis::Y, false, false, true),
            0xc8..=0xcf => arithmetic(ParameterBank::High, Axis::Y, true, false, true),
            0xd0..=0xd7 => arithmetic(ParameterBank::Low, Axis::X, false, false, false),
            0xd8..=0xdf => arithmetic(ParameterBank::Low, Axis::X, true, false, false),
            0xe0..=0xe7 => arithmetic(ParameterBank::High, Axis::Y, false, false, false),
            0xe8..=0xef => arithmetic(ParameterBank::High, Axis::Y, true, false, false),
            0xf0 => Self::Sentinel,
            _ => return Err(FontError::InvalidInstruction(opcode)),
        })
    }
}

pub(crate) fn truncate_256(value: i32) -> i32 { if value < 0 { value.wrapping_add(255) >> 8 } else { value >> 8 } }
fn initialize(state: &mut impl VmState) {
    state.set_scale(8);
    if !state.point_header_present() {
        for field in [Scalar::Register(Axis::X, PointIndex(0)), Scalar::Register(Axis::Y, PointIndex(0)), Scalar::FirstX, Scalar::FirstY] { state.write(field, 0); }
        for field in [Scalar::Register(Axis::X, PointIndex(1)), Scalar::Register(Axis::Y, PointIndex(1)), Scalar::SecondX, Scalar::SecondY, Scalar::ExtentX, Scalar::ExtentY] { state.write(field, 2047); }
    } else {
        for index in (0..8).rev() {
            let value = state.hint(index) as i32;
            if value != 0 {
                state.set_parameter(index, value);
                state.write(Scalar::Parameter(ParameterBank::Low, index as u8), value.wrapping_mul(8));
                state.write(Scalar::Parameter(ParameterBank::High, index as u8), value.wrapping_mul(8));
            }
        }
        for index in 0..state.point_count() {
            let point = state.initial_point(index);
            store(state, PointIndex(index as u8), Pair { x: point.x.wrapping_mul(8), y: point.y.wrapping_mul(8) });
        }
    }
    for index in 0..state.parameter_count() {
        let value = state.initial_parameter(index) as i32;
        state.set_parameter(index, value);
        state.write(Scalar::Parameter(ParameterBank::Low, index as u8), value.wrapping_mul(8));
        state.write(Scalar::Parameter(ParameterBank::High, index as u8), value.wrapping_mul(8));
    }
}
fn register(state: &impl VmState, index: PointIndex) -> Pair { Pair { x: state.read(Scalar::Register(Axis::X, index)), y: state.read(Scalar::Register(Axis::Y, index)) } }
fn store(state: &mut impl VmState, index: PointIndex, point: Pair) { state.write(Scalar::Register(Axis::X, index), point.x); state.write(Scalar::Register(Axis::Y, index), point.y); }
fn pair(state: &mut impl VmState, x: Scalar, y: Scalar, point: Pair) { state.write(x, point.x); state.write(y, point.y); }
fn basis(state: &mut impl VmState, operands: Operands, cross: bool) {
    let first = register(state, operands.source);
    let second = register(state, operands.destination);
    let (first_y, second_y) = if cross { (second.y, first.y) } else { (first.y, second.y) };
    pair(state, Scalar::FirstX, Scalar::FirstY, Pair { x: first.x, y: first_y });
    pair(state, Scalar::SecondX, Scalar::SecondY, Pair { x: second.x, y: second_y });
    pair(state, Scalar::ExtentX, Scalar::ExtentY, Pair { x: second.x.wrapping_sub(first.x), y: second_y.wrapping_sub(first_y) });
}
fn coordinate(reader: &mut impl ProgramReader, width: Width) -> Result<i32, FontError> { match width { Width::Short => Ok(reader.short()? as i32), Width::Byte => { let value = reader.byte()?; Ok(if value == 255 { 256 } else { value as i32 }) } } }
fn count(reader: &mut impl ProgramReader, width: Width) -> Result<i32, FontError> { match width { Width::Byte => Ok(reader.byte()? as i32), Width::Short => Ok(reader.short()? as i32) } }
fn point(state: &impl VmState, reader: &mut impl ProgramReader, width: Width, basis: Basis) -> Result<Pair, FontError> {
    let x = coordinate(reader, width)?;
    let y = coordinate(reader, width)?;
    let dx = state.read(Scalar::ExtentX);
    let dy = state.read(Scalar::ExtentY);
    let (x, y) = match basis { Basis::Axis => (dx.wrapping_mul(x), dy.wrapping_mul(y)), Basis::Rotated => (dx.wrapping_mul(x).wrapping_sub(dy.wrapping_mul(y)), dx.wrapping_mul(y).wrapping_add(dy.wrapping_mul(x))) };
    Ok(Pair { x: state.read(Scalar::FirstX).wrapping_add(truncate_256(x)), y: state.read(Scalar::FirstY).wrapping_add(truncate_256(y)) })
}
fn emit_current(state: &impl VmState, contours: &mut impl ContourStorage) -> Result<(), FontError> {
    let origin = state.origins();
    let output = |x, y| FontPoint { x: FontUnit::new((state.read(y) as i16).wrapping_sub(origin.y as i16) as i32), y: FontUnit::new((state.read(x) as i16).wrapping_sub(origin.x as i16) as i32) };
    contours.push(state.contour(), Segment { start: output(Scalar::StartX, Scalar::StartY), control: if state.has_control() { Some(output(Scalar::ControlX, Scalar::ControlY)) } else { None }, end: output(Scalar::EndX, Scalar::EndY) })
}
fn emit(state: &mut impl VmState, contours: &mut impl ContourStorage) -> Result<(), FontError> {
    let offset = state.offsets();
    let horizontal = offset.x.wrapping_mul(state.scale());
    let vertical = offset.y.wrapping_mul(state.scale());
    for field in [Scalar::StartX, Scalar::ControlX, Scalar::EndX] { state.write(field, state.read(field).wrapping_add(horizontal)); }
    for field in [Scalar::StartY, Scalar::ControlY, Scalar::EndY] { state.write(field, state.read(field).wrapping_add(vertical)); }
    emit_current(state, contours)
}
fn curve(state: &mut impl VmState, reader: &mut impl ProgramReader, contours: &mut impl ContourStorage, operands: Operands, width: Width, basis: Basis, start: bool, end: End, contour: Contour) -> Result<(), FontError> {
    state.set_contour(contour);
    let mut remaining = count(reader, width)?.wrapping_sub(start as i32);
    if start { let value = point(state, reader, width, basis)?; store(state, operands.source, value); }
    let source = register(state, operands.source);
    pair(state, Scalar::StartX, Scalar::StartY, source);
    state.set_control(true);
    loop {
        remaining = remaining.wrapping_sub(2);
        let control = point(state, reader, width, basis)?;
        pair(state, Scalar::ControlX, Scalar::ControlY, control);
        if matches!(end, End::Close) && remaining < 1 {
            let destination = register(state, operands.destination);
            pair(state, Scalar::EndX, Scalar::EndY, destination);
            return emit(state, contours);
        }
        let value = point(state, reader, width, basis)?;
        pair(state, Scalar::EndX, Scalar::EndY, value);
        emit(state, contours)?;
        pair(state, Scalar::StartX, Scalar::StartY, value);
        pair(state, Scalar::EndX, Scalar::EndY, value);
        if remaining <= 0 { store(state, operands.destination, value); return Ok(()); }
    }
}

pub(crate) fn finalize(contours: &impl ContourStorage, output: &mut impl OutlineOutput, writing: WritingMode) -> Result<(), FontError> {
    let forwards = contours.len(Contour::Forward);
    let reverses = contours.len(Contour::Reverse);
    if forwards == 0 || reverses == 0 { return Ok(()); }
    let orient = |point: FontPoint| match writing { WritingMode::Horizontal => point, WritingMode::Vertical => FontPoint { x: FontUnit::new(2048i16.wrapping_sub(point.y.get() as i16) as i32), y: point.x } };
    output.begin()?;
    for index in 0..forwards {
        let segment = contours.segment(Contour::Forward, index);
        if index == 0 { output.command(OutlineCommand::MoveTo(orient(segment.start)))?; }
        match segment.control {
            Some(control) if control.x.get() != -1 && control.y.get() != -1 => output.command(OutlineCommand::QuadraticTo { control: orient(control), end: orient(segment.end) })?,
            _ => output.command(OutlineCommand::LineTo(orient(segment.end)))?,
        }
    }
    for index in (0..reverses).rev() {
        let segment = contours.segment(Contour::Reverse, index);
        match segment.control {
            Some(control) if control.x.get() != -1 && control.y.get() != -1 => output.command(OutlineCommand::QuadraticTo { control: orient(control), end: orient(segment.start) })?,
            _ => output.command(OutlineCommand::LineTo(orient(segment.start)))?,
        }
    }
    output.command(OutlineCommand::Close)?;
    output.finish()
}

pub(crate) fn execute(state: &mut impl VmState, reader: &mut impl ProgramReader, contours: &mut impl ContourStorage, output: &mut impl OutlineOutput, writing: WritingMode) -> Result<(), FontError> {
    initialize(state);
    contours.reset();
    while reader.remaining() > 0 {
        let opcode = Opcode::new(reader.byte()?);
        state.set_opcode(opcode);
        let packed = reader.byte()?;
        let operands = Operands { source: PointIndex(packed >> 4), destination: PointIndex(packed & 15) };
        state.set_operands(operands);
        let instruction = Instruction::decode(opcode)?;
        let mut flush = false;
        let mut stop = false;
        match instruction {
            Instruction::Basis(cross) => basis(state, operands, cross),
            Instruction::Copy(axis) => { let value = state.read(Scalar::Register(axis, operands.source)); state.write(Scalar::Register(axis, operands.destination), value); }
            Instruction::Translate(width, basis) => { let value = point(state, reader, width, basis)?; store(state, operands.source, value); }
            Instruction::Reserved => (),
            Instruction::Finish(end) => { flush = true; stop = end; }
            Instruction::Arithmetic { bank, axis, subtract, scale, discard } => {
                let factor = if scale { reader.short()? as i32 } else { 1 };
                if discard { reader.short()?; }
                let mut amount = state.read(Scalar::Parameter(bank, opcode.get() & 7));
                if scale { amount = truncate_256(amount.wrapping_mul(factor)); }
                let old = state.read(Scalar::Register(axis, operands.source));
                state.write(Scalar::Register(axis, operands.destination), if subtract { old.wrapping_sub(amount) } else { old.wrapping_add(amount) });
            }
            Instruction::Line { width, basis, start, registers, contour } => {
                state.set_contour(contour);
                if start { let value = point(state, reader, width, basis)?; store(state, operands.source, value); }
                if !registers { let value = point(state, reader, width, basis)?; store(state, operands.destination, value); }
                let source = register(state, operands.source);
                pair(state, Scalar::StartX, Scalar::StartY, source);
                pair(state, Scalar::ControlX, Scalar::ControlY, source);
                let destination = register(state, operands.destination);
                pair(state, Scalar::EndX, Scalar::EndY, destination);
                state.set_control(false);
                emit(state, contours)?;
            }
            Instruction::BasisCurve { basis: shape, cross, contour } => { basis(state, operands, cross); curve(state, reader, contours, operands, Width::Byte, shape, false, End::Close, contour)?; }
            Instruction::Curve { width, basis, start, end, contour } => curve(state, reader, contours, operands, width, basis, start, end, contour)?,
            Instruction::Sentinel => {
                reader.rewind_sentinel()?;
                state.write(Scalar::StartX, -1);
                let offset = state.offsets();
                let vertical = state.scale().wrapping_mul(offset.y);
                for field in [Scalar::StartY, Scalar::ControlY, Scalar::EndY] { state.write(field, state.read(field).wrapping_add(vertical)); }
                if offset.x != 0 {
                    let horizontal = state.scale().wrapping_mul(offset.x);
                    state.write(Scalar::StartX, horizontal.wrapping_sub(1));
                    for field in [Scalar::ControlX, Scalar::EndX] { state.write(field, state.read(field).wrapping_add(horizontal)); }
                }
                emit_current(state, contours)?;
                contours.reset();
                continue;
            }
        }
        if flush || reader.remaining() < 1 {
            finalize(contours, output, writing)?;
            contours.reset();
            if stop { return Ok(()); }
        }
    }
    Ok(())
}

struct OwnedState {
    registers: [Pair; 16], parameters: [i32; 8], banks: [[i32; 8]; 2],
    basis: [i32; 6], segment: [i32; 6], scale: i32,
    inline_translation_scale: Pair,
    origins: Pair, offsets: Pair, point_bytes: [Pair; 16], point_count: usize,
    parameter_bytes: [u8; 8], parameter_count: usize, hints: [u16; 8],
    control: bool, contour: Contour,
}
impl OwnedState {
    const fn new() -> Self { Self { registers: [Pair { x: 0, y: 0 }; 16], parameters: [0; 8], banks: [[0; 8]; 2], basis: [0; 6], segment: [0; 6], scale: 0, inline_translation_scale: Pair { x: 0, y: 0 }, origins: Pair { x: 0, y: 0 }, offsets: Pair { x: 0, y: 0 }, point_bytes: [Pair { x: 0, y: 0 }; 16], point_count: 0, parameter_bytes: [0; 8], parameter_count: 0, hints: [0; 8], control: false, contour: Contour::Forward } }
}
impl VmState for OwnedState {
    fn read(&self, field: Scalar) -> i32 { match field {
        Scalar::Register(Axis::X, index) => self.registers[index.get()].x, Scalar::Register(Axis::Y, index) => self.registers[index.get()].y,
        Scalar::Parameter(bank, index) => self.banks[match bank { ParameterBank::Low => 0, ParameterBank::High => 1 }][index as usize],
        Scalar::FirstX => self.basis[0], Scalar::FirstY => self.basis[1], Scalar::SecondX => self.basis[2], Scalar::SecondY => self.basis[3], Scalar::ExtentX => self.basis[4], Scalar::ExtentY => self.basis[5],
        Scalar::StartX => self.segment[0], Scalar::StartY => self.segment[1], Scalar::ControlX => self.segment[2], Scalar::ControlY => self.segment[3], Scalar::EndX => self.segment[4], Scalar::EndY => self.segment[5],
    } }
    fn write(&mut self, field: Scalar, value: i32) { let slot = match field {
        Scalar::Register(Axis::X, index) => &mut self.registers[index.get()].x, Scalar::Register(Axis::Y, index) => &mut self.registers[index.get()].y,
        Scalar::Parameter(bank, index) => &mut self.banks[match bank { ParameterBank::Low => 0, ParameterBank::High => 1 }][index as usize],
        Scalar::FirstX => &mut self.basis[0], Scalar::FirstY => &mut self.basis[1], Scalar::SecondX => &mut self.basis[2], Scalar::SecondY => &mut self.basis[3], Scalar::ExtentX => &mut self.basis[4], Scalar::ExtentY => &mut self.basis[5],
        Scalar::StartX => &mut self.segment[0], Scalar::StartY => &mut self.segment[1], Scalar::ControlX => &mut self.segment[2], Scalar::ControlY => &mut self.segment[3], Scalar::EndX => &mut self.segment[4], Scalar::EndY => &mut self.segment[5],
    }; *slot = value; }
    fn scale(&self) -> i32 { self.scale }
    fn set_scale(&mut self, value: i32) { self.scale = value; }
    fn offsets(&self) -> Pair { self.offsets }
    fn origins(&self) -> Pair { self.origins }
    fn point_count(&self) -> usize { self.point_count }
    fn initial_point(&self, index: usize) -> Pair { self.point_bytes[index] }
    fn parameter_count(&self) -> usize { self.parameter_count }
    fn initial_parameter(&self, index: usize) -> u8 { self.parameter_bytes[index] }
    fn hint(&self, index: usize) -> u16 { self.hints[index] }
    fn set_parameter(&mut self, index: usize, value: i32) { self.parameters[index] = value; }
    fn set_opcode(&mut self, _: Opcode) {}
    fn set_operands(&mut self, _: Operands) {}
    fn has_control(&self) -> bool { self.control }
    fn set_control(&mut self, value: bool) { self.control = value; }
    fn contour(&self) -> Contour { self.contour }
    fn set_contour(&mut self, contour: Contour) { self.contour = contour; }
}

pub struct OutlineWorkspace { state: OwnedState, segments: [[Segment; 64]; 2], lengths: [usize; 2], frames: [RenderFrame; 64] }
impl OutlineWorkspace { pub const fn new() -> Self { Self { state: OwnedState::new(), segments: [[ZERO_SEGMENT; 64]; 2], lengths: [0; 2], frames: [RenderFrame { selected: Stream::Primary, transform: None }; 64] } } }
struct BorrowedContours<'a> { segments: &'a mut [[Segment; 64]; 2], lengths: &'a mut [usize; 2] }
impl ContourStorage for BorrowedContours<'_> {
    fn push(&mut self, contour: Contour, segment: Segment) -> Result<(), FontError> {
        let bank = contour.index(); let position = self.lengths[bank];
        let provided = self.segments[bank].len();
        let slot = self.segments[bank].get_mut(position).ok_or_else(|| FontError::workspace((position + 1) * core::mem::size_of::<Segment>(), provided * core::mem::size_of::<Segment>()))?;
        *slot = segment; self.lengths[bank] += 1; Ok(())
    }
    fn len(&self, contour: Contour) -> usize { self.lengths[contour.index()] }
    fn segment(&self, contour: Contour, index: usize) -> Segment { self.segments[contour.index()][index] }
    fn reset(&mut self) { *self.lengths = [0; 2]; }
}
struct BorrowedCommands<'a> { commands: &'a mut [OutlineCommand], used: usize }
impl OutlineOutput for BorrowedCommands<'_> {
    fn begin(&mut self) -> Result<(), FontError> { Ok(()) }
    fn command(&mut self, command: OutlineCommand) -> Result<(), FontError> {
        let provided = self.commands.len();
        let slot = self.commands.get_mut(self.used).ok_or_else(|| FontError::workspace((self.used + 1) * core::mem::size_of::<OutlineCommand>(), provided * core::mem::size_of::<OutlineCommand>()))?;
        *slot = command; self.used += 1; Ok(())
    }
    fn finish(&mut self) -> Result<(), FontError> { Ok(()) }
}

#[derive(Clone, Copy)]
struct Span { start: AssetOffset, end: AssetOffset }
impl Span {
    fn new(start: usize, length: usize, bytes: &[u8]) -> Result<Self, FontError> { let end = start.checked_add(length).ok_or(FontError::InvalidDirectory)?; if end > bytes.len() || end > u32::MAX as usize { return Err(FontError::Truncated); } Ok(Self { start: AssetOffset::new(start as u32), end: AssetOffset::new(end as u32) }) }
    fn len(self) -> usize { (self.end.get() - self.start.get()) as usize }
}
#[derive(Clone, Copy)]
struct Cursor { position: AssetOffset, end: AssetOffset }
impl Cursor {
    fn from_span(span: Span) -> Self { Self { position: span.start, end: span.end } }
    fn byte(&mut self, data: &[u8]) -> Result<u8, FontError> { if self.position.get() >= self.end.get() { return Err(FontError::Truncated); } let value = *data.get(self.position.get() as usize).ok_or(FontError::Truncated)?; self.position = AssetOffset::new(self.position.get() + 1); Ok(value) }
    fn short(&mut self, data: &[u8]) -> Result<u16, FontError> { let low = self.byte(data)? as u16; Ok(low | (self.byte(data)? as u16) << 8) }
    fn take(&mut self, data: &[u8], length: usize) -> Result<Span, FontError> { let span = Span::new(self.position.get() as usize, length, data)?; if span.end.get() > self.end.get() { return Err(FontError::Truncated); } self.position = span.end; Ok(span) }
    fn remaining(self) -> usize { (self.end.get() - self.position.get()) as usize }
}
struct BorrowedReader<'a> { bytes: &'a [u8], cursor: Cursor }
impl ProgramReader for BorrowedReader<'_> {
    fn byte(&mut self) -> Result<u8, FontError> { self.cursor.byte(self.bytes) }
    fn remaining(&self) -> i32 { self.cursor.remaining() as i32 }
    fn rewind_sentinel(&mut self) -> Result<(), FontError> { self.cursor.position = AssetOffset::new(self.cursor.position.get().checked_sub(1).ok_or(FontError::Truncated)?); Ok(()) }
}

#[derive(Clone, Copy)]
enum DirectoryFormat { Compact, Dense, Paged }
#[derive(Clone, Copy)]
struct Directory { entries: AssetOffset, data: AssetOffset, count: u32 }
pub struct FdlResource<'a> { bytes: &'a [u8], glyphs: Directory, format: DirectoryFormat, subroutines: Directory, compounds: Directory, mapping: AssetOffset, planes: AssetOffset, mapping_mode: u16, hints: [u16; 8], properties: Option<AssetOffset> }
fn little(bytes: &[u8], offset: usize, width: usize) -> Result<u32, FontError> { let input = bytes.get(offset..offset.checked_add(width).ok_or(FontError::InvalidDirectory)?).ok_or(FontError::Truncated)?; let mut value = 0; for (index, byte) in input.iter().enumerate() { value |= (*byte as u32) << (index * 8); } Ok(value) }
fn checked_offset(base: AssetOffset, delta: u32) -> Result<AssetOffset, FontError> { Ok(AssetOffset::new(base.get().checked_add(delta).ok_or(FontError::InvalidDirectory)?)) }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FdlFormat { Outline }
#[derive(Clone, Copy)]
pub struct ProgramSpan<'a> { bytes: &'a [u8], offset: AssetOffset }
impl<'a> ProgramSpan<'a> {
    pub const fn bytes(self) -> &'a [u8] { self.bytes }
    pub const fn offset(self) -> AssetOffset { self.offset }
    fn span(self) -> Span { Span { start: self.offset, end: AssetOffset::new(self.offset.get() + self.bytes.len() as u32) } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphProgramMetrics {
    pub horizontal_x: AdvancePerMille, pub horizontal_y: AdvancePerMille,
    pub vertical_x: AdvancePerMille, pub vertical_y: AdvancePerMille,
}
pub struct GlyphProgram<'a> { pub program: ProgramSpan<'a>, pub metrics: GlyphProgramMetrics }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScalePerMille(u16);
impl ScalePerMille { pub const fn get(self) -> u16 { self.0 } }
impl<'a> FdlResource<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, FontError> {
        if bytes.get(..8).ok_or(FontError::Truncated)? != b"FDL Font" { return Err(FontError::InvalidSignature); }
        if little(bytes, 0x36, 2)? != 4 { return Err(FontError::UnsupportedFormat); }
        let format = match little(bytes, 0x38, 2)? { 0 => DirectoryFormat::Compact, 0x100 => DirectoryFormat::Dense, 0x101 => DirectoryFormat::Paged, _ => return Err(FontError::UnsupportedFormat) };
        let entries = AssetOffset::new(little(bytes, 0x2e, 4)?);
        let mut glyphs = Directory { entries, data: AssetOffset::new(little(bytes, 0x32, 4)?), count: little(bytes, 0x1c, 2)? };
        let mapping = entries;
        let mut planes = entries;
        let mut mapping_mode = 0;
        if matches!(format, DirectoryFormat::Paged) {
            glyphs.count = little(bytes, entries.get() as usize, 4)?;
            glyphs.entries = checked_offset(entries, little(bytes, entries.get() as usize + 4, 4)?)?;
            mapping_mode = little(bytes, entries.get() as usize + 8, 2)? as u16;
            if mapping_mode > 1 { return Err(FontError::InvalidDirectory); }
            planes = checked_offset(entries, 12)?;
        } else if glyphs.count == 0 {
            glyphs.count = little(bytes, entries.get() as usize, 4)?;
            glyphs.entries = checked_offset(entries, 4)?;
        }
        let mut hints = [0; 8];
        for (index, hint) in hints.iter_mut().enumerate() { *hint = little(bytes, 8 + index * 2, 2)? as u16; }
        let subroutines = Directory { count: little(bytes, 0x18, 2)?, entries: AssetOffset::new(little(bytes, 0x1e, 4)?), data: AssetOffset::new(little(bytes, 0x22, 4)?) };
        let compounds = Directory { count: little(bytes, 0x1a, 2)?, entries: AssetOffset::new(little(bytes, 0x26, 4)?), data: AssetOffset::new(little(bytes, 0x2a, 4)?) };
        let property_offset = little(bytes, 0x3a, 4)?;
        let properties = if property_offset == 0 { None } else { Some(AssetOffset::new(property_offset)) };
        let resource = Self { bytes, glyphs, format, subroutines, compounds, mapping, planes, mapping_mode, hints, properties };
        if !matches!(format, DirectoryFormat::Paged) { resource.admit_entries(glyphs, if matches!(format, DirectoryFormat::Dense) { 4 } else { 3 })?; }
        resource.admit_entries(subroutines, 2)?;
        resource.admit_entries(compounds, 3)?;
        Ok(resource)
    }
    pub const fn bytes(&self) -> &'a [u8] { self.bytes }
    pub const fn default_values(&self) -> &[u16; 8] { &self.hints }
    pub const fn format(&self) -> FdlFormat { FdlFormat::Outline }
    fn program_span(&self, span: Span) -> ProgramSpan<'a> { ProgramSpan { bytes: &self.bytes[span.start.get() as usize..span.end.get() as usize], offset: span.start } }
    pub fn subroutine_directory(&self, glyph: GlyphId) -> Result<ProgramSpan<'a>, FontError> { Ok(self.program_span(self.record(self.subroutines, glyph.get(), 2)?)) }
    pub fn compound_directory(&self, glyph: GlyphId) -> Result<ProgramSpan<'a>, FontError> { Ok(self.program_span(self.record(self.compounds, glyph.get(), 3)?)) }
    pub fn property10(&self) -> Result<Option<ProgramSpan<'a>>, FontError> {
        let Some(offset) = self.properties else { return Ok(None); };
        let count = little(self.bytes, offset.get() as usize, 2)?;
        let mut cursor = checked_offset(offset, 2)?;
        for _ in 0..count {
            let identifier = little(self.bytes, cursor.get() as usize, 2)?;
            let length = little(self.bytes, cursor.get() as usize + 2, 2)? as usize;
            if length < 4 { return Err(FontError::InvalidDirectory); }
            let span = Span::new(cursor.get() as usize, length, self.bytes)?;
            if identifier == 10 { return Ok(Some(self.program_span(span))); }
            cursor = span.end;
        }
        Ok(None)
    }
    pub fn glyph_scale(&self, glyph: GlyphId) -> Result<Option<ScalePerMille>, FontError> {
        let Some(property) = self.property10()? else { return Ok(None); };
        let data = property.bytes();
        let first = little(data, 6, 2)?;
        let last = little(data, 8, 2)?;
        if glyph.get() < first || glyph.get() > last { return Ok(None); }
        let index = (glyph.get() - first) as usize;
        Ok(Some(ScalePerMille(little(data, 14 + index * 2, 2)? as u16)))
    }
    fn admit_entries(&self, directory: Directory, width: usize) -> Result<(), FontError> {
        if directory.count == 0 { return Ok(()); }
        let count = (directory.count as usize).checked_mul(width).and_then(|count| count.checked_add(width.min(3))).ok_or(FontError::InvalidDirectory)?;
        Span::new(directory.entries.get() as usize, count, self.bytes)?;
        if directory.data.get() as usize > self.bytes.len() { return Err(FontError::InvalidDirectory); }
        Ok(())
    }
    fn record(&self, directory: Directory, index: u32, width: usize) -> Result<Span, FontError> {
        if index >= directory.count { return Err(FontError::InvalidDirectory); }
        let entry = (directory.entries.get() as usize).checked_add((index as usize).checked_mul(width).ok_or(FontError::InvalidDirectory)?).ok_or(FontError::InvalidDirectory)?;
        let start = little(self.bytes, entry, width.min(3))?;
        let end = little(self.bytes, entry.checked_add(width).ok_or(FontError::InvalidDirectory)?, width.min(3))?;
        let length = end.checked_sub(start).ok_or(FontError::InvalidDirectory)?;
        let offset = checked_offset(directory.data, start)?;
        Span::new(offset.get() as usize, length as usize, self.bytes)
    }
    fn mapped_index(&self, glyph: GlyphId) -> Result<u32, FontError> {
        let plane = glyph.get() >> 16;
        let record = (self.planes.get() as usize).checked_add(plane as usize * 8).ok_or(FontError::InvalidDirectory)?;
        let mapping = checked_offset(self.mapping, little(self.bytes, record, 4)?)?;
        let base = little(self.bytes, record + 4, 4)?;
        let low = glyph.get() & 0xffff;
        let add = |value: u32| base.checked_add(value).ok_or(FontError::InvalidDirectory);
        match self.mapping_mode {
            0 => {
                let mut count = little(self.bytes, mapping.get() as usize, 2)?;
                let mut start = mapping.get() as usize + 2;
                while count != 0 {
                    let middle = count >> 1;
                    let entry = start.checked_add(middle as usize * 6).ok_or(FontError::InvalidDirectory)?;
                    let first = little(self.bytes, entry, 2)?;
                    let last = little(self.bytes, entry + 2, 2)?;
                    if first <= low && low <= last { return add(low - first + little(self.bytes, entry + 4, 2)?); }
                    if low < first { count = middle; } else { count -= middle + 1; start = entry + 6; }
                }
                Err(FontError::MissingGlyph(glyph))
            }
            1 => {
                let shift = little(self.bytes, mapping.get() as usize, 2)? & 255;
                let page = if shift < 32 { low >> shift } else { 0 };
                let mask = (if shift < 32 { 1u32 << shift } else { 0 }).wrapping_sub(1) & 0xffff;
                let slot = checked_offset(mapping, 4 + page * 2)?;
                let index = little(self.bytes, slot.get() as usize, 2)?;
                if index == 0xffff { return Err(FontError::MissingGlyph(glyph)); }
                add((low & mask).checked_add(index).ok_or(FontError::InvalidDirectory)?)
            }
            _ => Err(FontError::InvalidDirectory),
        }
    }
    pub fn glyph_program(&self, glyph: GlyphId, writing: WritingMode) -> Result<GlyphProgram<'a>, FontError> {
        if glyph.get() >= self.glyphs.count { return Err(FontError::MissingGlyph(glyph)); }
        let index = if matches!(self.format, DirectoryFormat::Paged) { self.mapped_index(glyph)? } else { glyph.get() };
        let mut directory = self.glyphs;
        if matches!(self.format, DirectoryFormat::Paged) { directory.count = u32::MAX; }
        let mut flags = if matches!(self.format, DirectoryFormat::Dense) { little(self.bytes, directory.entries.get() as usize + index as usize * 4 + 3, 1)? as u8 } else { 0 };
        let span = self.record(directory, index, if matches!(self.format, DirectoryFormat::Dense) { 4 } else { 3 })?;
        if span.len() == 0 { return Err(FontError::MissingGlyph(glyph)); }
        let mut cursor = Cursor::from_span(span);
        if matches!(self.format, DirectoryFormat::Paged) { flags = cursor.byte(self.bytes)?; }
        let vertical = matches!(writing, WritingMode::Vertical) && glyph.get() > 255;
        let zero = AdvancePerMille::new(0);
        let mut metrics = GlyphProgramMetrics { horizontal_x: zero, horizontal_y: zero, vertical_x: zero, vertical_y: zero };
        if flags & 1 != 0 {
            let x = AdvancePerMille::new(cursor.short(self.bytes)? as i16); let y = AdvancePerMille::new(cursor.short(self.bytes)? as i16);
            if !vertical { metrics.horizontal_x = x; metrics.horizontal_y = y; }
        }
        if flags & 2 != 0 {
            let x = AdvancePerMille::new(cursor.short(self.bytes)? as i16); let y = AdvancePerMille::new(cursor.short(self.bytes)? as i16);
            if vertical { metrics.vertical_x = x; metrics.vertical_y = y; }
        }
        let body = if flags != 0 {
            let length = cursor.short(self.bytes)? as usize;
            let primary = cursor.take(self.bytes, length)?;
            if vertical && flags & 4 != 0 && cursor.remaining() > 0 { let length = cursor.short(self.bytes)? as usize; if length != 0 { cursor.take(self.bytes, length)? } else { primary } } else { primary }
        } else { Span { start: cursor.position, end: cursor.end } };
        Ok(GlyphProgram { program: self.program_span(body), metrics })
    }
    pub fn render<'w>(&self, glyph: GlyphId, writing: WritingMode, workspace: &mut OutlineWorkspace, commands: &'w mut [OutlineCommand]) -> Result<GlyphOutline<'w>, FontError> {
        let program = self.glyph_program(glyph, writing)?;
        let source_advance = if matches!(writing, WritingMode::Vertical) && glyph.get() > 255 { program.metrics.vertical_y } else { program.metrics.horizontal_y };
        let metrics = OutlineMetrics {
            program_origin: FontPoint {
                x: FontUnit::new((i32::from(program.metrics.horizontal_x.get()).wrapping_mul(2048) / 1000) as i16 as i32),
                y: FontUnit::new((i32::from(program.metrics.vertical_x.get()).wrapping_mul(2048) / 1000) as i16 as i32),
            },
            source_advance: if source_advance.get() == 0 { GlyphAdvance::FullEm } else { GlyphAdvance::PerMille(source_advance) },
        };
        let body = program.program.span();
        workspace.state = OwnedState::new();
        workspace.state.hints = self.hints;
        workspace.state.origins = Pair { x: metrics.program_origin.y.get(), y: metrics.program_origin.x.get() };
        workspace.lengths = [0; 2];
        let mut cursors = Streams { primary: Cursor::from_span(body), alternate: None, active: Stream::Primary };
        let mut contours = BorrowedContours { segments: &mut workspace.segments, lengths: &mut workspace.lengths };
        let mut output = BorrowedCommands { commands, used: 0 };
        let output_writing = if matches!(writing, WritingMode::Vertical) && glyph.get() > 255 { WritingMode::Vertical } else { WritingMode::Horizontal };
        render_stream(self, &mut cursors, &mut workspace.state, &mut contours, &mut output, &mut workspace.frames, output_writing)?;
        Ok(GlyphOutline { commands: &output.commands[..output.used], metrics })
    }
}

#[derive(Clone, Copy)]
enum Stream { Primary, Alternate }
struct Streams { primary: Cursor, alternate: Option<Cursor>, active: Stream }
impl Streams {
    fn cursor(&mut self, stream: Stream) -> Result<&mut Cursor, FontError> { match stream { Stream::Primary => Ok(&mut self.primary), Stream::Alternate => self.alternate.as_mut().ok_or(FontError::InvalidDirectory) } }
    fn byte(&mut self, data: &[u8], stream: Stream) -> Result<u8, FontError> { self.cursor(stream)?.byte(data) }
    fn short(&mut self, data: &[u8], stream: Stream) -> Result<u16, FontError> { self.cursor(stream)?.short(data) }
}
#[derive(Clone, Copy)]
struct Transform { origin: Pair, extent: Pair, old_origin: Pair, old_extent: Pair, scale: i32 }
fn quotient(numerator: i32, denominator: i32) -> Result<i32, FontError> { if denominator == 0 { Err(FontError::DivisionByZero { numerator }) } else { Ok(numerator.wrapping_div(denominator)) } }
fn transform_parameters(state: &mut OwnedState, transform: Transform) -> Result<(), FontError> {
    if transform.scale != 128 {
        for index in 0..state.parameter_count {
            let product = (state.parameter_bytes[index] as i32).wrapping_mul(transform.scale).wrapping_add(64);
            state.parameter_bytes[index] = ((if product < 0 { product.wrapping_add(127) } else { product }) >> 7) as u8;
        }
    }
    for index in 0..state.point_count {
        let old = state.point_bytes[index];
        let x = quotient(transform.extent.x.wrapping_mul(old.y.wrapping_sub(transform.old_origin.x)).wrapping_add(transform.old_extent.x), transform.old_extent.x)?.wrapping_add(transform.origin.x);
        let y = quotient(transform.extent.y.wrapping_mul(old.x.wrapping_sub(transform.old_origin.y)).wrapping_add(transform.old_extent.y), transform.old_extent.y)?.wrapping_add(transform.origin.y);
        state.point_bytes[index] = Pair { x: y as u8 as i32, y: x as u8 as i32 };
    }
    Ok(())
}
#[derive(Clone, Copy)]
struct RenderFrame { selected: Stream, transform: Option<Transform> }
fn render_stream(resource: &FdlResource<'_>, streams: &mut Streams, state: &mut OwnedState, contours: &mut impl ContourStorage, output: &mut impl OutlineOutput, frames: &mut [RenderFrame], writing: WritingMode) -> Result<(), FontError> {
    let mut depth = 1;
    frames[0] = RenderFrame { selected: Stream::Primary, transform: None };
    streams.active = Stream::Primary;
    while depth != 0 {
        let RenderFrame { selected, transform } = frames[depth - 1];
        if streams.cursor(selected)?.remaining() == 0 { streams.active = Stream::Primary; depth -= 1; continue; }
        let active = streams.active;
        let opcode = streams.byte(resource.bytes, active)?;
        match opcode {
            0xfd => { let length = streams.short(resource.bytes, active)? as usize; streams.primary.take(resource.bytes, length)?; }
            0xfc => {
                let glyph = streams.short(resource.bytes, active)? as u32;
                let span = resource.record(resource.compounds, glyph, 3)?;
                let mut header = Cursor::from_span(span);
                let origin = Pair { x: header.byte(resource.bytes)? as i32, y: header.byte(resource.bytes)? as i32 };
                let extent = Pair { x: header.byte(resource.bytes)? as i32, y: header.byte(resource.bytes)? as i32 };
                streams.alternate = Some(header);
                let flags = streams.byte(resource.bytes, active)?;
                let mut child = Transform { origin, extent, old_origin: origin, old_extent: extent, scale: 128 };
                if flags & 1 != 0 { child.origin.x = streams.byte(resource.bytes, active)? as i32; child.extent.x = streams.byte(resource.bytes, active)? as i32; }
                if flags & 2 != 0 { child.origin.y = streams.byte(resource.bytes, active)? as i32; child.extent.y = streams.byte(resource.bytes, active)? as i32; }
                if flags & 4 != 0 { child.scale = streams.byte(resource.bytes, active)? as i32; }
                let provided = frames.len();
                let slot = frames.get_mut(depth).ok_or_else(|| FontError::workspace((depth + 1) * core::mem::size_of::<RenderFrame>(), provided * core::mem::size_of::<RenderFrame>()))?;
                *slot = RenderFrame { selected: Stream::Alternate, transform: if flags == 0 { None } else { Some(child) } };
                depth += 1;
                streams.active = Stream::Alternate;
            }
            0xfa => {
                state.point_count = 0;
                let count = (streams.byte(resource.bytes, active)? as usize + 1) & !1;
                if count > state.parameter_bytes.len() { return Err(FontError::InvalidDirectory); }
                state.parameter_count = count;
                for index in 0..count { state.parameter_bytes[index] = streams.byte(resource.bytes, active)?; }
                let length = streams.short(resource.bytes, active)? as usize;
                let program = streams.cursor(selected)?.take(resource.bytes, length)?;
                let mut reader = BorrowedReader { bytes: resource.bytes, cursor: Cursor::from_span(program) };
                if let Some(transform) = transform {
                    state.offsets = Pair {
                        x: truncate_256(state.inline_translation_scale.y.wrapping_mul(transform.origin.y.wrapping_sub(transform.old_origin.y))),
                        y: truncate_256(state.inline_translation_scale.x.wrapping_mul(transform.origin.x.wrapping_sub(transform.old_origin.x))),
                    };
                }
                execute(state, &mut reader, contours, output, writing)?;
                if transform.is_some() { state.offsets = Pair { x: 0, y: 0 }; }
            }
            _ => {
                let glyph = if opcode == 0xff { streams.short(resource.bytes, active)? as u32 } else { opcode as u32 };
                let span = resource.record(resource.subroutines, glyph, 2)?;
                let mut header = Cursor::from_span(span);
                let shape = header.byte(resource.bytes)?;
                let mask = header.byte(resource.bytes)?;
                state.point_count = (shape >> 4) as usize;
                state.parameter_count = (shape & 15) as usize;
                if state.parameter_count > state.parameter_bytes.len() { return Err(FontError::InvalidDirectory); }
                let mut bit = 1u16;
                for index in 0..state.point_count {
                    let compressed = (1..=4).contains(&index) && mask != 0;
                    let x = if compressed && mask as u16 & bit != 0 { state.point_bytes[index - 1].x } else { streams.byte(resource.bytes, active)? as i32 };
                    if compressed { bit = bit.wrapping_shl(1); }
                    let y = if compressed && mask as u16 & bit != 0 { state.point_bytes[index - 1].y } else { streams.byte(resource.bytes, active)? as i32 };
                    if compressed { bit = bit.wrapping_shl(1); }
                    state.point_bytes[index] = Pair { x, y };
                }
                for index in 0..state.parameter_count { state.parameter_bytes[index] = streams.byte(resource.bytes, active)?; }
                if let Some(transform) = transform { transform_parameters(state, transform)?; }
                let mut reader = BorrowedReader { bytes: resource.bytes, cursor: header };
                execute(state, &mut reader, contours, output, writing)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(bytes: &mut [u8], offset: usize, value: u32, width: usize) {
        for index in 0..width { bytes[offset + index] = (value >> (index * 8)) as u8; }
    }
    fn dense_font(program: &[u8], raw_advance: i16) -> [u8; 512] {
        let mut bytes = [0; 512];
        bytes[..8].copy_from_slice(b"FDL Font");
        put(&mut bytes, 0x1c, 1, 2);
        put(&mut bytes, 0x2e, 0x80, 4);
        put(&mut bytes, 0x32, 0xa0, 4);
        put(&mut bytes, 0x36, 4, 2);
        put(&mut bytes, 0x38, 0x100, 2);
        put(&mut bytes, 0x80, 0x01000000, 4);
        put(&mut bytes, 0x84, (6 + program.len()) as u32, 3);
        put(&mut bytes, 0xa2, raw_advance as u16 as u32, 2);
        put(&mut bytes, 0xa4, program.len() as u32, 2);
        bytes[0xa6..0xa6 + program.len()].copy_from_slice(program);
        bytes
    }
    fn p(x: i32, y: i32) -> FontPoint { FontPoint { x: FontUnit::new(x), y: FontUnit::new(y) } }

    #[test]
    fn actual_program_produces_baseline_relative_path_and_source_metrics() {
        let mut bytes = dense_font(&[0xfa,0,4,0,3,1,7,1], 500);
        put(&mut bytes, 0xa0, 125, 2);
        let resource = FdlResource::parse(&bytes).unwrap();
        let mut workspace = OutlineWorkspace::new();
        let mut commands = [OutlineCommand::Close; 8];
        let glyph = resource.render(GlyphId::new(0), WritingMode::Horizontal, &mut workspace, &mut commands).unwrap();
        assert_eq!(glyph.commands, &[OutlineCommand::MoveTo(p(-256,0)), OutlineCommand::LineTo(p(1791,2047)), OutlineCommand::LineTo(p(-256,0)), OutlineCommand::Close]);
        assert_eq!(glyph.metrics.program_origin, p(256,0));
        assert_eq!(glyph.metrics.source_advance, GlyphAdvance::PerMille(AdvancePerMille::new(500)));
    }

    #[test]
    fn invalid_instruction_is_a_typed_error() {
        let bytes = dense_font(&[0xfa,0,2,0,1,0], 0);
        let resource = FdlResource::parse(&bytes).unwrap();
        let mut workspace = OutlineWorkspace::new();
        let mut commands = [OutlineCommand::Close; 8];
        assert!(matches!(resource.render(GlyphId::new(0), WritingMode::Horizontal, &mut workspace, &mut commands), Err(FontError::InvalidInstruction(opcode)) if opcode == Opcode::new(1)));
    }

    #[test]
    fn bounded_command_workspace_reports_required_storage() {
        let bytes = dense_font(&[0xfa,0,4,0,3,1,7,1], 0);
        let resource = FdlResource::parse(&bytes).unwrap();
        let mut workspace = OutlineWorkspace::new();
        let mut commands = [OutlineCommand::Close; 2];
        assert!(matches!(resource.render(GlyphId::new(0), WritingMode::Horizontal, &mut workspace, &mut commands), Err(FontError::InsufficientWorkspace { required, provided }) if required.get() == 3 * core::mem::size_of::<OutlineCommand>() && provided.get() == 2 * core::mem::size_of::<OutlineCommand>()));
    }

    #[test]
    fn truncated_program_does_not_read_the_next_record() {
        let bytes = dense_font(&[0xfa,0,9,0,3,1], 0);
        let resource = FdlResource::parse(&bytes).unwrap();
        let mut workspace = OutlineWorkspace::new();
        let mut commands = [OutlineCommand::Close; 8];
        assert!(matches!(resource.render(GlyphId::new(0), WritingMode::Horizontal, &mut workspace, &mut commands), Err(FontError::Truncated)));
        assert!(matches!(resource.render(GlyphId::new(1), WritingMode::Horizontal, &mut workspace, &mut commands), Err(FontError::MissingGlyph(glyph)) if glyph == GlyphId::new(1)));
    }

    #[test]
    fn compact_directory_uses_unprefixed_program_records() {
        let mut bytes = dense_font(&[], 0);
        put(&mut bytes, 0x38, 0, 2);
        put(&mut bytes, 0x80, 0, 3);
        put(&mut bytes, 0x83, 8, 3);
        bytes[0xa0..0xa8].copy_from_slice(&[0xfa,0,4,0,3,1,7,1]);
        let resource = FdlResource::parse(&bytes).unwrap();
        let mut workspace = OutlineWorkspace::new();
        let mut commands = [OutlineCommand::Close; 8];
        let glyph = resource.render(GlyphId::new(0), WritingMode::Horizontal, &mut workspace, &mut commands).unwrap();
        assert_eq!(glyph.commands, &[OutlineCommand::MoveTo(p(0,0)), OutlineCommand::LineTo(p(2047,2047)), OutlineCommand::LineTo(p(0,0)), OutlineCommand::Close]);
        assert_eq!(glyph.metrics.source_advance, GlyphAdvance::FullEm);
    }

    #[test]
    fn paged_range_mapping_selects_vertical_program_metrics() {
        let mut bytes = [0; 512];
        bytes[..8].copy_from_slice(b"FDL Font");
        put(&mut bytes, 0x2e, 0x80, 4);
        put(&mut bytes, 0x32, 0x100, 4);
        put(&mut bytes, 0x36, 4, 2);
        put(&mut bytes, 0x38, 0x101, 2);
        put(&mut bytes, 0x80, 0x10000, 4);
        put(&mut bytes, 0x84, 0x40, 4);
        put(&mut bytes, 0x8a, 1, 2);
        put(&mut bytes, 0x8c, 0x20, 4);
        put(&mut bytes, 0xa0, 1, 2);
        put(&mut bytes, 0xa2, 0x100, 2);
        put(&mut bytes, 0xa4, 0x100, 2);
        put(&mut bytes, 0xc3, 19, 3);
        bytes[0x100] = 3;
        put(&mut bytes, 0x103, 500, 2);
        put(&mut bytes, 0x105, 125, 2);
        put(&mut bytes, 0x107, 750, 2);
        put(&mut bytes, 0x109, 8, 2);
        bytes[0x10b..0x113].copy_from_slice(&[0xfa,0,4,0,3,1,7,1]);
        let resource = FdlResource::parse(&bytes).unwrap();
        let mut workspace = OutlineWorkspace::new();
        let mut commands = [OutlineCommand::Close; 8];
        let glyph = resource.render(GlyphId::new(0x100), WritingMode::Vertical, &mut workspace, &mut commands).unwrap();
        assert_eq!(glyph.commands, &[OutlineCommand::MoveTo(p(2304,0)), OutlineCommand::LineTo(p(257,2047)), OutlineCommand::LineTo(p(2304,0)), OutlineCommand::Close]);
        assert_eq!(glyph.metrics.source_advance, GlyphAdvance::PerMille(AdvancePerMille::new(750)));
    }
    #[test]
    fn reader_exposes_actual_record_bytes_and_selected_raw_prefix_fields() {
        let mut bytes = dense_font(&[0xfa,0,4,0,3,1,7,1], 500);
        put(&mut bytes, 0xa0, 125, 2);
        let resource = FdlResource::parse(&bytes).unwrap();
        let glyph = resource.glyph_program(GlyphId::new(0), WritingMode::Horizontal).unwrap();
        assert_eq!(glyph.program.offset(), AssetOffset::new(0xa6));
        assert_eq!(glyph.program.bytes(), &[0xfa,0,4,0,3,1,7,1]);
        assert_eq!(glyph.metrics, GlyphProgramMetrics { horizontal_x: AdvancePerMille::new(125), horizontal_y: AdvancePerMille::new(500), vertical_x: AdvancePerMille::new(0), vertical_y: AdvancePerMille::new(0) });
    }

    #[test]
    fn property10_reads_source_coefficients_and_absence_without_fallback() {
        let mut bytes = dense_font(&[], 0);
        put(&mut bytes, 0x3a, 0x180, 4);
        put(&mut bytes, 0x180, 1, 2);
        put(&mut bytes, 0x182, 10, 2);
        put(&mut bytes, 0x184, 18, 2);
        put(&mut bytes, 0x18a, 1, 2);
        put(&mut bytes, 0x190, 500, 2);
        put(&mut bytes, 0x192, 750, 2);
        let resource = FdlResource::parse(&bytes).unwrap();
        let property = resource.property10().unwrap().unwrap();
        assert_eq!(property.offset(), AssetOffset::new(0x182));
        assert_eq!(property.bytes().len(), 18);
        assert_eq!(resource.glyph_scale(GlyphId::new(0)).unwrap().unwrap().get(), 500);
        assert_eq!(resource.glyph_scale(GlyphId::new(1)).unwrap().unwrap().get(), 750);
        assert_eq!(resource.glyph_scale(GlyphId::new(2)).unwrap(), None);
    }

    #[test]
    fn compound_zero_divisor_returns_the_actual_numerator() {
        let mut bytes = dense_font(&[0xfc,0,0,3,20,100,30,120], 0);
        put(&mut bytes, 0x18, 1, 2);
        put(&mut bytes, 0x1e, 0x140, 4);
        put(&mut bytes, 0x22, 0x160, 4);
        put(&mut bytes, 0x142, 6, 2);
        bytes[0x160..0x166].copy_from_slice(&[0x20,0,3,1,7,1]);
        put(&mut bytes, 0x1a, 1, 2);
        put(&mut bytes, 0x26, 0x148, 4);
        put(&mut bytes, 0x2a, 0x170, 4);
        put(&mut bytes, 0x14b, 9, 3);
        bytes[0x170..0x179].copy_from_slice(&[0,0,0,60,0,10,20,50,60]);
        let resource = FdlResource::parse(&bytes).unwrap();
        let mut workspace = OutlineWorkspace::new();
        let mut commands = [OutlineCommand::Close; 8];
        assert!(matches!(resource.render(GlyphId::new(0), WritingMode::Horizontal, &mut workspace, &mut commands), Err(FontError::DivisionByZero { numerator: 2000 })));
    }
}
