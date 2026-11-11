#[cfg(target_arch = "arm")]
#[no_mangle]
pub unsafe extern "C" fn xm1_power_stop_usb_and_display() -> u32 {
    use crate::shutdown::{QueueId, RequestId, ServiceMessage, ShutdownRtos, TaskId};
    let rtos = &mut crate::shutdown_rtos::NativeShutdownRtos;
    let completion = xm1_power_usb_stop_callback as usize as u32;
    let _ = rtos.post(ServiceMessage {
        operation: RequestId(0x1706), sender: TaskId(2), destination: QueueId(7),
        payload: &completion.to_le_bytes(),
    });
    let display = crate::shutdown_requests::OwnedCompletionAddress::display();
    let status = unsafe { display_scanout_panel::panel::xm1_scanout_panel_command(0, display.word()) };
    if status == 0 { 0 } else { 0xff }
}

#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn xm1_power_usb_stop_callback() -> i32 {
    use crate::shutdown::{QueueId, RequestId, ServiceMessage, ShutdownRtos};
    let rtos = &mut crate::shutdown_rtos::NativeShutdownRtos;
    let sender = rtos.current_task();
    match rtos.post(ServiceMessage { operation: RequestId(0x9706), sender, destination: QueueId(2), payload: &[] }) {
        Ok(()) => 0,
        Err(error) => error.0,
    }
}

#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn xm1_power_terminal_action() -> ! {
    let mut power = crate::power_serial::SerialTerminalPower {
        controller: crate::power_serial::NativeSerialController,
        configuration: crate::power_serial::NativePowerConfigurationSource,
    };
    crate::primitives::terminal(&mut power, &mut crate::primitives::NativeGpio)
}
