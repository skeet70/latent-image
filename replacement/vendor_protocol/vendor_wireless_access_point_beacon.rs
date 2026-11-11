use super::{Configuration, Error, MacAddress, Security};
use crate::vendor_wireless_station_driver::association::frames::Packet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rate(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtCapabilities {
    pub information: u16,
    pub ampdu: u8,
    pub mcs: [u8; 16],
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HardwareCapabilities {
    rates: [Rate; 32],
    rate_count: u8,
    pub ht: Option<HtCapabilities>,
    pub short_preamble: bool,
    pub short_slot: bool,
}
impl HardwareCapabilities {
    pub fn new(rates: &[Rate], ht: Option<HtCapabilities>, short_preamble: bool, short_slot: bool) -> Result<Self, Error> {
        if rates.is_empty() || rates.len() > 32 || rates.iter().any(|rate| rate.0 & 127 == 0) { return Err(Error::Capabilities); }
        let mut result = Self { rates: [Rate(0); 32], rate_count: rates.len() as u8, ht, short_preamble, short_slot };
        result.rates[..rates.len()].copy_from_slice(rates);
        Ok(result)
    }
    pub fn rates(&self) -> &[Rate] { &self.rates[..usize::from(self.rate_count)] }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Protection {
    pub non_erp: bool,
    pub use_protection: bool,
    pub long_preamble: bool,
    pub non_ht: bool,
    pub non_greenfield: bool,
    pub no_short_slot: bool,
}
#[derive(Debug, Eq, PartialEq)]
pub struct BeaconTemplate { pub head: Packet, pub tail: Packet, pub interval: u16, pub dtim_period: u8 }
impl BeaconTemplate {
    pub fn build(local: MacAddress, configuration: &Configuration, hardware: &HardwareCapabilities,
        protection: Protection, interval: u16, security_elements: &[u8], wps_elements: &[u8]) -> Result<Self, Error> {
        let mut head = Packet::new();
        head.header(0x80, MacAddress([255; 6]), local, local);
        head.append(&[0; 8]).map_err(|_| Error::Capacity)?;
        head.append(&interval.to_le_bytes()).map_err(|_| Error::Capacity)?;
        head.append(&capabilities(configuration, hardware, protection).to_le_bytes()).map_err(|_| Error::Capacity)?;
        head.element(0, configuration.ssid.as_bytes()).map_err(|_| Error::Capacity)?;
        let mut rates = [0; 32];
        for (out, rate) in rates.iter_mut().zip(hardware.rates()) { *out = rate.0; }
        head.element(1, &rates[..hardware.rates().len().min(8)]).map_err(|_| Error::Capacity)?;
        head.element(3, &[configuration.channel.number()]).map_err(|_| Error::Capacity)?;
        let mut tail = Packet::new();
        tail.element(42, &[u8::from(protection.non_erp) | (u8::from(protection.use_protection) << 1) | (u8::from(protection.long_preamble) << 2)]).map_err(|_| Error::Capacity)?;
        if hardware.rates().len() > 8 { tail.element(50, &rates[8..hardware.rates().len()]).map_err(|_| Error::Capacity)?; }
        tail.append(security_elements).map_err(|_| Error::Capacity)?;
        if !configuration.disable_11n && configuration.security != Security::WpaTkip {
            if let Some(ht) = hardware.ht {
                let mut capabilities = [0; 26];
                capabilities[..2].copy_from_slice(&ht.information.to_le_bytes()); capabilities[2] = ht.ampdu;
                capabilities[3..19].copy_from_slice(&ht.mcs);
                tail.element(45, &capabilities).map_err(|_| Error::Capacity)?;
                let mut operation = [0; 22]; operation[0] = configuration.channel.number();
                operation[2] = if protection.non_ht { 0x13 } else { 0 } | (u8::from(protection.non_greenfield) << 2);
                tail.element(61, &operation).map_err(|_| Error::Capacity)?;
            }
        }
        if hardware.ht.is_some() && configuration.security != Security::WpaTkip {
            tail.element(221, &[0,80,242,2,1,1,0,0,3,0xa4,0,0,0x27,0xa4,0,0,0x52,0x43,0x5d,0,0x72,0x32,0x2e,0]).map_err(|_| Error::Capacity)?;
        }
        tail.append(wps_elements).map_err(|_| Error::Capacity)?;
        Ok(Self { head, tail, interval, dtim_period: 2 })
    }
}
pub(crate) fn capabilities(configuration: &Configuration, hardware: &HardwareCapabilities, protection: Protection) -> u16 {
    1 | if configuration.security != Security::Open { 16 } else { 0 }
        | if hardware.short_preamble && !protection.long_preamble { 32 } else { 0 }
        | if hardware.short_slot && !protection.no_short_slot { 1024 } else { 0 }
}
