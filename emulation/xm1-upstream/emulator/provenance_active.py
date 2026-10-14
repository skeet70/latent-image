"""Fingerprint the classes actually attached to a paused board.

Module reloads can leave a factory or existing instance bound to an earlier
class object. This evidence reads those actual classes and compares their
current module bindings without importing, reloading or invoking devices.
"""
from datetime import datetime, timezone
import sys
from types import ModuleType

from .provenance import _class, _json_digest


def _module_binding(cls):
    value = sys.modules.get(cls.__module__)
    for name in cls.__qualname__.split('.'):
        if not isinstance(value, (ModuleType, type)) or name == '<locals>':
            return None
        value = vars(value).get(name)
    return value if isinstance(value, type) else None


def _identity(cls):
    return {'module': cls.__module__, 'qualname': cls.__qualname__}


def active_device_provenance(board):
    """Return JSON-safe actual-class evidence; no device methods are called."""
    devices = []
    for index, device in enumerate(tuple(board.devices)):
        cls = type(device)
        binding = _module_binding(cls)
        devices.append({
            'index': index, **_identity(cls),
            'class_fingerprint': _class(cls),
            'inherited_classes': [dict(**_identity(base), class_fingerprint=_class(base))
                                  for base in cls.__mro__[1:] if base is not object],
            'module_binding': {
                'present': binding is not None,
                'same_class_object': None if binding is None else binding is cls,
                'sha256': None if binding is None else _class(binding)['sha256']
            }
        })
    return {
        'schema': 'x70view-active-device-provenance-v1',
        'captured_at_utc': datetime.now(timezone.utc).isoformat(),
        'devices': devices, 'sha256': _json_digest(devices),
        'method': 'Actual attached classes and MRO methods/constants, using the existing model-class encoding; module bindings read directly',
        'limits': [
            'Paused host evidence; devices and classes are inspected sequentially, not atomically.',
            'Class functions/defaults/closures and uppercase constants use the model helper encoding; its opaque/cyclic and Python-version limits apply.',
            'Instance attributes, per-instance method overrides, stored callbacks, scheduled callback payloads and mutable device state are not captured here.',
            'Function global lookups and lowercase module/class data are not fully represented by these class fingerprints.',
            'An absent module binding may be a local/nested or removed class; it does not by itself establish a fault.',
            'Class identity drift documents loaded code selection; it does not validate RTOS, peripheral or physical-camera behavior.'
        ]
    }
