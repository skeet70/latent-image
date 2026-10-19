{-# LANGUAGE OverloadedStrings #-}
-- Closed read-plan document reader. Bounded ordinary file I/O only; a plan is
-- parsed and validated before any camera operation starts. The route window
-- list is the second document shape: a RAM window list the host derives from a
-- probe-routes phase-1 capture.
module FujiAF.PlanDocument (readPlanFile, parseReadPlan, readRoutePlanFile, parseRoutePlan, privateRegular) where

import Control.Exception (bracket, bracketOnError)
import Control.Monad (forM, unless)
import Data.Aeson (Value(..))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as M
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import Data.Char (digitToInt)
import Data.List (sort)
import Data.Scientific (toBoundedInteger)
import qualified Data.Text as T
import qualified Data.Vector as V
import System.FilePath (isAbsolute)
import System.IO (hClose)
import System.Posix.Files
import System.Posix.IO
import System.Posix.User (getEffectiveUserID)
import FujiAF.JSON (decodeStrictIntegers)
import FujiAF.Read
import FujiAF.Routes (RouteWindow(..), routePlanDocumentCap, validateRoutePlan)
import FujiAF.Transport (safeIdentifier, hexDigest)
import qualified FujiAF.Broker as P

require :: Bool -> String -> IO ()
require condition message = unless condition (P.reject message)

closed :: Value -> String -> IO Value
closed v keys = do
  case v of
    Object m -> require (sort (map K.toString (M.keys m)) == sort (words keys)) "closed document keys required"
    _ -> P.reject "JSON object required"
  pure v

field :: Value -> String -> Value
field (Object m) k = maybe Null id (M.lookup (K.fromString k) m)
field _ _ = Null

text :: Value -> Int -> IO String
text (String s) cap = do
  let str = T.unpack s
  require (not (null str) && length str <= cap && all (>= ' ') str) "bounded text required"
  pure str
text _ _ = P.reject "text required"

string :: Value -> IO String
string v = text v 256

integer :: Value -> Int -> Int -> IO Int
integer (Number n) low high = case toBoundedInteger n of
  Just i | i >= low && i <= high -> pure i
  _ -> P.reject "bounded exact integer required"
integer _ _ _ = P.reject "bounded exact integer required"

list :: Value -> Int -> Int -> IO [Value]
list (Array a) low high = do
  require (V.length a >= low && V.length a <= high) "bounded list required"
  pure (V.toList a)
list _ _ _ = P.reject "list required"

requireFields :: Value -> [(String,Value)] -> String -> IO ()
requireFields v pairs message = require (all (\(k,w) -> field v k == w) pairs) message

-- Fixed-width lowercase hex keeps plan offsets sortable as written and
-- unambiguous to parse.
address :: Value -> Int -> IO Int
address v width = do
  s <- text v (width+2)
  require (length s == width+2 && take 2 s == "0x" && all (`elem` ("0123456789abcdef" :: String)) (drop 2 s)) "fixed-width lowercase hex address required"
  pure (foldl (\acc c -> acc*16+digitToInt c) 0 (drop 2 s))

-- An absolute, owned, bounded regular file with no group or other
-- permission bits; noun names it in the refusals.
privateRegular :: String -> FilePath -> Int -> IO B.ByteString
privateRegular noun path cap = do
  require (isAbsolute path) ("absolute "++noun++" path required")
  bracket acquire hClose $ \h -> do
    bytes <- B.hGet h (cap+1)
    require (B.length bytes <= cap) (noun++" exceeds its input cap")
    pure bytes
  where
    valid st = do
      require (isRegularFile st && fileSize st <= fromIntegral cap) ("bounded regular nonsymlink "++noun++" required")
      uid <- getEffectiveUserID
      require (fileOwner st == uid && fileMode st .&. 0o077 == 0) ("private owned "++noun++" required")
    acquire = do
      getSymbolicLinkStatus path >>= valid
      bracketOnError (openFd path ReadOnly defaultFileFlags {nofollow=True,nonBlock=True,cloexec=True}) closeFd $ \fd -> do
        getFdStatus fd >>= valid
        fdToHandle fd

parseReadPlan :: Value -> IO ReadPlan
parseReadPlan v = do
  _ <- closed v "schema_version plan_id firmware_basis channel windows oracle generated_by"
  _ <- integer (field v "schema_version") 1 1
  identifier <- text (field v "plan_id") 80
  require (safeIdentifier identifier) "plan alias required"
  basis <- string (field v "firmware_basis")
  channel <- closed (field v "channel") "opcode function mb length"
  requireFields channel [("opcode",String "0x1a0002")] "read channel opcode mismatch"
  _ <- integer (field channel "function") readFunction readFunction
  _ <- integer (field channel "mb") readMailbox readMailbox
  _ <- integer (field channel "length") readWindowLength readWindowLength
  entries <- list (field v "windows") 1 planWindowCap
  windows <- forM entries $ \entry -> do
    _ <- closed entry "index offset length purpose"
    index <- integer (field entry "index") 0 (planWindowCap-1)
    offset <- address (field entry "offset") 6
    _ <- integer (field entry "length") readWindowLength readWindowLength
    purpose <- string (field entry "purpose")
    pure (ReadWindow index offset purpose)
  oracle <- closed (field v "oracle") "window_index byte_offset expect"
  requireFields oracle [("expect",String "product-string")] "only the product-string oracle is supported"
  oracleIndex <- integer (field oracle "window_index") 0 (planWindowCap-1)
  oracleByte <- address (field oracle "byte_offset") 2
  generated <- closed (field v "generated_by") "tool sha256"
  tool <- string (field generated "tool")
  digest <- text (field generated "sha256") 64
  require (hexDigest digest) "lowercase SHA256 required"
  let plan = ReadPlan identifier basis windows (ReadOracle oracleIndex oracleByte) tool digest
  either P.reject pure (validatePlan plan)
  pure plan

readPlanFile :: FilePath -> IO ReadPlan
readPlanFile path = do
  bytes <- privateRegular "plan" path planDocumentCap
  value <- either P.reject pure (decodeStrictIntegers bytes)
  parseReadPlan value

-- The phase-2 document the host derives from a probe-routes phase-1 capture:
-- one RAM window list, indices equal to positions, 8-digit lowercase hex
-- addresses. The derivation's cursor cell leads the list.
parseRoutePlan :: Value -> IO [RouteWindow]
parseRoutePlan v = do
  _ <- closed v "windows"
  entries <- list (field v "windows") 1 planWindowCap
  windows <- forM entries $ \entry -> do
    _ <- closed entry "index address length purpose"
    index <- integer (field entry "index") 0 (planWindowCap-1)
    at <- address (field entry "address") 8
    bytes <- integer (field entry "length") 1 readWindowLength
    purpose <- string (field entry "purpose")
    pure (RouteWindow index at bytes purpose)
  either P.reject pure (validateRoutePlan windows)
  pure windows

readRoutePlanFile :: FilePath -> IO [RouteWindow]
readRoutePlanFile path = do
  bytes <- privateRegular "plan" path routePlanDocumentCap
  value <- either P.reject pure (decodeStrictIntegers bytes)
  parseRoutePlan value
