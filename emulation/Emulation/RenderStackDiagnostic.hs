{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Emulation.RenderStackDiagnostic
  ( RenderStackDiagnosticConfig
  , readRenderStackDiagnosticConfig
  , renderStackDiagnosticBeginJson
  , checkRenderStackDiagnosticReport
  , checkRenderStackDiagnosticBegin
  ) where

import Control.Monad (unless)
import Data.Aeson
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (Parser, parseEither)
import qualified Data.ByteString as B
import Data.List (nub, sort)
import qualified Data.Text as T
import Data.Word (Word32)
import System.IO (IOMode (ReadMode), withBinaryFile)
import qualified Replacement.ModalRuntime.RenderStackEvidence as E

data DiagnosticRole = Stock | Owned deriving (Eq)
data SourceKind = OwnedSource | RetainedSource
data EntryKind = Tree | Picker | Pane deriving (Eq)
newtype DiagnosticAddress = DiagnosticAddress Word32 deriving (Eq)
newtype FrameDepth = FrameDepth Word32
newtype SourcePath = SourcePath FilePath
newtype SourceDigest = SourceDigest T.Text

data DiagnosticSource = DiagnosticSource SourceKind DiagnosticAddress SourcePath SourceDigest
data DiagnosticEntry = DiagnosticEntry EntryKind DiagnosticAddress FrameDepth
data RenderStackDiagnosticConfig = RenderStackDiagnosticConfig
  DiagnosticRole [DiagnosticSource] [DiagnosticEntry] [DiagnosticAddress] [DiagnosticAddress]


keys :: [Key] -> Object -> Parser ()
keys expected fields = unless (sort expected == sort (KM.keys fields)) (fail "render stack diagnostic keys differ from schema")

instance FromJSON DiagnosticAddress where
  parseJSON value = do
    address <- parseJSON value
    unless (address `mod` 4 == 0 && address < 0x20000000) (fail "diagnostic address is not aligned canonical RAM")
    pure (DiagnosticAddress address)
instance ToJSON DiagnosticAddress where
  toJSON (DiagnosticAddress address) = toJSON address

instance FromJSON DiagnosticRole where
  parseJSON = withText "diagnostic role" $ \tag -> case tag of
    "stock" -> pure Stock
    "owned" -> pure Owned
    _ -> fail "unknown diagnostic role"
instance ToJSON DiagnosticRole where
  toJSON Stock = String "stock"
  toJSON Owned = String "owned"
instance FromJSON SourceKind where
  parseJSON = withText "source kind" $ \tag -> case tag of
    "owned" -> pure OwnedSource
    "retained" -> pure RetainedSource
    _ -> fail "unknown diagnostic source kind"
instance ToJSON SourceKind where
  toJSON OwnedSource = String "owned"
  toJSON RetainedSource = String "retained"
instance FromJSON EntryKind where
  parseJSON = withText "entry kind" $ \tag -> case tag of
    "tree" -> pure Tree
    "picker" -> pure Picker
    "pane" -> pure Pane
    _ -> fail "unknown diagnostic entry kind"
instance ToJSON EntryKind where
  toJSON Tree = String "tree"
  toJSON Picker = String "picker"
  toJSON Pane = String "pane"

instance FromJSON DiagnosticSource where
  parseJSON = withObject "diagnostic source" $ \fields -> do
    keys ["kind", "start", "path", "sha256"] fields
    kind <- fields .: "kind"
    start <- fields .: "start"
    path <- fields .: "path"
    digest <- fields .: "sha256"
    unless (not (null path) && length path <= 4096) (fail "diagnostic source path is empty or too long")
    unless (T.length digest == 64 && T.all (\c -> c >= '0' && c <= '9' || c >= 'a' && c <= 'f') digest)
      (fail "diagnostic source digest is not lowercase SHA256")
    pure (DiagnosticSource kind start (SourcePath path) (SourceDigest digest))
instance ToJSON DiagnosticSource where
  toJSON (DiagnosticSource kind start (SourcePath path) (SourceDigest digest)) = object
    ["kind" .= kind, "start" .= start, "path" .= path, "sha256" .= digest]

instance FromJSON DiagnosticEntry where
  parseJSON = withObject "diagnostic entry" $ \fields -> do
    keys ["kind", "address", "frame_depth"] fields
    kind <- fields .: "kind"
    address <- fields .: "address"
    depth <- fields .: "frame_depth"
    let expected = case kind of Tree -> 0x01164e1c; Picker -> 0x01163f2c; Pane -> 0x011662a0
    unless (address == DiagnosticAddress expected && depth > 0 && depth <= 4096)
      (fail "diagnostic entry address or frame depth is invalid")
    pure (DiagnosticEntry kind address (FrameDepth depth))
instance ToJSON DiagnosticEntry where
  toJSON (DiagnosticEntry kind address (FrameDepth depth)) = object
    ["kind" .= kind, "address" .= address, "frame_depth" .= depth]

instance FromJSON RenderStackDiagnosticConfig where
  parseJSON = withObject "render stack diagnostic configuration" $ \fields -> do
    keys ["version", "role", "sources", "entries", "returns", "retained_services"] fields
    version <- fields .: "version" :: Parser Word32
    unless (version == 1) (fail "unsupported render stack diagnostic version")
    role <- fields .: "role"
    sources <- fields .: "sources"
    entries <- fields .: "entries"
    returns <- fields .: "returns"
    services <- fields .: "retained_services"
    let kinds = [kind | DiagnosticEntry kind _ _ <- entries]
    unless (not (null sources) && length sources <= 128 && length entries == 3 && length (nub kinds) == 3
      && not (null returns) && length returns <= 64 && length (nub returns) == length returns
      && length services <= 128 && length (nub services) == length services)
      (fail "diagnostic configuration exceeds bounds or repeats entries")
    pure (RenderStackDiagnosticConfig role sources entries returns services)

readRenderStackDiagnosticConfig :: FilePath -> IO RenderStackDiagnosticConfig
readRenderStackDiagnosticConfig path = withBinaryFile path ReadMode $ \handle -> do
  bytes <- B.hGet handle 65537
  unless (B.length bytes <= 65536) (ioError (userError "render stack diagnostic configuration exceeds 64KiB"))
  either (ioError . userError) pure (eitherDecodeStrict' bytes)

renderStackDiagnosticBeginJson :: RenderStackDiagnosticConfig -> Value
renderStackDiagnosticBeginJson (RenderStackDiagnosticConfig role sources entries returns services) = object
  [ "command" .= String "render-stack-diagnostic-begin", "version" .= (1 :: Word32)
  , "role" .= role, "sources" .= sources, "entries" .= entries
  , "returns" .= returns, "retained_services" .= services ]

checkRenderStackDiagnosticBegin :: Value -> Either String ()
checkRenderStackDiagnosticBegin = parseEither $ withObject "render stack diagnostic acknowledgement" $ \fields -> do
  keys ["version", "diagnostic_only", "active"] fields
  version <- fields .: "version" :: Parser Word32
  diagnosticOnly <- fields .: "diagnostic_only"
  active <- fields .: "active"
  unless (version == 2 && diagnosticOnly && active) (fail "render stack diagnostic did not activate")

checkRenderStackDiagnosticReport :: RenderStackDiagnosticConfig -> Value -> Either String ()
checkRenderStackDiagnosticReport config@(RenderStackDiagnosticConfig expectedRole _ _ _ _) value =
  () <$ E.parseRenderStackEvidence role (renderStackDiagnosticBeginJson config) value
 where
  role = case expectedRole of Stock -> E.StockStack; Owned -> E.OwnedStack
