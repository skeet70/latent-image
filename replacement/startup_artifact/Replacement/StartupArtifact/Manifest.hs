{-# LANGUAGE OverloadedStrings #-}
-- | The stager's input manifest:
-- @{"schema":4,"guard":{"attach":LABEL,"attach_state":RULES},"pieces":[{"kind":KIND,"inputs":INPUTS}],"application":{}}@,
-- with RULES as 'Replacement.StartupArtifact.AttachState.parseAttachRules' reads them. INPUTS is
-- @{"image":PATH,"symbols":PATH}@, or for @loader-startup@ one such object under each of
-- @loader_a@, @loader_b@, @dispatcher@ and @installer@. @application@ is optional and
-- asks for the contract's application section. Unknown keys refuse.
-- @kernel-slot-bodies@ takes @{"loader_a":LINKED,"loader_b":LINKED,"regions":PATH}@ with LINKED an image object.
module Replacement.StartupArtifact.Manifest
  ( Manifest (..), ManifestPiece (..), LinkedPaths (..), LoaderStartupPaths (..), ApplicationPhase (..), manifestPieceKind, parseManifest
  , KernelSlotPaths (..)
  ) where

import Control.Monad (forM, forM_, unless, when)
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KeyMap
import qualified Data.ByteString as B
import Data.List (sort)
import Data.Maybe (fromMaybe)
import Data.Scientific (toBoundedInteger)
import Data.Text (Text)
import qualified Data.Text as T
import qualified Data.Vector as V
import Replacement.StartupArtifact.AttachState (AttachRules, parseAttachRules)
import Replacement.StartupArtifact.Types

data LinkedPaths = LinkedPaths { linkedImagePath :: FilePath, linkedSymbolsPath :: FilePath } deriving (Eq, Show)

data LoaderStartupPaths = LoaderStartupPaths
  { loaderAPaths :: LinkedPaths, loaderBPaths :: LinkedPaths, dispatcherPaths :: LinkedPaths, installerPaths :: LinkedPaths }
  deriving (Eq, Show)

data ManifestPiece = SingleImagePiece PieceKind LinkedPaths | LoaderStartupPiece LoaderStartupPaths | KernelSlotPiece KernelSlotPaths
  deriving (Eq, Show)

data KernelSlotPaths = KernelSlotPaths { slotLoaderAPaths :: LinkedPaths, slotLoaderBPaths :: LinkedPaths, slotRegionsPath :: FilePath }
  deriving (Eq, Show)

manifestPieceKind :: ManifestPiece -> PieceKind
manifestPieceKind (SingleImagePiece kind _) = kind
manifestPieceKind (LoaderStartupPiece _) = LoaderStartup
manifestPieceKind (KernelSlotPiece _) = KernelSlotBodies

-- | Whether the contract carries the application section.
data ApplicationPhase = WithoutApplication | WithApplication deriving (Eq, Show)

data Manifest = Manifest
  { manifestAttach :: Label, manifestAttachRules :: AttachRules, manifestPieces :: [ManifestPiece], manifestApplication :: ApplicationPhase }
  deriving (Eq, Show)

refuse :: ManifestProblem -> Either Refusal a
refuse = Left . ManifestRefused

-- | An object with every required key and no key outside required and optional.
object :: Text -> [Text] -> [Text] -> J.Value -> Either Refusal (KeyMap.KeyMap J.Value)
object context required optional value = case value of
  J.Object fields -> do
    forM_ (KeyMap.keys fields) $ \key ->
      unless (Key.toText key `elem` required ++ optional) (refuse (ManifestUnknownKey (context <> "." <> Key.toText key)))
    forM_ required $ \key ->
      unless (KeyMap.member (Key.fromText key) fields) (refuse (ManifestMalformed (context <> " lacks " <> key)))
    pure fields
  _ -> refuse (ManifestMalformed (context <> " is not an object"))

field :: KeyMap.KeyMap J.Value -> Text -> J.Value
field fields key = fromMaybe J.Null (KeyMap.lookup (Key.fromText key) fields)

text :: Text -> J.Value -> Either Refusal Text
text _ (J.String value) | not (T.null value) = pure value
text context _ = refuse (ManifestMalformed (context <> " is not a nonempty string"))

kindOf :: Text -> Either Refusal PieceKind
kindOf name = case [kind | kind <- [minBound .. maxBound], pieceKindName kind == name] of
  [kind] -> pure kind
  _ -> refuse (ManifestUnknownKind name)

attachLabel :: Text -> Either Refusal Label
attachLabel value = case T.splitOn "/" value of
  [kindName, name] -> do
    kind <- either (const (refuse (GuardAttachUnknown value))) Right (kindOf kindName)
    either (const (refuse (GuardAttachUnknown value))) Right (label kind name)
  _ -> refuse (GuardAttachUnknown value)

parseManifest :: B.ByteString -> Either Refusal Manifest
parseManifest bytes = do
  value <- maybe (refuse (ManifestMalformed "not JSON")) Right (J.decodeStrict bytes)
  top <- object "manifest" ["schema", "guard", "pieces"] ["application"] value
  case field top "schema" of
    J.Number number | toBoundedInteger number == Just (4 :: Int) -> pure ()
    _ -> refuse ManifestSchemaUnsupported
  guard <- object "guard" ["attach", "attach_state"] [] (field top "guard")
  attach <- text "guard.attach" (field guard "attach") >>= attachLabel
  rules <- parseAttachRules (field guard "attach_state")
  rows <- case field top "pieces" of
    J.Array items | not (V.null items) -> pure (V.toList items)
    _ -> refuse (ManifestMalformed "pieces is not a nonempty array")
  pieces <- forM rows $ \row -> do
    piece <- object "piece" ["kind", "inputs"] [] row
    kind <- text "piece.kind" (field piece "kind") >>= kindOf
    let linked context entry = do
          inputs <- object context ["image", "symbols"] [] entry
          image <- text (context <> ".image") (field inputs "image")
          symbols <- text (context <> ".symbols") (field inputs "symbols")
          pure (LinkedPaths (T.unpack image) (T.unpack symbols))
    case kind of
      LoaderStartup -> do
        inputs <- object "piece.inputs" ["loader_a", "loader_b", "dispatcher", "installer"] [] (field piece "inputs")
        let side name = linked ("piece.inputs." <> name) (field inputs name)
        paths <- LoaderStartupPaths <$> side "loader_a" <*> side "loader_b" <*> side "dispatcher" <*> side "installer"
        pure (LoaderStartupPiece paths)
      KernelSlotBodies -> do
        inputs <- object "piece.inputs" ["loader_a", "loader_b", "regions"] [] (field piece "inputs")
        let side name = linked ("piece.inputs." <> name) (field inputs name)
        regions <- text "piece.inputs.regions" (field inputs "regions")
        paths <- KernelSlotPaths <$> side "loader_a" <*> side "loader_b" <*> pure (T.unpack regions)
        pure (KernelSlotPiece paths)
      _ -> SingleImagePiece kind <$> linked "piece.inputs" (field piece "inputs")
  let kinds = sort (map manifestPieceKind pieces)
  forM_ (zip kinds (drop 1 kinds)) $ \(left, right) -> when (left == right) (Left (DuplicatePieceKind left))
  phase <- case KeyMap.lookup "application" top of
    Nothing -> pure WithoutApplication
    Just section -> WithApplication <$ object "application" [] [] section
  pure (Manifest attach rules pieces phase)
