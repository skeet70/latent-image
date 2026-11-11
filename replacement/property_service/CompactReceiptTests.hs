{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module CompactReceiptTests (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (Value (..), eitherDecodeStrict', object, toJSON, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import Data.Either (isLeft)
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import qualified Emulation.NativeCase as C
import qualified Emulation.NativeObservation as N
import Emulation.Sha256 (Sha256, sha256, sha256Hex)
import FujiAF.Host (get, list, str)
import qualified Replacement.PropertyService.Oracle as O
import Replacement.PropertyService.CompactReceipt

check :: String -> Bool -> IO ()
check label passed = unless passed (fail label)

set :: String -> Value -> Value -> Value
set name value (Object fields) = Object (KM.insert (K.fromString name) value fields)
set _ _ _ = error "object fixture required"

text :: String -> Value
text = String . T.pack

digest :: Sha256
digest = sha256 "receipt-test"
identity :: [(String, Value)]
identity = [("case_sha256", text (sha256Hex digest)), ("wire_sha256", text (sha256Hex digest)), ("artifacts", toArtifacts)]
  where toArtifacts = toJSONArtifacts

toJSONArtifacts :: Value
toJSONArtifacts = objectArray [object ["id" .= ("code" :: String), "path" .= ("code.bin" :: String), "size" .= (4 :: Int), "sha256" .= sha256Hex digest]]

objectArray :: [Value] -> Value
objectArray = toJSON

native :: [O.WriteEvent] -> N.NativeObservation
native writes = N.NativeObservation N.Returned (M.fromList [(register, 7) | register <- N.registerLabels])
  [(ident, B.replicate size 0) | (ident, _, size) <- O.comparedRegions]
  (N.Complete (N.ProvenanceTotal (fromIntegral (length writes))) [N.Write (N.GuestAddress (fromIntegral (O.eventAddress event))) (N.WriteBytes (fromIntegral (O.eventWidth event))) | event <- writes])
  (N.InstructionCount 99) (N.EngineError 0) Nothing

report :: [O.WriteEvent] -> Value
report writes = object
  ([K.fromString name .= value | (name, value) <- identity] ++
  [ "case_id" .= ("receipt" :: String), "runner_sha256" .= sha256Hex digest
  , "status" .= ("passed" :: String), "mismatches" .= ([] :: [String]), "stderr" .= ("" :: String), "runner_exit" .= (0 :: Int)
  , "origin" .= ("synthetic" :: String), "profile" .= ("synthetic-only" :: String)
  , "provenance" .= ("host receipt mutation control" :: String), "exclusions" .= ["no native execution" :: String]
  , "runner" .= object
      [ "wire_version" .= (1 :: Int), "elapsed_ns" .= ("1234" :: String), "trace_total" .= ("900" :: String)
      , "trace" .= [object ["pc" .= ("0x00624694" :: String), "instruction" .= ("0xe92d49f8" :: String)]]
      , "access_total" .= ("500" :: String), "accesses" .= [object ["type" .= (16 :: Int), "address" .= ("0x009597d0" :: String), "size" .= (4 :: Int), "value" .= ("0x0000000011223344" :: String)]]
      , "events" .= ([] :: [Value]), "memory" .= ([] :: [Value])
      , "memory_provenance" .= (map eventJSON writes ++ [readEvent]) ] ])
  where
    eventJSON event = object ["kind" .= ("write" :: String), "address" .= N.hexWord32 (O.eventAddress event), "size" .= O.eventWidth event, "value" .= N.hexWord32 (fromIntegral (O.eventValue event)), "pc" .= ("0x00624694" :: String), "instruction" .= ("0xe92d49f8" :: String)]
    readEvent = object ["kind" .= ("read" :: String), "address" .= ("0x009597d0" :: String), "size" .= (4 :: Int), "value" .= ("0x0000000011223344" :: String), "pc" .= ("0x00624698" :: String), "instruction" .= ("0xe5934000" :: String)]

parse :: Bool -> N.NativeObservation -> Value -> Either String ObservationReceipt
parse claims = parseObservationReceipt claims digest (C.CaseId "receipt") identity

receipt :: Bool -> [O.WriteEvent] -> ObservationReceipt
receipt claims writes = either error id (parse claims (native writes) (report writes))

main :: IO ()
main = do
  base <- case O.comparedRegions of
    (_, address, _) : _ -> pure address
    [] -> fail "receipt fixture has no compared RAM"
  let first = O.WriteEvent (base + 32) 4 0x11223344
      second = O.WriteEvent (base + 36) 4 0x55667788
      writes = [first, second, first]
      stock = receipt False writes
      pairs candidate = pairReceipt Nothing stock candidate
  check "identical complete observations pass" (pairPassed (pairs stock))
  forM_ [drop 1 writes, writes ++ [second], [first {O.eventValue = 3}, second, first], reverse (take 2 writes) ++ [first]] $ \changed ->
    check "missing extra changed reordered physical writes fail" (not (pairPassed (pairs (receipt False changed))))
  let changedRegister = (native writes) {N.nativeRegisters = M.insert N.R0 8 (N.nativeRegisters (native writes))}
      changedMemory = (native writes) {N.nativeMemory = [(ident, B.cons 1 (B.drop 1 bytes)) | (ident, bytes) <- N.nativeMemory (native writes)]}
      parsed observation = either error id (parse False observation (report writes))
  check "changed public register fails" (not (pairPassed (pairs (parsed changedRegister))))
  check "changed compared byte fails" (not (pairPassed (pairs (parsed changedMemory))))
  let r3Changed = (native writes) {N.nativeRegisters = M.insert N.R3 9 (N.nativeRegisters (native writes))}
  check "undeclared r3 is retained but not compared" (pairPassed (pairs (parsed r3Changed)))
  check "declared r3 participates" (not (pairPassed (pairReceipt Nothing (receipt True writes) (either error id (parse True r3Changed (report writes))))))
  let expected = O.Expectation N.Returned [(N.R0, 8)] []
  check "independent model mismatch fails both roles" (not (pairPassed (pairReceipt (Just expected) stock stock)))
  check "model absence remains explicit" (pairModel (pairs stock) == Nothing)
  forM_ [(native writes) {N.nativeStatus = N.MemoryGuard}, (native writes) {N.nativeEngineError = N.EngineError 1}, (native writes) {N.nativeProvenance = N.Truncated (N.ProvenanceTotal 9000)}] $ \bad ->
    check "guest errors and incomplete provenance fail closed" (not (pairPassed (pairs (parsed bad))))
  check "provenance metadata disagreement rejected" (isLeft (parse False (native [first]) (report writes)))
  forM_ ["case_sha256", "wire_sha256", "runner_sha256", "case_id"] $ \name ->
    check "malformed or mismatched identity rejected" (isLeft (parse False (native writes) (set name (text "wrong") (report writes))))
  check "malformed artifacts rejected" (isLeft (parse False (native writes) (set "artifacts" (objectArray []) (report writes))))
  let failed message = set "mismatches" (objectArray [text message]) (set "status" (text "failed") (report writes))
      unknown = either error id (parse False (native writes) (failed "future SDK diagnostic"))
      stack = either error id (parse False (native writes) (failed "memory stack: differs from exact allowed final bytes"))
  check "failed SDK requires full raw" (observationRequiresFull stack)
  check "unknown exact diagnostic retained" (observationRequiresFull unknown && sdkExactMismatches (observationSDK unknown) == [MismatchDiagnostic "future SDK diagnostic"])
  check "mapped stack has no independent source witness" (observationDiagnosticClass stack == UnsupportedSourceBounds)
  encoded <- either fail pure (eitherDecodeStrict' (encodePairReceipt (pairs stock)) :: Either String Value)
  let encodedStock = get encoded "stock"
      encodedWrites = list (get encodedStock "physical_writes")
  check "serialized physical events preserve order duplicates and values" (map (getValue "value") encodedWrites == [Number 287454020, Number 1432778632, Number 287454020])
  let metadata = get encodedStock "runner_metadata"
      provenance = list (get metadata "memory_provenance")
  check "receipt preserves reads and per-event instruction site" (case reverse provenance of
    entry : _ -> str (get entry "kind") == "read" && get (get entry "site") "pc" == Number 6440600
    [] -> False)
  check "receipt preserves bounded trace totals independently" (get metadata "trace_total" == Number 900 && length (list (get metadata "trace")) == 1 && pairPassed (pairs stock))
  check "receipt preserves access values and elapsed time" (map (getValue "value") (list (get metadata "accesses")) == [Number 287454020] && get metadata "elapsed_ns" == Number 1234)
  check "unknown runner metadata is refused before cleanup" (isLeft (parse False (native writes) (set "runner" (set "future" Null (get (report writes) "runner")) (report writes))))
  check "serialized ABI retains unclaimed r3" (any (\entry -> str (get entry "register") == "r3") (list (get (get encodedStock "raw_run") "registers")))
  check "serialized compared memory contains actual byte values" (any (not . null . list . (`get` "bytes")) (list (get (get encodedStock "raw_run") "memory")))
  check "serialized model absence is null" (get encoded "model" == Null)
  failedEncoded <- either fail pure (eitherDecodeStrict' (encodePairReceipt (pairReceipt Nothing unknown unknown)) :: Either String Value)
  check "serialized SDK retains exact unknown diagnostic" (map str (list (get (get (get failedEncoded "stock") "sdk") "mismatches")) == ["future SDK diagnostic"])
  where getValue name value = get value name
