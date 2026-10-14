"""Resolve explicitly selected service device contracts from profile parts.

This supplies constructor parameters to the sole boot factory. It owns no
board constructor, firmware selection, device list, or per-model branches.
"""
from .profile import checked_bytes, number, thaw, ProfileError
from .control_bus_parameters import resolve_control_bus, _view
from tools import firmware_registry as registry
import struct
from emulator.control_bus_contracts import ControlBusRegisters, ControlBusReplies, RegisterStatusReply
from emulator.control_bus import REGISTER_STATUS_PROFILE, EXTENDED_PROFILE, STATIONARY_PROFILE
from emulator.usb_endpoint import USBEndpointContract


def service_parameters(profile):
    selected = profile.board.get('service_controllers', {})
    if not selected:
        return {}
    if set(selected) - {'control_bus0', 'usb', 'fe80', 'display_timing'}:
        raise ProfileError('Unknown service controller binding')
    result = {}
    if 'fe80' in selected:
        from emulator.fe80_contracts import FE80Program
        rows = profile.firmware['fe80_programs']
        entry = registry.load(profile.firmware_id)
        result['fe80'] = dict(programs=tuple(FE80Program(
            _view(entry,row['descriptor'],'immutable-source'),
            _view(entry,row['program'],'immutable-source'),number(row['period']),
            row.get('setup_phase','routed')) for row in rows))
        if any(row.get('setup_phase','routed')!='routed' or number(row['period']) not in (12,13,16) for row in rows):
            views=profile.firmware.get('fe80_setup_views',())
            if not views:raise ProfileError('Alternate FE80 setup requires immutable stock consumer views')
            for view in views:_view(entry,view,'immutable-source')
    if 'display_timing' in selected:
        from emulator.display_timing import DisplayTiming
        spec = selected['display_timing']
        if spec['phase_profile'] != DisplayTiming.EXTERNAL_CLOCK_PROFILE:
            raise ProfileError('Service external clock requires its explicit input profile')
        result['display_timing'] = dict(phase_profile=spec['phase_profile'],
            frame_period_ticks=number(spec['frame_period_ticks'],positive=True),
            stop_delay_ticks=number(spec['stop_delay_ticks'],positive=True))
    if 'control_bus0' in selected:
        spec = selected['control_bus0']
        reference = thaw(profile.firmware['control_bus_views'])
        checked_bytes(profile.root, reference)
        views = resolve_control_bus(profile.firmware_id, spec_file=reference['path'])
        if views.spec_sha256 != reference['sha256']:
            raise ProfileError('Control-bus executed view differs from profile reference')
        registers = profile.soc['register_maps']['control_bus0']
        targets = tuple(RegisterStatusReply(number(row['address']),
            tuple((number(r), number(mask)) for r, mask in row['write_masks']),
            bytes.fromhex(row['status_hex']),
            tuple((number(r), number(value), bytes.fromhex(frame)) for r, value, frame in row['write_replies']))
            for row in spec.get('status_targets', ()))
        if spec['profile'] not in (REGISTER_STATUS_PROFILE, EXTENDED_PROFILE, STATIONARY_PROFILE):
            raise ProfileError('Service bus binding requires an explicit bounded profile')
        if bool(targets) != (spec['profile'] == REGISTER_STATUS_PROFILE):
            raise ProfileError('Status targets require only the selected status profile')
        sensor = spec.get('sensor')
        if sensor is not None and sensor['reply_model'] not in ('stationary-z-plus-1g', 'write-only'):
            raise ProfileError('Unknown selected sensor input model')
        if ((spec['profile'] == EXTENDED_PROFILE and (sensor is None or sensor['reply_model'] != 'write-only'))
                or (spec['profile'] == STATIONARY_PROFILE and (sensor is None or sensor['reply_model'] != 'stationary-z-plus-1g'))):
            raise ProfileError('Sensor input differs from selected bounded bus profile')
        result['control_bus0'] = dict(profile=spec['profile'], transfer_ticks=number(spec['transfer_ticks'], positive=True),
            registers=ControlBusRegisters(base=number(registers['base']), irq=number(registers['irq']),
                offsets=frozenset(number(v) for v in registers['offsets']), modes=tuple(number(v) for v in registers['modes'])),
            endpoints=views.records_for_channel(0), polarity=views.polarity_words,
            replies=ControlBusReplies(sensor_address=None if sensor is None else number(sensor['address']),
                                      prefix_address=None))
        if spec['profile'] == REGISTER_STATUS_PROFILE:
            result['control_bus0'].update(status_targets=targets,
                stationary_input=sensor is not None and sensor['reply_model'] == 'stationary-z-plus-1g')
    if 'usb' in selected:
        spec = selected['usb']
        registers = profile.soc['register_maps']['usb']
        wiring = profile.firmware['usb_irq_wiring']
        for row in wiring['views']:
            view = _view(registry.load(profile.firmware_id), row, 'immutable-source')
            if len(view.data) != 12 or struct.unpack('<3I', view.data) != (number(wiring[row['name']]), 13, 1):
                raise ProfileError('USB IRQ binding differs from selected stock descriptor triple')
        result['usb'] = USBEndpointContract(core=number(registers['core']), phy=number(registers['phy']),
            irq=number(wiring['irq']), dma_irq=number(wiring['dma_irq']),
            transfer_ticks=number(spec['transfer_ticks'], positive=True),
            packet_bytes=number(spec['packet_bytes'], positive=True),
            out_endpoints=tuple((number(a), number(port)) for a, port in spec['out_endpoints']),
            in_endpoints=tuple((number(a), number(port)) for a, port in spec['in_endpoints']))
        if profile.board['inputs']['usb_connected'] is not True:
            raise ProfileError('Service USB binding requires an explicit connected cable input')
    return result
