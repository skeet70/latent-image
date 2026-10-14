"""Bounded FFFA9 pulse-output configuration, without external waveforms.

Legacy X70 1.10 stock 91A520 programs repetitive output mode, divider, period/reload and
duty registers, then uses a high-halfword write mask to enable a channel.
Stock 91BF88's exact channel0 208B and channel4 2089 configurations can
also be armed with thresholdFFFF, without supplying a count or completion.
The fixed-low input fixture permits +8 samples from the guest's zero reset.
A separate named lens-feedback fixture accepts unsigned external edge events
and exposes their modulo16-bit count. Neither uses instruction ticks as motion.
The named lens profile also provides one exact generic5/channel2 timer IRQ.
A separate opt-in capture profile admits the observed generic40/channel0 delay
and its stock interrupt cleanup after the final metering result.
Capture reads, other automatic edges/IRQ/DMA and physical timing are unresolved.
The X-M1 1.50 timer profile retains five-word C000..C003 configurations on
channels0/2/4/6/8 and routes their one-shot expiries to IRQ14/16/18/1A/1C.
Divider counts use a declared virtual-clock law, not measured oscillator timing.
"""
from copy import deepcopy
from dataclasses import dataclass, asdict
from enum import Enum
from .hardware import UnsupportedDevice
from .histogram_protocol import digest

class PulseClockProfile(Enum):
    XM1_CONFIGURED = 'xm1-150-configured-timer-clocks-unmeasured'


@dataclass
class PulseTimer:
    due_tick: int | None
    period_ticks: int
    irq_cpu: int
    one_shot: bool



class PulseOutputs:
    # Ticks at quiet quantum boundaries only advance elapsed-time bookkeeping
    # (emulator.board.Board.flush_deferred_ticks).
    quiet_ticks_defer = True
    BASE = 0xfffa9000
    CHANNEL_BASE = 0xfffa9100
    CHANNEL_COUNT = 12
    CHANNEL_OFFSETS = (0, 4, 8, 0xc, 0x10)
    COUNTER_CONTROLS = {0: 0x208b, 4: 0x2089}
    COUNTER_FIXED_LOW_PROFILE = 'focus-counter-inputs-fixed-low-no-edges'
    COUNTER_LENS_PROFILE = 'declared-lens-step-feedback-unmeasured'
    COUNTER_INPUT_PROFILES = (COUNTER_FIXED_LOW_PROFILE,COUNTER_LENS_PROFILE)
    SETTLE_TIMER_PROFILE='declared-lens-generic5-channel2-expiry-unmeasured'
    SETTLE_TIMER_IRQ=22
    SETTLE_TIMER_TICKS=24000
    CAPTURE_TIMER_PROFILE='declared-capture-metering-generic40-channel0-delay-unmeasured'
    CAPTURE_TIMER_IRQ=20
    CAPTURE_TIMER_DURATION=40000
    CAPTURE_TIMER_CONFIGURATION={0:0xc002,4:0,8:30000,0xc:29999,0x10:0}
    CAPTURE_TIMER_REQUEST=(40000,1,0,0,0,0x6f,0x12f9ba0)
    XM1_TIMER_IRQS = {0: 0x14, 2: 0x16, 4: 0x18, 6: 0x1a, 8: 0x1c}

    def __init__(self, board, *, counter_input_profile=None, timer_profile=None):
        if counter_input_profile not in (None, *self.COUNTER_INPUT_PROFILES):
            raise ValueError(f'Unsupported pulse counter input profile {counter_input_profile!r}')
        self.board = board
        self.counter_input_profile = counter_input_profile
        self.timer_profile = PulseClockProfile(timer_profile) if timer_profile is not None else None
        self.configured_timers = {}
        self.timer_expiries = 0
        self.counter_samples = {0: 0, 4: 0}
        self.last_counter_sample = None
        self.counter_edge_history = []
        self.counter_generations = {0:0,4:0}
        self.counter_expected = {0:0,4:0}
        self.settle_timer=None
        self.settle_timer_expiries=0
        self.registers = {}
        self.enabled = 0
        self.starts = self.stops = 0
        self.capture_route_written = False
        self.board.record('Pulse-output idle controller fixture',
                          provenance='zero initial configuration; stock repetitive-output writes retained',
                          limits='No physical output, counter progression, IRQ, DMA or completion fixture')
        if counter_input_profile is not None:
            self.board.record('Pulse counter external-input fixture',
                              profile=counter_input_profile,
                              provenance=('Declared fixed-low external inputs with no count edges; initial count is written by stock91BF88'
                                  if counter_input_profile==self.COUNTER_FIXED_LOW_PROFILE else
                                  'Named virtual lens-controller edge input; stock91BF88 initializes count0; physical feedback law unmeasured'),
                              limits='No measured signal/pin identity or feedback ratio; no actuator completion, tick clock, capture, IRQ or DMA')

    def _location(self, address, size):
        if size != 4 or address & 3:
            return None
        if address == self.BASE:
            return 'enable', None, 0
        if address == self.BASE+0x10:
            return 'input_route', None, 0x10
        delta = address-self.CHANNEL_BASE
        if 0 <= delta < self.CHANNEL_COUNT*0x100:
            channel, offset = divmod(delta, 0x100)
            if offset in self.CHANNEL_OFFSETS:
                return 'channel', channel, offset
        return None

    def contains(self, address, size):
        return self._location(address, size) is not None

    def mmio_ranges(self):
        """Superset of the start addresses contains() accepts (RustBoard._mmio_device index)."""
        return ((self.BASE, self.BASE+0x11),
                (self.CHANNEL_BASE, self.CHANNEL_BASE+self.CHANNEL_COUNT*0x100))

    def _counter_configuration(self,channel):
        base=self.CHANNEL_BASE+channel*0x100
        if (channel not in self.COUNTER_CONTROLS or not self.enabled&(1<<channel)
                or self.registers.get(base)!=self.COUNTER_CONTROLS[channel]
                or not self.capture_route_written
                or (self.registers.get(self.BASE+0x10,0)>>(channel*4))&15!=channel
                or self.registers.get(base+4,0)!=0
                or any(self.registers.get(base+o)!=value for o,value in ((0xc,0),(0x10,0xffff)))
                or type(self.registers.get(base+8)) is not int or not 0<=self.registers[base+8]<=0xffff):
            raise UnsupportedDevice('Declared lens feedback requires exact armed counter configuration')
        if (self.counter_input_profile==self.COUNTER_LENS_PROFILE
                and (type(self.counter_expected[channel]) is not int
                     or self.registers[base+8]!=self.counter_expected[channel])):
            raise UnsupportedDevice('Declared lens count differs from guest reset and external edge history')
        return base

    def add_counter_edges(self,channel,count,*,source):
        """Unsigned external edges; stock code applies direction and scaling."""
        if (self.counter_input_profile!=self.COUNTER_LENS_PROFILE or source!=self.COUNTER_LENS_PROFILE
                or type(channel) is not int or channel not in self.COUNTER_CONTROLS
                or type(count) is not int or not 0<=count<=0xffff):
            raise UnsupportedDevice('Lens counter edges require the named feedback fixture and typed bounded inputs')
        base=self._counter_configuration(channel)
        before=self.registers[base+8]
        after=(before+count)&0xffff
        self.registers[base+8]=after
        self.counter_expected[channel]=after
        event=dict(channel=channel,edges=count,previous=before,value=after,
                   generation=self.counter_generations[channel],tick=self.board.virtual_ticks,source=source)
        self.counter_edge_history.append(event)
        if len(self.counter_edge_history)>256:del self.counter_edge_history[:-256]
        self.board.record('Declared lens counter input edges',**event,
                          limits='Virtual edge source only; no elapsed-time count, physical encoder ratio or automatic IRQ')

    def read(self, core, address, size):
        handled,value=self._capture_access('read',core,address,size)
        if handled:return value
        location = self._location(address, size)
        if location is None:
            raise UnsupportedDevice(f'Unmodeled pulse-output read {address:#x}/{size}')
        if location[0] == 'enable':
            return self.enabled
        if (location[0] == 'channel' and location[2] in (8, 0xc)
                and self.enabled & (1 << location[1])
                and self.registers.get(self.CHANNEL_BASE+location[1]*0x100, 0)
                == self.COUNTER_CONTROLS.get(location[1])):
            if self.counter_input_profile==self.COUNTER_LENS_PROFILE and location[2]==8:
                channel=location[1]
                self._counter_configuration(channel)
                value=self.registers[address]
                self.counter_samples[channel]+=1
                self.last_counter_sample=dict(cpu=core,channel=channel,address=hex(address),value=value,
                    profile=self.counter_input_profile,generation=self.counter_generations[channel])
                return value
            if (self.counter_input_profile == self.COUNTER_FIXED_LOW_PROFILE
                    and location[2] == 8):
                channel = location[1]
                base = self.CHANNEL_BASE+channel*0x100
                if (not self.capture_route_written
                        or (self.registers.get(self.BASE+0x10, 0) >> (channel*4)) & 0xf != channel
                        or self.registers.get(base+4, 0) != 0
                        or any(self.registers.get(base+o) != value
                               for o, value in ((8, 0), (0xc, 0), (0x10, 0xffff)))):
                    raise UnsupportedDevice('Fixed-low counter sample requires exact guest-zero arm')
                self.counter_samples[channel] += 1
                self.last_counter_sample = {'cpu': core, 'channel': channel,
                                            'address': hex(address), 'value': 0,
                                            'profile': self.counter_input_profile}
                if self.counter_samples[channel] == 1:
                    self.board.record('Pulse counter fixed-low sample', **self.last_counter_sample,
                                      provenance='Guest-written count0 retained with declared absent external edges',
                                      limits='Sample is stationary/disconnected input; no movement or completion is implied')
                return self.registers[address]
            raise UnsupportedDevice(f'Armed channel{location[1]} counter read has unresolved external clock/input')
        return self.registers.get(address, 0)

    def write(self, core, address, size, value):
        handled,_=self._capture_access('write',core,address,size,value)
        if handled:return
        location = self._location(address, size)
        if location is None:
            raise UnsupportedDevice(f'Unmodeled pulse-output write {address:#x}/{size}')
        value &= 0xffffffff
        kind, channel, offset = location
        if kind == 'input_route':
            # ABC4F8[0]=0 and ABC4F8[4]=4; real 91BF88 replaces one
            # nibble while preserving the other channels' route fields.
            # Only these two observed route words are modeled.
            if value not in (0, 0x40000):
                raise UnsupportedDevice(f'Unmodeled pulse counter input route {value:#x}')
            for selected in self.COUNTER_CONTROLS:
                if self.enabled & (1 << selected) and (value >> (selected*4)) & 0xf != selected:
                    raise UnsupportedDevice('Changing an armed counter input route is unresolved')
            self.registers[address] = value
            self.capture_route_written = True
            return
        if kind == 'enable':
            mask, levels = value >> 16, value & 0xffff
            if mask & ~0xfff or levels & ~mask:
                raise UnsupportedDevice(f'Unsupported pulse-output enable mask {value:#x}')
            next_enabled = (self.enabled & ~mask) | levels
            starting = next_enabled & ~self.enabled
            timers = {}
            for selected in range(self.CHANNEL_COUNT):
                if starting & (1 << selected):
                    base = self.CHANNEL_BASE+selected*0x100
                    control = self.registers.get(base, 0)
                    period = self.registers.get(base+8, 0)
                    reload = self.registers.get(base+0xc, 0)
                    duty = self.registers.get(base+0x10, 0)
                    if self.timer_profile is PulseClockProfile.XM1_CONFIGURED and control in range(0xc000, 0xc004):
                        if (selected not in self.XM1_TIMER_IRQS or not 0 < period <= 0xffff
                                or reload != period - 1 or duty != 0
                                or any(base + offset not in self.registers for offset in self.CHANNEL_OFFSETS)):
                            raise UnsupportedDevice(f'Incomplete X-M1 configured timer channel{selected}')
                        line = self.XM1_TIMER_IRQS[selected]
                        targets = [cpu for cpu, bank in enumerate(self.board.interrupts.banks)
                                   if bank.enabled & (1 << line)]
                        if len(targets) != 1:
                            raise UnsupportedDevice(f'Configured timer IRQ{line:#x} has no unique enabled bank')
                        interval = period * (1, 8, 32, 128)[control & 3]
                        timers[selected] = PulseTimer(self.board.virtual_ticks + interval, interval, targets[0], True)
                        continue
                    if selected==2 and control in (0xc000,0xc001):
                        if control==0xc001 and not self._af_hold_selected():
                            raise UnsupportedDevice('C001 AF holdtimer requires its selected motion input')
                        abort=control==0xc000 and self._af_abort_selected()
                        count=period if abort and 1<=period<=0xffff else 30000 if control==0xc001 else 24000
                        if (self.counter_input_profile!=self.COUNTER_LENS_PROFILE
                                or period!=count or reload!=count-1 or duty!=0
                                or self.registers.get(base+4)!=0 or self._lens_timer() is not None
                                or any(base+o not in self.registers for o in self.CHANNEL_OFFSETS)):
                            raise UnsupportedDevice('Generic5 expiry requires its exact channel2 timer transaction')
                        self._settle_route()
                        if control==0xc001:self._af_hold_route()
                        if abort:self._af_abort_route()
                        continue
                    if control == self.COUNTER_CONTROLS.get(selected):
                        if (not getattr(self, 'capture_route_written', False) or period != 0
                                or reload != 0 or duty != 0xffff
                                or self.registers.get(base+4, 0) != 0
                                or (self.registers.get(self.BASE+0x10, 0) >> (selected*4)) & 0xf != selected
                                or any(base+o not in self.registers for o in (0, 8, 0xc, 0x10))):
                            raise UnsupportedDevice(f'Incomplete/unsupported channel{selected} counter arm')
                        continue
                    if (control not in range(0x8800, 0x8804) or period < 2
                            or reload != period-1 or not 0 < duty <= period):
                        raise UnsupportedDevice(f'Incomplete/unsupported repetitive pulse start channel{selected}: '
                                                f'control={control:#x}, period={period}, reload={reload}, duty={duty}')
            stopping = self.enabled & ~next_enabled
            if self.timer_profile is None and starting&4 and self.registers.get(self.CHANNEL_BASE+0x200) in (0xc000,0xc001):
                abort=self.registers[self.CHANNEL_BASE+0x200]==0xc000 and self._af_abort_selected()
                count=self.registers[self.CHANNEL_BASE+0x208] if abort else (30000 if self.registers[self.CHANNEL_BASE+0x200]==0xc001 else self.SETTLE_TIMER_TICKS)
                self._set_lens_timer(dict(state='pending',requested_tick=self.board.virtual_ticks,
                    due_tick=self.board.virtual_ticks+count,route=self._settle_route(),
                    configuration={o:self.registers[self.CHANNEL_BASE+0x200+o] for o in self.CHANNEL_OFFSETS},
                    **(dict(abort=True) if abort else {})))
                self.board.record('Declared lens settle timer armed',profile=self.SETTLE_TIMER_PROFILE,
                    timer=deepcopy(self._lens_timer()),limits='One expiry; virtual tick/count relation unmeasured; stock ISR owns stop/release')
            if stopping&4 and self._lens_timer() is not None:
                self.board.record('Declared lens settle timer stopped',timer=deepcopy(self._lens_timer()))
                self._set_lens_timer(None)
            for selected in tuple(self.configured_timers):
                if stopping & (1 << selected):
                    del self.configured_timers[selected]
            self.configured_timers.update(timers)
            self.enabled = next_enabled
            self.starts += starting.bit_count()
            self.stops += stopping.bit_count()
            for selected in self.COUNTER_CONTROLS:
                if starting&(1<<selected) and self.registers.get(self.CHANNEL_BASE+selected*0x100)==self.COUNTER_CONTROLS[selected]:
                    self.counter_generations[selected]+=1
            self.board.record('Pulse-output enable write', cpu=core, masked_word=hex(value),
                              enabled_mask=hex(self.enabled), started_mask=hex(starting),
                              stopped_mask=hex(stopping),
                              limits='Configuration/arm state only; no count progression, completion, external pulse or peripheral effect')
            return
        if offset == 0:
            timer_control=(channel==2 and value in (0xc000,0xc001) and self.counter_input_profile==self.COUNTER_LENS_PROFILE)
            timer_control |= self.timer_profile is PulseClockProfile.XM1_CONFIGURED and value in range(0xc000, 0xc004)
            if self.counter_input_profile==self.COUNTER_LENS_PROFILE and timer_control and value==0xc001 and not self._af_hold_selected():
                raise UnsupportedDevice('C001 AF holdtimer requires its selected motion input')
            if not timer_control and value not in range(0x8800, 0x8804) and value != self.COUNTER_CONTROLS.get(channel):
                raise UnsupportedDevice(f'Unmodeled pulse-output control mode {value:#x}')
        elif offset == 4:
            if value not in (0, 1):
                raise UnsupportedDevice(f'Unmodeled pulse-output polarity/configuration {value:#x}')
        elif value > 0xffff:
            raise UnsupportedDevice(f'Pulse-output count exceeds proven 16-bit range {value:#x}')
        if (self.counter_input_profile in self.COUNTER_INPUT_PROFILES
                and self.enabled & (1 << channel)
                and self.registers.get(self.CHANNEL_BASE+channel*0x100) == self.COUNTER_CONTROLS.get(channel)
                and ((offset in (8, 0xc) and value != 0)
                     or (offset == 0x10 and value != 0xffff)
                     or (offset == 4 and value != 0)
                     or (offset == 0 and value != self.COUNTER_CONTROLS[channel]))):
            raise UnsupportedDevice('Changing an armed counter input configuration is unresolved')
        if channel==2 and self._lens_timer() is not None and self.registers.get(address)!=value:
            raise UnsupportedDevice('Changing an armed lens settle timer is unresolved')
        self.registers[address] = value
        if kind=='channel' and offset==8 and channel in self.COUNTER_CONTROLS:
            self.counter_expected[channel]=value

    def _settle_route(self):
        banks=getattr(getattr(self.board,'interrupts',None),'banks',())
        if len(banks)!=2 or any(type(getattr(bank,'enabled',None)) is not int or bank.enabled<0 for bank in banks):
            raise UnsupportedDevice('Lens settle timer requires typed interrupt banks')
        targets=[core for core,bank in enumerate(banks) if bank.enabled&(1<<self.SETTLE_TIMER_IRQ)]
        if len(targets)!=1:
            raise UnsupportedDevice('Lens settle timer requires one enabled physical IRQ0x16 route')
        return targets[0]

    def _af_hold_selected(self):
        from .lens_af_motion import af_motor_timer_selected
        return af_motor_timer_selected(self.board)

    def _af_abort_selected(self):
        from .lens_af_motion import af_motor_abort_selected
        return af_motor_abort_selected(self.board)

    def _af_abort_route(self):
        route=self._settle_route();bank=self.board.interrupts.banks[route]
        if (route!=0 or bank.config.get(0x458)!=0xa0d75c or bank.config.get(0x858)!=12
                or (bank.source_config>>22)&1!=1):
            raise UnsupportedDevice('AF abort timer requires stock CPU0 IRQ22 vector A0D75C/priority/trigger')
        return route

    def _af_hold_route(self):
        route=self._settle_route();bank=self.board.interrupts.banks[route]
        if (route!=0 or bank.config.get(0x458)!=0xa0d0ac or bank.config.get(0x858)!=12
                or (bank.source_config>>22)&1!=1):
            raise UnsupportedDevice('AF holdtimer requires stock CPU0 IRQ22 vector/priority/trigger')
        return route

    def _validate_settle(self):
        timer=self._lens_timer()
        af=type(timer) is dict and type(timer.get('configuration')) is dict and timer['configuration'].get(0)==0xc001
        abort=type(timer) is dict and timer.get('abort') is True
        count=timer['configuration'].get(8) if abort and type(timer.get('configuration')) is dict else 30000 if af else self.SETTLE_TIMER_TICKS
        control=0xc001 if af else 0xc000
        if abort:
            self._af_abort_route()
        if af:
            if not self._af_hold_selected():
                raise UnsupportedDevice('Retained C001 timer lost its selected AF motion owner')
            self._af_hold_route()
        if (type(timer) is not dict or timer.get('state') not in ('pending','emitted')
                or type(timer.get('requested_tick')) is not int or timer['requested_tick']<0
                or type(timer.get('due_tick')) is not int or type(timer.get('route')) is not int
                or type(timer.get('configuration')) is not dict
                or any(type(o) is not int or type(v) is not int for o,v in timer['configuration'].items())
                or timer['due_tick']!=timer['requested_tick']+count
                or self.counter_input_profile!=self.COUNTER_LENS_PROFILE or not self.enabled&4
                or timer.get('configuration')!={0:control,4:0,8:count,0xc:count-1,0x10:0}
                or any(self.registers.get(self.CHANNEL_BASE+0x200+o)!=v for o,v in timer['configuration'].items())
                or timer.get('route')!=self._settle_route()):
            raise UnsupportedDevice('Lens settle timer configuration/IRQ route changed before expiry')
        return timer

    def _timer_pending(self):
        timer=self._lens_timer()
        if timer is None:return None
        if (type(timer) is not dict or timer.get('state') not in ('pending','emitted')
                or type(timer.get('due_tick')) is not int or timer['due_tick']<0):
            raise UnsupportedDevice('Lens settle timer pending ledger changed')
        return timer if timer['state']=='pending' else None

    def _settle_failure(self,error):
        self.board.failure=dict(message=str(error),peripheral='PulseOutputs lens settle timer')
        self.board.record('Lens settle timer unresolved boundary',**self.board.failure)

    def tick(self,board):
        self._capture_tick(board)
        if board is not self.board or getattr(board,'failure',None) is not None:return
        for selected, timer in self.configured_timers.items():
            if timer.due_tick is not None and board.virtual_ticks >= timer.due_tick:
                line = self.XM1_TIMER_IRQS[selected]
                board.queue_irq(timer.irq_cpu, line)
                timer.due_tick = None if timer.one_shot else timer.due_tick + timer.period_ticks
                self.timer_expiries += 1
                board.record('X-M1 configured timer expiry', channel=selected, physical_irq=hex(line),
                             limits='Count/divider timing is a declared virtual law; guest ISR owns disable')
        try:
            timer=self._timer_pending()
            if timer is None or board.virtual_ticks<timer['due_tick']:return
            timer=self._validate_settle()
            board.queue_irq(timer['route'],self.SETTLE_TIMER_IRQ)
            timer.update(state='emitted',expiry_tick=board.virtual_ticks)
            self.settle_timer_expiries+=1
            board.record('Declared lens settle timer physical expiry',profile=self.SETTLE_TIMER_PROFILE,
                         timer=deepcopy(timer),physical_irq=hex(self.SETTLE_TIMER_IRQ))
        except (UnsupportedDevice,KeyError,TypeError,ValueError) as error:
            self._settle_failure(error)

    def next_event_tick(self,now):
        if getattr(self.board,'failure',None) is not None:return None
        try:
            timer=self._timer_pending()
            capture=self._capture_marker()
            dates=[timer['due_tick']] if timer is not None else []
            if capture is not None and capture.get('state')=='pending':dates.append(capture['due_tick'])
            dates.extend(timer.due_tick for timer in self.configured_timers.values() if timer.due_tick is not None)
            return min(dates) if dates else None
        except (UnsupportedDevice,KeyError,TypeError,ValueError) as error:
            self._settle_failure(error)
            return None

    def _lens_timer(self):
        if type(self.settle_timer) is dict and 'capture_metering' in self.settle_timer:
            return self.settle_timer.get('lens')
        return self.settle_timer

    def _set_lens_timer(self,timer):
        if type(self.settle_timer) is dict and 'capture_metering' in self.settle_timer:
            self.settle_timer['lens']=timer
        else:self.settle_timer=timer

    def _capture_marker(self):
        wrapper=self.settle_timer
        if type(wrapper) is dict and 'capture_metering' in wrapper:
            if set(wrapper)!={'capture_metering','lens'}:
                raise UnsupportedDevice('Capture delay timer changed its plain lens-history wrapper')
            return wrapper['capture_metering']
        return None

    @staticmethod
    def _capture_seal(record):
        record['state_sha256']=digest({k:v for k,v in record.items() if k!='state_sha256'})

    def _capture_u(self,address,size=4):
        raw=bytes(self.board.read_memory(address,size,cpu_id=0))
        if len(raw)!=size:raise UnsupportedDevice('Capture delay caller RAM is truncated')
        return int.from_bytes(raw,'little')

    def _capture_anchor(self):
        owners=[d for d in self.board.devices if type(d).__name__=='ProducerControl']
        sources=[d for d in self.board.devices if type(d).__name__=='AfsFrameSource']
        if len(owners)!=1 or len(sources)!=1:
            raise UnsupportedDevice('Capture delay requires unique numeric and finite-stop owners')
        data=owners[0].capture_metstats_data_anchor(sample=2)
        stop=sources[0].capture_finite_stop_summary()
        if (type(data) is not dict or (data.get('sample'),data.get('state'),data.get('bank'))!=(2,'acknowledged',0x5fecd410)
                or data.get('profile')!='declared-afs-generation8-capture-scene-metering-final-unmeasured'
                or type(stop) is not dict or stop.get('state')!='acknowledged' or stop.get('eoi_tick') is None
                or stop.get('profile')!='declared-afs-generation8-metering-finite-stop-source0-clock-unmeasured'):
            raise UnsupportedDevice('Capture delay must follow both actual final numeric and finite-stop closures')
        return dict(data_sha256=digest(data),finite_stop_sha256=digest(stop))

    def _capture_drained(self):
        return all(not bank.pending&(1<<20) and bank.active!=20 and bank.injected!=20
                   for bank in self.board.interrupts.banks)

    def _capture_route(self):
        banks=self.board.interrupts.banks
        targets=[i for i,b in enumerate(banks) if b.enabled&(1<<20)]
        if (targets!=[0] or banks[0].config.get(0x450)!=0x12f9ba0
                or banks[0].config.get(0x850)!=12 or not banks[0].source_config&(1<<20)
                or (banks[0].config.get(0x140+4*(20//16),0)>>(2*(20%16)))&3 !=1
                or (banks[0].config.get(0x180+4*(20//16),0)>>(2*(20%16)))&3 !=0):
            raise UnsupportedDevice('Capture delay requires stock CPU0 IRQ20 vector, priority and timer trigger')
        return dict(cpu=0,irq=20,vector=0x12f9ba0,priority=12,source_type=1,trigger_mode=1,alternate_mode=0)

    def arm_capture_metering_delay(self,profile=CAPTURE_TIMER_PROFILE):
        if (profile!=self.CAPTURE_TIMER_PROFILE or self.board.failure is not None
                or self._capture_marker() is not None or self._lens_timer() is not None
                or self.enabled&1 or not self._capture_drained()):
            raise UnsupportedDevice('Capture delay requires its selected idle one-shot profile')
        words=((0x15d4908,4,5),(0x15d4888,1,0),(0x15f63a4,4,2),
               (0x15f68a4,4,0),(0x15f6bf4,4,0),(0x15d4dbc,1,0x32),
               (0x15d4dc0,4,0x5fecd410))
        if any(self._capture_u(a,n)!=v for a,n,v in words):
            raise UnsupportedDevice('Capture delay belongs to another private final-metering cleanup')
        anchor=self._capture_anchor()
        record=dict(profile=profile,state='selected',armed_tick=self.board.virtual_ticks,
                    anchor=anchor,anchor_sha256=digest(anchor),request=None,events=[],
                    enabled_tick=None,due_tick=None,expiry_tick=None,closed_tick=None,eoi_tick=None,route=None)
        self._capture_seal(record)
        self.settle_timer=dict(capture_metering=record,lens=self.settle_timer)
        self.board.record('Capture metering delay explicitly selected',profile=profile,
            limits='One channel0 timer; no image, numerical data, callback or guest-memory write')
        return deepcopy(record)

    def _capture_validate(self):
        record=self._capture_marker()
        if record is None:return None
        fields={'profile','state','armed_tick','anchor','anchor_sha256','request','events',
                'enabled_tick','due_tick','expiry_tick','closed_tick','eoi_tick','route','state_sha256'}
        if (type(record) is not dict or set(record)!=fields or record['profile']!=self.CAPTURE_TIMER_PROFILE
                or record['state'] not in ('selected','configuring','pending','emitted','closed')
                or type(record['armed_tick']) is not int or not 0<=record['armed_tick']<=self.board.virtual_ticks
                or record['state_sha256']!=digest({k:v for k,v in record.items() if k!='state_sha256'})
                or record['anchor_sha256']!=digest(record['anchor']) or record['anchor']!=self._capture_anchor()
                or type(record['events']) is not list or len(record['events'])>8):
            raise UnsupportedDevice('Capture delay identity, numeric history or plain ledger changed')
        operations=(('write',self.CHANNEL_BASE,0xc002),('read',self.CHANNEL_BASE+4,0),
                    ('write',self.CHANNEL_BASE+4,0),('write',self.CHANNEL_BASE+8,30000),
                    ('write',self.CHANNEL_BASE+12,29999),('write',self.CHANNEL_BASE+16,0),
                    ('write',self.BASE,0x10001),('write',self.BASE,0x10000))
        previous=record['armed_tick']
        for event,expected in zip(record['events'],operations):
            if (type(event) is not dict or set(event)!={'operation','cpu','address','word','tick'}
                    or (event['operation'],event['address'],event['word'])!=expected or event['cpu']!=0
                    or type(event['tick']) is not int or not previous<=event['tick']<=self.board.virtual_ticks):
                raise UnsupportedDevice('Capture delay whole configuration/enable/stop transaction changed')
            previous=event['tick']
        length=len(record['events'])
        if ((record['state']=='selected')!=(length==0)
                or record['state']=='configuring' and not 1<=length<=6):
            raise UnsupportedDevice('Capture delay selection claims hardware configuration')
        if length:
            if record['request']!=list(self.CAPTURE_TIMER_REQUEST):
                raise UnsupportedDevice('Capture delay lost the actual generic40 request parameters')
        elif record['request'] is not None:raise UnsupportedDevice('Capture delay selection claims a programmed request')
        if length>=7:
            if (record['enabled_tick']!=record['events'][6]['tick']
                    or record['due_tick']!=record['enabled_tick']+self.CAPTURE_TIMER_DURATION
                    or record['route']!=dict(cpu=0,irq=20,vector=0x12f9ba0,priority=12,source_type=1,trigger_mode=1,alternate_mode=0)):
                raise UnsupportedDevice('Capture delay enable/deadline/physical route changed')
        elif any(record[k] is not None for k in ('enabled_tick','due_tick','route','expiry_tick','closed_tick','eoi_tick')):
            raise UnsupportedDevice('Capture delay claims a boundary before hardware enable')
        if record['state'] in ('pending','emitted'):
            if length!=7 or not self.enabled&1 or any(self.registers.get(self.CHANNEL_BASE+o)!=v for o,v in self.CAPTURE_TIMER_CONFIGURATION.items()):
                raise UnsupportedDevice('Capture delay active channel0 configuration changed')
            self._capture_route()
        if record['state']=='pending' and record['expiry_tick'] is not None:
            raise UnsupportedDevice('Capture delay pending timer claims an expiry')
        if record['state'] in ('emitted','closed'):
            if type(record['expiry_tick']) is not int or not record['due_tick']<=record['expiry_tick']<=self.board.virtual_ticks:
                raise UnsupportedDevice('Capture delay physical expiry chronology changed')
        if record['state']=='closed':
            if length!=8 or self.enabled&1 or record['closed_tick']!=record['events'][7]['tick'] or record['closed_tick']<record['expiry_tick']:
                raise UnsupportedDevice('Capture delay closure changed its actual stock stop boundary')
        elif record['closed_tick'] is not None or record['eoi_tick'] is not None:
            raise UnsupportedDevice('Capture delay claims premature stop/EOI')
        if record['eoi_tick'] is not None and (type(record['eoi_tick']) is not int or not record['closed_tick']<=record['eoi_tick']<=self.board.virtual_ticks):
            raise UnsupportedDevice('Capture delay EOI chronology changed')
        return record

    def _capture_access(self,operation,core,address,size,value=None):
        record=self._capture_marker()
        if record is None or address not in (self.BASE,*[self.CHANNEL_BASE+o for o in self.CHANNEL_OFFSETS]):return False,None
        # Other masked channels retain their own independent lens law.
        if (address==self.BASE and operation=='write' and type(value) is int
                and value>>16 and not (value>>16)&1):return False,None
        if record['state']=='closed':return False,None
        self._capture_validate()
        if core!=0 or size!=4:
            raise UnsupportedDevice('Capture delay requires CPU0 WORD accesses')
        index=len(record['events'])
        operations=(('write',self.CHANNEL_BASE,0xc002),('read',self.CHANNEL_BASE+4,0),
                    ('write',self.CHANNEL_BASE+4,0),('write',self.CHANNEL_BASE+8,30000),
                    ('write',self.CHANNEL_BASE+12,29999),('write',self.CHANNEL_BASE+16,0),
                    ('write',self.BASE,0x10001),('write',self.BASE,0x10000))
        word=self.registers.get(address,0) if operation=='read' else value
        if index>=8 or (operation,address,word)!=operations[index]:
            raise UnsupportedDevice('Capture delay requires the next exact whole-driver access')
        if index==0:
            request=[self._capture_u(0xc65b0c+4*i) for i in range(7)]
            if request!=list(self.CAPTURE_TIMER_REQUEST):raise UnsupportedDevice('Capture delay request differs from observed generic40/40ms class')
            record['request']=request
        if index==6:self._capture_route()
        if index==7 and record['state']!='emitted':raise UnsupportedDevice('Capture delay stop precedes its physical expiry')
        event=dict(operation=operation,cpu=core,address=address,word=word,tick=self.board.virtual_ticks)
        if operation=='write':
            if address==self.BASE:
                if index==6:self.enabled|=1;self.starts+=1
                else:self.enabled&=~1;self.stops+=1
            else:self.registers[address]=word
        record['events'].append(event)
        if index==6:record.update(state='pending',enabled_tick=self.board.virtual_ticks,due_tick=self.board.virtual_ticks+self.CAPTURE_TIMER_DURATION,route=self._capture_route())
        elif index==7:record.update(state='closed',closed_tick=self.board.virtual_ticks)
        else:record['state']='configuring'
        self._capture_seal(record)
        return True,word

    def _capture_tick(self,board):
        if board is not self.board or getattr(board,'failure',None) is not None:return
        try:
            record=self._capture_marker()
            if record is None or record.get('eoi_tick') is not None:return
            if record.get('state') in ('selected','configuring'):return
            if record.get('state')=='pending' and board.virtual_ticks<record['due_tick']:return
            self._capture_validate()
            if record['state']=='pending':
                board.queue_irq(0,20);record.update(state='emitted',expiry_tick=board.virtual_ticks)
                self._capture_seal(record)
                board.record('Declared capture metering delay physical expiry',profile=self.CAPTURE_TIMER_PROFILE,
                             physical_irq='0x14',limits='Stock timer callback owns stop and release; no callback or result is supplied')
            elif record['state']=='closed' and self._capture_drained():
                record['eoi_tick']=board.virtual_ticks;self._capture_seal(record)
        except (UnsupportedDevice,KeyError,TypeError,ValueError) as error:
            board.failure=dict(message=str(error),peripheral='PulseOutputs capture metering delay')

    def capture_metering_delay_summary(self):
        return deepcopy(self._capture_validate())

    def snapshot(self):
        self._capture_validate()
        return {'name': 'PulseOutputs', 'enabled_mask': hex(self.enabled),
                'starts': self.starts, 'stops': self.stops,
                'capture_route_written': getattr(self, 'capture_route_written', False),
                'counter_input_profile': self.counter_input_profile,
                'timer_profile': self.timer_profile.value if self.timer_profile is not None else None,
                'configured_timers': {channel: asdict(timer) for channel, timer in self.configured_timers.items()},
                'configured_timer_expiries': self.timer_expiries,
                'settle_timer_profile':self.SETTLE_TIMER_PROFILE if self.counter_input_profile==self.COUNTER_LENS_PROFILE else None,
                'settle_timer':deepcopy(self._lens_timer()),'capture_metering_timer':deepcopy(self._capture_marker()),'settle_timer_expiries':self.settle_timer_expiries,
                'counter_samples': dict(self.counter_samples),
                'counter_generations':dict(self.counter_generations),
                'counter_edge_history':list(self.counter_edge_history),
                'counter_expected':dict(self.counter_expected),
                'last_counter_sample': self.last_counter_sample,
                'armed_counter_channels': [channel for channel, control in self.COUNTER_CONTROLS.items()
                                           if self.enabled & (1 << channel)
                                           and self.registers.get(self.CHANNEL_BASE+channel*0x100) == control],
                'registers': {hex(k): hex(v) for k, v in sorted(self.registers.items())},
                'channels': [channel for channel in range(self.CHANNEL_COUNT)
                             if self.enabled & (1 << channel)],
                'limits': ['Repetitive-output controls8800..8803 plus exact channel0 208B/route0 and channel4 2089/route4 thresholdFFFF arms are retained.',
                           'Idle configuration zero is an explicit model reset fixture.',
                           'The X-M1 timer profile admits configured C000..C003 one-shot channels0/2/4/6/8 with IRQ14/16/18/1A/1C; clock units and latency are unmeasured.',
                           'Default armed reads remain unsupported. Optional fixed-low/no-edges input retains only exact guest-zero +8 counts; it does not establish a working lens.',
                           'Named lens-feedback input permits unsigned declared source edges and modulo16-bit +8 samples; timing/encoder ratios are not measured.',
                           'Named lens input supports exact channel2 C000/count24000 and selected AF C001/count30000 holdtimers with one IRQ22 expiry; virtual tick/count relation is declared, not measured.',
                           'Armed +C capture reads, automatic edges, other IRQ/DMA, synchronized routing or physical waveforms remain unresolved.',
                           'Channel assignment to physical actuators/display/buzzer pins remains unresolved.']}
