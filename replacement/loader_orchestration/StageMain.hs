module Main (main) where

import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Emulation.Image4Staging as Image4
import qualified FujiAF.Host as H
import Replacement.LoaderOrchestration.Artifact
import Replacement.LoaderOrchestration.Interface
import System.Directory (canonicalizePath, createDirectory, doesPathExist)
import System.Environment (getArgs)
import System.FilePath ((</>), takeDirectory, takeFileName)

newtype OutputDirectory = OutputDirectory FilePath
data Inputs = Inputs FilePath FilePath FilePath FilePath FilePath FilePath OutputDirectory

parse :: [String] -> Either String Inputs
parse [dat,image4,linkedA,symbolsA,linkedB,symbolsB,output] =
  Right (Inputs dat image4 linkedA symbolsA linkedB symbolsB (OutputDirectory output))
parse _ = Left "usage: stage-loader-orchestration SOURCE_DAT IMAGE4 LINKED_A SYMBOLS_A LINKED_B SYMBOLS_B NEW_OUTPUT_DIRECTORY"

newOutput :: OutputDirectory -> IO FilePath
newOutput (OutputDirectory configured) = do
  parent <- canonicalizePath (takeDirectory configured)
  _ <- Image4.validateScratchDirectory parent
  let leaf = takeFileName configured
      output = parent </> leaf
  H.ensure (leaf/="."&&leaf/=".."&&not (null leaf)) "output requires a new directory name"
  exists <- doesPathExist output
  H.ensure (not exists) "staging output already exists"
  createDirectory output
  pure output

main :: IO ()
main = do
  Inputs datPath image4Path linkedAPath symbolsAPath linkedBPath symbolsBPath configured <- getArgs >>= either H.failH pure . parse
  dat <- H.readRegular (64*1024*1024) datPath
  image4 <- H.readRegular 0x300000 image4Path
  linkedA <- H.readRegular 340 linkedAPath
  symbolsA <- H.readRegular 0x10000 symbolsAPath
  linkedB <- H.readRegular 340 linkedBPath
  symbolsB <- H.readRegular 0x10000 symbolsBPath
  source <- either (H.failH . show) pure (canonicalSource dat image4)
  a <- either (H.failH . show) pure (deriveArtifact LoaderA linkedA symbolsA)
  b <- either (H.failH . show) pure (deriveArtifact LoaderB linkedB symbolsB)
  candidate <- either (H.failH . show) pure (stageCandidate source a b)
  output <- newOutput configured
  B.writeFile (output </> "candidate.DAT") (candidateBytes candidate)
  B.writeFile (output </> "loader-a.bin") (candidateLoader candidate LoaderA)
  B.writeFile (output </> "loader-b.bin") (candidateLoader candidate LoaderB)
  L.writeFile (output </> "contract.json") (J.encode (contractValue candidate))
