module Main where

import Control.Monad (unless)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified FujiAF.UpdateContainer as U
import Replacement.FixedImage.Layout
import Replacement.FixedImage.Record
import System.Environment (getArgs)
import System.Exit (die)

check :: String -> Bool -> IO ()
check label passed = unless passed (die label)

main :: IO ()
main = do
  check "CRC-32 standard vector" (crc32 (C.pack "123456789") == 0xcbf43926)
  let body = B.pack [1, 2, 3, 4]
      bounds bytes = BodyBounds (imageBase + headerBytes) (imageBase + headerBytes + fromIntegral (B.length bytes))
  record <- either die pure (makeRecord (BuildId 7) (bounds body) (Bss (imageBase + 36) (imageBase + 40)) body)
  check "header/body load layout" (B.length record == 36 && U.word32At record 4 == imageBase && U.word32At record 8 == 36 && B.drop 32 record == body)
  check "empty image refused" (isLeft (makeRecord (BuildId 7) (bounds B.empty) (Bss imageLimit imageLimit) B.empty))
  let oversized = B.replicate (fromIntegral (imageLimit - imageBase - headerBytes + 1)) 0
  check "oversize image refused" (isLeft (makeRecord (BuildId 7) (bounds oversized) (Bss imageLimit imageLimit) oversized))
  check "BSS cannot cover data" (isLeft (makeRecord (BuildId 7) (bounds body) (Bss imageBase imageLimit) body))
  check "shifted body link refused" (isLeft (makeRecord (BuildId 7) (BodyBounds (imageBase+36) (imageBase+40)) (Bss imageLimit imageLimit) body))
  check "body length from another link refused" (isLeft (makeRecord (BuildId 7) (BodyBounds (imageBase+32) (imageBase+40)) (Bss imageLimit imageLimit) body))
  patch <- either die pure (publication 0xefca6c 1 2)
  let bytes = B.replicate (0xefca6c - 0xbf1a00) 0 <> U.putWord32 1
  patched <- either die pure (applyPublications Image5 image5Load [patch] bytes)
  check "staged word reaches replacement" (U.word32At patched (0xefca6c - 0xbf1a00) == 2)
  check "mismatched preimage refused" (isLeft (applyPublications Image5 image5Load [patch] patched))
  check "duplicate publication refused" (isLeft (applyPublications Image5 image5Load [patch, patch] bytes))
  check "publication cannot leave stock windows" (isLeft (publication imageBase 1 2))
  let segments = [segment 4 0x61c400, segment 5 0xbf1a00]
      rc0Segments = [segment 4 0x61c340, segment 5 0xbf1a00]
  check "stock image 4 loads at its segment's address" (imageLoad segments Image4 == Right stockLoad)
  check "RC0 image 4 loads at its segment's address" (imageLoad rc0Segments Image4 == Right rc0Load)
  check "image 5 loads at its segment's address" (imageLoad rc0Segments Image5 == Right image5Load)
  check "missing image 4 segment refused" (isLeft (imageLoad [segment 5 0xbf1a00] Image4))
  check "duplicate image 4 segment refused" (isLeft (imageLoad (segment 4 0x61c340 : segments) Image4))
  let ceilingSite = U.putWord32 0xe3013a00 <> U.putWord32 0xe34030bf
      stockImage4 = B.replicate (0x72b12c - 0x61c400) 0 <> ceilingSite <> B.replicate 8 0
      rc0Prefix = B.replicate (0x61c400 - 0x61c340) 0xa5
  stockCeiling <- either die pure (applyPublications Image4 stockLoad ceilingPublications stockImage4)
  rc0Ceiling <- either die pure (applyPublications Image4 rc0Load ceilingPublications (rc0Prefix <> stockImage4))
  check "stock ceiling words reach their sites"
    (and [U.word32At stockCeiling (fromIntegral (address - 0x61c400)) == new | Publication _ address _ new <- ceilingPublications])
  check "RC0 image 4 takes the same words at the same addresses" (rc0Ceiling == rc0Prefix <> stockCeiling)
  check "image 4 at the wrong load refused" (isLeft (applyPublications Image4 rc0Load ceilingPublications stockImage4))
  let heapPrefix = B.replicate (0x89cda4 - 0x61c400) 0
      heap end = heapPrefix <> B.concat (map U.putWord32 [0x48454150,1,0xa71a00,end]) <> B.replicate 40 0
  check "default owned heap package refused with fixed reservation" (isLeft (applyPublications Image4 stockLoad [] (heap 0xbf1a00)))
  check "recipe owned heap package refused with fixed reservation" (isLeft (applyPublications Image4 stockLoad [] (heap 0xbe1a00)))
  check "fixed owned heap package admitted" (applyPublications Image4 stockLoad [] (heap imageBase) == Right (heap imageBase))
  check "default owned heap package refused under RC0's load" (isLeft (applyPublications Image4 rc0Load [] (rc0Prefix <> heap 0xbf1a00)))
  check "fixed owned heap package admitted under RC0's load" (isRight (applyPublications Image4 rc0Load [] (rc0Prefix <> heap imageBase)))
  arguments <- getArgs
  case arguments of
    [defaultPath,fixedPath] -> do
      defaultHeap <- B.readFile defaultPath
      fixedHeap <- B.readFile fixedPath
      let packaged linked = B.replicate (0x89cda4-0x61c400) 0 <> B.take (0x89ec00-0x89cda4) linked
      check "linked default heap package refused" (isLeft (applyPublications Image4 stockLoad [] (packaged defaultHeap)))
      check "linked fixed heap package admitted" (applyPublications Image4 stockLoad [] (packaged fixedHeap) == Right (packaged fixedHeap))
    _ -> die "expected default and fixed heap package binaries"
  putStrLn "fixed image record and publication tests pass"
  where
    isLeft (Left _) = True
    isLeft _ = False
    isRight = not . isLeft
    stockLoad = LoadAddress 0x61c400
    rc0Load = LoadAddress 0x61c340
    image5Load = LoadAddress 0xbf1a00
    segment number load = U.SegmentEntry
      { U.segId = number, U.segLoad = load, U.segSize = 0, U.segBss = 0, U.segBssLen = 0
      , U.segFlash = 0, U.segFlashLen = 0, U.segWord7 = 0, U.segFlag = 0 }
