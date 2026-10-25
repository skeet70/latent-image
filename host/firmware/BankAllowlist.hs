{-# LANGUAGE OverloadedStrings #-}

-- | The mechanical allowlist check for the bank part of a relinked RAM-push
-- ELF. Its guarantees and limits are stated in transport/README.md
-- ("Bank ELF allowlist").
module BankAllowlist
  ( Allowlist (..)
  , StockCall (..)
  , StockStore (..)
  , Declaration (..)
  , Access (..)
  , Provenance (..)
  , Location (..)
  , Callee (..)
  , parseAllowlist
  , Finding (..)
  , Reason (..)
  , Region (..)
  , Verdict (..)
  , Report (..)
  , checkBank
  , checkBankFile
  , violations
  , declared
  , renderFinding
  , renderDeclaration
  , reportValue
  ) where

import A32
import ArmElf
import Control.Monad (unless, when)
import Data.Aeson (Value (..))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (complement, shiftL, shiftR, testBit, xor, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Char (isDigit, isHexDigit)
import Data.Int (Int32)
import Data.List (find, nub, sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, listToMaybe)
import qualified Data.Set as S
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32)
import FujiAF.JSON (decodeStrictIntegers)
import Numeric (readHex, showHex)
import RamPushLayout (runtimeAllocationSize, runtimeBankEnd, runtimeDataBase, runtimeExecutableBase, runtimeInstallerBase)

-- * Allowlist

data Allowlist = Allowlist
  { stockCalls :: [StockCall]
  , stockStores :: [StockStore]
  , declarations :: [Declaration]
  }

data StockCall = StockCall {stockCallAddress :: Word32, stockCallName :: String, stockCallSource :: String}

data StockStore = StockStore {stockStoreAddress :: Word32, stockStoreBytes :: Word32, stockStoreName :: String, stockStoreSource :: String}

-- | A store or indirect branch whose address the analysis cannot bound,
-- accepted for one function and one provenance.
data Declaration = Declaration
  { declarationFunction :: String
  , declarationAccess :: Access
  , declarationThrough :: Provenance
  , declarationReason :: String
  }

data Access = StoreAccess | BranchAccess deriving (Eq, Show)

-- | Where an unbounded pointer came from; its offset is not part of it.
data Provenance
  = Argument Reg
  | -- | The word at entry sp plus this offset, which this function did not write.
    CallerFrame Int32
  | Cell Location
  | Result Callee
  | Index Location
  | Deref Provenance
  deriving (Eq, Show)

data Location = BankObject String | GeneratedData | StackFrame | StockAddress Word32
  deriving (Eq, Show)

data Callee = BankFunction String | StockFunction Word32
  deriving (Eq, Show)

parseAllowlist :: B.ByteString -> Either String Allowlist
parseAllowlist bytes = do
  document <- decodeStrictIntegers bytes
  closed "allowlist" document ["schema", "stock_calls", "stock_stores", "unproven"]
  schema <- member document "schema"
  unless (schema == Number 1) (Left "allowlist: schema 1 required")
  calls <- list "stock_calls" document >>= mapM parseCall
  stores <- list "stock_stores" document >>= mapM parseStore
  unproven <- list "unproven" document >>= mapM parseDeclaration
  let callAddresses = map stockCallAddress calls
  unless (length (nub callAddresses) == length callAddresses) (Left "allowlist: duplicate stock call address")
  pure (Allowlist calls stores unproven)
 where
  parseCall value = do
    closed "stock call" value ["address", "name", "source"]
    address <- member value "address" >>= hexWord
    unless (address .&. 3 == 0) (Left ("allowlist: stock call " ++ hex address ++ " is not an ARM-state entry"))
    unless (address < deviceSpaceStart) (Left ("allowlist: stock call " ++ hex address ++ " is in " ++ regionName DeviceSpace))
    StockCall address <$> (member value "name" >>= text) <*> (member value "source" >>= text)
  parseStore value = do
    closed "stock store" value ["address", "bytes", "name", "source"]
    address <- member value "address" >>= hexWord
    size <- member value "bytes" >>= byteCount
    unless (size `notElem` [2, 4, 8] || address .&. (size - 1) == 0) (Left ("allowlist: stock store " ++ hex address ++ " is not aligned to its size"))
    case (stockRegion address, stockRegion (address + size - 1)) of
      (StockMemory, StockMemory) -> pure ()
      (region, _) -> Left ("allowlist: stock store " ++ hex address ++ " lies in " ++ regionName region)
    StockStore address size <$> (member value "name" >>= text) <*> (member value "source" >>= text)
  parseDeclaration value = do
    closed "unproven declaration" value ["access", "function", "reason", "through"]
    function <- member value "function" >>= text
    access <- member value "access" >>= text >>= accessOf
    through <- member value "through" >>= parseProvenance (4 :: Int)
    Declaration function access through <$> (member value "reason" >>= text)
  accessOf "store" = Right StoreAccess
  accessOf "branch" = Right BranchAccess
  accessOf other = Left ("allowlist: access must be store or branch, got " ++ other)
  parseProvenance depth value
    | depth == 0 = Left "allowlist: provenance nesting is too deep"
    | otherwise = case value of
        Object object -> case KM.toList object of
          [("argument", String r)] -> Argument <$> parseReg (T.unpack r)
          [("caller-frame", Number n)] | n >= 0 && n <= 4096 && n == fromIntegral (truncate n :: Int32) -> Right (CallerFrame (truncate n))
          [("cell", location)] -> Cell <$> parseLocation location
          [("index", location)] -> Index <$> parseLocation location
          [("result", String callee)] -> Right (Result (calleeOf (T.unpack callee)))
          [("deref", inner)] -> Deref <$> parseProvenance (depth - 1) inner
          _ -> Left "allowlist: provenance must be one of argument, cell, index, result or deref"
        _ -> Left "allowlist: provenance must be an object"
  parseLocation (String location) = case T.unpack location of
    "generated-data" -> Right GeneratedData
    "stack-frame" -> Right StackFrame
    name@('0' : 'x' : _) -> StockAddress <$> hexWord (String (T.pack name))
    name -> Right (BankObject name)
  parseLocation _ = Left "allowlist: location must be a string"
  calleeOf callee = either (const (BankFunction callee)) StockFunction (hexWord (String (T.pack callee)))
  parseReg ('r' : digits) | not (null digits) && all isDigit digits && read digits <= (12 :: Int) = Right (Reg (read digits))
  parseReg other = Left ("allowlist: argument register r0..r12 required, got " ++ other)
  member (Object object) key = maybe (Left ("allowlist: missing " ++ key)) Right (KM.lookup (K.fromString key) object)
  member _ key = Left ("allowlist: object required around " ++ key)
  closed label (Object object) keys =
    unless (S.fromList (map K.toString (KM.keys object)) == S.fromList keys) (Left ("allowlist: " ++ label ++ " needs exactly " ++ unwords keys))
  closed label _ _ = Left ("allowlist: " ++ label ++ " must be an object")
  list key document = member document key >>= \value -> case value of
    Array values -> Right (V.toList values)
    _ -> Left ("allowlist: " ++ key ++ " must be a list")
  text (String value) | not (T.null value) = Right (T.unpack value)
  text _ = Left "allowlist: nonempty string required"
  byteCount (Number n)
    | n >= 1 && n <= 256 && n == fromIntegral (truncate n :: Int) = Right (truncate n)
  byteCount _ = Left "allowlist: stock store bytes must be an integer from 1 to 256"
  hexWord (String value) = case T.unpack value of
    '0' : 'x' : digits | length digits == 8 && all isHexDigit digits, [(word, "")] <- readHex digits -> Right word
    other -> Left ("allowlist: eight-digit 0x word required, got " ++ other)
  hexWord _ = Left "allowlist: hex word string required"

-- * Findings

data Verdict = Violation | DeclaredBy String deriving (Eq, Show)

data Finding = Finding
  { findingAddress :: Word32
  , findingFunction :: String
  , findingVerdict :: Verdict
  , findingReason :: Reason
  , findingDetail :: String
  }
  deriving (Eq, Show)

-- | What a reported site does.
data Reason
  = LayoutProblem
  | ForbiddenInstruction Forbidden
  | DeviceLoad
  | -- | First and last byte, and the region the store leaves the bank for.
    StoreOutside Word32 Word32 Region
  | StackStoreOverSaved
  | StackStoreBeyondFrame
  | UnboundedStore
  | UnboundedBranch
  | -- | Unbounded and either declared or not.
    Unproven Access Provenance
  | UnprovenArgument
  | TargetMisaligned Word32
  | TargetNotEntry Word32 Region
  | UndeclaredStockTarget Word32
  | TargetNotCode Word32
  | Unreached
  | UnknownDeclaredFunction
  deriving (Eq, Show)

data Report = Report
  { reportFindings :: [Finding]
  , -- | Declarations no site matched; they pass but widen what a later build may do.
    reportUnused :: [Declaration]
  , reportProvenStores :: Int
  , reportProvenBranches :: Int
  , reportInstructions :: Int
  , reportFunctions :: Int
  }

violations, declared :: Report -> [Finding]
violations = filter ((== Violation) . findingVerdict) . reportFindings
declared = filter ((/= Violation) . findingVerdict) . reportFindings

renderFinding :: Finding -> String
renderFinding finding =
  hex (findingAddress finding) ++ " " ++ findingFunction finding ++ ": " ++ findingDetail finding
    ++ case findingVerdict finding of
      Violation -> ""
      DeclaredBy reason -> " [declared: " ++ reason ++ "]"

reportValue :: Report -> Value
reportValue report =
  Object
    ( KM.fromList
        [ ("result", String (if null (violations report) then "pass" else "fail"))
        , ("functions", count (reportFunctions report))
        , ("instructions", count (reportInstructions report))
        , ("proven_stores", count (reportProvenStores report))
        , ("proven_branches", count (reportProvenBranches report))
        , ("violations", Array (V.fromList (map item (violations report))))
        , ("declared", Array (V.fromList (map item (declared report))))
        , ("unused_declarations", Array (V.fromList [String (T.pack (renderDeclaration d)) | d <- reportUnused report]))
        ]
    )
 where
  count = Number . fromIntegral
  item finding =
    Object
      ( KM.fromList
          ( [ ("address", String (T.pack (hex (findingAddress finding))))
            , ("function", String (T.pack (findingFunction finding)))
            , ("detail", String (T.pack (findingDetail finding)))
            ]
              ++ case findingVerdict finding of
                Violation -> []
                DeclaredBy reason -> [("reason", String (T.pack reason))]
          )
      )

hex :: Word32 -> String
hex value = let digits = showHex value "" in "0x" ++ replicate (8 - length digits) '0' ++ digits

renderDeclaration :: Declaration -> String
renderDeclaration d =
  declarationFunction d ++ " " ++ (if declarationAccess d == StoreAccess then "store" else "branch") ++ " through "
    ++ showProvenance (declarationThrough d)

-- * Layout

data Layout = Layout
  { allocationBase :: Word32
  , allocationEnd :: Word32
  , textStart :: Word32
  , textEnd :: Word32
  , dataStart :: Word32
  , imageEnd :: Word32
  , generatedStart :: Word32
  , bankEnd :: Word32
  , installerStart :: Word32
  , runtimeEnd :: Word32
  }

data Region
  = AllocationHeader
  | BankCode
  | BankData
  | AllocationGap
  | GeneratedArea
  | ExecutableBundle
  | InstallerArea
  | AllocationTail
  | StockImage String
  | DeviceSpace
  | StockMemory
  deriving (Eq, Show)

regionName :: Region -> String
regionName region = case region of
  AllocationHeader -> "the allocation header (installer metadata and flash store image)"
  BankCode -> "bank code"
  BankData -> "bank data"
  AllocationGap -> "unassigned allocation space"
  GeneratedArea -> "generated bank data"
  ExecutableBundle -> "the relocated executable bundle"
  InstallerArea -> "the RAM-push installer"
  AllocationTail -> "the allocation past the runtime"
  StockImage name -> "stock " ++ name ++ " code"
  DeviceSpace -> "boot ROM or device register space"
  StockMemory -> "stock RAM"

deviceSpaceStart :: Word32
deviceSpaceStart = 0x80000000

-- | The decoded images the installed loader runs: image 4, image 5 and the
-- plain image (the extents emitRamPush writes into the allocation header).
stockRegion :: Word32 -> Region
stockRegion address
  | address >= deviceSpaceStart = DeviceSpace
  | within 0x0061c400 0x00282800 = StockImage "image 4"
  | within 0x00bf1a00 0x004d0020 = StockImage "image 5"
  | within 0x01100000 0x00080000 = StockImage "plain image"
  | otherwise = StockMemory
 where
  within start size = start <= address && toInteger address < toInteger start + size

stockCode :: Region -> Bool
stockCode region = case region of
  StockImage _ -> True
  StockMemory -> True
  _ -> False

regionOf :: Layout -> Word32 -> Region
regionOf layout address
  | address < allocationBase layout || address >= allocationEnd layout = stockRegion address
  | address < textStart layout = AllocationHeader
  | address < textEnd layout = BankCode
  | address >= dataStart layout && address < imageEnd layout = BankData
  | address >= generatedStart layout && address < bankEnd layout = GeneratedArea
  | address >= bankEnd layout && address < installerStart layout = ExecutableBundle
  | address >= installerStart layout && address < runtimeEnd layout = InstallerArea
  | address >= runtimeEnd layout = AllocationTail
  | otherwise = AllocationGap

writable :: Region -> Bool
writable region = region == BankData || region == GeneratedArea

readLayout :: Elf -> Either String Layout
readLayout elf = do
  base <- absolute "RAM_PUSH_BASE"
  end <- absolute "RAM_PUSH_BANK_END"
  start <- symbol "__rb_image_start"
  text <- symbol "__rb_text_end"
  image <- symbol "__rb_image_end"
  installer <- symbol "__ram_push_installer_start"
  runtime <- symbol "__ram_push_end"
  dataSection <- case [s | s <- elfSections elf, sectionName s == ".data"] of
    [s] -> Right s
    _ -> Left "layout: exactly one .data section required"
  unless (toInteger base + toInteger runtimeAllocationSize <= 0xffffffff) (Left "layout: allocation leaves the address space")
  pure
    Layout
      { allocationBase = base
      , allocationEnd = base + runtimeAllocationSize
      , textStart = start
      , textEnd = text
      , dataStart = sectionAddress dataSection
      , imageEnd = image
      , generatedStart = runtimeDataBase base
      , bankEnd = end
      , installerStart = installer
      , runtimeEnd = runtime
      }
 where
  absolute name = case [s | s <- elfSymbols elf, symbolName s == name, symbolSection s == Absolute] of
    [s] -> Right (symbolValue s)
    _ -> Left ("layout: absolute symbol " ++ name ++ " required")
  symbol name = case [s | s <- elfSymbols elf, symbolName s == name, symbolSection s /= Undefined] of
    [s] -> Right (symbolValue s)
    _ -> Left ("layout: symbol " ++ name ++ " required")

bankSections, installerSections :: [String]
bankSections = [".text", ".data"]
installerSections = [".ram_push.text", ".ram_push.publications", ".ram_push.rendezvous", ".ram_push.mailbox"]

layoutFindings :: Elf -> Layout -> [Finding]
layoutFindings elf layout =
  concat
    [ [ violation (sectionAddress s) ("allocated section " ++ sectionName s ++ " is outside the bank and installer layout")
      | s <- allocated
      , sectionName s `notElem` bankSections ++ installerSections
      ]
    , [ violation (sectionAddress s) ("section " ++ sectionName s ++ " is not PROGBITS")
      | s <- allocated
      , sectionType s /= 1
      ]
    , check (textStart layout == runtimeExecutableBase (allocationBase layout)) (textStart layout) "bank code does not start at the allocation's executable offset"
    , sectionCheck ".text" $ \s ->
        sectionAddress s == textStart layout && sectionAddress s + sectionSize s == textEnd layout
          && sectionExecutable s && not (sectionWritable s)
    , sectionCheck ".data" $ \s ->
        sectionAddress s >= textEnd layout && sectionAddress s + sectionSize s == imageEnd layout
          && sectionWritable s && not (sectionExecutable s)
    , check (imageEnd layout <= generatedStart layout) (imageEnd layout) "bank image overlaps generated data"
    , check (bankEnd layout == runtimeBankEnd (allocationBase layout)) (bankEnd layout) "generated bank data has the wrong writable end"
    , check (generatedStart layout <= bankEnd layout && bankEnd layout <= installerStart layout) (bankEnd layout) "generated data overlaps the executable bundle"
    , check (installerStart layout == runtimeInstallerBase (allocationBase layout)) (installerStart layout) "installer has the wrong placement after the executable bundle"
    , check (runtimeEnd layout <= allocationEnd layout) (runtimeEnd layout) "runtime exceeds the allocation"
    , [ violation (sectionAddress s) ("installer section " ++ sectionName s ++ " lies outside the installer extent")
      | s <- allocated
      , sectionName s `elem` installerSections
      , sectionAddress s < installerStart layout || sectionAddress s + sectionSize s > runtimeEnd layout
      ]
    , [ violation (segmentAddress g) "loadable segment is both writable and executable"
      | g <- elfSegments elf
      , loadSegment g
      , segmentWritable g && segmentExecutable g
      ]
    , [ violation (symbolValue s) "Thumb code in the bank"
      | s <- elfSymbols elf
      , symbolName s == "$t"
      , symbolValue s >= textStart layout && symbolValue s < imageEnd layout
      ]
    ]
 where
  allocated = filter sectionAllocated (elfSections elf)
  violation address = Finding address "<layout>" Violation LayoutProblem
  check ok address message = [violation address message | not ok]
  sectionCheck name predicate = case [s | s <- allocated, sectionName s == name] of
    [s] -> check (predicate s) (sectionAddress s) ("section " ++ name ++ " has the wrong extent or permissions")
    _ -> [violation 0 ("exactly one " ++ name ++ " section required")]

-- * Abstract values

data Abstract
  = Exact (S.Set Word32)
  | -- | Unsigned, inclusive.
    Interval Word32 Word32
  | StackOffset Int32
  | -- | Stack-pointer offsets relative to entry, inclusive.
    StackSpan Int32 Int32
  | ReturnAddress
  | Derived Provenance
  | Unknown
  deriving (Eq, Show)

exact :: Word32 -> Abstract
exact = Exact . S.singleton

setCap :: Int
setCap = 64

fromSet :: S.Set Word32 -> Abstract
fromSet values
  | S.null values = Unknown
  | S.size values <= setCap = Exact values
  | otherwise = Interval (S.findMin values) (S.findMax values)

-- | The words a value can hold, when there are at most `setCap` of them.
enumerate :: Abstract -> Maybe [Word32]
enumerate (Exact values) = Just (S.toList values)
enumerate (Interval lo hi) | toInteger hi - toInteger lo < toInteger setCap = Just [lo .. hi]
enumerate _ = Nothing

bounds :: Abstract -> Maybe (Word32, Word32)
bounds (Exact values) = Just (S.findMin values, S.findMax values)
bounds (Interval lo hi) = Just (lo, hi)
bounds _ = Nothing

scalar :: Abstract -> Bool
scalar value = case value of
  Exact _ -> True
  Interval _ _ -> True
  Unknown -> True
  Derived _ -> True
  _ -> False

-- | `place` names the object an address lies in, so a known address joins
-- an unbounded index from the same object's base.
joinValue :: (Word32 -> Location) -> Abstract -> Abstract -> Abstract
joinValue _ a b | a == b = a
joinValue _ (Exact a) (Exact b) = fromSet (S.union a b)
joinValue _ a b | Just (alo, ahi) <- bounds a, Just (blo, bhi) <- bounds b = Interval (min alo blo) (max ahi bhi)
joinValue _ a b | Just (alo, ahi) <- stackBounds a, Just (blo, bhi) <- stackBounds b = StackSpan (min alo blo) (max ahi bhi)
joinValue _ (Exact zero) derived@(Derived _) | zero == S.singleton 0 = derived
joinValue _ derived@(Derived _) (Exact zero) | zero == S.singleton 0 = derived
joinValue place (Derived (Index l)) b | Just (lo, _) <- bounds b, place lo == l = Derived (Index l)
joinValue place a (Derived (Index l)) | Just (lo, _) <- bounds a, place lo == l = Derived (Index l)
joinValue _ _ _ = Unknown

stackBounds :: Abstract -> Maybe (Int32, Int32)
stackBounds (StackOffset o) = Just (o, o)
stackBounds (StackSpan lo hi) = Just (lo, hi)
stackBounds _ = Nothing

-- | A small index read as signed stack-offset deltas: every value, or the two bounds.
signedOffsets :: Abstract -> Maybe [Int32]
signedOffsets value = case (enumerate value, bounds value) of
  (Just xs, _) | all small offsets -> Just offsets where offsets = map fromIntegral xs
  (_, Just (lo, hi)) | hi < 0x10000 -> Just [fromIntegral lo, fromIntegral hi]
  _ -> Nothing
 where
  small o = abs o < 0x10000

stackAt :: [Int32] -> Abstract
stackAt [o] = StackOffset o
stackAt offsets = StackSpan (minimum offsets) (maximum offsets)

-- | Joins, then moves a growing interval bound out to the next threshold.
widenValue :: (Word32 -> Location) -> [Word32] -> Abstract -> Abstract -> Abstract
widenValue place thresholds old new
  | joined == old = old
  | Exact _ <- joined = joined
  | Derived _ <- joined = joined
  | Just (lo', _) <- bounds joined, lo' >= addressFloor = Derived (Index (place lo'))
  | Just (lo, hi) <- bounds old, Just (lo', hi') <- bounds joined =
      Interval
        (if lo' < lo then fromMaybe 0 (listToMaybe (reverse (filter (<= lo') thresholds))) else lo)
        (if hi' > hi then fromMaybe maxBound (listToMaybe (filter (>= hi') thresholds)) else hi)
  | otherwise = Unknown
 where
  joined = joinValue place old new

-- | Values from here up are taken as addresses when widened: a growing
-- pointer becomes an unbounded index from its lowest object.
addressFloor :: Word32
addressFloor = 0x10000

pointwise :: (Word32 -> Word32 -> Word32) -> Abstract -> Abstract -> Maybe Abstract
pointwise f a b = do
  xs <- enumerate a
  ys <- enumerate b
  if length xs * length ys > setCap then Nothing else Just (fromSet (S.fromList [f x y | x <- xs, y <- ys]))

-- * Abstract state

data State = State
  { registers :: M.Map Int Abstract
  , slots :: M.Map Int32 Abstract
  , comparison :: Maybe Comparison
  , -- | A condition the current flags are known to satisfy on this path.
    holds :: Maybe Cond
  }
  deriving (Eq, Show)

entryState :: State
entryState =
  State
    (M.fromList ([(n, Derived (Argument (Reg n))) | n <- [0 .. 12]] ++ [(13, StackOffset 0), (14, ReturnAddress)]))
    M.empty
    Nothing
    Nothing

joinState :: (Word32 -> Location) -> State -> State -> State
joinState place a b =
  State
    (M.unionWith (joinValue place) (registers a) (registers b))
    (M.intersectionWith (joinValue place) (slots a) (slots b))
    (if comparison a == comparison b then comparison a else Nothing)
    (if holds a == holds b then holds a else Nothing)

widenState :: (Word32 -> Location) -> [Word32] -> State -> State -> State
widenState place thresholds old new =
  State
    (M.unionWith (widenValue place thresholds) (registers old) (registers new))
    (M.intersectionWith (widenValue place thresholds) (slots old) (slots new))
    (if comparison old == comparison new then comparison old else Nothing)
    (if holds old == holds new then holds old else Nothing)

-- | The flags of `cmp r, #k` (unsigned conditions valid) or `cmn r, #-k` (only EQ and NE).
data Comparison = Comparison Reg Word32 Bool deriving (Eq, Show)

-- * Analysis context

data Context = Context
  { contextElf :: Elf
  , contextLayout :: Layout
  , contextAllowlist :: Allowlist
  , bankFunctions :: M.Map Word32 Function
  , codeWords :: S.Set Word32
  , namedObjects :: [(Word32, Word32, String)]
  , -- | The (callee entry, argument) pairs being analysed with that argument 0.
    zeroArguments :: [(Word32, Reg)]
  }

-- | A bank function, or a run of ARM code that no function symbol covers.
data Function = Function {functionName :: String, functionStart :: Word32, functionEnd :: Word32}

locate :: Context -> Word32 -> Location
locate context address = case regionOf (contextLayout context) address of
  GeneratedArea -> GeneratedData
  region
    | region == BankCode || region == BankData
    , Just (_, _, name) <- find (\(start, end, _) -> start <= address && address < end) (namedObjects context) ->
        BankObject name
    | otherwise -> StockAddress address

readSized :: Context -> Width -> Bool -> Word32 -> Maybe Word32
readSized context width signed address = case width of
  Word -> wordAt elf address
  Byte -> extend 8 . fromIntegral <$> byteAt elf address
  Half -> do
    low <- byteAt elf address
    high <- byteAt elf (address + 1)
    pure (extend 16 (fromIntegral low .|. (fromIntegral high `shiftL` 8)))
  Dual -> Nothing
 where
  elf = contextElf context
  extend :: Int -> Word32 -> Word32
  extend bits value
    | signed && testBit value (bits - 1) = value .|. complement ((1 `shiftL` bits) - 1)
    | otherwise = value

-- * Value arithmetic

-- | `base + index` with base the pointer side (Rn of an address or ADD).
offsetFrom :: Context -> Abstract -> Abstract -> Abstract
offsetFrom context base index = case (base, index) of
  (StackOffset o, _) | Just offsets <- signedOffsets index -> stackAt (map (o +) offsets)
  (StackSpan lo hi, _) | Just offsets <- signedOffsets index -> stackAt (map (lo +) offsets ++ map (hi +) offsets)
  (StackOffset _, _) | scalar index -> Derived (Index StackFrame)
  (StackSpan _ _, _) | scalar index -> Derived (Index StackFrame)
  (_, StackOffset o) | Just offsets <- signedOffsets base -> stackAt (map (o +) offsets)
  (Derived p, _) | scalar index -> Derived p
  _ | Just v <- pointwise (+) base index -> v
  _
    | Just (blo, bhi) <- bounds base
    , Just (ilo, ihi) <- bounds index
    , toInteger bhi + toInteger ihi <= 0xffffffff ->
        Interval (blo + ilo) (bhi + ihi)
  (Exact single, _) | S.size single == 1 && scalar index -> Derived (Index (locate context (S.findMin single)))
  (_, Derived p) | Just (_, hi) <- bounds base, hi < 0x10000 -> Derived p
  _ -> Unknown

subtractFrom :: Abstract -> Abstract -> Abstract
subtractFrom base index = case (base, index) of
  (StackOffset o, _) | Just offsets <- signedOffsets index -> stackAt (map (o -) offsets)
  (StackSpan lo hi, _) | Just offsets <- signedOffsets index -> stackAt (map (lo -) offsets ++ map (hi -) offsets)
  (StackOffset o, StackOffset q) -> exact (fromIntegral (o - q))
  (Derived p, _) | Just _ <- bounds index -> Derived p
  _ | Just v <- pointwise (-) base index -> v
  _ | Just (lo, hi) <- bounds base, Just [v] <- enumerate index, lo >= v -> Interval (lo - v) (hi - v)
  _ -> Unknown

shiftValue :: Shift -> Int -> Abstract -> Abstract
shiftValue LSL 0 value = value
shiftValue shift amount value = case enumerate value of
  Just xs -> fromSet (S.fromList (map (applyShift shift amount) xs))
  Nothing -> case (shift, bounds value) of
    (LSR, Just (lo, hi)) -> Interval (applyShift LSR amount lo) (applyShift LSR amount hi)
    (LSR, Nothing) -> Interval 0 (applyShift LSR amount maxBound)
    (LSL, Just (lo, hi)) | toInteger hi * 2 ^ amount <= 0xffffffff -> Interval (lo `shiftL` amount) (hi `shiftL` amount)
    _ -> Unknown

-- | An immediate shift; amount 0 encodes 32 for LSR and ASR and RRX for ROR (taken as unknown by callers' sets).
applyShift :: Shift -> Int -> Word32 -> Word32
applyShift shift amount value = case shift of
  LSL -> value `shiftL` amount
  LSR -> if amount == 0 then 0 else value `shiftR` amount
  ASR ->
    let n = if amount == 0 then 31 else amount
     in if testBit value 31 then complement (complement value `shiftR` n) else value `shiftR` n
  ROR -> if amount == 0 then value else (value `shiftR` amount) .|. (value `shiftL` (32 - amount))

dataResult :: Context -> DataOp -> Abstract -> Abstract -> Abstract
dataResult context op a b = case op of
  MOV -> b
  MVN -> fromMaybe Unknown (pointwise (\_ y -> complement y) (exact 0) b)
  ADD -> offsetFrom context a b
  SUB -> subtractFrom a b
  RSB -> subtractFrom b a
  AND -> case (pointwise (.&.) a b, bounds a, bounds b) of
    (Just v, _, _) -> v
    (_, Just (_, ha), Just (_, hb)) -> Interval 0 (min ha hb)
    (_, _, Just (_, hb)) -> Interval 0 hb
    (_, Just (_, ha), _) -> Interval 0 ha
    _ -> Unknown
  ORR -> fromMaybe Unknown (pointwise (.|.) a b)
  EOR -> fromMaybe Unknown (pointwise xor a b)
  BIC -> case (pointwise (\x y -> x .&. complement y) a b, bounds a) of
    (Just v, _) -> v
    (_, Just (_, ha)) -> Interval 0 ha
    _ -> Unknown
  _ -> Unknown

-- * Transfer

data Outcome = Outcome
  { outcomeSuccessors :: [(Word32, State)]
  , outcomeFindings :: [Finding]
  , outcomeStores :: Int
  , outcomeBranches :: Int
  }

always :: Cond
always = Cond 14

registerValue :: Word32 -> State -> Reg -> Abstract
registerValue address _ (Reg 15) = exact (address + 8)
registerValue _ state (Reg n) = fromMaybe Unknown (M.lookup n (registers state))

setRegister :: Reg -> Abstract -> State -> State
setRegister (Reg n) value state =
  state
    { registers = M.insert n value (registers state)
    , comparison = case comparison state of
        Just (Comparison (Reg compared) _ _) | compared == n -> Nothing
        other -> other
    }

-- | Narrows the compared register on a path where `cond` holds after `cmp r, #k`.
-- Nothing when the register's exact values rule out an EQ or NE path.
refine :: Cond -> State -> Maybe State
refine (Cond 14) state = Just state
refine (Cond c) state = case comparison state of
  Nothing -> Just state
  Just (Comparison _ _ False) | c > 1 -> Just state
  Just (Comparison (Reg n) k _)
    | c <= 1
    , Just (Exact values) <- M.lookup n (registers state)
    , if c == 0 then S.notMember k values else values == S.singleton k ->
        Nothing
  Just (Comparison (Reg n) k _) ->
    let current = fromMaybe Unknown (M.lookup n (registers state))
        nonNegative = maybe False ((<= 0x7fffffff) . snd) (bounds current) && k <= 0x7fffffff
        narrowed = case c of
          0 -> Just (exact k)
          1 -> case current of
            Exact values | S.size values > 1 -> Just (fromSet (S.delete k values))
            Interval lo hi | lo == k && lo < hi -> Just (Interval (lo + 1) hi)
            Interval lo hi | hi == k && lo < hi -> Just (Interval lo (hi - 1))
            _ -> Nothing
          2 -> clamp current k maxBound
          3 -> if k == 0 then Nothing else clamp current 0 (k - 1)
          8 -> if k == maxBound then Nothing else clamp current (k + 1) maxBound
          9 -> clamp current 0 k
          10 | nonNegative -> clamp current k maxBound
          11 | nonNegative -> if k == 0 then Nothing else clamp current 0 (k - 1)
          12 | nonNegative -> clamp current (k + 1) maxBound
          13 | nonNegative -> clamp current 0 k
          _ -> Nothing
     in Just (maybe state (\value -> state {registers = M.insert n value (registers state)}) narrowed)
 where
  clamp current lo hi = case current of
    Exact values -> let kept = S.filter (\v -> lo <= v && v <= hi) values in if S.null kept then Nothing else Just (Exact kept)
    Interval a b -> let lo' = max a lo; hi' = min b hi in if lo' <= hi' then Just (Interval lo' hi') else Nothing
    StackOffset _ -> Nothing
    StackSpan _ _ -> Nothing
    ReturnAddress -> Nothing
    _ -> Just (Interval lo hi)

operandValue :: Word32 -> State -> Operand -> Abstract
operandValue _ _ (Immediate value) = exact value
operandValue address state (ShiftedImmediate rm shift amount) = shiftValue shift amount (registerValue address state rm)
operandValue address state (ShiftedRegister rm shift rs) = case enumerate (registerValue address state rs) of
  Just [0] -> registerValue address state rm
  Just [amount] | amount < 32 -> shiftValue shift (fromIntegral amount) (registerValue address state rm)
  _ -> Unknown

-- | One instruction: intra-function successors with their states, and the
-- findings it produces in this state.
execute :: Context -> Function -> Word32 -> Instruction -> State -> Outcome
execute context function address instruction state = case instruction of
  Rejected reason -> (continue state) {outcomeFindings = [violation (ForbiddenInstruction reason) (forbiddenReason reason)]}
  Hint _ -> continue state
  DataProcessing cond op setsFlags rd rn operand
    | op `elem` [TST, TEQ, CMP, CMN] ->
        let compared s = case (op, enumerate (operandValue address s operand)) of
              (CMP, Just [k]) | rn /= pc -> Just (Comparison rn k True)
              (CMN, Just [k]) | rn /= pc -> Just (Comparison rn (negate k) False)
              _ -> Nothing
         in conditional cond (\s -> continue s {comparison = compared s, holds = Nothing}) id
    | rd == pc -> branching cond (\s -> indirectBranch False Nothing (result s) s)
    | otherwise -> conditional cond (\s -> continue (flags setsFlags (setRegister rd (result s) s))) (flags setsFlags)
   where
    result s = dataResult context op (registerValue address s rn) (operandValue address s operand)
  MoveWide cond rd value -> conditional cond (\s -> continue (setRegister rd (exact value) s)) id
  Multiply cond rd rn rm accumulator ->
    let factors s = case (pointwise (*) (registerValue address s rn) (registerValue address s rm), bounds (registerValue address s rn), bounds (registerValue address s rm)) of
          (Just v, _, _) -> v
          (_, Just (alo, ahi), Just (blo, bhi)) | toInteger ahi * toInteger bhi <= 0xffffffff -> Interval (alo * blo) (ahi * bhi)
          _ -> Unknown
        result s = maybe (factors s) (\ra -> offsetFrom context (registerValue address s ra) (factors s)) accumulator
     in conditional cond (\s -> continue (setRegister rd (result s) s)) id
  MoveTop cond rd value ->
    let top s = fromMaybe Unknown (pointwise (\low _ -> (low .&. 0xffff) .|. (value `shiftL` 16)) (registerValue address s rd) (exact 0))
     in conditional cond (\s -> continue (setRegister rd (top s) s)) id
  ZeroExtend cond rd rm rotation bits ->
    let extended s = dataResult context AND (shiftValue ROR rotation (registerValue address s rm)) (exact ((1 `shiftL` bits) - 1))
     in conditional cond (\s -> continue (setRegister rd (extended s) s)) id
  Compute cond defs -> conditional cond (\s -> continue (foldr (`setRegister` Unknown) s defs)) id
  Load cond width signed rt rn offset indexing
    | rt == pc ->
        branching cond $ \s ->
          let (value, found) = loaded s width signed (transferAddress s rn offset indexing)
           in withFindings found (indirectBranch False Nothing value (writeback s rn offset indexing))
    | otherwise ->
        conditional cond
          ( \s ->
              let at = transferAddress s rn offset indexing
                  (value, found) = loaded s width signed at
                  after = writeback s rn offset indexing
                  assigned = case width of
                    Dual -> setRegister (nextReg rt) (fst (loaded s Word False (offsetFrom context at (exact 4)))) (setRegister rt (fst (loaded s Word False at)) after)
                    _ -> setRegister rt value after
               in withFindings found (continue assigned)
          )
          id
  Store cond width rt rn offset indexing ->
    conditional cond
      ( \s ->
          let at = transferAddress s rn offset indexing
              size = widthBytes width
              stored = storeSlots s at size [registerValue address s rt, registerValue address s (nextReg rt)]
           in storing (storeCheck s rn at size) (continue (writeback stored rn offset indexing))
      )
      id
  StoreMultiple cond rn mode wb regs ->
    conditional cond
      ( \s ->
          let (lowest, count, end) = block s rn mode regs
              stored = storeSlots s lowest count (map (registerValue address s) regs)
              updated = if wb then setRegister rn end stored else stored
           in storing (storeCheck s rn lowest count) (continue updated)
      )
      id
  LoadMultiple cond rn mode wb regs ->
    let values s =
          let (lowest, _, _) = block s rn mode regs
           in [(r, fst (loaded s Word False (offsetFrom context lowest (exact (4 * i))))) | (i, r) <- zip [0 ..] regs]
        load s =
          let (_, _, end) = block s rn mode regs
              updated = if wb && rn `notElem` regs then setRegister rn end s else s
           in foldr (\(r, v) acc -> if r == pc then acc else setRegister r v acc) updated (values s)
     in if pc `elem` regs
          then branching cond (\s -> indirectBranch False Nothing (fromMaybe Unknown (lookup pc (values s))) (load s))
          else conditional cond (\s -> continue (load s)) id
  Branch cond link target
    | link -> conditional cond (\s -> branchCount (targetFindings s True target) (continue (afterCall (calleeAt target) s))) id
    | inside target -> branching cond (\s -> Outcome [(target, s)] [] 0 0)
    | otherwise -> branching cond (\s -> branchCount (targetFindings s False target) (Outcome [] [] 0 0))
  BranchExchange cond link rm
    | link ->
        conditional cond
          ( \s ->
              let value = registerValue address s rm
               in (indirectBranch True (Just rm) value s) {outcomeSuccessors = [next (afterCall (calleeOf value) s)]}
          )
          id
    | otherwise -> branching cond (\s -> indirectBranch False (Just rm) (registerValue address s rm) s)
 where
  layout = contextLayout context
  allowlist = contextAllowlist context
  name = functionName function
  violation = Finding address name Violation
  next s = (address + 4, s)
  continue s = Outcome [next s] [] 0 0
  withFindings found outcome = outcome {outcomeFindings = found ++ outcomeFindings outcome}
  storing (found, provenCount) outcome = (withFindings found outcome) {outcomeStores = provenCount}
  branchCount found outcome = (withFindings found outcome) {outcomeBranches = if null found then 1 else 0}
  inside target = functionStart function <= target && target < functionEnd function
  flags setsFlags s = if setsFlags then s {comparison = Nothing, holds = Nothing} else s
  nextReg (Reg n) = Reg (n + 1)
  widthBytes :: Width -> Word32
  widthBytes width = case width of
    Byte -> 1
    Half -> 2
    Word -> 4
    Dual -> 8
  block s rn mode regs =
    let count = fromIntegral (4 * length regs)
        base = registerValue address s rn
        lowest = case mode of
          IncrementAfter -> base
          IncrementBefore -> offsetFrom context base (exact 4)
          DecrementAfter -> subtractFrom base (exact (count - 4))
          DecrementBefore -> subtractFrom base (exact count)
        end = case mode of
          IncrementAfter -> offsetFrom context base (exact count)
          IncrementBefore -> offsetFrom context base (exact count)
          _ -> subtractFrom base (exact count)
     in (lowest, count, end)

  -- A conditional instruction runs on the path where its condition holds and
  -- is skipped on the other; each path carries what it knows about the flags
  -- until an instruction changes them. A path `refine` rules out is dropped.
  conditional :: Cond -> (State -> Outcome) -> (State -> State) -> Outcome
  conditional cond run skip = case known cond of
    Just True -> run state
    Just False -> continue (skip state)
    Nothing ->
      let failed = [next (skip s) | Just s <- [assume (negateCond cond)]]
       in case assume cond of
            Just held -> let taken = run held in taken {outcomeSuccessors = outcomeSuccessors taken ++ failed}
            Nothing -> Outcome failed [] 0 0

  -- A transfer that leaves the straight line: the fallthrough exists only
  -- when the condition can fail.
  branching :: Cond -> (State -> Outcome) -> Outcome
  branching cond run = conditional cond run id

  known cond
    | cond == always || holds state == Just cond = Just True
    | holds state == Just (negateCond cond) = Just False
    | otherwise = Nothing
  assume cond = (\s -> s {holds = maybe (Just cond) Just (holds state)}) <$> refine cond state

  transferAddress s rn offset indexing = case indexing of
    PostIndexed -> registerValue address s rn
    _ -> applyOffset s (registerValue address s rn) offset
  applyOffset s base offset = case offset of
    OffsetImmediate k
      | k >= 0 -> offsetFrom context base (exact (fromIntegral k))
      | otherwise -> subtractFrom base (exact (fromIntegral (negate k)))
    OffsetRegister up rm shift amount ->
      let index = shiftValue shift amount (registerValue address s rm)
       in if up then offsetFrom context base index else subtractFrom base index
  writeback s rn offset indexing = case indexing of
    OffsetOnly -> s
    _ -> setRegister rn (applyOffset s (registerValue address s rn) offset) s

  loaded :: State -> Width -> Bool -> Abstract -> (Abstract, [Finding])
  loaded s width signed at = case at of
    StackOffset o
      | width == Word -> (fromMaybe (if o >= 0 then Derived (CallerFrame o) else Unknown) (M.lookup o (slots s)), [])
      | otherwise -> (narrow, [])
    StackSpan _ _ -> (narrow, [])
    Derived p -> (if width == Word then deref p else narrow, [])
    _ -> case (enumerate at, bounds at) of
      (_, Just (lo, hi)) | regionOf layout lo == DeviceSpace || regionOf layout hi == DeviceSpace -> (Unknown, [violation DeviceLoad ("load from " ++ hex lo ++ " in " ++ regionName DeviceSpace)])
      (Just addresses, _)
        | all ((== BankCode) . regionOf layout) addresses
        , Just values <- mapM (readSized context width signed) addresses ->
            (fromSet (S.fromList values), [])
        | width == Word
        , (a : rest) <- addresses
        , all ((== locate context a) . locate context) rest ->
            (Derived (Cell (locate context a)), [])
      _ -> (narrow, [])
   where
    narrow = case (width, signed) of
      (Byte, False) -> Interval 0 0xff
      (Half, False) -> Interval 0 0xffff
      _ -> Unknown
  deref p = if derefDepth p >= 3 then Unknown else Derived (Deref p)
  derefDepth :: Provenance -> Int
  derefDepth (Deref p) = 1 + derefDepth p
  derefDepth _ = 0

  -- Stack slots hold whole words; a narrower store forgets the slots it touches.
  storeSlots :: State -> Abstract -> Word32 -> [Abstract] -> State
  storeSlots s (StackOffset o) size values =
    let end = o + fromIntegral size
        cleared = M.filterWithKey (\k _ -> k + 4 <= o || k >= end) (slots s)
        written
          | size `mod` 4 == 0 = M.union (M.fromList (zip [o, o + 4 .. end - 4] values)) cleared
          | otherwise = cleared
     in s {slots = written}
  storeSlots s (StackSpan lo hi) size _
    | hi + fromIntegral size <= 0 && not (overlapsSaved s lo (hi + fromIntegral size)) =
        s {slots = M.filterWithKey (\k _ -> k + 4 <= lo || k >= hi + fromIntegral size) (slots s)}
  storeSlots s _ _ _ = s

  -- Whether [lo, end) of the frame holds the return address or a register
  -- this function saved on entry.
  overlapsSaved s lo end = any (\(k, v) -> k + 4 > lo && k < end && saved v) (M.toList (slots s))
  saved v = case v of
    ReturnAddress -> True
    Derived (Argument (Reg n)) -> n >= 4
    _ -> False

  -- Findings and the number of proven stores (0 or 1).
  storeCheck :: State -> Reg -> Abstract -> Word32 -> ([Finding], Int)
  storeCheck s rn at size = case at of
    StackOffset o -> frame o o
    StackSpan lo hi
      | overlapsSaved s lo (hi + fromIntegral size) -> ([violation StackStoreOverSaved ("store through " ++ regName rn ++ " may overwrite the saved registers or return address (entry sp" ++ showSigned lo ++ ".." ++ showSigned (hi + fromIntegral size - 1) ++ ")")], 0)
      | otherwise -> frame lo hi
    Derived p -> (unproven StoreAccess ("store through " ++ regName rn) p, 0)
    _ -> case (enumerate at, bounds at) of
      (Just addresses, _) -> collect (concatMap (\a -> storeRange a (a + size - 1)) addresses)
      (_, Just (lo, hi)) | toInteger hi + toInteger size - 1 <= 0xffffffff -> collect (storeRange lo (hi + size - 1))
      _ -> ([violation UnboundedStore ("store through " ++ regName rn ++ " to an unbounded address" ++ describe (registerValue address s rn))], 0)
   where
    collect found = (found, if null found then 1 else 0)
    frame lo hi
      | hi + fromIntegral size <= 0 = ([], 1)
      | otherwise = ([violation StackStoreBeyondFrame ("store through " ++ regName rn ++ " reaches the caller's frame (entry sp" ++ showSigned lo ++ ".." ++ showSigned (hi + fromIntegral size - 1) ++ ")")], 0)
    showSigned o = if o < 0 then show o else '+' : show o
  storeRange lo hi
    | regionOf layout lo == regionOf layout hi && writable (regionOf layout lo) = []
    | regionOf layout lo == StockMemory
    , any (\st -> stockStoreAddress st <= lo && toInteger hi < toInteger (stockStoreAddress st) + toInteger (stockStoreBytes st)) (stockStores allowlist) =
        []
    | otherwise =
        let region = if writable (regionOf layout lo) then regionOf layout hi else regionOf layout lo
         in [violation (StoreOutside lo hi region) ("store to " ++ hex lo ++ ".." ++ hex hi ++ " in " ++ regionName region ++ (if region == StockMemory then " not declared in stock_stores" else ""))]

  describe value = case value of
    Derived p -> " (" ++ showProvenance p ++ ")"
    Interval lo hi -> " (" ++ hex lo ++ ".." ++ hex hi ++ ")"
    Exact values -> " (" ++ show (S.size values) ++ " values " ++ hex (S.findMin values) ++ ".." ++ hex (S.findMax values) ++ ")"
    StackOffset _ -> " (stack)"
    StackSpan _ _ -> " (stack)"
    _ -> " (no provenance)"

  unproven :: Access -> String -> Provenance -> [Finding]
  unproven access what p =
    case find (\d -> declarationFunction d == name && declarationAccess d == access && declarationThrough d == p) (declarations allowlist) of
      Just d -> [Finding address name (DeclaredBy (declarationReason d)) (Unproven access p) (what ++ " (" ++ showProvenance p ++ ")")]
      Nothing -> [violation (Unproven access p) (what ++ " (" ++ showProvenance p ++ ") is not bounded and not declared")]

  -- A transfer out of this function to `target`.
  targetFindings :: State -> Bool -> Word32 -> [Finding]
  targetFindings s call target
    | target .&. 3 /= 0 = [violation (TargetMisaligned target) (verb ++ hex target ++ " is not an ARM-state word address")]
    | M.member target (bankFunctions context) = obligations s target
    | not (stockCode (regionOf layout target)) = [violation (TargetNotEntry target (regionOf layout target)) (verb ++ hex target ++ " in " ++ regionName (regionOf layout target) ++ " is not a bank function entry")]
    | any ((== target) . stockCallAddress) (stockCalls allowlist) = []
    | otherwise = [violation (UndeclaredStockTarget target) (verb ++ "stock " ++ hex target ++ " not declared in stock_calls")]
   where
    verb = if call then "call to " else "branch to "

  -- A bank callee declared to store or branch through an argument register,
  -- or through words loaded from it, puts the same requirement on this
  -- caller's value of that register.
  obligations s target = case M.lookup target (bankFunctions context) of
    Nothing -> []
    Just callee ->
      concat
        [ obligation s (declarationAccess d) callee r depth
        | d <- declarations allowlist
        , declarationFunction d == functionName callee
        , Just (r, depth) <- [argumentRoot (declarationThrough d)]
        ]
  obligation s access callee r depth =
    let value = registerValue address s r
        what =
          "passes " ++ regName r ++ " to " ++ functionName callee ++ ", which "
            ++ (if access == StoreAccess then "stores" else "branches")
            ++ (if depth == 0 then " through it," else " through a word loaded from it,")
     in case value of
          Derived p -> unproven access what (derefs depth p)
          _
            | value == exact 0 -> zeroArgument callee r what
            | depth == 0 && access == StoreAccess && provenPointer value -> []
            | depth == 0 && access == BranchAccess, Just targets <- enumerate value -> concatMap (targetFindings s True) targets
            | depth > 0, Just (lo, _) <- bounds value -> unproven access what (derefs (depth - 1) (Cell (locate context lo)))
            | otherwise -> [violation UnprovenArgument (what ++ " with an unproven value" ++ describe value)]
  -- A 0 argument passes when the callee, analysed from an entry with that
  -- argument 0, has no violation besides code that analysis does not reach.
  zeroArgument callee r what
    | (functionStart callee, r) `notElem` zeroArguments context
    , let nested = context {zeroArguments = (functionStart callee, r) : zeroArguments context}
          (found, _, _, _) = analyse nested (setRegister r (exact 0) entryState) callee
    , all (\f -> findingVerdict f /= Violation || findingReason f == Unreached) found =
        []
    | otherwise = [violation UnprovenArgument (what ++ " with 0, and the callee analysed with that argument 0 has a violation")]
  provenPointer value = case value of
    StackOffset _ -> True
    StackSpan _ _ -> True
    _ -> case bounds value of
      Just (lo, hi) -> writable (regionOf layout lo) && regionOf layout hi == regionOf layout lo
      Nothing -> False
  argumentRoot p = case p of
    Argument r -> Just (r, 0 :: Int)
    Deref inner -> fmap (fmap (+ 1)) (argumentRoot inner)
    _ -> Nothing
  derefs :: Int -> Provenance -> Provenance
  derefs 0 p = p
  derefs n p = Deref (derefs (n - 1) p)

  -- A transfer through a computed or loaded value.
  indirectBranch :: Bool -> Maybe Reg -> Abstract -> State -> Outcome
  indirectBranch call register value s = case value of
    ReturnAddress | not call -> Outcome [] [] 0 1
    Derived p -> Outcome [] (unproven BranchAccess (kind ++ through) p) 0 0
    _ -> case enumerate value of
      Just targets ->
        let local = [t | t <- targets, not call, inside t]
            remote = [t | t <- targets, call || not (inside t)]
            stray = [violation (TargetNotCode t) ("branch to " ++ hex t ++ " lands outside the function's code") | t <- local, not (S.member t (codeWords context))]
         in branchCount (concatMap (targetFindings s call) remote ++ stray) (Outcome [(t, s) | t <- local, S.member t (codeWords context)] [] 0 0)
      Nothing -> Outcome [] [violation UnboundedBranch (kind ++ through ++ " is not bounded" ++ describe value)] 0 0
   where
    kind = if call then "call" else "branch"
    through = maybe " through a loaded program counter" ((" through " ++) . regName) register

  calleeAt target = Just (maybe (StockFunction target) (BankFunction . functionName) (M.lookup target (bankFunctions context)))
  calleeOf value = case enumerate value of
    Just [target] -> calleeAt target
    _ -> Nothing
  afterCall callee s =
    let clobbered = foldr (`setRegister` Unknown) s [Reg 1, Reg 2, Reg 3, Reg 12, lr]
     in (setRegister (Reg 0) (maybe Unknown (Derived . Result) callee) clobbered) {comparison = Nothing, holds = Nothing}

showProvenance :: Provenance -> String
showProvenance p = case p of
  Argument r -> "entry value of " ++ regName r
  CallerFrame o -> "word at entry sp+" ++ show o
  Cell location -> "word loaded from " ++ showLocation location
  Result callee -> "result of " ++ showCallee callee
  Index location -> "unbounded index into " ++ showLocation location
  Deref inner -> "word loaded through " ++ showProvenance inner
 where
  showLocation location = case location of
    BankObject object -> object
    GeneratedData -> "generated data"
    StackFrame -> "the stack frame"
    StockAddress a -> hex a
  showCallee callee = case callee of
    BankFunction function -> function
    StockFunction a -> hex a

-- * Driver

-- | Checks the bank part of a relinked RAM-push ELF against the allowlist.
checkBank :: Allowlist -> Elf -> Either String Report
checkBank allowlist elf = do
  layout <- readLayout elf
  let symbols = elfSymbols elf
      inText address = textStart layout <= address && address < textEnd layout
      named =
        sortOn functionStart
          [ Function (symbolName s) (symbolValue s) (symbolValue s + symbolSize s)
          | s <- symbols
          , symbolKind s == FunctionSymbol
          , symbolSize s > 0
          , inText (symbolValue s)
          ]
      covered a = any (\f -> functionStart f <= a && a < functionEnd f) named
      mapping = sortOn fst [(symbolValue s, symbolName s) | s <- symbols, symbolName s `elem` ["$a", "$d", "$t"], inText (symbolValue s)]
      boundaries = S.fromList (map fst mapping ++ map functionStart named ++ [textEnd layout])
      armRanges = [(start, end) | ((start, kind), end) <- zip mapping (map fst (drop 1 mapping) ++ [textEnd layout]), kind == "$a"]
      armWord a = any (\(start, end) -> start <= a && a < end) armRanges
      -- Code an `$a` symbol starts that no function symbol covers (a weak
      -- definition the linker kept without its symbol).
      anonymous =
        [ Function ("<code after " ++ maybe "the bank start" functionName (listToMaybe (reverse (filter ((<= start) . functionStart) named))) ++ ">") start end
        | (start, kind) <- mapping
        , kind == "$a"
        , not (covered start)
        , let end = fromMaybe (textEnd layout) (S.lookupGT start boundaries)
        ]
      functions = sortOn functionStart (named ++ anonymous)
      code = S.fromList [a | f <- functions, a <- [functionStart f, functionStart f + 4 .. functionEnd f - 4], armWord a]
      objects =
        [ (symbolValue s, symbolValue s + symbolSize s, symbolName s)
        | s <- symbols
        , symbolKind s `elem` [ObjectSymbol, FunctionSymbol]
        , symbolSize s > 0
        , symbolValue s >= textStart layout && symbolValue s < imageEnd layout
        ]
      context = Context elf layout allowlist (M.fromList [(functionStart f, f) | f <- named]) code objects []
      overlapping =
        [ Finding (functionStart g) "<layout>" Violation LayoutProblem ("function " ++ functionName g ++ " overlaps " ++ functionName f)
        | (f, g) <- zip functions (drop 1 functions)
        , functionStart g < functionEnd f
        ]
      perFunction = map (analyse context entryState) functions
      undeclaredNames =
        [ Finding 0 (declarationFunction d) Violation UnknownDeclaredFunction "unproven declaration names no bank function"
        | d <- declarations allowlist
        , declarationFunction d `notElem` map functionName named
        ]
      stockInAllocation =
        [ Finding (stockCallAddress c) "<allowlist>" Violation (TargetNotEntry (stockCallAddress c) (regionOf layout (stockCallAddress c))) ("stock call " ++ stockCallName c ++ " lies in " ++ regionName (regionOf layout (stockCallAddress c)))
        | c <- stockCalls allowlist
        , not (stockCode (regionOf layout (stockCallAddress c)))
        ]
      findings = layoutFindings elf layout ++ overlapping ++ undeclaredNames ++ stockInAllocation ++ concatMap (\(found, _, _, _) -> found) perFunction
      used d =
        any
          (\f -> findingFunction f == declarationFunction d && findingVerdict f /= Violation && findingReason f == Unproven (declarationAccess d) (declarationThrough d))
          findings
  when (null named) (Left "bank: no function symbols in bank code")
  pure
    Report
      { reportFindings = findings
      , reportUnused = filter (not . used) (declarations allowlist)
      , reportProvenStores = sum [n | (_, n, _, _) <- perFunction]
      , reportProvenBranches = sum [n | (_, _, n, _) <- perFunction]
      , reportInstructions = sum [n | (_, _, _, n) <- perFunction]
      , reportFunctions = length functions
      }

-- | Fixpoint over one function's control-flow graph from `entry`, with a state per
-- address and known flag condition, then one findings pass over the final
-- states. An address's store or branch counts as proven when it is proven
-- in every state that reaches it.
analyse :: Context -> State -> Function -> ([Finding], Int, Int, Int)
analyse context entry function =
  ( unreached ++ concatMap (\(found, _, _) -> found) perAddress
  , sum [n | (_, n, _) <- perAddress]
  , sum [n | (_, _, n) <- perAddress]
  , length instructions
  )
 where
  instructions =
    [ (a, decode a w)
    | a <- S.toList (codeWords context)
    , functionStart function <= a
    , a < functionEnd function
    , Just w <- [wordAt (contextElf context) a]
    ]
  table = M.fromList instructions
  thresholds =
    S.toList . S.fromList $
      [0xff, 0xffff, 0x7fffffff, maxBound]
        ++ concat [[k - 1, k, k + 1] | (_, DataProcessing _ CMP _ _ _ (Immediate k)) <- instructions, k > 0, k < maxBound]
  start = (functionStart function, Nothing)
  states = fixpoint (M.singleton start entry) (M.singleton start (0 :: Int)) [start]
  fixpoint known _ [] = known
  fixpoint known visits (key@(a, _) : rest) = case (M.lookup a table, M.lookup key known) of
    (Just instruction, Just s) ->
      let (known', visits', queued) = foldl (propagate a) (known, visits, rest) (outcomeSuccessors (execute context function a instruction s))
       in fixpoint known' visits' queued
    _ -> fixpoint known visits rest
  -- Widening applies on back edges only, so loop heads absorb the growth.
  propagate source (known, visits, queue) (target, s)
    | M.notMember target table = (known, visits, queue)
    | otherwise = case M.lookup key known of
        Nothing -> (M.insert key s known, M.insert key 1 visits, queue ++ [key])
        Just old ->
          let count = fromMaybe 0 (M.lookup key visits)
              joined = joinState (locate context) old s
              updated = if target <= source && count >= 3 then widenState (locate context) thresholds old joined else joined
           in if updated == old
                then (known, visits, queue)
                else (M.insert key updated known, M.insert key (count + 1) visits, queue ++ [key | key `notElem` queue])
   where
    key = (target, holds s)
  perAddress =
    [ (nub (concatMap outcomeFindings outcomes), minimum (map outcomeStores outcomes), minimum (map outcomeBranches outcomes))
    | (a, instruction) <- instructions
    , let outcomes = [execute context function a instruction s | ((b, _), s) <- M.toList states, b == a]
    , not (null outcomes)
    ]
  reached = S.fromList (map fst (M.keys states))
  unreached =
    [ Finding a (functionName function) Violation Unreached "instruction is not reached from the function entry"
    | (a, instruction) <- instructions
    , S.notMember a reached
    , instruction /= Hint always
    ]

checkBankFile :: B.ByteString -> B.ByteString -> Either String Report
checkBankFile allowlistBytes elfBytes = do
  allowlist <- parseAllowlist allowlistBytes
  elf <- parseElf elfBytes
  checkBank allowlist elf
