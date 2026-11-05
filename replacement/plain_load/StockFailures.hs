{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM, unless)
import Data.Aeson (object, (.=))
import qualified Data.ByteString as B
import Data.Word (Word32)
import Emulation.NativeObservation
import Emulation.OwnedCase
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.JSON (encodeCanonical)
import System.Directory (makeAbsolute, removeDirectoryRecursive)
import System.Environment (getArgs)

-- These services and memory-backed registers are synthetic, not a peripheral model.
data Route = Aggregate Word32 Word32 | Reader Bool | Joined Bool | Stop Word32 | ReaderStop
  deriving Show

routes :: [Route]
routes = [Aggregate 21 0, Aggregate 4 6, Aggregate 3 7, Aggregate 7 3, Aggregate 10 4,
          Reader True, Reader False, Joined True, Joined False, Stop 6000, Stop 6001, Stop 6002, Stop 0, ReaderStop]

address :: Word32 -> GuestAddress
address = GuestAddress . fromIntegral

region :: String -> Word32 -> Int -> Permission -> B.ByteString -> CaseRegion
region name base size = mkRegion (RegionId name) (address base) size

wordsAt :: Int -> [(Int, Word32)] -> B.ByteString
wordsAt size values = foldl put (B.replicate size 0) values
  where put bytes (offset, value) = B.take offset bytes <> word32le value <> B.drop (offset + 4) bytes

fixture :: B.ByteString -> Int -> Route -> CaseSpec
fixture image index route = CaseSpec
  { caseId = "stock-read-failure-" ++ show index
  , caseProvenance = "X-M1 1.50 stock bytes; x70view c44204c8c1c87e00ed15a7d33572968afadd1b29 NONMATCHING 0x6fc1bc, 0x6f1440, 0x6ee9ec; byte-confirmed expectations. Register fixtures, service bodies, geometry and handles are synthetic."
  , caseExclusion = "Conditional software control flow only; excludes natural reachability, peripheral/hardware fidelity, physical deadlines and image5 consequences."
  , caseEntry = address entry, caseExit = address 0x100000
  , caseRegions = executable ++ memory
  , caseFunctions = [(address base, fromIntegral (B.length bytes)) | (base, bytes) <- bodies, base /= 0x100000]
  , caseInitial = [(R0, if aggregate then 0x200000 else if stopOnly then 1 else 0), (R1, if aggregate then 0x200100 else 1), (R2, if aggregate then 1 else 0x210000), (R3, 0), (Sp, 0x308000), (Lr, 0x100000), (Cpsr, 0x13)] ++ [(reg,0) | reg <- [R4 .. R12]]
  , caseExpectedStatus = if busy then InstructionBudget else Returned
  , caseExpectedRegisters = if busy then [(R2, 0xfff52890)] else [(R0, expected), (Sp, 0x308000)]
  , caseExpectedMemory = [], caseCp14Identity = Nothing
  , caseKernel = Just (KernelCase (InstructionLimit budget) Nothing Nothing Nothing Nothing [])
  }
  where
    aggregate = case route of Aggregate _ _ -> True; Joined _ -> True; _ -> False
    stopOnly = case route of Stop _ -> True; _ -> False
    busy = case route of Stop n -> n /= 0; ReaderStop -> True; _ -> False
    releaseFails = case route of Reader failed -> failed; Joined failed -> failed; _ -> False
    expected = case route of Aggregate _ result -> result; Joined failed -> if failed then 0 else 6; Reader failed -> if failed then 21 else 4; _ -> 0
    budget = case route of Stop n | n /= 0 -> n; _ -> 6000
    entry = if aggregate then 0x6fc1bc else if stopOnly then 0x6ee9ec else 0x6f1440
    stock base end = (base, B.take (fromIntegral (end-base)) (B.drop (fromIntegral (base-0x61c400)) image))
    ret value = assemble [0xe3a00000 + value, 0xe12fff1e]
    service base value = (base, ret value)
    readerBodies = [stock 0x6f1440 0x6f1a74,
      (0x6ee75c, assemble [0xe3a02001,0xe5812000,0xe3a00000,0xe12fff1e]),
      service 0x6ee81c (if releaseFails then 1 else 0),
      service 0x6f0b90 4] ++
      [service base 0 | base <- [0x6ddca8,0x6ee648,0x6eeb04,0x6ee8bc,0x6dde18,0x6dddc8,0x6f0df4,0x6ddd08,0x6ddd28,0x723cb8,0x6ee6b8,0x6edecc,0x6ddd48]] ++
      (if routeIsReaderStop then stopBodies else [service 0x6ee9ec 0])
    routeIsReaderStop = case route of ReaderStop -> True; _ -> False
    stopBodies = [stock 0x6ee9ec 0x6eeb04,
      (0x657384,assemble [0xe3a0c001,0xe583c000,0xe3a00000,0xe12fff1e])]
    bodies = [(0x100000, assemble [0xe320f000])] ++
      (if aggregate then [stock 0x6fc1bc 0x6fc408,service 0x6fbac4 0,service 0x6fbadc 0] else []) ++
      (case route of Aggregate status _ -> [service 0x6fc138 status]; Joined _ -> stock 0x6fc138 0x6fc198 : readerBodies; Stop _ -> stopBodies; _ -> readerBodies)
    executable =
      [region "stock-and-synthetic-services" 0x650000 0xe0000 ReadExecute
        (foldl place (B.replicate 0xe0000 0) [(base,bytes) | (base,bytes) <- bodies, base /= 0x100000]),
       region "return" 0x100000 4 ReadExecute (assemble [0xe320f000])]
    place bytes (base, body) =
      let offset = fromIntegral (base - 0x650000)
      in B.take offset bytes <> body <> B.drop (offset + B.length body) bytes
    memory =
      [region "request" 0x200000 0x1000 ReadWrite (wordsAt 0x1000 [(0,0),(4,1),(12,0x210000),(16,0),(20,1)]),
       region "destination" 0x210000 0x1000 ReadWrite B.empty,
       region "stack" 0x300000 0x10000 ReadWrite (wordsAt 0x10000 [(0x8000,0),(0x8004,1),(0x8008,0),(0x800c,0x200100)]),
       region "geometry" 0x829000 0x1000 ReadOnly (wordsAt 0x1000 [(0x84c,0x00010200)]),
       region "dma-table" 0x812000 0x1000 ReadOnly B.empty,
       region "geometry-index" 0x918000 0x1000 ReadOnly B.empty,
       region "bad-list-pointer" 0x955000 0x1000 ReadOnly (wordsAt 0x1000 [(0x2f0,0x200200)]),
       region "channel-state" 0x916000 0x1000 ReadWrite (wordsAt 0x1000 [(0x528,2)]),
       region "synthetic-registers" 0xfff52800 0x100 ReadWrite (wordsAt 0x100 [(0x90,if busy then 1 else 0)])]

main :: IO ()
main = do
  args <- getArgs
  case args of
    [runner, imagePath, output, "--native-jobs", "1"] -> do
      image <- B.readFile imagePath
      runnerBytes <- B.readFile runner
      unless (sha256Hex (sha256 image) == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") (fail "stock image4 identity differs")
      root <- makeAbsolute output
      let specs = zipWith (fixture image) [0..] routes
      batch <- writeBatch (root ++ ".cases") specs
      observations <- loadObservedBatch runner batch
      results <- forM (zip3 routes specs observations) $ \(route, spec, (_, observation)) -> do
        let pc = registerValue observation Pc
            spinning = nativeStatus observation == InstructionBudget
            pending = case route of ReaderStop -> registerValue observation R6 == 4; _ -> True
            holds = nativeStatus observation == caseExpectedStatus spec && all (\(reg,value) -> registerValue observation reg == value) (caseExpectedRegisters spec)
              && (not spinning || pc `elem` [0x6eea34,0x6eea38,0x6eea3c]) && pending
              && (not spinning || readWordAt (regionBytesOf observation (RegionId "channel-state")) 0x528 == 2)
            InstructionCount instructions = nativeInstructions observation
        pure (holds, object ["case" .= caseId spec,"route" .= show route,"holds" .= holds,"status" .= runnerStatusLabel (nativeStatus observation),"r0" .= registerValue observation R0,"r6" .= registerValue observation R6,"pc" .= pc,"instructions" .= instructions])
      B.writeFile root (encodeCanonical (object
        ["schema" .= (1 :: Int),"image4_sha256" .= sha256Hex (sha256 image),
         "runner_sha256" .= sha256Hex (sha256 runnerBytes),
         "projection" .= ("status, return r0/SP; busy PC/address/channel state; pending timeout r6. Scratch memory is not compared." :: String),
         "results" .= map snd results,"passed" .= all fst results]))
      unless (all fst results) (fail ("stock failure oracle differs: " ++ root))
      removeDirectoryRecursive (root ++ ".cases")
      putStrLn (show (length results) ++ " stock failure cases pass; " ++ root)
    _ -> fail "usage: stock-read-failures RUNNER IMAGE4 REPORT --native-jobs 1"
