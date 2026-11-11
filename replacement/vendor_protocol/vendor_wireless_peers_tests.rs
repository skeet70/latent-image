use super::*;

#[test]
fn interfaces_share_twenty_slots_and_reuse_the_lowest_released_index() {
    let mut pool = PeerPool::new();
    let mut leases = std::vec::Vec::new();
    for i in 0..20 {
        let net = NetIndex::new(i % 3).unwrap();
        let mac = MacAddress([i; 6]);
        let lease = pool.reserve(net, mac).unwrap();
        assert_eq!(lease.index().wire(), i);
        assert_eq!(pool.lookup(net, mac), Some(lease));
        leases.push(lease);
    }
    assert_eq!(pool.reserve(NetIndex::PRIMARY, MacAddress([99; 6])), Err(PoolError::Exhausted));
    let removed = leases[7];
    assert_eq!(pool.release(removed).unwrap().record.mac(), MacAddress([7; 6]));
    let replacement = pool.reserve(NetIndex::P2P, MacAddress([99; 6])).unwrap();
    assert_eq!(replacement.index(), removed.index());
    assert_ne!(replacement.generation(), removed.generation());
    assert_eq!(pool.get(removed), Err(PoolError::StaleLease));
    assert_eq!(pool.release(removed), Err(PoolError::StaleLease));
    assert_eq!(pool.lookup(NetIndex::P2P, MacAddress([99; 6])), Some(replacement));
}

#[test]
fn identical_addresses_are_scoped_to_network_and_duplicate_admission_is_explicit() {
    let mut pool = PeerPool::new();
    let mac = MacAddress([3; 6]);
    let primary = pool.reserve(NetIndex::PRIMARY, mac).unwrap();
    let secondary = pool.reserve(NetIndex::SECONDARY, mac).unwrap();
    assert_ne!(primary.index(), secondary.index());
    assert_eq!(pool.reserve(NetIndex::PRIMARY, mac), Err(PoolError::DuplicatePeer));
    assert_eq!(pool.lookup(NetIndex::P2P, mac), None);
    assert_eq!(PeerIndex::new(20), None);
}

#[test]
fn state_transitions_preserve_source_local_only_paths() {
    let mut pool = PeerPool::new();
    let lease = pool.reserve(NetIndex::PRIMARY, MacAddress([4; 6])).unwrap();
    let auth = pool.set_state(lease, PeerState::Authenticated).unwrap();
    assert!(!auth.publish_device_state);
    let disconnected = pool.set_state(lease, PeerState::Disconnected).unwrap();
    assert!(!disconnected.publish_device_state);
    let associated = pool.set_state(lease, PeerState::Associated).unwrap();
    assert!(associated.publish_device_state);
    assert!(associated.request_activation);
    assert!(!pool.get(lease).unwrap().hardware_active());
    assert_eq!(pool.complete_association(lease, MacAddress([5; 6])), Err(PoolError::CompletionMismatch));
    pool.complete_association(lease, MacAddress([4; 6])).unwrap();
    assert!(pool.get(lease).unwrap().hardware_active());
    let repeated = pool.set_state(lease, PeerState::Associated).unwrap();
    assert!(repeated.publish_device_state);
    assert!(!repeated.request_activation);
    let down = pool.set_state(lease, PeerState::Authenticated).unwrap();
    assert!(down.publish_device_state);
    assert!(down.clear_hardware_queues);
    assert!(!pool.get(lease).unwrap().hardware_active());
}

#[test]
fn retiring_peer_cannot_receive_completion_or_new_descriptor_bindings() {
    let mut pool = PeerPool::new();
    let mac = MacAddress([8; 6]);
    let old = pool.reserve(NetIndex::SECONDARY, mac).unwrap();
    pool.set_state(old, PeerState::Associated).unwrap();
    pool.begin_release(old).unwrap();
    assert_eq!(pool.lease_for_index(old.index()), None);
    assert_eq!(pool.complete_association(old, mac), Err(PoolError::Retiring));
    assert_eq!(pool.reserve(NetIndex::SECONDARY, mac), Err(PoolError::DuplicatePeer));
    pool.release(old).unwrap();
    let new = pool.reserve(NetIndex::SECONDARY, mac).unwrap();
    assert_eq!(pool.complete_association(old, mac), Err(PoolError::StaleLease));
    assert_eq!(pool.lease_for_index(new.index()), Some(new));
}

#[test]
fn secondary_abort_clears_session_without_releasing_or_clearing_capabilities() {
    let mut pool = PeerPool::new();
    let mac = MacAddress([9; 6]);
    let lease = pool.reserve(NetIndex::SECONDARY, mac).unwrap();
    let record = pool.get_mut(lease).unwrap();
    record.session = PeerSessionState {
        association_status: AssociationStatus(17),
        deauthentication_reason: DeauthenticationReason(4),
        control_74: PeerControlFlags(3),
        control_15d: PeerControlFlags(1),
        control_64: PeerControlFlags(2),
    };
    record.ht = HtCapabilities(0x102);
    pool.clear_secondary_session(lease).unwrap();
    let record = pool.get(lease).unwrap();
    assert_eq!(record.session.association_status, AssociationStatus(0));
    assert_eq!(record.session.deauthentication_reason, DeauthenticationReason(0));
    assert_eq!(record.session.control_74, PeerControlFlags(0));
    assert_eq!(record.session.control_15d, PeerControlFlags(0));
    assert_eq!(record.session.control_64, PeerControlFlags(0));
    assert_eq!(record.ht, HtCapabilities(0x102));
    assert_eq!(pool.lookup(NetIndex::SECONDARY, mac), Some(lease));
}
