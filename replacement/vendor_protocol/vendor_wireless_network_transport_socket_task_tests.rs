use super::{SocketTaskCompletion, SocketTaskCompletionKind, SocketTaskPhase, SocketTaskRuntime};
use crate::vendor_wireless_access_point::AccessPoint;
use crate::vendor_wireless_adapter::Adapter;
use crate::vendor_wireless_network::MacAddress;
use crate::vendor_wireless_network::dhcp::{LeaseSlot, Milliseconds};
use crate::vendor_wireless_network::lan::LanRuntime;
use crate::vendor_wireless_network::state::Ipv4Address;
use crate::wireless_association::station::StationRuntime;

#[test]
fn disabling_a_reconfigured_access_point_stops_without_a_restart() {
    let local = MacAddress([2, 0, 0, 0, 0, 1]);
    let mut slots = [LeaseSlot::new(Ipv4Address([192, 168, 0, 2]))];
    let mut lan = LanRuntime::new(local, &mut slots);
    let mut station = StationRuntime::new(local);
    let mut ap = AccessPoint::new(local);
    let mut adapter: Adapter<'_, 2, 2> = Adapter::new();
    let mut output = [0; 64];
    let mut task = SocketTaskRuntime::new();
    let mut service = |task: &mut SocketTaskRuntime| { task.service(&mut lan, &mut station, &mut ap, &mut adapter, Milliseconds(0), &mut output).unwrap(); };

    task.initialize().unwrap();
    service(&mut task);
    assert_eq!(task.completion().map(|completion| completion.kind), Some(SocketTaskCompletionKind::Initialize));

    let operation = task.disable_reconfigured_ap().unwrap();
    service(&mut task);
    service(&mut task);
    assert_eq!(task.completion(), Some(SocketTaskCompletion { operation, kind: SocketTaskCompletionKind::ReconfigureAp, status: Ok(()) }));
    assert_eq!(task.phase(), SocketTaskPhase::Ready);

    service(&mut task);
    assert_eq!(task.completion(), None);
}
