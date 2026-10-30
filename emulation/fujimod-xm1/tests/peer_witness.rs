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

#[test]
fn accepts_only_the_two_source_bound_control_policies() {
    for control in ["held-sentinel", "delayed-release"] {
        let contract = format!(r#"{{"schema":2,"control":"{control}","budget":10000000}}"#);
        assert!(PeerContract::parse(contract.as_bytes()).is_ok());
    }
}

#[test]
fn rejects_control_budget_schema_and_site_changes() {
    for bytes in [
        br#"{"schema":2,"control":"held-sentinel","budget":9999999}"#.as_slice(),
        br#"{"schema":2,"control":"delayed-release","budget":10000001}"#.as_slice(),
        br#"{"schema":2,"control":"natural","budget":10000000}"#.as_slice(),
        br#"{"schema":1,"control":"held-sentinel","budget":10000000}"#.as_slice(),
        br#"{"schema":3,"control":"delayed-release","budget":10000000}"#.as_slice(),
        br#"{"schema":2,"control":"held-sentinel","budget":10000000,"core":0}"#.as_slice(),
        br#"{"schema":2,"control":"held-sentinel","budget":10000000,"waits":4}"#.as_slice(),
        br#"{"schema":2,"control":"delayed-release","budget":10000000,"gate":8180}"#.as_slice(),
        br#"{"schema":2,"control":"delayed-release","budget":10000000,"budget":10000000}"#.as_slice(),
        br#"{"schema":2,"control":"held-sentinel","control":"delayed-release","budget":10000000}"#.as_slice(),
        br#"{"schema":2,"control":"held-sentinel","budget":10000000.0}"#.as_slice(),
    ] {
        assert!(PeerContract::parse(bytes).is_err());
    }
}
