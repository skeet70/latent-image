"""Hash-checked data profiles; resolving never constructs a CPU or a device.

Schema 1 selects SoC, body and exact-firmware parts. The legacy registry
object is adapted using its registry-directory profile; no sibling fallback.
"""
from dataclasses import dataclass
import hashlib
import json
from pathlib import Path
import re
from types import MappingProxyType

from tools import firmware_registry as registry

ROOT = Path(__file__).resolve().parents[2]
KINDS = frozenset('BootControls KernelInterrupts NandController FujiDecompressor LoaderMemoryDMA '
    'PowerDevices USBControl AudioSerialControl PulseOutputs LensHomingFixture LensEndpoint SPI2Serial '
    'ControlBus SecondaryControlBus FE80Control InputProcessingControl InputAuxiliaryControl '
    'ProducerControl ImageParameterControl ImageWorkControl FaceAnalysisControl AFExtractionControl '
    'AfsFrameSource HDMIStartup OutputControl GraphicsAccelerator DisplayScanout DisplayTiming '
    'DisplayFormatter StillProcessingControl JPEGEncodeControl StillResizeControl SDCardController '
    'WiFiSDIOController'.split())


class ProfileError(ValueError):
    pass


def number(value, *, positive=False):
    if type(value) is int:
        result = value
    elif isinstance(value, str) and re.fullmatch(r'0x[0-9a-fA-F]+|[0-9]+', value):
        result = int(value, 16 if value.startswith('0x') else 10)
    else:
        raise ProfileError(f'Invalid profile integer: {value!r}')
    if not (int(positive) <= result < 2**32):
        raise ProfileError(f'Profile integer out of range: {value!r}')
    return result


def freeze(value):
    if isinstance(value, dict):
        return MappingProxyType({key: freeze(item) for key, item in value.items()})
    if isinstance(value, list):
        return tuple(freeze(item) for item in value)
    return value


def thaw(value):
    if isinstance(value, (dict, MappingProxyType)):
        return {key: thaw(item) for key, item in value.items()}
    if isinstance(value, tuple):
        return [thaw(item) for item in value]
    return value


def relative_path(root, value):
    if not isinstance(value, str) or not value or Path(value).is_absolute() or '..' in Path(value).parts:
        raise ProfileError(f'Expected repository-relative profile path: {value!r}')
    result = (root / value).resolve()
    if not result.is_relative_to(root.resolve()):
        raise ProfileError(f'Profile path escapes repository: {value!r}')
    return result


def checked_bytes(root, ref):
    if not isinstance(ref, dict) or not re.fullmatch('[0-9a-f]{64}', str(ref.get('sha256', ''))):
        raise ProfileError('File reference requires a SHA-256')
    path = relative_path(root, ref.get('path'))
    try:
        data = path.read_bytes()
    except OSError as exc:
        raise ProfileError(f'Cannot read profile input {path}: {exc}') from exc
    if hashlib.sha256(data).hexdigest() != ref['sha256']:
        raise ProfileError(f'Profile input hash mismatch: {ref["path"]}')
    if 'size' in ref and len(data) != number(ref['size'], positive=True):
        raise ProfileError(f'Profile input size mismatch: {ref["path"]}')
    return data


def document(data, role):
    try:
        value = json.loads(data)
    except (ValueError, UnicodeError) as exc:
        raise ProfileError(f'Invalid {role} JSON') from exc
    if not isinstance(value, dict) or type(value.get('schema')) is not int or value['schema'] != 1:
        raise ProfileError(f'Unsupported {role} schema')
    return value


def validate_graph(board):
    nodes = board.get('devices')
    if not isinstance(nodes, list) or not nodes:
        raise ProfileError('Board requires a device graph')
    names = [n.get('name') for n in nodes if isinstance(n, dict)]
    if len(names) != len(nodes) or any(not isinstance(n, str) or not re.fullmatch('[a-z][a-z0-9_]*', n) for n in names) or len(names) != len(set(names)):
        raise ProfileError('Device names must be unique safe identifiers')
    for node in nodes:
        if node.get('kind') not in KINDS:
            raise ProfileError(f'Unsupported model kind: {node.get("kind")}')
        if not isinstance(node.get('refs', {}), dict) or any(ref not in names for ref in node.get('refs', {}).values()):
            raise ProfileError(f'Unknown device reference: {node["name"]}')
    for key in ('construction_order', 'attachment_order'):
        order = board.get(key)
        if not isinstance(order, list) or len(order) != len(names) or set(order) != set(names):
            raise ProfileError(f'{key} must name each device exactly once')
    created = set()
    by_name = {n['name']: n for n in nodes}
    for name in board['construction_order']:
        if any(ref not in created for ref in by_name[name].get('refs', {}).values()):
            raise ProfileError(f'Device constructed before dependency: {name}')
        created.add(name)


@dataclass(frozen=True)
class ResolvedProfile:
    firmware_id: str
    profile_file: Path
    profile_sha256: str
    part_sha256: object
    soc: object
    board: object
    firmware: object
    _manifest: object
    flash: bytes
    root: Path

    def __deepcopy__(self, memo):
        # Resolution recursively freezes every mapping/list; paths, scalars
        # and firmware bytes are immutable too. Rollback retains this exact
        # verified configuration while copying mutable board/device state.
        memo[id(self)] = self
        return self

    @property
    def ram_size(self):
        return number(self.board['dram']['size'], positive=True)

    @property
    def dram_basis(self):
        return self.board['dram']['basis']

    @property
    def rom_file(self):
        return relative_path(self.root, self.firmware['rom']['path'])

    @property
    def rom_hash(self):
        return self.firmware['rom']['sha256']

    @property
    def entry_labels(self):
        return {number(a): label for a, label in self.firmware['entry_labels'].items()}

    @property
    def extra_mmio_ranges(self):
        return tuple((number(row['address']), number(row['size'], positive=True)) for row in self.soc['private_mmio'])

    @property
    def manifest(self):
        # Loader bookkeeping must not mutate shared resolved configuration.
        return thaw(self._manifest)

    @property
    def loader_validation(self):
        return thaw(self.firmware['loader_validation'])

    @property
    def reset_release(self):
        return thaw(self.firmware['reset_release'])


def _resolve(firmware_id=None, *, entry=None, profile_file=None, root=ROOT):
    root = Path(root)
    entry = registry.load(firmware_id) if entry is None else entry
    if firmware_id is not None and entry['id'] != firmware_id:
        raise ProfileError('Requested firmware differs from registry entry')
    firmware_id = entry['id']
    selected = entry.get('emulator')
    legacy = selected if isinstance(selected, dict) else None
    if profile_file is None:
        if selected is None:
            raise ProfileError(f'No emulator profile registered for {firmware_id}')
        if legacy is not None:
            # Transition convention follows the registry directory, not a body-id switch.
            profile_file = str((entry['_dir'] / 'emulator.json').relative_to(root))
        elif isinstance(selected, str):
            profile_file = selected
        else:
            raise ProfileError('Registry emulator must be a profile path, legacy object or null')
    path = relative_path(root, profile_file)
    top_bytes = path.read_bytes()
    top = document(top_bytes, 'selected profile')
    if top.get('firmware_id') != firmware_id:
        raise ProfileError('Profile firmware identity mismatch')
    parts = {role: document(checked_bytes(root, top.get(role)), role) for role in ('soc', 'board', 'firmware')}
    soc, board, firmware = (parts[k] for k in ('soc', 'board', 'firmware'))
    if firmware.get('firmware_id') != firmware_id or firmware.get('source_sha256') != entry['source']['sha256']:
        raise ProfileError('Firmware part differs from registry identity')
    if board.get('soc_id') != soc.get('id') or firmware.get('soc_id') != soc.get('id') or firmware.get('board_id') != board.get('id'):
        raise ProfileError('Incompatible SoC/board/firmware parts')
    dat_ref = dict(path=entry['source']['path'], sha256=entry['source']['sha256'], size=entry['source']['size'])
    dat = checked_bytes(root, dat_ref)
    if legacy is not None:
        # The old registry object remains authoritative for its existing three fields.
        board['dram']['size'] = legacy['ram_size']
        firmware['rom']['path'] = legacy['boot_rom']
        firmware['rom']['sha256'] = legacy['boot_rom_sha256']
    size = number(board['dram']['size'], positive=True)
    if size & 0xfff or size > 0x20000000 or not isinstance(board['dram'].get('basis'), str):
        raise ProfileError('DRAM size/basis unsupported by current EXR II runtime')
    if soc.get('architecture') != 'arm32' or soc.get('dram_alias') != '0x40000000':
        raise ProfileError('Unsupported SoC execution/memory-map contract')
    fixed = {'rom_address': 0x80000000, 'rom_size': 0x4000,
             'sram_address': 0xfff00000, 'sram_size': 0x1000}
    if (soc.get('runtime_abi') != 'exr2-r5f-fixed-map-v1'
            or any(number(soc[k]) != v for k, v in fixed.items())
            or [number(a) for a in soc['vector_aliases']] != [0x3fff0000, 0x7fff0000]):
        raise ProfileError('Unsupported SoC fixed-map ABI')
    for row in soc['private_mmio']:
        address, length = number(row['address']), number(row['size'], positive=True)
        if address & 0xfff or length & 0xfff or address + length > 2**32:
            raise ProfileError('Private MMIO range must be page-aligned and bounded')
    spans = sorted((number(r['address']), number(r['address']) + number(r['size'])) for r in soc['private_mmio'])
    if any(a[1] > b[0] for a, b in zip(spans, spans[1:])):
        raise ProfileError('Overlapping private MMIO ranges')
    if len(checked_bytes(root, firmware['rom'])) != 0x4000:
        raise ProfileError('Current EXR II runtime requires a 16 KiB ROM')
    labels = firmware.get('entry_labels')
    if not isinstance(labels, dict) or not labels or any(number(a) & 1 or not isinstance(v, str) or not v for a, v in labels.items()):
        raise ProfileError('Invalid firmware entry labels')
    if len({number(a) for a in labels}) != len(labels):
        raise ProfileError('Duplicate normalized entry addresses')
    manifest = json.loads(checked_bytes(root, firmware['manifest']))
    if manifest['source_sha256'] != entry['source']['sha256']:
        raise ProfileError('Manifest firmware identity mismatch')
    bias = number(firmware['flash_dat_bias'])
    if bias >= len(dat):
        raise ProfileError('Flash DAT bias exceeds source')
    flash = dat[bias:].translate(bytes(255 - n for n in range(256)))
    if 'boot_layout' in firmware:
        from .profile_layout import boot_layout
        manifest = boot_layout(entry, firmware, manifest, flash, root)
    for section in manifest['sections']:
        checked_bytes(root, dict(path=section['file'], sha256=section['sha256'], size=section['size']))
        if not 0 <= section['address'] < size or section['address'] + section['size'] > size:
            raise ProfileError('Mapped section exceeds board DRAM')
    names = {s['name'] for s in manifest['sections']}
    validation = firmware['loader_validation']
    if number(validation['entry']) not in {number(a) for a in labels} or any(n not in names for n in validation['sections']):
        raise ProfileError('Loader validation requires known entry/section names')
    reset = firmware['reset_release']
    if number(reset['source']) >= 128:
        raise ProfileError('Reset release source exceeds interrupt-controller grammar')
    for key in ('register', 'mask', 'target'):
        number(reset[key], positive=True)
    for span in reset['checks']:
        number(span['address'])
        if not isinstance(span['expected_hex'], str) or not re.fullmatch('([0-9a-f]{2})+', span['expected_hex']):
            raise ProfileError('Reset check requires nonempty expected bytes')
    validate_graph(board)
    return ResolvedProfile(firmware_id, path, hashlib.sha256(top_bytes).hexdigest(),
                           freeze({k: top[k]['sha256'] for k in parts}), freeze(soc), freeze(board),
                           freeze(firmware), freeze(manifest), flash, root)


def resolve(firmware_id=None, *, entry=None, profile_file=None, root=ROOT):
    try:
        return _resolve(firmware_id, entry=entry, profile_file=profile_file, root=root)
    except ProfileError:
        raise
    except (KeyError, TypeError, OSError, ValueError) as exc:
        raise ProfileError(f'Incomplete or malformed emulator profile: {exc}') from exc
