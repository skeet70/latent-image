#![forbid(unsafe_code)]

//! The configuration layer's lens requests by number, posted to lenscom through the shared
//! layer's request.

use xm1_saclac_config::services::LensRequests;
use xm1_saclac_interface::stock::StockCallbackWord;
use xm1_saclac_interface::LensCom;
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::lens_com::{self, RequestNumber};
use xm1_saclac_shared::stock::Machine;

use crate::services::Services;
use crate::stock;

/// 0x6b731c's result for a number above 10 (0x6b745c).
const UNKNOWN_NUMBER: u32 = -30i32 as u32;

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: LensCom> LensRequests for Services<M, R, E, P> {
    fn store_callback(&mut self, callback: u32) { self.machine.write_u32(stock::LENS_REQUEST_CALLBACK, callback) }

    /// 0 for every number up to 10, whether or not lenscom took the post (0x6b7454).
    fn request(&mut self, number: u8, callback: u32) -> u32 {
        let Some(number) = RequestNumber::new(u32::from(number)) else { return UNKNOWN_NUMBER };
        let callback = (callback != 0).then_some(StockCallbackWord(callback));
        lens_com::request(number, callback, &mut self.stock, &mut self.peers);
        0
    }
}
