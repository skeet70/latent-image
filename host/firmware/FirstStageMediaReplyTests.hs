{-# LANGUAGE OverloadedStrings #-}
-- The oracle consumer's reply parser: every model failure code is a typed refusal.
module Main (main) where

import Control.Monad (unless)
import Data.Either (isLeft)
import FirstStageMediaReply

check :: String -> Bool -> IO ()
check label passed = unless passed (fail label)

main :: IO ()
main = do
  let capacity = "{\"status\":\"refused\",\"record_bytes\":1336679,\"failure_code\":\"output-capacity\"}"
  case parseModelReply capacity of
    Left message -> fail message
    Right reply -> do
      check "capacity reply is a refusal" (replyStatus reply == Refused)
      check "capacity reply records the input bytes" (replyRecordBytes reply == ByteCount 1336679)
      check "capacity reply has no decoded length" (replyDecodedBytes reply == Nothing)
      check "capacity is a typed failure" (replyFailure reply == Just OutputCapacity)
  let decoded = "{\"status\":\"decoded\",\"record_bytes\":1336679,\"decoded_length\":2631680}"
  case parseModelReply decoded of
    Left message -> fail message
    Right reply -> do
      check "decoded reply" (replyStatus reply == Decoded)
      check "decoded length" (replyDecodedBytes reply == Just (ByteCount 2631680))
      check "decoded reply has no failure" (replyFailure reply == Nothing)
  check "an unknown failure code is refused" $
    isLeft (parseModelReply "{\"status\":\"refused\",\"record_bytes\":1,\"failure_code\":\"no-such\"}")
  check "a capacity reply over the byte bound is refused" $
    isLeft (parseModelReply "{\"status\":\"refused\",\"record_bytes\":16777217,\"failure_code\":\"output-capacity\"}")
