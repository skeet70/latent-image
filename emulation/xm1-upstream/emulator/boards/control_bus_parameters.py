"""Hash-checked firmware views; no controller or reply behavior is installed."""
from dataclasses import dataclass
import hashlib
import json
from pathlib import Path
import struct

from tools import firmware_registry as registry
from .profile import ROOT, ProfileError, number, relative_path


@dataclass(frozen=True)
class FirmwareView:
    firmware_id: str
    source_sha256: str
    address: int
    data: bytes
    sha256: str
    sampling: str

    def sample(self, memory_bus):
        """Sample mutable guest RAM at the declared transaction boundary.

        The immutable DAT bytes verify the initializer only. Runtime polarity
        need not equal those bytes; it must come from the guest bus view.
        """
        if self.sampling != 'transaction':
            raise ProfileError('Immutable view cannot sample mutable RAM')
        raw = bytes(memory_bus.read(self.address, len(self.data)))
        if len(raw) != len(self.data):
            raise ProfileError('Short firmware view read')
        return struct.unpack('<2I', raw)


@dataclass(frozen=True)
class EndpointDescriptor:
    index: int
    address: int
    polarity_selector: int
    channel: int


@dataclass(frozen=True)
class ControlBusParameters:
    descriptors: FirmwareView
    polarity_words: FirmwareView
    records: tuple
    spec_sha256: str

    def records_for_channel(self, channel):
        if type(channel) is not int or channel not in (0, 1):
            raise ProfileError('Unknown control-bus channel')
        return tuple(row for row in self.records if row.channel == channel)


def _view(entry, spec, sampling):
    address, size = number(spec['address']), number(spec['size'], positive=True)
    if address + size > 2**32 or spec.get('sampling') != sampling:
        raise ProfileError('Invalid firmware view extent/sampling')
    spans = [(base, data) for base, data in registry.regions(entry)
             if base <= address and address + size <= base + len(data)]
    if len(spans) != 1:
        raise ProfileError('Firmware view must have one mapped owner')
    base, data = spans[0]
    raw = data[address-base:address-base+size]
    digest = hashlib.sha256(raw).hexdigest()
    if digest != spec['sha256']:
        raise ProfileError('Firmware view hash mismatch')
    return FirmwareView(entry['id'], entry['source']['sha256'], address, raw, digest, sampling)


def resolve_control_bus(firmware_id, *, spec_file=None):
    entry = registry.load(firmware_id)
    selected = spec_file or f'emulator/boards/firmware_views/control-bus-{firmware_id}.json'
    path = relative_path(ROOT, selected)
    if not path.is_file():
        raise ProfileError(f'No control-bus parameter data for {firmware_id}')
    raw = path.read_bytes()
    spec = json.loads(raw)
    if type(spec.get('schema')) is not int or spec['schema'] != 1 or spec.get('firmware_id') != firmware_id:
        raise ProfileError('Control-bus parameter identity/schema mismatch')
    if spec.get('source_sha256') != entry['source']['sha256']:
        raise ProfileError('Control-bus parameters differ from registry identity')
    source = registry.source_path(entry)
    if source.stat().st_size != entry['source']['size'] or registry.file_sha256(source) != spec['source_sha256']:
        raise ProfileError('Original DAT differs from registry identity')
    table = _view(entry, spec['descriptors'], 'immutable-source')
    count = number(spec['descriptors']['count'], positive=True)
    if len(table.data) != count * 12:
        raise ProfileError('Descriptor count/extent mismatch')
    polarity = _view(entry, spec['polarity_words'], 'transaction')
    if len(polarity.data) != 8 or polarity.address & 3:
        raise ProfileError('Polarity view must contain two aligned words')
    records = tuple(EndpointDescriptor(n, *row) for n, row in enumerate(struct.iter_unpack('<3I', table.data)))
    if any(row.address > 0x7f or row.polarity_selector > 1 or row.channel > 1 for row in records):
        raise ProfileError('Unsupported descriptor grammar')
    return ControlBusParameters(table, polarity, records, hashlib.sha256(raw).hexdigest())
