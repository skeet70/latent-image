{-# LANGUAGE OverloadedStrings #-}
-- Runs //transport:camera ram-push and ram-unload on inputs that parse but
-- differ from the compiled reviewed contract, with no camera attached.
-- A leading payload flag selects its allocation size and compiled contract.
module Main where

import Control.Monad (unless)
import Data.Aeson (Value(..), encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.List (isInfixOf, isSuffixOf)
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
import RamPushCcReviewedPins (reviewedCcContract)
import RamPushCcIconReviewedPins (reviewedCcIconContract)
import RamPushToggleReviewedPins (reviewedToggleContract)

base :: Word32
base = 0x00c20000

hexWord :: Word32 -> String
hexWord value = "0x" ++ replicate (8 - length digits) '0' ++ digits where digits = showHex value ""

-- | The payload the inputs are sized for, its compiled contract, and an entry offset inside it.
data Target = Target Payload ReviewedContract Word32

publications :: Payload -> [Publication]
publications payload =
  [ Publication index (if index < payloadSetupCount payload then Setup else Activation)
      (Address (0x00630000 + fromIntegral (4*index)))
      (WordValue (0xe1000000 + fromIntegral index))
      (WordValue (0xea000000 + fromIntegral index)) Both
  | index <- [0..payloadPublicationCount payload-1] ]

entries :: Word32 -> RamPushEntries
entries offset = RamPushEntries (Address (base+offset)) (Address (base+offset+4)) (Address (base+offset+8))

runtime :: Payload -> B.ByteString
runtime payload = B.replicate (payloadAllocationSize payload) 0

manifestDocument :: Target -> [(String,String)] -> B.ByteString
manifestDocument (Target payload _ offset) identity = BL.toStrict (encode (object
  [ "schema" .= (1 :: Int)
  , "base" .= hexWord base
  , "size" .= payloadAllocationSize payload
  , "runtime_sha256" .= sha256 (runtime payload)
  , "source_identity" .= object [K.fromString name .= digest | (name,digest) <- identity]
  , "entry" .= object
      [ "install" .= hexWord (unAddress (installEntry (entries offset)))
      , "rollback" .= hexWord (unAddress (rollbackEntry (entries offset)))
      , "status" .= hexWord (unAddress (statusEntry (entries offset))) ]
  , "publications" .= map publication (publications payload) ]))
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
  (cameraArgument, target@(Target payload contract offset)) <- getArgs >>= \arguments -> case arguments of
    [camera] -> pure (camera, Target bankD3Payload reviewedContract 0xa200)
    [camera, "--classic-chrome"] -> pure (camera, Target classicChromePayload reviewedCcContract 0x1000)
    [camera, "--classic-chrome-icon"] -> pure (camera, Target classicChromeIconPayload reviewedCcIconContract 0x1000)
    [camera, "--usb-toggle"] -> pure (camera, Target usbTogglePayload reviewedToggleContract 0x600)
    _ -> putStrLn "usage: ram-push-camera-pins CAMERA [--classic-chrome|--classic-chrome-icon|--usb-toggle]" >> exitFailure
  camera <- makeAbsolute cameraArgument
  temporary <- lookupEnv "TEST_TMPDIR" >>= maybe (ioError (userError "TEST_TMPDIR is unset")) makeAbsolute
  root <- privateDirectory (temporary </> "camera-pins")

  allocDir <- privateDirectory (root </> "alloc")
  allocPath <- writeAllocStateFile allocDir
    (RamAllocState (Address base) (payloadAllocationSize payload) 0x4003 callbackTableAddress callbackStockWord bootstrapBytes (replicate 64 '0') "X-M1" 1)

  let manifestCase name identity = do
        directory <- privateDirectory (root </> name)
        writePrivateBytes (directory </> "runtime.bin") (runtime payload)
        writePrivateBytes (directory </> "ram-push-manifest.json") (manifestDocument target identity)
        pure (directory </> "ram-push-manifest.json")
      reviewed = reviewedSourceIdentity contract
      substituted = [(name, if "ram_push_installer.c" `isSuffixOf` name then replicate 64 'f' else digest) | (name,digest) <- reviewed]
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
    (RamPushState (replicate 64 'a') (sha256 (runtime payload)) (Address base) (payloadAllocationSize payload) (entries offset) (publications payload) callbackStockWord bootstrapBytes 1)
  let unloadReason = case payloadKind payload of
        UsbToggle -> "ram-unload refused: USB toggle requires full battery removal"
        _ -> "installed state publication contract differs from the compiled reviewed contract"
  refuses camera "ram-unload is refused before USB"
    ["ram-unload", statePath, root </> "unload-refused"]
    unloadReason
