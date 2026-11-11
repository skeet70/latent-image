use crate::shutdown::{KernelStatus, ShutdownRtos, TaskId};
use crate::shutdown_requests::{self, DevicePhase, OwnedCompletionAddress};
use crate::shutdown_task7_8::{self, UsbCompletionWord};
use crate::settings_shutdown_actor::{self, CallbackToken};
use crate::storage_shutdown_actor::{self, StorageCompletion};

pub struct StartupCallbacks {
    pub display: OwnedCompletionAddress,
    pub task7: OwnedCompletionAddress,
    pub device: OwnedCompletionAddress,
    pub settings: OwnedCompletionAddress,
    pub storage: OwnedCompletionAddress,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StartupObservation {
    pub display: Result<(), KernelStatus>,
    pub task7: Result<(), KernelStatus>,
    pub device: Result<(), KernelStatus>,
    pub controller: Result<(), KernelStatus>,
    pub settings: Result<(), KernelStatus>,
    pub storage: Result<(), KernelStatus>,
}

pub fn start_after_display(
    rtos: &mut impl ShutdownRtos,
    callbacks: &StartupCallbacks,
    device_phase: DevicePhase,
    storage_scratch: &mut [u8; 8],
    display: Result<(), KernelStatus>,
) -> StartupObservation {
    let task7 = shutdown_task7_8::task7_shutdown(rtos, TaskId(2), UsbCompletionWord(callbacks.task7.word()));
    let device = shutdown_requests::device_shutdown(rtos, device_phase, callbacks.device);
    let controller = shutdown_requests::controller_shutdown(rtos);
    let settings = settings_shutdown_actor::post_release(rtos, CallbackToken(callbacks.settings.word()));
    let storage = storage_shutdown_actor::post_shutdown(rtos, StorageCompletion(callbacks.storage.word()), storage_scratch)
        .map_err(|error| KernelStatus(error.abi_status()));
    StartupObservation { display, task7, device, controller, settings, storage }
}

#[cfg(target_arch = "arm")]
pub unsafe fn start_native(storage_scratch: &mut [u8; 8]) -> Result<StartupObservation, KernelStatus> {
    let failure = |error: crate::callback_registry::CallbackError| KernelStatus(crate::callback_registry::refused(error));
    OwnedCompletionAddress::task8().map_err(failure)?;
    let callbacks = StartupCallbacks {
        display: OwnedCompletionAddress::display(),
        task7: OwnedCompletionAddress::task7().map_err(failure)?,
        device: OwnedCompletionAddress::device(crate::callback_registry::registry(), crate::shutdown_requests::xm1_power_device_shutdown_callback).map_err(failure)?,
        settings: OwnedCompletionAddress::settings().map_err(failure)?,
        storage: OwnedCompletionAddress::storage(),
    };
    let display = unsafe { display_scanout_panel::panel::xm1_scanout_panel_command(6, callbacks.display.word()) };
    let display = if display == 0 { Ok(()) } else { Err(KernelStatus(-255)) };
    let device_phase = DevicePhase(unsafe { core::ptr::read_volatile(0x4089ea5c as *const u32) });
    Ok(start_after_display(&mut crate::shutdown_rtos::NativeShutdownRtos, &callbacks, device_phase, storage_scratch, display))
}
