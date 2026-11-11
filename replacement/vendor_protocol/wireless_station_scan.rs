use super::{Cipher, InformationElements, PersonalProtocol, ScanBatch, ScanCandidate, SelectedAccessPoint, StationError};
use super::super::super::vendor_wireless_network::{self as network, GroupCipher, KeyManagement, PairwiseCipher};
use super::super::super::vendor_wireless_station_driver::association::{Association, CandidateMode};

pub(super) fn completed_scan(association: &Association) -> Result<ScanBatch, StationError> {
    let mut results = ScanBatch::new();
    for candidate in association.candidates() {
        let parsed = network::parse_security_elements(candidate.elements()).map_err(StationError::Scan)?;
        let choices = [(PersonalProtocol::Rsn, parsed.rsn, parsed.rsn_group), (PersonalProtocol::Wpa, parsed.wpa, parsed.wpa_group)];
        let selected = choices.iter().find(|(_, security, group)| security.key_management == KeyManagement::Personal && security.cipher != PairwiseCipher::Unspecified && *group != GroupCipher::Unspecified)
            .or_else(|| choices.iter().find(|(_, security, _)| security.key_management != KeyManagement::Unspecified));
        let (protocol, key_management, pairwise, group) = match selected {
            Some((protocol, security, group)) => (*protocol, security.key_management, match security.cipher { PairwiseCipher::Unspecified => Cipher::None, PairwiseCipher::Tkip => Cipher::Tkip, PairwiseCipher::Ccmp => Cipher::Ccmp }, group_cipher(*group)),
            None => (PersonalProtocol::None, KeyManagement::Unspecified, Cipher::None, Cipher::None),
        };
        results.append(ScanCandidate {
            ssid: candidate.ssid,
            privacy: candidate.capabilities & 0x10 != 0,
            signal: candidate.signal,
            mode: match candidate.mode { CandidateMode::Infrastructure => network::NetworkMode::Infrastructure, CandidateMode::Independent => network::NetworkMode::Independent },
            key_management,
            access_point: SelectedAccessPoint { address: candidate.bssid, protocol, pairwise, group, advertised_elements: InformationElements::parse(candidate.elements()).map_err(StationError::Profile)? },
        })?;
    }
    Ok(results)
}
fn group_cipher(cipher: GroupCipher) -> Cipher {
    match cipher { GroupCipher::Unspecified => Cipher::None, GroupCipher::Wep40 => Cipher::Wep40, GroupCipher::Wep104 => Cipher::Wep104, GroupCipher::Tkip => Cipher::Tkip, GroupCipher::Ccmp => Cipher::Ccmp }
}
