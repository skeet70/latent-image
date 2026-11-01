{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (unless)
import Data.Aeson (Value (..), eitherDecodeStrict', encode)
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.Either (isLeft)
import qualified Emulation.InterfaceInitializer as I
import qualified Emulation.InterfaceInitializerConsumer as C
import System.Environment (getArgs)

withDispatchLr :: Integer -> Value -> Either String Value
withDispatchLr lr (Object receipt) = case KM.lookup "consumer" receipt of
  Just (Object consumer) -> case KM.lookup "dispatch" consumer of
    Just (Object dispatch) -> pure (Object (KM.insert "consumer"
      (Object (KM.insert "dispatch" (Object (KM.insert "lr" (Number (fromInteger lr)) dispatch)) consumer)) receipt))
    _ -> Left "fixture has no dispatch context"
  _ -> Left "fixture has no consumer evidence"
withDispatchLr _ _ = Left "fixture is not a receipt object"

main :: IO ()
main = do
  arguments <- getArgs
  (sourcePath, receiptPath) <- case arguments of
    [source, receipt] -> pure (source, receipt)
    _ -> fail "usage: interface-initializer-consumer-regression ORIGINAL_DAT COMPLETE_SCHEMA7_RECEIPT"
  source <- B.readFile sourcePath
  receiptBytes <- B.readFile receiptPath
  initializer <- either fail pure (I.deriveContract I.Retained source source B.empty B.empty)
  contract <- either fail pure (C.deriveContract initializer)
  fixture <- either fail pure (eitherDecodeStrict' receiptBytes)
  correct <- either fail pure (withDispatchLr 0x412c8 fixture)
  stale <- either fail pure (withDispatchLr 0x7235f8 fixture)
  let validate = C.validateReceipt contract . L.toStrict . encode
  either fail pure (validate correct)
  unless (isLeft (validate stale)) (fail "consumer admitted the caller BLX return address as live LR")
  putStrLn "consumer dispatch LR receipt regressions: pass"
