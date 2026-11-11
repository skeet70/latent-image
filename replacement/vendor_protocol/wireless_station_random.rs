use super::{DriverAction, DriverCommand, DriverReply, ReplyPayload, StationDrive, StationError, StationOperationId, StationRuntime};
use super::crypto::random::{RandomPool, RtosClock};
use super::super::super::vendor_wireless_station_driver::StationDriver;
use xm1_rtos_own_first::Machine;

impl DriverCommand {
    pub fn generate_nonce<M: Machine>(self, pool: &mut RandomPool, clock: &RtosClock<'_, M>) -> Result<DriverReply, StationError> {
        if !matches!(self.action(), DriverAction::RandomNonce) { return Err(StationError::WrongReply); }
        let mut bytes = [0; 32];
        let payload = match pool.generate(clock, &mut bytes) {
            Ok(()) => ReplyPayload::Nonce(bytes),
            Err(error) => ReplyPayload::RandomFailure(error),
        };
        Ok(DriverReply { id: self.id(), payload })
    }
}
impl StationRuntime {
    pub fn drive_with_clock<M: Machine>(&mut self, driver: &mut StationDriver, pool: &mut RandomPool, clock: &RtosClock<'_, M>) -> Result<StationDrive, StationError> {
        match self.drive(driver)? {
            StationDrive::Clock(command) => { self.receive(command.generate_nonce(pool, clock)?)?; Ok(StationDrive::Advanced) }
            outcome => Ok(outcome),
        }
    }
    pub fn receive_eapol_with_clock<M: Machine>(&mut self, operation: StationOperationId, peer: super::MacAddress, frame: &[u8], pool: &mut RandomPool, clock: &RtosClock<'_, M>) -> Result<(), StationError> {
        pool.add_randomness(clock, &peer.0).map_err(StationError::Random)?;
        self.receive_eapol(operation, peer, frame)
    }
}
