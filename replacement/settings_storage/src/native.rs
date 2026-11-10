#[path = "../../property_service/src/lib.rs"]
mod property;

#[no_mangle]
pub unsafe extern "C" fn xm1_getter_owned(id: u32, bank: u32, defaults: u32, selection: u32) -> u32 {
    unsafe { property::xm1_property_get(id, bank, defaults, selection) }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_setter_owned(id: u32, value: u32, bank: u32, selection: u32) -> u32 {
    unsafe { property::xm1_property_set(id, value, bank, selection) }
}
