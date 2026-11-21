{-# LANGUAGE GHC2021 #-}

-- | Image4 sound sample blocks. x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, xm1-150:
-- app/drivers/audio/sound_playback_state.h declares sound_clips[] {position, length} and
-- sound_tracks[] {3 operations x {begin, end} x {position, length}}; the bytes below give 22 clips at
-- 0x7dceb0 and 5 tracks at 0x810f3c. A block is a table entry with length >= 3, the bound by which
-- the readers queue it. Its bytes go through the bound readers to the DMA source of the audio FIFO.
module Replacement.PropertyService.RetainedImage4Pcm
  ( pcmClosures
  , pcmWordClosed
  ) where

import Control.Monad (guard)
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import Replacement.PropertyService.Placement (GuestAddress (..), ScanImage (..))
import Replacement.PropertyService.RetainedClosure (ClosedRole (..), ClosedSite (..), extentMatches, imageFor)
import ServiceInventory (Image (..), imageWord)

newtype ClipIndex = ClipIndex Word32 deriving (Eq, Show)
newtype TrackIndex = TrackIndex Word32 deriving (Eq, Show)

data Operation = Operation0 | Operation1 | Operation2 deriving (Eq, Show, Enum, Bounded)

data Edge = Begin | End deriving (Eq, Show, Enum, Bounded)

-- | The table entry naming a sample block.
data BlockName
  = ClipBlock ClipIndex
  | TrackBlock TrackIndex Operation Edge
  deriving (Eq, Show)

data Block = Block BlockName Word32 Word32

data Extent = Extent GuestAddress GuestAddress SHA.Sha256

pcmClosures :: [(ScanImage, Image)] -> [ClosedSite]
pcmClosures images = case imageFor ScanImage4 images of
  Nothing -> []
  Just image4 -> case boundBlocks image4 of
    Nothing -> []
    Just blocks ->
      [ ClosedSite ScanImage4 site expected ConsumedData
      | (site@(GuestAddress address), expected, owner) <- sites
      , any (\(Block name start end) -> name == owner && covers start end address) blocks
      , imageWord image4 address == Just expected
      ]

-- | Whether the image4 word at the site lies in a bound sample block.
pcmWordClosed :: Image -> GuestAddress -> Bool
pcmWordClosed image4 (GuestAddress address) = case boundBlocks image4 of
  Nothing -> False
  Just blocks -> address `mod` 4 == 0 && any (\(Block _ start end) -> covers start end address) blocks

covers :: Word32 -> Word32 -> Word32 -> Bool
covers start end address = start <= address && toInteger address + 4 <= toInteger end

-- | The walked blocks, when every bound extent matches and the walk yields exactly the reviewed blocks.
boundBlocks :: Image -> Maybe [Block]
boundBlocks image4 = do
  guard (imageBase image4 == 0x61c400)
  guard (all (\(Extent start end digest) -> extentMatches image4 start end digest) (readers ++ tables ++ [sampleRegion]))
  clips <- mapM (clipBlock image4) [0 .. 21]
  tracks <- sequence [trackBlock image4 track operation edge | track <- [0 .. 4], operation <- [minBound .. maxBound], edge <- [minBound .. maxBound]]
  let blocks = [block | Just block <- clips ++ tracks]
  guard (map (\(Block _ start end) -> (start, end)) blocks == map (\(start, end, _) -> (start, end)) reviewedBlocks)
  guard (all (\(start, end, digest) -> extentMatches image4 (GuestAddress start) (GuestAddress end) (reviewedHash digest)) reviewedBlocks)
  guard (all (\(Block _ start end) -> start >= regionStart && end <= regionEnd) blocks)
  pure blocks
  where
    Extent (GuestAddress regionStart) (GuestAddress regionEnd) _ = sampleRegion

clipBlock :: Image -> Word32 -> Maybe (Maybe Block)
clipBlock image4 index = entryBlock image4 (ClipBlock (ClipIndex index)) (0x7dceb0 + 8 * index)

trackBlock :: Image -> Word32 -> Operation -> Edge -> Maybe (Maybe Block)
trackBlock image4 track operation edge =
  entryBlock image4 (TrackBlock (TrackIndex track) operation edge)
    (0x810f3c + 48 * track + 16 * fromIntegral (fromEnum operation) + 8 * fromIntegral (fromEnum edge))

entryBlock :: Image -> BlockName -> Word32 -> Maybe (Maybe Block)
entryBlock image4 name entry = do
  position <- imageWord image4 entry
  size <- imageWord image4 (entry + 4)
  pure $
    if size < 3
      then Nothing
      else Just (Block name position (position + size))

-- | id4.dis extents: sound_play_clip, sound_play_track, sound_queue_track_end
-- (app/drivers/audio/sound_playback_queue.c, exact), sound_queue_submit
-- (sound_playback_queue_more.c, NONMATCHING), sound_queue_start_current (tu_006e9e24.c,
-- NONMATCHING), audio_stream_post_input (audio_stream_scheduling.c), and FUN_006e5590, whose
-- 0x6e56ac..0x6e56ec stores the queued position as the descriptor source with destination
-- 0xfffa0020 or 0xfffa1020.
readers :: [Extent]
readers =
  [ extent 0x6ed0d8 0x6ed150 "c673cbc0beda6b94ed84730f78651a7da7739d1855ec82c343669aa7cddf94ee"
  , extent 0x6ed150 0x6ed1e0 "4d4644f2ace405d7ac4022ad045bce8acc22a04991db5a69413b715813dc4d0e"
  , extent 0x6ed208 0x6ed2f0 "400103d4ff886b1662b0ec12d287fea6cb41e2d1dda0375ab610c3d91811b50e"
  , extent 0x6ecb64 0x6ecedc "ed674fd7e1cfea20a264c91ed2a361b47f1c6407dc7231aae39878a30794659e"
  , extent 0x6ec86c 0x6ec9f8 "17967fb03815cc03bae0037c6ee53a875f7f3f42eefaacc633c7b1ed01eaaa4a"
  , extent 0x6ec660 0x6ec694 "9a9095a0303a3a96ff4d6fa6d0708d38529247e63e39d43a29857337190c612e"
  , extent 0x6e5590 0x6e58d0 "1a00633b2219e95b20b9c65d143412b4424c637cdbf754a99ecac4afc222983e"
  ]

-- | sound_clips [0x7dceb0,0x7dcf60) and sound_tracks [0x810f3c,0x81102c).
tables :: [Extent]
tables =
  [ extent 0x7dceb0 0x7dcf60 "d70dd0e4c9eaa43eb1ad8907a998a39eae2b1db75da9a3ad3f7108b43550f2d8"
  , extent 0x810f3c 0x81102c "5cf9d3878091178c71e517ee69c7971835dc65cc74af6696e324bc7fb44f5ac2"
  ]

-- | The hull of all blocks.
sampleRegion :: Extent
sampleRegion = extent 0x7d2180 0x812b90 "98bdd3780d056c62695f5a08aff292fc6d2882813c577494de5340c06367e0d8"

-- | Blocks in walk order: clips 17-21, then track entries by track, operation, edge.
reviewedBlocks :: [(Word32, Word32, String)]
reviewedBlocks =
  [ (0x7d99d0, 0x7da0d0, "ae2f8c0200db922ea989b04082c0b46afaabe919bc3f309d17f45b3f1b7ad7fd")
  , (0x7dbbf0, 0x7dceb0, "c95863f09feb1a14aca4ac3b6d964615204e7406bc7416912cb463485df6051d")
  , (0x7eb00c, 0x80270c, "910f7ff8dcb4603a39c5671136461480e5db67244703a51ee6ea8bbad62b2368")
  , (0x7d2180, 0x7d4580, "0e2a7324c0c224f2c3689b0974670119dade8d5c261ce33caee94b9ed081ad8f")
  , (0x803f90, 0x80c710, "3c9f916ae2abceae1324f65a4058db5d18148124e2ebb1d29f9f0f35259f4873")
  , (0x811030, 0x812b90, "5673edad881014316f47c931937a35fe50ba82885140b07d30b1a2c2e5141351")
  , (0x7e1e7c, 0x7e383c, "22f11d8fddb6ab7b6390d0817ce76bef10fe25eb9f40e07fe74537b15c12e3eb")
  , (0x7d4588, 0x7d99c8, "2768af537b44cfc062fb3c6b1cd505a01c1e231457caf53d1055f10c7202bcd5")
  , (0x811030, 0x812b90, "5673edad881014316f47c931937a35fe50ba82885140b07d30b1a2c2e5141351")
  , (0x80f3d8, 0x810f38, "761dd60735f7651501cf78b71fef24f4993266685a34a2318a6ede24d95b4535")
  , (0x80270c, 0x803f8c, "058b98b6604d0cb1f2e0e58f3cf90ca3fc7bb8cf0a43bebc012e6d694e8cb921")
  , (0x7e383c, 0x7e683c, "6339e87563737e84007fa115d8badbc5d783d54d5174317b5c042be5f74179e4")
  , (0x80f3d8, 0x810f38, "761dd60735f7651501cf78b71fef24f4993266685a34a2318a6ede24d95b4535")
  , (0x7e683c, 0x7eaffc, "5fc19e24d4b4e3dd451fb4f640a1baf9b9462ab07a40c261a457dadd63adc5ad")
  , (0x80c718, 0x80f3d8, "26ccf0766699cda0317b14a4be045453dddd549f12fbc4a13aae4aa1e93f4a0e")
  , (0x7dcf64, 0x7e1e64, "4011bbcccae95ca7788cd8b46a135ba1d8311a57bca0bfea764bb0f5116b1987")
  , (0x7da0d0, 0x7dbbf0, "431a4c6fce7563693adbc422b12e4e0209316ee5c641a9ae74db562efa071894")
  ]

sites :: [(GuestAddress, Word32, BlockName)]
sites =
  [ (GuestAddress 0x7d4ebc, 0x00620008, track 2 Operation1 End)
  , (GuestAddress 0x7d5820, 0x00620113, track 2 Operation1 End)
  , (GuestAddress 0x7d5c84, 0x00620038, track 2 Operation1 End)
  , (GuestAddress 0x7d5ff8, 0x00620000, track 2 Operation1 End)
  , (GuestAddress 0x7d6168, 0x0062003e, track 2 Operation1 End)
  , (GuestAddress 0x7d62b8, 0x00620012, track 2 Operation1 End)
  , (GuestAddress 0x7d6864, 0x0062fffe, track 2 Operation1 End)
  , (GuestAddress 0x7d9e64, 0x00620070, ClipBlock (ClipIndex 17))
  , (GuestAddress 0x7d9ec4, 0x00630062, ClipBlock (ClipIndex 17))
  , (GuestAddress 0x7df194, 0x0062fef3, track 4 Operation1 End)
  , (GuestAddress 0x7e1fd4, 0x0062006c, track 2 Operation1 Begin)
  , (GuestAddress 0x7e3be0, 0x0bf90bc3, track 3 Operation1 End)
  , (GuestAddress 0x7e4e74, 0x0062003e, track 3 Operation1 End)
  , (GuestAddress 0x7e5098, 0x0062002e, track 3 Operation1 End)
  , (GuestAddress 0x7e50c8, 0x0062006c, track 3 Operation1 End)
  , (GuestAddress 0x7e53dc, 0x00620080, track 3 Operation1 End)
  , (GuestAddress 0x7e9bb0, 0x0062fef3, track 4 Operation0 Begin)
  , (GuestAddress 0x8039e0, 0x0062ff66, track 3 Operation1 Begin)
  , (GuestAddress 0x80e7e0, 0x0062fe7b, track 4 Operation1 Begin)
  , (GuestAddress 0x80f968, 0x0062009c, track 3 Operation0 Begin)
  , (GuestAddress 0x810c30, 0x00620044, track 3 Operation0 Begin)
  , (GuestAddress 0x81201c, 0x00620040, track 2 Operation0 Begin)
  ]
  where
    track index = TrackBlock (TrackIndex index)

extent :: Word32 -> Word32 -> String -> Extent
extent start end digest = Extent (GuestAddress start) (GuestAddress end) (reviewedHash digest)

reviewedHash :: String -> SHA.Sha256
reviewedHash = either (error . ("retained PCM digest: " ++)) id . SHA.parseSha256Hex
