{-# LANGUAGE OverloadedStrings #-}
module RecoveryStageReport
  ( StageRecord (..)
  , StageEvidence (..)
  , decodeStageReports
  , renderStageEvidence
  ) where

import Data.Bits (complement, shiftL, shiftR, (.&.))
import qualified Data.ByteString as B
import Data.Word (Word32)


data StageRecord = StageRecord
  { stageIndex :: Word32
  , stageEvidenceWord :: Word32
  , stageModeWord :: Word32
  , stageReadyWord :: Word32
  , stageCcsWord :: Word32
  , stagePresentWord :: Word32
  , stageClockWord :: Word32
  , stageNormalStatusWord :: Word32
  , stageErrorStatusWord :: Word32
  } deriving (Eq, Show)

data StageEvidence
  = StageAbsent Word32
  | StageInvalid Word32 String
  | StageValid StageRecord
  deriving (Eq, Show)

word32 :: B.ByteString -> Int -> Word32
word32 bytes offset = foldl' (\value index -> value + fromIntegral (B.index bytes (offset+index)) `shiftL` (8*index)) 0 [0..3]

stageName :: Word32 -> String
stageName stage = case stage of
  0 -> "payload-entry"
  1 -> "native-load-complete"
  2 -> "tap-restored"
  3 -> "native-close-complete"
  _ -> "unknown-stage"

decodeStage :: Word32 -> B.ByteString -> StageEvidence
decodeStage expected bytes
  | B.length bytes /= 512 = StageInvalid expected "sector length is not 512"
  | B.all (== 0) bytes = StageAbsent expected
  | B.take 8 bytes /= "XM1STAGE" = StageInvalid expected "magic mismatch"
  | word32 bytes 8 /= 1 = StageInvalid expected "version mismatch"
  | word32 bytes 12 /= expected = StageInvalid expected "slot/stage mismatch"
  | word32 bytes 16 /= complement expected = StageInvalid expected "stage complement mismatch"
  | word32 bytes 504 /= 0x504d4f43 = StageInvalid expected "completion marker absent"
  | foldl' (+) 0 [word32 bytes offset | offset <- [0,4..508]] /= 0 = StageInvalid expected "checksum mismatch"
  | otherwise = StageValid StageRecord
      { stageIndex = expected
      , stageEvidenceWord = word32 bytes 24
      , stageModeWord = word32 bytes 28
      , stageReadyWord = word32 bytes 32
      , stageCcsWord = word32 bytes 36
      , stagePresentWord = word32 bytes 40
      , stageClockWord = word32 bytes 44
      , stageNormalStatusWord = word32 bytes 48
      , stageErrorStatusWord = word32 bytes 52
      }

decodeStageReports :: [B.ByteString] -> [StageEvidence]
decodeStageReports sectors = zipWith decodeStage [0..] sectors

renderStageEvidence :: StageEvidence -> String
renderStageEvidence evidence = case evidence of
  StageAbsent stage -> stageName stage ++ ": absent"
  StageInvalid stage reason -> stageName stage ++ ": invalid: " ++ reason
  StageValid record -> stageName (stageIndex record) ++ ": valid"
    ++ "; evidence=0x" ++ hex8 (stageEvidenceWord record)
    ++ "; mode=0x" ++ hex8 (stageModeWord record)
    ++ "; ready=0x" ++ hex8 (stageReadyWord record)
    ++ "; ccs=0x" ++ hex8 (stageCcsWord record)
    ++ "; present=0x" ++ hex8 (stagePresentWord record)
    ++ "; clock=0x" ++ hex8 (stageClockWord record)
    ++ "; normal-status=0x" ++ hex8 (stageNormalStatusWord record)
    ++ "; error-status=0x" ++ hex8 (stageErrorStatusWord record)
  where
    hex8 value = [digits !! fromIntegral ((value `shiftR` shift) .&. 0xf) | shift <- [28,24..0]]
    digits = "0123456789abcdef"
