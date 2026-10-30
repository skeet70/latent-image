-- Stage an owned record-2 startup image into a reviewed X-M1 container.
module Main where

import Control.Monad (forM, forM_, unless, when)
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

retiredExtents :: [String]
retiredExtents =
  [ "entry"
  , "mpu_discriminator"
  , "entry_pool"
  , "handoff_pool"
  , "main"
  , "stub_a"
  , "stub_b"
  , "log"
  , "selection"
  ]

within :: (Int, Int) -> Int -> Bool
within (start, end) value = value >= start && value < end

declarations :: [Symbol] -> Either String StageDeclarations
declarations symbols = do
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
  retired <- forM retiredExtents $ \name -> do
    range <- retainedExtentRange <$> extent ("xm1_retired_" ++ name ++ "_start") ("xm1_retired_" ++ name ++ "_end")
    unless (fst range >= fst bodyRange && snd range <= snd bodyRange)
      (Left ("retired " ++ name ++ " is outside the retained body extent"))
    pure (name, range)
  services <- forM retainedServices $ \name -> do
    service <- offset name
    let address = slotOffsetBytes service
    unless (within bodyRange address) (Left (name ++ " is outside the retained body extent"))
    when (any (\(_, range) -> within range address) retired) (Left (name ++ " is inside a retired extent"))
    pure (name, address)
  publications <- forM ["xm1_publication_target", "xm1_publication_argument"] $ \name -> do
    cell <- offset name
    unless (slotOffsetBytes cell >= dataEnd) (Left (name ++ " is inside the retained record bytes"))
    pure (name, slotOffsetBytes cell)
  let guest value = U.showHexWord (fromIntegral (value + recordGuestBase SecondRecord))
      summary =
        ["retained service " ++ name ++ " " ++ guest address | (name, address) <- services]
          ++ ["retired " ++ name ++ " [" ++ guest start ++ "," ++ guest end ++ ")" | (name, (start, end)) <- retired]
          ++ ["publication " ++ name ++ " " ++ guest address | (name, address) <- publications]
  forM_ (zip retired (drop 1 retired)) $ \((name, (_, end)), (next, (start, _))) ->
    when (end > start) (Left ("retired " ++ name ++ " overlaps " ++ next))
  pure (StageDeclarations [before, after, body, retainedData] summary)

main :: IO ()
main =
  runStage
    StageContract
      { contractRecord = SecondRecord
      , contractTitle = "secondary stage"
      , contractProgram = "build-secondary-stage"
      , contractEntrySymbol = "xm1_secondary_entry"
      , contractCandidateSymbols =
          [ "xm1_secondary_main"
          , "xm1_secondary_rendezvous"
          , "xm1_secondary_peer_cell"
          , "xm1_secondary_publish"
          , "xm1_secondary_handoff"
          , "xm1_secondary_handoff_vector"
          ]
      , contractDeclarations = declarations
      }
