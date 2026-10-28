#[no_mangle]
pub extern "C" fn xm1_pilot_extra(a: u32, b: u32) -> u32 {
    a.wrapping_mul(b).wrapping_add(0x5a5a_5a5a)
}
