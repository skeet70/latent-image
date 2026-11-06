#[path = "src/lib.rs"]
mod objects;

use objects::layout::GuestAddress;
use objects::*;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

struct Memory {
    words: RefCell<BTreeMap<u32, u32>>,
    markers: Vec<u32>,
    released: Cell<bool>,
    masks: Cell<u32>,
    restores: Cell<u32>,
}

impl Memory {
    fn new(markers: Vec<u32>) -> Self {
        Self {
            words: RefCell::new(BTreeMap::new()),
            markers,
            released: Cell::new(false),
            masks: Cell::new(0),
            restores: Cell::new(0),
        }
    }

    fn word(&self, at: GuestAddress) -> u32 {
        *self.words.borrow().get(&at.0).unwrap_or(&0)
    }

    fn snapshot(&self, ats: &[GuestAddress]) -> Vec<u32> {
        ats.iter().map(|at| self.word(*at)).collect()
    }
}

impl Machine for Memory {
    fn load_word(&self, at: GuestAddress) -> u32 {
        self.word(at)
    }

    fn store_word(&self, at: GuestAddress, value: u32) {
        if self.markers.contains(&at.0) && value != 0 {
            assert!(self.released.get(), "publication marker stored before the release");
        }
        self.released.set(false);
        self.words.borrow_mut().insert(at.0, value);
    }

    fn mask_interrupts(&self) -> SavedInterrupts {
        self.masks.set(self.masks.get() + 1);
        SavedInterrupts(0xc0)
    }

    fn restore_interrupts(&self, saved: SavedInterrupts) {
        assert_eq!(saved.0, 0xc0);
        self.restores.set(self.restores.get() + 1);
    }

    fn dmb(&self) {
        self.released.set(true);
    }
}

fn create_request(service: Service, id: EncodedId) -> Request {
    Request {
        service,
        entry: Entry {
            handler_context: 0,
            irq_masked: false,
            dispatch_enabled: 0,
        },
        entry_domain: EntryDomain::new(Domain::A),
        id,
        records: Records {
            flags: 120,
            mailboxes: 1,
        },
        facts: Facts {
            exists: false,
            attributes: 0,
            priority_limit: 0,
        },
        params: Params::NONE,
    }
}

fn main() {
    let words = Domain::A.words();
    let flag_slot = FlagSlot::new(3, 120).unwrap();
    let mailbox_slot = MailboxSlot::new(1, 1).unwrap();
    let flag_config = words.flag_config(flag_slot);
    let flag_control = words.flag_control(flag_slot);
    let mailbox_config = words.mailbox_config(mailbox_slot);
    let mailbox_control = words.mailbox_control(mailbox_slot);
    let memory = Memory::new(vec![flag_config.exists().0, mailbox_config.exists().0]);

    let admitted = admit(&create_request(Service::FlagCreate, EncodedId::of(1, 3))).unwrap();
    let flag = match admitted.object {
        Object::Flag(id) => id,
        Object::Mailbox(_) => panic!("flag create admitted a mailbox"),
    };
    assert_eq!(
        create_flag(
            &memory,
            flag,
            FlagCreate {
                attributes: FlagAttributes(4),
                initial: Pattern(0x21)
            }
        ),
        Ok(flag)
    );
    println!(
        "flag: attributes {} initial {:#x} exists {} pattern {:#x} self-linked {}",
        memory.word(flag_config.attributes()),
        memory.word(flag_config.initial()),
        memory.word(flag_config.exists()),
        memory.word(flag_control.pattern()),
        memory.word(flag_control.queue().next()) == flag_control.0 .0
            && memory.word(flag_control.queue().prev()) == flag_control.0 .0
    );

    let watched = [
        flag_config.attributes(),
        flag_config.initial(),
        flag_config.exists(),
        flag_control.pattern(),
        flag_control.queue().next(),
        flag_control.queue().prev(),
    ];
    let before = memory.snapshot(&watched);
    let again = create_flag(
        &memory,
        flag,
        FlagCreate {
            attributes: FlagAttributes(9),
            initial: Pattern(9),
        },
    );
    println!(
        "flag duplicate: {:?} unchanged {}",
        again,
        memory.snapshot(&watched) == before
    );

    let admitted = admit(&create_request(Service::MailboxCreate, EncodedId::of(1, 1))).unwrap();
    let mailbox = match admitted.object {
        Object::Mailbox(id) => id,
        Object::Flag(_) => panic!("mailbox create admitted a flag"),
    };
    assert_eq!(
        create_mailbox(
            &memory,
            mailbox,
            MailboxCreate {
                attributes: MailboxAttributes(2),
                priority_limit: PriorityLimit(32)
            }
        ),
        Ok(mailbox)
    );
    println!(
        "mailbox: attributes {} limit {} exists {} head {} tail {} self-linked {}",
        memory.word(mailbox_config.attributes()),
        memory.word(mailbox_config.priority_limit()),
        memory.word(mailbox_config.exists()),
        memory.word(mailbox_control.head()),
        memory.word(mailbox_control.tail()),
        memory.word(mailbox_control.queue().next()) == mailbox_control.0 .0
            && memory.word(mailbox_control.queue().prev()) == mailbox_control.0 .0
    );

    let watched = [
        mailbox_config.attributes(),
        mailbox_config.priority_limit(),
        mailbox_config.exists(),
        mailbox_control.head(),
        mailbox_control.tail(),
    ];
    let before = memory.snapshot(&watched);
    let again = create_mailbox(
        &memory,
        mailbox,
        MailboxCreate {
            attributes: MailboxAttributes(7),
            priority_limit: PriorityLimit(9),
        },
    );
    println!(
        "mailbox duplicate: {:?} unchanged {}",
        again,
        memory.snapshot(&watched) == before
    );
}
