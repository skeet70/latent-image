{-# LANGUAGE OverloadedStrings #-}
-- | Decoding of e2e trajectory files: the checked-in files and synthetic trace-site and segment cases.
module Main (main) where

import Control.Monad (forM_, unless)
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.Aeson (Value (..), eitherDecode, encode, object, toJSON, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.List (intercalate, isInfixOf, isPrefixOf, isSuffixOf, sort)
import Emulation.PreservationObservation
import Emulation.Symbols
import Emulation.UiTrajectory
import Emulation.WatchObservation
import EmulatorUsb (CrawlKey (..), KeyPulse (..), KeyTiming (..), ModeDial (..), SettleBudget (..), defaultKeyTiming)
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

-- | A file whose segments carry the given steps, before and after unload.
sidesTrajectory :: ([String], [String]) -> Maybe Trajectories
sidesTrajectory (before, after) = either (const Nothing) Just (decodeTrajectories (C.pack (concat
  [ "{\"schema\":1,\"before_unload\":[", segments "b" before, "],\"after_unload\":[", segments "a" after, "]}" ])))
  where
    segments side entries = intercalate "," [ "{\"phase\":\"" ++ side ++ show index ++ "\",\"steps\":[" ++ steps ++ "]}" | (index, steps) <- zip [0 :: Int ..] entries ]

-- | variantMismatches over two 'sidesTrajectory' files.
sidesMismatches :: ([String], [String]) -> ([String], [String]) -> Maybe [String]
sidesMismatches old new = variantMismatches <$> sidesTrajectory old <*> sidesTrajectory new

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
  afReceiptControls
  let afSegment phase key action = object
        ["phase" .= (phase :: String), "steps" .= [[String key, Number 1, Number 246, Number 1]], "af_ownership" .= (action :: String)]
      down = afSegment "original-down" "down" "arm"
      left = afSegment "original-left" "left" "end"
      afRoute :: Bool -> [Value] -> [Value] -> Either String ()
      afRoute enabled before after = do
        trajectories <- decodeTrajectories (BL.toStrict (encode (object ["schema" .= (1 :: Int), "before_unload" .= before, "after_unload" .= after])))
        checkAfOwnershipRoute enabled (beforeUnload trajectories) (afterUnload trajectories)
      identity = object ["sdk" .= String "/nix/store/exact-sdk", "viewer" .= String "viewer-hash", "bridge" .= String "bridge-hash", "model" .= String "model-hash", "package" .= String "package-hash"]
      site = object ["pc" .= (4096 :: Int), "word" .= (0xe1a00000 :: Integer), "kind" .= String "consumer", "reads" .= [[8192 :: Int, 4]]]
      afPlan :: Value -> [Value] -> Either String AfOwnershipPlan
      afPlan bound sites = decodeAfOwnershipPlan (BL.toStrict (encode (object ["identity" .= bound, "sites" .= sites])))
      rejected = either (const True) (const False)
  unless (afRoute True [down, left] [] == Right ()) (ioError (userError "AF original DOWN/LEFT route must admit"))
  unless (all rejected
    [afRoute False [down, left] [], afRoute True [left, down] [], afRoute True [down] [], afRoute True [down] [left],
     afRoute True [afSegment "wrong-key" "right" "arm", left] [], afRoute True [down, down, left] []])
    (ioError (userError "AF route refusal controls"))
  unless (either (const False) ((== identity) . afOwnershipIdentity) (afPlan identity [site]))
    (ioError (userError "AF plan identity round trip"))
  unless (rejected (afPlan (object ["sdk" .= String "only-sdk"]) [site]) && rejected (afPlan identity []) && rejected (afPlan identity [site, site])
    && refused (segmentDocument ",\"af_ownership\":\"report\""))
    (ioError (userError "AF plan and action refusal controls"))
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
  pickers <- map ("emulation/modal75-picker-trajectories" </>) . sort . filter (".json" `isSuffixOf`)
    <$> listDirectory "emulation/modal75-picker-trajectories"
  embassy <- map (\directory -> "experiments" </> directory </> "trajectory.json") . sort . filter ("embassy_phase1" `isPrefixOf`)
    <$> listDirectory "experiments"
  -- The modal75 routes and pickers, the embassy experiments and the variants run only on FujiMod.
  unpinned <- mapM (\path -> do
      decoded <- decodeTrajectories <$> C.readFile path
      expect ("checked-in FujiMod " ++ path ++ " pins no instruction hold") (either (const False) (null . instructionPinnedPhases) decoded))
    (map ("emulation" </>) (variants ++ filter ("modal75-" `isPrefixOf`) files) ++ pickers ++ embassy)
  expectedFiles <- expect "the picker and embassy trajectory files are present" (length pickers == 21 && length embassy == 4)
  results <- sequence
    [ expect "the checked-in trajectory files are present" (not (null files))
    , expect "a trace site decodes to its address and label" $
        sitesOf (document "[[\"0114b6c4\",\"film-icon\"]]" "[\"a\"]")
          == Just [TraceSite (FixedAddress 0x0114b6c4) "film-icon" [] OmitsLr OmitsSp]
    , expect "a file without trace sites declares none" $
        sitesOf (C.pack "{\"schema\":1,\"before_unload\":[],\"after_unload\":[]}") == Just []
    , expect "a retained save lifecycle action retains its source paths" $
        (segmentSaveLifecycle =<< segmentOf (segmentDocument ",\"save_lifecycle\":{\"image5\":\"image5.bin\",\"image6\":\"image6.bin\",\"program\":\"retained\"}"))
          == Just (LifecycleAction "image5.bin" "image6.bin" RetainedLifecycle)
    , expect "an owned save lifecycle action selects the owned program" $
        (segmentSaveLifecycle =<< segmentOf (segmentDocument ",\"save_lifecycle\":{\"image5\":\"image5.bin\",\"image6\":\"image6.bin\",\"program\":\"owned\"}"))
          == Just (LifecycleAction "image5.bin" "image6.bin" OwnedLifecycle)
    , expect "an unknown save lifecycle program is refused" $
        refused (segmentDocument ",\"save_lifecycle\":{\"image5\":\"image5.bin\",\"image6\":\"image6.bin\",\"program\":\"both\"}")
    , expect "a save lifecycle action without image6 is refused" $
        refused (segmentDocument ",\"save_lifecycle\":{\"image5\":\"image5.bin\",\"program\":\"owned\"}")
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
          == Just [TraceSite (RuntimeSymbol "rb_call_stock_probe" 0x10) "svc-stock-return" [] OmitsLr OmitsSp]
    , expect "a trace site decodes its entry reads, keeps lr and asks for sp" $
        sitesOf (document "[[\"00d4dd2c\",\"stage-entry\",{\"reads\":[[\"009571d0\",1072],[\"01467580\",8]],\"sp\":true}]]" "[]")
          == Just [TraceSite (FixedAddress 0x00d4dd2c) "stage-entry" [SiteRead 0x009571d0 1072, SiteRead 0x01467580 8] RecordsLr RecordsSp]
    , expect "stack-relative site reads accept signed offsets" $
        sitesOf (document "[[\"00d4dd2c\",\"stage-entry\",{\"reads\":[{\"sp\":-16,\"bytes\":4},{\"sp\":8,\"bytes\":8}]}]]" "[]")
          == Just [TraceSite (FixedAddress 0x00d4dd2c) "stage-entry" [StackRead (-16) 4, StackRead 8 8] RecordsLr OmitsSp]
    , expect "site reads round-trip both forms" $
        let readSpecs = [SiteRead 0x009571d0 4, StackRead (-16) 8, StackRead 0 4096]
        in eitherDecode (encode readSpecs) == Right readSpecs
    , expect "malformed stack-relative reads are refused" $
        all (\read_ -> refused (document ("[[\"00d4dd2c\",\"stage-entry\",{\"reads\":[" ++ read_ ++ "]}]]") "[]"))
          ["{\"sp\":0,\"bytes\":0}", "{\"sp\":0,\"bytes\":4097}", "{\"sp\":0}", "{\"bytes\":4}",
           "{\"sp\":2147483648,\"bytes\":4}", "{\"sp\":0,\"bytes\":4,\"address\":\"00000000\"}"]
    , expect "a trace site may ask for lr alone" $
        sitesOf (document "[[\"00bfa234\",\"kind-switch\",{\"lr\":true}]]" "[]")
          == Just [TraceSite (FixedAddress 0x00bfa234) "kind-switch" [] RecordsLr OmitsSp]
    , expect "an unknown trace site option is refused" $
        refused (document "[[\"00bfa234\",\"kind-switch\",{\"r12\":true}]]" "[]")
    , expect "an empty trace site read is refused" $
        refused (document "[[\"00d4dd2c\",\"stage-entry\",{\"reads\":[[\"009571d0\",0]]}]]" "[]")
    , expect "a trace site read longer than 4096 bytes is refused" $
        refused (document "[[\"00d4dd2c\",\"stage-entry\",{\"reads\":[[\"009571d0\",4097]]}]]" "[]")
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
    , expect "a segment key pulse pins its phase to an instruction hold" $
        fmap instructionPinnedPhases (either (const Nothing) Just (decodeTrajectories
          (segmentDocument ",\"key_pulse\":{\"hold_instructions\":8000000,\"release_instructions\":8000000}"))) == Just ["a"]
    , expect "a step hold pins its phase and a settle cap alone does not" $
        map (fmap instructionPinnedPhases . either (const Nothing) Just . decodeTrajectories . C.pack)
          [ "{\"schema\":1,\"before_unload\":[{\"phase\":\"a\",\"steps\":[[\"ok\",1,5,2,{\"hold_instructions\":3,\"release_instructions\":4}]]}],\"after_unload\":[]}"
          , "{\"schema\":1,\"before_unload\":[{\"phase\":\"a\",\"steps\":[[\"ok\",1,5,2,{\"settle_instructions\":5000000}]]}],\"after_unload\":[]}" ]
          == [Just ["a"], Just []]
    , expect "a step without timing leaves the bridge defaults" $
        fmap (map stepTiming) (stepsOf "[\"menu\",1,5,2]") == Just [defaultKeyTiming]
    , expect "ordinary steps keep packet completion" $
        fmap (map stepCompletion) (stepsOf "[\"back\",63,5,0]") == Just [PacketCompletion]
    , expect "presented-frame parses a closed physical key" $
        fmap (map stepCompletion) (stepsOf "[\"back\",63,5,0,{\"completion\":\"presented-frame\"}]") == Just [PresentedFrameCompletion KeyBack]
    , expect "unknown completion and synthetic seek keys are refused" $
        stepsOf "[\"back\",63,5,0,{\"completion\":\"eventually\"}]" == Nothing
          && stepsOf "[\"seek:back:5:2\",63,5,0,{\"completion\":\"presented-frame\"}]" == Nothing
    , expect "presented-frame does not accept instruction timing" $
        stepsOf "[\"back\",63,5,0,{\"completion\":\"presented-frame\",\"settle_instructions\":5}]" == Nothing
    , expect "completion is an assertion, not a timing variant" $
        case (sidesTrajectory (["[\"back\",63,5,0]"], []), sidesTrajectory (["[\"back\",63,5,0,{\"completion\":\"presented-frame\"}]"], [])) of
          (Just retained, Just changed) -> not (null (variantMismatches retained changed))
          _ -> False
    , expect "presented-frame accepts only an acknowledged physical key and completed display wait" $
        let reply status failure = object
              ["failure" .= failure, "settle" .= object ["status" .= (status :: String)],
               "display_wait_instructions" .= (0 :: Int),
               "menu" .= object ["menu_id" .= (63 :: Int), "setting_id" .= (5 :: Int), "cursor" .= (0 :: Int)]]
        in presentedFrameMenu (reply "packet-consumed" Null) == Right (MenuState 63 5 0)
          && either (const True) (const False) (presentedFrameMenu (reply "prior-render-pending" Null))
          && either (const True) (const False) (presentedFrameMenu (reply "budget-exhausted" Null))
          && either (isInfixOf "no display plane") (const False) (presentedFrameMenu (reply "packet-consumed" (String "no display plane")))
    , expect "a step's timing object decodes to its pulse and settle cap" $
        fmap (map stepTiming) (stepsOf "[\"menu\",67,56,0,{\"hold_instructions\":3,\"release_instructions\":4,\"settle_instructions\":5000000}]")
          == Just [KeyTiming (Just (KeyPulse 3 4)) (Just (SettleBudget 5000000))]
    , expect "a step hold without its release is refused" $
        stepsOf "[\"menu\",1,5,2,{\"hold_instructions\":3}]" == Nothing
    , expect "a zero settle cap is refused" $
        stepsOf "[\"menu\",1,5,2,{\"settle_instructions\":0}]" == Nothing
    , expect "an unknown step timing field is refused" $
        stepsOf "[\"menu\",1,5,2,{\"wait_instructions\":5}]" == Nothing
    , expect "a step's idle decodes to the instructions run before its press" $
        fmap (map stepIdle) (stepsOf "[\"up\",1,246,0,{\"idle_instructions\":20000000}],[\"up\",1,246,0]")
          == Just [Just (Instructions 20000000), Nothing]
    , expect "a zero or over-cap idle is refused" $
        stepsOf "[\"up\",1,246,0,{\"idle_instructions\":0}]" == Nothing
          && stepsOf "[\"up\",1,246,0,{\"idle_instructions\":90000001}]" == Nothing
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
        fmap null (mismatchesOf ("[\"menu\",67,56,0],[\"menu\",1,5,2]", "") ("[\"menu\",67,56,0],[\"menu\",1,5,3]", "")) == Just False
    , expect "a variant without the MENU that leaves the retained overlay after install asserts what its retained file asserts" $
        sidesMismatches (["", "[\"menu\",67,56,0],[\"menu\",1,5,2]"], []) (["", "[\"menu\",1,5,2]"], []) == Just []
    , expect "a variant without the MENU that leaves the retained overlay after unload asserts what its retained file asserts" $
        sidesMismatches (["[\"back\",1,5,6]"], ["[\"menu\",1,5,6],[\"menu\",1,5,6]", "[\"ok\",1,254,0]"])
          (["[\"back\",1,5,6]"], ["[\"menu\",1,5,6]", "[\"ok\",1,254,0]"]) == Just []
    , expect "a variant without a later MENU is a mismatch" $
        fmap null (sidesMismatches (["[\"menu\",67,56,0],[\"menu\",1,5,2],[\"menu\",1,5,2]"], [])
          (["[\"menu\",67,56,0],[\"menu\",1,5,2]"], [])) == Just False
    , expect "a variant without a first key other than MENU is a mismatch" $
        fmap null (sidesMismatches (["[\"q\",122,5,1],[\"back\",122,5,1]"], []) (["[\"back\",122,5,1]"], [])) == Just False
    , expect "a variant that empties a segment by leaving out its only MENU is a mismatch" $
        fmap null (sidesMismatches (["[\"menu\",67,56,0]", "[\"menu\",1,5,2]"], []) (["", "[\"menu\",1,5,2]"], [])) == Just False
    , expect "a variant leaves out at most one MENU on each side of unload" $
        fmap null (sidesMismatches (["[\"menu\",67,56,0],[\"menu\",67,56,0],[\"menu\",1,5,2]"], []) (["[\"menu\",1,5,2]"], [])) == Just False
    , expect "the overlay exit is the first pressed MENU, in the first segment with steps" $
        fmap (fmap (\(index, step) -> (index, stepKey step, stepExpected step)) . overlayExit . beforeUnload)
          (sidesTrajectory (["", "[\"menu\",67,56,0],[\"menu\",1,5,2]", "[\"menu\",1,5,3]"], []))
          == Just (Just (1, "menu", MenuState 67 56 0))
    , expect "a first key other than MENU is no overlay exit" $
        fmap (overlayExit . beforeUnload) (sidesTrajectory (["[\"q\",122,5,1],[\"menu\",1,5,2]"], [])) == Just Nothing
    , expect "a MENU that is its segment's only step is no overlay exit" $
        fmap (overlayExit . afterUnload) (sidesTrajectory ([], ["[\"menu\",1,5,6]", "[\"ok\",1,254,0]"])) == Just Nothing
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
    , expect "forbidden execution decodes independently of write protection" $
        fmap trajectoryForbiddenExecution (either (const Nothing) Just (decodeTrajectories
          (executionFile "\"forbidden_execution\":[[\"00001000\",\"00001010\"]]"))) == Just [expectedRange]
    , expect "an execution control requires its exact PC" $
        refused (executionFile "\"forbidden_execution\":[[\"00001000\",\"00001010\"]],\"expect\":\"refused\"")
    , expect "an execution control PC must be inside a forbidden body" $
        refused (executionFile "\"forbidden_execution\":[[\"00001000\",\"00001010\"]],\"expect\":\"refused\",\"forbidden_execution_control_pc\":\"00002000\"")
    , expect "a clean trajectory cannot extend execution exclusion past unload" $
        refused (executionFile "\"forbidden_execution\":[[\"00001000\",\"00001010\"]],\"forbidden_execution_interval\":\"through-after-unload\"")
    , expect "a fetch refusal declares the through-after-unload interval and entry PC" $
        fmap (\t -> (trajectoryExecutionInterval t, trajectoryExecutionControlPc t))
          (either (const Nothing) Just (decodeTrajectories (executionFile
            "\"forbidden_execution\":[[\"00001000\",\"00001010\"]],\"expect\":\"refused\",\"forbidden_execution_interval\":\"through-after-unload\",\"forbidden_execution_control_pc\":\"00001000\"")))
          == Just (ThroughAfterUnload, Just (watchAddress 4096))
    , expect "a fixed complete observer with no records qualifies" $
        judgeExecution CandidateRun Nothing (executionGate fixedAck (fixedObservation [])) == Right ()
    , expect "a forbidden fetch is rejected at its actual PC" $
        let gate = executionGate fixedAck (fixedObservation [fetchRecord 0 4096 4096 4096 4])
         in isLeft (judgeExecution CandidateRun Nothing gate)
            && judgeExecution FaultControlRun (Just (watchAddress 4096)) gate == Right ()
            && isLeft (judgeExecution FaultControlRun (Just (watchAddress 4100)) gate)
    , expect "both cores' physical aliases are excluded" $
        and [isLeft (judgeExecution CandidateRun Nothing
              (executionGate fixedAck (fixedObservation [fetchRecord core 0x40001000 4096 0x40001000 4]))) | core <- [0, 1]]
    , expect "a fetch partially overlapping the body end is excluded" $
        isLeft (judgeExecution CandidateRun Nothing
          (executionGate fixedAck (fixedObservation [fetchRecord 1 4110 4110 4110 4])))
    , expect "a fetch starting at the half-open end is not accepted as observer evidence" $
        isLeft (executionGate fixedAck (fixedObservation [fetchRecord 0 4112 4112 4112 4]))
    , expect "reads cannot satisfy the execution refusal control" $
        isLeft (judgeExecution FaultControlRun (Just (watchAddress 4096))
          (executionGate fixedAck (fixedObservation [watchRecord "cpu-read" 4096])))
    , expect "a clean interval cannot satisfy the execution refusal control" $
        isLeft (judgeExecution FaultControlRun (Just (watchAddress 4096)) (executionGate fixedAck (fixedObservation [])))
    , expect "disabled, overflowed, decoder-violated and unclassified observers fail closed" $
        and [isLeft (executionGate ack state) | (ack, state) <-
          [(replaceField "armed" (Bool False) fixedAck, fixedObservation [])
          ,(fixedAck, replaceField "enabled" (Bool False) (fixedObservation []))
          ,(fixedAck, replaceField "violation" (String "record overflow") (fixedObservation []))
          ,(fixedAck, replaceField "violation" (String "Memory decoding changed") (fixedObservation []))
          ,(fixedAck, replaceField "views" (String "observe") (fixedObservation []))
          ,(fixedAck, replaceField "classes" (toJSON ([] :: [Value])) (fixedObservation []))
          ,(fixedAck, replaceField "ranges" (toJSON ([[4096, 4116]] :: [[Int]])) (fixedObservation []))
          ,(fixedAck, replaceField "records" Null (fixedObservation []))]]
    , expect "a fetch wider than the decoder's access bound fails closed" $
        isLeft (executionGate fixedAck (fixedObservation [fetchRecord 0 4096 4096 4096 17]))
    , expect "a classified observer still rejects protected writes" $
        let checked = do
              ack <- parseFixedWatchAck fixedAck
              observation <- parseFixedWatchObservation limit4 (fixedObservation [watchRecord "cpu-write" 4096])
              checkExecutionWrites ForbidProtectedWrites limit4 [expectedRange] [expectedRange] ack observation
         in isLeft (judgeExecutionWrites checked)
    , expect "classified observations reject reads without consuming a fetch-control verdict" $
        isLeft (executionGate fixedAck (fixedObservation [watchRecord "cpu-read" 4096]))
    , expect "a complete pre-unload receipt qualifies independently" $
        receiptGate BeforeUnload CandidateRun Nothing positiveExecutionReceipt == Right ExecutionAccepted
    , expect "a restored-entry receipt requires a clean pre-unload interval and exact fetched PC" $
        case receiptGate ThroughAfterUnload FaultControlRun (Just (watchAddress 4096)) controlExecutionReceipt of
          Right (ExecutionRejected [fetch]) -> forbiddenFetchPc fetch == watchAddress 4096
          _ -> False
    , expect "receipt summaries cannot hide a forbidden fetch" $
        isLeft (receiptGate BeforeUnload CandidateRun Nothing
          (replaceField "final_state" (fixedObservation [fetchRecord 0 4096 4096 4096 4]) positiveExecutionReceipt))
    , expect "an incomplete, scope-changed or error-bearing execution receipt fails closed" $
        and [isLeft (receiptGate BeforeUnload CandidateRun Nothing value) | value <-
          [replaceField "complete" (Bool False) positiveExecutionReceipt
          ,replaceField "ranges" (toJSON ([[4096, 4116]] :: [[Int]])) positiveExecutionReceipt
          ,replaceField "cores" (toJSON ([0] :: [Int])) positiveExecutionReceipt
          ,replaceField "interval" (String "through-after-unload") positiveExecutionReceipt
          ,replaceField "problem" (String "decoder failure") positiveExecutionReceipt
          ,replaceField "forbidden_fetch_count" (toJSON (1 :: Int)) positiveExecutionReceipt]]
    , expect "a control receipt without its clean pre-unload snapshot is refused" $
        isLeft (receiptGate ThroughAfterUnload FaultControlRun (Just (watchAddress 4096))
          (replaceField "pre_unload" Null controlExecutionReceipt))
    , expect "an original fetch before unload cannot satisfy the restored-entry control" $
        isLeft (receiptGate ThroughAfterUnload FaultControlRun (Just (watchAddress 4096))
          (replaceField "pre_unload"
            (replaceField "final_state" (fixedObservation [fetchRecord 0 4096 4096 4096 4]) positiveExecutionReceipt)
            controlExecutionReceipt))
    , expect "more records than the declared capacity are rejected" $
        isLeft (executionGate fixedAck (fixedObservation (replicate 5 (fetchRecord 0 4096 4096 4096 4))))
    , expect "byte preservation and forbidden execution share a typed classified declaration" $
        protectionOf (executionFile "\"protected_memory\":{\"kind\":\"ram-byte-preservation\",\"ranges\":[[\"00001000\",\"00001010\"]]},\"forbidden_execution\":[[\"00001000\",\"00001010\"]]")
          == Just (PreservationProtection preservedRange)
    , expect "preservation counts identity stores without admitting them under strict write policy" $
        let record = watchRecordSized "cpu-write" 4096 2 0x1234 [0x34,0x12] "little" 10
         in acceptedIdentityCount (policyWrites PreserveProtectedBytes [(4096,4112)] [record]) == Just (1,1)
            && rejectedWriteFault (policyWrites ForbidProtectedWrites [(4096,4112)] [record]) == Just StrictProtectedWrite
    , expect "preservation rejects the first changed store even after a restore" $
        let changed = watchRecordSized "cpu-write" 4096 4 1 [0,0,0,0] "little" 10
            restored = replaceField "pc" (toJSON (4664 :: Int)) (watchRecordSized "cpu-write" 4096 4 0 [1,0,0,0] "little" 11)
         in case policyWrites PreserveProtectedBytes [(4096,4112)] [changed, restored] of
              Right (ExecutionWritesRejected violation summary) ->
                executionWriteFault violation == ChangedProtectedByte
                  && (case executionWriteRecord violation of CpuRecord{cpuPc = pc} -> pc == watchAddress 4660; _ -> False)
                  && protectedCpuWritesValue (protectedCpuWrites (executionTotalWrites summary)) == 2
              _ -> False
    , expect "straddling stores may change only bytes outside the protected range" $
        acceptedIdentityCount (policyWrites PreserveProtectedBytes [(4096,4112)]
          [watchRecordSized "cpu-write" 4094 4 0x1234 [0,0,0,0] "little" 10]) == Just (1,1)
          && acceptedIdentityCount (policyWrites PreserveProtectedBytes [(4096,4112)]
            [watchRecordSized "cpu-write" 4110 4 0x12340000 [0,0,0,0] "little" 10]) == Just (1,1)
          && rejectedWriteFault (policyWrites PreserveProtectedBytes [(4096,4112)]
            [watchRecordSized "cpu-write" 4094 4 0x10000 [0,0,0,0] "little" 10]) == Just ChangedProtectedByte
          && rejectedWriteFault (policyWrites PreserveProtectedBytes [(4096,4112)]
            [watchRecordSized "cpu-write" 4110 4 1 [0,0,0,0] "little" 10]) == Just ChangedProtectedByte
    , expect "adjacent protected ranges and unprotected gaps are checked byte by byte" $
        rejectedWriteFault (policyWrites PreserveProtectedBytes [(4096,4098),(4098,4100)]
          [watchRecordSized "cpu-write" 4097 4 1 [0,0,0,0] "little" 10]) == Just ChangedProtectedByte
          && acceptedIdentityCount (policyWrites PreserveProtectedBytes [(4096,4098),(4098,4100)]
            [watchRecordSized "cpu-write" 4097 4 0x01000000 [0,0,0,0] "little" 10]) == Just (1,1)
          && acceptedIdentityCount (policyWrites PreserveProtectedBytes [(4096,4097),(4099,4100)]
            [watchRecordSized "cpu-write" 4096 4 0x100 [0,0,0,0] "little" 10]) == Just (1,1)
          && rejectedWriteFault (policyWrites PreserveProtectedBytes [(4096,4097),(4099,4100)]
            [watchRecordSized "cpu-write" 4096 4 0x01000000 [0,0,0,0] "little" 10]) == Just ChangedProtectedByte
    , expect "store preservation follows each record's byte order including eight-byte values" $
        acceptedIdentityCount (policyWrites PreserveProtectedBytes [(4096,4112)]
          [watchRecordSized "cpu-write" 4096 2 0x0102 [2,1] "little" 10
          ,watchRecordSized "cpu-write" 4098 2 0x0102 [1,2] "big" 11]) == Just (2,2)
          && rejectedWriteFault (policyWrites PreserveProtectedBytes [(4096,4112)]
            [watchRecordSized "cpu-write" 4096 2 0x0102 [2,1] "big" 10]) == Just ChangedProtectedByte
          && acceptedIdentityCount (policyWrites PreserveProtectedBytes [(4096,4112)]
            [replaceField "value" (toJSON (18446744073709551615 :: Integer))
              (watchRecordSized "cpu-write" 4096 8 0 (replicate 8 255) "big" 10)]) == Just (1,1)
    , expect "missing, incomplete, out-of-width and unsupported store evidence fails closed" $
        let valid = watchRecordSized "cpu-write" 4096 2 1 [1,0] "little" 10
         in and [isLeft (policyWrites PreserveProtectedBytes [(4096,4112)] [record]) | record <-
              [replaceField "before" Null valid
              ,replaceField "before" (toJSON ([1] :: [Int])) valid
              ,replaceField "value" Null valid
              ,replaceField "value" (toJSON (65536 :: Int)) valid
              ,replaceField "byte_order" (String "middle") valid]]
            && rejectedWriteFault (policyWrites PreserveProtectedBytes [(4096,4112)]
              [replaceField "value" Null (watchRecordSized "cpu-write" 4096 16 0 (replicate 16 0) "little" 10)]) == Just UnverifiableCpuStore
    , expect "host and model-bus writes cannot satisfy byte preservation" $
        rejectedWriteFault (policyWrites PreserveProtectedBytes [(4096,4112)] [transferRecord "host-write" 4096]) == Just ForbiddenHostWrite
          && rejectedWriteFault (policyWrites PreserveProtectedBytes [(4096,4112)] [transferRecord "model-bus-write" 4096]) == Just ForbiddenModelBusWrite
    , expect "identity stores do not weaken forbidden instruction-fetch exclusion" $
        let checked = do
              ack <- parseFixedWatchAck fixedAck
              observation <- parseFixedWatchObservation limit4 (fixedObservation
                [watchRecordSized "cpu-write" 4096 2 0x1234 [0x34,0x12] "little" 10, fetchRecord 0 4096 4096 4096 4])
              checkExecution PreserveProtectedBytes limit4 [expectedRange] [expectedRange] ack observation
         in isLeft (judgeExecution CandidateRun Nothing checked)
            && judgeExecution FaultControlRun (Just (watchAddress 4096)) checked == Right ()
    , expect "receipt consumers bind write policy and observed identity-store counts" $
        checkExecutionReceipt PreserveProtectedBytes limit4 [expectedRange] [expectedRange] BeforeUnload CandidateRun Nothing preservedExecutionReceipt == Right ExecutionAccepted
          && isLeft (receiptGate BeforeUnload CandidateRun Nothing preservedExecutionReceipt)
          && isLeft (receiptGate BeforeUnload CandidateRun Nothing
            (replaceField "write_policy" (String "forbid-protected-writes") preservedExecutionReceipt))
          && isLeft (checkExecutionReceipt PreserveProtectedBytes limit4 [expectedRange] [expectedRange] BeforeUnload CandidateRun Nothing
            (replaceField "write_observation" zeroWriteObservation preservedExecutionReceipt))
    , expect "literal read expectations decode exact lowercase bytes" $
        fmap segmentReadExpectations (segmentOf (segmentDocument
          ",\"reads\":[[\"value\",\"00001000\",2]],\"read_expectations\":[{\"read\":\"value\",\"hex\":\"01ab\"}]"))
          == Just [ReadExpectation "value" (C.pack "\x01\xab")]
    , expect "literal read assertions reject differing bytes and differing lengths" $
        let expected = ReadExpectation "value" (C.pack "\x01\xab")
         in checkReadExpectation expected (C.pack "\x01\xab") == Right ()
            && isLeft (checkReadExpectation expected (C.pack "\x01\xac"))
            && isLeft (checkReadExpectation expected (C.pack "\x01"))
    , expect "literal read assertions reject missing, duplicate, differently sized and uppercase inputs" $
        and [refused (segmentDocument fields) | fields <-
          [",\"read_expectations\":[{\"read\":\"missing\",\"hex\":\"00\"}]"
          ,",\"reads\":[[\"v\",\"00001000\",1],[\"v\",\"00001001\",1]],\"read_expectations\":[{\"read\":\"v\",\"hex\":\"00\"}]"
          ,",\"reads\":[[\"v\",\"00001000\",2]],\"read_expectations\":[{\"read\":\"v\",\"hex\":\"00\"}]"
          ,",\"reads\":[[\"v\",\"00001000\",1]],\"read_expectations\":[{\"read\":\"v\",\"hex\":\"AB\"}]"]]
    , expect "read preservation admits one earlier equal-length producer" $
        not (refused (preservationTrajectory "source" 2 False))
    , expect "read preservation rejects missing, forward, ambiguous and unequal-length sources" $
        refused (preservationTrajectory "missing" 2 False)
          && refused (preservationTrajectory "source" 1 False)
          && refused (preservationTrajectory "source" 2 True)
          && refused (C.pack "{\"schema\":1,\"before_unload\":[{\"phase\":\"target\",\"steps\":[],\"reads\":[[\"v\",\"00001000\",2]],\"read_preservations\":[{\"read\":\"v\",\"phase\":\"source\",\"source_read\":\"v\"}]},{\"phase\":\"source\",\"steps\":[],\"reads\":[[\"v\",\"00001000\",2]]}],\"after_unload\":[]}")
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
  unless (and (checkedIn ++ checkedInVariants ++ unpinned ++ expectedFiles : results)) exitFailure

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

replaceField :: String -> Value -> Value -> Value
replaceField name value (Object fields) = Object (KM.insert (K.fromString name) value fields)
replaceField _ _ value = value

executionFile :: String -> C.ByteString
executionFile fields = C.pack ("{\"schema\":1," ++ fields ++ ",\"before_unload\":[],\"after_unload\":[]}")

fixedAck :: Value
fixedAck = object ["armed" .= True, "ranges" .= [[4096, 4112 :: Int]], "views" .= ("fixed" :: String), "classes" .= fixedClasses]

fixedObservation :: [Value] -> Value
fixedObservation records = object
  [ "enabled" .= True, "ranges" .= [[4096, 4112 :: Int]], "records" .= records, "violation" .= Null
  , "views" .= ("fixed" :: String)
  , "classes" .= fixedClasses ]

fixedClasses :: [Value]
fixedClasses =
  [ object ["range" .= [4096, 4112 :: Int], "kinds" .= (["cpu-write", "host-write", "model-bus-write"] :: [String])]
  , object ["range" .= [4096, 4112 :: Int], "kinds" .= (["cpu-fetch"] :: [String])] ]

fetchRecord :: Int -> Int -> Int -> Int -> Int -> Value
fetchRecord core address physical pc size = object
  [ "source" .= ("cpu-fetch" :: String), "core" .= core, "address" .= address, "physical" .= physical
  , "pc" .= pc, "size" .= size, "value" .= Null, "before" .= Null, "byte_order" .= ("little" :: String)
  , "instructions" .= (10 :: Int), "tick" .= (10 :: Int) ]

executionGate :: Value -> Value -> Either String ExecutionGate
executionGate ackValue stateValue = do
  ack <- parseFixedWatchAck ackValue
  observation <- parseFixedWatchObservation limit4 stateValue
  checkExecution ForbidProtectedWrites limit4 [expectedRange] [expectedRange] ack observation

preservationTrajectory :: String -> Int -> Bool -> C.ByteString
preservationTrajectory source size duplicate = C.pack
  ("{\"schema\":1,\"before_unload\":[" ++ producer ++ (if duplicate then "," ++ producer else "")
   ++ ",{\"phase\":\"target\",\"steps\":[],\"reads\":[[\"v\",\"00001000\",2]],\"read_preservations\":[{\"read\":\"v\",\"phase\":\""
   ++ source ++ "\",\"source_read\":\"v\"}]}],\"after_unload\":[]}")
  where
    producer = "{\"phase\":\"source\",\"steps\":[],\"reads\":[[\"v\",\"00001000\"," ++ show size ++ "]]}"

positiveExecutionReceipt :: Value
positiveExecutionReceipt = object
  [ "schema" .= (1 :: Int), "enforcement" .= ("original-body-fetch-exclusion" :: String)
  , "ranges" .= [[4096, 4112 :: Int]], "protected_write_ranges" .= [[4096, 4112 :: Int]]
  , "watched_ranges" .= [[4096, 4112 :: Int]], "interval" .= ("before-unload" :: String)
  , "start" .= ("successful-install" :: String), "end" .= ("before-unload" :: String)
  , "cores" .= ([0, 1] :: [Int]), "views" .= ("fixed" :: String), "limit" .= (4 :: Int)
  , "acknowledgement" .= fixedAck, "acknowledgement_error" .= Null
  , "final_state" .= fixedObservation [], "state_error" .= Null, "complete" .= True
  , "forbidden_fetches" .= ([] :: [Value]), "forbidden_fetch_count" .= (0 :: Int)
  , "control_pc" .= Null, "expect" .= ("clean" :: String), "pre_unload" .= Null
  , "write_policy" .= ("forbid-protected-writes" :: String), "write_observation" .= zeroWriteObservation
  , "trajectory_failure" .= Null, "problem" .= Null ]

controlExecutionReceipt :: Value
controlExecutionReceipt =
  replaceField "interval" (String "through-after-unload")
  . replaceField "end" (String "through-after-unload")
  . replaceField "expect" (String "refused")
  . replaceField "control_pc" (toJSON (4096 :: Int))
  . replaceField "pre_unload" positiveExecutionReceipt
  . replaceField "final_state" (fixedObservation [fetchRecord 0 4096 4096 4096 4])
  . replaceField "forbidden_fetch_count" (toJSON (1 :: Int))
  . replaceField "forbidden_fetches" (toJSON [object
      [ "core" .= (0 :: Int), "pc" .= (4096 :: Int), "address" .= (4096 :: Int), "physical" .= (4096 :: Int)
      , "size" .= (4 :: Int), "instructions" .= (10 :: Int), "tick" .= (10 :: Int) ]])
  $ positiveExecutionReceipt

receiptGate :: ExecutionInterval -> RunPurpose -> Maybe WatchAddress -> Value -> Either String ExecutionGate
receiptGate = checkExecutionReceipt ForbidProtectedWrites limit4 [expectedRange] [expectedRange]

policyWrites :: WritePolicy -> [(Int, Int)] -> [Value] -> Either String ExecutionWriteGate
policyWrites policy bounds records = do
  protected <- mapM (\(low, high) -> watchRange (watchAddress (fromIntegral low)) (watchAddress (fromIntegral high))) bounds
  let classes =
        [object ["range" .= [low, high], "kinds" .= (["cpu-write","host-write","model-bus-write"] :: [String])] | (low, high) <- bounds]
        ++ [object ["range" .= [4096,4112 :: Int], "kinds" .= (["cpu-fetch"] :: [String])]]
  ack <- parseFixedWatchAck (replaceField "classes" (toJSON classes) fixedAck)
  observation <- parseFixedWatchObservation limit4 (replaceField "classes" (toJSON classes) (fixedObservation records))
  checkExecutionWrites policy limit4 protected [expectedRange] ack observation

acceptedIdentityCount :: Either String ExecutionWriteGate -> Maybe (Int, Int)
acceptedIdentityCount result = case result of
  Right (ExecutionWritesAccepted summary) ->
    Just (protectedCpuWritesValue (protectedCpuWrites (executionTotalWrites summary)),
      identityCpuStoresValue (executionIdentityCpuStores summary))
  _ -> Nothing

rejectedWriteFault :: Either String ExecutionWriteGate -> Maybe ExecutionWriteFault
rejectedWriteFault result = case result of
  Right (ExecutionWritesRejected violation _) -> Just (executionWriteFault violation)
  _ -> Nothing

zeroWriteObservation :: Value
zeroWriteObservation = object
  ["cpu_writes" .= (0 :: Int), "protected_identity_cpu_stores" .= (0 :: Int), "host_writes" .= (0 :: Int)
  , "model_bus_writes" .= (0 :: Int), "first_violation" .= Null]

preservedExecutionReceipt :: Value
preservedExecutionReceipt =
  replaceField "write_policy" (String "preserve-protected-bytes")
  . replaceField "final_state" (fixedObservation [watchRecordSized "cpu-write" 4096 2 0x1234 [0x34,0x12] "little" 10])
  . replaceField "write_observation" (object
      ["cpu_writes" .= (1 :: Int), "protected_identity_cpu_stores" .= (1 :: Int), "host_writes" .= (0 :: Int)
      , "model_bus_writes" .= (0 :: Int), "first_violation" .= Null])
  $ positiveExecutionReceipt

afReceiptControls :: IO ()
afReceiptControls = do
  let identity = object ["sdk" .= String "/nix/store/exact-sdk", "viewer" .= String "viewer-hash", "bridge" .= String "bridge-hash", "model" .= String "model-hash", "package" .= String "package-hash"]
      site dynamic = object (["pc" .= (4096 :: Int), "word" .= (0xe1a00000 :: Integer), "kind" .= String "consumer"]
        ++ if dynamic then ["reads" .= ([] :: [[Int]]), "register_reads" .= [object ["register" .= (0 :: Int), "offset" .= (0 :: Int), "bytes" .= (4 :: Int)]]]
           else ["reads" .= [[8192 :: Int, 4]]])
      plan dynamic = either error id (decodeAfOwnershipPlan (BL.toStrict (encode (object ["identity" .= identity, "sites" .= [site dynamic]]))))
      counts = [1, 0 :: Int]
      addresses = [0x016fec3c, 0x016fec4c] ++ [0x016fed28,0x016fed2c .. 0x016fee44]
        ++ [0x0170306c, 0x01703080, 0x01703084, 0x017030d0, 0x01703160, 0x0170319c, 0x017031a4, 0x0170327c :: Int]
      snapshot clock = object ["counts" .= (clock :: [Int]), "words" .= [object ["address" .= address, "value" .= (if clock == counts && address == 0x016fec3c then 1 else 0 :: Int)] | address <- addresses]]
      mapping core alias = object ["core" .= (core :: Int), "address" .= (alias :: Integer), "physical" .= (0 :: Int), "size" .= (0x10000000 :: Int), "permissions" .= (7 :: Int)]
      shared = object ["kind" .= String "shared-physical", "physical" .= (8192 :: Int), "observed_tcm" .= [[0,0],[0,0 :: Int]]]
      tcm = object ["kind" .= String "tcm", "core" .= String "primary", "bank" .= String "btcm", "register" .= (0x200d :: Int)
        , "window_start" .= (8192 :: Int), "window_end" .= (12288 :: Int), "offset" .= (0 :: Int), "observed_tcm" .= [[0x200d,0],[0,0 :: Int]]]
      readWith provenance = object ["address" .= (8192 :: Int), "bytes" .= [0x12,0x34,0x56,0x78 :: Int], "provenance" .= provenance]
      entry = object ["opaque_id" .= String "InstructionEntryId { fixture: 1 }", "address" .= (4096 :: Int), "bytes" .= (4 :: Int)
        , "progress" .= (1 :: Int), "pstate" .= (0xd3 :: Int), "physical" .= (4096 :: Int), "tcm" .= [Null,Null]]
      tcmWindow address bytes = object ["address" .= (address :: Int), "bytes" .= (bytes :: Int)]
      entryFor (Object provenance) | KM.lookup "observed_tcm" provenance == Just (toJSON [[0x200d,0],[0,0 :: Int]]) =
        replaceField "tcm" (toJSON [tcmWindow 8192 4096, Null]) entry
      entryFor _ = entry
      context provenance = object ["sequence" .= (0 :: Int), "core" .= (0 :: Int), "pc" .= (4096 :: Int)
        , "word" .= (0xe1a00000 :: Integer), "kind" .= String "consumer", "cpsr" .= (0xd3 :: Int)
        , "registers" .= (8192 : replicate 12 0 :: [Int]), "sp" .= (16384 :: Int), "lr" .= (4100 :: Int)
        , "sampled_task" .= object ["status" .= String "refused", "error" .= String "no active task", "context_depth" .= Null]
        , "entry" .= entryFor provenance, "counts" .= counts, "cursor" .= (0 :: Int), "reads" .= [readWith provenance]]
      row = object ["af_sequence" .= (0 :: Int), "source" .= String "host-write", "address" .= (0x016fec3c :: Int)
        , "physical" .= (0x016fec3c :: Int), "size" .= (1 :: Int), "core" .= Null, "pc" .= Null
        , "instructions" .= (1 :: Int), "tick" .= (1 :: Int), "before" .= [0 :: Int], "value" .= (1 :: Int), "byte_order" .= String "little"]
      receipt provenance = object ["schema" .= (4 :: Int), "status" .= String "complete", "identity" .= identity
        , "mode" .= String "passive", "retention" .= object ["status" .= String "stopped", "reason" .= String "end"]
        , "initial" .= snapshot [0,0], "final" .= snapshot counts, "rows" .= [row], "contexts" .= [context provenance]
        , "failure" .= Null, "read_refusal" .= Null, "row_count" .= (1 :: Int), "context_count" .= (1 :: Int)
        , "limits" .= object ["rows" .= (65536 :: Int), "contexts" .= (8192 :: Int), "pairs" .= (128 :: Int), "output_bytes" .= (134217728 :: Int)]
        , "mappings" .= [mapping core alias | core <- [0,1], alias <- [0,0x40000000]]]
      catalogue = either error id (afMappingCatalogue (toJSON
        [object ["core" .= core, "address" .= address, "physical" .= (0 :: Int)
          , "size" .= (268435456 :: Int), "permissions" .= (7 :: Int)]
        | core <- [0,1 :: Int], address <- [0,1073741824 :: Integer]]))
      validate dynamic = validateAfOwnershipReceipt catalogue (plan dynamic) EndAfOwnership
      accepts label dynamic value = unless (validate dynamic value == Right ()) (ioError (userError label))
      refuses label dynamic value = unless (either (const True) (const False) (validate dynamic value)) (ioError (userError label))
      contexts value = replaceField "contexts" (toJSON [value]) (receipt shared)
      delete name (Object fields) = Object (KM.delete name fields)
      delete _ value = value
  accepts "complete shared-physical receipt" False (receipt shared)
  accepts "register-derived shared read remains physical" True (receipt shared)
  accepts "complete executing-core TCM receipt" True (receipt tcm)
  let armed = replaceField "retention" (object ["status" .= String "armed"])
        . replaceField "status" (String "armed") . replaceField "final" Null
        . replaceField "rows" (toJSON ([] :: [Value])) . replaceField "contexts" (toJSON ([] :: [Value]))
        . replaceField "row_count" (toJSON (0 :: Int)) . replaceField "context_count" (toJSON (0 :: Int)) $ receipt shared
  unless (validateAfOwnershipReceipt catalogue (plan False) ArmAfOwnership armed == Right ()) (ioError (userError "complete armed receipt"))
  forM_ ["initial","final","identity","rows","contexts","mappings","read_refusal","mode","retention"] $ \name ->
    refuses ("missing receipt " ++ show name) False (delete name (receipt shared))
  forM_ [("schema",toJSON (2 :: Int)),("status",String "blocked"),("identity",object []),("failure",String "read failed")
        ,("read_refusal",object ["reason" .= String "payload-unreadable"]),("row_count",toJSON (0 :: Int))
        ,("context_count",toJSON (8193 :: Int)),("unknown",Null),("rows",toJSON ([] :: [Value]))] $ \(name,value) ->
    refuses ("receipt refusal " ++ show name) False (replaceField name value (receipt shared))
  forM_ ["entry","registers","sp","lr","sampled_task","counts","reads"] $ \name ->
    refuses ("missing preread context " ++ show name) False (contexts (delete name (context shared)))
  forM_ [("cpsr",toJSON (0xf3 :: Int)),("pc",toJSON (4100 :: Int)),("word",toJSON (0 :: Int))
        ,("sequence",toJSON (1 :: Int)),("cursor",toJSON (2 :: Int)),("counts",toJSON [2,0 :: Int]),("unknown",Null)] $ \(name,value) ->
    refuses ("context refusal " ++ show name) False (contexts (replaceField name value (context shared)))
  refuses "missing read payload" False (contexts (replaceField "reads" (toJSON [delete "bytes" (readWith shared)]) (context shared)))
  refuses "short read mask" False (contexts (replaceField "reads" (toJSON [replaceField "bytes" (toJSON [0 :: Int]) (readWith shared)]) (context shared)))
  refuses "read mask cannot disappear" False (contexts (replaceField "reads" (toJSON ([] :: [Value])) (context shared)))
  refuses "fixed physical read cannot claim TCM" False (receipt tcm)
  refuses "TCM read cannot fake physical" True (receipt (replaceField "observed_tcm" (toJSON [[0x200d,0],[0,0 :: Int]]) shared))
  forM_ [("core",String "secondary"),("bank",String "unknown"),("offset",toJSON (1 :: Int)),("window_end",toJSON (16384 :: Int))] $ \(name,value) ->
    refuses ("TCM identity " ++ show name) True (receipt (replaceField name value tcm))
  refuses "shared physical mismatch" False (receipt (replaceField "physical" (toJSON (8193 :: Int)) shared))
  refuses "unknown memory view" False (receipt (replaceField "kind" (String "virtual") shared))
  refuses "register-derived read overflow" True (contexts (replaceField "registers" (toJSON (4294967295 : replicate 12 0 :: [Integer])) (context shared)))
  refuses "reordered contexts" False (replaceField "context_count" (toJSON (2 :: Int))
    (replaceField "contexts" (toJSON [replaceField "sequence" (toJSON (1 :: Int)) (context shared),context shared]) (receipt shared)))
  forM_ [("address",toJSON (4097 :: Int)),("progress",toJSON (0 :: Int)),("pstate",toJSON (0x13 :: Int)),("physical",Null),("unknown",Null)] $ \(name,value) ->
    refuses ("actual instruction entry " ++ show name) False (contexts (replaceField "entry" (replaceField name value entry) (context shared)))
  refuses "failure with full preread evidence never passes" False
    (replaceField "failure" (String "payload-unreadable") (contexts (replaceField "reads" (toJSON ([] :: [Value])) (context shared))))
  refuses "unknown write source" False (replaceField "rows" (toJSON [replaceField "source" (String "unknown") row]) (receipt shared))
  refuses "write payload overflow" False (replaceField "rows" (toJSON [replaceField "value" (toJSON (256 :: Int)) row]) (receipt shared))
  refuses "reordered boundary words" False (replaceField "initial" (object ["counts" .= [0,0 :: Int]
    , "words" .= [object ["address" .= address, "value" .= (0 :: Int)] | address <- reverse addresses]]) (receipt shared))
  refuses "boundary clock overflow" False (replaceField "final" (object ["counts" .= [18446744073709551615,1 :: Integer]
    , "words" .= [object ["address" .= address, "value" .= (0 :: Int)] | address <- addresses]]) (receipt shared))
  unless (either (const True) (const False) (decodeAfOwnershipPlan (BL.toStrict (encode (object ["identity" .= identity,"sites" .= [site False]])) <> " trailing")))
    (ioError (userError "AF plan trailing bytes refuse"))
  refuses "captured write preimage mismatch" False (replaceField "rows" (toJSON [replaceField "before" (toJSON [1 :: Int]) row]) (receipt shared))
  refuses "final snapshot cannot conceal write" False (replaceField "final" (object ["counts" .= counts
    , "words" .= [object ["address" .= address, "value" .= (0 :: Int)] | address <- addresses]]) (receipt shared))
  let withTcmEntry windows = replaceField "contexts"
        (toJSON [replaceField "entry" (replaceField "tcm" (toJSON windows) (entryFor tcm)) (context tcm)]) (receipt tcm)
  forM_ [("null", [Null,Null]), ("wrong-bank", [Null,tcmWindow 8192 4096])
        ,("remapped", [tcmWindow 16384 4096,Null]), ("wrong-size", [tcmWindow 8192 8192,Null])] $ \(label,windows) ->
    refuses ("instruction entry TCM " ++ label) True (withTcmEntry windows)
  refuses "disabled observed TCM cannot retain entry window" True
    (replaceField "contexts" (toJSON [replaceField "reads"
      (toJSON [readWith (replaceField "observed_tcm" (toJSON [[0,0],[0,0 :: Int]]) tcm)]) (context tcm)]) (receipt tcm))
  refuses "disabled TCM cannot appear in shared entry" False
    (contexts (replaceField "entry" (replaceField "tcm" (toJSON [tcmWindow 8192 4096,Null]) entry) (context shared)))
  let substitutedMaps = toJSON [replaceField "permissions" (toJSON (3 :: Int)) (mapping core alias)
        | core <- [0,1], alias <- [0,0x40000000]]
      substitutedReceipt = replaceField "mappings" substitutedMaps (receipt shared)
      substitutedCatalogue = either error id (afMappingCatalogue substitutedMaps)
  unless (validateAfOwnershipReceipt substitutedCatalogue (plan False) EndAfOwnership substitutedReceipt == Right ())
    (ioError (userError "substituted catalogue receipt is internally consistent"))
  refuses "receipt cannot substitute a self-consistent mapping catalogue" False substitutedReceipt
  forM_ [Null, object [], object ["status" .= String "disabled"], object ["status" .= String "unarmed"]
        ,object ["status" .= String "armed"], object ["status" .= String "default"], object ["status" .= String "unknown"]
        ,object ["status" .= String "stopped", "reason" .= String "refused"]
        ,object ["status" .= String "stopped", "reason" .= String "capacity-exceeded"]
        ,object ["status" .= String "stopped", "reason" .= String "unknown"]
        ,object ["status" .= String "stopped", "reason" .= String "end", "unknown" .= Null]] $ \retention ->
    refuses "end receipt requires passive retention stopped at end" False (replaceField "retention" retention (receipt shared))
  forM_ [Null, String "default", String "active", String "unknown"] $ \mode ->
    refuses "capture mode is closed and passive" False (replaceField "mode" mode (receipt shared))
  refuses "schema3 has no passive retention proof" False (replaceField "schema" (toJSON (3 :: Int)) (receipt shared))
  forM_ [Null, object ["status" .= String "disabled"], object ["status" .= String "unarmed"]
        ,object ["status" .= String "stopped", "reason" .= String "end"]
        ,object ["status" .= String "armed", "unknown" .= Null]] $ \retention ->
    unless (either (const True) (const False) (validateAfOwnershipReceipt catalogue (plan False) ArmAfOwnership (replaceField "retention" retention armed)))
      (ioError (userError "arm receipt requires passive retention armed"))
  forM_ [0,1 :: Int] $ \core ->
    forM_ [0,1 :: Int] $ \bank -> do
      let observed = [[if sampledCore == core && sampledBank == bank then Null else toJSON (0 :: Int)
                      | sampledBank <- [0,1 :: Int]] | sampledCore <- [0,1 :: Int]]
      refuses ("shared provenance requires sampled TCM register " ++ show (core,bank)) False
        (receipt (replaceField "observed_tcm" (toJSON observed) shared))
      refuses ("dynamic shared provenance requires sampled TCM register " ++ show (core,bank)) True
        (receipt (replaceField "observed_tcm" (toJSON observed) shared))
  let crossingWrite = replaceField "address" (toJSON (0x016fec30 :: Int))
        . replaceField "physical" (toJSON (0x016fec30 :: Int))
        . replaceField "size" (toJSON (8 :: Int))
        . replaceField "before" (toJSON (replicate 8 (0 :: Int)))
        . replaceField "value" (toJSON (0x0807060504030201 :: Integer)) $ row
      crossingReceipt = replaceField "rows" (toJSON [crossingWrite])
        (replaceField "final" (object ["counts" .= counts
          , "words" .= [object ["address" .= address, "value" .= (0 :: Int)] | address <- addresses]]) (receipt shared))
      withCrossingWrite value = replaceField "rows" (toJSON [value]) crossingReceipt
  accepts "write straddling selected queue end remains captured" False crossingReceipt
  refuses "fully disjoint write cannot claim AF capture" False
    (withCrossingWrite (replaceField "address" (toJSON (0x01800000 :: Int))
      (replaceField "physical" (toJSON (0x01800000 :: Int)) crossingWrite)))
  refuses "captured write virtual extent cannot overflow" False
    (withCrossingWrite (replaceField "address" (toJSON (0xfffffffc :: Integer))
      (replaceField "physical" (toJSON (0xbffffffc :: Integer)) crossingWrite)))
  forM_ ["address","physical"] $ \field ->
    refuses ("captured write address domain " ++ show field) False
      (withCrossingWrite (replaceField field (toJSON (4294967296 :: Integer)) crossingWrite))
  refuses "unavailable provenance retention cannot pass as complete" False
    (replaceField "retention" (object ["status" .= String "stopped", "reason" .= String "provenance-unavailable"]) (receipt shared))
