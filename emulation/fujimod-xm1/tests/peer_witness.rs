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

fn loader_program() -> String {
    format!(r#""program":{{"kind":"owned","record_sha256":"{digest}","loader_sha256":"{digest}",
        "handoff":66592,"entry":66604,"load":66700,"post_load":66704,"loaded_register":0,"poll_period":256,
        "image":[66560,68096],"refusal":null,
        "forbid_fetch":[[852000,852168],[854444,854480],[854596,854628]],"forbid_read":[[852168,852220]]}}"#,
        digest = "ab".repeat(32))
}

fn composed(scenario: &str) -> String {
    format!(r#"{{"schema":5,"budget":50000000,"scenario":{{"kind":"{scenario}" }},{program},
        "startup":{{"kind":"composed","record1_sha256":"{digest}","image_sha256":"{digest}",
        "symbols_sha256":"{digest}","source_sha256":"{digest}","image":[24576,25088],"entry":24576,"handoff":24608,
        "handoff_target":262176,"handoff_argument":6934008,"secondary_entry":66560,"secondary_image_sha256":"{digest}",
        "secondary_symbols_sha256":"{digest}","secondary_source_sha256":"{digest}"}}}}"#,
        program = loader_program(), digest = "ab".repeat(32))
}

#[cfg(not(feature = "scoped-watch"))]
#[test]
fn default_sdk_refuses_composed_startup() {
    for scenario in ["natural", "held", "delayed", "non-one"] {
        assert!(PeerContract::parse(composed(scenario).as_bytes()).is_err());
    }
}

#[cfg(feature = "scoped-watch")]
#[test]
fn scoped_sdk_admits_composed_controls_and_preserves_standalone_schema_four() {
    for scenario in ["natural", "held", "delayed", "non-one"] {
        assert!(PeerContract::parse(composed(scenario).as_bytes()).is_ok());
        let standalone = format!(r#"{{"schema":4,"budget":50000000,"scenario":{{"kind":"{scenario}"}},{}}}"#, loader_program());
        assert!(PeerContract::parse(standalone.as_bytes()).is_ok());
    }
}

#[cfg(feature = "scoped-watch")]
#[test]
fn composed_startup_rejects_role_identity_extent_and_guard_changes() {
    let valid = composed("natural");
    for invalid in [
        valid.replace("\"schema\":5", "\"schema\":4"),
        valid.replace("\"kind\":\"owned\"", "\"kind\":\"retained\""),
        valid.replace("\"kind\":\"natural\"", "\"kind\":\"forced\""),
        valid.replace("\"budget\":50000000", "\"budget\":49999999"),
        valid.replace("\"kind\":\"composed\"", "\"kind\":\"retained\""),
        valid.replace("\"entry\":24576", "\"entry\":6144"),
        valid.replace("\"entry\":24576", "\"entry\":24580"),
        valid.replace("\"handoff\":24608", "\"handoff\":25088"),
        valid.replace("\"handoff\":24608", "\"handoff\":24576"),
        valid.replace("\"handoff_target\":262176", "\"handoff_target\":262177"),
        valid.replace("\"handoff_argument\":6934008", "\"handoff_argument\":0"),
        valid.replace("\"secondary_entry\":66560", "\"secondary_entry\":71680"),
        valid.replace("\"image\":[24576,25088]", "\"image\":[24572,25088]"),
        valid.replace("\"image\":[24576,25088]", "\"image\":[24576,65540]"),
        valid.replace("\"image\":[24576,25088]", "\"image\":[24576,25087]"),
        valid.replace("\"image\":[66560,68096]", "\"image\":[66556,68096]"),
        valid.replace("\"image\":[66560,68096]", "\"image\":[66560,71684]"),
        valid.replace("\"secondary_source_sha256\":\"abab", "\"secondary_source_sha256\":\"cdcd"),
        valid.replace("\"symbols_sha256\":\"abab", "\"symbols_sha256\":\"ABAB"),
        valid.replace("[852000,852168],", ""),
        valid.replace("[852168,852220]", "[852172,852220]"),
        valid.replace("\"loaded_register\":0", "\"loaded_register\":3"),
        valid.replace("\"secondary_entry\":66560", "\"secondary_entry\":66560,\"core\":1"),
    ] {
        assert!(PeerContract::parse(invalid.as_bytes()).is_err(), "{invalid}");
    }
    let standalone = format!(r#"{{"schema":5,"budget":50000000,"scenario":{{"kind":"natural"}},{}}}"#, loader_program());
    assert!(PeerContract::parse(standalone.as_bytes()).is_err());
}
