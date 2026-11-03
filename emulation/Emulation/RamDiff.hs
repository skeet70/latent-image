{-# LANGUAGE OverloadedStrings #-}
-- | Locates guest words that one key press toggles on one screen: the key changes the word, a
-- second press restores it, and a control key pressed from the same screen leaves it unchanged.
module Emulation.RamDiff
  ( RamDiffConfig (..)
  , decodeRamDiffConfig
  , readChunk
  , rangeChunks
  , Candidate (..)
  , toggledWords
  , changedWords
  , byReturnKey
  ) where

import Control.Monad (unless)
import Data.Aeson (FromJSON (..), eitherDecodeStrict', withObject, (.:))
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftL, (.|.))
import qualified Data.ByteString as B
import Data.List (sort)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import Emulation.MenuCrawl (CrawlKey, GuestRange (..))

-- | Bytes per `emulatorReadMemory` call.
readChunk :: Int
readChunk = 65536

-- | At most 64 MiB is sampled per screen.
sampleLimit :: Integer
sampleLimit = 64 * 1024 * 1024

-- | The key path from the root to the screen, the key sought, the keys pressed after it that
-- should restore the screen, the control key pressed from the screen, and the sampled ranges.
data RamDiffConfig = RamDiffConfig
  { diffPath :: [CrawlKey]
  , diffKey :: CrawlKey
  , diffReturnKeys :: [CrawlKey]
  , diffControl :: CrawlKey
  , diffWindows :: [GuestRange]
  } deriving (Eq, Show)

instance FromJSON RamDiffConfig where
  parseJSON = withObject "ram diff config" $ \document -> do
    unless (sort (KM.keys document) == ["control", "key", "path", "return_keys", "schema", "windows"])
      (fail "ram diff config takes exactly schema, path, key, return_keys, control and windows")
    schema <- document .: "schema"
    unless (schema == (1 :: Int)) (fail "unsupported ram diff config schema")
    config <- RamDiffConfig <$> document .: "path" <*> document .: "key" <*> document .: "return_keys" <*> document .: "control" <*> document .: "windows"
    unless (diffKey config /= diffControl config) (fail "the control key must differ from the key")
    unless (not (null (diffReturnKeys config)) && length (diffReturnKeys config) == length (S.fromList (diffReturnKeys config)))
      (fail "return keys must be nonempty and distinct")
    let windows = sort [(rangeStart window, rangeEnd window) | window <- diffWindows config]
    unless (not (null windows) && and (zipWith (\(_, end) (start, _) -> end <= start) windows (drop 1 windows)))
      (fail "windows must be nonempty and disjoint")
    unless (sum [toInteger end - toInteger start | (start, end) <- windows] <= sampleLimit) (fail "windows exceed 64 MiB")
    pure config

decodeRamDiffConfig :: B.ByteString -> Either String RamDiffConfig
decodeRamDiffConfig = eitherDecodeStrict'

-- | The read calls covering a range, in address order.
rangeChunks :: GuestRange -> [(Word32, Int)]
rangeChunks (GuestRange start end) =
  [(address, fromIntegral (min (toInteger readChunk) (toInteger end - toInteger address))) | address <- [start, start + fromIntegral readChunk .. end - 1]]

-- | A word the key changed, a second press restored and the control press left unchanged.
data Candidate = Candidate { candidateAddress :: Word32, candidateBefore :: Word32, candidateAfterKey :: Word32 }
  deriving (Eq, Show)

words32 :: B.ByteString -> [Word32]
words32 bytes = [word (B.drop offset bytes) | offset <- [0, 4 .. B.length bytes - 4]]
 where
  word chunk = foldr (\index value -> (value `shiftL` 8) .|. fromIntegral (B.index chunk index)) 0 [0 .. 3]

-- | Samples are one range's bytes before the key, after one press, after a second press and after
-- the control press from the starting screen.
toggledWords :: GuestRange -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> [Candidate]
toggledWords range before once twice control =
  [ Candidate address b o
  | (address, b, o, t, c) <- zip5 [rangeStart range, rangeStart range + 4 ..] (words32 before) (words32 once) (words32 twice) (words32 control)
  , b /= o, t == b, c == b ]
 where
  zip5 (a : as) (b : bs) (c : cs) (d : ds) (e : es) = (a, b, c, d, e) : zip5 as bs cs ds es
  zip5 _ _ _ _ _ = []

-- | The number of words that differ between two samples of one range.
changedWords :: B.ByteString -> B.ByteString -> Int
changedWords left right = length (filter id (zipWith (/=) (words32 left) (words32 right)))

-- | Each candidate address with the return keys under which it is a candidate, in key order.
byReturnKey :: [(CrawlKey, [Candidate])] -> [(Candidate, [CrawlKey])]
byReturnKey perKey = M.elems (M.fromListWith (\(candidate, later) (_, earlier) -> (candidate, earlier ++ later))
  [(candidateAddress candidate, (candidate, [key])) | (key, candidates) <- perKey, candidate <- candidates])
