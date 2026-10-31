-- Re-pins the selected payload's record from its current relink. `bazel run` only.
--   ram-push-repin [--classic-chrome|--classic-chrome-icon|--usb-toggle] approved REVIEW_ID COMMIT SUBJECTS
--   ram-push-repin [--classic-chrome|--classic-chrome-icon|--usb-toggle] content-checked CHECK_ID SUBJECTS
--   ram-push-repin [--classic-chrome|--classic-chrome-icon|--usb-toggle] unreviewed COMMIT SUBJECTS
-- SUBJECTS is one or more pin identities or `publication-contract`, or `--changed`:
-- every pin and the contract whose recorded digest differs from the relink.
module Main where

import Control.Monad (forM, forM_, when)
import qualified Data.ByteString as B
import System.Environment (getArgs, lookupEnv)
import System.Exit (die, exitSuccess)
import System.FilePath ((</>))
import FujiAF.RamPush
import FujiAF.RamPushReview

-- | The subjects a run re-pins: named ones, or every one the relink changed.
data Subjects = Named [Subject] | Changed

subjectArgument :: String -> Subject
subjectArgument "publication-contract" = ContractSubject
subjectArgument identity = PinSubject (PinIdentity identity)

subjectsArgument :: [String] -> Either String Subjects
subjectsArgument ["--changed"] = Right Changed
subjectsArgument [] = Left "name at least one pin identity or publication-contract, or --changed"
subjectsArgument names
  | "--changed" `elem` names = Left "--changed replaces the subject list"
  | otherwise = Right (Named (map subjectArgument names))

statusArguments :: [String] -> Either String (PinStatus, Subjects)
statusArguments arguments = case arguments of
  "approved" : review : commit : subjects -> (,) <$> (Approved <$> reviewIdentifier review <*> commitId commit) <*> subjectsArgument subjects
  "content-checked" : check : subjects -> (,) . ContentChecked <$> checkIdentifier check <*> subjectsArgument subjects
  "unreviewed" : commit : subjects -> (,) . Unreviewed <$> commitId commit <*> subjectsArgument subjects
  _ -> Left "usage: ram-push-repin approved REVIEW_ID COMMIT | content-checked CHECK_ID | unreviewed COMMIT, then SUBJECT... or --changed"

main :: IO ()
main = do
  arguments <- getArgs
  let (payload, recordName, rest) = case arguments of
        "--classic-chrome" : more -> (classicChromePayload, "ram-push-cc-review.json", more)
        "--classic-chrome-icon" : more -> (classicChromeIconPayload, "ram-push-cc-icon-review.json", more)
        "--usb-toggle" : more -> (usbTogglePayload, "ram-push-toggle-review.json", more)
        _ -> (bankD3Payload, "ram-push-review.json", arguments)
  (status, requested) <- either die pure (statusArguments rest)
  workspace <- lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= maybe (die "run with bazel run") pure
  let recordPath = workspace </> "transport" </> recordName
  record <- either (die . ((recordPath ++ ": ") ++)) pure . parseReviewRecord =<< B.readFile recordPath
  relinker <- runfilesRelinker payload
  manifest <- withRelinkedBuildUsing relinker EnforceAllowlist (Address (fst (payloadReferenceBases payload))) (pure . pinnedManifest)
  let built = sourceIdentity manifest
      recorded = [identity | pin <- recordPins record, let PinIdentity identity = pinIdentity pin]
  forM_ [name | (name, _) <- built, name `notElem` recorded] $ \name ->
    putStrLn ("the relink's manifest names " ++ name ++ ", which the record lacks; add its entry first")
  forM_ [name | name <- recorded, name `notElem` map fst built] $ \name ->
    putStrLn ("the record pins " ++ name ++ ", which the relink's manifest does not name")
  let contract = publicationContractDigest (manifestPublications manifest)
      changed = [PinSubject (pinIdentity pin) | pin <- recordPins record, Just digest <- [lookup (unPinIdentity (pinIdentity pin)) built], digest /= unSha256Hex (pinDigest pin)]
        ++ [ContractSubject | contract /= unSha256Hex (contractDigest (recordContract record))]
      subjects = case requested of
        Named named -> named
        Changed -> changed
  when (null subjects) $ do
    putStrLn ("every pin and the publication contract in " ++ recordPath ++ " match the relink; nothing re-pinned")
    exitSuccess
  current <- forM subjects $ \subject -> do
    digest <- case subject of
      ContractSubject -> pure contract
      PinSubject (PinIdentity identity) -> maybe (die ("the relink's manifest does not name " ++ identity)) pure (lookup identity built)
    maybe (die ("not a SHA-256: " ++ digest)) (pure . (,) subject) (sha256Hex digest)
  updated <- either die pure (repin status current record)
  B.writeFile recordPath (renderReviewRecord updated)
  forM_ subjects $ \subject -> putStrLn ("re-pinned " ++ subjectText subject)
  mapM_ (putStrLn . describeFinding) (verdictFindings updated)
  putStrLn ("re-pinned " ++ show (length current) ++ " subjects in " ++ recordPath)

subjectText :: Subject -> String
subjectText ContractSubject = "publication-contract"
subjectText (PinSubject (PinIdentity identity)) = identity
