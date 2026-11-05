module Main where
import Control.Monad (unless)
import Data.Bits
import qualified Data.ByteString as B
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import Replacement.Image5Init.Admission
import Replacement.Image5Init.Artifact
import System.Environment (getArgs)

ok :: String -> Bool -> IO ()
ok name result = unless result (fail name)
main :: IO ()
main = do
  args <- getArgs
  (bytes,symbols) <- case args of
    [image,table] -> (,) <$> B.readFile image <*> B.readFile table
    _ -> fail "expected image and symbols"
  artifact <- either (fail . show) pure (deriveArtifact bytes symbols)
  let proof = artifactProof artifact
      calls = proofCallSites proof
      mutate :: Word32 -> Word32 -> B.ByteString
      mutate pc value = let offset = fromIntegral (pc-0xd9856c) in B.take offset bytes <> U.putWord32 value <> B.drop (offset+4) bytes
      refuses candidate = case deriveFrameProof candidate of Left _ -> True; Right _ -> False
      at :: Word32 -> Word32
      at pc = U.word32At bytes (fromIntegral (pc-0xd9856c))
      branch pc target = 0xeb000000 .|. (((target-pc-8) `shiftR` 2) .&. 0xffffff)
      checks = [pc | pc <- [0xd985a4,0xd985a8..0xd9856c+fromIntegral (B.length bytes)-4], at pc .&. 0x0fffffff == 0x0afffffe]
  (first,lastCall) <- case (calls,reverse calls) of
    (front:_,back:_) -> pure (callInstruction front,callInstruction back)
    _ -> fail "admitted image has no calls"
  ok "admitted image contains all 21 calls" (length calls == 21)
  ok "wrong call order refuses" (refuses (mutate first (branch first 0x6c62a8)))
  ok "call at wrong SP refuses" (refuses (mutate 0xd985a8 0xe24dd008))
  ok "extra stack store refuses" (refuses (mutate 0xd985a8 0xe58d0000))
  ok "clobbered r4 refuses" (refuses (mutate 0xd985a8 0xe3a04000))
  ok "nonspinning check refuses" (not (null checks) && all (\pc -> refuses (mutate pc 0xe1a00000)) checks)
  ok "tail call refuses" (refuses (mutate lastCall (at lastCall .&. complement (bit 24))))
  ok "oversize image refuses" (refuses (bytes <> B.replicate 324 0))
  putStrLn ("admitted image bytes: " ++ show (B.length bytes))
  print (map (\site -> (callInstruction site, callTarget site, callReturn site)) calls :: [(Word32,Word32,Word32)])
