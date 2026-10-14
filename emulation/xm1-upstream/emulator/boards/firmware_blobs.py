"""Device allowlists from exact, resolved firmware bytes; absent means unmodeled."""
import hashlib
from .profile import ResolvedProfile, ProfileError, number


def device_blob(profile, name):
    if not isinstance(profile, ResolvedProfile):
        raise TypeError('Device blobs require an explicitly resolved firmware profile')
    reference = profile.firmware.get('device_blobs', {}).get(name)
    if reference is None:
        return b''
    offset = number(reference['native_flash_offset'])
    size = number(reference['size'], positive=True)
    if offset + size > len(profile.flash):
        raise ProfileError('Device blob exceeds verified native flash')
    data = profile.flash[offset:offset + size]
    if hashlib.sha256(data).hexdigest() != reference['sha256']:
        raise ProfileError('Device blob differs from its firmware binding')
    return data
