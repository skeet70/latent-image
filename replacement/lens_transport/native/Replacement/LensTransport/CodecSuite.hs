{-# LANGUAGE OverloadedStrings #-}
module Replacement.LensTransport.CodecSuite (runSuite, sourceIdentity) where

import qualified ArmElf as E
import Control.Monad (forM, forM_, unless)
import Data.Bits ((.|.), shiftL)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.CachedNativeBatch (cachedObservedBatch)
import Emulation.NativeObservation
import Emulation.OwnedCase
import Emulation.Sha256 (sha256, sha256Hex)
import System.FilePath ((</>))

sourceIdentity :: String
sourceIdentity = "x70view55436fc85c0560dd252b0410af6f2427fe198cd3; image4 base0061c400 SHA256 f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16; tu_006bdacc.c NONMATCHING guide; raw extents independently admitted"

data Operation = Command | ReadFrame | WriteFrame | Ack | Response deriving (Eq, Show)
data Role = Stock | Owned deriving (Eq, Show)
data Consumer = Absent | Immediate | Deferred deriving (Eq, Show)
data Fixture = Fixture { ident :: String, operation :: Operation, input :: Word32, width :: Word32, enabled :: Word32, sequenceNumber :: Word32, recoveryState :: Word32, busyCount :: Word32, queued :: Word32, previousResponse :: Word32, consumer :: Consumer } deriving Show

extents :: [(Word32, Word32, String)]
extents =
  [ (0x6bf724,220,"d1dfa7d449ae13d5ff704cd3713318941623db2d2af5c6fbb5945ebff98fc685")
  , (0x6bf50c,240,"8d34fbbe56614f7f93f29f93d4b35ab64e98c8807d44559b74b4c160c7c3e9f1")
  , (0x6bf5fc,296,"94ebbc114a3e5dd9bee2e3c143b3c70edd71a2563636832fafdeb7853d10ce02")
  , (0x6bdf44,1060,"a767f45e94b0811867a9b9bc534590482d87da5d5005f12a4ed688d3c7739e15")
  , (0x6be368,1120,"b20a19fd5a73c0ade3aa7c813e26ed961d8a8748bdc28f824df586b7866325c5")
  , (0x6bdd54,164,"5d44617fc00c80f0d407396d42112d3f02288796c3e6de0b38e3bf7b5b3fcd15")
  , (0x722054,292,"fdd89402855fb6d349f227be3f77f6d84336f1dfd1ff5e4ffe5bb88e27d8187f")
  , (0x7221c8,80,"95075b7d736be75661473a2b8d814e2e3b0335d6dd106d90052d1877e1ebf4cf")
  , (0x7221a0,40,"c129dcbee17753f3a8c22b072f38acfbfa7aa7e3dabf88b8adde1f0c5099b870")
  ]

fixtures :: [Fixture]
fixtures = zipWith (\n f -> f { ident = "codec-" ++ show n ++ "-" ++ ident f }) [0 :: Int ..] $
  [ base { ident = "command", operation = Command, input = word, sequenceNumber = seqn, queued = count }
  | word <- [0,1,0xffffffff,0x12345678,0x80008040], seqn <- [0,7], count <- [0,17,18] ] ++
  [ base { ident = operationLabel op, operation = op, input = page, width = size, enabled = flag }
  | op <- [ReadFrame,WriteFrame], page <- [0,0x1234,0xffff], size <- [0,1,2,31,1024,2076], flag <- [0,1] ] ++
  [ base { ident = operationLabel op, operation = op, input = word, recoveryState = mode, busyCount = busy }
  | op <- [Ack,Response], word <- [0xc33ca55a,0xa55a3cc3,0,1,0x08008054,0x08108064,0x08008040,0x08000400,0x000003d0,0x00000042,0x08018074,0x00400002], mode <- [0,1,2], busy <- [0,2,0xffffffff] ] ++
  [ base { ident = "previous-response-consumer", operation = Ack, input = word, sequenceNumber = seqn, previousResponse = previous, consumer = completion, recoveryState = mode }
  | word <- [0x08008054,0x08108064,0x0f00804c,0x0f10805c], seqn <- [0,7], previous <- [0,0xa55a3cc3,0x12345678], completion <- [Absent,Immediate,Deferred], mode <- [0,1] ]
  where base = Fixture "" Command 0 0 0 0 0 0 0 0 Absent

operationLabel :: Operation -> String
operationLabel Command = "command"
operationLabel ReadFrame = "read-frame"
operationLabel WriteFrame = "write-frame"
operationLabel Ack = "ack"
operationLabel Response = "response"

symbolName :: Operation -> String
symbolName Command = "lens_codec_command"
symbolName ReadFrame = "lens_codec_read"
symbolName WriteFrame = "lens_codec_write"
symbolName Ack = "lens_codec_ack"
symbolName Response = "lens_codec_response"
stockEntry :: Operation -> Word32
stockEntry Command = 0x6bf724
stockEntry ReadFrame = 0x6bf50c
stockEntry WriteFrame = 0x6bf5fc
stockEntry Ack = 0x6bdf44
stockEntry Response = 0x6be368
parser :: Operation -> Bool
parser op = op == Ack || op == Response
slice :: B.ByteString -> Word32 -> Word32 -> B.ByteString
slice bytes address size = B.take (fromIntegral size) (B.drop (fromIntegral (address - 0x61c400)) bytes)
patch :: B.ByteString -> Int -> Word32 -> B.ByteString
patch bytes offset value = B.take offset bytes <> word32le value <> B.drop (offset+4) bytes

-- Critical sections, routing, diagnostics and the transport publication edge are synthetic
-- external interfaces; every body is a return.
services :: [(Word32, [Word32])]
services = [(0x7235d8,[0xe3a00000,0xe12fff1e]),(0x7236b0,[0xe12fff1e]),(0x64982c,[0xe12fff1e]),(0x6be7c8,[0xe12fff1e]),(0x6bde9c,[0xe12fff1e]),(0x6bf218,[0xe12fff1e])]

makeCase :: B.ByteString -> E.Elf -> Role -> Fixture -> CaseSpec
makeCase image elf role f = CaseSpec
  { caseId = ident f, caseProvenance = sourceIdentity
  , caseExclusion = "Synthetic state, queue, payload, receive bytes and bounded caller callback consumer. The event-3 publication edge at 0x006bf218 is a synthetic return, so no dispatcher body or handler runs. External diagnostics discarded. No physical locking, timing, IRQ, timeout, reset or teardown qualification. Writeframe uninitialized tail remains caller-owned."
  , caseEntry = guest entry, caseExit = guest stop
  , caseRegions = code ++ external ++ memory
  , caseFunctions = [(guest a,z) | (a,z) <- functions ++ [(a,fromIntegral (4 * length body)) | (a,body) <- services ++ [(0x203000,consumerBody)]]]
  , caseInitial = [(r,maybe (seed r) id (lookup r arguments)) | r <- [R0 .. R12]] ++ [(Sp,0x308000),(Lr,0x202000),(Cpsr,0x10)]
  , caseExpectedStatus = Returned, caseExpectedRegisters = (Sp,0x308000) : [(r,seed r) | r <- [R4 .. R11]]
  , caseExpectedMemory = [], caseIgnoredMemory = [], caseCp14Identity = Nothing, caseKernel = Nothing
  }
  where
    guest = GuestAddress . fromIntegral
    region name address perm bytes = mkRegion (RegionId name) (guest address) (B.length bytes) perm bytes
    entry = if role == Stock then stockEntry (operation f) else case [E.symbolValue s | s <- E.elfSymbols elf, E.symbolName s == symbolName (operation f)] of
      [value] -> value
      _ -> error "admitted symbol disappeared"
    stop = 0x202000
    allocated = filter (\s -> E.sectionAllocated s && E.sectionSize s > 0) (E.elfSections elf)
    code = if role == Stock then [region ("stock-" ++ show a) a ReadExecute (slice image a z) | (a,z,_) <- extents]
      else [region ("owned-" ++ show (E.sectionAddress s)) (E.sectionAddress s) ReadExecute (B.pack [maybe (error "admitted ELF hole") id (E.byteAt elf a) | a <- [E.sectionAddress s .. E.sectionAddress s + E.sectionSize s - 1]]) | s <- allocated]
    functions = if role == Stock then [(a,z) | (a,z,_) <- extents] else M.toList (M.fromListWith max [(E.symbolValue s,E.symbolSize s) | s <- E.elfSymbols elf, E.symbolKind s == E.FunctionSymbol, E.symbolSize s > 0])
    external = [region ("external-" ++ show a) a ReadExecute (assemble body) | (a,body) <- services] ++ [region "caller-consumer" 0x203000 ReadExecute (assemble consumerBody)]
    consumerBody = [0xe3042000,0xe3402020,0xe8820003,if consumer f == Deferred then 0xe3a00001 else 0xe3a00000,0xe12fff1e]
    state = foldl (\bytes (offset,value) -> patch bytes offset value) (B.replicate 0xa0 0)
      [(0x1c,3),(0x40,0x0b008000),(0x44,0x40),(0x48,0x40),(0x4c,previousResponse f),(0x50,if consumer f == Absent then 0 else 0x203000),(0x8c,sequenceNumber f),(0x90,sequenceNumber f),(0x94,7),(0x98,busyCount f),(0x9c,recoveryState f)]
    queue = patch (patch (patch (B.replicate 0xac 0xa5) 0xa0 19) 0xa4 3) 0xa8 (queued f)
    payload = B.pack (take 2076 (cycle [0,255,17,128,90,1,254]))
    receive = B.reverse (word32le (input f)) <> B.replicate (0x820-4) 0xa5
    memory = [region "state" 0x95d3c0 ReadWrite state,region "queue" 0x95b254 ReadWrite queue,region "machine" 0x955dd0 ReadWrite (word32le 6 <> B.replicate 12 0),region "tx" 0x4096d000 ReadWrite (B.replicate 0x820 0xa5),region "rx" 0x4096d820 ReadWrite receive,region "payload" 0x210000 ReadOnly payload,region "stack" 0x300000 ReadWrite (patch (B.replicate 0x10000 0xa5) 0x8000 0),region "return" 0x202000 ReadExecute (word32le 0xe7f000f0)]
      ++ [region "consumer-output" 0x204000 ReadWrite (B.replicate 8 0xa5)]
    arguments = case operation f of
      Command -> [(R0,input f)]
      ReadFrame -> [(R0,input f .|. (width f `shiftL` 16)),(R1,enabled f),(R2,enabled f),(R3,0)]
      WriteFrame -> [(R0,input f),(R1,0x210000),(R2,width f),(R3,enabled f)]
      _ -> []

seed :: Register -> Word32
seed r = 0xa5a50000 + fromIntegral (fromEnum r)

compareResult :: Fixture -> NativeObservation -> NativeObservation -> [String]
compareResult f stock owned =
  [label ++ " status" | (label,o) <- [("stock",stock),("owned",owned)], nativeStatus o /= Returned] ++
  ["published bytes" | publication stock /= publication owned] ++
  ["retained consumer state" | boundaryState stock /= boundaryState owned] ++
  ["caller callback output" | regionBytesOf stock (RegionId "consumer-output") /= regionBytesOf owned (RegionId "consumer-output")] ++
  ["parser result" | parser (operation f), registerValue stock R0 /= registerValue owned R0] ++
  ["parser ABI " ++ registerLabel r | parser (operation f), r <- [R4 .. R11] ++ [Sp], registerValue stock r /= registerValue owned r] ++
  ["stack guard " ++ label | (label,o) <- [("stock",stock),("owned",owned)], let bytes = regionBytesOf o (RegionId "stack"), B.take 0x4000 bytes /= B.replicate 0x4000 0xa5 || B.drop 0x8004 bytes /= B.replicate 0x7ffc 0xa5]
  where
    boundaryState o = B.concat [B.take 4 (B.drop offset (regionBytesOf o (RegionId "state"))) | offset <- [0x78,0x8c,0x90,0x94,0x98,0x9c]]
      <> B.drop 160 (regionBytesOf o (RegionId "queue"))
    publication o = case operation f of
      Command -> B.take 160 (regionBytesOf o (RegionId "queue"))
      ReadFrame -> regionBytesOf o (RegionId "tx")
      WriteFrame -> regionBytesOf o (RegionId "tx")
      Response -> B.take 4 (B.drop 0x40 (regionBytesOf o (RegionId "state")))
      Ack -> B.empty

runSuite :: FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO [(String,[String])]
runSuite runner cache directory imagePath elfPath = do
  image <- B.readFile imagePath
  unless (sha256Hex (sha256 image) == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") (fail "image4 identity differs")
  forM_ extents $ \(a,z,digest) -> unless (sha256Hex (sha256 (slice image a z)) == digest) (fail ("raw stock extent differs: " ++ show a))
  elf <- either fail pure . E.parseElf =<< B.readFile elfPath
  forM_ [Command,ReadFrame,WriteFrame,Ack,Response] $ \op -> case [s | s <- E.elfSymbols elf, E.symbolName s == symbolName op, E.symbolKind s == E.FunctionSymbol, E.symbolSize s > 0] of
    [s] | E.InSection index <- E.symbolSection s, index >= 0, index < length (E.elfSections elf), let section = E.elfSections elf !! index, E.sectionExecutable section, E.sectionAllocated section, E.symbolValue s >= E.sectionAddress section, toInteger (E.symbolValue s) + toInteger (E.symbolSize s) <= toInteger (E.sectionAddress section) + toInteger (E.sectionSize section) -> pure ()
    _ -> fail ("missing owned function: " ++ symbolName op)
  forM_ (filter (\s -> E.sectionAllocated s && E.sectionSize s > 0) (E.elfSections elf)) $ \s ->
    unless (E.sectionType s == 1 && not (E.sectionWritable s) && E.sectionAddress s >= 0x100000 && toInteger (E.sectionAddress s) + toInteger (E.sectionSize s) <= 0x180000) (fail "owned allocated section escapes admitted bank")
  fmap concat $ forM (zip [0 :: Int ..] (chunks fixtures)) $ \(index,selected) -> do
    observations <- forM [Stock,Owned] $ \role -> do
      batch <- writeBatch (directory </> show index </> show role) (map (makeCase image elf role) selected)
      (_,results) <- cachedObservedBatch cache runner batch
      pure (M.fromList [(caseIdText key,value) | (key,value) <- results])
    case observations of
      [stock,owned] -> pure [(ident f,case (M.lookup (ident f) stock,M.lookup (ident f) owned) of
          (Just a,Just b) -> compareResult f a b ++
            ["status negative control" | null (compareResult f a a { nativeStatus = MemoryGuard })] ++
            ["publication negative control" | let changed = if parser (operation f) then a { nativeRegisters = M.insert R0 (registerValue a R0 + 1) (nativeRegisters a) } else mutateRegion (RegionId (if operation f == Command then "queue" else "tx")) 0 a, null (compareResult f a changed)] ++
            ["retained state negative control" | offset <- [0x78,0x8c,0x90,0x94,0x98,0x9c], null (compareResult f a (mutateRegion (RegionId "state") offset a))] ++
            ["queue cursor negative control" | offset <- [0xa0,0xa4,0xa8], null (compareResult f a (mutateRegion (RegionId "queue") offset a))] ++
            ["caller callback negative control" | null (compareResult f a (mutateRegion (RegionId "consumer-output") 0 a))]
          _ -> ["missing observation"]) | f <- selected]
      _ -> fail "role count"
  where
    chunks [] = []
    chunks rest = let (batch,remaining) = splitAt 32 rest in batch : chunks remaining

mutateRegion :: RegionId -> Int -> NativeObservation -> NativeObservation
mutateRegion target offset observation = observation
  { nativeMemory = [(key,if key == target then patch bytes offset (readWordAt bytes offset + 1) else bytes) | (key,bytes) <- nativeMemory observation] }
