module Main (main) where

import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified FujiAF.Host as H
import Replacement.RtosOwnFirst.Regions (Domain (..))
import Replacement.RtosScheduler.Artifact
import System.Directory (createDirectory, doesPathExist)
import System.Environment (getArgs)
import System.FilePath ((</>))

variantOf :: String -> Either String Variant
variantOf command = case command of
  "candidate" -> Right Candidate
  "stock-insert" -> Right StockInsert
  "silent-notify" -> Right SilentNotify
  "wrong-predicate" -> Right WrongPredicate
  _ -> Left "expected candidate, stock-insert, silent-notify or wrong-predicate"

main :: IO ()
main = do
  args <- getArgs
  case args of
    [regionsPath, binAPath, symbolsAPath, binBPath, symbolsBPath, command, datPath, loaderAPath, loaderBPath, output] -> do
      variant <- either H.failH pure (variantOf command)
      let refuse = either (H.failH . show) pure
          readInput = H.readRegularWith (pure ()) False
      exists <- doesPathExist output
      H.ensure (not exists) (output ++ " already exists")
      dat <- readInput (64 * 1024 * 1024) datPath
      loaderA <- readInput 0x20000 loaderAPath
      loaderB <- readInput 0x20000 loaderBPath
      regions <- readInput 0x10000 regionsPath
      binA <- readInput 0x20000 binAPath
      symbolsA <- readInput 0x10000 symbolsAPath
      binB <- readInput 0x20000 binBPath
      symbolsB <- readInput 0x10000 symbolsBPath
      source <- refuse (canonicalSource dat loaderA loaderB)
      refuse (requirePinned source)
      table <- refuse (parseTable regions)
      a <- refuse (controlLinked variant DomainA (Linked binA symbolsA))
      b <- refuse (controlLinked variant DomainB (Linked binB symbolsB))
      admitted <- refuse (admit source table variant a b)
      staged <- refuse (stage admitted)
      createDirectory output
      B.writeFile (output </> "candidate.DAT") (stagedDat staged)
      B.writeFile (output </> "loader-a.bin") (stagedLoader staged DomainA)
      B.writeFile (output </> "loader-b.bin") (stagedLoader staged DomainB)
      L.writeFile (output </> "contract.json") (J.encode (contractValue staged) <> L.singleton 10)
    _ -> H.failH "usage: stage-scheduler REGIONS BIN_A SYMBOLS_A BIN_B SYMBOLS_B {candidate|stock-insert|silent-notify|wrong-predicate} SOURCE_DAT LOADER_A LOADER_B NEW_OUTPUT_DIRECTORY"
