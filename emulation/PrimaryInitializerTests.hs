{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (forM_, unless)
import Data.Aeson (Value (..), encode, object, toJSON, (.=))
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.Either (isLeft, isRight)
import Data.Word (Word32, Word64)
import Emulation.PrimaryInitializer
import qualified Emulation.PrimaryInitializerSource as PS
import System.Environment (getArgs)

-- These synthetic documents contain no initializer observation.
nonobservation :: String -> Word64 -> Value
nonobservation status count = object
  [ "schema" .= (8 :: Word32), "role" .= ("retained-primary" :: String)
  , "contract" .= receiptIdentity
  , "result" .= ("failed" :: String), "status" .= status
  , "error" .= ("finite observation stopped" :: String), "machine_failure" .= Null
  , "instructions" .= count, "cp14_identities" .= ([0x4003,0x5003] :: [Word32])
  , "final" .= context count, "boundaries" .= ([] :: [Value]), "primary" .= Null
  ]

receiptIdentity :: Value
receiptIdentity = object
  [ "role" .= ("retained-primary" :: String)
  , "source_sha256" .= ("43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490" :: String)
  , "loader_a_sha256" .= ("21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882" :: String)
  , "run_sha256" .= ("efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef" :: String)
  , "boot_rom_sha256" .= ("4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be" :: String)
  ]

ownedHashes :: Value
ownedHashes = object
  [ name .= (replicate 64 'a') | name <-
      ["input_sha256","source_loader_a_sha256","image_sha256","symbols_sha256","publication_sha256"]
  ]

ownedIdentity :: Value
ownedIdentity = replace "owned" ownedHashes (replace "role" (String "owned-primary") receiptIdentity)

context :: Word64 -> Value
context count = object
  [ "pc" .= (0x40000 :: Word32), "registers" .= (replicate 13 0 :: [Word32])
  , "sp" .= (0x40063460 :: Word32), "lr" .= (0 :: Word32), "cpsr" .= (0xd3 :: Word32)
  , "counts" .= [count,0], "tcm" .= ([[0,0],[0,0]] :: [[Word32]])
  ]

replace :: String -> Value -> Value -> Value
replace name value (Object row) = Object (KM.insert (Key.fromString name) value row)
replace _ _ value = value

remove :: String -> Value -> Value
remove name (Object row) = Object (KM.delete (Key.fromString name) row)
remove _ value = value

parse :: Value -> Either String Receipt
parse = parseReceipt . L.toStrict . encode

assert :: String -> Bool -> IO ()
assert label condition = unless condition (fail label)

main :: IO ()
main = do
  arguments <- getArgs
  runBytes <- case arguments of
    [path] -> B.readFile path
    _ -> fail "expected canonical public run fixture"
  assert "canonical float-bearing run is admitted" (isRight (parseRunInput runBytes))
  assert "whole run bytes remain authoritative" (isLeft (parseRunInput (B.snoc runBytes 32)))
  let finite = nonobservation "absent" 50000000
      refused = nonobservation "refused" 12
      callerStop = object ["boundary" .= ("caller" :: String), "context" .= replace "pc" (Number 0x409ac) (context 10)]
      finiteAfterCaller = replace "boundaries" (toJSON [callerStop]) finite
  case parse finite of
    Right receipt -> assert "finite absence remains distinct" (receiptStatus receipt == Absent)
    Left problem -> fail problem
  case parse refused of
    Right receipt -> assert "refusal remains distinct" (receiptStatus receipt == Refused)
    Left problem -> fail problem
  case parse finiteAfterCaller of
    Right receipt -> assert "partial caller progress remains finite absence" (receiptStatus receipt == Absent)
    Left problem -> fail problem
  let ownedFinite = replace "role" (String "owned-primary") (replace "contract" ownedIdentity finite)
  case parse ownedFinite of
    Right receipt -> assert "owned finite absence remains distinct" (receiptStatus receipt == Absent)
    Left problem -> fail problem
  forM_
    [ replace "role" (String "owned-primary") finite
    , replace "contract" ownedIdentity finite
    , replace "contract" (replace "owned" ownedHashes receiptIdentity) finite
    , replace "contract" (remove "owned" ownedIdentity) ownedFinite
    , replace "contract" (replace "owned" Null ownedIdentity) ownedFinite
    , replace "contract" (replace "owned" (replace "extra" Null ownedHashes) ownedIdentity) ownedFinite
    , replace "contract" (replace "owned" (replace "image_sha256" (String "invalid") ownedHashes) ownedIdentity) ownedFinite
    ] $ \document -> assert "owned receipt identity and role remain closed" (isLeft (parse document))
  forM_ ["input_sha256","source_loader_a_sha256","image_sha256","symbols_sha256","publication_sha256"] $ \name ->
    assert "owned receipt requires each artifact binding" (isLeft (parse (replace "contract"
      (replace "owned" (remove name ownedHashes) ownedIdentity) ownedFinite)))
  forM_ ["schema","role","contract","result","status","error","machine_failure","instructions","cp14_identities","final","boundaries","primary"] $ \name ->
    assert ("required field " ++ name) (isLeft (parse (remove name finite)))
  forM_
    [ nonobservation "absent" 49999999
    , nonobservation "absent" 50000001
    , nonobservation "unknown" 50000000
    , nonobservation "unreachable" 12
    , replace "status" (String "observed") finite
    , replace "result" (String "passed") finite
    , replace "role" (String "owned") finite
    , replace "schema" (Number 7) finite
    , replace "extra" Null finite
    , replace "machine_failure" (String "fault") finite
    , replace "error" Null finite
    , replace "instructions" (Number 49999999) finite
    , replace "final" (replace "extra" Null (context 50000000)) finite
    , replace "cp14_identities" (object []) finite
    , replace "boundaries" (object []) finite
    , replace "contract" (remove "source_sha256" receiptIdentity) finite
    , replace "contract" (replace "role" (String "owned") receiptIdentity) finite
    , replace "contract" (replace "run_sha256" (String "invalid") receiptIdentity) finite
    , replace "boundaries" (toJSON [callerStop,callerStop]) finite
    , replace "boundaries" (toJSON [replace "boundary" (String "before") callerStop]) finite
    , replace "boundaries" (toJSON [replace "context" (replace "pc" (Number 0x409ac) (context 50000001)) callerStop]) finite
    ] $ \document -> assert "closed nonobservation admission" (isLeft (parse document))
  let encoded = L.toStrict (encode finite)
      schemaWire token = B.concat ["{\"schema\":",token,",",B.drop 1 (L.toStrict (encode (remove "schema" finite)))]
  forM_
    [ B.append "{\"schema\":8," (B.drop 1 encoded)
    , schemaWire "8.0"
    , schemaWire "8e0"
    , schemaWire "-1"
    , B.append encoded "{}"
    ] $ \bytes -> assert "strict JSON admission" (isLeft (parseReceipt bytes))
  assert "noncanonical source is refused before program construction" (isLeft (PS.parseSource B.empty))
  assert "noncanonical run is refused at its typed boundary" (isLeft (parseRunInput B.empty))
  assert "noncanonical ROM is refused at its typed boundary" (isLeft (parseBootRomInput B.empty))
