{-# LANGUAGE OverloadedStrings #-}
-- | Trajectory files for the emulator e2e: menu segments, the guest windows read after them
-- and the code sites traced while they run.
module Emulation.UiTrajectory
  ( MenuState (..)
  , ReadAddress (..)
  , Segment (..)
  , TraceSite (..)
  , Trajectories (..)
  , phaseNames
  , decodeTrajectories
  ) where

import Control.Monad (unless, when)
import Data.Aeson (FromJSON (..), eitherDecodeStrict', withObject, (.!=), (.:), (.:?))
import qualified Data.ByteString as B
import Data.List (nub, stripPrefix)
import qualified Data.Text as T
import Data.Word (Word32)
import EmulatorUsb (ModeDial (..))
import Numeric (readHex)

data MenuState = MenuState Int Int Int deriving (Eq, Show)

-- | A read window's start: a fixed guest address, or a symbol of the relinked runtime.
data ReadAddress = FixedAddress Word32 | RuntimeSymbol String deriving (Eq, Show)

-- | One trajectory segment: its phase name, the mode-dial detent set before its first key, the
-- expected menu state after each key, the message ids the menu message drawer is entered with during
-- the segment (none when absent), and an optional capture label taken after the last key. An
-- observing segment records each key's actual state instead of requiring the expected one; `reads`
-- are guest RAM windows recorded after the last key.
data Segment = Segment
  { segmentPhase :: T.Text
  , segmentModeDial :: Maybe ModeDial
  , segmentCapture :: Maybe String
  , segmentSteps :: [(String, MenuState)]
  , segmentMessages :: [Word32]
  , segmentObserve :: Bool
  , segmentReads :: [(String, ReadAddress, Int)]
  , segmentRequires :: [T.Text]
  }

instance FromJSON Segment where
  parseJSON = withObject "segment" $ \segment -> Segment
    <$> segment .: "phase"
    <*> (traverse modeDial =<< segment .:? "mode_dial")
    <*> segment .:? "capture"
    <*> (map step <$> segment .: "steps")
    <*> segment .:? "messages" .!= []
    <*> segment .:? "observe" .!= False
    <*> (mapM window =<< segment .:? "reads" .!= [])
    <*> segment .:? "requires" .!= []
    where
      step (key, menu, setting, cursor) = (key, MenuState menu setting cursor)
      modeDial position = case (position :: T.Text) of
        "M" -> pure ModeDialManual
        _ -> fail ("unsupported mode-dial position: " ++ T.unpack position)
      window (label, address, len) = case T.unpack address of
        text | Just symbol <- stripPrefix "runtime:" text, not (null symbol) -> pure (label, RuntimeSymbol symbol, len)
             | [(value, "")] <- readHex text -> pure (label, FixedAddress value, len)
             | otherwise -> fail ("read address must be hex without 0x or runtime:SYMBOL: " ++ text)

-- | An A32 instruction address whose every entry the bridge records with r0-r3, under a label.
data TraceSite = TraceSite { traceSiteAddress :: Word32, traceSiteLabel :: String } deriving (Eq, Show)

-- | Labels the e2e arms itself.
reservedLabels :: [String]
reservedLabels = ["menu-message", "d3-hook"]

instance FromJSON TraceSite where
  parseJSON value = do
    (address, label) <- parseJSON value
    site <- case readHex (T.unpack address) of
      [(word, "")] | T.length address == 8 -> pure word
      _ -> fail ("trace site address must be eight hex digits without 0x: " ++ T.unpack address)
    when (site `mod` 4 /= 0) (fail ("trace site address must be word aligned: " ++ T.unpack address))
    when (null label || label `elem` reservedLabels) (fail ("trace site label is empty or reserved: " ++ label))
    pure (TraceSite site label)

data Trajectories = Trajectories
  { beforeUnload :: [Segment]
  , afterUnload :: [Segment]
  , trajectoryTraceSites :: [TraceSite]
  }

instance FromJSON Trajectories where
  parseJSON = withObject "trajectories" $ \document -> do
    schema <- document .: "schema"
    unless (schema == (1 :: Int)) (fail "unsupported trajectory schema")
    sites <- document .:? "trace_sites" .!= []
    unless (distinct (map traceSiteAddress sites) && distinct (map traceSiteLabel sites))
      (fail "trace sites must have distinct addresses and labels")
    Trajectories <$> document .: "before_unload" <*> document .: "after_unload" <*> pure sites
    where
      distinct items = length (nub items) == length items

phaseNames :: Trajectories -> [T.Text]
phaseNames trajectories = nub (map segmentPhase (beforeUnload trajectories ++ afterUnload trajectories))

-- | Decodes a trajectory file and checks that every required phase names a segment.
decodeTrajectories :: B.ByteString -> Either String Trajectories
decodeTrajectories bytes = do
  trajectories <- eitherDecodeStrict' bytes
  case [ phase | entry <- beforeUnload trajectories ++ afterUnload trajectories
               , phase <- segmentRequires entry, phase `notElem` phaseNames trajectories ] of
    [] -> Right trajectories
    phase : _ -> Left ("trajectory requires an unknown segment: " ++ T.unpack phase)
