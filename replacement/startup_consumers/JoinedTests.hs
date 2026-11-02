{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import qualified Data.Aeson as J
import qualified Replacement.KernelInstall.Wire as K
import qualified Replacement.StartupConsumers.JoinedWire as S

assert :: String -> Bool -> IO ()
assert label condition = unless condition (ioError (userError label))
refused :: String -> Either String value -> IO ()
refused label result = assert label (case result of Left _ -> True; Right _ -> False)

main :: IO ()
main = do
  assert "absent menu remains an observed absence" ((J.eitherDecodeStrict' "{\"Ok\":null}" :: Either String (S.Observed (Maybe S.MenuCursor)))==Right (S.Observed Nothing))
  assert "actual menu cursor survives decoding" ((J.eitherDecodeStrict' "{\"Ok\":[1,5,2]}" :: Either String (S.Observed (Maybe S.MenuCursor)))==Right (S.Observed (Just (S.MenuCursor 1 5 2))))
  refused "conflicting observation variants are refused" (J.eitherDecodeStrict' "{\"Ok\":[1,5,2],\"Err\":\"failed\"}" :: Either String (S.Observed S.MenuCursor))
  assert "two actual core counters remain distinct" ((J.eitherDecodeStrict' "[7,9]" :: Either String S.CoreCounts)==Right (S.CoreCounts (K.Count 7) (K.Count 9)))
  refused "one core counter is incomplete" (J.eitherDecodeStrict' "[7]" :: Either String S.CoreCounts)
  refused "extra core counters are refused" (J.eitherDecodeStrict' "[7,9,11]" :: Either String S.CoreCounts)
  refused "negative instruction counter is refused" (J.eitherDecodeStrict' "[-1,9]" :: Either String S.CoreCounts)
  refused "standalone owned provider is not composed qualification" (J.eitherDecodeStrict' "{\"role\":\"owned\"}" :: Either String S.ProgramBinding)
  refused "unknown subobservation terminal is refused" (J.eitherDecodeStrict' "\"menu\"" :: Either String S.Termination)
  assert "artifact filenames stay inside the capture directory" (all (notElem '/' . S.relativeName . S.artifactFilename) [minBound..maxBound])
  refused "parent-relative sidecar is refused" (J.eitherDecodeStrict' "\"../watch.json\"" :: Either String S.RelativeFilename)
  refused "absolute sidecar is refused" (J.eitherDecodeStrict' "\"/watch.json\"" :: Either String S.RelativeFilename)
  refused "missing watch teardown result is not success" (J.eitherDecodeStrict' "{\"kind\":\"watch\",\"file\":{\"path\":\"watch.json\",\"sha256\":\"0000000000000000000000000000000000000000000000000000000000000000\"}}" :: Either String S.EpochEvidence)
  assert "maximum guest address remains exact" ((J.eitherDecodeStrict' "{\"address\":\"0xffffffff\",\"core\":1,\"instructions\":3}" :: Either String S.TraceHit)==Right (S.TraceHit (K.Address 0xffffffff) K.Secondary (K.Count 3)))
  refused "guest address overflow is refused" (J.eitherDecodeStrict' "{\"address\":\"0x100000000\",\"core\":1,\"instructions\":3}" :: Either String S.TraceHit)
  putStrLn "15 shared-capture wire behavior fixtures passed"
