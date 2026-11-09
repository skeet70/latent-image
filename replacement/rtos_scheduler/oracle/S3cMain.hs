{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM, forM_, unless)
import Data.Aeson (object, (.=))
import Data.Char (toLower)
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import qualified Data.ByteString as B
import Data.List (isPrefixOf, nub, sortOn)
import Data.Word (Word32)
import Emulation.NativeObservation
import Emulation.OwnedCase hiding (required)
import Emulation.Sha256 (sha256, sha256Hex)
import qualified Replacement.RtosPlatform.Aggregate as Agg
import qualified Replacement.RtosPlatform.Union as Union
import Replacement.RtosOwnFirst.Artifact (loaderSha256)
import qualified Replacement.RtosOwnFirst.Regions as R
import FujiAF.JSON (encodeCanonical)
import System.Directory (canonicalizePath, createDirectoryIfMissing, removeDirectoryRecursive)
import System.Environment (getArgs, lookupEnv)
import System.FilePath ((</>))
import System.Directory (setCurrentDirectory)

data Entry = GetPriority | Sleep | Suspend | Resume | ForceResume | Raise | InterruptRaise | Disable | Enable | DeliveryLoop | Delivery deriving (Eq, Show, Enum, Bounded)
data Domain = A | B deriving (Eq, Show)
data Role = Stock | Owned | Equivalent | Mutant | StockRepeat | CallerWrite | ReadonlyWrite | ReadonlyFetch deriving (Eq, Show)
newtype TaskPriority = TaskPriority {priorityWord :: Word32} deriving (Eq, Show)
data Row = Row { state :: Word32, flags :: Word32, pending :: Word32, discarded :: Word32, dispatch :: Word32, handler :: Word32, masked :: Bool, classId :: Word32, number :: Word32, self :: Bool, permission :: Bool, callback :: Bool, patternWord :: Word32, held :: Bool, retryReady :: Bool, callbackMode :: Word32, priority :: TaskPriority } deriving (Eq, Show)

baseRow :: Entry -> Row
baseRow entry = Row 1 0 0 0 1 (if entry == InterruptRaise then 1 else 0) False 0 1 False True True 0x20 False False 0 (TaskPriority 5)

rows :: Entry -> [Row]
rows entry = filter valid $ base :
    [base {state = value} | value <- [0,2,4,6,10,14,22,54]] ++
    [base {flags = value} | value <- [1,2,4,7,128]] ++
    [base {pending = value} | value <- [1,0x80000000]] ++
    [base {discarded = 1},base {dispatch = 0},base {handler = 1 - handler base},base {masked = True},base {permission = False},base {callback = False},base {patternWord = 0},base {held = True}] ++
    [base {classId = c, number = n} | c <- [0,1,2,3,0xffff], n <- [0,1,2,121,0xffff]] ++
    [base {classId = c,state = s,dispatch = enabled} | c <- [1,2],s <- [4,6],enabled <- [0,1,2,0x80000000]] ++
    [base {dispatch = enabled} | enabled <- [2,0x80000000]] ++
    [base {priority = TaskPriority p} | p <- [0,15]] ++
    [base {self = True, state = s, flags = f, discarded = d, pending = p, callbackMode = mode} | s <- [1,4,6], f <- [0,4,7], d <- [0,1], (p,mode) <- [(0,0),(0x42,0),(0x42,1),(0x42,2)]] ++
    [base {self = True, dispatch = 0},base {self = True, state = 2, retryReady = True},base {self = True, state = 2}]
  where
    base = baseRow entry
    valid row = (entry /= DeliveryLoop || callback row) && (not rechecks || state row .&. 1 /= 0 || retryReady row)
      where rechecks = entry `elem` [Sleep,Enable] || (entry == Suspend && (self row || number row == 0))

entryAt :: Domain -> Entry -> Word32
entryAt domain entry = address + if domain == A then 0 else 0x8ffc0
  where address = case entry of
          GetPriority -> 0x45b40; Sleep -> 0x45cd4; Suspend -> 0x46a00; Resume -> 0x46c80; ForceResume -> 0x46e38
          Raise -> 0x46fcc; InterruptRaise -> 0x471b0; Disable -> 0x47354; Enable -> 0x47428; DeliveryLoop -> 0x441b4; Delivery -> 0x44288

extent :: Entry -> Int
extent entry = case entry of
  GetPriority -> 404; Sleep -> 384; Suspend -> 640; Resume -> 440; ForceResume -> 16; Raise -> 484; InterruptRaise -> 420; Disable -> 212; Enable -> 280; DeliveryLoop -> 212; Delivery -> 144

loaderBase, contexts, runningCell, dispatchCell, readyHeads, bitmapCell, taskBase, currentCell, lockCell :: Domain -> Word32
loaderBase A = 0x40000
loaderBase B = 0xd0000
contexts A = 0x400545cc
contexts B = 0x400e458c
runningCell A = 0x40054760
runningCell B = 0x400e4720
dispatchCell d = runningCell d + 4
readyHeads A = 0x400546dc
readyHeads B = 0x400e469c
bitmapCell d = runningCell d - 4
taskBase A = 0x4005ff0c
taskBase B = 0x400efecc
currentCell d = contexts d - 4
lockCell d = currentCell d - 8

stockEntry :: Domain -> Word32 -> Word32
stockEntry domain address = address + if domain == A then 0 else if address >= 0x43c48 then 0x8ffbc else if address >= 0x439d8 then 0x8ffb4 else 0x90000

poke :: B.ByteString -> Word32 -> B.ByteString -> B.ByteString
poke bytes offset value = B.take index bytes <> value <> B.drop (index + B.length value) bytes where index = fromIntegral offset
wordsBytes :: [Word32] -> B.ByteString
wordsBytes = B.concat . map word32le

ram :: Domain -> Domain -> Row -> Word32 -> Int -> B.ByteString
ram executing described row base size = foldl apply (B.replicate size 0) (globals ++ records ++ tasks ++ queues)
  where
    apply bytes (address,value)
      | address >= base && address + 4 <= base + fromIntegral size = poke bytes (address - base) (word32le value)
      | otherwise = bytes
    currentRecord = contexts executing + if executing == A then 0 else 132
    targetDomain = if classId row == 1 then A else if classId row == 2 then B else executing
    targetSlot = if number row == 0 && executing /= targetDomain then 0 else if number row == 2 then 2 else if number row == 0 && handler row /= 0 then 0 else 1
    target = taskBase targetDomain + 56 * (targetSlot - 1)
    runTask = if self row then target else taskBase executing + 56 * 3
    globals = if executing == described then [(currentCell executing,currentRecord),(runningCell executing,runTask),(dispatchCell executing,dispatch row),(currentCell executing - 36,handler row),(lockCell executing,if held row then 1 else 0)] else [(dispatchCell described,dispatch row),(lockCell described,0)]
    records = if described /= executing then [] else concat
      [[(record,if d == A then 1 else 2),(record+8,lockCell d),(record+12,if retryReady row then 0x900040 else stockEntry executing 0x439d8),(record+16,stockEntry executing 0x43a14),(record+32,readyHeads d),(record+36,bitmapCell d),(record+40,runningCell d),(record+44,runningCell d+8),(record+48,dispatchCell d),(record+68,taskBase d),(record+72,120)] | (d,record) <- [(A,contexts executing),(B,contexts executing+132)]]
    idle = taskBase described + 56*2
    taskState t = if t == target || t == runTask then state row else if t == idle then 1 else 0
    taskPriority t = if t == idle then 10 else priorityWord (priority row)
    tasks = concat [[(t,t),(t+4,t),(t+8,0x910000+slot*64),(t+12,taskState t .|. (taskPriority t `shiftL` 8) .|. (flags row `shiftL` 16)),(t+16,pending row),(t+48,discarded row)] | slot <- [0..4], let t=taskBase described + 56*(slot-1)]
    members :: Word32 -> [Word32]
    members p = [t | t <- nub [target,runTask,idle], t >= taskBase described-56, t < taskBase described+56*4, taskState t .&. 1 /= 0, taskPriority t == p]
    queues = [(bitmapCell described,bit),(runningCell described+8,selected)] ++ concat [links (readyHeads described+p*8) (members p) | p <- [0..15]]
    links headAt items = [(node,next) | (node,next) <- zip nodes (drop 1 nodes ++ [headAt])] ++ [(node+4,prev) | (prev,node) <- zip (last nodes:nodes) nodes]
      where nodes = headAt:items
    selected = case concat [members p | p <- [0..15]] of t:_ -> t; [] -> 0
    bit = sum [1 `shiftL` fromIntegral p | p <- [0..15], not(null(members p))]

callbackCode :: Domain -> Row -> B.ByteString
callbackCode domain row = wordsBytes (capture ++ mode ++ [0xe12fff1e])
  where
    capture = [0xe3002100,0xe3402090,0xe5923000,0xe2833001,0xe5823000,0xe5820004,0xe5821008]
    mode = case callbackMode row of
      1 -> [0xf10c0080]
      2 -> [0xe3530001,0x1a000004,0xe3002000 .|. (runningCell domain .&. 0xfff) .|. ((runningCell domain .&. 0xf000) `shiftL` 4),0xe3442000 .|. ((runningCell domain `shiftR` 16) .&. 0xfff),0xe5922000,0xe3a03080,0xe5823010]
      _ -> []

spec :: (Word32, Word32) -> [(Word32, Word32)] -> [(Word32, Word32)] -> Domain -> Entry -> Row -> B.ByteString -> String -> CaseSpec
spec (codeLo, codeHi) functionSpans readonlySpans domain entry row code ident = CaseSpec ident "X-M1 1.50 S3c contracts at 55436fc85c, synthetic typed scheduler state" "one native service call, no concurrent CPU or devices beyond IRQ pending words; blocking calls stop at the real dispatcher entry" (ga (entryAt domain entry)) (ga 0x920000) (regions ++ [exitRegion]) ([(ga at,size) | (at,size) <- functionSpans] ++ [(ga 0x900000,64),(ga 0x900040,64),(ga 0x930000,16)]) initial Returned [(R4,0xabc00004)] [] Nothing (Just kernel)
  where
    ga = GuestAddress . fromIntegral
    codeRegions = [mkRegion (RegionId ("code-loader-" ++ show lo)) (ga lo) (fromIntegral (hi-lo)) permission (B.take (fromIntegral (hi-lo)) (B.drop (fromIntegral (lo-loaderBase domain)) code)) | (lo,hi) <- zip boundaries (drop 1 boundaries), let permission = if any (\(at,size) -> at <= lo && hi <= at+size) readonlySpans then ReadOnly else ReadExecute]
    boundaries = sortOn id (nub (codeLo : codeHi : [at | (lo,size) <- readonlySpans, at <- [lo,lo+size], codeLo < at, at < codeHi]))
    tableAt = if domain == A then 0x4daa0 else 0xdda64
    tableRegions = [mkRegion (RegionId "find-table") (ga tableAt) 16 ReadOnly (B.take 16 (B.drop (fromIntegral(tableAt-loaderBase domain)) code)) | tableAt >= codeHi || tableAt+16 <= codeLo]
    ramRegions = [mkRegion (RegionId (label ++ map toLower(show d))) (ga base) size ReadWrite (ram domain d row base size) | d <- [A,B], (label,base,size) <- [("globals-",currentCell d-36,fromIntegral(runningCell d+48-(currentCell d-36))),("tasks-",taskBase d-56,280)]]
    defs = foldl (\bytes slot -> foldl (\b (off,val) -> poke b (slot*64+off) (word32le val)) bytes [(4,0x12345678),(0x1c,if callback row then 0x900000 else 0),(0x20,slot+1),(0x24,if permission row then 1 else 0)]) (B.replicate 0x200 0) [0..4]
    retry = wordsBytes [0xe300210c,0xe3402090,0xe5923000,0xe2833001,0xe5823000,0xe3530001,0x03a00000,0x012fff1e,0xe3001000 .|. (runningCell domain .&. 0xfff) .|. ((runningCell domain .&. 0xf000) `shiftL` 4),0xe3441000 .|. ((runningCell domain `shiftR` 16) .&. 0xfff),0xe5911000,0xe5d1200c,0xe3822001,0xe5c1200c,0xe3a00000,0xe12fff1e]
    callbackBlob = poke (poke (B.replicate 128 0) 0 (callbackCode domain row)) 64 retry
    regions = codeRegions ++ tableRegions ++ [mkRegion (RegionId "code-callback") (ga 0x900000) 128 ReadExecute callbackBlob,mkRegion (RegionId "code-equivalent") (ga 0x930000) 16 ReadExecute (B.replicate 16 0),mkRegion (RegionId "definitions") (ga 0x910000) 0x200 ReadOnly defs,mkRegion (RegionId "callback-log") (ga 0x900100) 16 ReadWrite (B.replicate 16 0),mkRegion (RegionId "output") (ga 0x900200) 4 ReadWrite (word32le 0xeeeeeeee),mkRegion (RegionId "stack") (ga 0x940000) 1024 ReadWrite (B.replicate 1024 0)] ++ ramRegions ++ [mkRegion (RegionId ("bank-" ++ show address)) (ga address) 4 Device (word32le 0) | address <- [0xfffb30cc,0xfffb40cc]]
    initial = [(R0,(classId row `shiftL` 16) .|. number row),(R1,if entry == GetPriority then 0x900200 else patternWord row),(R2,0),(R3,0),(R12,0)] ++ [(r,0xabc00000+fromIntegral(fromEnum r)) | r <- [R4 .. R11]] ++ [(Sp,0x940200),(Lr,0x920000),(Cpsr,if masked row || entry `elem` [DeliveryLoop,Delivery] then 0xd3 else 0x53)]
    kernel = KernelCase (InstructionLimit 2500) (Just (ga (stockEntry domain 0x43640))) Nothing Nothing Nothing ([R1,R2,R3,R12,Lr,Pc,Cpsr] ++ [R0 | entry `elem` [DeliveryLoop,Delivery]]) []
    exitRegion = mkRegion (RegionId "code-exit") (ga 0x920000) 4 ReadExecute (word32le 0xe7f000f0)

frameHolds :: CaseSpec -> NativeObservation -> Bool
frameHolds reference observation = stackBound && returnedAbi && incomingBytes && incomingEffects
  where
    sp = registerValue observation Sp
    stackBound = 0x940000 <= sp && sp <= 0x940200
    returnedAbi = case caseOutcome reference observation of
      OutcomeReturned -> sp == 0x940200 && all (\r -> registerValue observation r == 0xabc00000 + fromIntegral (fromEnum r)) [R4..R11]
      _ -> True
    incomingBytes = fmap (B.all (== 0) . B.drop 512) (lookup (RegionId "stack") (nativeMemory observation)) == Just True
    incomingEffects = maybe False (all outsideIncoming) (nativeKernel observation >>= observedOrder)
    outsideIncoming (OrderedWrite (Write (GuestAddress address) (WriteBytes size)) _) = address + fromIntegral size <= 0x940200 || address >= 0x940400
    outsideIncoming _ = True

-- The callee-private stack interval is [0x940000,0x940200).
normalize :: NativeObservation -> NativeObservation
normalize observation = observation {nativeMemory = map project (nativeMemory observation),nativeKernel = fmap clean (nativeKernel observation)}
  where
    project (ident,bytes) = (ident,if ident == RegionId "stack" then B.drop 512 bytes else bytes)
    clean kernel = kernel {observedOrder = fmap (filter keep) (observedOrder kernel)}
    keep (OrderedWrite (Write (GuestAddress address) (WriteBytes size)) _) = not (address >= 0x940000 && address + fromIntegral size <= 0x940200)
    keep _ = True

branch :: Word32 -> Word32 -> Word32
branch from to = 0xea000000 .|. (((to-from-8) `shiftR` 2) .&. 0xffffff)

roleSpec :: (Word32, Word32) -> [(Word32, Word32)] -> [(Word32, Word32)] -> Role -> Domain -> Entry -> Row -> B.ByteString -> String -> CaseSpec
roleSpec bounds functionSpans readonlySpans role domain entry row code ident = case role of
  Mutant -> driver (wordsBytes [0xe3e00062,0xe12fff1e])
  Equivalent -> driver (word32le (branch 0x930000 (entryAt domain entry)))
  CallerWrite -> driver (wordsBytes (loadAddress 0x940200 ++ [0xe58c4000,branch 0x93000c (entryAt domain entry)]))
  ReadonlyWrite -> driver (wordsBytes (loadAddress readonlyAt ++ [0xe58c0000,0xe12fff1e]))
  ReadonlyFetch -> base {caseEntry = GuestAddress (fromIntegral readonlyAt), caseFunctions = (GuestAddress (fromIntegral readonlyAt),4) : caseFunctions base}
  _ -> base
  where
    base = spec bounds functionSpans readonlySpans domain entry row code ident
    readonlyAt = case readonlySpans of (at,_):_ -> at; [] -> if domain == A then 0x4daa0 else 0xdda64
    loadAddress at = [0xe300c000 .|. (at .&. 0xfff) .|. ((at .&. 0xf000) `shiftL` 4),0xe340c000 .|. ((at `shiftR` 16) .&. 0xfff) .|. ((at .&. 0xf0000000) `shiftR` 12)]
    driver bytes = base {caseEntry = GuestAddress 0x930000, caseRegions = map replace (caseRegions base)}
      where
        replace region
          | regionIdOf (caseRegion region) == RegionId "code-equivalent" = mkRegion (RegionId "code-equivalent") (GuestAddress 0x930000) 16 ReadExecute (bytes <> B.replicate (16-B.length bytes) 0)
          | otherwise = region
expectedResult :: Entry -> Row -> Maybe Word32
expectedResult entry row
  | entry `elem` [DeliveryLoop,Delivery] = Nothing
  | handler row /= (if entry == InterruptRaise then 1 else 0) || masked row = result (-25)
  | idTaking && (classId row > 2 || number row > 120 || (entry `elem` [Resume,ForceResume] && number row == 0)) = result (-18)
  | entry `elem` [Raise,InterruptRaise] && patternWord row == 0 = result (-17)
  | entry `elem` [GetPriority,Suspend,Resume,ForceResume] && not(permission row) = result (-42)
  | entry == Sleep && dispatch row == 0 = result (-25)
  | entry == Suspend && isSelf && dispatch row == 0 = result (-25)
  | entry == GetPriority && (state row == 0 || (isSelf && discarded row /= 0)) = result (-41)
  | entry == Suspend && state row == 0 = result (-41)
  | entry == Suspend && state row .&. 1 == 0 && state row .&. 4 /= 0 = result (-43)
  | entry `elem` [Resume,ForceResume] && (state row .&. 4 == 0 || (isSelf && discarded row /= 0)) = result (-41)
  | entry `elem` [Raise,InterruptRaise] && (state row == 0 || not(callback row) || (entry == Raise && isSelf && discarded row /= 0)) = result (-41)
  | entry `elem` [Disable,Enable] && (not(callback row) || discarded row /= 0) = result (-41)
  | otherwise = result 0
  where
    result :: Integer -> Maybe Word32
    result = Just . fromInteger
    idTaking = entry `elem` [GetPriority,Suspend,Resume,ForceResume,Raise,InterruptRaise]
    isSelf = self row || number row == 0

contractHolds :: Entry -> Row -> CaseSpec -> NativeObservation -> Bool
contractHolds entry row reference observation = case caseOutcome reference observation of
  OutcomeReturned -> maybe True (== registerValue observation R0) (expectedResult entry row) && outputHolds
  OutcomeStopped _ -> registerValue observation R0 == fromInteger reason
    where reason = case entry of Sleep -> -17; Suspend -> -22; Resume -> -23; ForceResume -> -23; _ -> 0
  OutcomeNoReturnWithin _ _ -> held row
  _ -> False
  where
    outputHolds = entry /= GetPriority || expectedResult entry row /= Just 0 || fmap (\bytes -> readWordAt bytes 0) (lookup (RegionId "output") (nativeMemory observation)) == Just (priorityWord (priority row) + 1)



mergeSpans :: [(Word32, Word32)] -> [(Word32, Word32)]
mergeSpans spans = go (sortOn fst spans)
  where
    go []=[]
    go ((start,size):rest)=extend start (start+size) rest
    extend start end []=[(start,end-start)]
    extend start end ((lo,size):rest)
      | lo<end=extend start (max end (lo+size)) rest
      | otherwise=(start,end-start):extend lo (lo+size) rest

readSymbols :: FilePath -> IO [Agg.AggregateSymbol]
readSymbols path = either fail pure . Agg.parseAggregateSymbols =<< B.readFile path

readPlacements :: FilePath -> IO [Agg.SectionPlacement]
readPlacements path = either fail pure . Agg.parsePlacements =<< B.readFile path

optional :: [String] -> String -> IO (Maybe FilePath)
optional args flag = case [value | (key,value) <- zip args (drop 1 args), key == flag] of
  [] -> if flag `elem` args then fail (flag ++ " needs a value") else pure Nothing
  values -> Just <$> canonicalizePath (last values)

data Aggregate = Aggregate Agg.AggregateImage [Agg.AggregateSymbol] [Agg.SectionPlacement] [(Word32,Word32)] [Word32]

textPlacement :: Agg.SectionPlacement -> Bool
textPlacement placement = Agg.placementSection placement == ".text" || ".text." `isPrefixOf` Agg.placementSection placement

readAggregate :: Union.DomainMap -> FilePath -> FilePath -> FilePath -> IO Aggregate
readAggregate dm bin symbolsPath placementsPath = do
  bytes <- B.readFile bin
  symbols <- readSymbols symbolsPath
  placements <- readPlacements placementsPath
  let carriers = [(lo,hi-lo) | carrier <- Union.mapCarriers dm, let R.Extent (R.GuestAddress lo) (R.GuestAddress hi) = Union.carrierExtent carrier]
      entries = [at | entry <- Union.mapEntries dm, let R.GuestAddress at = Union.entryAddress entry]
      inside (at,size) = size > 0 && any (\(lo,count) -> lo <= at && toInteger at + toInteger size <= toInteger lo + toInteger count) carriers
      spans = [(Agg.symbolAddress s,Agg.symbolSize s) | s <- symbols] ++ [(Agg.placementAddress p,Agg.placementBytes p) | p <- placements, Agg.placementBytes p > 0]
  unless (not (null carriers) && not (null symbols) && not (null placements)) (fail "aggregate metadata is empty")
  let base = minimum (map fst carriers)
      top = maximum [lo+size | (lo,size) <- carriers]
      image = Agg.AggregateImage base bytes
  unless (toInteger (B.length bytes) == toInteger top - toInteger base) (fail "aggregate effective payload does not span the complete carrier union")
  forM_ spans $ \span'@(at,size) -> do
    unless (inside span') (fail ("aggregate allocation outside a live carrier: " ++ show span'))
    _ <- either fail pure (Agg.sliceAggregate image at size)
    pure ()
  forM_ placements $ \p -> unless (all (\at -> at+4 <= Agg.placementAddress p || Agg.placementAddress p + Agg.placementBytes p <= at) entries) (fail "aggregate placement overlaps a fixed entry")
  let allocations = sortOn Agg.placementAddress (filter ((> 0) . Agg.placementBytes) placements)
  forM_ (zip allocations (drop 1 allocations)) $ \(left,right) ->
    unless (toInteger (Agg.placementAddress left) + toInteger (Agg.placementBytes left) <= toInteger (Agg.placementAddress right)) (fail "aggregate placements overlap")
  forM_ symbols $ \symbol ->
    unless (any (\entry -> Agg.symbolAddress symbol == entry && Agg.symbolSize symbol == 4) entries ||
      any (\p -> textPlacement p && Agg.placementAddress p <= Agg.symbolAddress symbol &&
        toInteger (Agg.symbolAddress symbol) + toInteger (Agg.symbolSize symbol) <= toInteger (Agg.placementAddress p) + toInteger (Agg.placementBytes p)) allocations)
      (fail ("aggregate function has no executable allocation: " ++ Agg.symbolName symbol))
  unless (all (\service -> entryAt (if Union.mapDomain dm == R.DomainA then A else B) service `elem` entries) [minBound..maxBound]) (fail "aggregate is missing an S3c fixed entry")
  forM_ (Union.mapEntries dm) $ \entry -> do
    let R.GuestAddress at = Union.entryAddress entry
    unless (inside (at,4) && any (\s -> Agg.symbolName s == Union.entryVeneer entry && Agg.symbolAddress s == at && Agg.symbolSize s == 4) symbols) (fail ("aggregate fixed veneer differs: " ++ Union.entryName entry))
    unless (any (\s -> Agg.symbolName s == Union.entrySymbol entry) symbols) (fail ("aggregate body is absent: " ++ Union.entryName entry))
    forM_ (filter (\s -> Agg.symbolName s == Union.entrySymbol entry) symbols) $ \body ->
      unless (readWordAt bytes (fromIntegral (at-base)) == branch at (Agg.symbolAddress body)) (fail ("aggregate veneer does not branch to its body: " ++ Union.entryName entry))
  pure (Aggregate image symbols placements carriers entries)

installAggregate :: Domain -> B.ByteString -> Aggregate -> B.ByteString
installAggregate domain loader (Aggregate image _ _ carriers _) = foldl install padded carriers
  where
    top = maximum (loaderBase domain + fromIntegral (B.length loader) : [lo+size | (lo,size) <- carriers])
    padded = loader <> B.replicate (fromIntegral (top-loaderBase domain)-B.length loader) 0
    install bytes (lo,size) = poke bytes (lo-loaderBase domain) (either error id (Agg.sliceAggregate image lo size))

splitEntries :: [Word32] -> [(Word32,Word32)] -> [(Word32,Word32)]
splitEntries entries spans = [(lo,hi-lo) | (at,size) <- mergeSpans spans, let points = at : sortOn id (nub [p | entry <- entries, p <- [entry,entry+4], at < p, p < at+size]) ++ [at+size], (lo,hi) <- zip points (drop 1 points)]

main :: IO ()
main = do
  args <- getArgs
  runner <- required args "--runner" >>= canonicalizePath
  aggregatePaths <- mapM (optional args) ["--aggregate-a-bin","--aggregate-a-symbols","--aggregate-a-placements","--aggregate-b-bin","--aggregate-b-symbols","--aggregate-b-placements","--aggregate-carriers"]
  legacyPaths <- mapM (optional args) ["--symbols-a","--placements-a","--symbols-b","--placements-b"]
  aggregates <- case (aggregatePaths,legacyPaths) of
    ([Nothing,Nothing,Nothing,Nothing,Nothing,Nothing,Nothing],[Nothing,Nothing,Nothing,Nothing]) -> pure Nothing
    ([Just a,Just sa,Just pa,Just b,Just sb,Just pb,Just carriers],[Nothing,Nothing,Nothing,Nothing]) -> loadAggregates carriers a sa pa b sb pb
    ([Nothing,Nothing,Nothing,Nothing,Nothing,Nothing,Just carriers],[Just sa,Just pa,Just sb,Just pb]) -> do
      a <- required args "--owned-a" >>= canonicalizePath
      b <- required args "--owned-b" >>= canonicalizePath
      loadAggregates carriers a sa pa b sb pb
    _ -> fail "supply all seven --aggregate-* inputs, or both legacy owned/symbol/placement triples with --aggregate-carriers"
  standalonePaths <- case aggregates of
    Nothing -> do
      a <- required args "--owned-a" >>= canonicalizePath
      b <- required args "--owned-b" >>= canonicalizePath
      pure (Just (a,b))
    Just _ -> pure Nothing
  aLoader <- required args "--loader-a" >>= canonicalizePath
  bLoader <- required args "--loader-b" >>= canonicalizePath
  work <- required args "--work" >>= canonicalizePath
  lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= mapM_ setCurrentDirectory
  createDirectoryIfMissing True work
  runnerBytes <- B.readFile runner
  results <- forM [(A,aLoader),(B,bLoader)] $ \(domain,loaderPath) -> do
    loader <- B.readFile loaderPath
    let aggregateForDomain = fmap (\(a,b) -> if domain == A then a else b) aggregates
    owned <- case (aggregateForDomain,standalonePaths) of
      (Just (Aggregate image _ _ _ _),_) -> pure (Agg.imageBytes image)
      (_,Just (a,b)) -> B.readFile (if domain == A then a else b)
      _ -> fail "missing owned image"
    let digest = sha256Hex . sha256
        aggregate = case aggregateForDomain of Just _ -> True; Nothing -> False
    unless (digest loader == loaderSha256 (if domain == A then R.DomainA else R.DomainB)) (fail "loader is not the pinned X-M1 1.50 image")
    unless (aggregate || B.length owned == fromIntegral(entryAt domain Enable - entryAt domain DeliveryLoop) + extent Enable) (fail "owned binary extent differs")
    B.writeFile (work </> ("inputs-" ++ show domain ++ ".json")) (encodeCanonical(object ["loader_sha256" .= digest loader,"owned_sha256" .= digest owned,"runner_sha256" .= digest runnerBytes]))
    let ownedBase = entryAt domain DeliveryLoop
        selected = case aggregateForDomain of Just (Aggregate _ symbols _ _ _) -> symbols; Nothing -> []
        placements = case aggregateForDomain of Just (Aggregate _ _ plans _ _) -> plans; Nothing -> []
        carriers = case aggregateForDomain of Just (Aggregate _ _ _ spans _) -> spans; Nothing -> []
        fixedEntries = case aggregateForDomain of Just (Aggregate _ _ _ _ entries) -> entries; Nothing -> []
        standaloneLoader = foldl (\bytes entry -> let address=entryAt domain entry in poke bytes (address-loaderBase domain) (B.take (extent entry) (B.drop (fromIntegral(address-ownedBase)) owned))) loader [minBound..maxBound]
        ownedLoader = maybe standaloneLoader (installAggregate domain loader) aggregateForDomain
        symbolSpans = [(Agg.symbolAddress symbol, Agg.symbolSize symbol) | symbol <- selected]
        stockFunctions = [(stockEntry domain at,size) | (at,size) <- [(0x439d8,60),(0x43a14,40),(0x43c48,92),(0x43ca4,124),(0x43d20,176),(0x44018,if domain == A then 44 else 48),(0x433e4,96),(0x43640,4)]] ++ [(entryAt domain e,fromIntegral(extent e)) | e <- [minBound .. maxBound]]
        placementFunctions = [(Agg.placementAddress p,Agg.placementBytes p) | p <- placements, Agg.placementBytes p > 0, textPlacement p]
        retained (at,size) = all (\(lo,count) -> at+size <= lo || lo+count <= at) carriers
        ownedFunctions = if aggregate then splitEntries fixedEntries (symbolSpans ++ filter retained stockFunctions ++ placementFunctions) else stockFunctions
        readonlySpans = mergeSpans ([(Agg.placementAddress p,Agg.placementBytes p) | p <- placements, Agg.placementBytes p > 0, not (textPlacement p)] ++ [(if domain == A then 0x4daa0 else 0xdda64,16)])
        stockBounds = (stockEntry domain 0x433e4, entryAt domain Enable + fromIntegral(extent Enable))
        ownedBounds = if aggregate then (min (fst stockBounds) (minimum (map fst carriers)),max (snd stockBounds) (maximum [at+size | (at,size) <- carriers])) else stockBounds
    forM [minBound..maxBound] $ \entry -> do
      let grid = rows entry
          directory = work </> show domain ++ "-" ++ show entry
      createDirectoryIfMissing True directory
      observed <- forM [Stock,Owned,Equivalent,Mutant,StockRepeat,CallerWrite,ReadonlyWrite,ReadonlyFetch] $ \role -> do
        let code = if role `elem` [Stock,StockRepeat] then loader else ownedLoader
            bounds = if role `elem` [Stock,StockRepeat] then stockBounds else ownedBounds
            functionSpans = if role `elem` [Stock,StockRepeat] then stockFunctions else ownedFunctions
            roleReadonly = if role `elem` [Stock,StockRepeat] then [] else readonlySpans
            specs = [roleSpec bounds functionSpans roleReadonly role domain entry row code (map toLower (show role) ++ "-" ++ show index) | (index,row) <- zip [0::Int ..] grid]
        batch <- writeBatch (directory </> show role) specs
        outcomes <- map snd <$> loadObservedBatch runner batch
        pure (specs,outcomes)
      case observed of
        [(stockSpecs,stock),(_,ownedObs),(_,equivalents),(_,mutants),(_,stockRepeat),(_,callerWrites),(_,readonlyWrites),(_,readonlyFetches)] -> do
          let compareOne reference left right = compareKernelCase adjusted (normalize left) (normalize right)
                where adjusted = case caseOutcome reference left of
                        OutcomeReturned -> reference
                        _ -> reference {caseKernel = fmap (\kernel -> kernel {kernelUncompared = kernelUncompared kernel ++ [R0,R4,R5,R6,R7,R8,R9,R10,R11,Sp]}) (caseKernel reference)}
              matches subjects = [null (compareOne reference left right) | (reference,left,right) <- zip3 stockSpecs stock subjects]
              good = matches ownedObs
              eqGood = matches equivalents
              mutantGood = matches mutants
              repeatGood = matches stockRepeat
              contractFailures subjects = [(index,show(expectedResult entry row),show(registerValue observation R0),show(caseOutcome reference observation)) | (index,(row,reference,observation)) <- zip [0::Int ..] (zip3 grid stockSpecs subjects),not(contractHolds entry row reference observation)]
              bad = [(i,caseOutcome reference left,caseOutcome reference right,compareOne reference left right) | (i,(reference,left,right)) <- zip [0::Int ..] (zip3 stockSpecs stock ownedObs), not(null(compareOne reference left right))]
          unless (null bad) (putStrLn (show(domain,entry) ++ " mismatches " ++ show (take 5 bad)))
          let validStock = and [contractHolds entry row reference observation | (row,reference,observation) <- zip3 grid stockSpecs stock]
              validOwned = and [contractHolds entry row reference observation | (row,reference,observation) <- zip3 grid stockSpecs ownedObs]
              callerRejected = all not (matches callerWrites)
              readonlyWriteHeld = all (guardHolds MemoryGuard) readonlyWrites
              readonlyFetchHeld = all (guardHolds FetchGuard) readonlyFetches
              framesHeld = and [frameHolds reference observation | subjects <- [stock,ownedObs,equivalents,stockRepeat], (reference,observation) <- zip stockSpecs subjects]
              holds = and good && and eqGood && and repeatGood && not(and mutantGood) && validStock && validOwned && callerRejected && readonlyWriteHeld && readonlyFetchHeld && framesHeld
          B.writeFile (directory </> "report.json") (encodeCanonical (object ["entry" .= show entry,"cases" .= length grid,"owned_matches" .= length(filter id good),"equivalent_matches" .= length(filter id eqGood),"stock_repeat_matches" .= length(filter id repeatGood),"mutant_differences" .= length(filter not mutantGood),"stock_valid" .= validStock,"owned_contract_valid" .= validOwned,"stock_contract_failures" .= contractFailures stock,"owned_contract_failures" .= contractFailures ownedObs,"caller_write_rejected" .= callerRejected,"readonly_write_refused" .= readonlyWriteHeld,"readonly_fetch_refused" .= readonlyFetchHeld,"frame_contract_valid" .= framesHeld,"holds" .= holds]))
          unless holds (putStrLn ("FAIL " ++ show(domain,entry) ++ " owned=" ++ show(length(filter id good)) ++ " equivalent=" ++ show(length(filter id eqGood)) ++ " mutant-diff=" ++ show(length(filter not mutantGood)) ++ " stock-valid=" ++ show validStock ++ " stock-contract=" ++ show(take 5(contractFailures stock)) ++ " owned-contract=" ++ show(take 5(contractFailures ownedObs))))
          if holds then mapM_ (removeDirectoryRecursive . (directory </>)) ["Stock","Owned","Equivalent","Mutant","StockRepeat","CallerWrite","ReadonlyWrite","ReadonlyFetch"] else pure ()
          pure (show(domain,entry),length grid,holds)
        _ -> error "role count"
  let flat = concat results
      passed = all (\(_,_,ok) -> ok) flat
  B.writeFile (work </> "summary.json") (encodeCanonical(object ["passed" .= passed,"entries" .= length flat,"cases_per_role" .= sum[n | (_,n,_) <- flat],"results" .= [object ["entry" .= name,"cases" .= count,"holds" .= ok] | (name,count,ok) <- flat]]))
  forM_ flat print
  unless passed (fail "S3c qualification refused")
  where
    loadAggregates carriers a sa pa b sb pb = do
      maps <- either (fail . show) pure . Union.parseUnion =<< B.readFile carriers
      left <- readAggregate (Union.unionA maps) a sa pa
      right <- readAggregate (Union.unionB maps) b sb pb
      pure (Just (left,right))
    guardHolds status observation = nativeStatus observation == status && registerValue observation R4 == 0xabc00004 && maybe False (all noWrite) (nativeKernel observation >>= observedOrder)
    noWrite (OrderedWrite _ _) = False
    noWrite _ = True

required :: [String] -> String -> IO String
required arguments flag = case [value | (key, value) <- zip arguments (drop 1 arguments), key == flag] of
  [] -> fail (flag ++ " is required")
  values -> pure (last values)
