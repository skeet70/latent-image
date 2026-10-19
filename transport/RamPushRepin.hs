-- Rewrites entries of transport/ram-push-review.json with the digests of the
-- current tree's relink and one status. `bazel run` only: the relink reads
-- private-data under BUILD_WORKSPACE_DIRECTORY.
--   ram-push-repin approved REVIEW_ID COMMIT SUBJECT...
--   ram-push-repin content-checked CHECK_ID SUBJECT...
--   ram-push-repin unreviewed COMMIT SUBJECT...
-- SUBJECT is a pin identity or `publication-contract`.
module Main where

import Control.Monad (forM, forM_, when)
import qualified Data.ByteString as B
import System.Environment (getArgs, lookupEnv)
import System.Exit (die)
import System.FilePath ((</>))
import FujiAF.RamPush
import FujiAF.RamPushReview

subjectArgument :: String -> Subject
subjectArgument "publication-contract" = ContractSubject
subjectArgument identity = PinSubject (PinIdentity identity)

statusArguments :: [String] -> Either String (PinStatus, [String])
statusArguments arguments = case arguments of
  "approved" : review : commit : subjects -> (\status -> (status, subjects)) <$> (Approved <$> reviewIdentifier review <*> commitId commit)
  "content-checked" : check : subjects -> (\status -> (status, subjects)) . ContentChecked <$> checkIdentifier check
  "unreviewed" : commit : subjects -> (\status -> (status, subjects)) . Unreviewed <$> commitId commit
  _ -> Left "usage: ram-push-repin approved REVIEW_ID COMMIT | content-checked CHECK_ID | unreviewed COMMIT, then SUBJECT..."

main :: IO ()
main = do
  (status, subjects) <- either die pure . statusArguments =<< getArgs
  when (null subjects) (die "name at least one pin identity or publication-contract")
  workspace <- lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= maybe (die "run with bazel run") pure
  let recordPath = workspace </> "transport" </> "ram-push-review.json"
  record <- either (die . ((recordPath ++ ": ") ++)) pure . parseReviewRecord =<< B.readFile recordPath
  relinker <- runfilesRamPushRelinker
  manifest <- withRelinkedBuildUsing relinker EnforceAllowlist (Address 0x00c20000) (pure . pinnedManifest)
  let built = sourceIdentity manifest
      recorded = [identity | pin <- recordPins record, let PinIdentity identity = pinIdentity pin]
  forM_ [name | (name, _) <- built, name `notElem` recorded] $ \name ->
    putStrLn ("the relink's manifest names " ++ name ++ ", which the record lacks; add its entry first")
  forM_ [name | name <- recorded, name `notElem` map fst built] $ \name ->
    putStrLn ("the record pins " ++ name ++ ", which the relink's manifest does not name")
  current <- forM (map subjectArgument subjects) $ \subject -> do
    digest <- case subject of
      ContractSubject -> pure (publicationContractDigest (manifestPublications manifest))
      PinSubject (PinIdentity identity) -> maybe (die ("the relink's manifest does not name " ++ identity)) pure (lookup identity built)
    maybe (die ("not a SHA-256: " ++ digest)) (pure . (,) subject) (sha256Hex digest)
  updated <- either die pure (repin status current record)
  B.writeFile recordPath (renderReviewRecord updated)
  mapM_ (putStrLn . describeFinding) (verdictFindings updated)
  putStrLn ("re-pinned " ++ show (length current) ++ " subjects in " ++ recordPath)
