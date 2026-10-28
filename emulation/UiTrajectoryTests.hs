{-# LANGUAGE OverloadedStrings #-}
-- | Decoding of e2e trajectory files: the checked-in files and synthetic trace-site and segment cases.
module Main (main) where

import Control.Monad (unless)
import qualified Data.ByteString.Char8 as C
import Data.Aeson (Value (..), object, (.=))
import Data.List (isInfixOf, isSuffixOf, sort)
import Emulation.PreservationObservation
import Emulation.Symbols
import Emulation.UiTrajectory
import Emulation.WatchObservation
import EmulatorUsb (KeyPulse (..), KeyTiming (..), ModeDial (..), SettleBudget (..), defaultKeyTiming)
import Numeric (showHex)
import System.Directory (listDirectory)
import System.Exit (exitFailure)
import System.FilePath ((</>))

document :: String -> String -> C.ByteString
document sites requires = C.pack (concat
  [ "{\"schema\":1,\"trace_sites\":", sites
  , ",\"before_unload\":[{\"phase\":\"a\",\"steps\":[[\"menu\",1,5,2]]}"
  , ",{\"phase\":\"b\",\"requires\":", requires, ",\"steps\":[]}]"
  , ",\"after_unload\":[]}" ])

sitesOf :: C.ByteString -> Maybe [TraceSite]
sitesOf bytes = either (const Nothing) (Just . trajectoryTraceSites) (decodeTrajectories bytes)

refused :: C.ByteString -> Bool
refused = either (const True) (const False) . decodeTrajectories

-- | A one-segment file whose segment carries the given extra fields.
segmentDocument :: String -> C.ByteString
segmentDocument fields = C.pack (concat
  [ "{\"schema\":1,\"before_unload\":[{\"phase\":\"a\",\"steps\":[]", fields, "}],\"after_unload\":[]}" ])

segmentOf :: C.ByteString -> Maybe Segment
segmentOf bytes = case decodeTrajectories bytes of
  Right trajectories | [segment] <- beforeUnload trajectories -> Just segment
  _ -> Nothing

-- | The steps of a one-segment file whose segment carries STEPS.
stepsOf :: String -> Maybe [Step]
stepsOf steps = segmentSteps <$> segmentOf (C.pack (concat
  [ "{\"schema\":1,\"before_unload\":[{\"phase\":\"a\",\"steps\":[", steps, "]}],\"after_unload\":[]}" ]))

-- | variantMismatches over two one-segment files with the given steps and segment fields.
mismatchesOf :: (String, String) -> (String, String) -> Maybe [String]
mismatchesOf (oldSteps, oldFields) (newSteps, newFields) = do
  let file steps fields = C.pack (concat
        [ "{\"schema\":1,\"before_unload\":[{\"phase\":\"a\",\"steps\":[", steps, "]", fields, "}],\"after_unload\":[]}" ])
  old <- either (const Nothing) Just (decodeTrajectories (file oldSteps oldFields))
  new <- either (const Nothing) Just (decodeTrajectories (file newSteps newFields))
  pure (variantMismatches old new)

-- | A linked build's @llvm-nm --print-size@ rows, one symbol each.
symbolTable :: SymbolTable
symbolTable = parseSymbolTable (C.pack (unlines
  [ "00be8cc8 00000400 d rb_resident"
  , "00be9994 00000004 d rb_current_gen"
  , "00be99b0 00000008 d rb_activation" ]))

repeatedSymbols :: SymbolTable
repeatedSymbols = parseSymbolTable (C.pack (unlines
  [ "00be8cc8 00000400 d rb_resident"
  , "00be8d00 00000004 d rb_resident" ]))

expect :: String -> Bool -> IO Bool
expect name ok = do
  putStrLn ((if ok then "ok   " else "FAIL ") ++ name)
  pure ok

main :: IO ()
main = do
  files <- sort . filter ("-trajectories.json" `isSuffixOf`) <$> listDirectory "emulation"
  checkedIn <- mapM (\file -> do
      decoded <- decodeTrajectories <$> C.readFile ("emulation" </> file)
      expect ("checked-in " ++ file ++ " decodes") (either (const False) (const True) decoded))
    files
  variants <- sort . filter ("-trajectories.fujimod.json" `isSuffixOf`) <$> listDirectory "emulation"
  checkedInVariants <- mapM (\file -> do
      let retainedFile = take (length file - length (".fujimod.json" :: String)) file ++ ".json"
      retained <- decodeTrajectories <$> C.readFile ("emulation" </> retainedFile)
      variant <- decodeTrajectories <$> C.readFile ("emulation" </> file)
      expect ("checked-in " ++ file ++ " asserts what " ++ retainedFile ++ " asserts")
        (either (const False) null (variantMismatches <$> retained <*> variant)))
    variants
  results <- sequence
    [ expect "the checked-in trajectory files are present" (not (null files))
    , expect "a trace site decodes to its address and label" $
        sitesOf (document "[[\"0114b6c4\",\"film-icon\"]]" "[\"a\"]")
          == Just [TraceSite (FixedAddress 0x0114b6c4) "film-icon"]
    , expect "a file without trace sites declares none" $
        sitesOf (C.pack "{\"schema\":1,\"before_unload\":[],\"after_unload\":[]}") == Just []
    , expect "an unaligned trace site is refused" $
        refused (document "[[\"0114b6c6\",\"film-icon\"]]" "[]")
    , expect "a trace site address with 0x is refused" $
        refused (document "[[\"0x114b6c4\",\"film-icon\"]]" "[]")
    , expect "a reserved trace label is refused" $
        refused (document "[[\"0114b6c4\",\"menu-message\"]]" "[]")
    , expect "a repeated trace label is refused" $
        refused (document "[[\"0114b6c4\",\"x\"],[\"01146cf0\",\"x\"]]" "[]")
    , expect "a runtime trace site decodes to its symbol, offset and label" $
        sitesOf (document "[[\"runtime:rb_call_stock_probe+0x10\",\"svc-stock-return\"]]" "[]")
          == Just [TraceSite (RuntimeSymbol "rb_call_stock_probe" 0x10) "svc-stock-return"]
    , expect "a runtime trace site without an offset is refused when unaligned" $
        refused (document "[[\"runtime:rb_call_stock_probe+0x6\",\"svc-stock-return\"]]" "[]")
    , expect "a runtime trace site needs a symbol" $
        refused (document "[[\"runtime:+0x10\",\"svc-stock-return\"]]" "[]")
    , expect "the same resolved trace site twice is refused" $
        refused (document "[[\"runtime:rb_call_stock_probe+0x10\",\"a\"],[\"runtime:rb_call_stock_probe+0x10\",\"b\"]]" "[]")
    , expect "a fixed trace site shorter than eight digits is refused" $
        refused (document "[[\"114b6c4\",\"x\"]]" "[]")
    , expect "a repeated trace address is refused" $
        refused (document "[[\"0114b6c4\",\"x\"],[\"0114b6c4\",\"y\"]]" "[]")
    , expect "a requirement naming no segment is refused" $
        refused (document "[]" "[\"missing\"]")
    , expect "a mode-dial position decodes by its dial name" $
        (segmentModeDial =<< segmentOf (segmentDocument ",\"mode_dial\":\"AUTO\"")) == Just ModeDialAuto
    , expect "the three remaining mode-dial positions decode by their dial names" $
        map ((segmentModeDial =<<) . segmentOf . segmentDocument)
          [",\"mode_dial\":\"SR+\"", ",\"mode_dial\":\"SP\"", ",\"mode_dial\":\"Adv.\""]
          == [Just ModeDialSRAuto, Just ModeDialScene, Just ModeDialAdvanced]
    , expect "an unknown mode-dial position is refused" $
        refused (segmentDocument ",\"mode_dial\":\"T\"")
    , expect "film rows 0 and 8 decode to PROVIA and the second recipe with their stored codes" $
        map (fmap filmStoredCode . (segmentFilmRow =<<) . segmentOf . segmentDocument)
          [",\"film_row\":0", ",\"film_row\":8"] == [Just 0, Just 14]
    , expect "a film row past 8 is refused" $
        refused (segmentDocument ",\"film_row\":9")
    , expect "a key pulse decodes to its hold and release cap" $
        (segmentKeyPulse =<< segmentOf (segmentDocument ",\"key_pulse\":{\"hold_instructions\":8000000,\"release_instructions\":6000000}"))
          == Just (KeyPulse 8000000 6000000)
    , expect "a segment without a key pulse uses the bridge default" $
        fmap segmentKeyPulse (segmentOf (segmentDocument "")) == Just Nothing
    , expect "a zero key-pulse budget is refused" $
        refused (segmentDocument ",\"key_pulse\":{\"hold_instructions\":0,\"release_instructions\":8000000}")
    , expect "a step without timing leaves the bridge defaults" $
        fmap (map stepTiming) (stepsOf "[\"menu\",1,5,2]") == Just [defaultKeyTiming]
    , expect "a step's timing object decodes to its pulse and settle cap" $
        fmap (map stepTiming) (stepsOf "[\"menu\",67,56,0,{\"hold_instructions\":3,\"release_instructions\":4,\"settle_instructions\":5000000}]")
          == Just [KeyTiming (Just (KeyPulse 3 4)) (Just (SettleBudget 5000000))]
    , expect "a step hold without its release is refused" $
        stepsOf "[\"menu\",1,5,2,{\"hold_instructions\":3}]" == Nothing
    , expect "a zero settle cap is refused" $
        stepsOf "[\"menu\",1,5,2,{\"settle_instructions\":0}]" == Nothing
    , expect "an unknown step timing field is refused" $
        stepsOf "[\"menu\",1,5,2,{\"wait_instructions\":5}]" == Nothing
    , expect "a read address decodes to a fixed guest window" $
        fmap segmentReads (segmentOf (segmentDocument ",\"reads\":[[\"mode\",\"00956838\",4]]"))
          == Just [("mode", FixedAddress 0x00956838, 4 :: Int)]
    , expect "a read address decodes to a build symbol and its byte offset" $
        fmap segmentReads (segmentOf (segmentDocument ",\"reads\":[[\"base\",\"runtime:rb_resident+0x40\",40]]"))
          == Just [("base", RuntimeSymbol "rb_resident" 0x40, 40 :: Int)]
    , expect "a read address names a symbol without an offset" $
        fmap segmentReads (segmentOf (segmentDocument ",\"reads\":[[\"gen\",\"runtime:rb_current_gen\",4]]"))
          == Just [("gen", RuntimeSymbol "rb_current_gen" 0, 4 :: Int)]
    , expect "a read address with a non-hex offset is refused" $
        refused (segmentDocument ",\"reads\":[[\"base\",\"runtime:rb_resident+zz\",40]]")
    , expect "a symbol table resolves a name to its address" $
        resolveSymbol symbolTable "rb_resident" == Right 0x00be8cc8
    , expect "a read naming a symbol the loaded build lacks fails with that name" $
        (case resolveSymbol symbolTable "rb_base" of
           Left problem -> "rb_base" `isInfixOf` problem
           Right _ -> False)
    , expect "a symbol the loaded build repeats fails" $
        (case resolveSymbol repeatedSymbols "rb_resident" of
           Left _ -> True
           Right _ -> False)
    , expect "a variant that changes only step timing and key pulses asserts what its retained file asserts" $
        mismatchesOf ("[\"menu\",67,56,0],[\"menu\",1,5,2]", "")
          ("[\"menu\",67,56,0,{\"settle_instructions\":5000000}],[\"menu\",1,5,2]", ",\"key_pulse\":{\"hold_instructions\":8000000,\"release_instructions\":8000000}")
          == Just []
    , expect "a variant that changes an expected menu state is a mismatch" $
        fmap null (mismatchesOf ("[\"menu\",67,56,0],[\"menu\",1,5,2]", "") ("[\"menu\",1,5,2]", "")) == Just False
    , expect "a variant that changes a capture is a mismatch" $
        fmap null (mismatchesOf ("[\"menu\",1,5,2]", "") ("[\"menu\",1,5,2]", ",\"capture\":\"x\"")) == Just False
    , expect "a trajectory without protection declares none" $
        protectionOf (C.pack "{\"schema\":1,\"before_unload\":[],\"after_unload\":[]}") == Just Unprotected
    , expect "a clean protected watch decodes its typed range" $
        fmap (\t -> (trajectoryExpect t, trajectoryProtection t))
          (either (const Nothing) Just (decodeTrajectories (C.pack "{\"schema\":1,\"protected_writes\":[[\"0096cc64\",\"0096ce26\"]],\"before_unload\":[],\"after_unload\":[]}")))
          == Just (ExpectClean, WatchProtection [either (error "range") id (watchRange (watchAddress 0x0096cc64) (watchAddress 0x0096ce26))])
    , expect "a preservation declaration decodes to its typed ranges" $
        fmap (\t -> (trajectoryExpect t, trajectoryProtection t))
          (either (const Nothing) Just (decodeTrajectories
            (preservationFile "{\"kind\":\"ram-byte-preservation\",\"ranges\":[[\"00001000\",\"00001010\"]]}")))
          == Just (ExpectClean, PreservationProtection preservedRange)
    , expect "a refusal expectation decodes with a preservation declaration" $
        fmap trajectoryExpect (either (const Nothing) Just (decodeTrajectories
          (C.pack "{\"schema\":1,\"protected_memory\":{\"kind\":\"ram-byte-preservation\",\"ranges\":[[\"00001000\",\"00001010\"]]},\"expect\":\"refused\",\"before_unload\":[],\"after_unload\":[]}")))
          == Just ExpectRefused
    , expect "conflicting watch and preservation declarations are refused" $
        refused (C.pack "{\"schema\":1,\"protected_writes\":[[\"00001000\",\"00001010\"]],\"protected_memory\":{\"kind\":\"ram-byte-preservation\",\"ranges\":[[\"00001000\",\"00001010\"]]},\"before_unload\":[],\"after_unload\":[]}")
    , expect "an unknown protection kind is refused" $
        refused (preservationFile "{\"kind\":\"ram-byte-watch\",\"ranges\":[[\"00001000\",\"00001010\"]]}")
    , expect "a preservation declaration without ranges is refused" $
        refused (preservationFile "{\"kind\":\"ram-byte-preservation\",\"ranges\":[]}")
    , expect "a preservation declaration above eight ranges is refused" $
        refused (preservationFile ("{\"kind\":\"ram-byte-preservation\",\"ranges\":["
          ++ concat (replicate 9 "[\"00001000\",\"00001010\"],") ++ "[\"00001000\",\"00001010\"]]}"))
    , expect "a refused watch without ranges is refused" $
        refused (C.pack "{\"schema\":1,\"expect\":\"refused\",\"before_unload\":[],\"after_unload\":[]}")
    , expect "a reversed watch range is refused" $
        refused (C.pack "{\"schema\":1,\"protected_writes\":[[\"0096ce26\",\"0096cc64\"]],\"before_unload\":[],\"after_unload\":[]}")
    , expect "a watch without records fails closed" $
        isLeft (parseWatchObservation (limit4) (object ["enabled" .= True, "ranges" .= [[4096, 4112 :: Int]], "violation" .= Null]))
    , expect "a null watch fails closed" $
        isLeft (parseWatchObservation (limit4) Null)
    , expect "an unknown watch source fails closed" $
        isLeft (parseWatchObservation (limit4) (watchObservation (watchRecord "cpu-write-typo" 4096)))
    , expect "a record outside the expected range is refused" $
        isLeft (gateOf (watchObservation (watchRecord "cpu-write" 8192)))
    , expect "a single protected write is refused by the shared gate" $
        let parsed = either (error "parse") id (parseWatchObservation limit4 (watchObservation (watchRecord "cpu-write" 4096)))
         in isLeft (judgeWatch CandidateRun (gateOfParsed parsed)) && judgeWatch FaultControlRun (gateOfParsed parsed) == Right ControlRefusedProtectedWrite
    , expect "a control fails when the gate accepts a clean interval" $
        isLeft (judgeWatch FaultControlRun (Right WatchAcceptedClean))
    , expect "an accepted SD write fails the media check" $
        isLeft (checkMedia False (mediaObservation 0) (mediaObservation 1))
    , expect "a read overlapping the range end is allowed" $
        fmap (const ()) (judgeWatch CandidateRun (gateOf (object
            [ "enabled" .= True, "ranges" .= [[4096, 4112 :: Int]], "violation" .= Null
            , "records" .= [object [ "source" .= ("cpu-read" :: String), "address" .= (4110 :: Int), "physical" .= (4110 :: Int)
              , "size" .= (4 :: Int), "value" .= (0 :: Int), "before" .= ([0, 0, 0, 0] :: [Int]), "byte_order" .= ("little" :: String)
              , "core" .= (0 :: Int), "pc" .= (4660 :: Int), "instructions" .= (10 :: Int), "tick" .= (10 :: Int) ]]] ))) == Right ()
    , expect "a write overlapping the range end is refused" $
        isLeft (judgeWatch CandidateRun (gateOf (watchObservation (watchRecordSized "cpu-write" 4110 4 0 [0,0,0,0] "little" 10))))
    , expect "a host write is refused and is not a control cpu-write" $
        let parsed = either (error "parse") id (parseWatchObservation limit4 (watchObservation (transferRecord "host-write" 4096)))
         in isLeft (judgeWatch CandidateRun (gateOfParsed parsed)) && isLeft (judgeWatch FaultControlRun (gateOfParsed parsed))
    , expect "a host read is allowed" $
        fmap (const ()) (judgeWatch CandidateRun (gateOf (watchObservation (transferRecord "host-read" 4096)))) == Right ()
    , expect "a two-record write-restore pair is refused and counts two cpu writes" $
        let records = [watchRecordSized "cpu-write" 4096 4 1 [0,0,0,0] "little" 10
                      ,watchRecordSized "cpu-write" 4096 4 0 [1,0,0,0] "little" 11]
            parsed = either (error "parse") id (parseWatchObservation limit4 (object ["enabled" .= True, "ranges" .= [[4096, 4112 :: Int]], "violation" .= Null, "records" .= records]))
         in isLeft (judgeWatch CandidateRun (gateOfParsed parsed))
            && judgeWatch FaultControlRun (gateOfParsed parsed) == Right ControlRefusedProtectedWrite
    , expect "a regressing instruction count fails closed" $
        let records = [watchRecordSized "cpu-write" 4096 4 1 [0,0,0,0] "little" 11
                      ,watchRecordSized "cpu-write" 4096 4 0 [1,0,0,0] "little" 10]
         in isLeft (gateOf (object ["enabled" .= True, "ranges" .= [[4096, 4112 :: Int]], "violation" .= Null, "records" .= records]))
    , expect "a CPU before length differing from size fails" $
        isLeft (parseWatchObservation limit4 (watchObservation (watchRecordSized "cpu-write" 4096 4 0 [0,0,0] "little" 10)))
    , expect "a CPU access exceeding 32 bits fails" $
        isLeft (parseWatchObservation limit4 (watchObservation (watchRecordSized "cpu-write" 0xffffffff 4 0 [0,0,0,0] "little" 10)))
    , expect "a transfer access exceeding 32 bits fails" $
        isLeft (parseWatchObservation limit4 (watchObservation (transferRecord "host-read" 0xffffffff)))
    , expect "an unknown row field fails closed" $
        isLeft (parseWatchObservation limit4 (watchObservation (object
          [ "source" .= ("cpu-write" :: String), "address" .= (4096 :: Int), "physical" .= (4096 :: Int), "size" .= (4 :: Int)
          , "value" .= (0 :: Int), "before" .= ([0,0,0,0] :: [Int]), "byte_order" .= ("little" :: String), "core" .= (0 :: Int)
          , "pc" .= (4660 :: Int), "instructions" .= (10 :: Int), "tick" .= (10 :: Int), "truncated" .= True ])))
    , expect "a big-endian read value must agree with its before bytes" $
        isLeft (parseWatchObservation limit4 (object ["enabled" .= True, "ranges" .= [[4096, 4112 :: Int]], "violation" .= Null
          , "records" .= [object [ "source" .= ("cpu-read" :: String), "address" .= (4096 :: Int), "physical" .= (4096 :: Int)
            , "size" .= (2 :: Int), "value" .= (0x0102 :: Int), "before" .= ([2,1] :: [Int]), "byte_order" .= ("big" :: String)
            , "core" .= (0 :: Int), "pc" .= (4660 :: Int), "instructions" .= (10 :: Int), "tick" .= (10 :: Int) ]]] ))
    , expect "identity-only preservation writes qualify a candidate" $
        preservationJudgement CandidateRun (preservationState 7 0 0 0 Null) == Right CandidateQualified
    , expect "identity-only preservation writes cannot qualify a control" $
        isLeft (preservationJudgement FaultControlRun (preservationState 7 0 0 0 Null))
    , expect "a changed protected byte is refused by a candidate and qualifies the control" $
        let state = preservationState 0 1 0 0 (cpuChange 4096 4096 [0,0,0,0] [1,0,0,0])
         in isLeft (preservationJudgement CandidateRun state)
            && preservationJudgement FaultControlRun state == Right ControlRefusedProtectedWrite
    , expect "a changed protected byte at the range end qualifies the control" $
        preservationJudgement FaultControlRun (preservationState 0 1 0 0 (cpuChange 4110 4110 [0,0] [1,0])) == Right ControlRefusedProtectedWrite
    , expect "a changed store spanning adjacent protected ranges qualifies the control" $
        let ranges = rangesOf [(4096, 4100), (4100, 4104)]
            state = preservationStateWith True [[4096, 4100], [4100, 4104]] 0 1 0 0
              (cpuChange 4098 4098 [0,0,0,0] [1,0,0,0])
         in preservationJudgementFor FaultControlRun ranges state == Right ControlRefusedProtectedWrite
    , expect "a changed store spanning overlapping protected ranges qualifies the control" $
        let ranges = rangesOf [(4096, 4100), (4098, 4104)]
            state = preservationStateWith True [[4096, 4100], [4098, 4104]] 0 1 0 0
              (cpuChange 4097 4097 [0,0,0,0,0,0] [1,0,0,0,0,0])
         in preservationJudgementFor FaultControlRun ranges state == Right ControlRefusedProtectedWrite
    , expect "a changed store crossing a protected gap is refused" $
        let ranges = rangesOf [(4096, 4100), (4104, 4108)]
            state = preservationStateWith True [[4096, 4100], [4104, 4108]] 0 1 0 0
              (cpuChange 4098 4098 [0,0,0,0] [1,0,0,0])
         in isLeft (preservationGateFor ranges state)
    , expect "a cpu-change whose before and after bytes agree is refused" $
        isLeft (parsePreservationObservation (preservationState 0 1 0 0 (cpuChange 4096 4096 [1,0] [1,0])))
    , expect "a cpu-change whose hex length differs from its size is refused" $
        isLeft (parsePreservationObservation (preservationState 0 1 0 0 (object
          [ "kind" .= ("cpu-change" :: String), "core" .= (0 :: Int), "pc" .= (512 :: Int), "instructions" .= (10 :: Int)
          , "address" .= (4096 :: Int), "physical" .= (4096 :: Int), "size" .= (2 :: Int)
          , "before" .= ("00" :: String), "after" .= ("01" :: String) ])))
    , expect "a cpu-change outside the protected range is refused" $
        isLeft (preservationGate (preservationState 0 1 0 0 (cpuChange 4096 8192 [0] [1])))
    , expect "a cpu-change crossing the protected range end is refused" $
        isLeft (preservationGate (preservationState 0 1 0 0 (cpuChange 4110 4110 [0,0,0,0] [1,0,0,0])))
    , expect "a host write overlapping the range is refused and is not a control cpu-write" $
        let state = preservationState 0 0 1 0 (unverifiableWrite "host-write" 4110 4110 4)
         in isLeft (preservationJudgement CandidateRun state) && isLeft (preservationJudgement FaultControlRun state)
    , expect "an unverifiable write outside the protected range is refused" $
        isLeft (preservationGate (preservationState 0 0 1 0 (unverifiableWrite "host-write" 8192 8192 4)))
    , expect "an unverifiable model-bus write without its counter is inconsistent" $
        isLeft (preservationGate (preservationState 0 0 0 0 (unverifiableWrite "model-bus-write" 4100 4100 2)))
    , expect "a clean state carrying a changed-cpu counter is inconsistent" $
        isLeft (preservationGate (preservationState 0 1 0 0 Null))
    , expect "a cpu-change without a changed-cpu counter is inconsistent" $
        isLeft (preservationGate (preservationState 0 0 0 0 (cpuChange 4096 4096 [0] [1])))
    , expect "an integrity fault is refused" $
        isLeft (preservationGate (preservationState 3 1 0 0 (integrityFault "counter-overflow")))
    , expect "an unknown preservation violation is refused" $
        isLeft (parsePreservationObservation (preservationState 0 1 0 0 (object ["kind" .= ("cpu-write" :: String)])))
    , expect "an unknown preservation reason is refused" $
        isLeft (preservationGate (preservationState 0 1 0 0 (integrityFault "vendor-status")))
    , expect "an unknown preservation field is refused" $
        isLeft (parsePreservationObservation (object ["enabled" .= True, "ranges" .= [[4096, 4112 :: Int]]
          , "identity_cpu_writes" .= (0 :: Int), "changed_cpu_writes" .= (0 :: Int), "host_writes" .= (0 :: Int)
          , "model_bus_writes" .= (0 :: Int), "violation" .= Null, "dropped" .= True]))
    , expect "a preservation counter beyond u64 is refused" $
        isLeft (parsePreservationObservation (object ["enabled" .= True, "ranges" .= [[4096, 4112 :: Int]]
          , "identity_cpu_writes" .= (0 :: Integer), "changed_cpu_writes" .= (18446744073709551616 :: Integer)
          , "host_writes" .= (0 :: Integer), "model_bus_writes" .= (0 :: Integer), "violation" .= Null]))
    , expect "a preservation violation re-emits its exact before and after bytes" $
        let parsed = either (error "parse") id (parsePreservationObservation
              (preservationState 0 1 0 0 (cpuChange 4096 4096 [0x12,0x34] [0x12,0xab])))
            violation = maybe (error "violation") id (preservationFirstViolation parsed)
         in preservationViolationJson violation == object
              [ "kind" .= ("cpu-change" :: String), "core" .= (0 :: Int), "pc" .= (0x200 :: Int)
              , "instructions" .= (10 :: Int), "address" .= (4096 :: Int), "physical" .= (4096 :: Int)
              , "size" .= (2 :: Int), "before" .= ("1234" :: String), "after" .= ("12ab" :: String) ]
    , expect "a disabled preservation state is parsed and refused" $
        let disabled = preservationStateWith False [] 0 0 0 0 Null
            parsed = either (const False) (const True) (parsePreservationObservation disabled)
         in parsed && isLeft (preservationGate disabled)
    , expect "an enabled preservation state without ranges is refused" $
        isLeft (parsePreservationObservation (preservationStateWith True [] 0 0 0 0 Null))
    , expect "a disabled preservation state with ranges is refused" $
        isLeft (parsePreservationObservation (preservationStateWith False [[4096, 4112 :: Int]] 0 0 0 0 Null))
    , expect "a disabled preservation state with counters is refused" $
        isLeft (parsePreservationObservation (preservationStateWith False [] 0 1 0 0 Null))
    , expect "a preservation acknowledgement with different ranges is refused" $
        let ack = parsePreservationAck (object ["armed" .= True, "ranges" .= [[4096, 4113 :: Int]]])
            result = do
              accepted <- ack
              observation <- parsePreservationObservation (preservationState 0 0 0 0 Null)
              checkPreservation preservedRange accepted observation
         in isLeft result
    , expect "an unarmed preservation acknowledgement is refused" $
        let ack = parsePreservationAck (object ["armed" .= False, "ranges" .= [[4096, 4112 :: Int]]])
            result = do
              accepted <- ack
              observation <- parsePreservationObservation (preservationState 0 0 0 0 Null)
              checkPreservation preservedRange accepted observation
         in isLeft result
    ]
  unless (and (checkedIn ++ checkedInVariants ++ results)) exitFailure

rangesOf :: [(Int, Int)] -> PreservationRanges
rangesOf bounds = either (error "preservation ranges") id
  (preservationRanges
    [ either (error "watch range") id (watchRange (watchAddress (fromIntegral low)) (watchAddress (fromIntegral high)))
    | (low, high) <- bounds ])

preservedRange :: PreservationRanges
preservedRange = rangesOf [(0x1000, 0x1010)]

preservationFile :: String -> C.ByteString
preservationFile body = C.pack ("{\"schema\":1,\"protected_memory\":" ++ body ++ ",\"before_unload\":[],\"after_unload\":[]}")

protectionOf :: C.ByteString -> Maybe TrajectoryProtection
protectionOf bytes = either (const Nothing) (Just . trajectoryProtection) (decodeTrajectories bytes)

preservationStateWith :: Bool -> [[Int]] -> Integer -> Integer -> Integer -> Integer -> Value -> Value
preservationStateWith enabled ranges identityCpu changedCpu hostWrites modelBusWrites violation = object
  [ "enabled" .= enabled
  , "ranges" .= ranges
  , "identity_cpu_writes" .= identityCpu
  , "changed_cpu_writes" .= changedCpu
  , "host_writes" .= hostWrites
  , "model_bus_writes" .= modelBusWrites
  , "violation" .= violation ]

preservationState :: Integer -> Integer -> Integer -> Integer -> Value -> Value
preservationState = preservationStateWith True [[4096, 4112]]

preservationGateFor :: PreservationRanges -> Value -> Either String PreservationGate
preservationGateFor ranges value = do
  ack <- parsePreservationAck (object ["armed" .= True, "ranges" .= preservationRangesJson ranges])
  observation <- parsePreservationObservation value
  checkPreservation ranges ack observation

preservationGate :: Value -> Either String PreservationGate
preservationGate = preservationGateFor preservedRange

preservationJudgementFor :: RunPurpose -> PreservationRanges -> Value -> Either String WatchJudgement
preservationJudgementFor purpose ranges value = judgePreservation purpose (preservationGateFor ranges value)

preservationJudgement :: RunPurpose -> Value -> Either String WatchJudgement
preservationJudgement purpose = preservationJudgementFor purpose preservedRange

cpuChange :: Int -> Int -> [Int] -> [Int] -> Value
cpuChange address physical before after = object
  [ "kind" .= ("cpu-change" :: String)
  , "core" .= (0 :: Int)
  , "pc" .= (0x200 :: Int)
  , "instructions" .= (10 :: Int)
  , "address" .= address
  , "physical" .= physical
  , "size" .= length before
  , "before" .= hexText before
  , "after" .= hexText after ]

unverifiableWrite :: String -> Int -> Int -> Int -> Value
unverifiableWrite source address physical size = object
  [ "kind" .= ("unverifiable-write" :: String)
  , "source" .= source
  , "address" .= address
  , "physical" .= physical
  , "size" .= size ]

integrityFault :: String -> Value
integrityFault reason = object ["kind" .= ("integrity-fault" :: String), "reason" .= reason]

hexText :: [Int] -> String
hexText = concatMap byte
  where
    byte value =
      let digits = showHex value ""
       in if length digits == 1 then '0' : digits else digits

limit4 :: WatchLimit
limit4 = either (error "limit") id (watchLimit 4)

guardAck :: WatchAck
guardAck = either (error "ack") id (parseWatchAck (object ["armed" .= True, "ranges" .= [[4096, 4112 :: Int]]]))

expectedRange :: WatchRange
expectedRange = either (error "range") id (watchRange (watchAddress 4096) (watchAddress 4112))

watchRecord :: String -> Int -> Value
watchRecord source address = watchRecordSized source address 4 0 [0, 0, 0, 0] "little" 10

watchRecordSized :: String -> Int -> Int -> Int -> [Int] -> String -> Int -> Value
watchRecordSized source address size value before byteOrder instructions = object
  [ "source" .= source, "address" .= address, "physical" .= address, "size" .= size
  , "value" .= value, "before" .= before, "byte_order" .= byteOrder, "core" .= (0 :: Int)
  , "pc" .= (4660 :: Int), "instructions" .= instructions, "tick" .= instructions ]

transferRecord :: String -> Int -> Value
transferRecord source address = object
  [ "source" .= source, "address" .= address, "physical" .= address, "size" .= (64 :: Int)
  , "core" .= Null, "pc" .= Null, "instructions" .= Null, "tick" .= Null ]

watchObservation :: Value -> Value
watchObservation record = object
  [ "enabled" .= True, "ranges" .= [[4096, 4112 :: Int]], "records" .= [record], "violation" .= Null ]

mediaObservation :: Int -> MediaObservation
mediaObservation writes = either (error "media") id (parseMediaObservation (object
  [ "present" .= False, "write_protected" .= False, "dirty_sectors" .= (0 :: Int)
  , "started" .= (0 :: Int), "completed" .= (0 :: Int), "irqs" .= (0 :: Int)
  , "dma_boundaries" .= (0 :: Int), "accepted_writes" .= writes
  , "accepted_write_bytes" .= (writes * 512) ]))

gateOf :: Value -> Either String WatchGate
gateOf value = either Left (\observation -> checkWatch limit4 [expectedRange] (guardAck) observation) (parseWatchObservation limit4 value)

gateOfParsed :: WatchObservation -> Either String WatchGate
gateOfParsed observation = checkWatch limit4 [expectedRange] (guardAck) observation

isLeft :: Either a b -> Bool
isLeft (Left _) = True
isLeft (Right _) = False
