{-# LANGUAGE OverloadedStrings #-}

-- | Global-lock try, release and observe cases over the stock loaders and the owned A32 entries.
module Replacement.RtosGlobalLock.Suite
  ( Image (..)
  , Aggregate (..)
  , readAggregate
  , CaseRun (..)
  , ControlRun (..)
  , runDomain
  , parseOwnedSymbols
  ) where

import Control.Monad (forM, forM_, unless)
import Data.Bits (xor)
import Data.List (isPrefixOf, sort, sortOn)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32, Word8)
import Emulation.CachedNativeBatch (BatchReuse, cachedObservedBatch)
import Emulation.NativeObservation
import Emulation.OwnedCase
import Replacement.RtosOwnFirst.Regions (Domain (..), domainLabel)
import qualified Replacement.RtosOwnFirst.Regions as Regions
import qualified Replacement.RtosPlatform.Aggregate as Agg
import qualified Replacement.RtosPlatform.Union as Union
import System.FilePath ((</>))

data Image = Image { imageBase :: Word32, imageBytes :: B.ByteString }

data Aggregate = Aggregate
  { aggregateImage :: Image
  , aggregateSymbols :: [(Word32, Word32)]
  , aggregatePlacements :: [Agg.SectionPlacement]
  , aggregateEntries :: [Word32]
  }

readAggregate :: Domain -> Union.Union -> FilePath -> FilePath -> FilePath -> IO Aggregate
readAggregate domain union bin symbolsPath placementsPath = do
  bytes <- B.readFile bin
  symbols <- either fail pure . Agg.parseAggregateSymbols =<< B.readFile symbolsPath
  placements <- either fail pure . Agg.parsePlacements =<< B.readFile placementsPath
  let domainMap = Union.domainMap union domain
      carriers = map Union.carrierExtent (Union.mapCarriers domainMap)
      lows = [low | Regions.Extent (Regions.GuestAddress low) _ <- carriers]
      highs = [high | Regions.Extent _ (Regions.GuestAddress high) <- carriers]
  unless (not (null carriers) && not (null symbols) && not (null placements)) (fail "aggregate metadata is empty")
  let base = minimum lows
      top = maximum highs
      image = Agg.AggregateImage base bytes
      spans = [(Agg.symbolAddress symbol, Agg.symbolSize symbol) | symbol <- symbols]
        ++ [(Agg.placementAddress placement, Agg.placementBytes placement) | placement <- placements, Agg.placementBytes placement > 0]
      admitted (address, size) = any (\(Regions.Extent (Regions.GuestAddress low) (Regions.GuestAddress high)) ->
        low <= address && toInteger address + toInteger size <= toInteger high) carriers
  unless (toInteger (B.length bytes) == toInteger top - toInteger base) (fail "effective aggregate bytes do not span the complete carriers")
  forM_ spans $ \span'@(address, size) -> do
    unless (admitted span') (fail "aggregate function or placement is outside a live carrier")
    _ <- either fail pure (Agg.sliceAggregate image address size)
    pure ()
  forM_ symbols $ \symbol ->
    forM_ [p | p <- placements, Agg.placementBytes p > 0, not (textPlacement p)] $ \p ->
      unless (toInteger (Agg.symbolAddress symbol) + toInteger (Agg.symbolSize symbol) <= toInteger (Agg.placementAddress p)
        || toInteger (Agg.placementAddress p) + toInteger (Agg.placementBytes p) <= toInteger (Agg.symbolAddress symbol))
        (fail "aggregate executable symbol overlaps readonly placement")
  let entries = [address | entry <- Union.mapEntries domainMap, let Regions.GuestAddress address = Union.entryAddress entry]
  forM_ (Union.mapEntries domainMap) $ \entry -> do
    let Regions.GuestAddress address = Union.entryAddress entry
    unless (any (\symbol -> Agg.symbolAddress symbol == address && Agg.symbolName symbol == Union.entryVeneer entry) symbols)
      (fail "aggregate fixed entry lacks its positive veneer symbol")
  pure (Aggregate (Image base bytes)
    [(Agg.symbolAddress symbol, Agg.symbolSize symbol) | symbol <- symbols] placements entries)

aggregateRegions :: Aggregate -> [CaseRegion]
aggregateRegions aggregate =
  [mkRegion ident (GuestAddress (fromIntegral low)) (B.length bytes) permission bytes
  | (ident, low, permission, bytes) <- aggregateStorage aggregate]

aggregateStorage :: Aggregate -> [(RegionId, Word32, Permission, B.ByteString)]
aggregateStorage aggregate =
  [(RegionId ("aggregate-" ++ show base ++ "-" ++ show low), low, permission, slice image low (high - low))
  | (low, high) <- zip boundaries (drop 1 boundaries)
  , let permission = if any (\(start, end) -> start <= low && high <= end) readonly then ReadOnly else ReadExecute]
  where
    image = aggregateImage aggregate
    base = imageBase image
    top = base + fromIntegral (B.length (imageBytes image))
    readonly = [(Agg.placementAddress p, Agg.placementAddress p + Agg.placementBytes p)
      | p <- aggregatePlacements aggregate, Agg.placementBytes p > 0, not (textPlacement p)]
    boundaries = M.keys (M.fromList [(address, ()) | address <- base : top : concatMap (\(low, high) -> [low, high]) readonly])

aggregateUnchanged :: [Aggregate] -> NativeObservation -> Bool
aggregateUnchanged aggregates observation = all unchanged (concatMap aggregateStorage aggregates)
  where
    unchanged (ident, _, _, bytes) = lookup ident (nativeMemory observation) == Just bytes

textPlacement :: Agg.SectionPlacement -> Bool
textPlacement placement = Agg.placementSection placement == ".text" || ".text." `isPrefixOf` Agg.placementSection placement

aggregateFunctions :: [Aggregate] -> [(Word32, Word32)]
aggregateFunctions aggregates =
  [(low, high - low)
  | (base, size) <- merge (sortOn fst spans)
  , let boundaries = base : filter (\address -> base < address && address < base + size) entries ++ [base + size]
  , (low, high) <- zip boundaries (drop 1 boundaries)]
  where
    spans = concatMap aggregateSymbols aggregates
      ++ [(Agg.placementAddress p, Agg.placementBytes p) | aggregate <- aggregates,
        p <- aggregatePlacements aggregate, Agg.placementBytes p > 0, textPlacement p]
    entries = sort (concatMap aggregateEntries aggregates)
    merge [] = []
    merge ((base, size) : rest) = extend base (base + size) rest
    extend base top [] = [(base, top - base)]
    extend base top ((low, size) : rest)
      | low <= top = extend base (max top (low + size)) rest
      | otherwise = (base, top - base) : extend low (low + size) rest

data Role = Stock | Owned deriving (Eq, Show)

data Protection = WriteProtection | FetchProtection deriving (Eq, Show)

data Service = Try | Release | Observe deriving (Eq, Show)

data Case = Case
  { caseName :: String
  , caseService :: Service
  , caseArgument :: Word32
  , caseStamp :: Word32
  , caseDefault :: Word32
  , caseExplicit :: Word32
  , caseCpsr :: Word32
  , caseResult :: Word32
  , caseFinalDefault :: Word32
  , caseFinalExplicit :: Word32
  }

data CaseRun = CaseRun
  { caseDomain :: Domain
  , runName :: String
  , runChecks :: [(String, Bool)]
  , runDifferences :: [String]
  , runReuse :: (BatchReuse, BatchReuse)
  }

data ControlRun = ControlRun
  { controlName :: String
  , controlCase :: String
  , controlHolds :: Bool
  }

refused :: Word32
refused = 0xffff_ffff

globalBase, globalSize, globalWord :: Word32
globalBase = 0x400c_0800
globalSize = 0x800
globalWord = 0x400c_0804

explicitBase, explicitSize, explicitWord :: Word32
explicitBase = 0x409c_2800
explicitSize = 0x800
explicitWord = 0x409c_2808

stackBase, stackSize, stackPointer :: Word32
stackBase = 0x0030_0000
stackSize = 0x1000
stackPointer = stackBase + 0x800

exitAddress :: Word32
exitAddress = 0x0100_0200

entryBase, entrySize :: Domain -> Word32
entryBase DomainA = 0x412cc
entryBase DomainB = 0xd12cc
entrySize _ = 0x54

atomicsBase, atomicsSize :: Domain -> Word32
atomicsBase DomainA = 0x427d4
atomicsBase DomainB = 0xd27d4
atomicsSize _ = 0x5c

tryEntry, releaseEntry, observeEntry :: Domain -> Word32
tryEntry domain = entryBase domain
releaseEntry domain = entryBase domain + 0x1c
observeEntry domain = entryBase domain + 0x38

seedRegister :: Register -> Word32
seedRegister register = 0xa5a5_0000 + fromIntegral (fromEnum register)

-- | The stock slot and atomic words at 0x412cc and 0x427d4, byte-identical in both loaders.
stockSlotWords, stockAtomicWords :: [Word32]
stockSlotWords =
  [ 0xe92d4800, 0xe28db004, 0xe3500000, 0x03000804, 0x0344000c, 0xeb00053b, 0xe8bd8800
  , 0xe92d4800, 0xe28db004, 0xe3500000, 0x03000804, 0x0344000c, 0xeb00053e, 0xe8bd8800
  , 0xe92d4800, 0xe28db004, 0xe3500000, 0x03000804, 0x0344000c, 0xeb000542, 0xe8bd8800
  ]

stockAtomicWords =
  [ 0xe1902f9f, 0xe3520000, 0x13e00000, 0x112fff1e, 0x01802f91, 0xe3520000, 0x0e072fba
  , 0x03a00000, 0x012fff1e, 0xeafffff5
  , 0xe3a01000, 0xe1902f9f, 0xe3520000, 0x03e00000, 0x012fff1e, 0x11802f91, 0xe3520000
  , 0x0e072fba, 0x03a00000, 0x012fff1e, 0xeafffff4
  , 0xe5900000, 0xe12fff1e
  ]

cases :: [Case]
cases =
  [ Case "try-free-default" Try 0 0x11 0 0 0x13 0 0x11 0
  , Case "try-held-default-irq" Try 0 0x12 0x11 0 0x13 refused 0x11 0
  , Case "try-held-default-critical" Try 0 0x22 0x21 0 0x13 refused 0x21 0
  , Case "try-free-explicit" Try explicitWord 0x21 0x12 0 0x13 0 0x12 0x21
  , Case "try-held-explicit" Try explicitWord 0x21 0 0x22 0x13 refused 0 0x22
  , Case "try-zero-stamp" Try 0 0 0 0 0x13 0 0 0
  , Case "try-arbitrary-stamp" Try 0 0xdeadbeef 0 0 0x13 0 0xdeadbeef 0
  , Case "try-free-masked" Try 0 0x11 0 0 0x93 0 0x11 0
  , Case "try-held-fiq" Try 0 0x12 0x11 0 0x53 refused 0x11 0
  , Case "release-held-default" Release 0 0 0x11 0 0x13 0 0 0
  , Case "release-free-default" Release 0 0 0 0 0x13 refused 0 0
  , Case "release-held-explicit" Release explicitWord 0 0x12 0x21 0x13 0 0x12 0
  , Case "release-free-explicit" Release explicitWord 0 0 0 0x13 refused 0 0
  , Case "release-arbitrary" Release 0 0 0xdeadbeef 0 0x13 0 0 0
  , Case "release-held-masked" Release 0 0 0x11 0 0xd3 0 0 0
  , Case "observe-free" Observe 0 0 0 0 0x13 0 0 0
  , Case "observe-irq" Observe 0 0 0x11 0 0x13 0x11 0x11 0
  , Case "observe-critical" Observe 0 0 0x22 0 0x13 0x22 0x22 0
  , Case "observe-explicit" Observe explicitWord 0 0 0x5a5a5a5a 0x13 0x5a5a5a5a 0 0x5a5a5a5a
  , Case "observe-masked" Observe 0 0 0x11 0 0x93 0x11 0x11 0
  , Case "observe-arbitrary" Observe 0 0 0xdeadbeef 0 0x13 0xdeadbeef 0xdeadbeef 0
  , Case "try-free-default-critical" Try 0 0x21 0 0 0x13 0 0x21 0
  , Case "release-held-default-critical" Release 0 0 0x21 0 0x13 0 0 0
  , Case "direct-try-free" Try 0x400c0804 0x11 0 0 0x13 0 0x11 0
  , Case "direct-try-held" Try 0x400c0804 0x12 0x11 0 0x13 refused 0x11 0
  , Case "direct-try-explicit" Try explicitWord 0x21 0 0 0x13 0 0 0x21
  , Case "direct-try-free-masked" Try 0x400c0804 0x11 0 0 0x93 0 0x11 0
  , Case "direct-release-held" Release 0x400c0804 0 0x11 0 0x13 0 0 0
  , Case "direct-release-free" Release 0x400c0804 0 0 0 0x13 refused 0 0
  , Case "direct-release-held-fiq" Release 0x400c0804 0 0x11 0 0x53 0 0 0
  , Case "direct-observe-irq" Observe 0x400c0804 0 0x11 0 0x13 0x11 0x11 0
  , Case "direct-observe-explicit" Observe explicitWord 0 0 0x5a5a5a5a 0x13 0x5a5a5a5a 0 0x5a5a5a5a
  ]

isDirect :: Case -> Bool
isDirect one = "direct-" `isPrefixOf` caseName one

lockAddress :: Case -> Word32
lockAddress one
  | caseArgument one == 0 = globalWord
  | otherwise = caseArgument one

storeValue :: Case -> Maybe Word32
storeValue one = case (caseService one, caseResult one) of
  (Try, 0) -> Just (caseStamp one)
  (Release, 0) -> Just 0
  _ -> Nothing

data Check
  = StatusReturned
  | ResultIs Word32
  | WordIs String Word32 Word32
  | CpsrIs Word32
  | CalleeSavedIs [(Register, Word32)]
  | StackIs Word32
  | LockEffect (Maybe Word32)
  deriving (Eq, Show)

checksFor :: Case -> [Check]
checksFor one =
  [ StatusReturned
  , ResultIs (caseResult one)
  , WordIs "global" globalWord (caseFinalDefault one)
  , WordIs "explicit" explicitWord (caseFinalExplicit one)
  , CpsrIs (caseCpsr one)
  , CalleeSavedIs [(register, seedRegister register) | register <- [R4 .. R11]]
  , StackIs stackPointer
  , LockEffect (storeValue one)
  ]

describe :: Check -> String
describe check' = case check' of
  StatusReturned -> "returned"
  ResultIs value -> "r0 = " ++ showHexWord value
  WordIs region address value -> region ++ " " ++ showHexWord address ++ " = " ++ showHexWord value
  CpsrIs value -> "cpsr control = " ++ showHexWord (cpsrControl value)
  CalleeSavedIs pairs -> "callee-saved " ++ show [(registerLabel register, showHexWord value) | (register, value) <- pairs]
  StackIs value -> "sp = " ++ showHexWord value
  LockEffect store -> "lock store " ++ show store

check :: Case -> NativeObservation -> Check -> Either String ()
check one observation check' = case check' of
  StatusReturned -> case nativeStatus observation of
    Returned -> Right ()
    status -> Left ("status " ++ runnerStatusLabel status)
  ResultIs value -> case lookupRegister R0 observation of
    Just actual | actual == value -> Right ()
    Just actual -> Left ("r0 is " ++ showHexWord actual ++ ", expected " ++ showHexWord value)
    Nothing -> Left "missing register r0"
  WordIs region address value -> do
    actual <- readWord observation region address
    unless (actual == value) (Left (region ++ " " ++ showHexWord address ++ " is " ++ showHexWord actual ++ ", expected " ++ showHexWord value))
  CpsrIs value -> case lookupRegister Cpsr observation of
    Just actual | cpsrControl actual == cpsrControl value -> Right ()
    Just actual -> Left ("cpsr control " ++ showHexWord (cpsrControl actual) ++ ", expected " ++ showHexWord (cpsrControl value))
    Nothing -> Left "missing register cpsr"
  CalleeSavedIs pairs ->
    forM_ pairs $ \(register, value) ->
      case lookupRegister register observation of
        Just actual | actual == value -> Right ()
        Just actual -> Left (registerLabel register ++ " is " ++ showHexWord actual ++ ", expected " ++ showHexWord value)
        Nothing -> Left ("missing register " ++ registerLabel register)
  StackIs value -> case lookupRegister Sp observation of
    Just actual | actual == value -> Right ()
    Just actual -> Left ("sp is " ++ showHexWord actual ++ ", expected " ++ showHexWord value)
    Nothing -> Left "missing register sp"
  LockEffect store -> do
    events <- maybe (Left "ordered provenance missing") Right (nativeKernel observation >>= observedOrder)
    let visible = [event | event@(OrderedWrite write _) <- events, not (privateWrite write)]
        writes = [(fromIntegral address, fromIntegral value) | OrderedWrite (Write (GuestAddress address) _) (WriteValue value) <- visible]
        stores = [index | (index, OrderedWrite write _) <- zip [0 :: Int ..] events, not (privateWrite write), writeAddress write == GuestAddress (fromIntegral (lockAddress one))]
        barriers = [index | (index, OrderedBarrier Dmb) <- zip [0 :: Int ..] events]
        expected = maybe [] (\value -> [(lockAddress one, value)]) store
    unless (writes == expected) (Left ("lock writes " ++ show (map hexPair writes) ++ ", expected " ++ show (map hexPair expected)))
    case store of
      Just _ -> do
        storeIndex <- case stores of
          [index] -> Right index
          _ -> Left ("lock store count " ++ show (length stores))
        barrierIndex <- case barriers of
          [index] -> Right index
          _ -> Left ("DMB count " ++ show (length barriers))
        unless (barrierIndex > storeIndex) (Left "the DMB does not follow the store")
      Nothing -> unless (null barriers) (Left "unexpected DMB")

privateWrite :: Write -> Bool
privateWrite (Write (GuestAddress address) (WriteBytes size)) =
  toInteger stackBase <= toInteger address
    && toInteger address + toInteger size <= toInteger stackBase + toInteger stackSize

readWord :: NativeObservation -> String -> Word32 -> Either String Word32
readWord observation region address = do
  bytes <- maybe (Left ("missing region " ++ region)) Right (lookup (RegionId region) (nativeMemory observation))
  let base = regionBase region
  if address < base || address + 4 > base + fromIntegral (B.length bytes)
    then Left ("address " ++ showHexWord address ++ " is outside " ++ region)
    else Right (readWordAt bytes (fromIntegral (address - base)))

regionBase :: String -> Word32
regionBase "global" = globalBase
regionBase "explicit" = explicitBase
regionBase _ = 0

regionDifferences :: NativeObservation -> NativeObservation -> [String]
regionDifferences stock owned =
  [ "region " ++ regionName ident ++ " differs"
  | ident <- [RegionId "global", RegionId "explicit"]
  , M.lookup ident (M.fromList (nativeMemory stock)) /= M.lookup ident (M.fromList (nativeMemory owned))
  ]

caseFor :: Domain -> Role -> Image -> Image -> [(Word32, Word32)] -> [Aggregate] -> Case -> CaseSpec
caseFor domain role stockImage ownedImage symbols aggregates one =
  CaseSpec
    { caseId = caseName one ++ "-" ++ domainLabel domain
    , caseProvenance = "X-M1 1.50 global-lock root extensions +0x161c/+0x1620/+0x1624, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; synthetic lock words and caller stack"
    , caseExclusion = "One call of one extension entry. The runner's single-core local monitor cannot lose a reservation between the exclusive pair, so the retry path is host-model only; cross-core exclusive-monitor, alias and cache laws are not established."
    , caseEntry = GuestAddress (fromIntegral entry)
    , caseExit = GuestAddress (fromIntegral exitAddress)
    , caseRegions =
        codeRegions
          ++ [ mkRegion (RegionId "global") (GuestAddress (fromIntegral globalBase)) (fromIntegral globalSize) ReadWrite (overlay globalBase globalSize [(globalWord, caseDefault one)])
             , mkRegion (RegionId "explicit") (GuestAddress (fromIntegral explicitBase)) (fromIntegral explicitSize) ReadWrite (overlay explicitBase explicitSize [(explicitWord, caseExplicit one)])
             , mkRegion (RegionId "stack") (GuestAddress (fromIntegral stackBase)) (fromIntegral stackSize) ReadWrite (B.replicate (fromIntegral stackSize) 0)
             , mkRegion (RegionId "exit") (GuestAddress (fromIntegral exitAddress)) 0x10 ReadExecute (B.replicate 0x10 0)
             ]
    , caseFunctions = [(GuestAddress (fromIntegral address), size) | (address, size) <- functions]
    , caseInitial = initialRegisters
    , caseExpectedStatus = Returned
    , caseExpectedRegisters =
        [(R0, caseResult one)]
          ++ [(register, seedRegister register) | register <- [R4 .. R11]]
          ++ [(Sp, stackPointer)]
    , caseExpectedMemory =
        [ (RegionId "global", overlay globalBase globalSize [(globalWord, caseFinalDefault one)])
        , (RegionId "explicit", overlay explicitBase explicitSize [(explicitWord, caseFinalExplicit one)])
        ]
          ++ [ (RegionId "stack", overlay stackBase stackSize [(stackPointer - 8, seedRegister R11), (stackPointer - 4, exitAddress)])
             | not direct
             ]
    , caseCp14Identity = Just (if domain == DomainA then 0x4003 else 0x4004)
    , caseKernel = Just (KernelCase plainLimit Nothing (Just 0x8000_1234) (Just 0) (Just 0) [R1, R2, R3, R12, Lr] [])
    }
  where
    entry
      | direct = helperEntry
      | otherwise = case caseService one of
          Try -> tryEntry domain
          Release -> releaseEntry domain
          Observe -> observeEntry domain
    helperEntry = case caseService one of
      Try -> atomicsBase domain
      Release -> atomicsBase domain + 0x28
      Observe -> atomicsBase domain + 0x54
    direct = isDirect one
    codeRegions = case role of
      Stock ->
        [ mkRegion (RegionId "slots") (GuestAddress (fromIntegral (entryBase domain))) (fromIntegral (entrySize domain)) ReadExecute (slice stockImage (entryBase domain) (entrySize domain))
        , mkRegion (RegionId "atomics") (GuestAddress (fromIntegral (atomicsBase domain))) (fromIntegral (atomicsSize domain)) ReadExecute (slice stockImage (atomicsBase domain) (atomicsSize domain))
        ]
      Owned ->
        if not (null aggregates) then concatMap aggregateRegions aggregates else
        [ mkRegion (RegionId "slots") (GuestAddress (fromIntegral (entryBase domain))) (fromIntegral (entrySize domain)) ReadExecute (slice ownedImage (entryBase domain) (entrySize domain))
        , mkRegion (RegionId "atomics") (GuestAddress (fromIntegral (atomicsBase domain))) (fromIntegral (atomicsSize domain)) ReadExecute (slice ownedImage (atomicsBase domain) (atomicsSize domain))
        ]
          ++ [ mkRegion (RegionId ("body-" ++ show n)) (GuestAddress (fromIntegral address)) (fromIntegral size) ReadExecute (slice ownedImage address size)
             | ((address, size), n) <- zip bodySymbols [0 :: Int ..]
             ]
    spans = [(entryBase domain, entrySize domain), (atomicsBase domain, atomicsSize domain)]
    bodySymbols = [symbol | symbol@(address, _) <- symbols, not (any (\(base, length') -> address >= base && address < base + length') spans)]
    functions = case role of
      Stock ->
        [ (entryBase domain, 0x1c), (entryBase domain + 0x1c, 0x1c), (entryBase domain + 0x38, 0x1c)
        , (atomicsBase domain, 0x28), (atomicsBase domain + 0x28, 0x2c), (atomicsBase domain + 0x54, 0x8)
        ]
      Owned | not (null aggregates) -> aggregateFunctions aggregates
      Owned -> symbols
    initialRegisters =
      [(R0, caseArgument one), (R1, caseStamp one)]
        ++ [(register, seedRegister register) | register <- [R2 .. R12]]
        ++ [(Sp, stackPointer), (Lr, exitAddress), (Cpsr, caseCpsr one)]

slice :: Image -> Word32 -> Word32 -> B.ByteString
slice image address size
  | address < imageBase image || toInteger address + toInteger size > toInteger (imageBase image) + toInteger (B.length (imageBytes image)) =
      error "global-lock code slice is outside its image"
  | otherwise = B.take (fromIntegral size) (B.drop (fromIntegral (address - imageBase image)) (imageBytes image))

overlay :: Word32 -> Word32 -> [(Word32, Word32)] -> B.ByteString
overlay base size words' = B.concat (pieces 0 ordered)
  where
    sizeI = fromIntegral size :: Int
    ordered =
      M.toAscList
        ( M.fromList
            [ (fromIntegral (address - base) :: Int, value)
            | (address, value) <- words'
            , address >= base
            , address + 4 <= base + size
            ]
        )
    pieces cursor [] = [B.replicate (sizeI - cursor) 0]
    pieces cursor ((offset, value) : rest) =
      B.replicate (offset - cursor) 0 : word32le value : pieces (offset + 4) rest

showHexWord :: Word32 -> String
showHexWord value = "0x" ++ showHex' value
  where
    showHex' 0 = "0"
    showHex' n = reverse (go n)
    go 0 = []
    go n = let (q, r) = n `divMod` 16 in "0123456789abcdef" !! fromIntegral r : go q

hexPair :: (Word32, Word32) -> String
hexPair (address, value) = showHexWord address ++ "=" ++ showHexWord value

parseOwnedSymbols :: Domain -> FilePath -> IO [(Word32, Word32)]
parseOwnedSymbols _ path = do
  rows <- lines <$> readFile path
  pure (concatMap row rows)
  where
    row line = case words line of
      [a, z, kind, _] -> case (parseWord32 a, parseWord32 z) of
        (Just address, Just size)
          | kind `elem` ["T", "t"], size > 0 -> [(address, size)]
        _ -> []
      _ -> []

checkStockBytes :: Domain -> Image -> Either String ()
checkStockBytes domain loader = do
  unless (slice loader (entryBase domain) (entrySize domain) == assemble stockSlotWords) (Left "stock slot bytes differ")
  unless (slice loader (atomicsBase domain) (atomicsSize domain) == assemble stockAtomicWords) (Left "stock atomic bytes differ")

cohortOf :: Case -> String
cohortOf one
  | isDirect one = "conventions"
  | caseName one `elem` ["try-free-default-critical", "release-held-default-critical"] = "conventions"
  | otherwise = "root"

runDomain :: FilePath -> FilePath -> FilePath -> Domain -> Image -> Image -> [(Word32, Word32)] -> [Aggregate] -> IO ([CaseRun], [ControlRun])
runDomain runner cacheRoot directory domain stockImage ownedImage symbols aggregates = do
  either error pure (checkStockBytes domain stockImage)
  let origins =
        [ entryBase domain, entryBase domain + 0x1c, entryBase domain + 0x38
        , atomicsBase domain, atomicsBase domain + 0x28, atomicsBase domain + 0x54
        ]
  unless (all (`elem` map fst symbols) origins) (error "owned symbols lack the six public entries")
  (rootRuns, rootControls) <- runCohort [one | one <- cases, cohortOf one == "root"] directory
  (conventionRuns, conventionControls) <- runCohort [one | one <- cases, cohortOf one == "conventions"] (directory </> "conventions")
  protectionControls <- runProtection
  pure (rootRuns ++ conventionRuns, rootControls ++ conventionControls ++ protectionControls)
  where
    runProtection = do
      let readonly = [(address, bytes) | aggregate <- aggregates,
            (_, address, ReadOnly, bytes) <- aggregateStorage aggregate]
          driver = 0x0100_0400
          driverBytes = assemble [0xe5c01000, 0xe12fff1e]
          probes = [(address, protection) | (address, _) <- readonly, protection <- [WriteProtection, FetchProtection]]
          name address protection = "readonly-" ++ show protection ++ "-" ++ show address
          spec address protection =
            let template = case cases of
                  one : _ -> caseFor domain Owned stockImage ownedImage symbols aggregates one
                  [] -> error "global-lock cases are empty"
                entry = if protection == WriteProtection then driver else address
             in template
               { caseId = name address protection
               , caseEntry = GuestAddress (fromIntegral entry)
               , caseRegions = caseRegions template ++
                   [mkRegion (RegionId "readonly-driver") (GuestAddress (fromIntegral driver)) (B.length driverBytes) ReadExecute driverBytes]
               , caseFunctions = [(GuestAddress (fromIntegral entry), if protection == WriteProtection then 8 else 4)]
               , caseInitial = [(R0, address), (R1, 0xa5), (Sp, stackPointer), (Lr, exitAddress), (Cpsr, 0x13)]
               , caseExpectedStatus = if protection == WriteProtection then MemoryGuard else FetchGuard
               , caseExpectedRegisters = [(R0, address), (Pc, if protection == WriteProtection then driver else address)]
               , caseExpectedMemory = []
               }
      unless (all (\aggregate -> let image = aggregateImage aggregate in
        driver + fromIntegral (B.length driverBytes) <= imageBase image
        || imageBase image + fromIntegral (B.length (imageBytes image)) <= driver) aggregates)
        (error "readonly probe driver overlaps supplied code storage")
      if null probes then pure [] else do
        batch <- writeBatch (directory </> "readonly") [spec address protection | (address, protection) <- probes]
        (_, raw) <- cachedObservedBatch cacheRoot runner batch
        let observed = M.fromList [(caseIdText ident, observation) | (ident, observation) <- raw]
        pure [ControlRun (show protection) (name address protection)
          (case M.lookup (name address protection) observed of
            Just observation -> nativeStatus observation == (if protection == WriteProtection then MemoryGuard else FetchGuard)
              && aggregateUnchanged aggregates observation
            Nothing -> False)
          | (address, protection) <- probes]
    runCohort chosen base = do
      let stockSpecs = map (caseFor domain Stock stockImage ownedImage symbols aggregates) chosen
          ownedSpecs = map (caseFor domain Owned stockImage ownedImage symbols aggregates) chosen
      stockBatch <- writeBatch (base </> "stock") stockSpecs
      ownedBatch <- writeBatch (base </> "owned") ownedSpecs
      (stockReuse, stockRaw) <- cachedObservedBatch cacheRoot runner stockBatch
      (ownedReuse, ownedRaw) <- cachedObservedBatch cacheRoot runner ownedBatch
      let reference = M.fromList [(caseIdText ident, observation) | (ident, observation) <- stockRaw]
          subject = M.fromList [(caseIdText ident, observation) | (ident, observation) <- ownedRaw]
      results <- forM chosen $ \one -> do
        let ident = caseName one ++ "-" ++ domainLabel domain
            stock = M.lookup ident reference
            owned = M.lookup ident subject
            roleChecks observation = map (check one observation) (checksFor one)
            errorsFor observation = [message | Left message <- roleChecks observation]
        let run = CaseRun
              { caseDomain = domain
              , runName = caseName one
              , runChecks = case (stock, owned) of
                  (Just stock', Just owned') ->
                    [("stock: " ++ describe check', either (const False) (const True) (check one stock' check')) | check' <- checksFor one]
                      ++ [("owned: " ++ describe check', either (const False) (const True) (check one owned' check')) | check' <- checksFor one]
                      ++ [("owned: complete aggregate source bytes unchanged", aggregateUnchanged aggregates owned') | not (null aggregates)]
                  _ -> []
              , runDifferences = case (stock, owned) of
                  (Just stock', Just owned') ->
                    ["stock: " ++ message | message <- errorsFor stock']
                      ++ ["owned: " ++ message | message <- errorsFor owned']
                      ++ regionDifferences stock' owned'
                  (Nothing, _) -> ["stock case did not run"]
                  (_, Nothing) -> ["owned case did not run"]
              , runReuse = (stockReuse, ownedReuse)
              }
            controls = maybe [] (\observation -> controlRuns one observation ++
              [ControlRun "aggregate-source-byte" (caseName one)
                (not (aggregateUnchanged aggregates (observation {nativeMemory = map corrupt (nativeMemory observation)})))
              | not (null aggregates)]) owned
            corrupt (regionIdent, bytes)
              | "aggregate-" `isPrefixOf` regionName regionIdent, Just (byte, rest) <- B.uncons bytes =
                  (regionIdent, B.cons (byte `xor` 1) rest)
              | otherwise = (regionIdent, bytes)
        pure (run, controls)
      unless (length stockRaw == length chosen && length ownedRaw == length chosen) (error "native observations do not cover the current case set")
      pure (map fst results, concatMap snd results)

controlRuns :: Case -> NativeObservation -> [ControlRun]
controlRuns one observation =
  [ ControlRun "result" (caseName one) (refusedWith (mutateRegister R0 (`xor` 1) observation))
  , ControlRun "cpsr-i" (caseName one) (refusedWith (mutateRegister Cpsr (`xor` 0x80) observation))
  , ControlRun "global-word-byte" (caseName one) (refusedWith (mutateGlobalWord observation))
  ]
    ++ case storeValue one of
      Just _ ->
        [ ControlRun "missing-barrier" (caseName one) (refusedWith (mutateOrder dropDmb observation))
        , ControlRun "wrong-store" (caseName one) (refusedWith (mutateOrder bumpStore observation))
        ]
      Nothing -> [ControlRun "extra-barrier" (caseName one) (refusedWith (mutateOrder (++ [OrderedBarrier Dmb]) observation))]
  where
    refusedWith mutated = any isRefusal [check one mutated check' | check' <- checksFor one]
    isRefusal (Left _) = True
    isRefusal (Right ()) = False

mutateRegister :: Register -> (Word32 -> Word32) -> NativeObservation -> NativeObservation
mutateRegister register f observation =
  observation {nativeRegisters = M.adjust f register (nativeRegisters observation)}

mutateGlobalWord :: NativeObservation -> NativeObservation
mutateGlobalWord observation =
  observation
    { nativeMemory =
        [ (ident, if regionName ident == "global" then flipWord bytes else bytes)
        | (ident, bytes) <- nativeMemory observation
        ]
    }
  where
    offset = fromIntegral (globalWord - globalBase)
    flipWord bytes =
      let (before, rest) = B.splitAt offset bytes
       in case B.uncons rest of
            Just (first, after) -> B.concat [before, B.singleton (first `xor` (0xff :: Word8)), after]
            Nothing -> bytes

mutateOrder :: ([Ordered] -> [Ordered]) -> NativeObservation -> NativeObservation
mutateOrder f observation =
  observation
    { nativeKernel = fmap (\kernel -> kernel {observedOrder = fmap f (observedOrder kernel)}) (nativeKernel observation)
    }

dropDmb :: [Ordered] -> [Ordered]
dropDmb events = case break isDmb events of
  (before, _ : after) -> before ++ after
  _ -> events
  where
    isDmb (OrderedBarrier Dmb) = True
    isDmb _ = False

bumpStore :: [Ordered] -> [Ordered]
bumpStore events = case break isLockStore events of
  (before, OrderedWrite write (WriteValue value) : after) -> before ++ [OrderedWrite write (WriteValue (value + 1))] ++ after
  _ -> events
  where
    isLockStore (OrderedWrite (Write (GuestAddress address) _) _) = address == fromIntegral globalWord || address == fromIntegral explicitWord
    isLockStore _ = False
