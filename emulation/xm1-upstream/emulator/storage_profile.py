"""Hash-checked logical camera captures with explicit synthetic NAND placement.

The service bank labels are logical addresses. We choose identically numbered
physical blocks and supply matching first/last page tags solely as an emulator
fixture. Neither physical mapping nor OOB/ECC were captured. The public DAT and
original captures remain unchanged.
"""
from pathlib import Path
import hashlib
import json
import struct

ROOT = Path(__file__).resolve().parents[1]
CAPTURE = ROOT/'research/hardware/ff80-configuration-backup-fast-20261003'
METADATA_SHA256 = 'fef165b1013643e8cdcda845f751e945eff72de51b95fa2da64775ae40b8569b'
BANKS = (
    ('storage-bank-0.bin', 0x3a3, 40,
     'e09bbebcfcb24cf381b257150b276e5eca0812649101d4609d5425b684399090'),
    ('storage-bank-1.bin', 0x3cb, 1,
     'b5a41c3758763bbec72769fab4a2533bf2db0b6312d93d25a695f9e4b9e02260'),
)
BLOCK_SIZE = 0x20000
PARTITION_START = 0x37a
PAGES_PER_BLOCK = 64
FIRMWARE_SHA256 = '7dd5b358a66f38d1fc7dad899a77d35fe4e6ed0edd0ed1d04fbca512d5dc8fd6'
USER_CAPTURES = {
    'main-read': {
        'directory': ROOT/'research/hardware/ff80-main-settings-journal-read-20261003',
        'log': ('journal-read.json', '4497a8ac9f9530a09e15d662ef327c43d626df542f09c2918b40684792f83e8e'),
        'journals': (
            {'kind': 'main', 'metadata': 'settings_journal_cache_metadata.bin',
             'metadata_sha256': '36bce2174dcc958b5976cb624dc1822dc39520b7195a7065396ba90739eeadf8',
             'buffers': 'settings_journal_cache_buffers.bin',
             'buffers_sha256': '3fa5194c970fff1af61d4c040f41d5e4e745805e4a2eaf2d2c2744cbd9a4d581',
             'bases': (0xae0000, 0xac0000), 'latest_address': 0xaca000, 'latest_sequence': 5738,
             'latest_sha256': 'edcdce6a7b0e45604c00a14f97b997c2fa049c19bd6875c83957ebf819c8d8b8'},
        ),
    },
    'restored': {
        'directory': ROOT/'research/hardware/ff80-settings-restoration-20261003',
        'log': ('restore.json', '7f81e39f2d91cca61dcd26a34e6206808ea5947de9e186bbfd386f4afbeb5372'),
        'validation': ('readback-validation.json', 'afcf6df42fdb02758d6ee11b0e6f12b2dcee19cd9c939e8f9ae97d21757ad451'),
        'journals': (
            {'kind': 'main', 'metadata': 'after-main-cache-metadata.bin',
             'metadata_sha256': 'f007aa7e666751ae1ca480d758c16f599d61324e638ea8bd1af6d45333db46d3',
             'buffers': 'after-main-cache-buffers.bin',
             'buffers_sha256': '95bf825976ba3289988cdf9b41c9918a008f31e14bd03c14cf34bf94604c0b16',
             'bases': (0xac0000, 0xae0000), 'latest_address': 0xacb000, 'latest_sequence': 5739,
             'latest_sha256': '6e09c78f13d3596ee5f4804c4d7b803379fcaea04990e5c37215d7b4dbb2c848'},
            {'kind': 'custom', 'metadata': 'after-custom-cache-metadata.bin',
             'metadata_sha256': '4ccf994b1bae602023d2b725866bb0c0e3a69a6eab6a59bdfd4cc6bb4fb6d8dd',
             'buffers': 'after-custom-cache-buffers.bin',
             'buffers_sha256': '62aba5db73b7e084b601e0d85be1cdfb9c801b87450e5a840347b2f56a106203',
             'bases': (0xb20000, 0xb00000), 'latest_address': 0xb27000, 'latest_sequence': 151,
             'latest_sha256': 'ad11a6ff26a72987e738610d743cdeb4690040db935bbf6d7473c12f3ac9e9ef'},
        ),
    },
}


def _validated_file(path, expected_hash, expected_size=None):
    data = path.read_bytes()
    if hashlib.sha256(data).hexdigest() != expected_hash:
        raise ValueError(f'Unexpected capture bytes in {path.name}')
    if expected_size is not None and len(data) != expected_size:
        raise ValueError(f'Unexpected capture length in {path.name}')
    return data


def _synthetic_tag(index):
    if not 0 <= index < 0x72:
        raise ValueError('Managed partition index is out of range')
    value = index << 1
    value |= value.bit_count() & 1
    return value.to_bytes(2, 'big')*2+b'\xff'*12


def captured_storage_profile(directory=None):
    """Return byte overlays and 16-byte spare prefixes without camera access.

    All input hashes, sizes, bank labels and metadata fields are pinned to the
    saved capture. The two mappings in the return value contain bytes and are
    intended for NandController, not direct JSON serialization.
    """
    directory = Path(directory) if directory is not None else CAPTURE
    metadata_path = directory/'configuration-backup.json'
    metadata_bytes = _validated_file(metadata_path, METADATA_SHA256)
    metadata = json.loads(metadata_bytes)
    views = {v['file']: v for v in metadata['views']}
    overlays, spare_pages, banks = {}, {}, []
    for filename, block, count, digest in BANKS:
        view = views[filename]
        logical = block-PARTITION_START
        if (int(view['bank'], 16) != block or view['blocks'] != count or
                view['block_size'] != BLOCK_SIZE or
                int(view['loader_first_block'], 16) != logical or
                view['sha256'] != digest):
            raise ValueError(f'Unexpected bank descriptor for {filename}')
        data = _validated_file(directory/filename, digest, count*BLOCK_SIZE)
        overlays[block*BLOCK_SIZE] = data
        for offset in range(count):
            tag = _synthetic_tag(logical+offset)
            first = (block+offset)*PAGES_PER_BLOCK
            spare_pages[first] = tag
            spare_pages[first+PAGES_PER_BLOCK-1] = tag
        banks.append({'file': str(directory/filename), 'sha256': digest,
                      'bytes': len(data), 'logical_first_index': hex(logical),
                      'synthetic_physical_first_block': hex(block), 'blocks': count})
    report = {
        'name': 'captured-logical-banks / synthetic-physical-map',
        'metadata': str(metadata_path), 'metadata_sha256': METADATA_SHA256,
        'capture_time_utc': metadata['time_utc'], 'banks': banks,
        'spare_prefix_count': len(spare_pages),
        'spare_encoding': 'BE16((index<<1)|popcount(index<<1)%2) twice; 12 FF bytes',
        'debug_flag_in_storage_view': metadata['debug_flag_in_storage_view'],
        'limits': ['Bank payloads are captured logical service-loaded data, not raw NAND.',
                   'One-to-one physical placement and all supplied spare tags are synthetic.',
                   'Unspecified spare/ECC bytes and unpopulated data remain ideal erased fixtures.',
                   'Physical recovery, bad-block layout and electrical timing are unvalidated.'],
    }
    return {'data_overlays': overlays, 'spare_pages': spare_pages, 'provenance': report}


def attach_captured_storage_profile(nand, directory=None, *, settings=None):
    """Attach validated profile through the NAND owner's explicit fixture API."""
    profile = captured_storage_profile(directory)
    provenance = profile['provenance']
    if settings=='owner':
        from .owner_settings import owner_capture
        captured=owner_capture()['cfgdata']
        stored=next(iter(profile['data_overlays'].values()))[:len(captured)]
        if [(i,a,b) for i,(a,b) in enumerate(zip(stored,captured)) if a!=b] != [(0xd8,0,128),(0xf7,0,1),(0x205,54,57)]:
            raise ValueError('Owner calibration/config image has unexpected differences')
        provenance['settings']='owner; captured persistent calibration unchanged; cfg d8/f7/205 exclusions declared in owner provenance'
    for offset, payload in profile['data_overlays'].items():
        bank = next(x for x in provenance['banks']
                    if int(x['synthetic_physical_first_block'], 16)*BLOCK_SIZE == offset)
        nand.add_data_overlay(offset, payload,
                              {'profile': provenance['name'], **bank,
                               'physical_placement': 'synthetic one-to-one managed mapping'})
    for row, prefix in profile['spare_pages'].items():
        nand.add_spare_fixture(row, prefix,
                              {'profile': provenance['name'], 'kind': 'synthetic managed logical tag',
                               'logical_index': hex(row//PAGES_PER_BLOCK-PARTITION_START),
                               'decoder_validation': 'https://github.com/justjake/x70view/blob/230a1267bb3115fdd40dc8eec3b9ac60e84c6e0e/research/emulator-rom/managed-nand-tag-validation.json'})
    return provenance


def captured_user_journals_profile(source='restored', directory=None):
    """Optional full captured user-journal blocks with synthetic managed tags.

    Source selection is explicit and independent of the factory-bank and MCU
    input profiles. No record is replaced, synthesized, or selected by changing
    firmware preference flags. Captured cache metadata determines block order.
    """
    if source not in USER_CAPTURES:
        raise ValueError('Unknown captured user journal source')
    specification = USER_CAPTURES[source]
    directory = Path(directory) if directory is not None else specification['directory']
    log_file, log_hash = specification['log']
    log_path = directory/log_file
    log = json.loads(_validated_file(log_path, log_hash))
    if log['proposal']['firmware_sha256'] != FIRMWARE_SHA256:
        raise ValueError('Captured user journal belongs to another firmware')
    validation = None
    if 'validation' in specification:
        filename, digest = specification['validation']
        checked = json.loads(_validated_file(directory/filename, digest))
        if (checked['firmware_sha256'] != FIRMWARE_SHA256 or
                checked['camera_restore_log_sha256'] != log_hash or
                any(item['validator_result'] or item['corrected_bytes'] for item in checked['checks'])):
            raise ValueError('Captured user journal readback validation failed')
        validation = {'file': str(directory/filename), 'sha256': digest,
                      'method': checked['method'], 'checks': checked['checks']}
    overlays, spare_pages, blocks, journals = {}, {}, [], []
    for descriptor in specification['journals']:
        metadata_path, buffers_path = directory/descriptor['metadata'], directory/descriptor['buffers']
        metadata = _validated_file(metadata_path, descriptor['metadata_sha256'], 32)
        buffers = _validated_file(buffers_path, descriptor['buffers_sha256'], 2*BLOCK_SIZE)
        bases = struct.unpack_from('<II', metadata)
        if bases != descriptor['bases']:
            raise ValueError('Unexpected captured user journal cache order')
        latest_base = descriptor['latest_address'] & ~(BLOCK_SIZE-1)
        latest_offset = descriptor['latest_address']-latest_base
        latest_cache = bases.index(latest_base)
        record = buffers[latest_cache*BLOCK_SIZE+latest_offset:latest_cache*BLOCK_SIZE+latest_offset+0x1000]
        if (hashlib.sha256(record).hexdigest() != descriptor['latest_sha256'] or
                struct.unpack_from('<H', record)[0] != descriptor['latest_sequence']):
            raise ValueError('Expected captured user journal record is missing')
        journals.append({'kind': descriptor['kind'], 'metadata_file': str(metadata_path),
                         'metadata_sha256': descriptor['metadata_sha256'], 'buffers_file': str(buffers_path),
                         'buffers_sha256': descriptor['buffers_sha256'],
                         'cache_order': [hex(base) for base in bases],
                         'expected_latest_record': {'logical_address': hex(descriptor['latest_address']),
                                                    'sequence': descriptor['latest_sequence'],
                                                    'sha256': descriptor['latest_sha256']}})
        for cache, logical_base in enumerate(bases):
            index = logical_base//BLOCK_SIZE
            physical_block = PARTITION_START+index
            payload = buffers[cache*BLOCK_SIZE:(cache+1)*BLOCK_SIZE]
            offset = physical_block*BLOCK_SIZE
            if offset in overlays or not 0 <= offset < 0x8000000-BLOCK_SIZE+1:
                raise ValueError('Captured journal synthetic placement is invalid')
            overlays[offset] = payload
            tag = _synthetic_tag(index)
            first = physical_block*PAGES_PER_BLOCK
            spare_pages[first] = spare_pages[first+PAGES_PER_BLOCK-1] = tag
            blocks.append({'kind': descriptor['kind'], 'logical_base': hex(logical_base),
                           'logical_index': hex(index), 'synthetic_physical_block': hex(physical_block),
                           'native_byte_offset': hex(offset), 'cache_index': cache,
                           'bytes': len(payload), 'sha256': hashlib.sha256(payload).hexdigest(),
                           'source_file': str(buffers_path), 'source_file_offset': cache*BLOCK_SIZE,
                           'source_file_sha256': descriptor['buffers_sha256']})
    provenance = {
        'name': f'captured user journals ({source}) / synthetic-physical-map',
        'source': source, 'firmware_sha256': FIRMWARE_SHA256,
        'capture_log': str(log_path), 'capture_log_sha256': log_hash,
        'capture_window_utc': {'started': log['started_utc'], 'finished': log['finished_utc']},
        'journals': journals, 'blocks': blocks, 'readback_validation': validation,
        'spare_prefix_count': len(spare_pages),
        'spare_encoding': 'BE16((index<<1)|popcount(index<<1)%2) twice; 12 FF bytes',
        'limits': ['Full captured logical cache blocks are preserved, including unused journal slots.',
                   'Physical placement and first/last-page OOB tags are synthetic; actual mapping/ECC was not captured.',
                   'Factory/service banks require their separate profile and remain unchanged.',
                   'MCU input packet requires its separate profile; this helper does not patch settings flags or UI mode.',
                   'Record integrity and remembered input do not establish normal boot, physical NAND timing, or live view.'],
    }
    return {'data_overlays': overlays, 'spare_pages': spare_pages, 'provenance': provenance}


def attach_captured_user_journals_profile(nand, source='restored', directory=None):
    """Attach optional journals before virtual NAND execution begins."""
    if source=='owner':
        if directory is not None:
            raise ValueError('Owner settings source is pinned')
        from .owner_settings import attach_owner_journals, owner_inputs
        return attach_owner_journals(nand,owner_inputs())
    profile = captured_user_journals_profile(source, directory)
    provenance = profile['provenance']
    for offset, payload in profile['data_overlays'].items():
        block = next(item for item in provenance['blocks'] if int(item['native_byte_offset'], 16) == offset)
        nand.add_data_overlay(offset, payload, {'profile': provenance['name'], **block,
                                              'physical_placement': 'synthetic one-to-one managed mapping'})
    for row, prefix in profile['spare_pages'].items():
        nand.add_spare_fixture(row, prefix, {'profile': provenance['name'],
            'kind': 'synthetic managed logical tag',
            'logical_index': hex(row//PAGES_PER_BLOCK-PARTITION_START),
            'decoder_validation': 'https://github.com/justjake/x70view/blob/230a1267bb3115fdd40dc8eec3b9ac60e84c6e0e/research/emulator-rom/managed-nand-tag-validation.json'})
    return provenance


def owner_mcu_profile():
    from .owner_settings import owner_inputs
    inputs=owner_inputs()
    return dict(packet=inputs['packet'], profile_name='owner remembered MCU packet / inferred SPI input',
                provenance=inputs['provenance'])
