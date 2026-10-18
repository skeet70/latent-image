"""Portable selected-target identity and synthetic ARM continuation contracts."""
import copy
import gc
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from emulator.boards.profile import ROOT, resolve, thaw
from native.tests.test_portable_checkpoint import ready, continuation
from x70_native.checkpoint import capture, semantic_fingerprint
from x70_native.portable_checkpoint import save, load, PortableCheckpointError


class PortableProfileOptionTests(unittest.TestCase):
    def setUp(self):
        Path('.tools').mkdir(exist_ok=True)
        self.tmp = tempfile.TemporaryDirectory(dir='.tools', prefix='profile-option-')
        self.addCleanup(self.tmp.cleanup)

    def tearDown(self):
        gc.collect()

    def test_default_omission_restores_pending_native_continuation_and_required_keys_stay_required(self):
        board = ready()
        path = Path(self.tmp.name) / 'paused'
        manifest = save(board, path)
        self.assertNotIn('firmware_profile', manifest['factory_options'])
        restored = load(path)
        self.assertEqual(semantic_fingerprint(restored), manifest['fingerprint'])
        self.assertEqual(continuation(restored), continuation(board))
        manifest['factory_options'].pop('quantum')
        (path / 'manifest.json').write_text(json.dumps(manifest))
        with self.assertRaises(PortableCheckpointError):
            load(path)

    def test_selected_xm1_capture_then_portable_save_reconstructs_identical_continuation(self):
        profile = resolve('xm1-150', profile_file='firmware/x-m1/1.50/emulator-rec.json')
        board = ready(firmware_profile=profile)
        original = semantic_fingerprint(board)
        local = capture(board)
        path = Path(self.tmp.name) / 'paused'
        manifest = save(board, path)
        descriptor = manifest['factory_options']['firmware_profile']
        self.assertEqual(descriptor['firmware_id'], 'xm1-150')
        self.assertEqual(descriptor['profile_file'], profile.profile_file.relative_to(ROOT).as_posix())
        self.assertEqual(descriptor['profile_sha256'], hashlib.sha256(profile.profile_file.read_bytes()).hexdigest())
        self.assertEqual(descriptor['part_sha256'], thaw(profile.part_sha256))
        self.assertEqual(manifest['fingerprint'], original)
        self.assertEqual(semantic_fingerprint(board), original)
        expected = continuation(board)
        self.assertIsNone(expected['failure'])
        local.restore()
        alternate = continuation(board, level=0, edges=3)
        self.assertIsNone(alternate['failure'])
        self.assertNotEqual(alternate['memory'], expected['memory'])
        local.restore()
        self.assertEqual(continuation(board), expected)
        restored = load(path)
        self.assertIsNot(restored, board)
        self.assertEqual(restored.manifest, profile.manifest)
        self.assertEqual(semantic_fingerprint(restored), original)
        self.assertEqual(continuation(restored), expected)
        roundtrip = save(load(path), Path(self.tmp.name) / 'roundtrip')
        self.assertEqual(roundtrip['factory_options']['firmware_profile'], descriptor)
        self.assertEqual(roundtrip['fingerprint'], original)
        self.assertEqual(roundtrip['identity'], manifest['identity'])
        code = ('import json,sys; from x70_native.portable_checkpoint import load; '
                'from native.tests.test_portable_checkpoint import continuation; '
                'print(json.dumps(continuation(load(sys.argv[1]),level=0,edges=3),sort_keys=True))')
        child = subprocess.run([sys.executable, '-c', code, str(path)],
                               env=dict(os.environ, PYTHONPATH='native/python:.'),
                               capture_output=True, text=True, timeout=60)
        self.assertEqual(child.returncode, 0, child.stderr)
        self.assertEqual(json.loads(child.stdout), alternate)

    def test_invalid_unresolved_unsafe_and_changed_selected_descriptors_refuse_save_and_load(self):
        profile = resolve('xm1-150', profile_file='firmware/x-m1/1.50/emulator-rec.json')
        board = ready(firmware_profile=profile)
        path = Path(self.tmp.name) / 'paused'
        manifest = copy.deepcopy(save(board, path))
        descriptor = manifest['factory_options']['firmware_profile']
        cases = [('unresolved', 'xm1-150'),
                 ('unresolved-object', {'firmware_id': 'xm1-150'}),
                 ('resolved-object', profile),
                 ('invalid-type', []),
                 ('missing-field', {key: value for key, value in descriptor.items() if key != 'resolved_sha256'})]
        for label, key, value in (
                ('unknown-firmware', 'firmware_id', 'unknown-firmware'),
                ('other-firmware', 'firmware_id', 'xe2s-111'),
                ('absolute-path', 'profile_file', str(profile.profile_file)),
                ('parent-path', 'profile_file', '../profile.json'),
                ('noncanonical-path', 'profile_file', './' + descriptor['profile_file']),
                ('missing-document', 'profile_file', '.tools/unavailable-profile.json'),
                ('changed-document', 'profile_sha256', '0' * 64),
                ('changed-parts', 'part_sha256', {}),
                ('changed-resolution', 'resolved_sha256', '0' * 64)):
            selected = copy.deepcopy(descriptor)
            selected[key] = value
            cases.append((label, selected))
        filename = path / 'manifest.json'
        for index, (label, selected) in enumerate(cases):
            with self.subTest(case=label):
                board._portable_factory_options['firmware_profile'] = selected
                rejected = Path(self.tmp.name) / ('rejected-' + str(index))
                with self.assertRaises(PortableCheckpointError):
                    save(board, rejected)
                self.assertFalse(rejected.exists())
                if selected is profile:
                    continue
                invalid = copy.deepcopy(manifest)
                invalid['factory_options']['firmware_profile'] = selected
                filename.write_text(json.dumps(invalid))
                with self.assertRaises(PortableCheckpointError):
                    load(path)
        board._portable_factory_options['firmware_profile'] = descriptor
        filename.write_text(json.dumps(manifest))
        self.assertEqual(continuation(load(path)), continuation(board))


class VirtualSDMediaTests(unittest.TestCase):
    def setUp(self):
        Path('.tools').mkdir(exist_ok=True)
        self.tmp = tempfile.TemporaryDirectory(dir='.tools', prefix='virtual-sd-')
        self.addCleanup(self.tmp.cleanup)

    def test_raw_image_factory_exposes_exact_sectors_and_provenance(self):
        from emulator.boot import make_board
        from emulator.sd_card import BLANK_SDHC_PROFILE, SDCardController
        image = Path(self.tmp.name) / 'empty-test.img'
        payload = b'\xeb' + bytes(511) + bytes(512) + b'\x55' * 512
        image.write_bytes(payload)
        board = make_board(entry='loader', sd_card_profile=BLANK_SDHC_PROFILE,
                           sd_card_present=True, sd_card_image=image)
        controller = next(d for d in board.devices if isinstance(d, SDCardController))
        self.assertEqual(controller.card.read(0, 3), payload)
        media = controller.snapshot()['image']
        self.assertEqual(media['sha256'], hashlib.sha256(payload).hexdigest())
        self.assertEqual(media['sectors'], 3)
        self.assertEqual(media['nonzero_sectors'], 2)
        self.assertNotIn('sd_card_image', board._portable_factory_options)

    def test_invalid_image_import_preserves_existing_sectors(self):
        from emulator.sd_card import SparseCard, load_raw_image
        from emulator.hardware import UnsupportedDevice
        card = SparseCard(max_dirty_sectors=2)
        card.write(7, b'\x7a' * 512)
        image = Path(self.tmp.name) / 'invalid.img'
        for payload in (b'\x11' * 512 + b'x', b'\x11' * 1024):
            image.write_bytes(payload)
            with self.assertRaises((ValueError, UnsupportedDevice)):
                load_raw_image(card, image)
            self.assertEqual(card.read(7, 1), b'\x7a' * 512)
            self.assertEqual(card.read(0, 2), bytes(1024))

    def test_media_only_restart_preserves_sectors_not_ram_or_faults(self):
        from emulator.boot import make_board, restart_with_sd_media
        from emulator.sd_card import BLANK_SDHC_PROFILE, SDCardController, MediaAction
        board = make_board(entry='loader', sd_card_profile=BLANK_SDHC_PROFILE,
                           sd_card_present=True)
        controller = next(d for d in board.devices if isinstance(d, SDCardController))
        controller.card.write(31, b'\x31' * 512)
        board.write_memory(0x30000, b'old guest RAM')
        controller.media_control(MediaAction.FAIL_NEXT_READ)
        restarted = restart_with_sd_media(board)
        replacement = next(d for d in restarted.devices if isinstance(d, SDCardController))
        self.assertEqual(replacement.card.read(31, 1), b'\x31' * 512)
        self.assertEqual(restarted.read_memory(0x30000, 13), bytes(13))
        replacement.card.write(31, bytes(512))
        controller.media_control(MediaAction.CLEAR_FAILURE)
        self.assertEqual(controller.card.read(31, 1), b'\x31' * 512)
        self.assertEqual(restarted.instructions, 0)

    def test_typed_media_controls_publish_gpio_and_fail_without_sector_changes(self):
        from emulator.boot import make_board
        from emulator.sd_card import BLANK_SDHC_PROFILE, SDCardController, MediaAction
        from emulator.hardware import UnsupportedDevice
        board = make_board(entry='loader', sd_card_profile=BLANK_SDHC_PROFILE,
                           sd_card_present=True)
        controller = next(d for d in board.devices if isinstance(d, SDCardController))
        for action, field, expected in (
                (MediaAction.PROTECT, 'write_protected', True),
                (MediaAction.REMOVE, 'present', False),
                (MediaAction.INSERT, 'present', True),
                (MediaAction.UNPROTECT, 'write_protected', False)):
            controller.media_control(action)
            self.assertEqual(controller.snapshot()[field], expected)
        controller.media_control(MediaAction.FAIL_NEXT_WRITE)
        with self.assertRaises(UnsupportedDevice):
            controller.card.write(8, b'\x88' * 512)
        self.assertEqual(controller.card.read(8, 1), bytes(512))
        controller.card.write(8, b'\x88' * 512)
        controller.media_control(MediaAction.FAIL_NEXT_READ)
        with self.assertRaises(UnsupportedDevice):
            controller.card.read(8, 1)
        self.assertEqual(controller.card.read(8, 1), b'\x88' * 512)
        with self.assertRaises(ValueError):
            controller.media_control('remove')

    def test_stock_sd_host_count_drains_on_sdma_and_pio_reads(self):
        import struct
        from emulator.boot import make_board
        from emulator.sd_card import BLANK_SDHC_PROFILE, SDCardController, MediaAction
        from emulator.hardware import UnsupportedDevice
        from unicorn.arm_const import UC_ARM_REG_PC, UC_ARM_REG_CPSR
        for word in (0x123a0037, 0x123a0036):
            with self.subTest(command_word=hex(word)):
                board = make_board(entry='loader', quantum=8,
                                   sd_card_profile=BLANK_SDHC_PROFILE, sd_card_present=True)
                sd = next(d for d in board.devices if isinstance(d, SDCardController))
                board.core_state[1]['runnable'] = False
                board.write_memory(0x30000, struct.pack('<2I', 0xe2866001, 0xeafffffd))
                board.cpus[0].reg_write(UC_ARM_REG_PC, 0x30000)
                board.cpus[0].reg_write(UC_ARM_REG_CPSR, 0xd3)
                for offset, value in ((0x68, 0), (0x150, 0), (0x190, 2 << 28),
                                      (0x538, 0x30000), (0x938, 12), (0x88, 1 << 14)):
                    board.interrupts.write(0, 0xfffb3000 + offset, 4, value)
                sd.controls.write(0, 0xfff5002c, 2, 5)
                sd.write(0, sd.BASE + 0x28, 2, 0x100)
                for offset in (0x34, 0x38):
                    sd.write(0, sd.BASE + offset, 2, 0xffff)

                def command(command_word, argument=0):
                    sd.write(0, sd.BASE + 0x30, 2, 0xffff)
                    sd.write(0, sd.BASE + 8, 2, argument & 0xffff)
                    sd.write(0, sd.BASE + 10, 2, argument >> 16)
                    sd.write(0, sd.BASE + 12, 4, command_word)
                    board.run(1000)
                    self.assertIsNone(board.failure)

                for command_word, argument in ((0, 0), (0x081a0000, 0x1aa),
                        (0x371a0000, 0), (0x29020000, 0x41100000), (0x02090000, 0),
                        (0x031a0000, 0), (0x09090000, 0x12340000), (0x071b0000, 0x12340000)):
                    command(command_word, argument)
                expected = b'\xa1' * 512 + b'\xb2' * 512
                sd.card.write(41, expected)
                sd.write(0, sd.BASE, 4, 0x40000)
                sd.write(0, sd.BASE + 4, 2, 512)
                sd.write(0, sd.BASE + 6, 2, 2)
                command(word, 41)
                if word == 0x123a0036:
                    words = []
                    for remaining in (1, 0):
                        sd.write(0, sd.BASE + 0x30, 2, 0xffff)
                        words.extend(sd.read(0, sd.BASE + 0x20, 4) for _ in range(128))
                        board.run(1000)
                        self.assertIsNone(board.failure)
                        self.assertEqual(sd.read(0, sd.BASE + 6, 2), remaining)
                    self.assertEqual(struct.pack('<256I', *words), expected)
                else:
                    self.assertEqual(board.read_memory(0x40000, 1024), expected)
                self.assertEqual(sd.read(0, sd.BASE + 6, 2), 0)
                self.assertEqual(sd.read(0, sd.BASE + 0x24, 4) & 3, 0)
                written = b'\xc3' * 512 + b'\xd4' * 512
                for write_word in (0x193a0027, 0x193a0026):
                    board.write_memory(0x40000, written)
                    sd.write(0, sd.BASE, 4, 0x40000)
                    sd.write(0, sd.BASE + 6, 2, 2)
                    sd.media_control(MediaAction.FAIL_NEXT_READ)
                    command(write_word, 53)
                    if write_word == 0x193a0026:
                        for index, remaining in enumerate((1, 0)):
                            sd.write(0, sd.BASE + 0x30, 2, 0xffff)
                            for value in struct.unpack('<128I', written[index * 512:(index + 1) * 512]):
                                sd.write(0, sd.BASE + 0x20, 4, value)
                            board.run(1000)
                            self.assertIsNone(board.failure)
                            self.assertEqual(sd.read(0, sd.BASE + 6, 2), remaining)
                    self.assertEqual(sd.read(0, sd.BASE + 6, 2), 0)
                    self.assertEqual(sd.read(0, sd.BASE + 0x24, 4) & 3, 0)
                    self.assertEqual(sd.snapshot()['failure_control'], 'read')
                    with self.assertRaises(UnsupportedDevice):
                        sd.card.read(53, 2)
                    self.assertEqual(sd.card.read(53, 2), written)


    def test_portable_media_restore_owns_sectors_after_source_image_removal(self):
        from emulator.sd_card import BLANK_SDHC_PROFILE, SDCardController, MediaAction
        from emulator.hardware import UnsupportedDevice
        image = Path(self.tmp.name) / 'portable-input.img'
        payload = b'\xa5' * 512 + b'\xb6' * 512 + bytes(512)
        image.write_bytes(payload)
        board = ready(sd_card_profile=BLANK_SDHC_PROFILE, sd_card_image=image)
        sd = next(d for d in board.devices if isinstance(d, SDCardController))
        sd.card.write(0, bytes(512))
        sd.card.write(2, b'\xc7' * 512)
        sd.media_control(MediaAction.FAIL_NEXT_WRITE)
        snapshot = Path(self.tmp.name) / 'portable-media'
        save(board, snapshot)
        image.unlink()
        restored = load(snapshot)
        card = next(d for d in restored.devices if isinstance(d, SDCardController)).card
        self.assertEqual(card.read(0, 3), bytes(512) + b'\xb6' * 512 + b'\xc7' * 512)
        with self.assertRaises(UnsupportedDevice):
            card.write(2, b'\xd8' * 512)
        self.assertEqual(card.read(2, 1), b'\xc7' * 512)
        card.write(2, b'\xd8' * 512)
        self.assertEqual(card.read(2, 1), b'\xd8' * 512)

    def test_portable_supplied_raw_image_rejects_before_external_file_access(self):
        import builtins
        import io
        from unittest.mock import patch
        from emulator.sd_card import BLANK_SDHC_PROFILE
        snapshot = Path(self.tmp.name) / 'malicious-media-option'
        manifest = save(ready(), snapshot)
        forbidden = Path(self.tmp.name) / 'never-open.img'
        manifest['factory_options'].update(sd_card_profile=BLANK_SDHC_PROFILE,
                                           sd_card_present=True, sd_card_image=str(forbidden))
        (snapshot / 'manifest.json').write_text(json.dumps(manifest))

        def guarded(real):
            def external_open(path, *args, **kwargs):
                if isinstance(path, (str, os.PathLike)) and Path(path) == forbidden:
                    raise AssertionError('Portable load attempted host raw-image access')
                return real(path, *args, **kwargs)
            return external_open

        with patch('builtins.open', guarded(builtins.open)), \
                patch('io.open', guarded(io.open)), patch('os.open', guarded(os.open)):
            with self.assertRaises(PortableCheckpointError):
                load(snapshot)

    def test_raw_image_admission_rejects_nonregular_symlink_and_oversize_files(self):
        from emulator.sd_card import SparseCard, load_raw_image
        card = SparseCard()
        fifo = Path(self.tmp.name) / 'stream.img'
        os.mkfifo(fifo)
        with self.assertRaises(ValueError):
            load_raw_image(card, fifo)
        oversized = Path(self.tmp.name) / 'oversize.img'
        with oversized.open('wb') as image:
            image.truncate(card.sectors * 512 + 512)
        with self.assertRaises(ValueError):
            load_raw_image(card, oversized)
        link = Path(self.tmp.name) / 'linked.img'
        link.symlink_to(oversized)
        with self.assertRaises(OSError):
            load_raw_image(card, link)
        self.assertEqual(card.read(0, 1), bytes(512))


class StoppedArchitectureDiagnosticTests(unittest.TestCase):
    def setUp(self):
        from tools.probe_setup_menus import MenuSession
        Path('.tools').mkdir(exist_ok=True)
        self.tmp = tempfile.TemporaryDirectory(dir='.tools', prefix='stopped-architecture-')
        self.addCleanup(self.tmp.cleanup)
        self.board = ready()
        self.session = MenuSession(
            self.board, Path(self.tmp.name) / 'viewer',
            firmware={'filename': 'synthetic-test.DAT', 'model': 'X-M1', 'version': '1.50',
                      'sha256': self.board.manifest['source_sha256'], 'modified': False})

    def tearDown(self):
        gc.collect()

    def stop_with_frame(self, stack=0x44000, frame=0x44080):
        import struct
        from x70_native import registers
        self.payload = bytes(index & 255 for index in range(640))
        self.board.write_memory(0x44000, self.payload)
        self.board.write_memory(0x30000, struct.pack('<4I', 0xe3a00000, 0xe5810000,
                                                   0xe2866001, 0xeafffffd))
        for register, value in ((registers.PC, 0x30000), (registers.CPSR, 0xd3),
                                (registers.R1, 0xfff47ffc), (registers.SP, stack),
                                (registers.R11, frame), (registers.LR, 0x6ded1c)):
            self.board.cpus[0].reg_write(register, value)
        self.board.run(32)
        self.assertIsNotNone(self.board.failure)

    def test_failed_guest_registers_and_bounded_dram_observation_preserve_all_state(self):
        self.stop_with_frame()
        before = semantic_fingerprint(self.board)
        observed = self.session.stopped_architecture()
        self.assertTrue(observed['available'])
        self.assertEqual(observed['cpu'], 0)
        self.assertEqual(observed['registers']['lr'], 0x6ded1c)
        self.assertEqual(observed['registers']['sp'], 0x44000)
        self.assertEqual(observed['registers']['r11'], 0x44080)
        self.assertEqual(observed['registers']['pc'], 0x30004)
        for name, offset, size in (('stack', 0, 512), ('frame', 0x60, 256)):
            window = observed['windows'][name]
            self.assertTrue(window['available'])
            self.assertEqual(window['address'], 0x44000 + offset)
            self.assertEqual(window['bytes'], size)
            self.assertEqual(window['little_endian_words'],
                             [int.from_bytes(self.payload[index:index + 4], 'little')
                              for index in range(offset, offset + size, 4)])
        self.assertEqual(semantic_fingerprint(self.board), before)
        self.assertEqual(self.session.stopped_architecture(), observed)
        self.assertEqual(semantic_fingerprint(self.board), before)
        self.assertEqual(self.session.sample(pixels=False)['stopped_architecture'], observed)

    def test_mmio_and_crossing_dram_windows_report_unavailable_without_state_change(self):
        self.stop_with_frame(stack=0xfff47000, frame=0x40000000 + len(self.board.ram) - 32)
        before = semantic_fingerprint(self.board)
        observed = self.session.stopped_architecture()
        self.assertTrue(observed['available'])
        for name in ('stack', 'frame'):
            self.assertFalse(observed['windows'][name]['available'])
            self.assertEqual(observed['windows'][name]['reason'], 'outside_dram')
            self.assertNotIn('little_endian_words', observed['windows'][name])
        self.assertEqual(semantic_fingerprint(self.board), before)
        from x70_native import registers
        self.board.cpus[0].reg_write(registers.SP, 0x40000000 + len(self.board.ram) - 256)
        self.board.cpus[0].reg_write(registers.R11, 0x44080)
        before = semantic_fingerprint(self.board)
        observed = self.session.stopped_architecture()
        self.assertFalse(observed['windows']['stack']['available'])
        self.assertEqual(observed['windows']['stack']['reason'], 'outside_dram')
        self.assertTrue(observed['windows']['frame']['available'])
        self.assertEqual(semantic_fingerprint(self.board), before)

    def test_successful_paused_guest_has_explicit_unavailable_observation(self):
        before = semantic_fingerprint(self.board)
        observed = self.session.stopped_architecture()
        self.assertFalse(observed['available'])
        self.assertEqual(observed['reason'], 'not_failed')
        self.assertNotIn('registers', observed)
        self.assertEqual(semantic_fingerprint(self.board), before)

    def test_current_irq_bank_and_dram_alias_windows_preserve_all_state(self):
        from x70_native import registers
        self.stop_with_frame(stack=0x40044000, frame=0x40044080)
        cpu = self.board.cpus[0]
        for register, value in ((registers.CPSR, 0xd2), (registers.SP, 0x40044000),
                                (registers.LR, 0x13572468)):
            cpu.reg_write(register, value)
        before = semantic_fingerprint(self.board)
        observed = self.session.stopped_architecture()
        self.assertTrue(observed['available'])
        self.assertEqual(observed['registers']['cpsr'], 0xd2)
        self.assertEqual(observed['registers']['sp'], 0x40044000)
        self.assertEqual(observed['registers']['lr'], 0x13572468)
        for name, offset, size in (('stack', 0, 512), ('frame', 0x60, 256)):
            window = observed['windows'][name]
            self.assertTrue(window['available'])
            self.assertEqual(window['address'], 0x40044000 + offset)
            self.assertEqual(window['physical_address'], 0x44000 + offset)
            self.assertEqual(window['bytes'], size)
            self.assertEqual(window['little_endian_words'],
                             [int.from_bytes(self.payload[index:index + 4], 'little')
                              for index in range(offset, offset + size, 4)])
        self.assertEqual(semantic_fingerprint(self.board), before)


class DeclaredUIConversionDeviceTests(unittest.TestCase):
    PROFILE = 'declared-ui-cpu-fallback'
    PROVENANCE = 'xm1-150-declared-ui-yycbcr-to-abgr-cpu-fallback-unmeasured'
    BASE = 0xfff47000
    SOURCE = 0x60000
    DESTINATION = 0x70000
    IRQ_BASES = (0xfffb3000, 0xfffb4000)
    PAIR = bytes((16, 235, 128, 128))
    PIXELS = bytes((255, 16, 16, 16, 255, 235, 235, 235))
    LIVE_PAIR = bytes((10, 200, 129, 129))
    LIVE_PIXELS = bytes((255, 11, 8, 11, 255, 201, 198, 201))

    def setUp(self):
        Path('.tools').mkdir(exist_ok=True)
        self.tmp = tempfile.TemporaryDirectory(dir='.tools', prefix='declared-ui-')
        self.addCleanup(self.tmp.cleanup)
        self.profile = resolve('xm1-150', profile_file='firmware/x-m1/1.50/emulator-rec.json')

    def tearDown(self):
        gc.collect()

    def device(self, board):
        devices = [device for device in board.devices if device.contains(self.BASE, 4)]
        self.assertEqual(len(devices), 1)
        return devices[0]

    def loop(self, board):
        import struct
        from x70_native import registers
        board.write_memory(0x30000, struct.pack('<2I', 0xe2866001, 0xeafffffd))
        for register, value in ((registers.PC, 0x30000), (registers.CPSR, 0xd3),
                                (registers.R6, 0)):
            board.cpus[0].reg_write(register, value)

    def board(self):
        board = ready(firmware_profile=self.profile, ui_conversion_profile=self.PROFILE)
        self.loop(board)
        for base in self.IRQ_BASES:
            board.interrupts.write(0, base + 0x90c, 4, 13)
            board.interrupts.write(0, base + 0x50c, 4, 0x30000)
        board.interrupts.write(0, self.IRQ_BASES[0] + 0x88, 4, 8)
        self.seed(board)
        return board

    def seed(self, board, pair=None):
        pair = self.PAIR if pair is None else pair
        source = b'\xa6' * 16 + (pair * 8 + b'\x5a' * 32) * 8 + b'\xa6' * 16
        board.write_memory(self.SOURCE - 16, source)
        board.write_memory(self.DESTINATION - 16, b'\xc7' * 16 + b'\xcd' * 512 + b'\xc7' * 16)

    def words(self):
        return ((0x08, 1), (0x10, self.SOURCE), (0x14, 32), (0x18, 16 | (8 << 16)),
                (0x20, self.DESTINATION), (0x24, 16), (0x28, 16 | (8 << 16)),
                (0x2c, 0), (0x70, 0), (0x7c, 0x1301), (0x80, 0xff))

    def program(self, board, *, omit=None, replace=None):
        device = self.device(board)
        device.write(0, self.BASE, 4, 0)
        for offset, value in self.words():
            if offset != omit:
                device.write(0, self.BASE + offset, 4, (replace or {}).get(offset, value))
        return device

    def advance(self, board, count=8):
        board.run(count + 16, pause_after=board.instructions + count)
        self.assertIsNone(board.failure)

    def pending(self, board):
        return tuple(int(bank['pending'], 16) & (1 << 67)
                     for bank in board.interrupts.snapshot()['banks'])

    def assert_unpublished(self, board):
        from emulator.hardware import UnsupportedDevice
        self.assertEqual(board.read_memory(self.DESTINATION, 512), b'\xcd' * 512)
        with self.assertRaises(UnsupportedDevice):
            self.device(board).read(0, self.BASE + 0x74, 4)

    def assert_output(self, board, *, pair=None, pixels=None):
        pair = self.PAIR if pair is None else pair
        pixels = self.PIXELS if pixels is None else pixels
        self.assertEqual(board.read_memory(self.DESTINATION, 512), pixels * 64)
        self.assertEqual(board.read_memory(self.DESTINATION - 16, 16), b'\xc7' * 16)
        self.assertEqual(board.read_memory(self.DESTINATION + 512, 16), b'\xc7' * 16)
        self.assertEqual(board.read_memory(self.SOURCE - 16, 544),
                         b'\xa6' * 16 + (pair * 8 + b'\x5a' * 32) * 8 + b'\xa6' * 16)

    def cleanup(self, board):
        from emulator.hardware import UnsupportedDevice
        device = self.device(board)
        device.write(0, self.BASE + 0x78, 4, 1)
        with self.assertRaises(UnsupportedDevice):
            device.read(0, self.BASE + 0x74, 4)
        with self.assertRaises(UnsupportedDevice):
            device.write(0, self.BASE + 0x78, 4, 1)
        with self.assertRaises(UnsupportedDevice):
            device.write(0, self.BASE, 4, 0)
        device.write(0, self.BASE + 0x7c, 4, 0)
        device.write(0, self.BASE, 4, 0)
        device.write(0, self.BASE, 4, 0)

    def test_factory_selection_provenance_and_portable_profile_roundtrip(self):
        strict = ready(firmware_profile=self.profile)
        baseline = save(strict, Path(self.tmp.name) / 'strict')
        self.assertNotIn('ui_conversion_profile', baseline['factory_options'])
        del strict
        gc.collect()
        board = self.board()
        self.assertEqual(self.device(board).snapshot()['provenance']['profile'], self.PROVENANCE)
        path = Path(self.tmp.name) / 'idle'
        manifest = save(board, path)
        self.assertEqual(manifest['factory_options']['ui_conversion_profile'], self.PROFILE)
        restored = load(path)
        self.assertEqual(self.device(restored).snapshot()['provenance']['profile'], self.PROVENANCE)
        self.assertEqual(semantic_fingerprint(restored), manifest['fingerprint'])
        device = self.program(restored)
        device.write(0, self.BASE + 4, 4, 3)
        self.advance(restored)
        self.assert_output(restored)
        self.assertEqual(device.read(0, self.BASE + 0x74, 4), 1)
        self.cleanup(restored)
        roundtrip = save(restored, Path(self.tmp.name) / 'cleaned')
        self.assertEqual(roundtrip['factory_options']['ui_conversion_profile'], self.PROFILE)

    def test_unknown_factory_selectors_refuse_at_the_boundary(self):
        from emulator.boot import make_board
        for value in ('unknown', 'strict', self.PROVENANCE, True, 1, [], {}):
            with self.subTest(profile=value):
                with self.assertRaises(ValueError):
                    make_board(entry='loader', firmware_profile=self.profile, ui_conversion_profile=value)

    def test_omitted_and_explicit_none_keep_strict_guest_mmio_refusal(self):
        import struct
        from emulator.boot import make_board
        from x70_native import registers
        for options in ({}, {'ui_conversion_profile': None}):
            with self.subTest(options=options):
                board = make_board(entry='loader', quantum=8, firmware_profile=self.profile, **options)
                board.core_state[1]['runnable'] = False
                board.write_memory(0x30000, struct.pack('<4I', 0xe3a01000, 0xe5801000,
                                                       0xe2866001, 0xeafffffd))
                for register, value in ((registers.PC, 0x30000), (registers.CPSR, 0xd3),
                                        (registers.R0, self.BASE)):
                    board.cpus[0].reg_write(register, value)
                board.run(32)
                self.assertIsNotNone(board.failure)
                self.assertEqual(board.failure['operation'], 'write')
                self.assertEqual(board.failure['address'], hex(self.BASE))
                self.assertEqual(board.failure['pc'], hex(0x30004))
                self.assertFalse(any(device.contains(self.BASE, 4) for device in board.devices))
                del board
                gc.collect()

    def test_command3_publishes_before_status_and_one_irq_then_allows_an_independent_job(self):
        from emulator.hardware import UnsupportedDevice
        board = self.board()
        device = self.program(board)
        now = board.virtual_ticks
        device.write(0, self.BASE + 4, 4, 3)
        self.assertEqual(device.next_event_tick(now), now + 1)
        self.assert_unpublished(board)
        self.assertEqual(self.pending(board), (0, 0))
        board.run(1)
        self.assertIsNone(board.failure)
        self.assertEqual(board.virtual_ticks, now + 1)
        self.assert_output(board)
        self.assertEqual(device.read(0, self.BASE + 0x74, 4), 1)
        self.assertEqual(device.read(0, self.BASE + 0x74, 4), 1)
        self.assertEqual(self.pending(board), (1 << 67, 0))
        self.assertEqual(board.interrupts.pending_line(0), 67)
        self.assertIsNone(board.interrupts.snapshot()['banks'][0]['injected'])
        for offset, value in ((0, 0), (4, 3), (0x7c, 0)):
            with self.assertRaises(UnsupportedDevice):
                device.write(0, self.BASE + offset, 4, value)
        self.cleanup(board)
        self.assertEqual(self.pending(board), (1 << 67, 0))
        board.interrupts.write(0, self.IRQ_BASES[0] + 0xe8, 4, 8)
        self.advance(board)
        self.assertEqual(self.pending(board), (0, 0))
        self.seed(board, self.LIVE_PAIR)
        device = self.program(board)
        device.write(0, self.BASE + 4, 4, 3)
        self.advance(board)
        self.assert_output(board, pair=self.LIVE_PAIR, pixels=self.LIVE_PIXELS)
        self.assertEqual(device.read(0, self.BASE + 0x74, 4), 1)
        self.assertEqual(self.pending(board), (1 << 67, 0))
        requests = [event for event in board.snapshot()['events']
                    if event['name'] == 'Peripheral IRQ request' and event.get('line') == 67]
        self.assertEqual([event['cpu_id'] for event in requests], [0, 0])
        self.cleanup(board)

    def test_arm_consumer_programs_reads_acknowledges_and_stops_without_host_callbacks(self):
        import struct
        from x70_native import registers
        board = self.board()
        words, literals = [], []

        def write(offset, value):
            literals.append((len(words), value))
            words.extend((0, 0xe5801000 | offset))

        for offset, value in ((0, 0), *self.words(), (4, 3)):
            write(offset, value)
        # MOV r7,#32; SUBS r7,r7,#1; BNE SUBS.
        words.extend((0xe3a07020, 0xe2577001, 0x1afffffd))
        # Sample the first and last output words before completion status.
        words.extend((0xe5932000, 0xe59341fc, 0xe5907074,
                      0xe5852000, 0xe5854004, 0xe5857008))
        write(0x78, 1)
        write(0x7c, 0)
        write(0, 0)
        write(0, 0)
        words.extend((0xe2866001, 0xe585600c, 0xeafffffc))
        for index, value in literals:
            displacement = 4 * (len(words) - index) - 8
            words[index] = 0xe59f1000 | displacement
            words.append(value)
        board.write_memory(0x30000, struct.pack('<' + 'I' * len(words), *words))
        for register, value in ((registers.PC, 0x30000), (registers.R0, self.BASE),
                                (registers.R3, self.DESTINATION), (registers.R5, 0x80000)):
            board.cpus[0].reg_write(register, value)
        self.advance(board, 192)
        self.assert_output(board)
        observed = struct.unpack('<4I', board.read_memory(0x80000, 16))
        self.assertEqual(observed[:3], (0x101010ff, 0xebebebff, 1))
        self.assertGreater(observed[3], 0)
        self.assertEqual(self.pending(board), (1 << 67, 0))
        device = self.device(board)
        device.write(0, self.BASE, 4, 0)
        board.interrupts.write(0, self.IRQ_BASES[0] + 0xe8, 4, 8)
        self.assertEqual(self.pending(board), (0, 0))

    def test_deadline_samples_live_source_after_start(self):
        board = self.board()
        device = self.program(board)
        device.write(0, self.BASE + 4, 4, 3)
        self.assert_unpublished(board)
        board.write_memory(self.SOURCE, (self.LIVE_PAIR * 8 + b'\x5a' * 32) * 8)
        self.assertEqual(self.pending(board), (0, 0))
        board.run(1)
        self.assertIsNone(board.failure)
        self.assert_output(board, pair=self.LIVE_PAIR, pixels=self.LIVE_PIXELS)
        self.assertEqual(device.read(0, self.BASE + 0x74, 4), 1)
        self.assertEqual(self.pending(board), (1 << 67, 0))

    def test_active_local_and_portable_capture_refuse_without_changing_completion(self):
        from emulator.checkpoint import CheckpointError
        board = self.board()
        device = self.program(board)
        device.write(0, self.BASE + 4, 4, 3)
        before = semantic_fingerprint(board)
        with self.assertRaises(CheckpointError):
            capture(board)
        rejected = Path(self.tmp.name) / 'active'
        with self.assertRaises(CheckpointError):
            save(board, rejected)
        self.assertFalse(rejected.exists())
        self.assertEqual(semantic_fingerprint(board), before)
        self.assert_unpublished(board)
        self.advance(board)
        self.assert_output(board)
        self.assertEqual(device.read(0, self.BASE + 0x74, 4), 1)
        self.assertEqual(self.pending(board), (1 << 67, 0))

    def test_completed_portable_restore_preserves_output_irq_latch_and_cleanup(self):
        board = self.board()
        device = self.program(board)
        device.write(0, self.BASE + 4, 4, 3)
        self.advance(board)
        path = Path(self.tmp.name) / 'completed'
        manifest = save(board, path)
        restored = load(path)
        self.assertEqual(semantic_fingerprint(restored), manifest['fingerprint'])
        self.assert_output(restored)
        self.assertEqual(self.device(restored).snapshot()['provenance']['profile'], self.PROVENANCE)
        self.assertEqual(self.device(restored).read(0, self.BASE + 0x74, 4), 1)
        self.assertEqual(self.pending(restored), (1 << 67, 0))
        self.cleanup(restored)
        self.assertEqual(self.pending(restored), (1 << 67, 0))
        restored.interrupts.write(0, self.IRQ_BASES[0] + 0xe8, 4, 8)
        self.advance(restored)
        self.assertEqual(self.pending(restored), (0, 0))
        cleaned = save(restored, Path(self.tmp.name) / 'completed-cleaned')
        self.assertEqual(cleaned['factory_options']['ui_conversion_profile'], self.PROFILE)

    def test_unobserved_words_widths_reads_and_acknowledgements_refuse(self):
        from emulator.hardware import UnsupportedDevice
        board = self.board()
        idle = capture(board)
        for offset, size, value in ((0x04, 4, 1), (0x40, 4, 0), (0x64, 4, 0),
                                    (0x7c, 4, 1), (0x80, 4, 0), (0x0c, 4, 0),
                                    (0x10, 2, self.SOURCE), (0x11, 4, self.SOURCE),
                                    (0x78, 4, 1)):
            with self.subTest(offset=offset, size=size, value=value):
                idle.restore()
                with self.assertRaises(UnsupportedDevice):
                    self.device(board).write(0, self.BASE + offset, size, value)
                self.assert_unpublished(board)
                self.assertEqual(self.pending(board), (0, 0))
        for offset in (0, 0x10, 0x74, 0x78, 0x7c):
            with self.subTest(read=offset):
                idle.restore()
                with self.assertRaises(UnsupportedDevice):
                    self.device(board).read(0, self.BASE + offset, 4)
                self.assert_unpublished(board)
        idle.restore()
        self.program(board)
        with self.assertRaises(UnsupportedDevice):
            self.device(board).read(0, self.BASE + 0x74, 4)

    def test_every_required_word_belongs_to_the_current_programming_generation(self):
        from emulator.hardware import UnsupportedDevice
        board = self.board()
        idle = capture(board)
        for missing, _ in self.words():
            with self.subTest(missing=missing):
                idle.restore()
                device = self.program(board, omit=missing)
                with self.assertRaises(UnsupportedDevice):
                    device.write(0, self.BASE + 4, 4, 3)
                self.assert_unpublished(board)
                self.assertEqual(self.pending(board), (0, 0))
        idle.restore()
        device = self.program(board)
        device.write(0, self.BASE + 4, 4, 3)
        self.advance(board)
        self.cleanup(board)
        board.interrupts.write(0, self.IRQ_BASES[0] + 0xe8, 4, 8)
        with self.assertRaises(UnsupportedDevice):
            device.write(0, self.BASE + 4, 4, 3)
        self.assert_output(board)
        self.assertEqual(self.pending(board), (0, 0))

    def test_unsupported_layouts_refuse_before_destination_or_completion_publication(self):
        from emulator.hardware import UnsupportedDevice
        board = self.board()
        idle = capture(board)
        cases = ({0x08: 0}, {0x10: self.SOURCE + 1}, {0x20: self.DESTINATION + 1},
                 {0x14: 0}, {0x14: 16, 0x18: 32 | (8 << 16), 0x28: 32 | (8 << 16)},
                 {0x14: 32768}, {0x18: 18 | (8 << 16)}, {0x18: 16 | (7 << 16)},
                 {0x18: 8192 | (8 << 16)}, {0x18: 0}, {0x24: 32},
                 {0x28: 16 | (16 << 16)}, {0x2c: 1}, {0x70: 1},
                 {0x20: self.SOURCE}, {0x10: len(board.ram) - 16},
                 {0x20: len(board.ram) - 16})
        for replacement in cases:
            with self.subTest(layout=replacement):
                idle.restore()
                with self.assertRaises(UnsupportedDevice):
                    device = self.program(board, replace=replacement)
                    device.write(0, self.BASE + 4, 4, 3)
                self.assert_unpublished(board)
                self.assertEqual(self.pending(board), (0, 0))

    def test_active_reset_restart_mask_and_reprogramming_refuse_without_aborting_job(self):
        from emulator.hardware import UnsupportedDevice
        board = self.board()
        device = self.program(board)
        device.write(0, self.BASE + 4, 4, 3)
        for offset, value in ((0, 0), (4, 3), (0x7c, 0), (0x10, self.SOURCE + 16),
                              (0x18, 16 | (16 << 16)), (0x78, 1)):
            with self.subTest(offset=offset):
                with self.assertRaises(UnsupportedDevice):
                    device.write(0, self.BASE + offset, 4, value)
                self.assert_unpublished(board)
                self.assertEqual(self.pending(board), (0, 0))
        self.advance(board)
        self.assert_output(board)
        self.assertEqual(device.read(0, self.BASE + 0x74, 4), 1)
        self.assertEqual(self.pending(board), (1 << 67, 0))

    def test_no_or_ambiguous_route_refuses_start(self):
        from emulator.hardware import UnsupportedDevice
        board = self.board()
        idle = capture(board)
        for selected in ((), (0, 1)):
            with self.subTest(route=selected):
                idle.restore()
                for bank, base in enumerate(self.IRQ_BASES):
                    board.interrupts.write(0, base + 0xa8, 4, 8)
                    if bank in selected:
                        board.interrupts.write(0, base + 0x88, 4, 8)
                device = self.program(board)
                with self.assertRaises(UnsupportedDevice):
                    device.write(0, self.BASE + 4, 4, 3)
                self.assert_unpublished(board)
                self.assertEqual(self.pending(board), (0, 0))

    def test_preexisting_pending_injected_and_acknowledged_irq67_refuse_start_in_either_bank(self):
        from emulator.hardware import UnsupportedDevice
        from x70_native import registers
        board = self.board()
        idle = capture(board)
        for bank, base in enumerate(self.IRQ_BASES):
            for phase in ('pending', 'injected', 'acknowledged'):
                with self.subTest(bank=bank, phase=phase):
                    idle.restore()
                    board.interrupts.write(0, base + 0x88, 4, 8)
                    board.queue_irq(bank, 67)
                    if phase != 'pending':
                        cpu = board.cpus[bank]
                        cpu.reg_write(registers.CPSR, 0x53)
                        self.assertTrue(board.interrupts.inject(bank, cpu))
                        cpu.reg_write(registers.CPSR, 0xd3)
                        if phase == 'acknowledged':
                            self.assertEqual(board.interrupts.read(bank, base + 0x200, 4), 0x30000)
                        board.interrupts.write(0, base + 0xe8, 4, 8)
                    if bank == 1:
                        board.interrupts.write(0, base + 0xa8, 4, 8)
                    before = board.interrupts.snapshot()['banks']
                    device = self.program(board)
                    with self.assertRaises(UnsupportedDevice):
                        device.write(0, self.BASE + 4, 4, 3)
                    self.assert_unpublished(board)
                    self.assertEqual(board.interrupts.snapshot()['banks'], before)

    def test_changed_route_and_new_irq_conflicts_refuse_at_deadline(self):
        from emulator.hardware import UnsupportedDevice
        board = self.board()
        idle = capture(board)
        changes = ('disabled', 'moved', 'ambiguous', 'pending-bank0', 'pending-bank1')
        for change in changes:
            with self.subTest(change=change):
                idle.restore()
                device = self.program(board)
                device.write(0, self.BASE + 4, 4, 3)
                if change in ('disabled', 'moved'):
                    board.interrupts.write(0, self.IRQ_BASES[0] + 0xa8, 4, 8)
                if change in ('moved', 'ambiguous'):
                    board.interrupts.write(0, self.IRQ_BASES[1] + 0x88, 4, 8)
                if change.startswith('pending-'):
                    board.queue_irq(0 if change == 'pending-bank0' else 1, 67)
                before = self.pending(board)
                with self.assertRaises(UnsupportedDevice):
                    board.run(1)
                self.assert_unpublished(board)
                self.assertEqual(self.pending(board), before)

    def test_boot_session_selection_survives_public_reset(self):
        from x70_native.boot import NativeBootSession
        for index, selection in enumerate((None, self.PROFILE)):
            with self.subTest(selection=selection):
                session = NativeBootSession(
                    Path(self.tmp.name) / f'boot-session-{index}', entry='loader', quantum=8,
                    firmware_profile=self.profile, ui_conversion_profile=selection)
                self.assertEqual(session.board.instructions, 0)
                before = [device.snapshot()['provenance']['profile']
                          for device in session.board.devices if device.contains(self.BASE, 4)]
                self.assertEqual(before, [] if selection is None else [self.PROVENANCE])
                session.run({'reset': True, 'limit': 1})
                after = [device.snapshot()['provenance']['profile']
                         for device in session.board.devices if device.contains(self.BASE, 4)]
                self.assertEqual(after, before)


class RenderSettleSession:
    def __init__(self, before, after):
        from types import SimpleNamespace
        self.board = SimpleNamespace(instructions=0, virtual_ticks=0, failure=None)
        self.execution_error = None
        self.inputs = []
        self.before = before
        self.after = after
        self.state = {'display': {'selected_instance': 1, 'last_frame': {
            'instance': 1, 'width': 720, 'height': 480, 'graphics': [{
                'source': '0x228c800', 'captured_bytes': 1382400,
                'sha256': '5588ecfbacad1af9bfb283bf5fcf7e8afe99e1f8d1ef5025000205e4ff65ef2e',
                'width': 720, 'height': 480, 'origin': [0, 0], 'alpha_field': 128,
                'layer': 1, 'channel': 0}]}}}

    def probe(self):
        return copy.deepcopy(self.after if self.inputs else self.before)

    def key(self, options):
        self.inputs.append(options['key'])
        duration = options['hold_instructions'] + options['release_instructions']
        self.board.instructions += duration
        self.board.virtual_ticks += duration
        return self.state

    def run(self, options):
        self.board.instructions += options['limit']
        self.board.virtual_ticks += options['limit']
        return self.state


class MenuRenderSettleTests(unittest.TestCase):
    @staticmethod
    def paused():
        return {
            'ready': True, 'copied_command': 0x8000, 'packet_token': 0,
            'sender_command': 0x8000, 'sender_token': 44, 'consumer_token': 44,
            'staged_render_command': 0x4047, 'staged_render_token': 44,
            'initialized': 1, 'display_suppression': 0, 'batch_busy': 0,
            'batch_callback': 0, 'commands_dirty': 0, 'display_pending': 0,
            'ui_pending': 0, 'ui_busy': 0,
            'tasks': {role: {'status': 0x12}
                      for role in ('main_ui', 'uilib', 'graphic', 'vglib')},
            'graphics': {
                'busy': False, 'command_pending': False, 'status': 0,
                'started': 367, 'completed': 367, 'acknowledgements': 367,
                'bytes_completed': 135558976, 'last_copy': None},
            'publications': []}

    @classmethod
    def published(cls):
        probe = cls.paused()
        probe.update(sender_token=45, consumer_token=45, staged_render_token=45)
        probe['graphics'].update(
            started=368, completed=368, acknowledgements=368,
            bytes_completed=136941376,
            last_copy={'destination': '0x228c800', 'bytes': 1382400,
                       'sha256': '5588ecfbacad1af9bfb283bf5fcf7e8afe99e1f8d1ef5025000205e4ff65ef2e',
                       'completed_job': 368})
        return probe

    @classmethod
    def settle(cls, before, after):
        return cls.settle_session(RenderSettleSession(before, after))

    @staticmethod
    def settle_session(session, *, budget=0, probe=None):
        from tools.menu_render_settle import MenuRenderSettle
        state = MenuRenderSettle(session, probe=probe or session.probe).key({
            'key': 'ok', 'hold_instructions': 1, 'release_instructions': 1,
            'max_settle_instructions': budget, 'chunk_instructions': 1})
        return session, state['navigation']['render_settle']

    def test_paused_probe_exposes_retained_render_stage(self):
        from tools.menu_render_settle import read_render_state
        board = ready()
        board.write_memory(0x015e46e8, b'\x47\x40\x2c\xa5')
        probe = read_render_state(board)
        self.assertEqual(probe['staged_render_command'], 0x4047)
        self.assertEqual(probe['staged_render_token'], 44)

    def test_auxiliary_packet_preserves_acknowledged_render_before_and_after_key(self):
        session, result = self.settle(self.paused(), self.published())
        self.assertEqual(session.inputs, ['ok'])
        self.assertEqual(result['key_instructions'], 2)
        self.assertEqual(result['status'], 'render-batch-published-candidate')
        self.assertEqual(result['requested_packet_token'], 45)
        self.assertEqual(result['publication']['destination'], '0x228c800')

    def test_original_tokenized_render_does_not_require_auxiliary_staging(self):
        before, after = self.paused(), self.published()
        for probe in (before, after):
            probe.update(copied_command=0x4047, sender_command=0x4047,
                         packet_token=probe['sender_token'])
            probe.pop('staged_render_command')
            probe.pop('staged_render_token')
        session, result = self.settle(before, after)
        self.assertEqual(session.inputs, ['ok'])
        self.assertEqual(result['status'], 'render-batch-published-candidate')

    def test_auxiliary_packet_refuses_missing_stale_or_nonrender_provenance(self):
        cases = (
            {'staged_render_command': 0},
            {'staged_render_command': 0x5001},
            {'staged_render_token': 43},
            {'staged_render_token': 0},
            {'consumer_token': 43},
            {'sender_token': 0, 'consumer_token': 0, 'staged_render_token': 0},
            {'packet_token': 1},
            {'copied_command': 0x8001, 'sender_command': 0x8001},
            {'sender_command': 0x8001})
        for changes in cases:
            with self.subTest(changes=changes):
                before = self.paused()
                before.update(changes)
                session, result = self.settle(before, self.published())
                self.assertEqual(session.inputs, [])
                self.assertEqual(result['key_instructions'], 0)
                self.assertEqual(result['status'], 'prior-render-pending')
        before = self.paused()
        before.pop('staged_render_command')
        before.pop('staged_render_token')
        session, result = self.settle(before, self.published())
        self.assertEqual(session.inputs, [])
        self.assertEqual(result['status'], 'prior-render-pending')

    def test_auxiliary_packet_preserves_every_prekey_drain_guard(self):
        cases = [
            ('initialized', 0), ('display_suppression', 1),
            ('batch_busy', 1), ('batch_callback', 1), ('commands_dirty', 1),
            ('display_pending', 1), ('ui_pending', 1), ('ui_busy', 1)]
        for field, value in cases:
            with self.subTest(field=field):
                before = self.paused()
                before[field] = value
                session, result = self.settle(before, self.published())
                self.assertEqual(session.inputs, [])
                self.assertEqual(result['status'], 'prior-render-pending')
        for field, value in (
                ('busy', True), ('command_pending', True), ('status', 1),
                ('started', 368), ('acknowledgements', 366)):
            with self.subTest(graphics_field=field):
                before = self.paused()
                before['graphics'][field] = value
                session, result = self.settle(before, self.published())
                self.assertEqual(session.inputs, [])
                self.assertEqual(result['status'], 'prior-render-pending')
        for role in ('main_ui', 'uilib', 'graphic', 'vglib'):
            with self.subTest(role=role):
                before = self.paused()
                before['tasks'][role]['status'] = 0x10
                session, result = self.settle(before, self.published())
                self.assertEqual(session.inputs, [])
                self.assertEqual(result['status'], 'prior-render-pending')

    def test_postkey_auxiliary_ack_requires_the_requested_token(self):
        for changes in (
                {'staged_render_token': 44},
                {'staged_render_token': 0},
                {'consumer_token': 44},
                {'staged_render_command': 0x5001},
                {'packet_token': 1},
                {'copied_command': 0x8001, 'sender_command': 0x8001},
                {'sender_command': 0x8001},
                {'sender_token': 44, 'consumer_token': 44, 'staged_render_token': 44}):
            with self.subTest(changes=changes):
                after = self.published()
                after.update(changes)
                session, result = self.settle(self.paused(), after)
                self.assertEqual(session.inputs, ['ok'])
                self.assertEqual(result['status'], 'budget-exhausted')
                self.assertIsNone(result['publication'])

    def test_token_wrap_requires_new_matching_stage_and_acknowledgement(self):
        before = self.paused()
        before.update(sender_token=255, consumer_token=255, staged_render_token=255)
        after = self.published()
        after.update(sender_token=1, consumer_token=1, staged_render_token=1)
        session, result = self.settle(before, after)
        self.assertEqual(session.inputs, ['ok'])
        self.assertEqual(result['status'], 'render-batch-published-candidate')
        self.assertEqual(result['requested_packet_token'], 1)
        after['staged_render_token'] = 255
        session, result = self.settle(before, after)
        self.assertEqual(session.inputs, ['ok'])
        self.assertEqual(result['status'], 'budget-exhausted')
        self.assertIsNone(result['publication'])

    def test_auxiliary_wrap_refuses_unchanged_or_unobserved_prior_stage(self):
        for prior_stage in (1, None):
            with self.subTest(prior_stage=prior_stage):
                before = self.paused()
                before.update(copied_command=0x4174, sender_command=0x4174,
                              packet_token=255, sender_token=255, consumer_token=255)
                if prior_stage is None:
                    before.pop('staged_render_token')
                else:
                    before['staged_render_token'] = prior_stage
                after = self.published()
                after.update(sender_token=1, consumer_token=1, staged_render_token=1)
                session, result = self.settle(before, after)
                self.assertEqual(session.inputs, ['ok'])
                self.assertEqual(result['requested_packet_token'], 1)
                self.assertEqual(result['status'], 'budget-exhausted')
                self.assertIsNone(result['publication'])

    def test_auxiliary_ack_does_not_replace_new_work_or_selected_plane_copy(self):
        for field, value in (
                ('bytes_completed', 135558976), ('last_copy', None),
                ('busy', True), ('command_pending', True), ('status', 1),
                ('started', 369), ('acknowledgements', 367)):
            with self.subTest(graphics_field=field):
                after = self.published()
                after['graphics'][field] = value
                session, result = self.settle(self.paused(), after)
                self.assertEqual(session.inputs, ['ok'])
                self.assertEqual(result['status'], 'budget-exhausted')
                self.assertIsNone(result['publication'])
        for changes in (
                {'destination': '0x228c804'},
                {'sha256': '0' * 64},
                {'bytes': 1382396},
                {'completed_job': 367}):
            with self.subTest(copy_changes=changes):
                after = self.published()
                after['graphics']['last_copy'].update(changes)
                session, result = self.settle(self.paused(), after)
                self.assertEqual(session.inputs, ['ok'])
                self.assertEqual(result['status'], 'budget-exhausted')
                self.assertIsNone(result['publication'])

    def test_original_nonrender_prekey_family_rules_are_unchanged(self):
        before, after = self.paused(), self.published()
        before.update(copied_command=0x9000, sender_command=0x9000, packet_token=44)
        after.update(copied_command=0x4047, sender_command=0x4047, packet_token=45)
        for probe in (before, after):
            probe.pop('staged_render_command')
            probe.pop('staged_render_token')
        session, result = self.settle(before, after)
        self.assertEqual(session.inputs, ['ok'])
        self.assertEqual(result['status'], 'render-batch-published-candidate')

    def test_unready_probe_never_injects_input(self):
        before = self.paused()
        before['ready'] = False
        session, result = self.settle(before, self.published())
        self.assertEqual(session.inputs, [])
        self.assertEqual(result['key_instructions'], 0)
        self.assertEqual(result['status'], 'rendering-not-ready')

    def test_postkey_missing_stage_and_drain_guards_refuse_publication(self):
        for field in ('staged_render_command', 'staged_render_token'):
            with self.subTest(missing=field):
                after = self.published()
                after.pop(field)
                session, result = self.settle(self.paused(), after)
                self.assertEqual(session.inputs, ['ok'])
                self.assertEqual(result['status'], 'budget-exhausted')
                self.assertIsNone(result['publication'])
        for field, value in (
                ('initialized', 0), ('display_suppression', 1),
                ('batch_busy', 1), ('batch_callback', 1), ('commands_dirty', 1),
                ('display_pending', 1), ('ui_pending', 1), ('ui_busy', 1),
                ('ready', False)):
            with self.subTest(field=field):
                after = self.published()
                after[field] = value
                session, result = self.settle(self.paused(), after)
                self.assertEqual(session.inputs, ['ok'])
                self.assertEqual(result['status'], 'rendering-not-ready'
                                 if field == 'ready' else 'budget-exhausted')
                self.assertIsNone(result['publication'])
        for role in ('main_ui', 'uilib', 'graphic', 'vglib'):
            with self.subTest(role=role):
                after = self.published()
                after['tasks'][role]['status'] = 0x10
                session, result = self.settle(self.paused(), after)
                self.assertEqual(session.inputs, ['ok'])
                self.assertEqual(result['status'], 'budget-exhausted')
                self.assertIsNone(result['publication'])

    def test_selected_instance_and_full_plane_geometry_remain_required(self):
        for changes in (
                {'width': 719}, {'height': 479}, {'origin': [1, 0]},
                {'alpha_field': 0}):
            with self.subTest(plane=changes):
                session = RenderSettleSession(self.paused(), self.published())
                session.state['display']['last_frame']['graphics'][0].update(changes)
                _, result = self.settle_session(session)
                self.assertEqual(result['status'], 'budget-exhausted')
                self.assertIsNone(result['publication'])
        session = RenderSettleSession(self.paused(), self.published())
        session.state['display']['selected_instance'] = 2
        _, result = self.settle_session(session)
        self.assertEqual(result['status'], 'budget-exhausted')
        self.assertIsNone(result['publication'])

    def test_delayed_acknowledgement_can_finish_within_the_original_budget(self):
        session = RenderSettleSession(self.paused(), self.published())
        pending = self.paused()
        pending.update(sender_token=45, staged_render_token=45)

        def probe():
            if session.inputs and session.board.instructions < 3:
                return copy.deepcopy(pending)
            return session.probe()

        session, result = self.settle_session(session, budget=1, probe=probe)
        self.assertEqual(session.inputs, ['ok'])
        self.assertEqual(result['key_instructions'], 2)
        self.assertEqual(result['settle_instructions'], 1)
        self.assertEqual(result['status'], 'render-batch-published-candidate')

    def test_pending_acknowledgement_exhausts_budget_without_retrying_input(self):
        after = self.published()
        after['consumer_token'] = 44
        session = RenderSettleSession(self.paused(), after)
        session, result = self.settle_session(session, budget=1)
        self.assertEqual(session.inputs, ['ok'])
        self.assertEqual(result['key_instructions'], 2)
        self.assertEqual(result['settle_instructions'], 1)
        self.assertEqual(result['status'], 'budget-exhausted')
        self.assertIsNone(result['publication'])


if __name__ == '__main__':
    unittest.main()
