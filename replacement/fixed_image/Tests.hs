module Main where

import Control.Monad (unless)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified FujiAF.UpdateContainer as U
import Replacement.FixedImage.Layout
import Replacement.FixedImage.Record
import System.Exit (die)

check :: String -> Bool -> IO ()
check label passed = unless passed (die label)

main :: IO ()
main = do
  check "CRC-32 standard vector" (crc32 (C.pack "123456789") == 0xcbf43926)
  let body = B.pack [1, 2, 3, 4]
  record <- either die pure (makeRecord (BuildId 7) (Bss (imageBase + 36) (imageBase + 40)) body)
  check "header/body load layout" (B.length record == 36 && U.word32At record 4 == imageBase && U.word32At record 8 == 36 && B.drop 32 record == body)
  check "two block range" (imageLimit - imageBase == recordBlocks * blockBytes)
  check "empty image refused" (isLeft (makeRecord (BuildId 7) (Bss imageLimit imageLimit) B.empty))
  check "oversize image refused" (isLeft (makeRecord (BuildId 7) (Bss imageLimit imageLimit) (B.replicate 0x40000 0)))
  check "BSS cannot cover data" (isLeft (makeRecord (BuildId 7) (Bss imageBase imageLimit) body))
  patch <- either die pure (publication 0xefca6c 1 2)
  let bytes = B.replicate (0xefca6c - 0xbf1a00) 0 <> U.putWord32 1
  patched <- either die pure (applyPublications Image5 [patch] bytes)
  check "staged word reaches replacement" (U.word32At patched (0xefca6c - 0xbf1a00) == 2)
  check "mismatched preimage refused" (isLeft (applyPublications Image5 [patch] patched))
  check "duplicate publication refused" (isLeft (applyPublications Image5 [patch, patch] bytes))
  check "publication cannot leave stock windows" (isLeft (publication imageBase 1 2))
  putStrLn "fixed image record and publication tests pass"
  where
    isLeft (Left _) = True
    isLeft _ = False
