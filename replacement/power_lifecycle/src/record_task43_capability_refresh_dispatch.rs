#![cfg(target_arch = "arm")]

extern "C" {
    fn xm1_power_record_helper_f300d0() -> u32;
    fn xm1_power_record_helper_f30230() -> u32;
    fn xm1_power_record_helper_f31674() -> u32;
    fn xm1_power_record_helper_f302e4() -> u32;
    fn xm1_power_record_helper_f3069c() -> u32;
    fn xm1_power_record_helper_f30338() -> u32;
    fn xm1_power_record_helper_f307ac() -> u32;
    fn xm1_power_record_helper_f30380() -> u32;
    fn xm1_power_record_helper_f303a8() -> u32;
    fn xm1_power_record_helper_f303e0() -> u32;
    fn xm1_power_record_helper_f30408() -> u32;
    fn xm1_power_record_helper_f3045c() -> u32;
    fn xm1_power_record_helper_f3049c() -> u32;
    fn xm1_power_record_helper_f30278() -> u32;
    fn xm1_power_record_helper_f3050c() -> u32;
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_f321b0(kind: u32) -> u32 {
    unsafe {
        match kind {
            2 => xm1_power_record_helper_f300d0(),
            3 => xm1_power_record_helper_f30230(),
            0xd => xm1_power_record_helper_f31674(),
            0x11 => xm1_power_record_helper_f302e4(),
            0x17 => xm1_power_record_helper_f3069c(),
            0x18 => xm1_power_record_helper_f30338(),
            0x1a => xm1_power_record_helper_f307ac(),
            0x1c => xm1_power_record_helper_f30380(),
            0x1d => xm1_power_record_helper_f303a8(),
            0x1e => xm1_power_record_helper_f303e0(),
            0x1f => xm1_power_record_helper_f30408(),
            0x20 => xm1_power_record_helper_f3045c(),
            0x21 => xm1_power_record_helper_f3049c(),
            0x23 => xm1_power_record_helper_f30278(),
            0x26 => xm1_power_record_helper_f3050c(),
            _ => 1,
        }
    }
}
