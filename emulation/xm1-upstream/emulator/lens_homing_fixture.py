"""One declared startup lens transaction, not measured camera mechanics.

The initial axis tails and focus state4 tail were observed. Later tails are
byte-audited stock predictions for this exact virtual input lattice. Firmware
owns every ISR, timer, callback, queue message and RAM state transition.
"""
from copy import copy, deepcopy

from .boot_devices import BootControls
from .hardware import UnsupportedDevice
from .pulse_devices import PulseOutputs


HOMING_PROFILE = 'declared-lens-startup-homing-lattice-unmeasured'
PROFILE = HOMING_PROFILE
PASSIVE_IRIS_OPTION_CONTRACT = 'stock-iris-regF-option1-no-strobe-retention'
POST_IRIS_CONTRACT = 'declared-rec-iris-zero-quantity-rate20-single-response-unmeasured'
POST_COMPLETE_OPTION_PROFILE = 'stock-iris-post-completion-option1-once'
CONTROLLER_REINIT_PROFILE = 'stock-lens-global-controller-reinitialization21-once'
CONTROLLER_CLOSE_PROFILE = 'stock-lens-global-reinitialization21-close20-once'
FOCUS_REENTRY_PROFILE = 'declared-lens-controller-close-focus-reference-target222-unmeasured'
IRIS_REENTRY_PROFILE = 'declared-lens-controller-close-focus222-iris-reference255-unmeasured'
CONTINUATION_PROFILES = (POST_COMPLETE_OPTION_PROFILE, CONTROLLER_REINIT_PROFILE, CONTROLLER_CLOSE_PROFILE,
                         FOCUS_REENTRY_PROFILE, IRIS_REENTRY_PROFILE)


def _words(text):
    return tuple(int(word, 16) for word in text.split())


# (phase, exact completed words, raw channel4 edges, direction, response IRQ,
# GPIO target). IRQ8 searches require a subsequent actual D801 stop word.
FOCUS_COMMANDS = (
    ('initial1', _words('2850 2C00 0400 0800 0230 15E0 0C7D 184A 1C39 2031 242A 1280 3850 D3F7 D800 3EC0'), 0, 1, 14, None),
    ('move4', _words('D800 3CC0 D3F5 D3F7 2800 2C00 04E0 0800 0000 1401 0C7D 187D 1C7D 207D 247D 1280 3850 D800 3E80'), 7, 0, 14, None),
    ('search5', _words('D800 3C80 D3F5 D3F7 2800 2C00 0400 0819 0030 15E0 0C7D 184A 1C39 2031 242A 1280 3850 D800 3E80'), 1, 0, 8, 1),
    ('move7', _words('D800 3C80 D3F5 D3F7 2800 2C00 0400 0802 0000 1401 0C7D 187D 1C7D 207D 247D 1280 3850 D800 3E80'), 64, 0, 14, None),
    ('settle8', _words('D800 3C80 0003 D800 3C80 28A0 2C00 0400 0800 0032 15E0 0C7D 184A 1C39 2031 242A 1280 3850 D800 3E80'), 0, 0, 14, None),
    ('search9', _words('D800 3C80 D3F5 D3F7 2800 2C00 0400 0832 0202 1401 0C7D 187D 1C7D 207D 247D 1280 3850 D800 3EC0'), 65, 1, 8, 0),
    ('target15', _words('D800 3CC0 020F D800 3CC0 D3F5 D3F7 2800 2CA0 04A0 0802 15E0 0C65 183C 1C31 202A 2424 1280 023E 3850 D800 3EC0'), 69, 1, 14, None),
)
IRIS_COMMANDS = (
    ('initial1', _words('4005 7C00 4004 5401 4C4B 584B 5C4B 604B 644B 5280 78D0 68A0 6D40 45C0 4802 7E80'), 0, 0, 12, None),
)
COMMANDS = {'focus': FOCUS_COMMANDS, 'iris': IRIS_COMMANDS}
INITIAL_INDICES = {'focus': 61, 'iris': 77}
CLEANUP = {'focus': _words('D3F5 D800 3C40 D3F4 D3F4'), 'iris': (0x7c00,0x7c80)}
# Observed later 4501 tail. A174E0/A1748C program it; physical IRQ12 lets
# stock A179D4/A178A4(C14=3)/A17338 emit7C00 and its own callback.
# Q0/rate20 is an unmeasured response opportunity, not finite motor travel.
POST_IRIS_WORDS = _words('7C80 4014 5440 4E71 584C 5C4C 604C 644C 5280 78D0 6940 6C00 4400 4800 7E80')
# Actual SIMPLE entry stack reaches A075FC/A13124. The unchanged whole driver
# emits this exact CALIB61/07/F7/F7, retained-phase2 transaction in isolation.
# SIMPLE and both manual paths replayed the whole21-word continuation.
# None of these words strobes axis registerF bit9; no motor response is supplied.
CONTROLLER_REINIT_WORDS = _words('C008 D3F0 FC03 4000 A081 A6C0 AC80 B1C0 BC04 C008 C41D CBF7 CFF7 D410 FC03 CBF7 CFF7 B1C0 A708 0005 D3F4')
# Their subsequent C008 call is A075B8(3), whose unchanged whole native driver
# emits the19-word initializer and retained phase2, without A11878/D3F4.
# Whole boot acceptance of this20-word close still requires validation.
CONTROLLER_CLOSE_WORDS = CONTROLLER_REINIT_WORDS[:-1]
# Whole native A10164 and A0D8F8 request bodies, with the actual SIMPLE4400
# target222/configquery0 fixture. ISR A0D438 and callback A09AFC use real
# kernel mailboxes in qualification; RTOS dequeue still needs full boot proof.
# Starting physical coordinate remains-62 from the archived cold transaction.
FOCUS_REENTRY_COMMANDS = (
    ('initial1', _words('D800 3C00 0001 D3F0 D3F2 2850 2C00 0400 0800 0230 15E0 0C7D 184A 1C39 2031 242A 1280 3850 D3F3 D800 3EC0'), 0, 1, 14, None),
    ('move4', _words('D800 3CC0 D3F1 D3F3 2800 2C00 04E0 0800 0000 1401 0C7D 187D 1C7D 207D 247D 1280 3850 D800 3E80'), 7, 0, 14, None),
    ('search5', _words('D800 3C80 D3F1 D3F3 2800 2C00 0400 0819 0030 15E0 0C7D 184A 1C39 2031 242A 1280 3850 D800 3E80'), 63, 0, 8, 1),
    ('retreat7', _words('D800 3C80 D3F1 D3F3 2800 2C00 0400 0802 0000 1401 0C7D 187D 1C7D 207D 247D 1280 3850 D800 3E80'), 64, 0, 14, None),
    ('settle8', _words('D800 3C80 000F D800 3C80 28A0 2C00 0400 0800 003E 15E0 0C7D 184A 1C39 2031 242A 1280 3850 D800 3E80'), 0, 0, 14, None),
    ('search9', _words('D800 3C80 D3F1 D3F3 2800 2C00 0400 0832 020E 1401 0C7D 187D 1C7D 207D 247D 1280 3850 D800 3EC0'), 65, 1, 8, 0),
    ('settle10', _words('D800 3CC0 020B D800 3CC0 28A0 2C00 0400 0800 023A 15E0 0C7D 184A 1C39 2031 242A 1280 3850 D800 3EC0'), 0, 1, 14, None),
    ('target15', _words('D800 3CC0 D3F1 D3F3 2800 2CA0 07E0 0806 15E0 0C65 183C 1C31 202A 2424 1280 003A 3850 D800 3E80'), 223, 0, 14, None),
)
FOCUS_REENTRY_STARTS = (-62, -62, -55, 8, 72, 72, 7, 7)
# B3=1 in the observed CALIB window enables the channel0 monitor. The
# complete target-result consumer releases it before the final shutdown.
FOCUS_REENTRY_CLEANUP = _words('D3F1 D800 3C00 D3F0 D3F0')
# Whole A17C04 with the later4500 request's desiredFF emits the original
# initial iris tail. A17924 then takes itsFF branch directly to7C00/4240.
IRIS_REENTRY_WORDS = IRIS_COMMANDS[0][1]
ASSUMPTIONS = (
    'Unmeasured virtual coordinate starts at0; home GPIO0/pin0 is high iff coordinate>=8.',
    'Mode2 finite wire quantities supply one unsigned channel4 event per32 wire units; channel0 receives floor(events/4).',
    'Search5 supplies one event to coordinate8; search9 supplies65 reverse events to coordinate7, then holds without overrun until actual D801.',
    'Initial quantity0/option1 and settle quantity0 are acknowledged without count events; stock alone advances settle software phase.',
    'AC==AE at the reverse threshold gives referencebase0; stock timer adjusts position to-1 and requests69 events/wire2208 for target-70.',
    'After complete cold homing and cleanup, exact same-owner iris option words7C80 may repeat within the whole512-word ledger without any motor response. This passive wire retention does not imply the software request is complete or has no later motion.',
    'One later exact iris Q0/rate20 tail receives one declared IRQ12 response and requires its actual7C00 cleanup; no mechanical progression is inferred.',
    'This cold request has desired irisindex0: one initial response then actual7C00 cleanup; earlier pausedindex1 trajectory is excluded.',
    'Response delay schedules an external input burst; it is not a motor rate, encoder measurement or time-derived count.',
    'Later trajectories and different configuration, count source, GPIO threshold or electrical polarity are unsupported.',
)


class LensHomingFixture:
    # Ticks at quiet quantum boundaries only advance elapsed-time bookkeeping
    # (emulator.board.Board.flush_deferred_ticks).
    quiet_ticks_defer = True
    WORD_LIMIT = 512

    def __init__(self, board, response_ticks=400000, *, continuation_profile=None):
        if type(response_ticks) is not int or not 1 <= response_ticks < 1 << 64:
            raise ValueError('Lens homing response delay must be a positive integer')
        if continuation_profile is not None and continuation_profile not in CONTINUATION_PROFILES:
            raise ValueError('Unknown lens continuation profile')
        self.board, self.response_ticks = board, response_ticks
        self.continuation_profile = continuation_profile
        self.words, self.registers = [], {}
        self.buffers = {'focus': [], 'iris': []}
        self.next_command = {'focus': 0, 'iris': 0}
        self.pending = {'focus': None, 'iris': None}
        self.cleanup = {'focus': 0, 'iris': 0}
        self.responses = {'focus': 0, 'iris': 0}
        self.coordinate, self.gpio_level = 0, 0
        self.deliveries = []
        self.post_iris = dict(state='initial', word_indices=[], responses=0)
        self.post_complete_option = None
        self.controller_reinit = None
        self.controller_close = None
        self.focus_reentry = None
        if continuation_profile == IRIS_REENTRY_PROFILE:
            self.iris_reentry = None
        board.record('Declared lens startup transaction fixture', profile=HOMING_PROFILE,
                     assumptions=list(ASSUMPTIONS),
                     limits='Stock callbacks only; predicted later tails require actual replay validation; no measured mechanics')

    def _continuation_profile(self):
        if self.continuation_profile is not None and type(self.continuation_profile) is not str:
            raise UnsupportedDevice('Lens continuation profile must remain a typed declared name')
        return POST_COMPLETE_REPEAT_BASES.get(self.continuation_profile,self.continuation_profile)

    def contains(self, address, size):
        return False

    def _devices(self):
        controls = [d for d in self.board.devices if type(d) is BootControls]
        pulses = [d for d in self.board.devices if type(d) is PulseOutputs]
        if (len(controls) != 1 or len(pulses) != 1
                or pulses[0].counter_input_profile != PulseOutputs.COUNTER_LENS_PROFILE):
            raise UnsupportedDevice('Lens homing fixture requires unique GPIO and named counter input devices')
        if (controls[0].gpio_input_pin(0, 0) != self.gpio_level
                or type(controls[0].registers.get(BootControls.GPIO + 0x14)) is not int
                or controls[0].registers[BootControls.GPIO + 0x14] & 1 != self.gpio_level):
            raise UnsupportedDevice('Lens homing GPIO input changed outside its declared threshold ledger')
        return controls[0], pulses[0]

    def _route(self, irq, owner):
        banks = getattr(getattr(self.board, 'interrupts', None), 'banks', ())
        if len(banks) != 2 or any(type(getattr(b, 'enabled', None)) is not int or b.enabled < 0 for b in banks):
            raise UnsupportedDevice('Lens homing response requires two typed IRQ banks')
        targets = [core for core, bank in enumerate(banks) if bank.enabled & (1 << irq)]
        if targets != [owner]:
            raise UnsupportedDevice(f'Lens homing requires one enabled owner route for physical IRQ{irq:#x}')
        return owner

    @staticmethod
    def _axis_configuration(axis, sequence):
        bank = 0 if axis == 'focus' else 1
        return {word & 0xfc00: word & 0x3ff for word in sequence if word >> 14 == bank}

    @staticmethod
    def _counter_state(pulses):
        result = {}
        for channel in (4, 0):
            base = pulses._counter_configuration(channel)
            generation = pulses.counter_generations[channel]
            if type(generation) is not int or generation < 1:
                raise UnsupportedDevice('Lens counter generation changed')
            result[channel] = (generation, pulses.registers[base + 8])
        return result

    def _request(self, axis, indices, prospective):
        stage = self.next_command[axis]
        phase, sequence, edges, direction, irq, level = COMMANDS[axis][stage]
        events = [self.words[i] for i in indices[:-1]]
        events.append(dict(cpu=indices[-1][0], word=indices[-1][1], tick=self.board.virtual_ticks))
        # Last index is represented by its proposed CPU/word until acceptance.
        owner = events[-1]['cpu']
        if (tuple(e['word'] for e in events) != sequence
                or any(type(e['word']) is not int or type(e['cpu']) is not int or e['cpu'] != owner
                       or type(e['tick']) is not int or not 0 <= e['tick'] <= self.board.virtual_ticks for e in events)
                or any(a['tick'] > b['tick'] for a, b in zip(events, events[1:]))):
            raise UnsupportedDevice(f'Lens {axis}/{phase} requires its whole exact ordered transaction')
        configuration = self._axis_configuration(axis, sequence)
        if any(type(prospective.get(a)) is not int or prospective[a] != v for a, v in configuration.items()):
            raise UnsupportedDevice('Lens homing retained register bank differs from its transaction')
        _, pulses = self._devices()
        counters = self._counter_state(pulses) if edges else None
        if counters is not None and any(value != 0 for generation, value in counters.values()):
            raise UnsupportedDevice('Lens homing motion requires stock reset counters before its external edges')
        return dict(stage=stage, phase=phase, owner=owner, word_indices=indices[:-1] + [len(self.words)],
                    configuration=configuration, requested_tick=self.board.virtual_ticks,
                    due_tick=self.board.virtual_ticks + self.response_ticks,
                    state='pending-input', counters=counters)

    def receive(self, core, word, registers):
        """Consume a completed wire word and its prospective encoded register bank."""
        if (type(core) is not int or core not in (0, 1) or type(word) is not int or not 0 <= word <= 0xffff
                or type(registers) is not dict or len(self.words) >= self.WORD_LIMIT
                or type(self.board.virtual_ticks) is not int or self.board.virtual_ticks < 0):
            raise UnsupportedDevice('Lens homing requires typed bounded completed words/register bank')
        prospective = dict(self.registers)
        prospective[word & 0xfc00] = word & 0x3ff
        if (any(type(a) is not int or type(v) is not int for a, v in registers.items())
                or registers != prospective):
            raise UnsupportedDevice('Lens endpoint register retention diverged from completed words')
        bank = word >> 14
        axis = 'iris' if bank == 1 else 'focus' if bank in (0, 3) else None
        if type(self.post_iris) is not dict or self.post_iris.get('state') not in (
                'initial', 'collecting', 'pending-response', 'awaiting-cleanup', 'complete'):
            raise UnsupportedDevice('Post-homing iris phase ledger changed')
        if (self.post_iris['state'] == 'complete'
                and self._continuation_profile() == IRIS_REENTRY_PROFILE
                and type(self.focus_reentry) is dict
                and self.focus_reentry.get('cleanup') == len(FOCUS_REENTRY_CLEANUP)):
            iris = self._iris_reentry_word(core, word, prospective)
            self.registers = prospective
            self.words.append(dict(cpu=core, word=word, tick=self.board.virtual_ticks))
            self.iris_reentry = iris
            return
        if (self.post_iris['state'] == 'complete'
                and self._continuation_profile() in (FOCUS_REENTRY_PROFILE, IRIS_REENTRY_PROFILE)
                and type(self.controller_close) is dict and self.controller_close.get('state') == 'complete'):
            reentry = self._focus_reentry_word(core, word, prospective)
            self.registers = prospective
            self.words.append(dict(cpu=core, word=word, tick=self.board.virtual_ticks))
            self.focus_reentry = reentry
            return
        if (self.post_iris['state'] == 'complete'
                and self._continuation_profile() in (CONTROLLER_CLOSE_PROFILE, FOCUS_REENTRY_PROFILE,
                                                  IRIS_REENTRY_PROFILE)
                and (self.controller_close is not None or
                     type(self.controller_reinit) is dict and self.controller_reinit.get('state') == 'complete')):
            close = self._controller_close_word(core, word)
            self.registers = prospective
            self.words.append(dict(cpu=core, word=word, tick=self.board.virtual_ticks))
            self.controller_close = close
            self.board.record('Passive global lens close word retained',
                              profile=self.continuation_profile, word=hex(word),
                              state=close['state'], index=len(self.words)-1,
                              limits='Bounded wire configuration only; no response, IRQ, timer, motion, counter, GPIO or firmware completion')
            return
        if (self.post_iris['state'] == 'complete'
                and self._continuation_profile() in (CONTROLLER_REINIT_PROFILE, CONTROLLER_CLOSE_PROFILE,
                                                  FOCUS_REENTRY_PROFILE, IRIS_REENTRY_PROFILE)
                and (self.controller_reinit is not None or word == CONTROLLER_REINIT_WORDS[0])):
            reinit = self._controller_reinit_word(core, word)
            self.registers = prospective
            self.words.append(dict(cpu=core, word=word, tick=self.board.virtual_ticks))
            self.controller_reinit = reinit
            self.board.record('Passive global lens initializer word retained',
                              profile=self.continuation_profile, word=hex(word),
                              state=reinit['state'], index=len(self.words)-1,
                              limits='Bounded wire configuration only; no response, IRQ, timer, motion, counter, GPIO or firmware completion')
            return
        if (self.post_iris['state'] == 'complete'
                and self.continuation_profile in CONTINUATION_PROFILES):
            option = self._post_complete_option_word(core, word)
            self.registers = prospective
            self.words.append(dict(cpu=core, word=word, tick=self.board.virtual_ticks))
            self.post_complete_option = option
            self.board.record('Passive completed-iris option word retained',
                              profile=self.continuation_profile, **deepcopy(option),
                              limits='Declared bounded option words only; no IRQ, timer, motor, counter, GPIO or firmware completion')
            return
        if self.post_iris['state'] != 'initial' or word == POST_IRIS_WORDS[1]:
            post = self._post_iris_word(core, word, prospective)
            self.registers = prospective
            self.words.append(dict(cpu=core, word=word, tick=self.board.virtual_ticks))
            self.post_iris = post
            return
        request = None
        buffer = None
        stop = False
        passive_option = False
        if axis is not None:
            pending = self.pending[axis]
            stage = self.next_command[axis]
            if pending is not None:
                self._validate(axis, pending)
                if axis != 'focus' or pending['state'] != 'awaiting-stop-word' or word != 0xd801 or core != pending['owner']:
                    raise UnsupportedDevice(f'Lens {axis} command arrived before its ordered external response/stop')
                stop = True
            elif stage == len(COMMANDS[axis]):
                index = self.cleanup[axis]
                if axis=='iris' and index==1 and self.cleanup['focus']!=len(CLEANUP['focus']):
                    raise UnsupportedDevice('Post-homing iris option write requires completed focus cleanup')
                if axis=='iris' and index==len(CLEANUP['iris']) and word==0x7c80:
                    self._passive_iris_option(core)
                    passive_option = True
                elif index >= len(CLEANUP[axis]) or word != CLEANUP[axis][index]:
                    raise UnsupportedDevice(f'Later lens {axis} transaction requires an additional declared contract')
            else:
                buffer = self.buffers[axis] + [len(self.words)]
                sequence = COMMANDS[axis][stage][1]
                if stage:
                    actual = tuple(self.words[i]['word'] for i in buffer[:-1]) + (word,)
                    if actual != sequence[:len(actual)]:
                        raise UnsupportedDevice(f'Lens {axis}/{COMMANDS[axis][stage][0]} ordered transaction differs')
                strobe = (word >> 10) & 15 == 15 and word & 0x200 and bank in (0, 1)
                if strobe:
                    if stage == 0:
                        if len(self.words) != INITIAL_INDICES[axis]:
                            raise UnsupportedDevice('Lens initial strobe ordinal differs from captured78-word trace')
                        buffer = list(range(len(self.words) - len(sequence) + 1, len(self.words) + 1))
                    if len(buffer) != len(sequence):
                        raise UnsupportedDevice('Lens homing command requires its complete ordered word tail')
                    request = self._request(axis, buffer[:-1] + [(core, word)], prospective)
        self.registers = prospective
        self.words.append(dict(cpu=core, word=word, tick=self.board.virtual_ticks))
        if axis is not None:
            if stop:
                pending.update(state='pending-stop-response', stop_index=len(self.words) - 1,
                               stop_tick=self.board.virtual_ticks, due_tick=self.board.virtual_ticks + self.response_ticks)
                self.board.record('Declared lens search stop received', axis=axis, phase=pending['phase'], word=hex(word))
            elif passive_option:
                self.board.record('Passive post-homing iris option word retained',
                                  contract=PASSIVE_IRIS_OPTION_CONTRACT, cpu=core, word=hex(word),
                                  limits='Wire retention only; no response, motion, timer, GPIO, counter or software completion')
            elif self.next_command[axis] == len(COMMANDS[axis]):
                self.cleanup[axis] += 1
            elif request is not None:
                self.pending[axis] = request
                self.buffers[axis] = []
                self.board.record('Declared lens whole transaction retained', axis=axis, request=deepcopy(request))
            else:
                self.buffers[axis] = buffer

    def _passive_iris_option(self, core):
        """Permit only the repeated nonstrobe word after this complete ledger."""
        self._completed_homing(core)
        if type(self.registers.get(0x7c00)) is not int or self.registers[0x7c00] != 0x80:
            raise UnsupportedDevice('Passive iris option retention changed its nonstrobe register')

    def _completed_homing(self, core):
        for axis in COMMANDS:
            if (type(self.next_command[axis]) is not int
                    or self.next_command[axis] != len(COMMANDS[axis])
                    or type(self.responses[axis]) is not int
                    or self.responses[axis] != len(COMMANDS[axis])
                    or type(self.cleanup[axis]) is not int
                    or self.cleanup[axis] != len(CLEANUP[axis])
                    or self.pending[axis] is not None or self.buffers[axis] != []):
                raise UnsupportedDevice('Passive iris option retention requires complete cold homing/cleanup')
        if len(self.words) <= INITIAL_INDICES['iris']:
            raise UnsupportedDevice('Passive iris option retention lacks its initial transaction')
        initial = self.words[INITIAL_INDICES['iris']]
        if (type(initial) is not dict or type(initial.get('cpu')) is not int or initial['cpu'] != core
                or type(initial.get('word')) is not int or initial['word'] != IRIS_COMMANDS[0][1][-1]):
            raise UnsupportedDevice('Post-homing iris changed its initial owner or transaction')

    def _post_iris_word(self, core, word, prospective):
        post = deepcopy(self.post_iris)
        if (type(post.get('word_indices')) is not list
                or type(post.get('responses')) is not int):
            raise UnsupportedDevice('Post-homing iris word/response ledger changed')
        if post['state'] == 'initial':
            self._passive_iris_option(core)
            if (post['word_indices'] != [] or post['responses'] != 0
                    or word != POST_IRIS_WORDS[1] or not self.words
                    or self.words[-1]['word'] != POST_IRIS_WORDS[0]
                    or self.words[-1]['cpu'] != core):
                raise UnsupportedDevice('Post-homing iris requires the preceding same-owner7C80')
            post.update(state='collecting', owner=core, word_indices=[len(self.words) - 1])
        elif post['state'] == 'awaiting-cleanup':
            self._validate_post_iris()
            if word != 0x7c00 or core != post['owner']:
                raise UnsupportedDevice('Post-homing iris requires its actual same-owner7C00 cleanup')
            post.update(state='complete', cleanup_index=len(self.words))
            return post
        elif post['state'] != 'collecting':
            raise UnsupportedDevice('Later iris input requires an additional declared contract')
        self._completed_homing(core)
        prior = post['word_indices']
        if (post['responses'] != 0 or type(post.get('owner')) is not int or post['owner'] != core
                or not 1 <= len(prior) < len(POST_IRIS_WORDS)
                or any(type(i) is not int or not 0 <= i < len(self.words) for i in prior)):
            raise UnsupportedDevice('Post-homing iris collecting ledger changed')
        indices = prior + [len(self.words)]
        events = [self.words[i] for i in prior] + [dict(cpu=core, word=word, tick=self.board.virtual_ticks)]
        if (indices != list(range(indices[0], indices[0] + len(indices)))
                or any(type(e) is not dict or type(e.get('word')) is not int
                       or type(e.get('cpu')) is not int or e['cpu'] != core
                       or type(e.get('tick')) is not int or not 0 <= e['tick'] <= self.board.virtual_ticks for e in events)
                or tuple(e['word'] for e in events) != POST_IRIS_WORDS[:len(events)]
                or any(a['tick'] > b['tick'] for a, b in zip(events, events[1:]))):
            raise UnsupportedDevice('Post-homing iris requires its whole exact same-owner ordered tail')
        post['word_indices'] = indices
        if len(indices) == len(POST_IRIS_WORDS):
            configuration = self._axis_configuration('iris', POST_IRIS_WORDS)
            if any(prospective.get(a) != v for a, v in configuration.items()):
                raise UnsupportedDevice('Post-homing iris configuration differs from its whole tail')
            post.update(state='pending-response', configuration=configuration,
                        requested_tick=self.board.virtual_ticks,
                        due_tick=self.board.virtual_ticks + self.response_ticks)
        return post

    def _validate_post_iris(self):
        post = self.post_iris
        indices = post.get('word_indices')
        if (post['state'] not in ('pending-response', 'awaiting-cleanup')
                or type(post.get('responses')) is not int
                or post['responses'] != (0 if post['state'] == 'pending-response' else 1)
                or type(post.get('owner')) is not int or post['owner'] not in (0, 1)
                or type(indices) is not list or len(indices) != len(POST_IRIS_WORDS)
                or any(type(i) is not int or not 0 <= i < len(self.words) for i in indices)
                or indices != list(range(indices[0], indices[0] + len(indices)))
                or type(post.get('requested_tick')) is not int or type(post.get('due_tick')) is not int
                or post['due_tick'] != post['requested_tick'] + self.response_ticks):
            raise UnsupportedDevice('Post-homing iris pending ledger changed')
        self._completed_homing(post['owner'])
        events = [self.words[i] for i in indices]
        configuration = self._axis_configuration('iris', POST_IRIS_WORDS)
        retained = post.get('configuration')
        if (any(type(e) is not dict or type(e.get('word')) is not int
                or type(e.get('cpu')) is not int or e['cpu'] != post['owner']
                or type(e.get('tick')) is not int or not 0 <= e['tick'] <= self.board.virtual_ticks for e in events)
                or tuple(e['word'] for e in events) != POST_IRIS_WORDS
                or events[-1]['tick'] != post['requested_tick']
                or any(a['tick'] > b['tick'] for a, b in zip(events, events[1:]))
                or type(retained) is not dict or any(type(v) is not int for v in retained.values())
                or retained != configuration
                or any(type(self.registers.get(a)) is not int or self.registers[a] != v for a, v in configuration.items())):
            raise UnsupportedDevice('Post-homing iris retained transaction/configuration changed')
        if (post['state'] == 'awaiting-cleanup'
                and (type(post.get('delivery_tick')) is not int
                     or not post['due_tick'] <= post['delivery_tick'] <= self.board.virtual_ticks)):
            raise UnsupportedDevice('Post-homing iris cleanup lacks its timed response')

    def _post_complete_option_word(self, core, word):
        """One stock A17338/A17260(0,1) option write after actual cleanup.

        The completed first response is historical: its retained7C80 became
        live7C00 through the stock cleanup. Validate the archived tail and
        replay the accepted wire bank, without changing that first ledger.
        No firmware fields, function PCs or callback predicates are consumed.
        """
        if self.post_complete_option is not None and self.continuation_profile in POST_COMPLETE_REPEAT_BASES:
            marker=self._validate_passive_iris_options(core)
            if (word!=0x7c80 or len(marker['word_indices'])>=POST_COMPLETE_REPEAT_LIMIT
                    or len(self.words)!=marker['original_option']['word_index']+1+len(marker['word_indices'])
                    or any(x is not None for x in (self.controller_reinit,self.controller_close,self.focus_reentry,
                                                   getattr(self,'iris_reentry',None)))):
                raise UnsupportedDevice('Declared passive iris extra-option word budget or value changed')
            option=deepcopy(self.post_complete_option)
            option['followup']['word_indices'].append(len(self.words))
            return option
        if word != 0x7c80 or self.post_complete_option is not None:
            raise UnsupportedDevice('Completed iris permits only one declared passive7C80 option word')
        cleanup = self._completed_post_projection(core)
        if len(self.words) != cleanup + 1 or self.registers.get(0x7c00) != 0:
            raise UnsupportedDevice('Passive completed-iris option live bank differs from actual cleanup')
        return dict(state='retained', owner=core, word_index=len(self.words),
                    cleanup_index=cleanup, word=word, tick=self.board.virtual_ticks,
                    prior_control=0, responses=0)

    def arm_passive_iris_options(self, profile):
        """Declare two extra nonstrobe words after the original once-only option.

        A17338/A17260(0,1) can repeat during preview initialization. Only the
        actual wire write is retained; stock owns its following A16D0C call.
        This deliberately changes the host fixture profile, never guest RAM.
        """
        from . import boot
        options=getattr(self.board,'_portable_factory_options',None)
        if (type(profile) is not str or profile not in POST_COMPLETE_REPEAT_BASES
                or self.continuation_profile!=POST_COMPLETE_REPEAT_BASES.get(profile)
                or type(options) is not dict
                or options.get('lens_response_profile')!=HOMING_PROFILE
                or options.get('lens_continuation_profile')!=self.continuation_profile
                or type(options.get('lens_response_ticks')) is not int
                or options['lens_response_ticks']!=self.response_ticks
                or getattr(self.board,'failure',None) is not None or self.board.scheduled
                or [d for d in self.board.devices if type(d) is LensHomingFixture]!=[self]
                or boot.LENS_CONTINUATION_PROFILES not in (_SINGLE_CONTINUATION_PROFILES,CONTINUATION_PROFILES)
                or any(x is not None for x in (self.controller_reinit,self.controller_close,self.focus_reentry,
                                               getattr(self,'iris_reentry',None)))
                or type(self.post_complete_option) is not dict):
            raise UnsupportedDevice('Passive iris repeat declaration requires the original completed once-only factory profile')
        option=self.post_complete_option;owner=option.get('owner')
        if type(owner) is not int or owner not in (0,1):
            raise UnsupportedDevice('Passive iris repeat declaration lacks its original typed owner')
        anchor=self._controller_anchor(owner);tick=self.board.virtual_ticks
        if (type(tick) is not int or not option['tick']<=tick<1<<64
                or len(self.words)!=anchor or anchor+POST_COMPLETE_REPEAT_LIMIT>self.WORD_LIMIT
                or self.registers.get(0x7c00)!=0x80):
            raise UnsupportedDevice('Passive iris repeat declaration requires an untouched first option and bounded time/words')
        replacement=deepcopy(option)
        replacement['followup']=dict(profile=profile,original_profile=self.continuation_profile,owner=owner,armed_tick=tick,
                                     original_option=deepcopy(option),word_indices=[])
        # Reconstruction inputs change together, only after every preflight.
        self.post_complete_option=replacement;self.continuation_profile=profile
        self.board._portable_factory_options=dict(options,lens_continuation_profile=profile)
        boot.LENS_CONTINUATION_PROFILES=CONTINUATION_PROFILES
        boot.LensHomingFixture=LensHomingFixture
        self.board.record('Declared passive iris option followup armed',profile=profile,owner=owner,
                          armed_tick=tick,extra_word_limit=POST_COMPLETE_REPEAT_LIMIT,
                          limits='Exact7C80 only; no IRQ, timing response, motion, counter, GPIO or guest completion')
        return deepcopy(replacement['followup'])

    def _validate_passive_iris_options(self, core):
        option=self.post_complete_option;marker=option.get('followup') if type(option) is dict else None
        options=getattr(self.board,'_portable_factory_options',None)
        if (self.continuation_profile not in POST_COMPLETE_REPEAT_BASES or type(core) is not int
                or type(marker) is not dict or set(marker)!={'profile','original_profile','owner','armed_tick','original_option','word_indices'}
                or marker.get('profile')!=self.continuation_profile
                or marker.get('original_profile')!=POST_COMPLETE_REPEAT_BASES[self.continuation_profile]
                or type(marker.get('owner')) is not int or marker['owner']!=core
                or type(marker.get('armed_tick')) is not int
                or type(marker.get('original_option')) is not dict
                or {k:v for k,v in option.items() if k!='followup'}!=marker['original_option']
                or type(marker.get('word_indices')) is not list
                or len(marker['word_indices'])>POST_COMPLETE_REPEAT_LIMIT
                or type(options) is not dict
                or options.get('lens_continuation_profile')!=self.continuation_profile
                or options.get('lens_response_profile')!=HOMING_PROFILE
                or type(options.get('lens_response_ticks')) is not int
                or options['lens_response_ticks']!=self.response_ticks):
            raise UnsupportedDevice('Passive iris extra-option declaration or original ledger changed')
        view=copy(self);view.post_complete_option=marker['original_option']
        anchor=view._controller_anchor(core)
        if (not marker['original_option']['tick']<=marker['armed_tick']<=self.board.virtual_ticks
                or any(type(i) is not int for i in marker['word_indices'])
                or marker['word_indices']!=list(range(anchor,anchor+len(marker['word_indices'])))
                or anchor+len(marker['word_indices'])>len(self.words)
                or any(e['cpu']!=core or e['word']!=0x7c80 or e['tick']<marker['armed_tick']
                       for e in self.words[anchor:anchor+len(marker['word_indices'])])
                or len(self.words)==anchor+len(marker['word_indices']) and self.registers.get(0x7c00)!=0x80):
            raise UnsupportedDevice('Passive iris extra-option owner/order/wire projection changed')
        return marker

    def _completed_post_projection(self, core):
        """Validate the first completed response independently of later writes."""
        post = self.post_iris
        indices = post.get('word_indices')
        cleanup = post.get('cleanup_index')
        if (post.get('state') != 'complete'
                or type(post.get('owner')) is not int or post['owner'] != core
                or type(post.get('responses')) is not int or post['responses'] != 1
                or type(indices) is not list or len(indices) != len(POST_IRIS_WORDS)
                or any(type(i) is not int or not 0 <= i < len(self.words) for i in indices)
                or indices != list(range(indices[0], indices[0] + len(indices)))
                or type(cleanup) is not int or cleanup != indices[-1] + 1
                or cleanup >= len(self.words)
                or any(type(post.get(key)) is not int
                       for key in ('requested_tick', 'due_tick', 'delivery_tick'))
                or post['due_tick'] != post['requested_tick'] + self.response_ticks
                or not post['due_tick'] <= post['delivery_tick'] <= self.board.virtual_ticks):
            raise UnsupportedDevice('Passive completed-iris option lacks its exact archived response/cleanup')
        self._completed_homing(core)
        events = [self.words[i] for i in indices]
        cleanup_event = self.words[cleanup]
        if (any(type(e) is not dict or type(e.get('cpu')) is not int or e['cpu'] != core
                       or type(e.get('word')) is not int or type(e.get('tick')) is not int
                       or not 0 <= e['tick'] <= self.board.virtual_ticks for e in events)
                or tuple(e['word'] for e in events) != POST_IRIS_WORDS
                or any(a['tick'] > b['tick'] for a, b in zip(events, events[1:]))
                or events[-1]['tick'] != post['requested_tick']
                or type(post.get('configuration')) is not dict
                or any(type(a) is not int or type(v) is not int for a, v in post['configuration'].items())
                or post['configuration'] != self._axis_configuration('iris', POST_IRIS_WORDS)
                or type(cleanup_event) is not dict
                or type(cleanup_event.get('cpu')) is not int
                or type(cleanup_event.get('tick')) is not int
                or not post['delivery_tick'] <= cleanup_event['tick'] <= self.board.virtual_ticks
                or cleanup_event != dict(cpu=core, word=0x7c00, tick=cleanup_event['tick'])):
            raise UnsupportedDevice('Passive completed-iris option archived words/configuration changed')
        projection, previous_tick = {}, 0
        for event in self.words:
            if (type(event) is not dict or type(event.get('cpu')) is not int or event['cpu'] not in (0, 1)
                    or type(event.get('word')) is not int or not 0 <= event['word'] <= 0xffff
                    or type(event.get('tick')) is not int
                    or not previous_tick <= event['tick'] <= self.board.virtual_ticks):
                raise UnsupportedDevice('Passive completed-iris option wire projection changed')
            projection[event['word'] & 0xfc00] = event['word'] & 0x3ff
            previous_tick = event['tick']
        if (any(type(a) is not int or type(v) is not int for a, v in self.registers.items())
                or projection != self.registers):
            raise UnsupportedDevice('Passive completed-iris option live bank differs from actual cleanup')
        # Historical cold axis tails and later focus words must remain exact;
        # an independently changed retained bank cannot recreate their archive.
        for axis in COMMANDS:
            sequence = COMMANDS[axis][0][1]
            end = INITIAL_INDICES[axis]
            initial = self.words[end-len(sequence)+1:end+1]
            if (tuple(e['word'] for e in initial) != sequence
                    or any(e['cpu'] != core for e in initial)):
                raise UnsupportedDevice('Completed lens continuation changed its initial axis archive')
        history = self.words[INITIAL_INDICES['iris']+1:indices[0]]
        expected = []
        for spec in FOCUS_COMMANDS[1:]:
            expected.extend(spec[1])
            if spec[4] == 8: expected.append(0xd801)
        expected.extend(CLEANUP['focus'])
        focus = [e['word'] for e in history if e['word'] >> 14 in (0, 3)]
        iris = [e['word'] for e in history if e['word'] >> 14 == 1]
        if (focus != expected or not iris or iris[0] != 0x7c00
                or len(iris) < 2 or any(w != 0x7c80 for w in iris[1:])
                or any(e['word'] >> 14 == 2 or e['cpu'] != core for e in history)):
            raise UnsupportedDevice('Completed lens continuation changed its cold homing/cleanup archive')
        return cleanup

    def _controller_anchor(self, core):
        """The archived first response and optional passive word own this wire."""
        cleanup = self._completed_post_projection(core)
        anchor = cleanup + 1
        option = self.post_complete_option
        followup=option.get('followup') if type(option) is dict else None
        if followup is not None:option={k:v for k,v in option.items() if k!='followup'}
        if option is not None:
            if (type(option) is not dict or set(option) != {
                    'state', 'owner', 'word_index', 'cleanup_index', 'word', 'tick', 'prior_control', 'responses'}
                    or any(type(option.get(key)) is not int for key in (
                        'owner', 'word_index', 'cleanup_index', 'word', 'tick', 'prior_control', 'responses'))
                    or option != dict(state='retained', owner=core, word_index=anchor,
                                      cleanup_index=cleanup, word=0x7c80, tick=option['tick'],
                                      prior_control=0, responses=0)
                    or anchor >= len(self.words)
                    or self.words[anchor] != dict(cpu=core, word=0x7c80, tick=option['tick'])):
                raise UnsupportedDevice('Global lens initializer changed its passive option anchor')
            anchor += 1
        if followup is not None:
            marker=self._validate_passive_iris_options(core)
            anchor+=len(marker['word_indices'])
        return anchor

    def _validate_controller_archive(self, core, current, sequence, anchor):
        if (type(current) is not dict or set(current) != {
                'state', 'owner', 'anchor_index', 'word_indices', 'started_tick', 'completed_tick', 'responses'}
                or type(current.get('owner')) is not int or current['owner'] != core
                or type(current.get('anchor_index')) is not int or current['anchor_index'] != anchor
                or type(current.get('responses')) is not int or current['responses'] != 0
                or type(current.get('started_tick')) is not int
                or type(current.get('word_indices')) is not list
                or not 1 <= len(current['word_indices']) <= len(sequence)
                or any(type(i) is not int for i in current['word_indices'])
                or current['word_indices'] != list(range(anchor, anchor + len(current['word_indices'])))
                or current['word_indices'][-1] >= len(self.words)):
            raise UnsupportedDevice('Global lens configuration archive changed')
        events = [self.words[i] for i in current['word_indices']]
        complete = len(events) == len(sequence)
        if (current['state'] != ('complete' if complete else 'collecting')
                or current['started_tick'] != events[0]['tick']
                or (complete and (type(current['completed_tick']) is not int
                                  or current['completed_tick'] != events[-1]['tick']))
                or (not complete and current['completed_tick'] is not None)
                or tuple(e['word'] for e in events) != sequence[:len(events)]
                or any(e['cpu'] != core for e in events)):
            raise UnsupportedDevice('Global lens configuration archived transaction changed')

    def _controller_word(self, core, word, current, sequence, anchor):
        if current is None:
            if len(self.words) != anchor or word != sequence[0]:
                raise UnsupportedDevice('Global lens initializer lacks its immediate completed-cycle anchor')
            current = dict(state='collecting', owner=core, anchor_index=anchor,
                           word_indices=[], started_tick=self.board.virtual_ticks,
                           completed_tick=None, responses=0)
        else:
            self._validate_controller_archive(core, current, sequence, anchor)
            if (current['state'] != 'collecting'
                    or current['word_indices'] != list(range(anchor, len(self.words)))):
                raise UnsupportedDevice('Global lens initializer partial ledger changed or already completed')
        indices = current['word_indices'] + [len(self.words)]
        events = [self.words[i] for i in current['word_indices']]
        events.append(dict(cpu=core, word=word, tick=self.board.virtual_ticks))
        if (tuple(e['word'] for e in events) != sequence[:len(indices)]
                or any(e['cpu'] != core for e in events)
                or any(a['tick'] > b['tick'] for a, b in zip(events, events[1:]))):
            raise UnsupportedDevice('Global lens initializer requires its whole exact ordered same-owner transaction')
        updated = deepcopy(current)
        updated['word_indices'] = indices
        if len(indices) == len(sequence):
            updated.update(state='complete', completed_tick=self.board.virtual_ticks)
        return updated

    def _controller_reinit_word(self, core, word):
        """Retain one exact stock global initializer, without external effects."""
        anchor = self._controller_anchor(core)
        return self._controller_word(core, word, self.controller_reinit, CONTROLLER_REINIT_WORDS, anchor)

    def _controller_close_word(self, core, word):
        """One separate A075B8(3) close after the completed21-word request."""
        anchor = self._controller_anchor(core)
        self._validate_controller_archive(core, self.controller_reinit, CONTROLLER_REINIT_WORDS, anchor)
        if self.controller_reinit['state'] != 'complete':
            raise UnsupportedDevice('Global lens close requires its whole completed21-word initializer')
        anchor += len(CONTROLLER_REINIT_WORDS)
        return self._controller_word(core, word, self.controller_close, CONTROLLER_CLOSE_WORDS, anchor)

    def _focus_reentry_anchor(self, core):
        anchor = self._controller_anchor(core)
        self._validate_controller_archive(core, self.controller_reinit, CONTROLLER_REINIT_WORDS, anchor)
        anchor += len(CONTROLLER_REINIT_WORDS)
        self._validate_controller_archive(core, self.controller_close, CONTROLLER_CLOSE_WORDS, anchor)
        if (self.controller_reinit['state'] != 'complete' or self.controller_close['state'] != 'complete'
                or type(self.coordinate) is not int or self.coordinate != -62
                or type(self.gpio_level) is not int or self.gpio_level != 0):
            raise UnsupportedDevice('Focus reentry requires the unchanged complete cold/21/20 archive')
        return anchor + len(CONTROLLER_CLOSE_WORDS)

    def _focus_reentry_devices(self, level):
        controls = [d for d in self.board.devices if type(d) is BootControls]
        pulses = [d for d in self.board.devices if type(d) is PulseOutputs]
        if (len(controls) != 1 or len(pulses) != 1
                or pulses[0].counter_input_profile != PulseOutputs.COUNTER_LENS_PROFILE
                or controls[0].gpio_input_pin(0, 0) != level
                or type(controls[0].registers.get(BootControls.GPIO + 0x14)) is not int
                or controls[0].registers[BootControls.GPIO + 0x14] & 1 != level):
            raise UnsupportedDevice('Focus reentry requires its unique unchanged GPIO/counter inputs')
        return controls[0], pulses[0]

    def _focus_pending_archive(self, owner, stage, pending):
        spec = FOCUS_REENTRY_COMMANDS[stage]
        indices = pending.get('word_indices') if type(pending) is dict else None
        if (type(pending) is not dict or pending.get('stage') != stage
                or type(pending.get('stage')) is not int or pending.get('phase') != spec[0]
                or type(pending.get('owner')) is not int or pending['owner'] != owner
                or type(indices) is not list or len(indices) != len(spec[1])
                or any(type(i) is not int or not 0 <= i < len(self.words) for i in indices)
                or indices != list(range(indices[0], indices[0] + len(indices)))
                or any(type(pending.get(k)) is not int for k in ('requested_tick', 'due_tick'))):
            raise UnsupportedDevice('Focus reentry request archive changed')
        events = [self.words[i] for i in indices]
        if (tuple(e['word'] for e in events) != spec[1]
                or any(e['cpu'] != owner for e in events)
                or events[-1]['tick'] != pending['requested_tick']
                or type(pending.get('configuration')) is not dict
                or any(type(a) is not int or type(v) is not int for a, v in pending['configuration'].items())
                or pending['configuration'] != self._axis_configuration('focus', spec[1])):
            raise UnsupportedDevice('Focus reentry exact request/configuration changed')
        counters = pending.get('counters')
        if spec[2]:
            if (type(counters) is not dict or set(counters) != {0, 4}
                    or any(type(v) is not tuple or len(v) != 2
                           or any(type(n) is not int for n in v) or v[0] < 1 for v in counters.values())):
                raise UnsupportedDevice('Focus reentry counter archive changed')
        elif counters is not None:
            raise UnsupportedDevice('Focus reentry zero-quantity phase must not supply counters')
        stopped = pending['state'] in ('pending-stop-response', 'completed') and spec[4] == 8
        base_tick = pending['requested_tick']
        if stopped:
            index = pending.get('stop_index')
            if (type(index) is not int or index != indices[-1] + 1 or index >= len(self.words)
                    or type(pending.get('stop_tick')) is not int
                    or self.words[index] != dict(cpu=owner, word=0xd801, tick=pending['stop_tick'])
                    or not pending['requested_tick'] <= pending['stop_tick'] <= self.board.virtual_ticks):
                raise UnsupportedDevice('Focus reentry search requires its actual ordered stop word')
            base_tick = pending['stop_tick']
        if pending['due_tick'] != base_tick + self.response_ticks:
            raise UnsupportedDevice('Focus reentry response timing changed')
        self._focus_delivery_archive(stage, pending)
        return spec

    def _focus_delivery_archive(self, stage, pending):
        spec = FOCUS_REENTRY_COMMANDS[stage]
        delivered = pending.get('deliveries')
        state = pending.get('state')
        count = 0 if state == 'pending-input' else 2 if state == 'completed' and spec[4] == 8 else 1
        if type(delivered) is not list or len(delivered) != count:
            raise UnsupportedDevice('Focus reentry external response archive changed')
        after = FOCUS_REENTRY_STARTS[stage] + (-spec[2] if spec[3] else spec[2])
        for i, event in enumerate(delivered):
            if (type(event) is not dict or set(event) != {'phase','tick','irq','cpu','edges4','edges0','coordinate','gpio'}
                    or any(type(event.get(k)) is not int for k in ('tick','irq','cpu','edges4','edges0','coordinate','gpio'))
                    or not (pending['requested_tick']+self.response_ticks if i == 0 else pending['stop_tick']+self.response_ticks) <= event['tick'] <= self.board.virtual_ticks
                    or event != dict(phase=spec[0], tick=event['tick'], irq=spec[4] if i == 0 else 14,
                                     cpu=pending['owner'], edges4=spec[2] if i == 0 else 0,
                                     edges0=spec[2]//4 if i == 0 else 0, coordinate=after, gpio=int(after>=8))):
                raise UnsupportedDevice('Focus reentry physical input archive changed')
        if spec[2]:
            expected = spec[2] if count else 0
            if (pending['counters'][4][1] != expected or pending['counters'][0][1] != expected//4):
                raise UnsupportedDevice('Focus reentry counter quantity archive changed')
        if spec[4] == 8 and state in ('pending-stop-response','completed') and pending['stop_tick'] < delivered[0]['tick']:
            raise UnsupportedDevice('Focus reentry stop precedes its GPIO input')

    def _validate_focus_reentry(self, current, archive_end=None):
        if archive_end is None and self._continuation_profile() == IRIS_REENTRY_PROFILE:
            iris = getattr(self, 'iris_reentry', None)
            if type(iris) is dict:
                archive_end = iris.get('anchor_index')
        if archive_end is not None:
            if (self._continuation_profile() != IRIS_REENTRY_PROFILE
                    or type(archive_end) is not int or not 0 <= archive_end <= len(self.words)
                    or type(current) is not dict or current.get('pending') is not None
                    or current.get('next_command') != len(FOCUS_REENTRY_COMMANDS)
                    or current.get('cleanup') != len(FOCUS_REENTRY_CLEANUP)):
                raise UnsupportedDevice('Iris reentry requires a complete bounded focus archive')
        if (self._continuation_profile() not in (FOCUS_REENTRY_PROFILE, IRIS_REENTRY_PROFILE)
                or type(current) is not dict or set(current) != {
                'owner', 'anchor_index', 'next_command', 'buffer', 'pending', 'cleanup',
                'responses', 'coordinate', 'gpio_level', 'completed', 'deliveries'}
                or type(current.get('owner')) is not int or current['owner'] not in (0, 1)
                or any(type(current.get(k)) is not int for k in (
                    'anchor_index', 'next_command', 'cleanup', 'responses', 'coordinate', 'gpio_level'))
                or not 0 <= current['next_command'] <= len(FOCUS_REENTRY_COMMANDS)
                or current['responses'] != current['next_command']
                or not 0 <= current['cleanup'] <= len(FOCUS_REENTRY_CLEANUP)
                or type(current['buffer']) is not list or type(current['completed']) is not list
                or len(current['completed']) != current['next_command']
                or type(current['deliveries']) is not list
                or current['anchor_index'] != self._focus_reentry_anchor(current['owner'])):
            raise UnsupportedDevice('Focus reentry generation ledger changed')
        expected, deliveries = [], []
        for stage, done in enumerate(current['completed']):
            spec = self._focus_pending_archive(current['owner'], stage, done)
            if done.get('state') != 'completed':
                raise UnsupportedDevice('Focus reentry completed response archive changed')
            if done['word_indices'][0] != current['anchor_index'] + len(expected):
                raise UnsupportedDevice('Focus reentry completed command ordinal changed')
            expected.extend(spec[1])
            if spec[4] == 8: expected.append(0xd801)
            deliveries.extend(done.get('deliveries', []))
        stage = current['next_command']
        pending = current['pending']
        after_edge = False
        if pending is not None:
            if stage == len(FOCUS_REENTRY_COMMANDS) or current['buffer'] != [] or current['cleanup'] != 0:
                raise UnsupportedDevice('Focus reentry pending phase order changed')
            spec = self._focus_pending_archive(current['owner'], stage, pending)
            if pending.get('state') not in ('pending-input', 'awaiting-stop-word', 'pending-stop-response'):
                raise UnsupportedDevice('Focus reentry pending state changed')
            if pending['word_indices'][0] != current['anchor_index'] + len(expected):
                raise UnsupportedDevice('Focus reentry pending command ordinal changed')
            expected.extend(spec[1])
            if pending['state'] == 'pending-stop-response': expected.append(0xd801)
            after_edge = pending['state'] != 'pending-input'
            deliveries.extend(pending.get('deliveries', []))
            if (pending['configuration'] != self._axis_configuration('focus', spec[1])
                    or any(self.registers.get(a) != v for a, v in pending['configuration'].items())):
                raise UnsupportedDevice('Focus reentry pending register bank changed')
            _, pulses = self._focus_reentry_devices(current['gpio_level'])
            if spec[2] and self._counter_state(pulses) != pending['counters']:
                raise UnsupportedDevice('Focus reentry live counters changed outside declared input')
        elif stage < len(FOCUS_REENTRY_COMMANDS):
            if current['cleanup'] != 0 or any(type(i) is not int for i in current['buffer']):
                raise UnsupportedDevice('Focus reentry partial command ledger changed')
            prefix = FOCUS_REENTRY_COMMANDS[stage][1][:len(current['buffer'])]
            if len(current['buffer']) >= len(FOCUS_REENTRY_COMMANDS[stage][1]):
                raise UnsupportedDevice('Focus reentry incomplete strobe ledger changed')
            expected.extend(prefix)
        else:
            if current['buffer'] != []: raise UnsupportedDevice('Focus reentry cleanup has a partial command')
            expected.extend(FOCUS_REENTRY_CLEANUP[:current['cleanup']])
        events = self.words[current['anchor_index']:archive_end]
        if (tuple(e['word'] for e in events) != tuple(expected)
                or any(e['cpu'] != current['owner'] for e in events)
                or (current['buffer'] and current['buffer'] != list(range(
                    len(self.words)-len(current['buffer']), len(self.words))))
                or deliveries != current['deliveries']):
            raise UnsupportedDevice('Focus reentry ordered generation archive changed')
        coordinate = 230 if stage == len(FOCUS_REENTRY_COMMANDS) else FOCUS_REENTRY_STARTS[stage]
        if after_edge:
            spec = FOCUS_REENTRY_COMMANDS[stage]
            coordinate += -spec[2] if spec[3] else spec[2]
        if current['coordinate'] != coordinate or current['gpio_level'] != int(coordinate >= 8):
            raise UnsupportedDevice('Focus reentry coordinate/threshold ledger changed')
        self._focus_reentry_devices(current['gpio_level'])

    def _focus_reentry_word(self, core, word, prospective):
        current = deepcopy(self.focus_reentry)
        if current is None:
            anchor = self._focus_reentry_anchor(core)
            if len(self.words) != anchor or word != FOCUS_REENTRY_COMMANDS[0][1][0]:
                raise UnsupportedDevice('Focus reentry requires its immediate complete21/20 anchor')
            current = dict(owner=core, anchor_index=anchor, next_command=0, buffer=[], pending=None,
                           cleanup=0, responses=0, coordinate=-62, gpio_level=0, completed=[], deliveries=[])
        self._validate_focus_reentry(current)
        if core != current['owner']:
            raise UnsupportedDevice('Focus reentry completed word changed owner')
        pending = current['pending']
        if pending is not None:
            if pending['state'] != 'awaiting-stop-word' or word != 0xd801:
                raise UnsupportedDevice('Focus reentry requires its ordered physical input then actual stop')
            pending.update(state='pending-stop-response', stop_index=len(self.words),
                           stop_tick=self.board.virtual_ticks, due_tick=self.board.virtual_ticks+self.response_ticks)
            return current
        stage = current['next_command']
        if stage == len(FOCUS_REENTRY_COMMANDS):
            if (current['cleanup'] >= len(FOCUS_REENTRY_CLEANUP)
                    or word != FOCUS_REENTRY_CLEANUP[current['cleanup']]):
                raise UnsupportedDevice('Focus reentry permits only its one exact cleanup')
            current['cleanup'] += 1
            return current
        spec = FOCUS_REENTRY_COMMANDS[stage]
        indices = current['buffer'] + [len(self.words)]
        if word != spec[1][len(indices)-1]:
            raise UnsupportedDevice('Focus reentry requires its whole exact ordered request')
        current['buffer'] = indices
        if len(indices) == len(spec[1]):
            configuration = self._axis_configuration('focus', spec[1])
            if any(prospective.get(a) != v for a, v in configuration.items()):
                raise UnsupportedDevice('Focus reentry whole request retained bank changed')
            _, pulses = self._focus_reentry_devices(current['gpio_level'])
            counters = self._counter_state(pulses) if spec[2] else None
            if counters is not None and any(value != 0 for generation, value in counters.values()):
                raise UnsupportedDevice('Focus reentry finite input requires stock reset counters')
            current['buffer'] = []
            current['pending'] = dict(stage=stage, phase=spec[0], owner=core, word_indices=indices,
                requested_tick=self.board.virtual_ticks, due_tick=self.board.virtual_ticks+self.response_ticks,
                state='pending-input', configuration=configuration, counters=counters, deliveries=[])
            self.board.record('Declared focus reentry transaction retained', profile=FOCUS_REENTRY_PROFILE,
                              phase=spec[0], owner=core, word_indices=indices)
        return current

    def _focus_reentry_due(self):
        current = self.focus_reentry
        if current is None: return None
        self._validate_focus_reentry(current)
        pending = current['pending']
        if pending is None or pending['state'] == 'awaiting-stop-word': return None
        if self.board.virtual_ticks < pending['due_tick']: return None
        spec = FOCUS_REENTRY_COMMANDS[pending['stage']]
        irq = 14 if pending['state'] == 'pending-stop-response' else spec[4]
        self._route(irq, pending['owner'])
        return spec, irq

    def _deliver_focus_reentry(self, spec, irq):
        current = self.focus_reentry
        pending = current['pending']
        controls, pulses = self._focus_reentry_devices(current['gpio_level'])
        edges = spec[2] if pending['state'] == 'pending-input' else 0
        if edges:
            pulses.add_counter_edges(4, edges, source=PulseOutputs.COUNTER_LENS_PROFILE)
            pulses.add_counter_edges(0, edges//4, source=PulseOutputs.COUNTER_LENS_PROFILE)
            current['coordinate'] += -edges if spec[3] else edges
            pending['counters'] = self._counter_state(pulses)
        level = int(current['coordinate'] >= 8)
        if level != current['gpio_level']:
            controls.set_gpio_input_pin(0, 0, level, source=FOCUS_REENTRY_PROFILE)
            current['gpio_level'] = level
        if irq == 8:
            pending['state'] = 'awaiting-stop-word'
        delivery = dict(phase=spec[0], tick=self.board.virtual_ticks, irq=irq, cpu=pending['owner'],
                        edges4=edges, edges0=edges//4, coordinate=current['coordinate'], gpio=current['gpio_level'])
        pending['deliveries'].append(delivery)
        current['deliveries'].append(deepcopy(delivery))
        if irq != 8:
            pending['state'] = 'completed'
            current['completed'].append(pending)
            current['pending'] = None
            current['next_command'] += 1
            current['responses'] += 1
        self.board.queue_irq(delivery['cpu'], irq)
        self.board.record('Declared focus reentry external input', profile=FOCUS_REENTRY_PROFILE, **delivery,
                          limits='Named unmeasured lattice only; stock ISR/queues/callback own completion')

    def _iris_reentry_anchor(self):
        focus = self.focus_reentry
        if type(focus) is not dict or type(focus.get('anchor_index')) is not int:
            raise UnsupportedDevice('Iris reentry lacks its completed focus lifetime')
        end = focus['anchor_index'] + sum(len(s[1]) + int(s[4] == 8) for s in FOCUS_REENTRY_COMMANDS)
        end += len(FOCUS_REENTRY_CLEANUP)
        self._validate_focus_reentry(focus, archive_end=end)
        return end

    def _validate_iris_reentry(self, current):
        keys = {'state', 'owner', 'anchor_index', 'word_indices', 'configuration',
                'requested_tick', 'due_tick', 'delivery_tick', 'responses', 'cleanup_index'}
        if (self._continuation_profile() != IRIS_REENTRY_PROFILE
                or type(current) is not dict or set(current) != keys
                or type(current['owner']) is not int or current['owner'] not in (0, 1)
                or current['owner'] != self.focus_reentry.get('owner')
                or type(current['anchor_index']) is not int
                or current['anchor_index'] != self._iris_reentry_anchor()
                or type(current['word_indices']) is not list
                or not 0 < len(current['word_indices']) <= len(IRIS_REENTRY_WORDS)
                or any(type(i) is not int for i in current['word_indices'])
                or current['word_indices'] != list(range(current['anchor_index'],
                    current['anchor_index'] + len(current['word_indices'])))
                or current['word_indices'][-1] >= len(self.words)
                or type(current['responses']) is not int):
            raise UnsupportedDevice('Iris reentry typed generation archive changed')
        events = [self.words[i] for i in current['word_indices']]
        if (tuple(e['word'] for e in events) != IRIS_REENTRY_WORDS[:len(events)]
                or any(e['cpu'] != current['owner'] for e in events)):
            raise UnsupportedDevice('Iris reentry ordered request archive changed')
        end = current['word_indices'][-1] + 1
        state = current['state']
        if len(events) < len(IRIS_REENTRY_WORDS):
            if (state != 'collecting' or current['responses'] != 0
                    or any(current[k] is not None for k in keys - {
                        'state', 'owner', 'anchor_index', 'word_indices', 'responses'})):
                raise UnsupportedDevice('Iris reentry partial request changed')
        else:
            configuration = self._axis_configuration('iris', IRIS_REENTRY_WORDS)
            if (state not in ('pending-response', 'awaiting-cleanup', 'complete')
                    or type(current['configuration']) is not dict
                    or any(type(a) is not int or type(v) is not int
                           for a, v in current['configuration'].items())
                    or current['configuration'] != configuration
                    or type(current['requested_tick']) is not int
                    or current['requested_tick'] != events[-1]['tick']
                    or type(current['due_tick']) is not int
                    or current['due_tick'] != current['requested_tick'] + self.response_ticks):
                raise UnsupportedDevice('Iris reentry complete configuration/timing changed')
            if state == 'pending-response':
                if current['responses'] != 0 or current['delivery_tick'] is not None:
                    raise UnsupportedDevice('Iris reentry pending response changed')
            elif (current['responses'] != 1 or type(current['delivery_tick']) is not int
                    or not current['due_tick'] <= current['delivery_tick'] <= self.board.virtual_ticks):
                raise UnsupportedDevice('Iris reentry response archive changed')
            if state == 'complete':
                if (type(current['cleanup_index']) is not int or current['cleanup_index'] != end
                        or end >= len(self.words)
                        or self.words[end]['cpu'] != current['owner']
                        or self.words[end]['word'] != 0x7c00
                        or self.words[end]['tick'] < current['delivery_tick']):
                    raise UnsupportedDevice('Iris reentry lacks its actual same-owner cleanup')
                configuration[0x7c00] = 0
                end += 1
            elif current['cleanup_index'] is not None:
                raise UnsupportedDevice('Iris reentry cleanup precedes its response')
            if any(self.registers.get(a) != v for a, v in configuration.items()):
                raise UnsupportedDevice('Iris reentry retained axis bank changed')
        if len(self.words) != end:
            raise UnsupportedDevice('Iris reentry permits only its bounded whole lifetime')

    def _iris_reentry_word(self, core, word, prospective):
        current = deepcopy(getattr(self, 'iris_reentry', None))
        if current is None:
            anchor = self._iris_reentry_anchor()
            if len(self.words) != anchor or core != self.focus_reentry['owner'] or word != IRIS_REENTRY_WORDS[0]:
                raise UnsupportedDevice('Iris reentry requires its immediate same-owner focus-completion anchor')
            current = dict(state='collecting', owner=core, anchor_index=anchor, word_indices=[],
                           configuration=None, requested_tick=None, due_tick=None,
                           delivery_tick=None, responses=0, cleanup_index=None)
        else:
            self._validate_iris_reentry(current)
        if core != current['owner']:
            raise UnsupportedDevice('Iris reentry completed word changed owner')
        if current['state'] == 'collecting':
            index = len(current['word_indices'])
            if word != IRIS_REENTRY_WORDS[index]:
                raise UnsupportedDevice('Iris reentry requires its whole exact ordered request')
            current['word_indices'].append(len(self.words))
            if index + 1 == len(IRIS_REENTRY_WORDS):
                configuration = self._axis_configuration('iris', IRIS_REENTRY_WORDS)
                if any(prospective.get(a) != v for a, v in configuration.items()):
                    raise UnsupportedDevice('Iris reentry strobe differs from its retained configuration')
                self._route(12, core)
                current.update(state='pending-response', configuration=configuration,
                    requested_tick=self.board.virtual_ticks, due_tick=self.board.virtual_ticks+self.response_ticks)
        elif current['state'] == 'awaiting-cleanup' and word == 0x7c00:
            current.update(state='complete', cleanup_index=len(self.words))
        else:
            raise UnsupportedDevice('Iris reentry permits only its one response and actual7C00 cleanup')
        return current

    def _iris_reentry_due(self):
        current = getattr(self, 'iris_reentry', None)
        if current is None: return None
        self._validate_iris_reentry(current)
        if current['state'] != 'pending-response' or self.board.virtual_ticks < current['due_tick']:
            return None
        self._route(12, current['owner'])
        return current['owner']

    def _validate(self, axis, pending):
        if (type(pending) is not dict or type(pending.get('stage')) is not int
                or pending['stage'] != self.next_command[axis]
                or not 0 <= pending['stage'] < len(COMMANDS[axis])):
            raise UnsupportedDevice('Lens homing phase ledger changed')
        phase, sequence, edges, direction, irq, level = COMMANDS[axis][pending['stage']]
        if (pending.get('phase') != phase or type(pending.get('owner')) is not int or pending['owner'] not in (0, 1)
                or pending.get('state') not in ('pending-input', 'awaiting-stop-word', 'pending-stop-response')
                or type(pending.get('requested_tick')) is not int or type(pending.get('due_tick')) is not int
                or type(pending.get('word_indices')) is not list or len(pending['word_indices']) != len(sequence)
                or any(type(i) is not int or not 0 <= i < len(self.words) for i in pending['word_indices'])
                or any(a >= b for a, b in zip(pending['word_indices'], pending['word_indices'][1:]))):
            raise UnsupportedDevice('Lens homing request/timing ledger changed')
        events = [self.words[i] for i in pending['word_indices']]
        configuration = self._axis_configuration(axis, sequence)
        if (tuple(e['word'] for e in events) != sequence
                or any(type(e['word']) is not int or type(e['cpu']) is not int or e['cpu'] != pending['owner']
                       or type(e['tick']) is not int or not 0 <= e['tick'] <= self.board.virtual_ticks for e in events)
                or events[-1]['tick'] != pending['requested_tick']
                or any(a['tick'] > b['tick'] for a, b in zip(events, events[1:]))
                or pending.get('configuration') != configuration
                or any(type(self.registers.get(a)) is not int or self.registers[a] != v for a, v in configuration.items())):
            raise UnsupportedDevice('Lens homing retained transaction/configuration changed before external input')
        base_tick = pending['requested_tick']
        if pending['state'] == 'pending-stop-response':
            index = pending.get('stop_index')
            if (irq != 8 or type(index) is not int or not 0 <= index < len(self.words)
                    or self.words[index] != dict(cpu=pending['owner'], word=0xd801, tick=pending.get('stop_tick'))
                    or type(pending.get('stop_tick')) is not int
                    or not pending['requested_tick'] <= pending['stop_tick'] <= self.board.virtual_ticks):
                raise UnsupportedDevice('Lens search response requires its actual typed stop word')
            base_tick = pending['stop_tick']
        if pending['due_tick'] != base_tick + self.response_ticks:
            raise UnsupportedDevice('Lens homing response delay changed')
        if axis == 'focus':
            start = (0, 0, 7, 8, 72, 72, 7)[pending['stage']]
            after_edge = pending['state'] in ('awaiting-stop-word', 'pending-stop-response')
            expected = start + (-edges if direction else edges) if after_edge else start
            expected_level = level if after_edge else int(start >= 8)
            if (type(self.coordinate) is not int or self.coordinate != expected
                    or type(self.gpio_level) is not int or self.gpio_level != expected_level):
                raise UnsupportedDevice('Lens virtual coordinate/threshold ledger changed')
            if irq == 8 and not after_edge and (start + (-edges if direction else edges) >= 8) != bool(level):
                raise UnsupportedDevice('Lens virtual search does not reach its declared threshold')
        _, pulses = self._devices()
        if edges:
            counters = self._counter_state(pulses)
            if (type(pending.get('counters')) is not dict
                    or any(type(v) is not tuple or len(v) != 2 or any(type(n) is not int for n in v)
                           for v in pending['counters'].values())
                    or counters != pending['counters']):
                raise UnsupportedDevice('Lens feedback generation/count changed outside its ordered external event')
        elif pending.get('counters') is not None:
            raise UnsupportedDevice('Zero-quantity lens response must not claim counter progression')

    def _failure(self, error):
        if getattr(self.board, 'failure', None) is None:
            self.board.failure = dict(message=str(error), endpoint='SPI2 lens homing fixture', profile=HOMING_PROFILE)
            self.board.record('Declared lens homing unsupported boundary', **self.board.failure)

    def tick(self, board):
        if board is not self.board or getattr(board, 'failure', None) is not None:
            return
        try:
            iris_reentry_due = self._iris_reentry_due()
            reentry_due = self._focus_reentry_due()
            due = []
            post_due=False
            if self.post_iris['state']=='pending-response':
                if type(self.post_iris.get('due_tick')) is not int:
                    raise UnsupportedDevice('Post-homing iris due time changed')
                if board.virtual_ticks>=self.post_iris['due_tick']:
                    self._validate_post_iris()
                    self._route(12,self.post_iris['owner'])
                    post_due=True
            for axis, pending in self.pending.items():
                if pending is not None and pending['state'] != 'awaiting-stop-word':
                    if type(pending['due_tick']) is not int:
                        raise UnsupportedDevice('Lens homing due time changed')
                    if board.virtual_ticks >= pending['due_tick']:
                        self._validate(axis, pending)
                        spec = COMMANDS[axis][pending['stage']]
                        irq = 14 if pending['state'] == 'pending-stop-response' else spec[4]
                        self._route(irq, pending['owner'])
                        if irq == 8:
                            prospective = self.coordinate + (-spec[2] if spec[3] else spec[2])
                            if (prospective >= 8) != bool(spec[5]) or spec[5] == self.gpio_level:
                                raise UnsupportedDevice('Lens virtual threshold law differs from its command lattice')
                        due.append((axis, pending, spec, irq))
            # All due requests/routes/counters are checked before any effect.
            for axis, pending, spec, irq in due:
                controls, pulses = self._devices()
                edges = spec[2] if pending['state'] == 'pending-input' else 0
                if edges:
                    pulses.add_counter_edges(4, edges, source=PulseOutputs.COUNTER_LENS_PROFILE)
                    pulses.add_counter_edges(0, edges // 4, source=PulseOutputs.COUNTER_LENS_PROFILE)
                    self.coordinate += -edges if spec[3] else edges
                    pending['counters'] = self._counter_state(pulses)
                if irq == 8:
                    level = spec[5]
                    controls.set_gpio_input_pin(0, 0, level, source=HOMING_PROFILE)
                    self.gpio_level = level
                    pending['state'] = 'awaiting-stop-word'
                else:
                    self.responses[axis] += 1
                    self.next_command[axis] += 1
                    self.pending[axis] = None
                board.queue_irq(pending['owner'], irq)
                delivery = dict(axis=axis, phase=spec[0], tick=board.virtual_ticks,
                                irq=irq, cpu=pending['owner'], edges4=edges, edges0=edges // 4,
                                coordinate=self.coordinate, gpio=self.gpio_level)
                self.deliveries.append(delivery)
                board.record('Declared lens startup external input', profile=HOMING_PROFILE, **delivery,
                             limits='Named virtual lattice only; actual stock ISR/callback owns completion')
            if reentry_due is not None:
                self._deliver_focus_reentry(*reentry_due)
            if iris_reentry_due is not None:
                self.iris_reentry.update(state='awaiting-cleanup', responses=1, delivery_tick=board.virtual_ticks)
                board.queue_irq(iris_reentry_due, 12)
                board.record('Declared iris reference reentry external input', profile=IRIS_REENTRY_PROFILE,
                             cpu=iris_reentry_due, irq=12,
                             limits='One unmeasured reference response; no counter/GPIO/RAM or callback substitution')
            if post_due:
                self.post_iris.update(state='awaiting-cleanup',responses=1,delivery_tick=board.virtual_ticks)
                board.queue_irq(self.post_iris['owner'],12)
                board.record('Declared post-homing iris external input',contract=POST_IRIS_CONTRACT,
                             cpu=self.post_iris['owner'],irq=12,
                             limits='One Q0/rate20 response opportunity; no count/GPIO/motion/RAM or software completion')
        except (UnsupportedDevice, KeyError, TypeError, ValueError, IndexError) as error:
            self._failure(error)

    def next_event_tick(self, now):
        if getattr(self.board, 'failure', None) is not None:
            return None
        try:
            times = [p['due_tick'] for p in self.pending.values()
                     if p is not None and p['state'] != 'awaiting-stop-word']
            if self.post_iris['state']=='pending-response':times.append(self.post_iris['due_tick'])
            if self.focus_reentry is not None:
                self._validate_focus_reentry(self.focus_reentry)
                pending = self.focus_reentry['pending']
                if pending is not None and pending['state'] != 'awaiting-stop-word':times.append(pending['due_tick'])
            if getattr(self, 'iris_reentry', None) is not None:
                self._validate_iris_reentry(self.iris_reentry)
                if self.iris_reentry['state'] == 'pending-response':times.append(self.iris_reentry['due_tick'])
            if any(type(t) is not int for t in times):
                raise UnsupportedDevice('Lens homing pending time changed')
            return min(times) if times else None
        except (UnsupportedDevice, KeyError, TypeError, ValueError) as error:
            self._failure(error)
            return None

    def snapshot(self):
        return dict(name=type(self).__name__, profile=HOMING_PROFILE, response_ticks=self.response_ticks,
                    continuation_profile=self.continuation_profile,
                    post_complete_option=deepcopy(self.post_complete_option),
                    controller_reinit=deepcopy(self.controller_reinit),
                    controller_close=deepcopy(self.controller_close),
                    focus_reentry=deepcopy(self.focus_reentry),
                    iris_reentry=deepcopy(getattr(self, 'iris_reentry', None)),
                    passive_iris_option_contract=PASSIVE_IRIS_OPTION_CONTRACT,
                    post_iris_contract=POST_IRIS_CONTRACT,post_iris=deepcopy(self.post_iris),
                    next_command=dict(self.next_command), pending=deepcopy(self.pending), cleanup=dict(self.cleanup),
                    endpoint_responses=dict(self.responses), coordinate=self.coordinate, gpio_level=self.gpio_level,
                    words=deepcopy(self.words), deliveries=deepcopy(self.deliveries), assumptions=list(ASSUMPTIONS),
                    status='external-input ledger only; firmware homing/REC completion must be observed separately',
                    transaction_sources='Initial axis tails and focus state4 observed; later exact tails are stock-byte predictions')


# Kept below the class so adding this profile preserves the unchanged
# constructor code/line identity used by reviewed warm method replacement.
POST_COMPLETE_REPEAT_PROFILE = 'declared-iris-post-completion-option1-two-extra-words-unmeasured'
POST_COMPLETE_REPEAT_LIMIT = 2
_SINGLE_CONTINUATION_PROFILES = CONTINUATION_PROFILES
POST_COMPLETE_REPEAT_FOCUS_PROFILE = 'declared-lens-focus222-plus-two-passive-iris-option1-words-unmeasured'
POST_COMPLETE_REPEAT_BASES = {POST_COMPLETE_REPEAT_PROFILE:POST_COMPLETE_OPTION_PROFILE,
                              POST_COMPLETE_REPEAT_FOCUS_PROFILE:FOCUS_REENTRY_PROFILE}
CONTINUATION_PROFILES = (*CONTINUATION_PROFILES,*POST_COMPLETE_REPEAT_BASES)
