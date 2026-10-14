"""Newest captured owner preferences as declared persistent boot inputs.

No camera access. Preserve the restored journal blocks and append one synthetic
record with the normal-REC capture's active payload, using stock-format parity.
Stock ROM, managed NAND, journal selection and UI initialization consume it.
"""
import hashlib
import json
from pathlib import Path
import struct

from .storage_profile import (ROOT, BLOCK_SIZE, PARTITION_START, PAGES_PER_BLOCK,
                              captured_user_journals_profile, _validated_file)

CAPTURE = ROOT / 'research/hardware/ff80-bulk-dram-20261004T012034Z-51a5ee5e'
DRAM_SHA256 = '9034379b794614ac1d522b60bcd053104103190e80d76dd583c971fb19e75d99'
CAPTURE_SHA256 = '8474b5229dd798b2f3da2a9c75733e2f18b0fe617826cccf757bf3bddf1eaf90'
OWNER_SNAPSHOT = ROOT / '.tools/owner-settings/owner-rec'
WINDOWS = {
    'cfgdata': (0x2068000, 0x7e600, '84331afe8fe9a0d7df23591c0a4677e59a9ff3a3837f626b53d23b589cc54c96'),
    'preferences': (0xd08354, 0x4c74, '2619a4e8f442cb26ed69d31eb36b22e93f901870265360b86d21023010a102f0'),
    'main_record': (0xc28280, 0x1000, '6e09c78f13d3596ee5f4804c4d7b803379fcaea04990e5c37215d7b4dbb2c848'),
    'custom_record': (0xbe5c00, 0x1000, 'ad11a6ff26a72987e738610d743cdeb4690040db935bbf6d7473c12f3ac9e9ef'),
    'q_layout': (0xb56674, 136, 'c5e1da24f41c25d9399cc5df442b9f2f0a1ec7c5c47cdc6c9fa2bf2cd8e439a5'),
    'profile': (0xc9bef0, 0x170c, '46558b2467f61d8a54371530bb9a3583f2d6d9c50e2bdcee5f124c52cb26f668'),
}


def owner_capture(directory=CAPTURE):
    """Hash-check the entire sequential capture without allocating 512 MiB."""
    directory = Path(directory)
    metadata = json.loads(_validated_file(directory / 'capture.json', CAPTURE_SHA256))
    if metadata['ui_mode'] != 3 or metadata['live_a3'] != 0 or metadata['live_f7'] != 1:
        raise ValueError('Owner source is not the pinned normal REC capture')
    result = {}
    with (directory / 'dram.bin').open('rb') as stream:
        if stream.seek(0, 2) != 0x20000000:
            raise ValueError('Owner DRAM length differs')
        stream.seek(0)
        if hashlib.file_digest(stream, 'sha256').hexdigest() != DRAM_SHA256:
            raise ValueError('Owner DRAM hash differs')
        for name, (address, size, digest) in WINDOWS.items():
            stream.seek(address)
            data = stream.read(size)
            if len(data) != size or hashlib.sha256(data).hexdigest() != digest:
                raise ValueError('Owner capture window differs: ' + name)
            result[name] = data
    return result


def main_record_with_payload(record, payload):
    """Append successor with parity of 0x94871c/0x94868c (checked by ARM test)."""
    if len(record) != 0x1000 or len(payload) != 0xe74:
        raise ValueError('Owner main record geometry differs')
    result = bytearray(record)
    struct.pack_into('<H', result, 0, (struct.unpack_from('<H', record)[0] + 1) & 0xffff)
    result[0x104:0xf78] = payload
    result[0xf7c:] = bytes(0x84)
    bit_parity = 0
    for index in range(0xf7c // 4):
        word = struct.unpack_from('<I', result, index * 4)[0]
        bit_parity ^= word
        result[0xf7c + index // 8] |= (word.bit_count() & 1) << (index % 8)
    struct.pack_into('<I', result, 0xffc, bit_parity)
    return bytes(result)


def owner_inputs():
    data = owner_capture()
    profile = captured_user_journals_profile('restored')
    main_block = (PARTITION_START + 0xac0000 // BLOCK_SIZE) * BLOCK_SIZE
    custom_block = (PARTITION_START + 0xb20000 // BLOCK_SIZE) * BLOCK_SIZE
    if (profile['data_overlays'][main_block][0xb000:0xc000] != data['main_record'] or
            profile['data_overlays'][custom_block][0x7000:0x8000] != data['custom_record']):
        raise ValueError('Normal REC records disagree with validated restoration')
    payload = data['preferences'][0x14:0xe88]
    changes = [(i, old, new) for i, (old, new) in enumerate(zip(data['main_record'][0x104:0xf78], payload)) if old != new]
    if changes != [(0x4b, 7, 0), (0x51, 0, 2), (0x1c3, 0, 15)]:
        raise ValueError('Unexpected active/persistent preference differences')
    record = main_record_with_payload(data['main_record'], payload)
    block = bytearray(profile['data_overlays'][main_block])
    if block[0xc000:0xd000] != b'\xff' * 0x1000:
        raise ValueError('Synthetic owner append slot is not erased')
    block[0xc000:0xd000] = record
    profile['data_overlays'][main_block] = bytes(block)
    packet = data['preferences'][4:20]
    if packet[14] != 0xaa or sum(packet) % 256:
        raise ValueError('Owner remembered MCU packet framing differs')
    provenance = {
        'profile': 'owner', 'source_file': str(CAPTURE / 'dram.bin'),
        'source_sha256': DRAM_SHA256, 'capture_metadata_sha256': CAPTURE_SHA256,
        'windows': {name: dict(address=hex(a), bytes=n, sha256=h) for name, (a, n, h) in WINDOWS.items()},
        'persistent_main': dict(sequence=5739, sha256=WINDOWS['main_record'][2]),
        'persistent_custom': dict(sequence=151, sha256=WINDOWS['custom_record'][2]),
        'synthetic_main': dict(sequence=5740, logical_address='0xacc000', sha256=hashlib.sha256(record).hexdigest(),
            payload_sha256=hashlib.sha256(payload).hexdigest(), parity='0x94871c/0x94868c format',
            active_payload_changes=[dict(offset=hex(i), persistent=old, active=new,
                purpose='remembered Q cursor' if i == 0x1c3 else 'captured numbering/state field') for i, old, new in changes]),
        'mcu': dict(packet_hex=packet.hex(), packet_sha256=hashlib.sha256(packet).hexdigest(),
            source_ram_address='0x40d08358', method='remembered RAM packet inferred as endpoint9 replies'),
        'excluded_session_state': {
            'cfg_d8': 'capture 0x80 is stock initialization output; persistent bank value 0 retained',
            'cfg_205': 'capture 0x39 is stock initialization version output; persistent bank 0x36 retained',
            'cfg_f7': 'capture 1 debug USB excluded; existing persistent fixture 0 retained',
            'cfg_a3': 'normal capture and persistent fixture both 0; no service exit guard supplied',
            'debug_words_0xbe0940_0xbe0944': 'no captured RAM copied; values remain stock boot outputs',
            'ui_tasks_lens_sensor_caches': 'no captured execution state copied'},
        'limits': ['Sequential capture, not atomic or a claim about settings after 2026-10-04.',
                   'Synthetic successor record preserves every original slot; not a camera journal record.',
                   'NAND physical placement/OOB tags and MCU bus replies are inferred fixtures.',
                   'RTC remains the existing model; the source packet does not establish current date/time.'],
    }
    profile['provenance']['owner'] = provenance
    profile['provenance']['blocks'][0]['sha256'] = hashlib.sha256(profile['data_overlays'][main_block]).hexdigest()
    return dict(journals=profile, packet=packet, provenance=provenance)


def attach_owner_journals(nand, inputs):
    profile = inputs['journals']
    for offset, payload in profile['data_overlays'].items():
        nand.add_data_overlay(offset, payload, inputs['provenance'])
    for row, prefix in profile['spare_pages'].items():
        nand.add_spare_fixture(row, prefix, {'profile': 'owner', 'kind': 'synthetic managed logical tag'})
    return profile['provenance']


def verify_owner_state(board):
    """Fail first on Q, then read the AF settings produced by stock boot."""
    data = owner_capture()
    actual = bytes(board.read_memory(0xb56674, 136))
    if actual != data['q_layout']:
        raise ValueError('Owner Q layout differs; open Q through stock input before verification')
    key, node = struct.unpack_from('<II', actual, 6 * 8)
    if (key, node) != (0x11, 0xa7e630) or int.from_bytes(board.read_memory(node + 8, 4), 'little') != 0x2c7:
        raise ValueError('Owner FACEDETECT slot 6 identity differs')
    checks = {'q_layout': dict(address='0xb56674', sha256=hashlib.sha256(actual).hexdigest(), face_slot=6, key=key, setting=0x2c7)}
    fields = [('record_mode', 0xca6cf0, 4, 33), ('focus_mode', 0xca6d44, 4, 2),
              ('ae_af_area_link', 0xca6d20, 4, 1), ('af_area_box', 0xca6dbe, 4, 0x01010000),
              ('area_ranging_rate', 0xca6dc4, 4, 2), ('pre_af', 0xca6dcc, 4, 1),
              ('release_priority_saf', 0xca6dd0, 4, 0), ('release_priority_caf', 0xca6dd4, 4, 0),
              ('face', 0xca7170, 4, 1), ('side', 0xca7174, 4, 1), ('eye', 0xca7178, 4, 3)]
    for name, address, size, expected in fields:
        value = int.from_bytes(board.read_memory(address, size), 'little')
        if value != expected:
            raise ValueError(f'Owner {name} at {address:#x}: {value} != capture {expected}')
        checks[name] = dict(address=hex(address), bytes=size, value=value)
    profile = bytes(board.read_memory(0xc9bef0, 0x170c))
    # Saved namespaces: focus, AF mode, center coordinate, face/eye, sizes, zone/track centers, Q cursor.
    for lo, hi in [(0xa1, 0xb1), (0xe9, 0xea), (0xf7, 0xf8), (0x18b, 0x1a3), (0x1d3, 0x1d4)]:
        if profile[lo:hi] != data['profile'][lo:hi]:
            raise ValueError(f'Owner profile settings differ at {lo:#x}..{hi:#x}')
    checks['profile_af'] = dict(address='0xc9bef0', q_assignments=profile[0xa1:0xb1].hex(),
        focus_mode=profile[0xe9], af_mode=profile[0xf7], af_fields=profile[0x18b:0x1a3].hex(), q_cursor=profile[0x1d3])
    checks['area_class_idle'] = int.from_bytes(board.read_memory(0x162db68, 4), 'little')
    checks['area_class_capture'] = 6  # Runtime AF decision output, not a preference to transplant.
    checks['ui_mode'] = int.from_bytes(board.read_memory(0xcbd3e0, 4), 'little')
    checks['menu_active'] = int.from_bytes(board.read_memory(0xc91c68, 4), 'little')
    return checks


def rehearse_owner_q(board):
    """First boot Q check using the existing scanner stub, with no RAM observer.

    The AF portable-parent builder uses the same no-watch mode. Restore the
    original callback and zero scratch before returning a portable board.
    """
    from .testbed import Testbed
    from .testbed import inputs
    from .testbed.patch import word
    if board.failure is not None or word(board, 0xcbd3e0) != 3 or word(board, 0xc91c68):
        raise ValueError('Owner verification requires a successful closed REC boot')
    old_watch = inputs.watch
    inputs.watch = lambda *args, **kwargs: None
    try:
        tb = Testbed(board)
        before = board.instructions
        emitted = []
        def press(name, durations=(2_000_000, 4_000_000)):
            for state, count in zip(('ON', 'OFF'), durations):
                ticket = tb.debug_key(name, state)
                tb.run(count)
                if board.failure is not None or not ticket.done:
                    raise ValueError('Owner Q event did not complete: ' + name + ' ' + state)
                emitted.append(dict(key=name, state=state, event=inputs.KEY_EVENTS[name, state],
                                    instructions=board.instructions))
        press('K_Q')
        opened = verify_owner_state(board)  # Q is the first check; AF is secondary.
        if opened['menu_active'] != 1:
            raise ValueError('Owner Q did not open')
        # Stock boot initializes the remembered root key to slot 0, even when
        # the loaded profile cursor is 15. Reproduce the capture's remembered
        # selection with ordinary guarded navigation before closing Q.
        initial_index = word(board, 0xc90908)
        if initial_index != 15:
            if initial_index != 0 or any(word(board, 0xc90920 + 4*i) >= 2 for i in (0,12,15)):
                raise ValueError('Owner Q remembered cursor cannot use the verified 0->12->15 path')
            for key, target in [('UP',12), ('LEFT',15)]:
                press(key, (200_000,400_000))
                matched = tb.run_until(lambda t: t.read(0xc90908)==target, 4_000_000, step=50_000)
                if not matched.matched:
                    raise ValueError('Owner Q navigation missed index ' + str(target))
        press('DISP_B')
        idle = verify_owner_state(board)
        if idle['menu_active'] != 0 or idle['ui_mode'] != 3:
            raise ValueError('Owner Q did not return to REC')
        from x70_native.registers import PC
        for cpu in board.cpus:
            if inputs.BASE <= cpu.reg_read(PC) < inputs.BASE + inputs.BLOCK:
                raise ValueError('Owner Q stopped in injection scratch; cannot remove it')
        receipt = dict(base=hex(inputs.BASE), reserve=inputs.BLOCK, scanner_word=hex(inputs.SCANNER_WORD),
                       original=hex(inputs.SCANNER), replacement=hex(inputs.BASE),
                       code_sha256=hashlib.sha256(inputs.build_stub()).hexdigest(),
                       observation='ticket polling; no store hooks', restored=True)
        tb.stub.handle.remove()
        del board.testbed_scanner_stub, board.testbed_events
        return dict(first_boot_q=opened, idle_rec=idle, events=emitted, scanner=receipt,
                    guest_instructions=board.instructions-before, first_q_index=initial_index, saved_q_index=15,
                    method='stock scanner Q open/close; no direct settings RAM writes')
    finally:
        inputs.watch = old_watch
