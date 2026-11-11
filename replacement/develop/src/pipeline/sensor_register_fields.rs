use super::shared::Address32;
use super::sensor_requests::{byte, half, store, store_byte, store_half, word};
use super::sensor_registers::{xm1_pipeline_sensor_clip_signed_field, SignedFieldValue, SignedFieldWidth};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct FieldConfiguration(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PackedRegisters(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct FieldBits(u32);

unsafe fn field(output: Address32, offset: u32, shift: u32, bits: FieldBits, value: u32) {
    let mask=((1u32<<bits.0)-1)<<shift;
    unsafe { store(output,offset,(word(output,offset)&!mask)|((value<<shift)&mask)); }
}
pub(super) unsafe fn unsigned(output: Address32, offset: u32, shift: u32, bits: u32, value: u32, maximum: u32) {
    unsafe { field(output,offset,shift,FieldBits(bits),value.min(maximum)); }
}
pub(super) unsafe fn signed(output: Address32, offset: u32, shift: u32, bits: u32, value: i32) {
    let mut packed=0;
    unsafe { xm1_pipeline_sensor_clip_signed_field(SignedFieldValue(value),SignedFieldWidth(bits as i32),Address32(&mut packed as *mut u32 as u32));field(output,offset,shift,FieldBits(bits),packed); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_input(output: PackedRegisters, input: FieldConfiguration) {
    let o=output.0;let i=input.0;
    unsafe {
        unsigned(o,0,0,1,byte(i,0) as u32,1);unsigned(o,0,16,2,byte(i,1) as u32,2);unsigned(o,0,24,1,byte(i,2) as u32,1);
        unsigned(o,4,0,7,byte(i,4) as u32,81);store_byte(o,5,byte(i,5));unsigned(o,4,16,14,half(i,6) as u32,0x3fff);
        unsigned(o,8,0,13,half(i,8) as u32,0x1fff);store_half(o,10,half(i,10));store_half(o,12,half(i,12));
        unsigned(o,16,0,9,half(i,14) as u32,256);unsigned(o,16,16,9,half(i,16) as u32,256);unsigned(o,20,0,9,half(i,18) as u32,256);store_byte(o,22,byte(i,20));
        unsigned(o,24,0,14,half(i,24) as u32,0x3fff);unsigned(o,24,16,14,half(i,22) as u32,0x3fff);
        for index in 0..4 { store_half(o,30+index*4,half(i,26+index*4)); }
        for index in 0..4 { signed(o,28+index*4,0,14,half(i,28+index*4) as i16 as i32); }
        field(o,44,0,FieldBits(15),half(i,42) as u32);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_origin(output: PackedRegisters, input: FieldConfiguration) {
    let o=output.0;let i=input.0;
    unsafe {
        for bank in 0..2 {
            let source=bank*9;let destination=bank*12;
            for index in 0..4 { unsigned(o,destination,8*index,1,byte(i,source+index) as u32,1); }
            store_byte(o,destination+4,byte(i,source+4));
            for index in 0..4 { unsigned(o,destination+8,index*8,6,byte(i,source+5+index) as u32,63); }
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_secondary_table(output: PackedRegisters, input: FieldConfiguration) {
    let o=output.0;let i=input.0;
    unsafe {
        unsigned(o,0,0,1,byte(i,0) as u32,1);
        for index in 0..4 { store_byte(o,4+index,byte(i,1+index)); }
        store_byte(o,8,byte(i,5));
        for index in 0..8 { unsigned(o,12+(index/4)*4,(index%4)*8,5,byte(i,6+index) as u32,31); }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_prepare_primary_table(input: FieldConfiguration) {
    let i=input.0;let mut source=Address32(unsafe { word(i,0xcec) });
    unsafe {
        for bank in 0..20 {
            let output=i.offset(super::shared::ByteOffset(0x1a+bank*0xa4));
            for index in 0..40 { store_half(output,index*4+2,half(source,0));source=source.offset(super::shared::ByteOffset(2));store_half(output,index*4,half(source,0));source=source.offset(super::shared::ByteOffset(2)); }
            store_half(output,162,half(source,0));source=source.offset(super::shared::ByteOffset(2));store_half(output,160,0);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_primary_table(output: PackedRegisters, input: FieldConfiguration) {
    let o=output.0;let i=input.0;
    unsafe {
        unsigned(o,0,0,1,byte(i,0) as u32,1);
        for index in 0..2 { unsigned(o,4,index*16,11,half(i,2+index*2) as u32,1024);unsigned(o,8,index*16,14,half(i,6+index*2) as u32,0x3fff);unsigned(o,12,index*16,10,half(i,10+index*2) as u32,1023); }
        signed(o,16,0,8,(byte(i,14) as i8 as i32).min(0));
        for index in 0..3 { store_byte(o,17+index,byte(i,15+index)); }
        store_byte(o,20,byte(i,18));unsigned(o,24,0,10,half(i,20) as u32,1023);store_byte(o,28,byte(i,22));store_byte(o,29,byte(i,23).min(128));signed(o,28,16,10,(half(i,24) as i16 as i32).clamp(-512,128));
        for bank in 0..20 { for index in 0..41 { let source=26+bank*164+index*4;let destination=32+bank*164+index*4;signed(o,destination,16,11,half(i,source) as i16 as i32);signed(o,destination,0,11,half(i,source+2) as i16 as i32); } }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_apply_primary_table(input: FieldConfiguration, output: PackedRegisters) {
    unsafe { xm1_pipeline_sensor_prepare_primary_table(input);xm1_pipeline_sensor_pack_primary_table(output,input); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_dimensions(output: PackedRegisters, input: FieldConfiguration) {
    let o=output.0;let i=input.0;
    unsafe {
        unsigned(o,0,0,2,byte(i,0) as u32,2);field(o,0,8,FieldBits(1),byte(i,1) as u32);
        for index in 0..10 { signed(o,4+(index/2)*4,(index%2)*16,10,half(i,2+index*2) as i16 as i32); }
        for index in 0..3 { store_byte(o,24+index,byte(i,22+index)); }
        for index in 0..4 { unsigned(o,28,index*8,6,byte(i,25+index) as u32,63); }
        for (destination,source,count) in [(32,29,2),(36,31,4),(40,35,2),(44,37,1),(56,43,3),(60,46,4),(64,50,4)] { for index in 0..count { store_byte(o,destination+index,byte(i,source+index)); } }
        for index in 0..2 { unsigned(o,48,index*16,13,half(i,38+index*2) as u32,8191); }
        unsigned(o,52,0,4,byte(i,42) as u32,15);store_byte(o,70,byte(i,55));store_byte(o,68,byte(i,54));
        for index in 0..10 { unsigned(o,72+(index/4)*4,(index%4)*8,5,byte(i,56+index) as u32,31); }
        store_byte(o,84,byte(i,66));unsigned(o,84,16,6,byte(i,67) as u32,63);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_region(output: PackedRegisters, input: FieldConfiguration) {
    let o=output.0;let i=input.0;
    unsafe {
        for index in 0..7 { unsigned(o,0,index*4,1,byte(i,index) as u32,1); }
        unsigned(o,0,28,3,byte(i,7) as u32,4);
        for index in 0..15 { let destination=4+index*2;signed(o,destination&!3,(destination&3)*8,11,half(i,8+index*2) as i16 as i32); }
        for index in 0..4 { let destination=36+index*2;signed(o,destination&!3,(destination&3)*8,13,half(i,38+index*2) as i16 as i32); }
        for index in 0..5 { store_byte(o,44+index,byte(i,46+index)); }
        for index in 0..5 { store_byte(o,48+index*4,byte(i,50+index*4));unsigned(o,48+index*4,16,10,half(i,52+index*4) as u32,1023); }
        for index in 0..2 { let destination=68+index*2;signed(o,destination&!3,(destination&3)*8,10,half(i,70+index*2) as i16 as i32); }
        unsigned(o,72,0,11,half(i,74) as u32,2047);
        for index in 0..2 { signed(o,76,index*16,13,half(i,76+index*2) as i16 as i32);signed(o,80,index*16,10,half(i,80+index*2) as i16 as i32); }
        unsigned(o,84,0,11,half(i,84) as u32,2047);
        for index in 0..2 { signed(o,88,index*16,13,half(i,86+index*2) as i16 as i32);unsigned(o,92,index*16,5,byte(i,90+index) as u32,31); }
        for index in 0..4 { store_byte(o,96+index,byte(i,92+index)); }
        for index in 0..5 { signed(o,100+index*4,0,11,half(i,96+index*4) as i16 as i32);unsigned(o,100+index*4,16,10,half(i,98+index*4) as u32,1023); }
        for index in 0..3 { store_byte(o,120+index,byte(i,116+index)); }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_coefficients(output: PackedRegisters, input: FieldConfiguration) {
    let o=output.0;let i=input.0;
    unsafe {
        for index in 0..3 { unsigned(o,0,index*8,1,byte(i,index) as u32,1); }
        for index in 0..8 { unsigned(o,4+(index/4)*4,(index%4)*8,5,byte(i,3+index) as u32,31); }
        store_byte(o,12,byte(i,11));store_byte(o,13,byte(i,12));
        for index in 0..3 { let destination=16+index*2;signed(o,destination&!3,(destination&3)*8,9,half(i,14+index*4) as i16 as i32);signed(o,destination&!3,(destination&3)*8+9,6,byte(i,16+index*4) as i8 as i32); }
        for index in 0..10 { store_byte(o,24+index,byte(i,26+index)); }
        for index in 0..3 { let destination=36+index*2;store_byte(o,destination,byte(i,36+index*2));signed(o,destination&!3,(destination&3)*8+8,7,byte(i,37+index*2) as i8 as i32); }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_control(output: PackedRegisters, input: FieldConfiguration) {
    let o=output.0;let i=input.0;
    unsafe {
        unsigned(o,0,0,2,byte(i,0) as u32,2);unsigned(o,0,8,5,byte(i,1) as u32,16);unsigned(o,0,16,3,byte(i,2) as u32,7);
        for index in 0..3 { unsigned(o,4,index*8,6,byte(i,3+index) as u32,32); }signed(o,4,24,6,byte(i,6) as i8 as i32);
        for index in 0..22 { unsigned(o,8+(index/2)*4,(index%2)*16,13,half(i,8+index*2) as u32,8191); }
        for index in 0..9 { unsigned(o,52+(index/8)*4,(index%8)*4,3,byte(i,52+index) as u32,7); }
        for index in 0..28 { unsigned(o,60+(index/2)*4,(index%2)*16,14,half(i,62+index*2) as u32,0x3fff); }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_filter_small(output: PackedRegisters, input: FieldConfiguration) {
    let o=output.0;let i=input.0;
    unsafe { for index in 0..3 { unsigned(o,48,index*8,5,byte(i,index) as u32,16); }store_byte(o,52,(half(i,4) as u32).min(255) as u8);store_byte(o,54,(half(i,6) as u32).min(255) as u8);unsigned(o,56,0,1,byte(i,8) as u32,1); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_filter_head(output: PackedRegisters, input: FieldConfiguration) {
    let o=output.0;let i=input.0;
    unsafe {
        for index in 0..10 { let (offset,shift)=if index<6 { ((index/3)*4,(index%3)*8) } else { (8,(index-6)*8) };unsigned(o,offset,shift,3,byte(i,index) as u32,7); }
        signed(o,12,0,12,half(i,10) as i16 as i32);signed(o,12,16,12,half(i,12) as i16 as i32);unsigned(o,16,0,12,half(i,14) as u32,4095);unsigned(o,16,16,3,byte(i,16) as u32,7);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_filter_bytes(output: PackedRegisters, input: FieldConfiguration) {
    let o=output.0;let i=input.0;
    unsafe {
        for index in 0..13 { let (offset,shift)=match index { 0..=3=>(20,index*8),4..=6=>(24,(index-4)*8),7..=8=>(28,(index-7)*8),_=>(32,(index-9)*8) };unsigned(o,offset,shift,5,byte(i,18+index) as u32,31); }
        for (destination,source) in [(36,32),(40,34),(42,36)] { store_half(o,destination,half(i,source)); }
        for index in 0..3 { unsigned(o,44,index*8,5,byte(i,40-index) as u32,31); }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_filter_flags(output: PackedRegisters, input: FieldConfiguration) {
    unsafe { for index in 0..4 { unsigned(output.0,60,(3-index)*8,1,byte(input.0,41+index) as u32,1); } }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_filter_coefficients(output: PackedRegisters, input: FieldConfiguration) {
    unsafe { for bank in 0..3 { for index in 0..3 { signed(output.0,64+bank*8+(index/2)*4,(index%2)*16,13,half(input.0,46+bank*6+index*2) as i16 as i32); } } }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_filter_curves(output: PackedRegisters, input: FieldConfiguration) {
    let o=output.0;let i=input.0;
    unsafe {
        for index in 0..9 { unsigned(o,88+(index/8)*4,(index%8)*4,3,byte(i,64+index) as u32,7); }
        for index in 0..28 { unsigned(o,96+(index/2)*4,(index%2)*16,14,half(i,74+index*2) as u32,0x3fff); }
        unsigned(o,152,16,14,half(i,130) as u32,0x3fff);unsigned(o,152,0,14,half(i,132) as u32,0x3fff);
        for index in 0..56 { unsigned(o,156+(index/2)*4,(index%2)*16,14,half(i,134+index*2) as u32,0x3fff); }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_first_filter(output: PackedRegisters, input: FieldConfiguration) {
    unsafe { xm1_pipeline_sensor_pack_filter_head(output,input);xm1_pipeline_sensor_pack_filter_bytes(output,input);xm1_pipeline_sensor_pack_filter_small(output,input);xm1_pipeline_sensor_pack_filter_flags(output,input);xm1_pipeline_sensor_pack_filter_coefficients(output,input);xm1_pipeline_sensor_pack_filter_curves(output,input); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_window(output: PackedRegisters, input: FieldConfiguration) {
    let o=output.0;let i=input.0;
    unsafe {
        field(o,0,0,FieldBits(1),byte(i,0) as u32);unsigned(o,0,16,4,byte(i,1) as u32,15);unsigned(o,0,20,4,byte(i,2) as u32,15);
        for index in 0..2 { unsigned(o,4,index*16,13,half(i,4+index*2) as u32,8191); }
        unsigned(o,8,0,13,half(i,8) as u32,8191);
        for index in 0..2 { unsigned(o,8,16+index*8,6,(half(i,10+index*2) as u32).max(1),63); }
        for index in 0..56 { signed(o,12+(index/2)*4,(index%2)*16,10,half(i,14+index*2) as i16 as i32); }
        unsigned(o,124,0,6,half(i,126) as u32,63);
    }
}
