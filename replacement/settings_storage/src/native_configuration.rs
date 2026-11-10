use crate::{ConfiguredDefault, DefaultProfile, NetworkNameDefaultByte, SettingsConfiguration, StartupResavePolicy};
use crate::table::ProfileSelector;

pub struct NativeConfiguration { _exclusive: () }
impl NativeConfiguration {
    /// # Safety
    /// The X-M1 configuration window and six default callbacks must remain mapped while this service exists.
    pub unsafe fn adopt() -> Self { Self { _exclusive: () } }
}
extern "C" {
    fn xm1_stock_61df44() -> u32;
    fn xm1_stock_61d8a4() -> u32;
    fn xm1_stock_61d59c() -> u32;
    fn xm1_stock_61d618() -> u32;
    fn xm1_stock_61d5f0() -> u32;
    fn xm1_stock_61d65c(selector: ProfileSelector) -> u32;
}
impl SettingsConfiguration for NativeConfiguration {
    fn network_name_default(&self) -> NetworkNameDefaultByte {
        NetworkNameDefaultByte(unsafe { core::ptr::read_volatile(0x96b538 as *const u8) })
    }
    fn startup_resave_policy(&self) -> StartupResavePolicy {
        StartupResavePolicy::from_abi(unsafe { core::ptr::read_volatile(0x96b4e8 as *const u8) })
    }
    fn default_61df44(&self) -> ConfiguredDefault { ConfiguredDefault(unsafe { xm1_stock_61df44() }) }
    fn default_61d8a4(&self) -> ConfiguredDefault { ConfiguredDefault(unsafe { xm1_stock_61d8a4() }) }
    fn default_61d59c(&self) -> ConfiguredDefault { ConfiguredDefault(unsafe { xm1_stock_61d59c() }) }
    fn default_61d618(&self) -> ConfiguredDefault { ConfiguredDefault(unsafe { xm1_stock_61d618() }) }
    fn default_61d5f0(&self) -> ConfiguredDefault { ConfiguredDefault(unsafe { xm1_stock_61d5f0() }) }
    fn default_61d65c(&self, profile: DefaultProfile) -> ConfiguredDefault {
        let profile = match profile { DefaultProfile::Two => ProfileSelector::Two, DefaultProfile::Three => ProfileSelector::Three, DefaultProfile::Four => ProfileSelector::Four };
        ConfiguredDefault(unsafe { xm1_stock_61d65c(profile) })
    }
}
