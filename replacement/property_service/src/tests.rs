use crate::{read_property, write_property, PropertyId, PropertyWriteOutcome, ReadMode, ValueBits};
use storage::{ConfiguredDefault, DefaultProfile, Field, HeaderBackend, NetworkNameDefaultByte, PreviousValues, ProfileSelection, PropertyValue, RequestOutcome, SettingsBank, SettingsConfiguration, SettingsStorage, StartupResavePolicy, WriteValue, BANK_STRIDE, BANK_WINDOW, PREVIOUS_WINDOW};

struct Backing { header: [u8; 16], opened: RequestOutcome, closed: storage::MirrorOutcome }
impl HeaderBackend for Backing {
    fn read_header(&mut self, destination: &mut [u8;16]) -> RequestOutcome {
        if self.opened.mapped == storage::mirror::RequestStatus::Success { *destination = self.header; }
        self.opened
    }
    fn write_header(&mut self, source: &[u8;16]) -> storage::MirrorOutcome {
        if self.closed.request.mapped == storage::mirror::RequestStatus::Success { self.header = *source; }
        self.closed
    }
}
struct Configuration { network: NetworkNameDefaultByte, startup: StartupResavePolicy }
impl SettingsConfiguration for Configuration {
    fn network_name_default(&self) -> NetworkNameDefaultByte { self.network }
    fn startup_resave_policy(&self) -> StartupResavePolicy { self.startup }
    fn default_61df44(&self) -> ConfiguredDefault { ConfiguredDefault(3) }
    fn default_61d8a4(&self) -> ConfiguredDefault { ConfiguredDefault(3) }
    fn default_61d59c(&self) -> ConfiguredDefault { ConfiguredDefault(3) }
    fn default_61d618(&self) -> ConfiguredDefault { ConfiguredDefault(3) }
    fn default_61d5f0(&self) -> ConfiguredDefault { ConfiguredDefault(3) }
    fn default_61d65c(&self, _: DefaultProfile) -> ConfiguredDefault { ConfiguredDefault(3) }
}
fn configuration() -> Configuration { Configuration { network: NetworkNameDefaultByte(0xff), startup: StartupResavePolicy::Preserve } }
fn backend() -> Backing { Backing { header: [0x55;16], opened: RequestOutcome::success(), closed: storage::MirrorOutcome::unchanged() } }
fn id(value: u32) -> PropertyId { PropertyId::decode(value).unwrap() }

#[test]
fn stock_default_saves_the_provisional_value_and_preserves_neighbor_bits() {
    let mut banks = [0xd5;BANK_WINDOW];
    let mut previous = [0xaa;PREVIOUS_WINDOW];
    let mut storage = SettingsStorage::adopt(&mut banks, PreviousValues::adopt(&mut previous), backend());
    assert_eq!(write_property(&mut storage, &configuration(), id(0x34), WriteValue::Default, SettingsBank::Active, ProfileSelection::Current).assignment, PropertyWriteOutcome::Assigned);
    assert_eq!(read_property(&storage, &configuration(), id(0x34), SettingsBank::Active, ReadMode::Live, ProfileSelection::Current), ValueBits::new(3));
    assert_eq!(storage.read_previous(Field::<PropertyValue<0x34>>::property().unwrap()).get(), 0);
    assert_eq!(storage.bank(SettingsBank::Active).bytes()[0x21], 0xc3);
    assert_eq!(storage.previous().bytes()[0x21], 0x80);
}
#[test]
fn configuration_default_returns_the_byte_but_stores_only_the_field() {
    let mut banks = [0x9b;BANK_WINDOW];
    let mut previous = [0xaa;PREVIOUS_WINDOW];
    let mut storage = SettingsStorage::adopt(&mut banks, PreviousValues::adopt(&mut previous), backend());
    assert_eq!(read_property(&storage, &configuration(), id(0xa9), SettingsBank::Active, ReadMode::Default, ProfileSelection::Current), ValueBits::new(255));
    write_property(&mut storage, &configuration(), id(0xa9), WriteValue::Default, SettingsBank::Active, ProfileSelection::Current);
    assert_eq!(storage.bank(SettingsBank::Active).bytes()[0x29], 0x9f);
    assert_eq!(storage.read_previous(Field::<PropertyValue<0xa9>>::property().unwrap()).get(), 27);
}
#[test]
fn zero_configuration_gate_resaves_the_new_value_only_on_active_bank() {
    let mut banks = [0x60;BANK_WINDOW];
    let mut previous = [0xaa;PREVIOUS_WINDOW];
    let mut storage = SettingsStorage::adopt(&mut banks, PreviousValues::adopt(&mut previous), backend());
    let configuration = Configuration { startup: StartupResavePolicy::Resave, ..configuration() };
    write_property(&mut storage, &configuration, id(0x49), WriteValue::Value(ValueBits::new(2)), SettingsBank::Active, ProfileSelection::Current);
    assert_eq!(storage.read_previous(Field::<PropertyValue<0x49>>::property().unwrap()).get(), 2);
    let saved = *storage.previous().bytes();
    write_property(&mut storage, &configuration, id(0x49), WriteValue::Value(ValueBits::new(0)), SettingsBank::Alternate, ProfileSelection::Current);
    assert_eq!(storage.previous().bytes(), &saved);
}
#[test]
fn active_header_refreshes_before_capture_and_alternate_does_not_persist_header() {
    let mut banks = [0xaa;BANK_WINDOW];
    let mut previous = [0xaa;PREVIOUS_WINDOW];
    let mut storage = SettingsStorage::adopt(&mut banks, PreviousValues::adopt(&mut previous), backend());
    write_property(&mut storage, &configuration(), id(1), WriteValue::Value(ValueBits::new(1)), SettingsBank::Active, ProfileSelection::Current);
    assert_eq!(storage.backend().header[0xb], 0x57);
    assert_eq!(storage.read_previous(Field::<PropertyValue<1>>::property().unwrap()).get(), 0);
    write_property(&mut storage, &configuration(), id(1), WriteValue::Value(ValueBits::new(0)), SettingsBank::Alternate, ProfileSelection::Current);
    assert_eq!(storage.backend().header[0xb], 0x57);
    assert_eq!(storage.bank(SettingsBank::Alternate).bytes()[0xb], 0xa8);
}
#[test]
fn pair_updates_both_containers_and_retains_their_distinct_previous_values() {
    let mut banks = [0x11;BANK_WINDOW];
    let mut previous = [0xaa;PREVIOUS_WINDOW];
    let mut storage = SettingsStorage::adopt(&mut banks, PreviousValues::adopt(&mut previous), backend());
    write_property(&mut storage, &configuration(), id(0x283), WriteValue::Default, SettingsBank::Active, ProfileSelection::Current);
    assert_eq!(storage.backend().header[0xe], 0xaa);
    assert_eq!(storage.bank(SettingsBank::Active).bytes()[0x126], 0xaa);
    assert_eq!(storage.previous().bytes()[0xe], 0x55);
    assert_eq!(storage.previous().bytes()[0x126], 0x11);
    assert_eq!(read_property(&storage, &configuration(), id(0x283), SettingsBank::Active, ReadMode::Live, ProfileSelection::Current), ValueBits::new(0));
}
#[test]
fn mirrored_property_reads_record_zero_for_every_admitted_selection() {
    let mut banks = [0x11;BANK_WINDOW];
    let mut previous = [0xaa;PREVIOUS_WINDOW];
    let mut storage = SettingsStorage::adopt(&mut banks, PreviousValues::adopt(&mut previous), backend());
    write_property(&mut storage, &configuration(), id(0xe9), WriteValue::Value(ValueBits::new(3)), SettingsBank::Active, ProfileSelection::Current);
    assert_eq!(storage.bank(SettingsBank::Active).bytes()[0x1a2], 3);
    assert_eq!(storage.bank(SettingsBank::Active).bytes()[0x14a], 0x71);
    for selection in [ProfileSelection::Current, ProfileSelection::Record0, ProfileSelection::Record2, ProfileSelection::Record3, ProfileSelection::WrappedPredecessor] {
        assert_eq!(read_property(&storage, &configuration(), id(0xe9), SettingsBank::Active, ReadMode::Live, selection), ValueBits::new(3));
    }
}
#[test]
fn read_only_set_leaves_all_backing_and_previous_bytes_unchanged() {
    let mut banks = [0x11;BANK_WINDOW];
    let mut previous = [0xaa;PREVIOUS_WINDOW];
    let mut storage = SettingsStorage::adopt(&mut banks, PreviousValues::adopt(&mut previous), backend());
    assert_eq!(write_property(&mut storage, &configuration(), id(0x27b), WriteValue::Value(ValueBits::new(0)), SettingsBank::Active, ProfileSelection::Current).assignment, PropertyWriteOutcome::Unassigned);
    assert_eq!(storage.bank(SettingsBank::Active).bytes(), &[0x11;BANK_STRIDE]);
    assert_eq!(storage.bank(SettingsBank::Alternate).bytes(), &[0x11;BANK_STRIDE]);
    assert_eq!(storage.previous().bytes(), &[0xaa;PREVIOUS_WINDOW]);
}

#[test]
fn owned_write_retains_request_failure_and_assignment_independently() {
    use storage::mirror::{BackingStatus, DeviceStatus, RequestStatus};
    let mut banks = [0x11;BANK_WINDOW];
    let mut previous = [0xaa;PREVIOUS_WINDOW];
    let mut backend = backend();
    backend.opened = RequestOutcome { underlying: BackingStatus::Refused(DeviceStatus(-30)), mapped: RequestStatus::Failed, reported: true };
    backend.closed.request = RequestOutcome { underlying: BackingStatus::Refused(DeviceStatus(-11)), mapped: RequestStatus::Failed, reported: true };
    let mut storage = SettingsStorage::adopt(&mut banks, PreviousValues::adopt(&mut previous), backend);
    let result = write_property(&mut storage, &configuration(), id(1), WriteValue::Value(ValueBits::new(1)), SettingsBank::Active, ProfileSelection::Current);
    assert_eq!(result.assignment, PropertyWriteOutcome::Assigned);
    let mirror = result.mirror.unwrap();
    assert!(matches!(mirror.opened, Some(RequestOutcome { underlying: BackingStatus::Refused(DeviceStatus(-30)), mapped: RequestStatus::Failed, reported: true })));
    assert!(matches!(mirror.closed.request, RequestOutcome { underlying: BackingStatus::Refused(DeviceStatus(-11)), mapped: RequestStatus::Failed, reported: true }));
    assert_eq!(storage.backend().header, [0x55;16]);
}

#[test]
fn owned_write_retains_backup_driver_failure_after_successful_request() {
    use storage::mirror::{BackupKey, BackupValue, DeviceStatus, DriverObservation, HeaderPosition};
    let mut banks = [0x11;BANK_WINDOW];
    let mut previous = [0xaa;PREVIOUS_WINDOW];
    let mut backend = backend();
    backend.closed.synchronized[11] = Some(DriverObservation { position: HeaderPosition::new(11).unwrap(), key: BackupKey(0x1a), value: BackupValue(0x57), status: DeviceStatus(-7) });
    let mut storage = SettingsStorage::adopt(&mut banks, PreviousValues::adopt(&mut previous), backend);
    let result = write_property(&mut storage, &configuration(), id(1), WriteValue::Value(ValueBits::new(1)), SettingsBank::Active, ProfileSelection::Current);
    assert_eq!(result.assignment, PropertyWriteOutcome::Assigned);
    let mirror = result.mirror.unwrap();
    assert_eq!(mirror.opened, Some(RequestOutcome::success()));
    assert_eq!(mirror.closed.request, RequestOutcome::success());
    assert!(matches!(mirror.closed.synchronized[11], Some(DriverObservation { status: DeviceStatus(-7), .. })));
}
