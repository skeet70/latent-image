module Main (main) where

import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified FujiAF.Host as H
import Replacement.RtosException.Stage
import Replacement.RtosOwnFirst.Regions (Domain (..))
import System.Directory (createDirectory, doesPathExist)
import System.Environment (getArgs)
import System.FilePath ((</>))

main :: IO ()
main = do
  args <- getArgs
  case args of
    [datPath, loaderAPath, loaderBPath, binAPath, symbolsAPath, binBPath, symbolsBPath, output] -> do
      let refuse = either (H.failH . show) pure
          readInput = H.readRegularWith (pure ()) False
      exists <- doesPathExist output
      H.ensure (not exists) (output ++ " already exists")
      dat <- readInput (64 * 1024 * 1024) datPath
      loaderA <- readInput 0x20000 loaderAPath
      loaderB <- readInput 0x20000 loaderBPath
      binA <- readInput 0x20000 binAPath
      symbolsA <- readInput 0x10000 symbolsAPath
      binB <- readInput 0x20000 binBPath
      symbolsB <- readInput 0x10000 symbolsBPath
      source <- refuse (canonicalSource dat loaderA loaderB)
      refuse (requirePinned source)
      admitted <- refuse (admit source (Linked binA symbolsA) (Linked binB symbolsB))
      staged <- refuse (stage admitted)
      createDirectory output
      B.writeFile (output </> "candidate.DAT") (stagedDat staged)
      B.writeFile (output </> "loader-a.bin") (stagedLoader staged DomainA)
      B.writeFile (output </> "loader-b.bin") (stagedLoader staged DomainB)
      L.writeFile (output </> "contract.json") (J.encode (contractValue staged) <> L.singleton 10)
    _ -> H.failH "usage: stage-exception SOURCE_DAT LOADER_A LOADER_B BIN_A SYMBOLS_A BIN_B SYMBOLS_B NEW_OUTPUT_DIRECTORY"
