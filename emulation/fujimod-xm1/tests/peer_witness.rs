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

fn owned(forbid_fetch: &str) -> String {
    format!(
        r#"{{"schema":3,"budget":20000000,"scenario":{{"kind":"natural","endpoint":"handoff"}},"program":{{"kind":"owned",
        "record_sha256":"{}","image":[66560,67936],
        "sites":{{"entry":66560,"main":66784,"peer_load":67704,"peer_loaded":67816,"clear_store":67804,"target_store":67748,
        "argument_store":67764,"target_load":66580,"argument_load":66588,"handoff":66592}},
        "loaded_register":0,"poll_period":512,"forbid_fetch":{forbid_fetch},
        "forbid_read":[[72388,72396],[72404,72424],[73700,73708]]}}}}"#,
        "ab".repeat(32)
    )
}

const FORBID_FETCH: &str = "[[71680,71764],[72164,72380],[72896,73700],[76128,76148],[76476,76496],[78992,79220],[79980,80044]]";

#[test]
fn accepts_retained_and_owned_startup_scenarios() {
    for scenario in [r#"{"kind":"natural","endpoint":"loader-entry"}"#, r#"{"kind":"natural","endpoint":"handoff"}"#] {
        let contract = format!(r#"{{"schema":3,"budget":20000000,"scenario":{scenario},"program":{{"kind":"retained"}}}}"#);
        assert!(PeerContract::parse(contract.as_bytes()).is_ok());
    }
    for scenario in [r#"{"kind":"held-sentinel"}"#, r#"{"kind":"delayed-release","endpoint":"handoff"}"#] {
        let contract = format!(r#"{{"schema":3,"budget":10000000,"scenario":{scenario},"program":{{"kind":"retained"}}}}"#);
        assert!(PeerContract::parse(contract.as_bytes()).is_ok());
    }
    assert!(PeerContract::parse(owned(FORBID_FETCH).as_bytes()).is_ok());
}

#[test]
fn rejects_retargeted_or_reduced_startup_guards() {
    let shifted = FORBID_FETCH.replace("[71680,71764]", "[71684,71764]");
    let omitted = FORBID_FETCH.replace("[71680,71764],", "");
    for contract in [
        owned(&shifted),
        owned(&omitted),
        owned(FORBID_FETCH).replace("\"budget\":20000000", "\"budget\":10000000"),
        owned(FORBID_FETCH).replace("\"handoff\":66592", "\"handoff\":66560"),
        owned(FORBID_FETCH).replace("\"image\":[66560,67936]", "\"image\":[65536,67936]"),
        owned(FORBID_FETCH).replace("\"loaded_register\":0", "\"loaded_register\":13"),
        r#"{"schema":3,"budget":10000000,"scenario":{"kind":"held-sentinel","endpoint":"handoff"},"program":{"kind":"retained"}}"#.into(),
        r#"{"schema":3,"budget":20000000,"scenario":{"kind":"natural"},"program":{"kind":"retained"}}"#.into(),
        r#"{"schema":3,"budget":20000000,"scenario":{"kind":"natural","endpoint":"handoff"},"program":{"kind":"retained","image":[0,4]}}"#.into(),
    ] {
        assert!(PeerContract::parse(contract.as_bytes()).is_err(), "{contract}");
    }
}
