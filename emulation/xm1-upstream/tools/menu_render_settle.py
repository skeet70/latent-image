"""Bounded host settling after one explicitly requested stock menu key.

MenuRenderSettle(session).key({'key': 'down'}) delegates exactly one ordinary
key pulse, then uses the session's run/sample API. It reads paused RAM and
retained graphics state; it never writes guest memory, adds an IRQ, supplies a
guest callback, changes a device profile, retries an input, or marks coverage.
Owner-thread serialization is required, just as for MenuSession itself.

The boundary requires X-M1 UILIB packet consumption, cleared stock batch
completion fields and a newer completed graphics copy matching the selected
display plane by address, byte count and payload hash. It is a candidate for
pixel review, not proof that a particular item or setting has rendered.
"""
from __future__ import annotations

import copy
import struct
from emulator.menu_inputs import MenuKey
from emulator.rec_state import MemoryField, TASK_ENTRIES as ENTRY_ROLES, TASK_TABLES, ram_range
from tools.continue_rec_scanout import MAX_CONTINUATION
from tools.probe_setup_menus import default_key_pulse

MAX_SETTLE = 90_000_000
MAX_CHUNK = 3_000_000
DEFAULT_CHUNK = 250_000
TASK_ENTRIES = {role.value: entry for entry, role in ENTRY_ROLES.items()}
# 011090A4 receives the packet; 00DB6628 advances the sender byte token.
# 01177C9C submits the batch; 01177760 clears busy and the callback.
FIELDS = {
    'copied_command': MemoryField(0x15e46d0, 2), 'packet_token': MemoryField(0x15e46d2, 1),
    'initialized': MemoryField(0x15e470d, 1), 'display_suppression': MemoryField(0x15e4820),
    'sender_token': MemoryField(0x14e32bc, 1), 'consumer_token': MemoryField(0x14e3338, 1),
    'sender_command': MemoryField(0x14e32ba, 2),
    'staged_render_command': MemoryField(0x15e46e8, 2),
    'staged_render_token': MemoryField(0x15e46ea, 1),
    'batch_busy': MemoryField(0x95a69c), 'batch_callback': MemoryField(0x95a6a0),
    'commands_dirty': MemoryField(0x95a6a8), 'display_pending': MemoryField(0x95a6ac),
    'ui_mode': MemoryField(0x9500ec), 'ui_pending': MemoryField(0x9500f8),
    'ui_busy': MemoryField(0x950110, 1),
    'menu_pointer': MemoryField(0x9571d0), 'cursor': MemoryField(0x9571d8),
    'subcursor': MemoryField(0x9571dc),
}


def read_render_state(board):
    """Read bounded paused-board state, without sampling any MMIO endpoint."""
    def raw(address, size, cpu=0):
        result = bytes(board.read_memory(address, size, cpu_id=cpu))
        if len(result) != size:
            raise ValueError('Truncated menu render diagnostic read')
        return result

    result = {name: int.from_bytes(raw(field.address, field.size), 'little')
              for name, field in FIELDS.items()}
    result.update(instructions=board.instructions, virtual_ticks=board.virtual_ticks)
    result['debounced_inputs_released'] = board._native.menu_keys_released()
    tasks, unavailable = {}, []
    for cpu, address in TASK_TABLES:
        try:
            table, count = struct.unpack('<II', raw(address, 8, cpu))
            if not 1 <= count <= 128 or not ram_range(table, count * 0x38):
                unavailable.append(f'CPU{cpu} task table is unloaded')
                continue
            rows = raw(table, count * 0x38, cpu)
            for index in range(count):
                words = struct.unpack_from('<14I', rows, index * 0x38)
                descriptor = words[2]
                if not ram_range(descriptor, 0x28):
                    continue
                desc = struct.unpack('<10I', raw(descriptor, 0x28, cpu))
                if not desc[9]:
                    continue
                for label, target in TASK_ENTRIES.items():
                    if desc[2] != target:
                        continue
                    if label in tasks:
                        unavailable.append(f'Ambiguous {label} task')
                    tasks[label] = {'cpu': cpu, 'tcb': table + index * 0x38,
                                    'status': words[3] & 0xff}
        except (ValueError, struct.error) as error:
            unavailable.append(f'CPU{cpu} task table unavailable: {error}')
    unavailable.extend(f'{label} task is unavailable' for label in TASK_ENTRIES if label not in tasks)
    result['tasks'] = tasks
    result['ready'] = not unavailable
    result['not_ready'] = unavailable
    devices = [device for device in board.devices
               if type(device).__name__ == 'GraphicsAccelerator']
    if len(devices) != 1:
        result['ready'] = False
        result['not_ready'].append('Unique graphics accelerator is unavailable')
        result['graphics'] = None
        result['publications'] = []
        return result
    graphics = devices[0]
    last = graphics.last_job or {}
    destinations = last.get('destinations') or []
    result['graphics'] = {
        'started': graphics.jobs_started, 'completed': graphics.jobs_completed,
        'acknowledgements': graphics.acknowledgements,
        'bytes_completed': graphics.bytes_completed, 'busy': graphics.busy,
        'command_pending': graphics._command_pending is not None,
        # Read the model's retained register, never stale MMIO RAM backing.
        'status': graphics.regs.get(0x34, 0),
        'last_operation': last.get('operation'), 'last_draw_count': last.get('draw_count'),
        'last_destinations': copy.deepcopy(destinations[:8]),
    }
    # A 1M-instruction owner-thread continuation can outlive recent event
    # history. The completed job is retained by the actual model, including
    # its payload hash. Its job count is an observation, not a completion tick.
    result['graphics']['last_copy'] = (
        {name: copy.deepcopy(last[name]) for name in ('destination', 'bytes', 'sha256')}
        | {'completed_job': graphics.jobs_completed}
        if last.get('operation') == 'copy' and last.get('completed') is True
        and all(name in last for name in ('destination', 'bytes', 'sha256')) else None)
    # Existing model records, not added hooks or fabricated callback counters.
    # A copy is staged independently from the selected LCD buffer, so require
    # the actual complete payload to appear in the sampled full-plane source.
    result['publications'] = [
        {name: copy.deepcopy(event[name]) for name in ('tick', 'destination', 'bytes', 'sha256')}
        for event in board.event_log[-512:]
        if event.get('name') == 'Graphics software-profile copy completed'
        and all(name in event for name in ('tick', 'destination', 'bytes', 'sha256'))
    ][-8:]
    return result


def _drained(probe):
    if not probe.get('ready'):
        return False
    graphics = probe['graphics']
    return (probe['debounced_inputs_released']
            and probe['ui_pending'] == 0 and probe['ui_busy'] == 0
            and probe['initialized'] == 1 and probe['display_suppression'] == 0
            and probe['batch_busy'] == 0 and probe['batch_callback'] == 0
            and probe['commands_dirty'] == 0 and probe['display_pending'] == 0
            and all(task['status'] == 0x12 for task in probe['tasks'].values())
            and not graphics['busy'] and not graphics['command_pending']
            and graphics['status'] == 0 and graphics['started'] == graphics['completed']
            and graphics['acknowledgements'] >= graphics['completed'])


def _auxiliary_render_acknowledged(probe):
    # DB6804's stack packet preserves DB673C's retained render staging.
    return (probe['copied_command'] == probe['sender_command'] == 0x8000
            and probe['packet_token'] == 0
            and probe.get('staged_render_command', 0) & 0xf000 == 0x4000
            and probe.get('staged_render_token', 0)
            == probe['sender_token'] == probe['consumer_token'] != 0)


def _consumer_finished(probe):
    # 01109130 finishes the entire 011090A4 dispatch before waiting again.
    return (probe.get('ready') and probe['tasks']['uilib']['status'] == 0x12
            and probe['sender_token'] != 0
            and (probe['packet_token'] == probe['sender_token'] == probe['consumer_token']
                 or _auxiliary_render_acknowledged(probe))
            and probe['copied_command'] == probe['sender_command'])


def _published(probe, state, after_tick, completed_before):
    display = state.get('display') or {}
    frame = display.get('last_frame') or {}
    if frame.get('instance') != display.get('selected_instance'):
        return None
    publications = list(reversed(probe['publications']))
    retained = probe['graphics'].get('last_copy')
    if retained is not None and retained['completed_job'] > completed_before:
        publications.append({**retained, 'observation': 'retained completed copy'})
    for publication in publications:
        if 'tick' in publication and publication['tick'] < after_tick:
            continue
        for plane in frame.get('graphics', []):
            if (plane.get('source') == publication['destination']
                    and plane.get('captured_bytes') == publication['bytes']
                    and plane.get('sha256') == publication['sha256']
                    and plane.get('width') == frame.get('width')
                    and plane.get('height') == frame.get('height')
                    and plane.get('origin') == [0, 0] and plane.get('alpha_field', 0) > 0):
                return {**copy.deepcopy(publication), 'instance': frame.get('instance'),
                        'layer': plane.get('layer'), 'channel': plane.get('channel')}
    return None


def _frame(state):
    return ((state.get('display') or {}).get('last_frame') or {}).get('frame_sha256')


class MenuRenderSettle:
    """Adapter for a MenuSession; keep the delegated key method unwrapped.

    key_action may retain the original bound key method to avoid recursion.
    completion_condition receives the paused target probe and returns a boolean;
    the default requires the X-M1 batch, task and hardware completion boundary.
    Packet consumption and selected-plane publication remain required.
    """
    def __init__(self, session, *, key_action=None, probe=None, completion_condition=None):
        self.session = session
        self.key_action = key_action or session.key
        self.probe = probe or (lambda: read_render_state(session.board))
        self.completion_condition = completion_condition or _drained

    def key(self, options):
        allowed = {'key', 'hold_instructions', 'release_instructions',
                   'max_settle_instructions', 'chunk_instructions'}
        if type(options) is not dict or set(options) - allowed:
            raise ValueError('Unknown settling key options')
        parsed_key = MenuKey.parse(options.get('key'))
        key = parsed_key.value
        pulse = default_key_pulse(parsed_key)
        hold = options.get('hold_instructions', pulse.hold_instructions)
        release = options.get('release_instructions', pulse.release_instructions)
        budget = options.get('max_settle_instructions', MAX_SETTLE)
        chunk = options.get('chunk_instructions', DEFAULT_CHUNK)
        if any(type(value) is not int or not low <= value <= high for value, low, high in
               ((hold, 1, MAX_CONTINUATION), (release, 1, MAX_CONTINUATION),
                (budget, 0, MAX_SETTLE), (chunk, 1, MAX_CHUNK))):
            raise ValueError('Key phases must be 1..15M; chunk 1..3M; settle budget 0..90M')
        session = self.session
        if session.board.failure is not None or session.execution_error is not None:
            raise ValueError('Restore before menu input after a strict stop')
        before = self.probe()
        old_frame = _frame(session.state or {})
        start = session.board.instructions
        state = session.state or {}
        prior_start = start
        runs = 0
        # Drain the preceding packet before the requested pulse.
        while not (self.completion_condition(before) and _consumer_finished(before)):
            if session.board.failure is not None or session.execution_error is not None:
                return self._result(state, before, before, key, start, 0, runs,
                                    'strict-stop', old_frame, budget, prior_start=prior_start)
            spent = session.board.instructions - prior_start
            if spent >= budget:
                return self._result(state, before, before, key, start, 0, runs,
                                    'rendering-not-ready' if not before.get('ready') else 'prior-render-pending', old_frame, budget,
                                    prior_start=prior_start)
            state = self._action(session.run, {'limit': min(chunk, budget - spent), 'pixels': False})
            runs += 1
            if session.board.failure is not None or session.execution_error is not None:
                continue
            try:
                before = self.probe()
            except Exception as diagnostic:
                return self._result(state, before, before, key, start, 0, runs,
                                    'probe-unavailable', old_frame, budget,
                                    error=f'{type(diagnostic).__name__}: {diagnostic}',
                                    prior_start=prior_start)
        prior_instructions = session.board.instructions - prior_start
        key_start = session.board.instructions
        key_tick = session.board.virtual_ticks
        state = self._action(self.key_action, {'key': key, 'hold_instructions': hold,
                                            'release_instructions': release})
        after = before
        status, error = 'budget-exhausted', None
        settle_start = session.board.instructions
        requested_token, work_tick, publication = None, None, None
        completed_floor = before['graphics']['completed']
        bytes_floor = before['graphics']['bytes_completed']
        while True:
            if session.board.failure is not None or session.execution_error is not None:
                status = 'strict-stop'
                break
            try:
                after = self.probe()
            except Exception as diagnostic:
                status, error = 'probe-unavailable', f'{type(diagnostic).__name__}: {diagnostic}'
                break
            if not after.get('ready'):
                status = 'rendering-not-ready'
                break
            if after['sender_token'] != before['sender_token']:
                requested_token = after['sender_token']
            # Nonzero packet tokens cycle from 1 through 255.
            consumed = (requested_token is not None and (
                (after['copied_command'] & 0xf000 == 0x4000
                 and after['packet_token'] == requested_token
                 and after['consumer_token'] == requested_token
                 and after['copied_command'] == after['sender_command'])
                or (_auxiliary_render_acknowledged(after)
                    and after['staged_render_token'] == requested_token
                    and before.get('staged_render_token') is not None
                    and before['staged_render_token'] != requested_token)))
            new_work = (after['graphics']['completed'] > completed_floor
                        and after['graphics']['bytes_completed'] > bytes_floor)
            if after['graphics']['started'] > before['graphics']['started']:
                work_tick = key_tick
            if (consumed and new_work and work_tick is not None
                    and self.completion_condition(after) and _consumer_finished(after)):
                publication = _published(after, state, key_tick, completed_floor)
                render_errors = [item.get('render_error')
                                 for item in (state.get('display') or {}).get('instances', [])
                                 if item.get('render_error')]
                if publication is None and _frame(state) is None and render_errors:
                    status, error = 'scanout-unavailable', '; '.join(render_errors)
                    break
            if publication is not None:
                status = 'render-batch-published-candidate'
                break
            spent = prior_instructions + session.board.instructions - settle_start
            if spent >= budget:
                break
            state = self._action(session.run, {'limit': min(chunk, budget - spent), 'pixels': False})
            runs += 1
        return self._result(state, before, after, key, start,
                            settle_start - key_start, runs, status, old_frame, budget,
                            requested_token=requested_token, work_tick=work_tick,
                            publication=publication, error=error,
                            prior_start=prior_start, prior_instructions=prior_instructions)

    def _result(self, state, before, after, key, start, key_instructions, runs,
                status, old_frame, budget, *, requested_token=None, work_tick=None,
                publication=None, error=None, prior_start=None, prior_instructions=None):
        session = self.session
        # Continuations sample plane metadata only; render pixels once, here.
        if getattr(session, 'compose_frames', None) is not None and state is session.state:
            state = session.compose_frames()
        if prior_instructions is None:
            prior_instructions = session.board.instructions - prior_start if prior_start is not None else 0
        state = copy.deepcopy(state)
        state.setdefault('navigation', {})['render_settle'] = {
            'status': status, 'key': key,
            'key_instructions': key_instructions,
            'settle_instructions': session.board.instructions - start - key_instructions,
            'prior_settle_instructions': prior_instructions,
            'max_settle_instructions': budget, 'continuations': runs,
            'before': before, 'after': after, 'diagnostic_error': error,
            'requested_packet_token': requested_token, 'work_observed_tick': work_tick,
            'publication': publication,
            'candidate_frame_changed': _frame(state) is not None and _frame(state) != old_frame,
            'limits': 'X-M1 packet consumption, target completion condition and a matching selected-plane copy are a pixel-review candidate; no item, setting or coverage success is inferred.'}
        session._last_render_settle = copy.deepcopy(state['navigation']['render_settle'])
        session.state = state
        return state

    def _action(self, action, options):
        before = self.session.board.instructions
        try:
            return action(options)
        except Exception as error:
            if (action == self.key_action and isinstance(error, ValueError)
                    and self.session.board.instructions == before
                    and self.session.board.failure is None and self.session.execution_error is None):
                # A cold/unavailable input or invalid operation refused before
                # execution is not a failed emulated transaction.
                raise
            # A run exception outside CPU dispatch must also latch the strict
            # stop. No automatic release, run, retry or restore follows it.
            if self.session.execution_error is None:
                self.session.execution_error = {'type': type(error).__name__, 'error': str(error)}
            state = copy.deepcopy(self.session.state or {})
            state['execution_error'] = copy.deepcopy(self.session.execution_error)
            return state
