#[path = "src/lib.rs"]
mod objects;

use objects::layout::{FlagConfig, GuestAddress, LockRecord, Message};
use objects::*;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

struct Memory {
    words: RefCell<BTreeMap<u32, u32>>,
    held: Cell<bool>,
}

impl Memory {
    fn new() -> Self {
        Self {
            words: RefCell::new(BTreeMap::new()),
            held: Cell::new(false),
        }
    }

    fn word(&self, at: GuestAddress) -> u32 {
        *self.words.borrow().get(&at.0).unwrap_or(&0)
    }

    fn set(&self, at: GuestAddress, value: u32) {
        self.words.borrow_mut().insert(at.0, value);
    }
}

impl Machine for Memory {
    fn load_word(&self, at: GuestAddress) -> u32 {
        self.word(at)
    }

    fn store_word(&self, at: GuestAddress, value: u32) {
        self.set(at, value);
    }

    fn mask_interrupts(&self) -> SavedInterrupts {
        SavedInterrupts(0)
    }

    fn restore_interrupts(&self, _saved: SavedInterrupts) {}

    fn dmb(&self) {}
}

impl ObjectLock for Memory {
    fn disable_irq(&self) {}

    fn enable_irq(&self) {}

    fn acquire(&self, _lock: LockRecord) -> Held {
        assert!(!self.held.get(), "object lock acquired twice");
        self.held.set(true);
        Held::held()
    }

    fn release(&self, _lock: LockRecord, _held: Held) {
        assert!(self.held.get(), "object lock released while not held");
        self.held.set(false);
    }
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
    let memory = Memory::new();
    let control = words.flag_control(flag_slot);
    let config = words.flag_config(flag_slot);
    let mailbox_control = words.mailbox_control(mailbox_slot);

    memory.set(control.pattern(), 0x31);
    clear_flag(&memory, flag, KeepMask(0x11)).unwrap();
    println!("clear: pattern {:#x}", memory.word(control.pattern()));

    memory.set(control.pattern(), 0x30);
    let mut out = Pattern(0xdead);
    let matched = poll_flag(&memory, flag, Pattern(0x10), MatchMode::All, &mut out).unwrap();
    println!(
        "poll matched: code {} out {:#x} pattern {:#x}",
        matched.code(),
        out.0,
        memory.word(control.pattern())
    );

    memory.set(config.attributes(), FlagConfig::CLEAR_ON_MATCH);
    memory.set(control.pattern(), 0x31);
    let mut out = Pattern(0xdead);
    let cleared = poll_flag(&memory, flag, Pattern(0x01), MatchMode::All, &mut out).unwrap();
    println!(
        "poll clear-on-match: code {} out {:#x} pattern {:#x}",
        cleared.code(),
        out.0,
        memory.word(control.pattern())
    );

    memory.set(config.attributes(), 0);
    memory.set(control.pattern(), 0x30);
    let mut out = Pattern(0xdead);
    let missing = poll_flag(&memory, flag, Pattern(0x40), MatchMode::All, &mut out).unwrap();
    println!("poll no match: code {} out {:#x}", missing.code(), out.0);

    memory.set(control.queue().next(), 0x1234);
    let mut out = Pattern(0xdead);
    let busy = poll_flag(&memory, flag, Pattern(0x10), MatchMode::All, &mut out);
    println!("poll with waiter: {:?} out {:#x}", busy, out.0);
    memory.set(control.queue().next(), control.0 .0);

    let first = Message(GuestAddress(0x4000_0100));
    let second = Message(GuestAddress(0x4000_0200));
    memory.set(mailbox_control.head(), first.0 .0);
    memory.set(first.next(), second.0 .0);
    let mut out = None;
    let received = poll_receive(&memory, mailbox, &mut out).unwrap();
    println!(
        "receive: code {} out {:#x} head {:#x}",
        received.code(),
        out.map(|message| message.0 .0).unwrap_or(0),
        memory.word(mailbox_control.head())
    );

    memory.set(mailbox_control.head(), 0);
    let mut out = None;
    let empty = poll_receive(&memory, mailbox, &mut out).unwrap();
    println!("receive empty: code {} out {}", empty.code(), out.is_some());
}
