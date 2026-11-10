{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson ((.:), withObject)
import Data.Aeson.Types (parseMaybe)
import qualified Data.ByteString as B
import Data.Either (isLeft)
import Emulation.CpuWorkerSource
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments <- getArgs
  image4 <- case arguments of
    [path] -> B.readFile path
    _ -> fail "usage: cpu-worker-source-tests ADMITTED_FULL02_IMAGE4"
  source <- either fail pure (bindCpuWorkerSource image4)
  let claim = parseMaybe (withObject "worker source" $ \fields -> do
        bound <- fields .: "instruction_source_bound"
        delivered <- fields .: "runtime_delivery_observed"
        installed <- fields .: "installed_lifetime_admitted"
        total <- fields .: "total"
        implementation <- fields .: "implementation"
        request <- fields .: "request"
        (kind, size) <- withObject "request" (\requestFields -> (,) <$> requestFields .: "message_kind" <*> requestFields .: "payload_bytes") request
        pure (bound, delivered, installed, total, kind, size, implementation)) (cpuWorkerSourceValue source)
  check "worker source claim differs" (claim == Just (True, False, False, False, 0x6800 :: Int, 28 :: Int, "RetainedProprietaryWorkerAndDispatch" :: String))
  check "different application source accepted" (isLeft (bindCpuWorkerSource (mutate 0 image4)))
  check "different submission producer accepted" (isLeft (bindCpuWorkerSource (mutate (0x6b090c - 0x61c400) image4)))
  check "different worker callback consumer accepted" (isLeft (bindCpuWorkerSource (mutate (0x6c6b5c - 0x61c400) image4)))
  check "truncated source accepted" (isLeft (bindCpuWorkerSource (B.drop 1 image4)))
  putStrLn "worker source identity controls passed"
  where
    check name condition = unless condition (fail name)
    mutate offset bytes = B.take offset bytes <> B.singleton (B.index bytes offset + 1) <> B.drop (offset + 1) bytes
