{-# LANGUAGE OverloadedStrings #-}
module Emulation.SdkContextAdmission
  ( ContextSdk, SdkContextInputs (..), compilerContextInputs, bindCompilerContextSdk, bindContextSdk, contextSdkValue
  ) where

import Control.Monad (unless)
import Data.Aeson (FromJSON (..), Value, eitherDecodeStrict', object, withObject, withText, (.:), (.=))
import Data.Aeson.Types (Parser)
import Data.Char (isAsciiLower, isAsciiUpper, isDigit)
import qualified Data.Text as T
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.Host as Host
import FujiModPaths (fujimodDriver, fujimodRamPushBridge, fujimodRun)
import System.FilePath ((</>), takeDirectory, takeFileName)

data SdkContextInputs = SdkContextInputs
  { sdkDriverPath :: FilePath
  , sdkRamPushPath :: FilePath
  , sdkRunDocumentPath :: FilePath
  }

data SdkArtifactRole = Driver | RamPushBridge | RunDocument
newtype SdkStoreRoot = SdkStoreRoot FilePath
newtype SourceNarDigest = SourceNarDigest T.Text

data SdkQualification = SdkQualification
  { qualificationRoot :: SdkStoreRoot
  , qualificationModel :: SHA.Sha256
  , qualificationSourceNar :: SourceNarDigest
  , qualificationDriver :: SHA.Sha256
  , qualificationRamPush :: SHA.Sha256
  , qualificationRunDocument :: SHA.Sha256
  }

data SdkArtifactIdentity = SdkArtifactIdentity FilePath SHA.Sha256
data ContextSdk = ContextSdk SdkQualification SdkArtifactIdentity SdkArtifactIdentity SdkArtifactIdentity SdkArtifactIdentity

instance FromJSON SdkQualification where
  parseJSON = withObject "SDK context qualification" $ \packet -> do
    schema <- packet .: "schema" :: Parser Int
    kind <- packet .: "kind" :: Parser T.Text
    unless (schema == 1 && kind == "sdk_context_catalogue_current_base_qualification")
      (fail "SDK qualification kind or schema differs")
    (model, nar, document) <- packet .: "source" >>= withObject "SDK source" (\source -> do
      model <- source .: "checkpoint_model_id" >>= parseDigest
      nar <- source .: "sdk_source_nar_sha256" >>= parseSourceNar
      document <- source .: "preserved" >>= withObject "SDK preserved inputs"
        (\preserved -> preserved .: "run_fixture_sha256" >>= parseDigest)
      pure (model, nar, document))
    (root, driver, bridge) <- packet .: "outputs" >>= withObject "SDK outputs" (\outputs -> do
      root <- outputs .: "full_sdk" >>= parseStoreRoot
      driver <- outputs .: "full_bridge_sha256" >>= parseDigest
      bridge <- outputs .: "ram_push_sha256" >>= parseDigest
      pure (root, driver, bridge))
    packet .: "qualification" >>= withObject "SDK qualification" (\qualification ->
      qualification .: "final_invocation" >>= withObject "SDK final invocation" (\final -> do
        result <- final .: "result" :: Parser T.Text
        code <- final .: "exit_code" :: Parser Int
        failed <- final .: "full_sdk_failed" :: Parser Int
        unless (result == "passed" && code == 0 && failed == 0)
          (fail "SDK qualification is not the current complete passing gate")))
    pure (SdkQualification root model nar driver bridge document)

parseDigest :: Value -> Parser SHA.Sha256
parseDigest = withText "SHA-256" (either fail pure . SHA.parseSha256Hex . T.unpack)

parseSourceNar :: Value -> Parser SourceNarDigest
parseSourceNar = withText "SDK source NAR digest" $ \value -> do
  body <- maybe (fail "SDK source digest requires sha256") pure (T.stripPrefix "sha256-" value)
  unless (T.length body == 44 && T.last body == '='
    && T.all (\c -> isAsciiLower c || isAsciiUpper c || isDigit c || c == '+' || c == '/') (T.init body))
    (fail "SDK source digest is not a SHA-256 NAR identity")
  pure (SourceNarDigest value)

parseStoreRoot :: Value -> Parser SdkStoreRoot
parseStoreRoot = withText "SDK store root" $ \value -> do
  let path = T.unpack value
      name = takeFileName path
      storeHash = take 32 name
  unless (takeDirectory path == "/nix/store" && length storeHash == 32
    && all (`elem` ("0123456789abcdfghijklmnpqrsvwxyz" :: String)) storeHash
    && take 1 (drop 32 name) == "-" && length name > 33)
    (fail "SDK qualification requires a canonical store output")
  pure (SdkStoreRoot path)

compilerContextInputs :: SdkContextInputs
compilerContextInputs = SdkContextInputs fujimodDriver fujimodRamPushBridge fujimodRun

bindCompilerContextSdk :: FilePath -> IO ContextSdk
bindCompilerContextSdk packet = bindContextSdk packet compilerContextInputs

bindContextSdk :: FilePath -> SdkContextInputs -> IO ContextSdk
bindContextSdk packetPath inputs = do
  bytes <- Host.readRegular (64 * 1024) packetPath
  expected <- either (ioError . userError) pure
    (SHA.parseSha256Hex "fe928fec604f0c950970423c9c098f563d49adcd8209ab79b09f4ae7c3aab678")
  let digest = SHA.sha256 bytes
  unless (digest == expected) (ioError (userError "SDK qualification packet differs from the admitted immutable record"))
  qualification <- either (ioError . userError) pure (eitherDecodeStrict' bytes)
  let bindArtifact role path = do
        let SdkStoreRoot root = qualificationRoot qualification
            (suffix, expectedDigest) = case role of
              Driver -> ("bin/fujimod-xm1", qualificationDriver qualification)
              RamPushBridge -> ("bin/fujimod-xm1-ram-push", qualificationRamPush qualification)
              RunDocument -> ("share/fujimod-xm1/xm1-150.json", qualificationRunDocument qualification)
        unless (path == root </> suffix) (ioError (userError "compiler-selected SDK path differs from its qualification output"))
        actual <- SHA.sha256File path
        unless (actual == expectedDigest) (ioError (userError ("SDK artifact differs from its complete qualification identity: " ++ path)))
        pure (SdkArtifactIdentity path actual)
  ContextSdk qualification (SdkArtifactIdentity packetPath digest)
    <$> bindArtifact Driver (sdkDriverPath inputs)
    <*> bindArtifact RamPushBridge (sdkRamPushPath inputs)
    <*> bindArtifact RunDocument (sdkRunDocumentPath inputs)

contextSdkValue :: ContextSdk -> Value
contextSdkValue (ContextSdk qualification packet driver bridge document) = object
  [ "qualification_packet" .= artifactValue packet
  , "driver" .= artifactValue driver, "ram_push_bridge" .= artifactValue bridge
  , "run_document" .= artifactValue document
  , "checkpoint_model_sha256" .= SHA.sha256Hex (qualificationModel qualification)
  , "source_nar_sha256" .= let SourceNarDigest value = qualificationSourceNar qualification in value
  , "installed_current_store_law" .= ("e5001008 stores selected r1 at checked selected-r0 minus eight; e5863000 stores selected r3 at r6; e5823000 stores selected r3 at r2" :: T.Text)
  , "executed_for_candidate" .= False, "installed_lifetime_admitted" .= False
  ]
  where
    artifactValue (SdkArtifactIdentity path digest) = object ["path" .= path, "sha256" .= SHA.sha256Hex digest]
