#[path = "src/lib.rs"]
mod objects;

use objects::layout::{GuestAddress, Message};
use objects::sim::Sim;
use objects::*;

fn dispatch_on(sim: &Sim, owner: Domain) {
    sim.install(owner, true);
}

fn main() {
    let words = Domain::A.words();
    let flag_slot = FlagSlot::new(3, 120).unwrap();
    let mailbox_slot = MailboxSlot::new(1, 1).unwrap();
    let flag = FlagId {
        domain: Domain::A,
        slot: flag_slot,
    };
    let mailbox = MailboxId {
        domain: Domain::A,
        slot: mailbox_slot,
    };
    let sim = Sim::new(Domain::A);
    dispatch_on(&sim, Domain::A);
    create_flag(&sim, flag, FlagCreate { attributes: FlagAttributes(0), initial: Pattern(0x01) }).unwrap();
    create_mailbox(
        &sim,
        mailbox,
        MailboxCreate {
            attributes: MailboxAttributes(2),
            priority_limit: PriorityLimit(32),
        },
    )
    .unwrap();

    let control = words.flag_control(flag_slot);
    let task = words.task(TaskSlot::new(7).unwrap());
    let wait = words.flag_wait(TaskSlot::new(7).unwrap());
    sim.set(task.domain(), 1);
    sim.set(task.wait(), wait.0 .0);
    sim.set_byte(task.priority(), 5);
    sim.set_byte(task.state(), 0x32);
    sim.set(wait.pattern(), 0x10);
    sim.set(control.queue().next(), task.0 .0);
    sim.set(control.queue().prev(), task.0 .0);
    sim.set(task.next(), control.0 .0);
    sim.set(task.prev(), control.0 .0);
    set_flag(&sim, flag, Pattern(0x10));
    assert_eq!(sim.ready(Domain::A, 5), task.0 .0);
    assert_eq!(sim.running(Domain::A), task.0 .0);
    println!(
        "set: pattern {:#x} reported {:#x} ready {} running {}",
        sim.word(control.pattern()),
        sim.word(wait.result()),
        sim.ready(Domain::A, 5) == task.0 .0,
        sim.running(Domain::A) == task.0 .0
    );

    let remote = Sim::new(Domain::A);
    dispatch_on(&remote, Domain::B);
    create_flag(&remote, flag, FlagCreate { attributes: FlagAttributes(0), initial: Pattern(0x01) }).unwrap();
    let rtask = words.task(TaskSlot::new(8).unwrap());
    let rwait = words.flag_wait(TaskSlot::new(8).unwrap());
    remote.set(rtask.domain(), 2);
    remote.set(rtask.wait(), rwait.0 .0);
    remote.set_byte(rtask.priority(), 5);
    remote.set_byte(rtask.state(), 0x32);
    remote.set(rwait.pattern(), 0x10);
    remote.set(control.queue().next(), rtask.0 .0);
    remote.set(control.queue().prev(), rtask.0 .0);
    remote.set(rtask.next(), control.0 .0);
    remote.set(rtask.prev(), control.0 .0);
    set_flag(&remote, flag, Pattern(0x10));
    assert_eq!(remote.ready(Domain::B, 5), rtask.0 .0);
    assert!(remote.doorbell(1));
    assert_ne!(remote.running(Domain::B), rtask.0 .0);
    println!(
        "remote: ready {} doorbell {} running {}",
        remote.ready(Domain::B, 5) == rtask.0 .0,
        remote.doorbell(1),
        remote.running(Domain::B) == rtask.0 .0
    );

    let send_sim = Sim::new(Domain::A);
    dispatch_on(&send_sim, Domain::A);
    create_mailbox(
        &send_sim,
        mailbox,
        MailboxCreate {
            attributes: MailboxAttributes(2),
            priority_limit: PriorityLimit(32),
        },
    )
    .unwrap();
    let mailbox_control = words.mailbox_control(mailbox_slot);
    let receiver = words.task(TaskSlot::new(9).unwrap());
    let receive = words.mailbox_wait(TaskSlot::new(9).unwrap());
    send_sim.set(receiver.domain(), 1);
    send_sim.set(receiver.wait(), receive.0 .0);
    send_sim.set_byte(receiver.priority(), 5);
    send_sim.set_byte(receiver.state(), 0x32);
    send_sim.set(mailbox_control.queue().next(), receiver.0 .0);
    send_sim.set(mailbox_control.queue().prev(), receiver.0 .0);
    send_sim.set(receiver.next(), mailbox_control.0 .0);
    send_sim.set(receiver.prev(), mailbox_control.0 .0);
    let message = Message(GuestAddress(0x4000_0300));
    send(&send_sim, mailbox, message, None);
    assert_eq!(send_sim.ready(Domain::A, 5), receiver.0 .0);
    assert_eq!(send_sim.running(Domain::A), receiver.0 .0);
    println!(
        "send: message {:#x} ready {} running {}",
        send_sim.word(receive.message()),
        send_sim.ready(Domain::A, 5) == receiver.0 .0,
        send_sim.running(Domain::A) == receiver.0 .0
    );

    let first = Message(GuestAddress(0x4000_0400));
    let second = Message(GuestAddress(0x4000_0500));
    send(&send_sim, mailbox, first, None);
    send(&send_sim, mailbox, second, None);
    assert_eq!(send_sim.word(mailbox_control.head()), first.0 .0);
    assert_eq!(send_sim.word(mailbox_control.tail()), second.0 .0);
    println!(
        "fifo: head {:#x} tail {:#x}",
        send_sim.word(mailbox_control.head()),
        send_sim.word(mailbox_control.tail())
    );
}
