{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (unless)
import qualified Data.Aeson as J
import Data.Bits ((.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.Set as Set
import Data.Word (Word32)
import qualified Emulation.Image4Staging as Tools
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Replacement.StartupArtifact.Adapters
import Replacement.StartupArtifact.Compose
import Replacement.StartupArtifact.Joins
import Replacement.StartupArtifact.Stage
import Replacement.StartupArtifact.Targets
import Replacement.StartupArtifact.Types
import qualified Replacement.Task1Actor.Artifact as T1
import System.Directory (canonicalizePath)
import System.Environment (getArgs)

check :: String -> Bool -> IO ()
check name condition = unless condition (H.failH ("failed: " ++ name))

right :: Show e => Either e a -> IO a
right = either (H.failH . show) pure

patch :: Int -> B.ByteString -> B.ByteString -> B.ByteString
patch offset replacement bytes = B.take offset bytes <> replacement <> B.drop (offset+B.length replacement) bytes

bl :: Word32 -> Word32 -> Word32
bl pc target = 0xeb000000 .|. (((target-pc-8) `div` 4) .&. 0xffffff)

main :: IO ()
main = do
  args <- getArgs
  (encoder,rebuild,decoder,actorBin,actorSymbols,pumpBin,pumpSymbols,producerBin,producerSymbols,datPath,scratchPath,output) <- case args of
    [a,b,c,d,e,f,g,h,i,j,k,l] -> pure (a,b,c,d,e,f,g,h,i,j,k,l)
    _ -> H.failH "usage: J11CanonicalTests ENCODER REBUILD DECODER ACTOR_BIN ACTOR_SYMBOLS PUMP_BIN PUMP_SYMBOLS PRODUCER_BIN PRODUCER_SYMBOLS DAT SCRATCH OUTPUT"
  tools <- Tools.Tools <$> canonicalizePath encoder <*> canonicalizePath rebuild <*> canonicalizePath decoder
  scratch <- Tools.validateScratchDirectory scratchPath
  source <- loadCanonical tools scratch datPath
  bin <- canonicalizePath actorBin >>= H.readRegular 0x30000
  symbols <- canonicalizePath actorSymbols >>= H.readRegular 0x10000
  artifact <- right (T1.deriveArtifact bin symbols)
  actor <- right (deriveTask1Actor source (LinkedImage (ImageBytes bin) (SymbolBytes symbols)))
  let load kind image names = do
        bytes <- canonicalizePath image >>= H.readRegular 0x10000
        rows <- canonicalizePath names >>= H.readRegular 0x10000
        right (derivePieceOf source (SingleImage kind (LinkedImage (ImageBytes bytes) (SymbolBytes rows))))
      refused result = case result of Left _ -> True; Right _ -> False
      ranges = [Span Image5 (Guest low) (Guest high) | T1.Carrier (T1.CodeAddress low) (T1.CodeAddress high) <- T1.carriers]
  pump <- load ApplicationPump pumpBin pumpSymbols
  producer <- load Image5Producer producerBin producerSymbols
  check "two fixed carriers" (pieceCarriers actor == ranges)
  check "27 semantic sites and distinct held/normal receive paths"
    (length (T1.semanticSites artifact) == 27 && lookup 0xc22fd4 (T1.semanticSites artifact) /= lookup 0xc2314c (T1.semanticSites artifact))
  check "nonzero sparse hole refuses" (refused (T1.deriveArtifact (patch 0x100 (B.singleton 1) bin) symbols))
  let moved = C.unlines [case C.words row of
        [_,size,kind,"xm1_task1_actor"] -> C.unwords ["00c22f64",size,kind,"xm1_task1_actor"]
        _ -> row | row <- C.lines symbols]
  check "moved actor entry refuses" (refused (T1.deriveArtifact bin moved))
  check "trailing code refuses" (refused (T1.deriveArtifact (bin <> U.putWord32 0xe320f000) symbols))
  let pumpCalls = [pc | T1.CallSite (T1.CodeAddress pc) T1.Pump _ <- T1.semanticMap artifact]
  pumpPc <- case pumpCalls of [pc] -> pure pc; _ -> H.failH "pump call missing"
  check "stock actor fallback refuses" (refused (T1.deriveArtifact (patch (fromIntegral (pumpPc-0xbfb124)) (U.putWord32 (bl pumpPc 0xc23274)) bin) symbols))
  check "unexpected retained service refuses" (refused (T1.deriveArtifact (patch (fromIntegral (pumpPc-0xbfb124)) (U.putWord32 (bl pumpPc 0xd36314)) bin) symbols))
  let pieces = [producer,pump,actor]
  composed <- right (compose registeredJoins source pieces)
  check "explicit pump join required" (refused (compose (filter ((/= Task1ActorPump) . joinKind) registeredJoins) source pieces))
  check "explicit producer join required" (refused (compose (filter ((/= Task1ActorProducer) . joinKind) registeredJoins) source pieces))
  let duplicate = actor {pieceKind = PlainProducer, pieceAreas = Set.singleton PlainProducerArea}
  check "carrier overlap refuses" (refused (compose registeredJoins source [actor,duplicate]))
  composedPump <- case [piece | piece <- compositionPieces composed, pieceKind piece == ApplicationPump] of
    [piece] -> pure piece
    _ -> H.failH "composed pump missing"
  let EnumeratedWrites pumpWrites _ = catalogWrites (pieceCatalog composedPump)
  check "pump frame follows owned actor SP" (all (\(WriteRange _ _ high) -> high == 0xa4cde0) pumpWrites)
  (dat,observed) <- stageComposition tools scratch source composed
  check "packaged record5 decodes to composed image5" (targetOf observed Image5 == targetOf (compositionTargets composed) Image5)
  check "image4 remains canonical" (targetOf observed Image4 == targetOf (canonicalTargets source) Image4)
  writeOutputs output dat observed (J.object ["qualification" J..= ("host-composition-only" :: String), "application_observer_qualified" J..= False, "semantic_sites" J..= T1.semanticSites artifact])
  putStrLn "J11 stager host checks passed; application observer qualification not claimed"
