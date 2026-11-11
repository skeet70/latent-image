//! Action 37, target 0xdbcf80, from X-M1 pin 55436fc85c0560dd252b0410af6f2427fe198cd3.
//! The NONMATCHING definition is not the specification. The authenticated 284-byte extent
//! has SHA-256 183a368ec4d103fcb678bf721461ee3d374b0d8a628fa2f974831a6141df9958.
//! Event registration, ECC and OOB extraction belong to application_nand.

use crate::{
    backend::{BackendError, ConfigImages},
    configuration_actions_low::ActionCompletion,
    configuration_actions_middle::MiddleArguments,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpareSelection { DataOnly, Include }

impl SpareSelection {
    fn parse(value: u8) -> Option<Self> {
        match value { 0 => Some(Self::DataOnly), 1 => Some(Self::Include), _ => None }
    }
    const fn abi(self) -> u32 { match self { Self::DataOnly => 0, Self::Include => 1 } }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BlockGroup(u8);
impl BlockGroup { const fn first_block(self) -> u32 { (self.0 as u32) << 3 } }

pub const DESTINATION_ADDRESS: u32 = 0x4b2e_ce00;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NandEventRequest { spare: SpareSelection, group: BlockGroup }
impl NandEventRequest {
    pub fn parse(arguments: MiddleArguments<'_>, configuration: &ConfigImages<'_>) -> Result<Option<Self>, BackendError> {
        let spare = arguments.read(0, configuration)?;
        let group = BlockGroup(arguments.read(1, configuration)?);
        Ok(SpareSelection::parse(spare).map(|spare| Self { spare, group }))
    }
    pub const fn first_block(self) -> u32 { self.group.first_block() }
    pub const fn spare(self) -> SpareSelection { self.spare }
    pub const fn output_bytes(self) -> usize {
        match self.spare { SpareSelection::DataOnly => 0x100000, SpareSelection::Include => 0x108000 }
    }
    fn native_request(self) -> [u32; 6] {
        [self.first_block(), 8, DESTINATION_ADDRESS, self.spare.abi(), 1, 2000]
    }
}

#[cfg(not(test))]
mod native {
    extern "C" {
        pub fn xm1_app_nand_configure(request: *const [u32; 2]) -> i32;
        pub fn xm1_app_nand_read_physical(request: *mut [u32; 6]) -> i32;
        pub fn xm1_app_nand_shutdown();
    }
}

/// # Safety
/// The caller exclusively owns NAND and the writable DMA destination at 0x4b2ece00
/// through 0x4b3f4dff, and satisfies application_nand's native OSAL contract.
/// Cancellation is observed between requests, never by concurrent controller shutdown.
pub unsafe fn execute(
    arguments: MiddleArguments<'_>,
    configuration: &mut ConfigImages<'_>,
) -> Result<ActionCompletion, BackendError> {
    let Some(request) = NandEventRequest::parse(arguments, configuration)? else {
        arguments.write(2, configuration, 1)?;
        return Ok(ActionCompletion::service_rejected());
    };
    let mut status = unsafe { native::xm1_app_nand_configure(&[1, 2000]) };
    if status == 0 {
        let mut request = request.native_request();
        status = unsafe { native::xm1_app_nand_read_physical(&mut request) };
        unsafe { native::xm1_app_nand_shutdown(); }
    }
    let result = match status { 0 => 0, -30 => 1, _ => 2 };
    arguments.write(2, configuration, result)?;
    Ok(if status == 0 { ActionCompletion::service_success() } else { ActionCompletion::service_rejected() })
}

pub struct NandEventDestination<'a> { bytes: &'a mut [u8] }
impl<'a> NandEventDestination<'a> {
    pub fn adopt(address: u32, bytes: &'a mut [u8]) -> Result<Self, BackendError> {
        if address != DESTINATION_ADDRESS { return Err(BackendError::Range); }
        if bytes.len() < 0x100000 { return Err(BackendError::Capacity); }
        Ok(Self { bytes })
    }
    pub fn bytes(&self) -> &[u8] { self.bytes }
}

pub fn execute_host(arguments: MiddleArguments<'_>, configuration: &mut ConfigImages<'_>,
    nand: &mut crate::application_nand::memory::MemoryNand<'_>, destination: &mut NandEventDestination<'_>,
) -> Result<ActionCompletion, BackendError> {
    use crate::application_nand::{PhysicalBlock, physical::{PhysicalBlockCount, SpareLayout}};
    let Some(request) = NandEventRequest::parse(arguments, configuration)? else {
        arguments.write(2, configuration, 1)?;
        return Ok(ActionCompletion::service_rejected());
    };
    let output = destination.bytes.get_mut(..request.output_bytes()).ok_or(BackendError::Capacity)?;
    let result = PhysicalBlock::new(request.first_block()).and_then(|first| {
        let count = PhysicalBlockCount::new(8)?;
        let layout = match request.spare() {
            SpareSelection::DataOnly => SpareLayout::DataOnly, SpareSelection::Include => SpareLayout::Interleaved,
        };
        nand.read_with_spare(first, count, layout, output)
    });
    let status = match &result { Ok(()) => 0, Err(error) if error.request_status() == -30 => 1, Err(_) => 2 };
    arguments.write(2, configuration, status)?;
    Ok(if result.is_ok() { ActionCompletion::service_success() } else { ActionCompletion::service_rejected() })
}

#[cfg(test)]
mod native {
    use std::cell::Cell;
    std::thread_local! {
        pub static CONFIGURE_STATUS: Cell<i32> = const { Cell::new(0) };
        pub static READ_STATUS: Cell<i32> = const { Cell::new(0) };
    }
    pub unsafe fn xm1_app_nand_configure(_: *const [u32; 2]) -> i32 { CONFIGURE_STATUS.with(Cell::get) }
    pub unsafe fn xm1_app_nand_read_physical(_: *mut [u32; 6]) -> i32 { READ_STATUS.with(Cell::get) }
    pub unsafe fn xm1_app_nand_shutdown() {}
}

#[cfg(test)]
mod tests {
    use crate::{backend::{BackendError, ConfigImages}, configuration_actions_low::ActionCompletion, configuration_actions_middle::MiddleArguments};

    fn run(spare: u8, group: u8, configure: i32, read: i32) -> (ActionCompletion, u8) {
        super::native::CONFIGURE_STATUS.with(|status| status.set(configure));
        super::native::READ_STATUS.with(|status| status.set(read));
        let mut bytes = [spare, group, 0xff];
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        let result = unsafe { super::execute(MiddleArguments::from_abi(&[0, 0, 1, 0, 2, 0]), &mut configuration) }.unwrap();
        (result, configuration.byte(2).unwrap())
    }

    #[test]
    fn successful_data_and_spare_requests_clear_status() {
        for spare in [0, 1] {
            assert_eq!(run(spare, 0, 0, 0), (ActionCompletion::service_success(), 0));
        }
    }

    #[test]
    fn invalid_spare_selection_is_a_parameter_rejection() {
        for spare in [2, 255] {
            assert_eq!(run(spare, 0, -255, -255), (ActionCompletion::service_rejected(), 1));
        }
    }

    #[test]
    fn driver_range_errors_are_parameter_rejections() {
        assert_eq!(run(1, 128, 0, -30), (ActionCompletion::service_rejected(), 1));
        assert_eq!(run(1, 0, -30, 0), (ActionCompletion::service_rejected(), 1));
    }

    #[test]
    fn initialization_and_read_errors_report_device_failure() {
        for error in [-10, -255, -40] {
            assert_eq!(run(1, 0, error, 0), (ActionCompletion::service_rejected(), 2));
            assert_eq!(run(1, 0, 0, error), (ActionCompletion::service_rejected(), 2));
        }
    }

    #[test]
    fn truncated_arguments_fail_before_hardware_admission() {
        let mut bytes = [0, 0, 0xff];
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        assert_eq!(unsafe { super::execute(MiddleArguments::from_abi(&[0]), &mut configuration) }, Err(BackendError::Capacity));
    }

    #[test]
    fn host_read_transfers_real_nand_data_and_preserves_unwritten_spare_padding() {
        use crate::application_nand::{PhysicalBlock, BLOCK_BYTES, PAGES_PER_BLOCK,
            memory::{BlockFailures, HostConfiguration, ImageProvenance, MemoryNand}};
        for (law, spare) in [
            (HostConfiguration::G1StrictRejectBeforeMutation, 0),
            (HostConfiguration::G1StrictRejectBeforeMutation, 1),
            (HostConfiguration::LegacyAndCommitFailedStatus, 0),
            (HostConfiguration::LegacyAndCommitFailedStatus, 1),
        ] {
            let mut data = std::vec![0x6b; 8 * BLOCK_BYTES as usize];
            data[..2048].fill(0x3c);
            data[2048..4096].fill(0x4d);
            let mut oob = std::vec![[0x12; 16]; 8 * PAGES_PER_BLOCK as usize];
            let mut failures = std::vec![BlockFailures::default(); 8];
            let mut nand = MemoryNand::adopt(PhysicalBlock::new(0).unwrap(), &mut data, &mut oob,
                &mut failures, ImageProvenance::Synthetic, law).unwrap();
            let mut output = std::vec![0xa5; 0x108000];
            let mut destination = super::NandEventDestination::adopt(super::DESTINATION_ADDRESS, &mut output).unwrap();
            let mut bytes = [spare, 0, 0xff];
            let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
            assert_eq!(super::execute_host(MiddleArguments::from_abi(&[0, 0, 1, 0, 2, 0]),
                &mut configuration, &mut nand, &mut destination).unwrap(), ActionCompletion::service_success());
            assert_eq!(configuration.byte(2).unwrap(), 0);
            drop(nand);
            let stride = if spare == 1 { 2112 } else { 2048 };
            for page in 0..8 * PAGES_PER_BLOCK as usize {
                let value = match page { 0 => 0x3c, 1 => 0x4d, _ => 0x6b };
                assert!(data[page * 2048..(page + 1) * 2048].iter().all(|byte| *byte == value));
                assert!(destination.bytes()[page * stride..page * stride + 2048].iter().all(|byte| *byte == value));
                if spare == 1 {
                    assert_eq!(&destination.bytes()[page * stride + 2048..page * stride + 2064], &[0x12; 16]);
                    assert_eq!(&destination.bytes()[page * stride + 2064..(page + 1) * stride], &[0xa5; 48]);
                }
            }
            assert!(oob.iter().all(|spare| *spare == [0x12; 16]));
            if spare == 0 { assert!(destination.bytes()[0x100000..].iter().all(|byte| *byte == 0xa5)); }
        }
    }
}
