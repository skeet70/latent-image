use xm1_saclac_interface::ClientCallback;
use crate::statistics_results::{Delivery, ResultWords};

pub static S1_REPORTS: crate::statistics_results::ReportingGate = crate::statistics_results::ReportingGate::new(true);

pub struct NativeDelivery;
impl Delivery for NativeDelivery {
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
