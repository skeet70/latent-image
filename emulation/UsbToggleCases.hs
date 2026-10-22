-- Native-harness cases for the USB personality toggle
-- (reports/usb-personality-toggle.md section 8.1), X-M1 1.50.
--
-- N1 runs the linked stub alone over the stock image-4 closures of its four
-- callees. N2 runs connect handler A [0x00ecf8b8, 0x00ecf9c8) on the patched
-- and the stock image 5 with register-only recording stubs at its boundary
-- calls. Expected registers, boundary calls and claimed bytes come from the
-- design's behaviour rule (toggledF7) and handler A's byte-read arms
-- (handlerA), fixed before any run. Stack bytes are pinned from a discovery
-- run of the same case: they are residue the exact-final-memory contract
-- requires, not claims.
module Main where

import Control.Monad
import Data.Aeson (Value)
import Data.Bits
import qualified Data.ByteString as B
import Data.Char (digitToInt, isHexDigit)
import Data.IORef
import Data.List (intercalate, sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe)
import Data.Word
import Emulation.NativeCase (runCase)
import FujiAF.Host
import FujiAF.JSON (decodeStrict, encodeCanonical)
import FujiAF.UsbToggle (Write (..), extentStart, hookCurrent, hookSite, image5Base, image5Sha256, stubCallees, toggleWrites)
import Numeric (readHex, showHex)
import System.Directory
import System.Environment
import System.Exit
import System.FilePath

-- ---------------------------------------------------------------------------
-- Inputs of the behaviour rule
-- ---------------------------------------------------------------------------

-- | The system mode word 0x009570dc as 0x0073c15c returns it.
data SystemMode = Ordinary | Playback | FirstStart | UsbCableBoot | Maintenance
  deriving (Eq, Ord, Show, Enum, Bounded)

modeWord :: SystemMode -> Word32
modeWord m = case m of
  Ordinary -> 1
  Playback -> 2
  FirstStart -> 3
  UsbCableBoot -> 5
  Maintenance -> 7

-- | Modes in which neither the stub nor handler A's service arms act.
serviceExcluded :: SystemMode -> Bool
serviceExcluded m = m == UsbCableBoot || m == Maintenance

-- | A one-bit level: the attach bit, the latch byte, the payload's bit 0.
data Level = Low | High deriving (Eq, Ord, Show, Enum, Bounded)

levelWord :: Level -> Word32
levelWord l = if l == High then 1 else 0

-- | Byte +0x15 of condition row 19 (KEY_Q), the key driver's debounced state.
data KeyQ = QReleased | QHeld deriving (Eq, Ord, Show, Enum, Bounded)

qByte :: KeyQ -> Word8
qByte q = if q == QHeld then 1 else 0

-- | cache[0xf7].
newtype F7 = F7 Word8 deriving (Eq, Ord, Show)

f7Byte :: F7 -> Word8
f7Byte (F7 v) = v

data Side = Stock | Patched deriving (Eq, Ord, Show, Enum, Bounded)

sideTag :: Side -> String
sideTag Stock = "stock"
sideTag Patched = "patched"

-- | The return of 0x0073abec on handler A's detach arm.
newtype DetachQuery = DetachQuery Word32 deriving (Eq, Ord, Show)

-- | The approved design: on the patched image, an attach edge with Q held in
-- a mode other than 5 and 7 flips cache[0xf7] (0 to 1, nonzero to 0); every
-- other input leaves the stored value. Stock never changes it.
toggledF7 :: Side -> Level -> KeyQ -> SystemMode -> F7 -> F7
toggledF7 side att q mode f7@(F7 v)
  | side == Patched && att == High && q == QHeld && not (serviceExcluded mode) = F7 (if v == 0 then 1 else 0)
  | otherwise = f7

-- ---------------------------------------------------------------------------
-- Handler A's boundary calls, byte-read from image 5 [0x00ecf8b8, 0x00ecf9c8)
-- ---------------------------------------------------------------------------

data TraceCode = Trace0b | Trace0e deriving (Eq, Show)

data AppEvent = AppAttach | AppDetach deriving (Eq, Show)

appCode :: AppEvent -> Word32
appCode AppAttach = 0x10
appCode AppDetach = 0x90

data Call
  = Trace TraceCode
  | TransportStart
  | TransportStop
  | RegisterDebug
  | DetachProbe
  | Application AppEvent
  deriving (Eq, Show)

data Boundary = BTrace | BStart | BStop | BRegister | BProbe | BApp deriving (Eq, Ord, Show, Enum, Bounded)

boundaryAddr :: Boundary -> Word32
boundaryAddr bd = case bd of
  BTrace -> 0x00723a14
  BStart -> 0x00ed117c
  BStop -> 0x00ed11a4
  BRegister -> 0x00647398
  BProbe -> 0x0073abec
  BApp -> 0x00ecf894

boundaryOf :: Call -> Boundary
boundaryOf c = case c of
  Trace _ -> BTrace
  TransportStart -> BStart
  TransportStop -> BStop
  RegisterDebug -> BRegister
  DetachProbe -> BProbe
  Application _ -> BApp

callText :: Call -> String
callText c = case c of
  Trace Trace0b -> "0x00723a14(0x24, 0xb)"
  Trace Trace0e -> "0x00723a14(0x24, 0xe)"
  TransportStart -> "0x00ed117c(7, 0) [queue-7 0x1705]"
  TransportStop -> "0x00ed11a4(7, 0) [queue-7 0x1706]"
  RegisterDebug -> "0x00647398(0x00ed19e4)"
  DetachProbe -> "0x0073abec()"
  Application e -> "0x00ecf894(" ++ hx (appCode e) ++ ", 0)"

data N2In = N2In {nLatch :: Level, nBit :: Level, nF7 :: F7, nQ :: KeyQ, nMode :: SystemMode, nProbe :: DetachQuery}
  deriving (Eq, Ord, Show)

data N2Out = N2Out {oCalls :: [Call], oLatch :: Level, oF7 :: F7}

-- | 0x00ecf8c4 trace; 0x00ecf8e8 equal-latch return; 0x00ecf8f8 latch store;
-- 0x00ecf900 cache[0xf7] read (the hook site); 0x00ecf910 system mode;
-- 0x00ecf93c attach arm; 0x00ecf964 detach arm; 0x00ecf99c application arm.
handlerA :: Side -> N2In -> N2Out
handlerA side i
  | att == nLatch i = N2Out [Trace Trace0b] (nLatch i) (nF7 i)
  | f7' == F7 0 || serviceExcluded (nMode i) = N2Out [Trace Trace0b, Application (if att == High then AppAttach else AppDetach)] att f7'
  | att == High = N2Out [Trace Trace0b, Trace Trace0e, TransportStart, RegisterDebug] att f7'
  | nProbe i /= DetachQuery 0 = N2Out [Trace Trace0b, DetachProbe, Application AppDetach] att f7'
  | otherwise = N2Out [Trace Trace0b, DetachProbe, TransportStop, Trace Trace0b] att f7'
  where
    att = nBit i
    f7' = toggledF7 side att (nQ i) (nMode i) (nF7 i)

-- ---------------------------------------------------------------------------
-- Case model
-- ---------------------------------------------------------------------------

data Gpr = R0 | R1 | R2 | R3 | R4 | R5 | R6 | R7 | R8 | R9 | R10 | R11 | R12 | SP | LR | PC | CPSR
  deriving (Eq, Ord, Show, Enum, Bounded)

gprName :: Gpr -> String
gprName g = case g of
  SP -> "sp"
  LR -> "lr"
  PC -> "pc"
  CPSR -> "cpsr"
  _ -> "r" ++ show (fromEnum g)

data Perm = PermR | PermRW | PermRX deriving (Eq, Show)

permText :: Perm -> String
permText p = case p of
  PermR -> "r"
  PermRW -> "rw"
  PermRX -> "rx"

data Reg = Reg {rgId :: String, rgAddr :: Word32, rgSize :: Int, rgPerm :: Perm, rgInit :: B.ByteString, rgSrc :: String}

data Stub = Stub {stAddr :: Word32, stIn :: [(Gpr, Word32)], stOut :: [(Gpr, Word32)]}

data Family = N1Matrix | N1Bitmap | N2Patched | N2Stock deriving (Eq, Ord, Show, Enum, Bounded)

familyText :: Family -> String
familyText f = case f of
  N1Matrix -> "N1 stub matrix"
  N1Bitmap -> "N1 UI-bitmap edge"
  N2Patched -> "N2 handler A, patched"
  N2Stock -> "N2 handler A, stock control"

data Case = Case
  { cId :: String
  , cFamily :: Family
  , cProv :: String
  , cEntry :: Word32
  , cArgs :: [(Gpr, Word32)]
  , cRegs :: [Reg]
  , cFns :: [(Word32, Int)]
  , cStubs :: [Stub]
  , cExpR0 :: Word32
  , cClaims :: [(String, Int, B.ByteString)]
  , cEvents :: [Stub]
  , cExpect :: String
  , cExcl :: [String]
  , cInputs :: [(String, String)]
  , cN2 :: Maybe (Side, N2In)
  }

exitA, stackBase, stackTop :: Word32
exitA = 0x00001080
stackBase = 0x00003000
stackTop = 0x00003400

stackReg :: Reg
stackReg = Reg "stack" stackBase 0x400 PermRW B.empty "zero-filled harness stack, SP 0x00003400"

baseRegs :: [Reg]
baseRegs = [stackReg, Reg "exit" exitA 4 PermRX B.empty "exit sentinel"]

allRegs :: Case -> [Reg]
allRegs c = cRegs c ++ baseRegs

initialRegs :: Case -> [(Gpr, Word32)]
initialRegs c = [(r, fromMaybe 0 (lookup r (cArgs c))) | r <- [R0 .. R12]] ++ [(SP, stackTop), (LR, exitA), (CPSR, 0x10)]

-- Callee-saved r4-r11 and SP restored, return to the exit address.
expectedRegs :: Case -> [(Gpr, Word32)]
expectedRegs c = M.toList (M.fromList ([(r, v) | (r, v) <- initialRegs c, r `elem` [R4 .. R11] ++ [SP]] ++ [(PC, exitA), (R0, cExpR0 c)]))

regsJ :: [(Gpr, Word32)] -> Value
regsJ rs = obj [(gprName k, s (h32 v)) | (k, v) <- rs]

stubJ :: Stub -> Value
stubJ (Stub a i o) = obj [("address", s (h32 a)), ("inputs", regsJ i), ("outputs", regsJ o), ("side_effects", s "registers-only")]

eventJ :: Stub -> Value
eventJ (Stub a i o) = obj [("address", s (h32 a)), ("inputs", regsJ i), ("outputs", regsJ o)]

-- ---------------------------------------------------------------------------
-- Bytes
-- ---------------------------------------------------------------------------

data Img = Img {iName :: String, iPath :: FilePath, iBase :: Word32, iBytes :: B.ByteString, iSha :: String}

loadImg :: String -> FilePath -> Word32 -> String -> IO Img
loadImg name path base pin = do
  bytes <- B.readFile path
  digest <- sha256 bytes
  ensure (digest == pin) (path ++ ": SHA-256 " ++ digest ++ " differs from the pin " ++ pin)
  pure (Img name path base bytes digest)

sl :: Img -> Word32 -> Int -> B.ByteString
sl img a len
  | a < iBase img || off + len > B.length (iBytes img) = error (iName img ++ ": slice " ++ h32 a ++ "+" ++ show len ++ " outside the image")
  | otherwise = B.take len (B.drop off (iBytes img))
  where
    off = fromIntegral (a - iBase img)

wd :: Img -> Word32 -> Word32
wd img a = le32At (sl img a 4) 0

le32At :: B.ByteString -> Int -> Word32
le32At bs o = foldr (\k acc -> acc .|. (fromIntegral (B.index bs (o + k)) `shiftL` (8 * k))) 0 [0 .. 3]

le32 :: Word32 -> B.ByteString
le32 v = B.pack [fromIntegral (v `shiftR` k) | k <- [0, 8, 16, 24]]

h32 :: Word32 -> String
h32 v = let t = showHex v "" in "0x" ++ replicate (8 - length t) '0' ++ t

hx :: Word32 -> String
hx v = "0x" ++ showHex v ""

hexBs :: B.ByteString -> String
hexBs = concatMap (\x -> let t = showHex x "" in replicate (2 - length t) '0' ++ t) . B.unpack

unhex :: String -> B.ByteString
unhex = B.pack . go
  where
    go (a : c : rest) | isHexDigit a && isHexDigit c = fromIntegral (digitToInt a * 16 + digitToInt c) : go rest
    go _ = []

parseU32 :: String -> Maybe Word32
parseU32 ('0' : 'x' : t) | [(v, "")] <- readHex t = Just v
parseU32 _ = Nothing

-- | Replaces the bytes at an address of an image.
poke :: Img -> Word32 -> B.ByteString -> B.ByteString
poke img a new = let off = fromIntegral (a - iBase img) in B.take off (iBytes img) <> new <> B.drop (off + B.length new) (iBytes img)

-- ---------------------------------------------------------------------------
-- Pins, layout and world
-- ---------------------------------------------------------------------------

stockId4Sha, stubSha :: String
stockId4Sha = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
stubSha = "35a7e9597f06bdc1dda250d991df4a468d2401e5ae3c24c22e37403318746d62"

handlerAStart :: Word32
handlerAStart = 0x00ecf8b8

handlerALen :: Int
handlerALen = 0x110

-- | Stock image-4 functions the stub and handler A reach, with their extents.
cacheByteFn, cacheStoreFn, rowSearchFn, keyStateFn, modeStoreFn, modeFn :: (Word32, Int)
cacheByteFn = (0x0069f8fc, 0x24)
cacheStoreFn = (0x0069fc94, 0x24)
rowSearchFn = (0x006a0b4c, 0x64)
keyStateFn = (0x006a0d00, 0x40)
modeStoreFn = (0x0073aff0, 0x1c)
modeFn = (0x0073c15c, 0x20)

cachePtrCell, cachePage, modeCell, rowTable, bitmapBase, usbState, payloadAddr :: Word32
cachePtrCell = 0x0096ca50
cachePage = 0x30000000
modeCell = 0x009570dc
rowTable = 0x00828120
bitmapBase = 0x0094f8b8
usbState = 0x015c20c4
payloadAddr = 0x31000000

rowCount, rowStride, qRow, latchOffset, f7Index :: Int
rowCount = 36
rowStride = 28
qRow = 19
latchOffset = 0x1d8
f7Index = 0xf7

keyQ :: Word32
keyQ = 0x32

-- | The payload's bits above bit 0, which handler A masks off.
payloadHigh :: Word32
payloadHigh = 0xa5a5a5a4

data World = World {wId4 :: Img, wId5 :: Img, wP5 :: Img, wStub :: B.ByteString, wStubPath :: FilePath, wPatchSets :: [FilePath]}

data Opts = Opts {oOut :: FilePath, oRunner :: FilePath, oStub :: FilePath, oPrivate :: FilePath, oPatchSets :: [FilePath]}

defaults :: Opts
defaults =
  Opts
    { oOut = "private-data/analysis/x-m1-1.50/native-case/usb-toggle"
    , oRunner = "bazel-bin/emulation/native-runner"
    , oStub = "bazel-bin/host/firmware/usb-toggle-stub.bin"
    , oPrivate = "private-data/analysis/x-m1-1.50"
    , oPatchSets = []
    }

defaultPatchSets :: [FilePath]
defaultPatchSets =
  [ "private-data/analysis/usb-toggle/cc-pinned/usb-toggle-image5.json"
  , "private-data/analysis/usb-toggle/rc0-pinned/usb-toggle-image5.json"
  ]

parseOpts :: Opts -> [String] -> IO Opts
parseOpts o [] = pure o
parseOpts o (flag : v : rest) = case flag of
  "--out" -> parseOpts o {oOut = v} rest
  "--runner" -> parseOpts o {oRunner = v} rest
  "--stub" -> parseOpts o {oStub = v} rest
  "--private" -> parseOpts o {oPrivate = v} rest
  "--patch-set" -> parseOpts o {oPatchSets = oPatchSets o ++ [v]} rest
  _ -> failH ("unknown option " ++ flag)
parseOpts _ [flag] = failH ("option " ++ flag ++ " needs a value")

loadWorld :: Opts -> IO World
loadWorld o = do
  let p = oPrivate o
  id4 <- loadImg "id4-stock" (p </> "lz/id4-0061c400.bin") 0x0061c400 stockId4Sha
  id5 <- loadImg "id5-stock" (p </> "lz/id5-00bf1a00.bin") image5Base image5Sha256
  stub <- B.readFile (oStub o)
  digest <- sha256 stub
  ensure (digest == stubSha) (oStub o ++ ": SHA-256 " ++ digest ++ " differs from the pin " ++ stubSha)
  writes <- either failH pure (toggleWrites stub (sl id5 extentStart 0x90))
  let patched = foldl (\bytes w -> poke id5 {iBytes = bytes} (wAddress w) (wNew w)) (iBytes id5) writes
  pd <- sha256 patched
  let sets = if null (oPatchSets o) then defaultPatchSets else oPatchSets o
  pure (World id4 id5 (Img "id5-patched" (iPath id5 ++ " with the FujiAF.UsbToggle writes of " ++ oStub o) image5Base patched pd) stub (oStub o) sets)

-- | Contract checks the expectations rest on.
checkWorld :: World -> IO ()
checkWorld w = do
  ensure (wd (wId5 w) hookSite == hookCurrent) "stock hook word"
  ensure (sl (wP5 w) extentStart (B.length (wStub w)) == wStub w) "patched stub bytes"
  ensure (B.length (wStub w) == 88) "stub length 88"
  ensure (stubCallees == map fst [cacheByteFn, cacheStoreFn, keyStateFn, modeFn]) "stub callee list"
  ensure
    (and [sl (wId5 w) a 1 == sl (wP5 w) a 1 | a <- [handlerAStart, handlerAStart + 1 .. handlerAStart + fromIntegral handlerALen - 1], a < hookSite || a >= hookSite + 4])
    "handler A differs from stock outside the hook word"
  let rowCode k = wd (wId4 w) (rowTable + fromIntegral (rowStride * k))
  ensure (rowCode qRow == keyQ && notElem keyQ (map rowCode [0 .. qRow - 1])) "row 19 is the first condition row with code 0x32"
  forM_ (wPatchSets w) $ \path -> do
    raw <- B.readFile path
    v <- either failH pure (decodeStrict raw)
    let blobs = [x | x <- list (get v "blobs"), str (get x "id") == "usb-toggle-stub"]
        hooks = [x | x <- list (get v "words"), str (get x "address") == h32 hookSite]
    ensure (str (get (get v "image") "decode_sha256") == image5Sha256) (path ++ ": image-5 decode pin")
    case (blobs, hooks) of
      ([bl], [hk]) -> do
        ensure (str (get bl "address") == h32 extentStart && unhex (str (get bl "new_hex")) == wStub w) (path ++ ": stub blob differs from " ++ wStubPath w)
        ensure (parseU32 (str (get hk "new")) == Just (wd (wP5 w) hookSite)) (path ++ ": hook word differs")
      _ -> failH (path ++ ": no single usb-toggle-stub blob and hook word")

imgInput :: Img -> (String, String)
imgInput img = (iName img, iPath img ++ " (SHA-256 " ++ iSha img ++ ", base " ++ h32 (iBase img) ++ ")")

-- ---------------------------------------------------------------------------
-- Shared regions
-- ---------------------------------------------------------------------------

codeReg :: String -> Img -> Word32 -> Int -> Reg
codeReg rid img a len = Reg rid a len PermRX (sl img a len) (iName img ++ " [" ++ h32 a ++ ", " ++ h32 (a + fromIntegral len) ++ ")")

-- | One rx region per cluster of the stock image-4 functions.
id4Regs :: Img -> [Reg]
id4Regs id4 =
  [ codeReg "id4-cache" id4 0x0069f8fc 0x3bc
  , codeReg "id4-key" id4 0x006a0b4c 0x1f4
  , codeReg "id4-mode" id4 0x0073aff0 0x118c
  ]

stubReg :: World -> Reg
stubReg w = Reg "stub" extentStart (B.length (wStub w)) PermRX (wStub w) ("usb_toggle, " ++ wStubPath w ++ " (SHA-256 " ++ stubSha ++ ")")

fillByte :: Word8
fillByte = 0x5a

dataRegs :: Img -> SystemMode -> F7 -> KeyQ -> [Reg]
dataRegs id4 mode f7 q =
  [ Reg "cache-ptr" cachePtrCell 4 PermR (le32 cachePage) "cache pointer cell 0x0096ca50 = 0x30000000"
  , Reg "cache" cachePage 0x100 PermRW (B.replicate f7Index fillByte <> B.singleton (f7Byte f7) <> B.replicate (0x100 - f7Index - 1) fillByte) ("synthetic cache page, 0x5a except cache[0xf7] = " ++ show (f7Byte f7))
  , Reg "mode" modeCell 4 PermR (le32 (modeWord mode)) ("system mode word 0x009570dc = " ++ show (modeWord mode))
  , Reg "rows" rowTable (rowCount * rowStride) PermR rows ("id4-stock [0x00828120, 0x00828510) with row 19 +0x15 = " ++ show (qByte q))
  ]
  where
    stockRows = sl id4 rowTable (rowCount * rowStride)
    qOff = qRow * rowStride + 0x15
    rows = B.take qOff stockRows <> B.singleton (qByte q) <> B.drop (qOff + 1) stockRows

f7Text :: F7 -> String
f7Text = show . f7Byte

commonExclusions :: [String]
commonExclusions =
  [ "No camera, USB device or full-system emulator is used: the case reads host files and runs the bounded Unicorn Cortex-R5F engine only."
  , "The R5F engine stands for the inferred R4F-class core only inside the admitted instruction set: no coprocessor, MPU, TCM, cache, interrupt, exception or timing behaviour is modelled and the A32 SDIV/UDIV refusal is unchanged."
  , "Q is byte +0x15 of condition row 19 set directly; the key driver, the debounce step 0x006a11f4 and ADC sampling are outside the case."
  , "The cache pointer, cache page and system mode word are synthetic inputs; the boot that forms them is outside the case."
  , "Patched image-5 bytes are the stock decode with the FujiAF.UsbToggle writes, equal to the stub blob and hook word of the pinned CC and RC0 image-5 patch sets; they are not read from a body."
  ]

-- ---------------------------------------------------------------------------
-- N1: the stub alone
-- ---------------------------------------------------------------------------

n1Fns :: [(Word32, Int)]
n1Fns = [(extentStart, 88), cacheByteFn, cacheStoreFn, rowSearchFn, keyStateFn, modeFn]

n1Case :: World -> Family -> Word32 -> F7 -> Level -> KeyQ -> SystemMode -> Case
n1Case w fam bitmap f7 att q mode =
  Case
    { cId = intercalate "-" (["n1", if fam == N1Bitmap then "bitmap-" ++ showHex bitmap "" else "m"] ++ ["f7", f7Text f7, "r1", show (levelWord att), "q", show (qByte q), "mode", show (modeWord mode)])
    , cFamily = fam
    , cProv = "usb_toggle (0x00bf4454, " ++ wStubPath w ++ ") entered directly with r0 = 0xf7, r1 = attach bit " ++ show (levelWord att) ++ ", over the stock image-4 0x0069f8fc, 0x0069fc94, 0x006a0d00 (with 0x006a0b4c and the 36-row table 0x00828120) and 0x0073c15c; UI key bitmap word 0x0094f8bc = " ++ h32 bitmap ++ "."
    , cEntry = extentStart
    , cArgs = [(R0, fromIntegral f7Index), (R1, levelWord att)]
    , cRegs =
        [stubReg w]
          ++ id4Regs (wId4 w)
          ++ dataRegs (wId4 w) mode f7 q
          ++ [Reg "bitmap" bitmapBase 8 PermR (le32 0 <> le32 bitmap) ("UI key bitmap 0x0094f8b8 = 0, 0x0094f8bc = " ++ h32 bitmap)]
    , cFns = n1Fns
    , cStubs = []
    , cExpR0 = fromIntegral (f7Byte f7')
    , cClaims = [("cache", f7Index, B.singleton (f7Byte f7'))]
    , cEvents = []
    , cExpect = "r0 = cache[0xf7] = " ++ f7Text f7' ++ (if f7' == f7 then " (unchanged)" else " (flipped)") ++ "; no other byte outside the stack changes."
    , cExcl = ["The stub is entered directly with r0 = 0xf7, not through the hook at 0x00ecf900; handler A is the N2 family."]
    , cInputs = [imgInput (wId4 w), ("stub", wStubPath w ++ " (SHA-256 " ++ stubSha ++ ")")]
    , cN2 = Nothing
    }
  where
    f7' = toggledF7 Patched att q mode f7

n1Cases :: World -> [Case]
n1Cases w =
  [n1Case w N1Matrix 0 f7 att q mode | f7 <- [F7 0, F7 1], att <- [Low, High], q <- [QReleased, QHeld], mode <- [minBound .. maxBound]]
    ++ [n1Case w N1Bitmap bitmap f7 High q Ordinary | f7 <- [F7 0, F7 1], (bitmap, q) <- [(bit 18, QReleased), (complement (bit 18), QHeld)]]

-- ---------------------------------------------------------------------------
-- N2: handler A
-- ---------------------------------------------------------------------------

-- | The recording stub at each boundary for one case's expected calls. Stubs
-- are matched by address, so the trace stub pins r1 = 0xb only when no 0xe
-- trace is expected, and the 0x00ecf894 stub pins r0 only when a call is
-- expected.
boundaryStub :: N2In -> [Call] -> Boundary -> Stub
boundaryStub i calls bd = case bd of
  BTrace -> Stub a ((R0, 0x24) : [(R1, 0xb) | Trace Trace0e `notElem` calls]) [(R0, 0)]
  BStart -> Stub a [(R0, 7), (R1, 0)] [(R0, 0)]
  BStop -> Stub a [(R0, 7), (R1, 0)] [(R0, 0)]
  BRegister -> Stub a [(R0, 0x00ed19e4)] [(R0, 0)]
  BProbe -> let DetachQuery v = nProbe i in Stub a [] [(R0, v)]
  BApp -> Stub a ([(R0, appCode e) | Application e <- take 1 [c | c@(Application _) <- calls]] ++ [(R1, 0)]) [(R0, 0)]
  where
    a = boundaryAddr bd

n2Case :: World -> Side -> N2In -> Case
n2Case w side i =
  Case
    { cId = intercalate "-" ["n2", sideTag side, "l", show (levelWord (nLatch i)), "b", show (levelWord (nBit i)), "f7", f7Text (nF7 i), "q", show (qByte (nQ i)), "mode", show (modeWord (nMode i)), "abec", show probe]
    , cFamily = if side == Patched then N2Patched else N2Stock
    , cProv =
        "Connect handler A [0x00ecf8b8, 0x00ecf9c8) of "
          ++ iName img5
          ++ " entered directly; latch 0x015c229c = "
          ++ show (levelWord (nLatch i))
          ++ ", payload word "
          ++ h32 payload
          ++ " through *(0x015c20cc) = 0x31000000, 0x0073abec returns "
          ++ show probe
          ++ "; 0x0073aff0 and 0x0073c15c run as stock image-4 bytes"
          ++ (if side == Patched then ", and the hook calls usb_toggle over the stock 0x0069f8fc, 0x0069fc94, 0x006a0d00 and 0x006a0b4c." else "; the f7 read is the stock bl 0x0069f8fc.")
    , cEntry = handlerAStart
    , cArgs = []
    , cRegs =
        [codeReg "handler-a" img5 handlerAStart handlerALen]
          ++ [stubReg w | side == Patched]
          ++ id4Regs (wId4 w)
          ++ dataRegs (wId4 w) (nMode i) (nF7 i) (nQ i)
          ++ [ Reg "usb-state" usbState (latchOffset + 4) PermRW usbBytes ("synthetic USB-manager state 0x015c20c4, 0x5a except +0x8 = 0x31000000 and the latch +0x1d8 = " ++ show (levelWord (nLatch i)))
             , Reg "payload" payloadAddr 4 PermR (le32 payload) ("payload word " ++ h32 payload)
             ]
    , cFns = sortOn fst ([(handlerAStart, handlerALen), cacheByteFn, modeStoreFn, modeFn] ++ (if side == Patched then [(extentStart, 88), cacheStoreFn, rowSearchFn, keyStateFn] else []))
    , cStubs = [boundaryStub i calls bd | bd <- [minBound .. maxBound]]
    , cExpR0 = 0
    , cClaims = [("usb-state", latchOffset, B.singleton (fromIntegral (levelWord (oLatch out)))), ("cache", f7Index, B.singleton (f7Byte (oF7 out)))]
    , cEvents = [boundaryStub i calls (boundaryOf c) | c <- calls]
    , cExpect = "calls " ++ intercalate ", " (map callText calls) ++ "; latch " ++ show (levelWord (oLatch out)) ++ ", cache[0xf7] " ++ f7Text (oF7 out) ++ ", r0 0."
    , cExcl =
        [ "Handler A is entered directly; the observer 0x00ecfe64, the queue-7 0x1714 dispatch and the group-1 state are outside the case."
        , "The boundary calls are register-only recording stubs; the trace sink, the queue-7 posts 0x1705/0x1706, the debug registration, 0x0073abec's state and the application event post are not executed."
        , "Stubs match by address: where the expected sequence has both trace calls (attach arm) the trace stub checks r0 = 0x24 only, not r1."
        ]
    , cInputs = [imgInput img5, imgInput (wId4 w)] ++ [("stub", wStubPath w ++ " (SHA-256 " ++ stubSha ++ ")") | side == Patched]
    , cN2 = Just (side, i)
    }
  where
    img5 = if side == Patched then wP5 w else wId5 w
    out = handlerA side i
    calls = oCalls out
    DetachQuery probe = nProbe i
    payload = payloadHigh .|. levelWord (nBit i)
    usbBytes = B.replicate 8 fillByte <> le32 payloadAddr <> B.replicate (latchOffset - 12) fillByte <> B.singleton (fromIntegral (levelWord (nLatch i))) <> B.replicate 3 fillByte

n2Inputs :: [N2In]
n2Inputs =
  [ N2In l att f7 q mode (DetachQuery probe)
  | l <- [Low, High]
  , att <- [Low, High]
  , f7 <- [F7 0, F7 1]
  , q <- [QReleased, QHeld]
  , mode <- [minBound .. maxBound]
  , probe <- [0, 1]
  ]

-- ---------------------------------------------------------------------------
-- Running
-- ---------------------------------------------------------------------------

data Env = Env {eOut :: FilePath, eRunner :: FilePath, eArts :: IORef (M.Map B.ByteString (String, String))}

artifactFor :: Env -> B.ByteString -> IO (String, String)
artifactFor env bytes = do
  seen <- readIORef (eArts env)
  case M.lookup bytes seen of
    Just x -> pure x
    Nothing -> do
      digest <- sha256 bytes
      let aid = "a-" ++ take 24 digest
          path = eOut env </> aid ++ ".bin"
      present <- doesFileExist path
      old <- if present then B.readFile path else pure B.empty
      when (old /= bytes) (B.writeFile path bytes)
      modifyIORef' (eArts env) (M.insert bytes (aid, digest))
      pure (aid, digest)

provenanceText :: Case -> String
provenanceText c =
  cProv c
    ++ " Expected before execution from the behaviour rule: "
    ++ cExpect c
    ++ " Inputs: "
    ++ intercalate "; " [k ++ " " ++ v | (k, v) <- cInputs c]
    ++ ". Regions: "
    ++ intercalate "; " [rgId r ++ " " ++ h32 (rgAddr r) ++ " " ++ permText (rgPerm r) ++ " = " ++ rgSrc r | r <- allRegs c]
    ++ ". Stack bytes are pinned from a discovery run of this case and are residue, not claims."

caseValue :: Case -> [(Reg, Maybe (String, String))] -> [(String, Int, B.ByteString)] -> Value
caseValue c regArts memory =
  obj
    [ ("schema_version", n 1)
    , ("id", s (cId c))
    , ("origin", s "stock")
    , ("provenance", s (provenanceText c))
    , ("cpu", obj [("architecture", s "arm"), ("model", s "cortex-r5f"), ("mode", s "arm"), ("endian", s "little")])
    , ("artifacts", arr [obj [("id", s aid), ("path", s (aid ++ ".bin")), ("size", n (toInteger len)), ("sha256", s d)] | (aid, d, len) <- uniqueArts])
    , ("entry", s (h32 (cEntry c)))
    , ("exit", s (h32 exitA))
    , ("registers", regsJ (initialRegs c))
    , ("regions", arr (map regionJ regArts))
    , ("functions", arr [obj [("address", s (h32 a)), ("size", n (toInteger z))] | (a, z) <- sortOn fst (cFns c)])
    , ("stubs", arr (map stubJ (cStubs c)))
    , ("limits", obj [("instructions", n 20000), ("wall_ms", n 5000)])
    , ( "expected"
      , obj
          [ ("status", s "returned")
          , ("registers", regsJ (expectedRegs c))
          , ("memory", arr [obj [("region", s rid), ("offset", n (toInteger off)), ("hex", s (hexBs bs))] | (rid, off, bs) <- memory])
          , ("events", arr (map eventJ (cEvents c)))
          ]
      )
    , ("exclusions", arr (map s (cExcl c ++ commonExclusions)))
    ]
  where
    uniqueArts = M.elems (M.fromList [(aid, (aid, d, B.length (rgInit r))) | (r, Just (aid, d)) <- regArts])
    regionJ (r, art) =
      obj
        [ ("id", s (rgId r))
        , ("address", s (h32 (rgAddr r)))
        , ("size", n (toInteger (rgSize r)))
        , ("permissions", s (permText (rgPerm r)))
        , ("initial", maybe (obj [("zero", b True)]) (\(aid, _) -> obj [("artifact", s aid), ("offset", n 0), ("length", n (toInteger (B.length (rgInit r))))]) art)
        ]

finalsOf :: Case -> Value -> IO (M.Map String B.ByteString)
finalsOf c report = do
  let mem = list (get (get report "runner") "memory")
  ensure (length mem == length (allRegs c)) (cId c ++ ": report has no final memory (" ++ str (get report "status") ++ " " ++ str (get report "reason") ++ ")")
  pure (M.fromList [(rgId r, unhex (str (get m "bytes"))) | (r, m) <- zip (allRegs c) mem])

initialBytes :: Reg -> B.ByteString
initialBytes r = rgInit r <> B.replicate (rgSize r - B.length (rgInit r)) 0

-- | Spans of the stack whose final bytes differ from the initial ones.
stackResidue :: Reg -> B.ByteString -> [(String, Int, B.ByteString)]
stackResidue r final =
  [ (rgId r, a, B.take (z - a) (B.drop a final))
  | a : _ <- [diffs]
  , let z = 1 + last diffs
  ]
  where
    ini = initialBytes r
    diffs = [k | k <- [0 .. rgSize r - 1], B.index final k /= B.index ini k]

-- | Bytes below the entry SP touched by any recorded access to the stack.
stackDepth :: Value -> Maybe Word32
stackDepth report =
  case [a | e <- list (get (get report "runner") "memory_provenance"), Just a <- [parseU32 (str (get e "address"))], a >= stackBase, a < stackTop] of
    [] -> Nothing
    as -> Just (stackTop - minimum as)

data Outcome = Outcome {oCase :: Case, oStatus :: String, oRunnerStatus :: String, oR0 :: String, oInstr :: Integer, oMismatch :: [String], oDepth :: Maybe Word32, oEvents :: Value, oFinals :: M.Map String B.ByteString, oProvTotal :: Integer}

runOne :: Env -> Case -> IO Outcome
runOne env c = do
  regArts <- forM (allRegs c) $ \r -> if B.null (rgInit r) then pure (r, Nothing) else (,) r . Just <$> artifactFor env (rgInit r)
  let path = eOut env </> cId c ++ ".json"
  B.writeFile path (encodeCanonical (caseValue c regArts (cClaims c)))
  (_, r1) <- runCase (eRunner env) path
  own <- finalsOf c r1
  B.writeFile path (encodeCanonical (caseValue c regArts (cClaims c ++ stackResidue stackReg (own M.! "stack"))))
  (_, r2) <- runCase (eRunner env) path
  B.writeFile (eOut env </> cId c ++ "-report.json") (encodeCanonical r2)
  finals <- finalsOf c r2
  let runner = get r2 "runner"
  pure
    Outcome
      { oCase = c
      , oStatus = str (get r2 "status")
      , oRunnerStatus = str (get runner "status")
      , oR0 = str (get (get runner "registers") "r0")
      , oInstr = num (get runner "instructions")
      , oMismatch = map str (list (get r2 "mismatches"))
      , oDepth = stackDepth r2
      , oEvents = get runner "events"
      , oFinals = finals
      , oProvTotal = num (get runner "memory_provenance_total")
      }

-- | Q = 0 pairs: the observed boundary events, r0, latch and cache byte of
-- each patched case against the stock case with the same inputs.
q0Pairs :: [Outcome] -> [(N2In, Bool)]
q0Pairs outcomes =
  [ (i, oEvents p == oEvents st && oR0 p == oR0 st && observed p == observed st)
  | p <- outcomes
  , Just (Patched, i) <- [cN2 (oCase p)]
  , nQ i == QReleased
  , st <- [x | x <- outcomes, cN2 (oCase x) == Just (Stock, i)]
  ]
  where
    observed o = (B.index (oFinals o M.! "usb-state") latchOffset, B.index (oFinals o M.! "cache") f7Index)

main :: IO ()
main = do
  o0 <- getArgs >>= parseOpts defaults
  runner <- makeAbsolute (oRunner o0)
  stub <- makeAbsolute (oStub o0)
  lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= mapM_ setCurrentDirectory
  let o = o0 {oRunner = runner, oStub = stub}
  w <- loadWorld o
  checkWorld w
  let out = oOut o
  createDirectoryIfMissing True out
  arts <- newIORef M.empty
  let env = Env out runner arts
      cases = n1Cases w ++ [n2Case w side i | side <- [Patched, Stock], i <- n2Inputs]
  outcomes <- forM cases $ \c -> do
    r <- runOne env c
    putStrLn (oStatus r ++ " " ++ cId c ++ " runner=" ++ oRunnerStatus r ++ " r0=" ++ oR0 r ++ " instructions=" ++ show (oInstr r) ++ concatMap (" " ++) (oMismatch r))
    pure r
  let depthText = maybe "none" (\d -> show d ++ " bytes")
      row r = "| " ++ cId (oCase r) ++ " | " ++ familyText (cFamily (oCase r)) ++ " | " ++ cExpect (oCase r) ++ " | " ++ oStatus r ++ ", " ++ oRunnerStatus r ++ ", r0 " ++ oR0 r ++ ", " ++ show (oInstr r) ++ " instructions, stack depth " ++ depthText (oDepth r) ++ " |"
      famOutcomes f = [r | r <- outcomes, cFamily (oCase r) == f]
      passed = filter ((== "passed") . oStatus)
      counts = [(f, length (passed (famOutcomes f)), length (famOutcomes f)) | f <- [minBound .. maxBound]]
      maxDepth f = maximum (0 : [d | r <- famOutcomes f, Just d <- [oDepth r]])
      truncated = [cId (oCase r) | r <- outcomes, oProvTotal r > 4096]
      pairs = q0Pairs outcomes
      equal = length (filter snd pairs)
  ensure (null truncated) ("memory provenance truncated: " ++ unwords truncated)
  writeFile (out </> "results.md") (unlines (["| Case | Family | Expected before execution | Observed |", "| --- | --- | --- | --- |"] ++ map row outcomes))
  B.writeFile
    (out </> "results.json")
    ( encodeCanonical
        ( obj
            [ ("families", arr [obj [("family", s (familyText f)), ("passed", n (toInteger p)), ("cases", n (toInteger t)), ("max_stack_depth", n (toInteger (maxDepth f)))] | (f, p, t) <- counts])
            , ("q0_stock_patched_pairs", n (toInteger (length pairs)))
            , ("q0_stock_patched_identical", n (toInteger equal))
            , ("q0_differing", arr [s (show i) | (i, False) <- pairs])
            , ( "cases"
              , arr
                  [ obj
                      [ ("id", s (cId (oCase r)))
                      , ("family", s (familyText (cFamily (oCase r))))
                      , ("expected", s (cExpect (oCase r)))
                      , ("status", s (oStatus r))
                      , ("runner_status", s (oRunnerStatus r))
                      , ("r0", s (oR0 r))
                      , ("instructions", n (oInstr r))
                      , ("stack_depth", maybe (s "none") (n . toInteger) (oDepth r))
                      , ("mismatches", arr (map s (oMismatch r)))
                      ]
                  | r <- outcomes
                  ]
              )
            ]
        )
    )
  forM_ counts $ \(f, p, t) -> putStrLn (familyText f ++ ": " ++ show p ++ "/" ++ show t ++ " passed, max stack depth " ++ show (maxDepth f) ++ " bytes")
  putStrLn ("Q = 0 stock/patched: " ++ show equal ++ "/" ++ show (length pairs) ++ " pairs with identical boundary events, r0, latch and cache[0xf7]")
  let failed = [r | r <- outcomes, oStatus r /= "passed"]
  putStrLn (show (length outcomes - length failed) ++ "/" ++ show (length outcomes) ++ " cases passed; cases and reports in " ++ out)
  unless (null failed && equal == length pairs && length pairs == 80) exitFailure
