{-# LANGUAGE OverloadedStrings #-}
-- | property-pair-check: qualifies one observation, compares the retained and owned observations,
-- or runs the serialized refusal controls against an admitted pair.
module Main (main) where

import Control.Monad (forM, unless, when)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.List (isPrefixOf)
import qualified Data.Map.Strict as M
import Emulation.PropertyPairCheck
import Emulation.PropertyPairPlan
import Emulation.Sha256 (sha256File, sha256Hex)
import System.Directory (doesPathExist)
import System.Environment (getArgs)
import System.Exit (die, exitFailure)
import System.FilePath ((</>))

usage :: String
usage = unlines
  [ "usage: property-pair-check [--elf ELF] qualify ROLE SOURCE_DAT OWNED_DAT ROUTES ANALYSIS LINKED SYMBOLS CONTRACT OBSERVER OBSERVATION OUTPUT"
  , "       property-pair-check [--elf ELF] compare SOURCE_DAT OWNED_DAT ROUTES ANALYSIS LINKED SYMBOLS CONTRACT OBSERVER RETAINED OWNED OUTPUT"
  , "       property-pair-check [--elf ELF] refusal-controls ROLE SOURCE_DAT OWNED_DAT ROUTES ANALYSIS LINKED SYMBOLS CONTRACT OBSERVER RETAINED OWNED OUTPUT"
  , "       property-pair-check [--elf ELF] control SOURCE_DAT OWNED_DAT ROUTES ANALYSIS LINKED SYMBOLS CONTRACT OBSERVER CONTROL_MANIFEST OWNED CONTROL_OBSERVATION OUTPUT" ]

bindings :: Maybe B.ByteString -> [FilePath] -> IO Bindings
bindings elf [sourceDat, ownedDat, routes, analysis, linked, symbols, contract, observer] = do
  images <- M.fromList <$> forM codeImages (\name -> (,) name <$> B.readFile (analysisPath analysis name))
  Bindings <$> digest sourceDat <*> digest ownedDat <*> B.readFile routes <*> B.readFile contract <*> B.readFile linked
    <*> B.readFile symbols <*> pure images <*> digest observer <*> pure elf
 where
  digest path = sha256Hex <$> sha256File path
bindings _ _ = die usage

observation :: FilePath -> IO Observation
observation directory = do
  reset <- B.readFile (directory </> "reset-observation.json")
  session <- B.readFile (directory </> "emulator-session.json")
  paths <- admitted "boot reset paths" (bootResetPaths session)
  resets <- M.fromList <$> forM paths (\(number,path) -> (,) number <$> B.readFile (directory </> path))
  Observation <$> B.readFile (directory </> "receipt.json") <*> B.readFile (directory </> "spec.json")
    <*> pure reset <*> B.readFile (directory </> "hits.jsonl") <*> B.readFile (directory </> "steps.json")
    <*> pure session <*> pure resets

admitted :: String -> Either Refusal a -> IO a
admitted what = either (\refusal -> die ("property-pair-check: " ++ what ++ " refused: " ++ show refusal)) pure

write :: FilePath -> Value -> IO ()
write output value = do
  exists <- doesPathExist output
  when exists (die "property-pair-check: OUTPUT exists")
  BL.writeFile output (A.encode value <> "\n")

role :: String -> IO Role
role text = either die pure (parseRole text) >>= \parsed -> do
  when (parsed == ControlRole) (die "property-pair-check: the control role has no qualification")
  pure parsed

main :: IO ()
main = getArgs >>= \arguments -> case arguments of
  "--elf" : path : rest -> B.readFile path >>= \elf -> run (Just elf) rest
  _ -> run Nothing arguments

run :: Maybe B.ByteString -> [String] -> IO ()
run elf arguments = case arguments of
  "qualify" : roleText : rest | length rest == 10 -> do
    selected <- role roleText
    bound <- bindings elf (take 8 rest)
    context <- admitted "bindings" (admitBindings bound)
    observed <- observation (rest !! 8)
    result <- admitted "qualify" (qualify context selected observed)
    write (rest !! 9) (qualifiedValue result)
    putStrLn ("property-pair-check: " ++ roleText ++ " qualified")
  "compare" : rest | length rest == 11 -> do
    bound <- bindings elf (take 8 rest)
    context <- admitted "bindings" (admitBindings bound)
    retained <- observation (rest !! 8) >>= admitted "retained qualify" . qualify context RetainedRole
    owned <- observation (rest !! 9) >>= admitted "owned qualify" . qualify context OwnedRole
    pairs <- admitted "compare" (compareRoles retained owned)
    write (rest !! 10) (object ["retained" .= qualifiedValue retained, "owned" .= qualifiedValue owned, "comparison" .= comparisonValue retained owned pairs])
    putStrLn ("property-pair-check: equal over " ++ show (length pairs) ++ " routes")
  "refusal-controls" : roleText : rest | length rest == 11 -> do
    selected <- role roleText
    bound <- bindings elf (take 8 rest)
    context <- admitted "bindings" (admitBindings bound)
    retainedObservation <- observation (rest !! 8)
    ownedObservation <- observation (rest !! 9)
    retained <- admitted "baseline retained qualify" (qualify context RetainedRole retainedObservation)
    owned <- admitted "baseline owned qualify" (qualify context OwnedRole ownedObservation)
    _ <- admitted "baseline compare" (compareRoles retained owned)
    let (mutated, other) = if selected == RetainedRole then (retainedObservation, ownedObservation) else (ownedObservation, retainedObservation)
    outcomes <- forM controlNames $ \control -> case runControl context selected mutated other control of
      Left problem -> pure (object ["control" .= show control, "passed" .= False, "construction" .= problem], False)
      Right result -> do
        let passed = case controlObserved result of
              Left refusal -> any (`isPrefixOf` show refusal) (controlExpected result)
              Right () -> False
        pure (object ([ "control" .= show control, "expected" .= controlExpected result
                      , "observed" .= either show (const "admitted") (controlObserved result), "passed" .= passed ]
                      ++ maybe [] (\stale -> ["mutation" .= staleValue stale]) (controlStale result)), passed)
    let passedAll = all snd outcomes
    write (rest !! 10) (object ["role" .= roleWire selected, "controls" .= map fst outcomes, "passed" .= passedAll])
    putStrLn ("property-pair-check: " ++ show (length (filter snd outcomes)) ++ "/" ++ show (length outcomes) ++ " controls refused as expected")
    unless passedAll exitFailure
  "control" : rest | length rest == 12 -> do
    bound <- bindings elf (take 8 rest)
    context <- admitted "bindings" (admitBindings bound)
    (control, datDigest, image4Digest) <- B.readFile (rest !! 8) >>= either (die . ("property-pair-check: control manifest: " ++)) pure . decodeControlManifest
    owned <- observation (rest !! 9) >>= admitted "owned qualify" . qualify context OwnedRole
    observed <- observation (rest !! 10)
    result <- admitted "guest control" (checkGuestControl context owned control datDigest image4Digest observed)
    write (rest !! 11) result
    putStrLn "property-pair-check: guest control refused at its restored arm"
  _ -> die usage

staleValue :: StaleGetter -> Value
staleValue stale = object
  [ "caller" .= getterCaller (staleKey stale), "arguments" .= getterArguments (staleKey stale), "call" .= staleCall stale
  , "hit" .= staleHit stale, "from" .= staleFrom stale, "to" .= staleTo stale ]
