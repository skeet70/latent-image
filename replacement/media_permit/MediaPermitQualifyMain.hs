{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}

module Main where

import Control.Exception (Exception, IOException, SomeException, displayException, fromException, throwIO, try)
import Control.Monad (forM_, unless, when)
import qualified ArmElf as E
import Data.Aeson (Value (Null), encode, object, (.=))
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import Data.List (intercalate, isInfixOf, isPrefixOf, isSuffixOf)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.CachedNativeBatch (BatchReuse (..), CacheEscape, ResultPolicy (WriteAllResults), cachedObservedBatchWith, defaultNativeCacheRoot)
import Emulation.NativeCase (CaseId (..))
import qualified Emulation.NativeObservation as N
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host hiding (sha256)
import FujiAF.JSON (encodeCanonical)
import FujiModNativePaths (fujimodNativeCase)
import Numeric (showHex)
import StockInventory (FunctionDeclaration (..), FunctionDerivation (..), FunctionSize (..), GuestAddress (..), PremiseUse (..), ProgramCounter (..), checkedStockFunctions, requiredReturnPremises, returnPremiseLabel)
import System.Directory (canonicalizePath, createDirectoryIfMissing, doesDirectoryExist, doesFileExist, getCurrentDirectory, listDirectory, removeFile)
import System.Environment (getArgs, getExecutablePath, lookupEnv, setEnv)
import System.Exit (ExitCode (..), exitWith)
import System.FilePath ((</>), isAbsolute)
import System.Posix.Process (executeFile)

stockFunctionExtent :: FunctionDerivation -> (Word32, Int)
stockFunctionExtent derivation =
  let FunctionDeclaration (GuestAddress address) (FunctionSize size) = derivedDeclaration derivation
   in (address, size)

inventoryFunction :: FunctionDerivation -> Value
inventoryFunction derivation =
  let (address, size) = stockFunctionExtent derivation
   in object
        [ "address" .= hex address
        , "size" .= size
        , "required_premises" .= map returnPremiseLabel (derivationPremises derivation)
        , "premise_uses" .=
            [object ["pc" .= hex pc, "premise" .= returnPremiseLabel premise] | PremiseUse (ProgramCounter pc) premise <- derivationPremiseUses derivation]
        ]

globalsBase, globalsSize, framesBase, framesSize, stackBase, stackSize, stackTop :: Word32
globalsBase = 0x01500000
globalsSize = 0xC0000
framesBase = 0x01600000
framesSize = 0x326c * 8
stackBase = 0x009a0000
stackSize = 0x8000
stackTop = stackBase + stackSize - 0x10

candidateBase, candidateSize, exitAddress :: Word32
candidateBase = 0x00900000
candidateSize = 0x8000
exitAddress = 0x00907ff0

senderBase, senderSize, frameBase, frameSize, midBase, midSize, ctxBase, ctxSize :: Word32
senderBase = 0x00e67000
senderSize = 0x1000
frameBase = 0x00e73000
frameSize = 0x00e86c00 - frameBase
midBase = 0x00e86d00
midSize = 0x00e87264 - midBase
ctxBase = 0x00e87ad4
ctxSize = 0x40

frameTwoBase, frameTwoSize, bitBase, bitSize :: Word32
frameTwoBase = 0x00e88400
frameTwoSize = 0x00e8d000 - frameTwoBase
bitBase = 0x00eb0000
bitSize = 0x1000

sideABase, sideASize, fatalTwoBase, fatalTwoSize, sideBBase, sideBSize :: Word32
sideABase = 0x00e87404
sideASize = 0x200
fatalTwoBase = 0x00e87944
fatalTwoSize = 0x100
sideBBase = 0x00e88000
sideBSize = 0x400

commandBase, commandSize, cancelBase, cancelSize :: Word32
commandBase = 0x00e8d000
commandSize = 0x1000
cancelBase = 0x00e6d000
cancelSize = 0x2000

lowBase, lowSize, resolveEntry, applyEntry, entrySpan :: Word32
lowBase = 0x00720000
lowSize = 0x7000
resolveEntry = 0x00900400
applyEntry = 0x00900800
entrySpan = 0x400

countCell, countArmed :: Word32
countCell = 0x015b8300
countArmed = 0x015b8304

mirrorBase, mirrorWord0, mirrorWord1, bitmapBase, minimaBase, activeBase :: Word32
mirrorBase = 0x015ad430
mirrorWord0 = 0x015ad43c
mirrorWord1 = 0x015ad440
bitmapBase = 0x01596918
minimaBase = 0x01581f60
activeBase = 0x015ad53c

traceCommands, traceCancels, traceFaults, traceHandbackCount, traceHandbackClass, traceLogs :: Word32
traceCommands = 0x015b8000
traceCancels = 0x015b8100
traceFaults = 0x015b8200
traceHandbackClass = 0x015b8204
traceHandbackCount = 0x015b8208
traceLogs = 0x015b820c

hex :: Word32 -> String
hex value = "0x" ++ replicate (8 - length digits) '0' ++ digits where digits = showHex value ""

u32le :: Word32 -> B.ByteString
u32le value =
  B.pack
    [ fromIntegral value
    , fromIntegral (value `shiftR` 8)
    , fromIntegral (value `shiftR` 16)
    , fromIntegral (value `shiftR` 24)
    ]

u16le :: Word32 -> B.ByteString
u16le value = B.pack [fromIntegral value, fromIntegral (value `shiftR` 8)]

putU32 :: Int -> Word32 -> B.ByteString -> B.ByteString
putU32 offset value bytes
  | offset < 0 || offset + 4 > B.length bytes = error ("word write outside region at " ++ show offset)
  | otherwise = B.take offset bytes <> u32le value <> B.drop (offset + 4) bytes

putU16 :: Int -> Word32 -> B.ByteString -> B.ByteString
putU16 offset value bytes
  | offset < 0 || offset + 2 > B.length bytes = error ("halfword write outside region at " ++ show offset)
  | otherwise = B.take offset bytes <> u16le value <> B.drop (offset + 2) bytes

readU32 :: B.ByteString -> Int -> Either String Word32
readU32 bytes offset
  | offset < 0 || offset + 4 > B.length bytes = Left ("read outside region at " ++ show offset)
  | otherwise =
      Right
        ( fromIntegral (B.index bytes offset)
            .|. (fromIntegral (B.index bytes (offset + 1)) `shiftL` 8)
            .|. (fromIntegral (B.index bytes (offset + 2)) `shiftL` 16)
            .|. (fromIntegral (B.index bytes (offset + 3)) `shiftL` 24)
        )

word :: B.ByteString -> Word32 -> Word32
word bytes address =
  case readU32 bytes (fromIntegral (address - globalsBase)) of
    Right value -> value
    Left problem -> error problem

frameWord :: B.ByteString -> Word32 -> Word32
frameWord bytes address =
  case readU32 bytes (fromIntegral (address - framesBase)) of
    Right value -> value
    Left problem -> error problem

pad :: Int -> B.ByteString -> B.ByteString
pad size bytes
  | B.length bytes > size = error ("region bytes exceed declared size: " ++ show (B.length bytes) ++ " > " ++ show size)
  | otherwise = bytes <> B.replicate (size - B.length bytes) 0

data Kind = K0 | K1 | K2 | K3 | K4 deriving (Eq, Show)

kindWord :: Kind -> Word32
kindWord K0 = 0
kindWord K1 = 1
kindWord K2 = 2
kindWord K3 = 3
kindWord K4 = 4

data Family = Resolver | Handler | Message deriving (Eq, Show)

data Layout = Primary | Secondary | KindLayout deriving (Eq, Show)

data Effects = Effects
  { eBitmap :: [Word32]
  , eCapture :: [Word32]
  , ePermit :: [Word32]
  , eMinima :: (Word32, Word32, Word32)
  , eActive :: Word32
  , eCommands :: [(Word32, Word32)]
  , eCancels :: [(Word32, Word32)]
  , eHandback :: (Word32, Word32)
  , eFaults :: Word32
  , eMirror :: (Word32, Word32, Word32)
  }
  deriving (Eq, Show)

data Scenario = Scenario
  { scId :: String
  , scFamily :: Family
  , scLayout :: Layout
  , scKey :: Word32
  , scKinds :: [(Int, Kind)]
  , scGroups :: [(Int, Word32)]
  , scKeySlots :: [Int]
  , scWord0 :: Word32
  , scWord1 :: Word32
  , scMin0 :: Word32
  , scResolve :: Maybe Word32
  , scCountModel :: Bool
  , scExpect :: Effects
  }

slotCount :: Int
slotCount = 8

inspectedSlots :: [Int]
inspectedSlots = [0 .. slotCount - 1]

ctxAddress :: Int -> Word32
ctxAddress slot = framesBase + fromIntegral slot * 0x326c

listAddress :: Int -> Word32
listAddress slot = 0x0157bdf0 + fromIntegral (slot + 4) * 8

keyAddress :: Layout -> Kind -> Int -> Word32
keyAddress layout kind slot = base + fromIntegral (slot * 0x1d0) + fromIntegral field
  where
    base :: Word32
    base = case layout of
      Primary -> 0x01596800
      Secondary -> 0x015967fc
      KindLayout -> 0x01596804
    field :: Int
    field = case kind of
      K0 -> 0
      K2 -> 0
      K1 -> 8
      K3 -> -4
      K4 -> -4

kindAt :: Scenario -> Int -> Kind
kindAt scenario slot = maybe K0 id (lookup slot (scKinds scenario))

initialGlobals :: Scenario -> B.ByteString
initialGlobals scenario = putU16 (fromIntegral (mirrorBase - globalsBase)) 0x1367 (writes values)
  where
    writes entries = foldl (\bytes (address, value) -> putU32 (fromIntegral (address - globalsBase)) value bytes) (B.replicate (fromIntegral globalsSize) 0) entries
    values =
      [ (0x0157dd34, if scLayout scenario == Primary then 1 else 0)
      , (0x0157dd35, if scLayout scenario == Secondary then 1 else 0)
      , (0x0157dd3c, framesBase)
      , (minimaBase, scMin0 scenario)
      , (minimaBase + 4, scMin0 scenario)
      , (minimaBase + 8, scMin0 scenario)
      , (activeBase, 0xffffffff)
      , (countCell, if scCountModel scenario then 3 else 0)
      , (countArmed, if scCountModel scenario then 1 else 0)
      ]
        ++ [ (listAddress slot, fromIntegral slot) | (slot, _) <- scKinds scenario ]
        ++ [ (listAddress slot + 4, kindWord kind) | (slot, kind) <- scKinds scenario ]
        ++ [ (listAddress terminator, 0xffffffff), (listAddress terminator + 4, 0) ]
        ++ [ (keyAddress (scLayout scenario) (kindAt scenario slot) slot, scKey scenario) | slot <- scKeySlots scenario ]
        ++ [ (mirrorBase + 4, 8), (mirrorWord0, scWord0 scenario), (mirrorWord1, scWord1 scenario) ]
    terminator = case map fst (scKinds scenario) of
      [] -> 0
      slots -> maximum slots + 1

initialFrames :: Scenario -> B.ByteString
initialFrames scenario = writes (slots ++ kinds)
  where
    writes values = foldl (\bytes (address, value) -> putU32 (fromIntegral (address - framesBase)) value bytes) (B.replicate (fromIntegral framesSize) 0) values
    slots =
      [ (ctxAddress slot + 0x27c8, 0xdead) | slot <- inspectedSlots ]
        ++ [ (ctxAddress slot + 0x2fb0, 0xbeef) | slot <- inspectedSlots ]
    kinds = concat
      [ let address = ctxAddress slot
            (flag, state) = case kind of
              K0 -> (0, 0)
              K1 -> (1, 0)
              K2 -> (0, 0x19)
              K3 -> (0, 0x1d)
              K4 -> (0, 0x2c)
            group = maybe 0 id (lookup slot (scGroups scenario))
         in [(address + 0x4a8, flag), (address + 0x4b4, state), (address + 0x2fa0, group)]
      | (slot, kind) <- scKinds scenario
      ]

emptyEffects :: Effects
emptyEffects =
  Effects
    { eBitmap = replicate slotCount 0
    , eCapture = replicate slotCount 0xdead
    , ePermit = replicate slotCount 0xbeef
    , eMinima = (0, 0, 0)
    , eActive = 0xffffffff
    , eCommands = []
    , eCancels = []
    , eHandback = (0, 0)
    , eFaults = 0
    , eMirror = (0, 0, 0)
    }

defaultEffects :: Scenario -> Effects
defaultEffects scenario =
  emptyEffects
    { eMinima = (scMin0 scenario, scMin0 scenario, scMin0 scenario)
    , eMirror = (0x1367, scWord0 scenario, scWord1 scenario)
    }

setBit :: Int -> Effects -> Effects
setBit slot effects = effects {eBitmap = replace slot 0x40000000 (eBitmap effects)}

setCapture :: Int -> Word32 -> [Word32] -> [Word32]
setCapture slot value values = replace slot value values

setPermit :: Int -> Word32 -> [Word32] -> [Word32]
setPermit slot value values = replace slot value values

replace :: Int -> a -> [a] -> [a]
replace index value values = take index values ++ [value] ++ drop (index + 1) values

resolverCase :: String -> Layout -> Word32 -> Int -> [(Int, Kind)] -> Maybe Word32 -> Scenario
resolverCase ident layout key target extra expected =
  let missing = expected == Just 0xffffffff
      keySlots = if missing then [] else [target]
      scenario = Scenario ident Resolver layout key extra [] keySlots 0 key 0x1000 expected False emptyEffects
      faults = if missing then 1 else 0
   in scenario {scExpect = (defaultEffects scenario) {eFaults = faults}}

handlerCase :: String -> Word32 -> Word32 -> [(Int, Kind)] -> [(Int, Word32)] -> (Effects -> Effects) -> Scenario
handlerCase ident word0 key kinds groups modify =
  let scenario = Scenario ident Handler KindLayout key kinds groups [0] word0 key 0x1000 Nothing (ident == "kind1-count-drop") emptyEffects
   in scenario {scExpect = modify (defaultEffects scenario)}

messageCase :: String -> Layout -> Word32 -> Word32 -> [(Int, Kind)] -> [(Int, Word32)] -> [Int] -> (Effects -> Effects) -> Scenario
messageCase ident layout word0 key kinds groups keySlots modify =
  let scenario = Scenario ident Message layout key kinds groups keySlots word0 key 0x1000 Nothing False emptyEffects
   in scenario {scExpect = modify (defaultEffects scenario)}

scenarioById :: String -> Scenario
scenarioById ident =
  case [scenario | scenario <- scenarios, scId scenario == ident] of
    (scenario : _) -> scenario
    [] -> error ("missing scenario " ++ ident)

scenarios :: [Scenario]
scenarios =
  [ resolverCase "primary-slot0" Primary 0x40 0 [] (Just 0)
  , resolverCase "primary-slot3" Primary 0x43 3 [] (Just 3)
  , resolverCase "secondary-slot2" Secondary 0x52 2 [] (Just 2)
  , resolverCase "kind-slot0" KindLayout 0x60 0 [(0, K0)] (Just 0)
  , resolverCase "kind-slot1" KindLayout 0x61 1 [(1, K1)] (Just 1)
  , resolverCase "kind-slot3" KindLayout 0x63 3 [(3, K3)] (Just 3)
  , resolverCase "primary-unknown" Primary 0x7ff 0 [] (Just 0xffffffff)
  , resolverCase "primary-bound-slot200" Primary 0x5000 200 [] (Just 200)
  , handlerCase "kind0-zero" 0 0x70 [(0, K0)] [] (\e -> (setBit 0 e) {eCommands = [(0, 0)], eActive = 0, eCapture = replace 0 0 (eCapture e)})
  , handlerCase "kind0-nonzero" 7 0x71 [(0, K0)] [] (\e -> (setBit 0 e) {eCommands = [(0, 0)], eActive = 0})
  , handlerCase "kind1-subframes" 4 0x72 [(0, K1), (1, K1), (2, K1)] [(0, 0x700), (1, 0x700), (2, 0x700)]
      (\e -> (setBit 0 (setBit 1 (setBit 2 e))) {eCommands = [(2, 0), (1, 0), (0, 0)], eActive = 0, ePermit = setPermit 0 4 (setPermit 1 4 (setPermit 2 4 (ePermit e))), eMinima = (4, 4, 4)})
  , handlerCase "kind1-large-word" 0x2000 0x73 [(0, K1), (1, K1)] [(0, 0x700), (1, 0x700)]
      (\e -> (setBit 0 (setBit 1 e)) {eCommands = [(1, 0), (0, 0)], eActive = 0, ePermit = setPermit 0 0x2000 (setPermit 1 0x2000 (ePermit e))})
  , handlerCase "kind1-count-guard" 1 0x74 [(0, K1), (1, K1), (2, K1)] [(0, 0x700), (1, 0x700), (2, 0x700)]
      (\e -> (setBit 0 (setBit 1 (setBit 2 e))) {eCommands = [(2, 0), (1, 0), (0, 0)], eActive = 0, ePermit = setPermit 0 1 (setPermit 1 1 (setPermit 2 1 (ePermit e)))})
  , handlerCase "kind1-count-one" 2 0x79 [(0, K1), (1, K1)] [(0, 0x700), (1, 0x701)]
      (\e -> (setBit 0 e) {eCommands = [(0, 0)], eActive = 0, ePermit = setPermit 0 2 (ePermit e), eMinima = (2, 2, 2)})
  , handlerCase "kind1-count-drop" 4 0x7a [(0, K1), (1, K1), (2, K1)] [(0, 0x700), (1, 0x700), (2, 0x700)]
      (\e -> (setBit 0 (setBit 1 (setBit 2 e))) {eCommands = [(2, 0)], eActive = 2, ePermit = setPermit 0 4 (setPermit 1 4 (setPermit 2 4 (ePermit e))), eMinima = (4, 4, 4)})
  , handlerCase "kind2-broadcast" 0 0x75 [(0, K2), (1, K0), (2, K2)] []
      (\e -> (setBit 0 (setBit 2 e)) {eCancels = [(0, 5), (2, 5)], eCapture = setCapture 0 0 (setCapture 2 0 (eCapture e))})
  , handlerCase "kind3-broadcast" 9 0x76 [(0, K3)] []
      (\e -> (setBit 0 e) {eCancels = [(0, 5)]})
  , handlerCase "kind4-no-effect" 5 0x77 [(0, K4)] [] id
  , messageCase "message-known" KindLayout 0 0x78 [(0, K0)] [] [0]
      (\e -> (setBit 0 e) {eHandback = (10, 1), eActive = 0})
  , messageCase "message-unknown" Primary 0 0x7ff [] [] []
      (\e -> e {eHandback = (10, 1), eFaults = 1})
  ]

selectScenarios :: [String] -> Either String ([Scenario], Bool)
selectScenarios [] = Right (scenarios, False)
selectScenarios names =
  let known = map scId scenarios
      unknown = [name | name <- names, name `notElem` known]
      selected = filter (\scenario -> scId scenario `elem` names) scenarios
   in if null unknown
        then Right (selected, length selected < length scenarios)
        else Left ("unknown scenario " ++ intercalate ", " unknown ++ "; known scenarios: " ++ intercalate ", " known)


data Side = Original | Candidate | DefectCandidate deriving (Eq, Show)

sideLabel :: Side -> String
sideLabel Original = "original"
sideLabel Candidate = "candidate"
sideLabel DefectCandidate = "defect-candidate"

data Inputs = Inputs
  { inCandidate :: B.ByteString
  , inDefect :: B.ByteString
  , inLow :: B.ByteString
  , inSideA :: B.ByteString
  , inFatal2 :: B.ByteString
  , inSideB :: B.ByteString
  , inCommand :: B.ByteString
  , inCancel :: B.ByteString
  , inCount :: B.ByteString
  , inStock :: B.ByteString
  , inFunctions :: [(Word32, Int)]
  , inCountFunctions :: [(Word32, Int)]
  }

newtype Prerequisite = Prerequisite String
  deriving (Show)

instance Exception Prerequisite

prerequisite :: String -> IO a
prerequisite = throwIO . Prerequisite

stockImageSha256 :: String
stockImageSha256 = "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"

exceptionText :: IOException -> String
exceptionText problem =
  let text :: String
      text = displayException problem
      prefix :: String
      prefix = "user error ("
   in if prefix `isPrefixOf` text && ")" `isSuffixOf` text
        then take (length text - length prefix - 1) (drop (length prefix) text)
        else text

readInputFile :: String -> FilePath -> IO B.ByteString
readInputFile label path = do
  present <- doesFileExist path
  unless present (prerequisite ("missing " ++ label ++ " at " ++ path))
  result <- try (B.readFile path)
  either (\problem -> prerequisite ("cannot read " ++ label ++ " at " ++ path ++ ": " ++ exceptionText problem)) pure result

readInput :: FilePath -> FilePath -> IO B.ByteString
readInput root relative = readInputFile ("declared input " ++ relative) (root </> relative)

loadInputs :: FilePath -> FilePath -> IO Inputs
loadInputs build stockElf = do
  candidate <- readInput build "replacement/media_permit/media-permit-candidate.bin"
  defect <- readInput build "replacement/media_permit/media-permit-defect.bin"
  low <- readInput build "replacement/media_permit/services/low.bin"
  sideA <- readInput build "replacement/media_permit/services/side-a.bin"
  fatal2 <- readInput build "replacement/media_permit/services/fatal2.bin"
  sideB <- readInput build "replacement/media_permit/services/side-b.bin"
  command <- readInput build "replacement/media_permit/services/command.bin"
  cancel <- readInput build "replacement/media_permit/services/cancel.bin"
  count <- readInput build "replacement/media_permit/services/count.bin"
  stock <- readInputFile "stock ELF" stockElf
  when (B.null candidate || B.length candidate `mod` 4 /= 0 || B.length candidate > fromIntegral candidateSize) $
    prerequisite "candidate bin is empty, unaligned or exceeds its region"
  when (B.null defect || B.length defect `mod` 4 /= 0 || B.length defect > fromIntegral candidateSize) $
    prerequisite "defect candidate bin is empty, unaligned or exceeds its region"
  when (B.length candidate > fromIntegral (exitAddress - candidateBase)) $
    prerequisite "candidate text reaches the case exit"
  when (B.length defect > fromIntegral (exitAddress - candidateBase)) $
    prerequisite "defect text reaches the case exit"
  when (B.length low > fromIntegral lowSize) $
    prerequisite "low model exceeds its declared region"
  when (any B.null [sideA, fatal2, sideB, command, cancel, count]) $
    prerequisite "service model bin is empty"
  when (B.length count > 0x80) $
    prerequisite "count model exceeds the stock subframe-count span"
  when (B.length stock < elfOffset (bitBase + bitSize)) $
    prerequisite "stock ELF is shorter than the declared regions"
  unless (sha256Hex (sha256 (B.take 5185120 (B.drop 0xe8 stock))) == stockImageSha256) $
    prerequisite "stock ELF does not contain the pinned image-5 bytes"
  when (any (\(address, expected) -> stockWord stock address /= expected) stockEntries) $
    prerequisite "stock ELF does not carry the admitted entry words"
  stockCode <-
    case checkedStockFunctions (B.take 5185120 (B.drop 0xe8 stock)) of
      Left problem -> prerequisite ("stock inventory refused: " ++ problem)
      Right functions -> pure (map stockFunctionExtent functions)
  models <- concat <$> mapM (linkedFunctions build) [LowModel, HandbackModel, FatalLogModel, FatalModel, CommandModel, CancelModel]
  countFunctions <- linkedFunctions build CountModel
  pure
    Inputs
      { inCandidate = candidate
      , inDefect = defect
      , inLow = low
      , inSideA = sideA
      , inFatal2 = fatal2
      , inSideB = sideB
      , inCommand = command
      , inCancel = cancel
      , inCount = count
      , inStock = stock
      , inFunctions = stockCode ++ models
      , inCountFunctions = countFunctions
      }

data ServiceModel = LowModel | HandbackModel | FatalLogModel | FatalModel | CommandModel | CancelModel | CountModel

linkedFunctions :: FilePath -> ServiceModel -> IO [(Word32, Int)]
linkedFunctions build model = do
  bytes <- readInput build (artifactPath ++ ".elf")
  flat <- readInput build (artifactPath ++ ".bin")
  elf <- either (prerequisite . ("service model ELF refused: " ++)) pure (E.parseElf bytes)
  text <- case [section | section <- E.elfSections elf, E.sectionExecutable section, E.sectionAllocated section] of
    [section] -> pure section
    _ -> prerequisite ("service model " ++ artifact ++ " requires one executable section")
  let linked = B.take (fromIntegral (E.sectionSize text)) (B.drop (fromIntegral (E.sectionOffset text)) bytes)
  unless (flat == linked) (prerequisite ("service model " ++ artifact ++ " ELF and flat image differ"))
  mapM (one text (E.elfSymbols elf)) names
  where
    (artifact, names) = case model of
      LowModel -> ("low", ["rb_mp_memcpy", "rb_mp_log"])
      HandbackModel -> ("side-a", ["rb_mp_handback"])
      FatalLogModel -> ("fatal2", ["rb_mp_fatal2"])
      FatalModel -> ("side-b", ["rb_mp_fatal"])
      CommandModel -> ("command", ["rb_mp_command"])
      CancelModel -> ("cancel", ["rb_mp_cancel"])
      CountModel -> ("count", ["rb_mp_subframe_count"])
    artifactPath = "replacement/media_permit/services" </> artifact
    one text symbols name =
      case [symbol | symbol <- symbols, E.symbolName symbol == name, E.symbolKind symbol == E.FunctionSymbol, E.symbolSize symbol > 0] of
        [symbol]
          | E.symbolValue symbol >= E.sectionAddress text
          , toInteger (E.symbolValue symbol) + toInteger (E.symbolSize symbol) <= toInteger (E.sectionAddress text) + toInteger (E.sectionSize text)
          , E.symbolValue symbol .&. 3 == 0
          , E.symbolSize symbol .&. 3 == 0 -> pure (E.symbolValue symbol, fromIntegral (E.symbolSize symbol))
        _ -> prerequisite ("missing unique bounded service-model function " ++ name ++ " in " ++ artifact)

stockEntries :: [(Word32, Word32)]
stockEntries =
  [ (0x00e77168, 0xe92d48f0)
  , (0x00e7e95c, 0xe92d4df0)
  , (0x00e67750, 0xe92d4830)
  ]

elfOffset :: Word32 -> Int
elfOffset address = fromIntegral (address - 0x00bf1a00 + 0xe8)

stockRegion :: Inputs -> Word32 -> Word32 -> B.ByteString
stockRegion inputs base size
  | offset + fromIntegral size > B.length (inStock inputs) =
      error ("stock ELF is too short for region " ++ hex base ++ " size " ++ show size)
  | otherwise = pad (fromIntegral size) (B.take (fromIntegral size) (B.drop offset (inStock inputs)))
  where
    offset = elfOffset base

stockWord :: B.ByteString -> Word32 -> Word32
stockWord stock address =
  case readU32 stock (elfOffset address) of
    Right value -> value
    Left problem -> error ("stock entry " ++ hex address ++ " unreadable: " ++ problem)

armBranch :: Word32 -> Word32 -> Word32
armBranch from target =
  let displacement = (fromIntegral target :: Int) - (fromIntegral from + 8)
      immediate = (displacement `div` 4) .&. 0xffffff
   in 0xea000000 .|. fromIntegral immediate

patchRegion :: Word32 -> B.ByteString -> Word32 -> B.ByteString -> Either String B.ByteString
patchRegion base bytes at piece
  | at < base || at + fromIntegral (B.length piece) > base + fromIntegral (B.length bytes) =
      Left ("patch outside region at " ++ hex at)
  | otherwise =
      Right (B.take offset bytes <> piece <> B.drop (offset + B.length piece) bytes)
  where
    offset = fromIntegral (at - base)

overlay :: Word32 -> Word32 -> B.ByteString -> Word32 -> Either String B.ByteString
overlay base size bytes at
  | at < base || at + fromIntegral (B.length bytes) > base + size = Left "model exceeds its region"
  | otherwise =
      Right (pad (fromIntegral size) (B.replicate (fromIntegral (at - base)) 0 <> bytes))

frameBytes :: Inputs -> Side -> Scenario -> Either String B.ByteString
frameBytes inputs side scenario
  | side /= Original && scFamily scenario == Message = do
      let stock = stockRegion inputs frameBase frameSize
          branch = armBranch 0xe77168 resolveEntry
          offset = fromIntegral (0xe77168 - frameBase)
          traps = B.concat (replicate ((0x1a0 - 4) `div` 4) (u32le 0xe7f000f0))
      case readU32 stock offset of
        Right 0xe92d48f0 -> pure ()
        Right actual -> Left ("stock resolver entry differs: " ++ hex actual)
        Left problem -> Left problem
      patched <- patchRegion frameBase stock 0xe77168 (u32le branch <> traps)
      case readU32 patched offset of
        Right actual
          | actual == branch
          , Right trap <- readU32 patched (offset + 4)
          , trap == 0xe7f000f0 ->
              pure patched
        Right actual -> Left ("candidate resolver entry or body guard is " ++ hex actual)
        Left problem -> Left problem
  | scCountModel scenario =
      patchRegion frameBase (stockRegion inputs frameBase frameSize) 0xe74c68 (inCount inputs)
  | otherwise = Right (stockRegion inputs frameBase frameSize)

regionBytes :: Inputs -> Side -> Scenario -> Either String [(String, Word32, Word32, B.ByteString)]
regionBytes inputs side scenario = do
  sideA <- overlay sideABase sideASize (inSideA inputs) sideABase
  fatal2 <- overlay fatalTwoBase fatalTwoSize (inFatal2 inputs) fatalTwoBase
  sideB <- overlay sideBBase sideBSize (inSideB inputs) 0x00e881e8
  command <- overlay commandBase commandSize (inCommand inputs) 0x00e8d82c
  cancel <- overlay cancelBase cancelSize (inCancel inputs) 0x00e6e368
  low <- overlay lowBase lowSize (inLow inputs) 0x00722054
  frame <- frameBytes inputs side scenario
  let candidate = pad (fromIntegral candidateSize) (case side of
        Original -> B.empty
        Candidate -> inCandidate inputs
        DefectCandidate -> inDefect inputs)
  pure
    [ ("globals", globalsBase, globalsSize, initialGlobals scenario)
    , ("frames", framesBase, framesSize, initialFrames scenario)
    , ("stack", stackBase, stackSize, B.empty)
    , ("sender", senderBase, senderSize, stockRegion inputs senderBase senderSize)
    , ("frame", frameBase, frameSize, frame)
    , ("mid", midBase, midSize, stockRegion inputs midBase midSize)
    , ("side-a", sideABase, sideASize, sideA)
    , ("fatal2", fatalTwoBase, fatalTwoSize, fatal2)
    , ("ctx", ctxBase, ctxSize, stockRegion inputs ctxBase ctxSize)
    , ("frame2", frameTwoBase, frameTwoSize, stockRegion inputs frameTwoBase frameTwoSize)
    , ("bit", bitBase, bitSize, stockRegion inputs bitBase bitSize)
    , ("side-b", sideBBase, sideBSize, sideB)
    , ("command", commandBase, commandSize, command)
    , ("cancel", cancelBase, cancelSize, cancel)
    , ("low", lowBase, lowSize, low)
    , ("candidate", candidateBase, candidateSize, candidate)
    ]

artifactJson :: String -> B.ByteString -> Value
artifactJson ident bytes =
  object
    [ "id" .= ident
    , "path" .= (ident ++ ".bin")
    , "size" .= B.length bytes
    , "sha256" .= sha256Hex (sha256 bytes)
    ]

regionJson :: String -> Word32 -> Word32 -> B.ByteString -> Value
regionJson ident base size initial =
  object
    [ "id" .= ident
    , "address" .= hex base
    , "size" .= size
    , "permissions" .= ((if ident `elem` ["stack", "globals", "frames"] then "rw" else "rx") :: String)
    , "initial" .=
        if B.null initial
          then object ["zero" .= True]
          else object ["artifact" .= ident, "offset" .= (0 :: Int), "length" .= B.length initial]
    ]

entryFor :: Family -> Side -> Word32
entryFor family side = case family of
  Resolver -> if side == Original then 0x00e77168 else candidateBase
  Handler -> if side == Original then 0x00e7e95c else candidateBase
  Message -> 0x00e67750

modeFor :: Family -> Word32
modeFor Resolver = 0
modeFor Handler = 1
modeFor Message = 2

initialRegisters :: Scenario -> Side -> Value
initialRegisters scenario side =
  object
    [ "r0" .= hex r0
    , "r1" .= hex r1
    , "r2" .= hex r2
    , "r3" .= hex 0
    , "r4" .= hex 0x4a4a4a4a
    , "r5" .= hex 0x5b5b5b5b
    , "r6" .= hex 0x6c6c6c6c
    , "r7" .= hex 0x7d7d7d7d
    , "r8" .= hex 0x8e8e8e8e
    , "r9" .= hex 0x9f9f9f9f
    , "r10" .= hex 0xa0a0a0a0
    , "r11" .= hex 0xb1b1b1b1
    , "r12" .= hex 0
    , "sp" .= hex stackTop
    , "lr" .= hex exitAddress
    , "cpsr" .= hex 0x10
    ]
  where
    (r0, r1, r2) = case (side, scFamily scenario) of
      (Original, Resolver) -> (scKey scenario, 0, 0)
      (Original, Handler) -> (0, 0, 0)
      (Original, Message) -> (scWord0 scenario, scWord1 scenario, 0)
      (_, Resolver) -> (modeFor Resolver, scKey scenario, 0)
      (_, Handler) -> (modeFor Handler, 0, 0)
      (_, Message) -> (scWord0 scenario, scWord1 scenario, 0)

candidateSpan :: Inputs -> Side -> Word32
candidateSpan inputs side =
  let bytes = case side of
        Original -> B.empty
        Candidate -> inCandidate inputs
        DefectCandidate -> inDefect inputs
   in fromIntegral (((B.length bytes + 3) `div` 4) * 4)

candidateSpans :: Inputs -> Side -> [(Word32, Int)]
candidateSpans inputs side
  | side == Original = []
  | otherwise =
      let total = fromIntegral (candidateSpan inputs side) :: Int
          block = fromIntegral entrySpan :: Int
       in [ (candidateBase, block)
          , (resolveEntry, block)
          , (applyEntry, block)
          , (applyEntry + entrySpan, total - 3 * block)
          ]

caseJson :: Inputs -> Scenario -> Side -> [(String, Word32, Word32, B.ByteString)] -> Value
caseJson inputs scenario side regions =
  caseJsonAt inputs scenario side (entryFor (scFamily scenario) side) regions

caseJsonAt :: Inputs -> Scenario -> Side -> Word32 -> [(String, Word32, Word32, B.ByteString)] -> Value
caseJsonAt inputs scenario side entry regions =
  object
    [ "schema_version" .= (1 :: Int)
    , "id" .= (scId scenario ++ "-" ++ sideLabel side)
    , "origin" .= ((if side == Original then "stock" else "synthetic") :: String)
    , "provenance" .= ("X-M1 1.50 image 5 Reccf_SvPermitMediaWrite contract; retained services at their stock call addresses" :: String)
    , "cpu" .= object ["architecture" .= ("arm" :: String), "model" .= ("cortex-r5f" :: String), "mode" .= ("arm" :: String), "endian" .= ("little" :: String)]
    , "artifacts" .= [artifactJson ident bytes | (ident, _, _, bytes) <- regions, not (B.null bytes)]
    , "entry" .= hex entry
    , "exit" .= hex exitAddress
    , "registers" .= initialRegisters scenario side
    , "regions" .= [regionJson ident base size bytes | (ident, base, size, bytes) <- regions]
    , "functions" .=
        ( [ object ["address" .= hex address, "size" .= admittedSize address size]
          | (address, size) <- functions
          ]
            ++ [object ["address" .= hex address, "size" .= size] | (address, size) <- candidateSpans inputs side]
        )
    , "stubs" .= ([] :: [Value])
    , "limits" .= object ["instructions" .= (4000000 :: Int), "wall_ms" .= (8000 :: Int)]
    , "expected" .=
        object
          [ "status" .= ("returned" :: String)
          , "registers" .=
              object
                ( [ "sp" .= hex stackTop
                  , "pc" .= hex exitAddress
                  ]
                    ++ ( case scResolve scenario of
                           Nothing -> []
                           Just expected -> ["r0" .= hex expected]
                       )
                )
          , "memory" .= ([] :: [Value])
          , "events" .= ([] :: [Value])
          ]
    , "exclusions" .=
        ( [ "Frame records and slot tables are synthetic fixtures; the stack is execution scratch."
          , "Only the contract effects at the named globals and frame words are compared."
          ] :: [String]
        )
    ]
  where
    functions
      | scCountModel scenario = filter ((/= 0x00e74c68) . fst) (inFunctions inputs) ++ inCountFunctions inputs
      | otherwise = inFunctions inputs
    admittedSize address size
      | side /= Original && scFamily scenario == Message && address == 0x00e77168 = 4
      | otherwise = size

data ResultStatus = ResultPassed | ResultFailed | ResultInvalidCase | ResultInvalidOutput | ResultHostTimeout
  deriving (Eq, Show)

resultStatusLabel :: ResultStatus -> String
resultStatusLabel ResultPassed = "passed"
resultStatusLabel ResultFailed = "failed"
resultStatusLabel ResultInvalidCase = "invalid_case"
resultStatusLabel ResultInvalidOutput = "invalid_output"
resultStatusLabel ResultHostTimeout = "host_timeout"

data Execution = Executed | Reused | FailedExecution
  deriving (Eq, Show)

executionLabel :: Execution -> String
executionLabel Executed = "executed"
executionLabel Reused = "reused"
executionLabel FailedExecution = "failed"

data Result = Result
  { resultStatus :: ResultStatus
  , resultProblems :: [String]
  , resultObservation :: Maybe N.NativeObservation
  , resultEffects :: Maybe Effects
  , resultExecution :: Maybe Execution
  , resultArtifact :: Maybe FilePath
  }

data CaseRun = CaseRun
  { caseRunStatus :: ResultStatus
  , caseRunProblems :: [String]
  , caseRunObservation :: Maybe N.NativeObservation
  , caseRunExecution :: Maybe Execution
  }

data Attempt = Attempt
  { attemptRunner :: FilePath
  , attemptCache :: FilePath
  , attemptOut :: FilePath
  , attemptExecuted :: IORef Int
  , attemptReused :: IORef Int
  , attemptFailed :: IORef Int
  }

checkEffects :: Scenario -> N.NativeObservation -> [String]
checkEffects scenario observation =
  case (lookup (N.RegionId "globals") (N.nativeMemory observation), lookup (N.RegionId "frames") (N.nativeMemory observation)) of
    (Just globals, Just frames) ->
      let observed = observeEffects globals frames (N.nativeRegisters observation)
          resolveProblems =
            case scResolve scenario of
              Nothing -> []
              Just expected ->
                case M.lookup N.R0 (N.nativeRegisters observation) of
                  Just actual
                    | actual == expected -> []
                    | otherwise -> ["r0 expected " ++ hex expected ++ ", observed " ++ hex actual]
                  Nothing -> ["r0 missing"]
       in (if observed == scExpect scenario then [] else ["effects differ: expected " ++ show (scExpect scenario) ++ ", observed " ++ show observed])
            ++ resolveProblems
    _ -> ["globals or frames region missing"]

observeEffects :: B.ByteString -> B.ByteString -> M.Map N.Register Word32 -> Effects
observeEffects globals frames _ =
  Effects
    { eBitmap = [word globals (bitmapBase + fromIntegral slot * 0x1d0 + 0x14) | slot <- inspectedSlots]
    , eCapture = [frameWord frames (ctxAddress slot + 0x27c8) | slot <- inspectedSlots]
    , ePermit = [frameWord frames (ctxAddress slot + 0x2fb0) | slot <- inspectedSlots]
    , eMinima = (word globals minimaBase, word globals (minimaBase + 4), word globals (minimaBase + 8))
    , eActive = word globals activeBase
    , eCommands = pairs traceCommands
    , eCancels = pairs traceCancels
    , eHandback = (word globals traceHandbackClass, word globals traceHandbackCount)
    , eFaults = word globals traceFaults
    , eMirror =
        ( word globals mirrorBase .&. 0xffff
        , word globals mirrorWord0
        , word globals mirrorWord1
        )
    }
  where
    pairs base =
      let count = word globals base
       in if count == 0
            then []
            else
              [ (word globals (base + 4 + index * 8), word globals (base + 8 + index * 8))
              | index <- [0 .. min count 16 - 1]
              ]

contains :: N.GuestAddress -> Int -> N.GuestAddress -> Int -> Bool
contains (N.GuestAddress base) extent (N.GuestAddress address) size =
  address >= base && address + fromIntegral size <= base + fromIntegral extent

checkWrites :: N.NativeObservation -> [String]
checkWrites observation =
  case N.nativeProvenance observation of
    N.Truncated _ -> ["provenance truncated"]
    N.Complete _ entries ->
      concat
        [ if contains (N.GuestAddress (fromIntegral stackBase)) (fromIntegral stackSize) address (fromIntegral size)
            || any (\(base, extent) -> contains (N.GuestAddress (fromIntegral base)) extent address (fromIntegral size)) allowedStores
            then []
            else ["write outside declared stores at " ++ show (N.guestAddress address)]
        | N.Write address (N.WriteBytes size) <- entries
        ]

allowedStores :: [(Word32, Int)]
allowedStores =
  [ (activeBase, 4)
  , (minimaBase, 4)
  , (minimaBase + 4, 4)
  , (minimaBase + 8, 4)
  , (mirrorBase, 8)
  , (mirrorWord0, 8)
  , (traceCommands, 4 + 16 * 8)
  , (traceCancels, 4 + 16 * 8)
  , (traceFaults, 4)
  , (traceHandbackClass, 8)
  , (traceLogs, 4)
  , (countCell, 8)
  ]
    ++ [ (bitmapBase + fromIntegral slot * 0x1d0 + 0x14, 4) | slot <- inspectedSlots ]
    ++ [ (ctxAddress slot + offset, 4) | slot <- inspectedSlots, offset <- [0x27c8, 0x2fb0] ]

checkAbi :: N.NativeObservation -> [String]
checkAbi observation =
  registers ++ cpsrControl
  where
    registersOf = N.nativeRegisters observation
    register which expected =
      case M.lookup which registersOf of
        Nothing -> ["register " ++ N.registerLabel which ++ ": missing"]
        Just actual
          | actual /= expected -> ["register " ++ N.registerLabel which ++ ": expected " ++ hex expected ++ ", observed " ++ hex actual]
          | otherwise -> []
    abiPreserved =
      [ (N.R4, 0x4a4a4a4a)
      , (N.R5, 0x5b5b5b5b)
      , (N.R6, 0x6c6c6c6c)
      , (N.R7, 0x7d7d7d7d)
      , (N.R8, 0x8e8e8e8e)
      , (N.R9, 0x9f9f9f9f)
      , (N.R10, 0xa0a0a0a0)
      , (N.R11, 0xb1b1b1b1)
      ]
    registers = register N.Sp stackTop ++ register N.Pc exitAddress ++ concat [register which value | (which, value) <- abiPreserved]
    cpsrControl =
      case M.lookup N.Cpsr registersOf of
        Nothing -> ["register cpsr: missing"]
        Just cpsr
          | cpsr .&. 0x3ff /= 0x10 -> ["cpsr control bits expected 0x00000010, observed " ++ hex cpsr]
          | otherwise -> []

classifyCacheFailure :: String -> ResultStatus
classifyCacheFailure message
  | "host_timeout" `isInfixOf` message = ResultHostTimeout
  | "invalid_case" `isInfixOf` message = ResultInvalidCase
  | "admission failed" `isInfixOf` message = ResultInvalidCase
  | "invalid batch" `isInfixOf` message = ResultInvalidCase
  | "does not exist" `isInfixOf` message = ResultInvalidCase
  | "must be a capped regular file" `isInfixOf` message = ResultInvalidCase
  | otherwise = ResultInvalidOutput

readFailureReport :: FilePath -> IO (Maybe Value)
readFailureReport path = do
  present <- doesFileExist path
  if not present
    then pure Nothing
    else do
      parsed <- try (B.readFile path >>= jsonRead) :: IO (Either IOException Value)
      pure (either (const Nothing) Just parsed)

classifyFailureReport :: Maybe Value -> String -> ResultStatus
classifyFailureReport failure fallback =
  case failure >>= statusOf of
    Just "host_timeout" -> ResultHostTimeout
    Just "invalid_case" -> ResultInvalidCase
    Just "runner_failure" -> ResultInvalidOutput
    _ -> classifyCacheFailure fallback
  where
    statusOf value = case str (get value "status") of
      "" -> Nothing
      status -> Just status

removeIfPresent :: FilePath -> IO ()
removeIfPresent path = do
  present <- doesFileExist path
  when present (removeFile path)

extractObservation :: FilePath -> FilePath -> IO ()
extractObservation resultPath observationPath = do
  bytes <- B.readFile resultPath
  value <- jsonRead bytes
  case get value "runner" of
    Null -> ioError (userError "native result has no runner observation")
    runner -> B.writeFile observationPath (encodeCanonical runner)

executeCase :: Attempt -> FilePath -> String -> Value -> [(String, Word32, Word32, B.ByteString)] -> IO CaseRun
executeCase attempt directory caseId value regions = do
  createDirectoryIfMissing True directory
  forM_ regions $ \(ident, _, _, bytes) ->
    unless (B.null bytes) (B.writeFile (directory </> (ident ++ ".bin")) bytes)
  BL.writeFile (directory </> "case.json") (encode value)
  let batchPath = directory </> "batch.json"
      resultPath = directory </> (caseId ++ ".native-result.json")
      observationPath = directory </> "observation.json"
  BL.writeFile batchPath (encode (object ["schema_version" .= (1 :: Int), "cases" .= [object ["id" .= caseId, "path" .= ("case.json" :: String)]]]))
  outcome <- try (cachedObservedBatchWith WriteAllResults (attemptCache attempt) (attemptRunner attempt) batchPath) :: IO (Either IOException (BatchReuse, [(CaseId, N.NativeObservation)]))
  case outcome of
    Left problem -> do
      removeIfPresent batchPath
      failure <- readFailureReport resultPath
      let message = exceptionText problem
          detail =
            case failure of
              Just failureValue ->
                let status = str (get failureValue "status")
                    reason = str (get failureValue "reason")
                 in "status " ++ status ++ (if null reason then "" else ": " ++ reason) ++ "; failure report " ++ resultPath
              Nothing -> message
      B.writeFile (directory </> "diagnostic.txt") (BC.pack detail)
      modifyIORef' (attemptFailed attempt) (+ 1)
      pure (CaseRun (classifyFailureReport failure message) [detail] Nothing (Just FailedExecution))
    Right (reuse, observations) ->
      case [observation | (ident, observation) <- observations, caseIdText ident == caseId] of
        [] -> do
          removeIfPresent batchPath
          modifyIORef' (attemptFailed attempt) (+ 1)
          pure (CaseRun ResultInvalidOutput ["native batch did not return " ++ caseId] Nothing (Just FailedExecution))
        (observation : _) -> do
          extracted <- try (extractObservation resultPath observationPath) :: IO (Either IOException ())
          case extracted of
            Left problem -> do
              removeIfPresent batchPath
              let message = "cannot retain the native response: " ++ exceptionText problem ++ "; failure report " ++ resultPath
              B.writeFile (directory </> "diagnostic.txt") (BC.pack message)
              modifyIORef' (attemptFailed attempt) (+ 1)
              pure (CaseRun ResultInvalidOutput [message] Nothing (Just FailedExecution))
            Right () -> do
              removeIfPresent resultPath
              removeIfPresent batchPath
              let execution = case reuse of BatchExecuted -> Executed; BatchReused -> Reused
              case execution of
                Executed -> modifyIORef' (attemptExecuted attempt) (+ 1)
                Reused -> modifyIORef' (attemptReused attempt) (+ 1)
                FailedExecution -> pure ()
              pure (CaseRun ResultPassed [] (Just observation) (Just execution))

resultFromCaseRun :: Scenario -> FilePath -> CaseRun -> Result
resultFromCaseRun scenario directory caseRun =
  case caseRunObservation caseRun of
    Nothing -> Result (caseRunStatus caseRun) (caseRunProblems caseRun) Nothing Nothing (caseRunExecution caseRun) (Just directory)
    Just observation ->
      let problems =
            (if N.nativeStatus observation == N.Returned then [] else ["runner status " ++ N.runnerStatusLabel (N.nativeStatus observation)])
              ++ checkAbi observation
              ++ checkEffects scenario observation
              ++ checkWrites observation
          effects =
            case (lookup (N.RegionId "globals") (N.nativeMemory observation), lookup (N.RegionId "frames") (N.nativeMemory observation)) of
              (Just globals, Just frames) -> Just (observeEffects globals frames (N.nativeRegisters observation))
              _ -> Nothing
       in Result (if null problems then ResultPassed else ResultFailed) problems (Just observation) effects (caseRunExecution caseRun) (Just directory)

runSide :: Attempt -> FilePath -> Inputs -> Scenario -> Side -> IO Result
runSide attempt root inputs scenario side =
  case regionBytes inputs side scenario of
    Left problem -> pure (Result ResultInvalidCase [problem] Nothing Nothing Nothing Nothing)
    Right regions -> do
      let directory = root </> (scId scenario ++ "-" ++ sideLabel side)
          caseId = scId scenario ++ "-" ++ sideLabel side
      caseRun <- executeCase attempt directory caseId (caseJson inputs scenario side regions) regions
      pure (resultFromCaseRun scenario directory caseRun)

-- The retired resolver body is outside the candidate message case's declared code.
runForbiddenBody :: Attempt -> Inputs -> Scenario -> IO Result
runForbiddenBody attempt inputs scenario =
  case regionBytes inputs Candidate control of
    Left problem -> pure (Result ResultInvalidCase [problem] Nothing Nothing Nothing Nothing)
    Right regions -> do
      let directory = attemptOut attempt </> "forbidden-body"
          caseId = "forbidden-body-candidate"
      caseRun <- executeCase attempt directory caseId (caseJsonAt inputs control Candidate 0x00900c00 regions) regions
      case caseRunObservation caseRun of
        Nothing -> pure (Result (caseRunStatus caseRun) (caseRunProblems caseRun) Nothing Nothing (caseRunExecution caseRun) (Just directory))
        Just observation ->
          let pc = M.lookup N.Pc (N.nativeRegisters observation)
              refused =
                N.nativeStatus observation == N.UnknownCall
                  && pc == Just 0x00e7716c
                  && N.nativeProvenance observation == N.Complete (N.ProvenanceTotal 0) []
           in pure
                ( Result
                    (if refused then ResultPassed else ResultFailed)
                    ["forbidden-body status " ++ N.runnerStatusLabel (N.nativeStatus observation) ++ " at " ++ maybe "missing pc" hex pc]
                    (Just observation)
                    Nothing
                    (caseRunExecution caseRun)
                    (Just directory)
                )
  where
    control = scenario {scId = "forbidden-body"}

forbiddenWitness :: Result -> Bool
forbiddenWitness result = resultStatus result == ResultPassed

data ControlStatus = ControlRefused | ControlAccepted | ControlInvalidCase | ControlHostTimeout | ControlInvalidOutput
  deriving (Eq, Show)

controlStatusLabel :: ControlStatus -> String
controlStatusLabel ControlRefused = "refused"
controlStatusLabel ControlAccepted = "accepted"
controlStatusLabel ControlInvalidCase = "invalid_case"
controlStatusLabel ControlHostTimeout = "host_timeout"
controlStatusLabel ControlInvalidOutput = "invalid_output"

data Control = Control
  { controlName :: String
  , controlStatus :: ControlStatus
  , controlProblems :: [String]
  , controlExecution :: Maybe Execution
  , controlArtifact :: Maybe FilePath
  }

controlJson :: Control -> Value
controlJson control =
  object
    [ "name" .= controlName control
    , "status" .= controlStatusLabel (controlStatus control)
    , "problems" .= controlProblems control
    , "execution" .= fmap executionLabel (controlExecution control)
    , "artifact" .= controlArtifact control
    ]

expectedDefect :: Scenario -> Effects
expectedDefect scenario =
  let expected = scExpect scenario
   in expected {eBitmap = replace 0 0x80000000 (eBitmap expected)}

faultWitness :: Scenario -> Result -> Bool
faultWitness scenario result =
  resultStatus result == ResultFailed
    && case (resultObservation result, resultEffects result) of
      (Just observation, Just effects) ->
        N.nativeStatus observation == N.Returned
          && null (checkAbi observation)
          && null (checkWrites observation)
          && effects == expectedDefect scenario
      _ -> False

controlStatusFor :: Bool -> Result -> ControlStatus
controlStatusFor refused result
  | refused = ControlRefused
  | otherwise = case resultStatus result of
      ResultInvalidCase -> ControlInvalidCase
      ResultHostTimeout -> ControlHostTimeout
      ResultInvalidOutput -> ControlInvalidOutput
      _ -> ControlAccepted

runBoundaryControls :: Attempt -> Inputs -> IO [Control]
runBoundaryControls attempt inputs = do
  let scenario = scenarioById "primary-slot0"
      altered words' = inputs {inCandidate = B.concat (map u32le words') <> B.drop (4 * length words') (inCandidate inputs)}
      isaInputs = altered [0xe710f110]
  abi <- runSide attempt (attemptOut attempt </> "abi-control") (altered [0xe3a04000, 0xe3a00000, 0xe12fff1e]) scenario Candidate
  isa <- runSide attempt (attemptOut attempt </> "isa-control") isaInputs scenario Candidate
  let abiRefused = case resultObservation abi of
        Just observation ->
          resultStatus abi == ResultFailed
            && N.nativeStatus observation == N.Returned
            && M.lookup N.R4 (N.nativeRegisters observation) == Just 0
            && length (checkAbi observation) == 1
            && null (checkEffects scenario observation)
            && null (checkWrites observation)
        Nothing -> False
      isaRefused = case (resultObservation isa, regionBytes isaInputs Candidate scenario) of
        (Just observation, Right regions) ->
          N.nativeStatus observation == N.UnsupportedInstruction
            && M.lookup N.Pc (N.nativeRegisters observation) == Just candidateBase
            && N.nativeProvenance observation == N.Complete (N.ProvenanceTotal 0) []
            && and
              [ M.lookup register (N.nativeRegisters observation) == N.parseWord32 (str (get (initialRegisters scenario Candidate) (N.registerLabel register)))
              | register <- N.registerLabels
              , register /= N.Pc
              ]
            && and
              [ lookup (N.RegionId ident) (N.nativeMemory observation) == Just (pad (fromIntegral size) bytes)
              | (ident, _, size, bytes) <- regions
              ]
        _ -> False
      control name refused result = Control name (controlStatusFor refused result) (resultProblems result) (resultExecution result) (resultArtifact result)
  pure [control "callee-saved-register" abiRefused abi, control "unsupported-isa-before-effect" isaRefused isa]

data ExecutionCounts = ExecutionCounts
  { countExecuted :: Int
  , countReused :: Int
  , countFailed :: Int
  }

headOr :: String -> [String] -> String
headOr fallback [] = fallback
headOr _ (value : _) = value

data CaseStatus = CasePassed | CaseFailed | CaseInvalidCase | CaseHostTimeout | CaseInvalidOutput
  deriving (Eq, Show)

caseStatusLabel :: CaseStatus -> String
caseStatusLabel CasePassed = "passed"
caseStatusLabel CaseFailed = "failed"
caseStatusLabel CaseInvalidCase = "invalid_case"
caseStatusLabel CaseHostTimeout = "host_timeout"
caseStatusLabel CaseInvalidOutput = "invalid_output"

data OutcomeStatus = OutcomeQualified | OutcomeFailed | OutcomePrerequisite | OutcomeInfrastructure
  deriving (Eq, Show)

outcomeStatusLabel :: OutcomeStatus -> String
outcomeStatusLabel OutcomeQualified = "qualified"
outcomeStatusLabel OutcomeFailed = "failed"
outcomeStatusLabel OutcomePrerequisite = "prerequisite"
outcomeStatusLabel OutcomeInfrastructure = "infrastructure"

outcomeExitCode :: OutcomeStatus -> ExitCode
outcomeExitCode OutcomeQualified = ExitSuccess
outcomeExitCode OutcomeFailed = ExitFailure 1
outcomeExitCode OutcomePrerequisite = ExitFailure 2
outcomeExitCode OutcomeInfrastructure = ExitFailure 75

data Report = Report
  { reportStatus :: OutcomeStatus
  , reportProblems :: [String]
  , reportCandidate :: CandidateSelection
  , reportValue :: Value
  }

report :: CandidateSelection -> Int -> Bool -> ExecutionCounts -> FilePath -> [(Scenario, Result, Result)] -> Scenario -> Result -> Result -> [Control] -> Report
report candidateMode expectedPairs subset counts cachePath results faultScenario fault forbidden boundaryControls =
  Report
    { reportStatus = outcome
    , reportProblems = problems
    , reportCandidate = candidateMode
    , reportValue =
        object
          [ "schema_version" .= (1 :: Int)
          , "profile" .= ("media-permit" :: String)
          , "candidate" .= candidateModeLabel candidateMode
          , "subset" .= subset
          , "cases" .=
              [ object
                  [ "id" .= scId scenario
                  , "model" .= show (scExpect scenario)
                  , "original" .= resultJson original
                  , "candidate" .= resultJson candidate
                  , "status" .= caseStatusLabel (pairStatus original candidate)
                  ]
              | (scenario, original, candidate) <- results
              ]
          , "controls" .= map controlJson allControls
          , "execution" .=
              object
                [ "executed" .= countExecuted counts
                , "reused" .= countReused counts
                , "failed" .= countFailed counts
                , "cache" .= cachePath
                ]
          , "status" .= outcomeStatusLabel outcome
          , "problems" .= problems
          ]
    }
  where
    defectControl =
      Control
        "defect-candidate"
        (controlStatusFor (faultWitness faultScenario fault) fault)
        (resultProblems fault)
        (resultExecution fault)
        (resultArtifact fault)
    forbiddenControl =
      Control
        "forbidden-body"
        (controlStatusFor (forbiddenWitness forbidden) forbidden)
        (resultProblems forbidden)
        (resultExecution forbidden)
        (resultArtifact forbidden)
    allControls = defectControl : forbiddenControl : boundaryControls
    allResults = [result | (_, original, candidate) <- results, result <- [original, candidate]] ++ [fault, forbidden]
    qualified :: Bool
    qualified =
      length results == expectedPairs
        && all (\(_, original, candidate) -> resultStatus original == ResultPassed && resultStatus candidate == ResultPassed) results
        && faultWitness faultScenario fault
        && forbiddenWitness forbidden
        && all ((== ControlRefused) . controlStatus) boundaryControls
    outcome :: OutcomeStatus
    outcome
      | any ((== ResultInvalidCase) . resultStatus) allResults || any ((== ControlInvalidCase) . controlStatus) allControls = OutcomePrerequisite
      | any ((`elem` [ResultHostTimeout, ResultInvalidOutput]) . resultStatus) allResults
          || any ((`elem` [ControlHostTimeout, ControlInvalidOutput]) . controlStatus) allControls = OutcomeInfrastructure
      | qualified = OutcomeQualified
      | otherwise = OutcomeFailed
    problems :: [String]
    problems = caseProblems ++ controlProblemsList
    caseProblems :: [String]
    caseProblems =
      [ scId scenario ++ "/" ++ sideLabel side ++ ": " ++ decisiveProblem result
      | (scenario, original, candidate) <- results
      , (side, result) <- [(Original, original), (Candidate, candidate)]
      , resultStatus result /= ResultPassed
      ]
    controlProblemsList :: [String]
    controlProblemsList =
      [ "control " ++ controlName control ++ ": " ++ headOr (controlStatusLabel (controlStatus control)) (controlProblems control)
          ++ artifactSuffix (controlArtifact control)
      | control <- allControls
      , controlStatus control /= ControlRefused
      ]
    decisiveProblem :: Result -> String
    decisiveProblem result =
      headOr (resultStatusLabel (resultStatus result)) (resultProblems result) ++ artifactSuffix (resultArtifact result)
    artifactSuffix :: Maybe FilePath -> String
    artifactSuffix = maybe "" (\directory -> " (artifact " ++ directory ++ ")")
    pairStatus :: Result -> Result -> CaseStatus
    pairStatus original candidate
      | resultStatus original == ResultPassed && resultStatus candidate == ResultPassed = CasePassed
      | any ((== ResultInvalidCase) . resultStatus) [original, candidate] = CaseInvalidCase
      | any ((== ResultHostTimeout) . resultStatus) [original, candidate] = CaseHostTimeout
      | any ((== ResultInvalidOutput) . resultStatus) [original, candidate] = CaseInvalidOutput
      | otherwise = CaseFailed
    resultJson :: Result -> Value
    resultJson result =
      object
        [ "status" .= resultStatusLabel (resultStatus result)
        , "problems" .= resultProblems result
        , "execution" .= fmap executionLabel (resultExecution result)
        , "artifact" .= resultArtifact result
        ]

data CandidateSelection = CurrentCandidate | KnownDefectCandidate
  deriving (Eq, Show)

candidateModeLabel :: CandidateSelection -> String
candidateModeLabel CurrentCandidate = "current"
candidateModeLabel KnownDefectCandidate = "known-defect"

data Options = Options
  { optionRunner :: Maybe FilePath
  , optionBuildRoot :: Maybe FilePath
  , optionStockElf :: Maybe FilePath
  , optionOut :: FilePath
  , optionCache :: Maybe FilePath
  , optionScenarios :: [String]
  , optionCandidate :: CandidateSelection
  }

data Command = Inventory FilePath | Qualify Options | Help

usage :: String
usage =
  unlines
    [ "usage: media-permit-qualification [--runner PATH] [--build-root DIR] [--stock-elf PATH]"
    , "       --out DIR [--cache DIR] [--scenario NAME]... [--candidate current|defect]"
    , "       --cache defaults to private-data/cache/native"
    , "       media-permit-qualification --inventory-only STOCK_ELF"
    ]

parseCommand :: [String] -> Either String Command
parseCommand arguments
  | "--help" `elem` arguments || "-h" `elem` arguments = Right Help
  | otherwise = case arguments of
      ["--inventory-only", stockElf] -> Right (Inventory stockElf)
      _ -> do
        (options, positionals) <- collect arguments (Options Nothing Nothing Nothing "" Nothing [] CurrentCandidate)
        unless (null positionals) (Left ("unexpected positional argument " ++ headOr "argument" positionals ++ "; use --out, --cache and --scenario"))
        unless (not (null (optionOut options))) (Left "--out DIR is required")
        pure (Qualify options)
  where
    collect [] options = Right (options, [])
    collect (flag : value : rest) options
      | flag `elem` valueFlags = do
          next <- setOption flag value options
          collect rest next
    collect (flag : []) _
      | flag `elem` valueFlags = Left (flag ++ " needs a value")
    collect (flag : _) _
      | flag `elem` valueFlags = Left (flag ++ " needs a value")
    collect ("--inventory-only" : _) _ = Left "--inventory-only must be used alone"
    collect (value : rest) options = do
      (final, positionals) <- collect rest options
      pure (final, value : positionals)
    valueFlags = ["--runner", "--build-root", "--stock-elf", "--out", "--cache", "--scenario", "--candidate"]
    setOption flag value options = case flag of
      "--runner" -> Right options {optionRunner = Just value}
      "--build-root" -> Right options {optionBuildRoot = Just value}
      "--stock-elf" -> Right options {optionStockElf = Just value}
      "--out" -> Right options {optionOut = value}
      "--cache" -> Right options {optionCache = Just value}
      "--scenario" -> Right options {optionScenarios = optionScenarios options ++ [value]}
      "--candidate" -> case value of
        "current" -> Right options {optionCandidate = CurrentCandidate}
        "defect" -> Right options {optionCandidate = KnownDefectCandidate}
        _ -> Left "--candidate must be current or defect"
      _ -> Right options

data Paths = Paths
  { pathRunner :: FilePath
  , pathBuildRoot :: FilePath
  , pathStockElf :: FilePath
  , pathOut :: FilePath
  , pathCache :: FilePath
  }

resolvePaths :: Options -> IO Paths
resolvePaths options = do
  workspace <- lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= maybe getCurrentDirectory pure
  buildRoot <- case optionBuildRoot options of
    Just path -> pure (resolve workspace path)
    Nothing -> do
      executable <- getExecutablePath
      let runfiles = executable ++ ".runfiles/_main"
      present <- doesDirectoryExist runfiles
      pure (if present then runfiles else workspace </> "bazel-bin")
  let runner = resolve workspace (maybe fujimodNativeCase id (optionRunner options))
      stockElf = resolve workspace (maybe "private-data/analysis/x-m1-1.50/lz/id5.elf" id (optionStockElf options))
      out = resolve workspace (optionOut options)
      cache = resolve workspace (maybe defaultNativeCacheRoot id (optionCache options))
  pure Paths {pathRunner = runner, pathBuildRoot = buildRoot, pathStockElf = stockElf, pathOut = out, pathCache = cache}
  where
    resolve root path = if isAbsolute path then path else root </> path

acquireSlot :: [String] -> IO ()
acquireSlot originalArguments = do
  inside <- lookupEnv "XM1_EMULATOR_SLOT"
  case inside of
    Just _ -> pure ()
    Nothing -> do
      executable <- getExecutablePath
      let wrapper = executable ++ ".runfiles/_main/emulation/emulator-slot"
      present <- doesFileExist wrapper
      if present
        then executeFile wrapper False (executable : originalArguments) Nothing
        else prerequisite ("emulator slot wrapper missing at " ++ wrapper ++ "; run through bazel run or set XM1_EMULATOR_SLOT")

prepareOutputPaths :: Paths -> IO Paths
prepareOutputPaths paths = do
  let out = pathOut paths
  createDirectoryIfMissing True out
  canonicalOut <- canonicalizePath out
  stale <- listDirectory canonicalOut
  unless (null stale) (prerequisite ("output directory must be fresh: " ++ canonicalOut))
  cache <- do
    let path = pathCache paths
    createDirectoryIfMissing True path
    canonicalizePath path
  pure paths {pathOut = canonicalOut, pathCache = cache}

setStoragePath :: Paths -> IO ()
setStoragePath paths = setEnv "XM1_STORAGE_PATH" (pathOut paths)

prepareAndRun :: Options -> [String] -> IO (ExitCode, String)
prepareAndRun options arguments = do
  paths <- resolvePaths options
  prepared <- prepareOutputPaths paths
  setStoragePath prepared
  acquireSlot arguments
  runQualification prepared options

writeResults :: FilePath -> Report -> IO ()
writeResults out attemptReport = BL.writeFile (out </> "results.json") (encode (reportValue attemptReport))

prerequisiteReport :: CandidateSelection -> FilePath -> String -> Report
prerequisiteReport candidateMode cachePath message =
  Report
    { reportStatus = OutcomePrerequisite
    , reportProblems = [message]
    , reportCandidate = candidateMode
    , reportValue =
        object
          [ "schema_version" .= (1 :: Int)
          , "profile" .= ("media-permit" :: String)
          , "candidate" .= candidateModeLabel candidateMode
          , "subset" .= False
          , "cases" .= ([] :: [Value])
          , "controls" .= ([] :: [Value])
          , "execution" .= object ["executed" .= (0 :: Int), "reused" .= (0 :: Int), "failed" .= (0 :: Int), "cache" .= cachePath]
          , "status" .= outcomeStatusLabel OutcomePrerequisite
          , "problems" .= [message]
          ]
    }

terminalMessage :: FilePath -> [Scenario] -> Bool -> ExecutionCounts -> Report -> String
terminalMessage out selected subset counts attemptReport =
  "media-permit: "
    ++ knownPrefix
    ++ case reportStatus attemptReport of
      OutcomeQualified ->
        "qualified ("
          ++ scope
          ++ "; "
          ++ show (length selected)
          ++ " pair(s), 4 controls refused; executed="
          ++ show (countExecuted counts)
          ++ " reused="
          ++ show (countReused counts)
          ++ "); results="
          ++ results
      OutcomeFailed -> "failed: " ++ firstProblem ++ "; results=" ++ results
      OutcomePrerequisite -> "prerequisite: " ++ firstProblem ++ "; results=" ++ results
      OutcomeInfrastructure -> "infrastructure: " ++ firstProblem ++ "; results=" ++ results
  where
    results = out </> "results.json"
    scope = if subset then "subset " ++ show (length selected) ++ "/" ++ show (length scenarios) else "full"
    firstProblem = headOr "see results.json" (reportProblems attemptReport)
    knownPrefix = case reportCandidate attemptReport of
      CurrentCandidate -> ""
      KnownDefectCandidate -> "known-defect candidate; "

runQualification :: Paths -> Options -> IO (ExitCode, String)
runQualification paths options = do
  let cachePath = pathCache paths
  loaded <- try (loadInputs (pathBuildRoot paths) (pathStockElf paths)) :: IO (Either Prerequisite Inputs)
  case loaded of
    Left (Prerequisite message) -> do
      let failureReport = prerequisiteReport (optionCandidate options) cachePath message
      writeResults (pathOut paths) failureReport
      pure (outcomeExitCode (reportStatus failureReport), "media-permit: prerequisite: " ++ message)
    Right inputs ->
      case selectScenarios (optionScenarios options) of
        Left message -> do
          let failureReport = prerequisiteReport (optionCandidate options) cachePath message
          writeResults (pathOut paths) failureReport
          pure (outcomeExitCode (reportStatus failureReport), "media-permit: prerequisite: " ++ message)
        Right (selected, subset) -> do
          executed <- newIORef 0
          reused <- newIORef 0
          failed <- newIORef 0
          let attempt =
                Attempt
                  { attemptRunner = pathRunner paths
                  , attemptCache = cachePath
                  , attemptOut = pathOut paths
                  , attemptExecuted = executed
                  , attemptReused = reused
                  , attemptFailed = failed
                  }
              selectedInputs = case optionCandidate options of
                CurrentCandidate -> inputs
                KnownDefectCandidate -> inputs {inCandidate = inDefect inputs}
          results <- runCases attempt selectedInputs selected []
          let faultScenario = scenarioById "kind0-zero"
              forbiddenScenario = scenarioById "message-known"
              faultRoot = pathOut paths </> "defect"
          createDirectoryIfMissing True faultRoot
          fault <- runSide attempt faultRoot selectedInputs faultScenario DefectCandidate
          forbidden <- runForbiddenBody attempt selectedInputs forbiddenScenario
          boundaryControls <- runBoundaryControls attempt selectedInputs
          counts <- ExecutionCounts <$> readIORef executed <*> readIORef reused <*> readIORef failed
          let attemptReport = report (optionCandidate options) (length selected) subset counts cachePath results faultScenario fault forbidden boundaryControls
              code = outcomeExitCode (reportStatus attemptReport)
          writeResults (pathOut paths) attemptReport
          pure (code, terminalMessage (pathOut paths) selected subset counts attemptReport)

runInventory :: FilePath -> IO ()
runInventory stockElf = do
  stock <- readInputFile "stock ELF" stockElf
  unless (sha256Hex (sha256 (B.take 5185120 (B.drop 0xe8 stock))) == stockImageSha256) $
    prerequisite "stock ELF does not contain the pinned image-5 bytes"
  functions <-
    case checkedStockFunctions (B.take 5185120 (B.drop 0xe8 stock)) of
      Left problem -> prerequisite ("stock inventory refused: " ++ problem)
      Right values -> pure values
  jsonPrint
    ( object
        [ "functions" .= map inventoryFunction functions
        , "required_premises" .= map returnPremiseLabel requiredReturnPremises
        ]
    )

main :: IO ()
main = do
  arguments <- getArgs
  case parseCommand arguments of
    Left message -> putStrLn ("media-permit: prerequisite: " ++ message) >> exitWith (ExitFailure 2)
    Right Help -> putStr usage >> exitWith ExitSuccess
    Right (Inventory stockElf) -> do
      outcome <- try (runInventory stockElf) :: IO (Either SomeException ())
      case outcome of
        Right () -> pure ()
        Left problem -> handleException problem
    Right (Qualify options) -> do
      outcome <- try (prepareAndRun options arguments) :: IO (Either SomeException (ExitCode, String))
      case outcome of
        Right (code, message) -> putStrLn message >> exitWith code
        Left problem -> handleException problem
  where
    handleException (problem :: SomeException) =
      case (fromException problem :: Maybe Prerequisite) of
        Just (Prerequisite message) -> putStrLn ("media-permit: prerequisite: " ++ message) >> exitWith (ExitFailure 2)
        Nothing ->
          case (fromException problem :: Maybe CacheEscape) of
            Just escape -> putStrLn ("media-permit: prerequisite: cache refused: " ++ show escape) >> exitWith (ExitFailure 2)
            Nothing ->
              case (fromException problem :: Maybe IOException) of
                Just problem' -> putStrLn ("media-permit: prerequisite: " ++ exceptionText problem') >> exitWith (ExitFailure 2)
                Nothing -> throwIO problem

runCases :: Attempt -> Inputs -> [Scenario] -> [(Scenario, Result, Result)] -> IO [(Scenario, Result, Result)]
runCases _ _ [] done = pure (reverse done)
runCases attempt inputs (scenario : rest) done = do
  original <- runSide attempt (attemptOut attempt) inputs scenario Original
  candidate <- runSide attempt (attemptOut attempt) inputs scenario Candidate
  let entry = (scenario, original, candidate)
      passed = resultStatus original == ResultPassed && resultStatus candidate == ResultPassed
  if passed then runCases attempt inputs rest (entry : done) else pure (reverse (entry : done))
