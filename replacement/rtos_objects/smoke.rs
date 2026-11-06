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
    fn with_object_lock<R>(&self, _lock: LockRecord, body: impl FnOnce() -> R) -> R {
        assert!(!self.held.get(), "object lock acquired twice");
        self.held.set(true);
        let result = body();
        assert!(self.held.get(), "object lock released while not held");
        self.held.set(false);
        result
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
    let out = GuestAddress(0x4000_1000);

    create_flag(
        &memory,
        flag,
        FlagCreate {
            attributes: FlagAttributes(0),
            initial: Pattern(0x31),
        },
    )
    .unwrap();
    create_mailbox(
        &memory,
        mailbox,
        MailboxCreate {
            attributes: MailboxAttributes(2),
            priority_limit: PriorityLimit(32),
        },
    )
    .unwrap();

    memory.set(control.pattern(), 0x31);
    clear_flag(&memory, flag, KeepMask(0x11)).unwrap();
    println!("clear: pattern {:#x}", memory.word(control.pattern()));

    memory.set(control.pattern(), 0x30);
    memory.set(out, 0xdead);
    let matched = poll_flag(
        &memory,
        flag,
        WantedMask::new(Pattern(0x10)).unwrap(),
        MatchMode::All,
        OutWord(out),
    )
    .unwrap();
    println!(
        "poll matched: code {} out {:#x} pattern {:#x}",
        matched.code(),
        memory.word(out),
        memory.word(control.pattern())
    );

    memory.set(config.attributes(), FlagConfig::CLEAR_ON_MATCH);
    memory.set(control.pattern(), 0x31);
    memory.set(out, 0xdead);
    let cleared = poll_flag(
        &memory,
        flag,
        WantedMask::new(Pattern(0x01)).unwrap(),
        MatchMode::All,
        OutWord(out),
    )
    .unwrap();
    println!(
        "poll clear-on-match: code {} out {:#x} pattern {:#x}",
        cleared.code(),
        memory.word(out),
        memory.word(control.pattern())
    );

    memory.set(config.attributes(), 0);
    memory.set(control.pattern(), 0x30);
    memory.set(out, 0xdead);
    let missing = poll_flag(
        &memory,
        flag,
        WantedMask::new(Pattern(0x40)).unwrap(),
        MatchMode::All,
        OutWord(out),
    )
    .unwrap();
    println!(
        "poll no match: code {} out {:#x}",
        missing.code(),
        memory.word(out)
    );

    memory.set(control.queue().next(), 0x1234);
    memory.set(out, 0xdead);
    let busy = poll_flag(
        &memory,
        flag,
        WantedMask::new(Pattern(0x10)).unwrap(),
        MatchMode::All,
        OutWord(out),
    );
    println!("poll with waiter: {:?} out {:#x}", busy, memory.word(out));
    memory.set(control.queue().next(), control.0 .0);

    let first = Message(GuestAddress(0x4000_0100));
    let second = Message(GuestAddress(0x4000_0200));
    memory.set(mailbox_control.head(), first.0 .0);
    memory.set(first.next(), second.0 .0);
    memory.set(out, 0xdead);
    let received = poll_receive(&memory, mailbox, OutWord(out)).unwrap();
    println!(
        "receive: code {} out {:#x} head {:#x}",
        received.code(),
        memory.word(out),
        memory.word(mailbox_control.head())
    );

    memory.set(mailbox_control.head(), 0);
    memory.set(out, 0xdead);
    let empty = poll_receive(&memory, mailbox, OutWord(out)).unwrap();
    println!(
        "receive empty: code {} out {:#x}",
        empty.code(),
        memory.word(out)
    );
}
