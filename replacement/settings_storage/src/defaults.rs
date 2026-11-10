use crate::{default_bytes::{SLOT_BYTES, TAIL_BYTES}, ProfileSelection, SlotRecord, RamTail};

pub struct SlotDefaultsWindow;
impl SlotDefaultsWindow {
    pub fn slot(selection: ProfileSelection) -> SlotRecord<'static> {
        let offset = selection.offset() - 0x118;
        SlotRecord(SLOT_BYTES.get(offset..offset+40).unwrap().try_into().ok().unwrap())
    }
    pub fn read<V: crate::FieldValue>(selection: ProfileSelection, field: &crate::SlotField<V>) -> V {
        let selection = if cfg!(storage_slot_defaults_moved) {
            match selection {
                ProfileSelection::WrappedPredecessor => ProfileSelection::Record0,
                ProfileSelection::Record0 => ProfileSelection::Current,
                ProfileSelection::Current => ProfileSelection::Record2,
                ProfileSelection::Record2 => ProfileSelection::Record3,
                ProfileSelection::Record3 => ProfileSelection::WrappedPredecessor,
            }
        } else { selection };
        let record = Self::slot(selection);
        V::from_bits(field.record.extract(field.record.load(record.bytes(), 0)))
    }
}
pub struct TailDefaults;
impl TailDefaults {
    pub fn tail() -> RamTail<'static> { RamTail(&TAIL_BYTES.0) }
    pub fn first_halfword() -> u16 { u16::from_le_bytes([TAIL_BYTES.0[0], TAIL_BYTES.0[1]]) }
    #[inline(always)]
    pub fn byte(offset: crate::table::TailOffset) -> Option<u8> {
        TAIL_BYTES.0.get(offset.0 as usize).copied()
    }
    #[inline(always)]
    pub fn halfword(offset: crate::table::TailOffset) -> Option<u16> {
        let bytes = TAIL_BYTES.0.get(offset.0 as usize..(offset.0 as usize).checked_add(2)?)?;
        Some(u16::from_le_bytes([bytes[0], bytes[1]]))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetworkNameDefaultByte(pub u8);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupResavePolicy { Resave, Preserve }
impl StartupResavePolicy {
    pub const fn from_abi(value: u8) -> Self { if value == 0 { Self::Resave } else { Self::Preserve } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultProfile { Two, Three, Four }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfiguredDefault(pub u32);
pub trait SettingsConfiguration {
    fn network_name_default(&self) -> NetworkNameDefaultByte;
    fn startup_resave_policy(&self) -> StartupResavePolicy;
    fn default_61df44(&self) -> ConfiguredDefault;
    fn default_61d8a4(&self) -> ConfiguredDefault;
    fn default_61d59c(&self) -> ConfiguredDefault;
    fn default_61d618(&self) -> ConfiguredDefault;
    fn default_61d5f0(&self) -> ConfiguredDefault;
    fn default_61d65c(&self, profile: DefaultProfile) -> ConfiguredDefault;
}

pub struct ConfigurationInputs {
    network: core::cell::Cell<NetworkNameDefaultByte>,
    startup: core::cell::Cell<StartupResavePolicy>,
}
impl ConfigurationInputs {
    pub const fn new(network: NetworkNameDefaultByte, startup: StartupResavePolicy) -> Self {
        Self { network: core::cell::Cell::new(network), startup: core::cell::Cell::new(startup) }
    }
    pub fn set_network_name_default(&self, value: NetworkNameDefaultByte) { self.network.set(value); }
    pub fn set_startup_resave_policy(&self, value: StartupResavePolicy) { self.startup.set(value); }
}
pub trait DefaultDependencies {
    fn default_61df44(&self) -> ConfiguredDefault;
    fn default_61d8a4(&self) -> ConfiguredDefault;
    fn default_61d59c(&self) -> ConfiguredDefault;
    fn default_61d618(&self) -> ConfiguredDefault;
    fn default_61d5f0(&self) -> ConfiguredDefault;
    fn default_61d65c(&self, profile: DefaultProfile) -> ConfiguredDefault;
}
pub struct SettingsConfigurationService<'a, D> { inputs: &'a ConfigurationInputs, defaults: D }
impl<'a, D> SettingsConfigurationService<'a, D> {
    pub const fn new(inputs: &'a ConfigurationInputs, defaults: D) -> Self { Self { inputs, defaults } }
}
impl<D: DefaultDependencies> SettingsConfiguration for SettingsConfigurationService<'_, D> {
    fn network_name_default(&self) -> NetworkNameDefaultByte { self.inputs.network.get() }
    fn startup_resave_policy(&self) -> StartupResavePolicy { self.inputs.startup.get() }
    fn default_61df44(&self) -> ConfiguredDefault { self.defaults.default_61df44() }
    fn default_61d8a4(&self) -> ConfiguredDefault { self.defaults.default_61d8a4() }
    fn default_61d59c(&self) -> ConfiguredDefault { self.defaults.default_61d59c() }
    fn default_61d618(&self) -> ConfiguredDefault { self.defaults.default_61d618() }
    fn default_61d5f0(&self) -> ConfiguredDefault { self.defaults.default_61d5f0() }
    fn default_61d65c(&self, profile: DefaultProfile) -> ConfiguredDefault { self.defaults.default_61d65c(profile) }
}
