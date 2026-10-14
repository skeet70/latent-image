"""Pure identities for the qualified ordinary and late-card REC prefixes.

The seeded-card cold boot needs the already selected eighth startup source
edge to finish display staging. It still performs four producer rotations.
Logical AF frames5..9 are separate from the preceding startup scene frames;
only their physical source ordinal/attempt identities shift by one.
"""
from .hardware import UnsupportedDevice
from .input_processing_devices import CARD_STAGING_EVENT_PROFILE, REC_DISPLAY_EVENT_PROFILES


_IDENTITIES = (
    ('declared-afs-post-s1-one-source0-edge-unmeasured', 3, 6, 8, 5),
    ('declared-afs-second-source0-edge-after-paired-ack-unmeasured', 3, 7, 9, 6),
    ('declared-afs-generation4-measurement-source0-edge-unmeasured', 4, 1, 10, 7),
    ('declared-afs-generation4-first-pixel-source0-edge-unmeasured', 4, 2, 11, 8),
    ('declared-afs-generation4-second-pixel-source0-edge-after-paired-ack-unmeasured', 4, 3, 12, 9),
)


def scope(input_profile):
    if input_profile not in REC_DISPLAY_EVENT_PROFILES:
        raise UnsupportedDevice('AF-S source has no qualified REC startup profile')
    card = input_profile == CARD_STAGING_EVENT_PROFILE
    count = 8 if card else 7
    return dict(profile=input_profile, counters=(3, count, count, count, count - 3, count - 2),
                history_count=count, ordinal_offset=int(card), attempt_offset=int(card))


def identities(input_profile):
    selected = scope(input_profile)
    return tuple((profile, generation, ordinal + (selected['ordinal_offset'] if generation == 3 else 0),
                  attempt + selected['attempt_offset'], frame, 0, 0, '0x27', 1)
                 for profile, generation, ordinal, attempt, frame in _IDENTITIES)


def identity(input_profile, profile):
    matches = [row for row in identities(input_profile) if row[0] == profile]
    if len(matches) != 1:
        raise UnsupportedDevice('AF-S source identity is outside its five qualified lifetimes')
    return matches[0]
