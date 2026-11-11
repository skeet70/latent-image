{-# LANGUAGE GHC2021 #-}

-- | Serialized GVFF field evidence. This module does not establish admission.
module Replacement.PropertyService.RetainedGvff
  ( GvffFieldMap
  , GvffFieldRole (..)
  , GvffField (..)
  , GvffUnclosedPremise (..)
  , deriveGvffFieldMap
  , gvffFieldAt
  , gvffUnclosedPremises
  ) where

import Control.Monad (foldM, guard, unless)
import Data.Bits (shiftL, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (sortOn)
import Data.List.NonEmpty (NonEmpty (..))
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import Replacement.PropertyService.Classify (GuestAddress (..))
import ServiceInventory (Image (..))

-- | Neither field is loaded by 0x6da610..0x6dad8c.
data GvffFieldRole = ReservedHeader04 | ReservedHeader24
  deriving (Eq, Ord, Show)

data GvffField = GvffField
  { gvffFieldRole :: GvffFieldRole
  , gvffFieldWord :: Word32
  } deriving (Eq, Show)

newtype GvffFieldMap = GvffFieldMap (M.Map GuestAddress GvffField)

-- | 0x84f234 and 0x84f314 pass serialized payload pointers to [0x890fac].
data GvffUnclosedPremise
  = Callback890facWriteClosure
  | LabelPayloadAliasClosure
  | ProductionFieldAndDispatchLifetime
  deriving (Eq, Show)

gvffUnclosedPremises :: GvffFieldMap -> NonEmpty GvffUnclosedPremise
gvffUnclosedPremises _ =
  Callback890facWriteClosure :| [LabelPayloadAliasClosure, ProductionFieldAndDispatchLifetime]

gvffFieldAt :: GvffFieldMap -> GuestAddress -> Maybe GvffField
gvffFieldAt (GvffFieldMap fields) site = M.lookup site fields

newtype Cursor = Cursor Integer deriving (Eq, Ord)
newtype ByteCount = ByteCount Integer
newtype RecordCount = RecordCount Integer

data Block = Block Cursor Cursor [(GuestAddress, GvffField)]
data ConsumerExtent = ConsumerExtent GuestAddress GuestAddress SHA.Sha256

deriveGvffFieldMap :: Image -> Image -> Image -> Image -> Maybe GvffFieldMap
deriveGvffFieldMap image2 image4 plain loader = do
  guard (imageBase image2 == 0x130000)
  guard (imageBase image4 == 0x61c400)
  guard (imageBase plain == 0x1100000)
  guard (imageBase loader == 0x40000)
  guard (SHA.sha256 (imageBytes image4) == reviewedHash "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")
  guard (SHA.sha256 (imageBytes plain) == reviewedHash "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d")
  guard (SHA.sha256 (imageBytes loader) == reviewedHash "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882")
  mapM_ (bindConsumer image4) image4Consumers
  mapM_ (bindConsumer plain) plainConsumers
  bindConsumer loader (extent 0x4357c 0x435ac "e50d121ff6c8881f53d3720e9570e6511103f7d253577e90f9665b9f038ba5e3")
  blocks <- mapM (recordBlock image2) (map (Cursor . (0x22a8b4 +) . (12 *)) [0..2244] ++ [Cursor 0x37bab0])
  let ordered = sortOn blockStart blocks
  guard (and (zipWith disjoint ordered (drop 1 ordered)))
  guard (all outsideTables ordered)
  let fields = concatMap blockFields ordered
  pure (GvffFieldMap (M.fromList fields))
  where
    blockStart (Block start _ _) = start
    blockFields (Block _ _ fields) = fields
    disjoint (Block _ end _) (Block start _ _) = end <= start
    outsideTables (Block (Cursor start) (Cursor end) _) =
      all (\(low,high) -> end <= low || high <= start)
        [(0x22a8b4,0x22a8b4 + 2245*12),(0x37bab0,0x37babc)]

recordBlock :: Image -> Cursor -> Maybe Block
recordBlock image record = do
  _ <- bytesAt image record (ByteCount 12)
  kind <- wordAt image (advance record (ByteCount 4))
  guard (kind == 1 || kind == 2)
  pointer <- wordAt image (advance record (ByteCount 8))
  let start = Cursor (toInteger pointer)
  guard (pointer .&. 3 == 0)
  _ <- bytesAt image start (ByteCount 56)
  magic <- wordAt image start
  version <- wordAt image (advance start (ByteCount 8))
  encoding <- wordAt image (advance start (ByteCount 12))
  headerBytes <- wordAt image (advance start (ByteCount 16))
  guard (magic == 0x46465647 && version == 0x10002 && encoding == 0x1902cf && headerBytes == 56)
  paths <- countAt image (advance start (ByteCount 20))
  paints <- countAt image (advance start (ByteCount 24))
  ramps <- countAt image (advance start (ByteCount 28))
  afterRamps <- walkRecords image (ByteCount 4) ramps (advance start (ByteCount 56)) (walkRamp image)
  afterPaints <- walkRecords image (ByteCount 36) paints afterRamps (walkPaint image ramps)
  end <- walkRecords image (ByteCount 56) paths afterPaints (walkPath image paints)
  fields <- mapM (reservedField image start) [(ByteCount 4, ReservedHeader04),(ByteCount 36, ReservedHeader24)]
  pure (Block start end fields)

reservedField :: Image -> Cursor -> (ByteCount, GvffFieldRole) -> Maybe (GuestAddress, GvffField)
reservedField image start (offset, role) = do
  let cursor@(Cursor address) = advance start offset
  value <- wordAt image cursor
  guard (address <= toInteger (maxBound :: Word32))
  pure (GuestAddress (fromInteger address), GvffField role value)

-- | 0x6da864..0x6da880 advances by 4 + 20 * stopCount.
walkRamp :: Image -> Cursor -> Maybe Cursor
walkRamp image start = do
  RecordCount stops <- countAt image start
  let size = ByteCount (4 + 20*stops)
  _ <- bytesAt image start size
  pure (advance start size)

walkPaint :: Image -> RecordCount -> Cursor -> Maybe Cursor
walkPaint image (RecordCount ramps) start = do
  _ <- bytesAt image start (ByteCount 36)
  kind <- wordAt image (advance start (ByteCount 4))
  case kind of
    0x1b00 -> pure ()
    0x1b01 -> rampIndex
    0x1b02 -> rampIndex
    _ -> Nothing
  pure (advance start (ByteCount 36))
  where
    rampIndex = do
      index <- wordAt image (advance start (ByteCount 28))
      guard (toInteger index < ramps)

walkPath :: Image -> RecordCount -> Cursor -> Maybe Cursor
walkPath image (RecordCount paints) start = do
  _ <- bytesAt image start (ByteCount 52)
  flags <- wordAt image start
  guard (flags .&. 0xfffffffc == 0)
  mapM_ (checkPaint flags) [(1,12),(2,16)]
  tailIndex <- wordAt image (advance start (ByteCount 20))
  guard (tailIndex == 666)
  RecordCount segments <- countAt image (advance start (ByteCount 44))
  coordinateBytes <- wordAt image (advance start (ByteCount 48))
  codes <- bytesAt image (advance start (ByteCount 52)) (ByteCount segments)
  floats <- B.foldl' addSegment (Just 0) codes
  guard (4*floats == toInteger coordinateBytes)
  let size = ByteCount (52 + segments + (4 - segments `mod` 4) + toInteger coordinateBytes)
  _ <- bytesAt image start size
  pure (advance start size)
  where
    checkPaint flags (mask,offset) = unless (flags .&. mask == 0) $ do
      index <- wordAt image (advance start (ByteCount offset))
      guard (toInteger index < paints)
    addSegment total code = do
      n <- total
      case code of
        0 -> Just n
        2 -> Just (n+2)
        4 -> Just (n+2)
        12 -> Just (n+6)
        _ -> Nothing

walkRecords :: Image -> ByteCount -> RecordCount -> Cursor -> (Cursor -> Maybe Cursor) -> Maybe Cursor
walkRecords image (ByteCount minimumBytes) (RecordCount count) start@(Cursor address) step = do
  let imageEnd = toInteger (imageBase image) + toInteger (B.length (imageBytes image))
  guard (count >= 0 && count <= (imageEnd-address) `div` minimumBytes)
  foldM (\cursor _ -> step cursor) start [1..count]

advance :: Cursor -> ByteCount -> Cursor
advance (Cursor address) (ByteCount count) = Cursor (address+count)

bytesAt :: Image -> Cursor -> ByteCount -> Maybe B.ByteString
bytesAt image (Cursor address) (ByteCount count) = do
  let offset = address - toInteger (imageBase image)
      size = toInteger (B.length (imageBytes image))
  guard (offset >= 0 && count >= 0 && offset <= size && count <= size-offset)
  guard (address+count <= 0x100000000)
  pure (B.take (fromInteger count) (B.drop (fromInteger offset) (imageBytes image)))

wordAt :: Image -> Cursor -> Maybe Word32
wordAt image cursor = do
  bytes <- bytesAt image cursor (ByteCount 4)
  pure (B.foldr (\byte value -> (value `shiftL` 8) .|. fromIntegral byte) 0 bytes)

countAt :: Image -> Cursor -> Maybe RecordCount
countAt image cursor = do
  value <- wordAt image cursor
  guard (value <= 0x7fffffff)
  pure (RecordCount (toInteger value))

bindConsumer :: Image -> ConsumerExtent -> Maybe ()
bindConsumer image (ConsumerExtent (GuestAddress start) (GuestAddress end) expected) = do
  guard (end > start)
  bytes <- bytesAt image (Cursor (toInteger start)) (ByteCount (toInteger end-toInteger start))
  guard (SHA.sha256 bytes == expected)

-- | Complete inspected base methods/initializer, GVFF methods/parser, factory,
-- copy/clear primitives, vtable, paint/path consumers, and callback thunks.
image4Consumers :: [ConsumerExtent]
image4Consumers =
  [ extent 0x6d8140 0x6d8784 "66dcb10e026cda75d72cd986739cd0d2fd202ef3adc83e4506df8265f78bb139"
  , extent 0x6d9f94 0x6dad8c "536d19d0ad85612f3fe08a32b20ed4c57fdb753177a04577ce7703dfaff69358"
  , extent 0x6dce28 0x6dcfec "709e212ba1595619dcd988f7128f6579a3f126c6f931166d5daa08cfc279e90c"
  , extent 0x722054 0x722178 "fdd89402855fb6d349f227be3f77f6d84336f1dfd1ff5e4ffe5bb88e27d8187f"
  , extent 0x7221a0 0x722218 "51b1a1f121246c404e91db21363301158e6364c34c679ed8a081a7ca96bd899b"
  , extent 0x7cfc94 0x7cfd3c "d8b172b84a957714236bff3159ba958a5aff43bc48907f89b16978dc1e67c935"
  , extent 0x840cd0 0x8418fc "4083cbf91553bf7fce8714e828435214d927be1b1adc99f8123b45d34ea856c1"
  , extent 0x84efc8 0x84f374 "f260bac3b1a23a528e62d9c2932d9e7ddc833a9f4afae13e145e988b4617ae56"
  , extent 0x859480 0x859660 "8d70558722a5a98ce52722385ad51d6c2d3ecec005993d7200067d2e5c0043e4"
  , extent 0x85a614 0x85a6c0 "0961c63a07d645fbafe497f6d5919cf58f62e618f759e1b7ac5e613f628e0327"
  , extent 0x85aea8 0x85af48 "b1a1ba9e8c963ce7d79d2420b2ef6b6216b395e2a0ba6d11b4e3f9788a75cacc"
  , extent 0x890ec0 0x890fb0 "df274eddc5669768294e23f34f2ed864165bbd5e9e17142c45c9b173ea9ff7ad"
  ]

-- | Kind dispatch, descriptor preparation/factory call, and Q accessors.
plainConsumers :: [ConsumerExtent]
plainConsumers =
  [ extent 0x1179a50 0x1179ab4 "90efdc6784c283912624846f836cb9fe6813fd24229dcd9a514817a3ccd4ebd7"
  , extent 0x117d200 0x117d844 "21f99489ae230e87af7e4a18f49f3f987d6272a2b00c40cf13d0609a8f41bc57"
  , extent 0x117e100 0x117e1b8 "99d2e19d631012d32f2e68b6ee0babd8085a8d838e20a4947de7b73684f2c97d"
  ]

extent :: Word32 -> Word32 -> String -> ConsumerExtent
extent start end expected = ConsumerExtent (GuestAddress start) (GuestAddress end) (reviewedHash expected)

reviewedHash :: String -> SHA.Sha256
reviewedHash = either (error . ("retained GVFF digest: " ++)) id . SHA.parseSha256Hex
