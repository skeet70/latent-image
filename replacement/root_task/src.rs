#![no_std]

use core::panic::PanicInfo;

#[repr(transparent)]
struct ResourceId(u32);
#[repr(u32)]
enum ResourceMode {
    One = 1,
}
#[repr(u32)]
enum PhaseMarker {
    Initial = 0x01010000,
    Later = 0x11010000,
}
#[repr(transparent)]
struct DataAddress(u32);
#[repr(transparent)]
struct RetainedStatus(u32);
#[repr(transparent)]
struct InitStatus(u32);
#[repr(transparent)]
struct RootMode(u32);
#[repr(u32)]
enum InitOption {
    One = 1,
}
#[repr(u32)]
enum ConfigKey {
    Index31 = 31,
    Index32 = 32,
    Index34 = 34,
}
#[repr(transparent)]
struct ConfigValue(u32);
#[repr(u32)]
enum ConfigOption {
    Zero = 0,
}
#[repr(transparent)]
struct PeriodTicks(u32);
#[repr(u32)]
enum CallbackMode {
    Event = 0,
}
#[repr(transparent)]
struct CallbackHandler(u32);
#[repr(transparent)]
struct TokenOutput(u32);
#[repr(u32)]
enum CleanupTarget {
    Current = 0,
}
#[repr(transparent)]
pub struct CleanupStatus(i32);

extern "C" {
    fn root_platform_init();
    fn root_runtime_init();
    fn root_mark(marker: PhaseMarker);
    fn root_install_table(table: DataAddress);
    fn root_create_resource(id: ResourceId, mode: ResourceMode) -> RetainedStatus;
    fn root_create_workers();
    fn root_init_676054();
    fn root_init_662e48();
    fn root_init_665224();
    fn root_init_649070();
    fn root_init_6490dc(option: InitOption);
    fn root_checked_init_a() -> InitStatus;
    fn root_checked_init_b() -> InitStatus;
    fn root_init_69f204();
    fn root_init_656788();
    fn root_init_7262cc();
    fn root_install_descriptor(descriptor: DataAddress);
    fn root_init_6a5634();
    fn root_config_write(key: ConfigKey, value: ConfigValue, option: ConfigOption);
    fn root_mode() -> RootMode;
    fn root_config_query(key: ConfigKey) -> ConfigValue;
    fn root_init_6a564c();
    fn root_init_6a5c08();
    fn root_init_6568ec();
    fn root_submit_later();
    fn xm1_app_register_callback(
        period: PeriodTicks,
        mode: CallbackMode,
        callback: CallbackHandler,
        token: TokenOutput,
    ) -> RetainedStatus;
    fn root_cleanup(target: CleanupTarget) -> CleanupStatus;
    fn root_periodic_callback();
    static root_periodic_context: u8;
}

#[inline(always)]
fn halt() -> ! {
    loop {}
}

/// # Safety
/// Entry requires an aligned allocated A32 task stack and retained AAPCS services with immutable code and nonoverlapping scratch.
#[no_mangle]
#[link_section = ".text.xm1_root_task"]
pub unsafe extern "C" fn xm1_root_task() -> CleanupStatus {
    unsafe {
        root_platform_init();
        root_runtime_init();
        root_mark(PhaseMarker::Initial);
        root_install_table(DataAddress(0x72835c));

        let mut resource = ResourceId(1);
        while resource.0 != 98 {
            let _ = root_create_resource(ResourceId(resource.0), ResourceMode::One);
            resource.0 += 1;
        }

        root_create_workers();
        root_init_676054();
        root_init_662e48();
        root_init_665224();
        root_init_649070();
        root_init_6490dc(InitOption::One);
        if root_checked_init_a().0 != 0 {
            halt();
        }
        if root_checked_init_b().0 != 0 {
            halt();
        }

        root_init_69f204();
        root_init_656788();
        root_init_7262cc();
        root_install_descriptor(DataAddress(0x726478));
        root_init_6a5634();
        root_config_write(ConfigKey::Index34, ConfigValue(0), ConfigOption::Zero);

        if root_mode().0 == 0 {
            if root_config_query(ConfigKey::Index34).0 != 0 {
                root_config_write(ConfigKey::Index34, ConfigValue(0), ConfigOption::Zero);
                root_config_write(ConfigKey::Index31, ConfigValue(60), ConfigOption::Zero);
                root_config_write(ConfigKey::Index32, ConfigValue(0), ConfigOption::Zero);
                root_config_write(ConfigKey::Index34, ConfigValue(1), ConfigOption::Zero);
            }
            root_init_6a564c();
            root_init_6a5c08();
            root_init_6568ec();
            halt();
        }

        root_submit_later();
        let _ = xm1_app_register_callback(
            PeriodTicks(30),
            CallbackMode::Event,
            CallbackHandler(root_periodic_callback as *const () as u32),
            TokenOutput(core::ptr::addr_of!(root_periodic_context) as u32),
        );
        root_mark(PhaseMarker::Later);
        root_cleanup(CleanupTarget::Current)
    }
}

#[panic_handler]
fn panic(_: &PanicInfo<'_>) -> ! {
    halt()
}
