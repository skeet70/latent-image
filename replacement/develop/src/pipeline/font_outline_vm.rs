use core::ptr::{read_unaligned, write_unaligned};
use crate::develop::font_core::{FontError, FontPoint, FontUnit, Opcode, OutlineCommand, WritingMode};
use crate::develop::font_core::outline::{self, Axis, Contour, ContourStorage, Operands, OutlineOutput, Pair, ParameterBank, ProgramReader, Scalar, Segment, VmState, truncate_256};

#[derive(Clone, Copy)]
struct Address(u32);
impl Address {
    fn at(self, offset: u32) -> Self { Self(self.0.wrapping_add(offset)) }
    unsafe fn byte(self) -> u8 { read_unaligned(self.0 as *const u8) }
    unsafe fn half(self) -> i16 { read_unaligned(self.0 as *const i16) }
    unsafe fn word(self) -> i32 { read_unaligned(self.0 as *const i32) }
    unsafe fn put_byte(self, value: u8) { write_unaligned(self.0 as *mut u8, value); }
    unsafe fn put_half(self, value: i16) { write_unaligned(self.0 as *mut i16, value); }
    unsafe fn put(self, value: i32) { write_unaligned(self.0 as *mut i32, value); }
}

unsafe extern "C" {
    fn xm1_pipeline_image_font_program_read_byte(context: u32) -> u8;
    fn xm1_pipeline_image_font_program_read_short(context: u32) -> i16;
}

#[derive(Clone, Copy)]
struct PhysicalState(Address);
fn scalar_offset(field: Scalar) -> u32 {
    match field {
        Scalar::Register(axis, index) => (match axis { Axis::X => 0x670, Axis::Y => 0x6b0 }) + index.get() as u32 * 4,
        Scalar::Parameter(bank, index) => (match bank { ParameterBank::Low => 0x710, ParameterBank::High => 0x730 }) + index as u32 * 4,
        Scalar::FirstX => 0x764, Scalar::FirstY => 0x768, Scalar::SecondX => 0x76c, Scalar::SecondY => 0x770, Scalar::ExtentX => 0x774, Scalar::ExtentY => 0x778,
        Scalar::StartX => 0x77c, Scalar::StartY => 0x780, Scalar::ControlX => 0x784, Scalar::ControlY => 0x788, Scalar::EndX => 0x78c, Scalar::EndY => 0x790,
    }
}
impl VmState for PhysicalState {
    fn read(&self, field: Scalar) -> i32 { unsafe { self.0.at(scalar_offset(field)).word() } }
    fn write(&mut self, field: Scalar, value: i32) { unsafe { self.0.at(scalar_offset(field)).put(value) }; }
    fn scale(&self) -> i32 { unsafe { self.0.at(0x750).word() } }
    fn set_scale(&mut self, value: i32) { unsafe { self.0.at(0x750).put(value) }; }
    fn offsets(&self) -> Pair { unsafe { Pair { x: self.0.at(0x60).word(), y: self.0.at(0x5c).word() } } }
    fn origins(&self) -> Pair { unsafe { Pair { x: self.0.at(0x44).word(), y: self.0.at(0x40).half() as i32 } } }
    fn point_count(&self) -> usize { unsafe { self.0.at(0x18).word().max(0) as usize } }
    fn point_header_present(&self) -> bool { unsafe { self.0.at(0x18).word() != 0 } }
    fn initial_point(&self, index: usize) -> Pair { unsafe { Pair { x: self.0.at(0x1c + index as u32 * 2).byte() as i32, y: self.0.at(0x1d + index as u32 * 2).byte() as i32 } } }
    fn parameter_count(&self) -> usize { unsafe { self.0.at(0x2c).word().max(0) as usize } }
    fn initial_parameter(&self, index: usize) -> u8 { unsafe { self.0.at(0x30 + index as u32).byte() } }
    fn hint(&self, index: usize) -> u16 { unsafe { self.0.at(8 + index as u32 * 2).half() as u16 } }
    fn set_parameter(&mut self, index: usize, value: i32) { unsafe { self.0.at(0x6f0 + index as u32 * 4).put(value) }; }
    fn set_opcode(&mut self, opcode: Opcode) { unsafe { self.0.at(0x66c).put_byte(opcode.get()) }; }
    fn set_operands(&mut self, operands: Operands) {
        unsafe {
            self.0.at(0x754).put(operands.source.get() as i32); self.0.at(0x758).put(operands.destination.get() as i32);
            self.0.at(0x75c).put(operands.source.get() as i32); self.0.at(0x760).put(operands.destination.get() as i32);
        }
    }
    fn has_control(&self) -> bool { unsafe { self.0.at(0x794).word() != 0 } }
    fn set_control(&mut self, value: bool) { unsafe { self.0.at(0x794).put(value as i32) }; }
    fn contour(&self) -> Contour { if unsafe { self.0.at(0x798).word() } == 0 { Contour::Forward } else { Contour::Reverse } }
    fn set_contour(&mut self, contour: Contour) { unsafe { self.0.at(0x798).put(contour.index() as i32) }; }
}
struct PhysicalReader(Address);
impl ProgramReader for PhysicalReader {
    fn byte(&mut self) -> Result<u8, FontError> { Ok(unsafe { xm1_pipeline_image_font_program_read_byte(self.0.word() as u32) }) }
    fn short(&mut self) -> Result<i16, FontError> { Ok(unsafe { xm1_pipeline_image_font_program_read_short(self.0.word() as u32) }) }
    fn remaining(&self) -> i32 { unsafe { self.0.at(0x38).word() } }
    fn rewind_sentinel(&mut self) -> Result<(), FontError> {
        unsafe { self.0.at(0x38).put(self.0.at(0x38).word().wrapping_add(1)); self.0.at(0x3c).put(self.0.at(0x3c).word().wrapping_sub(1)) };
        Ok(())
    }
}
struct PhysicalContours(Address);
fn physical_point(address: Address) -> FontPoint { unsafe { FontPoint { x: FontUnit::new(address.half() as i32), y: FontUnit::new(address.at(2).half() as i32) } } }
fn put_point(address: Address, point: FontPoint) { unsafe { address.put_half(point.x.get() as i16); address.at(2).put_half(point.y.get() as i16) }; }
impl ContourStorage for PhysicalContours {
    fn push(&mut self, contour: Contour, segment: Segment) -> Result<(), FontError> {
        unsafe {
            let slot = self.0.at(0x664 + contour.index() as u32 * 4);
            let destination = Address(slot.word() as u32);
            put_point(destination, segment.start);
            match segment.control { Some(control) => put_point(destination.at(4), control), None => { destination.at(4).put_half(-1); destination.at(6).put_half(-1); } }
            put_point(destination.at(8), segment.end);
            slot.put(destination.at(12).0 as i32);
        }
        Ok(())
    }
    fn len(&self, contour: Contour) -> usize {
        unsafe { ((self.0.at(0x664 + contour.index() as u32 * 4).word().wrapping_sub(self.0.at(if contour.index() == 0 { 0x64 } else { 0x364 }).0 as i32) >> 1) / 6).max(0) as usize }
    }
    fn segment(&self, contour: Contour, index: usize) -> Segment {
        let source = self.0.at((if contour.index() == 0 { 0x64 } else { 0x364 }) + index as u32 * 12);
        Segment { start: physical_point(source), control: Some(physical_point(source.at(4))), end: physical_point(source.at(8)) }
    }
    fn reset(&mut self) { unsafe { self.0.at(0x664).put(self.0.at(0x64).0 as i32); self.0.at(0x668).put(self.0.at(0x364).0 as i32) }; }
}
struct PhysicalOutput { state: Address, capacity: i32, used: i32, header: Option<Address>, records: i16, points: i16 }
impl PhysicalOutput {
    fn new(state: Address) -> Self { Self { state, capacity: 0, used: 0, header: None, records: 0, points: 0 } }
    fn emit(&mut self, kind: i16, first: FontPoint, second: Option<FontPoint>) {
        self.used = self.used.wrapping_add(if second.is_some() { 5 } else { 3 });
        unsafe {
            if self.used <= self.capacity {
                let destination = Address(self.state.at(0x50).word() as u32);
                destination.put_half(kind);
                put_point(destination.at(2), first);
                if let Some(second) = second { put_point(destination.at(6), second); }
                self.state.at(0x50).put(destination.at(if second.is_some() { 10 } else { 6 }).0 as i32);
            }
            self.state.at(0x58).put(self.used);
        }
        self.records = self.records.wrapping_add(1);
        self.points = self.points.wrapping_add(if second.is_some() { 2 } else { 1 });
    }
}
impl OutlineOutput for PhysicalOutput {
    fn begin(&mut self) -> Result<(), FontError> {
        unsafe {
            self.capacity = self.state.at(0x54).word(); self.used = self.state.at(0x58).word().wrapping_add(2);
            self.header = if self.used <= self.capacity { let header = Address(self.state.at(0x50).word() as u32); header.put_half(0); header.at(2).put_half(0); self.state.at(0x50).put(header.at(4).0 as i32); Some(header) } else { None };
            self.state.at(0x58).put(self.used);
        }
        self.records = 0; self.points = 0; Ok(())
    }
    fn command(&mut self, command: OutlineCommand) -> Result<(), FontError> {
        match command {
            OutlineCommand::MoveTo(point) => self.emit(1, point, None),
            OutlineCommand::LineTo(point) => self.emit(2, point, None),
            OutlineCommand::QuadraticTo { control, end } => self.emit(3, control, Some(end)),
            OutlineCommand::Close => {
                unsafe {
                    if self.used < self.capacity { let destination = Address(self.state.at(0x50).word() as u32); destination.put_half(4); self.state.at(0x50).put(destination.at(2).0 as i32); }
                    self.used = self.used.wrapping_add(1); self.state.at(0x58).put(self.used);
                }
                self.records = self.records.wrapping_add(1);
            }
        }
        Ok(())
    }
    fn finish(&mut self) -> Result<(), FontError> { if let Some(header) = self.header { unsafe { header.put_half(self.records); header.at(2).put_half(self.points) }; } Ok(()) }
}
fn writing(state: Address) -> WritingMode { if unsafe { state.at(4).word() } == 0 { WritingMode::Horizontal } else { WritingMode::Vertical } }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_execute_outline_program(vm: u32, _arg1: u32, _arg2: u32) -> i32 {
    let state = Address(vm);
    match outline::execute(&mut PhysicalState(state), &mut PhysicalReader(state), &mut PhysicalContours(state), &mut PhysicalOutput::new(state), writing(state)) {
        Ok(()) => 0,
        Err(FontError::DivisionByZero { numerator }) => unsafe { super::font_fdl::signed_quotient(numerator, 0) },
        Err(_) => -3,
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_emit_outline(vm: u32) {
    let state = Address(vm);
    let _ = outline::finalize(&PhysicalContours(state), &mut PhysicalOutput::new(state), writing(state));
}

#[derive(Clone, Copy)]
enum Stream { Primary, Alternate }
impl Stream {
    fn parse(value: u32) -> Self { if value == 0 { Self::Primary } else { Self::Alternate } }
    fn remaining(self) -> u32 { match self { Self::Primary => 0x60, Self::Alternate => 0x70 } }
    fn cursor(self) -> u32 { self.remaining() + 4 }
}
#[derive(Clone, Copy)]
struct Program(Address);
impl Program {
    unsafe fn get(self, offset: u32) -> i32 { self.0.at(offset).word() }
    unsafe fn set(self, offset: u32, value: i32) { self.0.at(offset).put(value); }
    unsafe fn data(self) -> Address { Address(self.get(8) as u32) }
    unsafe fn byte(self, stream: Stream) -> u8 {
        let cursor = self.get(stream.cursor());
        let value = self.data().at(cursor as u32).byte();
        self.set(stream.cursor(), cursor.wrapping_add(1));
        self.set(stream.remaining(), self.get(stream.remaining()).wrapping_sub(1));
        value
    }
    unsafe fn short(self, stream: Stream) -> u16 { let low = self.byte(stream) as u16; low | (self.byte(stream) as u16) << 8 }
    unsafe fn little(self, offset: u32, count: u32) -> i32 {
        let mut value = 0u32;
        let mut index = 0;
        while index < count { value |= (self.data().at(offset.wrapping_add(index)).byte() as u32) << (index * 8); index += 1; }
        value as i32
    }
    unsafe fn skip(self, stream: Stream, count: i32) {
        self.set(stream.cursor(), self.get(stream.cursor()).wrapping_add(count));
        self.set(stream.remaining(), self.get(stream.remaining()).wrapping_sub(count));
    }
}

#[derive(Clone, Copy)]
struct GlyphIndex(u16);
enum RenderCommand { Skip, Compound, Inline, Glyph(GlyphIndex) }
impl RenderCommand {
    unsafe fn read(program: Program, stream: Stream) -> Self {
        match program.byte(stream) {
            0xfd => Self::Skip,
            0xfc => Self::Compound,
            0xfa => Self::Inline,
            0xff => Self::Glyph(GlyphIndex(program.short(stream))),
            glyph => Self::Glyph(GlyphIndex(glyph as u16)),
        }
    }
}

#[repr(C)]
pub struct RenderCallerState {
    compression_mask: u32,
    descriptor: [u8; 24],
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_render_outline_program_state(context: u32, transform: u32, stream: u32, caller: *mut RenderCallerState) -> i32 {
    let program = Program(Address(context));
    let stream_kind = Stream::parse(stream);
    program.set(0x6c, stream as i32);
    let descriptor = Address((*caller).descriptor.as_mut_ptr() as usize as u32);
    loop {
        if program.get(stream_kind.remaining()) < 1 { program.set(0x6c, 0); return 0; }
        let active = Stream::parse(program.get(0x6c) as u32);
        let command = RenderCommand::read(program, active);
        let status = match command {
            RenderCommand::Skip => {
                let count = program.short(active) as i32;
                program.skip(Stream::Primary, count);
                continue;
            }
            RenderCommand::Compound => {
                let glyph = program.short(active) as i32;
                if glyph < program.get(0x10) {
                    let entry = program.get(0x20).wrapping_add(glyph.wrapping_mul(3)) as u32;
                    let start = program.little(entry, 3);
                    let size = program.little(entry.wrapping_add(3), 3).wrapping_sub(start);
                    if size > 0 {
                        let cursor = start.wrapping_add(program.get(0x24));
                        let packed = program.little(cursor as u32, 4) as u32;
                        descriptor.put_half(glyph as i16);
                        descriptor.at(4).put_half((packed & 255) as i16);
                        descriptor.at(6).put_half(((packed >> 8) & 255) as i16);
                        descriptor.at(8).put_half(((packed >> 16) & 255) as i16);
                        descriptor.at(10).put_half((packed >> 24) as i16);
                        for offset in [4, 6, 8, 10] { descriptor.at(offset + 8).put_half(descriptor.at(offset).half()); }
                        descriptor.at(20).put_half(128);
                        program.set(0x70, size.wrapping_sub(4));
                        program.set(0x74, cursor.wrapping_add(4));
                    }
                }
                let flags = program.byte(active);
                descriptor.at(2).put_byte(flags);
                if flags & 1 != 0 { descriptor.at(4).put_half(program.byte(active) as i16); descriptor.at(8).put_half(program.byte(active) as i16); }
                if flags & 2 != 0 { descriptor.at(6).put_half(program.byte(active) as i16); descriptor.at(10).put_half(program.byte(active) as i16); }
                if flags & 4 != 0 { descriptor.at(20).put_half(program.byte(active) as i16); }
                let child_transform = if flags == 0 { 0 } else { descriptor.0 };
                xm1_pipeline_image_font_render_outline_program_child(context, child_transform, 1, (*caller).compression_mask)
            }
            RenderCommand::Inline => {
                program.set(0x90, 0);
                let count = (program.byte(active) as u32 + 1) & !1;
                program.set(0xa4, count as i32);
                let mut index = 0;
                while index < count { program.0.at(0xa8 + index).put_byte(program.byte(active)); index += 1; }
                let bytes = program.short(active) as i32;
                program.set(0xb0, bytes);
                let cursor = program.get(stream_kind.cursor());
                program.skip(stream_kind, bytes);
                program.set(0xb4, cursor);
                if transform != 0 {
                    let transform = Address(transform);
                    program.set(0xd4, truncate_256(program.get(0xc0).wrapping_mul((transform.at(4).half() as i32).wrapping_sub(transform.at(12).half() as i32))));
                    program.set(0xd8, truncate_256(program.get(0xc4).wrapping_mul((transform.at(6).half() as i32).wrapping_sub(transform.at(14).half() as i32))));
                    let status = xm1_pipeline_image_font_execute_outline_program(context.wrapping_add(0x78), 0, 0);
                    program.set(0xd4, 0);
                    program.set(0xd8, 0);
                    status
                } else { xm1_pipeline_image_font_execute_outline_program(context.wrapping_add(0x78), 0, 0) }
            }
            RenderCommand::Glyph(glyph) => {
                let glyph = glyph.0 as i32;
                if glyph < program.get(0xc) {
                    let entry = program.get(0x18).wrapping_add(glyph.wrapping_mul(2)) as u32;
                    let start = program.little(entry, 2);
                    let size = program.little(entry.wrapping_add(2), 2).wrapping_sub(start);
                    if size > 0 {
                        let cursor = start.wrapping_add(program.get(0x1c));
                        let header = program.data().at(cursor as u32).byte();
                        program.set(0x90, (header >> 4) as i32);
                        program.set(0xa4, (header & 15) as i32);
                        (*caller).compression_mask = program.data().at(cursor.wrapping_add(1) as u32).byte() as u32;
                        program.set(0xb0, size.wrapping_sub(2));
                        program.set(0xb4, cursor.wrapping_add(2));
                    }
                }
                let mut index = 0;
                let mut bit = 1u16;
                while index < program.get(0x90) {
                    for axis in 0..2 {
                        let destination = program.0.at(0x94 + index as u32 * 2 + axis);
                        let repeated = (1..=4).contains(&index) && (*caller).compression_mask != 0 && ((*caller).compression_mask & bit as u32) != 0;
                        destination.put_byte(if repeated { program.0.at(0x92 + index as u32 * 2 + axis).byte() } else { program.byte(active) });
                        if (1..=4).contains(&index) && (*caller).compression_mask != 0 { bit = bit.wrapping_shl(1); }
                    }
                    index += 1;
                }
                let mut index = 0;
                while index < program.get(0xa4) { program.0.at(0xa8 + index as u32).put_byte(program.byte(active)); index += 1; }
                if transform != 0 { transform_parameters(program, Address(transform)); }
                xm1_pipeline_image_font_execute_outline_program(context.wrapping_add(0x78), 0, 0)
            }
        };
        if status != 0 { program.set(0x6c, 0); return status; }
    }
}

unsafe extern "C" {
    pub fn xm1_pipeline_image_font_render_outline_program(context: u32, transform: u32, stream: u32) -> i32;
    fn xm1_pipeline_image_font_render_outline_program_child(context: u32, transform: u32, stream: u32, compression_mask: u32) -> i32;
}
unsafe fn transform_parameters(program: Program, transform: Address) {
    let scale = transform.at(20).half() as i32;
    if scale != 128 {
        let mut index = 0;
        while index < program.get(0xa4) {
            let slot = program.0.at(0xa8 + index as u32);
            let product = (slot.byte() as i32).wrapping_mul(scale).wrapping_add(64);
            slot.put_byte((if product < 0 { product.wrapping_add(127) } else { product } >> 7) as u8);
            index += 1;
        }
    }
    let mut index = 0;
    while index < program.get(0x90) {
        for (axis, factor, origin, divisor, add) in [(1, 8, 12, 16, 4), (0, 10, 14, 18, 6)] {
            let slot = program.0.at(0x94 + index as u32 * 2 + axis);
            let numerator = (transform.at(factor).half() as i32).wrapping_mul((slot.byte() as i32).wrapping_sub(transform.at(origin).half() as i32)).wrapping_add(transform.at(divisor).half() as i32);
            let quotient = super::font_fdl::signed_quotient(numerator, transform.at(divisor).half() as i32);
            slot.put_byte((quotient as u8).wrapping_add(transform.at(add).half() as u8));
        }
        index += 1;
    }
}
