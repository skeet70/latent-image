-- Retained extents, services, forbidden ranges and publication cells an owned
-- record-2 image declares through its linker symbols.
module SecondaryStageContract
  ( secondaryDeclarations
  , within
  ) where

import Control.Monad (forM, forM_, unless, when)
import Data.List (sortOn)
import FujiAF.FirstStage
import qualified FujiAF.UpdateContainer as U
import StageBuild

retainedServices :: [String]
retainedServices =
  [ "xm1_retained_execution_state"
  , "xm1_retained_tcm_stacks"
  , "xm1_retained_peripheral_init"
  , "xm1_retained_delay"
  , "xm1_retained_companion_transfer"
  , "xm1_retained_peripheral_final"
  , "xm1_retained_conversion"
  , "xm1_retained_config_record"
  , "xm1_retained_segment_lookup"
  , "xm1_retained_companion_flags"
  , "xm1_retained_structure_init"
  ]

data Forbidden = ForbidFetch | ForbidRead

forbiddenName :: Forbidden -> String
forbiddenName ForbidFetch = "fetch"
forbiddenName ForbidRead = "read"

forbiddenExtents :: [(Forbidden, String)]
forbiddenExtents =
  [ (ForbidFetch, "entry")
  , (ForbidFetch, "mpu_discriminator")
  , (ForbidRead, "entry_pool")
  , (ForbidRead, "handoff_pool")
  , (ForbidFetch, "main")
  , (ForbidRead, "main_pool")
  , (ForbidFetch, "stub_a")
  , (ForbidFetch, "stub_b")
  , (ForbidFetch, "log")
  , (ForbidFetch, "selection")
  ]

within :: (Int, Int) -> Int -> Bool
within (start, end) value = value >= start && value < end

secondaryDeclarations :: [Symbol] -> Either String StageDeclarations
secondaryDeclarations symbols = do
  let offset = symbolSlotOffset SecondRecord symbols
      extent start end = do
        first <- offset start
        final <- offset end
        retainedExtent first final
  before <- extent "xm1_vector_table_start" "xm1_vector_before_reset_end"
  after <- extent "xm1_vector_after_reset_start" "xm1_vector_table_end"
  body <- extent "xm1_retained_body_start" "xm1_retained_body_end"
  retainedData <- extent "xm1_retained_data_start" "xm1_retained_data_end"
  let bodyRange = retainedExtentRange body
      (_, dataEnd) = retainedExtentRange retainedData
  forbidden <- forM forbiddenExtents $ \(kind, name) -> do
    let prefix = "xm1_forbid_" ++ forbiddenName kind ++ "_" ++ name
    range <- retainedExtentRange <$> extent (prefix ++ "_start") (prefix ++ "_end")
    unless (fst range >= fst bodyRange && snd range <= snd bodyRange)
      (Left (prefix ++ " is outside the retained body extent"))
    pure (kind, name, range)
  services <- forM retainedServices $ \name -> do
    service <- offset name
    let address = slotOffsetBytes service
    unless (within bodyRange address) (Left (name ++ " is outside the retained body extent"))
    when (any (\(_, _, range) -> within range address) forbidden) (Left (name ++ " is inside a forbidden extent"))
    pure (name, address)
  publications <- forM ["xm1_publication_target", "xm1_publication_argument"] $ \name -> do
    cell <- offset name
    unless (slotOffsetBytes cell >= dataEnd) (Left (name ++ " is inside the retained record bytes"))
    pure (name, slotOffsetBytes cell)
  let guest value = U.showHexWord (fromIntegral (value + recordGuestBase SecondRecord))
      ordered = sortOn (\(_, _, range) -> range) forbidden
      summary =
        ["retained service " ++ name ++ " " ++ guest address | (name, address) <- services]
          ++ [ "candidate forbidden " ++ forbiddenName kind ++ " " ++ name ++ " [" ++ guest start ++ "," ++ guest end ++ ")"
             | (kind, name, (start, end)) <- forbidden
             ]
          ++ ["publication " ++ name ++ " " ++ guest address | (name, address) <- publications]
  forM_ (zip ordered (drop 1 ordered)) $ \((_, name, (_, end)), (_, next, (start, _))) ->
    when (end > start) (Left ("forbidden " ++ name ++ " overlaps " ++ next))
  pure (StageDeclarations [before, after, body, retainedData] [] summary)
