{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import Control.Monad (unless, when)
import Data.Aeson (Value (..), eitherDecodeStrict', encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Data.Text as T
import Emulation.ModalObservation (SettingsWrite, compareSettingsWrites, completedSettingsWrites, parseRecordedModalBeginRequest, settingsWritesJson, stockSettingsWriters)
import Emulation.ModalWatchStream (readFinalModalWatch)
import Emulation.Sha256 (sha256File, sha256Hex)
import Emulation.WatchObservation (watchRangeBounds)
import Data.Scientific (Scientific, floatingOrInteger)
import System.Directory (doesPathExist)
import System.Environment (getArgs, lookupEnv)
import System.Exit (die, exitFailure)
import System.FilePath (isAbsolute, takeFileName, (</>))

data Role = Role
  { roleScoped :: Bool
  , roleDirectory :: FilePath
  , roleWrites :: [SettingsWrite]
  , roleSettingsWriters :: Value
  , roleWriterBounds :: [(Integer, Integer)]
  , roleRuntime :: (Integer, Integer)
  , roleSettingsReloads :: Value
  , roleSegments :: Value
  , roleFinalDigest :: String
  , rolePackage :: Value
  }

main :: IO ()
main = do
  arguments <- getArgs
  (stockPath, ownedPath, outputPath) <- case arguments of
    [stock, owned, output] -> pure (stock, owned, output)
    _ -> die "usage: modal-settings-compare STOCK_RECEIPT_DIRECTORY OWNED_RECEIPT_DIRECTORY NEW_OUTPUT_JSON"
  workspace <- lookupEnv "BUILD_WORKSPACE_DIRECTORY"
  let resolve path = if isAbsolute path then path else maybe path (</> path) workspace
  exists <- doesPathExist (resolve outputPath)
  when exists (die "modal-settings-compare: the output path already exists")
  stock <- loadRole False (resolve stockPath) >>= either (die . ("modal-settings-compare: stock: " ++)) pure
  owned <- loadRole True (resolve ownedPath) >>= either (die . ("modal-settings-compare: owned: " ++)) pure
  let outcome = do
        unless (roleSettingsReloads stock == roleSettingsReloads owned) (Left "the roles declare different settings reloads")
        unless (roleSegments stock == roleSegments owned) (Left "the roles pressed different keys or observed different menu states")
        checkWriterIdentity stock
        checkWriterIdentity owned
        compareSettingsWrites (roleWrites stock) (roleWrites owned)
      side role = object
        [ "receipt" .= roleDirectory role, "watch_final_sha256" .= roleFinalDigest role
        , "settings_writers" .= roleSettingsWriters role, "settings_reloads" .= roleSettingsReloads role
        , "segments" .= roleSegments role, "owned_package" .= rolePackage role
        , "writes" .= settingsWritesJson (roleWrites role) ]
  B.writeFile (resolve outputPath) (BL.toStrict (encode (object
    [ "schema" .= (1 :: Int), "stock" .= side stock, "owned" .= side owned
    , "equal" .= either (const False) (const True) outcome
    , "problem" .= either Just (const Nothing) outcome ])) <> "\n")
  either (const exitFailure) pure outcome

field :: K.Key -> Value -> Either String Value
field key (Object fields) = maybe (Left ("missing " ++ K.toString key)) Right (KM.lookup key fields)
field key _ = Left ("not an object at " ++ K.toString key)

text :: K.Key -> Value -> Either String T.Text
text key value = field key value >>= \entry -> case entry of
  String content -> Right content
  _ -> Left (K.toString key ++ " is not text")

-- | Re-derives one role's settings writes from its request and the final watch its
-- observation receipt binds; the completed lifetime check and every segment must
-- have passed, and the receipt must be the requested role.
loadRole :: Bool -> FilePath -> IO (Either String Role)
loadRole scoped directory = do
  observationBytes <- B.readFile (directory </> "e2e-modal-observation.json")
  resultBytes <- B.readFile (directory </> "e2e-result.json")
  sessionBytes <- B.readFile (directory </> "emulator-session.json")
  let packagePath = directory </> "e2e-modal-owned-package.json"
  packaged <- doesPathExist packagePath
  packageBytes <- if packaged then Just <$> B.readFile packagePath else pure Nothing
  let binding = do
        observation <- eitherDecodeStrict' observationBytes
        result <- eitherDecodeStrict' resultBytes
        problem <- field "observation_error" observation
        unless (problem == Null) (Left "the completed lifetime check did not pass")
        requestValue <- field "request" observation
        request <- parseRecordedModalBeginRequest observation
        ownedExtent <- field "owned" requestValue
        unless ((ownedExtent /= Null) == scoped && packaged == scoped)
          (Left (if scoped then "the receipt is not a scoped owned-package run" else "the receipt is not an unscoped stock run"))
        package <- maybe (Right Null) eitherDecodeStrict' packageBytes
        lifetime <- field "lifetime" requestValue
        writers <- field "settings_writers" lifetime
        writerBounds <- parseExtents writers
        runtime <- field "runtime" lifetime >>= parseExtent
        reloads <- field "settings_reloads" lifetime
        finalFile <- field "watch_final_file" observation
        finalName <- text "path" finalFile
        finalDigest <- text "sha256" finalFile
        segments <- field "trajectories" result
        case segments of
          Array entries | not (null entries), all (\entry -> field "result" entry == Right (String "matched")) entries -> pure ()
          _ -> Left "the receipt's trajectory segments are absent or unmatched"
        session <- eitherDecodeStrict' sessionBytes
        let list key value = field key value >>= \entries -> case entries of
              Array items -> Right (foldr (:) [] items)
              _ -> Left (K.toString key ++ " is not a list")
        captures <- list "menu_receipts" result >>= traverse (\entry -> do
          label <- field "label" entry
          menu <- field "menu" entry
          pure (object ["label" .= label, "menu" .= menu]))
        keys <- list "key_log" session >>= traverse (field "key")
        let steps = object ["captures" .= captures, "keys" .= keys]
        pure (request, writers, writerBounds, runtime, reloads, finalName, finalDigest, steps, package)
  case binding of
    Left problem -> pure (Left problem)
    Right (request, writers, writerBounds, runtime, reloads, finalName, finalDigest, steps, package) -> do
      let finalPath = directory </> takeFileName (T.unpack finalName)
      digest <- sha256Hex <$> sha256File finalPath
      parsed <- readFinalModalWatch finalPath
      pure $ do
        unless (T.unpack finalDigest == digest) (Left "the final watch file differs from its observation binding")
        final <- parsed
        writes <- completedSettingsWrites request final
        pure (Role scoped directory writes writers writerBounds runtime reloads steps digest package)

parseExtent :: Value -> Either String (Integer, Integer)
parseExtent (Array entries) = case foldr (:) [] entries of
  [Number low, Number high]
    | Just low' <- whole low, Just high' <- whole high -> Right (low', high')
  _ -> Left "an extent is not two integers"
parseExtent _ = Left "an extent is not an array"

whole :: Scientific -> Maybe Integer
whole value = case floatingOrInteger value :: Either Double Integer of
  Right integer -> Just integer
  Left _ -> Nothing

parseExtents :: Value -> Either String [(Integer, Integer)]
parseExtents (Array entries) = traverse parseExtent (foldr (:) [] entries)
parseExtents _ = Left "settings writers are not a list"

-- | Each role declares the stock setter it executes; the owned role also declares
-- the storage store extents its image exports.
checkWriterIdentity :: Role -> Either String ()
checkWriterIdentity role
  | roleScoped role = unless (take (length stockBounds) bounds == stockBounds
      && not (null ownedBounds) && all inside ownedBounds)
      (Left "the owned role's settings writers are not the stock setter plus extents inside its image")
  | otherwise = unless (bounds == stockBounds)
      (Left "the stock role's settings writer is not the stock setter")
 where
  bounds = roleWriterBounds role
  stockBounds = [(toInteger low, toInteger high) | (low, high) <- map watchRangeBounds stockSettingsWriters]
  ownedBounds = drop (length stockBounds) bounds
  (runtimeLow, runtimeHigh) = roleRuntime role
  inside (low, high) = runtimeLow <= low && low < high && high <= runtimeHigh
