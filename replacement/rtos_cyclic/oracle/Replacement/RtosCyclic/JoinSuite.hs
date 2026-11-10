{-# LANGUAGE OverloadedStrings #-}

-- | The A32 app-to-owned-cyclic join on core 0: the emitted app caller at @0x725424@ runs over
-- the loader-published interface table and root, calls the emitted cyclic create/start entries at
-- their stock addresses, arms a phase-30 record, and the emitted timer tick delivers through the
-- emitted registry dispatcher and retained stack-guard callback. The stock image4 caller and loader
-- entries run the same join for comparison.
--
-- Fixture premises, not live state: the kernel context records, the timer heap pointer, the
-- scheduler lock stubs and the handler-context word are seeded like the cyclic family oracle; the
-- root table and the interface table are produced by executing the stock loader initializer and the
-- stock root publisher prefix. The app callback registry, the stack-guard callback and the
-- task-definition pages are stock addresses with fixture contents. The registry and the root104
-- caller are core-0 only, so the join is domain A.
module Replacement.RtosCyclic.JoinSuite
  ( Inputs (..)
  , KernelImage (..)
  , ApplicationImage (..)
  , Scope (..)
  , admitPackagedKernel
  , admitPackagedApplication
  , CaseRun (..)
  , ControlRun (..)
  , runJoin
  ) where

import Control.Monad (forM)
import Data.Bits ((.&.), (.|.), shiftL, shiftR, xor)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.String (fromString)
import Data.Word (Word32)
import Emulation.CachedNativeBatch (BatchReuse, cachedObservedBatch)
import Emulation.NativeObservation
import Emulation.OwnedCase
import qualified Replacement.RtosCyclic.Aggregate as CyclicAggregate
import Replacement.RtosCyclic.Suite (Image (..), stockFunctions)
import Replacement.RtosOwnFirst.Regions (Domain (..))
import qualified Replacement.RtosOwnFirst.Regions as R
import qualified Replacement.RtosPlatform.Aggregate as Aggregate
import qualified Replacement.RtosPlatform.Union as Union
import qualified Replacement.RtosTimer.Suite as Timer
import System.FilePath ((</>))

data Inputs = Inputs
  { inImage4 :: B.ByteString
  , inLoaderA :: B.ByteString
  , inLoaderB :: B.ByteString
  , inApp :: B.ByteString
  , inCyclic :: B.ByteString
  , inTimer :: B.ByteString
  , inAppSymbols :: [(Word32, Word32)]
  , inCyclicSymbols :: [(Word32, Word32)]
  , inTimerSymbols :: [Timer.OwnedSymbol]
  , inKernel :: KernelImage
  , inApplication :: ApplicationImage
  }

data KernelImage
  = StandaloneKernel
  | PackagedKernel B.ByteString [(Image, Bool, [(Word32, Word32)])]

data ApplicationImage = StandaloneApplication | PackagedApplication B.ByteString

admitPackagedApplication :: B.ByteString -> B.ByteString -> B.ByteString -> Either String ApplicationImage
admitPackagedApplication stock app staged
  | B.length staged /= B.length stock = Left "packaged image4 length differs from canonical image4"
  | B.length app /= 0x242c = Left "app-callback image is not 0x242c bytes"
  | slice image4Base staged dispatchEvent 0x1c /= B.take 0x1c app = Left "packaged dispatcher differs from emitted owned code"
  | slice image4Base staged appRegister 0x384 /= B.drop (fromIntegral (appRegister - dispatchEvent)) app = Left "packaged app caller differs from emitted owned code"
  | otherwise = Right (PackagedApplication staged)

data Scope = FullScope | LifecycleSmoke deriving (Eq, Show)

admitPackagedKernel :: Union.Union -> B.ByteString -> B.ByteString -> Aggregate.AggregateImage -> [Aggregate.AggregateSymbol] -> [Aggregate.SectionPlacement] -> Either String KernelImage
admitPackagedKernel union stock loader image symbols placements = do
  let domain = Union.unionA union
      carriers = [ (low, high) | carrier <- Union.mapCarriers domain, R.Extent (R.GuestAddress low) (R.GuestAddress high) <- [Union.carrierExtent carrier] ]
      filled = combine 0x40000 (B.length stock)
        ((0x40000, stock) : [(low, B.concat (replicate (fromIntegral ((high - low) `div` 4)) (word32le (Union.unionFill union)))) | (low, high) <- carriers])
  withSymbols <- Aggregate.installAggregate 0x40000 filled image symbols
  expected <- Aggregate.installPlacements 0x40000 withSymbols image placements
  if loader /= expected
    then Left "packaged loader differs from allocated sections, veneers, carrier fill or retained stock"
    else case carriers of
      [] -> Left "packaged loader has no carriers"
      _ -> do
        let low = minimum (map fst carriers)
            high = maximum (map snd carriers)
            effective = slice 0x40000 loader low (fromIntegral (high - low))
        mappings <- CyclicAggregate.bindAggregate domain (Image 0x40000 stock) effective symbols placements
        pure (PackagedKernel loader mappings)

data Role = Stock | Owned deriving (Eq, Show)

roleLabel :: Role -> String
roleLabel Stock = "stock"
roleLabel Owned = "owned"

-- | The core-0 (domain A) addresses the join binds.
data Layout = Layout
  { lRoot :: Word32
  , lConfigs :: Word32
  , lControls :: Word32
  , lCyclicCount :: Word32
  , lWords :: Word32
  , lHandlerContext :: Word32
  , lCurrentContext :: Word32
  , lContexts :: Word32
  , lTimerBase :: Word32
  , lTick :: Word32
  , lTimerCount :: Word32
  , lHeap :: Word32
  , lArena :: Word32
  , lTaskDefs :: Word32
  , lSyslogTable :: Word32
  , lIrqSave :: Word32
  , lIrqRestore :: Word32
  , lCoreIndexAdapter :: Word32
  , lCoreIndex :: Word32
  , lSyslog :: Word32
  , lLoaderInit :: Word32
  , lCp14 :: Word32
  , lStockCyclicLow :: Word32
  , lStockCyclicSize :: Word32
  , lStockTimerLow :: Word32
  , lStockTimerSize :: Word32
  , lStockSupport :: Word32
  , lCreateEntry :: Word32
  , lStartEntry :: Word32
  , lStopEntry :: Word32
  , lDeleteEntry :: Word32
  , lExpireEntry :: Word32
  , lTickEntry :: Word32
  }

layoutA :: Layout
layoutA =
  Layout
    { lRoot = 0x4004dae0
    , lConfigs = 0x4005ef78
    , lControls = 0x4005928c
    , lCyclicCount = 0x4004dacc
    , lWords = 0x40054000
    , lHandlerContext = 0x400545a4
    , lCurrentContext = 0x400545c8
    , lContexts = 0x400545cc
    , lTimerBase = 0x4005476c
    , lTick = 0x40054770
    , lTimerCount = 0x40054778
    , lHeap = 0x4005d718
    , lArena = 0x40a5a200
    , lTaskDefs = 0x4005beac
    , lSyslogTable = 0x4004f5e4
    , lIrqSave = 0x41140
    , lIrqRestore = 0x41164
    , lCoreIndexAdapter = 0x412bc
    , lCoreIndex = 0x427b8
    , lSyslog = 0x41a34
    , lLoaderInit = 0x40430
    , lCp14 = 0x4003
    , lStockCyclicLow = 0x4be3c
    , lStockCyclicSize = 0x1ac0
    , lStockTimerLow = 0x44a50
    , lStockTimerSize = 0x4dc
    , lStockSupport = 0x42560
    , lCreateEntry = 0x4d818
    , lStartEntry = 0x4bfcc
    , lStopEntry = 0x4c13c
    , lDeleteEntry = 0x4d8a8
    , lExpireEntry = 0x4beb0
    , lTickEntry = 0x44d8c
    }

interfacesAddress, registryBase, tokenAddress, guardAddress, lastErrorAddress :: Word32
interfacesAddress = 0x96bd20
registryBase = 0x409c3500
tokenAddress = 0x96ca4c
guardAddress = 0x828094
lastErrorAddress = 0x40a0f82c

dispatchEvent, dispatchError, stackGuard, stackCheck, publisher, errorCallback :: Word32
dispatchEvent = 0x72337c
dispatchError = 0x723398
stackGuard = 0x69cc18
stackCheck = 0x725b74
publisher = 0x725a04
errorCallback = 0x4089ea8c

appRegister, appUnregister :: Word32
appRegister = 0x725424
appUnregister = 0x7256c4

image4Base :: Word32
image4Base = 0x61c400

stubBase, stubAcquire, stubRelease, stubError, stubCreateFault, stubStartFault, stubDelivery, stubSize :: Word32
stubBase = 0x01000000
stubAcquire = stubBase
stubRelease = stubBase + 0x18
stubError = stubBase + 0x30
stubCreateFault = stubBase + 0x50
stubStartFault = stubBase + 0x60
stubDelivery = stubBase + 0x70
stubSize = 0x200

errorReceipt, deliveryReceipt, lockCell :: Word32
errorReceipt = 0x40054f04
deliveryReceipt = 0x40054f08
lockCell = 0x40054f00

stackBase, stackPointer, exitAddress :: Word32
stackBase = 0x300000
stackPointer = stackBase + 0x800
exitAddress = 0x01000200

heapSize :: Role -> Int
heapSize Stock = 0x5c0
heapSize Owned = 0x800

heapBase :: Layout -> Role -> Word32
heapBase layout Stock = lHeap layout
heapBase layout Owned = lArena layout

-- | The fixture state the join chains from stage to stage.
data State = State
  { stRoot :: B.ByteString
  , stInterfaces :: B.ByteString
  , stRegistry :: B.ByteString
  , stConfigs :: B.ByteString
  , stControls :: B.ByteString
  , stCyclicCount :: B.ByteString
  , stHeap :: B.ByteString
  , stWords :: B.ByteString
  , stGuard :: B.ByteString
  , stLastError :: B.ByteString
  , stToken :: B.ByteString
  , stTaskDefs :: B.ByteString
  , stSyslog :: B.ByteString
  }

-- | The emitted regions, merged to stay inside the runner's 16-region case limit. The kernel-low
-- block holds the cyclic count, the root table and the syslog table; the kernel-objects block holds
-- the context words, control table, task definitions, stock heap and creation table; the ram-block
-- holds the error callback pointer, the callback registry and the last-error word.
stateLayout :: Layout -> Role -> [(String, Word32, Int, Permission)]
stateLayout layout role =
  [ ("kernel-low", lCyclicCount layout, 0x1c18, ReadWrite)
  , ("kernel-objects", lWords layout, 0xb578, ReadWrite)
  , ("ram-block", errorCallback, 0x170da4, ReadWrite)
  , ("interfaces-token", interfacesAddress, 0xd30, ReadWrite)
  , ("guard", guardAddress, 4, ReadWrite)
  ]
    ++ [("arena", lArena layout, 0x800, ReadWrite) | role == Owned]

stateBytes :: Layout -> Role -> State -> String -> B.ByteString
stateBytes layout role state name = case name of
  "kernel-low" ->
    combine (lCyclicCount layout) 0x1c18
      [ (lCyclicCount layout, stCyclicCount state)
      , (lRoot layout, stRoot state)
      , (lSyslogTable layout, stSyslog state)
      ]
  "kernel-objects" ->
    combine (lWords layout) 0xb578
      [ (lWords layout, stWords state)
      , (lControls layout, stControls state)
      , (lTaskDefs layout, stTaskDefs state)
      , (heapBase layout role, stHeap state)
      , (lConfigs layout, stConfigs state)
      ]
  "ram-block" ->
    combine errorCallback 0x170da4
      [ (errorCallback, word32le stubError)
      , (registryBase, stRegistry state)
      , (lastErrorAddress, stLastError state)
      ]
  "interfaces-token" ->
    combine interfacesAddress 0xd30
      [ (interfacesAddress, stInterfaces state)
      , (tokenAddress, stToken state)
      ]
  "guard" -> stGuard state
  "arena" -> stHeap state
  _ -> error ("no state bytes for region " ++ name)

stateRegions :: Layout -> Role -> State -> [(String, Word32, Int, Permission, B.ByteString)]
stateRegions layout role state =
  [ (name, base, size, permission, stateBytes layout role state name)
  | (name, base, size, permission) <- stateLayout layout role
  ]

codeLayout :: Layout -> Role -> [(String, Word32, Int, Permission)]
codeLayout _layout _role =
  [ ("loader-code", 0x40000, 0xdae0, ReadExecute)
  , ("guard-code", stackGuard, 0x54, ReadExecute)
  , ("dispatch-code", dispatchEvent, 0x40, ReadExecute)
  , ("stack-check-code", stackCheck, 0xf8, ReadExecute)
  , ("app", appRegister, 0x384, ReadExecute)
  ]
    ++ [("stubs", stubBase, fromIntegral stubSize, ReadExecute)
       , ("stack", stackBase, 0x1000, ReadWrite)
       , ("exit", exitAddress, 0x10, ReadExecute)
       ]

codeBytes :: Layout -> Inputs -> Role -> String -> B.ByteString
codeBytes layout inputs role name = case name of
  "loader-code" -> case (role, inKernel inputs) of
    (Stock, _) -> inLoaderA inputs
    (Owned, PackagedKernel loader _) -> loader
    (Owned, StandaloneKernel) ->
      combine 0x40000 0xdae0
        [ (0x40000, inLoaderA inputs)
        , (lStockCyclicLow layout, inCyclic inputs)
        , (lStockTimerLow layout, inTimer inputs)
        ]
  "guard-code" -> slice image4Base roleImage4 stackGuard 0x54
  "dispatch-code" -> case role of
    Stock -> stockDispatch
    Owned -> case inApplication inputs of
      StandaloneApplication -> B.take 0x1c (inApp inputs) <> B.drop 0x1c stockDispatch
      PackagedApplication bytes -> slice image4Base bytes dispatchEvent 0x40
  "stack-check-code" -> slice image4Base roleImage4 stackCheck 0xf8
  "app" -> case role of
    Owned -> case inApplication inputs of
      StandaloneApplication -> B.drop (fromIntegral (appRegister - dispatchEvent)) (inApp inputs)
      PackagedApplication bytes -> slice image4Base bytes appRegister 0x384
    Stock -> slice image4Base (inImage4 inputs) appRegister 0x384
  "cyclic" -> inCyclic inputs
  "timer" -> inTimer inputs
  "stubs" -> stubBytes
  "stack" -> B.replicate 0x1000 0
  "exit" -> B.replicate 0x10 0
  _ -> error ("no code bytes for region " ++ name)
  where
    roleImage4 = case (role, inApplication inputs) of
      (Owned, PackagedApplication bytes) -> bytes
      _ -> inImage4 inputs
    stockDispatch = slice image4Base (inImage4 inputs) dispatchEvent 0x40

codeRegions :: Layout -> Inputs -> Role -> [(String, Word32, Int, Permission, B.ByteString)]
codeRegions layout inputs role = case (role, inKernel inputs) of
  (Owned, PackagedKernel loader mappings) ->
    [ ("loader-code-" ++ show index, base, B.length bytes, if executable then ReadExecute else ReadOnly, bytes)
    | (index, (Image base bytes, executable, _)) <- zip [0 :: Int ..] mappings
    ] ++
    [ ("loader-prefix", 0x40000, fromIntegral (low - 0x40000), ReadExecute, B.take (fromIntegral (low - 0x40000)) loader)
    , ("loader-suffix", high, B.length loader - fromIntegral (high - 0x40000), ReadOnly, B.drop (fromIntegral (high - 0x40000)) loader)
    ] ++ regionsWithoutLoader
    where
      low = minimum [base | (Image base _, _, _) <- mappings]
      high = maximum [base + fromIntegral (B.length bytes) | (Image base bytes, _, _) <- mappings]
  _ -> regions
  where
    regions = [(name, base, size, permission, codeBytes layout inputs role name) | (name, base, size, permission) <- codeLayout layout role]
    regionsWithoutLoader = [region | region@(name, _, _, _, _) <- regions, name /= "loader-code"]

emittedBases :: Layout -> Role -> [(String, Word32, Int)]
emittedBases layout role =
  [ (name, base, size)
  | (name, base, size, _) <- stateLayout layout role ++ codeLayout layout role
  ]

-- | Place byte runs into one zero-filled block, dropping runs outside it.
combine :: Word32 -> Int -> [(Word32, B.ByteString)] -> B.ByteString
combine base size pieces = foldl place (B.replicate size 0) pieces
  where
    place bytes (address, value)
      | address < base || address + fromIntegral (B.length value) > base + fromIntegral size = bytes
      | otherwise =
          let offset = fromIntegral (address - base)
           in B.take offset bytes <> value <> B.drop (offset + B.length value) bytes

extractBytes :: Layout -> Role -> NativeObservation -> Word32 -> Int -> B.ByteString
extractBytes layout role observation address size = go (emittedBases layout role)
  where
    go [] = error ("observed state has no region covering " ++ showHexWord address)
    go ((name, base, regionSize) : rest)
      | address >= base && address + fromIntegral size <= base + fromIntegral regionSize =
          case lookup (RegionId (fromString name)) (nativeMemory observation) of
            Just bytes -> B.take size (B.drop (fromIntegral (address - base)) bytes)
            Nothing -> error ("observed state is missing region " ++ name)
      | otherwise = go rest

stateFrom :: Layout -> Role -> NativeObservation -> State
stateFrom layout role observation =
  State
    { stRoot = extract (lRoot layout) 0x1700
    , stInterfaces = extract interfacesAddress 8
    , stRegistry = extract registryBase 0x404
    , stConfigs = extract (lConfigs layout) 0x600
    , stControls = extract (lControls layout) 0x600
    , stCyclicCount = extract (lCyclicCount layout) 4
    , stHeap = extract (heapBase layout role) (heapSize role)
    , stWords = extract (lWords layout) 0x1000
    , stGuard = extract guardAddress 4
    , stLastError = extract lastErrorAddress 4
    , stToken = extract tokenAddress 4
    , stTaskDefs = extract (lTaskDefs layout) 0x12c0
    , stSyslog = extract (lSyslogTable layout) 0x100
    }
  where
    extract address size = extractBytes layout role observation address size

controlSeed :: Layout -> [(Word32, Word32)]
controlSeed layout =
  [ (lControls layout + fromIntegral index * 0x18, lConfigs layout + fromIntegral index * 0x18)
  | index <- [0 .. 63 :: Int]
  ]

contextSeed :: Layout -> Role -> Word32 -> [(Word32, Word32)]
contextSeed layout role context =
  [ (context + 0x08, lockCell)
  , (context + 0x0c, stubAcquire)
  , (context + 0x10, stubRelease)
  , (context + 0x34, heapBase layout role)
  , (context + 0x38, lTimerCount layout)
  , (context + 0x3c, lTimerBase layout)
  , (context + 0x40, lTick layout)
  , (context + 0x74, lControls layout)
  , (context + 0x78, lCyclicCount layout)
  , (context + 0x80, lExpireEntry layout)
  ]

wordsSeed :: Layout -> Role -> [(Word32, Word32)]
wordsSeed layout role =
  [ (lHandlerContext layout, 0)
  , (lCurrentContext layout, lContexts layout)
  , (lTimerBase layout, 0)
  , (lTick layout, 0)
  , (lTimerCount layout, 0)
  ]
    ++ contextSeed layout role (lContexts layout)
    ++ contextSeed layout role (lContexts layout + 0x84)

seedState :: Layout -> Role -> B.ByteString -> B.ByteString -> State
seedState layout role rootBytes interfaceBytes =
  State
    { stRoot = padTo 0x1700 rootBytes
    , stInterfaces = padTo 8 interfaceBytes
    , stRegistry = overlay registryBase 0x404 [(registryBase, 30001)]
    , stConfigs = B.replicate 0x600 0
    , stControls = overlay (lControls layout) 0x600 (controlSeed layout)
    , stCyclicCount = word32le 64
    , stHeap = B.replicate (heapSize role) 0
    , stWords = overlay (lWords layout) 0x1000 (wordsSeed layout role)
    , stGuard = word32le 1
    , stLastError = word32le 0
    , stToken = word32le 0
    , stTaskDefs = B.replicate 0x12c0 0
    , stSyslog = B.replicate 0x100 0
    }

padTo :: Int -> B.ByteString -> B.ByteString
padTo size bytes
  | B.length bytes >= size = B.take size bytes
  | otherwise = bytes <> B.replicate (size - B.length bytes) 0

overlay :: Word32 -> Int -> [(Word32, Word32)] -> B.ByteString
overlay base size words' = B.concat (pieces 0 ordered)
  where
    ordered =
      M.toAscList
        ( M.fromList
            [ (fromIntegral (address - base) :: Int, value)
            | (address, value) <- words'
            , address >= base
            , address + 4 <= base + fromIntegral size
            ]
        )
    pieces cursor [] = [B.replicate (size - cursor) 0]
    pieces cursor ((offset, value) : rest) =
      B.replicate (offset - cursor) 0 : word32le value : pieces (offset + 4) rest

poke :: B.ByteString -> Int -> Word32 -> B.ByteString
poke bytes offset value =
  let (before, after) = B.splitAt offset bytes
   in before <> word32le value <> B.drop 4 after

slice :: Word32 -> B.ByteString -> Word32 -> Int -> B.ByteString
slice base bytes address size
  | address < base || address + fromIntegral size > base + fromIntegral (B.length bytes) =
      error ("join slice outside its image: base=" ++ show base ++ " length=" ++ show (B.length bytes) ++ " address=" ++ show address ++ " size=" ++ show size)
  | otherwise = B.take size (B.drop (fromIntegral (address - base)) bytes)

movwI, movtI, ldrI, strI, movImm, mvnImm :: Word32 -> Word32 -> Word32
bxLr :: Word32
bxLr = 0xe12fff1e
movwI rd imm = 0xe3000000 .|. (((imm `shiftR` 12) .&. 0xf) `shiftL` 16) .|. (rd `shiftL` 12) .|. (imm .&. 0xfff)
movtI rd imm = 0xe3400000 .|. (((imm `shiftR` 12) .&. 0xf) `shiftL` 16) .|. (rd `shiftL` 12) .|. (imm .&. 0xfff)
ldrI rd rn = 0xe5900000 .|. (rn `shiftL` 16) .|. (rd `shiftL` 12)
strI rd rn = 0xe5800000 .|. (rn `shiftL` 16) .|. (rd `shiftL` 12)
movImm rd imm = 0xe3a00000 .|. (rd `shiftL` 12) .|. (imm .&. 0xfff)
mvnImm rd imm = 0xe3e00000 .|. (rd `shiftL` 12) .|. (imm .&. 0xfff)

stubBytes :: B.ByteString
stubBytes = overlay stubBase (fromIntegral stubSize)
  [ (stubAcquire, ldrI 1 0)
  , (stubAcquire + 4, movImm 2 1)
  , (stubAcquire + 8, strI 2 1)
  , (stubAcquire + 12, movImm 0 0)
  , (stubAcquire + 16, bxLr)
  , (stubRelease, ldrI 1 0)
  , (stubRelease + 4, movImm 2 0)
  , (stubRelease + 8, strI 2 1)
  , (stubRelease + 12, movImm 0 0)
  , (stubRelease + 16, bxLr)
  , (stubError, movwI 1 (errorReceipt .&. 0xffff))
  , (stubError + 4, movtI 1 (errorReceipt `shiftR` 16))
  , (stubError + 8, strI 0 1)
  , (stubError + 12, bxLr)
  , (stubCreateFault, mvnImm 0 40)
  , (stubCreateFault + 4, bxLr)
  , (stubStartFault, mvnImm 0 4)
  , (stubStartFault + 4, bxLr)
  , (stubDelivery, movwI 1 (deliveryReceipt .&. 0xffff))
  , (stubDelivery + 4, movtI 1 (deliveryReceipt `shiftR` 16))
  , (stubDelivery + 8, strI 0 1)
  , (stubDelivery + 12, bxLr)
  ]

seedRegister :: Register -> Word32
seedRegister register = 0xa5a50000 + fromIntegral (fromEnum register)

negative :: Int -> Word32
negative value = fromIntegral (negate value :: Int)

data Check
  = RegionWord String Word32 Word32
  | RegisterIs Register Word32
  | CpsrControlIs Word32
  | RunnerStatusIs RunnerStatus
  | HeapNode Int Word32 Word32
  deriving (Eq, Show)

data Expectation = Expectation
  { xStatus :: Word32
  , xChecks :: [Check]
  }

describe :: Check -> String
describe check' = case check' of
  RegionWord region address value -> region ++ " " ++ showHexWord address ++ " = " ++ showHexWord value
  RegisterIs register value -> registerLabel register ++ " = " ++ showHexWord value
  CpsrControlIs value -> "cpsr control = " ++ showHexWord value
  RunnerStatusIs status -> "runner status = " ++ runnerStatusLabel status
  HeapNode index expiry entry -> "heap node " ++ show index ++ " = (" ++ showHexWord expiry ++ ", " ++ showHexWord entry ++ ")"

showHexWord :: Word32 -> String
showHexWord value = "0x" ++ showHex' value
  where
    showHex' 0 = "0"
    showHex' n = reverse (go n)
    go 0 = []
    go n = let (q, r) = n `divMod` 16 in "0123456789abcdef" !! fromIntegral r : go q

readAbsWith :: [(String, Word32, Int)] -> NativeObservation -> Word32 -> Either String Word32
readAbsWith bases observation address = go bases
  where
    go [] = Left ("address " ++ showHexWord address ++ " is outside the case regions")
    go ((name, base, size) : rest)
      | address >= base && address + 4 <= base + fromIntegral size =
          case lookup (RegionId (fromString name)) (nativeMemory observation) of
            Just bytes -> Right (readWordAt bytes (fromIntegral (address - base)))
            Nothing -> Left ("missing region " ++ name)
      | otherwise = go rest

evaluateWith :: [(String, Word32, Int)] -> Layout -> Role -> NativeObservation -> Check -> Either String ()
evaluateWith bases layout role observation check' = case check' of
  RegionWord _ address value -> do
    actual <- readAbsWith bases observation address
    if actual == value then Right () else Left (describe check' ++ " but is " ++ showHexWord actual)
  RegisterIs register value ->
    case lookupRegister register observation of
      Just actual
        | actual == value -> Right ()
        | otherwise -> Left (describe check' ++ " but is " ++ showHexWord actual)
      Nothing -> Left ("missing register " ++ registerLabel register)
  CpsrControlIs value ->
    case lookupRegister Cpsr observation of
      Just actual
        | cpsrControl actual == cpsrControl value -> Right ()
        | otherwise -> Left (describe check' ++ " but is " ++ showHexWord (cpsrControl actual))
      Nothing -> Left "missing register cpsr"
  RunnerStatusIs status
    | nativeStatus observation == status -> Right ()
    | otherwise -> Left (describe check' ++ " but is " ++ runnerStatusLabel (nativeStatus observation))
  HeapNode index expiry entry -> do
    let node = heapBase layout role + fromIntegral (index - 1) * 8
    actualExpiry <- readAbsWith bases observation node
    actualEntry <- readAbsWith bases observation (node + 4)
    if (actualExpiry, actualEntry) == (expiry, entry)
      then Right ()
      else Left (describe check' ++ " but is (" ++ showHexWord actualExpiry ++ ", " ++ showHexWord actualEntry ++ ")")

evaluate :: Layout -> Role -> NativeObservation -> Check -> Either String ()
evaluate layout role = evaluateWith (emittedBases layout role) layout role

stateWord :: State -> String -> Word32 -> Word32
stateWord state name address =
  let (bytes, base) = case name of
        "words" -> (stWords state, lWords layoutA)
        "controls" -> (stControls state, lControls layoutA)
        "guard" -> (stGuard state, guardAddress)
        _ -> error ("no state region " ++ name)
   in readWordAt bytes (fromIntegral (address - base))

controlSlot, configSlot :: Word32
controlSlot = lControls layoutA + 63 * 0x18
configSlot = lConfigs layoutA + 63 * 0x18
controlTimer :: Word32
controlTimer = controlSlot + 0x0c

commonRegisters :: [Check]
commonRegisters =
  [ RegisterIs register (seedRegister register)
  | register <- [R4, R5, R6, R7, R8, R9, R10, R11]
  ]
    ++ [RegisterIs Sp stackPointer, CpsrControlIs 0x13]

expectation :: Layout -> Role -> Variant -> State -> Expectation
expectation layout role variant state = case variant of
  Register argument -> registered argument
  RegisterNoSlot -> noSlot
  RegisterCreateFault -> fault (negative 41)
  RegisterStartFault -> startedButNotArmed
  RegisterZeroPeriod -> zeroPeriod
  Unregister token -> if token == 30001 then unregistered else unregisterMissing
  Tick -> ticked
  TickRegistryFault -> registryFault
  where
    configWord offset = RegionWord "configs" (configSlot + offset)
    controlWord offset = RegionWord "controls" (controlSlot + offset)
    registered argument =
      Expectation
        0
        ( [ RegisterIs R0 0
          , RegionWord "token" tokenAddress 30001
          , RegionWord "registry" registryBase 30002
          , RegionWord "registry" (registryBase + 0x004 + 63 * 4) stackGuard
          , RegionWord "registry" (registryBase + 0x104 + 63 * 4) 30001
          , RegionWord "registry" (registryBase + 0x204 + 63 * 4) argument
          , RegionWord "registry" (registryBase + 0x304 + 63 * 4) 1
          , RegionWord "registry" (registryBase + 0x304 + 62 * 4) 0
          , configWord 0x00 0
          , configWord 0x04 64
          , configWord 0x08 dispatchEvent
          , configWord 0x0c argument
          , configWord 0x10 argument
          , configWord 0x14 1
          , controlWord 0x00 configSlot
          , controlWord 0x04 1
          , controlWord 0x08 argument
          , controlWord 0x0c 1
          , controlWord 0x10 (lExpireEntry layout)
          , controlWord 0x14 controlSlot
          , HeapNode 1 argument controlTimer
          , RegionWord "words" (lTimerCount layout) 1
          , RegionWord "words" (lTimerBase layout) 0
          , RegionWord "words" (lTick layout) 0
          , RegionWord "guard" guardAddress 1
          , RegionWord "last-error" lastErrorAddress 0
          , RegionWord "entries" errorReceipt 0
          ]
            ++ commonRegisters
        )
    startedButNotArmed =
      Expectation
        (negative 5)
        ( [ RegisterIs R0 (negative 5)
          , RegionWord "token" tokenAddress 30001
          , RegionWord "registry" registryBase 30002
          , RegionWord "registry" (registryBase + 0x004 + 63 * 4) stackGuard
          , RegionWord "registry" (registryBase + 0x104 + 63 * 4) 30001
          , RegionWord "registry" (registryBase + 0x304 + 63 * 4) 1
          , configWord 0x14 1
          , configWord 0x0c 30
          , controlWord 0x04 0
          , RegionWord "words" (lTimerCount layout) 0
          , RegionWord "last-error" lastErrorAddress (negative 5)
          , RegionWord "entries" errorReceipt (negative 5)
          ]
            ++ commonRegisters
        )
    noSlot =
      Expectation
        (negative 34)
        ( [ RegisterIs R0 (negative 34)
          , RegionWord "token" tokenAddress (negative 1)
          , RegionWord "registry" registryBase 30001
          , RegionWord "registry" (registryBase + 0x304 + 63 * 4) 1
          , configWord 0x14 0
          , configWord 0x0c 0
          , controlWord 0x04 0
          , RegionWord "words" (lTimerCount layout) 0
          , RegionWord "last-error" lastErrorAddress (negative 34)
          , RegionWord "entries" errorReceipt (negative 34)
          ]
            ++ commonRegisters
        )
    fault status =
      Expectation
        status
        ( [ RegisterIs R0 status
          , RegionWord "token" tokenAddress (negative 1)
          , RegionWord "registry" registryBase 30001
          , RegionWord "registry" (registryBase + 0x304 + 63 * 4) 0
          , configWord 0x14 0
          , configWord 0x0c 0
          , controlWord 0x04 0
          , RegionWord "words" (lTimerCount layout) 0
          , RegionWord "last-error" lastErrorAddress status
          , RegionWord "entries" errorReceipt status
          ]
            ++ commonRegisters
        )
    zeroPeriod = case role of
      Stock ->
        Expectation
          0
          ( [ RegisterIs R0 0
            , RegionWord "token" tokenAddress 30001
            , RegionWord "registry" registryBase 30002
            , configWord 0x0c 0
            , configWord 0x10 0
            , configWord 0x14 1
            , controlWord 0x04 1
            , controlWord 0x08 0
            , controlWord 0x0c 1
            , HeapNode 1 0 controlTimer
            , RegionWord "words" (lTimerCount layout) 1
            , RegionWord "guard" guardAddress 1
            ]
              ++ commonRegisters
          )
      Owned ->
        Expectation
          (negative 17)
          ( [ RegisterIs R0 (negative 17)
            , RegionWord "token" tokenAddress (negative 1)
            , RegionWord "registry" registryBase 30001
            , RegionWord "registry" (registryBase + 0x304 + 63 * 4) 0
            , configWord 0x14 0
            , controlWord 0x04 0
            , RegionWord "words" (lTimerCount layout) 0
            , RegionWord "last-error" lastErrorAddress (negative 17)
            , RegionWord "entries" errorReceipt (negative 17)
            ]
              ++ commonRegisters
          )
    unregistered =
      Expectation
        0
        ( [ RegisterIs R0 0
          , RegionWord "token" tokenAddress 30001
          , RegionWord "registry" (registryBase + 0x104 + 63 * 4) 0
          , RegionWord "registry" (registryBase + 0x304 + 63 * 4) 0
          , configWord 0x14 0
          , configWord 0x08 0
          , controlWord 0x04 0
          , controlWord 0x0c (if role == Stock then 1 else 0)
          , RegionWord "words" (lTimerCount layout) 0
          , RegionWord "guard" guardAddress 2
          ]
            ++ commonRegisters
        )
    unregisterMissing =
      Expectation
        0
        ( [ RegisterIs R0 0
          , RegionWord "token" tokenAddress 30001
          , RegionWord "registry" (registryBase + 0x104 + 63 * 4) 30001
          , RegionWord "registry" (registryBase + 0x304 + 63 * 4) 1
          , configWord 0x14 1
          , controlWord 0x04 1
          , RegionWord "words" (lTimerCount layout) 1
          , RegionWord "guard" guardAddress 2
          ]
            ++ commonRegisters
        )
    ticked =
      let tick = stateWord state "words" (lTick layout)
          count = stateWord state "words" (lTimerCount layout)
          expiry = stateWord state "controls" (controlSlot + 0x08)
          guard = stateWord state "guard" guardAddress
          due = count > 0 && expiry <= tick
       in if count == 0
            then
              Expectation
                0
                ( [ RegisterIs R0 0
                  , RegionWord "words" (lTimerCount layout) 0
                  , RegionWord "words" (lTimerBase layout) tick
                  , RegionWord "words" (lTick layout) (tick + 10)
                  , RegionWord "guard" guardAddress guard
                  , RegionWord "entries" deliveryReceipt 0
                  ]
                    ++ commonRegisters
                )
            else
              if due
                then
                  Expectation
                    0
                    ( [ RegisterIs R0 0
                      , RegionWord "words" (lTimerCount layout) 1
                      , RegionWord "words" (lTimerBase layout) tick
                      , RegionWord "words" (lTick layout) (tick + 10)
                      , RegionWord "guard" guardAddress 2
                      , controlWord 0x04 1
                      , controlWord 0x0c 1
                      , controlWord 0x08 (expiry + 30)
                      , HeapNode 1 (expiry + 30) controlTimer
                      , RegionWord "entries" deliveryReceipt 0
                      ]
                        ++ commonRegisters
                    )
                else
                  Expectation
                    0
                    ( [ RegisterIs R0 0
                      , RegionWord "words" (lTimerCount layout) 1
                      , RegionWord "words" (lTimerBase layout) tick
                      , RegionWord "words" (lTick layout) (tick + 10)
                      , RegionWord "guard" guardAddress guard
                      , controlWord 0x04 1
                      , controlWord 0x0c 1
                      , controlWord 0x08 expiry
                      , HeapNode 1 expiry controlTimer
                      , RegionWord "entries" deliveryReceipt 0
                      ]
                        ++ commonRegisters
                    )
    registryFault =
      Expectation
        0
        ( [ RegisterIs R0 0
          , RegionWord "entries" deliveryReceipt 64
          , RegionWord "guard" guardAddress 1
          , controlWord 0x04 1
          , controlWord 0x08 60
          , HeapNode 1 60 controlTimer
          , RegionWord "words" (lTimerCount layout) 1
          , RegionWord "words" (lTimerBase layout) 30
          , RegionWord "words" (lTick layout) 40
          ]
            ++ commonRegisters
        )

data Variant
  = Register Word32
  | RegisterNoSlot
  | RegisterCreateFault
  | RegisterStartFault
  | RegisterZeroPeriod
  | Unregister Word32
  | Tick
  | TickRegistryFault
  deriving (Eq, Show)

variantName :: Variant -> String
variantName variant = case variant of
  Register argument -> "register-" ++ show argument
  RegisterNoSlot -> "register-no-slot"
  RegisterCreateFault -> "register-create-fault"
  RegisterStartFault -> "register-start-fault"
  RegisterZeroPeriod -> "register-zero-period"
  Unregister token -> "unregister-" ++ show token
  Tick -> "tick"
  TickRegistryFault -> "tick-registry-fault"

statusFor :: Role -> Variant -> Word32
statusFor role variant = xStatus (expectation layoutA role variant (seedState layoutA role B.empty B.empty))

patchState :: Layout -> Variant -> State -> State
patchState layout variant state = case variant of
  RegisterNoSlot -> busyAll state
  RegisterCreateFault -> rootWord 0x108 stubCreateFault state
  RegisterStartFault -> rootWord 0x110 stubStartFault state
  TickRegistryFault -> handler 1 (registryWord (0x004 + 63 * 4) stubDelivery state)
  Tick -> handler 1 state
  Unregister _ -> handler 0 state
  _ -> state
  where
    rootWord :: Word32 -> Word32 -> State -> State
    rootWord offset value s = s {stRoot = poke (stRoot s) (fromIntegral offset) value}
    registryWord :: Word32 -> Word32 -> State -> State
    registryWord offset value s = s {stRegistry = poke (stRegistry s) (fromIntegral offset) value}
    handler :: Word32 -> State -> State
    handler value s = s {stWords = poke (stWords s) (fromIntegral (lHandlerContext layout - lWords layout)) value}
    busyAll :: State -> State
    busyAll s =
      s
        { stRegistry =
            foldl
              (\bytes index -> poke bytes (0x304 + index * 4) 1)
              (stRegistry s)
              [0 .. 63 :: Int]
        }

-- | The observed regions two roles must agree on. The stock heap lives inside kernel-objects; the
-- owned heap is the arena. The context heap pointers differ by role and are masked.
compareRoles :: Layout -> NativeObservation -> NativeObservation -> [String]
compareRoles layout stock owned =
  [ "kernel-low differs between stock and owned"
  | block "kernel-low" stock /= block "kernel-low" owned
  ]
    ++ [ "kernel-objects differ between stock and owned outside the heap and its pointers"
       | fmap maskKernel (block "kernel-objects" stock) /= fmap maskKernel (block "kernel-objects" owned)
       ]
    ++ [ "heap differs between stock and owned"
       | heapBytes Stock stock /= heapBytes Owned owned
       ]
    ++ [ "ram-block differs between stock and owned"
       | block "ram-block" stock /= block "ram-block" owned
       ]
    ++ [ "interfaces-token differs between stock and owned"
       | block "interfaces-token" stock /= block "interfaces-token" owned
       ]
    ++ [ "guard differs between stock and owned"
       | block "guard" stock /= block "guard" owned
       ]
  where
    block name observation = lookup (RegionId (fromString name)) (nativeMemory observation)
    heapBytes role observation = case block (heapRegion role) observation of
      Nothing -> Nothing
      Just bytes -> Just (B.take 0x5c0 (B.drop (heapOffset role) bytes))
    heapOffset Stock = fromIntegral (lHeap layout - lWords layout)
    heapOffset Owned = 0
    heapRegion Stock = "kernel-objects"
    heapRegion Owned = "arena"
    maskKernel bytes =
      let heapStart = fromIntegral (lHeap layout - lWords layout)
          pointers = [ fromIntegral (lContexts layout + 0x34 - lWords layout)
                     , fromIntegral (lContexts layout + 0x84 + 0x34 - lWords layout)
                     , fromIntegral (controlSlot + 0x0c - lWords layout)
                     ]
          zeroed = foldl (\b offset -> poke b offset 0) bytes pointers
       in B.take heapStart zeroed <> B.replicate 0x5c0 0 <> B.drop (heapStart + 0x5c0) zeroed

data CaseRun = CaseRun
  { runStage :: String
  , runRole :: String
  , runChecks :: [(String, Bool)]
  , runDifferences :: [String]
  , runReuse :: BatchReuse
  }

data ControlRun = ControlRun
  { controlName :: String
  , controlCase :: String
  , controlHolds :: Bool
  }

buildCase :: Layout -> Inputs -> Role -> Variant -> State -> CaseSpec
buildCase layout inputs role variant state =
  CaseSpec
    { caseId = variantName variant ++ "-" ++ roleLabel role
    , caseProvenance = "X-M1 1.50 core-0 app callback join; x70view 55436fc85c0560dd252b0410af6f2427fe198cd3 (cpu_startup_phases.c:349-399, kernel_publication.c:3-118, startup_requests.c:218-337)"
    , caseExclusion = "Fixture kernel context, timer heap, lock stubs, handler-context word, registry and task-definition pages; the root and interface tables are the stock initializer's and publisher prefix's output; not a live run."
    , caseEntry = GuestAddress (fromIntegral entry)
    , caseExit = GuestAddress (fromIntegral exitAddress)
    , caseRegions = stateCaseRegions ++ codeCaseRegions
    , caseFunctions =
        [ (GuestAddress (fromIntegral address), size)
        | (address, size) <- retainedFunctions ++ roleFunctions ++ stubFunctions
        ]
    , caseInitial =
        [ (R0, argument0), (R1, 0), (R2, stackGuard), (R3, tokenAddress)
        ]
          ++ [(register, seedRegister register) | register <- [R4 .. R12]]
          ++ [(Sp, stackPointer), (Lr, exitAddress), (Cpsr, 0x13)]
    , caseExpectedStatus = Returned
    , caseExpectedRegisters =
        (R0, statusFor role variant)
          : [(register, seedRegister register) | register <- [R4 .. R11]]
          ++ [(Sp, stackPointer)]
    , caseExpectedMemory = []
    , caseCp14Identity = Just (lCp14 layout)
    , caseKernel = Just (KernelCase plainLimit Nothing (Just 0x80001234) (Just 0) (Just 0) [R1, R2, R3, R12, Lr] [])
    }
  where
    entry = case variant of
      Register _ -> appRegister
      RegisterNoSlot -> appRegister
      RegisterCreateFault -> appRegister
      RegisterStartFault -> appRegister
      RegisterZeroPeriod -> appRegister
      Unregister _ -> appUnregister
      Tick -> lTickEntry layout
      TickRegistryFault -> lTickEntry layout
    argument0 = case variant of
      Register argument -> argument
      RegisterZeroPeriod -> 0
      RegisterStartFault -> 30
      RegisterCreateFault -> 30
      RegisterNoSlot -> 30
      Unregister token -> token
      _ -> 0
    stateCaseRegions =
      [ mkRegion (RegionId (fromString name)) (GuestAddress (fromIntegral base)) size permission bytes
      | (name, base, size, permission, bytes) <- stateRegions layout role state
      ]
    codeCaseRegions =
      [ mkRegion (RegionId (fromString name)) (GuestAddress (fromIntegral base)) size permission bytes
      | (name, base, size, permission, bytes) <- codeRegions layout inputs role
      ]
    retainedFunctions = case (role, inKernel inputs) of
      (Owned, PackagedKernel _ mappings) ->
        [ function
        | function@(address, size) <- commonFunctions
        , address /= dispatchEvent
        , not (any (\(owned, bytes) -> address < owned + bytes && owned < address + size) (concat [functions | (_, _, functions) <- mappings]))
        ]
      _ -> filter (\(address, _) -> role == Stock || address /= dispatchEvent) commonFunctions
    roleFunctions = case role of
      Stock -> [(appRegister, 0x2a0), (appUnregister, 0xe4)] ++ stockFunctions DomainA
      Owned -> inAppSymbols inputs ++ case inKernel inputs of
        PackagedKernel _ mappings -> concat [functions | (_, _, functions) <- mappings]
        StandaloneKernel ->
          inCyclicSymbols inputs
            ++ [(Timer.ownedAddress symbol, Timer.ownedSize symbol) | symbol <- inTimerSymbols inputs]

commonFunctions :: [(Word32, Word32)]
commonFunctions =
  [ (dispatchEvent, 0x1c)
  , (dispatchError, 0x1c)
  , (stackGuard, 0x54)
  , (stackCheck, 0xf8)
  , (0x412bc, 0x10)
  , (0x427b8, 0x1c)
  , (0x41140, 0x24)
  , (0x41164, 0x28)
  , (0x41a34, 0x158)
  ]

stubFunctions :: [(Word32, Word32)]
stubFunctions =
  [ (stubAcquire, 0x18)
  , (stubRelease, 0x18)
  , (stubError, 0x14)
  , (stubCreateFault, 0x08)
  , (stubStartFault, 0x08)
  , (stubDelivery, 0x14)
  ]

rootPrepCase :: Layout -> B.ByteString -> CaseSpec
rootPrepCase layout loader =
  CaseSpec
    { caseId = "prep-root"
    , caseProvenance = "stock loader A kernel_publish_interfaces at 0x40430; x70view 55436fc85c0560dd252b0410af6f2427fe198cd3 kernel0/src/core/kernel_publication.c:3-118"
    , caseExclusion = "Runs the stock initializer to publish the root table; the code at the published addresses is the role under test."
    , caseEntry = GuestAddress (fromIntegral (lLoaderInit layout))
    , caseExit = GuestAddress (fromIntegral exitAddress)
    , caseRegions =
        [ mkRegion (RegionId "loader-init") (GuestAddress (fromIntegral (lLoaderInit layout))) 0x57c ReadExecute (slice 0x40000 loader (lLoaderInit layout) 0x57c)
        , mkRegion (RegionId "root") (GuestAddress (fromIntegral (lRoot layout))) 0x1700 ReadWrite (B.replicate 0x1700 0)
        , mkRegion (RegionId "stack") (GuestAddress (fromIntegral stackBase)) 0x1000 ReadWrite (B.replicate 0x1000 0)
        , mkRegion (RegionId "exit") (GuestAddress (fromIntegral exitAddress)) 0x10 ReadExecute (B.replicate 0x10 0)
        ]
    , caseFunctions = [(GuestAddress (fromIntegral (lLoaderInit layout)), 0x57c)]
    , caseInitial =
        [(register, seedRegister register) | register <- [R0 .. R12]]
          ++ [(Sp, stackPointer), (Lr, exitAddress), (Cpsr, 0x10)]
    , caseExpectedStatus = Returned
    , caseExpectedRegisters = [(Sp, stackPointer)]
    , caseExpectedMemory = []
    , caseCp14Identity = Nothing
    , caseKernel = Nothing
    }

interfacesPrepCase :: Inputs -> CaseSpec
interfacesPrepCase inputs =
  CaseSpec
    { caseId = "prep-interfaces"
    , caseProvenance = "stock image4 root publisher prefix 0x725a04; x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; loader image headers at 0x40000010 and 0xd00010"
    , caseExclusion = "Stops at the publisher's cache-clean call 0x725a34; the remaining dispatch state is not published."
    , caseEntry = GuestAddress (fromIntegral publisher)
    , caseExit = GuestAddress (fromIntegral exitAddress)
    , caseRegions =
        [ mkRegion (RegionId "publisher") (GuestAddress (fromIntegral publisher)) 0x34 ReadExecute (slice image4Base (inImage4 inputs) publisher 0x34)
        , mkRegion (RegionId "interfaces") (GuestAddress (fromIntegral interfacesAddress)) 8 ReadWrite (B.replicate 8 0)
        , mkRegion (RegionId "loader-a-literals") (GuestAddress 0x40000) 0x20 ReadOnly (slice 0x40000 (inLoaderA inputs) 0x40000 0x20)
        , mkRegion (RegionId "loader-b-literals") (GuestAddress 0xd0000) 0x20 ReadOnly (slice 0xd0000 (inLoaderB inputs) 0xd0000 0x20)
        , mkRegion (RegionId "stack") (GuestAddress (fromIntegral stackBase)) 0x1000 ReadWrite (B.replicate 0x1000 0)
        , mkRegion (RegionId "exit") (GuestAddress (fromIntegral exitAddress)) 0x10 ReadExecute (B.replicate 0x10 0)
        ]
    , caseFunctions = [(GuestAddress (fromIntegral publisher), 0x34)]
    , caseInitial =
        [(register, seedRegister register) | register <- [R0 .. R12]]
          ++ [(Sp, stackPointer), (Lr, exitAddress), (Cpsr, 0x13)]
    , caseExpectedStatus = StopAddress
    , caseExpectedRegisters = [(Sp, stackPointer - 8)]
    , caseExpectedMemory = []
    , caseCp14Identity = Nothing
    , caseKernel = Just (KernelCase plainLimit (Just (GuestAddress 0x725a34)) Nothing Nothing Nothing [R1, R2, R3, R12, Lr] [])
    }

runBatch :: FilePath -> FilePath -> FilePath -> [CaseSpec] -> IO (BatchReuse, [(CaseId, NativeObservation)])
runBatch runner cacheRoot directory specs = do
  path <- writeBatch directory specs
  cachedObservedBatch cacheRoot runner path

observationOf :: String -> [(CaseId, NativeObservation)] -> NativeObservation
observationOf ident pairs =
  case [observation | (caseId', observation) <- pairs, caseIdText caseId' == ident] of
    [observation] -> observation
    _ -> error ("join batch did not return observation " ++ ident)

checkRun :: Layout -> Role -> Variant -> State -> NativeObservation -> [String] -> BatchReuse -> CaseRun
checkRun layout role variant state observation differences reuse =
  CaseRun
    { runStage = variantName variant
    , runRole = roleLabel role
    , runChecks = checks
    , runDifferences = [description | (description, False) <- checks] ++ differences
    , runReuse = reuse
    }
  where
    expected = expectation layout role variant state
    checks =
      [ (describe check', either (const False) (const True) (evaluate layout role observation check'))
      | check' <- RunnerStatusIs Returned : xChecks expected
      ]

payloadHolds :: Inputs -> NativeObservation -> Bool
payloadHolds inputs observation = all unchanged
  [ (name, bytes)
  | (name, _, _, _, bytes) <- codeRegions layoutA inputs Owned
  , take 6 name == "loader"
  ]
  where
    unchanged (name, bytes) = lookup (RegionId (fromString name)) (nativeMemory observation) == Just bytes

runOneStage :: FilePath -> FilePath -> Inputs -> Variant -> (State, State) -> IO (CaseRun, CaseRun, State, State, BatchReuse, NativeObservation, NativeObservation)
runOneStage runner cacheRoot inputs variant (stockState, ownedState) = do
  let patchedStock = patchState layoutA variant stockState
      patchedOwned = patchState layoutA variant ownedState
      stockSpec = buildCase layoutA inputs Stock variant patchedStock
      ownedSpec = buildCase layoutA inputs Owned variant patchedOwned
      directory = cacheRoot </> variantName variant
  (reuse, pairs) <- runBatch runner cacheRoot directory [stockSpec, ownedSpec]
  let stockObservation = observationOf (variantName variant ++ "-stock") pairs
      ownedObservation = observationOf (variantName variant ++ "-owned") pairs
      repair = case variant of
        RegisterZeroPeriod -> True
        Unregister _ -> True
        _ -> False
      differences = if repair then [] else compareRoles layoutA stockObservation ownedObservation
  pure
    ( checkRun layoutA Stock variant patchedStock stockObservation differences reuse
    , let checked = checkRun layoutA Owned variant patchedOwned ownedObservation differences reuse
          payloadCheck = ("complete executing loader bytes", payloadHolds inputs ownedObservation)
       in checked
            { runChecks = runChecks checked ++ [payloadCheck]
            , runDifferences = runDifferences checked ++ [fst payloadCheck | not (snd payloadCheck)]
            }
    , stateFrom layoutA Stock stockObservation
    , stateFrom layoutA Owned ownedObservation
    , reuse
    , stockObservation
    , ownedObservation
    )

runJoin :: Scope -> FilePath -> FilePath -> FilePath -> Inputs -> IO ([CaseRun], [ControlRun])
runJoin scope runner resultsDirectory cacheRoot inputs = do
  let rootSpec = rootPrepCase layoutA (inLoaderA inputs)
  (rootReuse, rootPairs) <- runBatch runner cacheRoot (resultsDirectory </> "prep-root") [rootSpec]
  let rootObservation = observationOf "prep-root" rootPairs
      rootChecks observation =
        [ ("root table " ++ describe check', either (const False) (const True) (evaluateWith [("root", lRoot layoutA, 0x1700)] layoutA Stock observation check'))
        | check' <-
            [ RunnerStatusIs Returned
            , RegionWord "root" (lRoot layoutA + 0x108) (lCreateEntry layoutA)
            , RegionWord "root" (lRoot layoutA + 0x10c) (lDeleteEntry layoutA)
            , RegionWord "root" (lRoot layoutA + 0x110) (lStartEntry layoutA)
            , RegionWord "root" (lRoot layoutA + 0x114) (lStopEntry layoutA)
            , RegionWord "root" (lRoot layoutA + 0x164) (lIrqSave layoutA)
            , RegionWord "root" (lRoot layoutA + 0x168) (lIrqRestore layoutA)
            , RegionWord "root" (lRoot layoutA + 0x1618) (lCoreIndexAdapter layoutA)
            , RegionWord "root" (lRoot layoutA + 0x174) 0x400c0800
            ]
        ]
      rootRun observation =
        let checks = rootChecks observation
         in CaseRun "prep-root" "fixture" checks [description | (description, False) <- checks] rootReuse
      rootBytes = regionBytesOf rootObservation (RegionId "root")
  let interfaceSpec = interfacesPrepCase inputs
  (interfaceReuse, interfacePairs) <- runBatch runner cacheRoot (resultsDirectory </> "prep-interfaces") [interfaceSpec]
  let interfaceObservation = observationOf "prep-interfaces" interfacePairs
      interfaceChecks observation =
        [ ("interface table " ++ describe check', either (const False) (const True) (evaluateWith [("interfaces", interfacesAddress, 8)] layoutA Stock observation check'))
        | check' <-
            [ RunnerStatusIs StopAddress
            , RegionWord "interfaces" interfacesAddress (lRoot layoutA)
            , RegionWord "interfaces" (interfacesAddress + 4) 0x400ddaa0
            ]
        ]
      interfaceRun observation =
        let checks = interfaceChecks observation
         in CaseRun "prep-interfaces" "fixture" checks [description | (description, False) <- checks] interfaceReuse
      interfaceBytes = regionBytesOf interfaceObservation (RegionId "interfaces")
      initialStock = seedState layoutA Stock rootBytes interfaceBytes
      initialOwned = seedState layoutA Owned rootBytes interfaceBytes
  (registerStock, registerOwned, afterRegisterStock, afterRegisterOwned, _, _, _) <-
    runOneStage runner cacheRoot inputs (Register 30) (initialStock, initialOwned)
  (tick0Stock, tick0Owned, afterTick0Stock, afterTick0Owned, _, _, _) <-
    runOneStage runner cacheRoot inputs Tick (afterRegisterStock, afterRegisterOwned)
  (tick10Stock, tick10Owned, afterTick10Stock, afterTick10Owned, _, _, _) <-
    runOneStage runner cacheRoot inputs Tick (afterTick0Stock, afterTick0Owned)
  (tick20Stock, tick20Owned, afterTick20Stock, afterTick20Owned, _, _, noDeliveryOwned) <-
    runOneStage runner cacheRoot inputs Tick (afterTick10Stock, afterTick10Owned)
  (deliverStock, deliverOwned, afterDeliverStock, afterDeliverOwned, deliverReuse, _, deliveryOwned) <-
    runOneStage runner cacheRoot inputs Tick (afterTick20Stock, afterTick20Owned)
  (unregisterStock, unregisterOwned, afterUnregisterStock, afterUnregisterOwned, _, _, _) <-
    runOneStage runner cacheRoot inputs (Unregister 30001) (afterDeliverStock, afterDeliverOwned)
  (postTickStock, postTickOwned, _, _, _, _, _) <-
    runOneStage runner cacheRoot inputs Tick (afterUnregisterStock, afterUnregisterOwned)
  let faultInputs = case scope of
        LifecycleSmoke -> []
        FullScope ->
          [ (RegisterCreateFault, (initialStock, initialOwned))
          , (RegisterStartFault, (initialStock, initialOwned))
          , (RegisterNoSlot, (initialStock, initialOwned))
          , (RegisterZeroPeriod, (initialStock, initialOwned))
          , (Unregister 12345, (afterDeliverStock, afterDeliverOwned))
          , (TickRegistryFault, (afterTick20Stock, afterTick20Owned))
          ]
  faultRuns <- forM faultInputs $ \(variant, state) -> do
    (stock, owned, _, _, _, _, _) <- runOneStage runner cacheRoot inputs variant state
    pure [stock, owned]
  let runs =
        [ rootRun rootObservation
        , interfaceRun interfaceObservation
        , registerStock
        , registerOwned
        , tick0Stock
        , tick0Owned
        , tick10Stock
        , tick10Owned
        , tick20Stock
        , tick20Owned
        , deliverStock
        , deliverOwned
        , unregisterStock
        , unregisterOwned
        , postTickStock
        , postTickOwned
        ] ++ concat faultRuns
      controls =
        comparatorControls deliveryOwned noDeliveryOwned
          ++ statusControls Returned (\observation -> checkRun layoutA Owned Tick afterTick20Owned observation [] deliverReuse) deliveryOwned
          ++ statusControls Returned rootRun rootObservation
          ++ statusControls StopAddress interfaceRun interfaceObservation
  pure (runs, controls)

statusControls :: RunnerStatus -> (NativeObservation -> CaseRun) -> NativeObservation -> [ControlRun]
statusControls expected check observation =
  [ ControlRun
      ("runner-" ++ runnerStatusLabel status)
      (runStage baseline)
      (null (runDifferences baseline) && not (null (runDifferences (check (observation {nativeStatus = status})))))
  | status <- [InstructionBudget, MemoryGuard, StopAddress, Returned]
  , status /= expected
  ]
  where
    baseline = check observation

-- | Comparator sensitivity: each control mutates one observed word and requires its check to fail.
comparatorControls :: NativeObservation -> NativeObservation -> [ControlRun]
comparatorControls observation noDelivery =
  [ ControlRun name "delivery" (refused check' (mutate name))
  | (name, check') <- checks
  ]
    ++ [ ControlRun "no-delivery-guard" "tick-20" (refused (RegionWord "guard" guardAddress 1) (mutateAbs noDelivery guardAddress 2))
       , ControlRun "no-delivery-expiry" "tick-20" (refused (RegionWord "controls" (controlSlot + 0x08) 30) (mutateAbs noDelivery (controlSlot + 0x08) 60))
       ]
  where
    checks =
      [ ("guard", RegionWord "guard" guardAddress 2)
      , ("registry-token", RegionWord "registry" (registryBase + 0x104 + 63 * 4) 30001)
      , ("config-exists", RegionWord "configs" (configSlot + 0x14) 1)
      , ("control-expiry", RegionWord "controls" (controlSlot + 0x08) 60)
      , ("heap-expiry", HeapNode 1 60 controlTimer)
      , ("timer-count", RegionWord "words" (lTimerCount layoutA) 1)
      , ("status", RegisterIs R0 0)
      , ("token", RegionWord "token" tokenAddress 30001)
      , ("cpsr", CpsrControlIs 0x13)
      , ("delivery-receipt", RegionWord "entries" deliveryReceipt 0)
      ]
    refused check' mutated = case evaluate layoutA Owned mutated check' of
      Left _ -> True
      Right () -> False
    mutate name = case name of
      "guard" -> mutateAbs observation guardAddress 3
      "registry-token" -> mutateAbs observation (registryBase + 0x104 + 63 * 4) 7
      "config-exists" -> mutateAbs observation (configSlot + 0x14) 0
      "control-expiry" -> mutateAbs observation (controlSlot + 0x08) 61
      "heap-expiry" -> mutateAbs observation (heapBase layoutA Owned) 61
      "timer-count" -> mutateAbs observation (lTimerCount layoutA) 0
      "status" -> mutateRegister observation R0 1
      "token" -> mutateAbs observation tokenAddress 0
      "cpsr" -> mutateCpsr observation
      "delivery-receipt" -> mutateAbs observation deliveryReceipt 64
      _ -> observation

mutateAbs :: NativeObservation -> Word32 -> Word32 -> NativeObservation
mutateAbs observation address value = go (emittedBases layoutA Owned)
  where
    go [] = observation
    go ((name, base, size) : rest)
      | address >= base && address + 4 <= base + fromIntegral size =
          observation
            { nativeMemory =
                [ (ident, if regionName ident == name then poke bytes (fromIntegral (address - base)) value else bytes)
                | (ident, bytes) <- nativeMemory observation
                ]
            }
      | otherwise = go rest

mutateRegister :: NativeObservation -> Register -> Word32 -> NativeObservation
mutateRegister observation register value =
  observation {nativeRegisters = M.insert register value (nativeRegisters observation)}

mutateCpsr :: NativeObservation -> NativeObservation
mutateCpsr observation =
  observation {nativeRegisters = M.adjust (`xor` 0x80) Cpsr (nativeRegisters observation)}
