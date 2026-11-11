use super::*;
use crate::configuration_storage::{BLOCK_BYTES, CONFIGURATION_BYTES, ConfigurationIdentity, FlashStatus, PhysicalBlock, WritePolicy};
use crate::lens_queue_runtime::physical_fixture;
use crate::updater_serial::{emulator::Emulator, InterruptBinding, PhysicalAddress, UpdaterSerial};
use xm1_lens_transport::{control_actor::ControlActor, control_runtime::{CompletionMailbox, ControlPublications, ControlSender}, control_types::*};
use std::sync::Arc;
use std::task::Wake;

struct Wakeup;
impl Wake for Wakeup { fn wake(self: Arc<Self>) {} }
fn context() -> Context<'static> {
    static WAKER: std::sync::LazyLock<Waker> = std::sync::LazyLock::new(|| Waker::from(Arc::new(Wakeup)));
    Context::from_waker(&WAKER)
}
fn uart(io: &Emulator) -> UpdaterSerial<&Emulator> {
    let binding = unsafe { InterruptBinding::new(PhysicalAddress(0x1000), PhysicalAddress(0x2000), PhysicalAddress(0x3000), PhysicalAddress(0xfffb3000)) }.unwrap();
    UpdaterSerial::new(io, binding)
}
struct RejectTask;
impl ControlSender for RejectTask { fn post(&self, _: ControlMessage) -> Result<(), ControlError> { Err(ControlError::Queue) } }

struct Flash { persisted: std::vec::Vec<u8> }
impl ConfigurationFlash for Flash {
    fn bad_blocks(&mut self, _: &mut [PhysicalBlock; 0x13b]) -> Result<usize, FlashStatus> { Ok(0) }
    fn read(&mut self, _: PhysicalBlock, output: &mut [u8; BLOCK_BYTES]) -> Result<(), FlashStatus> { output.fill(0xff); Ok(()) }
    fn erase(&mut self, _: PhysicalBlock, _: u16) -> Result<(), FlashStatus> { Ok(()) }
    fn program(&mut self, _: PhysicalBlock, input: &[u8], _: u16) -> Result<bool, FlashStatus> { self.persisted.extend_from_slice(input); Ok(false) }
    fn delay(&mut self, _: u32) {}
    fn priority(&mut self, _: u8) {}
}
fn storage() -> ConfigurationStorage<Flash> {
    ConfigurationStorage::mount(Flash { persisted: std::vec::Vec::new() }, ConfigurationIdentity::new([0; 20]), WritePolicy { erase_passes: 0, delay_seconds: 0, replacement_limit: 20 }).unwrap()
}
fn serial_image() -> std::vec::Vec<u8> {
    let mut image = std::vec![0; 0x2000];
    for (offset, word) in [(0x5c, 1u32), (0x60, 1), (0x14, 0x1800), (0x18, u32::MAX), (0x20, 1), (0x58, 1)] {
        image[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    image
}

#[test]
fn delay_waits_across_tick_wrap() {
    let clock = HostClock::new(u32::MAX - 4);
    let mut delay = core::pin::pin!(Delay::new(&clock, 10));
    let mut cx = context();
    assert!(delay.as_mut().poll(&mut cx).is_pending());
    clock.advance(9);
    assert!(delay.as_mut().poll(&mut cx).is_pending());
    clock.advance(1);
    assert!(delay.as_mut().poll(&mut cx).is_ready());
}

#[test]
fn aggregate_configuration_upload_persists_before_success_publication() {
    let (queue, _) = physical_fixture::setup();
    let queue = RefCell::new(queue);
    let readiness = SharedReadiness::new();
    let lens_publications = ControlPublications::new();
    lens_publications.publish(&ControlActor::new([0; 128], false, &readiness));
    let mailbox = CompletionMailbox::new();
    let lens = Task99Client::new(&RejectTask, &mailbox, &lens_publications, &readiness);
    let io = Emulator::new();
    let clock = HostClock::new(0);
    let publications = UploadPublications::new();
    let mut runtime = UploadRuntime::new(&queue, &lens, &publications, uart(&io), storage(), &clock);
    let mut image = std::vec![0xff; crate::startup::WORKSPACE_BYTES];
    image[..0x200].fill(0);
    image[..4].copy_from_slice(&3u32.to_le_bytes());
    image[0x14..0x18].copy_from_slice(&(CONFIGURATION_BYTES as u32).to_le_bytes());
    image[0x18..0x1c].copy_from_slice(&(!(CONFIGURATION_BYTES as u32).wrapping_mul(255)).to_le_bytes());
    let control = UploadControl::new();
    let signals = crate::updater_peripheral::io::HostSignals::new();
    let outcome = {
        let mut operation = core::pin::pin!(runtime.run(&mut image, (CONFIGURATION_BYTES + 0x200) as u32, signals.port().unwrap(), PassLimit::from_configuration_byte(1), ControlOwner(3), &control));
        operation.as_mut().poll(&mut context())
    };
    assert_eq!(outcome, Poll::Ready(Ok(UploadOutcome::Finished(UploadResult { status: UploadStatus::Complete }))));
    assert_eq!(runtime.storage.flash().persisted, std::vec![0xff; CONFIGURATION_BYTES]);
    assert!(matches!(control.publication(), UploadPublication::Finished { result: UploadResult { status: UploadStatus::Complete }, .. }));
    assert!(image.iter().all(|byte| *byte == 0xff));
}

#[test]
fn cancellation_restores_serial_power_and_rejects_previous_upload_tokens() {
    let (queue, physical) = physical_fixture::setup();
    let queue = RefCell::new(queue);
    let readiness = SharedReadiness::new();
    let lens_publications = ControlPublications::new();
    lens_publications.publish(&ControlActor::new([0; 128], false, &readiness));
    let mailbox = CompletionMailbox::new();
    let lens = Task99Client::new(&RejectTask, &mailbox, &lens_publications, &readiness);
    let io = Emulator::new();
    let clock = HostClock::new(0);
    let publications = UploadPublications::new();
    let mut runtime = UploadRuntime::new(&queue, &lens, &publications, uart(&io), storage(), &clock);
    let mut image = serial_image();
    let control = UploadControl::new();
    let signals = crate::updater_peripheral::io::HostSignals::new();
    let token;
    {
        let mut operation = core::pin::pin!(runtime.run(&mut image, 0x2000, signals.port().unwrap(), PassLimit::from_configuration_byte(1), ControlOwner(3), &control));
        assert!(operation.as_mut().poll(&mut context()).is_pending());
        token = match control.publication() { UploadPublication::Running(token) => token, other => panic!("unexpected publication {other:?}") };
        assert!(control.cancel(token));
        assert_eq!(operation.as_mut().poll(&mut context()), Poll::Ready(Ok(UploadOutcome::Cancelled)));
    }
    assert_eq!(control.publication(), UploadPublication::Cancelled(token));
    assert_eq!(io.register(PhysicalAddress(0xfff89084)), 0x10000000);
    assert!(!physical.acquired.get());
    assert!(!control.cancel(token));
    {
        let mut operation = core::pin::pin!(runtime.run(&mut image, 0x2000, signals.port().unwrap(), PassLimit::from_configuration_byte(1), ControlOwner(3), &control));
        assert!(operation.as_mut().poll(&mut context()).is_pending());
        assert!(!control.cancel(token));
    }
    assert!(matches!(control.publication(), UploadPublication::Cancelled(new) if new != token));
}

#[test]
fn retained_serial_engine_programs_both_sections_with_physical_uart_completions() {
    let io = Emulator::new();
    let mut port = uart(&io);
    let clock = HostClock::new(0);
    let mut state = SerialProgramState::new();
    let mut image = serial_image();
    let frames = [(30, 0), (1, 0xe6), (1, 0x30), (7, 6), (1, 0x31), (4, 6), (10, 6), (1, 6), (1, 0x26), (1, 6), (262, 6), (262, 6), (6, 6)];
    let mut frame = 0;
    let mut sent = 0;
    let mut observed = std::vec::Vec::new();
    {
        let mut operation = core::pin::pin!(serial_program::run(&mut port, &mut state, &mut image, &clock));
        let mut cx = context();
        for step in 0..30_000 {
            if let Poll::Ready(result) = operation.as_mut().poll(&mut cx) {
                assert!(result.is_ok());
                assert_eq!(frame, frames.len());
                break;
            }
            if let Some(byte) = io.complete_transmit() {
                observed.push(byte);
                assert_eq!(io.take_transmitted(), Some(byte));
                sent += 1;
                if sent == frames[frame].0 {
                    let response = frames[frame].1;
                    if response == 0x30 || response == 0x31 { io.receive(&[response, 0, 0u8.wrapping_sub(response)]); }
                    else { io.receive(&[response]); }
                    sent = 0;
                    frame += 1;
                }
            }
            clock.advance(1);
            assert_ne!(step, 29_999, "serial transaction did not finish");
        }
    }
    assert_eq!(&observed[..30], &[0; 30]);
    assert_eq!(&image[0x9a0..0x9b0], b"UxFRI JfIeFlMsLn");
    assert_eq!(io.register(PhysicalAddress(0xfff89344)), 0x200000);
}

#[test]
fn peripheral_gpio_tokens_do_not_survive_disarm_or_a_new_owner() {
    let signals = crate::updater_peripheral::io::HostSignals::new();
    let old;
    {
        let mut port = signals.port().unwrap();
        port.arm().unwrap();
        old = signals.armed().unwrap();
        let mut glitch = [true; 10];
        glitch[9] = false;
        assert!(signals.interrupt(old, glitch));
        assert!(!port.take_interrupt());
        assert!(signals.interrupt(old, [true; 10]));
        assert!(port.take_interrupt());
        port.disarm();
        assert!(!signals.interrupt(old, [true; 10]));
    }
    let mut port = signals.port().unwrap();
    port.arm().unwrap();
    assert_ne!(signals.armed(), Some(old));
    assert!(!signals.interrupt(old, [true; 10]));
    assert!(!port.take_interrupt());
}
