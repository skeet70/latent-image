{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module SourceAdmission where

import Control.Monad (unless)
import Data.Aeson (FromJSON (..), eitherDecodeStrict', withObject, withText, (.:))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (Object, Parser)
import qualified Data.ByteString as B
import Data.Char (isHexDigit, isLower, isDigit)
import Data.List (sort)
import qualified Data.Text as T
import Data.Word (Word32)
import Scenarios (Address (..))

newtype Digest = Digest { digestText :: String } deriving (Eq,Show)
newtype Revision = Revision String deriving (Eq,Show)
newtype ByteCount = ByteCount { byteCount :: Word32 } deriving (Eq,Show)
newtype Instruction = Instruction { instructionWord :: Word32 } deriving (Eq,Show)
newtype StockName = StockName { stockNameText :: String } deriving (Eq,Show)
newtype Invocation = Invocation String deriving (Eq,Show)
newtype Scope = Scope String deriving (Eq,Show)
newtype Premise = Premise String deriving (Eq,Show)
data Firmware = Xm1_150 deriving (Eq,Show)
data Extent = Extent { extentAddress :: Address, extentSize :: ByteCount, extentDigest :: Digest } deriving (Eq,Show)
data StockFunction = StockFunction { functionExtent :: Extent, functionName :: StockName } deriving (Eq,Show)
data Callsite = Callsite { callPc :: Address, callTarget :: Address, callInstruction :: Instruction } deriving (Eq,Show)
data Ownership = Owned deriving (Eq,Show)
data Binding = Binding { bindingAddress :: Address, bindingSymbol :: StockName, bindingOwnership :: Ownership } deriving (Eq,Show)
data Region = Region Address ByteCount deriving (Eq,Show)
data Footprint = Footprint
  { contextRegion :: Region, stateRegion :: Region, stackRegion :: Region, candidateRegion :: Region
  , stockRegion :: Region, globalsRegion :: Region, observerExit :: Address, frameRegion :: Region
  , lowServiceRegion :: Region, helperRegion :: Region, storageRegion :: Region
  } deriving (Eq,Show)
data SourceAdmission = SourceAdmission
  { sourceFirmware :: Firmware, sourceRevision :: Revision, sourceImageDigest :: Digest, sourceImageBase :: Address
  , sourceFunctions :: [StockFunction], sourceTables :: [Extent], sourceCalls :: [Callsite]
  , sourceInvocation :: Invocation, sourceFootprint :: Footprint, sourceScope :: Scope, sourcePremises :: [Premise]
  , sourceBindings :: [Binding]
  } deriving (Eq,Show)

closed :: Object -> [String] -> Parser ()
closed object fields = unless (sort (map K.toString (KM.keys object)) == sort fields) (fail "source admission fields differ")
hexText :: Int -> T.Text -> Parser String
hexText count text = do
  let value = T.unpack text
  unless (length value == count && all (\character -> isHexDigit character && (isDigit character || isLower character)) value) (fail "invalid lowercase hexadecimal identity")
  pure value
instance FromJSON Digest where parseJSON = withText "SHA-256" (fmap Digest . hexText 64)
instance FromJSON Revision where parseJSON = withText "source revision" (fmap Revision . hexText 40)
instance FromJSON ByteCount where
  parseJSON value = do
    size <- parseJSON value
    unless (size > 0) (fail "empty stock extent")
    pure (ByteCount size)
instance FromJSON Instruction where parseJSON value = Instruction <$> parseJSON value
instance FromJSON StockName where parseJSON = withText "stock symbol" (pure . StockName . T.unpack)
instance FromJSON Invocation where parseJSON = withText "invocation" (pure . Invocation . T.unpack)
instance FromJSON Scope where parseJSON = withText "scope" (pure . Scope . T.unpack)
instance FromJSON Premise where parseJSON = withText "premise" (pure . Premise . T.unpack)
instance FromJSON Firmware where
  parseJSON = withText "firmware" $ \text -> if text == "xm1-150" then pure Xm1_150 else fail "unsupported firmware"
instance FromJSON Extent where
  parseJSON = withObject "extent" $ \object -> do
    closed object ["address","size","sha256"]
    Extent <$> object .: "address" <*> object .: "size" <*> object .: "sha256"
instance FromJSON StockFunction where
  parseJSON = withObject "stock function" $ \object -> do
    closed object ["address","size","sha256","name"]
    extent <- Extent <$> object .: "address" <*> object .: "size" <*> object .: "sha256"
    StockFunction extent <$> object .: "name"
instance FromJSON Callsite where
  parseJSON = withObject "callsite" $ \object -> do
    closed object ["pc","target","instruction"]
    Callsite <$> object .: "pc" <*> object .: "target" <*> object .: "instruction"
instance FromJSON Ownership where
  parseJSON = withText "ownership" $ \text -> if text == "owned" then pure Owned else fail "unsupported ownership"
instance FromJSON Binding where
  parseJSON = withObject "publication" $ \object -> do
    closed object ["stock_address","owned_symbol","ownership"]
    Binding <$> object .: "stock_address" <*> object .: "owned_symbol" <*> object .: "ownership"
instance FromJSON Region where
  parseJSON value = do
    words' <- parseJSON value :: Parser [Word32]
    case words' of
      [address,size] | size > 0 -> pure (Region (Address address) (ByteCount size))
      _ -> fail "region must contain an address and a positive byte count"
instance FromJSON Footprint where
  parseJSON = withObject "footprint" $ \object -> do
    closed object ["context","state","stack","candidate","stock","globals","exit","frames","service_low","service_helper","storage_models"]
    Footprint <$> object .: "context" <*> object .: "state" <*> object .: "stack" <*> object .: "candidate" <*> object .: "stock" <*> object .: "globals" <*> object .: "exit" <*> object .: "frames" <*> object .: "service_low" <*> object .: "service_helper" <*> object .: "storage_models"
instance FromJSON SourceAdmission where
  parseJSON = withObject "source admission" $ \object -> do
    closed object ["firmware","decomp_commit","image5_sha256","image_base","functions","tables","calltable","invocation","footprint","scope","premises","publications"]
    SourceAdmission <$> object .: "firmware" <*> object .: "decomp_commit" <*> object .: "image5_sha256" <*> object .: "image_base" <*> object .: "functions" <*> object .: "tables" <*> object .: "calltable" <*> object .: "invocation" <*> object .: "footprint" <*> object .: "scope" <*> object .: "premises" <*> object .: "publications"

parseAdmission :: B.ByteString -> Either String SourceAdmission
parseAdmission = eitherDecodeStrict'
