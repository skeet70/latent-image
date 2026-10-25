{-# LANGUAGE OverloadedStrings #-}
-- | Decoding of e2e trajectory files: the checked-in files and synthetic trace-site and segment cases.
module Main (main) where

import Control.Monad (unless)
import qualified Data.ByteString.Char8 as C
import Data.List (isInfixOf, isSuffixOf, sort)
import Emulation.Symbols
import Emulation.UiTrajectory
import EmulatorUsb (KeyPulse (..), KeyTiming (..), ModeDial (..), SettleBudget (..), defaultKeyTiming)
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
          == Just [TraceSite 0x0114b6c4 "film-icon"]
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
    , expect "a repeated trace address is refused" $
        refused (document "[[\"0114b6c4\",\"x\"],[\"0114b6c4\",\"y\"]]" "[]")
    , expect "a requirement naming no segment is refused" $
        refused (document "[]" "[\"missing\"]")
    , expect "a mode-dial position decodes by its dial name" $
        (segmentModeDial =<< segmentOf (segmentDocument ",\"mode_dial\":\"AUTO\"")) == Just ModeDialAuto
    , expect "an unmodelled mode-dial position is refused" $
        refused (segmentDocument ",\"mode_dial\":\"SR+\"")
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
    ]
  unless (and (checkedIn ++ checkedInVariants ++ results)) exitFailure
