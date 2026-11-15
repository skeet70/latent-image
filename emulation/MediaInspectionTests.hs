{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import Data.Either (isLeft)
import qualified Emulation.MediaInspection as Media

check :: String -> Bool -> IO ()
check label holds = unless holds (ioError (userError label))

main :: IO ()
main = do
  check "empty inspection refuses" (isLeft (Media.sectorExtent 0 0))
  check "oversized inspection refuses" (isLeft (Media.sectorExtent 0 513))
  check "past-card inspection refuses" (isLeft (Media.sectorExtent 8388607 2))
  check "wrapped address refuses" (isLeft (Media.sectorExtent maxBound 1))
  extent <- either (ioError . userError) pure (Media.sectorExtent 9 2)
  let original = B.replicate 512 17 <> B.replicate 512 34
      changed = B.take 1023 original <> B.singleton 99
  equal <- either (ioError . userError) pure (Media.inspect extent original original)
  check "identical allocated sectors match" (Media.inspectionMatches equal)
  different <- either (ioError . userError) pure (Media.inspect extent original changed)
  check "last sector slack byte is not trimmed" (not (Media.inspectionMatches different))
  case Media.inspectionValue different of
    Object fields -> do
      check "exact differing byte count" (KM.lookup "differing_bytes" fields == Just (Number 1))
      check "independent absolute mismatch witness" (KM.lookup "first_difference" fields == Just (object
        ["extent_byte_offset" .= (1023 :: Int), "card_byte_offset" .= (5631 :: Int), "original" .= (34 :: Int), "observed" .= (99 :: Int)]))
    _ -> ioError (userError "missing inspection report")
  check "short original refuses" (isLeft (Media.inspect extent (B.take 1023 original) original))
  check "short observation refuses" (isLeft (Media.inspect extent original (B.take 1023 original)))
  putStrLn "media sector inspection semantic boundaries passed"
