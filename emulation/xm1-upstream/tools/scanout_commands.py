"""The RAM-push bridge protocol over a booted X-M1 board.

The attach module boots and attaches the guest up to the saved boundary; this
module serves the commands the Haskell e2e sends from there: opaque endpoint
transfers with the rendezvous holds and keepalive, key and dial input, trace
sites, captures, and the post-INSTALL checkpoint. An install checkpoint names
the procedure that pushed it, so a change here invalidates that checkpoint
while the attach checkpoint it extends still restores.
"""
import hashlib
import json
import sys
from pathlib import Path
from time import perf_counter

from tools.scanout_attach import (_FF80EndpointBridge, INSTALL_RECORD, _read_ram_push_record,
                                  _save_ram_push_checkpoint, _unique_object)

MODE_DIAL_SETTLE = 40_000_000


class _FF80CommandBridge(_FF80EndpointBridge):
    """Serves the RAM-push command protocol over the endpoint."""

    # A command-dial push in stock 1.50 live view leaves the screen and the MENU path unchanged
    # (emulated).
    KEEPALIVE_KEY = 'dial-push'
    # Each keepalive runs inside a host transfer's 5 s budget.
    KEEPALIVE_PULSE = 2_000_000

    def keep_session_awake(self):
        self.use_timing_ratio(24, 'inactivity reset')
        before = self.board.instructions
        state = self.session.key({'key': self.KEEPALIVE_KEY, 'render': False,
                                  'hold_instructions': self.KEEPALIVE_PULSE,
                                  'release_instructions': self.KEEPALIVE_PULSE})
        if self.board.failure is not None or self.session.execution_error is not None:
            raise RuntimeError(f'inactivity reset failed: {state}')
        self.keepalive_inputs.append({
            'key': self.KEEPALIVE_KEY, 'before_instructions': before,
            'after_instructions': self.board.instructions})
        self.use_timing_ratio(2400, 'USB transfer')

    # A 455,645-instruction UP hold at the USB ratio did not enter stock 1.50 focus-area selection;
    # 1,778,831 did (emulated).
    RENDEZVOUS_HOLD = 4_000_000

    def hold_rendezvous_inputs(self):
        from emulator.menu_inputs import MenuKey
        from emulator.sd_card import MediaAction
        self.session.inputs.press(MenuKey.UP)
        self.session._sd_controller().media_control(MediaAction.PROTECT)
        return self.board.instructions

    def release_rendezvous_inputs(self, latched):
        from emulator.sd_card import MediaAction
        held = self.board.instructions - latched
        if held < self.RENDEZVOUS_HOLD:
            self.run_for(self.RENDEZVOUS_HOLD - held)
        self.session.inputs.release()
        self.session._sd_controller().media_control(MediaAction.UNPROTECT)

    def release_rendezvous_activity(self):
        if not self.rendezvous_activity_complete:
            return
        self.release_rendezvous_inputs(self.rendezvous_activity[-1]['latched_instructions'])
        event = self.rendezvous_activity[-1]
        event['released_instructions'] = self.board.instructions
        event['menu_after'] = self.key_state(self.session.sample(False))
        event['cores_after'] = self.board.snapshot()['cores']
        self.rendezvous_activity_complete = False

    def bulk_out(self, endpoint_address, payload):
        if endpoint_address != 2:
            raise ValueError('RAM-push bridge admits only bulk OUT endpoint 2')
        self.run_until('bulk OUT DMA readiness',
                       lambda: (self.endpoint.dma_active is None
                                or (self.endpoint.dma_active['direction'] == 'out'
                                    and self.endpoint.dma_active['exact'] == len(payload))),
                       30_000_000, self.READY_STEP)
        for start in range(0, len(payload), self.endpoint.contract.packet_bytes):
            chunk = payload[start:start + self.endpoint.contract.packet_bytes]
            self.endpoint.host_out(endpoint_address, chunk)
            self.run_until('bulk OUT packet consumption',
                           lambda: self.endpoint.rx_positions[1] == len(self.endpoint.rx[1]),
                           30_000_000, self.READY_STEP)
            self.run_for(20_000)
        self.run_until('bulk OUT DMA completion',
                       lambda: self.endpoint.dma_active is None,
                       30_000_000, self.READY_STEP)
        self.run_until('bulk OUT DMA teardown',
                       lambda: all(not channel[0x004] & 1 for channel in self.endpoint.dma),
                       30_000_000, self.READY_STEP)
        self.run_for(100_000)

    def transfer(self, request):
        started, instructions = perf_counter(), self.board.instructions
        count, payload = self._transfer(request)
        frame = bytes.fromhex(request.get('bytes', ''))
        self.transfer_log.append({
            'transfer': request.get('transfer'), 'bytes': count,
            'opcode': frame[:4][::-1].hex() if len(frame) >= 4 else None,
            'function': int.from_bytes(frame[16:20], 'little') if len(frame) >= 20 else None,
            'instructions': self.board.instructions - instructions,
            'seconds': round(perf_counter() - started, 4)})
        return count, payload

    def _transfer(self, request):
        self.release_rendezvous_activity()
        self.transfer_count += 1
        if self.keepalive_enabled and self.transfer_count % 32 == 0:
            self.keep_session_awake()
        self.use_timing_ratio(2400, 'USB transfer')
        kind = request.get('transfer')
        if kind == 'control':
            payload = bytes.fromhex(request['bytes'])
            if len(payload) != 12:
                raise ValueError('control transfer must carry the internal twelve-byte request')
            self.control_out(payload)
            return len(payload), b''
        if kind == 'bulk-out':
            payload = bytes.fromhex(request['bytes'])
            self.bulk_out(request['endpoint'], payload)
            return len(payload), b''
        if kind == 'bulk-in':
            count = request['count']
            payload = self.take_in(request['endpoint'], count)
            return len(payload), payload
        raise ValueError('unknown bridge transfer kind')

    def set_trace_sites(self, sites, enable_keepalive=True):
        from emulator.rec_state import Reader, task_state
        from unicorn.arm_const import (UC_ARM_REG_CPSR, UC_ARM_REG_PC, UC_ARM_REG_R0,
                                       UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3,
                                       UC_ARM_REG_SP)
        if not isinstance(sites, dict) or any(not isinstance(key, str)
                or not isinstance(value, str) for key, value in sites.items()):
            raise ValueError('trace sites must be a string map')
        parsed = {int(address, 0): label for address, label in sites.items()}
        _, tasks = task_state(Reader(self.board))
        stacks = []
        for task in tasks:
            if all(name in task for name in ('stack_base', 'stack_bytes', 'task_id')):
                base = int(task['stack_base'], 16)
                stacks.append((task['cpu'], base, base + task['stack_bytes'], task['task_id']))
        self.trace_sites = parsed


        def core_states():
            states = []
            for owner, owner_cpu in enumerate(self.board.cpus):
                stack = owner_cpu.reg_read(UC_ARM_REG_SP)
                task_id = next((task for cpu_id, low, high, task in stacks
                                if cpu_id == owner and low <= stack < high), None)
                states.append({
                    'core': owner, 'pc': hex(owner_cpu.reg_read(UC_ARM_REG_PC)),
                    'cpsr': hex(owner_cpu.reg_read(UC_ARM_REG_CPSR)),
                    'irq_masked': bool(owner_cpu.reg_read(UC_ARM_REG_CPSR) & 0x80),
                    'sp': hex(stack), 'task': task_id,
                    'task_hex': None if task_id is None else hex(task_id),
                    'retired_instructions': self.board.core_state[owner]['instructions'],
                    'virtual_ticks': self.board.virtual_ticks})
            return states
        def handler(cpu, core, address):
            sp = cpu.reg_read(UC_ARM_REG_SP)
            task = next((task_id for owner, low, high, task_id in stacks
                         if owner == core and low <= sp < high), None)
            identities = list(self.board._native.cp14_identity())
            request = bytes(self.board.read_memory(0x01469a9c, 0xa4))
            self.trace_hits.append({
                'site': self.trace_sites[address], 'address': hex(address), 'core': core,
                'task': task, 'task_hex': None if task is None else hex(task),
                'registers': {name: hex(cpu.reg_read(register)) for name, register in
                              (('r0', UC_ARM_REG_R0), ('r1', UC_ARM_REG_R1),
                               ('r2', UC_ARM_REG_R2), ('r3', UC_ARM_REG_R3))},
                'raw_identity': hex(identities[core]), 'all_raw_identities': list(map(hex, identities)),
                'sp': hex(sp), 'instructions': self.board.instructions,
                'request': {'opcode': hex(int.from_bytes(request[0:4], 'little')),
                            'tag_or_function': hex(int.from_bytes(request[0x10:0x14], 'little')),
                            'tail_sha256': hashlib.sha256(request[0x84:0xa4]).hexdigest()}})
            label = self.trace_sites[address]
            if label == 'rendezvous-locked' and (
                    not self.rendezvous_activity
                    or 'released_instructions' in self.rendezvous_activity[-1]):
                locked = core_states()
                if core != 0 or any(not state['irq_masked'] for state in locked):
                    raise RuntimeError(f'rendezvous trace lacks two masked cores: {locked}')
                latched = self.hold_rendezvous_inputs()
                self.rendezvous_activity.append({
                    'latched_instructions': latched,
                    'latched_virtual_ticks': self.board.virtual_ticks,
                    'inputs': ['menu-up', 'sd-write-protect'],
                    'cores_locked': locked,
                    'window_exits': []})
            elif (label in ('cpu0-window-exit', 'cpu1-window-exit')
                  and self.rendezvous_activity
                  and 'callback_instructions' not in self.rendezvous_activity[-1]):
                self.rendezvous_activity[-1]['window_exits'].append({
                    'label': label, 'global_instructions': self.board.instructions,
                    'virtual_ticks': self.board.virtual_ticks, 'cores': core_states()})
            elif label == 'query-callback-result' and self.rendezvous_activity:
                event = self.rendezvous_activity[-1]
                if 'callback_instructions' not in event:
                    labels = [item['label'] for item in event['window_exits']]
                    if labels.count('cpu0-window-exit') != 1 or labels.count('cpu1-window-exit') != 1:
                        raise RuntimeError(f'incomplete rendezvous window trace: {labels}')
                    event['callback_instructions'] = self.board.instructions
                    event['callback_virtual_ticks'] = self.board.virtual_ticks
                    event['cores_at_callback'] = core_states()
                    event['retired_instruction_deltas'] = [
                        after['retired_instructions'] - before['retired_instructions']
                        for before, after in zip(event['cores_locked'], event['cores_at_callback'])]
                    event['virtual_tick_delta'] = (
                        event['callback_virtual_ticks'] - event['latched_virtual_ticks'])
                    if any(delta <= 0 for delta in event['retired_instruction_deltas']):
                        raise RuntimeError(
                            f'rendezvous lacks two-core execution: {event["retired_instruction_deltas"]}')
                    self.rendezvous_activity_complete = True
        for core in range(2):
            self.board._native.set_breakpoints(core, sorted(parsed))
        self.board.breakpoint_handler = handler
        self._breakpoint_handler = handler
        if enable_keepalive:
            self.transfer_count = 0
            self.keepalive_enabled = True

    def prepare_install_checkpoint(self):
        """Release held rendezvous inputs, return to the UI tick ratio and drain scheduled
        closures, whether or not the install checkpoint is then saved."""
        self.release_rendezvous_activity()
        self.use_timing_ratio(24, 'install checkpoint')
        self.drain_scheduled()

    def disarm_for_checkpoint(self):
        """Clear the trace breakpoints, which portable checkpoints cannot carry."""
        for core in range(2):
            self.board._native.set_breakpoints(core, [])
        self.board.breakpoint_handler = None

    def rearm(self):
        for core in range(2):
            self.board._native.set_breakpoints(core, sorted(self.trace_sites))
        self.board.breakpoint_handler = self._breakpoint_handler

    def key(self, name, hold_instructions=None, release_instructions=None):
        self.use_timing_ratio(24, 'UI input')
        started, instructions = perf_counter(), self.board.instructions
        options = {'key': name, 'render': True}
        if hold_instructions is not None:
            options.update(hold_instructions=hold_instructions, release_instructions=release_instructions)
        state = self.session.key(options)
        settle = state.get('navigation', {}).get('render_settle') or {}
        latched = getattr(self.session, 'last_key_latched', None)
        self.key_log.append({'key': name, 'seconds': round(perf_counter() - started, 3),
                             'instructions': self.board.instructions - instructions,
                             'key_instructions': settle.get('key_instructions'),
                             'settle_instructions': settle.get('settle_instructions'),
                             'settle_status': settle.get('status'), 'latched': latched})
        if latched is False:
            raise RuntimeError(f'{name} press was not latched: its key row read released when the '
                               f'hold ended, at instruction {self.board.instructions}')
        current = state.get('navigation', {}).get('current_menu', {})
        return {key: current.get(key) for key in ('menu_id', 'setting_id', 'cursor')}

    def mode_dial(self, name):
        self.use_timing_ratio(24, 'UI input')
        window = self.session.inputs.set_mode_dial(name)
        self.run_for(MODE_DIAL_SETTLE)
        return {'position': window.binding.position.value, 'sample': window.midpoint,
                'low': window.low, 'high': window.high,
                'menu': self.key_state(self.session.sample(False))}

    def capture(self, label, output):
        if not label or any(character not in 'abcdefghijklmnopqrstuvwxyz0123456789-' for character in label):
            raise ValueError('screenshot label must be lowercase ASCII')
        state = self.session.sample()
        if not self.session.png:
            raise RuntimeError('no display plane to capture')
        target = Path(output) / 'screenshots' / f'{label}.png'
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(self.session.png)
        frame = state.get('display', {}).get('last_frame') or {}
        self.screenshots.append({'label': label, 'path': str(target),
                                 'frame_number': frame.get('frame_number'),
                                 'sha256': hashlib.sha256(self.session.png).hexdigest(),
                                 'menu': self.key_state(state)})
        return self.screenshots[-1]


def _ram_push_bridge(board, session, *, attached=False):
    """The command bridge for a board that booted or restored the attach boundary."""
    return _FF80CommandBridge(board, session, attached=attached)


def _restore_ram_push_checkpoint(directory, record, output):
    from x70_native.portable_checkpoint import load
    from tools.probe_setup_menus import MenuSession
    board = load(directory)
    options = dict(board._portable_factory_options, recovery=record['recovery'])
    session = MenuSession(board, output, options=options)
    bridge = _ram_push_bridge(board, session, attached=True)
    attachment = {name: record[name] for name in ('pre_attach', 'device', 'configuration')}
    return board, session, bridge, attachment


def _install_checkpoint_name(key, manifest_sha256, runtime_sha256, procedure_sha256):
    digest = hashlib.sha256(
        f'{manifest_sha256}:{runtime_sha256}:{procedure_sha256}'.encode()).hexdigest()
    return f'{key}-installed-{digest}'


def _install_checkpoints(root, key):
    """Install checkpoints saved over the attach checkpoint KEY, with their host records."""
    found = []
    for directory in sorted(root.glob(f'{key}-installed-*')):
        if directory.is_dir() and (directory / INSTALL_RECORD).is_file():
            record = _read_ram_push_record(directory, INSTALL_RECORD, key)
            found.append({'directory': str(directory.resolve()), 'base': record['base'],
                          'manifest_sha256': record['manifest_sha256'],
                          'runtime_sha256': record['runtime_sha256'],
                          'procedure_sha256': record['procedure_sha256'],
                          'host': record['host']})
    return found


INSTALL_FIELDS = ('base', 'manifest_sha256', 'runtime_sha256', 'procedure_sha256', 'host')


def _save_install_checkpoint(bridge, session, root, checkpoint, attachment, request):
    from emulator.checkpoint import CheckpointError
    if set(request) != {'command', *INSTALL_FIELDS}:
        raise ValueError('save-install-checkpoint requires exactly ' + ', '.join(INSTALL_FIELDS))
    name = _install_checkpoint_name(checkpoint['key'], request['manifest_sha256'],
                                    request['runtime_sha256'], request['procedure_sha256'])
    bridge.prepare_install_checkpoint()
    if (root / name).is_dir():
        return {'directory': str((root / name).resolve()), 'saved': False, 'existing': True}
    started = perf_counter()
    bridge.disarm_for_checkpoint()
    try:
        target = _save_ram_push_checkpoint(
            session, root, name, INSTALL_RECORD,
            {'key': checkpoint['key'], **attachment, **{field: request[field] for field in INSTALL_FIELDS}},
            replace=False)
    except CheckpointError as error:
        return {'saved': False, 'existing': False, 'save_error': str(error)}
    finally:
        bridge.rearm()
    return {'directory': str(target.resolve()), 'saved': True, 'existing': False,
            'seconds': round(perf_counter() - started, 3)}


def _run_ram_push_server(board, session, output, bridge, attachment, checkpoint, checkpoint_root):
    pre_attach = attachment['pre_attach']
    device = bytes.fromhex(attachment['device'])
    configuration = bytes.fromhex(attachment['configuration'])
    fixed_sites = {
        '0x1469b20': 'request-bootstrap',
        '0x1469b38': 'request-bootstrap-return',
        '0xd806e8': 'query-callback-result',
        '0xd8073c': 'query-response-ready',
        '0x6b0aa8': 'cpu1-sync-wrapper',
        '0x6b0c3c': 'cpu1-range-wrapper',
        '0x61c490': 'cpu1-cache-control',
        '0x6c6b5c': 'cpu1-worker',
        '0x6b0704': 'cpu1-worker-completion',
    }
    bridge.set_trace_sites(fixed_sites, enable_keepalive=False)
    ready = {'ok': True, 'event': 'ready', 'device': device.hex(),
             'configuration': configuration.hex(), 'pre_attach': pre_attach,
             'checkpoint': checkpoint}
    print(json.dumps(ready, separators=(',', ':')), flush=True)
    recovery = session.options['recovery']
    inputs = {}
    for name, path in (('installed_dat', recovery['media']),
                       ('adjustment_nand', recovery['adjustment_nand'])):
        source = Path(path)
        with source.open('rb') as stream:
            digest = hashlib.file_digest(stream, 'sha256').hexdigest()
        inputs[name] = {'path': str(source), 'bytes': source.stat().st_size,
                        'sha256': digest}
    inputs['menu_sd_image'] = recovery['card']
    receipt = {'schema': 1, 'scope': 'emulated X-M1 ordinary-boot RAM push',
               'inputs': inputs, 'pre_attach': pre_attach,
               'device_descriptor': device.hex(),
               'configuration_descriptor': configuration.hex(),
               'checkpoint': checkpoint}
    try:
        for line in sys.stdin:
            request = json.loads(line, object_pairs_hook=_unique_object)
            command = request.get('command')
            if command == 'list':
                result = {'candidate': {'path': 'emulator:xm1', 'vid': 0x04cb, 'pid': 0xff80,
                    'configuration': configuration[5], 'interface': 0, 'alternate': 0,
                    'class': configuration[14], 'subclass': configuration[15],
                    'protocol': configuration[16], 'input': 0x81, 'output': 2,
                    'input_packet': 64, 'output_packet': 64}}
            elif command == 'transfer':
                count, payload = bridge.transfer(request)
                result = {'count': count, 'bytes': payload.hex()}
            elif command == 'trace-sites':
                bridge.set_trace_sites({**fixed_sites, **request['sites']})
                result = {'sites': len(bridge.trace_sites)}
            elif command == 'save-install-checkpoint':
                result = _save_install_checkpoint(bridge, session, checkpoint_root, checkpoint,
                                                  attachment, request)
            elif command == 'trace-hits':
                start = request.get('from')
                if type(start) is not int or not 0 <= start <= len(bridge.trace_hits):
                    raise ValueError('trace-hits requires a hit index within the recorded hits')
                result = {'count': len(bridge.trace_hits), 'hits': bridge.trace_hits[start:]}
            elif command == 'key':
                result = {'menu': bridge.key(request['key'], request.get('hold_instructions'),
                                             request.get('release_instructions'))}
            elif command == 'mode-dial':
                result = bridge.mode_dial(request['position'])
            elif command == 'read-memory':
                address, count = request.get('address'), request.get('count')
                if (type(address) is not int or type(count) is not int or not 0 < count <= 4096
                        or not 0 <= address <= 0x100000000 - count):
                    raise ValueError('read-memory requires an address and 1..4096 bytes')
                result = {'bytes': bytes(board.read_memory(address, count)).hex()}
            elif command == 'capture':
                result = bridge.capture(request['label'], output)
            elif command == 'finish':
                result = {'finished': True}
                print(json.dumps({'ok': True, **result}, separators=(',', ':')), flush=True)
                break
            else:
                raise ValueError('unknown RAM-push bridge command')
            print(json.dumps({'ok': True, **result}, separators=(',', ':')), flush=True)
    except Exception as error:
        receipt['bridge_error'] = {'type': type(error).__name__, 'message': str(error)}
        print(json.dumps({'ok': False, 'error': type(error).__name__,
                          'message': str(error)}, separators=(',', ':')), flush=True)
    from emulator.rec_state import Reader, task_state
    task_tables, tasks = task_state(Reader(board))
    receipt['task_tables'] = task_tables
    receipt['worker_tasks'] = [
        task for task in tasks if task.get('task_id') in (0x24, 0x58, 0x59, 0x5a)]
    receipt['diagnostic_memory'] = {
        'query_output': bytes(board.read_memory(0x01469878, 8)).hex(),
        'callback_slot': bytes(board.read_memory(0x015c22ac, 4)).hex(),
        'response_descriptor': bytes(board.read_memory(0x0146989c, 32)).hex(),
        'response_state': bytes(board.read_memory(0x01469864, 32)).hex(),
        'worker_rows': bytes(board.read_memory(0x0089e608, 0x330)).hex(),
        'worker_payloads': bytes(board.read_memory(0x009c2f5c, 0x324)).hex(),
    }
    receipt['usb_endpoint'] = bridge.endpoint.snapshot()
    receipt['timing_transitions'] = bridge.timing_transitions
    receipt['keepalive_inputs'] = bridge.keepalive_inputs
    receipt['key_log'] = bridge.key_log
    receipt['transfer_log'] = bridge.transfer_log
    receipt['rendezvous_activity'] = bridge.rendezvous_activity
    receipt['cores'] = board.snapshot()['cores']
    receipt.update(contexts=bridge.trace_hits, screenshots=bridge.screenshots,
                   boundary={'instructions': board.instructions, 'failure': board.failure,
                             'execution_error': session.execution_error,
                             'service_mode': int.from_bytes(
                                 board.read_memory(0x0146a0c0, 4), 'little')})
    target = Path(output) / 'emulator-session.json'
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(receipt, indent=2) + '\n')
