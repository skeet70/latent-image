use core::convert::Infallible;
use core::ptr::{read_unaligned, write_unaligned};
use crate::develop::font_core::scanlines::{self, BitmapOffset, BitmapStorage, Bounds, Config, Coordinate,
    Dimension, Expansion, Orientation, Output, Point, RenderStatus, Row, RowCount, Shear, Slant,
    Span, Storage, Stride, Style};

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Vm(u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct RunTable(u32);
#[derive(Clone, Copy)]
#[repr(u32)]
enum Field {
    Context=8, ClipX=0x44, ClipY=0x48, OriginX=0x4c, OriginY=0x50,
    Width=0x54, Height=0x58, Style=0x5c, Expansion=0x60, Shear=0x64,
    Bitmap=0x68, Stride=0x6c, MinX=0x74, MinY=0x78, MaxX=0x7c, MaxY=0x80,
    Output=0x84, Count=0xb8, FirstRow=0x804c,
}
extern "C" {
    fn xm1_pipeline_image_font_raster_fill_span(vm: u32, row: i32, first: i32, last: i32);
    fn xm1_pipeline_image_font_raster_merge_span(table: u32, row: i32, first: i32, last: i32) -> i32;
    fn xm1_pipeline_image_font_raster_subtract_run(table: u32, row: i32, first: i32, last: i32) -> i32;
    fn __aeabi_idiv(numerator: i32, denominator: i32) -> i32;
}
impl Vm {
    unsafe fn get(self, field: Field) -> i32 { unsafe { read_unaligned(self.0.wrapping_add(field as u32) as *const i32) } }
    unsafe fn set(self, field: Field, value: i32) { unsafe { write_unaligned(self.0.wrapping_add(field as u32) as *mut i32, value) } }
    unsafe fn byte(self, offset: u32) -> u8 { unsafe { read_unaligned(self.0.wrapping_add(offset) as *const u8) } }
    unsafe fn half(self, base: u32, row: Row) -> i32 { unsafe { i32::from(read_unaligned(self.0.wrapping_add(base).wrapping_add((row.0 as u32).wrapping_mul(2)) as *const i16)) } }
    unsafe fn put_half(self, base: u32, row: Row, value: i32) { unsafe { write_unaligned(self.0.wrapping_add(base).wrapping_add((row.0 as u32).wrapping_mul(2)) as *mut i16, value as i16) } }
    unsafe fn table(self) -> RunTable { unsafe { RunTable((self.get(Field::Context) as u32).wrapping_add(0x83b0)) } }
    unsafe fn config(self) -> Config {
        unsafe {
            let style=Style::from(self.get(Field::Style));
            let rotated=self.byte(0x71)!=0;
            let amount=self.get(Field::Shear);
            let slant=if amount<=0 { Slant::None } else if self.byte(0x70)!=0 && !rotated { Slant::Columns(Shear(amount)) } else { Slant::Rows(Shear(amount)) };
            Config {
                clip: Point { x: Coordinate(self.get(Field::ClipX)),y: Coordinate(self.get(Field::ClipY)) },
                origin: Point { x: Coordinate(self.get(Field::OriginX)),y: Coordinate(self.get(Field::OriginY)) },
                width: Dimension(self.get(Field::Width)),height: Dimension(self.get(Field::Height)),style,
                expansion: Expansion(self.get(Field::Expansion)),slant,
                orientation: if rotated { Orientation::Rotate } else { Orientation::Normal },
                output: if self.get(Field::Output)==0 { Output::Bitmap } else { Output::Runs },
                stride: Stride(self.get(Field::Stride)),count: RowCount(self.get(Field::Count)),first_row: Row(self.get(Field::FirstRow)),
                run_first_row: if matches!(style,Style::Outline) { Row(i32::from(read_unaligned(self.table().0.wrapping_add(12) as *const i16))) } else { Row(0) },
            }
        }
    }
}
impl BitmapStorage for Vm {
    type Error=Infallible;
    fn bitmap_byte(&self,offset: BitmapOffset) -> Result<u8,Infallible> {
        unsafe { Ok(read_unaligned((self.get(Field::Bitmap) as u32).wrapping_add(offset.0 as u32) as *const u8)) }
    }
    fn set_bitmap_byte(&mut self,offset: BitmapOffset,value: u8) -> Result<(),Infallible> {
        unsafe { write_unaligned((self.get(Field::Bitmap) as u32).wrapping_add(offset.0 as u32) as *mut u8,value); }
        Ok(())
    }
    fn bounds(&self) -> Bounds {
        unsafe { Bounds { minimum: Point { x: Coordinate(self.get(Field::MinX)),y: Coordinate(self.get(Field::MinY)) },
            maximum: Point { x: Coordinate(self.get(Field::MaxX)),y: Coordinate(self.get(Field::MaxY)) } } }
    }
    fn set_bounds(&mut self,bounds: Bounds) {
        unsafe { self.set(Field::MinX,bounds.minimum.x.0); self.set(Field::MinY,bounds.minimum.y.0);
            self.set(Field::MaxX,bounds.maximum.x.0); self.set(Field::MaxY,bounds.maximum.y.0); }
    }
}
impl Storage for Vm {
    fn input(&self,row: Row) -> Result<Span,Infallible> { unsafe { Ok(Span::new(self.half(0xbc,row),self.half(0x32bc,row))) } }
    fn set_input(&mut self,row: Row,span: Span) -> Result<(),Infallible> {
        unsafe { self.put_half(0xbc,row,span.first.0); self.put_half(0x32bc,row,span.last.0); }
        Ok(())
    }
    fn scratch(&self,row: Row) -> Result<Span,Infallible> { unsafe { Ok(Span::new(self.half(0x64bc,row),self.half(0x713c,row))) } }
    fn set_scratch(&mut self,row: Row,span: Span) -> Result<(),Infallible> {
        unsafe { self.put_half(0x64bc,row,span.first.0); self.put_half(0x713c,row,span.last.0); }
        Ok(())
    }
    fn merge(&mut self,row: Row,span: Span) -> Result<bool,Infallible> {
        unsafe { Ok(xm1_pipeline_image_font_raster_merge_span(self.table().0,row.0,span.first.0,span.last.0)!=0) }
    }
    fn subtract(&mut self,row: Row,span: Span) -> Result<bool,Infallible> {
        unsafe { Ok(xm1_pipeline_image_font_raster_subtract_run(self.table().0,row.0,span.first.0,span.last.0)!=0) }
    }
    fn fill_span(&mut self,row: Row,span: Span) -> Result<(),Infallible> {
        unsafe { xm1_pipeline_image_font_raster_fill_span(self.0,row.0,span.first.0,span.last.0); }
        Ok(())
    }
    fn divide(&mut self,numerator: i32,denominator: i32) -> Result<i32,Infallible> { unsafe { Ok(__aeabi_idiv(numerator,denominator)) } }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_flush_scanlines(vm: u32) -> i32 {
    unsafe {
        let mut vm=Vm(vm);
        match scanlines::render(vm.config(),&mut vm) {
            Ok(RenderStatus::Complete) => 1,
            Ok(RenderStatus::RunCapacity) => 0,
            Err(impossible) => match impossible {},
        }
    }
}
