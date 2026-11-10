//! The camera transport's USB provider: the `fuji_usb_*` C ABI that
//! `FujiAF.Native` imports, over nusb. Only 04cb:ff80 devices are described or
//! opened. On Linux their listing descriptors come from sysfs, so listing opens
//! nothing; on macOS listing opens each one to read its active configuration.
//! A process holds at most one listing, one open device and one claim.

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("fuji-usb supports Linux and macOS hosts");

use std::ffi::{c_char, c_int, c_uint, CStr};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use nusb::descriptors::{ConfigurationDescriptor, TransferType};
use nusb::transfer::{Buffer, Bulk, ControlOut, ControlType, In, Out, Recipient, TransferError};
use nusb::{Device, DeviceInfo, ErrorKind, Interface, MaybeFuture};

const VENDOR: u16 = 0x04cb;
const PRODUCT: u16 = 0xff80;
const MAX_DEVICES: usize = 64;
const MAX_PORTS: usize = 8;
const PATH_BYTES: usize = 80;
const FIELDS: usize = 39;
const MAX_EXTRA_ENDPOINTS: usize = 6;
/// `transferCeilingMs` in FujiAF.Transport, the held `commit-f7` trigger reply.
const TIMEOUT_CEILING_MS: c_uint = 33_000;
/// One `read-bulk` chunk.
const BULK_IN_CEILING: c_int = 0x10_0000;
const TRANSFER_CEILING: c_int = 292;
const CONTROL_BYTES: c_int = 12;

/// Device and transfer statuses in libusb's numbering, which journals record.
mod libusb {
    use std::ffi::c_int;
    pub const IO: c_int = -1;
    pub const INVALID_PARAM: c_int = -2;
    pub const ACCESS: c_int = -3;
    pub const NO_DEVICE: c_int = -4;
    pub const NOT_FOUND: c_int = -5;
    pub const BUSY: c_int = -6;
    pub const TIMEOUT: c_int = -7;
    pub const OVERFLOW: c_int = -8;
    pub const PIPE: c_int = -9;
    pub const NOT_SUPPORTED: c_int = -12;
    pub const OTHER: c_int = -99;
}

/// The provider's own refusals.
mod refusal {
    use std::ffi::c_int;
    pub const LOADED: c_int = -100;
    pub const INIT: c_int = -103;
    pub const LIST: c_int = -104;
    pub const TOO_MANY: c_int = -105;
    pub const PORTS: c_int = -106;
    pub const DESCRIPTORS: c_int = -107;
    pub const FIELD: c_int = -108;
    pub const OPEN: c_int = -109;
    pub const AMBIGUOUS: c_int = -110;
    pub const NOT_ELIGIBLE: c_int = -111;
    pub const CLAIM: c_int = -112;
    pub const DRIVER_BOUND: c_int = -113;
    pub const TRANSFER: c_int = -114;
    pub const CONTROL_SIZE: c_int = -115;
    pub const KIND: c_int = -116;
}

/// One listed device. Fields 0 and 1 are the vendor and product ids; the rest
/// and the path are set for eligible devices only.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    path: String,
    fields: [c_int; FIELDS],
}

impl Entry {
    fn ineligible(vendor: u16, product: u16) -> Entry {
        let mut fields = [0; FIELDS];
        fields[0] = c_int::from(vendor);
        fields[1] = c_int::from(product);
        Entry { path: String::new(), fields }
    }

    fn eligible(&self) -> bool {
        self.fields[0] == c_int::from(VENDOR) && self.fields[1] == c_int::from(PRODUCT)
    }
}

struct Session {
    loaded: bool,
    initialized: bool,
    listing: Option<Vec<(DeviceInfo, Entry)>>,
    /// NUL-terminated listing paths; `fuji_usb_path` hands out pointers into them.
    paths: [[u8; PATH_BYTES]; MAX_DEVICES],
    opened: Option<(DeviceInfo, Device)>,
    claimed: Option<Interface>,
}

static SESSION: Mutex<Session> = Mutex::new(Session {
    loaded: false,
    initialized: false,
    listing: None,
    paths: [[0; PATH_BYTES]; MAX_DEVICES],
    opened: None,
    claimed: None,
});

fn session() -> MutexGuard<'static, Session> {
    SESSION.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Session {
    fn listed(&self, index: c_int) -> Option<&(DeviceInfo, Entry)> {
        self.listing.as_ref()?.get(usize::try_from(index).ok()?)
    }
}

#[no_mangle]
pub extern "C" fn fuji_usb_load() -> c_int {
    let mut session = session();
    if session.loaded {
        return refusal::LOADED;
    }
    session.loaded = true;
    0
}

#[no_mangle]
pub extern "C" fn fuji_usb_init() -> c_int {
    let mut session = session();
    if !session.loaded || session.initialized {
        return refusal::INIT;
    }
    session.initialized = true;
    0
}

/// Lists the host's devices and returns their count. Listing again is refused
/// while a device is open.
#[no_mangle]
pub extern "C" fn fuji_usb_list() -> c_int {
    let mut session = session();
    if !session.initialized || session.opened.is_some() {
        return refusal::LIST;
    }
    session.listing = None;
    let devices: Vec<DeviceInfo> = match nusb::list_devices().wait() {
        Ok(devices) => devices.collect(),
        Err(error) => return error_status(&error),
    };
    match inventory(&devices, |device| (device.vendor_id(), device.product_id()), describe) {
        Ok(entries) => {
            for (slot, entry) in session.paths.iter_mut().zip(&entries) {
                *slot = [0; PATH_BYTES];
                slot[..entry.path.len()].copy_from_slice(entry.path.as_bytes());
            }
            let count = entries.len();
            session.listing = Some(devices.into_iter().zip(entries).collect());
            count as c_int
        }
        Err(status) => status,
    }
}

/// The listed path of device `index`, valid until the next listing; the empty
/// string for an ineligible device.
#[no_mangle]
pub extern "C" fn fuji_usb_path(index: c_int) -> *const c_char {
    let session = session();
    match session.listed(index) {
        Some(_) => session.paths[index as usize].as_ptr().cast(),
        None => std::ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn fuji_usb_field(index: c_int, field: c_int) -> c_int {
    let session = session();
    match (session.listed(index), usize::try_from(field)) {
        (Some((_, entry)), Ok(field)) if field < FIELDS => entry.fields[field],
        _ => refusal::FIELD,
    }
}

/// Opens the one listed eligible device at `path`.
///
/// # Safety
/// `path` is null or a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn fuji_usb_open(path: *const c_char) -> c_int {
    let mut session = session();
    let Some(listing) = session.listing.as_ref().filter(|_| session.opened.is_none()) else {
        return refusal::OPEN;
    };
    if path.is_null() {
        return refusal::NOT_ELIGIBLE;
    }
    // SAFETY: the caller passes a NUL-terminated string.
    let path = unsafe { CStr::from_ptr(path) }.to_bytes();
    let mut matches = listing.iter().filter(|(_, entry)| entry.path.as_bytes() == path);
    let info = match (matches.next(), matches.next()) {
        (Some(_), Some(_)) => return refusal::AMBIGUOUS,
        (Some((info, entry)), None) if entry.eligible() => info.clone(),
        _ => return refusal::NOT_ELIGIBLE,
    };
    match info.open().wait() {
        Ok(device) => {
            session.opened = Some((info, device));
            0
        }
        Err(error) => error_status(&error),
    }
}

/// Claims interface 0 of the open device, refusing while a kernel driver holds
/// it rather than detaching the driver.
#[no_mangle]
pub extern "C" fn fuji_usb_claim() -> c_int {
    let mut session = session();
    let Some((info, device)) = session.opened.clone().filter(|_| session.claimed.is_none()) else {
        return refusal::CLAIM;
    };
    match claim(kernel_driver_bound(&info), || device.claim_interface(0).wait().map_err(|error| error_status(&error))) {
        Ok(interface) => {
            session.claimed = Some(interface);
            0
        }
        Err(status) => status,
    }
}

fn claim<T>(driver_bound: Result<bool, c_int>, claim: impl FnOnce() -> Result<T, c_int>) -> Result<T, c_int> {
    match driver_bound? {
        true => Err(refusal::DRIVER_BOUND),
        false => claim(),
    }
}

#[no_mangle]
pub extern "C" fn fuji_usb_transfer_size_admitted(kind: c_int, size: c_int) -> c_int {
    size_admitted(kind, size).into()
}

/// A bulk IN (kind 2) up to one `read-bulk` chunk, every other kind up to 292 bytes.
fn size_admitted(kind: c_int, size: c_int) -> bool {
    let ceiling = if kind == 2 { BULK_IN_CEILING } else { TRANSFER_CEILING };
    (1..=ceiling).contains(&size)
}

/// Runs one transfer on the claimed interface: kind 0 the 12-byte vendor control
/// write, 1 a bulk OUT, 2 a bulk IN. Returns the status, with the bytes moved,
/// including those of a partial transfer, in `transferred`.
///
/// # Safety
/// `bytes` holds `size` bytes and `transferred` is writable.
#[no_mangle]
pub unsafe extern "C" fn fuji_usb_transfer(
    kind: c_int,
    endpoint: c_int,
    bytes: *mut u8,
    size: c_int,
    timeout: c_uint,
    transferred: *mut c_int,
) -> c_int {
    // SAFETY: the caller passes a writable count.
    unsafe { *transferred = 0 };
    let session = session();
    let Some(interface) = session.claimed.as_ref() else {
        return refusal::TRANSFER;
    };
    if timeout == 0 || timeout > TIMEOUT_CEILING_MS || !size_admitted(kind, size) {
        return refusal::TRANSFER;
    }
    // SAFETY: the caller passes `size` bytes, and `size` is positive.
    let bytes = unsafe { std::slice::from_raw_parts_mut(bytes, size as usize) };
    let timeout = Duration::from_millis(timeout.into());
    let (status, count) = match kind {
        0 if size != CONTROL_BYTES => return refusal::CONTROL_SIZE,
        0 => control(interface, bytes, timeout),
        1 => bulk_out(interface, endpoint as u8, bytes, timeout),
        2 => bulk_in(interface, endpoint as u8, bytes, timeout),
        _ => return refusal::KIND,
    };
    // SAFETY: as above.
    unsafe { *transferred = count as c_int };
    status
}

fn control(interface: &Interface, bytes: &[u8], timeout: Duration) -> (c_int, usize) {
    // bmRequestType 0x43: host to device, vendor, other recipient.
    let request = ControlOut {
        control_type: ControlType::Vendor,
        recipient: Recipient::Other,
        request: 0,
        value: 0,
        index: 0,
        data: bytes,
    };
    match interface.control_out(request, timeout).wait() {
        Ok(()) => (0, bytes.len()),
        Err(error) => (transfer_status(error), 0),
    }
}

fn bulk_out(interface: &Interface, address: u8, bytes: &[u8], timeout: Duration) -> (c_int, usize) {
    let mut endpoint = match interface.endpoint::<Bulk, Out>(address) {
        Ok(endpoint) => endpoint,
        Err(error) => return (error_status(&error), 0),
    };
    let completion = endpoint.transfer_blocking(Buffer::from(&*bytes), timeout);
    completion_outcome(completion.status, completion.actual_len, bytes.len())
}

/// nusb requests whole packets, so a reply longer than `bytes` arrives whole and
/// is reported as libusb reports it: an overflow that fills `bytes`.
fn bulk_in(interface: &Interface, address: u8, bytes: &mut [u8], timeout: Duration) -> (c_int, usize) {
    let mut endpoint = match interface.endpoint::<Bulk, In>(address) {
        Ok(endpoint) => endpoint,
        Err(error) => return (error_status(&error), 0),
    };
    let requested = bytes.len().next_multiple_of(endpoint.max_packet_size().max(1));
    let completion = endpoint.transfer_blocking(endpoint.allocate(requested), timeout);
    let (status, count) = completion_outcome(completion.status, completion.actual_len, bytes.len());
    bytes[..count].copy_from_slice(&completion.buffer[..count]);
    (status, count)
}

fn completion_outcome(status: Result<(), TransferError>, actual: usize, requested: usize) -> (c_int, usize) {
    if actual > requested {
        return (libusb::OVERFLOW, requested);
    }
    (status.map_or_else(transfer_status, |()| 0), actual)
}

#[no_mangle]
pub extern "C" fn fuji_usb_release() -> c_int {
    match session().claimed.take() {
        Some(interface) => interface.release().wait().map_or_else(|error| error_status(&error), |()| 0),
        None => 0,
    }
}

#[no_mangle]
pub extern "C" fn fuji_usb_close() -> c_int {
    let mut session = session();
    session.claimed = None;
    session.opened = None;
    0
}

#[no_mangle]
pub extern "C" fn fuji_usb_exit() -> c_int {
    let mut session = session();
    session.listing = None;
    session.initialized = false;
    session.loaded = false;
    0
}

/// Describes eligible devices only; the others keep their ids alone.
fn inventory<T>(
    devices: &[T],
    ids: impl Fn(&T) -> (u16, u16),
    mut describe: impl FnMut(&T) -> Result<Entry, c_int>,
) -> Result<Vec<Entry>, c_int> {
    if devices.len() > MAX_DEVICES {
        return Err(refusal::TOO_MANY);
    }
    devices
        .iter()
        .map(|device| match ids(device) {
            (VENDOR, PRODUCT) => describe(device),
            (vendor, product) => Ok(Entry::ineligible(vendor, product)),
        })
        .collect()
}

fn describe(info: &DeviceInfo) -> Result<Entry, c_int> {
    let mut entry = Entry::ineligible(info.vendor_id(), info.product_id());
    entry.path = device_path(bus_number(info), info.port_chain())?;
    with_active_configuration(info, |configuration| descriptor_fields(configuration, &mut entry.fields))?;
    Ok(entry)
}

/// `BBB:p.p.p`, libusb's bus number and port chain.
fn device_path(bus: u8, ports: &[u8]) -> Result<String, c_int> {
    if ports.is_empty() || ports.len() > MAX_PORTS {
        return Err(refusal::PORTS);
    }
    let ports: Vec<String> = ports.iter().map(u8::to_string).collect();
    Ok(format!("{bus:03}:{}", ports.join(".")))
}

#[cfg(target_os = "linux")]
fn bus_number(info: &DeviceInfo) -> u8 {
    info.busnum()
}

#[cfg(target_os = "macos")]
fn bus_number(info: &DeviceInfo) -> u8 {
    (info.location_id() >> 24) as u8
}

/// Fills slots 2..=38 from the active configuration, refusing any shape but
/// one interface with one alternate setting, exactly one bulk IN and one bulk
/// OUT, and at most six other endpoints. Slots 8..=11 and 37..=38 are the bulk
/// pair; 12 counts the others and 13.. holds four slots for each.
fn descriptor_fields(configuration: &ConfigurationDescriptor, fields: &mut [c_int; FIELDS]) -> Result<(), c_int> {
    let mut interfaces = configuration.interfaces();
    let interface = match (configuration.num_interfaces(), interfaces.next(), interfaces.next()) {
        (1, Some(interface), None) => interface,
        _ => return Err(refusal::DESCRIPTORS),
    };
    let mut alternates = interface.alt_settings();
    let alternate = match (alternates.next(), alternates.next()) {
        (Some(alternate), None) => alternate,
        _ => return Err(refusal::DESCRIPTORS),
    };
    let endpoints: Vec<_> = alternate.endpoints().collect();
    if alternate.interface_number() != 0
        || alternate.alternate_setting() != 0
        || !(2..=8).contains(&endpoints.len())
        || endpoints.len() != usize::from(alternate.num_endpoints())
    {
        return Err(refusal::DESCRIPTORS);
    }
    let (mut ins, mut outs, mut extras) = (0, 0, 0);
    for endpoint in endpoints {
        let (address, attributes) = (endpoint.address(), endpoint.attributes());
        let packet = c_int::from(endpoint.max_packet_size_raw());
        let interval = c_int::from(endpoint.interval());
        if endpoint.transfer_type() != TransferType::Bulk {
            if extras == MAX_EXTRA_ENDPOINTS {
                return Err(refusal::DESCRIPTORS);
            }
            let slot = 13 + extras * 4;
            fields[slot..slot + 4].copy_from_slice(&[address.into(), attributes.into(), packet, interval]);
            extras += 1;
        } else if attributes != 2 {
            return Err(refusal::DESCRIPTORS);
        } else if address & 0x80 != 0 {
            ins += 1;
            (fields[8], fields[10], fields[37]) = (address.into(), packet, interval);
        } else {
            outs += 1;
            (fields[9], fields[11], fields[38]) = (address.into(), packet, interval);
        }
    }
    if (ins, outs) != (1, 1) {
        return Err(refusal::DESCRIPTORS);
    }
    fields[2] = configuration.configuration_value().into();
    fields[3] = alternate.interface_number().into();
    fields[4] = alternate.alternate_setting().into();
    fields[5] = alternate.class().into();
    fields[6] = alternate.subclass().into();
    fields[7] = alternate.protocol().into();
    fields[12] = extras as c_int;
    Ok(())
}

/// The active configuration from the kernel's cached descriptors in sysfs.
#[cfg(target_os = "linux")]
fn with_active_configuration<T>(
    info: &DeviceInfo,
    read: impl FnOnce(&ConfigurationDescriptor) -> Result<T, c_int>,
) -> Result<T, c_int> {
    let directory = info.sysfs_path();
    let value = std::fs::read_to_string(directory.join("bConfigurationValue")).map_err(|error| io_status(&error))?;
    let value: u8 = value.trim().parse().map_err(|_| libusb::NOT_FOUND)?;
    let descriptors = std::fs::read(directory.join("descriptors")).map_err(|error| io_status(&error))?;
    let configuration = configurations(&descriptors)
        .find(|configuration| configuration.configuration_value() == value)
        .ok_or(libusb::NOT_FOUND)?;
    read(&configuration)
}

/// The active configuration of the eligible device, opened for the read.
#[cfg(target_os = "macos")]
fn with_active_configuration<T>(
    info: &DeviceInfo,
    read: impl FnOnce(&ConfigurationDescriptor) -> Result<T, c_int>,
) -> Result<T, c_int> {
    let device = info.open().wait().map_err(|error| error_status(&error))?;
    let configuration = device.active_configuration().map_err(|_| libusb::NOT_FOUND)?;
    read(&configuration)
}

/// The configuration descriptors that follow the 18-byte device descriptor in
/// a sysfs `descriptors` file.
#[cfg(any(target_os = "linux", test))]
fn configurations(descriptors: &[u8]) -> impl Iterator<Item = ConfigurationDescriptor<'_>> {
    let mut rest = descriptors.get(18..).unwrap_or_default();
    std::iter::from_fn(move || {
        let total = usize::from(u16::from_le_bytes([*rest.get(2)?, *rest.get(3)?]));
        let configuration = rest.get(..total.max(1))?;
        rest = &rest[configuration.len()..];
        ConfigurationDescriptor::new(configuration)
    })
}

/// Whether a kernel driver is bound to interface 0 of the active configuration,
/// read from sysfs.
#[cfg(target_os = "linux")]
fn kernel_driver_bound(info: &DeviceInfo) -> Result<bool, c_int> {
    let directory = info.sysfs_path();
    let value = std::fs::read_to_string(directory.join("bConfigurationValue")).map_err(|error| io_status(&error))?;
    let name = directory.file_name().ok_or(libusb::NOT_FOUND)?.to_string_lossy();
    let interface = directory.join(format!("{name}:{}.0", value.trim()));
    Ok(interface.join("driver").exists())
}

/// Not checked on macOS: an interface a driver or another process holds fails
/// the claim as busy.
#[cfg(target_os = "macos")]
fn kernel_driver_bound(_info: &DeviceInfo) -> Result<bool, c_int> {
    Ok(false)
}

fn error_status(error: &nusb::Error) -> c_int {
    match error.kind() {
        ErrorKind::Disconnected => libusb::NO_DEVICE,
        ErrorKind::Busy => libusb::BUSY,
        ErrorKind::PermissionDenied => libusb::ACCESS,
        ErrorKind::NotFound => libusb::NOT_FOUND,
        ErrorKind::Unsupported => libusb::NOT_SUPPORTED,
        _ => libusb::OTHER,
    }
}

#[cfg(target_os = "linux")]
fn io_status(error: &std::io::Error) -> c_int {
    match error.kind() {
        std::io::ErrorKind::NotFound => libusb::NO_DEVICE,
        std::io::ErrorKind::PermissionDenied => libusb::ACCESS,
        _ => libusb::IO,
    }
}

fn transfer_status(error: TransferError) -> c_int {
    match error {
        TransferError::Cancelled => libusb::TIMEOUT,
        TransferError::Stall => libusb::PIPE,
        TransferError::Disconnected => libusb::NO_DEVICE,
        TransferError::Fault => libusb::IO,
        TransferError::InvalidArgument => libusb::INVALID_PARAM,
        _ => libusb::OTHER,
    }
}

#[cfg(test)]
mod tests;
