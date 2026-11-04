-- Relinks every payload that has a compiled reviewed contract at its lower
-- reference base, from the checkout's sources and private inputs, and requires
-- 'validateReviewedContract' to accept the manifest, listing each identity and
-- publication-contract difference. The argument is a checked-in file whose
-- runfile links into the checkout.
module Main where

import Control.Exception (SomeException, displayException, try)
import Control.Monad (forM, unless)
import System.Directory (canonicalizePath)
import System.Environment (getArgs, setEnv)
import System.Exit (die, exitFailure)
import System.FilePath (takeDirectory)
import FujiAF.RamPush
import RamPushReviewedPins (reviewedContract)
import RamPushCcReviewedPins (reviewedCcContract)
import RamPushCcIconReviewedPins (reviewedCcIconContract)
import RamPushToggleReviewedPins (reviewedToggleContract)

reviewedPayloads :: [(String, Payload, ReviewedContract)]
reviewedPayloads =
  [ ("bank+D3", bankD3Payload, reviewedContract)
  , ("Classic Chrome", classicChromePayload, reviewedCcContract)
  , ("Classic Chrome icon", classicChromeIconPayload, reviewedCcIconContract)
  , ("USB toggle", usbTogglePayload, reviewedToggleContract)
  ]

contractFindings :: ReviewedContract -> RamPushManifest -> [String]
contractFindings contract manifest =
  sourceIdentityDifferences contract manifest
    ++ [ "publication contract: pinned " ++ pinned ++ ", built " ++ built
       | let pinned = reviewedPublicationContractDigest contract
             built = publicationContractDigest (manifestPublications manifest)
       , pinned /= built ]

main :: IO ()
main = do
  arguments <- getArgs
  checkedIn <- case arguments of
    [path] -> canonicalizePath path
    _ -> die "usage: ram-push-reviewed-identity CHECKED_IN_FILE (a transport/ file)"
  setEnv "BUILD_WORKSPACE_DIRECTORY" (takeDirectory (takeDirectory checkedIn))
  findings <- fmap concat $ forM reviewedPayloads $ \(label, payload, contract) -> do
    relinker <- runfilesRelinker payload
    manifest <- withRelinkedBuildUsing relinker EnforceAllowlist (Address (fst (payloadReferenceBases payload))) (pure . pinnedManifest)
    validation <- try (validateReviewedContract contract manifest)
    let found = case validation of
          Right () -> []
          Left (refusal :: SomeException) -> displayException refusal : contractFindings contract manifest
    putStrLn (label ++ ": " ++ (if null found then "relink matches its compiled reviewed contract" else "refused"))
    pure (map ((label ++ ": ") ++) found)
  mapM_ putStrLn findings
  unless (null findings) $ do
    putStrLn "the reviewed relink refuses these builds: reviewed RAM-push routes fail until the record matches the sources"
    exitFailure
