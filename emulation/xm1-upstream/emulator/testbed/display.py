"""Display outputs for the testbed: LCD/HDMI screenshots, PNG sequences and wait predicates.

Capture reads the display engine's registers and the RAM they point at through
`board.display.frame_boundary` (emulator/scanout.py). It is host sampling only:
the CPU/PC/SP/CPSR/tick/IRQ boundary is compared before and after every capture
and a change raises `BoundaryChanged`. Sampling does change the scanout's own
`frames[instance]` counter and event log. `frames[instance]` counts host samples,
not panel refreshes; recording uses virtual display frames instead (see `record`).

Outputs: `'lcd'` is the image instance the stock LCD route selects (`1 - LCD
selector`, instance 1 in idle REC); `'hdmi'` is instance 0 (the instance the
AV-output flag selects, research/display-hdmi.md); `0`/`1` name an instance
directly. The PNG is the engine's declared interpretation (YYCbCr 4:2:2 BT.601 plus
mode-1 BGRA graphics), not a measured panel or capture-card image.
"""
from dataclasses import dataclass, field
import hashlib
from pathlib import Path

import numpy as np

from . import events
from .predicates import Pred

AV_OUTPUT = 0xc64338        # nonzero: image API addresses instance 0 (HDMI)
LCD_SELECTOR = 0xc6431c     # LCD selector; LCD instance = 1 - selector


class BoundaryChanged(RuntimeError):
    """A host sample altered CPU, tick, IRQ or scheduler state (must not happen)."""


class NoFrame(RuntimeError):
    """The selected output has no frame: plane disabled, or unsupported composition."""


@dataclass
class Screenshot:
    output: str
    instance: int
    png: bytes
    rgb: np.ndarray = field(repr=False)
    raw: bytes = field(repr=False)
    sha256: str = ''
    width: int = 0
    height: int = 0
    frame_count: int = 0       # scanout host-sample counter for this instance
    instructions: int = 0
    ticks: int = 0
    state: dict = field(default_factory=dict, repr=False)

    @property
    def metadata(self):
        return {'output': self.output, 'instance': self.instance, 'sha256': self.sha256,
                'width': self.width, 'height': self.height, 'frame_count': self.frame_count,
                'instructions': self.instructions, 'ticks': self.ticks,
                'source': (self.state.get('main') or {}).get('source'),
                'frame_sha256': self.state.get('frame_sha256')}


def guest_boundary(board):
    """Cheap execution/IRQ boundary (as tools/continue_rec_scanout.py); not a RAM fingerprint."""
    import copy
    from unicorn.arm_const import UC_ARM_REG_CPSR, UC_ARM_REG_PC, UC_ARM_REG_SP
    return (board.instructions, board.virtual_ticks, copy.deepcopy(board.failure),
            tuple(tuple(cpu.reg_read(reg) for reg in (UC_ARM_REG_PC, UC_ARM_REG_SP, UC_ARM_REG_CPSR))
                  for cpu in board.cpus), copy.deepcopy(board.interrupts.snapshot()),
            tuple((item[0], item[1], id(item[2])) for item in board.scheduled))


@dataclass
class Recording:
    directory: Path
    output: str
    frames: list = field(default_factory=list)     # one metadata dict per written PNG
    stopped: str = ''                              # max_frames | timeout | failure | until
    skipped: int = 0                               # sample points with no frame for the output

    @property
    def count(self):
        return len(self.frames)


class DisplayRecorder:
    def __init__(self, testbed):
        self.tb, self.board = testbed, testbed.board
        self.last_error = {0: None, 1: None}
        # Snapshots saved before the scanout default grew restore ram_bytes=128 MiB; the board's
        # RAM is larger and the HDMI OSD buffer lives above 128 MiB (e.g. 0x1e834f40).
        scanout = testbed.board.display
        if getattr(scanout, 'ram_bytes', len(testbed.board.ram)) < len(testbed.board.ram):
            scanout.ram_bytes = len(testbed.board.ram)
        self.hdmi_sink = None

    # Output selection
    def instance(self, output='lcd'):
        if output in (0, 1) and not isinstance(output, bool):
            return int(output)
        if output == 'hdmi':
            return 0
        if output == 'lcd':
            selector = self.tb.read(LCD_SELECTOR)
            return 1 - selector if selector in (0, 1) else 0
        raise ValueError("output must be 'lcd', 'hdmi', 0 or 1")

    def _label(self, output):
        return output if isinstance(output, str) else f'instance{output}'

    # Capture
    def capture(self, output='lcd'):
        """Screenshot of an output, or None when its plane is disabled/unsupported
        (the reason is kept in `last_error[instance]`)."""
        from emulator.scanout import UnsupportedScanout
        instance, board = self.instance(output), self.board
        before = guest_boundary(board)
        try:
            frame = board.display.frame_boundary(instance, cpu_id=0)
            error = None if frame is not None else 'plane disabled (no main video or graphics layer enabled)'
        except UnsupportedScanout as exc:
            frame, error = None, str(exc)
        if guest_boundary(board) != before:
            raise BoundaryChanged('Host display sampling changed the CPU/tick/IRQ boundary')
        self.last_error[instance] = error
        if frame is None:
            return None
        return Screenshot(self._label(output), instance, frame.png, frame.rgb, frame.raw,
                          frame.state['png_sha256'], frame.state['width'], frame.state['height'],
                          board.display.frames[instance], board.instructions, board.virtual_ticks, frame.state)

    def screenshot(self, output='lcd'):
        """`Screenshot` (PNG bytes in `.png`, decoded `.rgb`, `.metadata`); raises `NoFrame`."""
        shot = self.capture(output)
        if shot is None:
            raise NoFrame(f'{output}: {self.last_error[self.instance(output)]}')
        return shot

    # Frame clock
    def frame_period_ticks(self):
        timing = next((d for d in self.board.devices if type(d).__name__ == 'DisplayTiming'), None)
        period = getattr(timing, 'frame_period_ticks', None)
        if not period:
            raise RuntimeError('No DisplayTiming virtual frame period on this board')
        return period

    # Recording
    def record(self, directory, every_n_frames=1, max_frames=60, *, output='lcd', timeout_instructions=None,
               until=None, step=20_000, skip_unchanged=False):
        """Run the board and write a PNG sequence, one sample per `every_n_frames` virtual display
        frames (ticks // DisplayTiming.frame_period_ticks, an unmeasured model period; independent
        of whether the output's timing generator runs). Stops after `max_frames` PNGs, the
        instruction `timeout_instructions`, a board failure, or when `until(tb)` is true. Emits a
        `display.frame` event per PNG (output, instance, frame_count, index, ticks, sha256, path).
        Frames where the output has no plane are skipped (counted in `Recording.skipped`).
        `directory` must be new."""
        if type(every_n_frames) is not int or every_n_frames < 1:
            raise ValueError('every_n_frames must be a positive integer')
        directory = Path(directory)
        directory.mkdir(parents=True, exist_ok=False)
        board, period = self.board, self.frame_period_ticks()
        recording = Recording(directory, self._label(output))
        start = board.instructions
        next_frame = board.virtual_ticks // period + 1
        last_sha = None
        while True:
            if board.failure is not None:
                recording.stopped = 'failure'
                break
            if until is not None and until(self.tb):
                recording.stopped = 'until'
                break
            if len(recording.frames) >= max_frames:
                recording.stopped = 'max_frames'
                break
            if timeout_instructions is not None and board.instructions - start >= timeout_instructions:
                recording.stopped = 'timeout'
                break
            self.tb.run(step)
            frame_index = board.virtual_ticks // period
            if frame_index < next_frame:
                continue
            next_frame = frame_index + every_n_frames
            shot = self.capture(output)
            if shot is None or (skip_unchanged and shot.sha256 == last_sha):
                recording.skipped += 1
                continue
            last_sha = shot.sha256
            path = directory / f'frame-{len(recording.frames):06d}.png'
            path.write_bytes(shot.png)
            meta = {**shot.metadata, 'index': len(recording.frames), 'virtual_frame': frame_index,
                    'path': str(path)}
            recording.frames.append(meta)
            events.emit(board, 'display.frame', output=shot.output, instance=shot.instance,
                        frame_count=shot.frame_count, index=meta['index'], virtual_frame=frame_index,
                        sha256=shot.sha256, path=str(path))
        return recording

    # Connected HDMI sink (opt-in)
    def connect_hdmi(self, **options):
        """Replace the stock disconnected HDMI startup device by the declared connected-sink profile
        (emulator/hdmi_sink.py). HPD stays low until `plug_hdmi()`."""
        from emulator.hdmi_sink import HDMISink
        self.hdmi_sink = HDMISink.install(self.board, **options)
        return self.hdmi_sink

    def plug_hdmi(self):
        if self.hdmi_sink is None:
            self.connect_hdmi()
        self.hdmi_sink.plug()
        events.emit(self.board, 'hdmi.plug')
        return self.hdmi_sink

    def unplug_hdmi(self):
        self.hdmi_sink.unplug()
        events.emit(self.board, 'hdmi.unplug')


class display_changed(Pred):
    """True once the output's frame hash differs from its first sample after the wait began
    (an output with no frame counts as a distinct state). `region=(x0, y0, x1, y1)` compares only
    that pixel rectangle of the RGB image."""

    def __init__(self, output='lcd', region=None):
        self.output, self.region, self.baseline = output, region, None

    def _key(self, tb):
        shot = tb.display.capture(self.output)
        if shot is None:
            return None
        if self.region is None:
            return shot.sha256
        x0, y0, x1, y1 = self.region
        return hashlib.sha256(np.ascontiguousarray(shot.rgb[y0:y1, x0:x1]).tobytes()).hexdigest()

    def begin(self, tb):
        self.baseline = ('set', self._key(tb))

    def __call__(self, tb):
        if self.baseline is None:
            self.begin(tb)
            return False
        return self._key(tb) != self.baseline[1]


class display_matches(Pred):
    """True when `fn(rgb)` (uint8 array, height x width x 3) is true for the output's current frame."""

    def __init__(self, output, fn):
        self.output, self.fn = output, fn

    def __call__(self, tb):
        shot = tb.display.capture(self.output)
        return shot is not None and bool(self.fn(shot.rgb))
