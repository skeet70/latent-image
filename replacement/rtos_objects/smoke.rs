#[path = "src/lib.rs"]
mod objects;

use objects::*;

fn send(priority: Option<u32>, limit: u32) -> Result<Option<u32>, ObjectStatus> {
    let request = Request {
        service: Service::MailboxSend,
        entry: Entry {
            handler_context: 0,
            irq_masked: false,
            dispatch_enabled: 0,
        },
        entry_domain: EntryDomain::new(Domain::A),
        id: EncodedId::of(1, 1),
        records: Records {
            flags: 120,
            mailboxes: 1,
        },
        facts: Facts {
            exists: true,
            attributes: 2,
            priority_limit: limit,
        },
        params: Params {
            pattern: Pattern(0),
            mode: 0,
            timeout: None,
            message_priority: priority,
        },
    };
    admit(&request).map(|admitted| admitted.priority.map(MessagePriority::get))
}

fn main() {
    let a = EntryDomain::new(Domain::A);
    let b = EntryDomain::new(Domain::B);

    println!("class 2 from an A body: {:?}", resolve_flag(a, EncodedId::of(2, 3), 120).map(|id| (id.domain, id.slot.get())));
    println!("class 0 from a B body: {:?}", resolve_flag(b, EncodedId::of(0, 3), 120).map(|id| (id.domain, id.slot.get())));
    println!("class 1 from a B body: {:?}", resolve_mailbox(b, EncodedId::of(1, 1), 1).map(|id| (id.domain, id.slot.get())));

    println!("priority 7 under limit 32: {:?}", send(Some(7), 32));
    println!("priority 0xffffffff under limit 0xffffffff: {:?}", send(Some(0xffff_ffff), 0xffff_ffff));
    println!("priority 0x80000000 under limit 32: {:?}", send(Some(0x8000_0000), 32));
    println!("priority 33 under limit 32: {:?}", send(Some(33), 32));
}
