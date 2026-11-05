module Main (main) where

import Control.Monad (forM, forM_)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified FujiAF.Host as H
import Replacement.RtosOwnFirst.Regions (Domain (..))
import qualified Replacement.RtosScheduler.Artifact as S
import Replacement.RtosScheduler.Cutover
import Replacement.RtosScheduler.CutoverPins
import System.Directory (createDirectory, doesPathExist)
import System.Environment (getArgs)
import System.FilePath ((</>))

data InputPaths = InputPaths Group FilePath FilePath FilePath FilePath FilePath

splitInputs :: [Group] -> [String] -> Either String ([InputPaths], [String])
splitInputs [] args = Right ([],args)
splitInputs (group:groups) (metadata:a:as:b:bs:rest) = do
  (inputs,args) <- splitInputs groups rest
  pure (InputPaths group metadata a as b bs:inputs,args)
splitInputs _ _ = Left "expected metadata, A bin/symbols and B bin/symbols for each group"

variants :: [(String,CutoverVariant)]
variants = [("candidate",CombinedCandidate),("restore-s1",RestoreS1),("restore-s2",RestoreS2),("restore-s3",RestoreS3),("silent-notify",CombinedSilentNotify),("wrong-predicate",CombinedWrongPredicate),("wake-no-insert",WakeNoInsert)]

main :: IO ()
main = do
  (inputs,args) <- getArgs >>= either H.failH pure . splitInputs [minBound .. maxBound]
  case args of
    [command,datPath,loaderAPath,loaderBPath,output] -> do
      selected <- if command == "all" then pure variants else case lookup command variants of
        Just variant -> pure [(command,variant)]
        Nothing -> H.failH "expected all, candidate, restore-s1, restore-s2, restore-s3, silent-notify, wrong-predicate or wake-no-insert"
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
      dat <- readInput (64*1024*1024) datPath
      loaderA <- readInput 0x20000 loaderAPath
      loaderB <- readInput 0x20000 loaderBPath
      source <- refuse (S.canonicalSource dat loaderA loaderB)
      refuse (S.requirePinned source)
      admitted <- refuse (admitCutover source groups)
      staged <- forM selected $ \(label,variant) -> do
        artifact <- refuse (stageCutover admitted variant)
        pure (label,artifact)
      createDirectory output
      L.writeFile (output </> "own-first-guards.json") (J.encode (ownFirstGuards admitted) <> L.singleton 10)
      forM_ staged $ \(label,artifact) -> do
        let directory = if command == "all" then output </> label else output
        if command == "all" then createDirectory directory else pure ()
        B.writeFile (directory </> "candidate.DAT") (cutoverDat artifact)
        B.writeFile (directory </> "loader-a.bin") (cutoverLoader artifact DomainA)
        B.writeFile (directory </> "loader-b.bin") (cutoverLoader artifact DomainB)
        L.writeFile (directory </> "contract.json") (J.encode (cutoverContract artifact) <> L.singleton 10)
    _ -> H.failH "usage: stage-cutover [Bazel-supplied inputs] COMMAND SOURCE_DAT LOADER_A LOADER_B NEW_OUTPUT_DIRECTORY"
