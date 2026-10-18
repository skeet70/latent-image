"""Fresh-engine/process reconstruction with ARM MMIO, RAM and a pending IRQ."""
import gc
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from emulator.boot import make_board
from emulator.boot_devices import BootControls, AcquisitionActuatorProfile, AcquisitionDrive
from emulator.checkpoint import CheckpointError
from emulator.pulse_devices import PulseOutputs
from emulator.power_devices import PowerDevices
from emulator.scanout import DisplayScanout, RectangleProfile
from x70_native.checkpoint import capture, semantic_fingerprint
from x70_native.portable_checkpoint import load, save, PortableCheckpointError, _require_native_portable
from unicorn.arm_const import (UC_ARM_REG_PC, UC_ARM_REG_CPSR, UC_ARM_REG_R0,
    UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3, UC_ARM_REG_R4,
    UC_ARM_REG_R5, UC_ARM_REG_R9, UC_ARM_REG_R10, UC_ARM_REG_SP)


def ready(*, firmware_profile=None, rectangle_profile=None, sd_card_profile=None, sd_card_image=None,
          ui_conversion_profile=None):
    options = {} if ui_conversion_profile is None else {'ui_conversion_profile': ui_conversion_profile}
    board = make_board(entry='loader', quantum=8, storage_profile='erased',
                       counter_input_profile=PulseOutputs.COUNTER_LENS_PROFILE,
                       firmware_profile=firmware_profile, rectangle_profile=rectangle_profile,
                       sd_card_profile=sd_card_profile, sd_card_image=sd_card_image,
                       sd_card_present=sd_card_image is not None, **options)
    board.core_state[1]['runnable'] = False
    board.interrupts.banks[0].enabled = 1 << 22
    controls = next(d for d in board.devices if type(d) is BootControls)
    controls.set_gpio_input_pin(0, 0, 0, source='portable-unit-input')
    pulses = next(d for d in board.devices if type(d) is PulseOutputs)
    pulses.write(0, pulses.BASE, 4, 0x110000)
    for channel in (0, 4):
        base = pulses.CHANNEL_BASE + channel * 0x100
        for offset, value in ((0, pulses.COUNTER_CONTROLS[channel]), (4, 0), (8, 0), (12, 0), (16, 0xffff)):
            pulses.write(0, base + offset, 4, value)
        pulses.write(0, pulses.BASE + 16, 4, 0 if channel == 0 else 0x40000)
        pulses.write(0, pulses.BASE, 4, 0x10001 << channel)
    for offset, value in ((0, 0xc000), (4, 0), (8, 24000), (12, 23999), (16, 0)):
        pulses.write(0, pulses.CHANNEL_BASE + 0x200 + offset, 4, value)
    pulses.write(0, pulses.BASE, 4, 0x40004)
    # PanelSerial is constructed after a completed shift.
    power = next(d for d in board.devices if type(d) is PowerDevices)
    transport, selection, framing, _ = power._panel_contract
    for port, pin, level in (selection[:3], selection[3:]):
        mask = 1 << pin
        controls.write(0, controls.GPIO + port * 0x40, 4, mask << 16 | level << pin)
    for offset, value in ((0, transport[0]), (4, transport[1]), (8, 0)):
        power.write(0, power.SPI + offset, 4, value)
    if framing[2] is None:
        initial_panel_word, continued_panel_word = 0x401, 0x402
    else:
        initial_panel_word = 0xb0 << framing[2]
        continued_panel_word = 1 << framing[1] | 4 << framing[2]
    power.write(0, power.SPI + 0x1c, 4, initial_panel_word)
    # Real ARM waits past the one-shot expiry, samples two strict devices,
    # writes RAM, and stops the peripheral. IRQ22 remains masked and pending.
    program = (0xe2500001, 0x1afffffd, 0xe5927000, 0xe5938000,
               0xe0877008, 0xe5857000, 0xe5841000, 0xe58a9000,
               0xe2866001, 0xe5856004, 0xeafffffc)
    board.write_memory(0x30000, struct.pack('<11I', *program))
    cpu = board.cpus[0]
    for reg, value in ((UC_ARM_REG_PC, 0x30000), (UC_ARM_REG_CPSR, 0xd3),
                       (UC_ARM_REG_R0, 12004), (UC_ARM_REG_R1, 0x40000),
                       (UC_ARM_REG_R2, controls.GPIO + 0x14),
                       (UC_ARM_REG_R3, pulses.CHANNEL_BASE + 0x400 + 8),
                       (UC_ARM_REG_R4, pulses.BASE), (UC_ARM_REG_R5, 0x40000),
                       (UC_ARM_REG_R9, continued_panel_word), (UC_ARM_REG_R10, power.SPI + 0x1c)):
        cpu.reg_write(reg, value)
    board.run(30000, pause_after=23984)
    if board.failure or pulses.settle_timer['state'] != 'pending':
        raise AssertionError(board.failure or pulses.snapshot())
    if power.panel_serial.words_received != 1 or power.spi_busy or board.scheduled:
        raise AssertionError(power.snapshot())
    return board


def continuation(board, level=1, edges=9):
    controls = next(d for d in board.devices if type(d) is BootControls)
    pulses = next(d for d in board.devices if type(d) is PulseOutputs)
    power = next(d for d in board.devices if type(d) is PowerDevices)
    controls.set_gpio_input_pin(0, 0, level, source='portable-unit-input')
    pulses.add_counter_edges(4, edges, source=PulseOutputs.COUNTER_LENS_PROFILE)
    board.run(30000, pause_after=board.instructions + 48)
    return dict(fingerprint=semantic_fingerprint(board),
                memory=list(struct.unpack('<2I', board.read_memory(0x40000, 8))),
                pending=board.interrupts.banks[0].pending,
                expiries=pulses.settle_timer_expiries, timer=pulses.settle_timer,
                enabled=pulses.enabled, panel=power.panel_serial.snapshot(), failure=board.failure)


class PortableCheckpointTests(unittest.TestCase):
    def setUp(self):
        Path('.tools').mkdir(exist_ok=True)
        self.tmp = tempfile.TemporaryDirectory(prefix='portable-test-', dir='.tools')

    def tearDown(self):
        self.tmp.cleanup()
        gc.collect()

    def test_all_stock_devices_fresh_engines_and_fresh_process_match_same_engine_replay(self):
        board = ready()
        path = Path(self.tmp.name) / 'paused'
        original = semantic_fingerprint(board)
        saved = capture(board)
        manifest = save(board, path)
        self.assertEqual(semantic_fingerprint(board), original)
        expected = continuation(board)
        self.assertIsNone(expected['failure'])
        controls = next(d for d in board.devices if type(d) is BootControls)
        self.assertEqual(expected['memory'][0], controls.gpio_inputs[0] + 9)
        self.assertEqual(expected['pending'], 1 << 22)
        self.assertEqual(expected['expiries'], 1)
        self.assertIsNone(expected['timer'])
        self.assertEqual(expected['enabled'], 0x11)
        self.assertEqual(expected['panel']['words_received'], 2)
        self.assertEqual([item['word'] for item in expected['panel']['retained_words']], ['0x401', '0x402'])
        saved.restore()
        expected_low = continuation(board, level=0, edges=3)
        self.assertNotEqual(expected_low['memory'], expected['memory'])
        saved.restore()
        self.assertEqual(continuation(board), expected)
        restored = load(path)
        self.assertIsNot(restored, board)
        self.assertEqual(semantic_fingerprint(restored), original)
        self.assertEqual(continuation(restored), expected)
        self.assertEqual(semantic_fingerprint(board), expected['fingerprint'])
        del restored
        gc.collect()
        code = ('import json,sys; from x70_native.portable_checkpoint import load; '
                'from native.tests.test_portable_checkpoint import continuation; '
                'print(json.dumps(continuation(load(sys.argv[1]),int(sys.argv[2]),int(sys.argv[3])),sort_keys=True))')
        env = dict(os.environ, PYTHONPATH='native/python:.')
        children = [subprocess.Popen([sys.executable, '-c', code, str(path), str(level), str(edges)], env=env,
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True) for level, edges in ((1, 9), (0, 3))]
        results = []
        for child in children:
            out, error = child.communicate(timeout=60)
            self.assertEqual(child.returncode, 0, error)
            results.append(json.loads(out))
        self.assertEqual(results, [expected, expected_low])
        self.assertEqual(semantic_fingerprint(board), expected['fingerprint'])
        saved.restore()
        self.assertEqual(semantic_fingerprint(board), original)
        self.assertEqual(continuation(board), expected)
        self.assertEqual(manifest['fingerprint'], original)
        self.assertNotIn('opaque_context', (path / 'manifest.json').read_text())

    def test_pending_acquisition_motion_preserves_gpio_irq_and_phase_timing(self):
        board = ready()
        controls = next(d for d in board.devices if type(d) is BootControls)
        board.write_memory(0x30000, struct.pack('<2I', 0xe2866001, 0xeafffffd))
        board.cpus[0].reg_write(UC_ARM_REG_PC, 0x30000)
        bank = board.interrupts.banks[0]
        bank.enabled |= 1 << 0x11
        bank.config[0x184] = 2 << 2
        bank.config[0x844] = 12
        controls.write(0, controls.GPIO + 0x40, 4, 0x60002)
        start = board.virtual_ticks
        path = Path(self.tmp.name) / 'acquisition'
        save(board, path)
        restored = load(path)
        observations = []
        for current in (board, restored):
            pins = next(d for d in current.devices if type(d) is BootControls)
            irq = current.interrupts.banks[0]
            seen = []
            for advance in (2992, 8):
                current.run(3200, pause_after=current.instructions + advance)
                self.assertIsNone(current.failure)
                seen.append((current.virtual_ticks, pins.gpio_input_pin(0, 9),
                             pins.gpio_input_pin(0, 6), irq.pending & (1 << 0x11)))
            self.assertEqual(seen[0][0], start + 2992)
            self.assertEqual(seen[0][1:], (1, 1, 0))
            self.assertEqual(seen[1][0], start + 3000)
            self.assertEqual(seen[1][1:], (0, 0, 1 << 0x11))
            irq.pending &= ~(1 << 0x11)
            pins.write(0, pins.GPIO + 0x40, 4, 0x60002)
            repeated = Path(self.tmp.name) / ('repeated-' + str(len(observations)))
            save(current, repeated)
            current = load(repeated)
            pins = next(d for d in current.devices if type(d) is BootControls)
            irq = current.interrupts.banks[0]
            current.run(3200, pause_after=current.instructions + 3000)
            self.assertIsNone(current.failure)
            self.assertEqual(irq.pending & (1 << 0x11), 0)
            irq.config[0x184] = 1 << 2
            pins.write(0, pins.GPIO + 0x40, 4, 0x60006)
            current.run(3200, pause_after=current.instructions + 3000)
            self.assertIsNone(current.failure)
            seen.append((current.virtual_ticks, pins.gpio_input_pin(0, 9),
                         pins.gpio_input_pin(0, 6), irq.pending & (1 << 0x11)))
            self.assertEqual(seen[-1][1:], (1, 1, 1 << 0x11))
            observations.append(seen)
        self.assertEqual(observations[0], observations[1])

    def test_partial_acquisition_command_restores_without_inventing_motion(self):
        board = ready()
        controls = next(d for d in board.devices if type(d) is BootControls)
        board.write_memory(0x30000, struct.pack('<2I', 0xe2866001, 0xeafffffd))
        board.cpus[0].reg_write(UC_ARM_REG_PC, 0x30000)
        controls.write(0, controls.GPIO + 0x40, 4, 0x20002)
        path = Path(self.tmp.name) / 'partial-acquisition'
        save(board, path)
        restored = load(path)
        for current in (board, restored):
            pins = next(d for d in current.devices if type(d) is BootControls)
            self.assertEqual(pins.gpio_output_pin(1, 1), 1)
            self.assertIsNone(pins.gpio_output_pin(1, 2))
            current.run(3200, pause_after=current.instructions + 3000)
            self.assertIsNone(current.failure)
            self.assertEqual((pins.gpio_input_pin(0, 9), pins.gpio_input_pin(0, 6)), (1, 1))

    def test_coalesced_acquisition_command_restores_between_gpio_writes(self):
        board = ready()
        controls = next(d for d in board.devices if type(d) is BootControls)
        board.write_memory(0x30000, struct.pack('<2I', 0xe2866001, 0xeafffffd))
        board.cpus[0].reg_write(UC_ARM_REG_PC, 0x30000)
        controls.write(0, controls.GPIO + 0x40, 4, 0x60006)
        board.run(3200, pause_after=board.instructions + 3000)
        controls.write(0, controls.GPIO + 0x40, 4, 0x20000)
        path = Path(self.tmp.name) / 'coalesced-acquisition'
        save(board, path)
        restored = load(path)
        pins = next(d for d in restored.devices if type(d) is BootControls)
        self.assertEqual((pins.gpio_output_pin(1, 1), pins.gpio_output_pin(1, 2)), (0, 1))
        pins.write(0, pins.GPIO + 0x40, 4, 0x40000)
        restored.run(3200, pause_after=restored.instructions + 3000)
        self.assertIsNone(restored.failure)
        self.assertEqual((pins.gpio_input_pin(0, 9), pins.gpio_input_pin(0, 6)), (1, 1))

    def test_rectangle_snapshot_preserves_clipped_edges_without_double_blending_corners(self):
        board = ready(rectangle_profile=RectangleProfile.XM1.value)
        board.write_memory(0x31000, bytes((40, 40, 128, 128)) * 24)
        display = next(d for d in board.devices if type(d) is DisplayScanout)
        # Synthetic fixture ABI: stock X-M1 1.50 setters 006CA04C/006CA328/006CA3E0.
        registers = {0: 1, 0x10: 0x70005, 0x24: 0, 0x28: 0x70005,
                     0x2c: 0x31000, 0x30: 0, 0xd4: 0x10001000, 0xd8: 0, 0xdc: 0,
                     0x108: 1, 0x11c: 0x20001, 0x120: 0x80005, 0x16c: 2,
                     0x174: 0, 0x178: 200, 0x188: 0, 0x18c: 64, 0x190: 0}
        for offset, value in registers.items():
            display.write(0, display.BASES[0] + offset, 4, value)
        path = Path(self.tmp.name) / 'rectangles'
        save(board, path)
        restored = load(path)
        expected = [[40, 40, 40, 40, 40, 40, 40, 40],
                    [40, 40, 120, 120, 120, 120, 120, 120],
                    [40, 40, 120, 120, 120, 120, 120, 120],
                    [40, 40, 120, 120, 40, 40, 40, 120],
                    [40, 40, 120, 120, 120, 120, 120, 120],
                    [40, 40, 120, 120, 120, 120, 120, 120]]
        for current in (board, restored):
            output = next(d for d in current.devices if type(d) is DisplayScanout)
            output.frame_boundary(pixels=False)
            frame = output.compose(0)
            self.assertEqual(frame.rgb[:, :, 0].tolist(), expected)
            self.assertEqual(frame.rgb[:, :, 1].tolist(), expected)
            self.assertEqual(frame.rgb[:, :, 2].tolist(), expected)

    def test_invalid_acquisition_values_and_pending_motion_refuse_portable_load(self):
        board = ready()
        controls = next(d for d in board.devices if type(d) is BootControls)
        controls.write(0, controls.GPIO + 0x40, 4, 0x60002)
        path = Path(self.tmp.name) / 'invalid-acquisition'
        manifest = save(board, path)
        original_model = gzip.decompress((path / 'model.json.gz').read_bytes())
        cases = [
            ('profile', 'unknown'), ('profile', None), ('profile', AcquisitionDrive.IDLE),
            ('phase', 'DRIVE'), ('phase', 1), ('phase', True), ('phase', None),
            ('phase', AcquisitionActuatorProfile.XM1),
            ('due_tick', -1), ('due_tick', True), ('due_tick', '3000'),
            ('due_tick', None), ('settle_ticks', 0), ('settle_ticks', -1),
            ('settle_ticks', True), ('settle_ticks', '3000'),
            ('transitions', -1), ('transitions', True), ('transitions', '0'),
            ('virtual_ticks', -1), ('virtual_ticks', True),
            ('acquisition_actuator', 'untyped'),
            ('gpio_output_known', 0)]
        for field, invalid in cases:
            with self.subTest(field=field, invalid=invalid):
                model = json.loads(original_model)
                actuator = next(node for node in model['nodes']
                                if node.get('cls') == 'emulator.boot_devices.AcquisitionActuator')
                if field == 'virtual_ticks':
                    owner, attr, old = board, field, board.virtual_ticks
                    model['board'][field] = invalid
                    setattr(owner, attr, invalid)
                elif field == 'acquisition_actuator':
                    owner, attr, old = controls, field, controls.acquisition_actuator
                    model['devices'][board.devices.index(controls)][field] = invalid
                    setattr(owner, attr, invalid)
                elif field.startswith('gpio_output_'):
                    owner = getattr(controls, field)
                    attr, old = 1, owner[1]
                    state = model['devices'][board.devices.index(controls)]
                    words = model['nodes'][state[field]['node']]['items']
                    next(item for item in words if item[0] == 1)[1] = invalid
                    owner[attr] = invalid
                else:
                    owner, attr = controls.acquisition_actuator, field
                    old = getattr(owner, attr)
                    actuator['fields'][field] = (
                        {'enum': [type(invalid).__module__ + '.' + type(invalid).__qualname__, invalid.value]}
                        if isinstance(invalid, (AcquisitionActuatorProfile, AcquisitionDrive)) else invalid)
                    setattr(owner, attr, invalid)
                try:
                    manifest['fingerprint'] = semantic_fingerprint(board)
                    raw = json.dumps(model, sort_keys=True, separators=(',', ':')).encode()
                    compressed = gzip.compress(raw, mtime=0)
                    manifest['model'] = {
                        'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest(),
                        'compressed_bytes': len(compressed),
                        'compressed_sha256': hashlib.sha256(compressed).hexdigest()}
                    (path / 'model.json.gz').write_bytes(compressed)
                    (path / 'manifest.json').write_text(json.dumps(manifest))
                    with self.assertRaises(PortableCheckpointError):
                        load(path)
                finally:
                    if isinstance(owner, dict):
                        owner[attr] = old
                    else:
                        setattr(owner, attr, old)

    def test_unsafe_state_unknown_host_resource_and_custom_callback_refuse(self):
        board = ready()
        cases = ('scheduled', 'busy', 'external', 'callback', 'panel_schema', 'fetch', 'explore', 'rom', 'audit')
        for index, bad in enumerate(cases):
            with self.subTest(bad=bad):
                path = Path(self.tmp.name) / str(index)
                cleanup = lambda: None
                if bad == 'scheduled':
                    board.schedule(1, lambda: None)
                    cleanup = board.scheduled.clear
                elif bad == 'busy':
                    board.devices[-1].busy = True
                    cleanup = lambda: delattr(board.devices[-1], 'busy')
                elif bad == 'external':
                    board.host_resource = io.StringIO('external')
                    cleanup = lambda: delattr(board, 'host_resource')
                elif bad == 'callback':
                    old = board.nand.record
                    board.nand.record = lambda *a, **k: None
                    cleanup = lambda: setattr(board.nand, 'record', old)
                elif bad == 'panel_schema':
                    panel = next(d for d in board.devices if type(d) is PowerDevices).panel_serial
                    panel.extra = 1
                    cleanup = lambda: delattr(panel, 'extra')
                elif bad == 'audit':
                    board._native.audit_begin(0, 0x30000, 0x30028)
                elif bad == 'fetch':
                    board._native.fetch_guard_begin(0, [(0x30000, 0x30028)])
                    cleanup = lambda: board._native.fetch_guard_end(0)
                elif bad == 'rom':
                    old_byte = board.rom_storage[0]
                    board.rom_storage[0] ^= 1
                    cleanup = lambda: board.rom_storage.__setitem__(0, old_byte)
                else:
                    board.mmio_policy = 'explore'
                    cleanup = lambda: delattr(board, 'mmio_policy')
                try:
                    with self.assertRaises(CheckpointError):
                        save(board, path)
                    self.assertFalse(path.exists())
                finally:
                    cleanup()
        self.assertIsNone(board.failure)
        self.assertEqual(board.instructions, 23984)

    def test_wrong_schema_changed_blob_and_factory_topology_refuse(self):
        board = ready()
        path = Path(self.tmp.name) / 'paused'
        save(board, path)
        filename = path / 'manifest.json'
        raw = filename.read_bytes()
        manifest = json.loads(raw)
        manifest['schema'] = 'unknown'
        filename.write_text(json.dumps(manifest))
        with self.assertRaises(PortableCheckpointError):
            load(path)
        manifest = json.loads(raw)
        manifest['device_classes'].pop()
        filename.write_text(json.dumps(manifest))
        with self.assertRaises(PortableCheckpointError):
            load(path)
        filename.write_bytes(raw)
        digest = next(iter(manifest['blobs']))
        blob = path / (digest + '.gz')
        data = bytearray(blob.read_bytes())
        data[-1] ^= 1
        blob.write_bytes(data)
        with self.assertRaises(PortableCheckpointError):
            load(path)
        self.assertIsNone(board.failure)

    def test_unexported_native_state_and_missing_or_malformed_eligibility_refuse(self):
        board = ready()
        # Host PMSELR writes bypass Rust's guest-selector tracking.
        board.cpus[0].cpr_write(15, 0, 9, 12, 5, 0, False, 1)
        status = board._native.portable_checkpoint_status()
        self.assertFalse(status['supported'])
        self.assertTrue(status['reasons'])
        board.invalidate_execution_caches()
        self.assertEqual(board._native.portable_checkpoint_status(), status)
        path = Path(self.tmp.name) / 'unsafe-native'
        with self.assertRaises(PortableCheckpointError):
            save(board, path)
        self.assertFalse(path.exists())
        with self.assertRaises(PortableCheckpointError):
            _require_native_portable(SimpleNamespace(_native=object()))
        for malformed in ({}, {'contract': 'unknown', 'supported': True, 'reasons': []},
                          {'contract': 'lifetime-arm32-rust-state-v1', 'supported': 1, 'reasons': []}):
            fake = SimpleNamespace(_native=SimpleNamespace(portable_checkpoint_status=lambda: malformed))
            with self.assertRaises(PortableCheckpointError):
                _require_native_portable(fake)

    def test_imported_histogram_helper_behavior_change_refuses_restore(self):
        from emulator import histogram_protocol
        from emulator.hardware import UnsupportedDevice
        board = ready()
        path = Path(self.tmp.name) / 'paused-helper'
        saved = save(board, path)
        self.assertIn('emulator.histogram_protocol', saved['identity']['models'])
        with self.assertRaises(UnsupportedDevice):
            histogram_protocol.validate_sample({})
        # A validator override leaves Producer's own class digest unchanged.
        with patch.object(histogram_protocol, 'validate_sample', lambda record: None):
            self.assertIsNone(histogram_protocol.validate_sample({}))
            with self.assertRaises(PortableCheckpointError):
                load(path)
        self.assertEqual(semantic_fingerprint(board), saved['fingerprint'])


class RecoveryCardMediaTests(unittest.TestCase):
    def test_complete_backing_exceeds_dirty_budget_and_remains_immutable(self):
        from emulator.sd_card import SparseCard, load_raw_image
        with tempfile.TemporaryDirectory() as directory:
            image = Path(directory) / 'complete.img'
            with image.open('wb') as target:
                target.write(b'\x6a' * (33 << 20))
            card = SparseCard(max_dirty_sectors=1)
            load_raw_image(card, image, immutable_backing=True)
            image.write_bytes(b'\x91' * 512)
            self.assertEqual(card.read(0, 1), b'\x6a' * 512)
            self.assertEqual(card.read(65536, 1), b'\x6a' * 512)
            card.write(0, bytes(512))
            self.assertEqual(card.read(0, 2), bytes(512) + b'\x6a' * 512)
            from emulator.hardware import UnsupportedDevice
            with self.assertRaises(UnsupportedDevice):
                card.write(1, b'\x33' * 512)
            self.assertEqual(card.read(1, 1), b'\x6a' * 512)

    def test_logical_export_keeps_zero_overwrite_and_untouched_base(self):
        from emulator.sd_card import SparseCard, load_raw_image, save_raw_image
        with tempfile.TemporaryDirectory() as directory:
            image = Path(directory) / 'original.img'
            image.write_bytes(b'\xa7' * 1024 + b'\x38' * 512)
            card = SparseCard()
            load_raw_image(card, image, immutable_backing=True)
            card.write(1, bytes(512))
            exported = Path(directory) / 'exported.img'
            save_raw_image(card, exported)
            self.assertEqual(exported.read_bytes(), b'\xa7' * 512 + bytes(512) + b'\x38' * 512)
            with self.assertRaises(FileExistsError):
                save_raw_image(card, exported)

    def test_portable_restore_keeps_complete_card_and_zero_overlay(self):
        from emulator.sd_card import BLANK_SDHC_PROFILE, SDCardController, load_raw_image
        board = ready(sd_card_profile=BLANK_SDHC_PROFILE)
        with tempfile.TemporaryDirectory() as directory:
            image = Path(directory) / 'input.img'
            image.write_bytes(b'\xa1' * 512 + b'\xb2' * 512)
            controller = next(device for device in board.devices if isinstance(device, SDCardController))
            load_raw_image(controller.card, image, immutable_backing=True)
            controller.card.write(0, bytes(512))
            paused = Path(directory) / 'paused'
            save(board, paused)
            image.unlink()
            restored = load(paused)
            restored_card = next(device.card for device in restored.devices if isinstance(device, SDCardController))
            self.assertEqual(restored_card.read(0, 3), bytes(512) + b'\xb2' * 512 + bytes(512))


class DeclaredUIConversionNativeTests(unittest.TestCase):
    SOURCE = 0x10000
    DESTINATION = 0x18000
    PAIR = bytes((10, 11, 129, 128))
    PIXELS = bytes((255, 11, 9, 10, 255, 12, 10, 11))

    def machine(self, ram_size=0x40000):
        from x70_native._native import Machine
        from x70_native.board import unicorn_library
        machine = Machine(str(unicorn_library()), ram_size, bytes(0x4000), False)
        machine.set_handler(self.unexpected_event)
        return machine

    def unexpected_event(self, *event):
        self.fail(('Unexpected native event', event))

    def write_register(self, machine, core, register, value):
        machine.reg_write(core, register, value.to_bytes(16, 'little'))

    def read_register(self, machine, core, register):
        return int.from_bytes(machine.reg_read(core, register), 'little')

    def fill(self, machine, source=None, destination=None):
        source = self.SOURCE if source is None else source
        destination = self.DESTINATION if destination is None else destination
        rows = (self.PAIR * 8 + b'\xcd' * 32) * 8
        machine.mem_write(0, source - 16, b'\x91' * 16 + rows + b'\x92' * 16)
        machine.mem_write(0, destination - 16, b'\xa5' * 544)
        return rows

    def prepare(self, machine):
        return machine.prepare_declared_ui_conversion(self.SOURCE, self.DESTINATION, 16, 8, 32)

    def exclusive_program(self, machine, core, address):
        machine.mem_write(core, 0x8000, struct.pack('<2I', 0xe1910f9f, 0xe1812f93))
        self.write_register(machine, core, UC_ARM_REG_CPSR, 0xd3)
        self.write_register(machine, core, UC_ARM_REG_R1, address)
        self.write_register(machine, core, UC_ARM_REG_R3, 0x12345678)
        self.assertEqual(machine.emu_start(core, 0x8000, 0, count=1), 0)

    def test_literal_conversion_reads_padded_rows_live_and_preserves_guards(self):
        machine = self.machine()
        rows = self.fill(machine)
        plan = self.prepare(machine)
        machine.mem_write(0, self.SOURCE, bytes((0, 255, 0, 255)))
        self.assertIsNone(machine.convert_declared_ui(plan, 1, 123, 456))
        expected = bytes((255, 0, 0, 178, 255, 28, 208, 255)) + self.PIXELS * 63
        self.assertEqual(machine.mem_read(0, self.DESTINATION - 16, 544),
                         b'\xa5' * 16 + expected + b'\xa5' * 16)
        self.assertEqual(machine.mem_read(0, self.SOURCE - 16, len(rows) + 32),
                         b'\x91' * 16 + bytes((0, 255, 0, 255)) + rows[4:] + b'\x92' * 16)

    def test_prepare_rejects_invalid_boundary_values_without_writing(self):
        machine = self.machine()
        self.fill(machine)
        valid = (self.SOURCE, self.DESTINATION, 16, 8, 32)
        cases = []
        for index in range(5):
            for value in (True, False, None, '16', 16.0, -1, 1 << 64):
                arguments = list(valid)
                arguments[index] = value
                cases.append(tuple(arguments))
        cases.extend((
            (self.SOURCE + 1, self.DESTINATION, 16, 8, 32),
            (self.SOURCE, self.DESTINATION + 1, 16, 8, 32),
            (self.SOURCE, self.DESTINATION, 0, 8, 32),
            (self.SOURCE, self.DESTINATION, 15, 8, 32),
            (self.SOURCE, self.DESTINATION, 16, 0, 32),
            (self.SOURCE, self.DESTINATION, 16, 7, 32),
            (self.SOURCE, self.DESTINATION, 32, 8, 16),
            (self.SOURCE, self.DESTINATION, 16, 8, 31),
            (self.SOURCE, self.DESTINATION, 8192, 8, 8192),
            (self.SOURCE, self.DESTINATION, 16, 8192, 32),
            (self.SOURCE, self.DESTINATION, 16, 8, 32768),
            (self.SOURCE, self.SOURCE + 16, 16, 8, 32),
            (self.SOURCE, self.SOURCE - 16, 16, 8, 32),
            (0x3fff0, self.DESTINATION, 16, 8, 32),
            (self.SOURCE, 0x3fff0, 16, 8, 32),
            (0x40010000, self.DESTINATION, 16, 8, 32),
            (self.SOURCE, 0x40018000, 16, 8, 32),
            (0xfffffff0, self.DESTINATION, 16, 8, 32),
        ))
        for arguments in cases:
            with self.subTest(arguments=arguments):
                with self.assertRaises(ValueError):
                    machine.prepare_declared_ui_conversion(*arguments)
                self.assertEqual(machine.mem_read(0, self.DESTINATION - 16, 544), b'\xa5' * 544)

    def test_output_limit_accepts_exact_64mib_and_refuses_larger_metadata(self):
        machine = self.machine(0x8000000)
        machine.mem_write(0, 0x3000000, b'\xa5' * 16)
        machine.prepare_declared_ui_conversion(0x1000, 0x3000000, 4096, 4096, 4096)
        with self.assertRaises(ValueError):
            machine.prepare_declared_ui_conversion(0x1000, 0x3000000, 4096, 4104, 4096)
        self.assertEqual(machine.mem_read(0, 0x3000000, 16), b'\xa5' * 16)

    def test_apply_revalidates_plan_against_target_backing(self):
        owner = self.machine(0x80000)
        plan = owner.prepare_declared_ui_conversion(0x10000, 0x50000, 16, 8, 32)
        target = self.machine()
        self.fill(target)
        with self.assertRaises(ValueError):
            target.convert_declared_ui(plan, 0, 0, 0)
        self.assertEqual(target.mem_read(0, self.DESTINATION - 16, 544), b'\xa5' * 544)
        source_plan = owner.prepare_declared_ui_conversion(0x50000, self.DESTINATION, 16, 8, 32)
        with self.assertRaises(ValueError):
            target.convert_declared_ui(source_plan, 0, 0, 0)
        self.assertEqual(target.mem_read(0, self.DESTINATION - 16, 544), b'\xa5' * 544)

    def test_apply_rejects_invalid_plan_core_and_clock_types_before_writes(self):
        machine = self.machine()
        self.fill(machine)
        plan = self.prepare(machine)
        for invalid_plan in (None, True, 'plan', object()):
            with self.subTest(plan=invalid_plan):
                with self.assertRaises(ValueError):
                    machine.convert_declared_ui(invalid_plan, 0, 123, 456)
                self.assertEqual(machine.mem_read(0, self.DESTINATION - 16, 544), b'\xa5' * 544)
        for index in range(3):
            values = (True, None, '1', 1.0, -1, 1 << 64)
            if index == 0:
                values += (2,)
            for value in values:
                with self.subTest(index=index, value=value):
                    arguments = [0, 123, 456]
                    arguments[index] = value
                    with self.assertRaises(ValueError):
                        machine.convert_declared_ui(plan, *arguments)
                    self.assertEqual(machine.mem_read(0, self.DESTINATION - 16, 544), b'\xa5' * 544)

    def test_each_core_audit_refuses_before_destination_or_reservation_effects(self):
        from x70_native._native import NativeError
        for audit_core in (0, 1):
            with self.subTest(audit_core=audit_core):
                machine = self.machine()
                self.fill(machine)
                plan = self.prepare(machine)
                self.exclusive_program(machine, 0, 0x40000000 + self.DESTINATION)
                machine.audit_begin(audit_core, 0x30000, 0x31000)
                self.write_register(machine, 0, UC_ARM_REG_SP, 0x30800)
                with self.assertRaises(NativeError):
                    machine.convert_declared_ui(plan, 0, 123, 456)
                self.assertEqual(machine.mem_read(0, self.DESTINATION - 16, 544), b'\xa5' * 544)
                self.assertEqual(machine.emu_start(0, 0x8004, 0, count=1), 0)
                self.assertEqual(self.read_register(machine, 0, UC_ARM_REG_R2), 0)

    def test_probe_records_exact_rows_then_output_with_originating_clock(self):
        machine = self.machine()
        self.fill(machine)
        plan = self.prepare(machine)
        machine.access_probe_begin([(self.SOURCE, self.SOURCE + 512),
                                    (self.DESTINATION, self.DESTINATION + 512)], limit=9)
        machine.convert_declared_ui(plan, 1, 123, 456)
        state = machine.access_probe_state()
        self.assertIsNone(state['violation'])
        observed = [(row['core'], row['address'], row['physical'], row['size'],
                     row['instructions'], row['ticks']) for row in state['records']]
        expected = [(1, self.SOURCE + 64 * row, self.SOURCE + 64 * row, 32, 123, 456)
                    for row in range(8)]
        expected.append((1, self.DESTINATION, self.DESTINATION, 512, 123, 456))
        self.assertEqual(observed, expected)
        machine.access_probe_end()
        self.assertEqual(machine.mem_read(0, self.DESTINATION, 512), self.PIXELS * 64)

    def test_probe_exhaustion_on_final_row_or_output_precedes_any_write(self):
        from x70_native._native import NativeError
        for limit in (7, 8):
            with self.subTest(limit=limit):
                machine = self.machine()
                self.fill(machine)
                plan = self.prepare(machine)
                self.exclusive_program(machine, 0, self.DESTINATION)
                machine.access_probe_begin([(self.SOURCE, self.SOURCE + 512),
                                            (self.DESTINATION, self.DESTINATION + 512)], limit=limit)
                with self.assertRaises(NativeError):
                    machine.convert_declared_ui(plan, 0, 123, 456)
                self.assertIsNotNone(machine.access_probe_state()['violation'])
                machine.access_probe_end()
                self.assertEqual(machine.mem_read(0, self.DESTINATION - 16, 544), b'\xa5' * 544)
                self.assertEqual(machine.emu_start(0, 0x8004, 0, count=1), 0)
                self.assertEqual(self.read_register(machine, 0, UC_ARM_REG_R2), 0)

    def test_physical_dma_invalidates_other_core_alias_exclusive_store(self):
        machine = self.machine()
        self.fill(machine)
        plan = self.prepare(machine)
        self.exclusive_program(machine, 1, 0x40000000 + self.DESTINATION)
        machine.convert_declared_ui(plan, 0, 123, 456)
        self.assertEqual(machine.emu_start(1, 0x8004, 0, count=1), 0)
        self.assertEqual(self.read_register(machine, 1, UC_ARM_REG_R2), 1)
        self.assertEqual(machine.mem_read(0, self.DESTINATION, 512), self.PIXELS * 64)

    def test_dma_output_replaces_already_executed_thumb_code_in_other_alias(self):
        machine = self.machine()
        self.fill(machine)
        plan = self.prepare(machine)
        machine.mem_write(0, self.SOURCE, bytes((32, 32, 128, 128)) * 8)
        machine.mem_write(0, self.DESTINATION, b'\x07\x20')
        self.write_register(machine, 1, UC_ARM_REG_CPSR, 0xf3)
        self.assertEqual(machine.emu_start(1, (0x40000000 + self.DESTINATION) | 1, 0, count=1), 0)
        self.assertEqual(self.read_register(machine, 1, UC_ARM_REG_R0), 7)
        machine.convert_declared_ui(plan, 0, 123, 456)
        self.assertEqual(machine.emu_start(1, (0x40000000 + self.DESTINATION) | 1, 0, count=1), 0)
        self.assertEqual(self.read_register(machine, 1, UC_ARM_REG_R0), 255)

    def test_tcm_straddling_destination_refuses_for_each_core_and_alias(self):
        from x70_native._native import NativeError
        for core in (0, 1):
            for alias in (0, 0x40000000):
                with self.subTest(core=core, alias=alias):
                    machine = self.machine()
                    destination = 0x21ff0
                    self.fill(machine, destination=destination)
                    plan = machine.prepare_declared_ui_conversion(self.SOURCE, destination, 16, 8, 32)
                    machine.cpr(core, 15, 0, 9, 1, 1, 0, False, alias + 0x20001)
                    register = machine.cpr(core, 15, 0, 9, 1, 1, 0, False)
                    self.assertEqual(register & 0xfffff001, alias + 0x20001)
                    with self.assertRaises(NativeError):
                        machine.convert_declared_ui(plan, 1 - core, 123, 456)
                    machine.cpr(core, 15, 0, 9, 1, 1, 0, False, register & ~1)
                    self.assertEqual(machine.mem_read(0, destination - 16, 544), b'\xa5' * 544)

    def test_board_apply_pulls_dma_tracking_before_next_public_push(self):
        board = ready()
        board.write_memory(self.SOURCE, (self.PAIR * 8 + b'\xcd' * 32) * 8)
        board.write_memory(self.DESTINATION, b'\xa5' * 512)
        plan = board.prepare_declared_ui_conversion(self.SOURCE, self.DESTINATION, 16, 8, 32)
        board.write_memory(0x8000, struct.pack('<2I', 0xe1910f9f, 0xe1812f93))
        cpu = board.cpus[0]
        for register, value in ((UC_ARM_REG_CPSR, 0xd3),
                                (UC_ARM_REG_R1, 0x40000000 + self.DESTINATION),
                                (UC_ARM_REG_R3, 0x12345678)):
            cpu.reg_write(register, value)
        cpu.emu_start(0x8000, 0, count=1)
        self.assertIsNone(board.apply_declared_ui_conversion(plan))
        self.assertEqual(board.read_memory(self.DESTINATION, 512), self.PIXELS * 64)
        board.write_memory(0x9000, b'\x17')
        cpu.emu_start(0x8004, 0, count=1)
        self.assertEqual(cpu.reg_read(UC_ARM_REG_R2), 1)
        self.assertEqual(board.read_memory(self.DESTINATION, 512), self.PIXELS * 64)
        self.assertIsNone(board.failure)

    def test_board_apply_keeps_executed_alias_code_visible_after_public_push(self):
        board = ready()
        board.write_memory(self.SOURCE, bytes((32, 32, 128, 128)) * 64)
        board.write_memory(self.DESTINATION, b'\x07\x20' + b'\xa5' * 510)
        plan = board.prepare_declared_ui_conversion(self.SOURCE, self.DESTINATION, 16, 8, 16)
        cpu = board.cpus[0]
        cpu.reg_write(UC_ARM_REG_CPSR, 0xf3)
        address = (0x40000000 + self.DESTINATION) | 1
        cpu.emu_start(address, 0, count=1)
        self.assertEqual(cpu.reg_read(UC_ARM_REG_R0), 7)
        board.apply_declared_ui_conversion(plan)
        board.write_memory(0x9000, b'\x17')
        cpu.emu_start(address, 0, count=1)
        self.assertEqual(cpu.reg_read(UC_ARM_REG_R0), 255)
        self.assertEqual(board.read_memory(self.DESTINATION, 512), bytes((255, 32, 32, 32)) * 128)
        self.assertIsNone(board.failure)

    def test_board_prepare_rejects_bool_without_destination_changes(self):
        board = ready()
        board.write_memory(self.DESTINATION, b'\xa5' * 512)
        valid = (self.SOURCE, self.DESTINATION, 16, 8, 32)
        for index in range(5):
            with self.subTest(index=index):
                arguments = list(valid)
                arguments[index] = True
                with self.assertRaises(ValueError):
                    board.prepare_declared_ui_conversion(*arguments)
                self.assertEqual(board.read_memory(self.DESTINATION, 512), b'\xa5' * 512)


if __name__ == '__main__':
    unittest.main()
