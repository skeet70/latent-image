"""Captured SPI2 lens words and optional declared external response fixtures.

This endpoint receives completed 16-bit transfers, independently of SPI2's
transport status. Bank/register/data are bits15:14/13:10/9:0. Axis registerF
bit9 is a word strobe, retained here even though the stock driver's FDFF mask
clears its remembered bit9 after appending. The full named profile delegates
its unmeasured startup lattice to a separate GPIO/counter/IRQ input fixture.
"""
from copy import deepcopy

from .hardware import UnsupportedDevice
from .lens_homing_fixture import HOMING_PROFILE, LensHomingFixture


INITIAL_RESPONSE_PROFILE = 'declared-lens-initial-axis-response-unmeasured'
RESPONSE_PROFILES = (INITIAL_RESPONSE_PROFILE,HOMING_PROFILE)
OBSERVED_CAPTURE_WORDS = 78
INITIAL_INDICES = {'focus': 61, 'iris': 77}  # Zero-based completed-word indices.
INITIAL_SEQUENCES = {
    'focus': (0x2850,0x2c00,0x0400,0x0800,0x0230,0x15e0,0x0c7d,0x184a,
              0x1c39,0x2031,0x242a,0x1280,0x3850,0xd3f7,0xd800,0x3ec0),
    'iris': (0x4005,0x7c00,0x4004,0x5401,0x4c4b,0x584b,0x5c4b,0x604b,
             0x644b,0x5280,0x78d0,0x68a0,0x6d40,0x45c0,0x4802,0x7e80),
}
AXES = {0: 'focus', 1: 'iris'}
IRQS = {'focus': 14, 'iris': 12}  # Decimal sources14/12: physical0xE/0xC.


class LensEndpoint:
    # Ticks at quiet quantum boundaries only advance elapsed-time bookkeeping
    # (emulator.board.Board.flush_deferred_ticks).
    quiet_ticks_defer = True
    WORD_LIMIT = 512

    def __init__(self, board, response_profile=None, response_ticks=400000, *, homing_fixture=None):
        if response_profile is not None and response_profile not in RESPONSE_PROFILES:
            raise ValueError('Unknown lens response profile')
        if type(response_ticks) is not int or not 1 <= response_ticks < 1 << 64:
            raise ValueError('Lens response delay must be a positive integer')
        if response_profile==HOMING_PROFILE:
            if (type(homing_fixture) is not LensHomingFixture or homing_fixture.board is not board
                    or homing_fixture.response_ticks!=response_ticks or homing_fixture not in board.devices):
                raise ValueError('Full lens profile requires its attached matching homing fixture')
        elif homing_fixture is not None:
            raise ValueError('Homing fixture requires the full lens profile')
        self.homing_fixture=homing_fixture
        self.board, self.response_profile, self.response_ticks = board, response_profile, response_ticks
        self.registers, self.words = {}, []
        self.requests = dict.fromkeys(IRQS)
        self.responses = dict.fromkeys(IRQS, 0)
        self.response_pending = False  # Plain checkpointed timer state; no closure or in-flight DMA.
        board.record('SPI2 lens endpoint retention fixture', response_profile=response_profile,
                     response_ticks=response_ticks, observed_capture_words=OBSERVED_CAPTURE_WORDS,
                     limits='Actual transmitted words retained; declared optional response/helper profile requires separate stock execution validation')

    def contains(self, address, size):
        return False  # The endpoint is reached through completed SPI2 words, never CPU MMIO.

    def mmio_ranges(self):
        return ()  # contains() is constantly False (RustBoard._mmio_device index).

    def arm_measurement_iris(self, profile):
        from .lens_measurement_iris import arm
        return arm(self, profile)

    def enable_afs_motion(self, profile):
        from .lens_af_motion import enable
        return enable(self, profile)

    def enable_generic_focus_moves(self, **parameters):
        """Opt in to any stock relative/continuous focus move (see lens_focus_motion.GENERIC_PROFILE)."""
        from .lens_af_motion import enable_generic
        return enable_generic(self, **parameters)

    def focus_pose(self, tick=None):
        """Physical focus position (pulses) of the generic stock-move model at ``tick`` (default now), or None.

        Declared simulation state derived from the accepted commands and the ramp law (see
        ``lens_focus_motion._physical``); it can lead the committed pose ``0xccde64`` while a move runs."""
        from .lens_focus_motion import generic_physical_position
        return generic_physical_position(self, tick)

    def focus_motion_state(self, tick=None):
        from .lens_focus_motion import generic_motion_state
        return generic_motion_state(self, tick)

    def _route(self, axis):
        banks = getattr(getattr(self.board, 'interrupts', None), 'banks', ())
        if len(banks) != 2 or any(type(getattr(b, 'enabled', None)) is not int or b.enabled < 0 for b in banks):
            raise UnsupportedDevice('Lens first response requires two typed interrupt banks')
        targets = [core for core, bank in enumerate(banks) if bank.enabled & (1 << IRQS[axis])]
        if len(targets) != 1:
            raise UnsupportedDevice(f'Lens {axis} first response requires one enabled physical IRQ{IRQS[axis]:#x} route')
        return targets[0]

    @staticmethod
    def _configuration(sequence):
        return {hex(word & 0xfc00): word & 0x3ff for word in sequence}

    def _fixture(self, axis, events):
        sequence = INITIAL_SEQUENCES[axis]
        if (len(events) != 16
                or any(type(e) is not dict or set(e) != {'cpu','word','tick'}
                       or type(e['word']) is not int for e in events)
                or tuple(e['word'] for e in events) != sequence
                or any(type(e['cpu']) is not int or e['cpu'] != events[-1]['cpu']
                       or type(e['tick']) is not int or not 0 <= e['tick'] <= self.board.virtual_ticks
                       for e in events)
                or any(a['tick'] > b['tick'] for a, b in zip(events, events[1:]))):
            raise UnsupportedDevice(f'Lens {axis} first response requires its exact captured16-word transaction')
        return self._configuration(sequence)

    def _validate_request(self, axis, request):
        index = INITIAL_INDICES[axis]
        if (type(request) is not dict or request.get('profile') != INITIAL_RESPONSE_PROFILE
                or self.response_profile != INITIAL_RESPONSE_PROFILE
                or request.get('state') != 'pending-declared-first-response'
                or type(request.get('cpu')) is not int or request['cpu'] not in (0, 1)
                or type(request.get('index')) is not int or request['index'] != index
                or request.get('word') != INITIAL_SEQUENCES[axis][-1]
                or type(request.get('requested_tick')) is not int
                or type(request.get('due_tick')) is not int
                or request['due_tick'] != request['requested_tick'] + self.response_ticks
                or type(self.responses[axis]) is not int or self.responses[axis] != 0):
            raise UnsupportedDevice(f'Lens {axis} pending first-response request changed')
        events = self.words[index-15:index+1]
        configuration = self._fixture(axis, events)
        if (request.get('configuration') != configuration
                or events[-1]['cpu'] != request['cpu'] or events[-1]['tick'] != request['requested_tick']
                or any(type(self.registers.get(int(a, 16))) is not int
                       or self.registers[int(a, 16)] != value for a, value in configuration.items())
                or type(request.get('route')) is not int or request['route'] != self._route(axis)):
            raise UnsupportedDevice(f'Lens {axis} retained transaction/configuration/IRQ route changed before response')

    def transmit(self, core, word):
        """Consume one successful shift; reject unsupported commands before endpoint mutation."""
        try:
            return self._transmit(core, word)
        except UnsupportedDevice as error:
            from .lens_focus_motion import lens_note
            lens_note(self, 'refusal', message=str(error), word=word, core=core, axis='transmit',
                      tick=self.board.virtual_ticks)
            raise

    def _transmit(self, core, word):
        if type(core) is not int or core not in (0, 1) or type(word) is not int or not 0 <= word <= 0xffff:
            raise UnsupportedDevice('Lens endpoint requires a typed CPU and unsigned16-bit transmitted word')
        from .lens_focus_motion import GENERIC_WORD_LIMIT, generic_selected
        if len(self.words) >= (GENERIC_WORD_LIMIT if generic_selected(self) else self.WORD_LIMIT):
            raise UnsupportedDevice('Lens endpoint word-retention bound reached')
        if self.response_profile==HOMING_PROFILE:
            prospective=dict(self.registers)
            prospective[word&0xfc00]=word&0x3ff
            from .lens_af_motion import selected, generic_selected
            axis = 'iris'
            if generic_selected(self):
                from .lens_af_motion import receive_generic
                axis, request = receive_generic(self,core,word)
            elif selected(self):
                from .lens_af_motion import receive
                axis, request = receive(self,core,word,prospective)
            elif self.requests['iris'] is not None:
                from .lens_measurement_iris import receive
                request=receive(self,core,word,prospective)
            else:
                self.homing_fixture.receive(core,word,prospective)
                request=None
            self.registers=prospective
            self.words.append(dict(cpu=core,word=word,tick=self.board.virtual_ticks))
            if request is not None:self.requests[axis]=request
            return
        for axis, pending in self.requests.items():
            if pending is not None and pending['state'] == 'pending-declared-first-response':
                self._validate_request(axis, pending)
                if hex(word & 0xfc00) in pending['configuration']:
                    raise UnsupportedDevice(f'Lens {axis} pending command cannot be reconfigured')
        bank, register, data = word >> 14, (word >> 10) & 15, word & 0x3ff
        axis = AXES.get(bank) if register == 15 and data & 0x200 else None
        event = dict(cpu=core, word=word, tick=self.board.virtual_ticks)
        request = None
        if axis is not None:
            if self.requests[axis] is not None:
                raise UnsupportedDevice(f'Later lens {axis} request requires the unresolved full homing/input contract')
            configuration = {hex(a): v for a, v in self.registers.items() if a >> 14 == bank}
            configuration[hex(word & 0xfc00)] = data
            route, due = None, None
            if self.response_profile is not None:
                if len(self.words) != INITIAL_INDICES[axis]:
                    raise UnsupportedDevice(f'Lens {axis} initial fixture strobe index differs from the78-word capture')
                configuration = self._fixture(axis, self.words[-15:] + [event])
                prospective = dict(self.registers)
                prospective[word & 0xfc00] = data
                if any(type(prospective.get(int(a, 16))) is not int
                       or prospective[int(a, 16)] != value for a, value in configuration.items()):
                    raise UnsupportedDevice('Lens initial configuration differs from its transmitted transaction')
                route, due = self._route(axis), self.board.virtual_ticks + self.response_ticks
            request = dict(cpu=core, index=len(self.words), word=word,
                requested_tick=self.board.virtual_ticks, due_tick=due, route=route,
                configuration=configuration, profile=self.response_profile,
                state='armed-awaiting-unmodeled-response' if due is None else 'pending-declared-first-response',
                initial_focus_quantity=0 if axis == 'focus' and word == 0x3ec0
                    and configuration.get('0x400') == configuration.get('0x800') == 0 else None,
                option=(data >> 7) & 1, driver_remembered_bit9='Cleared by stock FDFF memory mask, not by this endpoint')
        self.registers[word & 0xfc00] = data
        self.words.append(event)
        if request is not None:
            self.requests[axis] = request
            self.response_pending = self.response_pending or due is not None
            self.board.record('Lens axis request retained', axis=axis, request=deepcopy(request),
                limits='Quantity0/option1 is an unmeasured initial focus command, not finite motor travel or full homing')

    def _failure(self, error, axis=None):
        if getattr(self.board, 'failure', None) is None:
            self.board.failure = dict(message=str(error), endpoint='SPI2 lens', axis=axis,
                                      operation='declared initial external response')
            self.board.record('Lens endpoint unresolved response', **self.board.failure)
            from .lens_focus_motion import lens_note
            lens_note(self, 'refusal', message=str(error), axis=axis, tick=self.board.virtual_ticks)

    def tick(self, board):
        if board is not self.board or getattr(board, 'failure', None) is not None:
            return
        if self.response_profile==HOMING_PROFILE:
            from .lens_af_motion import selected, generic_selected
            if generic_selected(self):
                from .lens_af_motion import tick_generic
                try:tick_generic(self)
                except (UnsupportedDevice,KeyError,TypeError,ValueError) as error:self._failure(error,'generic focus')
                return
            if selected(self):
                from .lens_af_motion import tick
                try:tick(self)
                except (UnsupportedDevice,KeyError,TypeError,ValueError) as error:self._failure(error,'AF motion')
            if self.requests['iris'] is not None:
                from .lens_measurement_iris import tick
                try:tick(self)
                except (UnsupportedDevice,KeyError,TypeError,ValueError) as error:self._failure(error,'iris')
            return
        axis = None
        try:
            due = []
            for axis, request in self.requests.items():
                if request is not None and request['state'] == 'pending-declared-first-response':
                    self._validate_request(axis, request)
                    if board.virtual_ticks >= request['due_tick']:
                        due.append((axis, request))
            # Preflight every pending axis before the first physical IRQ effect.
            for axis, request in due:
                board.queue_irq(request['route'], IRQS[axis])
                request.update(state='declared-first-response-emitted', response_tick=board.virtual_ticks)
                self.responses[axis] += 1
                board.record('Declared lens initial physical response IRQ', axis=axis, cpu=request['route'],
                    physical_irq=hex(IRQS[axis]), request_word=hex(request['word']),
                    limits='One unmeasured external input only; stock ISR owns callbacks/queues; full homing unresolved')
            self.response_pending = any(r is not None and r['state'] == 'pending-declared-first-response'
                                        for r in self.requests.values())
        except (UnsupportedDevice, KeyError, TypeError, ValueError) as error:
            self._failure(error, axis)

    def next_event_tick(self, now):
        if getattr(self.board, 'failure', None) is not None:
            return None
        if self.response_profile==HOMING_PROFILE:
            from .lens_af_motion import selected, generic_selected
            if generic_selected(self):
                from .lens_af_motion import next_event_tick_generic
                try:return next_event_tick_generic(self)
                except (UnsupportedDevice,KeyError,TypeError,ValueError) as error:
                    self._failure(error,'generic focus');return None
            if selected(self):
                from .lens_af_motion import next_event_tick
                try:
                    due=next_event_tick(self)
                    if due is not None:return due
                except (UnsupportedDevice,KeyError,TypeError,ValueError) as error:
                    self._failure(error,'AF motion');return None
            if self.requests['iris'] is None:return None
            from .lens_measurement_iris import next_event_tick
            try:return next_event_tick(self)
            except (UnsupportedDevice,KeyError,TypeError,ValueError) as error:
                self._failure(error,'iris');return None
        try:
            times = [r['due_tick'] for r in self.requests.values()
                     if r is not None and r['state'] == 'pending-declared-first-response']
            if any(type(t) is not int for t in times):
                raise UnsupportedDevice('Lens pending response delay changed')
            return min(times) if times else None
        except (UnsupportedDevice, KeyError, TypeError, ValueError) as error:
            self._failure(error)
            return None

    def snapshot(self):
        if self.response_profile == HOMING_PROFILE:
            from .lens_af_motion import selected, generic_selected
            if generic_selected(self):
                from .lens_af_motion import validate_generic
                validate_generic(self)
            elif selected(self):
                from .lens_af_motion import validate
                validate(self)
            elif self.requests['iris'] is not None:
                from .lens_measurement_iris import validate
                validate(self)
        return dict(name=type(self).__name__, response_profile=self.response_profile,
            response_ticks=self.response_ticks, response_pending=self.response_pending,
            observed_capture_words=OBSERVED_CAPTURE_WORDS,
            observed_initial_strobe_indices=INITIAL_INDICES, physical_irqs={a: hex(v) for a, v in IRQS.items()},
            registers={hex(a): hex(v) for a, v in sorted(self.registers.items())},
            retained_words=[dict(e, word=hex(e['word']), bank=e['word'] >> 14,
                register=(e['word'] >> 10) & 15, data=hex(e['word'] & 0x3ff)) for e in self.words],
            requests=deepcopy(self.requests), responses=dict(self.responses),
            homing_fixture_attached=self.homing_fixture is not None,
            limits=['Observed initial axis16-word tails at indices61/77 in a78-word trace; earlier words are retained, not fixture-validated.',
                'Endpoint bit9 remains transmitted; stock driver clears only its own remembered bit9.',
                'Optional one physicalIRQ0xE focus/IRQ0xC iris (decimal14/12) after an unmeasured delay and unique enabled route.',
                'Initial-response profile supplies no later homing. Full named profile delegates only to its attached declared GPIO/counter/IRQ transaction fixture.',
                'No firmware callbacks/queue messages/RAM completion flags are supplied.',
                'The first focus response advances to another movement request, not completed homing.'])
