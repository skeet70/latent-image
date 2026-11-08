{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (forM, unless)
import Data.Aeson (Value, encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Char (isAlphaNum, toLower)
import Data.Word (Word32)
import Emulation.RenderCases
import Emulation.Sha256 (sha256, sha256Hex)
import Emulation.ValuePickerCases
import System.Directory (createDirectoryIfMissing)
import System.Environment (getArgs)
import System.FilePath ((</>))

main :: IO ()
main = do
  args <- getArgs
  let out = option "--out" "private-data/analysis/x-m1-1.50/native-case/value-picker" args
      analysis = option "--analysis" "private-data/analysis/x-m1-1.50" args
      candidate = optional "--candidate" args
      candidateBank = optional "--candidate-bank" args
  unless ((candidate == Nothing) == (candidateBank == Nothing)) (fail "--candidate and --candidate-bank must be supplied together")
  image <- B.readFile (analysis </> "lz/plain-01100000.bin")
  image4 <- B.readFile (analysis </> "lz/id4-0061c400.bin")
  unless (sha256Hex (sha256 image) == "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d") (fail "image 6 identity differs")
  unless (sha256Hex (sha256 image4) == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") (fail "image 4 identity differs")
  owned <- traverse B.readFile candidate
  ownedBank <- traverse B.readFile candidateBank
  createDirectoryIfMissing True out
  let stock = B.take 0x2000 (B.drop 0x63000 image)
      bank = patch 0x10f0 (wordsLE [0xe300a849]) stock
      table = B.take 12 (B.drop 0xba2c image)
      report = B.replicate 0x44 0xa5
      stack = B.replicate 0x1100 0
      artifacts = [("stock",stock),("bank",bank),("table",table),("palette",B.take 20 (B.drop 0x13be4 image)),("font",wordsLE [9]),("side",sideState),("report",report)] ++ maybe [] (\bytes -> [("candidate",bytes)]) owned ++ maybe [] (\bytes -> [("candidate-bank",bytes)]) ownedBank
  artifactJSON <- forM artifacts $ \(name,bytes) -> do
    let path = name ++ ".bin"
    B.writeFile (out </> path) bytes
    pure (name,object ["id" .= name,"path" .= path,"size" .= B.length bytes,"sha256" .= sha256Hex (sha256 bytes)])
  entries <- forM pickerCases $ \c -> do
    let ident = "picker-" ++ map (\ch -> if isAlphaNum ch then toLower ch else '-') (pickerName c)
        role = maybe "stock" (const "candidate") owned
        caseId = ident ++ "-" ++ role
        reportAddress = case pickerSite c of MenuPicker -> 0x03000000; FnPicker -> 0x0310104c
        reportRegion = case pickerSite c of MenuPicker -> "report"; FnPicker -> "stack"
        reportOffset = case pickerSite c of MenuPicker -> 0; FnPicker -> 0x104c
        initialStack = case pickerSite c of MenuPicker -> stack; FnPicker -> patch 0x104c report stack
        stackBytes = patch 0x1000 (pickerStackArguments c (ReportAddress reportAddress)) initialStack
        stackPath = caseId ++ "-stack.bin"
    B.writeFile (out </> stackPath) stackBytes
    let stackArtifact = object ["id" .= ("case-stack" :: String),"path" .= stackPath,"size" .= B.length stackBytes,"sha256" .= sha256Hex (sha256 stackBytes)]
        useCode = case pickerGate c of StockGate -> "stock"; BankGate -> "bank"
        ownedCode = case pickerGate c of StockGate -> owned; BankGate -> ownedBank
        ownedArtifact = case pickerGate c of StockGate -> "candidate"; BankGate -> "candidate-bank"
        codeEntry = maybe 0x01163f2c (const 0x02000000) owned
        regions = [region "stock-code" 0x01163000 0x2000 "rx" useCode,region "brackets" 0x0110ba2c 12 "r" "table",region "palette" 0x01113be4 20 "rw" "palette",region "font" 0x015e46c0 4 "rw" "font",region "side" 0x015e64d8 20 "rw" "side",region "report" 0x03000000 0x44 "rw" "report",region "stack" 0x03100000 0x1100 "rw" "case-stack",zeroRegion "exit" 0x020f0000 4 "rx",zeroRegion "menu" 0x009571d0 44 "rw"] ++ maybe [] (\bytes -> [region "candidate-code" 0x02000000 (B.length bytes) "rx" ownedArtifact]) ownedCode
        NodeId node = pickerNode c
        registers = object [K.fromString name .= addressText value | (name,value) <- [("r"++show n,case n of 0 -> node;1 -> pickerX c;2 -> pickerY c;_ -> 0) | n <- [0::Int ..12]] ++ [("sp",0x03101000),("lr",0x020f0000),("cpsr",0x10)]]
        funcs = [object ["address" .= addressText codeEntry,"size" .= maybe (3824::Int) B.length ownedCode]]
        spec = object
          ["schema_version" .= (1::Int),"id" .= caseId,"origin" .= ("stock"::String)
          ,"provenance" .= ("R1 value-picker contract, instruction-derived service oracle; stock bytes 01163f2c..01164e18. Expected events and report fixed before execution."::String)
          ,"cpu" .= object ["architecture" .= ("arm"::String),"model" .= ("cortex-r5f"::String),"mode" .= ("arm"::String),"endian" .= ("little"::String)]
          ,"artifacts" .= (map snd artifactJSON ++ [stackArtifact]),"entry" .= addressText codeEntry,"exit" .= addressText 0x020f0000,"registers" .= registers,"regions" .= regions,"functions" .= funcs
          ,"stubs" .= callsJSON targets (pickerCalls c),"limits" .= object ["instructions" .= (100000::Int),"wall_ms" .= (10000::Int)]
          ,"expected" .= object ["status" .= ("returned"::String),"registers" .= object ["sp" .= addressText 0x03101000,"r4" .= addressText 0,"r5" .= addressText 0,"r6" .= addressText 0,"r7" .= addressText 0,"r8" .= addressText 0,"r9" .= addressText 0,"r10" .= addressText 0,"r11" .= addressText 0]
              ,"memory" .= [object ["region" .= (reportRegion::String),"offset" .= (reportOffset::Int),"hex" .= hexBytes (pickerReport c)]]
              ,"ignored_memory" .= [object ["region" .= ("stack"::String),"offset" .= (0x100::Int),"length" .= (0xf00::Int)]]
              ,"events" .= eventsJSON (pickerCalls c)]
          ,"exclusions" .= (["All callees are scripted services. No stock leaf drawing, task scheduling, hardware display or package-node ownership is qualified.","Private callee stack scratch is excluded; stack guard, caller arguments, report, side state, panel font and all other mapped bytes remain checked."]::[String])]
    BL.writeFile (out </> caseId ++ ".json") (encode spec)
    pure (object ["id" .= caseId,"path" .= (caseId++".json")])
  BL.writeFile (out </> "batch.json") (encode (object ["schema_version" .= (1::Int),"cases" .= entries]))
  putStrLn ("Emitted " ++ show (length entries) ++ " contract-derived value-picker cases into " ++ out)

patch :: Int -> B.ByteString -> B.ByteString -> B.ByteString
patch offset bytes initial = B.take offset initial <> bytes <> B.drop (offset+B.length bytes) initial

region :: String -> Word32 -> Int -> String -> String -> Value
region name address size permissions artifact = object ["id" .= name,"address" .= addressText address,"size" .= size,"permissions" .= permissions,"initial" .= object ["artifact" .= artifact,"offset" .= (0::Int),"length" .= size]]

zeroRegion :: String -> Word32 -> Int -> String -> Value
zeroRegion name address size permissions = object ["id" .= name,"address" .= addressText address,"size" .= size,"permissions" .= permissions,"initial" .= object ["zero" .= True]]

option :: String -> String -> [String] -> String
option name fallback args = maybe fallback id (optional name args)

optional :: String -> [String] -> Maybe String
optional _ [] = Nothing
optional name (key:value:rest) | name == key = Just value
                               | otherwise = optional name rest
optional _ [_] = Nothing
