use super::*;
use crate::callback_registry::CallbackError;

unsafe extern "C" fn host_receive() {}
unsafe extern "C" fn host_send() {}

#[test]
fn both_stream_handlers_are_admitted_before_the_attach() {
    // `attach_irq` registers each completion through `register_completion` and then calls
    // `xm1_power_timer_irq_attach`, whose first act is this admission.
    let receive = CallbackWord(host_receive as *const () as usize);
    let send = CallbackWord(host_send as *const () as usize);
    assert_eq!(crate::charge_timers::admit_irq_handler(receive), Err(CallbackError::Unadmitted));
    let receive = register_completion(host_receive).unwrap();
    let send = register_completion(host_send).unwrap();
    assert_eq!(crate::charge_timers::admit_irq_handler(receive), Ok(()));
    assert_eq!(crate::charge_timers::admit_irq_handler(send), Ok(()));
}
