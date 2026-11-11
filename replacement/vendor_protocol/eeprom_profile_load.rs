//! X-M1 1.50 task 100 provider, source pin 55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::eeprom_profile_medium::ProfileSlot;
use crate::eeprom_task100::{AdmissionError, EepromTask100};
use crate::eeprom_detection::DetectionIo;
use xm1_lens_transport::wire::{InterruptController, Mmio, Platform};
use xm1_power_services::shutdown::RequestId;
use xm1_power_services::storage_shutdown_actor::MediumStatus;
use xm1_power_services::storage_shutdown_defaults::DefaultStorageIo;
use xm1_power_services::storage_shutdown_lifecycle::{LensCompletion, LensDmaRegister, LensEepromSoftware, LensTransferPlan};
use xm1_power_services::storage_shutdown_load::{DefaultBank, DescriptorStatus, LensChannel, LensLoadSoftware, LoadDiagnostic};
use xm1_power_services::storage_shutdown_medium::NandSoftware;

impl<M: Mmio, P: Platform, I: InterruptController, N: NandSoftware, D: DefaultStorageIo, B: DetectionIo>
    LensLoadSoftware for EepromTask100<'_, '_, M, P, I, N, D, B>
{
    fn software_status(&self) -> MediumStatus {
        MediumStatus(if self.events.software_failure().is_some() { -255 } else { 0 })
    }
    fn request_channel(&mut self, channel: LensChannel) {
        self.control(RequestId(0x722e), &[0x3000 | (u32::from(channel.abi() & 0xf) << 16)], LensCompletion::Information);
    }
    fn lens_available(&mut self) -> bool { self.task99.available() }
    fn snapshot_descriptor(&mut self, destination: &mut [u8; 0x80]) -> DescriptorStatus {
        DescriptorStatus(self.task99.snapshot_descriptor(destination).0)
    }
    async fn delay_ticks(&mut self, ticks: u32) { self.events.delay(ticks).await; }
    fn configuration_variant(&mut self) -> [u8; 2] {
        [
            self.configuration.byte(0x204).expect("admitted configuration extent"),
            self.configuration.byte(0x205).expect("admitted configuration extent"),
        ]
    }
    async fn synchronize_lens(&mut self) {
        if !matches!(self.task99.synchronize().await, Ok(status) if status.0 == 0) {
            self.events.fail_software(AdmissionError::Transport);
        }
    }
    fn read_cached_profile(&mut self, slot: u8) -> MediumStatus {
        let Some(slot) = ProfileSlot::new(slot) else {
            self.events.fail_software(AdmissionError::Configuration);
            return MediumStatus(-255);
        };
        self.profile.read_profile(slot, self.configuration)
    }
    fn release_lens(&mut self) {
        if self.task99.release().is_err() { self.events.fail_software(AdmissionError::Transport); }
    }
    fn reset_lens(&mut self) {
        if self.task99.reset_lens().is_err() { self.events.fail_software(AdmissionError::Transport); }
    }
    fn read_defaults(&mut self, bank: DefaultBank) {
        if self.profile.read_defaults(bank, self.configuration).0 != 0 {
            self.events.fail_software(AdmissionError::Configuration);
        }
    }
    fn diagnostic(&mut self, diagnostic: LoadDiagnostic) { self.last_load_diagnostic = Some(diagnostic); }
    fn prepare_output(&mut self, unregistered: bool) {
        if self.startup.prepare_output(self.configuration, unregistered).is_err() {
            self.events.fail_software(AdmissionError::Configuration);
        }
    }
    fn publish_configuration(&mut self) {
        if self.startup.publish_configuration(self.configuration).is_err() {
            self.events.fail_software(AdmissionError::Configuration);
        }
    }
    async fn finish_load(&mut self) {
        if self.task99.finish().is_err() { self.events.fail_software(AdmissionError::Transport); }
    }
}

impl<M: Mmio, P: Platform, I: InterruptController, N: NandSoftware, D: DefaultStorageIo, B: DetectionIo>
    LensEepromSoftware for EepromTask100<'_, '_, M, P, I, N, D, B>
{
    fn prepare_service(&mut self) {
        if self.detection.prepare_service().is_err() { self.events.fail_software(AdmissionError::Transport); }
    }
    fn register_dma_callback(&mut self, enabled: bool) {
        if enabled {
            match self.events.arm(LensCompletion::Dma) {
                Ok(callback) => {
                    if self.detection.register_dma_callback(Some(callback)).is_err() {
                        self.events.cancel_dma();
                        self.events.fail_software(AdmissionError::Transport);
                    }
                }
                Err(error) => self.events.fail_software(error),
            }
        } else {
            if self.detection.register_dma_callback(None).is_err() { self.events.fail_software(AdmissionError::Transport); }
            self.events.cancel_dma();
        }
    }
    fn prepare_primary(&mut self) {
        self.control(RequestId(0x722b), &[0x80102800], LensCompletion::Ready);
    }
    fn prepare_auxiliary(&mut self) {
        self.control(RequestId(0x7234), &[0x4540], LensCompletion::Auxiliary);
    }
    fn select_auxiliary(&mut self, plan: LensTransferPlan) {
        let selector = 0x80000600 | (u32::from(plan.pages & 0x7ff) << 16) | ((plan.operation.abi() & 0xf) << 27);
        self.control(RequestId(0x7258), &[selector, 0x2480], LensCompletion::Auxiliary);
    }
    fn select_read(&mut self, plan: LensTransferPlan) {
        let selector = 0x80000600 | (u32::from(plan.pages & 0x7ff) << 16) | ((plan.operation.abi() & 0xf) << 27);
        self.control(RequestId(0x7259), &[selector, 0x2440], LensCompletion::Ready);
    }
    fn read_ready(&mut self) { self.control(RequestId(0x7257), &[0x2400], LensCompletion::Ready); }
    fn request_extended(&mut self, scratch: &mut [u8; 4]) {
        self.control(RequestId(0x7233), &[0x4500 | (u32::from(scratch[0] & 0xf) << 16)], LensCompletion::Extended);
    }
    fn auxiliary_size(&mut self) -> u16 { self.task99.auxiliary_size() }
    fn write_dma_register(&mut self, register: LensDmaRegister, value: u32) { self.detection.write_dma_register(register, value); }
}
