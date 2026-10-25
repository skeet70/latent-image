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
The guest inputs live in scanout_attach.py and the bridge commands in
scanout_commands.py.
"""
import argparse
import copy
from dataclasses import asdict
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import struct
import sys
from time import perf_counter
from uuid import uuid4

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from tools.scanout_attach import (ATTACH_RECORD, FIRST_START_PULSE, INSTALL_RECORD,
                                  MODE_DIAL_SETTLE, _ram_push_attach, _read_ram_push_record,
                                  _save_ram_push_checkpoint, _unique_object, cold_board,
                                  load_profile)
from tools.scanout_commands import (_FF80CommandBridge as _FF80EndpointBridge,
                                    _install_checkpoints, _ram_push_bridge,
                                    _restore_ram_push_checkpoint, _run_ram_push_server)

MAX_CONTINUATION = 15_000_000

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
        'GraphicsAccelerator': ('clear_profile', 'copy_profile', 'max_command_capture_bytes',
                                'busy', 'jobs_started', 'jobs_completed', 'irqs_raised', 'acknowledgements'),
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
        elif name in ('BootControls', 'PulseOutputs', 'AudioSerialControl', 'FujiDecompressor', 'LoaderMemoryDMA', 'InputAuxiliaryControl', 'SDCardController', 'FFF63Engine'):
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
            recovery=copy.deepcopy(self.options.get('recovery')),
            sampling={'cpu_tick_irq_boundary_unchanged': True,
                      'limits': 'Host sampling changes only scanout counters/metadata and event log; no full-memory fingerprint is taken.'})
        if self.execution_error is not None and self.state['failure'] is None:
            self.state['failure'] = {'message': 'Strict continuation stopped outside CPU dispatch',
                                     **self.execution_error}
        if self.checkpoint is not None:
            self.state['resume_checkpoint']['metadata'] = self.checkpoint.metadata
        if pixels:
            self._write_recovery_state()
        return self.state

    def _write_recovery_state(self):
        if 'recovery' in self.options:
            self.state['machine'] = self.board.snapshot()
            self.state['installed_nand'] = self.board.nand.snapshot()
            (self.output / 'state.json').write_text(json.dumps(self.state, indent=2) + '\n')

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
        self._write_recovery_state()
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

    def save_nand(self, path):
        import hashlib
        payload = self.board.nand.export_persistent_image()
        with Path(path).open('xb') as target:
            target.write(payload)
        return {'installed_nand': str(Path(path).resolve()),
                'sha256': hashlib.sha256(payload).hexdigest(),
                'limits': 'Committed NAND backing only; no CPU, RAM, IRQ or controller state survives cold reload'}

    def save_sd(self, path):
        from emulator.sd_card import SDCardController, save_raw_image
        controller = next(device for device in self.board.devices if isinstance(device, SDCardController))
        return save_raw_image(controller.card, path)




# A USB-toggle boot checkpoint: saved at the shooting screen with the cable level as the boot left it.
TOGGLE_RECORD = 'usb-toggle-boot.json'
def _run_ff80_rehearsal(board, session, output):
    """Drive the stock ordinary-boot service session through the guest USB stack."""
    from emulator.power_devices import PowerDevices
    from emulator.rec_state import Reader, task_state
    from emulator.usb_devices import USBControl
    from tools.probe_setup_menus import MenuSession
    from unicorn.arm_const import UC_ARM_REG_SP

    if not isinstance(session, MenuSession):
        raise ValueError('ff80 rehearsal requires --menus')
    power = next(device for device in board.devices if type(device) is PowerDevices)
    usb = next(device for device in board.devices if type(device) is USBControl)
    endpoint = usb._endpoint
    if endpoint is None:
        raise ValueError('ff80 rehearsal requires the selected endpoint transport')
    if power.usb_connected:
        raise ValueError('ff80 rehearsal requires a post-boot cable attach')

    def run_until(label, predicate, budget):
        end = board.instructions + budget
        while not predicate():
            if board.failure is not None or session.execution_error is not None:
                machine = board.snapshot()
                cores = [{'pc': core.get('pc'), 'lr': core.get('lr'), 'sp': core.get('sp'),
                          'recent': core.get('recent_instructions', [])[-12:]}
                         for core in machine.get('cores', [])]
                raise RuntimeError(f'{label}: {board.failure or session.execution_error}; cores={cores}')
            if board.instructions >= end:
                raise TimeoutError(f'{label}: instruction budget expired; usb={endpoint.snapshot()}')
            target = min(end, board.instructions + 1_000_000)
            try:
                board.run(target - board.instructions + 2 * board.quantum, pause_after=target)
            except Exception as error:
                session.execution_error = {'type': type(error).__name__, 'error': str(error)}

    def run_for(count):
        target = board.instructions + count
        while board.instructions < target:
            run_until('bounded continuation', lambda: board.instructions >= target,
                      target - board.instructions)

    def take_in(endpoint_address, count):
        chunks = bytearray()
        while len(chunks) < count:
            if not endpoint.tx[dict(endpoint.contract.in_endpoints)[endpoint_address]]:
                endpoint.host_in_token(endpoint_address)
            port = dict(endpoint.contract.in_endpoints)[endpoint_address]
            run_until('bulk IN reply', lambda: bool(endpoint.tx[port]), 30_000_000)
            chunks.extend(endpoint.host_in(endpoint_address, count - len(chunks)))
            run_for(200_000)
        return bytes(chunks)

    def control_out(payload):
        setup = bytes((0x43, 0, 0, 0, 0, 0, len(payload) & 0xff, len(payload) >> 8))
        endpoint.host_setup(setup)
        run_for(200_000)
        endpoint.host_out(0, payload)
        run_until('control OUT status', lambda: bool(endpoint.tx[0]), 30_000_000)
        status = endpoint.host_in(0x80, 64)
        if status:
            raise RuntimeError('control OUT status packet was not empty')
        run_for(200_000)

    def control_status(setup):
        endpoint.host_setup(setup)
        run_until('control status IN', lambda: bool(endpoint.tx[0]), 30_000_000)
        status = endpoint.host_in(0x80, 64)
        if status:
            raise RuntimeError('control status packet was not empty')
        run_for(200_000)

    def control_in(setup, count):
        endpoint.host_setup(setup)
        run_until('control IN data', lambda: bool(endpoint.tx[0]), 30_000_000)
        payload = endpoint.host_in(0x80, count)
        endpoint.host_out(0, b'')
        run_for(200_000)
        return payload

    def command(op, frame, reply_bytes, sid):
        control_out(bytes((op, 0, 0, 0, 0, 0, 0, 0,
                           len(frame) >> 8, len(frame) & 0xff, sid >> 8, sid & 0xff)))
        if frame:
            endpoint.host_out(2, frame)
            run_until('bulk OUT completion',
                      lambda: (endpoint.rx_positions[1] == len(endpoint.rx[1])
                               and endpoint.dma_active is None),
                      30_000_000)
            run_for(1_000_000)
        if reply_bytes is None:
            return b''
        opcode = int.from_bytes(frame[:4], 'little') if len(frame) >= 4 else 0
        if opcode in (0x1a0002, 0x200001):
            published = 0x124 if opcode == 0x1a0002 else 0x11c
            run_until('service reply publication',
                      lambda: int.from_bytes(board.read_memory(0x0146986c, 4), 'little') == published,
                      30_000_000)
        else:
            run_for(1_000_000)
        control_out(bytes((0xd1, 0, 0, 0, 0, 0, 0, 0,
                           reply_bytes >> 8, reply_bytes & 0xff, sid >> 8, sid & 0xff)))
        return take_in(0x81, reply_bytes)

    def words(*values):
        return b''.join(struct.pack('<I', value) for value in values)

    def debug_frame(function, address, count, payload=b''):
        return words(0x200001, 0, 0, 0, function, 0, 0, address, count) + payload

    def trace_contexts():
        _, tasks = task_state(Reader(board))
        stacks = []
        for task in tasks:
            if all(name in task for name in ('stack_base', 'stack_bytes', 'task_id')):
                base = int(task['stack_base'], 16)
                stacks.append((task['cpu'], base, base + task['stack_bytes'], task['task_id']))
        hits = []
        sites = {0x00d803cc: 'debug-dispatch', 0x00647b78: 'function-3',
                 0x00dc7874: 'stock-query-callback'}
        def handler(cpu, core, address):
            sp = cpu.reg_read(UC_ARM_REG_SP)
            task = next((task_id for owner, low, high, task_id in stacks
                         if owner == core and low <= sp < high), None)
            request = bytes(board.read_memory(0x01469a9c, 0x24))
            hits.append({'site': sites[address], 'address': hex(address), 'core': core,
                         'task': task, 'task_hex': None if task is None else hex(task),
                         'sp': hex(sp), 'instructions': board.instructions,
                         'request': {'opcode': hex(int.from_bytes(request[0:4], 'little')),
                                     'function_or_tag': hex(int.from_bytes(request[0x10:0x14], 'little')),
                                     'address': hex(int.from_bytes(request[0x1c:0x20], 'little')),
                                     'length': int.from_bytes(request[0x20:0x24], 'little')}})
        for core in range(2):
            board._native.set_breakpoints(core, sorted(sites))
        board.breakpoint_handler = handler
        return hits

    recovery = session.options.get('recovery', {})
    nand_path = Path(recovery['media'])
    with nand_path.open('rb') as source:
        nand_sha256 = hashlib.file_digest(source, 'sha256').hexdigest()
    receipt = {'schema': 1, 'scope': 'emulated X-M1 1.50 ordinary-boot ff80 service',
               'firmware_sha256': board.snapshot().get('firmware_sha256'),
               'installed_nand': {'sha256': nand_sha256, 'stored_f7': 1},
               'source': 'stock guest controller, IRQ, USB task and service handlers',
               'limits': ['Synthetic USB host timing and endpoint register reset values.',
                          'Shared emulator RAM is not a hardware cache-coherence model.']}
    run_until('first-start screen', lambda: board.instructions >= 100_000_000, 100_000_000)
    for key in ['up'] * 12 + ['ok', 'ok']:
        session.key({'key': key, 'hold_instructions': FIRST_START_PULSE,
                     'release_instructions': FIRST_START_PULSE})
    run_for(40_000_000)
    receipt['pre_attach'] = {'instructions': board.instructions,
                             'cp14': list(board._native.cp14_identity())}
    power.set_usb_connected(True)
    run_until('USB controller activation', lambda: endpoint.regs[0x090] & 6 == 6,
              140_000_000)
    endpoint.host_reset()
    run_for(1_000_000)

    device = control_in(bytes((0x80, 6, 0, 1, 0, 0, 18, 0)), 18)
    if len(device) != 18 or device[8:12] != bytes((0xcb, 0x04, 0x80, 0xff)):
        raise RuntimeError(f'unexpected device descriptor {device.hex()}')
    receipt['device_descriptor'] = {'vid': '0x04cb', 'pid': '0xff80',
                                    'reply': device.hex()}
    control_status(bytes((0, 5, 1, 0, 0, 0, 0, 0)))
    config_head = control_in(bytes((0x80, 6, 0, 2, 0, 0, 9, 0)), 9)
    config_bytes = int.from_bytes(config_head[2:4], 'little')
    if len(config_head) != 9 or not 9 <= config_bytes <= 256:
        raise RuntimeError(f'unexpected configuration descriptor head {config_head.hex()}')
    configuration = control_in(
        bytes((0x80, 6, 0, 2, 0, 0, config_bytes & 0xff, config_bytes >> 8)),
        config_bytes)
    control_status(bytes((0, 9, configuration[5], 0, 0, 0, 0, 0)))
    run_for(1_000_000)
    receipt['configuration_descriptor'] = configuration.hex()

    open_request = bytes((0xc0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0))
    control_out(open_request)
    opened = take_in(0x81, 8)
    sid = int.from_bytes(opened[:2], 'little')
    if opened[2:] != bytes((0, 0, 0, 0, 1, 0)):
        raise RuntimeError(f'unexpected C0 reply {opened.hex()}')
    receipt['c0'] = {'opcode': '0xc0', 'sid': sid, 'reply': opened.hex()}
    contexts = trace_contexts()

    identity_request = words(0x150000, 0, 0, 0, 4, 0, 0, 0)
    identity = command(0xd0, identity_request, 128, sid)
    if identity[:16] != words(0x150000, 0, 0, 0) or identity[16:26] != b'X\x00-\x00M\x001\x00\x00\x00':
        raise RuntimeError(f'unexpected identity reply {identity.hex()}')
    receipt['identity'] = {'tag': 4, 'reply': identity.hex()}

    config_request = words(0x1a0002, 0, 0, 0, 2, 1, 0, 0, 256)
    config = command(0xd0, config_request, 0x124, sid)
    if config[0x24 + 0xf7] != 1:
        raise RuntimeError(f'config f7 was {config[0x24 + 0xf7]:#x}')
    receipt['config'] = {'opcode': '0x1a0002', 'function': 2, 'window': 0, 'bytes': 256,
                         'reply_sha256': hashlib.sha256(config).hexdigest(),
                         'f7': config[0x24 + 0xf7]}

    ram_address, ram_count = 0x015c22a0, 32
    expected = bytes(board.read_memory(ram_address, ram_count))
    ram = command(0xd0, debug_frame(2, ram_address, ram_count), 0x24 + ram_count, sid)
    if ram[0x24:] != expected:
        raise RuntimeError('function-2 reply differs from guest memory')
    receipt['ram_read'] = {'address': hex(ram_address), 'bytes': ram_count,
                           'guest_memory_hex': expected.hex(), 'reply_hex': ram[0x24:].hex(),
                           'sha256': hashlib.sha256(expected).hexdigest()}

    scratch, payload = 0x015c22d0, b'XM1!'
    before = bytes(board.read_memory(scratch, len(payload)))
    write_reply = command(0xd0, debug_frame(3, scratch, len(payload), payload), 0x24, sid)
    readback = command(0xd0, debug_frame(2, scratch, len(payload)), 0x24 + len(payload), sid)
    if readback[0x24:] != payload or bytes(board.read_memory(scratch, len(payload))) != payload:
        raise RuntimeError('function-3 scratch write did not read back')
    receipt['ram_write'] = {'address': hex(scratch), 'before': before.hex(),
                            'written': payload.hex(), 'reply': write_reply.hex(),
                            'readback': readback[0x24:].hex()}

    query = command(0xd0, words(0x450006, 0, 0, 0, 0, 0, 0, 0), 32, sid)
    receipt['query'] = {'tag': 0, 'reply': query.hex()}
    receipt['contexts'] = contexts
    receipt['boundary'] = {'instructions': board.instructions,
                           'failure': board.failure,
                           'execution_error': session.execution_error,
                           'service_mode': int.from_bytes(board.read_memory(0x0146a0c0, 4), 'little')}
    output.mkdir(parents=True, exist_ok=True)
    target = output / 'ff80-session.json'
    target.write_text(json.dumps(receipt, indent=2) + '\n')
    return receipt


class TraceSites:
    """Pre-instruction breakpoints on both cores recording r0-r3, sp, lr and the site's memory
    windows; observation never writes. A site given an r0 records only entries with that r0.
    `hits` is append-only; `counts` counts every matching entry, including those past
    `recorded_per_site`."""
    REGISTERS = ('r0', 'r1', 'r2', 'r3', 'sp', 'lr')

    def __init__(self, board, recorded_per_site=None):
        self.board, self.recorded_per_site = board, recorded_per_site
        self.sites, self.hits, self.counts = {}, [], {}

    def arm(self, sites):
        """Replace the armed sites with `sites`: dicts of address, label and optional r0 and windows."""
        parsed = {}
        for site in sites:
            if (type(site) is not dict or not {'address', 'label'} <= set(site) <= {'address', 'label', 'r0', 'windows'}
                    or type(site['address']) is not int or type(site['label']) is not str
                    or type(site.get('r0', 0)) is not int):
                raise ValueError('a trace site takes an integer address, a label and an optional integer r0')
            windows = tuple(tuple(window) for window in site.get('windows', ()))
            if any(len(window) != 2 or any(type(value) is not int for value in window) for window in windows):
                raise ValueError('a trace window is an integer start and length')
            parsed[site['address']] = (site['label'], site.get('r0'), windows)
        self.sites = parsed
        for address in parsed:
            self.counts.setdefault(address, 0)
        for core in range(len(self.board.cpus)):
            self.board._native.set_breakpoints(core, sorted(parsed))
        self.board.breakpoint_handler = self._hit if parsed else None

    def specs(self):
        """The armed sites in `arm`'s input form."""
        return [{'address': address, 'label': label, **({} if r0 is None else {'r0': r0}),
                 **({'windows': [list(window) for window in windows]} if windows else {})}
                for address, (label, r0, windows) in self.sites.items()]

    def _hit(self, cpu, core, address):
        from unicorn import arm_const as arm
        label, wanted, windows = self.sites[address]
        if wanted is not None and cpu.reg_read(arm.UC_ARM_REG_R0) != wanted:
            return
        self.counts[address] += 1
        if self.recorded_per_site is not None and self.counts[address] > self.recorded_per_site:
            return
        hit = {'label': label, 'address': address, 'core': core,
               'registers': {name: cpu.reg_read(getattr(arm, 'UC_ARM_REG_' + name.upper()))
                             for name in self.REGISTERS},
               'instructions': self.board.instructions}
        if windows:
            hit['windows'] = {start: bytes(self.board.read_memory(start, length)).hex()
                              for start, length in windows}
        self.hits.append(hit)


def _run_toggle_server(board, session, output, start):
    """Serve primitive commands for the USB-toggle scenarios of emulation/UsbToggleEmu.hs: run,
    key pulses, key hold and release, raw ADC levels, cable level, memory reads, trace sites, enumeration without a
    descriptor expectation, endpoint transfers, captures, the access probe and the boot checkpoint."""
    from emulator.power_devices import PowerDevices
    power = next(device for device in board.devices if type(device) is PowerDevices)
    bridge = _ram_push_bridge(board, session, attached=power.usb_connected)
    trace = TraceSites(board)

    def boundary():
        machine = board.snapshot()
        return {'instructions': board.instructions, 'failure': board.failure,
                'execution_error': session.execution_error,
                'cores': [{'pc': core.get('pc'), 'lr': core.get('lr'), 'sp': core.get('sp'),
                           'recent': core.get('recent_instructions', [])[-12:]}
                          for core in machine.get('cores', [])]}

    def run(count, stop_on):
        seen, stopped, end = len(trace.hits), [None], board.instructions + count

        def done():
            nonlocal seen
            for hit in trace.hits[seen:]:
                if hit['label'] in stop_on:
                    stopped[0] = hit['label']
                    return True
            seen = len(trace.hits)
            return board.instructions >= end
        bridge.run_until('toggle run', done, count + 1)
        return stopped[0]

    print(json.dumps({'ok': True, 'event': 'ready', **start, 'instructions': board.instructions,
                      'usb_connected': power.usb_connected}, separators=(',', ':')), flush=True)
    for line in sys.stdin:
        try:
            request = json.loads(line, object_pairs_hook=_unique_object)
            command = request.get('command')
            if command == 'state':
                ready = session.inputs.availability()['ready']
                inputs = session.inputs.current_state(ready=ready)
                result = {'instructions': board.instructions, 'virtual_ticks': board.virtual_ticks,
                          'usb_connected': power.usb_connected,
                          'tick_ratio': f'{board.tick_ratio[0]}/{board.tick_ratio[1]}',
                          'inputs_ready': ready, 'asserted_keys': inputs['asserted_keys'],
                          'menu': bridge.key_state(session.sample(False))}
            elif command == 'run':
                count, stop_on = request.get('instructions'), request.get('stop_on', [])
                if (type(count) is not int or not 0 < count <= 4_000_000_000 or type(stop_on) is not list
                        or any(type(label) is not str for label in stop_on)):
                    raise ValueError('run takes 1..4000000000 instructions and a list of stop labels')
                result = {'stopped_on': run(count, set(stop_on)), 'instructions': board.instructions}
            elif command == 'release-context':
                if set(request) != {'command', 'segment'}:
                    raise ValueError('release-context requires exactly a segment')
                session.set_release_context(request['segment'])
                result = {'segment': request['segment']}
            elif command == 'key':
                options = {field: request[field] for field in ('key', 'hold_instructions', 'release_instructions')
                           if field in request}
                result = {'menu': bridge.key_state(session.key(options))}
            elif command == 'press':
                result = {'sample': session.inputs.press(request['key'])}
            elif command == 'release':
                result = {'sample': session.inputs.release()}
            elif command == 'adc':
                channel, sample = request.get('channel'), request.get('sample')
                if type(channel) is not int or type(sample) is not int or not 0 <= channel < 12 or not 0 <= sample <= 0xffff:
                    raise ValueError('adc takes a channel 0..11 and a sample 0..65535')
                samples = list(power.adc_samples)
                samples[channel] = sample
                power.set_adc_samples(samples, source=f'USB-toggle ADC channel {channel} level')
                result = {'samples': list(power.adc_samples)}
            elif command == 'cable':
                if type(request.get('connected')) is not bool:
                    raise ValueError('cable takes a boolean level')
                power.set_usb_connected(request['connected'])
                result = {'usb_connected': power.usb_connected, 'instructions': board.instructions}
            elif command == 'read-memory':
                address, count = request.get('address'), request.get('count')
                if (type(address) is not int or type(count) is not int or not 0 < count <= 4096
                        or not 0 <= address <= 0x100000000 - count):
                    raise ValueError('read-memory requires an address and 1..4096 bytes')
                result = {'bytes': bytes(board.read_memory(address, count)).hex()}
            elif command == 'trace-sites':
                trace.arm(request['sites'])
                result = {'sites': len(trace.sites)}
            elif command == 'trace-hits':
                first = request.get('from')
                if type(first) is not int or not 0 <= first <= len(trace.hits):
                    raise ValueError('trace-hits requires a hit index within the recorded hits')
                result = {'count': len(trace.hits), 'hits': trace.hits[first:]}
            elif command == 'enumerate':
                budget = request.get('budget')
                if type(budget) is not int or not 0 < budget <= 1_000_000_000:
                    raise ValueError('enumerate takes an activation budget of 1..1000000000 instructions')
                device, configuration = bridge.enumerate_attached(budget)
                result = {'device': device.hex(), 'configuration': configuration.hex(),
                          'instructions': board.instructions}
            elif command == 'transfer':
                count, payload = bridge.transfer(request)
                result = {'count': count, 'bytes': payload.hex()}
            elif command == 'capture':
                result = bridge.capture(request['label'], output)
            elif command == 'access-probe':
                state = board._native.access_probe_state()
                result = {'enabled': state['enabled'], 'violation': state['violation'],
                          'ranges': state.get('ranges'), 'records': list(state['records'])}
            elif command == 'save-checkpoint':
                if start.get('checkpoint_root') is None or type(request.get('replace')) is not bool:
                    raise ValueError('save-checkpoint needs --toggle-checkpoints and a boolean replace')
                bridge.drain_scheduled()
                sites = trace.specs()
                trace.arm([])
                try:
                    target = _save_ram_push_checkpoint(
                        session, Path(start['checkpoint_root']), start['key'], TOGGLE_RECORD,
                        {'key': start['key'], 'inputs': start['inputs'], 'instructions': board.instructions},
                        replace=request['replace'])
                finally:
                    trace.arm(sites)
                result = {'directory': str(target.resolve()), 'instructions': board.instructions}
            elif command == 'finish':
                print(json.dumps({'ok': True, 'finished': True}, separators=(',', ':')), flush=True)
                break
            else:
                raise ValueError('unknown USB-toggle command')
            print(json.dumps({'ok': True, **result}, separators=(',', ':')), flush=True)
        except Exception as error:
            try:
                stop = boundary()
            except Exception as observed:
                stop = {'unavailable': f'{type(observed).__name__}: {observed}'}
            print(json.dumps({'ok': False, 'error': type(error).__name__, 'message': str(error),
                              'boundary': stop}, separators=(',', ':'), default=str), flush=True)
    target = Path(output) / 'emulator-session.json'
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps({
        'schema': 1, 'scope': 'emulated X-M1 USB-toggle primitive session', 'start': start,
        'usb_endpoint': bridge.endpoint.snapshot(), 'timing_transitions': bridge.timing_transitions,
        'transfer_log': bridge.transfer_log, 'screenshots': bridge.screenshots,
        'trace_hits': len(trace.hits), 'boundary': boundary()}, indent=2, default=str) + '\n')


def _console_window(field):
    start, _, length = field.partition(':')
    start, length = int(start, 0), int(length, 0)
    if not 0 <= start <= 0xffffffff or not 1 <= length <= 0x1000:
        raise ValueError('Window is ADDRESS:LENGTH with 1..0x1000 bytes')
    return start, length


def _console_hit(hit):
    return {'address': hex(hit['address']), 'core': hit['core'], 'instructions': hit['instructions'],
            'registers': {name: hex(value) for name, value in hit['registers'].items()},
            'windows': {hex(start): data for start, data in hit.get('windows', {}).items()}}


def main():
    from emulator.boot import make_board
    from emulator.server import make_menu_server as make_server
    from emulator.image_parameter_devices import UIConversionProfile
    parser = argparse.ArgumentParser(description=__doc__)
    starting = parser.add_mutually_exclusive_group(required=True)
    starting.add_argument('--profile', type=Path, help='Cold boot using the selected options JSON')
    starting.add_argument('--snapshot', type=Path, help='Restore a portable snapshot into fresh native engines')
    starting.add_argument('--recovery-ram', type=Path, help='Import generated bootstrap RAM and its observed .state.json CPU0 companion into cold engines')
    parser.add_argument('--cold-profile', type=Path, default=ROOT / 'firmware/x-m1/1.50/emulator-rec.json',
                        help='Independent cold-board options/profile for --recovery-ram')
    parser.add_argument('--rom-card', type=Path, help='Complete recovery SD image for a direct cold ROM attempt')
    parser.add_argument('--sd-image', type=Path, help='Complete updater/recovery card retained by the RAM continuation')
    parser.add_argument('--rom-route', choices=('fat', 'raw'), default='fat')
    installed = parser.add_mutually_exclusive_group()
    installed.add_argument('--installed-media', type=Path, help='Independent installed logical NAND data, including damaged application bytes')
    installed.add_argument('--installed-nand', type=Path, help='Committed NAND image from an earlier modeled run; not a machine checkpoint')
    installed.add_argument('--installed-dat', type=Path,
                           help='X-M1 1.50 updater container installed into the modeled NAND extent')
    parser.add_argument('--adjustment-nand', type=Path,
                        help='Complete committed adjustment blocks layered over installed logical media')
    parser.add_argument('--menu-sd-image', type=Path,
                        help='Ordinary-boot virtual SD image for menu operations')
    parser.add_argument('--export-nand', type=Path, help='Save committed installed NAND to a fresh file after this run')
    parser.add_argument('--export-sd', type=Path, help='Save logical SD bytes and guest write overlays to a fresh image')
    budget = parser.add_mutually_exclusive_group()
    budget.add_argument('--pause-after', type=int, help='Absolute instruction threshold; default60M for cold boot, zero for restore')
    budget.add_argument('--instructions', type=int, help='Instructions to run from the selected starting state')
    parser.add_argument('--settings', choices=('default', 'owner'), help='Persistent settings selection; verify restored selection')
    parser.add_argument('--save-snapshot', type=Path, help='Save drained machine state to a fresh local directory')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--serve', action='store_true')
    parser.add_argument('--ff80-rehearsal', action='store_true',
                        help='Run the closed ordinary-boot ff80 endpoint session and write its receipt')
    parser.add_argument('--menus', action='store_true',
                        help='Enable declared ADC button input and stock menu navigation')
    parser.add_argument('--ram-push-server', action='store_true',
                        help='Serve the closed emulator-backed UsbApi line protocol')
    parser.add_argument('--ram-push-checkpoints', type=Path,
                        help='Directory of RAM-push attach checkpoints, one per --checkpoint-key')
    parser.add_argument('--checkpoint-key', metavar='SHA256',
                        help='--ram-push-checkpoints/--toggle-server: the launcher\'s checkpoint key')
    parser.add_argument('--checkpoint-inputs', type=json.loads, metavar='JSON',
                        help='--checkpoint-key: the key document recorded with each checkpoint')
    parser.add_argument('--fresh-boot', action='store_true',
                        help='Cold boot even when the keyed RAM-push checkpoint exists, then replace it')
    parser.add_argument('--attach-hold-key',
                        help='--ram-push-server: hold this menu key across the attach edge')
    parser.add_argument('--toggle-server', action='store_true',
                        help='Serve the primitive USB-toggle scenario commands')
    parser.add_argument('--toggle-checkpoints', type=Path,
                        help='--toggle-server: directory of boot checkpoints, one per --checkpoint-key')
    parser.add_argument('--cable-at-reset', action='store_true',
                        help='--toggle-server: raise the USB cable level before the first instruction')
    parser.add_argument('--adc-at-reset', metavar='CHANNEL=SAMPLE',
                        help='--toggle-server: hold one ADC channel at SAMPLE from the first instruction')
    parser.add_argument('--access-probe-range', metavar='LOW:HIGH',
                        help='--toggle-server: record every access to [LOW, HIGH) from the first instruction')
    parser.add_argument('--port', type=int, default=0)
    parser.add_argument('--bind', default='127.0.0.1')
    parser.add_argument('--hostname')
    parser.add_argument('--ui-conversion-profile', type=UIConversionProfile.parse,
                        choices=tuple(UIConversionProfile),
                        help='Opt-in stock CPU conversion contract for a cold boot; a checkpoint keeps its own')
    parser.add_argument('--fixed-scene-frame', type=int,
                        help='Pin every scene render to this positive frame index for the run')
    args = parser.parse_args()
    if args.fixed_scene_frame is not None:
        from emulator.virtual_source import select_fixed_scene_frame
        select_fixed_scene_frame(args.fixed_scene_frame)
    installed_input = args.installed_media or args.installed_nand or args.installed_dat
    recovery_start = args.recovery_ram is not None or args.rom_card is not None
    if recovery_start and (args.snapshot is not None or installed_input is None):
        parser.error('Recovery requires a cold board and independent installed storage')
    if args.recovery_ram is not None and args.rom_card is not None:
        parser.error('Select direct ROM card execution or exported RAM continuation, not both')
    if args.snapshot is not None and installed_input is not None:
        parser.error('Installed storage inputs belong to independent cold boards, not machine checkpoints')
    if args.sd_image is not None and args.recovery_ram is None:
        parser.error('--sd-image belongs to --recovery-ram; use --rom-card for direct ROM execution')
    if args.adjustment_nand is not None and not (args.installed_media or args.installed_dat):
        parser.error('--adjustment-nand requires --installed-media or --installed-dat')
    if args.menu_sd_image is not None and not args.ram_push_server:
        parser.error('--menu-sd-image belongs to --ram-push-server')
    for value in (args.pause_after, args.instructions):
        if value is not None and not 0 <= value <= 100_000_000:
            parser.error('Instruction threshold/budget must be 0..100000000')
    if args.ram_push_server and (
            args.snapshot is not None or args.installed_dat is None
            or args.adjustment_nand is None or args.menu_sd_image is None
            or not args.menus or args.ff80_rehearsal
            or args.pause_after is not None or args.instructions is not None
            or args.serve or args.save_snapshot is not None):
        parser.error('--ram-push-server requires --profile, --installed-dat, --adjustment-nand, --menu-sd-image and --menus without another run mode')
    if args.ram_push_checkpoints is not None and not args.ram_push_server:
        parser.error('--ram-push-checkpoints belongs to --ram-push-server')
    if args.fresh_boot and args.ram_push_checkpoints is None:
        parser.error('--fresh-boot requires --ram-push-checkpoints')
    if args.attach_hold_key is not None and not args.ram_push_server:
        parser.error('--attach-hold-key belongs to --ram-push-server')
    if args.toggle_server and (
            args.snapshot is not None or args.installed_dat is None or args.adjustment_nand is None
            or args.menu_sd_image is not None or not args.menus or args.ff80_rehearsal
            or args.ram_push_server or args.pause_after is not None or args.instructions is not None
            or args.serve or args.save_snapshot is not None):
        parser.error('--toggle-server requires --profile, --installed-dat, --adjustment-nand and --menus without another run mode')
    if not args.toggle_server and (args.toggle_checkpoints is not None or args.cable_at_reset
                                   or args.adc_at_reset is not None or args.access_probe_range is not None):
        parser.error('--toggle-checkpoints, --cable-at-reset, --adc-at-reset and --access-probe-range belong to --toggle-server')
    keyed = args.ram_push_checkpoints is not None or args.toggle_server
    if keyed != (args.checkpoint_key is not None) or keyed != (args.checkpoint_inputs is not None):
        parser.error('--checkpoint-key and --checkpoint-inputs go with --ram-push-checkpoints or --toggle-server')
    if keyed and (len(args.checkpoint_key) != 64 or any(c not in '0123456789abcdef' for c in args.checkpoint_key)):
        parser.error('--checkpoint-key must be a lowercase SHA-256')
    adc_at_reset = probe_range = None
    try:
        if args.adc_at_reset is not None:
            channel, sample = (int(field, 0) for field in args.adc_at_reset.split('='))
            if not 0 <= channel < 12 or not 0 <= sample <= 0xffff:
                raise ValueError
            adc_at_reset = [channel, sample]
        if args.access_probe_range is not None:
            low, high = (int(field, 0) for field in args.access_probe_range.split(':'))
            if not 0 <= low < high:
                raise ValueError
            probe_range = (low, high)
    except ValueError:
        parser.error('--adc-at-reset takes CHANNEL=SAMPLE and --access-probe-range takes LOW:HIGH')
    output = args.output or ROOT / '.tools/rec-scanout' / (
        datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ') + '-' + uuid4().hex[:8])
    checkpoint = None
    if args.ram_push_checkpoints is not None:
        key, key_inputs = args.checkpoint_key, args.checkpoint_inputs
        directory = args.ram_push_checkpoints / key
        checkpoint = {'key': key, 'inputs': key_inputs, 'directory': str(directory.resolve())}
        installed = _install_checkpoints(args.ram_push_checkpoints, key)
        print(json.dumps({'ok': True, 'event': 'start-options', 'boot_key': key, 'installed': installed},
                         separators=(',', ':')), flush=True)
        choice = json.loads(sys.stdin.readline(), object_pairs_hook=_unique_object)
        if set(choice) != {'command', 'installed'} or choice['command'] != 'start':
            raise ValueError('RAM-push start requires {"command": "start", "installed": DIRECTORY or null}')
        if choice['installed'] is not None:
            if choice['installed'] not in [item['directory'] for item in installed]:
                raise ValueError('RAM-push start names no listed install checkpoint')
            started = perf_counter()
            directory = Path(choice['installed'])
            record = _read_ram_push_record(directory, INSTALL_RECORD, key)
            board, session, bridge, attachment = _restore_ram_push_checkpoint(directory, record, output)
            checkpoint.update(start='installed', installed=str(directory),
                              restore_seconds=round(perf_counter() - started, 3))
            _run_ram_push_server(board, session, output, bridge, attachment, checkpoint,
                                 args.ram_push_checkpoints)
            return
        if directory.is_dir() and not args.fresh_boot:
            started = perf_counter()
            record = _read_ram_push_record(directory, ATTACH_RECORD, key)
            board, session, bridge, attachment = _restore_ram_push_checkpoint(directory, record, output)
            checkpoint.update(start='restored', restore_seconds=round(perf_counter() - started, 3))
            _run_ram_push_server(board, session, output, bridge, attachment, checkpoint,
                                 args.ram_push_checkpoints)
            return
        checkpoint['start'] = 'fresh-boot' if args.fresh_boot else 'cold'
        boot_started = perf_counter()
    toggle_start = None
    if args.toggle_server:
        key, key_inputs = args.checkpoint_key, args.checkpoint_inputs
        root = None if args.toggle_checkpoints is None else args.toggle_checkpoints.resolve()
        saved = root is not None and probe_range is None and (root / key).is_dir()
        toggle_start = {'key': key, 'inputs': key_inputs, 'checkpoint_root': None if root is None else str(root),
                        'access_probe_range': None if probe_range is None else list(probe_range)}
        print(json.dumps({'ok': True, 'event': 'start-options', 'key': key, 'inputs': key_inputs,
                          'checkpoint': str(root / key) if saved else None}, separators=(',', ':')), flush=True)
        choice = json.loads(sys.stdin.readline(), object_pairs_hook=_unique_object)
        if set(choice) != {'command', 'restore'} or choice['command'] != 'start' or type(choice['restore']) is not bool:
            raise ValueError('USB-toggle start requires {"command": "start", "restore": BOOLEAN}')
        if choice['restore']:
            if not saved:
                raise ValueError('USB-toggle start names no saved checkpoint')
            from x70_native.portable_checkpoint import load
            from tools.probe_setup_menus import MenuSession
            started = perf_counter()
            record = _read_ram_push_record(root / key, TOGGLE_RECORD, key)
            board = load(root / key)
            session = MenuSession(board, output, options=dict(board._portable_factory_options,
                                                               recovery=record['recovery']))
            toggle_start.update(start='restored', restore_seconds=round(perf_counter() - started, 3))
            _run_toggle_server(board, session, output, toggle_start)
            return
        toggle_start['start'] = 'cold'
    if args.snapshot is not None:
        from x70_native.portable_checkpoint import load
        board = load(args.snapshot)
        options = board._portable_factory_options
        if args.settings is not None and options.get("settings", "default") != args.settings:
            parser.error("Snapshot settings selection differs; cold boot the requested settings")
    else:
        board, options = cold_board(args, make_board, parser.error, output)
    if args.instructions is not None:
        threshold = board.instructions + args.instructions if args.instructions else 0
    else:
        threshold = args.pause_after if args.pause_after is not None else (0 if args.snapshot else 60_000_000)
    if threshold and threshold < board.instructions:
        parser.error('Absolute threshold precedes the saved machine; use --instructions for continuation')
    if args.menus:
        from tools.probe_setup_menus import MenuSession
        session = MenuSession(board, output, options=options)
    else:
        session = WarmScanoutSession(board, output, options=options)
    if args.ram_push_server:
        bridge, attachment = _ram_push_attach(board, session, args.attach_hold_key)
        if checkpoint is not None:
            from emulator.checkpoint import CheckpointError
            checkpoint['boot_seconds'] = round(perf_counter() - boot_started, 3)
            started = perf_counter()
            bridge.drain_scheduled()
            try:
                _save_ram_push_checkpoint(session, args.ram_push_checkpoints, checkpoint['key'],
                                          ATTACH_RECORD, {'key': checkpoint['key'],
                                                          'inputs': checkpoint['inputs'], **attachment},
                                          replace=args.fresh_boot)
                checkpoint['saved'] = True
            except CheckpointError as error:
                checkpoint.update(saved=False, save_error=str(error))
            checkpoint['save_seconds'] = round(perf_counter() - started, 3)
        _run_ram_push_server(board, session, output, bridge, attachment, checkpoint,
                             args.ram_push_checkpoints)
        return
    if args.toggle_server:
        if board.instructions != 0:
            raise ValueError('USB-toggle reset inputs need an unstarted board')
        power = next(device for device in board.devices if type(device).__name__ == 'PowerDevices')
        if args.cable_at_reset:
            power.set_usb_connected(True)
        if adc_at_reset is not None:
            samples = list(power.adc_samples)
            samples[adc_at_reset[0]] = adc_at_reset[1]
            power.set_adc_samples(samples, source=f'USB-toggle reset input: ADC channel {adc_at_reset[0]}')
        if probe_range is not None:
            board._native.access_probe_begin([probe_range], limit=100_000)
        _run_toggle_server(board, session, output, toggle_start)
        return
    if args.ff80_rehearsal:
        if (args.snapshot is not None or args.installed_nand is None or not args.menus
                or args.pause_after is not None or args.instructions is not None
                or args.serve or args.save_snapshot is not None):
            parser.error('--ff80-rehearsal requires --profile, --installed-nand and --menus without another run mode')
        receipt = _run_ff80_rehearsal(board, session, output)
        print(json.dumps(receipt, separators=(',', ':')), flush=True)
        return
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
    if args.export_nand is not None:
        print(json.dumps(session.save_nand(args.export_nand)), flush=True)
    if args.export_sd is not None:
        print(json.dumps(session.save_sd(args.export_sd)), flush=True)
    if args.serve:
        from emulator.server import make_menu_server
        factory = make_menu_server if args.menus else make_server
        server = factory(session, args.port, page='boot.html', bind=args.bind, hostname=args.hostname)
        print(f'Warm scanout viewer: http://{args.hostname or args.bind}:{server.server_port}/', flush=True)
        server.serve_forever()
        return
    print('Commands: sample, status, run COUNT, checkpoint, snapshot DIRECTORY, nand FILE, sd FILE, restore,'
          ' read ADDRESS LENGTH, watch ADDRESS [ADDRESS:LENGTH...], hits, nandstat, quit. No camera I/O.', flush=True)
    if args.menus:
        print('Menu input: key ok|back|q|up|down|left|right [HOLD RELEASE]. Stock ADC scanning/debounce.', flush=True)
    trace, reported = TraceSites(board, recorded_per_site=64), 0
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
            elif len(fields) == 2 and fields[0] == 'nand':
                value = session.save_nand(fields[1])
            elif len(fields) == 2 and fields[0] == 'sd':
                value = session.save_sd(fields[1])
            elif fields == ['restore']:
                value = session.run({'restore': True})
            elif len(fields) == 2 and fields[0] == 'run':
                value = session.run({'limit': int(fields[1], 0)})
            elif args.menus and len(fields) in (2, 4) and fields[0] == 'key':
                options = {'key': fields[1]}
                if len(fields) == 4:
                    options.update(hold_instructions=int(fields[2], 0), release_instructions=int(fields[3], 0))
                value = session.key(options)
            elif len(fields) == 3 and fields[0] == 'read':
                start, length = _console_window(fields[1] + ':' + fields[2])
                value = {'address': hex(start), 'hex': bytes(board.read_memory(start, length)).hex()}
            elif len(fields) >= 2 and fields[0] == 'watch':
                address = int(fields[1], 0)
                trace.arm([site for site in trace.specs() if site['address'] != address]
                          + [{'address': address, 'label': hex(address),
                              'windows': [_console_window(field) for field in fields[2:]]}])
                value = {'watching': [hex(site) for site in sorted(trace.sites)]}
            elif fields == ['hits']:
                value = {'hits': [_console_hit(hit) for hit in trace.hits[reported:]],
                         'counts': {hex(site): count for site, count in sorted(trace.counts.items())}}
                reported = len(trace.hits)
            elif fields == ['nandstat']:
                value = {'busy': board.nand.busy, 'transactions': dict(board.nand.transaction_counts)}
            else:
                raise ValueError('Unknown command')
            if fields != ['status'] and isinstance(value, dict) and 'instructions' in value and 'display' in value:
                value = console_summary(value)
            print(json.dumps(value, separators=(',', ':')), flush=True)
        except Exception as error:
            print(json.dumps({'error': type(error).__name__, 'message': str(error)}), flush=True)


if __name__ == '__main__':
    main()
