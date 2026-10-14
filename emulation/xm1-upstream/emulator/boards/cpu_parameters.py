"""SoC CPU-memory parameters, independent of firmware-selected TCM addresses."""
import json

from .profile import ROOT, ProfileError, number


def tcm_parameters(soc):
    spec = soc['tcm']
    result = []
    for name in ('btcm', 'atcm'):  # Architectural CP15 opc2 order.
        row = spec[name]
        size, base = number(row['size']), number(row['reset_base'])
        if (size and (not 0x1000 <= size <= 0x800000 or size & (size-1))
                or type(row['reset_enabled']) is not bool or base & 0xfff
                or (not size and row['reset_enabled']) or not row['basis']):
            raise ProfileError('Invalid SoC TCM physical/reset parameters')
        result.append((size, base | int(row['reset_enabled'])))
    return result


def default_tcm_parameters():
    # Machine's existing default platform is EXR II. A different SoC must
    # supply explicit constructor parameters; no body/firmware fallback.
    return tcm_parameters(json.loads((ROOT / 'emulator/boards/soc/exr2.json').read_text()))
