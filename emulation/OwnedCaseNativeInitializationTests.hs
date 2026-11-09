{-# LANGUAGE GHC2021 #-}
module Main (main) where

import Control.Monad (forM, forM_)
import qualified Data.ByteString as B
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import Data.Word (Word8, Word32)
import Emulation.EmulatorSlot (reexecInSlot, slotPolicyFromEnvironment)
import qualified Emulation.NativeCase as C
import qualified Emulation.NativeObservation as N
import qualified Emulation.OwnedCase as O
import FujiAF.Host (ensure, withTemporary)
import FujiModNativePaths (fujimodNativeCase)
import System.Directory (canonicalizePath)
import System.Environment (getArgs)
import System.FilePath ((</>))

data Tile = Tile
  { tileIndex :: Int
  , tilePermission :: O.Permission
  , tileBytes :: B.ByteString
  }

data Scenario = RoundTrip | RefuseWrite Tile

main :: IO ()
main = do
  arguments <- getArgs
  policy <- slotPolicyFromEnvironment
  reexecInSlot policy arguments
  runner <- canonicalizePath fujimodNativeCase
  withTemporary "owned-case-native-initialization" $ \root -> do
    let large = tiles (88 * 1024) 7
        other = tiles (88 * 1024) 91
        small = tiles 64 19
        cases = [fixture "boundary-a" large RoundTrip, fixture "boundary-b" other RoundTrip]
          ++ [fixture ("readonly-" ++ show (tileIndex tile)) small (RefuseWrite tile) | tile <- small, not (writable tile)]
          ++ [fixture ("boundary-readonly-" ++ show (tileIndex tile)) large (RefuseWrite tile) | tile <- large, tileIndex tile `elem` [0, 23]]
    ensure (sum (map (B.length . tileBytes) large) > C.sizeBytes) "boundary fixture does not cross the artifact byte limit"
    standalone <- forM cases $ \spec -> do
      path <- O.writeCase (root </> O.caseId spec) spec
      checkAdmission path spec
      observation <- O.loadObserved runner path
      checkObservation spec observation
      pure (C.CaseId (O.caseId spec), observation)
    batch <- O.writeBatch (root </> "batch") cases
    (_, entries) <- C.readBatch batch
    ensure (map C.entryId entries == map fst standalone) "batch case order differs"
    forM_ (zip entries cases) $ \(entry, spec) -> checkAdmission (C.entryPath entry) spec
    observed <- O.loadObservedBatch runner batch
    ensure (observed == standalone) "standalone and batch observations differ"
    forM_ (zip cases observed) $ \(spec, (_, observation)) -> checkObservation spec observation
  putStrLn "owned-case native initializer roundtrip and readonly controls passed"

tiles :: Int -> Word8 -> [Tile]
tiles size seed = [make index | index <- [0 .. 23]]
  where
    make index = Tile index permission bytes
      where
        permission = case index `mod` 3 of
          0 -> O.ReadOnly
          1 -> O.ReadWrite
          _ -> O.ReadExecute
        patternBytes = B.pack [seed + fromIntegral index * 7 + offset | offset <- [0 .. 255]]
        payload = B.take (size + index * 4) (B.concat (replicate ((size + index * 4) `div` 256 + 1) patternBytes))
        bytes = case permission of
          O.ReadExecute -> O.assemble [0xe3a00000 .|. fromIntegral index, 0xe12fff1e] <> B.drop 8 payload
          _ -> payload

writable :: Tile -> Bool
writable tile = case tilePermission tile of
  O.ReadWrite -> True
  _ -> False

executable :: Tile -> Bool
executable tile = case tilePermission tile of
  O.ReadExecute -> True
  _ -> False

tileId :: Tile -> N.RegionId
tileId tile = N.RegionId ("tile-" ++ show (tileIndex tile))

tileAddress :: Tile -> Word32
tileAddress tile = 0x100000 + fromIntegral (tileIndex tile) * 0x20000

address :: Word32 -> N.GuestAddress
address = N.GuestAddress . fromIntegral

loadAddress :: Word32 -> [Word32]
loadAddress value = [immediate 0xe3002000 value, immediate 0xe3402000 (value `shiftR` 16)]
  where
    immediate opcode half = opcode .|. ((half .&. 0xf000) `shiftL` 4) .|. (half .&. 0xfff)

fixture :: String -> [Tile] -> Scenario -> O.CaseSpec
fixture ident regions scenario = O.CaseSpec
  { O.caseId = ident
  , O.caseProvenance = "OwnedCaseNativeInitializationTests.hs: original synthetic A32 initializer consumers"
  , O.caseExclusion = "Serializer and native memory permissions only; no firmware or device behavior."
  , O.caseEntry = address 0x1000
  , O.caseExit = address 0x2000
  , O.caseRegions =
      [ O.mkRegion (N.RegionId "code") (address 0x1000) (B.length code) O.ReadExecute code
      , O.mkRegion (N.RegionId "exit") (address 0x2000) 4 O.ReadExecute (O.word32le 0xe7f000f0)
      , O.mkRegion (N.RegionId "sink") (address 0x4000) (B.length sinkSeed) O.ReadWrite sinkSeed
      , O.mkRegion (N.RegionId "stack") (address 0x8000) 32 O.ReadWrite (B.replicate 32 0xa5)
      ] ++ [O.mkRegion (tileId tile) (address (tileAddress tile)) (B.length (tileBytes tile)) (tilePermission tile) (tileBytes tile) | tile <- regions]
  , O.caseFunctions = (address 0x1000, fromIntegral (B.length code))
      : [(address (tileAddress tile), 8) | tile <- regions, executable tile]
  , O.caseInitial = [(register, initial register) | register <- [N.R0 .. N.R12]]
      ++ [(N.Sp, 0x8020), (N.Lr, 0x2000), (N.Cpsr, 0x10)]
  , O.caseExpectedStatus = case scenario of RoundTrip -> N.Returned; RefuseWrite _ -> N.MemoryGuard
  , O.caseExpectedRegisters = case scenario of
      RoundTrip -> [(N.R0, 23), (N.Sp, 0x8020), (N.Lr, 0x2000), (N.Pc, 0x2000)]
      RefuseWrite _ -> [(N.Sp, 0x8020), (N.Lr, 0x2000)]
  , O.caseExpectedMemory =
      [(N.RegionId "code", code), (N.RegionId "exit", O.word32le 0xe7f000f0), (N.RegionId "sink", sinkFinal), (N.RegionId "stack", B.replicate 32 0xa5)]
      ++ [(tileId tile, finalBytes tile) | tile <- regions]
  , O.caseCp14Identity = Nothing
  , O.caseKernel = Nothing
  }
  where
    initial N.R4 = 0x4000
    initial N.R5 = 0xdeadbeef
    initial _ = 0
    sinkSeed = B.replicate (24 * 12) 0x6d
    code = O.assemble $ case scenario of
      RefuseWrite tile -> loadAddress (tileAddress tile) ++ [0xe5825000, 0xe12fff1e]
      RoundTrip -> [0xe1a0600e] ++ concatMap consume regions ++ [0xe1a0e006, 0xe12fff1e]
    consume tile =
      loadAddress (tileAddress tile) ++ [0xe5920000, store 0 0]
      ++ loadAddress (tileAddress tile + fromIntegral (B.length (tileBytes tile)) - 4)
      ++ [0xe5921000, store 1 4]
      ++ (if writable tile then loadAddress (tileAddress tile) ++ [0xe5825000] else [])
      ++ (if executable tile then loadAddress (tileAddress tile) ++ [0xe12fff32, store 0 8] else [])
      where
        store register offset = 0xe5840000 .|. (register `shiftL` 12) .|. fromIntegral (tileIndex tile * 12 + offset)
    sinkFinal = case scenario of
      RefuseWrite _ -> sinkSeed
      RoundTrip -> B.concat
        [ B.take 4 (tileBytes tile) <> B.drop (B.length (tileBytes tile) - 4) (tileBytes tile)
            <> (if executable tile then O.word32le (fromIntegral (tileIndex tile)) else B.replicate 4 0x6d)
        | tile <- regions
        ]
    finalBytes tile = case scenario of
      RoundTrip | writable tile -> O.word32le 0xdeadbeef <> B.drop 4 (tileBytes tile)
      _ -> tileBytes tile

checkAdmission :: FilePath -> O.CaseSpec -> IO ()
checkAdmission path spec = do
  (_, decoded, _, _) <- C.loadCase path
  ensure (length decoded == length (O.caseRegions spec)) "admitted region count differs"
  forM_ (O.caseRegions spec) $ \(O.CaseRegion original permission) -> do
    case filter ((== N.regionIdOf original) . N.regionIdOf) decoded of
      [actual] -> do
        ensure (C.baseAddress actual == C.baseAddress original && C.regionSize actual == C.regionSize original) "admitted region extent differs"
        ensure (C.regionBytes actual == C.regionBytes original) "admitted initializer bytes differ"
        let expected = case permission of
              O.ReadOnly -> "r"
              O.ReadWrite -> "rw"
              O.ReadExecute -> "rx"
              O.Device -> "rw"
        forM_ "rwx" $ \access ->
          ensure (C.regionAt decoded (C.baseAddress actual) (C.regionSize actual) access == (access `elem` expected)) "admitted region permissions differ"
      _ -> ensure False "admitted region identity differs"

checkObservation :: O.CaseSpec -> N.NativeObservation -> IO ()
checkObservation spec observation = do
  let context = O.caseId spec ++ ": "
  ensure (N.nativeStatus observation == O.caseExpectedStatus spec) (context ++ "unexpected native status: " ++ show (N.nativeStatus observation))
  forM_ (O.caseExpectedRegisters spec) $ \(register, expected) ->
    ensure (O.registerValue observation register == expected) (context ++ "register differs: " ++ show register)
  forM_ (O.caseExpectedMemory spec) $ \(ident, expected) ->
    ensure (O.regionBytesOf observation ident == expected) (context ++ "final bytes differ: " ++ N.regionName ident)
  case O.caseExpectedStatus spec of
    N.MemoryGuard -> ensure (N.provenanceWrites (N.nativeProvenance observation) == Just []) (context ++ "refused store had write effects")
    _ -> pure ()
