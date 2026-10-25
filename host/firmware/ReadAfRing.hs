module Main where

import Control.Exception (throwIO)
import qualified Data.ByteString as B
import FujiAF.AfRing
import qualified FujiAF.Host as H
import FujiAF.JSON (encodeCanonical)
import FujiAF.Transport (writePrivateBytes)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ["decode",input] -> decodeFile input >>= H.jsonPrint . afRingValue
    ["decode",input,output] -> do
      capture <- decodeFile input
      writePrivateBytes output (encodeCanonical (afRingValue capture) <> B.singleton 10)
    ["--help"] -> putStrLn usage
    _ -> hPutStrLn stderr usage >> exitFailure
  where
    decodeFile path = H.readRegular afRingBytes path >>= either throwIO pure . decodeAfRing
    usage = "usage: read-af-ring decode RING.bin [OUT.json]\nOffline only. A single bulk capture is best-effort, not a coherent snapshot."
