{-# LANGUAGE OverloadedStrings #-}
module Replacement.RtosScheduler.S2 (main) where

import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (object, (.=))
import Data.Bits ((.&.), (.|.), shiftL)
import qualified Data.ByteString as B
import Data.Char (toLower)
import Data.List (sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe)
import Data.Word (Word32)
import Emulation.NativeObservation
import Emulation.OwnedCase
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.JSON (encodeCanonical)
import Numeric (readHex)
import Replacement.RtosOwnFirst.Artifact (loaderSha256)
import Replacement.RtosOwnFirst.Regions (Domain (..), domainLabel)
import Replacement.RtosScheduler.Oracle (stackBase, stackPointer)
import System.Directory (canonicalizePath, createDirectoryIfMissing, doesPathExist, makeAbsolute, removeDirectoryRecursive, setCurrentDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>), takeDirectory)

-- Values are synthetic except the version-bound loader and layout addresses.
data Entry = Save | Resume | NoSave | Select | FirstEntry | IrqReturn | IrqResume | IrqTail | Doorbell deriving (Eq, Ord, Show, Enum, Bounded)
data Redesign = NullOwnRegisters | NullUnlock | RemoteRestart deriving (Eq, Ord, Show)
data Selection = Same | Other | Null deriving (Eq, Show)
data Role = Stock | StockRepeat | Owned | Equivalent Entry | Mutant Entry | Revert Redesign deriving (Eq, Show)
newtype CaseIndex = CaseIndex Int deriving (Eq, Ord, Show)
newtype LockTag = LockTag Word32 deriving (Eq, Show)
data Row = Row { entry :: Entry, selection :: Selection, discarded :: Word32, enabled :: Word32, flags :: Word32, pending :: Word32, cpacr :: Word32, fpexc :: Word32, cpsr :: Word32, lockTag :: LockTag } deriving (Show)
data Image = Image { imageDomain :: Domain, stockBytes :: B.ByteString, ownedBytes :: B.ByteString, imageReloc :: Reloc, imageExtras :: [(Word32, Word32)] }

isAggregate :: Image -> Bool
isAggregate image = not (null (imageExtras image)) || any (\(a, _) -> not (any (\(lo, hi) -> a >= lo && a < hi) [(0xb00,0xb24),(0x3640,0x37dc),(0x37fc,0x3810)])) (relocRegions (imageReloc image))

extraRegions :: M.Map String (Word32, Word32) -> Domain -> [(Word32, Word32)]
extraRegions symbols domain =
  [ (start, start + size)
  | name <- ["helper_try", "helper_release", "helper_observe"]
  , Just (address, size) <- [M.lookup name symbols]
  , let start = address - base domain
  ]

-- | (S2-linked offset, aggregate offset, S2 size, aggregate size) for each body.
data Reloc = Reloc [(Word32, Word32, Word32, Word32)]

relocOffset :: Reloc -> Word32 -> Word32
relocOffset (Reloc spans) offset = case [agg + (offset - src) | (src, agg, size, _) <- spans, offset >= src, offset < src + size] of
  (value : _) -> value
  [] -> offset

relocRegions :: Reloc -> [(Word32, Word32)]
relocRegions (Reloc spans) = [(agg, agg + size) | (_, agg, _, size) <- spans]

relocEnd :: Reloc -> Word32 -> Word32
relocEnd (Reloc spans) offset = case [agg + aggSize | (src, agg, s2Size, aggSize) <- spans, offset > src, offset <= src + s2Size] of
  (value : _) -> value
  [] -> offset

parsePlatformSymbols :: FilePath -> IO (M.Map String (Word32, Word32))
parsePlatformSymbols path = do
  lines' <- lines <$> readFile path
  pure (M.fromList [(name, (address, size)) | row <- lines', Just (name, address, size) <- [parseRow row]])
  where
    parseRow row = case words row of
      [a, z, kind, name] | kind `elem` ["T", "t"], Just address <- hexWord a, Just size <- hexWord z -> Just (name, address, size)
      _ -> Nothing
    hexWord text = case text of
      '0' : 'x' : digits -> hexDigits digits
      _ -> hexDigits text
    hexDigits digits = case readHex digits of
      [(value, "")] | value <= toInteger (maxBound :: Word32) -> Just (fromInteger value)
      _ -> Nothing

relocation :: Domain -> M.Map String (Word32, Word32) -> Reloc
relocation domain symbols = Reloc
  [ (entryOffset False e, offsetOf (symbolName e), entrySize e, symbolSize (symbolName e))
  | e <- [minBound .. maxBound]
  ]
  where
    offsetOf name = maybe (error ("missing S2 symbol " ++ name)) (\(address, _) -> address - base domain) (M.lookup name symbols)
    symbolSize name = maybe 0 snd (M.lookup name symbols)
    symbolName e = case e of
      Save -> "xm1_rtos_scheduler_save"
      Resume -> "xm1_rtos_scheduler_resume"
      NoSave -> "xm1_rtos_scheduler_no_save"
      Select -> "xm1_rtos_scheduler_select"
      FirstEntry -> "xm1_rtos_scheduler_first_entry"
      IrqReturn -> "xm1_rtos_scheduler_irq_return"
      IrqResume -> "xm1_rtos_scheduler_irq_resume"
      IrqTail -> "xm1_rtos_scheduler_irq_tail"
      Doorbell -> "xm1_rtos_scheduler_doorbell"
    entrySize e = case e of
      Save -> 0x28
      Resume -> 0x20
      NoSave -> 0x1c
      Select -> 0x84
      FirstEntry -> 0x34
      IrqReturn -> 0x48
      IrqResume -> 0x4
      IrqTail -> 0x34
      Doorbell -> 0x24

redesignId :: Redesign -> String
redesignId NullOwnRegisters = "null-selection-own-registers"
redesignId NullUnlock = "null-loop-releases-leaked-global-lock"
redesignId RemoteRestart = "remote-terminate-restarts"

base :: Domain -> Word32
base DomainA = 0x40000
base DomainB = 0xd0000

at :: Domain -> Word32 -> Word32
at d offset = base d + offset

ownedAt :: Image -> Role -> Word32 -> Word32
ownedAt image role offset = at (imageDomain image) (if isStock role then offset else relocOffset (imageReloc image) offset)

entryOffset :: Bool -> Entry -> Word32
entryOffset stock e = case e of
  Save -> 0x3640
  Resume -> 0x3668
  NoSave -> 0x3688
  Select -> if stock then 0x36ac else 0x36a4
  FirstEntry -> 0x3728
  IrqReturn -> 0x375c
  IrqResume -> 0x37a4
  IrqTail -> 0x37a8
  Doorbell -> 0xb00

running, context, reschedule, task, config, globalLock, scratch, target, savedStack :: Domain -> Word32
running DomainA = 0x40054760
running DomainB = 0x400e4720
context DomainA = 0x400545a4
context DomainB = 0x400e4564
reschedule DomainA = 0x400546d8
reschedule DomainB = 0x400e4698
task DomainA = 0x4005ff0c
task DomainB = 0x400efecc
config _ = 0x96c000
globalLock _ = 0x400c0804
scratch _ = 0x96d000
target _ = 0x958f00
savedStack _ = 0x959c00

localTag :: Domain -> Word32
localTag DomainA = 0x11
localTag DomainB = 0x12

selected :: Domain -> Row -> Word32
selected d row = case selection row of Same -> task d; Other -> task d + 0x38; Null -> 0

trigger :: Domain -> Row -> [Redesign]
trigger d row =
  [NullOwnRegisters | entry row == IrqReturn, enabled row /= 0, selection row == Null]
  ++ [NullUnlock | loops row, lockTag row == LockTag (localTag d)]
  ++ [RemoteRestart | discarded row == 1, entry row == Save || (entry row == IrqReturn && enabled row /= 0 && selection row == Same)]

loops :: Row -> Bool
loops row = selection row == Null && (entry row `elem` [Save, NoSave, Select] || (entry row == IrqReturn && enabled row /= 0))

rows :: Domain -> [Row]
rows d =
  [ standard {entry=e, selection=s, discarded=q, cpacr=c, fpexc=f, cpsr=p}
  | e <- [Save, NoSave, Select], s <- [Same, Other], q <- [0,1,2], c <- [0,0x500000,0xf00000], f <- [0,0x40000000], p <- [0x93,0xd3] ]
  ++ [ standard {entry=IrqReturn, selection=s, discarded=q, enabled=en, cpacr=c, fpexc=f, cpsr=p}
     | s <- [Same,Other,Null], q <- [0,1,2], en <- [0,1,2,0x80000000], c <- [0,0xf00000], f <- [0,0x40000000], p <- [0x93,0xd3] ]
  ++ [ standard {entry=e, flags=fl, pending=pe, cpsr=p}
     | e <- [Resume,IrqResume,IrqTail], fl <- [0,4], pe <- [0,0x1234], p <- [0x93,0xd3] ]
  ++ [ standard {entry=FirstEntry, discarded=q, cpsr=p} | q <- [0,1,2], p <- [0x93,0xd3] ]
  ++ [ standard {entry=Doorbell, cpsr=p} | p <- [0x93,0xd3] ]
  ++ [ standard {entry=e, selection=Null, lockTag=LockTag tag, cpsr=p}
     | e <- [NoSave,Select], tag <- [0,localTag d,if d == DomainA then 0x12 else 0x11,0x21,0x22], p <- [0x93,0xd3] ]
  ++ [standard {entry=Save, selection=Null, discarded=q, cpsr=p} | q <- [0,2], p <- [0x93,0xd3]]
  where standard = Row Save Other 0 1 0 0 0xf00000 0x40000000 0xd3 (LockTag 0)

isStock :: Role -> Bool
isStock Stock = True
isStock StockRepeat = True
isStock _ = False

slice :: Word32 -> Word32 -> B.ByteString -> Word32 -> Word32 -> B.ByteString
slice imageBase imageEnd bytes start end
  | start < imageBase || end > imageEnd || end <= start = error ("S2 slice outside image " ++ show (imageBase, imageEnd, start, end))
  | otherwise = B.take (fromIntegral (end-start)) (B.drop (fromIntegral (start-imageBase)) bytes)

replace :: Word32 -> B.ByteString -> Word32 -> B.ByteString -> B.ByteString
replace imageBase bytes address replacement = B.take offset bytes <> replacement <> B.drop (offset+B.length replacement) bytes
  where offset = fromIntegral (address-imageBase)

branch :: Word32 -> Word32 -> Word32
branch from to = 0xea000000 .|. (((to-from-8) `div` 4) .&. 0xffffff)

-- Controls contain authored instructions, not loader instruction words.
control :: Reloc -> Domain -> Role -> B.ByteString -> B.ByteString
control reloc d role bytes = case role of
  Mutant e -> patchRaw (entryOffset False e) (branch (at d (entryOffset False e)) (target d))
  Equivalent e -> case e of
    Save -> patch 0x3650 0xe3330001
    Resume -> patch 0x3684 0xe12fff1e
    NoSave -> patch 0x369c 0xe2800093
    Select -> patch 0x36c8 0xe213360f
    FirstEntry -> patch 0x3734 0xe1821001
    IrqReturn -> patch 0x376c 0xe1100000
    IrqResume -> patch 0x37d0 0xe1820000
    IrqTail -> patch 0x37d0 0xe1820000
    Doorbell -> patch 0xb04 0xe24db004
  Revert NullOwnRegisters -> patch 0x36e0 0xe1a06006
  Revert NullUnlock -> patch 0x3714 0xe1a00000
  Revert RemoteRestart -> replace (at d 0xb00) (patch 0x3654 0xe1a00000) (at d (mapped 0x377c)) (word32le 0xe1a00000)
  _ -> bytes
  where
    mapped offset = relocOffset reloc offset
    patch offset value = replace (at d 0xb00) bytes (at d (mapped offset)) (word32le value)
    patchRaw offset value = replace (at d 0xb00) bytes (at d offset) (word32le value)

regions :: Image -> Role -> Row -> [CaseRegion]
regions image role row = code ++ dat
  where
    d = imageDomain image
    b = base d
    stock = stockBytes image
    own = control (imageReloc image) d role (ownedBytes image)
    raw = if isStock role then stock else own
    imageBase = if isStock role then b else at d 0xb00
    imageEnd = imageBase + fromIntegral (B.length raw)
    blocks = [(0xb00,0xb24),(0x3640,0x37dc),(0x37fc,0x3810)] ++ relocated ++ extras
    relocated = if isStock role || not aggregate then [] else mergedRelocated
    extras = if isStock role || not aggregate then [] else imageExtras image
    mergedRelocated = case relocRegions (imageReloc image) of
      [] -> []
      spans -> [(minimum (map fst spans), maximum (map snd spans))]
    aggregate = isAggregate image
    installed = if isStock role then stock else foldl install stock blocks
    install bytes (a,z) = replace b bytes (b+a) (slice imageBase imageEnd raw (b+a) (b+z))
    codeSpans = [(0xb00,0xb24),(if isStock role || not aggregate then 0x252c else 0x12b4, if isStock role || not aggregate then 0x3810 else maximum (0x403c : map snd (relocRegions (imageReloc image)) ++ map snd (imageExtras image)))]
    code = [mk ("code-"++show a) (b+a) ReadExecute (slice b (b+fromIntegral (B.length stock)) installed (b+a) (b+z)) | (a,z) <- codeSpans]
    (low,high) = (context d, running d + 12)
    LockTag tag = lockTag row
    pokes = [(context d,1),(running d,task d),(running d+4,enabled row),(running d+8,selected d row),(reschedule d,0)]
    imageWords start end values = B.concat [word32le (fromMaybe 0 (lookup address values)) | address <- [start,start+4..end-4]]
    taskWords t = [(t+8,config d),(t+0xc,1 .|. (flags row `shiftL` 16)),(t+0x10,pending row),(t+0x18,savedStack d),(t+0x1c,target d),(t+0x30,discarded row)]
    frame = [(stackPointer+fromIntegral (4*n),if n==8 then 0x958e00 else 0xa5a50000+fromIntegral n) | n <- [0::Int .. 15]]
    dat =
      [mk "domain" low ReadWrite (imageWords low high pokes)
      ,mk "tasks" (task d) ReadWrite (imageWords (task d) (task d+0x70) (taskWords (task d) ++ taskWords (task d+0x38)))
      ,mk "configuration" (config d) ReadWrite (imageWords (config d) (config d+0x24) [(config d+4,0x12345678),(config d+8,target d),(config d+0x20,0x10002)])
      ,mk "global" (globalLock d) ReadWrite (word32le tag)
      ,mk "sentinel" (scratch d) ReadWrite (word32le 0xfeedcafe)
      ,mk "stack" stackBase ReadWrite (B.replicate (fromIntegral (stackPointer-stackBase)) 0)
      ,mk "caller-frame" stackPointer ReadWrite (imageWords stackPointer (stackPointer+0x80) frame)
      ,mk "idle-stack" (if d==DomainA then 0x9ff00 else 0x12ff00) ReadWrite (B.replicate 0x100 0)
      ,mk "target-stack" (savedStack d-0x100) ReadWrite (B.replicate 0x100 0)
      ,mk "bank-a" 0xfffb30ec Device (word32le 0)
      ,mk "bank-b" 0xfffb40ec Device (word32le 0)
      ,mk "exit" 0x958e00 ReadExecute (word32le 0xe12fff1e)
      ,mk "dispatch-target" (target d) ReadExecute (word32le 0xe12fff1e)]
      ++ [mk "delivery-stop" (at d (if d==DomainA then 0x4288 else 0x4248)) ReadExecute (word32le 0xe12fff1e) | isStock role || not aggregate]
    mk name address permission bytes = mkRegion (RegionId name) (guest address) (B.length bytes) permission bytes

guest :: Word32 -> GuestAddress
guest = GuestAddress . fromIntegral

stop :: Image -> Role -> Domain -> Row -> Word32
stop image role d row
  | entry row==Save && selection row==Same && (isStock role || discarded row/=1) = ownedAt image role 0x3668
  | entry row `elem` [Resume,IrqResume,IrqTail], flags row .&. 4 /= 0, pending row /= 0 = at d (if d==DomainA then 0x4288 else 0x4248)
  | entry row `elem` [IrqResume,IrqTail] = ownedAt image role 0x37d4
  | entry row == IrqReturn && (enabled row==0 || (selection row==Same && discarded row/=1)) = ownedAt image role 0x37d4
  | otherwise = target d

spec :: Image -> Role -> CaseIndex -> Row -> CaseSpec
spec image role (CaseIndex index) row = CaseSpec
  { caseId = label role index
  , caseProvenance = "X-M1 1.50 S2 recovered dispatcher ABI and synthetic task/frame values"
  , caseExclusion = "No exception returns; stop before SPSR writes. Loop budgets end after one full null window."
  , caseEntry = guest (at d (entryOffset (isStock role) (entry row)))
  , caseExit = guest 0x958e00
  , caseRegions = regions image role row
  , caseFunctions =
      if isStock role
        then [(guest (at d a),z-a) | (a,z) <- baseFunctions]
        else [(guest (at d a),z-a) | (a,z) <- ownedFunctions]
  , caseInitial = [(r,initial r) | r <- [R0 .. R12]] ++ [(Sp,stackPointer),(Lr,0x958e00),(Cpsr,cpsr row)]
  , caseExpectedStatus = if loops row then InstructionBudget else StopAddress
  , caseExpectedRegisters = [(R8,0xa5a50000 + if entry row `elem` [Resume,IrqResume] then 4 else 8)]
  , caseExpectedMemory = []
  , caseCp14Identity = Just (if d==DomainA then 0x4003 else 0x4004)
  , caseKernel = Just (KernelCase (InstructionLimit limit) (if loops row || (isStock role && entry row==IrqReturn && RemoteRestart `elem` trigger d row) then Nothing else Just (guest (stop image role d row))) (Just 0x80001234) (Just (cpacr row)) (Just (fpexc row)) ignored [])
  }
  where
    d = imageDomain image
    initial r = case r of
      R1 -> task d
      R4 -> if NullOwnRegisters `elem` trigger d row then 0x10 else 0x13
      R5 -> if NullOwnRegisters `elem` trigger d row then 0x10 else 0x93
      R6 -> if NullOwnRegisters `elem` trigger d row then scratch d else context d
      _ -> 0xa5a50000 + fromIntegral (fromEnum r)
    limit | not (loops row) = 500
          | not (null (trigger d row)) = 400
          | otherwise = (if isStock role then 22 else 21) + case entry row of NoSave -> if isStock role then 9 else 7; Save -> 10; _ -> 0
    ignored = [R1,R2,R3,R12,Lr,Cpsr] ++ [r | entry row `elem` [Save,NoSave,Select,IrqReturn] && not (loops row), r <- [R4,R5,R6]] ++ [Pc | loops row]
    ownedFunctions = if isAggregate image then mergeSpans ([(entryOffset False e, entryOffset False e + 4) | e <- [minBound .. maxBound]] ++ [(relocOffset (imageReloc image) a, relocEnd (imageReloc image) z) | (a,z) <- baseFunctions] ++ imageExtras image) else baseFunctions
    mergeSpans spans = go (sortOn fst spans)
      where
        go [] = []
        go ((a,z) : rest) = let (a', z', rest') = extend a z rest in (a', z') : go rest'
        extend a z [] = (a, z, [])
        extend a z ((lo, hi) : rs) | lo < z = extend a (max z hi) rs
                                   | otherwise = (a, z, (lo, hi) : rs)
    baseFunctions = [(0xb00,0xb24),(0x3640,0x3668),(0x3668,0x3688),(if entry row==Select then entryOffset (isStock role) Select else 0x3688,0x3728),(0x3728,0x375c),(0x375c,0x37a4),(if entry row==IrqTail then 0x37a8 else 0x37a4,0x37dc),(0x252c,0x2554),(0x27b8,0x27d4),(0x27fc,0x2828),(0x2854,0x286c),(0x3444,0x34a4)]
    label r n = domainLabel d ++ "-" ++ map (\c -> if c==' ' then '-' else toLower c) (show r) ++ "-" ++ show n

reg :: Register -> NativeObservation -> Word32
reg r o = fromMaybe (error ("missing register "++show r)) (M.lookup r (nativeRegisters o))

word :: String -> Int -> NativeObservation -> Word32
word name offset o = maybe (error ("missing memory "++name)) (`readWordAt` offset) (lookup (RegionId name) (nativeMemory o))

order :: NativeObservation -> [Ordered]
order o = fromMaybe (error "truncated kernel order") (nativeKernel o >>= observedOrder)

writes :: Word32 -> NativeObservation -> [Word32]
writes address o = [fromIntegral v | OrderedWrite (Write a _) (WriteValue v) <- order o, a==guest address]

filtered :: NativeObservation -> NativeObservation
filtered o = o { nativeMemory=filter (not . excluded . fst) (nativeMemory o), nativeKernel=fmap (\k -> k {observedOrder=fmap (filter keep) (observedOrder k)}) (nativeKernel o) }
  where
    excluded ident = regionName ident `elem` ["stack","idle-stack","target-stack"] || take 5 (regionName ident)=="code-" || take 6 (regionName ident)=="table-"
    keep (OrderedWrite (Write a _) _) = let n=guestWord a in not (stackBase<=n && n<stackPointer) && not (0x9ff00<=n && n<0xa0000) && not (0x12ff00<=n && n<0x130000)
    keep _ = True

comparison :: Image -> Row -> CaseSpec -> NativeObservation -> NativeObservation -> [KernelDifference]
comparison _ _ description reference subject = compareKernelCase description {caseRegions=filter retained (caseRegions description)} (filtered reference) (filtered subject)
  where retained (CaseRegion region _) = not (regionIdOf region `elem` map RegionId ["stack","idle-stack","target-stack"])

expected :: Image -> Role -> Row -> NativeObservation -> Bool
expected image role row o
  | NullOwnRegisters `elem` tags = if stock then nativeStatus o==UnsupportedInstruction && reg Pc o==at d 0x3704 && word "sentinel" 0 o==1 else nativeStatus o==InstructionBudget && word "sentinel" 0 o==0xfeedcafe && reg Cpsr o .&. 0x1f==0x13 && 1 `elem` writes (context d) o
  | NullUnlock `elem` tags = nativeStatus o==InstructionBudget && if stock then word "global" 0 o==localTag d else word "global" 0 o==0 && writes (globalLock d) o==[0] && barrierBeforeUnlock
  | RemoteRestart `elem` tags = if stock then if entry row==Save then nativeStatus o==StopAddress && reg Pc o==stop image role d row && word "tasks" 0x1c o==at d 0x3668 else nativeStatus o==UnsupportedInstruction && reg Pc o==at d 0x37d4 else nativeStatus o==StopAddress && reg Pc o==target d && reg Sp o==savedStack d && word "tasks" 0x18 o==savedStack d && word "tasks" 0x1c o==target d
  | otherwise = ordinary
  where
    d=imageDomain image
    stock=isStock role
    tags=trigger d row
    barrierBeforeUnlock = case break isUnlock (order o) of (before,_:_) -> OrderedBarrier Dmb `elem` before && 1 `elem` writes (context d) o; _ -> False
    isUnlock (OrderedWrite (Write a _) (WriteValue 0)) = a==guest (globalLock d)
    isUnlock _ = False
    ordinary
      | loops row = nativeStatus o==InstructionBudget && word "global" 0 o==let LockTag t=lockTag row in t
      | entry row==Doorbell = nativeStatus o==Returned && word "domain" (fromIntegral (reschedule d-context d)) o==1 && word (if d==DomainA then "bank-a" else "bank-b") 0 o==0x1000000
      | entry row==Resume = if flags row .&. 4 /=0 && pending row/=0 then nativeStatus o==StopAddress else nativeStatus o==Returned
      | otherwise = nativeStatus o==StopAddress && reg Pc o==stop image role d row && systemHolds && frameHolds
    systemHolds
      | entry row `elem` [Save,NoSave,Select] || (entry row==IrqReturn && enabled row/=0 && selection row/=Same) =
          (nativeKernel o >>= observedTpidrurw)==Just 0x10002 && (nativeKernel o >>= observedFpexc)==Just (if cpacr row .&. 0xf00000/=0 then 0 else fpexc row) && reg Sp o==(if entry row==Save && selection row==Same then stackPointer-36 else savedStack d)
      | entry row==FirstEntry = reg R0 o==0x12345678 && reg Lr o==at d (if d==DomainA then 0x53f0 else 0x53b0) && word "tasks" 0x30 o==0 && reg Cpsr o .&. 0x1ff==0x53
      | otherwise = True
    frameHolds
      | entry row==Save && discarded row/=1 = saved 36 0x3668
      | entry row==IrqReturn && enabled row/=0 && selection row==Other && discarded row/=1 = saved 32 0x37a4
      | otherwise = True
    saved bytes resume =
      word "tasks" 0x18 o==stackPointer-bytes && word "tasks" 0x1c o==ownedAt image role resume
      && and [word "stack" (fromIntegral (stackPointer-stackBase-bytes+4*n)) o==value | (n,value)<-zip [0..] values]
      where values = [0x13,0x93,context d] ++ [0xa5a50000+n | n<-[7..11]] ++ [0x958e00 | bytes==36]

main :: IO ()
main = do
  args <- getArgs
  let input name = required args name >>= canonicalizePath
  runner <- input "--runner"
  paths <- mapM input ["--loader-a","--loader-b","--owned-a","--owned-b","--symbols-a","--symbols-b"]
  work <- required args "--work" >>= makeAbsolute
  summary <- required args "--summary" >>= makeAbsolute
  lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= mapM_ setCurrentDirectory
  forM_ [work,summary] $ \p -> doesPathExist p >>= \exists -> when exists (fail (p++" already exists"))
  images <- case paths of
    [la,lb,oa,ob,sa,sb] -> forM [(DomainA,la,oa,sa),(DomainB,lb,ob,sb)] $ \(d,l,p,s) -> do
      loader <- B.readFile l
      owned <- B.readFile p
      symbols <- parsePlatformSymbols s
      unless (sha256Hex (sha256 loader)==loaderSha256 d) (fail "not pinned X-M1 1.50 loader")
      unless (B.length owned > 0x1000) (fail "owned image is too small")
      pure (Image d loader owned (relocation d symbols) (extraRegions symbols d))
    _ -> fail "input arity"
  createDirectoryIfMissing True work
  reports <- forM images $ \image -> do
    let d=imageDomain image
        grid=zip (map CaseIndex [0..]) (rows d)
        indexed=if "--sample" `elem` args then concat [take 3 [pair | pair@(_,row)<-grid,entry row==e] | e<-[minBound..maxBound]] ++ concat [take 1 [pair | pair@(_,row)<-grid,r `elem` trigger d row] | r<-[NullOwnRegisters,NullUnlock,RemoteRestart]] else grid
        roles=[Stock,Owned,StockRepeat] ++ map Equivalent [minBound..maxBound] ++ map Mutant [minBound..maxBound] ++ map Revert [NullOwnRegisters,NullUnlock,RemoteRestart]
        applicable role row = case role of Equivalent e -> entry row==e; Mutant e -> entry row==e; Revert r -> r `elem` trigger d row; _ -> True
        descriptions=[(caseId s,s) | (key,row)<-indexed, role<-roles, applicable role row, let s=spec image role key row]
        directory=work </> domainLabel d
    path <- writeBatch directory (map snd descriptions)
    results <- loadObservedBatch runner path
    let observations = M.fromList [(name,o) | (CaseId name,o) <- results]
    let observed role key row = observations M.! caseId (spec image role key row)
        matches role key row = expected image role row (observed role key row)
        stockHolds=and [expected image Stock row (observed Stock key row) | (key,row)<-indexed]
        checks=[(key,row,matches Owned key row) | (key,row)<-indexed]
        controls=[(show role,case role of Mutant _ -> any (\(key,row)->not (matches role key row)) chosen; Revert _ -> any (\(key,row)->not (matches role key row)) chosen; _ -> all (\(key,row)->matches role key row) chosen) | role<-drop 2 roles, let chosen=filter (applicable role . snd) indexed]
        failures=[object ["case" .= show key,"row" .= show row,"tags" .= map redesignId (trigger d row),"stock_status" .= show (nativeStatus (observed Stock key row)),"owned_status" .= show (nativeStatus (observed Owned key row)),"stock_pc" .= show (reg Pc (observed Stock key row)),"owned_pc" .= show (reg Pc (observed Owned key row)),"owned_sp" .= show (reg Sp (observed Owned key row)),"owned_lr" .= show (reg Lr (observed Owned key row)),"owned_r0" .= show (reg R0 (observed Owned key row)),"declared_stop" .= show (stop image Owned d row),"owned_order_tail" .= show (take 8 (reverse (order (observed Owned key row)))),"stock_expected" .= expected image Stock row (observed Stock key row),"owned_expected" .= expected image Owned row (observed Owned key row),"differences" .= map show (comparison image row (spec image Owned key row) (observed Stock key row) (observed Owned key row))] | (key,row,ok)<-checks, not ok || not (expected image Stock row (observed Stock key row))]
        holds=stockHolds && all (\(_,_,ok)->ok) checks && all snd controls
        report=object ["domain" .= domainLabel d,"cases" .= length indexed,"untagged" .= length [() | (_,r)<-indexed,null (trigger d r)],"expected_differences" .= [object ["id" .= redesignId r,"count" .= length [() | (_,row)<-indexed,r `elem` trigger d row]] | r<-[NullOwnRegisters,NullUnlock,RemoteRestart]],"stock_holds" .= stockHolds,"owned_holds" .= all (\(_,_,ok)->ok) checks,"controls" .= [object ["name" .= name,"holds" .= ok] | (name,ok)<-controls],"failures" .= failures,"holds" .= holds]
    when holds (removeDirectoryRecursive directory)
    pure (holds,report)
  createDirectoryIfMissing True (takeDirectory summary)
  runnerBytes <- B.readFile runner
  B.writeFile summary (encodeCanonical (object ["schema" .= (1::Int),"runner_sha256" .= sha256Hex (sha256 runnerBytes),"inputs" .= [object ["domain" .= domainLabel (imageDomain image),"loader_sha256" .= sha256Hex (sha256 (stockBytes image)),"owned_sha256" .= sha256Hex (sha256 (ownedBytes image))] | image<-images],"domains" .= map snd reports,"holds" .= all fst reports]))
  unless (all fst reports) exitFailure
