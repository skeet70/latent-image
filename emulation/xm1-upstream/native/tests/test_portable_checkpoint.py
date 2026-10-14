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
    UC_ARM_REG_R5, UC_ARM_REG_R9, UC_ARM_REG_R10)


def ready(*, firmware_profile=None, rectangle_profile=None):
    board = make_board(entry='loader', quantum=8, storage_profile='erased',
                       counter_input_profile=PulseOutputs.COUNTER_LENS_PROFILE,
                       firmware_profile=firmware_profile, rectangle_profile=rectangle_profile)
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


if __name__ == '__main__':
    unittest.main()
