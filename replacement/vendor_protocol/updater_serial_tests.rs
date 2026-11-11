use super::{Baud, Error, Handle, InterruptBinding, PhysicalAddress, UpdaterSerial};
use super::emulator::Emulator;
use core::task::Poll;

fn serial() -> UpdaterSerial<Emulator> {
    let binding = unsafe { InterruptBinding::new(PhysicalAddress(0x1000), PhysicalAddress(0x2000), PhysicalAddress(0x3000), PhysicalAddress(0xfffb_3000)) }.unwrap();
    UpdaterSerial::new(Emulator::new(), binding)
}

#[test]
fn transmitter_only_finishes_after_both_physical_completion_bits() {
    let mut serial = serial();
    let handle = serial.open().unwrap();
    let mut write = serial.write_begin(handle, &[0x80, 0x5a]).unwrap();
    assert_eq!(serial.write_poll(&mut write), Poll::Pending);
    assert_eq!(serial.write_poll(&mut write), Poll::Pending);
    assert_eq!(serial.io().take_transmitted(), None);
    serial.io().set_register(PhysicalAddress(0xfffa_a00c), 0x20);
    assert_eq!(serial.write_poll(&mut write), Poll::Pending);
    serial.io().set_register(PhysicalAddress(0xfffa_a00c), 0x40);
    assert_eq!(serial.write_poll(&mut write), Poll::Pending);
    assert_eq!(serial.io().complete_transmit(), Some(0x80));
    assert_eq!(serial.write_poll(&mut write), Poll::Pending);
    assert_eq!(write.bytes_completed(), 1);
    assert_eq!(serial.io().complete_transmit(), Some(0x5a));
    assert_eq!(serial.write_poll(&mut write), Poll::Ready(Ok(())));
    assert_eq!(serial.io().take_transmitted(), Some(0x80));
    assert_eq!(serial.io().take_transmitted(), Some(0x5a));
}

#[test]
fn receive_snapshot_and_drain_preserve_wire_bytes() {
    let mut serial = serial();
    let handle = serial.open().unwrap();
    let epoch = serial.start(handle).unwrap();
    serial.io().receive(&[0xaa, 3, 0, 0xfe]);
    serial.receive_interrupt(epoch).unwrap();
    assert_eq!(&serial.rx_buffer(handle).unwrap()[..4], &[0xaa, 3, 0, 0xfe]);
    let mut bytes = [0; 8];
    assert_eq!(serial.read_received(handle, &mut bytes), Ok(4));
    assert_eq!(&bytes[..4], &[0xaa, 3, 0, 0xfe]);
    assert_eq!(serial.read_received(handle, &mut bytes), Ok(0));
    serial.clear_rx(handle, 0xff).unwrap();
    assert_eq!(serial.rx_buffer(handle).unwrap(), &[0xff; 100]);
}

#[test]
fn receive_overwrites_oldest_and_wraps_at_one_hundred_bytes() {
    let mut serial = serial();
    let handle = serial.open().unwrap();
    let epoch = serial.start(handle).unwrap();
    let bytes = core::array::from_fn::<_, 105, _>(|index| index as u8);
    serial.io().receive(&bytes);
    serial.receive_interrupt(epoch).unwrap();
    let mut received = [0; 100];
    assert_eq!(serial.read_received(handle, &mut received), Ok(99));
    assert_eq!(&received[..99], &bytes[6..]);
    assert_eq!(&serial.rx_buffer(handle).unwrap()[..5], &bytes[100..]);
}

#[test]
fn stop_rejects_late_interrupt_and_restart_invalidates_old_epoch() {
    let mut serial = serial();
    let handle = serial.open().unwrap();
    let old = serial.start(handle).unwrap();
    serial.stop(handle).unwrap();
    serial.io().receive(&[0x7f]);
    assert_eq!(serial.receive_interrupt(old), Err(Error::Cancelled));
    assert_eq!(serial.rx_buffer(handle).unwrap(), &[0; 100]);
    let new = serial.start(handle).unwrap();
    assert_eq!(serial.receive_interrupt(old), Err(Error::Cancelled));
    serial.receive_interrupt(new).unwrap();
    assert_eq!(serial.rx_buffer(handle).unwrap()[0], 0x7f);
    serial.close(handle).unwrap();
    assert_eq!(serial.receive_interrupt(new), Err(Error::Cancelled));
    assert_eq!(serial.io().register(PhysicalAddress(0xfffa_a002)), 0);
    assert_eq!(serial.io().register(PhysicalAddress(0xfffb_35ac)), 0x3000);
}

#[test]
fn closing_invalidates_handles_and_pending_writes_without_faking_completion() {
    let mut serial = serial();
    let handle = serial.open().unwrap();
    assert_eq!(serial.open(), Err(Error::Busy));
    assert_eq!(serial.baud(Handle(0), Baud::new(115_200).unwrap()), Err(Error::InvalidHandle));
    let mut write = serial.write_begin(handle, &[7, 8]).unwrap();
    assert_eq!(serial.write_poll(&mut write), Poll::Pending);
    serial.close(handle).unwrap();
    assert_eq!(serial.write_poll(&mut write), Poll::Ready(Err(Error::Cancelled)));
    assert_eq!(serial.io().complete_transmit(), Some(7));
    let reopened = serial.open().unwrap();
    assert_ne!(reopened, handle);
    assert_eq!(serial.reset_rx(handle), Err(Error::InvalidHandle));
    assert_eq!(serial.write_poll(&mut write), Poll::Ready(Err(Error::Cancelled)));
}

#[test]
fn cancellation_keeps_in_flight_byte_observable() {
    let mut serial = serial();
    let handle = serial.open().unwrap();
    let mut write = serial.write_begin(handle, &[9, 10]).unwrap();
    assert_eq!(serial.write_poll(&mut write), Poll::Pending);
    serial.cancel_write(&write).unwrap();
    assert_eq!(serial.write_poll(&mut write), Poll::Ready(Err(Error::Cancelled)));
    assert_eq!(serial.io().complete_transmit(), Some(9));
    assert_eq!(serial.io().take_transmitted(), Some(9));
    assert_eq!(serial.io().take_transmitted(), None);
}

#[test]
fn line_errors_are_counted_without_inventing_received_bytes() {
    let mut serial = serial();
    let handle = serial.open().unwrap();
    let epoch = serial.start(handle).unwrap();
    serial.io().line_error(0x1e);
    serial.receive_interrupt(epoch).unwrap();
    let errors = serial.receive_errors();
    assert_eq!((errors.overrun, errors.parity, errors.framing, errors.break_signal), (1, 1, 1, 1));
    assert_eq!(serial.read_received(handle, &mut [0; 1]), Ok(0));
}

#[test]
fn baud_programming_preserves_line_format_and_uses_integer_divisor() {
    let mut serial = serial();
    let handle = serial.open().unwrap();
    serial.baud(handle, Baud::new(115_200).unwrap()).unwrap();
    assert_eq!(serial.io().register(PhysicalAddress(0xfffa_a012)), 13);
    assert_eq!(serial.io().register(PhysicalAddress(0xfffa_a014)), 0);
    assert_eq!(serial.io().register(PhysicalAddress(0xfffa_a008)), 3);
    assert_eq!(Baud::new(22), Err(Error::InvalidBaud));
    assert_eq!(Baud::new(1_500_001), Err(Error::InvalidBaud));
}

#[test]
fn next_write_waits_for_cancelled_byte_to_leave_the_transmitter() {
    let mut serial = serial();
    let handle = serial.open().unwrap();
    let mut first = serial.write_begin(handle, &[1]).unwrap();
    assert_eq!(serial.write_poll(&mut first), Poll::Pending);
    serial.cancel_write(&first).unwrap();
    let mut second = serial.write_begin(handle, &[2]).unwrap();
    assert_eq!(serial.write_poll(&mut second), Poll::Pending);
    assert_eq!(serial.io().complete_transmit(), Some(1));
    assert_eq!(serial.write_poll(&mut second), Poll::Pending);
    assert_eq!(serial.io().complete_transmit(), Some(2));
    assert_eq!(serial.write_poll(&mut second), Poll::Ready(Ok(())));
    assert_eq!(serial.io().take_transmitted(), Some(1));
    assert_eq!(serial.io().take_transmitted(), Some(2));
}

#[test]
fn read_position_tracks_drain_wrap_overwrite_and_reset() {
    let mut serial = serial();
    let handle = serial.open().unwrap();
    let epoch = serial.start(handle).unwrap();
    assert_eq!(serial.read_position(handle).unwrap().get(), 0);
    serial.io().receive(&[1, 2, 3]);
    serial.receive_interrupt(epoch).unwrap();
    assert_eq!(serial.read_received(handle, &mut [0; 2]), Ok(2));
    assert_eq!(serial.read_position(handle).unwrap().get(), 2);
    serial.io().receive(&[9; 100]);
    serial.receive_interrupt(epoch).unwrap();
    assert_eq!(serial.read_position(handle).unwrap().get(), 4);
    serial.reset_rx(handle).unwrap();
    assert_eq!(serial.read_position(handle).unwrap().get(), 0);
    serial.close(handle).unwrap();
    assert_eq!(serial.read_position(handle), Err(Error::InvalidHandle));
}
