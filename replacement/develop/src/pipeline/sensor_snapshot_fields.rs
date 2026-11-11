use super::sensor_register_fields::{signed, unsigned, FieldConfiguration, PackedRegisters};
use super::sensor_requests::{byte, half, store_byte, store_half};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_snapshot_base(output: PackedRegisters,input: FieldConfiguration) {
    let o=output.0;let i=input.0;
    unsafe {
        unsigned(o,0,0,6,byte(i,0) as u32,45);
        for (source,shift) in [(1,8),(2,12),(3,16),(4,20),(5,24)] { unsigned(o,0,shift,1,byte(i,source) as u32,1); }
        for (source,shift) in [(6,4),(7,8),(8,12),(9,16),(10,20),(11,22),(12,24),(13,28)] { unsigned(o,4,shift,1,byte(i,source) as u32,1); }
        unsigned(o,4,30,2,byte(i,14) as u32,3);unsigned(o,8,0,1,byte(i,15) as u32,1);unsigned(o,8,8,1,byte(i,17) as u32,1);unsigned(o,8,4,2,byte(i,16) as u32,3);unsigned(o,8,12,2,byte(i,18) as u32,2);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_snapshot_window(output: PackedRegisters,input: FieldConfiguration) {
    let o=output.0;let i=input.0;
    unsafe {
        unsigned(o,12,0,1,byte(i,19) as u32,1);unsigned(o,12,16,1,byte(i,20) as u32,1);unsigned(o,16,0,7,byte(i,21) as u32,81);store_byte(o,18,byte(i,22));unsigned(o,20,0,13,half(i,24) as u32,8191);store_half(o,22,half(i,26));store_half(o,24,half(i,28));unsigned(o,24,16,14,half(i,30) as u32,16383);
        unsigned(o,28,0,9,half(i,32) as u32,256);unsigned(o,28,16,9,half(i,34) as u32,256);unsigned(o,32,0,9,half(i,36) as u32,256);store_byte(o,34,byte(i,38));unsigned(o,36,0,14,half(i,40) as u32,16383);unsigned(o,36,16,14,half(i,42) as u32,16383);
        for index in 0..4 { signed(o,40+index*4,0,14,half(i,44+index*4) as i16 as i32);store_half(o,42+index*4,half(i,46+index*4)); }
        unsigned(o,56,0,15,half(i,60) as u32,32767);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_snapshot_tail_controls(output: PackedRegisters,input: FieldConfiguration) {
    let o=output.0;let i=input.0;
    unsafe {
        for index in 0..3 { unsigned(o,68,index*8,4,byte(i,68+index) as u32,15); }
        for index in 0..4 { store_byte(o,72+index,byte(i,71+index)); }
        unsigned(o,76,0,3,byte(i,75) as u32,7);unsigned(o,76,8,3,byte(i,76) as u32,7);store_byte(o,78,byte(i,77));store_byte(o,79,byte(i,78));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_snapshot_pairs(output: PackedRegisters,input: FieldConfiguration) {
    let o=output.0;let i=input.0;
    unsafe {
        for index in 0..3 { unsigned(o,80,index*8,6,byte(i,79+index) as u32,32); }signed(o,80,24,6,byte(i,82) as i8 as i32);
        for index in 0..24 { unsigned(o,84+(index/2)*4,(index%2)*16,13,half(i,84+index*2) as u32,8191); }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_snapshot_signed(output: PackedRegisters,input: FieldConfiguration) {
    unsafe { for bank in 0..3 { for index in 0..3 { signed(output.0,128+bank*8+(index/2)*4,(index%2)*16,13,half(input.0,128+bank*6+index*2) as i16 as i32); } } }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_snapshot_curves(output: PackedRegisters,input: FieldConfiguration) {
    unsafe {
        for index in 0..9 { unsigned(output.0,152+(index/8)*4,(index%8)*4,3,byte(input.0,146+index) as u32,7); }
        for index in 0..30 { unsigned(output.0,160+(index/2)*4,(index%2)*16,14,half(input.0,156+index*2) as u32,16383); }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_snapshot_flags(output: PackedRegisters,input: FieldConfiguration) {
    unsafe { for index in 0..3 { unsigned(output.0,220,index*8,4,byte(input.0,216+index) as u32,15); } }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_snapshot_coefficients(output: PackedRegisters,input: FieldConfiguration) {
    unsafe { for index in 0..4 { signed(output.0,224+(index/2)*4,(index%2)*16,13,half(input.0,220+index*2) as i16 as i32); } }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_snapshot_byte_pairs(output: PackedRegisters,input: FieldConfiguration) {
    unsafe {
        for index in 0..4 { store_byte(output.0,232+index,byte(input.0,228+index)); }
        for index in 0..5 { store_byte(output.0,236+index*4,byte(input.0,232+index*4));unsigned(output.0,236+index*4,16,10,half(input.0,234+index*4) as u32,1023); }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_snapshot_signed_pairs(output: PackedRegisters,input: FieldConfiguration) {
    let o=output.0;let i=input.0;
    unsafe {
        for bank in 0..2 {
            for index in 0..2 { signed(o,256+bank*12,index*16,10,half(i,252+bank*10+index*2) as i16 as i32); }
            unsigned(o,260+bank*12,0,11,half(i,256+bank*10) as u32,2047);
            for index in 0..2 { signed(o,264+bank*12,index*16,13,half(i,258+bank*10+index*2) as i16 as i32); }
        }
        unsigned(o,280,0,5,byte(i,272) as u32,31);unsigned(o,280,16,5,byte(i,273) as u32,31);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_snapshot_curve_pairs(output: PackedRegisters,input: FieldConfiguration) {
    unsafe {
        for index in 0..4 { store_byte(output.0,284+index,byte(input.0,274+index)); }
        for index in 0..5 { signed(output.0,288+index*4,0,11,half(input.0,278+index*4) as i16 as i32);unsigned(output.0,288+index*4,16,10,half(input.0,280+index*4) as u32,1023); }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_snapshot_sixbit(output: PackedRegisters,input: FieldConfiguration) {
    unsafe { for index in 0..4 { unsigned(output.0,308,index*8,6,byte(input.0,298+index) as u32,63); } }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_snapshot_tail(output: PackedRegisters,input: FieldConfiguration) {
    let o=output.0;let i=input.0;
    unsafe {
        for index in 0..12 { signed(o,312+(index/2)*4,(index%2)*16,10,half(i,302+index*2) as i16 as i32); }
        for index in 0..3 { store_byte(o,336+index,byte(i,326+index)); }
        for index in 0..4 { unsigned(o,340,index*8,6,byte(i,329+index) as u32,63); }
        for index in 0..2 { store_byte(o,344+index,byte(i,333+index)); }
        store_byte(o,348,byte(i,335));store_byte(o,350,byte(i,337));signed(o,348,8,8,byte(i,336) as i8 as i32);signed(o,348,24,8,byte(i,338) as i8 as i32);
        store_byte(o,352,byte(i,339));signed(o,352,8,8,byte(i,340) as i8 as i32);store_byte(o,356,byte(i,341));
        for index in 0..6 { unsigned(o,360+(index/4)*4,(index%4)*8,5,byte(i,342+index) as u32,31); }
        store_byte(o,368,byte(i,348));unsigned(o,368,16,6,byte(i,349) as u32,63);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_snapshot_bytes(output: PackedRegisters,input: FieldConfiguration) {
    let o=output.0;let i=input.0;
    unsafe {
        for index in 0..2 { store_byte(o,372+index,byte(i,350+index).min(128)); }
        store_byte(o,374,byte(i,352));store_byte(o,376,byte(i,353));unsigned(o,376,16,4,byte(i,354) as u32,15);unsigned(o,376,24,4,byte(i,355) as u32,15);
        for index in 0..3 { store_byte(o,380+index,byte(i,356+index)); }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_pack_snapshot(output: PackedRegisters,input: FieldConfiguration) {
    unsafe {
        xm1_pipeline_sensor_pack_snapshot_base(output,input);xm1_pipeline_sensor_pack_snapshot_window(output,input);xm1_pipeline_sensor_pack_snapshot_tail_controls(output,input);xm1_pipeline_sensor_pack_snapshot_pairs(output,input);xm1_pipeline_sensor_pack_snapshot_signed(output,input);xm1_pipeline_sensor_pack_snapshot_curves(output,input);xm1_pipeline_sensor_pack_snapshot_flags(output,input);xm1_pipeline_sensor_pack_snapshot_coefficients(output,input);xm1_pipeline_sensor_pack_snapshot_byte_pairs(output,input);xm1_pipeline_sensor_pack_snapshot_signed_pairs(output,input);xm1_pipeline_sensor_pack_snapshot_curve_pairs(output,input);xm1_pipeline_sensor_pack_snapshot_sixbit(output,input);xm1_pipeline_sensor_pack_snapshot_tail(output,input);xm1_pipeline_sensor_pack_snapshot_bytes(output,input);
    }
}
