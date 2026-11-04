module Main (main) where

import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified FujiAF.Host as H
import Replacement.RtosOwnFirst.Artifact
import Replacement.RtosOwnFirst.Regions (Domain (..))
import System.Directory (createDirectory, doesPathExist)
import System.Environment (getArgs)
import System.FilePath ((</>))

data Inputs = Inputs
  { inputDat :: FilePath
  , inputLoaderA :: FilePath
  , inputLoaderB :: FilePath
  , inputRegions :: FilePath
  , inputBinA :: FilePath
  , inputSymbolsA :: FilePath
  , inputBinB :: FilePath
  , inputSymbolsB :: FilePath
  , inputOutput :: FilePath
  }

parse :: [String] -> Either String Inputs
parse [regions, binA, symbolsA, binB, symbolsB, dat, loaderA, loaderB, output] =
  Right (Inputs dat loaderA loaderB regions binA symbolsA binB symbolsB output)
parse _ = Left "usage: stage-own-first REGIONS_JSON BIN_A SYMBOLS_A BIN_B SYMBOLS_B SOURCE_DAT LOADER_A LOADER_B NEW_OUTPUT_DIRECTORY"

main :: IO ()
main = do
  inputs <- getArgs >>= either H.failH pure . parse
  let refuse = either (H.failH . show) pure
      output = inputOutput inputs
      readInput = H.readRegularWith (pure ()) False
  exists <- doesPathExist output
  H.ensure (not exists) (output ++ " already exists")
  dat <- readInput (64 * 1024 * 1024) (inputDat inputs)
  loaderA <- readInput 0x20000 (inputLoaderA inputs)
  loaderB <- readInput 0x20000 (inputLoaderB inputs)
  regions <- readInput 0x10000 (inputRegions inputs)
  binA <- readInput 0x20000 (inputBinA inputs)
  symbolsA <- readInput 0x10000 (inputSymbolsA inputs)
  binB <- readInput 0x20000 (inputBinB inputs)
  symbolsB <- readInput 0x10000 (inputSymbolsB inputs)
  source <- refuse (canonicalSource dat loaderA loaderB)
  refuse (requirePinned source)
  admitted <- refuse (admit source regions (LinkedDomain binA symbolsA) (LinkedDomain binB symbolsB))
  staged <- refuse (stage source admitted)
  createDirectory output
  B.writeFile (output </> "candidate.DAT") (stagedDat staged)
  B.writeFile (output </> "loader-a.bin") (stagedLoader staged DomainA)
  B.writeFile (output </> "loader-b.bin") (stagedLoader staged DomainB)
  L.writeFile (output </> "contract.json") (J.encode (contractValue staged) <> L.singleton 10)
