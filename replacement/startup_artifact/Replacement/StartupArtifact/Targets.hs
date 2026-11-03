module Replacement.StartupArtifact.Targets
  ( TargetBytes, targetBytes, targetOf, targetList
  , targetBase, preTableTargets, imageExtent
  , extractPreTable, applyPreTableEdits, restoreTargets
  , applyTargetEdits, diffSpans
  , normalizeSpans, intersectSpan, subtractSpans, coveredBy
  ) where

import Control.Monad (forM, forM_, unless, when)
import Data.Bits (complement)
import qualified Data.ByteString as B
import Data.List (sort, sortOn)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import Replacement.StartupArtifact.Types

-- | Decoded, uncomplemented bytes of all six targets.
newtype TargetBytes = TargetBytes (M.Map Target B.ByteString) deriving (Eq, Show)

targetBytes :: M.Map Target B.ByteString -> Maybe TargetBytes
targetBytes values
  | M.keys values == [minBound .. maxBound] = Just (TargetBytes values)
  | otherwise = Nothing

targetOf :: TargetBytes -> Target -> B.ByteString
targetOf (TargetBytes values) target = M.findWithDefault B.empty target values

targetList :: TargetBytes -> [(Target, B.ByteString)]
targetList (TargetBytes values) = M.toList values

-- | Guest address of target byte 0.
targetBase :: Target -> Word32
targetBase target = case target of
  Record1 -> 0x0
  Record2 -> 0x10000
  LoaderA -> 0x40000
  LoaderB -> 0xd0000
  Image4 -> 0x61c400
  Image5 -> 0xbf1a00

preTableTargets :: [Target]
preTableTargets = [Record1, Record2, LoaderA, LoaderB]

-- | Pre-table offset and extent of a record slot or loader segment.
preTableLocation :: U.Parts -> Target -> Either Refusal (Int, Int)
preTableLocation parts target = do
  (offset, extent) <- case target of
    Record1 -> pure (0x40000, 0x10000)
    Record2 -> pure (0x50000, 0x10000)
    LoaderA -> segment 0 0x40000 0xdae0
    LoaderB -> segment 1 0xd0000 0xdaa0
    _ -> Left (TargetGeometryRefused target)
  unless (offset + extent <= B.length (U.partPreTable parts)) (Left (TargetGeometryRefused target))
  pure (offset, extent)
  where
    segment ident load size = case filter ((== ident) . U.segId) (U.partSegments parts) of
      [found] | U.segLoad found == load && U.segSize found == size -> pure (fromIntegral (U.segFlash found), fromIntegral size)
      _ -> Left (TargetGeometryRefused target)

preTableLocations :: U.Parts -> Either Refusal [(Target, (Int, Int))]
preTableLocations parts = do
  located <- forM preTableTargets $ \target -> (,) target <$> preTableLocation parts target
  let ordered = sortOn (fst . snd) located
  forM_ (zip ordered (drop 1 ordered)) $ \((target, (offset, extent)), (_, (next, _))) ->
    when (offset + extent > next) (Left (TargetGeometryRefused target))
  pure located

imageExtent :: U.Parts -> Target -> Either Refusal Int
imageExtent parts target = case target of
  Image4 -> do
    let size = decoded (U.partRecord4 parts)
    unless (size == 0x282800) (Left (TargetGeometryRefused Image4))
    pure size
  Image5 -> pure (decoded (U.partRecord5 parts))
  _ -> Left (TargetGeometryRefused target)
  where decoded = fromIntegral . U.recDecodedSize . U.recHeader

-- | Record slots and loaders, uncomplemented.
extractPreTable :: U.Parts -> Either Refusal (M.Map Target B.ByteString)
extractPreTable parts = do
  located <- preTableLocations parts
  pure (M.fromList [(target, B.map complement (slice offset extent (U.partPreTable parts))) | (target, (offset, extent)) <- located])

slice :: Int -> Int -> B.ByteString -> B.ByteString
slice offset count = B.take count . B.drop offset

splice :: Int -> B.ByteString -> B.ByteString -> B.ByteString
splice offset bytes base = B.take offset base <> bytes <> B.drop (offset + B.length bytes) base

relative :: Span -> Int
relative (Span target (Guest start) _) = fromIntegral (start - targetBase target)

-- | Writes pre-table target edits complemented; other edits refuse.
applyPreTableEdits :: [Edit] -> U.Parts -> Either Refusal U.Parts
applyPreTableEdits edits parts = do
  located <- preTableLocations parts
  patched <- foldl (\current change -> current >>= \pre -> do
    let range = editSpan change
        target = spanTarget range
    (offset, extent) <- maybe (Left (TargetGeometryRefused target)) Right (lookup target located)
    unless (relative range >= 0 && relative range + spanLength range <= extent) (Left (TargetGeometryRefused target))
    pure (splice (offset + relative range) (B.map complement (editBytes change)) pre)) (Right (U.partPreTable parts)) edits
  pure parts {U.partPreTable = patched}

-- | The staged parts with every target restored from the canonical parts:
-- the pre-table target bytes, record 4 with its gap and record 5 with its post-head.
restoreTargets :: U.Parts -> U.Parts -> Maybe U.Parts
restoreTargets canonical staged = do
  located <- either (const Nothing) Just (preTableLocations canonical)
  stagedLocated <- either (const Nothing) Just (preTableLocations staged)
  unless (sort located == sort stagedLocated && B.length (U.partPreTable canonical) == B.length (U.partPreTable staged)) Nothing
  let pre = foldl (\current (_, (offset, extent)) -> splice offset (slice offset extent (U.partPreTable canonical)) current)
        (U.partPreTable staged) located
  pure staged
    { U.partPreTable = pre
    , U.partRecord4 = U.partRecord4 canonical, U.partGap45 = U.partGap45 canonical
    , U.partRecord5 = U.partRecord5 canonical, U.partPostHead = U.partPostHead canonical
    }

applyTargetEdits :: [Edit] -> TargetBytes -> Either Refusal TargetBytes
applyTargetEdits edits (TargetBytes values) = TargetBytes <$> foldl step (Right values) edits
  where
    step current change = current >>= \bytes -> do
      let range = editSpan change
          target = spanTarget range
          original = M.findWithDefault B.empty target bytes
      unless (relative range >= 0 && relative range + spanLength range <= B.length original) (Left (TargetGeometryRefused target))
      pure (M.insert target (splice (relative range) (editBytes change) original) bytes)

-- | Maximal runs of differing bytes between equal-length images.
diffSpans :: Target -> B.ByteString -> B.ByteString -> [Span]
diffSpans target before after = runs 0 Nothing
  where
    size = min (B.length before) (B.length after)
    base = targetBase target
    chunk = 4096
    close start end = [Span target (Guest (base + fromIntegral start)) (Guest (base + fromIntegral end))]
    runs index open
      | index >= size = maybe [] (`close` size) open
      | open == Nothing && index + chunk <= size && slice index chunk before == slice index chunk after = runs (index + chunk) Nothing
      | B.index before index /= B.index after index = runs (index + 1) (Just (maybe index id open))
      | otherwise = maybe [] (`close` index) open ++ runs (index + 1) Nothing

-- | Sorted, with overlapping or abutting spans of one target merged.
normalizeSpans :: [Span] -> [Span]
normalizeSpans = merge . sort
  where
    merge (Span target start end : Span other otherStart otherEnd : rest)
      | target == other && otherStart <= end = merge (Span target start (max end otherEnd) : rest)
    merge (first : rest) = first : merge rest
    merge [] = []

intersectSpan :: Span -> Span -> Maybe Span
intersectSpan (Span target (Guest start) (Guest end)) (Span other (Guest otherStart) (Guest otherEnd))
  | target == other = span' target (max start otherStart) (min end otherEnd)
  | otherwise = Nothing

subtractSpans :: Span -> [Span] -> [Span]
subtractSpans whole holes = foldl cut [whole] holes
  where
    cut pieces (Span target (Guest low) (Guest high)) = concatMap (\piece@(Span own (Guest start) (Guest end)) ->
      if own /= target || high <= start || end <= low then [piece]
      else [Span own (Guest start) (Guest low) | start < low] ++ [Span own (Guest high) (Guest end) | high < end]) pieces

coveredBy :: Span -> [Span] -> Bool
coveredBy range covering = null (subtractSpans range covering)
