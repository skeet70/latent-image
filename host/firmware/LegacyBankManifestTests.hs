{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (FromJSON (..), eitherDecodeStrict', withObject, (.:), (.:?))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import System.Environment (getArgs)

newtype Digest = Digest T.Text deriving (Eq, Show)
instance FromJSON Digest where parseJSON value = Digest <$> parseJSON value
newtype PublicationIndex = PublicationIndex Int
instance FromJSON PublicationIndex where
  parseJSON = withObject "publication" $ \fields -> PublicationIndex <$> fields .: "index"
data LegacyManifest = LegacyManifest
  { manifestIdentity :: M.Map FilePath Digest
  , manifestPublications :: [PublicationIndex]
  , manifestVariant :: Maybe T.Text
  }
instance FromJSON LegacyManifest where
  parseJSON = withObject "legacy bank manifest" $ \fields -> LegacyManifest
    <$> fields .: "source_identity" <*> fields .: "publications" <*> fields .:? "bank_variant"

data LegacyVariant = Lifecycle | ModalLifecycle
publicationCount :: LegacyVariant -> Int
publicationCount Lifecycle = 226
publicationCount ModalLifecycle = 227

main :: IO ()
main = do
  paths <- getArgs
  variants <- case paths of
    [lifecycle,modalLifecycle] -> pure [(Lifecycle,lifecycle),(ModalLifecycle,modalLifecycle)]
    _ -> fail "usage: legacy-bank-manifest-tests LIFECYCLE_MANIFEST MODAL_LIFECYCLE_MANIFEST"
  forM_ variants $ \(variant,path) -> do
    manifest <- B.readFile path >>= either fail pure . eitherDecodeStrict'
    unless (length (manifestPublications manifest) == publicationCount variant && manifestVariant manifest == Nothing)
      (fail "legacy lifecycle publication identity differs")
    unless (M.size (manifestIdentity manifest) > 68 && all (`M.member` manifestIdentity manifest) lifecycleSources)
      (fail "legacy lifecycle source identity was truncated or grouped")
    forM_ (M.elems (manifestIdentity manifest)) $ \(Digest digest) ->
      unless (T.length digest == 64 && T.all (`elem` ("0123456789abcdef" :: String)) digest)
        (fail "legacy lifecycle source digest is malformed")
    putStrLn ("legacy manifest emitted: " ++ show (publicationCount variant) ++ " publications, " ++ show (M.size (manifestIdentity manifest)) ++ " ungrouped source identities")
 where
  lifecycleSources =
    [ "replacement/save/src/lib.rs", "replacement/save/src/abi.rs", "replacement/save/src/contract.rs", "replacement/save/src/facade.rs"
    , "replacement/save/src/ports.rs", "replacement/save/save_stock.S", "replacement/save/save_abi.h"
    , "replacement/save/stock_calls.json", "replacement/media_permit/src/lib.rs"
    , "replacement/media_permit/src/stock.rs", "replacement/media_permit/rb_media_permit_stock.S"
    , "replacement/media_permit/stock_calls.json"
    ]
