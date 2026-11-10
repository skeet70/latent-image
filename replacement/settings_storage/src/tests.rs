use crate::*;

struct Backing { body: [u8;0x118], writes: usize }
impl HeaderBackend for Backing {
    fn read_header(&mut self, destination: &mut [u8;16]) -> RequestOutcome { destination.fill(0x55); RequestOutcome::success() }
    fn write_header(&mut self, _: &[u8;16]) -> MirrorOutcome { MirrorOutcome::unchanged() }
}
impl SettingsBackend for Backing {
    fn read_body(&mut self, destination: &mut [u8;0x118]) -> RequestOutcome { *destination = self.body; RequestOutcome::success() }
    fn write_body(&mut self, source: &[u8;0x118]) -> BodyOutcome { self.body = *source; self.writes+=1; BodyOutcome::unchanged() }
    fn read_custom(&mut self, destination: &mut [u8;40]) -> RequestOutcome { destination.fill(0x77); RequestOutcome::success() }
}
fn backend() -> Backing { Backing { body: [0x66;0x118], writes: 0 } }

#[test]
fn previous_fields_preserve_unrelated_bits_and_repeat_captures() {
    let mut banks = [0x55;BANK_WINDOW];
    let mut previous = [0xaa;PREVIOUS_WINDOW];
    let mut storage = SettingsStorage::adopt(&mut banks, PreviousValues::adopt(&mut previous), backend());
    let field = Field::<PropertyValue<1>>::property().unwrap();
    let original = storage.read(SettingsBank::Active, field);
    storage.write(SettingsBank::Active, field, PropertyValue::new(0), PreviousUpdate::FieldBits);
    assert_eq!(storage.read_previous(field), original);
    storage.write(SettingsBank::Active, field, PropertyValue::new(1), PreviousUpdate::FieldBits);
    assert_eq!(storage.read_previous(field), PropertyValue::new(0));
    let previous = *storage.previous().bytes();
    storage.write(SettingsBank::Alternate, field, PropertyValue::new(0), PreviousUpdate::WholeContainer);
    assert_eq!(storage.previous().bytes(), &previous);
}
#[test]
fn capture_restore_copies_whole_image_then_persists_body() {
    let mut banks = [0x55;BANK_WINDOW];
    let mut previous = [0xaa;PREVIOUS_WINDOW];
    let mut storage = SettingsStorage::adopt(&mut banks, PreviousValues::adopt(&mut previous), backend());
    storage.capture_active();
    let field = Field::<PropertyValue<0xa9>>::property().unwrap();
    storage.write(SettingsBank::Active, field, PropertyValue::new(0), PreviousUpdate::None);
    assert_ne!(storage.bank(SettingsBank::Active).bytes(), &[0x55;BANK_STRIDE]);
    storage.restore_active_body();
    assert_eq!(storage.bank(SettingsBank::Active).bytes(), &[0x55;BANK_STRIDE]);
    assert_eq!(storage.backend().writes, 1);
    assert_eq!(storage.backend().body, [0x55;0x118]);
}
#[test]
fn selector_views_keep_spill_and_wrapped_aliases() {
    let mut banks = [0;BANK_WINDOW];
    for (index, byte) in banks.iter_mut().enumerate() { *byte = index as u8; }
    let mut previous = [0;PREVIOUS_WINDOW];
    let storage = SettingsStorage::adopt(&mut banks, PreviousValues::adopt(&mut previous), backend());
    assert_eq!(&storage.slot(SettingsBank::Active, ProfileSelection::Record3).bytes()[10..], &storage.bank(SettingsBank::Alternate).bytes()[..30]);
    assert_eq!(&storage.slot(SettingsBank::Active, ProfileSelection::WrappedPredecessor).bytes()[..16], &storage.bank(SettingsBank::Active).body().bytes()[0x108..]);
    assert_eq!(&storage.slot(SettingsBank::Active, ProfileSelection::Record2).bytes()[10..], &storage.bank(SettingsBank::Active).tail().bytes()[..30]);
}
#[test]
fn header_open_precedes_previous_capture() {
    let mut banks = [0;BANK_WINDOW];
    let mut previous = [0xaa;PREVIOUS_WINDOW];
    let mut storage = SettingsStorage::adopt(&mut banks, PreviousValues::adopt(&mut previous), backend());
    let field = Field::<PropertyValue<1>>::property().unwrap();
    let header_field = HeaderField::try_from(field).unwrap();
    let mut session = storage.open_header();
    let refreshed = session.read(header_field);
    session.write(header_field, PropertyValue::new(0), PreviousUpdate::FieldBits);
    session.close();
    assert_eq!(storage.read_previous(field), refreshed);
}
#[test]
fn default_windows_preserve_wrapped_predecessor_and_tail_halfword() {
    assert_eq!(SlotDefaultsWindow::slot(ProfileSelection::WrappedPredecessor).bytes().len(),40);
    let bytes = TailDefaults::tail().bytes();
    assert_eq!(TailDefaults::first_halfword(), u16::from_le_bytes([bytes[0],bytes[1]]));
    assert_eq!(WriteValue::<PropertyValue<1>>::from_abi(0x1affffff),WriteValue::Default);
    assert_eq!(ProfileSelection::from_abi(u32::MAX), Some(ProfileSelection::WrappedPredecessor));
    assert_eq!(ProfileSelection::from_abi(4), None);
}

#[test]
fn mirror_fields_reject_body_properties() {
    let header = Field::<PropertyValue<1>>::property().unwrap();
    assert!(HeaderField::try_from(header).is_ok());
    let body = Field::<PropertyValue<0xa9>>::property().unwrap();
    assert!(matches!(HeaderField::try_from(body), Err(NotHeaderField)));
}

struct Configuration(StartupResavePolicy);
impl SettingsConfiguration for Configuration {
    fn network_name_default(&self) -> NetworkNameDefaultByte { NetworkNameDefaultByte(0) }
    fn startup_resave_policy(&self) -> StartupResavePolicy { self.0 }
    fn default_61df44(&self) -> ConfiguredDefault { ConfiguredDefault(0) }
    fn default_61d8a4(&self) -> ConfiguredDefault { ConfiguredDefault(0) }
    fn default_61d59c(&self) -> ConfiguredDefault { ConfiguredDefault(0) }
    fn default_61d618(&self) -> ConfiguredDefault { ConfiguredDefault(0) }
    fn default_61d5f0(&self) -> ConfiguredDefault { ConfiguredDefault(0) }
    fn default_61d65c(&self, _: DefaultProfile) -> ConfiguredDefault { ConfiguredDefault(0) }
}

#[test]
fn initialize_loads_three_images_without_overwriting_gaps_or_spill() {
    let mut banks = [0xa1; BANK_WINDOW];
    banks[BANK_STRIDE..BANK_STRIDE*2].fill(0xa2);
    banks[BANK_STRIDE*2..].fill(0xa3);
    let mut previous = [0xb1; PREVIOUS_WINDOW];
    let mut storage = SettingsStorage::adopt(&mut banks, PreviousValues::adopt(&mut previous), backend());
    let outcome = storage.initialize(&Configuration(StartupResavePolicy::Preserve), |_| panic!("resave disabled"));
    assert_eq!(outcome.requests, [RequestOutcome::success(); 9]);
    assert!(!outcome.startup_resaved);
    for (image, poison) in [
        (storage.bank(SettingsBank::Active).bytes().as_slice(), 0xa1),
        (storage.bank(SettingsBank::Alternate).bytes().as_slice(), 0xa2),
        (&storage.previous().bytes()[..BANK_STRIDE], 0xb1),
    ] {
        assert_eq!(&image[..16], &[0x55; 16]);
        assert_eq!(&image[16..0x128], &[0x66; 0x118]);
        assert_eq!(&image[0x128..0x140], &[poison; 24]);
        assert_eq!(&image[0x140..0x168], &[0x77; 40]);
        assert_eq!(&image[0x168..0x19a], &[poison; 50]);
        assert_eq!(&image[0x19a..], TailDefaults::tail().bytes());
    }
    assert_eq!(&storage.previous().bytes()[BANK_STRIDE..], &[0xb1; PREVIOUS_WINDOW-BANK_STRIDE]);
    drop(storage);
    assert_eq!(&banks[BANK_STRIDE*2..], &[0xa3; BANK_STRIDE]);
}

#[test]
fn initialize_resave_observes_loaded_images_and_retains_its_changes() {
    let mut banks = [0xa1; BANK_WINDOW];
    let mut previous = [0xb1; PREVIOUS_WINDOW];
    let mut storage = SettingsStorage::adopt(&mut banks, PreviousValues::adopt(&mut previous), backend());
    let field = Field::<PropertyValue<1>>::property().unwrap();
    let mut loaded_value = None;
    let outcome = storage.initialize(&Configuration(StartupResavePolicy::Resave), |storage| {
        assert_eq!(storage.bank(SettingsBank::Active).tail().bytes(), TailDefaults::tail().bytes());
        assert_eq!(storage.bank(SettingsBank::Alternate).header().bytes(), &[0x55; 16]);
        assert_eq!(&storage.previous().bytes()[0x140..0x168], &[0x77; 40]);
        loaded_value = Some(storage.read(SettingsBank::Active, field));
        storage.write(SettingsBank::Active, field, PropertyValue::new(0), PreviousUpdate::FieldBits);
    });
    assert!(outcome.startup_resaved);
    assert_eq!(storage.read(SettingsBank::Active, field), PropertyValue::new(0));
    assert_eq!(Some(storage.read_previous(field)), loaded_value);
}

struct Defaults;
impl DefaultDependencies for Defaults {
    fn default_61df44(&self) -> ConfiguredDefault { ConfiguredDefault(11) }
    fn default_61d8a4(&self) -> ConfiguredDefault { ConfiguredDefault(12) }
    fn default_61d59c(&self) -> ConfiguredDefault { ConfiguredDefault(13) }
    fn default_61d618(&self) -> ConfiguredDefault { ConfiguredDefault(14) }
    fn default_61d5f0(&self) -> ConfiguredDefault { ConfiguredDefault(15) }
    fn default_61d65c(&self, profile: DefaultProfile) -> ConfiguredDefault {
        ConfiguredDefault(match profile { DefaultProfile::Two => 2, DefaultProfile::Three => 3, DefaultProfile::Four => 4 })
    }
}
#[test]
fn configuration_service_observes_live_typed_inputs_and_retained_dependencies() {
    let inputs = ConfigurationInputs::new(NetworkNameDefaultByte(0xff), StartupResavePolicy::Preserve);
    let service = SettingsConfigurationService::new(&inputs, Defaults);
    assert_eq!(service.network_name_default(), NetworkNameDefaultByte(0xff));
    assert_eq!(service.startup_resave_policy(), StartupResavePolicy::Preserve);
    inputs.set_network_name_default(NetworkNameDefaultByte(0x80));
    inputs.set_startup_resave_policy(StartupResavePolicy::Resave);
    assert_eq!(service.network_name_default(), NetworkNameDefaultByte(0x80));
    assert_eq!(service.startup_resave_policy(), StartupResavePolicy::Resave);
    assert_eq!(service.default_61df44(), ConfiguredDefault(11));
    assert_eq!(service.default_61d8a4(), ConfiguredDefault(12));
    assert_eq!(service.default_61d59c(), ConfiguredDefault(13));
    assert_eq!(service.default_61d618(), ConfiguredDefault(14));
    assert_eq!(service.default_61d5f0(), ConfiguredDefault(15));
    assert_eq!(service.default_61d65c(DefaultProfile::Three), ConfiguredDefault(3));
}

#[derive(Clone, Copy)]
struct TestValue(u32);
impl FieldValue for TestValue {
    fn from_bits(value: u32) -> Self { Self(value) }
    fn bits(self) -> u32 { self.0 }
}

#[test]
fn runtime_layouts_reject_crossing_containers_and_preserve_little_endian_values() {
    let table::Kind::Field(layout) = table::Entry(0x1006).kind().unwrap() else { panic!("halfword descriptor") };
    assert!(Field::<TestValue>::from_bank_layout(table::BankOffset((BANK_STRIDE - 1) as u32), layout.bits).is_none());
    let field = Field::<TestValue>::from_bank_layout(table::BankOffset((BANK_STRIDE - 2) as u32), layout.bits).unwrap();
    assert!(SlotField::from_layout(field, table::RecordOffset(39), layout.bits).is_none());
    let mut banks = [0xaa;BANK_WINDOW];
    let mut previous = [0x55;PREVIOUS_WINDOW];
    let mut storage = SettingsStorage::adopt(&mut banks, PreviousValues::adopt(&mut previous), backend());
    storage.write(SettingsBank::Active, field, TestValue(0x1234), PreviousUpdate::WholeContainer);
    assert_eq!(&storage.bank(SettingsBank::Active).bytes()[BANK_STRIDE - 2..], &[0x34, 0x12]);
    assert_eq!(storage.read_previous(field).0, 0xaaaa);
    assert_eq!(storage.bank(SettingsBank::Alternate).bytes(), &[0xaa;BANK_STRIDE]);
}

#[test]
fn runtime_layouts_reject_maximum_offsets_for_each_container_width() {
    for encoded in [0x802, 0x1006] {
        let table::Kind::Field(layout) = table::Entry(encoded).kind().unwrap() else { panic!("field descriptor") };
        assert!(Field::<TestValue>::from_bank_layout(table::BankOffset(u32::MAX), layout.bits).is_none());
        let current = Field::<TestValue>::from_bank_layout(table::BankOffset(0), layout.bits).unwrap();
        assert!(SlotField::from_layout(current, table::RecordOffset(u32::MAX), layout.bits).is_none());
    }
}

#[test]
fn exchange_preserves_saved_and_spill_bytes_and_persists_only_restored_body() {
    let mut banks = [0x33; BANK_WINDOW];
    banks[..BANK_STRIDE].fill(0x11);
    banks[BANK_STRIDE..BANK_STRIDE*2].fill(0x55);
    let mut previous = [0xaa; PREVIOUS_WINDOW];
    let mut storage = SettingsStorage::adopt(&mut banks, PreviousValues::adopt(&mut previous), backend());
    assert_eq!(storage.exchange(BankExchange::ActiveToAlternate), ExchangeOutcome::CopiedToAlternate);
    assert_eq!(storage.bank(SettingsBank::Active).bytes(), &[0x11; BANK_STRIDE]);
    assert_eq!(storage.bank(SettingsBank::Alternate).bytes(), &[0x11; BANK_STRIDE]);
    assert_eq!(storage.backend().body, [0x66; 0x118]);
    let field = Field::<PropertyValue<0xa9>>::property().unwrap();
    storage.write(SettingsBank::Active, field, PropertyValue::new(0), PreviousUpdate::None);
    assert_ne!(storage.bank(SettingsBank::Active).bytes(), storage.bank(SettingsBank::Alternate).bytes());
    assert_eq!(storage.exchange(BankExchange::AlternateToActive), ExchangeOutcome::ActiveRestored(BodyOutcome::unchanged()));
    assert_eq!(storage.bank(SettingsBank::Active).bytes(), &[0x11; BANK_STRIDE]);
    assert_eq!(storage.backend().body, [0x11; 0x118]);
    assert_eq!(storage.previous().bytes(), &[0xaa; PREVIOUS_WINDOW]);
    drop(storage);
    assert_eq!(&banks[BANK_STRIDE*2..], &[0x33; BANK_STRIDE]);
}
