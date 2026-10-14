"""Optional remembered MCU packet from a hash-pinned camera RAM capture.

This does not capture or replay a physical SPI bus. The stock boot path reads
the same sixteen-byte packet through endpoint9 registers 0x50..0x5f, so using
the remembered RAM copy as those replies is an explicit emulator inference.
"""
from pathlib import Path
import hashlib
import json

ROOT = Path(__file__).resolve().parents[1]
CAPTURE = ROOT / 'research/hardware/ff80-settings-restoration-20261003'
FIRMWARE_SHA256 = '7dd5b358a66f38d1fc7dad899a77d35fe4e6ed0edd0ed1d04fbca512d5dc8fd6'
RESTORE_SHA256 = '7f81e39f2d91cca61dcd26a34e6206808ea5947de9e186bbfd386f4afbeb5372'
PREFERENCES_SHA256 = 'b57e9375d1abd69dfb13dc56976aed5e079eb7a7e1723b93a8cb237e932ceedf'
PACKET_SHA256 = '4627a0772892ba76d729428d12f3e226a91a712e211a0a3b5cc299196bdeaa5d'
PACKET_HEX = '00000000d00739ff008000070000aac0'


def _checked(path, digest, size=None):
    data = path.read_bytes()
    if hashlib.sha256(data).hexdigest() != digest or (size is not None and len(data) != size):
        raise ValueError(f'Unexpected captured MCU input in {path.name}')
    return data


def captured_mcu_profile(directory=None):
    """Return packet/profile_name/provenance; no device or camera is changed.

    Pass packet and profile_name to PowerDevices explicitly. The provenance
    belongs in the enclosing boot report, not in the device constructor kwargs.
    This does not alter ADC, boot status, RTC registers, or other MCU replies.
    """
    directory = Path(directory) if directory is not None else CAPTURE
    log_path = directory / 'restore.json'
    log = json.loads(_checked(log_path, RESTORE_SHA256))
    if log['proposal']['firmware_sha256'] != FIRMWARE_SHA256:
        raise ValueError('Captured MCU input belongs to another firmware')
    source_path = directory / 'preferences-after.bin'
    source = _checked(source_path, PREFERENCES_SHA256, 0x4c74)
    packet = source[4:20]
    if (packet.hex() != PACKET_HEX or hashlib.sha256(packet).hexdigest() != PACKET_SHA256 or
            packet[14] != 0xaa or sum(packet) % 256):
        raise ValueError('Invalid remembered MCU packet framing')
    name = 'captured remembered MCU packet / inferred SPI input'
    provenance = {
        'name': name,
        'firmware_sha256': FIRMWARE_SHA256,
        'source_file': str(source_path), 'source_sha256': PREFERENCES_SHA256,
        'source_bytes': len(source), 'source_ram_address': '0x40d08354',
        'packet_file_offset': 4, 'packet_ram_address': '0x40d08358',
        'packet_bytes': len(packet), 'packet_hex': packet.hex(), 'packet_sha256': PACKET_SHA256,
        'capture_log': str(log_path), 'capture_log_sha256': RESTORE_SHA256,
        'capture_window_utc': {'started': log['started_utc'], 'finished': log['finished_utc']},
        'emulated_registers': 'endpoint9 0x50..0x5f',
        'packet_byte11': packet[11],
        'software_completion_bits': {'language': bool(packet[11] & 2),
                                     'datetime': bool(packet[11] & 1),
                                     'performance': bool(packet[11] & 4)},
        'limits': [
            'The source is remembered application RAM after settings restoration, not a physical SPI capture.',
            'Supplying these saved bytes as endpoint9 replies is an explicit input-profile inference.',
            'MCU firmware, electrical timing, reset behavior, and subsequent physical replies are unvalidated.',
            'Captured packet seconds are zero; no current clock, status, ADC, or other reply is fabricated.',
            'A remembered packet and valid journals do not establish successful normal boot or live view.',
        ],
    }
    return {'packet': packet, 'profile_name': name, 'provenance': provenance}
