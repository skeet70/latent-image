{-# LANGUAGE OverloadedStrings #-}
-- Independent synthetic wire construction: no author case loader is imported.
module Main where
import qualified FujiAF.Host as H
import Control.Monad (forM)
import Data.Bits ((.&.), shiftR)
import qualified Data.ByteString.Char8 as C
import Numeric (showHex)
import System.Directory (canonicalizePath)
import System.Environment (getArgs)
import System.Exit (ExitCode(..))

hex :: Integer -> String
hex number = showHex number ""
wordBytes :: Integer -> String
wordBytes number = concat [let value=hex ((number `shiftR` shift) .&. 255) in replicate (2-length value) '0' ++ value | shift <- [0,8,16,24]]
h32 :: Integer -> String
h32 number = let value=hex number in "0x" ++ replicate (8-length value) '0' ++ value

main :: IO ()
main = do
  args <- getArgs
  runner <- case args of [path] -> canonicalizePath path; _ -> H.failH "usage: f01-native-probes NATIVE_RUNNER"
  results <- forM [("stmia-writeback-boundary",0xe8a20003,0x2004,0x2004,4),
                   ("stmdb-writeback-boundary",0xe9220003,0x2008,0x2004,4)] $ \(label,instruction,base,address,size) -> do
    let code = wordBytes instruction ++ wordBytes 0xe12fff1e
        initial = [0xfffffffe,5,base] ++ replicate 10 0 ++ [0x3020,0x1080,0x10]
        wire = unlines ["FUJI_NATIVE_V1","cortex-r5f-arm-le","1004 1080 100 1000",unwords (map hex initial),"4 1 0",
          "1004 8 5 " ++ code,"1080 4 5 00000000",hex address ++ " " ++ hex size ++ " 3 " ++ replicate (fromIntegral size*2) '0',
          "3000 20 3 " ++ replicate 64 '0',"1004 8"]
    (stopped,status,output,errors) <- H.runBounded 3000000 (1024*1024) runner [] (C.pack wire)
    H.ensure (not stopped && status == ExitFailure 1) ("native guard probe failed: " ++ C.unpack errors)
    result <- H.jsonRead output
    let memory = H.list (H.get result "memory")
    H.ensure (H.get result "status" == H.s "memory_guard" && H.get (H.get result "registers") "r2" == H.s (h32 base)) "native guard changed base or exit"
    case memory of
      [codeMemory,_,dataMemory,_] -> H.ensure (H.get codeMemory "bytes" == H.s code && H.get dataMemory "bytes" == H.s (replicate (fromIntegral size*2) '0')) "native guard changed code/data"
      _ -> H.failH "native guard returned unexpected memory regions"
    H.ensure (H.get result "instructions" == H.s "1") "native guard instruction count differs"
    pure $ H.obj [("case",H.s label),("status",H.get result "status"),("base_unchanged",H.b True),("code_data_unchanged",H.b True),("instructions",H.get result "instructions")]
  H.jsonPrint (H.arr results)
