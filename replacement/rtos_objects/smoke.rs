#[path = "src/lib.rs"]
mod objects;

use objects::layout::{GuestAddress, Message};
use objects::sim::Sim;
use objects::*;

fn dispatch_on(sim: &Sim, entry: Domain, target: Domain) {
    let context = entry.words().context(target);
    let scratch = GuestAddress(0x4000_9000);
    sim.set(context.dispatch_enabled(), scratch.0);
    sim.set(scratch, 1);
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
    dispatch_on(&sim, Domain::A, Domain::A);
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
    sim.set(task.priority(), 5);
    sim.set(task.state(), 0x32);
    sim.set(wait.pattern(), 0x10);
    sim.set(control.queue().next(), task.0 .0);
    sim.set(control.queue().prev(), task.0 .0);
    sim.set(task.next(), control.0 .0);
    sim.set(task.prev(), control.0 .0);
    set_flag(&sim, flag, Pattern(0x10));
    println!(
        "set: pattern {:#x} reported {:#x} ready {} running {}",
        sim.word(control.pattern()),
        sim.word(wait.result()),
        sim.ready(Domain::A, 5) == task.0 .0,
        sim.running(Domain::A) == task.0 .0
    );

    let remote = Sim::new(Domain::A);
    dispatch_on(&remote, Domain::A, Domain::B);
    create_flag(&remote, flag, FlagCreate { attributes: FlagAttributes(0), initial: Pattern(0x01) }).unwrap();
    let rtask = words.task(TaskSlot::new(8).unwrap());
    let rwait = words.flag_wait(TaskSlot::new(8).unwrap());
    remote.set(rtask.domain(), 2);
    remote.set(rtask.wait(), rwait.0 .0);
    remote.set(rtask.priority(), 5);
    remote.set(rtask.state(), 0x32);
    remote.set(rwait.pattern(), 0x10);
    remote.set(control.queue().next(), rtask.0 .0);
    remote.set(control.queue().prev(), rtask.0 .0);
    remote.set(rtask.next(), control.0 .0);
    remote.set(rtask.prev(), control.0 .0);
    set_flag(&remote, flag, Pattern(0x10));
    println!(
        "remote: ready {} doorbell {} running {}",
        remote.ready(Domain::B, 5) == rtask.0 .0,
        remote.doorbell(1),
        remote.running(Domain::B) == rtask.0 .0
    );

    let mailbox_control = words.mailbox_control(mailbox_slot);
    let receiver = words.task(TaskSlot::new(9).unwrap());
    let receive = words.mailbox_wait(TaskSlot::new(9).unwrap());
    sim.set(receiver.domain(), 1);
    sim.set(receiver.wait(), receive.0 .0);
    sim.set(receiver.priority(), 5);
    sim.set(receiver.state(), 0x32);
    sim.set(mailbox_control.queue().next(), receiver.0 .0);
    sim.set(mailbox_control.queue().prev(), receiver.0 .0);
    sim.set(receiver.next(), mailbox_control.0 .0);
    sim.set(receiver.prev(), mailbox_control.0 .0);
    let message = Message(GuestAddress(0x4000_0300));
    send(&sim, mailbox, message, None);
    println!(
        "send: message {:#x} ready {} running {}",
        sim.word(receive.message()),
        sim.ready(Domain::A, 5) == receiver.0 .0,
        sim.running(Domain::A) == receiver.0 .0
    );

    let first = Message(GuestAddress(0x4000_0400));
    let second = Message(GuestAddress(0x4000_0500));
    send(&sim, mailbox, first, None);
    send(&sim, mailbox, second, None);
    println!(
        "fifo: head {:#x} tail {:#x}",
        sim.word(mailbox_control.head()),
        sim.word(mailbox_control.tail())
    );
}
