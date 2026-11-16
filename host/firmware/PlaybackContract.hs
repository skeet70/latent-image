module PlaybackContract
  ( PlaybackEntry (..), SourceSha256 (..), playbackContract, playbackEntries
  , preparationEntries, jpegEntries, PinnedBody (..), jpegInternalBodies
  ) where

import Data.Word (Word32)

data PlaybackEntry
  = PlaybackSource | PlaybackMode
  | PlaybackJpegAcquire | PlaybackJpegRestart | PlaybackJpegRelease | PlaybackJpegInterrupt
  deriving (Eq, Ord, Show)
newtype SourceSha256 = SourceSha256 String deriving (Eq, Show)

preparationEntries, jpegEntries, playbackEntries :: [PlaybackEntry]
preparationEntries = [PlaybackSource, PlaybackMode]
jpegEntries = [PlaybackJpegAcquire, PlaybackJpegRestart, PlaybackJpegRelease, PlaybackJpegInterrupt]
playbackEntries = preparationEntries ++ jpegEntries

playbackContract :: PlaybackEntry -> (Word32, Word32, SourceSha256, String)
playbackContract PlaybackSource =
  (0x0120b40c, 0x0120b778, SourceSha256 "90da01fffdecd496e473c74aa012c9028be5d2dad33a1b8f4a95b4ea7c466d32", "xm1_playback_source_entry")
playbackContract PlaybackMode =
  (0x0120cab0, 0x0120cd74, SourceSha256 "d9b5d69e40838f9bcecc142fb1bfeaefa18e43b790a027e0442d7b31deef0f0c", "xm1_playback_mode_entry")
playbackContract PlaybackJpegAcquire =
  (0x00d9882c, 0x00d988a8, SourceSha256 "9010056d9d8cfb72c81836b41be1d8a78bcee7144f2b50016d818eed778bf381", "xm1_playback_jpeg_acquire")
playbackContract PlaybackJpegRestart =
  (0x00d988a8, 0x00d988b8, SourceSha256 "8fe358e626b4776b89d0a0f40b8697fe03e3eb5c7aab77f3b59e6288c0e3f731", "xm1_playback_jpeg_restart")
playbackContract PlaybackJpegRelease =
  (0x00d988b8, 0x00d98904, SourceSha256 "db638b2a6dc404d71eda6c3da34001f8fa603e181da22555d6b8aaafdc04a7aa", "xm1_playback_jpeg_release")
playbackContract PlaybackJpegInterrupt =
  (0x00d99570, 0x00d99714, SourceSha256 "4ff7a2b304d223fed19b7f99f081018f54b93be2e1e7d1827956794c2c9576a5", "xm1_playback_jpeg_interrupt")

data PinnedBody = PinnedBody
  { bodyStart :: Word32
  , bodyEnd :: Word32
  , bodySha256 :: SourceSha256
  } deriving (Eq, Show)

jpegInternalBodies :: [PinnedBody]
jpegInternalBodies =
  [ PinnedBody 0x00d99714 0x00d999d4 (SourceSha256 "13206b095f700897abfaaecefc3444ac51bb4a80a35a3f85d7b2a1f3507f0860")
  , PinnedBody 0x00d999d4 0x00d99a24 (SourceSha256 "ff9f5b23ce22074d8bfbe6b358370ef5001c652ff47e1cf22cddc3a8549dbd77")
  , PinnedBody 0x00d99a24 0x00d99a5c (SourceSha256 "5e52e2e0793efc75abb39ba8e2b170958c1863f3a6e8f73f32108d6254ae1f5a")
  ]
