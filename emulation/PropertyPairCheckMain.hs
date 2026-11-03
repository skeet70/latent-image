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
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import Numeric (readHex)
import Emulation.PropertyPairCheck
import Emulation.PropertyPairPlan
import Emulation.Sha256 (sha256File, sha256Hex)
import System.Directory (doesPathExist)
import System.Environment (getArgs)
import System.Exit (die, exitFailure)
import System.FilePath ((</>))

usage :: String
usage = unlines
  [ "usage: property-pair-check qualify ROLE SOURCE_DAT OWNED_DAT ROUTES ANALYSIS LINKED SYMBOLS CONTRACT OBSERVER OBSERVATION OUTPUT"
  , "       property-pair-check compare SOURCE_DAT OWNED_DAT ROUTES ANALYSIS LINKED SYMBOLS CONTRACT OBSERVER RETAINED OWNED OUTPUT"
  , "       property-pair-check refusal-controls ROLE SOURCE_DAT OWNED_DAT ROUTES ANALYSIS LINKED SYMBOLS CONTRACT OBSERVER RETAINED OWNED OUTPUT"
  , "       property-pair-check control SOURCE_DAT OWNED_DAT ROUTES ANALYSIS LINKED SYMBOLS CONTRACT OBSERVER CONTROL_MANIFEST OWNED CONTROL_OBSERVATION OUTPUT" ]

bindings :: [FilePath] -> IO Bindings
bindings [sourceDat, ownedDat, routes, analysis, linked, symbols, contract, observer] = do
  images <- M.fromList <$> forM codeImages (\name -> (,) name <$> B.readFile (analysisPath analysis name))
  Bindings <$> digest sourceDat <*> digest ownedDat <*> B.readFile routes <*> B.readFile contract <*> B.readFile linked
    <*> digest symbols <*> pure images <*> digest observer
 where
  digest path = sha256Hex <$> sha256File path
bindings _ = die usage

observation :: FilePath -> IO Observation
observation directory = Observation
  <$> B.readFile (directory </> "receipt.json") <*> B.readFile (directory </> "spec.json")
  <*> B.readFile (directory </> "reset-observation.json") <*> B.readFile (directory </> "hits.jsonl")
  <*> B.readFile (directory </> "steps.json") <*> (sha256Hex <$> sha256File (directory </> "emulator-session.json"))

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
  "qualify" : roleText : rest | length rest == 10 -> do
    selected <- role roleText
    bound <- bindings (take 8 rest)
    observed <- observation (rest !! 8)
    result <- admitted "qualify" (qualify bound selected observed)
    write (rest !! 9) (qualifiedValue result)
    putStrLn ("property-pair-check: " ++ roleText ++ " qualified")
  "compare" : rest | length rest == 11 -> do
    bound <- bindings (take 8 rest)
    retained <- observation (rest !! 8) >>= admitted "retained qualify" . qualify bound RetainedRole
    owned <- observation (rest !! 9) >>= admitted "owned qualify" . qualify bound OwnedRole
    pairs <- admitted "compare" (compareRoles retained owned)
    write (rest !! 10) (object ["retained" .= qualifiedValue retained, "owned" .= qualifiedValue owned, "comparison" .= comparisonValue pairs])
    putStrLn ("property-pair-check: equal over " ++ show (length pairs) ++ " routes")
  "refusal-controls" : roleText : rest | length rest == 11 -> do
    selected <- role roleText
    bound <- bindings (take 8 rest)
    retainedObservation <- observation (rest !! 8)
    ownedObservation <- observation (rest !! 9)
    retained <- admitted "baseline retained qualify" (qualify bound RetainedRole retainedObservation)
    owned <- admitted "baseline owned qualify" (qualify bound OwnedRole ownedObservation)
    _ <- admitted "baseline compare" (compareRoles retained owned)
    let (mutated, other) = if selected == RetainedRole then (retainedObservation, ownedObservation) else (ownedObservation, retainedObservation)
    outcomes <- forM controlNames $ \control -> case runControl bound selected mutated other control of
      Left problem -> pure (object ["control" .= show control, "passed" .= False, "construction" .= problem], False)
      Right result -> do
        let passed = case controlObserved result of
              Left refusal -> any (`isPrefixOf` show refusal) (controlExpected result)
              Right () -> False
        pure (object [ "control" .= show control, "expected" .= controlExpected result
                     , "observed" .= either show (const "admitted") (controlObserved result), "passed" .= passed ], passed)
    let passedAll = all snd outcomes
    write (rest !! 10) (object ["role" .= roleWire selected, "controls" .= map fst outcomes, "passed" .= passedAll])
    putStrLn ("property-pair-check: " ++ show (length (filter snd outcomes)) ++ "/" ++ show (length outcomes) ++ " controls refused as expected")
    unless passedAll exitFailure
  "control" : rest | length rest == 12 -> do
    bound <- bindings (take 8 rest)
    manifest <- B.readFile (rest !! 8) >>= either (die . ("property-pair-check: control manifest: " ++)) pure . A.eitherDecodeStrict'
    let text name = case manifest of
          Object fields | Just (String value) <- KM.lookup name fields -> pure (T.unpack value)
          _ -> die ("property-pair-check: control manifest lacks " ++ show name)
        word name = text name >>= \value -> case value of
          '0' : 'x' : digits | [(parsed, "")] <- readHex digits -> pure (fromInteger parsed)
          _ -> die ("property-pair-check: control manifest " ++ show name ++ " is not hexadecimal")
    control <- ControlWord <$> word "property" <*> word "address" <*> word "target"
    datDigest <- text "dat_sha256"
    image4Digest <- text "image4_sha256"
    owned <- observation (rest !! 9) >>= admitted "owned qualify" . qualify bound OwnedRole
    observed <- observation (rest !! 10)
    result <- admitted "guest control" (checkGuestControl bound owned control datDigest image4Digest observed)
    write (rest !! 11) result
    putStrLn "property-pair-check: guest control refused at its restored arm"
  _ -> die usage
