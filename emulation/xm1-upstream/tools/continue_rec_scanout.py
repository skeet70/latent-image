#!/usr/bin/env python3
"""Keep a native X-M1 1.50 board warm and sample firmware-selected display RAM.

Example (the profile explicitly selects unmeasured peripheral/render laws):
  PYTHONPATH=native/python:. python tools/continue_rec_scanout.py \
      --profile firmware/x-m1/1.50/emulator-rec.json --serve

After saving a drained snapshot, start a fresh process with:
  PYTHONPATH=native/python:. .venv/bin/python tools/continue_rec_scanout.py \
      --snapshot .tools/paused-rec --instructions 5000000 --serve

Only explicit sample/run operations capture frames. GET requests serve the last
sample; frame counters count host samples, not panel refreshes. PNG and raw plane
concatenations overwrite bounded latest artifacts in a fresh local .tools folder.
Snapshots are local machine state; unsupported CPU/model state refuses save.
No guest memory edits, camera I/O or automatic input are used.
With --menus, key presses change the declared ADC input and execute the stock
scanner. The viewer exposes OK, BACK, Q and the four selector buttons.
"""
import argparse
import copy
from dataclasses import asdict
from datetime import datetime, timezone
import inspect
import json
from pathlib import Path
import sys
from time import perf_counter
from uuid import uuid4

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

MAX_CONTINUATION = 15_000_000


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('Duplicate JSON key: ' + key)
        result[key] = value
    return result


def load_profile(path, factory, *, firmware_profile=None):
    """Read options JSON or resolve the canonical X-M1 firmware profile."""
    document = json.loads(Path(path).read_text(), object_pairs_hook=_unique_object)
    if firmware_profile is not None or isinstance(document, dict) and document.get('firmware_id') == 'xm1-150':
        from emulator.boards.profile import resolve
        return {
            'firmware_profile': (firmware_profile if firmware_profile is not None else
                resolve('xm1-150', profile_file=str(Path(path).resolve().relative_to(ROOT)))),
            'instructions_per_tick': 24, 'storage_profile': 'erased',
            'irq_tie_profile': 'kernel-active-scan-order',
            'graphics_profile': 'mode1-bgra8888-straight',
            'backdrop_profile': 'declared-black-canvas-unmeasured',
            'rectangle_profile': 'xm1-150-declared-luma-rectangle-overlay-unmeasured',
            'copy_profile': 'stock-vg-32bpp-fallback',
            'clear_profile': 'declared-vg-ui-primitives-unmeasured',
            'input_processing_profile': 'xm1-150-packed-command-writes',
            'input_phase_profile': 'xm1-150-phase2-ack-unmeasured',
            'input_event_profile': 'xm1-150-configured-phase-clock-unmeasured',
            'input_event_ticks': 200000,
            'pulse_timer_profile': 'xm1-150-configured-timer-clocks-unmeasured',
            'auxiliary_control_profile': 'stock-control3-bit0-clear-ack-unmeasured',
            'producer_stop_profile': 'stock-idle-stop-handshakes-unmeasured',
            'image_work_stop_profile': 'stock-initial-image-banks-bit0-stop-ack-unmeasured',
            'image_work_data_profile': 'xm1-150-declared-continuous-postisp-scene-unmeasured',
            'image_work_data_ticks': 50000,
            'movrsz_profile': 'declared-packed-yycbcr-pyramid-unmeasured',
            'movrsz_ticks': 50000,
            'producer_data_profile': 'xm1-150-declared-continuous-statistics-scene-unmeasured',
        }
    if not isinstance(document, dict) or set(document) != {'options'}:
        raise ValueError('Profile must be X-M1 1.50 or contain only an options object')
    options = document['options']
    if not isinstance(options, dict):
        raise ValueError('Profile options must be an object')
    unknown = set(options) - set(inspect.signature(factory).parameters)
    if unknown:
        raise ValueError('Unknown board options: ' + ', '.join(sorted(unknown)))
    if 'firmware_profile' not in options:
        from emulator.boards.profile import resolve
        options['firmware_profile'] = resolve(
            'xm1-150', profile_file='firmware/x-m1/1.50/emulator-rec.json')
    return options


def _pick(obj, keys):
    return {key: copy.deepcopy(getattr(obj, key)) for key in keys if hasattr(obj, key)}


def console_summary(state):
    """Keep ordinary execution output small; detailed state stays in the API."""
    display = state.get('display') or {}
    frame = display.get('last_frame') or {}
    ui = state.get('ui') or {}
    return {key: value for key, value in {
        'instructions': state.get('instructions'),
        'virtual_ticks': state.get('virtual_ticks'),
        'failure': state.get('failure'),
        'execution_error': state.get('execution_error'),
        'ui': {key: ui[key] for key in ('mode', 'pending', 'busy') if key in ui},
        'rec_initialization_idle': state.get('rec_initialization_idle'),
        'frame': {key: frame[key] for key in ('width', 'height', 'png_sha256') if key in frame},
        'artifact_directory': state.get('artifact_directory'),
        'performance': state.get('performance'),
    }.items() if value is not None}


def compact_board_state(board):
    """Read modeled registers/retained transactions, not stale MMIO RAM backing."""
    from emulator.rec_state import Reader, ui_state
    from emulator.producer_stream import StatisticsFrame
    ui = ui_state(Reader(board))
    devices = []
    fields = {
        'GraphicsAccelerator': ('clear_profile', 'copy_profile', 'busy', 'jobs_started',
                                'jobs_completed', 'irqs_raised', 'acknowledgements'),
        'InputProcessingControl': ('event_profile', 'event_enable_generation', 'event_due',
                                  'event_source', 'event_irq', 'event_pending', 'event_arms',
                                  'event_assertions', 'event_acknowledgments', 'event_cancellations',
                                  'event_extra_attempts', 'event_stream_blocked', 'last_event',
                                  'registers', 'command_profile', 'command_armed',
                                  'pending_command', 'last_command', 'last_rejected',
                                  'phase_clock_due'),
        'ProducerControl': ('data_profile', 'busy', 'dma_busy', 'data_jobs', 'data_transfers',
                            'data_irqs', 'pending_data', 'pending_ack', 'last_data',
                            'ack_requests', 'ack_completions', 'rearm', 'stream'),
        'ImageWorkControl': ('data_profile', 'busy', 'dma_busy', 'pending_data', 'data_history', 'stream'),
    }
    for device in board.devices:
        name = type(device).__name__
        if name in ('DisplayScanout', 'DisplayTiming', 'DisplayFormatter'):
            item = copy.deepcopy(device.snapshot())
            item.setdefault('device', name)
        elif name in fields:
            item = {'device': name, **_pick(device, fields[name])}
            if name == 'GraphicsAccelerator':
                job = getattr(device, 'last_job', None) or {}
                item['last_job'] = {key: copy.deepcopy(job[key]) for key in (
                    'operation', 'draw_count', 'filled_paths', 'stroked_paths', 'sampled_images',
                    'destinations', 'completed', 'covered_bytes_in_draw_order', 'rejection') if key in job}
            if name == 'InputProcessingControl':
                item['phase_clock_pending'] = {int(source): asdict(edge)
                                               for source, edge in device.phase_clock_pending.items()}
                item['recent_event_history'] = copy.deepcopy(device.event_history[-8:])
            if name == 'ProducerControl' and item.get('stream') is not None:
                frame = item['stream'].get('frame')
                if type(frame) is StatisticsFrame:
                    item['stream']['frame'] = asdict(frame)
        elif name in ('BootControls', 'PulseOutputs', 'AudioSerialControl', 'FujiDecompressor', 'LoaderMemoryDMA', 'InputAuxiliaryControl'):
            item = {'device': name, 'state': device.snapshot()}
        else:
            continue
        devices.append(item)
    deadlines = []
    for device in board.devices:
        if hasattr(device, 'next_event_tick'):
            due = device.next_event_tick(board.virtual_ticks)
            if due is not None:
                deadlines.append({'device': type(device).__name__, 'tick': due})
    return {'instructions': board.instructions, 'virtual_ticks': board.virtual_ticks,
            'entry': board.entry, 'failure': copy.deepcopy(board.failure), 'quiescent': board.quiescent,
            'ui': ui, 'rec_initialization_idle': None, 'devices': devices,
            'milestones': copy.deepcopy(board.milestones),
            'events': copy.deepcopy(board.event_log[-12:]), 'pending_deadlines': deadlines,
            'queued_callbacks': len(board.scheduled),
            'readiness_limits': 'Current idle flags do not establish handshake history or responsive shooting.'}


def _guest_boundary(board):
    """Cheap execution/IRQ boundary check; this is not a whole-DRAM fingerprint."""
    from unicorn.arm_const import UC_ARM_REG_PC, UC_ARM_REG_SP, UC_ARM_REG_CPSR
    return (board.instructions, board.virtual_ticks, copy.deepcopy(board.failure),
            tuple(tuple(cpu.reg_read(reg) for reg in (UC_ARM_REG_PC, UC_ARM_REG_SP, UC_ARM_REG_CPSR))
                  for cpu in board.cpus), copy.deepcopy(board.interrupts.snapshot()),
            tuple((item[0], item[1], id(item[2])) for item in board.scheduled))


class WarmScanoutSession:
    """Small make_server-compatible adapter; also usable on an existing board."""
    def __init__(self, board, output, *, options=None):
        self.board, self.output = board, Path(output)
        self.output.mkdir(parents=True, exist_ok=False)
        self.options = dict(options or {})
        profile = self.options.get('firmware_profile')
        if profile is not None and not isinstance(profile, dict):
            self.options['firmware_profile'] = {
                'firmware_id': profile.firmware_id, 'profile_file': str(profile.profile_file),
                'profile_sha256': profile.profile_sha256}
        self.state = None
        self.raw = self.png = b''
        self.checkpoint = None
        self.execution_error = None

    def sample(self, pixels=True):
        """Sample both display planes; pixels=False keeps only plane metadata.

        A metadata sample (used between settle continuations) skips
        composition, PNG encoding and frame files; session.png is then empty
        until compose_frames() renders the same boundary.
        """
        from emulator.scanout import UnsupportedScanout
        before = _guest_boundary(self.board)
        frames, samples = {}, []
        for instance in (0, 1):
            try:
                frame = self.board.display.frame_boundary(instance, cpu_id=0, pixels=pixels)
            except UnsupportedScanout as error:
                samples.append({'instance': instance, 'render_error': str(error), 'last_frame': None,
                                'frame_count': self.board.display.frames[instance]})
                continue
            samples.append({'instance': instance, 'last_frame': None if frame is None else frame.state,
                            'frame_count': self.board.display.frames[instance], 'render_error': None})
            if frame is not None:
                frames[instance] = frame
        if before != _guest_boundary(self.board):
            raise RuntimeError('Host scanout sampling changed the CPU/tick/IRQ boundary')
        # Clear old pixels even if all planes are now disabled/unsupported.
        # This prevents the viewer from presenting an earlier frame as current.
        self.raw = self.png = b''
        selected = min(frames) if frames else None
        self._pixels_pending = not pixels
        if pixels:
            self._write_frames(frames)
        elif selected is not None:
            self.raw = frames[selected].raw
        self.state = compact_board_state(self.board)
        self.state.update(
            display={'selected_instance': selected,
                     'frame_count': 0 if selected is None else self.board.display.frames[selected],
                     'last_frame': None if selected is None else frames[selected].state,
                     'instances': samples},
            profiles=self.options, storage_profile=copy.deepcopy(getattr(self.board, 'storage_profile', None)),
            execution_backend={'name': 'rust-unicorn'}, artifact_directory=str(self.output.resolve()),
            resume_checkpoint={'present': self.checkpoint is not None},
            execution_error=copy.deepcopy(self.execution_error),
            sampling={'cpu_tick_irq_boundary_unchanged': True,
                      'limits': 'Host sampling changes only scanout counters/metadata and event log; no full-memory fingerprint is taken.'})
        if self.execution_error is not None and self.state['failure'] is None:
            self.state['failure'] = {'message': 'Strict continuation stopped outside CPU dispatch',
                                     **self.execution_error}
        if self.checkpoint is not None:
            self.state['resume_checkpoint']['metadata'] = self.checkpoint.metadata
        return self.state

    def _write_frames(self, frames):
        selected = min(frames) if frames else None
        for instance in (0, 1):
            for suffix in ('bin', 'png'):
                target = self.output / f'frame-{instance}.{suffix}'
                if instance not in frames:
                    target.unlink(missing_ok=True)
                    continue
                data = frames[instance].raw if suffix == 'bin' else frames[instance].png
                temporary = target.with_suffix(target.suffix + '.tmp')
                temporary.write_bytes(data)
                temporary.replace(target)
        if selected is not None:
            self.raw, self.png = frames[selected].raw, frames[selected].png

    def compose_frames(self):
        """Render pixels for the boundary of the last metadata-only sample.

        Leaves frame counters and the event log as a pixel sample would have.
        """
        from emulator.scanout import UnsupportedScanout
        if not getattr(self, '_pixels_pending', False) or self.state is None:
            return self.state
        before = _guest_boundary(self.board)
        frames = {}
        for item in self.state['display']['instances']:
            instance = item['instance']
            if item['render_error'] is not None or item['last_frame'] is None:
                continue
            try:
                frame = self.board.display.compose(instance, cpu_id=0)
            except UnsupportedScanout as error:
                self.board.display.render_errors[instance] = str(error)
                item['render_error'] = str(error)
                continue
            if frame is not None:
                frames[instance] = frame
                item['last_frame'] = frame.state
        if before != _guest_boundary(self.board):
            raise RuntimeError('Host scanout composition changed the CPU/tick/IRQ boundary')
        self._pixels_pending = False
        self.raw = self.png = b''
        self._write_frames(frames)
        selected = min(frames) if frames else None
        self.state['display']['last_frame'] = None if selected is None else frames[selected].state
        return self.state

    def run(self, options):
        if not isinstance(options, dict) or set(options) - {'limit', 'restore', 'reset', 'pixels'}:
            raise ValueError('Only limit, restore, reset=false and pixels are supported')
        if type(options.get('pixels', True)) is not bool:
            raise ValueError('Pixels must be a boolean')
        limit = options.get('limit', 0)
        if type(limit) is not int or not 0 <= limit <= MAX_CONTINUATION:
            raise ValueError('Continuation limit must be 0..15000000; zero samples without execution')
        if type(options.get('reset', False)) is not bool or options.get('reset', False):
            raise ValueError('Start a new process to reset and cold boot')
        if type(options.get('restore', False)) is not bool:
            raise ValueError('Restore must be a boolean')
        if options.get('restore', False):
            if self.checkpoint is None:
                raise ValueError('No process-local checkpoint saved')
            self.checkpoint.restore(self.board)
            self.execution_error = None
        if limit and self.execution_error is not None:
            raise ValueError('Restore the saved checkpoint or start a new process after the strict stop')
        started, previous = perf_counter(), self.board.instructions
        if limit:
            try:
                self.board.run(limit + 2 * self.board.quantum,
                               pause_after=self.board.instructions + limit)
            except Exception as error:
                # Some strict tick errors escape Board.run before failure is
                # assigned. Preserve the paused frame/status, without altering
                # guest RAM or clearing the failed model transaction.
                self.execution_error = {'type': type(error).__name__, 'error': str(error)}
        self.sample(pixels=options.get('pixels', True))
        seconds = perf_counter() - started
        executed = self.board.instructions - previous
        self.state['performance'] = {'execution_and_frame_sampling_seconds': seconds,
                                     'instructions_this_run': executed,
                                     'instructions_per_second': executed / seconds if seconds else None}
        return self.state

    def save_checkpoint(self):
        from x70_native.checkpoint import capture
        self.checkpoint = capture(self.board)
        return self.checkpoint.metadata

    def save_snapshot(self, directory):
        from x70_native.portable_checkpoint import save
        if self.execution_error is not None:
            raise ValueError('Restore the saved checkpoint before saving a stopped machine')
        manifest = save(self.board, directory)
        return {'snapshot': str(Path(directory).resolve()),
                'instructions': self.board.instructions, 'virtual_ticks': self.board.virtual_ticks,
                'schema': manifest['schema']}


def main():
    from emulator.boot import make_board
    from emulator.server import make_menu_server as make_server
    parser = argparse.ArgumentParser(description=__doc__)
    starting = parser.add_mutually_exclusive_group(required=True)
    starting.add_argument('--profile', type=Path, help='Cold boot using the selected options JSON')
    starting.add_argument('--snapshot', type=Path, help='Restore a portable snapshot into fresh native engines')
    budget = parser.add_mutually_exclusive_group()
    budget.add_argument('--pause-after', type=int, help='Absolute instruction threshold; default60M for cold boot, zero for restore')
    budget.add_argument('--instructions', type=int, help='Instructions to run from the selected starting state')
    parser.add_argument('--settings', choices=('default', 'owner'), help='Persistent settings selection; verify restored selection')
    parser.add_argument('--save-snapshot', type=Path, help='Save drained machine state to a fresh local directory')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--serve', action='store_true')
    parser.add_argument('--menus', action='store_true',
                        help='Enable declared ADC button input and stock menu navigation')
    parser.add_argument('--port', type=int, default=0)
    parser.add_argument('--bind', default='127.0.0.1')
    parser.add_argument('--hostname')
    args = parser.parse_args()
    for value in (args.pause_after, args.instructions):
        if value is not None and not 0 <= value <= 100_000_000:
            parser.error('Instruction threshold/budget must be 0..100000000')
    if args.snapshot is not None:
        from x70_native.portable_checkpoint import load
        board = load(args.snapshot)
        options = board._portable_factory_options
        if args.settings is not None and options.get("settings", "default") != args.settings:
            parser.error("Snapshot settings selection differs; cold boot the requested settings")
    else:
        options = load_profile(args.profile, make_board)
        if args.settings is not None:
            options["settings"] = args.settings
        board = make_board(**options)
    if args.instructions is not None:
        threshold = board.instructions + args.instructions if args.instructions else 0
    else:
        threshold = args.pause_after if args.pause_after is not None else (0 if args.snapshot else 60_000_000)
    if threshold and threshold < board.instructions:
        parser.error('Absolute threshold precedes the saved machine; use --instructions for continuation')
    output = args.output or ROOT / '.tools/rec-scanout' / (
        datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ') + '-' + uuid4().hex[:8])
    if args.menus:
        from tools.probe_setup_menus import MenuSession
        session = MenuSession(board, output, options=options)
    else:
        session = WarmScanoutSession(board, output, options=options)
    if args.snapshot is not None and args.serve:
        # The imported snapshot is already a validated drained boundary.
        # Retain it in this process so the viewer can undo later continuations.
        session.save_checkpoint()
    if threshold:
        print('Execute selected native profile to a natural boundary', flush=True)
        try:
            board.run(threshold - board.instructions + 2 * board.quantum, pause_after=threshold)
        except Exception as error:
            session.execution_error = {'type': type(error).__name__, 'error': str(error)}
    if args.serve and args.snapshot is None and board.failure is None and session.execution_error is None:
        session.save_checkpoint()
    if args.save_snapshot is not None:
        print(json.dumps(session.save_snapshot(args.save_snapshot)), flush=True)
    print(json.dumps(console_summary(session.sample()), separators=(',', ':')), flush=True)
    if args.serve:
        from emulator.server import make_menu_server
        factory = make_menu_server if args.menus else make_server
        server = factory(session, args.port, page='boot.html', bind=args.bind, hostname=args.hostname)
        print(f'Warm scanout viewer: http://{args.hostname or args.bind}:{server.server_port}/', flush=True)
        server.serve_forever()
        return
    print('Commands: sample, status, run COUNT, checkpoint, snapshot DIRECTORY, restore, quit. No guest writes.', flush=True)
    if args.menus:
        print('Menu input: key ok|back|q|up|down|left|right. Stock ADC scanning/debounce.', flush=True)
    for line in sys.stdin:
        fields = line.split()
        try:
            if fields == ['quit']:
                break
            if fields == ['sample']:
                value = session.sample()
            elif fields == ['status']:
                value = compact_board_state(board)
            elif fields == ['checkpoint']:
                value = session.save_checkpoint()
            elif len(fields) == 2 and fields[0] == 'snapshot':
                value = session.save_snapshot(fields[1])
            elif fields == ['restore']:
                value = session.run({'restore': True})
            elif len(fields) == 2 and fields[0] == 'run':
                value = session.run({'limit': int(fields[1], 0)})
            elif args.menus and len(fields) == 2 and fields[0] == 'key':
                value = session.key({'key': fields[1]})
            else:
                raise ValueError('Unknown command')
            if fields != ['status'] and isinstance(value, dict) and 'instructions' in value and 'display' in value:
                value = console_summary(value)
            print(json.dumps(value, separators=(',', ':')), flush=True)
        except Exception as error:
            print(json.dumps({'error': type(error).__name__, 'message': str(error)}), flush=True)


if __name__ == '__main__':
    main()
