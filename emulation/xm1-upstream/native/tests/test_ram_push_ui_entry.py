"""The RAM-push e2e trajectory entry against stock X-M1 1.50 live view after the bridge's inputs.

RAM_PUSH_E2E_INPUTS names a directory holding selected.DAT, adjustment.nand and card.img
(private data); RAM_PUSH_E2E_TRAJECTORIES overrides emulation/ram-push-e2e-trajectories.json.
"""
import json
import os
import tempfile
import unittest
from pathlib import Path

INPUTS = os.environ.get('RAM_PUSH_E2E_INPUTS')
TRAJECTORIES = Path(os.environ.get('RAM_PUSH_E2E_TRAJECTORIES',
                                   Path(__file__).resolve().parents[3] / 'ram-push-e2e-trajectories.json'))


def stock_prefix(steps):
    """Steps up to and including the first RIGHT, which enters SHOOTING MENU page 5 on stock."""
    for index, step in enumerate(steps):
        if step[0] == 'right':
            return steps[:index + 1]
    raise AssertionError('trajectory never enters a menu page')


@unittest.skipUnless(INPUTS, 'RAM_PUSH_E2E_INPUTS names the private emulator inputs')
class TrajectoryEntryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        from emulator.boot import make_board
        from emulator.sd_card import BLANK_SDHC_PROFILE, SDCardController, load_raw_image
        from tools import continue_rec_scanout as crs
        from tools.probe_setup_menus import MenuSession
        inputs = Path(INPUTS)
        options = crs.load_profile(Path('firmware/x-m1/1.50/emulator-rec.json'), make_board)
        options['entry'] = 'rom'
        options.update(sd_card_profile=BLANK_SDHC_PROFILE, sd_card_present=True)
        board = make_board(**options)
        crs.attach_installed_dat(board, inputs / 'selected.DAT')
        crs.attach_adjustment_nand(board, inputs / 'adjustment.nand')
        controller = next(device for device in board.devices if isinstance(device, SDCardController))
        options['recovery'] = {'phase': 'independent-installed-cold-boot',
                               'media': str(inputs / 'selected.DAT'), 'card_present': True,
                               'adjustment_nand': str(inputs / 'adjustment.nand'),
                               'card': load_raw_image(controller.card, (inputs / 'card.img').resolve())}
        cls.output = tempfile.TemporaryDirectory()
        cls.session = MenuSession(board, Path(cls.output.name) / 'session', options=options)
        cls.board = board
        cls.bridge, _ = crs._ram_push_attach(board, cls.session)
        cls.bridge.use_timing_ratio(2400, 'USB transfer')
        # Without this run, a rendezvous-length UP hold did not enter focus-area selection (emulated).
        cls.bridge.run_for(5_000_000)
        for _ in range(2000):
            if not board.scheduled:
                break
            cls.bridge.run_for(10_000)
        cls.session.save_checkpoint()
        cls.trajectories = json.loads(TRAJECTORIES.read_text())

    @classmethod
    def tearDownClass(cls):
        cls.output.cleanup()

    def setUp(self):
        self.session.checkpoint.restore(self.board)
        self.session.execution_error = None

    def replay(self, steps):
        return [[key, *self.bridge.key(key).values()] for key, *_ in steps]

    def hold_up(self):
        # Instructions between the install hold and the next host transfer in e2e run-e2e3-nav-3.
        latched = self.bridge.hold_rendezvous_inputs()
        self.bridge.run_for(455_645)
        self.bridge.release_rendezvous_inputs(latched)

    def test_capture_writes_the_live_view_frame(self):
        import hashlib
        captured = self.bridge.capture('live-view', self.output.name)
        data = Path(captured['path']).read_bytes()
        self.assertTrue(data.startswith(b'\x89PNG\r\n\x1a\n'))
        self.assertEqual(captured['sha256'], hashlib.sha256(data).hexdigest())
        self.assertEqual(captured['menu'], {'menu_id': 67, 'setting_id': 56, 'cursor': 0})

    def test_keepalive_leaves_menu_opening_shooting_menu(self):
        self.bridge.keep_session_awake()
        self.bridge.keep_session_awake()
        self.assertEqual(self.bridge.key('menu'), {'menu_id': 1, 'setting_id': 5, 'cursor': 2})

    def test_bank_navigation_entry_matches_after_keepalive_and_rendezvous_hold(self):
        steps = stock_prefix(self.trajectories['before_unload'][0]['steps'])
        self.bridge.keep_session_awake()
        self.hold_up()
        self.assertEqual(self.replay(steps), steps)

    def test_bank_navigation_entry_matches_after_rendezvous_hold(self):
        steps = stock_prefix(self.trajectories['before_unload'][0]['steps'])
        self.hold_up()
        self.assertEqual(self.replay(steps), steps)


if __name__ == '__main__':
    unittest.main()
