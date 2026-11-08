{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM, unless)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Data.Map.Strict as M
import qualified FujiAF.Host as H
import Replacement.RtosOwnFirst.Regions (Domain (..))
import qualified Replacement.RtosScheduler.Artifact as S
import Replacement.RtosScheduler.Cutover
import Replacement.RtosScheduler.CutoverPins
import Replacement.RtosScheduler.Family
import System.Directory (createDirectory, doesPathExist)
import System.Environment (getArgs)
import System.FilePath ((</>))

data InputPaths = InputPaths Group FilePath FilePath FilePath FilePath FilePath

splitInputs :: [Group] -> [String] -> Either String ([InputPaths], [String])
splitInputs [] args = Right ([], args)
splitInputs (group:groups) (metadata:a:as:b:bs:rest) = do
  (inputs, args) <- splitInputs groups rest
  pure (InputPaths group metadata a as b bs:inputs, args)
splitInputs _ _ = Left "expected metadata, A bin/symbols and B bin/symbols for each group"

data FamilyPaths = FamilyPaths FilePath FilePath FilePath FilePath

splitFamily :: [String] -> Either String (FamilyPaths, [String])
splitFamily (aBin:aSymbols:bBin:bSymbols:rest) = Right (FamilyPaths aBin aSymbols bBin bSymbols, rest)
splitFamily _ = Left "expected family A bin/symbols and B bin/symbols"

controlValue :: FamilyControl -> J.Value
controlValue item = J.object
  [ "name" J..= controlName item
  , "expected" J..= controlExpected item
  , "observed" J..= either show (const ("accepted" :: String)) (controlObserved item)
  , "passed" J..= controlPassed item
  ]

main :: IO ()
main = do
  (inputs, rest) <- getArgs >>= either H.failH pure . splitInputs [minBound .. maxBound]
  (familyPaths, args) <- either H.failH pure (splitFamily rest)
  case args of
    [command, datPath, loaderAPath, loaderBPath, output] -> do
      unless (command == "candidate" || command == "controls") (H.failH "expected candidate or controls")
      let readInput = H.readRegularWith (pure ()) False
          refuse :: Show e => Either e a -> IO a
          refuse = either (H.failH . show) pure
      exists <- doesPathExist output
      H.ensure (not exists) (output ++ " already exists")
      groups <- forM inputs $ \(InputPaths group metadata a as b bs) -> do
        regionBytes <- readInput 0x10000 metadata
        linkedA <- S.Linked <$> readInput 0x20000 a <*> readInput 0x10000 as
        linkedB <- S.Linked <$> readInput 0x20000 b <*> readInput 0x10000 bs
        pure (GroupInput group regionBytes linkedA linkedB)
      let FamilyPaths aBin aSymbols bBin bSymbols = familyPaths
      images <- do
        a <- FamilyImage <$> readInput 0x20000 aBin <*> readInput 0x10000 aSymbols
        b <- FamilyImage <$> readInput 0x20000 bBin <*> readInput 0x10000 bSymbols
        pure (M.fromList [(DomainA, a), (DomainB, b)])
      dat <- readInput (64 * 1024 * 1024) datPath
      loaderA <- readInput 0x20000 loaderAPath
      loaderB <- readInput 0x20000 loaderBPath
      source <- refuse (S.canonicalSource dat loaderA loaderB)
      refuse (S.requirePinned source)
      base <- refuse (admitCutover source groups)
      baseStaged <- refuse (stageCutover base CombinedCandidate)
      baseRestored <- refuse (restoreCutover base (cutoverDat baseStaged))
      H.ensure (baseRestored == S.sourceDat source) "qualified base does not restore to the source DAT"
      family <- refuse (admitFamily source base baseStaged groups images)
      staged <- refuse (stageFamily family)
      let controls = familyControls source base baseStaged groups images staged
      H.ensure (all controlPassed controls) "family control refused"
      createDirectory output
      L.writeFile (output </> "controls.json") (J.encode (map controlValue controls) <> L.singleton 10)
      if command == "candidate"
        then do
          B.writeFile (output </> "candidate.DAT") (familyDat staged)
          B.writeFile (output </> "loader-a.bin") (familyLoader staged DomainA)
          B.writeFile (output </> "loader-b.bin") (familyLoader staged DomainB)
          L.writeFile (output </> "contract.json") (J.encode (familyContract staged) <> L.singleton 10)
        else pure ()
    _ -> H.failH "usage: family-cutover [Bazel-supplied inputs] FAMILY_A_BIN FAMILY_A_SYMBOLS FAMILY_B_BIN FAMILY_B_SYMBOLS candidate|controls SOURCE_DAT LOADER_A LOADER_B NEW_OUTPUT_DIRECTORY"
