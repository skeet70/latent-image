module Main where

import Control.Monad (unless)
import qualified Data.ByteString as B
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import qualified StartupStageContract as Stage
import System.Environment (getArgs)

expectRefused :: Either String a -> IO ()
expectRefused (Left _) = pure ()
expectRefused (Right _) = fail "unexpected negative artifact admission"

main :: IO ()
main = do
  args <- getArgs
  case args of
    [faultImagePath, faultSymbolsPath, normalImagePath, normalSymbolsPath] -> do
      faultImage <- B.readFile faultImagePath
      faultSymbols <- B.readFile faultSymbolsPath
      normalImage <- B.readFile normalImagePath
      normalSymbols <- B.readFile normalSymbolsPath
      let artifact bytes = Stage.parseFirstStageArtifact (U.MediaImage bytes) faultSymbols
      fault <- either fail pure (artifact faultImage)
      hook <- either fail pure (Stage.interleavedTailHook fault)
      unless (Stage.interleavedTailSite hook == 0x40040) (fail "interleaved store does not follow the first installer store")
      normal <- either fail pure (Stage.parseFirstStageArtifact (U.MediaImage normalImage) normalSymbols)
      expectRefused (Stage.interleavedTailHook normal)
      let plan = Stage.firstStageArtifactPlan fault
          start = F.recordGuestBase (F.planRecord plan) + F.slotOffsetBytes (F.planLoadOffset plan)
          offset = fromIntegral (Stage.interleavedTailTarget hook) - start
          replace at value = B.take at faultImage <> U.putWord32 value <> B.drop (at + 4) faultImage
      expectRefused (artifact (replace (offset + 20) 0x40100) >>= Stage.interleavedTailHook)
      expectRefused (artifact (replace (offset + 24) 0x40048) >>= Stage.interleavedTailHook)
      expectRefused (artifact (replace (offset + 12) 0xe3a0302c) >>= Stage.interleavedTailHook)
      putStrLn "startup negative artifact tests pass"
    _ -> fail "expected fault image/symbols and normal image/symbols"
