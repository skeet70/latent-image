{-# LANGUAGE OverloadedStrings #-}
module Replacement.LensTransport.ActorSuite (runSuite, sourceIdentity, CaseResult (..), ControlResult (..), ReplyResult (..), StockEvidence (..)) where

import qualified ArmElf as E
import Control.Monad (forM, forM_, unless)
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.List (findIndex, isPrefixOf, nub, sort)
import Data.Maybe (fromMaybe)
import Data.Word (Word32, Word64)
import Emulation.NativeObservation
import Emulation.OwnedCase
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host (get, jsonRead, list, num, str)
import System.FilePath ((</>))

sourceIdentity :: String
sourceIdentity = "x70view55436fc85c0560dd252b0410af6f2427fe198cd3; image4 base0061c400 SHA256 f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16; task 99 body 0x006c28f4, dispatch 0x006c2b6c, loop clear 0x006c2bc4"

imageBase :: Word32
imageBase = 0x61c400

actorBase, sharedBase, codecState, txBase, rxBase :: Word32
actorBase = 0x96aff4
sharedBase = 0x95e1d0
codecState = 0x95d3c0
txBase = 0x4096d000
rxBase = 0x4096d820

ringBase, configBase, flagsBase, callbackStub, exitAddress, stackBase, mmioBase :: Word32
ringBase = 0x204000
configBase = 0x210000
flagsBase = 0x210400
callbackStub = 0x203000
exitAddress = 0x202000
stackBase = 0x308000
mmioBase = 0xfff89000

dispatchEntry, loopClear, loopReceive, loopReceiveCheck :: Word32
dispatchEntry = 0x6c2b6c
loopClear = 0x6c2bc4
loopReceive = 0x6c2b54
loopReceiveCheck = 0x6c2b64

codeLow, codeHigh :: Word32
codeLow = 0x6c1800
codeHigh = 0x6c6c00

tagSend, tagRecv, tagTimr, tagKey, tagDiag, tagFlag, tagDetm, tagDeth, tagGpio, tagDely, tagCall, tagAttach, tagFinish :: Word32
tagSend = 0x53454e44
tagRecv = 0x52454356
tagTimr = 0x54494d52
tagKey  = 0x4b455900
tagDiag = 0x44494147
tagFlag = 0x464c4147
tagDetm = 0x4445544d
tagDeth = 0x44455448
tagGpio = 0x4750494f
tagDely = 0x44454c59
tagCall = 0x43414c4c
tagAttach = 0x41545443
tagFinish = 0x46494e49

data Role = Stock | Owned deriving (Eq, Show)

data Case = Case
  { cId :: String
  , cOpcode :: Word32
  , cState :: Word32
  , cPayload :: [Word32]
  , cFlags :: Word32
  , cRaw :: [(Int, Word32)]
  , cEntry :: Word32
  , cStop :: Word32
  , cExpectShared :: Maybe [Word32]
  , cExpectState :: Maybe Word32
  }

stateName :: Word32 -> String
stateName 0 = "idle"
stateName 1 = "initializing"
stateName 2 = "active"
stateName 3 = "stopping"
stateName 4 = "suspended"
stateName 5 = "detached"
stateName _ = "invalid"

opcodeName :: Word32 -> String
opcodeName 0x7300 = "register"
opcodeName 0x7301 = "setup"
opcodeName 0x7302 = "teardown"
opcodeName 0x7303 = "synchronize"
opcodeName 0x7304 = "start-polling"
opcodeName 0x7305 = "begin-detection"
opcodeName 0x7306 = "ready"
opcodeName 0x7307 = "initialization-finished"
opcodeName 0x7308 = "resume"
opcodeName 0x7309 = "suspend"
opcodeName 0x730a = "finish"
opcodeName 0x730b = "response"
opcodeName 0x730c = "attached"
opcodeName 0x730d = "detached"
opcodeName 0x730e = "poll"
opcodeName 0x730f = "mode-complete"
opcodeName 0x7310 = "teardown-complete"
opcodeName _ = "unknown"

opcodes :: [Word32]
opcodes = [0x7300 .. 0x7310]

payloadFor :: Word32 -> [Word32]
payloadFor 0x7300 = [1, callbackStub]
payloadFor 0x7301 = [7, 1, 1, 0, callbackStub]
payloadFor 0x7302 = [callbackStub]
payloadFor 0x7303 = [callbackStub]
payloadFor 0x7304 = [callbackStub]
payloadFor 0x7307 = [0]
payloadFor 0x7308 = [callbackStub]
payloadFor 0x7309 = [callbackStub]
payloadFor 0x730b = [1, 0, 0, 0, 0, 0]
payloadFor 0x730f = [0xf]
payloadFor _ = []

-- The table at 0x007cd1a4 selects one handler per opcode and state.
routing :: [Case]
routing =
  [ Case { cId = "actor-routing-" ++ opcodeName op ++ "-" ++ stateName st
         , cOpcode = op, cState = st, cPayload = payloadFor op
         , cFlags = 0, cRaw = [], cEntry = dispatchEntry, cStop = loopClear
         , cExpectShared = Nothing, cExpectState = Nothing
         }
  | op <- opcodes, st <- [0 .. 5] ]

effects :: [Case]
effects =
  [ Case { cId = "actor-register-idle", cOpcode = 0x7300, cState = 0
         , cPayload = [1, callbackStub], cFlags = 0, cRaw = [(0x50, 0), (0x54, 0)]
         , cEntry = dispatchEntry, cStop = loopClear
         , cExpectShared = Nothing, cExpectState = Nothing
         }
  , Case { cId = "actor-attached-idle", cOpcode = 0x730c, cState = 0
         , cPayload = [], cFlags = 0, cRaw = []
         , cEntry = dispatchEntry, cStop = loopClear
         , cExpectShared = Just [0, 1, 0xffffff01, 0], cExpectState = Just 0
         }
  , Case { cId = "actor-attached-detached", cOpcode = 0x730c, cState = 5
         , cPayload = [], cFlags = 0, cRaw = []
         , cEntry = dispatchEntry, cStop = loopClear
         , cExpectShared = Just [0, 1, 0xffffff01, 0], cExpectState = Just 5
         }
  , Case { cId = "actor-mode-complete-resume-idle", cOpcode = 0x730f, cState = 0
         , cPayload = [0xf], cFlags = 0, cRaw = [(0x3d8, 0), (0x3dc, 0)]
         , cEntry = dispatchEntry, cStop = loopClear
         , cExpectShared = Just [0, 0, 0xffffff01, 1], cExpectState = Just 0
         }
  , Case { cId = "actor-mode-complete-suspend-idle", cOpcode = 0x730f, cState = 0
         , cPayload = [0x10], cFlags = 0, cRaw = [(0x3d8, 0), (0x3dc, 0)]
         , cEntry = dispatchEntry, cStop = loopClear
         , cExpectShared = Just [0, 0, 0xffffff01, 2], cExpectState = Just 0
         }
  , Case { cId = "actor-teardown-complete-idle", cOpcode = 0x7310, cState = 0
         , cPayload = [], cFlags = 0, cRaw = [(0x2c, 0), (0x3d8, 0), (0x3dc, 0), (0x3e0, 0), (0x4c, 0)]
         , cEntry = dispatchEntry, cStop = loopClear
         , cExpectShared = Nothing, cExpectState = Just 0
         }
  , Case { cId = "actor-wake", cOpcode = 0, cState = 0, cPayload = [], cFlags = 0, cRaw = []
         , cEntry = loopReceive, cStop = loopReceiveCheck
         , cExpectShared = Nothing, cExpectState = Just 0
         }
  ]

cases :: [Case]
cases = effects ++ routing

-- The dispatcher at 0x006c2b6c finds the opcode's position in the 17-entry table at
-- 0x007cd160, then indexes the 102-entry handler table at 0x007cd1a4 with
-- `state + 6 * position`.
opcodeTable, handlerTable :: Word32
opcodeTable = 0x7cd160
handlerTable = 0x7cd1a4

-- Handler bodies as the pinned decomp commit bounds them, each authenticated against image4
-- before a case runs. The evidence below counts executed instructions inside the selected body.
handlerExtents :: [(Word32, Word32, String)]
handlerExtents =
  [ (0x006c1858, 24, "aab533f64e7d3f50628f3b74c122bcfaae90f7a06e1282e0e04e4d03d9706543")
  , (0x006c1870, 220, "1ca0484035ad97fb8dfd500392ad0e1adc401f527f20878bb43fe7133274d689")
  , (0x006c194c, 156, "1149b5dd8052dc43441df3665e148fdb0d5e56d9e252662bcf411c3ce43697ec")
  , (0x006c1bcc, 252, "e040c603caa5219f12728fcebb007571e881fbec4464f0594f3bbd4f14f86f8f")
  , (0x006c1cc8, 308, "8998d9386d59839aece1cd61125023a740f0a2b783162a0ac62a0dc77c8de46f")
  , (0x006c1e18, 212, "56715fb0a469ac62b440f538a2aee00b4e57f33fd2a5291985824b8b888aa111")
  , (0x006c1eec, 44, "fd19c30ce59bfbe6935579cc395f4dd8d884724edf593862849cd8e8ad40eeb1")
  , (0x006c20b4, 80, "024f7c45f6c87d7d742988ccfa298a8c4cc8bcf46cba9dd98adef422b82a5ca7")
  , (0x006c21d4, 116, "227c3b4b57603bae2fd6309b985b1ac38bd75e4e18ac36429dc05ad0bb697243")
  , (0x006c2248, 336, "808f394e1140aa081300d95ba1f722ea29a4e7237e5db1089de3904e4a455376")
  , (0x006c2398, 180, "c8042ee2d69c35001b02f8f4d23230ab9705cbdf86ee3260796ad38fe599eff7")
  , (0x006c244c, 160, "a097d77a6085690a539fc164b10a3fcb4425a0b7a28dcc14f6aaf0a350346c06")
  , (0x006c2668, 36, "529a7775c2399f64944b0d7294ed1798199494f1cdd29df44caf2c6cca2590c7")
  , (0x006c268c, 96, "9abdd1fc8fa00f66df8e9bdfd1831b79022a4c91a7d77c3b1c636e6391249173")
  , (0x006c26ec, 108, "175664bc3f38d72eed0de5e0a67a3fc08d34a5ee6b6d07ebed5388f58d1ba919")
  , (0x006c27f8, 252, "226ad2093bb5eb16c648e59b8cab979fa0c1e4f312eb2976117438595f08c6df")
  , (0x006c3144, 172, "823a1fd6c261ac838be1ce2c618420acfeee4584e547036d419c336506083daf")
  , (0x006c31f0, 972, "18784560fbd632c88829458f18ff9cd6bbdd4d3d824c131218c0b94b9b841936")
  , (0x006c3b70, 3312, "4b867ecad8f6a00f9411f9fec017be21e288cee80bc3b3b94c9f751c97e6a2f0")
  , (0x006c4c94, 2772, "4aec6f730e1e13bda6d78844b4edd9c5d10f389f77095edf30cb701476fd4eaf")
  ]

-- The handler the dispatcher selects for one staged record, or the receive call's own extent for
-- the queue-empty wake case.
handlerFor :: B.ByteString -> Case -> (Word32, Word32)
handlerFor image c
  | cEntry c == loopReceive = (loopReceive, loopReceiveCheck - loopReceive)
  | otherwise =
      let position = fromMaybe (error ("opcode absent from 0x007cd160: " ++ show (cOpcode c)))
            (findIndex (== cOpcode c) [readWordAt (slice image opcodeTable 0x44) (4 * index) | index <- [0 .. 16 :: Int]])
          state = cState c
          handler = readWordAt (slice image handlerTable 0x198) (4 * (position * 6 + fromIntegral state))
       in case lookup handler [(address, size) | (address, size, _) <- handlerExtents] of
            Just size | state < 6 -> (handler, size)
            _ -> error ("handler outside the pinned table for " ++ cId c)

-- What the stock side executed inside the selected handler body, from the runner's own trace and
-- memory-provenance instruction addresses.
data StockEvidence = StockEvidence
  { evidenceHandler :: Word32
  , evidenceExtent :: Word32
  , evidenceTrace :: Int
  , evidenceProvenance :: Int
  , evidenceInstructions :: Word64
  , evidenceStatus :: String
  } deriving (Eq, Show)

stockEvidence :: B.ByteString -> FilePath -> Case -> IO StockEvidence
stockEvidence image path c = uncurry (evidenceAt path) (handlerFor image c)

-- The stock side's execution inside one authenticated extent. A case with no instruction there
-- proves nothing, whatever the comparison then reports.
evidenceAt :: FilePath -> Word32 -> Word32 -> IO StockEvidence
evidenceAt path handler size = do
  report <- jsonRead =<< B.readFile path
  let runner = get report "runner"
      tracePCs = [pc | entry <- list (get runner "trace"), Just pc <- [parseWord32 (str (get entry "pc"))]]
      provenancePCs = [pc | entry <- list (get runner "memory_provenance"), Just pc <- [parseWord32 (str (get entry "pc"))]]
      inside = [() | pc <- tracePCs ++ provenancePCs, pc >= handler, pc < handler + size]
      traceInside = length [() | pc <- tracePCs, pc >= handler, pc < handler + size]
  pure StockEvidence
    { evidenceHandler = handler
    , evidenceExtent = size
    , evidenceTrace = traceInside
    , evidenceProvenance = length inside - traceInside
    , evidenceInstructions = fromIntegral (num (get runner "instructions"))
    , evidenceStatus = str (get runner "status")
    }

-- A case whose stock side ran no instruction inside the handler it claims to exercise proves
-- nothing, whatever the comparison then reports.
evidenceProblems :: StockEvidence -> [String]
evidenceProblems evidence =
  [ "stock handler " ++ show (evidenceHandler evidence) ++ " executed no instruction inside its extent"
  | evidenceTrace evidence + evidenceProvenance evidence == 0 ]

data CaseResult = CaseResult
  { crId :: String
  , crEvidence :: StockEvidence
  , crDifferences :: [String]
  } deriving (Eq, Show)

data ControlResult = ControlResult
  { coName :: String
  , coCase :: String
  , coExpected :: String
  , coReported :: Bool
  , coDifferences :: [String]
  } deriving (Eq, Show)

loadImmediate :: Int -> Word32 -> [Word32]
loadImmediate r value =
  [ 0xe3000000 .|. ((value .&. 0xf000) `shiftL` 4) .|. (fromIntegral r `shiftL` 12) .|. (value .&. 0xfff)
  , 0xe3400000 .|. (((value .&. 0xf0000000) `shiftR` 12)) .|. (fromIntegral r `shiftL` 12) .|. ((value `shiftR` 16) .&. 0xfff)
  ]

silentStub :: Word32 -> [Word32]
silentStub result = [0xe3a00000 .|. (result .&. 0xff), 0xe12fff1e]

-- Appends {tag, word0, word1, word2, word3} at ring+4+20*count.
recordPrefix :: Word32 -> [Word32]
recordPrefix tag = loadImmediate 12 ringBase ++
  [ 0xe59c3000, 0xe2833001, 0xe58c3000, 0xe2433001, 0xe0833103
  , 0xe28cc004, 0xe08cc103
  ] ++ loadImmediate 3 tag ++
  [ 0xe58c3000 ]

-- A one-argument service: the remaining argument registers are not contract values.
record1Stub :: Word32 -> [Word32]
record1Stub tag = recordPrefix tag ++
  [ 0xe58c0004
  , 0xe3a03000, 0xe58c3008, 0xe58c300c, 0xe58c3010
  , 0xe3a00000, 0xe12fff1e
  ]

-- A two-argument service.
record2Stub :: Word32 -> [Word32]
record2Stub tag = recordPrefix tag ++
  [ 0xe58c0004, 0xe58c1008
  , 0xe3a03000, 0xe58c300c, 0xe58c3010
  , 0xe3a00000, 0xe12fff1e
  ]

-- A service with no arguments: the owned side has no value to compare.
record0Stub :: Word32 -> [Word32]
record0Stub tag = recordPrefix tag ++
  [ 0xe3a03000, 0xe58c3004, 0xe58c3008, 0xe58c300c, 0xe58c3010
  , 0xe3a00000, 0xe12fff1e
  ]

-- The kernel queue send: opcode, destination, then the first two payload words.
sendStub :: [Word32]
sendStub = recordPrefix tagSend ++
  [ 0xe1d130b0, 0xe58c3004
  , 0xe5d13003, 0xe58c3008
  , 0xe5913008, 0xe5933000, 0xe58c300c
  , 0xe5913008, 0xe5933004, 0xe58c3010
  , 0xe3a00000, 0xe12fff1e
  ]

-- A queue-98 transaction whose payload is the adapter's typed packet; the actor boundary compares
-- the operation and destination, not the packet words.
queueStub :: Word32 -> [Word32]
queueStub opcode = recordPrefix tagSend ++ loadImmediate 3 opcode ++
  [ 0xe58c3004
  ] ++ loadImmediate 3 98 ++
  [ 0xe58c3008
  , 0xe3a03000, 0xe58c300c, 0xe58c3010
  , 0xe3a00000, 0xe12fff1e
  ]

-- A callback's only contract value is whether it rejected (r0 < 0).
callbackBody :: [Word32]
callbackBody = recordPrefix tagCall ++
  [ 0xe3500000
  , 0xe3a03000, 0x43a03001, 0xe58c3004
  , 0xe3a03000, 0xe58c3008, 0xe58c300c, 0xe58c3010
  , 0xe3a00000, 0xe12fff1e
  ]

receiveStub :: Word32 -> [Word32]
receiveStub result = recordPrefix tagRecv ++
  [ 0xe58c0004, 0xe58c2008
  , 0xe3a03000, 0xe58c300c, 0xe58c3010
  ] ++ silentStub result

memsetStub :: [Word32]
memsetStub =
  [ 0xe3500000
  , 0x0a000004
  , 0xe1a03000
  , 0xe0832002
  , 0xe4c31001
  , 0xe1530002
  , 0x1afffffc
  , 0xe3a00000
  , 0xe12fff1e
  ]

copyStub :: [Word32]
copyStub =
  [ 0xe92d0010
  , 0xe1a03000
  , 0xe0812002
  , 0xe4d14001
  , 0xe4c34001
  , 0xe1510002
  , 0x1afffffb
  , 0xe8bd0010
  , 0xe3a00000
  , 0xe12fff1e
  ]

recording :: [(Word32, [Word32])]
recording =
  [ (0x72503c, sendStub)
  , (0x725318, receiveStub 0)
  , (0x69b328, record2Stub tagTimr)
  , (0x6a1b40, record1Stub tagKey)
  , (0x6bcf44, record2Stub tagDiag)
  , (0x726c74, record2Stub tagDiag)
  , (0x6b8194, record1Stub tagDetm)
  , (0x6b8284, record1Stub tagDeth)
  , (0x724558, record2Stub tagFlag)
  , (0x6b7850, record0Stub tagGpio)
  , (0x6568ec, record0Stub tagGpio)
  , (0x723ce8, record1Stub tagDely)
  , (0x64e918, record1Stub tagAttach)
  , (0x6bd934, queueStub 0x7200)
  , (0x6bd8d0, queueStub 0x7201)
  , (0x6ba648, queueStub 0x7202)
  , (0x6bd71c, queueStub 0x7205)
  , (0x6bd680, queueStub 0x7206)
  , (0x6b94a0, record0Stub tagFinish)
  , (0x7221c8, memsetStub)
  , (0x722054, copyStub)
  , (0x69f8fc, silentStub 0)
  ]

-- Callees outside the mapped stock region, found by walking its branches from the dispatch site.
-- Only the out-of-range targets are returned: the stock pages keep their own bytes, so every
-- in-range handler and helper executes as stock code and only the boundary callees are stubbed.
outsideTargets :: B.ByteString -> B.ByteString -> [Word32]
outsideTargets image code = nub (sort (walk roots S.empty [] (0 :: Int)))
  where
    roots = dispatchEntry : [readWordAt (slice image 0x7cd1a4 0x198) (4 * index) | index <- [0 .. 101]]
    walk [] _ targets _ = targets
    walk (pc:rest) visited targets budget
      | budget > 40000 = targets
      | S.member pc visited = walk rest visited targets budget
      | pc < codeLow || pc >= codeHigh || (pc - codeLow) `mod` 4 /= 0 = walk rest visited (pc : targets) (budget + 1)
      | otherwise =
          let offset = fromIntegral (pc - codeLow)
              instr = readWordAt code offset
              ends = instr == 0xe12fff1e
                || (instr .&. 0x0e000000) == 0x08000000 && (instr .&. 0x00008000) /= 0 && (instr .&. 0x000f0000) == 0x000d0000
                || (instr .&. 0x0c10f000) == 0x0410f000
              unconditional = (instr .&. 0xf0000000) == 0xea000000
              next = [pc + 4 | not ends && not unconditional]
              visited' = S.insert pc visited
           in case branchTarget pc instr of
                Just target
                  | target >= codeLow && target < codeHigh -> walk (target : next ++ rest) visited' targets (budget + 1)
                  | otherwise -> walk (next ++ rest) visited' (target : targets) (budget + 1)
                Nothing -> walk (next ++ rest) visited' targets (budget + 1)

branchTarget :: Word32 -> Word32 -> Maybe Word32
branchTarget pc instr
  | (instr .&. 0x0e000000) == 0x0a000000 =
      let raw = fromIntegral (instr .&. 0x00ffffff) :: Int
          signed = if raw .&. 0x00800000 /= 0 then raw - 0x01000000 else raw
       in Just (pc + 8 + fromIntegral (signed * 4))
  | otherwise = Nothing

slice :: B.ByteString -> Word32 -> Word32 -> B.ByteString
slice bytes address size = B.take (fromIntegral size) (B.drop (fromIntegral (address - imageBase)) bytes)

region :: String -> Word32 -> Permission -> B.ByteString -> CaseRegion
region name address permission bytes = mkRegion (RegionId name) (GuestAddress (fromIntegral address)) (B.length bytes) permission bytes

word :: Word32 -> B.ByteString
word = word32le

caseBytes :: Case -> B.ByteString
caseBytes c = B.concat
  [ word (cState c)
  , B.take 2 (word (cOpcode c)) <> B.pack [0, 0x63]
  , word 24
  , word (actorBase + 0x10)
  , B.concat [word (payloadAt index) | index <- [0 .. 5]]
  , B.replicate (0x500 - 0x28) 0
  ]
  where
    payloadAt index = case drop index (cPayload c) of
      (value:_) -> value
      [] -> 0

patchedState :: Case -> B.ByteString
patchedState c = foldl (\bytes (offset, value) -> B.take offset bytes <> word value <> B.drop (offset + 4) bytes) (caseBytes c) (cRaw c)

initialRegisters :: Case -> [(Register, Word32)]
initialRegisters _ =
  [ (R0, 0), (R1, 0), (R2, 0), (R3, 0), (R4, actorBase), (R5, 0x20000000)
  , (R6, 0), (R7, 0), (R8, 0), (R9, 0), (R10, 0), (R11, 0), (R12, 0)
  , (Sp, stackBase), (Lr, exitAddress), (Cpsr, 0x13) ]

entrySymbol :: Case -> String
entrySymbol c
  | cEntry c == loopReceive = "lens_actor_wake"
  | otherwise = "lens_actor_request"

makeCase :: B.ByteString -> E.Elf -> Role -> [Word32] -> Case -> CaseSpec
makeCase image elf role externals c = CaseSpec
  { caseId = cId c
  , caseProvenance = sourceIdentity
  , caseExclusion = "Synthetic actor state, configuration, message record and payload. Kernel queue, timer, event-flag, key, diagnostic, detection and library callees are exit-recording or silent stubs at their stock addresses; the owned role runs the owned actor against the same staged record and writes the same effect ring. The stock role keeps its own handler bytes and is required to execute an instruction inside the handler the dispatcher selects. No physical serial, DMA, IRQ, scheduling, timing, reset or teardown qualification."
  , caseEntry = GuestAddress (fromIntegral (entry role))
  , caseExit = GuestAddress (fromIntegral exitAddress)
  , caseRegions = code ++ externalsRegion ++ memory
  , caseFunctions = [(GuestAddress (fromIntegral a), z) | (a, z) <- functions]
  , caseInitial = initialRegisters c
  , caseExpectedStatus = if role == Stock then StopAddress else Returned
  , caseExpectedRegisters = [(Sp, stackBase)]
  , caseExpectedMemory = []
  , caseIgnoredMemory = []
  , caseCp14Identity = Nothing
  , caseKernel = Just (KernelCase plainLimit stop Nothing Nothing Nothing [] [])
  }
  where
    guest = GuestAddress . fromIntegral
    entry Stock = cEntry c
    entry Owned = case [E.symbolValue s | s <- E.elfSymbols elf, E.symbolName s == entrySymbol c] of
      [value] -> value
      _ -> error ("admitted symbol disappeared: " ++ entrySymbol c)
    stop = if role == Stock then Just (guest (cStop c)) else Nothing
    allocated = filter (\s -> E.sectionAllocated s && E.sectionSize s > 0) (E.elfSections elf)
    code = if role == Stock then stockPages else
        [ if E.sectionExecutable s
            then region ("owned-" ++ show (E.sectionAddress s)) (E.sectionAddress s) ReadExecute (B.pack [maybe (error "admitted ELF hole") id (E.byteAt elf a) | a <- [E.sectionAddress s .. E.sectionAddress s + E.sectionSize s - 1]])
            else let padded = max 8 (8 * ((E.sectionSize s + 7) `div` 8))
                  in region ("owned-data-" ++ show (E.sectionAddress s)) (E.sectionAddress s) ReadWrite (B.replicate (fromIntegral padded) 0)
        | s <- allocated ]
    functions = if role == Stock then
        concat [if start <= cEntry c && cEntry c < start + 0x10000
                  then [(start, cEntry c - start), (cEntry c, start + 0x10000 - cEntry c)]
                  else [(start, 0x10000)] | start <- pages]
          ++ [(a, fromIntegral (4 * length body)) | (a, body) <- recording, a `elem` externals, a < imageBase]
          ++ [(a, 8) | a <- externals, a `notElem` map fst recording, a < imageBase]
          ++ [(callbackStub, fromIntegral (4 * length callbackBody))]
      else M.toList (M.fromListWith max [(E.symbolValue s, E.symbolSize s) | s <- E.elfSymbols elf, E.symbolKind s == E.FunctionSymbol, E.symbolSize s > 0])
    stubBody a = maybe (silentStub 0) id (lookup a recording)
    pages = nub (sort [start | start <- map (.&. 0xffff0000) externals, start >= imageBase])
    pageBytes start = foldl (\bytes (offset, body) -> B.take offset bytes <> assemble body <> B.drop (offset + 4 * length body) bytes)
      (slice image start 0x10000) [(fromIntegral (a - start), stubBody a) | a <- externals, a .&. 0xffff0000 == start]
    stockPages = [region ("stock-page-" ++ show start) start ReadExecute (pageBytes start) | start <- pages]
    externalsRegion = [region ("external-" ++ show a) a ReadExecute (assemble (stubBody a)) | a <- externals, a < imageBase]
    memory =
      [ region "actor" actorBase ReadWrite (patchedState c)
      , region "shared" sharedBase ReadWrite (word 0 <> word 0 <> word 0xffffff01 <> word 0 <> B.replicate 0x20 0)
      , region "codec-state" codecState ReadWrite (B.replicate 0x100 0)
      , region "reply-queue" 0x95b254 ReadWrite (B.replicate 0xac 0)
      , region "machine" 0x955000 ReadWrite (B.replicate 0x1000 0)
      , region "stock-detect" 0x930000 ReadWrite (B.replicate 0x10000 0)
      , region "diagnostic" 0xa5440000 ReadWrite (B.replicate 0x1000 0)
      , region "tx" txBase ReadWrite (B.replicate 0x820 0)
      , region "rx" rxBase ReadWrite (B.replicate 0x820 0)
      , region "ring" ringBase ReadWrite (B.replicate 0x600 0)
      , region "config" configBase ReadWrite (B.replicate 0x400 0)
      , region "flags" flagsBase ReadWrite (word (cFlags c))
      , region "callback" callbackStub ReadExecute (B.concat (map word callbackBody))
      , region "exit" exitAddress ReadExecute (word 0xe12fff1e)
      , region "stack" 0x300000 ReadWrite (B.replicate 0x10000 0)
      , region "mmio" mmioBase ReadWrite (B.replicate 0x400 0)
      , region "latch" 0x8e1000 ReadWrite (B.replicate 0x1000 0)
      , region "tables" 0x7cd140 ReadOnly (slice image 0x7cd140 0x2c0)
      , region "driver" 0x7cca00 ReadOnly (slice image 0x7cca00 0x400)
      ]

-- A callback record is (tag, rejection, 0, 0, 0). Both stubs reduce the callback's argument to
-- its sign because the stock accept paths pass the dispatch index in r0 (0x006c1920) while the
-- owned accept path passes zero; only a rejection is a contract value.
ringRecords :: NativeObservation -> [(Word32, Word32, Word32, Word32, Word32)]
ringRecords observation = case lookup (RegionId "ring") (nativeMemory observation) of
  Nothing -> []
  Just bytes ->
    let count = min 64 (fromIntegral (readWordAt bytes 0) :: Int)
        one index = let at = 4 + index * 20
                     in (readWordAt bytes at, readWordAt bytes (at + 4), readWordAt bytes (at + 8), readWordAt bytes (at + 12), readWordAt bytes (at + 16))
     in [one index | index <- [0 .. count - 1]]

compareResult :: Case -> NativeObservation -> NativeObservation -> [String]
compareResult c stock owned =
  [label ++ " status " ++ show (nativeStatus o) ++ " pc " ++ show (registerValue o Pc) | (label, o) <- roles, nativeStatus o /= expectedStatus label] ++
  ["effect ring" | ringRecords stock /= ringRecords owned] ++
  ["shared publication" | sharedWords stock /= sharedWords owned] ++
  ["actor state" | B.take 4 (regionBytesOf stock (RegionId "actor")) /= B.take 4 (regionBytesOf owned (RegionId "actor"))] ++
  ["message buffer" | B.take 0x24 (B.drop 4 (regionBytesOf stock (RegionId "actor"))) /= B.take 0x24 (B.drop 4 (regionBytesOf owned (RegionId "actor")))] ++
  ["mmio" | regionBytesOf stock (RegionId "mmio") /= regionBytesOf owned (RegionId "mmio")] ++
  ["serial transmit" | regionBytesOf stock (RegionId "tx") /= regionBytesOf owned (RegionId "tx")] ++
  ["expected shared" | Just expected <- [cExpectShared c], B.take 16 (regionBytesOf stock (RegionId "shared")) /= B.concat (map word expected)] ++
  ["expected state" | Just expected <- [cExpectState c], readWordAt (regionBytesOf stock (RegionId "actor")) 0 /= expected]
  where
    roles = [("stock", stock), ("owned", owned)]
    -- 0x006c2868 and 0x006c28c0 write the shared +4 word from Register, and 0x006c1f00 from
    -- Attached; the owned connected field is the same publication word.
    sharedWords o = let bytes = regionBytesOf o (RegionId "shared") in (readWordAt bytes 0, readWordAt bytes 4, readWordAt bytes 8, readWordAt bytes 12)
    expectedStatus "stock" = StopAddress
    expectedStatus _ = Returned

controls :: Case -> NativeObservation -> NativeObservation -> [String]
controls c stock owned =
  ["status negative control" | null (compareResult c stock { nativeStatus = Returned } owned)] ++
  ["ring negative control" | null (compareResult c (replaceRegion (RegionId "ring") (word 1 <> B.replicate 20 0xa5) stock) owned)] ++
  ["shared negative control" | null (compareResult c (mutateRegion (RegionId "shared") 0 stock) owned)] ++
  ["state negative control" | null (compareResult c (mutateRegion (RegionId "actor") 0 stock) owned)] ++
  ["mmio negative control" | null (compareResult c (mutateRegion (RegionId "mmio") 0 stock) owned)] ++
  ["transmit negative control" | null (compareResult c (mutateRegion (RegionId "tx") 0 stock) owned)]

-- The reply family composes the wire decode with the actor. The reply bytes run through the stock
-- parser at 0x006be368 and through the owned wire entry; the codec status each produces is staged
-- into that side's 0x730b actor record. The family compares the decoder status and codec state,
-- then the actor's state word, shared block and effect ring. The driver that copies a decoder's
-- fields into the actor record is modelled here by staging the observed status; no reply bytes
-- reach the actor record itself.
data ReplyPlan = ReplyPlan
  { replyName :: String
  , replyWire :: Maybe Word32
  , replyMachine :: (Word32, Word32)
  , replyFixedStatus :: Maybe Word32
  , replyData :: [Word32]
  } deriving (Eq, Show)

replyPlans :: [ReplyPlan]
replyPlans =
  [ ReplyPlan "valid" (Just 0xa55a3cc3) (6, 12) Nothing [0, 0, 0, 0]
  , ReplyPlan "timeout" Nothing (6, 12) (Just 4) [0, 0x00010000, 1, 0]
  , ReplyPlan "invalid" (Just 0x00000001) (6, 12) Nothing [0, 0, 0, 0]
  , ReplyPlan "gate" (Just 0xa55a3cc3) (5, 12) Nothing [0, 0, 0, 0]
  ]

codecReplyEntry, codecReplySize :: Word32
codecReplyEntry = 0x6be368
codecReplySize = 1120

codecReplyServices :: [(Word32, [Word32])]
codecReplyServices =
  [ (0x7235d8, [0xe3a00000, 0xe12fff1e]), (0x7236b0, [0xe12fff1e]), (0x64982c, [0xe12fff1e])
  , (0x6be7c8, [0xe12fff1e]), (0x6bde9c, [0xe12fff1e]), (0x6bf218, [0xe12fff1e])
  ]

replyCodecCase :: B.ByteString -> E.Elf -> Role -> ReplyPlan -> CaseSpec
replyCodecCase image elf role plan = CaseSpec
  { caseId = "actor-reply-codec-" ++ replyName plan
  , caseProvenance = sourceIdentity
  , caseExclusion = "Synthetic codec state, queue state, machine words and four-byte reply. The stock parser at 0x006be368 and the owned wire entry run to their return; the publication edge at 0x006bf218 and the queue, diagnostic and critical-section calls are silent returns. No physical frame, DMA, serial or timing qualification."
  , caseEntry = GuestAddress (fromIntegral entry)
  , caseExit = GuestAddress (fromIntegral exitAddress)
  , caseRegions = code ++ services ++ memory
  , caseFunctions = functions
  , caseInitial = [(r, seed r) | r <- [R0 .. R12]] ++ [(Sp, 0x308000), (Lr, exitAddress), (Cpsr, 0x10)]
  , caseExpectedStatus = Returned
  , caseExpectedRegisters = (Sp, 0x308000) : [(r, seed r) | r <- [R4 .. R11]]
  , caseExpectedMemory = []
  , caseIgnoredMemory = []
  , caseCp14Identity = Nothing
  , caseKernel = Nothing
  }
  where
    guest = GuestAddress . fromIntegral
    entry = case role of
      Stock -> codecReplyEntry
      Owned -> case [E.symbolValue s | s <- E.elfSymbols elf, E.symbolName s == "lens_codec_response"] of
        [value] -> value
        _ -> error "admitted codec symbol disappeared"
    regionAt name address perm bytes = mkRegion (RegionId name) (guest address) (B.length bytes) perm bytes
    allocated = filter (\s -> E.sectionAllocated s && E.sectionSize s > 0) (E.elfSections elf)
    sectionBytes s = B.pack [maybe (error "admitted ELF hole") id (E.byteAt elf a) | a <- [E.sectionAddress s .. E.sectionAddress s + E.sectionSize s - 1]]
    padded s = let bytes = sectionBytes s
                   target = fromIntegral (max 8 (8 * ((E.sectionSize s + 7) `div` 8)))
               in bytes <> B.replicate (target - B.length bytes) 0
    code = case role of
      Stock -> [regionAt "stock-reply" codecReplyEntry ReadExecute (slice image codecReplyEntry codecReplySize)]
      Owned ->
        [ if E.sectionExecutable s
            then regionAt ("owned-" ++ show (E.sectionAddress s)) (E.sectionAddress s) ReadExecute (sectionBytes s)
            else regionAt ("owned-data-" ++ show (E.sectionAddress s)) (E.sectionAddress s) ReadWrite (padded s)
        | s <- allocated ]
    seed r = 0xa5a50000 + fromIntegral (fromEnum r)
    services = [regionAt ("external-" ++ show a) a ReadExecute (assemble body) | (a, body) <- codecReplyServices]
    functions = case role of
      Stock -> (guest codecReplyEntry, codecReplySize) : [(guest a, fromIntegral (4 * length body)) | (a, body) <- codecReplyServices]
      Owned -> [(guest a, z) | (a, z) <- M.toList (M.fromListWith max [(E.symbolValue s, E.symbolSize s) | s <- E.elfSymbols elf, E.symbolKind s == E.FunctionSymbol, E.symbolSize s > 0])]
    patchAt bytes (offset, value) = B.take offset bytes <> word value <> B.drop (offset + 4) bytes
    state = foldl patchAt (B.replicate 0xa0 0)
      [(0x1c, 3), (0x40, 0x0b008000), (0x44, 0x40), (0x48, 0x40), (0x4c, 0), (0x8c, 0), (0x90, 0), (0x94, 7), (0x98, 0), (0x9c, 0)]
    queue = foldl patchAt (B.replicate 0xac 0xa5) [(0xa0, 19), (0xa4, 3), (0xa8, 0)]
    (four, eight) = replyMachine plan
    machine = word 6 <> word four <> word eight <> word 0
    receive = case replyWire plan of
      Nothing -> B.replicate 0x820 0
      Just value -> B.reverse (word value) <> B.replicate (0x820 - 4) 0xa5
    memory =
      [ regionAt "codec-state" codecState ReadWrite state
      , regionAt "reply-queue" 0x95b254 ReadWrite queue
      , regionAt "machine" 0x955dd0 ReadWrite machine
      , regionAt "tx" txBase ReadWrite (B.replicate 0x820 0)
      , regionAt "rx" rxBase ReadWrite receive
      , regionAt "payload" 0x210000 ReadOnly (B.pack (take 0x1000 (cycle [0, 255, 17, 128, 90, 1, 254])))
      , regionAt "stack" 0x300000 ReadWrite (B.replicate 0x10000 0xa5)
      , regionAt "exit" exitAddress ReadExecute (word 0xe7f000f0)
      ]

replyActorCase :: ReplyPlan -> Word32 -> Case
replyActorCase plan status = Case
  { cId = "actor-reply-" ++ replyName plan
  , cOpcode = 0x730b
  , cState = 2
  , cPayload = [1, status] ++ replyData plan
  , cFlags = 0
  , cRaw = []
  , cEntry = dispatchEntry
  , cStop = loopClear
  , cExpectShared = Nothing
  , cExpectState = Nothing
  }

replyBoundary :: NativeObservation -> B.ByteString
replyBoundary observation =
  B.concat [B.take 4 (B.drop offset (regionBytesOf observation (RegionId "codec-state"))) | offset <- 0x40 : [0x78, 0x8c, 0x90, 0x94, 0x98, 0x9c]]
    <> B.drop 160 (regionBytesOf observation (RegionId "reply-queue"))

data ReplyResult = ReplyResult
  { rrName :: String
  , rrStockCode :: Word32
  , rrOwnedCode :: Word32
  , rrStockActorHandler :: Word32
  , rrStockActorInstructions :: Int
  , rrStockCodecInstructions :: Maybe Int
  , rrDifferences :: [String]
  } deriving (Eq, Show)

-- Runs the wire stage for one role over the plans that carry reply bytes. The label keeps the
-- control's batch and result files apart from the correct owned run.
runReplyCodec :: B.ByteString -> FilePath -> FilePath -> E.Elf -> Role -> String -> [ReplyPlan] -> IO (M.Map String NativeObservation)
runReplyCodec image directory runner elf role label plans = do
  let wired = [plan | plan <- plans, Just _ <- [replyWire plan]]
  batch <- writeBatch (directory </> "reply-codec" </> label) (map (replyCodecCase image elf role) wired)
  observed <- loadObservedBatch runner batch
  pure (M.fromList [(replyName plan, o) | (plan, (_, o)) <- zip wired observed])

-- The stock parser's execution per wired scenario, from its own judged result.
replyCodecEvidence :: FilePath -> [ReplyPlan] -> IO (M.Map String StockEvidence)
replyCodecEvidence directory plans =
  M.fromList <$> forM plans (\plan -> do
    let ident = "actor-reply-codec-" ++ replyName plan
    found <- evidenceAt (directory </> "reply-codec" </> "Stock" </> (ident ++ ".native-result.json")) codecReplyEntry codecReplySize
    pure (replyName plan, found))

replyFamily :: B.ByteString -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> [Word32] -> IO ([ReplyResult], ControlResult)
replyFamily image directory runner codecPath codecDefectPath actorElfPath externals = do
  codecElf <- either fail pure . E.parseElf =<< B.readFile codecPath
  defectElf <- either fail pure . E.parseElf =<< B.readFile codecDefectPath
  actorElf <- either fail pure . E.parseElf =<< B.readFile actorElfPath
  stockCodec <- runReplyCodec image directory runner codecElf Stock "Stock" replyPlans
  ownedCodec <- runReplyCodec image directory runner codecElf Owned "Owned" replyPlans
  defectCodec <- runReplyCodec image directory runner defectElf Owned "Owned-defect-gate" replyPlans
  let wired = [plan | plan <- replyPlans, Just _ <- [replyWire plan]]
      code role name = (\o -> registerValue o R0) <$> M.lookup name role
      stockStatus plan = maybe (fromMaybe (error "reply status absent") (replyFixedStatus plan)) id (code stockCodec (replyName plan))
      ownedStatus plan = maybe (fromMaybe (error "reply status absent") (replyFixedStatus plan)) id (code ownedCodec (replyName plan))
      defectStatus plan = maybe (fromMaybe (error "reply status absent") (replyFixedStatus plan)) id (code defectCodec (replyName plan))
      stockCases = [replyActorCase plan (stockStatus plan) | plan <- replyPlans]
      ownedCases = [replyActorCase plan (ownedStatus plan) | plan <- replyPlans]
  stockBatch <- writeBatch (directory </> "reply-actor" </> "Stock") (map (makeCase image actorElf Stock externals) stockCases)
  ownedBatch <- writeBatch (directory </> "reply-actor" </> "Owned") (map (makeCase image actorElf Owned externals) ownedCases)
  stockObserved <- loadObservedBatch runner stockBatch
  ownedObserved <- loadObservedBatch runner ownedBatch
  stockActorEvidence <- M.fromList <$> forM replyPlans (\plan -> do
    let c = replyActorCase plan 0
        (handler, size) = handlerFor image c
    found <- evidenceAt (directory </> "reply-actor" </> "Stock" </> (cId c ++ ".native-result.json")) handler size
    pure (replyName plan, found))
  stockCodecEvidence <- replyCodecEvidence directory wired
  let stocks = M.fromList [(cId c, o) | (c, (_, o)) <- zip stockCases stockObserved]
      owneds = M.fromList [(cId c, o) | (c, (_, o)) <- zip ownedCases ownedObserved]
      evidenceProblemsFor plan =
        [ "stock actor handler " ++ show (evidenceHandler found) ++ " executed no instruction inside its extent"
        | Just found <- [M.lookup (replyName plan) stockActorEvidence]
        , evidenceTrace found + evidenceProvenance found == 0 ]
          ++ [ "stock actor handler evidence absent" | Nothing <- [M.lookup (replyName plan) stockActorEvidence] ]
          ++ [ "stock reply parser " ++ show (evidenceHandler found) ++ " executed no instruction inside its extent"
             | Just _ <- [replyWire plan]
             , Just found <- [M.lookup (replyName plan) stockCodecEvidence]
             , evidenceTrace found + evidenceProvenance found == 0 ]
          ++ [ "stock reply parser evidence absent" | Just _ <- [replyWire plan], Nothing <- [M.lookup (replyName plan) stockCodecEvidence] ]
      codecProblems plan =
        [ "decoder status " ++ show (stockStatus plan) ++ " / " ++ show (ownedStatus plan)
        | stockStatus plan /= ownedStatus plan ]
          ++ [ "decoder state"
             | Just s <- [M.lookup (replyName plan) stockCodec]
             , Just o <- [M.lookup (replyName plan) ownedCodec]
             , replyBoundary s /= replyBoundary o ]
      defectProblems plan =
        [ "decoder status " ++ show (stockStatus plan) ++ " / " ++ show (defectStatus plan)
        | stockStatus plan /= defectStatus plan ]
          ++ [ "decoder state"
             | Just s <- [M.lookup (replyName plan) stockCodec]
             , Just d <- [M.lookup (replyName plan) defectCodec]
             , replyBoundary s /= replyBoundary d ]
      defectDifferences = concatMap defectProblems wired
  pure
    ( [ ReplyResult
          { rrName = replyName plan
          , rrStockCode = stockStatus plan
          , rrOwnedCode = ownedStatus plan
          , rrStockActorHandler = maybe 0 evidenceHandler (M.lookup (replyName plan) stockActorEvidence)
          , rrStockActorInstructions = maybe 0 ((\e -> evidenceTrace e + evidenceProvenance e)) (M.lookup (replyName plan) stockActorEvidence)
          , rrStockCodecInstructions = (\e -> evidenceTrace e + evidenceProvenance e) <$> M.lookup (replyName plan) stockCodecEvidence
          , rrDifferences =
              evidenceProblemsFor plan
                ++ codecProblems plan
                ++ (case (M.lookup (cId (replyActorCase plan 0)) stocks, M.lookup (cId (replyActorCase plan 0)) owneds) of
                      (Just s, Just o) ->
                        [ "actor state" | B.take 4 (regionBytesOf s (RegionId "actor")) /= B.take 4 (regionBytesOf o (RegionId "actor")) ]
                          ++ [ "shared publication" | B.take 16 (regionBytesOf s (RegionId "shared")) /= B.take 16 (regionBytesOf o (RegionId "shared")) ]
                          ++ [ "effect ring" | ringRecords s /= ringRecords o ]
                      _ -> ["missing reply actor observation"])
          }
      | plan <- replyPlans
      ]
    , ControlResult
        { coName = "reply-gate-ignores-machine"
        , coCase = "actor-reply-valid"
        , coExpected = "decoder status"
        , coReported = any ("decoder status" `isPrefixOf`) defectDifferences
        , coDifferences = defectDifferences
        } )

mutateRegion :: RegionId -> Int -> NativeObservation -> NativeObservation
mutateRegion target offset observation = observation
  { nativeMemory = [(key, if key == target then B.take offset bytes <> word (readWordAt bytes offset + 1) <> B.drop (offset + 4) bytes else bytes) | (key, bytes) <- nativeMemory observation] }

replaceRegion :: RegionId -> B.ByteString -> NativeObservation -> NativeObservation
replaceRegion target replacement observation = observation
  { nativeMemory = [(key, if key == target then replacement else bytes) | (key, bytes) <- nativeMemory observation] }

-- Two deliberately wrong actor variants run as controls: one without the detection IRQ attach and
-- one that suppresses the stop sequence's 0xfff89340 writes. The reply family adds a third: an
-- owned wire entry whose invalid-magic gate ignores the machine words. The suite must report each
-- different.
controlsFor :: B.ByteString -> FilePath -> FilePath -> FilePath -> FilePath -> [Word32] -> M.Map String NativeObservation -> IO [ControlResult]
controlsFor image directory runner attachDefect stopDefect externals stocks =
  forM
    [ ("register-without-attach", attachDefect, "actor-register-idle", "effect ring")
    , ("stop-without-fff89340", stopDefect, "actor-routing-teardown-idle", "mmio")
    ] $ \(name, elfPath, ident, expected) -> do
      let c = fromMaybe (error ("control case disappeared: " ++ ident)) (lookup ident [(cId current, current) | current <- cases])
          stock = fromMaybe (error ("control stock observation missing: " ++ ident)) (M.lookup ident stocks)
      elf <- either fail pure . E.parseElf =<< B.readFile elfPath
      batch <- writeBatch (directory </> ("control-" ++ name) </> "owned") [makeCase image elf Owned externals c]
      observed <- loadObservedBatch runner batch
      observedCase <- case observed of
        [(_, value)] -> pure value
        _ -> fail ("control batch did not report exactly one case: " ++ name)
      let differences = compareResult c stock observedCase
      pure ControlResult
        { coName = name
        , coCase = ident
        , coExpected = expected
        , coReported = expected `elem` differences
        , coDifferences = differences
        }

runSuite :: FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO ([CaseResult], [ControlResult], [ReplyResult])
runSuite runner directory imagePath elfPath codecElfPath codecDefect attachDefect stopDefect = do
  image <- B.readFile imagePath
  unless (sha256Hex (sha256 image) == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") (fail "image4 identity differs")
  forM_ handlerExtents $ \(address, size, digest) ->
    unless (sha256Hex (sha256 (slice image address size)) == digest) (fail ("stock handler extent differs: " ++ show address))
  elf <- either fail pure . E.parseElf =<< B.readFile elfPath
  forM_ ["lens_actor_request", "lens_actor_wake"] $ \name -> case [s | s <- E.elfSymbols elf, E.symbolName s == name, E.symbolKind s == E.FunctionSymbol, E.symbolSize s > 0] of
    [_] -> pure ()
    _ -> fail ("missing owned function: " ++ name)
  forM_ (filter (\s -> E.sectionAllocated s && E.sectionSize s > 0) (E.elfSections elf)) $ \s ->
    unless (E.sectionAddress s >= 0x100000 && toInteger (E.sectionAddress s) + toInteger (E.sectionSize s) <= 0x180000) (fail "owned allocated section escapes admitted bank")
  -- The branch walk cannot see a callee reached through a switch table or an indirect call, so
  -- every declared service stub is a boundary by definition, not only the reachable ones.
  let externals = nub (sort (outsideTargets image (slice image codeLow (codeHigh - codeLow)) ++ map fst recording))
  perBatch <- forM (zip [0 :: Int ..] (chunks cases)) $ \(index, selected) -> do
    stock <- runRole image externals index Stock elf selected
    owned <- runRole image externals index Owned elf selected
    let observation run c = M.lookup (cId c) (rrObservations run)
        perCase =
          [ CaseResult (cId c) evidence (compareResult c s o ++ controls c s o ++ evidenceProblems evidence)
          | c <- selected
          , Just s <- [observation stock c]
          , Just o <- [observation owned c]
          , Just evidence <- [M.lookup (cId c) (rrEvidence stock)]
          ]
    pure (perCase, [(cId c, s) | c <- selected, Just s <- [observation stock c]])
  let results = concatMap fst perBatch
      stocks = M.fromList (concatMap snd perBatch)
  controlResults <- controlsFor image directory runner attachDefect stopDefect externals stocks
  (replyResults, replyControl) <- replyFamily image directory runner codecElfPath codecDefect elfPath externals
  pure (results, controlResults ++ [replyControl], replyResults)
  where
    chunks [] = []
    chunks rest = let (batch, remaining) = splitAt 16 rest in batch : chunks remaining
    runRole image externals index role elf selected = do
      batch <- writeBatch (directory </> show index </> show role) (map (makeCase image elf role externals) selected)
      observed <- loadObservedBatch runner batch
      evidence <-
        if role == Stock
          then M.fromList <$> forM (zip selected observed) (\(c, _) -> do
            found <- stockEvidence image (directory </> show index </> show role </> (cId c ++ ".native-result.json")) c
            pure (cId c, found))
          else pure M.empty
      pure RoleRun
        { rrObservations = M.fromList [(cId c, o) | (c, (_, o)) <- zip selected observed]
        , rrEvidence = evidence
        }

data RoleRun = RoleRun
  { rrObservations :: M.Map String NativeObservation
  , rrEvidence :: M.Map String StockEvidence
  }

