use super::{Authentication, Continuation, DriverAction, Encryption, InformationElements, InterfaceMode, ScanBatch, StationError, StationOperationId, StationOutcome, StationRuntime, StationState, TimerDurationMs, WpsConfiguration, WpsError, WpsMethod};
use super::wps::{WpsEffect, WpsEnrollee, WpsOutcome};

impl StationRuntime {
    pub fn wps_configuration(&self) -> Option<WpsConfiguration> {
        self.provisioning.as_ref().map(WpsEnrollee::configuration).or(self.last_wps_configuration)
    }
    pub fn wps_credential(&self) -> Option<&super::wps::WpsCredential> {
        self.provisioning.as_ref().and_then(WpsEnrollee::credential).or(self.wps_credential.as_ref())
    }
    pub(super) fn start_provisioning(&mut self, configuration: WpsConfiguration) -> Result<(), StationError> {
        self.session = None; self.supplicant = None; self.wps_credential = None;
        self.last_wps_configuration = Some(configuration);
        let mut enrollee = WpsEnrollee::new(self.local, configuration).map_err(StationError::Wps)?;
        enrollee.start().map_err(StationError::Wps)?;
        self.provisioning = Some(enrollee);
        self.state = StationState::Provisioning;
        self.enqueue(DriverAction::Disconnect, Continuation::Next)?;
        self.enqueue_key_removal()?;
        self.pump_provisioning()
    }
    pub(super) fn receive_wps_scan(&mut self, results: ScanBatch) -> Result<(), StationError> {
        let enrollee = self.provisioning.as_mut().ok_or(StationError::WrongReply)?;
        let mut selected = None;
        let mut overlap = false;
        for candidate in results.candidates() {
            if !enrollee.scan_eligible(candidate.access_point.address, candidate.access_point.advertised_elements.as_bytes()).map_err(StationError::Wps)? { continue; }
            if enrollee.configuration().method == WpsMethod::Pbc && selected.as_ref().is_some_and(|ap: &super::SelectedAccessPoint| ap.address != candidate.access_point.address) {
                overlap = true;
                break;
            }
            if selected.is_none() { selected = Some(candidate.access_point.clone()); }
        }
        self.scan_results = results;
        if overlap {
            let error = StationError::Wps(WpsError::Overlap);
            self.begin_cleanup(StationOutcome::Failed(error))?;
            return Err(error);
        }
        if let Some(access_point) = selected {
            enrollee.select(access_point.address).map_err(StationError::Wps)?;
            self.access_point = Some(access_point);
        } else { enrollee.retry_scan().map_err(StationError::Wps)?; }
        Ok(())
    }
    pub(super) fn pump_provisioning(&mut self) -> Result<(), StationError> {
        if self.pending.is_some() || self.queued != 0 { return Ok(()); }
        let Some(enrollee) = self.provisioning.as_mut() else { return Ok(()); };
        let effect = enrollee.poll();
        match effect {
            Some(WpsEffect::Entropy) => self.enqueue(DriverAction::RandomNonce, Continuation::Nonce)?,
            Some(WpsEffect::Scan) => self.enqueue(DriverAction::Scan { ssid: None }, Continuation::Scan)?,
            Some(WpsEffect::Associate { peer }) => {
                let elements = InformationElements::parse(enrollee.association_elements()).map_err(StationError::Profile)?;
                self.enqueue(DriverAction::SetInterfaceMode(InterfaceMode::Infrastructure), Continuation::Next)?;
                self.enqueue(DriverAction::SetAuthentication(Authentication::Open), Continuation::Next)?;
                self.enqueue(DriverAction::SetEncryption(Encryption::Disabled), Continuation::Next)?;
                self.enqueue(DriverAction::SetSecurityElements(elements), Continuation::Next)?;
                self.enqueue(DriverAction::SelectBssid(peer), Continuation::Association)?;
            }
            Some(WpsEffect::Transmit { peer, frame }) => self.enqueue(DriverAction::TransmitWps { destination: peer, frame }, Continuation::Next)?,
            Some(WpsEffect::ArmTimer { milliseconds }) => self.enqueue(DriverAction::ArmWpsTimer(TimerDurationMs(milliseconds)), Continuation::Next)?,
            Some(WpsEffect::CancelTimer) => self.enqueue(DriverAction::CancelWpsTimer, Continuation::Next)?,
            Some(WpsEffect::Disconnect) => self.enqueue(DriverAction::Disconnect, Continuation::Next)?,
            None => {
                if let Some(outcome) = enrollee.take_outcome() {
                    self.last_wps_configuration = Some(enrollee.configuration());
                    self.wps_credential = enrollee.credential().cloned();
                    self.provisioning = None;
                    match outcome {
                        WpsOutcome::Credentials(profile) => { self.profile = Some(profile); self.begin_selection()?; }
                        WpsOutcome::Cancelled => self.begin_cleanup(StationOutcome::Cancelled)?,
                        WpsOutcome::Failed(error) => self.begin_cleanup(StationOutcome::Failed(StationError::Wps(error)))?,
                    }
                }
            }
        }
        Ok(())
    }
    pub fn wps_timeout(&mut self, operation: StationOperationId) -> Result<(), StationError> {
        if self.operation != Some(operation) { return Err(StationError::StaleReply); }
        self.provisioning.as_mut().ok_or(StationError::WrongReply)?.timeout().map_err(StationError::Wps)?;
        self.pump_provisioning()
    }
}
