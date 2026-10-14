"""Immutable recorded output for one explicitly foreign-mode experiment.

Loading this fixture performs file validation only. It does not write guest
memory, construct a peripheral, execute firmware, or signal an interrupt.
"""
from __future__ import annotations

from dataclasses import dataclass
import hashlib
import json
from pathlib import Path
from types import MappingProxyType
from typing import Mapping

ROOT = Path(__file__).resolve().parents[1]
PROFILE = 'captured-rec28-output-under-startup50-experiment'
PROFILES = (PROFILE,)
SOURCE_DRAM_SHA256 = '9034379b794614ac1d522b60bcd053104103190e80d76dd583c971fb19e75d99'
BANK_SHA256 = '892a121a90484b174c5cb364b6eebf39dff09c447be9ca974195ee87b3b18482'
EXPECTED_CONFIG_SHA256 = 'b59c4cc7c9ecc8d438733dcce900f755212c5712e9f87e51c158fa1698f9c864'
SOURCE_BANK_ADDRESS = 0x5FECAA60
BANK_SIZE = 0x29B0
MANIFEST_SHA256 = '1840c5c918b57b6df9b717c98dd92241c4b5afa49f6041ffa686b3a3362eb742'
DEFAULT_FIXTURE = ROOT / 'research/emulator-native/fixtures/recorded-statistics-rec28-20261004T071226Z-14f6509d'

REGISTER_OFFSETS = (
    (0xFFF18500, 0), (0xFFF18504, 0x800), (0xFFF18508, 0x850),
    (0xFFF1850C, 0x1850), (0xFFF18510, 0x1990),
    (0xFFF18514, 0x1D90), (0xFFF18518, 0x2190),
    (0xFFF1851C, 0x21B0), (0xFFF18520, 0x23B0),
)

# register, destination-relative offset, recorded-bank offset, byte length,
# scope. These are selected consumer windows, not hardware transfer lengths.
SPAN_LAYOUT = (
    (0xFFF18500, 0, 0, 4, 'current-1x1-grid-cell'),
    (0xFFF18500, 0x400, 0x400, 4, 'current-1x1-grid-cell'),
    (0xFFF18504, 0, 0x800, 0x50, 'maximum-ten-u64-consumer-window'),
    (0xFFF18508, 0, 0x850, 0x400, 'unique-256-u32-consumer-plane'),
    (0xFFF18508, 0x400, 0xC50, 0x400, 'unique-256-u32-consumer-plane'),
    (0xFFF18508, 0x800, 0x1050, 0x400, 'unique-256-u32-consumer-plane'),
    *((0xFFF1850C, i * 0x20, 0x1850 + i * 0x20, 0x18,
       'six-payload-words-per-32-byte-record-preserving-padding') for i in range(10)),
    (0xFFF18510, 0, 0x1990, 4, 'current-1x1-packed-grid-cell'),
    (0xFFF18514, 0, 0x1D90, 4, 'current-1x1-grid-cell'),
    (0xFFF18518, 0, 0x2190, 0x10, 'four-scalar-totals'),
    (0xFFF1851C, 0, 0x21B0, 0x200,
     'opaque-experiment-window-not-a-proven-consumer-or-hardware-length'),
    (0xFFF18520, 0, 0x23B0, 0x100, 'four-16-word-consumer-arrays'),
    (0xFFF18520, 0x200, 0x25B0, 0x400, 'separate-histogram-consumer-window'),
)
assert len(SPAN_LAYOUT) == 22 and sum(row[3] for row in SPAN_LAYOUT) == 0x1460


def _sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _freeze(value):
    if isinstance(value, dict):
        return MappingProxyType({key: _freeze(item) for key, item in value.items()})
    if isinstance(value, list):
        return tuple(_freeze(item) for item in value)
    return value


def _thaw(value):
    if isinstance(value, Mapping):
        return {key: _thaw(item) for key, item in value.items()}
    if isinstance(value, tuple):
        return [_thaw(item) for item in value]
    return value


def manifest_spans(bank: bytes) -> list[dict]:
    """Canonical manifest records; used by the read-only extractor as well."""
    if len(bank) != BANK_SIZE:
        raise ValueError('Recorded bank must contain exactly 0x29B0 bytes')
    return [dict(register=hex(register), destination_offset=hex(destination),
                 source_offset=hex(source), bytes=length, scope=scope,
                 sha256=_sha256(bank[source:source + length]))
            for register, destination, source, length, scope in SPAN_LAYOUT]


@dataclass(frozen=True, slots=True, init=False)
class RecordedStatisticsFixture:
    """Validated, immutable bytes and offsets; no guest-side effects."""

    profile: str
    manifest: Mapping
    spans: tuple[tuple[int, int, bytes], ...]
    register_offsets: Mapping[int, int]
    expected_config_sha256: str
    sha256: str
    manifest_sha256: str
    source_bank_address: int
    bank_size: int
    source_mode: int
    requested_mode: int

    def __init__(self):
        raise TypeError('Use RecordedStatisticsFixture.load(directory)')

    @classmethod
    def load(cls, directory: Path | str) -> RecordedStatisticsFixture:
        directory = Path(directory)
        manifest_path = directory / 'manifest.json'
        if manifest_path.stat().st_size > 128 * 1024:
            raise ValueError('Recorded statistics manifest exceeds its bound')
        raw_manifest = manifest_path.read_bytes()
        if _sha256(raw_manifest) != MANIFEST_SHA256:
            raise ValueError('Recorded statistics manifest SHA-256 differs')
        manifest = json.loads(raw_manifest)
        if (manifest['schema'] != 1 or manifest['profile'] != PROFILE
                or manifest['source']['dram_sha256'] != SOURCE_DRAM_SHA256
                or manifest['source']['bank_address'] != hex(SOURCE_BANK_ADDRESS)
                or manifest['source']['bank_bytes'] != BANK_SIZE
                or manifest['source']['mode'] != 0x28
                or manifest['source']['mode_class'] != 2
                or manifest['experiment']['requested_mode'] != 0x50
                or manifest['experiment']['requested_mode_class'] != 4
                or manifest['experiment']['matching_acquisition'] is not False
                or manifest['experiment']['configuration_sha256'] != EXPECTED_CONFIG_SHA256
                or manifest['experiment']['metadata_policy'] != 'guest-built-not-copied'
                or manifest['payload'] != dict(file='bank0.bin', bytes=BANK_SIZE,
                                               sha256=BANK_SHA256)
                or manifest['register_offsets'] != {hex(r): hex(o) for r, o in REGISTER_OFFSETS}):
            raise ValueError('Recorded statistics fixture contract differs')
        bank_path = directory / 'bank0.bin'
        if bank_path.stat().st_size != BANK_SIZE:
            raise ValueError('Recorded statistics bank size differs')
        bank = bank_path.read_bytes()
        if len(bank) != BANK_SIZE or _sha256(bank) != BANK_SHA256:
            raise ValueError('Recorded statistics bank size or SHA-256 differs')
        if (manifest['spans'] != manifest_spans(bank)
                or manifest['span_count'] != 22 or manifest['transfer_bytes'] != 0x1460):
            raise ValueError('Recorded statistics declared spans differ')
        # Independent bank-relative bounds/nonoverlap checks protect the public
        # span plan even if a future edit changes its canonical layout.
        ranges = []
        offsets = dict(REGISTER_OFFSETS)
        for register, destination, source, length, _scope in SPAN_LAYOUT:
            begin = offsets[register] + destination
            if (begin != source or length < 1 or begin < 0
                    or begin + length > BANK_SIZE):
                raise ValueError('Recorded statistics span escapes its bank')
            if any(begin < end and previous < begin + length for previous, end in ranges):
                raise ValueError('Recorded statistics spans overlap')
            ranges.append((begin, begin + length))
        instance = object.__new__(cls)
        values = dict(
            profile=PROFILE, manifest=_freeze(manifest),
            spans=tuple((r, d, bank[s:s + length]) for r, d, s, length, _ in SPAN_LAYOUT),
            register_offsets=MappingProxyType(offsets),
            expected_config_sha256=EXPECTED_CONFIG_SHA256,
            sha256=BANK_SHA256, manifest_sha256=MANIFEST_SHA256,
            source_bank_address=SOURCE_BANK_ADDRESS, bank_size=BANK_SIZE,
            source_mode=0x28, requested_mode=0x50,
        )
        for name, value in values.items():
            object.__setattr__(instance, name, value)
        return instance

    def snapshot(self) -> dict:
        """Detached JSON-compatible provenance for device/run evidence."""
        return dict(profile=self.profile, bank_sha256=self.sha256,
                    manifest_sha256=self.manifest_sha256,
                    expected_config_sha256=self.expected_config_sha256,
                    source_mode=hex(self.source_mode), requested_mode=hex(self.requested_mode),
                    span_count=len(self.spans), transfer_bytes=sum(len(s[2]) for s in self.spans),
                    manifest=_thaw(self.manifest))

    def as_report(self) -> dict:
        return self.snapshot()

    def __copy__(self):
        return self

    def __deepcopy__(self, memo):
        # Every reachable field is immutable; checkpoint state may retain this
        # shared immutable value without copying MappingProxyType objects.
        memo[id(self)] = self
        return self


def load_fixture(profile: str, directory: Path | str | None = None) -> RecordedStatisticsFixture:
    if profile != PROFILE:
        raise ValueError('Unknown recorded statistics profile')
    return RecordedStatisticsFixture.load(DEFAULT_FIXTURE if directory is None else directory)
