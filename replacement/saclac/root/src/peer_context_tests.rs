use xm1_kernel::{fake::{Call, Fake}, Form, TaskId};
use xm1_saclac_interface::stock::StockCallbackWord;
use xm1_saclac_peers::{EmulatorOnly, PeerPorts};
use crate::peer_context::Poster;

#[test]
fn interrupt_sender_does_not_inherit_the_interrupted_task() {
    static SAFETY: u32 = 1;
    for (backend, form, sender) in [
        (Fake::with_task(TaskId::new(43).unwrap()), Form::Task, 43),
        (Fake::with_task(TaskId::new(43).unwrap()).in_handler(), Form::Handler, 0xff),
    ] {
        let poster = Poster::new(backend);
        let mut peers = PeerPorts::new(&poster, EmulatorOnly::new(&SAFETY).unwrap());
        xm1_saclac_config::face::submit_triplet(&mut peers, [4, 5, 6], StockCallbackWord(0)).unwrap();
        let posts: Vec<_> = poster.kernel().backend().calls().into_iter().filter_map(|call| {
            if let Call::Post { form, header, .. } = call { Some((form, header.sender)) } else { None }
        }).collect();
        assert_eq!(posts, [(form, sender)]);
    }
}
