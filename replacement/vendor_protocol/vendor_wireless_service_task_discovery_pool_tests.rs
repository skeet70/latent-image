use super::pool::*;

#[test]
fn retained_packet_survives_other_packet_release_and_reuse() {
    let mut first = [0;32];
    let mut second = [0;32];
    let mut storage = [PacketStorage::new(&mut first),PacketStorage::new(&mut second)];
    let mut pool = NetworkPacketPool::new(&mut storage);
    let retained = pool.insert(&[1,2,3]).unwrap();
    let transient = pool.insert(&[4,5]).unwrap();
    assert!(matches!(pool.insert(&[6]),Err(PacketPoolError::Full)));
    pool.release(transient).unwrap();
    let replacement = pool.insert(&[7,8]).unwrap();
    assert_eq!(pool.bytes(&retained).unwrap(), &[1,2,3]);
    assert_eq!(pool.bytes(&replacement).unwrap(), &[7,8]);
    pool.write(&retained,1,&[9]).unwrap();
    assert_eq!(pool.bytes(&retained).unwrap(), &[1,9,3]);
}

#[test]
fn too_large_and_out_of_bounds_writes_leave_packets_intact() {
    let mut bytes = [0;4];
    let mut storage = [PacketStorage::new(&mut bytes)];
    let mut pool = NetworkPacketPool::new(&mut storage);
    assert!(matches!(pool.insert(&[0;5]),Err(PacketPoolError::TooLarge)));
    let lease = pool.insert(&[1,2]).unwrap();
    assert_eq!(pool.write(&lease,1,&[3,4]),Err(PacketPoolError::OutOfBounds));
    assert_eq!(pool.bytes(&lease).unwrap(), &[1,2]);
    pool.release(lease).unwrap();
    let reused = pool.insert(&[8,7,6,5]).unwrap();
    assert_eq!(pool.bytes(&reused).unwrap(), &[8,7,6,5]);
}
