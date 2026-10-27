-- SPDX-License-Identifier: AGPL-3.0-only
module Main (main) where

import Control.Monad (forM_)
import qualified Data.ByteString as B
import Data.Char (digitToInt)
import Data.List (isInfixOf)
import qualified FujiAF.Host as H
import System.Directory (createDirectory)
import System.Environment (getArgs)
import System.Exit (ExitCode(..))
import System.FilePath ((</>))
import System.Process (readProcessWithExitCode)

hexBytes :: String -> B.ByteString
hexBytes = B.pack . pairs
  where
    pairs [] = []
    pairs (a:b:rest) = fromIntegral (16*digitToInt a+digitToInt b):pairs rest
    pairs _ = error "Invalid synthetic fixture hex"

run :: FilePath -> [String] -> IO String
run executable args = do
  (status,out,err) <- readProcessWithExitCode executable args ""
  H.ensure (status == ExitSuccess) ("Decoder failed: " ++ err)
  pure out
reject :: FilePath -> [String] -> IO ()
reject executable args = do
  (status,_,_) <- readProcessWithExitCode executable args ""
  H.ensure (status /= ExitSuccess) "Decoder accepted an invalid input"

main :: IO ()
main = do
  args <- getArgs
  case args of
    [ring,census,diff] -> H.withTemporary "console-trace-test" $ \root -> do
      let header = B.replicate 0x1c 0 <> hexBytes "fc1f0a00" <> B.replicate 0x14 0
          records = map hexBytes
            [ "0f253412030722000100070064000000"
            , "0f2578560307220000008700b4000000"
            , "0f3b9abc0207220000000500b0040000"
            , "722c3412320000004d41524b54455354"
            , "092ccdab443322118403000003000000"
            , "0944efbe8877665507000000ffffffff"
            , "712c34122a00000088776655feffffff"
            , "55557856556677881122334409000000"
            , "0f2c9abc0322320078563412f4010000"
            , "81663412d204000098badcfeefcdab89"
            ]
          store = B.concat (header:records) <> B.replicate (0x1fff4-0x34-10*16) 0
          direct = root </> "store.bin"
          high = root </> "high.bin"
          windows = root </> "windows"
      B.writeFile direct store
      B.writeFile high (B.replicate 0x800 0 <> store <> B.replicate 0xc0c 0)
      createDirectory windows
      B.writeFile (windows </> "400a0800.bin") (B.take 256 store)
      B.writeFile (windows </> "400a0900.bin") (B.drop 256 store)
      forM_ [ring,census] $ \executable -> do
        expected <- run executable ["store",direct]
        actualHigh <- run executable ["high-known",high]
        actualWindows <- run executable ["windows",windows]
        H.ensure (expected == actualHigh && expected == actualWindows) "Equivalent console inputs produced different reports"
      report <- run census ["store",direct,"2c","44","55","66"]
      H.ensure ("2c:4(x4,t42..900)" `isInfixOf` report) "Known record ticks not grouped correctly"
      H.ensure ("44:1(x1,t7..7)" `isInfixOf` report) "Failure tick decoded from timer delta"
      H.ensure ("55:1(x1,tunknown)" `isInfixOf` report) "Unknown record contributed a tick"
      H.ensure ("66:1(x1,t1234..1234)" `isInfixOf` report) "Error report tick not decoded at +4"
      H.ensure ("category 0xfedcba98 error 0x89abcdef" `isInfixOf` report) "Error report payload words not preserved"
      ringReport <- run ring ["store",direct]
      H.ensure ("type 81 tick   1234" `isInfixOf` ringReport) "Error report missing from ring tick output"
      H.ensure ("str MARKTEST" `isInfixOf` report) "Marker string missing"
      H.ensure (not ("4294967295" `isInfixOf` report)) "Failure delta interpreted as tick"
      keyReport <- run ring ["--quiet","store",direct]
      H.ensure ("0x0007 press 1 release 1 unreleased 0 ticks 100..180" `isInfixOf` keyReport) "Key press/release grouping incorrect"
      reject census ["store",direct,"100"]
      reject ring ["store",direct,"extra"]
      let bad = root </> "bad.bin"
      B.writeFile bad (B.take 20 store)
      reject ring ["store",bad]
      B.writeFile bad (B.take 0x1c store <> hexBytes "0000" <> B.drop 0x1e store)
      reject census ["store",bad]
      B.writeFile bad (B.take 0x1e store <> hexBytes "fc1f" <> B.drop 0x20 store)
      reject census ["store",bad]
      let sparse = root </> "sparse"
      createDirectory sparse
      B.writeFile (sparse </> "400a0900.bin") (B.take 256 store)
      reject census ["windows",sparse]
      let bank0 = root </> "bank0.bin"
          bank1 = root </> "bank1.bin"
          record sequenceBytes value = hexBytes sequenceBytes <> B.replicate 0x26 0 <> B.singleton value <> B.replicate (512-0x29) 0
          bank rs = B.concat rs <> B.replicate (0x20000-length rs*512) 255
      B.writeFile bank0 (bank [record "fe7f" 1,record "ff7f" 1])
      B.writeFile bank1 (bank [record "0000" 2])
      differences <- run diff [bank0,bank1]
      H.ensure ("sequence 0x7ffe..0x0000, contiguous yes" `isInfixOf` differences) "Settings wrap chain not ordered"
      H.ensure ("body group 1: 2 record(s)" `isInfixOf` differences && "body group 2: 1 record(s)" `isInfixOf` differences) "Settings body equality groups incorrect"
      H.ensure ("p+000:01>02" `isInfixOf` differences) "Settings payload movement missing"
      B.writeFile bank1 (bank [record "ff7f" 2])
      reject diff [bank0,bank1]
      B.writeFile bank1 (bank [record "0200" 2])
      reject diff [bank0,bank1]
      B.writeFile bank1 (bank [record "0080" 2])
      reject diff [bank0,bank1]
      B.writeFile bank0 (B.replicate 0x20000 255)
      B.writeFile bank1 (B.replicate 0x20000 255)
      reject diff [bank0,bank1]
      B.writeFile bank0 (B.replicate 512 255)
      reject diff [bank0,bank1]
    _ -> fail "Expected ring-check, trace-census and log-diff-check executable paths"
