{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Codec.Picture (PixelYCbCr8 (..), encodeJpegAtQuality, generateImage)
import Control.Monad (unless)
import Data.Bits (shiftR)
import Data.Aeson (Value (..))
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Either (isLeft, isRight)
import Data.Word (Word32)
import qualified Emulation.PlaybackDecode as Decode

check :: String -> Bool -> IO ()
check label held = unless held (ioError (userError label))

wordsAt :: Int -> [(Int, Word32)] -> B.ByteString
wordsAt count fields = B.pack [byte i | i <- [0..count-1]]
  where byte i = case lookup (i - i `mod` 4) fields of
          Nothing -> 0
          Just value -> fromIntegral (value `shiftR` (8 * (i `mod` 4)))

main :: IO ()
main = do
  let jpeg = BL.toStrict (encodeJpegAtQuality 90 (generateImage (\_ _ -> PixelYCbCr8 64 128 128) 18 9))
      n = fromIntegral (B.length jpeg)
      fixture = B.replicate 37 7 <> jpeg <> B.replicate 11 8
      params source capacity = wordsAt 0x78 [(0,0x14eae2c),(8,source),(12,n),(16,0x40200000),(0x14,18),(0x18,9),(0x1c,capacity),(0x20,1)]
      regs source capacity = wordsAt 16 [(0,0x40200000),(4,capacity),(8,source),(12,n)]
      pixels = B.pack [if y < 9 && x < 36 then if x `mod` 4 < 2 then 64 else 128 else 0 | y <- [0..15 :: Int], x <- [0..63 :: Int]]
      snap source capacity endpoint bytes = Decode.withEndpoint endpoint <$> Decode.snapshot [params source capacity, regs source capacity, jpeg, bytes]
      inspect source capacity endpoint bytes = do
        started <- snap source capacity 0 B.empty
        completed <- snap source capacity endpoint bytes
        Decode.inspect fixture started completed
  case inspect 0x40100000 1024 0x40200400 pixels of
    Right _ -> pure ()
    Left refusal -> ioError (userError ("embedded JPEG alias acceptance: " ++ refusal))
  check "all model-supported alias bits normalize identically" (Decode.physical 0xc0100000 == Decode.physical 0x40100000)
  check "wrong completed extent is rejected" (isLeft (inspect 0x40100000 1024 0x402003c0 pixels))
  check "short destination allocation is rejected" (isLeft (inspect 0x40100000 960 0x40200400 (B.take 960 pixels)))
  check "nonzero visible-row padding is rejected" (isLeft (inspect 0x40100000 1024 0x40200400 (B.take 36 pixels <> B.singleton 1 <> B.drop 37 pixels)))
  check "nonzero bottom padding is rejected" (isLeft (inspect 0x40100000 1024 0x40200400 (B.take 576 pixels <> B.singleton 1 <> B.drop 577 pixels)))
  check "physical range cannot wrap" (isLeft (inspect 0xdfffff00 1024 0x40200400 pixels))
  case (snap 0x40100000 1024 0 B.empty, snap 0x40100000 1024 0x40200400 pixels) of
    (Right started, Right completed) -> do
      check "unrelated source bytes are rejected" (isLeft (Decode.inspect (B.replicate (B.length fixture) 0) started completed))
      check "ambiguous embedded-image identity is refused" (isLeft (Decode.inspect (jpeg <> jpeg) started completed))
      case Decode.inspect fixture started completed of
        Right (Object _) -> pure ()
        _ -> ioError (userError "independent comparison is missing")
      case inspect 0x40100000 1024 0x40200400 (B.singleton 0 <> B.drop 1 pixels) of
        Right (Object report) -> check "visible pixel disagreement is reported without tolerance qualification" (KM.lookup "exact_match" report == Just (Bool False))
        _ -> ioError (userError "visible pixel disagreement became a layout refusal")
      binding <- either (ioError . userError) pure (Decode.completionBinding completed (Decode.OutputEvent 0x87654321))
      let patch offset value bytes = B.take offset bytes <> wordsAt 4 [(0,value)] <> B.drop (offset+4) bytes
          shared = B.take 0x1a0 (wordsAt 0x210 [(0x198,0x14eae2c)]) <> B.drop 8 (params 0x40100000 1024)
          request = wordsAt 0xb0 [(8,18),(12,9),(0x10,32),(0x14,16),(0x18,0x40100000),(0x1c,n)
            ,(0x20,0x40200000),(0x24,1024),(0x30,0x87654321),(0x4c,0x121fec8),(0x50,0x121ff20)]
      check "completed output reaches its released request" (isRight (Decode.inspectRelease binding shared request))
      check "measured completion is handed back before callback execution" (isRight (Decode.inspectHandback binding request))
      mapM_ (\(offset,value) -> check "unbound callback or result is refused at completion entry"
        (isLeft (Decode.inspectHandback binding (patch offset value request))))
        [(0x24,0),(0x28,1),(0x2c,1),(0x30,0x87654320),(0x4c,0x121ff20),(0x50,0)]
      mapM_ (\(offset,value) -> check "wrong request handback refused"
        (isLeft (Decode.inspectRelease binding shared (patch offset value request))))
        [(0x24,0),(0x28,1),(0x30,0x87654320),(0x18,0x40100020),(0x1c,n+1),(0x20,0x40200020),(0x10,16),(0x14,8)]
      mapM_ (\(offset,value) -> check "unreleased or mismatched transaction refused"
        (isLeft (Decode.inspectRelease binding (patch offset value shared) request)))
        [(4,0x1e123456),(0x19c,0x76123456),(0x198,0x14eae30),(0x1a0,0x40100020)]
      let constructed = jpeg <> B.replicate 89 0
          countWord = wordsAt 4 [(0,fromIntegral (B.length constructed))]
          constructedParams = B.take 12 (params 0x40100000 1024) <> countWord <> B.drop 16 (params 0x40100000 1024)
          constructedRegs = B.take 12 (regs 0x40100000 1024) <> countWord
          constructedSnapshot output = Decode.snapshot [constructedParams,constructedRegs,constructed,output]
          constructedResult tailBytes = do
            before <- Decode.sourceCopy 0x40100000 jpeg tailBytes
            a <- constructedSnapshot B.empty
            b <- constructedSnapshot pixels
            Decode.inspect fixture (Decode.withSourceCopy before a) (Decode.withEndpoint 0x40200400 b)
      case constructedResult (B.replicate 4096 0) of
        Right (Object _) -> pure ()
        Left refusal -> ioError (userError ("measured construction refused: " ++ refusal))
        _ -> ioError (userError "constructed source report missing")
      check "constructed source requires every trailing byte observed before copy" (isLeft (constructedResult (B.replicate 88 0)))
      check "a changed preserved source tail is rejected" (isLeft (constructedResult (B.singleton 1 <> B.replicate 4095 0)))
    _ -> ioError (userError "invalid synthetic snapshot")
  putStrLn "playback decode semantic boundaries passed"
