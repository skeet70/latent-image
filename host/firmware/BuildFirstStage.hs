-- Stage an owned first-record image into a reviewed X-M1 container.
module Main where

import Control.Monad (forM_)
import FujiAF.FirstStage
import StageBuild

retainedContract :: [Symbol] -> Either String StageDeclarations
retainedContract symbols = do
  let offset = symbolSlotOffset FirstRecord symbols
  vectorStart <- offset "xm1_vector_table_start"
  beforeReset <- offset "xm1_vector_before_reset_end"
  afterReset <- offset "xm1_vector_after_reset_start"
  vectorEnd <- offset "xm1_vector_table_end"
  bodyStart <- offset "xm1_retained_body_start"
  bodyEnd <- offset "xm1_retained_body_end"
  dataStart <- offset "xm1_retained_data_start"
  dataEnd <- offset "xm1_retained_data_end"
  beforeExtent <- retainedExtent vectorStart beforeReset
  afterExtent <- retainedExtent afterReset vectorEnd
  bodyExtent <- retainedExtent bodyStart bodyEnd
  dataExtent <- retainedExtent dataStart dataEnd
  let bodyRange = (slotOffsetBytes bodyStart, slotOffsetBytes bodyEnd)
  forM_
    [ "xm1_retained_core_init"
    , "xm1_retained_storage_init"
    , "xm1_retained_skip_scan"
    , "xm1_retained_segment_table"
    , "xm1_retained_trailer_check"
    , "xm1_retained_stage_images"
    , "xm1_retained_storage_fini"
    ]
    $ \name -> do
      symbol <- lookupSymbol symbols name
      let address = fromIntegral (symbolAddress symbol)
      if address < fst bodyRange || address >= snd bodyRange
        then Left (name ++ " is outside the retained body extent")
        else pure ()
  pure (StageDeclarations [beforeExtent, afterExtent, bodyExtent, dataExtent] [])

main :: IO ()
main =
  runStage
    StageContract
      { contractRecord = FirstRecord
      , contractTitle = "first stage"
      , contractProgram = "build-first-stage"
      , contractEntrySymbol = "xm1_first_stage_entry"
      , contractCandidateSymbols = ["xm1_first_stage_handoff", "xm1_first_stage_handoff_vector"]
      , contractDeclarations = retainedContract
      }
