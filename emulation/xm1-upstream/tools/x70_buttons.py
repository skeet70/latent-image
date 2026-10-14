#!/usr/bin/env python3
"""One X70 1.10 parser table and button API for native and USB transports.

All 244 parser names and CMD are available through Buttons.emit. Press pairs
only verified X70 physical button events. Event kinds, effects, opt-ins and
service guards are metadata in KEY_TABLE; nothing here authorizes camera use.
"""
import sys
import time
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
sys.modules.setdefault('x70_buttons', sys.modules[__name__])
sys.modules.setdefault('tools.x70_buttons', sys.modules[__name__])

# Firmware parser table 013d7edc (including sentinel): SHA-256
# 2e264c337dd1ec01f36ecec37b4565214b9acadb341f3be84d03b1977808ba0b.
# Kinds/effects are host safety metadata, not fields in the firmware records.
# Unknown effects are explicit; no claim that all entries are X70 controls.
from dataclasses import dataclass
import math

@dataclass(frozen=True)
class Key:
    name: str
    event: int | None
    kind: str
    effect: str
    side_effects: bool = False
    service_guard: bool = False

    @property
    def on(self): return self.event
    @property
    def off(self): return None if self.event is None else (self.event | 0x80) & 0xffff

KEYS = (
    Key('EXE_M', 0x0001, 'button', 'Menu/OK: open menu or apply selection', True, False),
    Key('UP', 0x0002, 'button', 'Move menu cursor up', False, False),
    Key('DOWN', 0x0003, 'button', 'Move menu cursor down', False, False),
    Key('LEFT', 0x0004, 'button', 'Move menu cursor left/back', False, False),
    Key('RIGHT', 0x0005, 'button', 'Move menu cursor right/enter', False, False),
    Key('POWER', 0x0006, 'button', 'Power transition / teardown', True, True),
    Key('S1', 0x0007, 'button', 'Shutter half-press / AF and AE', False, False),
    Key('S2', 0x0008, 'button', 'Full shutter press; may capture and write card', True, False),
    Key('BARR_OP', 0x000c, 'device notification', 'Parser event; UI effect not qualified on X70', True, False),
    Key('BARR_CL', 0x000d, 'device notification', 'Parser event; UI effect not qualified on X70', True, False),
    Key('CRADLE', 0x0221, 'device notification', 'Parser event; UI effect not qualified on X70', True, False),
    Key('ADAPTER', 0x000e, 'device notification', 'Parser event; UI effect not qualified on X70', True, False),
    Key('USB', 0x000f, 'device notification', 'USB device notification; connection handling', True, False),
    Key('DOOR', 0x021d, 'device notification', 'Door notification; storage/power handling', True, False),
    Key('CARD', 0x0010, 'device notification', 'Card notification; storage handling', True, False),
    Key('FMODE', 0x000a, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('EXP', 0x0011, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('METER', 0x022a, 'sensor', 'Parser event; UI effect not qualified on X70', True, False),
    Key('TELE2', 0x0014, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('WIDE2', 0x0015, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('TELE', 0x0012, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('WIDE', 0x0013, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('AF', 0x0016, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('DRIVE', 0x0017, 'button', 'Open drive selection; capture settings can change', True, False),
    Key('VIDEO', 0x0018, 'device notification', 'Parser event; UI effect not qualified on X70', True, False),
    Key('HDTV', 0x0019, 'device notification', 'Parser event; UI effect not qualified on X70', True, False),
    Key('CONT', 0x001a, 'device notification', 'Parser event; UI effect not qualified on X70', True, False),
    Key('AS_MODE', 0x030d, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('DISP_B', 0x0009, 'button', 'Display/back; close menu', False, False),
    Key('K_HOME', 0x0228, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('K_PLAY', 0x000b, 'button', 'Playback transition / card access', True, True),
    Key('FACE', 0x021e, 'sensor', 'Face detector notification (not face setting)', True, False),
    Key('CRD_P', 0x0222, 'device notification', 'Parser event; UI effect not qualified on X70', True, False),
    Key('CRD_U', 0x0223, 'device notification', 'Parser event; UI effect not qualified on X70', True, False),
    Key('MET_SP', 0x021a, 'sensor', 'Parser event; UI effect not qualified on X70', True, False),
    Key('MET_AV', 0x021c, 'sensor', 'Parser event; UI effect not qualified on X70', True, False),
    Key('MET_MU', 0x021b, 'sensor', 'Parser event; UI effect not qualified on X70', True, False),
    Key('HVS_0', 0x0200, 'sensor', 'Parser event; UI effect not qualified on X70', True, False),
    Key('HVS_90', 0x0201, 'sensor', 'Parser event; UI effect not qualified on X70', True, False),
    Key('HVS_180', 0x0202, 'sensor', 'Parser event; UI effect not qualified on X70', True, False),
    Key('HVS_270', 0x0203, 'sensor', 'Parser event; UI effect not qualified on X70', True, False),
    Key('POP', 0x001b, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('C_DIAL+', 0x001c, 'button', 'Command dial increment; selected setting can change', True, False),
    Key('C_DIAL-', 0x001d, 'button', 'Command dial decrement; selected setting can change', True, False),
    Key('W_DIAL+', 0x0207, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('W_DIAL-', 0x0208, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('MF_AF_A', 0x001e, 'mode', 'Focus mode selector notification: AF-A', True, True),
    Key('MF_AF_C', 0x001f, 'mode', 'Focus mode selector notification: AF-C', True, True),
    Key('MF_AF_M', 0x0020, 'mode', 'Focus mode selector notification: MF', True, True),
    Key('1P_AF+', 0x0021, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('F_RING+', 0x0209, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('F_RING-', 0x020a, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('Z_RING+', 0x0022, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('Z_RING-', 0x0023, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('AE-LOCK', 0x0024, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('AF-LOCK', 0x0030, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('EVF_LCD', 0x0025, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('INFO', 0x0229, 'button', 'Parser event; UI effect not qualified on X70', False, False),
    Key('AEAF_LOCK', 0x0211, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('WB', 0x002d, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('K_PS', 0x0220, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('AE', 0x002e, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('VIEW', 0x0224, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('K_Q', 0x0031, 'button', 'Open Q menu (distinct from QUICK mode)', False, False),
    Key('EVF_OVF', 0x0028, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('MACRO', 0x021f, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('CMD_PUSH', 0x002a, 'button', 'X70 rear-dial push; selected UI action', True, False),
    Key('CMD_LEFT', 0x002b, 'button', 'X70 rear-dial left; selected setting can change', True, False),
    Key('CMD_RIGHT', 0x002c, 'button', 'X70 rear-dial right; selected setting can change', True, False),
    Key('MOVIE_REC', 0x0033, 'button', 'Movie recording; card writes', True, False),
    Key('EYE_SEN', 0x0032, 'sensor', 'Eye sensor notification (not eye AF setting)', True, False),
    Key('RING_OFF', 0x0204, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('RING_STB', 0x0205, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('RING_ON', 0x0206, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('FUNCTION2', 0x0226, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('FUNCTION', 0x0027, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('WLAN', 0x0036, 'button', 'Wireless handling', True, False),
    Key('CRING+', 0x020e, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('CRING-', 0x020f, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('CRING_MF', 0x0210, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('CR_TELE2', 0x003a, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('CR_WIDE2', 0x003c, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('CR_TELE', 0x0039, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('CR_WIDE', 0x003b, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('FOCUS', 0x022c, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('DELETE', 0x022d, 'button', 'Deletion UI; may remove images', True, False),
    Key('CMD2_L', 0x022e, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('CMD2_R', 0x022f, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('EXTFUNC', 0x0230, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('GRIP_C_L', 0x0231, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('GRIP_C_R', 0x0232, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('GRIP_C2_L', 0x0233, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('GRIP_C2_R', 0x0234, 'button', 'Parser event; UI effect not qualified on X70', True, False),
    Key('PLAY', 0x0300, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('REC', 0x0301, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('AUTO', 0x0302, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('QUICK', 0x0303, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('SP', 0x0304, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('SP2', 0x0305, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('SR', 0x0306, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('PORTRAIT', 0x0307, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('LANDSCAPE', 0x0308, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('SPORTS', 0x0309, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('NIGHT', 0x0236, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('BLURRE', 0x0238, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('NATURAL', 0x0237, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('PROGRAM', 0x030a, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('SHUTTER', 0x030b, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('APERTURE', 0x030c, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('MANUAL', 0x030e, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('N_FLASH', 0x0239, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('C1', 0x0313, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('C2', 0x0314, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('C3', 0x0315, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('FSB', 0x0316, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('ISO', 0x0026, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('ADVANCE', 0x0318, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('FILTER', 0x0319, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('PANORAMA', 0x031a, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('S-A', 0x031b, 'mode', 'Shutter selector: A (setting event)', True, True),
    Key('S-1/4000', 0x031c, 'mode', 'Shutter selector: 1/4000 (setting event)', True, True),
    Key('S-1/2000', 0x031d, 'mode', 'Shutter selector: 1/2000 (setting event)', True, True),
    Key('S-1/1000', 0x031e, 'mode', 'Shutter selector: 1/1000 (setting event)', True, True),
    Key('S-1/500', 0x031f, 'mode', 'Shutter selector: 1/500 (setting event)', True, True),
    Key('S-1/250', 0x0320, 'mode', 'Shutter selector: 1/250 (setting event)', True, True),
    Key('S-1/125', 0x0322, 'mode', 'Shutter selector: 1/125 (setting event)', True, True),
    Key('S-1/60', 0x0323, 'mode', 'Shutter selector: 1/60 (setting event)', True, True),
    Key('S-1/30', 0x0324, 'mode', 'Shutter selector: 1/30 (setting event)', True, True),
    Key('S-1/15', 0x0325, 'mode', 'Shutter selector: 1/15 (setting event)', True, True),
    Key('S-1/8', 0x0326, 'mode', 'Shutter selector: 1/8 (setting event)', True, True),
    Key('S-1/4', 0x0327, 'mode', 'Shutter selector: 1/4 (setting event)', True, True),
    Key('S-1/2', 0x0328, 'mode', 'Shutter selector: 1/2 (setting event)', True, True),
    Key('S-1/1', 0x0329, 'mode', 'Shutter selector: 1/1 (setting event)', True, True),
    Key('S-T', 0x032a, 'mode', 'Shutter selector: T (setting event)', True, True),
    Key('S-B', 0x032b, 'mode', 'Shutter selector: B (setting event)', True, True),
    Key('E-D+3.0', 0x032c, 'mode', 'Exposure compensation selector: +3.0 (setting event)', True, True),
    Key('E-D+2.7', 0x032d, 'mode', 'Exposure compensation selector: +2.7 (setting event)', True, True),
    Key('E-D+2.3', 0x032e, 'mode', 'Exposure compensation selector: +2.3 (setting event)', True, True),
    Key('E-D+2.0', 0x032f, 'mode', 'Exposure compensation selector: +2.0 (setting event)', True, True),
    Key('E-D+1.7', 0x0330, 'mode', 'Exposure compensation selector: +1.7 (setting event)', True, True),
    Key('E-D+1.3', 0x0330, 'mode', 'Exposure compensation selector: +1.3 (setting event)', True, True),
    Key('E-D+1.0', 0x0332, 'mode', 'Exposure compensation selector: +1.0 (setting event)', True, True),
    Key('E-D+0.7', 0x0333, 'mode', 'Exposure compensation selector: +0.7 (setting event)', True, True),
    Key('E-D+0.3', 0x0334, 'mode', 'Exposure compensation selector: +0.3 (setting event)', True, True),
    Key('E-D=0', 0x0335, 'mode', 'Exposure compensation selector: =0 (setting event)', True, True),
    Key('E-D-0.3', 0x0336, 'mode', 'Exposure compensation selector: -0.3 (setting event)', True, True),
    Key('E-D-0.7', 0x0337, 'mode', 'Exposure compensation selector: -0.7 (setting event)', True, True),
    Key('E-D-1.0', 0x0338, 'mode', 'Exposure compensation selector: -1.0 (setting event)', True, True),
    Key('E-D-1.3', 0x033a, 'mode', 'Exposure compensation selector: -1.3 (setting event)', True, True),
    Key('E-D-1.7', 0x033a, 'mode', 'Exposure compensation selector: -1.7 (setting event)', True, True),
    Key('E-D-2.0', 0x033b, 'mode', 'Exposure compensation selector: -2.0 (setting event)', True, True),
    Key('E-D-2.3', 0x033c, 'mode', 'Exposure compensation selector: -2.3 (setting event)', True, True),
    Key('E-D-2.7', 0x033d, 'mode', 'Exposure compensation selector: -2.7 (setting event)', True, True),
    Key('E-D-3.0', 0x033e, 'mode', 'Exposure compensation selector: -3.0 (setting event)', True, True),
    Key('ISO_A', 0x0517, 'mode', 'ISO selector: ISO_A (setting event)', True, True),
    Key('ISO_L', 0x0518, 'mode', 'ISO selector: ISO_L (setting event)', True, True),
    Key('I200', 0x0519, 'mode', 'ISO selector: I200 (setting event)', True, True),
    Key('I200_1_3', 0x051a, 'mode', 'ISO selector: I200_1_3 (setting event)', True, True),
    Key('I200_2_3', 0x051b, 'mode', 'ISO selector: I200_2_3 (setting event)', True, True),
    Key('I400', 0x051c, 'mode', 'ISO selector: I400 (setting event)', True, True),
    Key('I400_1_3', 0x051d, 'mode', 'ISO selector: I400_1_3 (setting event)', True, True),
    Key('I400_2_3', 0x051e, 'mode', 'ISO selector: I400_2_3 (setting event)', True, True),
    Key('I800', 0x051f, 'mode', 'ISO selector: I800 (setting event)', True, True),
    Key('I800_1_3', 0x0520, 'mode', 'ISO selector: I800_1_3 (setting event)', True, True),
    Key('I800_2_3', 0x0521, 'mode', 'ISO selector: I800_2_3 (setting event)', True, True),
    Key('I1600', 0x0522, 'mode', 'ISO selector: I1600 (setting event)', True, True),
    Key('I1600_1_3', 0x0523, 'mode', 'ISO selector: I1600_1_3 (setting event)', True, True),
    Key('I1600_2_3', 0x0524, 'mode', 'ISO selector: I1600_2_3 (setting event)', True, True),
    Key('I3200', 0x0525, 'mode', 'ISO selector: I3200 (setting event)', True, True),
    Key('I3200_1_3', 0x0526, 'mode', 'ISO selector: I3200_1_3 (setting event)', True, True),
    Key('I3200_2_3', 0x0527, 'mode', 'ISO selector: I3200_2_3 (setting event)', True, True),
    Key('I6400', 0x0528, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('ISO_H1', 0x0529, 'mode', 'ISO selector: ISO_H1 (setting event)', True, True),
    Key('ISO_H2', 0x052a, 'mode', 'ISO selector: ISO_H2 (setting event)', True, True),
    Key('DRV-PANO', 0x052b, 'mode', 'Drive selector: PANO (setting/card handling)', True, True),
    Key('DRV-ADV', 0x052c, 'mode', 'Drive selector: ADV (setting/card handling)', True, True),
    Key('DRV-MULTI', 0x052d, 'mode', 'Drive selector: MULTI (setting/card handling)', True, True),
    Key('DRV-ONE', 0x052e, 'mode', 'Drive selector: ONE (setting/card handling)', True, True),
    Key('DRV-C_L', 0x052f, 'mode', 'Drive selector: C_L (setting/card handling)', True, True),
    Key('DRV-C_H', 0x0530, 'mode', 'Drive selector: C_H (setting/card handling)', True, True),
    Key('DRV-BKT', 0x0531, 'mode', 'Drive selector: BKT (setting/card handling)', True, True),
    Key('I-M_A', 0x0400, 'mode', 'Aperture selector: A (setting event)', True, True),
    Key('I-M_F16', 0x0500, 'mode', 'Aperture selector: F16 (setting event)', True, True),
    Key('I-M_F11', 0x0503, 'mode', 'Aperture selector: F11 (setting event)', True, True),
    Key('I-M_F8.0', 0x0506, 'mode', 'Aperture selector: F8.0 (setting event)', True, True),
    Key('I-M_F5.6', 0x0509, 'mode', 'Aperture selector: F5.6 (setting event)', True, True),
    Key('I-M_F4.0', 0x050c, 'mode', 'Aperture selector: F4.0 (setting event)', True, True),
    Key('I-M_F2.8', 0x050f, 'mode', 'Aperture selector: F2.8 (setting event)', True, True),
    Key('I-M_F2.0', 0x0512, 'mode', 'Aperture selector: F2.0 (setting event)', True, True),
    Key('I-M_POS30', 0x041f, 'mode', 'Aperture selector: POS30 (setting event)', True, True),
    Key('I-M_POS29', 0x041e, 'mode', 'Aperture selector: POS29 (setting event)', True, True),
    Key('I-M_POS28', 0x041d, 'mode', 'Aperture selector: POS28 (setting event)', True, True),
    Key('I-M_POS27', 0x041c, 'mode', 'Aperture selector: POS27 (setting event)', True, True),
    Key('I-M_POS26', 0x041b, 'mode', 'Aperture selector: POS26 (setting event)', True, True),
    Key('I-M_POS25', 0x041a, 'mode', 'Aperture selector: POS25 (setting event)', True, True),
    Key('I-M_POS24', 0x0419, 'mode', 'Aperture selector: POS24 (setting event)', True, True),
    Key('I-M_POS23', 0x0418, 'mode', 'Aperture selector: POS23 (setting event)', True, True),
    Key('I-M_POS22', 0x0417, 'mode', 'Aperture selector: POS22 (setting event)', True, True),
    Key('I-M_POS21', 0x0416, 'mode', 'Aperture selector: POS21 (setting event)', True, True),
    Key('I-M_POS20', 0x0415, 'mode', 'Aperture selector: POS20 (setting event)', True, True),
    Key('I-M_POS19', 0x0414, 'mode', 'Aperture selector: POS19 (setting event)', True, True),
    Key('I-M_POS18', 0x0413, 'mode', 'Aperture selector: POS18 (setting event)', True, True),
    Key('I-M_POS17', 0x0412, 'mode', 'Aperture selector: POS17 (setting event)', True, True),
    Key('I-M_POS16', 0x0411, 'mode', 'Aperture selector: POS16 (setting event)', True, True),
    Key('I-M_POS15', 0x0410, 'mode', 'Aperture selector: POS15 (setting event)', True, True),
    Key('I-M_POS14', 0x040f, 'mode', 'Aperture selector: POS14 (setting event)', True, True),
    Key('I-M_POS13', 0x040e, 'mode', 'Aperture selector: POS13 (setting event)', True, True),
    Key('I-M_POS12', 0x040d, 'mode', 'Aperture selector: POS12 (setting event)', True, True),
    Key('I-M_POS11', 0x040c, 'mode', 'Aperture selector: POS11 (setting event)', True, True),
    Key('I-M_POS10', 0x040b, 'mode', 'Aperture selector: POS10 (setting event)', True, True),
    Key('I-M_POS9', 0x040a, 'mode', 'Aperture selector: POS9 (setting event)', True, True),
    Key('I-M_POS8', 0x0409, 'mode', 'Aperture selector: POS8 (setting event)', True, True),
    Key('I-M_POS7', 0x0408, 'mode', 'Aperture selector: POS7 (setting event)', True, True),
    Key('I-M_POS6', 0x0407, 'mode', 'Aperture selector: POS6 (setting event)', True, True),
    Key('I-M_POS5', 0x0406, 'mode', 'Aperture selector: POS5 (setting event)', True, True),
    Key('I-M_POS4', 0x0405, 'mode', 'Aperture selector: POS4 (setting event)', True, True),
    Key('I-M_POS3', 0x0404, 'mode', 'Aperture selector: POS3 (setting event)', True, True),
    Key('I-M_POS2', 0x0403, 'mode', 'Aperture selector: POS2 (setting event)', True, True),
    Key('I-M_POS1', 0x0402, 'mode', 'Aperture selector: POS1 (setting event)', True, True),
    Key('I-M_POS0', 0x0401, 'mode', 'Aperture selector: POS0 (setting event)', True, True),
    Key('F_R_+4', 0x0101, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('F_R_+3', 0x0102, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('F_R_+2', 0x0103, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('F_R_+1', 0x0104, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('F_R_STOP', 0x0105, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('F_R_-1', 0x0106, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('F_R_-2', 0x0107, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('F_R_-3', 0x0108, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('F_R_-4', 0x0109, 'lens', 'Parser event; UI effect not qualified on X70', True, False),
    Key('SETUP', 0x0311, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('M_SEL', 0x030e, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('VOICE', 0x030f, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('MOVIE', 0x0310, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('JIG', 0x033f, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('EXR', 0x0317, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('VREC', 0x030f, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('MJPEG', 0x0310, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('SCEN_P', 0x0304, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('SCEN_S', 0x0304, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('MMOD_P', 0x030a, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('MMOD_S', 0x030b, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('MMOD_A', 0x030c, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('MMOD_M', 0x030e, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
    Key('APERT', 0x0011, 'mode', 'Parser event; UI effect not qualified on X70', True, True),
)
KEY_TABLE = {key.name: key for key in KEYS}
KEY_TABLE['CMD'] = Key('CMD', None, 'CMD', 'Stock command callback; arbitrary settings/device effects', True, True)
KEY_EVENTS = {(k.name, state): code for k in KEYS for state, code in (('ON', k.on), ('OFF', k.off))}
# Verified stock X70 physical input records (0x1c stride, event in word 0).
# The parser includes controls from other models. Those remain emit()-only.
BUTTONS = {
    'POWER':0xb6318c, 'S1':0xb631a8, 'S2':0xb631c4,
    'CMD_LEFT':0xb635ec, 'CMD_RIGHT':0xb63608, 'CMD_PUSH':0xb63624,
    'DRIVE':0xb6365c, 'K_PLAY':0xb63678, 'DELETE':0xb63694,
    'UP':0xb636b0, 'DOWN':0xb636cc, 'LEFT':0xb636e8, 'RIGHT':0xb63704,
    'AEAF_LOCK':0xb63720, 'K_Q':0xb6373c, 'FUNCTION':0xb63758,
    'EXE_M':0xb63774, 'DISP_B':0xb63790, 'FUNCTION2':0xb63800,
}

def key_entry(name):
    if not isinstance(name, str) or not name.isascii() or name.upper() not in KEY_TABLE:
        raise ValueError(f'Unknown firmware key: {name!r}')
    return KEY_TABLE[name.upper()]

def validate_emit(name, state, allow_side_effects=False):
    key = key_entry(name)
    if key.kind == 'CMD':
        if not isinstance(state, str) or not state or len(state.encode('ascii')) > 254 or any(c in state for c in '\r\n\0'):
            raise ValueError('CMD requires a bounded ASCII command argument, without line delimiters')
    elif not isinstance(state, str) or not state.isascii() or state.upper() not in ('ON', 'OFF'):
        raise ValueError('Key state must be ON or OFF')
    else: state = state.upper()
    if key.side_effects and allow_side_effects is not True:
        raise PermissionError(f'{key.name}: {key.effect}; requires allow_side_effects=True')
    return key, state

@dataclass(frozen=True)
class Press:
    key: str
    hold: float = .05
    settle: float = .15
    before: object = None
    expect: object = None
    timeout: float = 2
    allow_side_effects: bool = False
    note: str = ''

class ReadbackError(RuntimeError): pass

class Buttons:
    """One table/API for both transports. Emission is distinct from UI completion.

    Hooks receive (phase, action, observation), and can raise to abort. Predicates
    consume backend observations; all sequence names and durations are validated
    before hooks or emission. Each potentially sent ON is followed by OFF even
    if transport completion fails. A mismatch never causes compensating actions.
    """
    def __init__(self, backend, verifier=None, record=None, save=None, *, log=None):
        if record is not None and log is not None:raise ValueError('Choose record or log')
        if log is not None:record=log
        self.backend, self.verifier = backend, verifier
        self.record = record if record is not None else []
        self.log = self.record
        self.save = save or (lambda: None)

    def _validate_press(self, action):
        if not isinstance(action, Press): raise TypeError('sequence accepts Press entries')
        key, _ = validate_emit(action.key, 'ON', action.allow_side_effects)
        if key.name not in BUTTONS: raise ValueError(f'{key.name} is not a verified X70 paired button ({key.kind})')
        for value in (action.hold, action.settle, action.timeout):
            if not isinstance(value, (float, int)) or not math.isfinite(value) or value < 0 or value > 10:
                raise ValueError('hold, settle and timeout must be finite seconds in 0..10')
        for predicate in (action.before, action.expect):
            if predicate is not None and not callable(predicate): raise TypeError('read-back predicate must be callable')
        return key

    def emit(self, name, state, *, allow_side_effects=False):
        key, state = validate_emit(name, state, allow_side_effects)
        entry = dict(name=key.name, state=state, kind=key.kind, effect=key.effect,
                     SIDE_EFFECTS=key.side_effects, started=self.backend.now())
        self.record.append(entry)  # attempted emission survives lost response
        self.save()
        try:
            if key.service_guard: self.backend.service_guard(key, state)
            ticket = self.backend.emit(key, state)
            entry['response'] = self.backend.wait(ticket)
            if key.service_guard: self.backend.service_guard(key, state, after=True)
            entry['finished'] = self.backend.now()
            self.save()
            return entry
        except BaseException as exc:
            entry['error'] = repr(exc); self.save(); raise

    def _check(self, predicate, timeout):
        deadline = self.backend.now() + timeout
        while True:
            observation = self.backend.observe()
            passed = predicate is None or bool(predicate(observation))
            self.record.append(dict(readback=observation, passed=passed, at=self.backend.now()))
            self.save()
            if passed: return observation
            if self.backend.now() >= deadline: raise ReadbackError('Button read-back predicate mismatch')
            self.backend.sleep(min(.05, max(0, deadline-self.backend.now())))

    def sequence(self, actions):
        actions = tuple(actions)
        for action in actions: self._validate_press(action)
        def hook(phase, action=None):
            if self.verifier: self.verifier(phase, action, self.backend.observe())
        hook('before_sequence')
        for action in actions:
            hook('before_action', action)
            if action.before: self._check(action.before, action.timeout)
            attempted = False
            try:
                attempted = True
                self.emit(action.key, 'ON', allow_side_effects=action.allow_side_effects)
                self.backend.sleep(action.hold)
            finally:
                if attempted: self.emit(action.key, 'OFF', allow_side_effects=action.allow_side_effects)
            self.backend.sleep(action.settle)
            if action.expect: self._check(action.expect, action.timeout)
            hook('after_action', action)
        hook('after_sequence')
        return self.record

    def press(self, key, hold=.05, **kwargs):
        return self.sequence([Press(key, hold, **kwargs)])

class CameraKeys:
    """Diagnostic USB emitter. Bulk reader supplies uncached read-only bytes.

    Service-affecting entries fail closed without a caller-provided transaction
    that implements research/service-exit/README.md. Opt-in is not camera action
    authorization and does not bypass that transaction.
    """
    def __init__(self, jig, read, observer=None, guard=None, save=None):
        self.jig, self.read_memory, self.observer, self.guard, self.save = jig, read, observer, guard, save
    def now(self): return time.monotonic()
    def sleep(self, seconds): time.sleep(seconds)
    def emit(self, key, state): return bytes(self.jig.debug_send_key(key.name, state)).hex()
    def wait(self, ticket): return ticket
    def observe(self): return self.observer() if self.observer else {}
    def service_guard(self, key, state, after=False):
        if self.guard is None: raise PermissionError('Guarded service-exit transaction required for '+key.name)
        self.guard(key, state, after=after)

class EmulatorKeys:
    """Existing Testbed/ScannerStub on RustBoard; guest time, not host sleeps.

    CMD uses the live stock command callback and declared zero-checked scratch;
    no command implementations or stock setting writes are substituted.
    """
    def __init__(self, bench, observer=None, guard=None):
        self.bench, self.tb = bench, bench.tb
        self.observer, self.guard, self.scratch = observer, guard, None
    def read_memory(self, address, size): return bytes(self.tb.board.read_memory(address, size, cpu_id=0))
    def now(self): return self.tb.board.virtual_ticks / 12000000
    def sleep(self, seconds):
        # AFBench owns the qualified clock/instruction conversion; do not
        # invent a second ratio for menu holds or settling.
        if self.bench is not None:
            result = self.bench.run_seconds(seconds)
        else:
            target=self.tb.board.virtual_ticks+round(seconds*12000000)
            result=self.tb.run_until(lambda tb:tb.board.virtual_ticks>=target,2_000_000_000,step=200000)
        if not result.matched: raise RuntimeError('Guest clock failed to advance: '+result.reason)
    def emit(self, key, state):
        from emulator.testbed.inputs import EMIT_KEY
        if key.kind != 'CMD':
            ticket = self.tb.stub.submit(EMIT_KEY, (KEY_EVENTS[key.name, state], 0))
        else:
            callback = int.from_bytes(self.read_memory(0xbe2fa0, 4), 'little')
            if not 0x800000 <= callback < 0x1400000: raise ReadbackError('Stock CMD callback unavailable')
            if self.scratch is None:
                if self.bench is not None:
                    address=self.bench.find_free_block(256)
                    self.scratch_handle=self.bench.install_patch(bytes(256),base=address,reserve=256,name='debug-CMD-argument')
                else:
                    # Same zero-block allocator and explicit patch ledger used
                    # by AFBench, without a second boot or AF preparation path.
                    from emulator.testbed.afbench import AFBench
                    from emulator.testbed.patch import install_ram_patch
                    from types import SimpleNamespace
                    address=AFBench.find_free_block(SimpleNamespace(board=self.tb.board),256)
                    self.scratch_handle=install_ram_patch(self.tb.board,bytes(256),address,reserve=256)
                self.scratch=address
            self.tb.board.write_memory(self.scratch, state.encode('ascii') + b'\0')
            ticket = self.tb.stub.submit(callback, (self.scratch,))
        return ticket
    def wait(self, ticket):
        if isinstance(ticket,(int,float)):
            # Compatibility for existing UI callers: wait(seconds) is sleep.
            return self.sleep(ticket)
        if self.bench is not None:
            result = self.bench.wait(lambda tb: ticket.done, timeout_virtual_s=.25, step=20000)
        else:
            result = self.tb.run_until(lambda tb:ticket.done,200_000_000,step=20000)
        if not ticket.done: raise TimeoutError('Guest event completion: '+result.reason)
        from emulator.testbed.inputs import BASE, SCANNER_WORD, SCANNER, build_stub
        import hashlib
        return dict(r0=ticket.result(),function=hex(ticket.function),args=list(ticket.args),
                    scanner_stub=dict(base=hex(BASE),callback_word=hex(SCANNER_WORD),stock_callback=hex(SCANNER),
                                      sha256=hashlib.sha256(build_stub()).hexdigest()),
                    command_scratch=None if self.scratch is None else hex(self.scratch))
    def observe(self): return self.observer() if self.observer else {}
    def service_guard(self, key, state, after=False):
        if self.guard is None: raise PermissionError('Declared emulator service guard required for '+key.name)
        self.guard(key, state, after=after)


# Existing film-session names are adapters to the same engine, not a second
# table or sequence implementation. Settings-affecting calls must now opt in.
SequenceStopped = ReadbackError

class EmulatorBackend(EmulatorKeys):
    def __init__(self,testbed,step=200000,guard=None):
        self.bench=None;self.tb=testbed;self.guard=guard;self.scratch=None
        self.observer=lambda:self
    def read(self,address,size=4):return int.from_bytes(self.read_memory(address,size),'little')
    def wait_until(self,predicate,timeout):
        deadline=self.now()+timeout
        while not predicate(self):
            if self.now()>=deadline:return False
            self.sleep(min(.05,deadline-self.now()))
        return True

class CameraBackend(CameraKeys):
    def __init__(self,jig,bulk,guard=None):
        super().__init__(jig,lambda a,n:bulk.read(0x40000000|a,n),guard=guard)
        self.observer=lambda:self
    def read(self,address,size=4):return int.from_bytes(self.read_memory(address,size),'little')
    def wait(self,ticket):
        if isinstance(ticket,(int,float)):return self.sleep(ticket)
        return super().wait(ticket)
    def wait_until(self,predicate,timeout):
        deadline=self.now()+timeout
        while not predicate(self):
            if self.now()>=deadline:return False
            self.sleep(min(.05,deadline-self.now()))
        return True

def events(name):
    key=key_entry(name)
    if key.name not in BUTTONS:raise ValueError('Not a verified X70 paired button: '+key.name)
    return key.on,key.off

if __name__=='__main__':
    import json
    from dataclasses import asdict
    print(json.dumps([dict(asdict(k),on=k.on,off=k.off,physical_record=BUTTONS.get(k.name))
                      for k in KEY_TABLE.values()],indent=2))
