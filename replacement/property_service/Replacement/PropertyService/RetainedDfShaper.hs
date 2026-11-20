{-# LANGUAGE GHC2021 #-}

-- | Plain-image words of the 'DFShaper' Arabic ligature blob whose value lies in retired property
-- code, closed as consumed data. The blob closes when its bytes, its readers' bytes, its format and
-- the census over all seven images hold: the format tiles the blob with objects whose every field is
-- a magic, count, glyph code or offset relative to a blob object; every address in the blob that an
-- instruction forms is formed inside a bound reader; a word equal to an object start is a literal
-- inside a bound reader that only bound readers load; no branch lands in the blob. Computed addresses
-- are outside this census.
--
-- Citations: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, decomp/firmwares/xm1-150,
-- app/graphics/fonts/tu_01187034.c, tu_0118bda0.c, font_glyph_access.c, font_stream_helpers.c.
module Replacement.PropertyService.RetainedDfShaper
  ( dfShaperClosures
  ) where

import Control.Monad (foldM, guard)
import Data.Bits (shiftL, (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe)
import qualified Data.Set as S
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import Replacement.PropertyService.Placement
  ( Carrier (..)
  , GuestAddress (..)
  , Reference (..)
  , ScanHit (..)
  , ScanImage (..)
  , hitTarget
  , inCarrier
  , scanReferences
  )
import Replacement.PropertyService.RetainedClosure (ClosedRole (..), ClosedSite (..), extentMatches, imageFor)
import ServiceInventory (Image (..), imageWord)

data Extent = Extent GuestAddress GuestAddress SHA.Sha256

-- | A byte offset from the blob start.
newtype BlobOffset = BlobOffset Int
  deriving (Eq, Ord)

newtype ByteCount = ByteCount Int

-- | Object starts and the offsets of ligature-set offset entries.
data Layout = Layout
  { layoutObjects :: S.Set BlobOffset
  , layoutLigatureOffsetEntries :: S.Set BlobOffset
  }

dfShaperClosures :: [(ScanImage, Image)] -> [ClosedSite]
dfShaperClosures images = fromMaybe [] $ do
  guard (all (\label -> any ((== label) . fst) images) [minBound .. maxBound])
  plain <- imageFor ScanPlain images
  guard (all (bound plain) (container : readers))
  blob <- containerBytes plain
  layout <- shaperLayout blob
  guard (censusClear images layout)
  pure
    [ ClosedSite ScanPlain site word ConsumedData
    | (site@(GuestAddress address), word) <- sites
    , imageWord plain address == Just word
    , all (`S.member` layoutLigatureOffsetEntries layout) [offsetOf address, offsetOf (address + 2)]
    ]
  where
    bound image (Extent start end digest) = extentMatches image start end digest

-- | Two ligature-set offset entries each: entries 1 and 2 of the set at 0x118394c and entries 5
-- and 6 of the set at 0x1184498, halfwords 0x005a and 0x0062.
sites :: [(GuestAddress, Word32)]
sites =
  [ (GuestAddress 0x1183950, 0x0062005a)
  , (GuestAddress 0x11844a4, 0x0062005a)
  ]

container :: Extent
container = extent 0x118360c 0x11859dc "299754f49cfb6d8836c9c5f2f900492af7443fc757cc93b543d73202e04314de"

blobStart :: Word32
blobStart = 0x118360c

blobCarrier :: Carrier
blobCarrier = let Extent start end _ = container in Carrier start end

offsetOf :: Word32 -> BlobOffset
offsetOf address = BlobOffset (fromIntegral (address - blobStart))

-- | overlay_font_object_create and FUN_011831bc load the blob base from literals 0x1181c94 and
-- 0x1183420 (at 0x1181b48, 0x1183354) as r0 of overlay_font_initialize_engine_backends, which keeps
-- it at r11 - 0x24 and stores it only to engine + 0x70 and engine + 4 (0x118f484, 0x118f4c8).
-- The [engine + 0xff8] backends it calls receive [engine + 0x68], [arg1 + 0x64], [engine + 0x1030]
-- and [engine + 0x1034], not the blob. Blob bytes are read only by ldrb pairs through engine + 0x70
-- or engine + 4 plus a blob offset: overlay_font_read_directory (header), FUN_0118ce2c (script
-- record), FUN_0118757c and overlay_font_shape_arabic (lookup header, the latter keeping
-- blob + lookup + 6 for overlay_font_find_ligature), overlay_font_read_glyph and FUN_0118cb08
-- (coverage glyphs), overlay_font_find_ligature (lookup offset to the ligature set, then its count)
-- and overlay_font_read_run, which adds the ligature-set offset entry at set + 2 + 2 * index to the
-- set offset and reads the record's glyph, category, count and components.
readers :: [Extent]
readers =
  [ extent 0x11818c8 0x1181c98 "51ccdfba026a56e4d8dcaff64b2819bb615d9fb1a38a7927d217b87f6f0cf65e"
  , extent 0x11831bc 0x1183424 "abb8e5d14e47f55409498346dc163441d4d474c0ac64969e3e8bfb59ca7e7726"
  , extent 0x118effc 0x118f68c "304c595c621ce079576be7e652d7ec1351e4b6b1fa107372e4c8fbcd3cc2eef9"
  , extent 0x118cc68 0x118ce2c "31d58858a639de1e85379c02178871aaf27ebe53ee54b8b0ec6f3dd82e1bda6d"
  , extent 0x118ce2c 0x118cfc4 "c7b46704e70d7d11378a254cc29a5d850a11f37ce1d9f3e5ff6129dabd5b0b21"
  , extent 0x11874d0 0x118757c "f2d9c61b51a247b2c9430d3191b2f88a738aa70b5666e844bf895a492bc4d224"
  , extent 0x118757c 0x1187638 "5f54878179ab8441b34d2cee1183034c7819baac1a476c791e376ed5bdb5df68"
  , extent 0x1187638 0x118768c "b1d780f7885599a21c99e205591debea49c2ad61b1ec1df43f9709cea0c4c457"
  , extent 0x118768c 0x1187934 "6f09477ba2d43472d0477be768d75359dd8ad19c8c20975f1f84bceccef1ecb3"
  , extent 0x1187c08 0x1188cd8 "cbbf3c5d73505d008d68056abfb3c9324b553f9772753b026181f7bbd5548a8a"
  , extent 0x118cb08 0x118cb7c "dbeb7d4d023b3d7b96e2f22872004f45b3aa5c5b91d820f5b1aa8845c470af63"
  , extent 0x118cb7c 0x118cba4 "a48b52abb5234338d428f796a902d9aafede5eabc10d0abaaecf3dcffe0906df"
  , extent 0x118cbe8 0x118cc20 "4a6a7a1dcb3c0847cf48fce7d23cd462c7599c237aeb4bc6c05d5b623a8ff6ba"
  ]

containerBytes :: Image -> Maybe B.ByteString
containerBytes image = do
  let Extent (GuestAddress start) (GuestAddress end) _ = container
  guard (start >= imageBase image)
  let bytes = B.take (fromIntegral (end - start)) (B.drop (fromIntegral (start - imageBase image)) (imageBytes image))
  guard (B.length bytes == fromIntegral (end - start))
  pure bytes

-- | Header {"DFShaper", u16 version, u16 n, u32 script[n]}; script {u16 script, u16 language,
-- u16 n, u32 lookup[n] from the script}; lookup {u16 1, u16 coverage, u16 n, u16 set[n] from the
-- lookup}; coverage {u16 1, u16 n, u16 glyph[n]}, n equal to the lookup's; ligature set {u16 n,
-- u16 record[n] from the set}; record {u16 glyph, u16 category, u16 n >= 1, u16 component[n - 1]}.
-- The objects tile the blob up to at most three zero bytes of alignment.
shaperLayout :: B.ByteString -> Maybe Layout
shaperLayout blob = do
  guard (B.take 8 blob == BC.pack "DFShaper")
  scriptCount <- u16 (BlobOffset 0xa)
  scripts <- mapM (\index -> relative (BlobOffset 0) =<< u32 (BlobOffset (0xc + 4 * index))) [0 .. scriptCount - 1]
  let header = (BlobOffset 0, ByteCount (0xc + 4 * scriptCount))
  scriptObjects <- mapM scriptObject scripts
  lookups <- concat <$> mapM lookupStarts scripts
  lookupObjects <- mapM lookupObject lookups
  coverageObjects <- mapM coverageObject lookups
  sets <- concat <$> mapM setStarts lookups
  setObjects <- mapM setObject sets
  records <- concat <$> mapM recordStarts sets
  recordObjects <- mapM recordObject records
  objects <- foldM insertObject M.empty (header : scriptObjects ++ lookupObjects ++ coverageObjects ++ setObjects ++ recordObjects)
  end <- foldM tile (BlobOffset 0) (M.toList objects)
  let BlobOffset tail' = end
      padding = B.drop tail' blob
  guard (B.length padding <= 3 && B.all (== 0) padding)
  entries <- concat <$> mapM setEntries sets
  pure (Layout (M.keysSet objects) (S.fromList entries))
  where
    scriptObject start = do
      count <- u16 (advance start 4)
      pure (start, ByteCount (6 + 4 * count))
    lookupStarts start = do
      count <- u16 (advance start 4)
      mapM (\index -> relative start =<< u32 (advance start (6 + 4 * index))) [0 .. count - 1]
    lookupObject start = do
      format <- u16 start
      guard (format == 1)
      count <- u16 (advance start 4)
      pure (start, ByteCount (6 + 2 * count))
    coverageObject table = do
      coverage <- relative table =<< u16 (advance table 2)
      setCount <- u16 (advance table 4)
      format <- u16 coverage
      count <- u16 (advance coverage 2)
      guard (format == 1 && count == setCount)
      pure (coverage, ByteCount (4 + 2 * count))
    setStarts table = do
      count <- u16 (advance table 4)
      mapM (\index -> relative table =<< u16 (advance table (6 + 2 * index))) [0 .. count - 1]
    setObject start = do
      count <- u16 start
      pure (start, ByteCount (2 + 2 * count))
    setEntries start = do
      count <- u16 start
      pure [advance start (2 + 2 * index) | index <- [0 .. count - 1]]
    recordStarts start = do
      count <- u16 start
      mapM (\index -> relative start =<< u16 (advance start (2 + 2 * index))) [0 .. count - 1]
    recordObject start = do
      components <- u16 (advance start 4)
      guard (components >= 1)
      pure (start, ByteCount (4 + 2 * components))
    insertObject objects (start, ByteCount size) = case M.lookup start objects of
      Just existing -> objects <$ guard (existing == size)
      Nothing -> pure (M.insert start size objects)
    tile cursor (start, size) = do
      guard (cursor == start)
      pure (advance start size)
    advance (BlobOffset offset) count = BlobOffset (offset + count)
    relative (BlobOffset from) offset = do
      let target = from + offset
      guard (target < B.length blob)
      pure (BlobOffset target)
    byte offset = if offset >= 0 && offset < B.length blob then Just (fromIntegral (B.index blob offset)) else Nothing
    u16 (BlobOffset offset) = do
      low <- byte offset
      high <- byte (offset + 1)
      pure (low .|. (high `shiftL` 8) :: Int)
    u32 offset = do
      low <- u16 offset
      high <- u16 (advance offset 2)
      pure (low .|. (high `shiftL` 16))

-- | Every hit into the blob from outside it is accepted: a word that is not an object start, or a
-- literal inside a bound reader that only PC-relative loads inside bound readers reach; a MOVW/MOVT
-- pair or PC-relative load inside a bound reader. Branches are refused.
censusClear :: [(ScanImage, Image)] -> Layout -> Bool
censusClear images layout = all accepted (filter (not . ownBytes) (scanReferences blobCarrier images S.empty))
  where
    ownBytes hit = hitImage hit == ScanPlain && inCarrier blobCarrier (hitSite hit)
    objectStart (GuestAddress address) = S.member (offsetOf address) (layoutObjects layout)
    inReader hit = hitImage hit == ScanPlain && any (\(Extent start end _) -> inCarrier (Carrier start end) (hitSite hit)) readers
    accepted hit = case hitReference hit of
      WordInto -> not (objectStart (hitTarget hit)) || (inReader hit && literalLoadsBound (hitSite hit))
      MovPairInto _ -> inReader hit
      PcRelativeInto _ -> inReader hit
      BranchInto _ -> False
    literalLoadsBound (GuestAddress literal) =
      let loads = scanReferences (Carrier (GuestAddress literal) (GuestAddress (literal + 4))) images S.empty
      in not (null loads) && all (\hit -> isPcRelative (hitReference hit) && inReader hit) loads
    isPcRelative reference = case reference of
      PcRelativeInto _ -> True
      _ -> False

extent :: Word32 -> Word32 -> String -> Extent
extent start end digest = Extent (GuestAddress start) (GuestAddress end) (reviewedHash digest)

reviewedHash :: String -> SHA.Sha256
reviewedHash = either (error . ("retained DFShaper digest: " ++)) id . SHA.parseSha256Hex
