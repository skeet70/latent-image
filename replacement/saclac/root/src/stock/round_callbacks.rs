#![cfg(target_arch = "arm")]

use xm1_saclac_interface::{Completion, stock::StockCallbackWord};
use xm1_saclac_shared::stock::{Guest, Machine};
use crate::completion_callbacks::post;

fn trace(word: u32) {
    if Guest.enter(0x69_f8fc, &[0x24e]) == 1 { Guest.enter(0x72_872c, &[word as usize]); }
}

#[no_mangle]
pub extern "C" fn xm1_saclac_root_round_search_mode(status: u32) {
    post(&crate::peer_context::Poster::new(xm1_kernel::SlotTable), Completion::Image45, &status.to_le_bytes()).expect("round search-mode completion");
}

fn pair(completion: Completion, first: u32, second: u32, marker: u32) {
    let mut bytes = [0; 8];
    bytes[..4].copy_from_slice(&first.to_le_bytes());
    bytes[4..].copy_from_slice(&second.to_le_bytes());
    trace(marker);
    post(&crate::peer_context::Poster::new(xm1_kernel::SlotTable), completion, &bytes).expect("round pair completion");
}

/// 0xf3a3f8: calc's status and its result record, collected by 0xf3a4c0.
#[no_mangle]
pub extern "C" fn xm1_saclac_root_round_window(status: u32, result: u32) {
    pair(Completion::Calculation8d, status, result, 0x1110_7000);
}

#[no_mangle]
pub extern "C" fn xm1_saclac_root_round_auxiliary(first: u32, second: u32) {
    pair(Completion::Calculation8a, first, second, 0x1110_4000);
}

#[no_mangle]
pub extern "C" fn xm1_saclac_root_round_source(source: u32) {
    let bytes = crate::stock::acquisition::read_record::<6>(&Guest, xm1_saclac_interface::calc::SharedAddress(source));
    trace(0x1110_5000);
    post(&crate::peer_context::Poster::new(xm1_kernel::SlotTable), Completion::Calculation9a, &bytes).expect("round source completion");
}

pub fn callbacks() -> crate::round_services::Callbacks {
    crate::round_services::Callbacks {
        search_mode: StockCallbackWord(xm1_saclac_root_round_search_mode as *const () as u32),
        window: StockCallbackWord(xm1_saclac_root_round_window as *const () as u32),
        auxiliary: StockCallbackWord(xm1_saclac_root_round_auxiliary as *const () as u32),
        source: StockCallbackWord(xm1_saclac_root_round_source as *const () as u32),
        image: StockCallbackWord(super::callbacks::xm1_saclac_root_image_finished as *const () as u32),
        snapshot: StockCallbackWord(super::callbacks::xm1_saclac_root_snapshot_finished as *const () as u32),
    }
}
