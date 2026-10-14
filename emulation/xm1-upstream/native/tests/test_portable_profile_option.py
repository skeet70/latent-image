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


if __name__ == '__main__':
    unittest.main()
