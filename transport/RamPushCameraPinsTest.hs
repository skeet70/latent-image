{-# LANGUAGE OverloadedStrings #-}
-- Runs //transport:camera ram-push and ram-unload on inputs that parse but
-- differ from the compiled reviewed contract, with no camera attached.
module Main where

import Control.Monad (unless)
import Data.Aeson (Value(..), encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.List (isInfixOf)
import Data.Word (Word32)
import Numeric (showHex)
import System.Directory (createDirectory, makeAbsolute)
import System.Environment (getArgs, lookupEnv)
import System.Exit (ExitCode(..), exitFailure)
import System.FilePath ((</>))
import System.Posix.Files (setFileMode)
import System.Process (readProcessWithExitCode)
import FujiAF.RamPush
import FujiAF.SHA256 (sha256)
import FujiAF.Transport (writePrivateBytes)
import RamPushReviewedPins (reviewedContract)

base :: Word32
base = 0x00c20000

hexWord :: Word32 -> String
hexWord value = "0x" ++ replicate (8 - length digits) '0' ++ digits where digits = showHex value ""

publications :: [Publication]
publications =
  [ Publication index (if index < setupCount then Setup else Activation)
      (Address (0x00630000 + fromIntegral (4*index)))
      (WordValue (0xe1000000 + fromIntegral index))
      (WordValue (0xea000000 + fromIntegral index)) Both
  | index <- [0..publicationCount-1] ]

entries :: RamPushEntries
entries = RamPushEntries (Address (base+0xa200)) (Address (base+0xa204)) (Address (base+0xa208))

runtime :: B.ByteString
runtime = B.replicate reviewedAllocationSize 0

manifestDocument :: [(String,String)] -> B.ByteString
manifestDocument identity = BL.toStrict (encode (object
  [ "schema" .= (1 :: Int)
  , "base" .= hexWord base
  , "size" .= reviewedAllocationSize
  , "runtime_sha256" .= sha256 runtime
  , "source_identity" .= object [K.fromString name .= digest | (name,digest) <- identity]
  , "entry" .= object
      [ "install" .= hexWord (unAddress (installEntry entries))
      , "rollback" .= hexWord (unAddress (rollbackEntry entries))
      , "status" .= hexWord (unAddress (statusEntry entries)) ]
  , "publications" .= map publication publications ]))
  where
    publication p = object
      [ "index" .= publicationIndex p
      , "phase" .= String (if publicationPhase p == Setup then "setup" else "activation")
      , "address" .= hexWord (unAddress (publicationAddress p))
      , "old" .= hexWord (unWordValue (publicationOld p))
      , "new" .= hexWord (unWordValue (publicationNew p))
      , "core" .= String "both" ]

privateDirectory :: FilePath -> IO FilePath
privateDirectory path = createDirectory path >> setFileMode path 0o700 >> pure path

-- | Requires the camera to exit nonzero with `reason` on stderr.
refuses :: FilePath -> String -> [String] -> String -> IO ()
refuses camera label arguments reason = do
  (code,_,errors) <- readProcessWithExitCode camera arguments ""
  unless (code /= ExitSuccess && reason `isInfixOf` errors) $ do
    putStrLn (label ++ ": expected a refusal containing " ++ show reason ++ ", got " ++ show code ++ " " ++ show errors)
    exitFailure
  putStrLn (label ++ ": refused")

main :: IO ()
main = do
  [cameraArgument] <- getArgs
  camera <- makeAbsolute cameraArgument
  temporary <- lookupEnv "TEST_TMPDIR" >>= maybe (ioError (userError "TEST_TMPDIR is unset")) makeAbsolute
  root <- privateDirectory (temporary </> "camera-pins")

  allocDir <- privateDirectory (root </> "alloc")
  allocPath <- writeAllocStateFile allocDir
    (RamAllocState (Address base) reviewedAllocationSize 0x4003 callbackTableAddress callbackStockWord bootstrapBytes (replicate 64 '0') "X-M1" 1)

  let manifestCase name identity = do
        directory <- privateDirectory (root </> name)
        writePrivateBytes (directory </> "runtime.bin") runtime
        writePrivateBytes (directory </> "ram-push-manifest.json") (manifestDocument identity)
        pure (directory </> "ram-push-manifest.json")
      reviewed = reviewedSourceIdentity reviewedContract
      substituted = [(name, if name == "menu/ram_push_installer.c" then replicate 64 'f' else digest) | (name,digest) <- reviewed]
  unreviewedSource <- manifestCase "unreviewed-source" substituted
  refuses camera "ram-push with an unreviewed source identity"
    ["ram-push", allocPath, unreviewedSource, root </> "push-unreviewed-source"]
    "manifest source identity differs from the compiled reviewed contract"
  unreviewedContract <- manifestCase "unreviewed-contract" reviewed
  refuses camera "ram-push with an unreviewed publication contract"
    ["ram-push", allocPath, unreviewedContract, root </> "push-unreviewed-contract"]
    "manifest publication contract differs from the compiled reviewed contract"

  stateDir <- privateDirectory (root </> "state")
  statePath <- writeStateFile stateDir
    (RamPushState (replicate 64 'a') (sha256 runtime) (Address base) reviewedAllocationSize entries publications callbackStockWord bootstrapBytes 1)
  refuses camera "ram-unload of an unreviewed publication contract"
    ["ram-unload", statePath, root </> "unload-unreviewed-contract"]
    "installed state publication contract differs from the compiled reviewed contract"
