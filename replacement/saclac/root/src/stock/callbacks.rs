#[cfg(target_arch = "arm")]
use xm1_saclac_shared::stock::{Guest, Machine};

#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn xm1_saclac_root_primary_end(status: u32, result: u32) {
    let machine = Guest;
    let words = [machine.read_u32(result), machine.read_u32(result + 4), machine.read_u32(result + 8)];
    let poster = crate::peer_context::Poster::new(xm1_kernel::SlotTable);
    crate::completion_callbacks::primary_end(&poster, status, words).expect("primary end completion post");
}

#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn xm1_saclac_root_primary_notice(status: u32) {
    crate::completion_callbacks::primary_notice(&Guest, status);
}

#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn xm1_saclac_root_image_finished(status: u32) {
    let mut payload = [0; 8];
    payload[..4].copy_from_slice(&status.to_le_bytes());
    let poster = crate::peer_context::Poster::new(xm1_kernel::SlotTable);
    crate::completion_callbacks::post(&poster, xm1_saclac_interface::Completion::Image47, &payload).expect("image completion post");
}

#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn xm1_saclac_root_snapshot_finished(status: u32) {
    let poster = crate::peer_context::Poster::new(xm1_kernel::SlotTable);
    crate::completion_callbacks::post(&poster, xm1_saclac_interface::Completion::Image66, &status.to_le_bytes()).expect("snapshot completion post");
}

#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn xm1_saclac_root_lens_status(position: u16, mode: u32) {
    let poster = crate::peer_context::Poster::new(xm1_kernel::SlotTable);
    crate::completion_callbacks::lens_status(&poster, position, mode).expect("lens status completion post");
}

#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn xm1_saclac_root_lens_corrected() {
    let poster = crate::peer_context::Poster::new(xm1_kernel::SlotTable);
    crate::completion_callbacks::post(&poster, xm1_saclac_interface::Completion::Lens37, &[]).expect("lens correction completion post");
}

#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn xm1_saclac_root_s1_result(value: u32) {
    let poster = crate::peer_context::Poster::new(xm1_kernel::SlotTable);
    crate::completion_callbacks::s1_result(&poster, &crate::stock::statistics::S1_REPORTS, value).expect("S1 result completion post");
}

#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn xm1_saclac_root_adjustment_synchronized() {
    let poster = crate::peer_context::Poster::new(xm1_kernel::SlotTable);
    crate::peer_services::signal(&Guest, &poster, crate::peer_services::Rendezvous::Adjustment).expect("adjustment waiter signal");
}

#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn xm1_saclac_root_lens_synchronized() {
    let poster = crate::peer_context::Poster::new(xm1_kernel::SlotTable);
    crate::peer_services::signal(&Guest, &poster, crate::peer_services::Rendezvous::Lens).expect("lens waiter signal");
}
