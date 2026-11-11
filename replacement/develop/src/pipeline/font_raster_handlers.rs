use core::convert::Infallible;
use core::ptr::{read_unaligned, write_unaligned};
use crate::develop::font_core::raster_vm::{self as portable, Axis, BytePoint, CoefficientBank, Dimension, FlushStatus, GridCoordinate, GridMask, GridShift, GridUnit, Instruction, OutputOffset, PointCount, PointIndex, ProgramOffset, Remaining, Row, RowCount, RowPair, Scale, ScaleValue as CoreScaleValue, Side, State, Storage, ValueCount};
use crate::develop::font_core::Opcode;

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RasterVm(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RasterContext(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RasterStatus(pub i32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RasterMode(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ScaleIndex(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ScaleValue(pub i32);

struct PhysicalState { vm: RasterVm }
struct PhysicalStorage { state: PhysicalState }

unsafe fn word(vm: RasterVm, offset: u32) -> i32 { unsafe { read_unaligned(vm.0.wrapping_add(offset) as *const i32) } }
unsafe fn store(vm: RasterVm, offset: u32, value: i32) { unsafe { write_unaligned(vm.0.wrapping_add(offset) as *mut i32, value) } }
unsafe fn byte(vm: RasterVm, offset: u32) -> u8 { unsafe { read_unaligned(vm.0.wrapping_add(offset) as *const u8) } }
fn output_base(vm: RasterVm, side: Side) -> u32 { vm.0.wrapping_add(match side { Side::Minimum => 0xbc, Side::Maximum => 0x32bc }) }
fn point_base(axis: Axis) -> u32 { match axis { Axis::X => 0x7dc0, Axis::Y => 0x7e00 } }
fn coefficient_base(bank: CoefficientBank) -> u32 { match bank { CoefficientBank::Values => 0x7e40, CoefficientBank::X => 0x7e60, CoefficientBank::Y => 0x7e80 } }
fn physical(vm: RasterVm) -> PhysicalStorage { PhysicalStorage { state: PhysicalState { vm } } }
fn infallible<T>(result: Result<T, Infallible>) -> T { match result { Ok(value) => value, Err(error) => match error {} } }

macro_rules! numeric_fields {
    ($($field:ident, $setter:ident: $kind:ident => $offset:expr),* $(,)?) => { $(
        fn $field(&self) -> $kind { $kind(unsafe { word(self.vm, $offset) } as _) }
        fn $setter(&mut self, value: $kind) { unsafe { store(self.vm, $offset, value.0 as i32) } }
    )* };
}
macro_rules! boolean_fields {
    ($($field:ident, $setter:ident => $offset:expr),* $(,)?) => { $(
        fn $field(&self) -> bool { unsafe { word(self.vm, $offset) != 0 } }
        fn $setter(&mut self, value: bool) { unsafe { store(self.vm, $offset, i32::from(value)) } }
    )* };
}
impl State for PhysicalState {
    numeric_fields! {
        point_count, set_point_count: PointCount => 0x1c,
        value_count, set_value_count: ValueCount => 0x30,
        remaining, set_remaining: Remaining => 0x3c,
        cursor, set_cursor: ProgramOffset => 0x40,
        width, set_width: Dimension => 0x54,
        height, set_height: Dimension => 0x58,
        scale_x, set_scale_x: CoreScaleValue => 0xa8,
        scale_y, set_scale_y: CoreScaleValue => 0xac,
        multiplier, set_multiplier: CoreScaleValue => 0xb0,
        row_count, set_row_count: RowCount => 0xb8,
        source_x, set_source_x: PointIndex => 0x7eb4,
        destination_x, set_destination_x: PointIndex => 0x7eb8,
        source_y, set_source_y: PointIndex => 0x7ebc,
        destination_y, set_destination_y: PointIndex => 0x7ec0,
        first_x, set_first_x: GridCoordinate => 0x7ec4,
        first_y, set_first_y: GridCoordinate => 0x7ec8,
        second_x, set_second_x: GridCoordinate => 0x7ecc,
        second_y, set_second_y: GridCoordinate => 0x7ed0,
        extent_x, set_extent_x: GridCoordinate => 0x7ed4,
        extent_y, set_extent_y: GridCoordinate => 0x7ed8,
        start_x, set_start_x: GridCoordinate => 0x7edc,
        start_y, set_start_y: GridCoordinate => 0x7ee0,
        control_x, set_control_x: GridCoordinate => 0x7ee4,
        control_y, set_control_y: GridCoordinate => 0x7ee8,
        end_x, set_end_x: GridCoordinate => 0x7eec,
        end_y, set_end_y: GridCoordinate => 0x7ef0,
        finish_x, set_finish_x: GridCoordinate => 0x7ef8,
        finish_y, set_finish_y: GridCoordinate => 0x7f00,
        left_y, set_left_y: GridCoordinate => 0x7ef4,
        right_y, set_right_y: GridCoordinate => 0x7efc,
        unit, set_unit: GridUnit => 0x7ea0,
        half_unit, set_half_unit: GridUnit => 0x7ea4,
        mask, set_mask: GridMask => 0x7ea8,
        shift, set_shift: GridShift => 0x7eac,
        half_shift, set_half_shift: GridShift => 0x7eb0,
        first_row, set_first_row: Row => 0x804c,
        minimum_row, set_minimum_row: Row => 0x8064,
        left_maximum_row, set_left_maximum_row: Row => 0x8068,
        right_maximum_row, set_right_maximum_row: Row => 0x806c,
        scan_half, set_scan_half: GridUnit => 0x8070,
        scan_step, set_scan_step: GridUnit => 0x8074,
    }
    boolean_fields! {
        oversample, set_oversample => 0xa4,
        left_state, set_left_state => 0x7f04,
        right_state, set_right_state => 0x7f08,
        has_control, set_has_control => 0x8050,
        edge_side, set_edge_side => 0x8054,
        small_scale, set_small_scale => 0x8058,
    }
    fn mode(&self) -> portable::RasterMode { portable::RasterMode(unsafe { word(self.vm, 0xb4) } as u32) }
    fn set_mode(&mut self, value: portable::RasterMode) { unsafe { store(self.vm, 0xb4, value.0 as i32) } }
    fn left_cursor(&self) -> OutputOffset { OutputOffset((unsafe { word(self.vm, 0x805c) } as u32).wrapping_sub(output_base(self.vm, Side::Minimum)) as i32) }
    fn set_left_cursor(&mut self, value: OutputOffset) { unsafe { store(self.vm, 0x805c, output_base(self.vm, Side::Minimum).wrapping_add(value.0 as u32) as i32) } }
    fn right_cursor(&self) -> OutputOffset { OutputOffset((unsafe { word(self.vm, 0x8060) } as u32).wrapping_sub(output_base(self.vm, Side::Maximum)) as i32) }
    fn set_right_cursor(&mut self, value: OutputOffset) { unsafe { store(self.vm, 0x8060, output_base(self.vm, Side::Maximum).wrapping_add(value.0 as u32) as i32) } }
    fn operand(&self) -> Opcode { Opcode::new(unsafe { byte(self.vm, 0x7dbc) }) }
    fn set_operand(&mut self, value: Opcode) { unsafe { write_unaligned(self.vm.0.wrapping_add(0x7dbc) as *mut u8, value.get()) } }
}

extern "C" {
    fn xm1_pipeline_image_font_raster_read_byte(context: RasterContext) -> u8;
    fn xm1_pipeline_image_font_raster_read_short(context: RasterContext) -> i16;
    fn xm1_pipeline_image_font_raster_draw_curve(vm: RasterVm);
    fn xm1_pipeline_image_font_raster_flush_scanlines(vm: RasterVm) -> i32;
}
impl Storage for PhysicalStorage {
    type Error = Infallible;
    type State = PhysicalState;
    fn state(&self) -> &Self::State { &self.state }
    fn state_mut(&mut self) -> &mut Self::State { &mut self.state }
    fn read_byte(&mut self) -> Result<u8, Self::Error> {
        Ok(unsafe { xm1_pipeline_image_font_raster_read_byte(RasterContext(word(self.state.vm, 8) as u32)) })
    }
    fn read_short(&mut self) -> Result<i16, Self::Error> {
        Ok(unsafe { xm1_pipeline_image_font_raster_read_short(RasterContext(word(self.state.vm, 8) as u32)) })
    }
    fn point(&self, axis: Axis, index: PointIndex) -> Result<GridCoordinate, Self::Error> {
        Ok(GridCoordinate(unsafe { word(self.state.vm, point_base(axis).wrapping_add(index.0.wrapping_mul(4))) }))
    }
    fn set_point(&mut self, axis: Axis, index: PointIndex, value: GridCoordinate) -> Result<(), Self::Error> {
        unsafe { store(self.state.vm, point_base(axis).wrapping_add(index.0.wrapping_mul(4)), value.0) }
        Ok(())
    }
    fn coefficient(&self, bank: CoefficientBank, index: portable::ScaleIndex) -> Result<CoreScaleValue, Self::Error> {
        Ok(CoreScaleValue(unsafe { word(self.state.vm, coefficient_base(bank).wrapping_add(index.0.wrapping_mul(4))) }))
    }
    fn set_coefficient(&mut self, bank: CoefficientBank, index: portable::ScaleIndex, value: CoreScaleValue) -> Result<(), Self::Error> {
        unsafe { store(self.state.vm, coefficient_base(bank).wrapping_add(index.0.wrapping_mul(4)), value.0) }
        Ok(())
    }
    fn initial_half(&self, index: portable::ScaleIndex) -> Result<u16, Self::Error> {
        Ok(unsafe { read_unaligned(self.state.vm.0.wrapping_add(0xc).wrapping_add(index.0.wrapping_mul(2)) as *const u16) })
    }
    fn initial_value(&self, index: portable::ScaleIndex) -> Result<u8, Self::Error> {
        Ok(unsafe { byte(self.state.vm, 0x34u32.wrapping_add(index.0)) })
    }
    fn initial_point(&self, index: PointIndex) -> Result<BytePoint, Self::Error> {
        let offset = index.0.wrapping_mul(2);
        Ok(BytePoint { x: unsafe { byte(self.state.vm, 0x20u32.wrapping_add(offset)) }, y: unsafe { byte(self.state.vm, 0x21u32.wrapping_add(offset)) } })
    }
    fn output_half(&self, side: Side, offset: OutputOffset) -> Result<i16, Self::Error> {
        Ok(unsafe { read_unaligned(output_base(self.state.vm, side).wrapping_add(offset.0 as u32) as *const i16) })
    }
    fn set_output_half(&mut self, side: Side, offset: OutputOffset, value: i16) -> Result<(), Self::Error> {
        unsafe { write_unaligned(output_base(self.state.vm, side).wrapping_add(offset.0 as u32) as *mut i16, value) }
        Ok(())
    }
    fn set_output_pair(&mut self, side: Side, offset: OutputOffset, value: RowPair) -> Result<(), Self::Error> {
        unsafe { write_unaligned(output_base(self.state.vm, side).wrapping_add(offset.0 as u32) as *mut u32, value.packed()) }
        Ok(())
    }
    fn contour_rows(&mut self) -> Result<RowCount, Self::Error> {
        let vm = self.state.vm;
        let left = unsafe { word(vm, 0x805c) } as u32;
        let right = unsafe { word(vm, 0x8060) } as u32;
        let end = left.min(right);
        if left > right { unsafe { store(vm, 0x805c, right as i32) } }
        Ok(RowCount((end.wrapping_sub(output_base(vm, Side::Minimum)) as i32) >> 1))
    }
    fn select_scale(&mut self, scale: Scale) {
        let callback = match scale {
            Scale::Small => xm1_pipeline_image_font_raster_vm_set_scale as unsafe extern "C" fn(RasterVm, ScaleIndex, ScaleValue),
            Scale::Large => xm1_pipeline_image_font_raster_vm_large_scale as unsafe extern "C" fn(RasterVm, ScaleIndex, ScaleValue),
        };
        unsafe { store(self.state.vm, 0, callback as usize as u32 as i32) }
    }
    fn draw_curve(&mut self) -> Result<(), Self::Error> { unsafe { xm1_pipeline_image_font_raster_draw_curve(self.state.vm) } Ok(()) }
    fn flush_scanlines(&mut self) -> Result<FlushStatus, Self::Error> {
        Ok(if unsafe { xm1_pipeline_image_font_raster_flush_scanlines(self.state.vm) } == 0 { FlushStatus::RunCapacity } else { FlushStatus::Complete })
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_vm_set_scale(vm: RasterVm, index: ScaleIndex, value: ScaleValue) {
    infallible(portable::set_scale(&mut physical(vm), portable::ScaleIndex(index.0), CoreScaleValue(value.0), Scale::Small));
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_vm_large_scale(vm: RasterVm, index: ScaleIndex, value: ScaleValue) {
    infallible(portable::set_scale(&mut physical(vm), portable::ScaleIndex(index.0), CoreScaleValue(value.0), Scale::Large));
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_vm_refresh_scales(vm: RasterVm) {
    infallible(portable::refresh_scales(&mut physical(vm)));
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_execute(vm: RasterVm, mode: RasterMode) -> RasterStatus {
    RasterStatus(match infallible(portable::execute(&mut physical(vm), portable::RasterMode(mode.0))) {
        portable::ExecuteStatus::Complete => 0,
        portable::ExecuteStatus::InvalidOpcode => -7,
        portable::ExecuteStatus::RunCapacity => -4,
    })
}
macro_rules! handler {
    ($name:ident, $instruction:ident) => {
        #[no_mangle]
        pub unsafe extern "C" fn $name(vm: RasterVm) -> RasterStatus {
            RasterStatus(infallible(portable::instruction(&mut physical(vm), Instruction::$instruction)) as i32)
        }
    };
}
handler!(xm1_pipeline_image_font_raster_vm_finish_3, Finish3);
handler!(xm1_pipeline_image_font_raster_vm_finish_4, Finish4);
handler!(xm1_pipeline_image_font_raster_vm_copy_x, CopyX);
handler!(xm1_pipeline_image_font_raster_vm_copy_y, CopyY);
handler!(xm1_pipeline_image_font_raster_vm_capture_bounds, CaptureBounds);
handler!(xm1_pipeline_image_font_raster_vm_capture_crossed_bounds, CaptureCrossedBounds);
handler!(xm1_pipeline_image_font_raster_vm_add_right_x, AddScaledX);
handler!(xm1_pipeline_image_font_raster_vm_subtract_left_x, SubtractScaledX);
handler!(xm1_pipeline_image_font_raster_vm_add_left_to_y, AddScaledY);
handler!(xm1_pipeline_image_font_raster_vm_subtract_lower_y, SubtractScaledY);
handler!(xm1_pipeline_image_font_raster_vm_no_op_first, NoOp);
handler!(xm1_pipeline_image_font_raster_vm_no_op_second, NoOp);
handler!(xm1_pipeline_image_font_raster_vm_subtract_bottom_y, SubtractY);
handler!(xm1_pipeline_image_font_raster_vm_add_top_y, AddY);
handler!(xm1_pipeline_image_font_raster_vm_subtract_left_y, SubtractX);
handler!(xm1_pipeline_image_font_raster_vm_add_right_y, AddCrossX);
handler!(xm1_pipeline_image_font_raster_vm_scale_subtract_left, ScaledSubtractY);
handler!(xm1_pipeline_image_font_raster_vm_scale_add_middle, ScaledAddY);
handler!(xm1_pipeline_image_font_raster_vm_scale_subtract_middle, ScaledSubtractX);
handler!(xm1_pipeline_image_font_raster_vm_scale_add_right, ScaledAddX);
handler!(xm1_pipeline_image_font_raster_vm_scale_subtract_bottom, ScaleSubtractY);
handler!(xm1_pipeline_image_font_raster_vm_scale_add_top, ScaleAddY);
handler!(xm1_pipeline_image_font_raster_vm_scale_subtract_x, ScaleSubtractX);
handler!(xm1_pipeline_image_font_raster_vm_scale_add_x, ScaleAddX);
handler!(xm1_pipeline_image_font_raster_vm_finish_curve, FinishCurve);
handler!(xm1_pipeline_image_font_raster_vm_stored_line, StoredLine);
handler!(xm1_pipeline_image_font_raster_vm_short_curve, ShortCurve);
handler!(xm1_pipeline_image_font_raster_vm_short_curve_to, ShortCurveTo);
handler!(xm1_pipeline_image_font_raster_vm_rotated_curve, RotatedCurve);
handler!(xm1_pipeline_image_font_raster_vm_rotated_curve_to, RotatedCurveTo);
handler!(xm1_pipeline_image_font_raster_vm_short_rotated_start, ShortRotatedStart);
handler!(xm1_pipeline_image_font_raster_vm_short_segment, ShortSegment);
handler!(xm1_pipeline_image_font_raster_vm_short_axis_start, ShortAxisStart);
handler!(xm1_pipeline_image_font_raster_vm_translate_short, TranslateShort);
handler!(xm1_pipeline_image_font_raster_vm_short_line_to, ShortLineTo);
handler!(xm1_pipeline_image_font_raster_vm_rotate_short, RotateShort);
handler!(xm1_pipeline_image_font_raster_vm_rotated_line_to, RotatedLineTo);
handler!(xm1_pipeline_image_font_raster_vm_rotated_segment, RotatedSegment);
handler!(xm1_pipeline_image_font_raster_vm_byte_curve, ByteCurve);
handler!(xm1_pipeline_image_font_raster_vm_byte_curve_to, ByteCurveTo);
handler!(xm1_pipeline_image_font_raster_vm_byte_axis_close_cross_basis, ByteAxisCloseCross);
handler!(xm1_pipeline_image_font_raster_vm_byte_axis_close_normal_basis, ByteAxisCloseNormal);
handler!(xm1_pipeline_image_font_raster_vm_byte_rotated_close_cross_basis, ByteRotatedCloseCross);
handler!(xm1_pipeline_image_font_raster_vm_byte_rotated_close_normal_basis, ByteRotatedCloseNormal);
handler!(xm1_pipeline_image_font_raster_vm_byte_rotated_close, ByteRotatedClose);
handler!(xm1_pipeline_image_font_raster_vm_rotated_byte_curve_to, RotatedByteCurveTo);
handler!(xm1_pipeline_image_font_raster_vm_byte_segment, ByteSegment);
handler!(xm1_pipeline_image_font_raster_vm_byte_axis_start, ByteAxisStart);
handler!(xm1_pipeline_image_font_raster_vm_translate_byte, TranslateByte);
handler!(xm1_pipeline_image_font_raster_vm_byte_line_to, ByteLineTo);
handler!(xm1_pipeline_image_font_raster_vm_byte_rotated_start, ByteRotatedStart);
handler!(xm1_pipeline_image_font_raster_vm_rotate_byte, RotateByte);
handler!(xm1_pipeline_image_font_raster_vm_rotated_byte_line_to, ByteRotatedLineTo);
handler!(xm1_pipeline_image_font_raster_vm_byte_rotated_line, ByteRotatedSegment);
