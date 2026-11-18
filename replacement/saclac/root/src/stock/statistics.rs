use xm1_saclac_interface::ClientCallback;
use crate::statistics_results::{Delivery, ResultWords};

pub static S1_REPORTS: crate::statistics_results::ReportingGate = crate::statistics_results::ReportingGate::new(true);
pub static PIPELINE_PROGRESS: crate::statistics_results::ReportingGate = crate::statistics_results::ReportingGate::new(false);

pub struct NativeDelivery;
impl Delivery for NativeDelivery {
    fn resolve_callback(&self, word: xm1_saclac_interface::stock::StockCallbackWord) -> xm1_saclac_interface::stock::StockCallbackWord {
        #[cfg(target_arch = "arm")]
        {
            if word.0 == 0xf2_2730 {
                xm1_saclac_interface::stock::StockCallbackWord(super::callbacks::xm1_saclac_root_s1_result as *const () as u32)
            } else { word }
        }
        #[cfg(not(target_arch = "arm"))]
        { let _ = word; panic!("native callback binding requires ARM"); }
    }
    fn report_results(&mut self, open: bool) { S1_REPORTS.publish(open); }
    fn enter(&mut self, callback: ClientCallback, result: ResultWords) {
        #[cfg(target_arch = "arm")]
        unsafe {
            let target: unsafe extern "C" fn(ResultWords) = core::mem::transmute(xm1_saclac_interface::stock::callback_word(callback) as usize);
            target(result);
        }
        #[cfg(not(target_arch = "arm"))]
        {
            let _ = (callback, result);
            panic!("native statistics callback requires ARM");
        }
    }
}
