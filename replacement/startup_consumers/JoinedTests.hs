{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString.Lazy as L
import Numeric (showHex)
import qualified Replacement.KernelInstall.Wire as K
import qualified Replacement.StartupConsumers.JoinedWire as S

assert :: String -> Bool -> IO ()
assert label condition = unless condition (ioError (userError label))
refused :: String -> Either String value -> IO ()
refused label result = assert label (case result of Left _ -> True; Right _ -> False)

main :: IO ()
main = do
  let registers = KM.fromList [(Key.fromString ("r"++show index),J.toJSON ("0x"++showHex index "")) | index <- [0::Int ..12]]
      trace pc bank = L.toStrict (J.encode (J.object ["address" J..= pc,"core" J..= (1::Int),"instructions" J..= (3::Int)
        ,"registers" J..= J.Object bank,"sp" J..= ("0x4000"::String),"lr" J..= ("0x2000"::String),"cpsr" J..= (0x13::Int)]))
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
  assert "full maximum-address trace context remains exact" (case J.eitherDecodeStrict' (trace ("0xffffffff"::String) registers) of
    Right hit -> S.hitPc hit==K.Address 0xffffffff && S.hitCore hit==K.Secondary && S.hitInstructions hit==K.Count 3
      && S.registerWords (S.hitRegisters hit)==map K.WordValue [0..12] && S.hitSp hit==K.Address 0x4000
      && S.hitLr hit==K.Address 0x2000 && S.hitCpsr hit==K.WordValue 0x13
    Left _ -> False)
  refused "guest address overflow is refused" (J.eitherDecodeStrict' (trace ("0x100000000"::String) registers) :: Either String S.TraceHit)
  refused "missing r12 is not a complete trace context" (J.eitherDecodeStrict' (trace ("0xffffffff"::String) (KM.delete "r12" registers)) :: Either String S.TraceHit)
  refused "nonhexadecimal PC is refused" (J.eitherDecodeStrict' (trace ("4294967295"::String) registers) :: Either String S.TraceHit)
  putStrLn "17 shared-capture wire behavior fixtures passed"
