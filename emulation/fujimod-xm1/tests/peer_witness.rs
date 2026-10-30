use fujimod_xm1::peer_witness::PeerContract;

#[test]
fn accepts_bounded_pre_execution_observation() {
    assert!(PeerContract::parse(br#"{"schema":1,"budget":20000000}"#).is_ok());
    assert!(PeerContract::parse(br#"{"schema":1,"budget":1}"#).is_ok());
    assert!(PeerContract::parse(br#"{"schema":1,"budget":50000000}"#).is_ok());
}

#[test]
fn rejects_unbounded_or_retargeted_observation() {
    for bytes in [
        br#"{"schema":1,"budget":0}"#.as_slice(),
        br#"{"schema":1,"budget":50000001}"#.as_slice(),
        br#"{"schema":2,"budget":20000000}"#.as_slice(),
        br#"{"schema":1,"budget":20000000,"core":0}"#.as_slice(),
        br#"{"schema":1,"budget":20000000,"address":73728}"#.as_slice(),
    ] {
        assert!(PeerContract::parse(bytes).is_err());
    }
}
