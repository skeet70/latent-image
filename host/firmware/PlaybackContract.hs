module PlaybackContract
  ( PlaybackEntry (..), SourceSha256 (..), playbackContract, playbackEntries
  , preparationEntries, jpegEntries, consumerEntries, transactionEntries, PinnedBody (..), jpegInternalBodies
  , JpegDeviceService (..), jpegDeviceServices, jpegDeviceContract
  ) where

import Data.Word (Word32)

data PlaybackEntry
  = PlaybackSource | PlaybackMode
  | PlaybackJpegAcquire | PlaybackJpegRestart | PlaybackJpegRelease | PlaybackJpegInterrupt
  | PlaybackJpegBuildRequest | PlaybackJpegCompleteFirst
  deriving (Eq, Ord, Show)
newtype SourceSha256 = SourceSha256 String deriving (Eq, Show)

preparationEntries, jpegEntries, consumerEntries, transactionEntries, playbackEntries :: [PlaybackEntry]
preparationEntries = [PlaybackSource, PlaybackMode]
jpegEntries = [PlaybackJpegAcquire, PlaybackJpegRestart, PlaybackJpegRelease, PlaybackJpegInterrupt]
consumerEntries = [PlaybackJpegBuildRequest, PlaybackJpegCompleteFirst]
transactionEntries = preparationEntries ++ jpegEntries
playbackEntries = transactionEntries ++ consumerEntries

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
playbackContract PlaybackJpegBuildRequest =
  (0x00d98904, 0x00d989f8, SourceSha256 "6392e76503ea8e6679b13f3003577a298a7e022d990194b7f484f686effef503", "xm1_playback_jpeg_build_request")
playbackContract PlaybackJpegCompleteFirst =
  (0x0121fec8, 0x0121ff20, SourceSha256 "62ed3b398b8c37d6bbce5ea41b4a8bfa3b088f33f5ab647a12a52770ec38ec23", "xm1_playback_jpeg_complete_first")

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

data JpegDeviceService
  = JpegReset | JpegProgram | JpegStart
  | JpegStatusRead | JpegStatusAck | JpegInputRead | JpegInputAck
  | JpegOutputRead | JpegOutputAck | JpegEndpointRead
  deriving (Eq, Ord, Enum, Bounded, Show)

jpegDeviceServices :: [JpegDeviceService]
jpegDeviceServices = [minBound .. maxBound]

jpegDeviceContract :: JpegDeviceService -> (PinnedBody, String)
jpegDeviceContract service = case service of
  JpegReset -> body 0x0065a478 0x0065a548 "e95a566fe7f4e5876b045d0be471321c783450e3c132c301cb8a095664e6f4e7" "xm1_jpeg_reset"
  JpegProgram -> body 0x0065a58c 0x0065a984 "f1a0d6aebc6ce3c9927b056e6c068471f2a2fbd6d7759bff94f04ae39a930673" "xm1_jpeg_program"
  JpegStart -> body 0x0065a548 0x0065a58c "e3afad5b0aa3dce5bf28fe09c81df31b0cd0f3a6bf22e9039dd80d7e6a3c1223" "xm1_jpeg_start"
  JpegStatusRead -> body 0x0065ab20 0x0065ab50 "19ceb15672e4a76566905ab42d49ddb6caa165ad67eeaffbeb487ba24c465c53" "xm1_jpeg_status_read"
  JpegStatusAck -> body 0x0065ab50 0x0065ab7c "a788d3c26eec732ee01d6d185f0bfb18285728d10dce1a77d0d9af39b99d023e" "xm1_jpeg_status_ack"
  JpegInputRead -> body 0x0065ab7c 0x0065abac "3e56fa3e4cdf2ceaaab220464ee8a15d1223a0d79423aaf9347243c58e44a2f9" "xm1_jpeg_input_event_read"
  JpegInputAck -> body 0x0065abac 0x0065abd8 "2b11ba6192f17dfb64aae257f6c845b4e28e917b0dfdc61e68e7391ac280a80d" "xm1_jpeg_input_event_ack"
  JpegOutputRead -> body 0x0065abd8 0x0065ac08 "199338db6406bb63221ef0ce4fd86c829373fd6f44e4628ba4766bad22477ae5" "xm1_jpeg_output_event_read"
  JpegOutputAck -> body 0x0065ac08 0x0065ac34 "6f99190d1f5151af505644ac7156fe41116df9873181c7deb98c77c40e66706c" "xm1_jpeg_output_event_ack"
  JpegEndpointRead -> body 0x0065a9e4 0x0065aa14 "15e60c1e860137e6465147e59182cafbfb9e2717f03446b1f589e6df7d90ff95" "xm1_jpeg_endpoint_read"
  where body start end hash symbol = (PinnedBody start end (SourceSha256 hash), symbol)
