use super::{AccessPoint, Configuration, Error, Security, State};
use crate::vendor_wireless_access_point_authenticator as auth;
use crate::vendor_wireless_network::MacAddress;
use crate::wireless_association::station::{WpsPin, crypto::random::{RandomPool, RtosClock}, wps::{WpsSecurity, registrar::{Registrar, RegistrarCredentials, RegistrarEffect, RegistrarOutcome}}};
use xm1_rtos_own_first::Machine;
use crate::vendor_wireless_peers::{PeerPool, PEER_CAPACITY};

pub enum WpsMethod { PushButton, Pin(WpsPin) }
impl WpsMethod {
    pub fn parse(payload: &[u8]) -> Result<Self, Error> {
        if payload.len() < 32 { return Err(Error::Truncated); }
        match super::word(payload, 0)? {
            0 => Ok(Self::PushButton),
            1 => {
                let length = super::word(payload, 12)? as usize;
                if length > 8 { return Err(Error::Key); }
                Ok(Self::Pin(WpsPin::parse(&payload[4..4 + length]).map_err(|_| Error::Key)?))
            }
            _ => Err(Error::Security),
        }
    }
}
#[derive(Debug)]
pub enum SecurityEffect { Authenticator(auth::Effect), Registrar(RegistrarEffect), WpsOutcome(RegistrarOutcome) }
impl AccessPoint {
    pub(super) fn prepare_security(&mut self, configuration: &Configuration) -> Result<(), Error> {
        self.authenticator = match configuration.security {
            Security::WpaTkip | Security::Wpa2Ccmp => Some(auth::Authenticator::new(self.local,
                if configuration.security == Security::WpaTkip { auth::Protocol::Wpa } else { auth::Protocol::Wpa2 },
                if configuration.security == Security::WpaTkip { auth::Cipher::Tkip } else { auth::Cipher::Ccmp },
                &configuration.ssid, configuration.key.as_bytes()).map_err(|_| Error::Security)?),
            _ => None,
        };
        self.auth_initialized = false;
        self.registrar_io_pending = false;
        let security = match configuration.security { Security::Open => WpsSecurity::Open, Security::Wep => WpsSecurity::Wep,
            Security::WpaTkip => WpsSecurity::Wpa, Security::Wpa2Ccmp => WpsSecurity::Wpa2 };
        let credentials = RegistrarCredentials::new(configuration.ssid, security, configuration.key.as_bytes(), configuration.key_index.get()).map_err(|_| Error::Security)?;
        let registrar = Registrar::new(self.local, credentials);
        let advertisement = registrar.initial_advertisement().map_err(|_| Error::Security)?;
        self.registrar = Some(registrar);
        self.wps_elements.clear();
        self.wps_elements.append(advertisement.as_bytes()).map_err(|_| Error::Capacity)?;
        Ok(())
    }
    pub fn start_wps(&mut self, method: WpsMethod) -> Result<(), Error> {
        if self.state != State::Running { return Err(Error::NotRunning); }
        let registrar = self.registrar.as_mut().ok_or(Error::Security)?;
        match method { WpsMethod::PushButton => registrar.start_pbc(None), WpsMethod::Pin(pin) => registrar.start_pin(pin, None) }.map_err(|_| Error::Security)
    }
    pub fn cancel_wps(&mut self) { if let Some(registrar) = self.registrar.as_mut() { registrar.cancel(); } }
    pub fn wps_timeout(&mut self) -> Result<(), Error> { self.registrar.as_mut().ok_or(Error::Security)?.timeout().map_err(|_| Error::Security) }
    pub fn retry_wps(&mut self) -> Result<(), Error> { self.registrar.as_mut().ok_or(Error::Security)?.retry().map_err(|_| Error::Security) }
    pub fn receive_eapol<M: Machine>(&mut self, peer: MacAddress, bytes: &[u8], now: auth::Milliseconds, random: &mut RandomPool, clock: &RtosClock<'_, M>) -> Result<(), Error> {
        if self.state != State::Running { return Err(Error::NotRunning); }
        if !self.peers.iter().flatten().any(|entry| entry.peer.address == peer && entry.associated) { return Err(Error::UnknownPeer); }
        if bytes.get(1) == Some(&3) {
            self.authenticator.as_mut().ok_or(Error::Security)?.receive_eapol(peer, bytes, now, random, clock).map_err(|_| Error::Security)
        } else {
            self.registrar.as_mut().ok_or(Error::Security)?.receive_eapol(peer, bytes).map_err(|_| Error::Security)
        }
    }
    pub fn security_tick<M: Machine>(&mut self, now: auth::Milliseconds, random: &mut RandomPool, clock: &RtosClock<'_, M>) -> Result<(), Error> {
        if let Some(authenticator) = self.authenticator.as_mut() { authenticator.tick(now, random, clock).map_err(|_| Error::Security)?; }
        Ok(())
    }
    pub fn poll_security<M: Machine>(&mut self, random: &mut RandomPool, clock: &RtosClock<'_, M>) -> Result<Option<SecurityEffect>, Error> {
        if let Some(authenticator) = self.authenticator.as_mut() {
            if !self.auth_initialized && self.state == State::Starting {
                authenticator.initialize(random, clock).map_err(|_| Error::Security)?;
                self.auth_initialized = true;
            }
            if let Some(effect) = authenticator.next_effect() { return Ok(Some(SecurityEffect::Authenticator(effect))); }
        }
        if self.registrar_io_pending || self.beacon_dirty && self.state == State::Running || self.pending == Some(super::Operation::UpdateBeacon) { return Ok(None); }
        loop {
            let Some(registrar) = self.registrar.as_mut() else { return Ok(None); };
            match registrar.poll() {
                Some(RegistrarEffect::Entropy) => {
                    let mut bytes = [0; 32];
                    random.generate(clock, &mut bytes).map_err(|_| Error::Random)?;
                    let result = registrar.supply_entropy(bytes).map_err(|_| Error::Security);
                    bytes.fill(0); result?;
                }
                Some(RegistrarEffect::SelectedRegistrar { elements }) => {
                    self.wps_elements.clear();
                    self.wps_elements.append(elements.as_bytes()).map_err(|_| Error::Capacity)?;
                    self.beacon_dirty = true;
                    return Ok(None);
                }
                Some(effect) => { self.registrar_io_pending = true; return Ok(Some(SecurityEffect::Registrar(effect))); }
                None => return Ok(registrar.take_outcome().map(SecurityEffect::WpsOutcome)),
            }
        }
    }
    pub fn poll_teardown_security(&mut self) -> Result<Option<SecurityEffect>, Error> {
        if self.state != State::Stopping { return Err(Error::Busy); }
        if let Some(authenticator) = self.authenticator.as_mut() {
            if let Some(effect) = authenticator.next_effect() { return Ok(Some(SecurityEffect::Authenticator(effect))); }
        }
        if self.registrar_io_pending { return Ok(None); }
        loop {
            let Some(registrar) = self.registrar.as_mut() else { return Ok(None); };
            match registrar.poll() {
                Some(RegistrarEffect::SelectedRegistrar { .. }) => {}
                Some(RegistrarEffect::Entropy) => return Err(Error::Security),
                Some(effect) => { self.registrar_io_pending = true; return Ok(Some(SecurityEffect::Registrar(effect))); }
                None => return Ok(registrar.take_outcome().map(SecurityEffect::WpsOutcome)),
            }
        }
    }
    pub fn complete_registrar_io(&mut self, success: bool) -> Result<(), Error> {
        if !self.registrar_io_pending { return Err(Error::UnexpectedCompletion); }
        self.registrar_io_pending = false;
        if !success { self.failure = Some(Error::Security); self.stop()?; return Err(Error::Security); }
        Ok(())
    }
    pub fn complete_authenticator(&mut self, id: auth::EffectId, success: bool, pool: &mut PeerPool) -> Result<(), Error> {
        let authenticator = self.authenticator.as_mut().ok_or(Error::Security)?;
        if authenticator.effect_complete(id, success).is_err() {
            self.failure = Some(Error::Security); self.stop()?; return Err(Error::Security);
        }
        let mut authorized = [None; PEER_CAPACITY];
        for (slot, entry) in authorized.iter_mut().zip(self.peers.iter().flatten()) {
            if !entry.published && authenticator.is_authorized(entry.peer.address) { *slot = Some(entry.peer.address); }
        }
        for peer in authorized.into_iter().flatten() { self.authorized(peer, pool)?; }
        Ok(())
    }
    pub fn security_io_failed(&mut self) -> Result<(), Error> { self.failure = Some(Error::Security); self.stop() }
}
