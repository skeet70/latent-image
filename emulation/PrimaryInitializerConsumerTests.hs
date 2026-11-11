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
import Emulation.PrimaryInitializerConsumer
import Emulation.InterfaceInitializer (Context (..))
import qualified Emulation.PrimaryInitializerConsumerSource as PSC

identity :: Value
identity = object
  [ "role" .= ("retained-primary" :: String)
  , "source_sha256" .= ("43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490" :: String)
  , "loader_a_sha256" .= ("21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882" :: String)
  , "run_sha256" .= ("efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef" :: String)
  , "boot_rom_sha256" .= ("4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be" :: String)
  ]

context :: Word64 -> Word64 -> Value
context core0 core1 = object
  [ "pc" .= (0x40000 :: Word32), "registers" .= (replicate 13 0 :: [Word32])
  , "sp" .= (0x40063460 :: Word32), "lr" .= (0 :: Word32), "cpsr" .= (0xd3 :: Word32)
  , "counts" .= [core0,core1], "tcm" .= ([[0,0],[0,0]] :: [[Word32]])
  ]

producer :: String -> Word64 -> Word64 -> Value
producer status core0 core1 = object
  [ "schema" .= (8 :: Word32), "role" .= ("retained-primary" :: String), "contract" .= identity
  , "result" .= ("failed" :: String), "status" .= status, "error" .= ("observation stopped" :: String), "machine_failure" .= Null
  , "instructions" .= (core0 + core1), "cp14_identities" .= ([0x4003,0x5003] :: [Word32])
  , "final" .= context core0 core1, "boundaries" .= ([] :: [Value]), "primary" .= Null
  ]

terminal :: String -> Word64 -> Word64 -> Value
terminal status core0 core1 = object
  [ "schema" .= (9 :: Word32), "result" .= ("failed" :: String), "status" .= status
  , "error" .= ("observation stopped" :: String), "machine_failure" .= Null, "instructions" .= (core0 + core1)
  , "cp14_identities" .= ([0x4003,0x5003] :: [Word32]), "final" .= context core0 core1
  , "boundaries" .= ([] :: [Value]), "initializer" .= producer status core0 core1, "continuation" .= Null, "consumer" .= Null
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
stackContext :: Word32 -> Word32 -> [[Word32]] -> Context
stackContext sp cpsr banks = Context 0x725b74 (1 : replicate 12 0) sp 0x69cc30 cpsr [20,10] banks

checkConsumerStack :: IO ()
checkConsumerStack = do
  let banks = [[0x70000011,0],[0x70000011,0]]
      natural = stackContext 0x70001fbc 0x60000153 banks
  assert "natural IRQ consumer stack is admitted" (isRight (admitConsumerStack natural))
  forM_ [0x70000020,0x70002000] $ \sp ->
    assert "complete bounded ATCM frame is admitted" (isRight (admitConsumerStack (stackContext sp 0x153 banks)))
  forM_ [0x153,0x60000153,0xf0000153] $ \cpsr ->
    assert "caller arithmetic flags do not select stack backing" (isRight (admitConsumerStack (stackContext 0x70001fbc cpsr banks)))
  forM_ [0x7000001c,0x70002004,0x70001fbd,0x40063450] $ \sp ->
    assert "stack frame must fit aligned core0 ATCM" (isLeft (admitConsumerStack (stackContext sp 0x153 banks)))
  forM_ [0xd3,0x53,0x113,0x152,0x173,0x1d3,0x353,0x02000153,0x01000153] $ \cpsr ->
    assert "consumer requires the source IRQ SVC control state without IT state" (isLeft (admitConsumerStack (stackContext 0x70001fbc cpsr banks)))
  forM_ [[0x70000010,0],[0x71000011,0],[0x70000015,0],[0x70000011,0x70000011]] $ \core0 ->
    assert "stack backing requires the exact core0 bank binding" (isLeft (admitConsumerStack (stackContext 0x70001fbc 0x153 [core0,[0x70000011,0]])))

main :: IO ()
main = do
  let refused = terminal "refused" 11 1
      deadline = terminal "absent" 30000000 20000000
      stop name pc = object ["boundary" .= (name :: String),"context" .= replace "pc" (toJSON (pc :: Word32)) (context 11 1)]
  forM_ [(refused,Refused),(deadline,Absent)] $ \(document,expected) -> case parse document of
    Left problem -> fail problem
    Right receipt -> assert "terminal status remains distinct" (receiptStatus receipt == expected)
  assert "machine failure preserves refusal" (isRight (parse (replace "machine_failure" (String "execution fault") refused)))
  forM_ ["schema","result","status","error","machine_failure","instructions","cp14_identities","final","boundaries","initializer","continuation","consumer"] $ \name ->
    assert ("required terminal field " ++ name) (isLeft (parse (remove name refused)))
  forM_
    [ replace "status" (String "unreachable") refused
    , replace "status" (String "observed") refused
    , replace "result" (String "passed") refused
    , replace "schema" (Number 7) refused
    , replace "extra" Null refused
    , replace "error" Null refused
    , replace "consumer" (object []) refused
    , replace "continuation" (object []) refused
    , replace "machine_failure" (String "fault") deadline
    , terminal "absent" 29999999 20000000
    , terminal "absent" 30000001 20000000
    , replace "instructions" (Number 13) refused
    , replace "final" (context 12 0) refused
    , replace "final" (replace "registers" (toJSON ([0] :: [Word32])) (context 11 1)) refused
    , replace "final" (replace "extra" Null (context 11 1)) refused
    , replace "cp14_identities" (toJSON ([0x5003,0x4003] :: [Word32])) refused
    , replace "initializer" (producer "absent" 30000000 20000000) refused
    , replace "initializer" (producer "refused" 30000000 20000000) deadline
    , replace "initializer" (replace "schema" (Number 9) (producer "refused" 11 1)) refused
    , replace "boundaries" (toJSON [stop "installation" 0x725a04]) refused
    , replace "boundaries" (toJSON [stop "entry" 0x725b74]) refused
    , replace "boundaries" (toJSON [stop "installation" 0x725b74]) refused
    , replace "boundaries" (toJSON [stop "installation" 0x725a04,stop "installation" 0x725a04]) refused
    ] $ \document -> assert "closed terminal and producer precedence admission" (isLeft (parse document))
  let encoded = L.toStrict (encode refused)
      withSchema token = B.concat ["{\"schema\":",token,",",B.drop 1 (L.toStrict (encode (remove "schema" refused)))]
  forM_ [B.append "{\"schema\":9," (B.drop 1 encoded),withSchema "9.0",withSchema "9e0",B.append encoded "{}"] $ \wire ->
    assert "strict receipt JSON boundary" (isLeft (parseReceipt wire))
  forM_ [1,120] $ \ident -> case PSC.descriptorId ident of
    Left problem -> fail problem
    Right admitted -> assert "descriptor bounds preserve actual identifier" (PSC.descriptorIdentifier admitted == ident)
  forM_ [0,121,maxBound] $ \ident -> assert "descriptor range rejects wraparound" (isLeft (PSC.descriptorId ident))
  checkConsumerStack
