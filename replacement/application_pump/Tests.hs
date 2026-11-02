module Main where

import Control.Monad (forM_, unless)
import Data.Bits (xor)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Fixtures
import Replacement.ApplicationPump.Admission
import Replacement.ApplicationPump.Artifact
import qualified Replacement.ApplicationPump.Staging as Image5
import qualified FujiAF.UpdateContainer as U
import System.Environment (getArgs)

assert :: Bool -> String -> IO ()
assert premise message = unless premise (fail message)

accept :: B.ByteString -> B.ByteString -> IO ()
accept bytes symbols = do
  _ <- either (fail . show) pure (deriveArtifact bytes symbols)
  proof <- either (fail . show) pure (deriveFrameProof bytes)
  assert (retainedCalls proof == [Prehook,Normalizer,Posthook]) "ordered hook effects differ"
  assert (scratchLow proof < 0 && scratchLow proof >= -64) "invalid stack requirement"

refuse :: AdmissionError -> B.ByteString -> IO ()
refuse reason bytes = assert (case deriveFrameProof bytes of Left actual -> actual == reason; Right _ -> False)
  ("expected refusal: " ++ show reason)

main :: IO ()
main = do
  mapM_ (\bytes -> accept bytes (symbolsFor bytes)) [goodCarrier,compactCarrier,spillCarrier]
  forM_
    [ (UnmodelledPcWrite DataDestination,0,0xe1a0f000)
    , (UnmodelledPcWrite DataDestination,0,0xe28df004)
    , (UnmodelledPcWrite DataDestination,0,0x01a0f000)
    , (UnmodelledPcWrite DataDestination,0,0xe300f000)
    , (UnmodelledPcWrite LoadDestination,0,0xe59df000)
    , (UnmodelledPcWrite LoadDestination,0,0x0590f000)
    , (UnmodelledPcWrite MultipleDestination,0,0xe8908000)
    , (UnmodelledPcWrite MultipleDestination,0,0xe8bdc000)
    , (UnmodelledPcWrite WritebackDestination,0,0xe5af0004)
    , (UnmodelledPcWrite WritebackDestination,0,0xe8bf0001)
    , (ConditionalTransfer,0,0x1afffffe)
    , (ConditionalTransfer,2,0x0bfffffe)
    , (ConditionalInstruction,1,0x028db008)
    , (LoopRejected,0,0xeafffffe)
    , (LoopRejected,1,0xeafffffd)
    , (UnexpectedTransfer,0,0xea000000)
    , (ForbiddenStockFallback,2,callAt 2 0xbfbd20)
    , (UnexpectedCall,2,callAt 2 0xc22f60)
    , (CallOrderRejected,2,callAt 2 0xbfb20c)
    , (CallOrderRejected,5,callAt 5 0xbfb20c)
    , (ResultRejected,6,0xe3a00000)
    , (ResultRejected,6,0xe1a00000)
    , (CalleeSavedRejected,7,0xe8bd8c20)
    , (ReturnRejected,7,0xe12fff14)
    , (MemoryEffectRejected,1,0xe5800000)
    , (MemoryEffectRejected,1,0xe59d0010)
    , (StackRejected,0,0xe24dd001)
    , (UnsupportedInstruction,1,0xe29db008)
    ] $ \(reason,index,word) -> refuse reason (mutate index word goodCarrier)
  mapM_ (refuse CarrierRejected) [B.empty,B.replicate 33 0,B.replicate 36 0,B.replicate 31 0]
  refuse TrailingCodeRejected (compactCarrier <> wordsFor [0xe320f000])
  let badSymbols =
        [ B.empty, symbolsFor goodCarrier <> C.pack "00bfbd24 4 T alias\n"
        , symbolsFor goodCarrier <> C.pack "00bfbd20 0 t $d.0\n"
        , symbolsFor goodCarrier <> C.pack "00bfbd20 0 T xm1_application_pump\n"
        , C.pack "not a symbol row\n"
        , C.unlines (C.pack "bfbd24 20 T xm1_application_pump" :
            filter (not . C.isSuffixOf (C.pack " T xm1_application_pump")) (C.lines (symbolsFor goodCarrier)))
        ]
  forM_ badSymbols $ \symbols -> assert (case deriveArtifact goodCarrier symbols of Left SymbolRejected -> True; _ -> False)
    "malformed or aliased symbol table admitted"
  assert (case deriveArtifact (B.replicate 36 0) (symbolsFor goodCarrier) of Left ExtentRejected -> True; _ -> False)
    "carrier overflow admitted"
  assert (case canonicalSource B.empty B.empty of Left SourceMismatch -> True; _ -> False) "synthetic canonical source admitted"
  arguments <- getArgs
  case arguments of
    [] -> pure ()
    [imagePath,symbolsPath] -> do
      image <- B.readFile imagePath
      symbols <- B.readFile symbolsPath
      accept image symbols
    [datPath,imagePath,linkedPath,symbolsPath] -> do
      dat <- B.readFile datPath
      original <- B.readFile imagePath
      linked <- B.readFile linkedPath
      symbols <- B.readFile symbolsPath
      source <- either (fail . show) pure (canonicalSource dat original)
      artifact <- either (fail . show) pure (deriveArtifact linked symbols)
      staged <- either (fail . show) pure (stageImage5 source artifact)
      let image = stagedBytes staged
          offset = 0xbfbd20 - 0xbf1a00
          patch = B.take 32 (B.drop offset image)
          stock = B.take 32 (B.drop offset original)
      assert (B.length image == B.length original && B.take offset image == B.take offset original
        && B.drop (offset+32) image == B.drop (offset+32) original) "staging changes bytes outside the carrier"
      assert (B.take (B.length linked) patch == linked
        && B.drop (B.length linked) patch == B.concat (replicate ((32-B.length linked) `div` 4) (wordsFor [0xe320f000])))
        "staged carrier differs from the admitted emitted code and declared padding"
      stockArtifact <- either (fail . show) pure (deriveArtifact stock (symbolsFor stock))
      assert (case stageImage5 source stockArtifact of Left StockContentsRejected -> True; _ -> False)
        "stock contents admitted as an owned replacement"
      assert (case canonicalSource (dat <> B.singleton 0) original of Left SourceMismatch -> True; _ -> False)
        "modified DAT admitted as canonical"
      assert (case canonicalSource dat (original <> B.singleton 0) of Left SourceMismatch -> True; _ -> False)
        "modified image5 admitted as canonical"
    ["--package",datPath,expectedPath,candidatePath,carriedPath] -> do
      dat <- B.readFile datPath
      expected <- B.readFile expectedPath
      candidate <- B.readFile candidatePath
      carried <- B.readFile carriedPath
      either (fail . show) pure (Image5.validatePackagedDelta dat expected candidate carried)
      let check reason actual = assert (actual == Left reason) ("expected package refusal: " ++ show reason)
          flipByte index bytes = B.take index bytes <> B.singleton (B.index bytes index `xor` 1) <> B.drop (index+1) bytes
      check Image5.PackageMalformed (Image5.validatePackagedDelta B.empty expected candidate carried)
      check Image5.PackageDeltaRejected (Image5.validatePackagedDelta dat expected dat carried)
      check Image5.PackageDeltaRejected (Image5.validatePackagedDelta dat expected (flipByte 0 candidate) carried)
      check Image5.PackageMalformed (Image5.validatePackagedDelta dat expected (flipByte U.mainChecksumOffset candidate) carried)
      check Image5.PackageExtentRejected (Image5.validatePackagedDelta dat B.empty candidate carried)
      check Image5.PackageDecodeRejected (Image5.validatePackagedDelta dat expected candidate (carried <> B.singleton 0))
    _ -> fail "usage: application-pump-tests [LINKED_BIN SYMBOLS | SOURCE_DAT IMAGE5 LINKED_BIN SYMBOLS | --package SOURCE_DAT EXPECTED_IMAGE5 CANDIDATE_DAT CARRIED_IMAGE5]"
