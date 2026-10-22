{-# LANGUAGE OverloadedStrings #-}
-- Classic Chrome develop-builder cases (X-M1 1.50, host-only).
--
-- Executes the stock still develop path in two chained stages on the bounded
-- native runner and compares hardware parameter blocks:
--
--   stage a  driver -> FUN_00d77abc (record memset) -> FUN_00d791b0 (record
--            init) -> FUN_00d79408 (record from a request, request[9] = col2)
--            -> record +0xf0 = 0 and +0x140 = 0 as FUN_00e561b4 stores them
--            -> FUN_00d77a48 (preset expander, preset key 2, level 0)
--   stage b  FUN_00d5da2c(block, record), the record being stage a's final
--            record bytes
--
-- The driver is the only synthetic code: A32 calls and stores at 0x01f00000,
-- within BL range of image 5. No stubs. Every executed stock byte is an exact slice of the
-- pinned images or of a patched build named on the command line.
--
-- Comparisons (a) patched col2 0..12 equal stock, (b) the +0x110 = 0xd marker
-- on stock leaves the block unchanged, (c) patched col2 0xd equals the stock
-- PROVIA block outside the colour, chroma and tone output ranges and a stock
-- substitution oracle inside them. Each case is written with its expected
-- final state and rerun through the native-case adapter.
--
-- A RAM set (--ram-set) is the RC0 decodes with a CC RAM-push manifest's
-- publications applied and the runtime's CC code and data block mapped; it
-- takes (a) and (c) and its CC block must equal its flash set's.
--
-- The movie/live-view path (stage a "lv") replaces FUN_00d79408 with the film
-- fragment the MOVDC builders FUN_00edd120 and FUN_00edccd8 run on their record
-- 0x01699d8c: FUN_00eee958(col2, &local), FUN_00d78a80(record, local),
-- FUN_00d78538(record, local), then preset key 15. A set whose image 5
-- carries the live-view fragment (host/firmware/classic-chrome-liveview-image5.json)
-- takes (a) and (c) on this path; a set without it must render col2 0xd as
-- PROVIA there.
module Main where

import Control.Monad
import Data.Aeson (Value)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Bits
import Data.Char (digitToInt)
import Data.IORef
import Data.List (foldl', intercalate)
import qualified Data.Map.Strict as M
import Data.Word
import Emulation.ClassicChromeRam
import Emulation.NativeCase (runCase)
import FujiAF.Host
import FujiAF.JSON (decodeStrictIntegers, encodeCanonical)
import Numeric (showHex)
import System.Directory
import System.Environment
import System.Exit
import System.FilePath
import System.IO

-- ---------------------------------------------------------------------------
-- Pinned inputs.

data Pin = Pin { pinPath :: FilePath, pinBase :: Word32, pinSha :: String }

id4Pin, id5Pin, loaderPin, payloadPin, rc0Id4Pin, plainPin, id2Pin :: Pin
id4Pin = Pin "private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin" 0x0061c400 "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
id5Pin = Pin "private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin" 0x00bf1a00 "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"
loaderPin = Pin "private-data/analysis/x-m1-1.50/decode/loader-40000.bin" 0x00040000 "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"
payloadPin = Pin "private-data/analysis/x-m1-1.50/payload.bin" 0 "8c6f806ef4554128e5be887a76e438a91b7d6cb030c940fc1c8fd6d32d5926c0"
rc0Id4Pin = Pin "private-data/analysis/x-m1-1.50/update/prefix/id4-patched.bin" 0x0061c340 "a4d169bd961ec7df34e3b9b7108b624b79680c3cc50297e434e0b3607fb65551"
plainPin = Pin "private-data/analysis/x-m1-1.50/lz/plain-01100000.bin" 0x01100000 "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d"
id2Pin = Pin "private-data/analysis/x-m1-1.50/filmsim/id2-00130000.bin" 0x00130000 "236d12a11c8b386f71192675c12398358840327f21e392e6eaa91ddfd6e020a5"

-- Fallback adjustment image: payload offset 0xe0048 (E-ADJ-READ-150).
adjPayloadOffset :: Int
adjPayloadOffset = 0xe0048

data Image = Image { imName :: String, imBase :: Word32, imBytes :: B.ByteString, imSha :: String, imPath :: FilePath }

fileSha :: FilePath -> IO String
fileSha path = do
  (timed, code, out, _) <- runBounded 60000000 1024 "sha256sum" [path] B.empty
  ensure (not timed && code == ExitSuccess && B.length out >= 64) (path ++ ": sha256sum failed")
  pure (take 64 (BC.unpack out))

loadImage :: String -> FilePath -> Word32 -> Maybe String -> IO Image
loadImage name path base expected = do
  isLink <- pathIsSymbolicLink path
  ensure (not isLink) (path ++ ": symbolic link refused")
  digest <- fileSha path
  forM_ expected $ \e -> ensure (digest == e) (path ++ ": pinned SHA-256 differs: " ++ digest)
  bytes <- B.readFile path
  pure (Image name base bytes digest path)

slice :: Image -> Word32 -> Int -> B.ByteString
slice im addr size
  | addr >= imBase im && off + size <= B.length (imBytes im) = B.take size (B.drop off (imBytes im))
  | otherwise = error (imName im ++ ": slice outside image at 0x" ++ showHex addr "")
  where off = fromIntegral (addr - imBase im)

word32At :: B.ByteString -> Int -> Word32
word32At bs o = foldr (.|.) 0 [fromIntegral (B.index bs (o + k)) `shiftL` (8 * k) | k <- [0 .. 3]]

word16At :: B.ByteString -> Int -> Word32
word16At bs o = fromIntegral (B.index bs o) .|. (fromIntegral (B.index bs (o + 1)) `shiftL` 8)

le32 :: Word32 -> B.ByteString
le32 v = B.pack [fromIntegral (v `shiftR` k) | k <- [0, 8, 16, 24]]

le16 :: Integer -> B.ByteString
le16 v = B.pack [fromIntegral v, fromIntegral (v `shiftR` 8)]

patchAt :: Int -> B.ByteString -> B.ByteString -> B.ByteString
patchAt off new old = B.take off old <> new <> B.drop (off + B.length new) old

hex8 :: Word32 -> String
hex8 v = "0x" ++ replicate (8 - length t) '0' ++ t where t = showHex v ""

hv :: Word32 -> Value
hv = s . hex8

hexBytes :: B.ByteString -> String
hexBytes = concatMap (\x -> let t = showHex x "" in replicate (2 - length t) '0' ++ t) . B.unpack

unhex :: String -> B.ByteString
unhex = B.pack . go where
  go (a:c:rest) = fromIntegral (digitToInt a * 16 + digitToInt c) : go rest
  go _ = []

-- ---------------------------------------------------------------------------
-- Parameter sets (latent-image.classic-chrome-parameters.v1, consumed fields).

data Params = Params { prName :: String, prPath :: FilePath, prSha :: String, prMatrix :: [Integer], prChroma :: [Integer], prHead :: [Integer], prCurve :: [Integer] }

loadParams :: String -> FilePath -> IO Params
loadParams name path = do
  digest <- fileSha path
  raw <- B.readFile path
  v <- either failH pure (decodeStrictIntegers raw)
  ensure (get v "schema" == s "latent-image.classic-chrome-parameters.v1") (path ++ ": schema")
  let ints xs = map num xs
      matrix = concatMap (ints . list) (list (get v "colour_matrix"))
      chroma = ints (list (get v "chroma"))
      tone = get v "tone_record"
      hd = ints (list (get tone "head"))
      curve = ints (list (get tone "curve"))
      field x = x >= 0 && x <= 0xffff && x `shiftR` 12 <= 1
  ensure (length matrix == 9 && all field matrix) (path ++ ": colour_matrix")
  ensure (length chroma == 4 && all field chroma) (path ++ ": chroma")
  ensure (length hd == 9 && all (\x -> x >= 0 && x <= 255) hd) (path ++ ": tone head")
  ensure (length curve == 28 && and (zipWith (<=) curve (drop 1 curve)) && all (\x -> x >= 0 && x <= 0x3fff) curve && last curve == 0x3fff) (path ++ ": tone curve")
  pure (Params name path digest matrix chroma hd curve)

-- Data block at 0x0061c200 (reports/classic-chrome-patch.md section 7).
dataBlock :: Params -> B.ByteString
dataBlock p = B.concat
  [ BC.pack "CCD1", B.concat (map le16 (prMatrix p))
  , B.concat (map (le16 . (prChroma p !!)) [0, 2, 3, 1]), B.replicate 2 0
  , B.pack (map fromIntegral (prHead p)), B.singleton 0
  , B.concat (map le16 (prCurve p)), B.replicate 2 0 ]

-- ---------------------------------------------------------------------------
-- Function tables: recursive-descent static closures over the pinned images,
-- extents from the Ghidra function index (next start), with the three
-- fall-through pairs of the 64-bit divide helpers merged.

stageAFunctions, stageBFunctions :: [(Word32, Word32)]
stageAFunctions =
  [(0x0069f8fc,0x24),(0x00722054,0x124),(0x007221a0,0x28),(0x007221c8,0x50),(0x0073ed6c,0x28),(0x0073f760,0x2c),(0x0073fc64,0x7c),(0x00d5db1c,0x1cc)
  ,(0x00d5ddec,0xc4),(0x00d5deb0,0x4c),(0x00d5defc,0xc8),(0x00d5e658,0x14),(0x00d5fab4,0x388),(0x00d60594,0x3c)
  ,(0x00d60c5c,0x24),(0x00d61350,0x38),(0x00d61388,0x3c),(0x00d6235c,0x54),(0x00d63998,0xa4),(0x00d6415c,0x2c8)
  ,(0x00d66f00,0x90),(0x00d66f90,0xac),(0x00d68a1c,0x14),(0x00d68a30,0x14),(0x00d68a44,0x24),(0x00d69b24,0xec)
  ,(0x00d69ec0,0x84),(0x00d69f44,0x48),(0x00d77590,0x474),(0x00d77a04,0x44),(0x00d77a48,0x74),(0x00d77abc,0x18),(0x00d77f60,0x1c)
  ,(0x00d77f7c,0x14),(0x00d77f90,0x5c),(0x00d77fec,0x14),(0x00d78000,0x5c),(0x00d7805c,0x14),(0x00d78070,0x18)
  ,(0x00d78088,0x364),(0x00d783ec,0x14),(0x00d78418,0x94),(0x00d784ec,0x1c),(0x00d78508,0x14),(0x00d7851c,0x1c)
  ,(0x00d78538,0x14),(0x00d7854c,0x5c),(0x00d785a8,0x14),(0x00d785bc,0x24),(0x00d785e0,0x14),(0x00d785f4,0x6c)
  ,(0x00d78660,0x14),(0x00d786ac,0x10c),(0x00d787b8,0x24),(0x00d787dc,0xa0),(0x00d7887c,0x134),(0x00d789b0,0x20)
  ,(0x00d789d0,0x20),(0x00d78a10,0x5c),(0x00d78a6c,0x14),(0x00d78a80,0x14),(0x00d78a94,0x5c),(0x00d78af0,0x14)
  ,(0x00d78b04,0x78),(0x00d78b7c,0x5c),(0x00d78bd8,0x14),(0x00d78bec,0x20),(0x00d78c0c,0x20),(0x00d78c2c,0x20)
  ,(0x00d78c4c,0x16c),(0x00d78db8,0x134),(0x00d78eec,0x18),(0x00d78f04,0x48),(0x00d78f4c,0x1c),(0x00d78f68,0x18)
  ,(0x00d78f80,0x1c),(0x00d78f9c,0x1c),(0x00d78fb8,0x20),(0x00d78fd8,0x58),(0x00d79030,0xc8),(0x00d790f8,0x14)
  ,(0x00d7910c,0x20),(0x00d7912c,0x20),(0x00d79178,0x1c),(0x00d79194,0x1c),(0x00d791b0,0x134),(0x00d792e4,0x20)
  ,(0x00d79304,0x20),(0x00d79324,0x20),(0x00d79344,0x20),(0x00d79384,0x20),(0x00d793a4,0x34),(0x00d793d8,0x30)
  ,(0x00d79408,0x488),(0x00eee958,0x12c),(0x00ef6964,0x18),(0x00ef697c,0x958)]
stageBFunctions =
  [(0x0004357c,0x30),(0x0004d978,0xf4),(0x0004da6c,0x20),(0x00690924,0x250),(0x00690b74,0x170),(0x0069f8fc,0x24)
  ,(0x0069fec8,0x28),(0x0069ff7c,0x28),(0x00722054,0x124),(0x007221a0,0x28),(0x007221c8,0x50),(0x0073ebcc,0x40)
  ,(0x0073ec0c,0x38),(0x0073ec44,0x38),(0x0073ec7c,0x4c),(0x0073ecc8,0x4c),(0x0073ed6c,0x28),(0x0073f338,0x428)
  ,(0x0073f760,0x2c),(0x0073f78c,0x2c),(0x0073f7b8,0x30),(0x0073f7e8,0x158),(0x0073f940,0x90),(0x0073f9d0,0x294)
  ,(0x0073fc64,0x7c),(0x0073fce0,0x80),(0x0073fdb8,0x6c),(0x0073fe24,0x68),(0x0073fe8c,0x64),(0x0089aa90,0x128)
  ,(0x0089be8c,0x44),(0x0089bed0,0x3c),(0x0089bf0c,0x44),(0x0089bf50,0x44),(0x0089bf94,0x4f0),(0x0089c484,0x494)
  ,(0x0089c918,0x48),(0x00d5d294,0x798),(0x00d5da2c,0x2c),(0x00d5dce8,0x104),(0x00d5e66c,0x1dc),(0x00d5e848,0x31c)
  ,(0x00d5eb64,0x200),(0x00d5ed64,0x28),(0x00d5ed8c,0x144),(0x00d5eed0,0x1a0),(0x00d5f070,0x60),(0x00d5f0d0,0x24)
  ,(0x00d5f0f4,0x9c0),(0x00d5fe3c,0x1fc),(0x00d60038,0x50),(0x00d60088,0x50c),(0x00d605d0,0x2d4),(0x00d608a4,0x13c)
  ,(0x00d609e0,0x27c),(0x00d60c80,0x44),(0x00d60cc4,0x98),(0x00d60d5c,0x2c),(0x00d60d88,0x38),(0x00d60dc0,0x7c)
  ,(0x00d60e3c,0x2c),(0x00d60e68,0x40),(0x00d60ea8,0x410),(0x00d612b8,0x50),(0x00d61308,0x48),(0x00d613c4,0x280)
  ,(0x00d61644,0x260),(0x00d618a4,0x1e4),(0x00d61a88,0x1c),(0x00d61aa4,0x24),(0x00d61ac8,0xc0),(0x00d61b88,0x738)
  ,(0x00d622c0,0x9c),(0x00d623b0,0x1ec),(0x00d6259c,0xd0),(0x00d6266c,0x2a4),(0x00d62910,0x44),(0x00d62954,0x1a8)
  ,(0x00d62afc,0xe9c),(0x00d63a3c,0x88),(0x00d63ac4,0x698),(0x00d64424,0x3dc),(0x00d64800,0x40),(0x00d64840,0x10)
  ,(0x00d64850,0xc4),(0x00d64914,0x2c),(0x00d64940,0x10),(0x00d64950,0x64),(0x00d649b4,0x34),(0x00d649e8,0x10)
  ,(0x00d649f8,0x164),(0x00d64b5c,0x3c),(0x00d64b98,0x74),(0x00d64c0c,0x1f4),(0x00d64e00,0x2c),(0x00d64e2c,0x550)
  ,(0x00d6537c,0x6c),(0x00d653e8,0x34),(0x00d6541c,0x108),(0x00d65524,0xd0),(0x00d655f4,0xb8),(0x00d656ac,0xf0)
  ,(0x00d6579c,0x98),(0x00d65834,0x6c),(0x00d658a0,0x11c),(0x00d659bc,0x328),(0x00d65ce4,0x1ac),(0x00d65e90,0x58)
  ,(0x00d65ee8,0xac),(0x00d65f94,0x168),(0x00d660fc,0x1a4),(0x00d662a0,0x140),(0x00d663e0,0xacc),(0x00d66eac,0x54)
  ,(0x00d6703c,0x64),(0x00d670a0,0x114),(0x00d671b4,0x3f0),(0x00d675a4,0x34),(0x00d675d8,0x2c0),(0x00d67898,0x160)
  ,(0x00d679f8,0x298),(0x00d67c90,0xf0),(0x00d67d80,0xbbc),(0x00d6893c,0xe0),(0x00d68a68,0x340),(0x00d68da8,0x9dc)
  ,(0x00d69784,0x6c),(0x00d697f0,0x334),(0x00d69c10,0x74),(0x00d69c84,0x54),(0x00d69cd8,0x118),(0x00d69df0,0xd0)
  ,(0x00d69f8c,0x190),(0x00d6a11c,0x134),(0x00d6a250,0x1a0),(0x00d6a3f0,0xa28)]

-- Stub functions of the patched image 4 (bazel-bin/host/firmware/classic-chrome-stubs.sym):
-- cc_chroma falls through into cc_copy, so one extent covers both and
-- cc_colour's branch to cc_copy is a tail transfer into it.
stubFunctions :: [(Word32, Word32)]
stubFunctions = [(0x00738ff4, 0x1c), (0x00739010, 0x38), (0x00736a80, 0x48)]

-- ---------------------------------------------------------------------------
-- Memory map.

recordBase, requestBase, captureBase, blockBase, stackBase, stackTop, driverBase, exitA, exitB, adjBase, bssBase, toneScratch :: Word32
recordBase = 0x30000000
requestBase = 0x30001000
captureBase = 0x30001800
blockBase = 0x30004000
stackBase = 0x30010000
stackTop = 0x30014000
driverBase = 0x01f00000
exitA = 0x01f00100
exitB = 0x3e000000
adjBase = 0x41acd800
bssBase = 0x0095ad00
toneScratch = 0x01467a00

recordSize, blockSize, adjSize, adjHighOffset, adjHighSize, bssSize :: Int
recordSize = 0x400
blockSize = 0x2000
-- FUN_0069f204's fallback path clears the 0x420000-byte area and loads 0x42000
-- bytes; the develop path also reads offsets 0x3e3d40.. and 0x408a00.., which
-- that path leaves zero.
adjSize = 0x42000
adjHighOffset = 0x3e0000
adjHighSize = 0x40000
bssSize = 0x0096ca60 - 0x0095ad00

data Code = Code { cdImage :: String, cdBase :: Word32, cdSize :: Int }

stageACode, stageBCode :: [Code]
stageACode = [Code "id4" 0x0069f000 0x1000, Code "id4" 0x00722000 0x1e000, Code "id5" 0x00d5d000 0x1d000, Code "id5" 0x00eee000 0x1000, Code "id5" 0x00ef6000 0x2000]
stageBCode = [Code "loader" 0x00043000 0xaae0, Code "id4" 0x00690000 0x10000, Code "id4" 0x00722000 0x1e000, Code "id4" 0x0089a000 0x4c00, Code "id5" 0x00d5d000 0x1d000]

-- Read-only id4 data window: tone records 0x824bc4.., tables 0x7c2de2, 0x8179a0, 0x829a10..
id4Data :: Code
id4Data = Code "id4" 0x007c2000 0x68000

-- ---------------------------------------------------------------------------
-- A32 driver.

movw, movt :: Int -> Word32 -> Word32
movw rd v = 0xe3000000 .|. ((v `shiftR` 12) .&. 0xf) `shiftL` 16 .|. fromIntegral rd `shiftL` 12 .|. (v .&. 0xfff)
movt rd v = 0xe3400000 .|. ((v `shiftR` 28) .&. 0xf) `shiftL` 16 .|. fromIntegral rd `shiftL` 12 .|. ((v `shiftR` 16) .&. 0xfff)
movReg :: Int -> Int -> Word32
movReg rd rm = 0xe1a00000 .|. fromIntegral rd `shiftL` 12 .|. fromIntegral rm
movImm :: Int -> Word32 -> Word32
movImm rd v = 0xe3a00000 .|. fromIntegral rd `shiftL` 12 .|. v
strImm :: Int -> Int -> Word32 -> Word32
strImm rt rn off = 0xe5800000 .|. fromIntegral rn `shiftL` 16 .|. fromIntegral rt `shiftL` 12 .|. off
ldrImm :: Int -> Int -> Word32 -> Word32
ldrImm rt rn off = 0xe5900000 .|. fromIntegral rn `shiftL` 16 .|. fromIntegral rt `shiftL` 12 .|. off
bl :: Word32 -> Word32 -> Word32
bl at target = 0xeb000000 .|. (((target - (at + 8)) `shiftR` 2) .&. 0xffffff)

-- Still: the record from a request. Lv: FUN_00edd120's film fragment, the
-- validated film in the request word 0 as its stack local.
data DevPath = Still | Lv deriving Eq

-- r4 record, r5 request, r6 capture-side block.
driverWords :: DevPath -> Int -> Bool -> Word32 -> [Word32]
driverWords path col2 marker preset = zipWith ($) (map place body) [driverBase, driverBase + 4 ..]
  where
    place (Left w) _ = w
    place (Right target) at = bl at target
    body =
      map Left [0xe92d4070, movw 4 recordBase, movt 4 recordBase, movw 5 requestBase, movt 5 requestBase, movw 6 captureBase, movt 6 captureBase, movReg 0 4]
      ++ [Right 0x00d77abc, Left (movReg 0 4), Right 0x00d791b0]
      ++ (case path of
            Still -> [Left (movReg 0 4), Left (movReg 1 5), Left (movReg 2 6), Right 0x00d79408]
            Lv ->
              [ Left (movImm 0 (fromIntegral col2)), Left (movReg 1 5), Right 0x00eee958
              , Left (movReg 0 4), Left (ldrImm 1 5 0), Right 0x00d78a80
              , Left (movReg 0 4), Left (ldrImm 1 5 0), Right 0x00d78538 ])
      ++ (if marker then map Left [movImm 2 13, strImm 2 4 0x110] else [])
      ++ map Left [movImm 2 0, strImm 2 4 0xf0, strImm 2 4 0x140, movReg 0 4, movImm 1 preset, movImm 2 0]
      ++ [Right 0x00d77a48, Left 0xe8bd8070]

-- ---------------------------------------------------------------------------
-- Image sets and runs.

-- Stock: the pinned images or the oracle's substitutions. Flash: build-classic-chrome
-- decodes, data block at 0x0061c200. Ram: RC0 decodes with the manifest's publications.
data Patch = Stock | Flash | Ram RamPayload

data ImageSet = ImageSet { isTag :: String, isId4 :: Image, isId5 :: Image, isAdjTag :: String, isAdj :: B.ByteString, isPatch :: Patch, isParams :: Maybe Params }

data Stage = StageA | StageB deriving Eq

-- Preset key 2 is the still job's (FUN_00e543b0); it enables stage flags [5], [6]
-- and [0x12]. Key 15 (preset 0xd77e58) enables the second instances [0x25],
-- [0x26] and [0x29] instead.
data Run = Run { rnSet :: ImageSet, rnCol2 :: Int, rnMarker :: Bool, rnPreset :: Word32, rnPath :: DevPath }

runTag :: Run -> String
runTag r = isTag (rnSet r) ++ (if rnPath r == Lv then "-lv" else "") ++ (if rnMarker r then "-mark" else "") ++ (if rnPreset r == 2 then "" else "-p" ++ show (rnPreset r)) ++ "-c" ++ (let t = show (rnCol2 r) in replicate (2 - length t) '0' ++ t)

caseId :: Stage -> Run -> String
caseId st r = (if st == StageA then "a-" else "b-") ++ runTag r

data Region = Region { rgId :: String, rgBase :: Word32, rgSize :: Int, rgPerm :: String, rgArtifact :: Maybe (String, B.ByteString) }

data Env = Env { envOut :: FilePath, envRunner :: FilePath, envLoader :: Image, envArtifacts :: IORef (M.Map String Value) }

artifact :: Env -> String -> B.ByteString -> IO Value
artifact env name bytes = do
  known <- readIORef (envArtifacts env)
  case M.lookup name known of
    Just v -> pure v
    Nothing -> do
      let path = envOut env </> name <.> "bin"
      B.writeFile path bytes
      digest <- sha256 bytes
      let v = obj [("id", s name), ("path", s (name <.> "bin")), ("size", n (fromIntegral (B.length bytes))), ("sha256", s digest)]
      modifyIORef' (envArtifacts env) (M.insert name v)
      pure v

codeRegion :: Env -> ImageSet -> Code -> Region
codeRegion env set (Code which base size) = Region (which ++ "-" ++ drop 2 (hex8 base)) base size perm (Just (name, slice im base size))
  where
    im = case which of "id4" -> isId4 set; "id5" -> isId5 set; _ -> envLoader env
    perm = if base == cdBase id4Data then "r" else "rx"
    name = which ++ "-" ++ drop 2 (hex8 base) ++ "-" ++ take 12 (imSha im)

zeroRegion :: String -> Word32 -> Int -> String -> Region
zeroRegion i base size perm = Region i base size perm Nothing

bssBytes :: B.ByteString
bssBytes = patchAt (fromIntegral (0x0096ca50 - bssBase)) (le32 adjBase) (B.replicate bssSize 0)

regionsFor :: Env -> Stage -> Run -> B.ByteString -> [Region]
regionsFor env st r input =
  map (codeRegion env set) ((if st == StageA then stageACode else stageBCode) ++ [id4Data])
  ++ [ Region "bss" bssBase bssSize "rw" (Just ("bss-adjptr", bssBytes))
     , Region "adjustment" adjBase adjSize "r" (Just ("adj-" ++ isAdjTag set, isAdj set))
     , zeroRegion "adjustment-high" (adjBase + fromIntegral adjHighOffset) adjHighSize "r" ]
  ++ (if st == StageA
        then [ zeroRegion "record" recordBase recordSize "rw"
             , Region "request" requestBase 0x1000 "rw" (Just ("request-c" ++ show (rnCol2 r), requestBytes (rnCol2 r)))
             , zeroRegion "stack" stackBase (fromIntegral (stackTop - stackBase)) "rw"
             , Region "driver" driverBase 0x104 "rx" (Just (driverName, B.concat (map le32 (runDriver r)))) ]
        else [ Region "record" recordBase recordSize "rw" (Just ("rec-" ++ runTag r, input))
             , zeroRegion "block" blockBase blockSize "rw"
             , zeroRegion "tone-scratch" toneScratch 0x100 "rw"
             , zeroRegion "stack" stackBase (fromIntegral (stackTop - stackBase)) "rw"
             , zeroRegion "exit" exitB 4 "rx" ]
           ++ [Region "prefix" 0x0061c200 0x64 "r" (Just ("prefix-" ++ take 12 (imSha (isId4 set)), slice (isId4 set) 0x0061c200 0x64)) | Flash <- [isPatch set]])
  ++ case isPatch set of
       Ram rp -> [ Region "runtime-text" (fst (textRange rp)) (snd (textRange rp)) "rx" (Just ("runtime-text-" ++ take 12 (rpRuntimeSha rp), runtimeSlice rp (textRange rp)))
                 , Region "runtime-data" (rpData rp) 100 "r" (Just ("runtime-data-" ++ take 12 (rpRuntimeSha rp), runtimeSlice rp (dataRange rp))) ]
       _ -> []
  where
    set = rnSet r
    driverName = (if rnMarker r then "driver-marker" else "driver") ++ (if rnPath r == Lv then "-lv-c" ++ show (rnCol2 r) else "") ++ "-p" ++ show (rnPreset r)

runDriver :: Run -> [Word32]
runDriver r = driverWords (rnPath r) (rnCol2 r) (rnMarker r) (rnPreset r)

-- Request: zero except request[9] (+0x24) = col2.
requestBytes :: Int -> B.ByteString
requestBytes col2 = patchAt 0x24 (le32 (fromIntegral col2)) (B.replicate 0x1000 0)

functionsFor :: Stage -> Run -> [(Word32, Word32)]
functionsFor st r = base' ++ case (st, isPatch (rnSet r)) of
  (StageB, Flash) -> stubFunctions
  (StageA, Ram rp) -> ram rp [CcMarker]
  (StageB, Ram rp) -> ram rp [CcColour, CcChroma, CcTone]
  _ -> []
  where
    base' = if st == StageA then (driverBase, fromIntegral (4 * length (runDriver r))) : stageAFunctions else stageBFunctions
    ram rp = map (\e -> let (a, z) = entryFunction rp e in (a, fromIntegral z))

regionJson :: Env -> Region -> IO (Value, Maybe Value)
regionJson env rg = case rgArtifact rg of
  Nothing -> pure (obj (common ++ [("initial", obj [("zero", b True)])]), Nothing)
  Just (name, bytes) -> do
    a <- artifact env name bytes
    pure (obj (common ++ [("initial", obj [("artifact", s name), ("offset", n 0), ("length", n (fromIntegral (B.length bytes)))])]), Just a)
  where common = [("id", s (rgId rg)), ("address", hv (rgBase rg)), ("size", n (fromIntegral (rgSize rg))), ("permissions", s (rgPerm rg))]

-- ---------------------------------------------------------------------------
-- Results.

data Final = Final { fnStatus :: String, fnRegisters :: Value, fnMemory :: M.Map String B.ByteString, fnInstructions :: Integer, fnProvenance :: [Value], fnProvenanceTotal :: Integer }

finalOf :: [Region] -> Value -> Final
finalOf regions runner = Final
  (str (get runner "status")) (get runner "registers")
  (M.fromList [(rgId rg, unhex (str (get m "bytes"))) | (rg, m) <- zip regions (list (get runner "memory"))])
  (read (str (get runner "instructions")))
  (list (get runner "memory_provenance")) (read (str (get runner "memory_provenance_total")))

region :: Final -> String -> B.ByteString
region f i = M.findWithDefault (error ("region " ++ i)) i (fnMemory f)

-- Expected result: every writable region's full final bytes and every register.
expectedOf :: [Region] -> Final -> Value
expectedOf regions f = obj
  [ ("status", s (fnStatus f)), ("registers", fnRegisters f)
  , ("memory", arr [obj [("region", s (rgId rg)), ("offset", n 0), ("hex", s (hexBytes (region f (rgId rg))))] | rg <- regions, rgPerm rg == "rw"])
  , ("events", arr []) ]

data Outcome = Outcome { ocId :: String, ocFinal :: Final, ocPassed :: Bool, ocMismatches :: [String] }

provenanceText :: Stage -> Run -> String -> String
provenanceText st r expectation = intercalate " " $
  [ "Classic Chrome develop-builder case, generated by //emulation:classic-chrome-builder-cases."
  , if st == StageA
      then case rnPath r of
        Still -> "Stage a: synthetic driver at 0x01f00000 calls stock FUN_00d77abc(record), FUN_00d791b0(record), FUN_00d79408(record, request, capture-side area) with request zero except request[9] = " ++ show (rnCol2 r) ++ (if rnMarker r then ", stores 0xd to record +0x110 after FUN_00d79408 (marker injection)," else ",") ++ " stores 0 to record +0xf0 and +0x140 as FUN_00e561b4 does, then FUN_00d77a48(record, preset key " ++ show (rnPreset r) ++ ", level 0)."
        Lv -> "Stage a (movie/live-view film fragment): synthetic driver at 0x01f00000 calls stock FUN_00d77abc(record), FUN_00d791b0(record), then as FUN_00edd120 does at 0x00edd228-0x00edd2d0 FUN_00eee958(" ++ show (rnCol2 r) ++ ", request word 0), FUN_00d78a80(record, request word 0) and FUN_00d78538(record, request word 0)" ++ (if rnMarker r then ", stores 0xd to record +0x110 (marker injection)," else ",") ++ " stores 0 to record +0xf0 and +0x140, then FUN_00d77a48(record, preset key " ++ show (rnPreset r) ++ ", level 0)."
      else "Stage b: stock FUN_00d5da2c(block 0x30004000, record 0x30000000); the record artifact is the final record of case " ++ caseId StageA r ++ "."
  , "Images: id4 " ++ imSha (isId4 (rnSet r)) ++ ", id5 " ++ imSha (isId5 (rnSet r)) ++ ", loader " ++ pinSha loaderPin ++ "; adjustment area 0x41acd800 = " ++ adjNote (rnSet r) ++ "; pointer word 0x0096ca50 = 0x41acd800; bss cache 0x0095ad00.. zero."
  ] ++ [ "Parameter set " ++ prPath p ++ " sha256 " ++ prSha p ++ "." | Just p <- [isParams (rnSet r)] ]
    ++ [ "RAM images: id4 is the RC0 decode " ++ pinPath rc0Id4Pin ++ " (sha256 " ++ pinSha rc0Id4Pin ++ ") at 0x0061c340 with zeros from 0x0061c200, id5 the pinned stock decode, each with the publications of " ++ rpManifestPath rp ++ " (sha256 " ++ rpManifestSha rp ++ ") applied in manifest order; runtime " ++ rpRuntimePath rp ++ " (sha256 " ++ rpRuntimeSha rp ++ ") at " ++ hex8 (rpBase rp) ++ ", its CC code mapped rx at " ++ hex8 (fst (textRange rp)) ++ " and its data block cc_ram_data mapped r at " ++ hex8 (rpData rp) ++ "." | Ram rp <- [isPatch (rnSet r)] ]
    ++ [ "Expected state: " ++ expectation ]

adjNote :: ImageSet -> String
adjNote set = "fallback adjustment image, payload.bin (sha256 " ++ pinSha payloadPin ++ ") offset 0xe0048, 0x42000 bytes at area offset 0, area offsets 0x3e0000..0x41ffff zero" ++ (if isAdjTag set == "fallback" then "" else ", with " ++ isAdjTag set ++ " substitutions")

exclusions :: [String]
exclusions =
  [ "No scheduler, interrupts, MMIO, DMA, caches or silicon timing; the develop path runs on configured memory only."
  , "The request is zero except request[9]; capture fields other than the film value are not modelled, so this is one record, not every record a body builds."
  , "Record init uses FUN_00d77abc and FUN_00d791b0 as the movie builders do; the still job's earlier record state is not modelled."
  , "The adjustment area is the fallback image, which equals the body's adjustment bytes over 0xa000-0xcfff (E-ADJ-READ-150); bytes outside that window are not body-verified."
  , "The bss adjustment cache at 0x0095ad00 starts empty (all entries invalid)."
  , "On the movie/live-view path only FUN_00edd120's film fragment sets the record; its other setters, the MOVDC config at 0x01699584 and FUN_00edccd8's incremental stage wrappers are not modelled." ]

runStage :: Env -> Stage -> Run -> B.ByteString -> Maybe Final -> String -> IO Outcome
runStage env st r input reference expectation = do
  let cid = caseId st r
      regions = regionsFor env st r input
      fns = functionsFor st r
      entry = if st == StageA then driverBase else 0x00d5da2c
      exit' = if st == StageA then exitA else exitB
      registers = obj ([("r" ++ show i, hv (case (st, i) of (StageB, 0) -> blockBase; (StageB, 1) -> recordBase; _ -> 0)) | i <- [0 :: Int .. 12]]
                       ++ [("sp", hv stackTop), ("lr", hv exit'), ("cpsr", hv 0x10)])
      casePath = envOut env </> cid <.> "json"
  forM_ fns $ \(a, sz) -> ensure (any (\rg -> rgPerm rg == "rx" && rgBase rg <= a && a + sz <= rgBase rg + fromIntegral (rgSize rg)) regions) (cid ++ ": function 0x" ++ showHex a "" ++ " outside code regions")
  described <- mapM (regionJson env) regions
  let artifacts = foldr (\a acc -> if a `elem` acc then acc else a : acc) [] [a | (_, Just a) <- described]
      write expected = B.writeFile casePath (encodeCanonical (obj
        [ ("schema_version", n 1), ("id", s cid), ("origin", s "stock"), ("provenance", s (provenanceText st r expectation))
        , ("cpu", obj [("architecture", s "arm"), ("model", s "cortex-r5f"), ("mode", s "arm"), ("endian", s "little")])
        , ("artifacts", arr artifacts), ("entry", hv entry), ("exit", hv exit'), ("registers", registers)
        , ("regions", arr (map fst described))
        , ("functions", arr [obj [("address", hv a), ("size", n (fromIntegral sz))] | (a, sz) <- fns])
        , ("stubs", arr []), ("limits", obj [("instructions", n 1000000), ("wall_ms", n 10000)])
        , ("expected", expected), ("exclusions", arr (map s exclusions)) ]) <> "\n")
      execute = do
        (_, report) <- runCase (envRunner env) casePath
        when (get report "status" `notElem` [s "passed", s "failed"]) $ do
          hPutStrLn stderr (cid ++ ": " ++ BC.unpack (encodeCanonical report))
          failH (cid ++ ": case did not run")
        pure report
  case reference of
    Just f -> do
      write (expectedOf regions f)
      report <- execute
      pure (Outcome cid (finalOf regions (get report "runner")) (get report "status" == s "passed") (map str (list (get report "mismatches"))))
    Nothing -> do
      write (obj [("status", s "returned"), ("registers", obj [("pc", hv exit')]), ("memory", arr []), ("events", arr [])])
      probe <- execute
      let runner = get probe "runner"
          f = finalOf regions runner
      when (fnStatus f /= "returned") $ do
        hPutStrLn stderr (cid ++ ": status " ++ fnStatus f ++ " pc " ++ str (get (get runner "registers") "pc"))
        let lastN k xs = drop (length xs - k) xs
        hPutStrLn stderr ("trace: " ++ unwords [str (get t "pc") ++ ":" ++ str (get t "instruction") | t <- lastN 16 (list (get runner "trace"))])
        hPutStrLn stderr ("accesses: " ++ BC.unpack (encodeCanonical (arr (lastN 4 (list (get runner "accesses"))))))
        failH (cid ++ ": stock path did not return")
      write (expectedOf regions f)
      report <- execute
      pure (Outcome cid (finalOf regions (get report "runner")) (get report "status" == s "passed") (map str (list (get report "mismatches"))))

-- ---------------------------------------------------------------------------
-- Comparison helpers.

type Range = (Int, Int) -- inclusive block offsets

ccRanges :: [(String, [Range])]
ccRanges =
  [ ("colour", [(0xd2, 0xe5), (0x1020, 0x1020), (0x109c, 0x10ad)])
  , ("tone", [(0xe6, 0x12b), (0x10ae, 0x10f3)])
  , ("chroma", [(0xf26, 0xf26), (0xf4a, 0xf51), (0x1024, 0x1024), (0x10f8, 0x10ff)]) ]

inRanges :: Int -> Bool
inRanges o = any (\(lo, hi) -> lo <= o && o <= hi) (concatMap snd ccRanges)

diffOffsets :: B.ByteString -> B.ByteString -> [Int]
diffOffsets x y = [i | i <- [0 .. B.length x - 1], B.index x i /= B.index y i]

spans :: [Int] -> [Range]
spans = foldr step [] where
  step i ((lo, hi) : rest) | i + 1 == lo = (i, hi) : rest
  step i acc = (i, i) : acc

spanText :: [Int] -> String
spanText offs = if null offs then "none" else intercalate ", " [if lo == hi then h lo else h lo ++ ".." ++ h hi | (lo, hi) <- spans offs]
  where h x = "+0x" ++ showHex x ""

-- Reads of record +0x110..+0x113 in the retained provenance window.
markerReads :: Final -> [String]
markerReads f =
  [ str (get p "pc") ++ " " ++ str (get p "instruction")
  | p <- fnProvenance f, get p "kind" == s "read"
  , let a = read ("0x" ++ drop 2 (str (get p "address"))) :: Integer
  , a >= fromIntegral recordBase + 0x110, a < fromIntegral recordBase + 0x114 ]

-- ---------------------------------------------------------------------------

data Check = Check { ckName :: String, ckPass :: Bool, ckDetail :: String }

usage :: IO a
usage = do
  hPutStrLn stderr "usage: classic-chrome-builder-cases --runner RUNNER --out DIR [--set NAME PARAMS PATCHED_ID4 PATCHED_ID5]... [--ram-set NAME FLASH_SET MANIFEST RUNTIME SYMS]..."
  exitFailure

data RamArg = RamArg { raName :: String, raFlash :: String, raManifest :: FilePath, raRuntime :: FilePath, raSyms :: FilePath }

parseArgs :: [String] -> Maybe (FilePath, FilePath, [(String, FilePath, FilePath, FilePath)], [RamArg])
parseArgs = go Nothing Nothing [] [] where
  go (Just r) (Just o) sets rams [] = Just (r, o, reverse sets, reverse rams)
  go _ o sets rams ("--runner" : r : rest) = go (Just r) o sets rams rest
  go r _ sets rams ("--out" : o : rest) = go r (Just o) sets rams rest
  go r o sets rams ("--set" : name : p : i4 : i5 : rest) = go r o ((name, p, i4, i5) : sets) rams rest
  go r o sets rams ("--ram-set" : name : flash : m : rt : sy : rest) = go r o sets (RamArg name flash m rt sy : rams) rest
  go _ _ _ _ _ = Nothing

main :: IO ()
main = do
  hSetBuffering stdout LineBuffering
  args <- getArgs
  (runner, out, setArgs, ramArgs) <- maybe usage pure (parseArgs args)
  createDirectoryIfMissing True out
  stale <- filter (\f -> takeExtension f `elem` [".json", ".bin"]) <$> listDirectory out
  mapM_ (removeFile . (out </>)) stale
  id4 <- loadImage "id4" (pinPath id4Pin) (pinBase id4Pin) (Just (pinSha id4Pin))
  id5 <- loadImage "id5" (pinPath id5Pin) (pinBase id5Pin) (Just (pinSha id5Pin))
  loader <- loadImage "loader" (pinPath loaderPin) (pinBase loaderPin) (Just (pinSha loaderPin))
  payload <- loadImage "payload" (pinPath payloadPin) 0 (Just (pinSha payloadPin))
  ref <- newIORef M.empty
  let env = Env out runner loader ref
      adj = B.take adjSize (B.drop adjPayloadOffset (imBytes payload))
      stock = ImageSet "stock" id4 id5 "fallback" adj Stock Nothing
      t16 base i = fromIntegral (word16At (imBytes id5) (fromIntegral (base - imBase id5) + 2 * i)) :: Int
      colourFields = [t16 0x00d67f88 i | i <- [0 .. 8]]
      chromaFields = [t16 0x00d61e14 i | i <- [0 .. 3]]
      toneRecord = 0x00824bc8 + 0x48 :: Word32
  ensure (B.take 4 (B.drop 0x15c adj) == BC.pack "X-M1" && word16At adj 0x202 == 0xaa55) "fallback adjustment image lacks X-M1 at +0x15c or 0xaa55 at +0x202"
  ensure (colourFields == [0xacc3, 0xacc5 .. 0xacd3]) "colour matrix id 1 field table differs from 0xacc3.."
  ensure (chromaFields == [0xb077, 0xb079, 0xb07b, 0xb07d]) "chroma id 1 field table differs from 0xb077.."
  ensure (word32At (slice id4 toneRecord 4) 0 == 1) "tone record at 0x824c10 is not key 1"
  sets <- forM setArgs $ \(name, p, i4, i5) -> do
    params <- loadParams name p
    pid4 <- loadImage (name ++ "-id4") i4 0x0061c200 Nothing
    pid5 <- loadImage (name ++ "-id5") i5 (imBase id5) Nothing
    ensure (B.length (imBytes pid4) == B.length (imBytes id4) + 0x200) (i4 ++ ": not a decode at 0x0061c200")
    ensure (B.length (imBytes pid5) == B.length (imBytes id5)) (i5 ++ ": length differs from stock id5")
    ensure (slice pid4 0x0061c200 0x64 == dataBlock params) (i4 ++ ": data block differs from " ++ p)
    let oracleAdj = foldl' (\a (o, v) -> patchAt o (le16 v) a) adj (zip colourFields (prMatrix params) ++ zip chromaFields (prChroma params))
        oracleTone = B.pack (map fromIntegral (prHead params)) <> B.singleton (B.index (slice id4 toneRecord 0x48) 13) <> B.concat (map le16 (prCurve params))
        oracleId4 = id4 { imName = "id4-oracle-" ++ name, imBytes = patchAt (fromIntegral (toneRecord + 4 - imBase id4)) oracleTone (imBytes id4) }
    oracleSha <- sha256 (imBytes oracleId4)
    pure ( ImageSet name pid4 pid5 "fallback" adj Flash (Just params)
         , ImageSet ("oracle-" ++ name) oracleId4 { imSha = oracleSha } id5 ("oracle-" ++ name) oracleAdj Stock (Just params) )
  ramPins <- if null ramArgs then pure [] else forM [(rc0Id4Pin, "id4-rc0"), (plainPin, "plain"), (id2Pin, "id2")] $ \(p, nm) -> loadImage nm (pinPath p) (pinBase p) (Just (pinSha p))
  rams <- forM ramArgs $ \ra -> do
    params <- case [pr | (pt, _) <- sets, isTag pt == raFlash ra, Just pr <- [isParams pt]] of
      [pr] -> pure pr
      _ -> failH (raName ra ++ ": no --set named " ++ raFlash ra)
    rp <- loadRamPayload (raManifest ra) (raRuntime ra) (raSyms ra)
    ensure (runtimeSlice rp (dataRange rp) == dataBlock params) (raRuntime ra ++ ": cc_ram_data differs from the data block of " ++ prPath params)
    let asRam im = RamImage (imName im) (imBase im) (imBytes im)
        rc0Padded im = RamImage (imName im) 0x0061c200 (B.replicate 0x140 0 <> imBytes im)
    applied <- applyPublications rp (case ramPins of [rc0, plain, id2] -> [rc0Padded rc0, asRam id5, asRam plain, asRam id2]; _ -> [])
    (r4, r5) <- case applied of
      [a4, a5, _, _] -> pure (a4, a5)
      _ -> failH "RAM images"
    sha4 <- sha256 (riBytes r4)
    sha5 <- sha256 (riBytes r5)
    let ram4 = Image (raName ra ++ "-id4") 0x0061c200 (riBytes r4) sha4 (pinPath rc0Id4Pin)
        ram5 = Image (raName ra ++ "-id5") (imBase id5) (riBytes r5) sha5 (pinPath id5Pin)
    ensure (B.length (imBytes ram4) == B.length (imBytes id4) + 0x200) (raName ra ++ ": RAM id4 is not a decode at 0x0061c200")
    pure (ImageSet (raName ra) ram4 ram5 "fallback" adj (Ram rp) (Just params), ra)

  summary <- newIORef ([] :: [Value])
  checks <- newIORef ([] :: [Check])
  let record' o = modifyIORef' summary (++ [o])
      check name ok detail = do
        modifyIORef' checks (++ [Check name ok detail])
        putStrLn ((if ok then "PASS " else "FAIL ") ++ name ++ ": " ++ detail)
      both run reference expectation = do
        a <- runStage env StageA run B.empty (fst <$> reference) expectation
        let rec = region (ocFinal a) "record"
        bOut <- runStage env StageB run rec (snd <$> reference) expectation
        forM_ [a, bOut] $ \o -> do
          blockSha <- if ocId o == ocId a then pure "" else sha256 (region (ocFinal o) "block")
          recSha <- sha256 (region (ocFinal o) "record")
          record' (obj ([ ("case", s (ocId o)), ("passed", b (ocPassed o)), ("mismatches", arr (map s (ocMismatches o)))
                        , ("instructions", n (fnInstructions (ocFinal o))), ("record_sha256", s recSha)
                        , ("marker_reads", arr (map s (markerReads (ocFinal o)))), ("provenance_total", n (fnProvenanceTotal (ocFinal o))) ]
                        ++ [("block_sha256", s blockSha) | not (null blockSha)]
                        ++ [("parameter_sha256", s (prSha p)) | Just p <- [isParams (rnSet run)]]))
          putStrLn (ocId o ++ ": " ++ (if ocPassed o then "passed" else "FAILED " ++ unwords (ocMismatches o)) ++ ", " ++ show (fnInstructions (ocFinal o)) ++ " instructions" ++ (if null blockSha then "" else ", block " ++ blockSha))
        pure (ocFinal a, ocFinal bOut, ocPassed a && ocPassed bOut)

  stockRuns <- forM [0 .. 13] $ \k -> do
    (fa, fb, ok) <- both (Run stock k False 2 Still) Nothing "the stock images' own final state from this run (the stock reference)."
    check ("stock-c" ++ show k ++ "-pinned") ok "stock run reproduces its pinned state"
    pure (k, (fa, fb))
  let stockA k = fst (maybe (error "stock") id (lookup k stockRuns))
      stockB k = snd (maybe (error "stock") id (lookup k stockRuns))
      provia = region (stockB 0) "block"
  check "stock-c13-equals-c0" (region (stockB 13) "block" == provia) ("stock col2 0xd block differs from col2 0 at " ++ spanText (diffOffsets (region (stockB 13) "block") provia))

  -- (b) census on stock: marker injected after the setter.
  forM_ [0 .. 12] $ \k -> do
    (fa, fb, ok) <- both (Run stock k True 2 Still) Nothing "the stock images' own final state from this run with record +0x110 = 0xd injected after FUN_00d79408."
    let recDiff = diffOffsets (region fa "record") (region (stockA k) "record")
        blockDiff = diffOffsets (region fb "block") (region (stockB k) "block")
        recBDiff = diffOffsets (region fb "record") (region (stockB k) "record")
    -- 0x00d671d4 reads +0x110 as the mono filter index only for develop code 2.
    let code = word32At (region fa "record") 0x104
        colourOnly = all (\o -> any (\(lo, hi) -> lo <= o && o <= hi) (maybe [] id (lookup "colour" ccRanges))) blockDiff
    check ("census-c" ++ show k) (ok && (null blockDiff || (code == 2 && colourOnly)) && all (\o -> o >= 0x110 && o < 0x114) (recDiff ++ recBDiff))
      ("develop code " ++ show code ++ "; block differs at " ++ spanText blockDiff ++ "; record differs at " ++ spanText recDiff ++ " (stage a), " ++ spanText recBDiff ++ " (stage b); +0x110 reads in retained provenance: stage a " ++ show (markerReads fa) ++ " of " ++ show (fnProvenanceTotal fa) ++ " events, stage b " ++ show (markerReads fb) ++ " of " ++ show (fnProvenanceTotal fb) ++ " events")

  (stockP15A, stockP15, okP15) <- both (Run stock 0 False 15 Still) Nothing "the stock images' own final state from this run (the stock reference, preset key 15)."
  check "stock-p15-c0-pinned" okP15 "stock run reproduces its pinned state"
  (_, stockP15c13, okP15c13) <- both (Run stock 13 False 15 Still) Nothing "the stock images' own final state from this run (preset key 15)."
  check "stock-p15-c13-equals-c0" (okP15c13 && region stockP15c13 "block" == region stockP15 "block") ("stock col2 0xd block (preset key 15) differs from col2 0 at " ++ spanText (diffOffsets (region stockP15c13 "block") (region stockP15 "block")))

  ccBlocks <- newIORef (M.empty :: M.Map (String, Word32) (B.ByteString, B.ByteString, Bool))
  let patchedChecks patched oracleOf onCc = do
        let name = isTag patched
        -- (a) stock films on the patched images, expected = stock state.
        forM_ [0 .. 12] $ \k -> do
          (_, fb, ok) <- both (Run patched k False 2 Still) (Just (stockA k, stockB k)) ("the stock images' final state for col2 " ++ show k ++ " (case " ++ caseId StageB (Run stock k False 2 Still) ++ "); the stock image's own output is the reference.")
          check (name ++ "-stock-film-c" ++ show k) (ok && region fb "block" == region (stockB k) "block") "patched block, record and final state equal the stock run"
        -- (c) oracle and CC, for the still preset (key 2) and the second-instance preset (key 15).
        forM_ [(2 :: Word32, provia), (15, region stockP15 "block")] $ \(preset, reference) -> do
          let label = name ++ (if preset == 2 then "" else "-p15")
          (oracleBlock, okO) <- oracleOf preset
          (fa, fb, okC) <- both (Run patched 13 False preset Still) Nothing "the patched images' own final state from this run, pinned for regression after the generator's comparisons with the stock PROVIA block and the substitution oracle."
          let cc = region fb "block"
              outside = [o | o <- diffOffsets cc reference, not (inRanges o)]
              inside = [o | o <- diffOffsets cc oracleBlock, inRanges o]
              changed = diffOffsets cc reference
          check (label ++ "-cc-marker") (word32At (region fa "record") 0x110 == 13) "patched col2 0xd record carries +0x110 = 0xd"
          check (label ++ "-cc-outside") (okC && null outside) ("CC block differs from stock PROVIA block outside the ranges at " ++ spanText outside ++ "; all CC differences: " ++ spanText changed)
          check (label ++ "-cc-inside") (okO && null inside) ("CC block differs from the oracle inside the ranges at " ++ spanText inside ++ "; oracle differs from PROVIA at " ++ spanText (diffOffsets oracleBlock reference))
          forM_ ccRanges $ \(stage, rs) -> do
            let touched = [o | o <- changed, any (\(lo, hi) -> lo <= o && o <= hi) rs]
            putStrLn (label ++ " " ++ stage ++ " bytes changed by CC: " ++ spanText touched)
          onCc label preset cc oracleBlock okO
        (_, fp, okP) <- both (Run patched 0 False 15 Still) (Just (stockP15A, stockP15)) "the stock images' final state for col2 0, preset key 15 (case b-stock-p15-c00); the stock image's own output is the reference."
        check (name ++ "-p15-stock-film-c0") (okP && region fp "block" == region stockP15 "block") "patched block, record and final state equal the stock run (preset key 15)"

  forM_ sets $ \(patched, oracle) ->
    patchedChecks patched
      (\preset -> do
        (_, fo, okO) <- both (Run oracle 0 False preset Still) Nothing "the stock images' own final state from this run over the substituted adjustment image and tone record (the substitution oracle)."
        pure (region fo "block", okO))
      (\_ preset cc oracleBlock okO -> modifyIORef' ccBlocks (M.insert (isTag patched, preset) (cc, oracleBlock, okO)))

  -- Movie/live-view film fragment (FUN_00edd120's preset key 15).
  lvStock <- forM [0 .. 13] $ \k -> do
    (fa, fb, ok) <- both (Run stock k False 15 Lv) Nothing "the stock images' own final state from this run (the movie/live-view reference, preset key 15)."
    check ("stock-lv-c" ++ show k ++ "-pinned") ok "stock run reproduces its pinned state"
    pure (k, (fa, fb))
  let lvA k = fst (maybe (error "lv stock") id (lookup k lvStock))
      lvB k = snd (maybe (error "lv stock") id (lookup k lvStock))
      lvProvia = region (lvB 0) "block"
  check "stock-lv-c13-equals-c0" (region (lvB 13) "block" == lvProvia && word32At (region (lvA 13) "record") 0x110 == 0)
    ("stock col2 0xd on the movie/live-view path: +0x110 = " ++ show (word32At (region (lvA 13) "record") 0x110) ++ ", block differs from col2 0 at " ++ spanText (diffOffsets (region (lvB 13) "block") lvProvia))
  forM_ sets $ \(patched, oracle) -> do
    let name = isTag patched ++ "-lv"
        fragment = word32At (slice (isId5 patched) 0x00eee960 4) 0 == 0xe350000d
    forM_ [0 .. 12] $ \k -> do
      (_, fb, ok) <- both (Run patched k False 15 Lv) (Just (lvA k, lvB k)) ("the stock images' final state for col2 " ++ show k ++ " on the movie/live-view path (case " ++ caseId StageB (Run stock k False 15 Lv) ++ "); the stock image's own output is the reference.")
      check (name ++ "-stock-film-c" ++ show k) (ok && region fb "block" == region (lvB k) "block") "patched block, record and final state equal the stock run"
    (fa, fb, okC) <- both (Run patched 13 False 15 Lv) Nothing "the patched images' own final state from this run, pinned for regression after the generator's comparisons with the stock PROVIA block and the substitution oracle."
    let cc = region fb "block"
        rec = region fa "record"
    if fragment
      then do
        (_, fo, okO) <- both (Run oracle 0 False 15 Lv) Nothing "the stock images' own final state from this run over the substituted adjustment image and tone record (the substitution oracle, movie/live-view path)."
        let oracleBlock = region fo "block"
            outside = [o | o <- diffOffsets cc lvProvia, not (inRanges o)]
            inside = [o | o <- diffOffsets cc oracleBlock, inRanges o]
        check (name ++ "-cc-marker") (word32At rec 0x110 == 13 && word32At rec 0x104 == 0) ("record +0x104 = " ++ show (word32At rec 0x104) ++ ", +0x110 = " ++ show (word32At rec 0x110) ++ " (expected develop code 0, marker 0xd)")
        check (name ++ "-cc-outside") (okC && null outside) ("CC block differs from the stock PROVIA block outside the ranges at " ++ spanText outside ++ "; all CC differences: " ++ spanText (diffOffsets cc lvProvia))
        check (name ++ "-cc-inside") (okO && null inside) ("CC block differs from the oracle inside the ranges at " ++ spanText inside ++ "; oracle differs from PROVIA at " ++ spanText (diffOffsets oracleBlock lvProvia))
      else
        check (name ++ "-cc-provia") (okC && cc == lvProvia && word32At rec 0x110 == 0) ("without the live-view fragment col2 0xd must render PROVIA here; +0x110 = " ++ show (word32At rec 0x110) ++ ", block differs from PROVIA at " ++ spanText (diffOffsets cc lvProvia))

  -- RAM sets: (a) and (c) against their flash set's oracle, and the CC block equal to the flash set's.
  forM_ rams $ \(ram, ra) -> do
    let flashRun preset = do
          stored <- readIORef ccBlocks
          maybe (failH (raName ra ++ ": flash set " ++ raFlash ra ++ " has no CC block for preset " ++ show preset)) pure (M.lookup (raFlash ra, preset) stored)
    patchedChecks ram
      (\preset -> (\(_, ob, okO) -> (ob, okO)) <$> flashRun preset)
      (\label preset cc _ _ -> do
        (flashCc, _, _) <- flashRun preset
        check (label ++ "-cc-equals-flash") (cc == flashCc) ("RAM CC block differs from the CC block of flash set " ++ raFlash ra ++ " at " ++ spanText (diffOffsets cc flashCc)))

  results <- readIORef checks
  rows <- readIORef summary
  B.writeFile (out </> "summary" <.> "json") (encodeCanonical (obj
    [ ("generator", s "//emulation:classic-chrome-builder-cases")
    , ("inputs", arr ([obj [("path", s (pinPath p)), ("sha256", s (pinSha p))] | p <- [id4Pin, id5Pin, loaderPin, payloadPin] ++ (if null rams then [] else [rc0Id4Pin, plainPin, id2Pin])]
                      ++ concat [[obj [("path", s (imPath (isId4 pt))), ("sha256", s (imSha (isId4 pt)))], obj [("path", s (imPath (isId5 pt))), ("sha256", s (imSha (isId5 pt)))], obj [("path", s (prPath pr)), ("sha256", s (prSha pr))]] | (pt, _) <- sets, Just pr <- [isParams pt]]
                      ++ concat [[obj [("path", s (rpManifestPath rp)), ("sha256", s (rpManifestSha rp))], obj [("path", s (rpRuntimePath rp)), ("sha256", s (rpRuntimeSha rp))]] | (rs, _) <- rams, Ram rp <- [isPatch rs]]))
    , ("cases", arr rows)
    , ("checks", arr [obj [("name", s (ckName c)), ("pass", b (ckPass c)), ("detail", s (ckDetail c))] | c <- results]) ]) <> "\n")
  let failed = filter (not . ckPass) results
  putStrLn (show (length results - length failed) ++ "/" ++ show (length results) ++ " checks passed")
  unless (null failed) exitFailure
