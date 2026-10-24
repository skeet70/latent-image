{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
-- Recipe-bank cases observe public returns, readback buffers, live settings
-- and card bytes. Foreign media requests complete synchronously in the model.
module Main where

import Control.Monad
import FujiAF.Host (str)
import Data.Aeson hiding (encodeFile)
import Data.Aeson.Types (Parser, parseEither, parseMaybe)
import qualified Data.ByteString as B
import qualified Data.ByteString.Builder as BB
import qualified Data.ByteString.Lazy as BL
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import Data.Bits
import Data.Char (toUpper)
import Data.Foldable (toList)
import Data.List
import Data.String (fromString)
import Data.Maybe
import Data.Word
import Emulation.NativeCase (functionCount, runCase)
import Emulation.Sha256 (sha256, sha256Hex)
import Emulation.ParallelCases (mapBounded, readJobs)
import Numeric
import System.Directory
import System.Environment
import System.Exit (exitFailure)
import System.FilePath

hex :: Integer -> String
hex x = "0x" ++ replicate (8 - length s) '0' ++ s where s = showHex x ""

hexB :: B.ByteString -> String
hexB = concatMap (\w -> let h = showHex w "" in replicate (2 - length h) '0' ++ h) . B.unpack

unhex :: String -> B.ByteString
unhex s = B.pack (go s) where
  go (a:b:r) = fromIntegral (d a * 16 + d b) : go r
  go _ = []
  d c = fromEnum c - (if c >= 'a' then fromEnum 'a' - 10 else fromEnum '0')

hexI :: String -> Integer
hexI s = case readHex (if take 2 s == "0x" then drop 2 s else s) of [(v, "")] -> v; _ -> 0

u32le :: Word32 -> B.ByteString
u32le v = B.pack [fromIntegral v, fromIntegral (v `shiftR` 8), fromIntegral (v `shiftR` 16), fromIntegral (v `shiftR` 24)]

u16le :: Word16 -> B.ByteString
u16le v = B.pack [fromIntegral v, fromIntegral (v `shiftR` 8)]

readU32 :: B.ByteString -> Int -> Word32
readU32 b o = fromIntegral (B.index b o) + fromIntegral (B.index b (o + 1)) `shiftL` 8
  + fromIntegral (B.index b (o + 2)) `shiftL` 16 + fromIntegral (B.index b (o + 3)) `shiftL` 24

-- bank file codecs, built independently of menu/recipe_bank_storage.c: the
-- checksum covers the whole file with its own four bytes zeroed
checksum :: B.ByteString -> Word32
checksum f = complement (sum [fromIntegral (complement (if i >= 0x18 && i < 0x1c then 0 else B.index f i) :: Word8) | i <- [0 .. B.length f - 1]])
  where complement w = maxBound - w

putChecksum :: B.ByteString -> B.ByteString
putChecksum f = B.take 0x18 f <> u32le (checksum f) <> B.drop 0x1c f

-- | A v1 file (512 bytes), the format the kernel reads only for migration.
encodeFile :: Word32 -> Word16 -> [(B.ByteString, B.ByteString)] -> B.ByteString
encodeFile gen mask slots = putChecksum (hdr <> payloads <> names <> B.replicate 18 0)
  where
    hdr = B.concat [B.pack (map (fromIntegral . fromEnum) "FJRB"), u16le 1, u16le 0x20, u16le 150, u16le 7, u16le 12, u16le 0x28, u32le gen, u32le 462, u32le 0, u16le mask, u16le 0]
    payloads = B.concat [maybe (B.replicate 0x28 0) snd (at i) | i <- [0 .. 6]]
    names = B.concat [maybe (B.replicate 26 0) fst (at i) | i <- [0 .. 6]]
    at i = if i < length slots then Just (slots !! i) else Nothing

data CaptureMode = CaptureP | CaptureS | CaptureA | CaptureM

data BankBase = BaseInvalid | BaseValid CaptureMode B.ByteString

data BankRecord = Unpopulated | Populated B.ByteString B.ByteString B.ByteString

-- | One v2 generation's contents: the selection byte (0 NOTHING SELECTED,
-- 1-7 CUSTOM n), the auto-update byte, the base and seven records.
data BankV2 = BankV2 { bSelection :: Word8, bAuto :: Word8, bBase :: BankBase, bRecords :: [BankRecord] }

-- | A v2 file (1024 bytes): header, selection block, base payload, seven
-- records of name 26, saved 40 and working 40 bytes, zero to 1024.
encodeFileV2 :: Word32 -> BankV2 -> B.ByteString
encodeFileV2 gen bank = putChecksum (B.take 1024 (body <> B.replicate 1024 0))
  where
    records = take 7 (bRecords bank ++ repeat Unpopulated)
    mask = foldl (.|.) 0 [bit k | (k, Populated {}) <- zip [0 ..] records] :: Word16
    hdr = B.concat [B.pack (map (fromIntegral . fromEnum) "FJRB"), u16le 2, u16le 0x40, u16le 150, u16le 7, u16le 12, u16le 0x28, u32le gen, u32le 782, u32le 0, u16le mask, u16le 0]
    (baseValid, modeByte, basePayload) = case bBase bank of
      BaseInvalid -> (0, 0, B.replicate 0x28 0)
      BaseValid m p -> (1, case m of CaptureP -> 1; CaptureS -> 2; CaptureA -> 3; CaptureM -> 4, p)
    selectionBlock = B.pack [bSelection bank, bAuto bank, baseValid, modeByte] <> B.replicate 0x1c 0
    record r = case r of
      Unpopulated -> B.replicate 106 0
      Populated n s w -> n <> s <> w
    body = hdr <> selectionBlock <> basePayload <> B.concat (map record records)

-- | The resident state the kernel derives from a v1 bank: populated slots keep
-- their name with working = saved = the v1 payload, unpopulated slots are
-- dropped, NOTHING SELECTED, DISABLE, base invalid.
migrated :: Word16 -> [(B.ByteString, B.ByteString)] -> BankV2
migrated mask slots = BankV2 0 0 BaseInvalid
  [ if testBit mask k then Populated n p p else Unpopulated
  | (k, (n, p)) <- zip [0 ..] (take 7 (slots ++ repeat (B.replicate 26 0, B.replicate 0x28 0))) ]

-- | One record replaced in a bank.
withRecord :: Int -> BankRecord -> BankV2 -> BankV2
withRecord k r bank = bank { bRecords = [if i == k then r else old | (i, old) <- zip [0 ..] (take 7 (bRecords bank ++ repeat Unpopulated))] }

-- | The native model's card: file A, file B and the swap source, one stride
-- apart (emulation/recipe-bank-model.c RBM_FILE_STRIDE).
fileStride, cardBytes :: Int
fileStride = 0x420
cardBytes = 3 * fileStride

fileSlot :: B.ByteString -> B.ByteString
fileSlot f = B.take fileStride (f <> B.replicate fileStride 0)

name26 :: String -> B.ByteString
name26 s = B.pack (take 26 (concatMap (\c -> [fromIntegral (fromEnum c), 0]) (take 12 s) ++ repeat 0))

data Field = Field { fProp :: !Word32, fOff :: !Int, fShift :: !Int, fBytes :: !Int, fMask :: !Word32 }

data MediaSwapBoundary = BeforeDestructiveOpen | BeforeWrite

swapBoundaryCell :: MediaSwapBoundary -> (String, Word32)
swapBoundaryCell boundary = ("rbm_swap_boundary", case boundary of
  BeforeDestructiveOpen -> 1
  BeforeWrite -> 2)

data CloseFault = NoopWriteClose | KeepVerifyClose | LoseVerifyCallback

closeFaultCell :: CloseFault -> (String, Word32)
closeFaultCell fault = ("rbm_close_fault", case fault of
  NoopWriteClose -> 1
  KeepVerifyClose -> 2
  LoseVerifyCallback -> 3)

extractCodes :: [(Word32, Word32)]
extractCodes = [(0x104, 1), (0x110, 3), (0x111, 4), (0x10e, 4), (0x10f, 3), (0xef, 2), (0xed, 3), (0x141, 0xb), (0xfe, 5)]

placeCode :: B.ByteString -> Field -> Word32 -> B.ByteString
placeCode b f c = B.take off b <> B.pack [byte] <> B.drop (off + 1) b
  where
    off = 0x140 + 0x28 + fOff f
    old = B.index b off
    byte = fromIntegral ((fromIntegral old .&. complement (fMask f)) .|. ((c `shiftL` fShift f) .&. fMask f))

newtype Syms = Syms (M.Map String (Integer, Integer))

addr, sizeOf :: Syms -> String -> Integer
addr (Syms m) n = maybe (error ("symbol missing: " ++ n)) fst (M.lookup n m)
sizeOf (Syms m) n = maybe (error ("symbol missing: " ++ n)) snd (M.lookup n m)

parseSyms :: FilePath -> IO Syms
parseSyms p = Syms . M.fromList . catMaybes . map row . lines <$> readFile p
  where
    row l = case words l of
      [a, z, _, n] -> Just (n, (hexI a, hexI z))
      [a, _, n] -> Just (n, (hexI a, 0))
      _ -> Nothing

newtype CodeBytes = CodeBytes Integer deriving (Eq, Ord)
data CodeSpan = CodeSpan GuestAddress CodeBytes deriving (Eq, Ord)
data ExecutableDomain = ExecutableDomain [CodeSpan] [String]

codeSpan :: Integer -> Integer -> Maybe CodeSpan
codeSpan a z
  | a >= 0 && a `mod` 4 == 0 && z >= 4 && z `mod` 4 == 0 && a + z <= 0x100000000 =
      Just (CodeSpan (GuestAddress a) (CodeBytes z))
  | otherwise = Nothing

renderSpan :: CodeSpan -> String
renderSpan (CodeSpan (GuestAddress a) (CodeBytes z)) = hex a ++ "/" ++ show z

insideSpan :: CodeSpan -> CodeSpan -> Bool
insideSpan (CodeSpan (GuestAddress a) (CodeBytes z)) (CodeSpan (GuestAddress b) (CodeBytes n)) =
  b <= a && a + z <= b + n

containsAddress :: CodeSpan -> GuestAddress -> Bool
containsAddress (CodeSpan (GuestAddress a) (CodeBytes z)) (GuestAddress b) = a <= b && b < a + z

overlappingSpans :: [CodeSpan] -> Bool
overlappingSpans spans = any overlaps (zip ordered (drop 1 ordered))
  where
    ordered = sort spans
    overlaps (CodeSpan (GuestAddress a) (CodeBytes z), CodeSpan (GuestAddress b) _) = a + z > b

functionSpans :: FilePath -> IO [CodeSpan]
functionSpans path = do
  rows <- lines <$> readFile path
  catMaybes <$> mapM row rows
  where
    row line = case words line of
      [_, value, bytes, "FUNC", _, _, section, _]
        | section `elem` ["UND", "ABS"] -> pure Nothing
        | otherwise -> case (readHex value, reads bytes) of
            ([(a, "")], [(z, "")])
              | z == 0 -> pure Nothing
              | otherwise -> maybe (fail ("invalid function span in " ++ path)) (pure . Just) (codeSpan a z)
            _ -> fail ("invalid function metadata in " ++ path)
      fields | "FUNC" `elem` fields -> fail ("invalid function metadata in " ++ path)
      _ -> pure Nothing

loadDomain :: [CodeSpan] -> [FilePath] -> IO ExecutableDomain
loadDomain regions paths = do
  functions <- concat <$> mapM functionSpans paths
  let spans = nub (sort [s | s <- functions, any (insideSpan s) regions])
  when (overlappingSpans spans) (fail "compiled function spans overlap")
  pure (ExecutableDomain spans (map renderSpan spans))

restrictDomain :: CodeSpan -> ExecutableDomain -> ExecutableDomain
restrictDomain region (ExecutableDomain spans _) =
  let selected = filter (`insideSpan` region) spans
   in ExecutableDomain selected (map renderSpan selected)

declaredSpan :: Syms -> Syms -> String -> Maybe CodeSpan
declaredSpan (Syms ks) (Syms ms) name
  | take 2 name == "0x" = case break (== '/') name of
      (a, '/' : z) -> case (readHex (drop 2 a), reads z) of
        ([(start, "")], [(bytes, "")]) -> codeSpan start bytes
        _ -> Nothing
      _ -> Nothing
  | otherwise = case M.lookup name ks of
      Just (a, z) -> codeSpan a z
      Nothing -> M.lookup name ms >>= uncurry codeSpan

seedDomain :: Syms -> Syms -> [ExecutableDomain] -> Spec -> Spec
seedDomain ks ms domains sp = choose domains
  where
    choose [] = sp
    choose (ExecutableDomain spans entries : rest)
      | length functions > functionCount || overlappingSpans combined || stubOverlap || foreignOverlap = choose rest
      | otherwise = sp { spFns = functions }
      where
        retained = [n | n <- spFns sp, maybe True (`notElem` spans) (declaredSpan ks ms n)]
        functions = entries ++ nub retained
        combined = spans ++ mapMaybe (declaredSpan ks ms) retained
        stubAddresses = [GuestAddress (hexI a) | n <- spStubs sp, let (a, _, _) = stubDef n 0]
        stubOverlap = any (\s -> any (containsAddress s) stubAddresses) spans
        foreignOverlap = any (\(CodeSpan (GuestAddress a) (CodeBytes z)) ->
          any (\(GuestAddress b, bytes) -> not (B.null bytes) && a < b + fromIntegral (B.length bytes) && b < a + z)
            (mForeignCode (spModel sp))) spans

data Model = Model
  { mCard :: B.ByteString            -- file A, file B and swap source at 0x00980000
  , mLive :: B.ByteString            -- 0x1c2 bytes at 0x0096cc64
  , mCells :: M.Map String Word32    -- model-data cells by symbol
  , mMode :: Word32
  , mShootingMode :: Word32          -- the mode cell 0x00956838 (0x21 P, 0x22 S)
  , mCase :: B.ByteString            -- bytes at 0x009a0000 (name buffers)
  , mExtra :: [(String, Integer, B.ByteString)] -- named regions at RAM addresses
  , mForeignCode :: [(GuestAddress, B.ByteString)]
  }

data Expect = Expect
  { eStatus :: String, eR0 :: Word32
  , eClaims :: [(String, Int, B.ByteString)]
  }

newtype GuestAddress = GuestAddress Integer deriving (Eq, Ord)
newtype RegionId = RegionId String deriving (Eq, Ord)
newtype StockFailureRedraw = StockFailureRedraw B.ByteString

data BankOperation = BankSaveCurrent | BankReset | BankRename | BankSelect

-- | A dial detent, in the order of the model's RBM_DIAL table.
data Dial = DialP | DialS | DialA | DialM | DialAuto | DialC | DialSports | DialSrAuto
  | DialPortrait | DialLandscape | DialSp | DialAdvanced
  deriving (Eq, Enum, Bounded)

-- | The rbm_seq_selects op that moves the dial to a detent.
dialOp :: Dial -> Word8
dialOp d = 0x80 + fromIntegral (fromEnum d)

-- | The (mode cell, mode class) stock routes at a detent (FUN_0063b4c8,
-- FUN_0063b728); at C the mode is the C record's, M in the fixture.
dialCell :: Dial -> (Word32, Word32)
dialCell d = case d of
  DialP -> (0x21, 0)
  DialS -> (0x22, 0)
  DialA -> (0x23, 0)
  DialM -> (0x24, 0)
  DialAuto -> (0x00, 0)
  DialC -> (0x24, 3)
  DialSports -> (0x0b, 0)
  DialSrAuto -> (0x01, 0)
  DialPortrait -> (0x05, 0)
  DialLandscape -> (0x08, 0)
  DialSp -> (0x18, 0)
  DialAdvanced -> (0x20, 0)

dialName :: Dial -> String
dialName d = case d of
  DialP -> "p"
  DialS -> "s"
  DialA -> "a"
  DialM -> "m"
  DialAuto -> "auto"
  DialC -> "c"
  DialSports -> "sports"
  DialSrAuto -> "sr-auto"
  DialPortrait -> "portrait"
  DialLandscape -> "landscape"
  DialSp -> "sp"
  DialAdvanced -> "adv"

psamDials, excludedDials :: [Dial]
psamDials = [DialP, DialS, DialA, DialM]
excludedDials = [d | d <- [minBound .. maxBound], d `notElem` psamDials]

data Spec = Spec
  { spId :: String, spProv :: String, spEntry :: String
  , spArgs :: [Word32], spModel :: Model, spExpect :: Expect
  , spFns :: [String], spStubs :: [String]
  }

-- the five register-only stubs, per-case outputs
stubDef :: String -> Word32 -> (String, [(String, Word32)], [(String, Word32)])
stubDef n v = case n of
  "task" -> ("0x00723cb8", [], [("r0", v)])
  "card" -> ("0x0071ddf0", [("r0", 0)], [("r0", v)])
  "vol" -> ("0x0071f708", [], [("r0", v)])
  "live" -> ("0x006439e0", [], [("r0", v)])
  "store" -> ("0x0061d8dc", [("r0", 0x0096cc74)], [("r0", 0)])
  "tone-notify" -> ("0x0063c51c", [("r1", 0)], [("r0", v)])
  "pipeline-notify" -> ("0x00641438", [("r1", 0)], [("r0", v)])
  "display-notify" -> ("0x00641334", [], [("r0", v)])
  "mode-state-a" -> ("0x0063e810", [], [("r0", v)])
  "mode-state-b" -> ("0x0063fb74", [], [("r0", v)])
  "mode-state-c" -> ("0x00640e78", [], [("r0", v)])
  "journal-stub" -> ("0x0072b858", [], [("r0", v)])
  "relay-folder" -> ("0x007160b8", [], [("r0", 1)])
  "relay-front-ready" -> ("0x007240c8", [], [("r0", 0)])
  "relay-tag-lock" -> ("0x00724b68", [("r0", 0x3c)], [("r0", 0)])
  "out-window-stub" -> ("0x0004357c", [], [("r0", v)])
  _ -> error ("unknown stub " ++ n)

data BankMode = SdBank | VolatileBank deriving (Eq)

bankModeText :: BankMode -> String
bankModeText SdBank = "sd"
bankModeText VolatileBank = "volatile"

data VolOp = VolFixed Word32 | VolMove Word32 Word32 | VolLoad Word32 Integer
  | VolStore Word32

armImmediate :: Word32 -> Maybe Word32
armImmediate v =
  listToMaybe [ fromIntegral (rot `div` 2) `shiftL` 8 .|. r
             | rot <- [0, 2 .. 30], let r = v `rotateL` rot, r <= 0xff ]

-- Literals follow the instructions; ARM loads use PC+8.
volWords :: [VolOp] -> [Word32]
volWords ops = snd (mapAccumL word 0 (zip [0 ..] ops))
  ++ [fromIntegral v | VolLoad _ v <- ops]
  where
    n = length ops
    word ordinal (_, VolFixed w) = (ordinal, w)
    word ordinal (_, VolMove r e) = (ordinal, 0xe3a00000 .|. (r `shiftL` 12) .|. e)
    word ordinal (_, VolStore o) = (ordinal, 0xe5840000 .|. o)
    word ordinal (i, VolLoad r _) =
      let off = 4 * (n + ordinal - i) - 8 :: Int
       in if off < 0 || off > 0xfff
            then error "volatile workload: literal outside the ldr range"
            else (ordinal + 1, 0xe59f0000 .|. (r `shiftL` 12) .|. fromIntegral off)

-- | Calls public entries with r0-r3, stores each r0 at 0x009a0200 + 4*k and
-- returns the last status in r0; r4 holds the case buffer, lr the exit address.
volWorkload :: [(Integer, [Word32])] -> B.ByteString
volWorkload calls = BL.toStrict (BB.toLazyByteString (foldMap BB.word32LE (volWords ops)))
  where
    ops = concat
      [ [VolFixed 0xe92d4010, VolLoad 4 0x009a0000]
      , concat
          [ concatMap argument (zip [0, 1, 2, 3] args)
              ++ [VolLoad 12 target, VolFixed 0xe12fff3c, VolStore (0x200 + 4 * k)]
          | (k, (target, args)) <- zip [(0 :: Word32) ..] calls ]
      , [VolFixed 0xe8bd8010] ]
    argument (r, v) = case armImmediate v of
      Just e -> [VolMove r e]
      Nothing -> [VolLoad r (toInteger v)]

main :: IO ()
main = do
  args <- getArgs
  let opt n = lookup n [(drop 2 a, b) | (a, b) <- pair args]
      pair (a:b:r) = (a, b) : pair r
      pair _ = []
  build <- maybe (fail "--build") pure (opt "build")
  out <- maybe (fail "--out") pure (opt "out")
  runner <- maybe (fail "--runner") pure (opt "runner")
  fieldsPath <- maybe (fail "--fields") pure (opt "fields")
  id4Path <- maybe (fail "--id4") pure (opt "id4")
  id5Path <- maybe (fail "--id5") pure (opt "id5")
  hostFix <- maybe (fail "--host-fixtures") pure (opt "host-fixtures")
  jobs <- readJobs (opt "jobs")
  let candidateDir = fromMaybe "private-data/analysis/x-m1-1.50/recipes/rc-complete-20261013/package/image" (opt "candidate")
      kernelBase = hexI (fromMaybe "0x00be1f00" (opt "rb-base"))
      id4Base = hexI (fromMaybe "0x0061c400" (opt "id4-base"))
  createDirectoryIfMissing True out
  ks <- parseSyms (build </> "kernel.syms")
  unless (addr ks "__rb_image_start" == kernelBase) (fail "--rb-base differs from linked __rb_image_start")
  ms <- parseSyms (build </> "model.syms")
  kbin <- B.readFile (build </> "kernel.bin")
  ktext <- B.readFile (build </> "kernel-text.bin")
  mdataRaw <- B.readFile (build </> "model-data.bin")
  id4Raw <- B.readFile id4Path
  unless (id4Base <= 0x61c400 && id4Base >= 0x61c1fc) (fail "--id4-base outside the 1.50 prefix range")
  let id4 = B.drop (fromIntegral (0x61c400 - id4Base)) id4Raw
  id5 <- B.readFile id5Path
  fieldsRaw <- B.readFile fieldsPath
  let ktextLen = B.length ktext
      kdataLen = B.length kbin - ktextLen
      fields = parseFields fieldsRaw
      fTable = B.concat [B.concat [u32le (fProp f), u32le (fromIntegral (fOff f)), u32le (fromIntegral (fShift f)), u32le (fromIntegral (fBytes f)), u32le (fMask f)] | f <- fields]
      mediaSpans = ["rbm_low_close", "rbm_read", "rbm_write", "rbm_close", "rbm_open", "rbm_stat", "rbm_index_free", "rbm_mapping_delete", "rbm_resolve", "rbm_tag_release"]
      mediaLo = minimum [addr ms n | n <- mediaSpans]
      mediaHi = maximum [addr ms n + sizeOf ms n | n <- mediaSpans ++ ["helpers"]]
      chunk n = B.readFile (build </> ("model-" ++ n ++ ".bin"))
  getter <- chunk "getter"
  setter <- chunk "setter"
  mediaParts <- forM mediaSpans $ \n -> do
    b <- B.readFile (build </> ("model-" ++ n ++ ".bin"))
    pure (addr ms n, b)
  extraParts <- forM ["helpers"] $ \n -> do
    b <- B.readFile (build </> ("model-" ++ n ++ ".bin"))
    pure (addr ms n, b)
  let mediaImg = compose mediaLo mediaHi (mediaParts ++ extraParts)
      compose lo hi ps = foldl (\acc (a, p) -> B.take (fromIntegral (a - lo)) acc <> p <> B.drop (fromIntegral (a - lo) + B.length p) acc) (B.replicate (fromIntegral (hi - lo)) 0) ps
  (tabs, tabEntries) <- loadTables id4
  let (tabBlob, tabAt) = packTables id4 tabEntries
      tabImg = B.take 0xc00 (B.drop (fromIntegral (0x81b09c - 0x61c400)) id4) `rewrite` tabAt `append` tabBlob
      rewrite blk at = B.concat [if ptr == 0 then B.take 4 (B.drop p blk) else u32le (fromIntegral (M.findWithDefault 0 (fromIntegral p `div` 4) at)) | p <- [0, 4 .. 0xbfc], let ptr = readU32 blk p]
      append :: B.ByteString -> B.ByteString -> B.ByteString
      append a b = a <> b
      _ = append
  hostBank <- B.readFile (hostFix </> "host-bank.bin")
  id4Final <- B.readFile (candidateDir </> "id4-final.bin")
  id5Final <- B.readFile (candidateDir </> "id5-final.bin")
  let d3Id4 = sha256Hex (sha256 id4Final)
      d3Id5 = sha256Hex (sha256 id5Final)
  putStrLn ("kernel .text " ++ show ktextLen ++ " .data " ++ show kdataLen ++ " media " ++ show (B.length mediaImg) ++ " fields " ++ show (length fields))
  putStrLn
    ( "compiled-recipe candidate "
        ++ candidateDir
        ++ ": id4-final "
        ++ d3Id4
        ++ ", id5-final "
        ++ d3Id5
    )
  bankMode <- case fromMaybe "sd" (opt "bank-mode") of
    "sd" -> pure SdBank
    "volatile" -> pure VolatileBank
    _ -> fail "--bank-mode must be sd or volatile"
  domains <- case bankMode of
    SdBank -> do
      let region a z = maybe (fail "invalid compiled executable region") pure (codeSpan a z)
      kernelRegion <- region kernelBase (fromIntegral ktextLen)
      mediaRegion <- region mediaLo (fromIntegral (B.length mediaImg))
      domain <- loadDomain [kernelRegion, mediaRegion] [build </> "kernel.symbols", build </> "model.symbols"]
      pure [domain, restrictDomain mediaRegion domain]
    VolatileBank -> pure []
  let failureRedraw = StockFailureRedraw (B.take 132 (B.drop (0x00d38d98 - 0x00bf1a00) id5))
      (sdSpecs, volatileSpecs) = specSet ks ms fields tabs hostBank failureRedraw (realStock (GuestAddress id4Base) id4Raw id5)
      suite = case bankMode of
        VolatileBank -> volatileSpecs
        SdBank -> map (seedDomain ks ms domains) sdSpecs ++ relaySpecSet ks (GuestAddress id4Base) id4Raw ++ d3SpecSet id4Final id5Final
      -- --case ID runs one case; ID* runs every case whose id starts with ID
      selects cid = case reverse cid of
        '*' : prefix -> (reverse prefix `isPrefixOf`)
        _ -> (== cid)
      specs = maybe suite (\cid -> filter (selects cid . spId) suite) (opt "case")
  when (null specs) (fail "--case did not name a case")
  results <- mapBounded jobs (\sp -> do
    let loop s fuel = do
          r <- runSpec bankMode build out runner ks ms kbin ktextLen kdataLen mdataRaw fTable getter setter mediaImg tabImg mediaLo s
          let missing = parseMaybe (withObject "r" (\o -> o .: "missing_fn" >>= withText "t" (pure . T.unpack))) r
          case missing of
            -- a volatile case declares its foreign boundary; an undeclared call fails it
            Just n | fuel > 0 && bankMode == SdBank && n `notElem` spFns s ->
              loop (s { spFns = spFns s ++ [n] }) (fuel - 1)
            _ -> pure r
    loop sp (48 :: Int)) specs
  bootResults <- maybe (pure []) (\package -> (++) <$> runBootCases jobs out runner package <*> runAllocatorCases jobs out runner package) (opt "boot-package")
  let allResults = results ++ bootResults
  BL.writeFile (out </> "results.json") (encode (object ["cases" .= allResults, "bank_mode" .= bankModeText bankMode, "compiled_recipe_candidate" .= object ["dir" .= candidateDir, "id4_final_sha256" .= d3Id4, "id5_final_sha256" .= d3Id5]]))
  writeFile (out </> "findings.txt") (findingsText specs results ++ if null bootResults then "" else "\nBoot cases use register-only stock semaphore ABI boundaries and stop at DSB after publication stores. Cache/core/source-coherence acceptance is not claimed. Allocator cases execute the packaged stock backing allocator at its new capacity boundary.\n")
  putStrLn ("bank mode " ++ bankModeText bankMode ++ ": " ++ show (length specs) ++ " cases")
  putStrLn ("ran " ++ show (length allResults) ++ " cases")
  unless (all ((== Just ("passed" :: String)) . parseMaybe (withObject "result" (.: "status"))) allResults) exitFailure

parseFields :: B.ByteString -> [Field]
parseFields raw = either (error . ("invalid field map: " ++)) id (eitherDecodeStrict' raw >>= parseEither p)
  where
    p = withObject "f" $ \o -> do
      pfm <- o .: "payload_field_map"
      fl <- pfm .: "fields" :: Parser [Value]
      mapM one fl
    one v = withObject "field" inner v
    inner f = do
      prop <- f .: "property"
      off <- f .: "offset"
      bits <- f .: "bits"
      bytes <- f .:? "bytes" .!= (0 :: Int)
      case bitsOf (str' bits) of
        Just (s, m) ->
          let width = length [() | bit <- [0 .. 15 :: Int], m .&. (1 `shiftL` bit) /= 0]
              count = if bytes == 0 then (width + 7) `div` 8 else bytes
           in pure (Field (fromIntegral (hexI (str' prop))) (fromIntegral (hexI (str' off))) s count m)
        Nothing -> fail ("bits range not mapped: " ++ str' bits)
    bitsOf s = case span (/= '-') s of
      (lo, '-' : hi) ->
        let l = read lo :: Int
            h = read hi :: Int
            w = h - l + 1
         in if w >= 1 && w <= 16 then Just (l, ((1 `shiftL` w) - 1) `shiftL` l) else Nothing
      (b, "") ->
        let l = read b :: Int
         in Just (l, 1 `shiftL` l)
      _ -> Nothing
    str' :: Value -> String
    str' (String t) = T.unpack t
    str' _ = ""

loadTables :: B.ByteString -> IO (M.Map Word32 [[Word32]], [(Word32, Integer)])
loadTables id4 = pure (M.fromList [(pr, rowsAt t) | (pr, t) <- entries], entries)
  where
    base = 0x61c400 :: Integer
    u32at a = readU32 id4 (fromIntegral (a - base))
    ptr p = fromIntegral (readU32 (B.take 0xc00 (B.drop (fromIntegral (0x81b09c - base)) id4)) p)
    entries = [(fromIntegral p `div` 4, ptr p) | p <- [0, 4 .. 0xbfc], ptr p /= 0]
    rowsAt t = takeWhile (\row -> case row of c : _ -> c /= 0xffff; [] -> False) [ [u32at (t + 12 * i), u32at (t + 12 * i + 4), u32at (t + 12 * i + 8)] | i <- [0 .. 63]]

packTables :: B.ByteString -> [(Word32, Integer)] -> (B.ByteString, M.Map Word32 Word32)
packTables id4 entries = (B.concat (map snd pieces), M.fromList (zip (map fst pieces) (snd (foldl step (0x81b09c + 0xc00, []) (map snd pieces)))))
  where
    step (a, acc) b = (a + fromIntegral (B.length b), acc ++ [fromIntegral a])
    base = 0x61c400 :: Integer
    pieces = [(pr, B.take (fromIntegral (12 * n)) (B.drop (fromIntegral (t - base)) id4)) | (pr, t) <- entries, let n = length (take 64 (takeWhile (\i -> readU32 id4 (fromIntegral (t - base) + 12 * i) /= 0xffff) [0 :: Int ..]))]

-- | The real-stock slices the kernel's field walk and code scan execute (the
-- storage lane's real-byte contract and the D3 row-path cases): the chain and
-- dispatcher code with the two copy wrappers replaced by one bx lr each, the
-- conversion tables and pointer block at their stock addresses, the cells
-- block with the mode cell 0x21 and the settings mirror with the not-found
-- flag set. The menu, mode and availability tables share one region, and the
-- id5 slice runs on through the render probe FUN_00d52684. The synthetic
-- getter and setter retire: 0x0061e110 and 0x00624694 are the real code
-- inside the chain.
realStock :: GuestAddress -> B.ByteString -> B.ByteString -> [(String, Integer, B.ByteString)]
realStock (GuestAddress id4Base) id4Raw id5 =
  [ ("chain", id4Base, chain)
  , ("dispatcher", 0x00d4d604, B.take (0x00d5c000 - 0x00d4d604) (B.drop (0x00d4d604 - 0x00bf1a00) id5))
  , ("conv-tables", 0x00774f48, B.take (0x780000 - 0x774f48) (B.drop (0x774f48 - 0x61c400) id4))
  , ("conv-ptrs", 0x0081b09c, B.take 0x860 (B.drop (0x81b09c - 0x61c400) id4))
  , ("cells", 0x00956838, poke 0 (u32le 0x21) (B.replicate 0x1000 0))
  , ("menutab", 0x00780000, B.take (0x7a13c8 + 4 * 0x2d - 0x780000) (B.drop (0x780000 - 0x61c400) id4))
  , ("media-state", 0x00910140, B.replicate 0x42100 0)
  , ("mirror", 0x0089ec00, poke 0x1c8 (u32le 1) (B.replicate 0x200 0))
  ]
  where
    id4 = B.drop (fromIntegral (0x61c400 - id4Base)) id4Raw
    chain = poke (fromIntegral (0x61d9ec - id4Base)) bxlr
      (poke (fromIntegral (0x61d8dc - id4Base)) bxlr raw)
    raw = B.take (fromIntegral (0x64891c - id4Base)) id4Raw
    bxlr = B.pack [0x1e, 0xff, 0x2f, 0xe1]
    poke o v b = B.take o b <> v <> B.drop (o + B.length v) b

data RelayClient = RecipeClient | StockClient | SynchronousClient
  deriving (Eq)

data RelayRegister = RelayR0 | RelayR2 | RelayR12

-- Queue delivery is synchronous. The front task and media worker execute stock
-- bytes; the filesystem leaf records each open attempt and returns raw 7.
relaySpecSet :: Syms -> GuestAddress -> B.ByteString -> [Spec]
relaySpecSet ks (GuestAddress imageBase) image =
  [ fixture RecipeClient True
  , fixture RecipeClient False
  , fixture StockClient True
  , fixture SynchronousClient True
  ]
  where
    fixture client changed = Spec name
      "contract: an accepted recipe open refuses changed media before the filesystem open boundary; stock clients and same-card opens retain completion, relay wake and tag cleanup. Stock X-M1 front-task forwarding, callback relay, open worker, open implementation, status mapper, tag release and mapping cleanup execute unchanged. Queue delivery is synchronous, filesystem open returns raw missing-file status, and scheduler/semaphore boundaries are host adapters."
      "0x009b0000" [] initial
      (Expect "returned" status
        ([ ("casebuf", 0x100, B.concat (map u32le [status, 0, 1, 1, attempts]))
        , ("media-state", 0, B.replicate 128 0)
        , ("media-state", 0x80, B.replicate 64 0)
        , ("media-state", 0x14c, u32le 0)
        , ("media-state", 0xfed8, u32le 0)
        ] ++ [("casebuf", 0x114, u32le status <> u32le 0) | client == StockClient]))
      (kernelFns ++ rawFns ++ ["0x009b0000/40", "0x009b0200/20"])
      ["relay-folder", "relay-front-ready", "relay-tag-lock"]
      where
        name = "r-media-relay-" ++ case client of
          RecipeClient -> if changed then "changed" else "same-card"
          StockClient -> "stock-client"
          SynchronousClient -> "synchronous-client"
        refused = client == RecipeClient && changed
        status = if refused then 2 else 6
        attempts = if refused then 0 else 1
        callback = case client of
          RecipeClient -> fromIntegral (addr ks "rb_io_cb")
          StockClient -> 0x009b0200
          SynchronousClient -> 0
        request = B.concat
          [ u32le callback
          , B.take 44 ("C:\\RECIPEA.BIN\0" <> B.replicate 44 0)
          , u32le 0, u32le 0, u32le 1, u32le 1
          , u32le 1, u32le 0x009a0100, u32le 0x009a0104, u32le 1 ]
        message = u16le 0x2c17 <> B.pack [0x11, 0x65] <> u32le 80 <> u32le 0x009a0400
        caseBytes = patch 0x500 message (patch 0x400 request (B.replicate 0x600 0))
        state = patch 0x26b30 (u32le 1)
          (patch 0xc4 (u32le 0x00920000)
          (patch 0x80 (u32le 1) (B.replicate 0x42100 0)))
        initial = Model (B.replicate cardBytes 0) (B.replicate 0x1c2 0) M.empty 0 0x24 caseBytes
          [ ("media-state", 0x00910140, state)
          , ("prefix", 0x009b0000, driver <> B.replicate (0x200 - B.length driver) 0 <> stockCallback) ]
          (stockCode ++ adapters changed)
    patch offset bytes original = B.take offset original <> bytes <> B.drop (offset + B.length bytes) original
    words32 = B.concat . map u32le
    load register value = words32
      [ 0xe3000000 .|. (operand `shiftL` 12) .|. (value .&. 0xfff) .|. ((value .&. 0xf000) `shiftL` 4)
      , 0xe3400000 .|. (operand `shiftL` 12) .|. ((value `shiftR` 16) .&. 0xfff) .|. ((value .&. 0xf0000000) `shiftR` 12) ]
      where operand = case register of RelayR0 -> 0; RelayR2 -> 2; RelayR12 -> 12
    call target = load RelayR12 target <> words32 [0xe12fff3c]
    driver = words32 [0xe92d4010] <> load RelayR0 0x009a0500
      <> call (fromIntegral (addr ks "rb_media_request_post"))
      <> load RelayR0 0x009a0100 <> words32 [0xe5900000, 0xe8bd8010]
    stockCallback = load RelayR2 0x009a0114 <> words32 [0xe5820000, 0xe5821004, 0xe12fff1e]
    adapters changed =
      [ (GuestAddress 0x0071fde4, words32 [0xe3a01001, 0xe3a02002] <> jump 0x0072129c)
      , (GuestAddress 0x0072503c, words32 [0xe92d4010, 0xe1a04001]
          <> (if changed then call (fromIntegral (addr ks "rb_media_epoch_bump")) else B.empty)
          <> words32 [0xe1a00004] <> call (fromIntegral (addr ks "rb_media_guarded_open"))
          <> words32 [0xe3a00000, 0xe8bd8010])
      , (GuestAddress 0x00701634, increment 0x009a0110 <> words32 [0xe3a00007, 0xe12fff1e])
      , (GuestAddress 0x00724144, increment 0x009a010c <> words32 [0xe3a00000, 0xe12fff1e])
      , (GuestAddress 0x00724c00, words32 [0xe350003b, 0x112fff1e]
          <> increment 0x009a0108 <> words32 [0xe3a00000, 0xe12fff1e])
      ]
    increment target = load RelayR2 target <> words32 [0xe5923000, 0xe2833001, 0xe5823000]
    jump target = load RelayR12 target <> words32 [0xe12fff1c]
    kernelFns = ["rb_media_request_post", "rb_io_request_identity", "rb_media_guarded_open",
      "rb_media_open_is_recipe", "rb_media_matches", "rb_media_epoch", "rb_media_epoch_bump",
      "rb_media_complete", "rb_io_cb", "rb_call_stock_media_open"]
    spans =
      [ (0x0070e6c4, 0x84), (0x007115a4, 0x140), (0x0071c634, 0x14)
      , (0x0071d450, 0x21c), (0x0071e960, 0x8c), (0x0071e9ec, 0x48), (0x0071ea34, 0x78)
      , (0x0071eaac, 0x7c), (0x0071eff4, 0x84), (0x0071f2a8, 0x170)
      , (0x007211d4, 0xc8), (0x0072129c, 0x124), (0x007217f0, 0x44) ]
    stockCode = [(GuestAddress address, B.take count (B.drop (fromIntegral (address - imageBase)) image)) | (address, count) <- spans]
    adapterSpans = [(0x0071fde4, 20), (0x0072503c, 64), (0x00701634, 28),
      (0x00724144, 28), (0x00724c00, 36)]
    rawFns = [hex address ++ "/" ++ show count | (address, count) <- spans ++ adapterSpans]

-- the compiled-recipe consumer family ---------------------------------------

-- | The composed candidate's image slices at their runtime addresses: the
-- prefix-resident hook and tables, the commit dispatcher with the patched
-- case-0xfe site, the chain (its two copy wrappers one bx lr word, as the
-- kernel family does), the conversion tables and pointer block, the consumer
-- cell block and the settings mirror.
d3Extras :: B.ByteString -> B.ByteString -> [(String, Integer, B.ByteString)]
d3Extras id4f id5f =
  [ ("prefix", 0x61c1fc, at4 0x61c1fc 0x204)
  , ("dispatcher", 0xd4d604, at5 0xd4d604 20608)
  , ("chain", 0x61c91c, chain)
  , ("conv-tables", 0x774f48, at4 0x774f48 0x68dc)
  , ("conv-ptrs", 0x81b09c, at4 0x81b09c 0x860)
  , ("modetab", 0x7a0018, at4 0x7a0018 (0x7a13c8 + 4 * 0x2d - 0x7a0018))
  , ("cells", 0x956838, poke 0 (u32le 0x21) (B.replicate 0x310 0))
  , ("mirror", 0x89ec00, poke 0x1c8 (u32le 1) (B.replicate 0x200 0))
  , ("mode-cell", 0x951564, B.replicate 4 0)
  ]
 where
  at4 a n = B.take n (B.drop (fromIntegral (a - 0x61c1fc)) id4f)
  at5 a n = B.take n (B.drop (fromIntegral (a - 0xbf1a00)) id5f)
  chain = poke (0x61d9ec - 0x61c91c) bxlr (poke (0x61d8dc - 0x61c91c) bxlr (at4 0x61c91c 0x2c000))
  bxlr = B.pack [0x1e, 0xff, 0x2f, 0xe1]
  poke o v b = B.take o b <> v <> B.drop (o + B.length v) b

-- | The compiled-recipe consumer cases: the stock commit dispatcher entered
-- with property 0xfe and a film row token. The three stock rows are the modes
-- the body's nine-shot render check selected; the two recipe rows are the
-- compiled ones. The consumer cells and live-field bits are the maps the
-- kernel family already derives from the stock arms.
d3SpecSet :: B.ByteString -> B.ByteString -> [Spec]
d3SpecSet id4f id5f = map mk d3Cases
 where
  mk (cid, claim, rowToken, recipeTable) =
    Spec
      cid
      claim
      "0x00d4fc74"
      (d3Args rowToken)
      (Model (B.replicate 0x660 0) (B.replicate 0x1c2 0xaa) M.empty 1 0x21 B.empty (d3Extras id4f id5f) [])
      (Expect "returned" 0x4d (d3Claims rowToken recipeTable))
      d3Fns
      d3Stubs
  d3Cases =
    [ ( "d3-render-provia"
      , "the PROVIA row (film row 0) committed through the stock dispatcher on the composed candidate: the renderer input is stored code 0 with downstream value 0, and no recipe parameter is applied, so the seven parameter cells keep the fixture bytes"
      , 0
      , Nothing
      )
    , ( "d3-render-pro-neg-std"
      , "the PRO Neg Std row (film row 5): stored code 9 and downstream value 0xb, the second render input the nine-shot check measured"
      , 5
      , Nothing
      )
    , ( "d3-render-pro-neg-hi"
      , "the PRO Neg Hi row (film row 6): stored code 0xa and downstream value 0xc, the third render input the nine-shot check measured"
      , 6
      , Nothing
      )
    , ( "d3-row-a-consumer"
      , "the compiled recipe A row (film row 2) applied through the stock dispatcher: the film field stores 0xd with PROVIA's downstream value 0, and the seven parameter cells take the recipe's rows' col2 values - distinct from the stock PROVIA input, whose cells stay at the fixture bytes"
      , 2
      , Just 0
      )
    , ( "d3-row-b-consumer"
      , "the compiled recipe B row (film row 4): the film field stores 0xe with Velvia's downstream value 3, and the parameter cells take the recipe's rows' col2 values - distinct from recipe A and from PROVIA"
      , 4
      , Just 1
      )
    ]
  d3Args rowToken =
    [0xfe, rowToken, 0, 0, 0x40000004, 0x50000005, 0x60000006, 0x70000007, 0x80000008, 0x90000009, 0xa000000a, 0xb000000b, 0xc000000c]
  -- the hook walk's row-table order, the live byte each property commits
  -- through, its field bits, and the consumer cell the arm pushes the row's
  -- col2 into (film first, then the seven parameter cells)
  d3Params =
    [ (0x104, 0x5a, 0x60, 5)
    , (0x110, 0x26, 0xe0, 5)
    , (0x111, 0x34, 0xe0, 5)
    , (0x10e, 0x2b, 0xe0, 5)
    , (0x10f, 0x33, 0xe0, 5)
    , (0xef, 0x2d, 0xe0, 5)
    , (0xed, 0x85, 0xf0, 4)
    , (0x141, 0x52, 0x1f, 0)
    ]
  d3Cells = [(0x104, 0x84), (0x110, 0x308), (0x111, 0x30c), (0x10e, 0x88), (0x10f, 0x68), (0xef, 0x164), (0xed, 0x2c)]
  u32at a = readU32 id4f (fromIntegral (a - 0x61c1fc))
  rowsAt p = go 0
   where
    t = u32at (0x81b09c + 4 * p)
    go i = let w0 = u32at (t + 12 * i)
            in if w0 == 0xffff then [] else (w0, u32at (t + 12 * i + 8)) : go (i + 1)
  rowAt p r = rowsAt p !! fromIntegral r
  tableRows k = [fromIntegral (B.index id4f (fromIntegral (0x61c38c + 8 * k + i - 0x61c1fc))) | i <- [0 .. 7]]
  byteOf (_, mask, shift) code = B.pack [fromIntegral ((0xaa .&. complement mask) .|. ((code `shiftL` shift) .&. mask))]
  d3Claims rowToken recipeTable =
    ("cells", 0x80, u32le (col2 0xfe rowToken))
      : [("cells", off, u32le v) | (off, v) <- paramCells]
      ++ [("live-state", 0x8a, byteOf (0x8a, 0xf0, 4) (col0 0xfe rowToken))]
      ++ [("live-state", off, byteOf (off, mask, shift) code) | ((_, off, mask, shift), code) <- paramFields]
   where
    col0 p r = fst (rowAt p r)
    col2 p r = snd (rowAt p r)
    paramCells = case recipeTable of
      Nothing -> [(off, 0) | (_, off) <- d3Cells]
      Just k -> [(off, col2 p r) | ((p, off), r) <- zip d3Cells (tableRows k)]
    paramFields = case recipeTable of
      Nothing -> []
      Just k -> [((p, off, mask, shift), col0 p r) | ((p, off, mask, shift), r) <- zip d3Params (tableRows k)]
  d3Fns =
    [ "0x00d4d604/20"
    , "0x00d4fc74/10768"
    , "0x0061c3ac/76"
    , "0x0061c91c/16"
    , "0x0061c93c/16"
    , "0x0061d514/16"
    , "0x0061d8dc/80"
    , "0x0061d9ec/80"
    , "0x0061e110/25832"
    , "0x00624620/44"
    , "0x00624694/49968"
    , "0x006309c4/112"
    , "0x00631a00/336"
    , "0x0063396c/32"
    , "0x00633b6c/32"
    , "0x00633b8c/32"
    , "0x00633cac/32"
    , "0x00633d0c/32"
    , "0x0063458c/32"
    , "0x0063460c/32"
    , "0x006347b0/32"
    , "0x006347d0/32"
    , "0x006439e0/40"
    , "0x00643d94/48"
    , "0x00643dc4/40"
    , "0x00643fb8/76"
    , "0x00644848/476"
    ]
  d3Stubs = ["tone-notify", "pipeline-notify", "display-notify", "mode-state-a", "mode-state-b", "mode-state-c"]

specSet :: Syms -> Syms -> [Field] -> M.Map Word32 [[Word32]] -> B.ByteString -> StockFailureRedraw -> [(String, Integer, B.ByteString)] -> ([Spec], [Spec])
specSet ks ms fields tabs hostBank (StockFailureRedraw failureRedraw) real = (map ($ ()) builders, volatileBuilders)
  where
    -- contract-derived fixture: every field carries a distinct valid code
    liveFixture :: B.ByteString
    liveFixture = foldl lay (B.replicate 0x1c2 0) [(f, i) | (f, i) <- zip fields [1 ..]]
    lay b (f, i) = placeCode b f (validFor (fProp f) i)
    validFor p i = case M.lookup p tabs of
      Just rows@(_:_) -> col0 (rows !! (fromIntegral i `mod` length rows))
      _ -> fromIntegral i
    col0 r = case r of
      c : _ -> c
      [] -> 0
    -- The live layout of the walked rows, read from the stock getter's
    -- ordinary-branch arms: byte offset in the 0x0096cc64 block, mask, shift.
    -- The payload's record map differs in offset and shift for some rows
    -- (0x1d, 0xfa, 0x1b1 among them), so the save's walk needs both.
    liveTable :: [(Word32, Int, Word32, Int)]
    liveTable =
      [ (0x01cb, 0x10, 0xffff, 0)
      , (0x01cc, 0x12, 0xffff, 0)
      , (0x01c6, 0x1a6, 0x00ff, 0)
      , (0x01cf, 0x1c, 0x003f, 0)
      , (0x0104, 0x5a, 0x007f, 5)
      , (0x01ce, 0x1b, 0x003f, 0)
      , (0x001d, 0x80, 0x007f, 5)
      , (0x0123, 0x2d, 0x001f, 0)
      , (0x010e, 0x2b, 0x00e0, 5)
      , (0x0121, 0x2e, 0x001f, 0)
      , (0x010f, 0x33, 0x00e0, 5)
      , (0x011f, 0x2f, 0x001f, 0)
      , (0x0111, 0x34, 0x00e0, 5)
      , (0x0117, 0x30, 0x001f, 0)
      , (0x011d, 0x31, 0x001f, 0)
      , (0x0152, 0x47, 0x00e0, 5)
      , (0x011b, 0x32, 0x001f, 0)
      , (0x014f, 0x4b, 0x00e0, 5)
      , (0x0125, 0x33, 0x001f, 0)
      , (0x00fa, 0x46, 0x00e0, 5)
      , (0x0115, 0x34, 0x001f, 0)
      , (0x01fb, 0x25, 0x00e0, 5)
      , (0x0124, 0x38, 0x001f, 0)
      , (0x01fe, 0x27, 0x00e0, 5)
      , (0x0122, 0x39, 0x001f, 0)
      , (0x00ef, 0x2d, 0x00e0, 5)
      , (0x0120, 0x3a, 0x001f, 0)
      , (0x01fd, 0x28, 0x00e0, 5)
      , (0x0118, 0x3b, 0x001f, 0)
      , (0x00f2, 0x35, 0x00e0, 5)
      , (0x011e, 0x3c, 0x001f, 0)
      , (0x0137, 0x36, 0x00e0, 5)
      , (0x011c, 0x3d, 0x001f, 0)
      , (0x010c, 0x39, 0x00e0, 5)
      , (0x0126, 0x3e, 0x001f, 0)
      , (0x0109, 0x30, 0x00e0, 5)
      , (0x0116, 0x3f, 0x001f, 0)
      , (0x0217, 0x3a, 0x00e0, 5)
      , (0x00ee, 0x41, 0x001f, 0)
      , (0x0110, 0x26, 0x00e0, 5)
      , (0x015c, 0x57, 0x001f, 0)
      , (0x0129, 0x3b, 0x00e0, 5)
      , (0x015b, 0x58, 0x001f, 0)
      , (0x01fc, 0x24, 0x00e0, 5)
      , (0x015d, 0x59, 0x001f, 0)
      , (0x00f6, 0x6f, 0x007f, 5)
      , (0x01e3, 0x6a, 0x0080, 7)
      , (0x015a, 0x61, 0x001f, 0)
      , (0x01c2, 0x75, 0x007f, 5)
      , (0x01e2, 0x6b, 0x0080, 7)
      , (0x0158, 0x62, 0x001f, 0)
      , (0x0139, 0x5b, 0x007f, 5)
      , (0x00f3, 0x97, 0x0007, 2)
      , (0x0159, 0x50, 0x001f, 0)
      , (0x01d9, 0x7e, 0x007f, 5)
      , (0x017e, 0x99, 0x001f, 4)
      , (0x014e, 0x5b, 0x001f, 0)
      , (0x00f9, 0x7d, 0x007f, 5)
      , (0x0147, 0x9c, 0x0003, 1)
      , (0x014b, 0x60, 0x001f, 0)
      , (0x0162, 0x67, 0x007f, 5)
      , (0x0146, 0x9c, 0x0007, 2)
      , (0x0141, 0x52, 0x001f, 0)
      , (0x0164, 0x6a, 0x007f, 5)
      , (0x0149, 0x9c, 0x000f, 3)
      , (0x00ed, 0x85, 0x00f0, 4)
      , (0x0156, 0x84, 0x000f, 0)
      , (0x0153, 0x86, 0x000f, 0)
      , (0x01c5, 0x90, 0x00f0, 4)
      , (0x01b1, 0x83, 0x00f0, 4)
      , (0x0138, 0x87, 0x00f0, 4)
      , (0x00fe, 0x8a, 0x00f0, 4)
      , (0x01d8, 0x95, 0x00f0, 4)
      , (0x0214, 0x86, 0x00f0, 4)
      , (0x0163, 0x6d, 0x007f, 5)
      , (0x0161, 0x6e, 0x007f, 5)
      , (0x0183, 0x77, 0x007f, 5)
      , (0x0107, 0x96, 0x000f, 2)
      , (0x0106, 0x96, 0x003f, 4)
      , (0x0148, 0x9c, 0x0080, 7)
      , (0x0142, 0x9d, 0x0001, 0)
      , (0x01e8, 0x9e, 0x001f, 4)
      , (0x001a, 0x5d, 0x0080, 7)
      ]
    liveEntry p = case [ (o, m, s) | (q, o, m, s) <- liveTable, q == p ] of
      (e : _) -> e
      [] -> (0, 0, 0)
    placeAt b off mask shift bytes code
      | bytes == 2 =
          let old = fromIntegral (B.index b off) .|. (fromIntegral (B.index b (off + 1)) `shiftL` 8) :: Word32
              v = (old .&. complement mask) .|. ((code `shiftL` shift) .&. mask)
           in B.take off b <> B.pack [fromIntegral (v .&. 0xff), fromIntegral ((v `shiftR` 8) .&. 0xff)] <> B.drop (off + 2) b
      | otherwise =
          let old = fromIntegral (B.index b off) :: Word32
              v = (old .&. complement mask) .|. ((code `shiftL` shift) .&. mask)
           in B.take off b <> B.pack [fromIntegral v] <> B.drop (off + 1) b
    placeLive b f code = let (o, m, s) = liveEntry (fProp f) in placeAt b o m s (fBytes f) code
    placePayload b f code = placeAt b (fOff f) (fMask f) (fShift f) (fBytes f) code
    -- 0x137 against table 0x776f34 is the one walked row whose col0 set
    -- excludes zero, so it carries the smallest admitted code (1); 0xf9 = 1
    -- with 0xf6 = 0 is IMAGE SIZE row 1, the pair (0, 0) being row 0, which
    -- the IMAGE SIZE list does not offer; the nine recipe rows carry the
    -- literal codes the extraction case names
    fieldCode f = fromMaybe (case fProp f of 0x137 -> 1; 0xf9 -> 1; _ -> 0) (lookup (fProp f) extractCodes)
    payloadReal = foldl (\b f -> placePayload b f (fieldCode f)) (B.replicate 0x28 0) fields
    liveReal = foldl (\b f -> placeLive b f (fieldCode f)) (B.replicate 0x1c2 0) fields
    -- A fixture recall admits in P, S and M: the stock row check
    -- (FUN_00d52684) admits each code's key in those columns of the
    -- availability matrix 0x0079944c and each menu property's row is one its
    -- list offers. Film code 3 is offered row 3 (code 5 is row 9, which no
    -- FILM SIMULATION list offers); 0x217, 0xf3, 0x214 and 0x1c5 hold the
    -- codes those columns admit, and 0xf9 = 1 with 0xf6 = 0 is IMAGE SIZE
    -- row 1, an offered row. 0x146 = 1 makes ISO 0xb a manual ISO: with the
    -- flag at 0 a P/S/A/M live state is AUTO, whose 0x141 is the code of the
    -- AUTO-maximum step, never 0xb. 0x1c6 = 0x7f is key 0, the one key its
    -- matrix 0x0079b474 admits in S and M, and the per-slot defaults row's byte.
    admittedCodes = [(0xfe, 3), (0x217, 5), (0xf3, 1), (0x214, 6), (0x1c5, 6), (0xf9, 1), (0x146, 1), (0x1c6, 0x7f)]
    admittedCode f = fromMaybe (fieldCode f) (lookup (fProp f) admittedCodes)
    admittedPayload = foldl (\b f -> placePayload b f (admittedCode f)) (B.replicate 0x28 0) fields
    -- 0x149's live mask 0x0f covers 0x146's bit 2, so 0x146 is placed last
    admittedLive = placeLive (foldl (\b f -> placeLive b f (admittedCode f)) (B.replicate 0x1c2 0) fields)
      (fieldOf 0x146) (admittedCode (fieldOf 0x146))
    -- the payload the save's walk emits for the live fixture, and the live
    -- block the stock getter reads it from
    validPayload = payloadReal
    emptyCard = B.replicate cardBytes 0
    cardWith a b = fileSlot a <> fileSlot b <> B.replicate fileStride 0
    model st = Model emptyCard liveFixture (M.fromList st) 1 0x21 B.empty real []
    -- the readback buffers of rbm_seq_load_readback: name at 0, payload at 0x20
    readbackOf name payload = [("casebuf", 0, name), ("casebuf", 0x20, payload)]
    loadReadback cid claim slot card cells claims = Spec cid claim
      "rbm_seq_load_readback" [slot, 0x009a0000, 0x009a0020] ((model cells) { mCard = card })
      (Expect "returned" 0 (claims ++ [("card", 0, card)]))
      ("rbm_seq_load_readback" : "rb_readback" : loadFns) bankStubs
    builders = map (\operation _ -> staleMediaOperation operation)
      [BankRename, BankSaveCurrent, BankReset, BankSelect] ++
      existingBuilders ++ map const v2Builders ++ map const selectionBuilders
    existingBuilders =
      [ \_ -> stageMediaChanged, \_ -> blankLoad, \_ -> validLoad, \_ -> bothLoad, \_ -> hostLoad
      , \_ -> atCRecall, \_ -> unsetRecall, \_ -> genExhaust, \_ -> extractSave
      , \_ -> realRecall, \_ -> realConsumers, \_ -> realE9, \_ -> realSkipped
      , \_ -> aspectZeroRefused, \_ -> sizeRowZeroRefused, \_ -> restoreAfterRefusal
      , \_ -> renameCreatedRecall, \_ -> editCreatedRecall, \_ -> isoEditRows
      , \_ -> realIsoAuto ("s", 0x22, "0x22"), \_ -> realIsoAuto ("m", 0x24, "0x24"), \_ -> realIsoManualAfterAuto
      , \_ -> isoCountSkipped, \_ -> key1c6Skipped
      , \_ -> savedIsoAuto ("s", 0x22, "0x22"), \_ -> savedIsoAuto ("m", 0x24, "0x24")
      , \_ -> rawOnlySizeSkipped, \_ -> rawOnlyRestore, \_ -> isoAuto1c6Reset, \_ -> isoManual1c6Kept ]
      ++ map (\m _ -> modeRefused m) [0, 1, 0x18, 0x20] ++
      [ \_ -> badChecksum, \_ -> badCode, \_ -> badUnbound
      , \_ -> stageOk, \_ -> stageCancel, \_ -> resetCase, \_ -> renameCase, \_ -> renameUnsetCase ]
      ++ map (\row _ -> imageSizeRecall row) offeredSizeRows ++
      [ \_ -> readOnlySave, \_ -> fullCardSave, \_ -> partialWrite, \_ -> powerLoss
      , \_ -> mediaMoveInScan, \_ -> mediaMoveInOpen, \_ -> mediaMoveInWrite, \_ -> closeDeferred
      , \_ -> twoDeferredReadback, \_ -> openTagZero, \_ -> writeCloseOwned, \_ -> writeCloseLate ]
    staleMediaOperation operation =
      let (suffix, target) = case operation of
            BankSaveCurrent -> ("save", "rb_save_current")
            BankReset -> ("reset", "rb_reset")
            BankRename -> ("rename", "rb_name_commit")
            BankSelect -> ("select", "rb_select")
          identityArgument = case operation of
            BankRename -> 0xe1a02004
            _ -> 0xe1a01004
          -- r0: slot 0, or CUSTOM 1's selection code
          firstArgument = case operation of
            BankSelect -> 0xe3a00001
            _ -> 0xe3a00000
          workload = B.concat (map u32le
            [ 0xe92d4010, 0xe59fc024, 0xe12fff3c, 0xe1a04000
            , 0xe59fc01c, 0xe12fff3c, firstArgument, 0xe59f1018
            , identityArgument, 0xe59fc00c, 0xe12fff3c, 0xe8bd8010
            , fromIntegral (addr ks "rb_media_epoch")
            , fromIntegral (addr ks "rb_media_epoch_bump")
            , fromIntegral (addr ks target), 0x009a0000
            ])
       in Spec ("h-entry-media-" ++ suffix)
          "contract: a caller's entry card identity cannot be rebound after a media change; the operation returns MEDIA_CHANGED and preserves both files and all live settings"
          "0x009b0000" []
          ((model (fileA []))
            { mCard = cardWith validA B.empty, mLive = extractLive
            , mCase = name26 "DELTA"
            , mExtra = ("prefix", 0x009b0000, workload) :
                [region | region@(name, _, _) <- real, RegionId name /= RegionId "dispatcher"]
            })
          (Expect "returned" 13 [("card", 0, cardWith validA B.empty), ("live-state", 0, extractLive)])
          ("0x009b0000/48" : target : "rb_media_epoch" : "rb_media_epoch_bump" : loadFns)
          bankStubs
    stageMediaChanged = Spec "e-stage-media-change"
      "contract: a card identity change after opening a staged edit refuses commit with MEDIA_CHANGED; both bank files and the live settings remain unchanged"
      "0x009b0000" []
      ((model (fileA []))
        { mCard = cardWith validA B.empty
        , mLive = extractLive
        , mExtra = ("prefix", 0x009b0000, stageMediaWorkload) :
            [region | region@(name, _, _) <- real, RegionId name /= RegionId "dispatcher"]
        })
      (Expect "returned" 13 [("card", 0, cardWith validA B.empty), ("live-state", 0, extractLive)])
      ("0x009b0000/52" : "rb_stage_load" : "rb_stage_commit" : "rb_media_epoch_bump" : loadFns)
      bankStubs
    stageMediaWorkload = B.concat (map u32le
      [ 0xe92d4010, 0xe3a00000, 0xe59fc024, 0xe12fff3c
      , 0xe3500000, 0x1a000005, 0xe59fc018, 0xe12fff3c
      , 0xe3a00000, 0xe3a01000, 0xe59fc00c, 0xe12fff3c, 0xe8bd8010
      , fromIntegral (addr ks "rb_stage_load")
      , fromIntegral (addr ks "rb_media_epoch_bump")
      , fromIntegral (addr ks "rb_stage_commit")
      ])
    hostLoad = Spec "a-load-host-fixture"
      "contract: public readback returns all seven literal names and halfword-bearing working payloads from the independently Haskell-encoded v2 bank without changing card bytes"
      "rbm_seq_load_all_readback" [0x009a0000, 0x009a0100] ((model (fileA2 [])) { mCard = cardWith hostBank B.empty })
      (Expect "returned" 0
        [ ("casebuf", 0, B.concat [B.take 26 (B.drop (0x68 + 106 * k) hostBank) | k <- [0 .. 6]])
        , ("casebuf", 0x100, B.concat [B.take 0x28 (B.drop (0x68 + 106 * k + 66) hostBank) | k <- [0 .. 6]])
        , ("card", 0, cardWith hostBank B.empty)])
      ("rbm_seq_load_all_readback" : "rb_readback" : loadFns) bankStubs
    extractLive = liveReal
    extractPayload = payloadReal
    extractSave = Spec "b-save-extract-codes"
      "contract: the fixed extraction, recipeOf 1 3 4 4 3 2 3 0xb 5 (DR/highlight/shadow/colour/sharp/NR/WB/ISO/film) with IMAGE SIZE row 1: the save's v2 generation 1 in A carries slot 1 with saved = working = the payload with the literal shifted bytes +0x05=0x40 +0x07=0x80 +0x08=0x60 +0x09=0x80 +0x10=0x40 +0x17=0x60 +0x1e=0x20 +0x20=0x0b +0x21=0x03 +0x24=0x05, and the live state (C record row 0, ordinary row 1, the wireless 0xa8/0xa9 bytes) is byte-identical after the save - the old zero-on-shift extraction would write zeros at the shifted fields and the C-preservation claim would still pass, so the payload claim is the distinguishing assertion"
      "rb_save_current" [1, 0]
      ((model []) { mLive = extractLive, mCard = emptyCard })
      (Expect "returned" 0 [("card", 0, cardWith (encodeFileV2 1 (withRecord 1 (Populated (name26 "") extractPayload extractPayload) emptyBank)) B.empty),
        ("live-state", 0, extractLive)]
        )
      saveFns bankStubs
    blankLoad = Spec "a-load-blank"
      "contract: rb_load on a card with no bank file returns 9 (no-bank) without changing card bytes or live settings"
      "rb_load" [] (model []) (Expect "returned" 9 [("card", 0, emptyCard), ("live-state", 0, liveFixture)])
      loadFns bankStubs
    validASlots = replicate 7 (name26 "ALPHA", validPayload)
    validA = encodeFile 7 0x0001 validASlots
    fileA st = ("rbm_a_present", 1) : ("rbm_a_size", 512) : st
    fileA2 st = ("rbm_a_present", 1) : ("rbm_a_size", 1024) : st
    fileB st = ("rbm_b_present", 1) : ("rbm_b_size", 512) : st
    fileB2 st = ("rbm_b_present", 1) : ("rbm_b_size", 1024) : st
    emptyBank = BankV2 0 0 BaseInvalid []
    -- the v2 generation a write over v1 bank `mask`/`slots` produces with
    -- record `k` replaced
    writtenOver gen mask slots k r = encodeFileV2 gen (withRecord k r (migrated mask slots))
    -- The ordinary payload the getter walk reads from a live block: each walked
    -- field's code at its live offset, placed at its payload offset.
    payloadOfLive live = foldl (\p f -> placePayload p f (liveCode live f)) (B.replicate 0x28 0) fields
    liveCode live f =
      let (o, m, s) = liveEntry (fProp f)
          raw = if fBytes f == 2
            then fromIntegral (B.index live o) .|. (fromIntegral (B.index live (o + 1)) `shiftL` 8)
            else fromIntegral (B.index live o) :: Word32
       in (raw .&. m) `shiftR` s
    captureOf mode = case mode of
      0x21 -> CaptureP
      0x22 -> CaptureS
      0x23 -> CaptureA
      _ -> CaptureM
    -- A selection from NOTHING SELECTED over `bank` (winning in A at `gen`, B
    -- empty or older): the base checkpoint of `live` at gen+1 in B with the
    -- selection unchanged, then the selection `sel` at gen+2 in A.
    checkpointOf gen bank mode live = encodeFileV2 (gen + 1) (bank { bBase = BaseValid (captureOf mode) (payloadOfLive live) })
    selectedOf gen bank mode live sel = encodeFileV2 (gen + 2) (bank { bBase = BaseValid (captureOf mode) (payloadOfLive live), bSelection = sel })
    selectedCard gen bank mode live sel = cardWith (selectedOf gen bank mode live sel) (checkpointOf gen bank mode live)
    checkpointedCard file gen bank mode live = cardWith file (checkpointOf gen bank mode live)
    validLoad = Spec "a-load-valid-a"
      "contract: one valid RECIPEA.BIN loads successfully and public readback returns slot 0's ALPHA name and payload without changing either file"
      "rbm_seq_load_readback" [0, 0x009a0000, 0x009a0020] ((model (fileA [])) { mCard = cardWith validA B.empty })
      (Expect "returned" 0 [("casebuf", 0, name26 "ALPHA"), ("casebuf", 0x20, validPayload), ("card", 0, cardWith validA B.empty)])
      ("rbm_seq_load_readback" : "rb_readback" : loadFns) bankStubs
    validB = encodeFile 9 0x0002 (replicate 7 (name26 "BETA", validPayload))
    bothLoad = Spec "a-load-b-newer"
      "contract: the higher generation wins; public readback of slot 1 returns BETA from B's generation 9 instead of ALPHA from A's generation 7"
      "rbm_seq_load_readback" [1, 0x009a0000, 0x009a0020] ((model (fileA [("rbm_b_present", 1), ("rbm_b_size", 512)])) { mCard = cardWith validA validB })
      (Expect "returned" 0 [("casebuf", 0, name26 "BETA"), ("casebuf", 0x20, validPayload), ("card", 0, cardWith validA validB)])
      ("rbm_seq_load_readback" : "rb_readback" : loadFns) bankStubs
    atCRecall = Spec "c-select-at-c"
      "contract: selection at mode-class 3 (dial at C) returns 8 and leaves the card untouched"
      "rb_select" [3] ((model []) { mMode = 3 })
      (Expect "returned" 8 [("card", 0, emptyCard)])
      ["rb_select"] []
    unsetRecall = Spec "c-select-unset"
      "contract: selecting an unset slot returns 6 (unset) with no record change and no base checkpoint; the scan runs but the mask bit is clear"
      "rb_select" [2] ((model (fileA [])) { mCard = cardWith validA B.empty })
      (Expect "returned" 6 [("card", 0, cardWith validA B.empty)])
      ("rb_select" : loadFns) bankStubs
    exhaust = encodeFile 0xffffffff 0x0001 (replicate 7 (name26 "ALPHA", validPayload))
    -- the real-byte recall (the storage lane's real-stock contract): the
    -- kernel walks every one of the 83 fields through the stock getter and
    -- setter and re-reads them through the stock reader, so status 0 is the
    -- aggregate verdict that every field, the halfwords included, holds its
    -- stored code. The halfword pair rides the setter's ordinary branch to
    -- the live block head at 0x0096cc74/0x0096cc76 (the record pair at
    -- 0x0096cdcc/ce is the slot != 1 branch this path never takes).
    realRecallPayload = u16le 0x1234 <> u16le 0x5678 <> B.drop 4 admittedPayload
    realRecallSlots = (name26 "ALPHA", realRecallPayload) : replicate 6 (name26 "ALPHA", payloadReal)
    realRecallFile = encodeFile 7 0x0001 realRecallSlots
    -- selections from NOTHING SELECTED over a v1 bank at generation 7 whose
    -- slot 0 is the only populated slot
    oneSlotBank p = migrated 0x0001 ((name26 "ALPHA", p) : replicate 6 (name26 "ALPHA", payloadReal))
    selectedFrom p mode live = selectedCard 7 (oneSlotBank p) mode live 1
    checkpointedFrom p mode live = checkpointedCard (encodeFile 7 0x0001 ((name26 "ALPHA", p) : replicate 6 (name26 "ALPHA", payloadReal))) 7 (oneSlotBank p) mode live
    realStockFns =
      ["0x0061c3ac/76" | ("chain", a, bytes) <- real, a <= 0x61c3ac, a + fromIntegral (B.length bytes) >= 0x61c3f8]
      ++
      [ "0x00d4d604/20", "0x00d4fc74/10768", "0x0061c91c/16"
      , "0x0061c93c/16", "0x0061d8dc/80", "0x0061d9ec/80"
      , "0x0061e110/25832", "0x00624620/44", "0x00624694/49968", "0x006309c4/112"
      , "0x0063396c/32", "0x00633b6c/32", "0x00633b8c/32"
      , "0x00633cac/32", "0x00633d0c/32", "0x0063458c/32", "0x0063460c/32"
      , "0x006347b0/32", "0x006347d0/32", "0x006439e0/40", "0x00643d94/48"
      , "0x00643dc4/40", "0x00643e9c/40", "0x00643e58/68", "0x00643fb8/76", "0x00644848/476"
      , "0x0061cd14/264", "0x0061d154/80", "0x0061d534/16", "0x0061d544/16", "0x00631d58/388", "0x00631b74/484", "0x00631edc/292", "0x00632860/192", "0x00632920/20", "0x00632934/28", "0x00632950/116", "0x0063386c/32", "0x006338ec/32", "0x0063390c/32", "0x0063392c/32", "0x0063394c/32", "0x0063398c/32", "0x006339ac/32", "0x00633a0c/32", "0x00633a2c/32", "0x00633a4c/32", "0x00633a8c/32", "0x00633aac/32", "0x00633acc/32", "0x00633aec/32", "0x00633b0c/32", "0x00633b4c/32", "0x00633c0c/32", "0x00633c8c/32", "0x00633ccc/32", "0x00633cec/32", "0x00633e0c/32", "0x00633e8c/32", "0x00633eac/32", "0x006340cc/32", "0x0063412c/32", "0x0063432c/32", "0x0063462c/36", "0x00634830/32", "0x00634850/32", "0x00634890/32", "0x006348b0/32", "0x006348d0/32", "0x00635aac/36", "0x00635ad0/36", "0x006394d0/24", "0x0063d3bc/220", "0x0063d5fc/24", "0x0063d614/116", "0x0063d688/1180", "0x0063db24/80", "0x0063ead0/144", "0x006425ec/80", "0x00643c4c/152", "0x00643e30/40", "0x00643ec4/68", "0x00643f08/40", "0x00644004/80", "0x00644054/44", "0x006440b4/212", "0x00644188/56", "0x006441c0/244", "0x006442b4/56", "0x006442ec/116", "0x00644360/244", "0x006464cc/40", "0x006464f4/44", "0x00646574/48", "0x00646af0/116", "0x00646c04/152" ]
    realStubs = ["task", "vol", "card", "tone-notify", "pipeline-notify", "display-notify", "mode-state-a", "mode-state-b", "mode-state-c", "journal-stub", "out-window-stub"]
    realRecall = Spec "c-select-real-bytes"
      "contract: selecting CUSTOM 1 from NOTHING SELECTED through the real stock getter, setter, writer, reader and commit dispatcher: the status is 0 (the kernel's own re-read of all 83 walked fields agrees), the live halfwords at 0x0096cc74/0x0096cc76 carry the payload's +0x00/+0x02 codes through the setter's ordinary branch, the film cell 0x009568b8 carries the selected film code's col2 from the table at 0x0077ac30, and the card holds the base checkpoint of the live settings captured in P at generation 8 in B and the selection at generation 9 in A"
      "rb_select" [1] ((model (fileA [])) { mCard = cardWith realRecallFile B.empty, mLive = admittedLive })
      (Expect "returned" 0
        [ ("live-state", 0x10, B.take 4 realRecallPayload)
        , cellFor 0xfe 0x80
        , ("card", 0, selectedFrom realRecallPayload 0x21 admittedLive)
        ])
      ("rb_select" : loadFns ++ realStockFns) realStubs
    -- RECIPE-STATE-APPLY-01 closure: the grouped consumer cells, the e9 row
    -- and the refusal fixture on the passing recall's real-byte setup
    payloadCode f = fromIntegral ((fromIntegral (B.index realRecallPayload (fOff f)) .&. fMask f) `shiftR` fShift f)
    col2For p = case M.lookup p tabs of
      Just rows -> let c = payloadCode (fromMaybe (error "row missing") (find ((== p) . fProp) fields))
                    in fromMaybe 0 (listToMaybe [ c2 | (c0 : _ : c2 : _) <- rows, c0 == c ])
      Nothing -> 0
    cellFor p off = ("cells", off, u32le (col2For p))
    realConsumers = Spec "c-select-real-consumers"
      "contract: every walked row whose arm carries a consumer runs with the real code: the eight cell writers observed effective are WB mode 0x00956864, sharpness 0x009568a0, film 0x009568b8, DR 0x009568bc, colour 0x009568c0, noise reduction 0x0095699c, highlight 0x00956b40 and shadow 0x00956b44, each holding the col2 of the committed row whose col0 is the payload's stored code; the selection still reports 0"
      "rb_select" [1] ((model (fileA [])) { mCard = cardWith realRecallFile B.empty, mLive = admittedLive })
      (Expect "returned" 0
        [ cellFor 0xed 0x2c, cellFor 0x10f 0x68, cellFor 0xfe 0x80, cellFor 0x104 0x84
        , cellFor 0x10e 0x88, cellFor 0xef 0x164, cellFor 0x110 0x308, cellFor 0x111 0x30c
        , ("card", 0, selectedFrom realRecallPayload 0x21 admittedLive) ])
      ("rb_select" : loadFns ++ realStockFns) realStubs
    e9Payload = putByte 0x0a (.|. 0xe0) realRecallPayload
    e9RecallFile = encodeFile 7 0x0001 ((name26 "ALPHA", e9Payload) : replicate 6 (name26 "ALPHA", payloadReal))
    e9Reread = encodeFileV2 8 (withRecord 0 (Populated (name26 "") e9Payload e9Payload) emptyBank)
    realE9 = Spec "c-recall-real-e9-preserved"
      "contract: the remembered payload's 0xe9 bits survive staged commit in the durable new v2 generation, in both the saved and the working payload"
      "rbm_seq_stage_roundtrip" [0, 0x009a0000]
      ((model (fileA [])) { mCard = cardWith e9RecallFile B.empty })
      (Expect "returned" 0
        [ ("card", 0, cardWith e9RecallFile e9Reread) ])
      ("rbm_seq_stage_roundtrip" : "rb_stage_load" : "rb_stage_commit" : "rb_stage_image" : "rb_write_back" : loadFns ++ realStockFns) realStubs
    refusedPayload = putByte 0x23 (\v -> (v .&. 0xf0) .|. 0x0b) realRecallPayload
    refusedRecallFile = encodeFile 7 0x0001 ((name26 "ALPHA", refusedPayload) : replicate 6 (name26 "ALPHA", payloadReal))
    liveOffset p = let (o, _, _) = liveEntry p in o
    liveAt live p = ("live-state", liveOffset p, B.singleton (B.index live (liveOffset p)))
    realSkipped = Spec "c-select-real-skipped"
      "contract: in M, 0x1b1 stored as 0xb (row 9 of table 0x77a54c, whose column-3 key is the 0x7ffffffe sentinel, so the availability matrix that lists 0x1b1 admits it in no mode) is skipped: the selection applies every other field and returns 0, live 0x1b1 keeps its base code, and rb_select_trace records count 1, status 0, selection 1, mode 0x24, no refused field, no restore, one skipped field and 0x1b1 first"
      "rb_select" [1] (mModel { mCard = cardWith refusedRecallFile B.empty })
      (Expect "returned" 0
        [ liveAt admittedLive 0x1b1
        , traceClaim [1, 0, 1, 0, 0, 0, 0x24, 0, 0, 1, 0x1b1]
        , ("card", 0, selectedFrom refusedPayload 0x24 admittedLive) ])
      ("rb_select" : loadFns ++ realStockFns) realStubs
    -- the 48-byte rb_select_trace after its magic: count, status, selection,
    -- prop, stored, read-back, mode, refused fields, unrestored fields,
    -- skipped fields, first skipped property
    traceClaim ws = ("kdata", traceOffset, B.concat (map u32le (0x52424352 : ws)))
    -- IMAGE SIZE: row r of the IMAGE SIZE list stores (0xf6, 0xf9) =
    -- 0x007a00cc[r] = (r / 4, r % 4), and the list 0x00784014 offers rows 1, 2,
    -- 3, 5, 6, 7, 9, 0xa and 0xb. The arm sets the size cell 0x00956888 and
    -- the aspect cell 0x00956898 from the row and writes both fields from them.
    sizePayload size aspect = placePayload (placePayload realRecallPayload (fieldOf 0xf6) size) (fieldOf 0xf9) aspect
    sizeFile p = encodeFile 7 0x0001 ((name26 "ALPHA", p) : replicate 6 (name26 "ALPHA", payloadReal))
    mModel = (model (fileA [])) { mLive = admittedLive, mShootingMode = 0x24 }
    sizeRefusal cid what size aspect = Spec cid
      ("contract: in M (mode cell 0x24) a v1 bank whose populated slot has " ++ what ++ " cannot migrate: the pair (0xf6, 0xf9) = (" ++ show size ++ ", " ++ show aspect ++ ") is no row the IMAGE SIZE list offers, so the load refuses the bank before admission; the kernel answers 15 (unsafe bank), every live byte and both files are unchanged, and rb_select_trace records count 1, status 0x0f, selection 1, property 0, mode 0x24 and no refused, unrestored or skipped field")
      "rb_select" [1] (mModel { mCard = cardWith (sizeFile (sizePayload size aspect)) B.empty })
      (Expect "returned" 15
        [ ("live-state", 0, admittedLive)
        , traceClaim [1, 0x0f, 1, 0, 0, 0, 0x24, 0, 0, 0, 0]
        , ("card", 0, cardWith (sizeFile (sizePayload size aspect)) B.empty) ])
      ("rb_select" : loadFns ++ realStockFns) realStubs
    aspectZeroRefused = sizeRefusal "c-select-aspect-zero-refused" "0xf6 = 1 (size M) and 0xf9 = 0" 1 0
    sizeRowZeroRefused = sizeRefusal "c-select-size-row-0-refused" "IMAGE SIZE row 0, the pair (0, 0) of the stock per-slot defaults row 0x007cb804" 0 0
    offeredSizeRows = [1, 2, 3, 5, 6, 7, 9, 0xa, 0xb] :: [Word32]
    imageSizeRecall row = Spec ("c-select-image-size-row-" ++ showHex row "")
      ("contract: in M a slot storing IMAGE SIZE row " ++ show row ++ "'s pair (0xf6, 0xf9) = (" ++ show size ++ ", " ++ show aspect ++ ") is selected through one commit of that row and returns 0: live 0xf6 reads " ++ show size ++ " and 0xf9 reads " ++ show aspect ++ ", and the arm's size and aspect cells 0x00956888/0x00956898 hold the same pair")
      "rb_select" [1] (mModel { mCard = cardWith file B.empty })
      (Expect "returned" 0
        [ ("live-state", 0x6f, B.singleton (B.index after 0x6f))
        , ("live-state", 0x7d, B.singleton (B.index after 0x7d))
        , ("cells", 0x50, u32le size)
        , ("cells", 0x60, u32le aspect)
        , ("card", 0, selectedFrom (sizePayload size aspect) 0x24 admittedLive) ])
      ("rb_select" : loadFns ++ realStockFns) realStubs
      where
        size = row `div` 4
        aspect = row `mod` 4
        file = sizeFile (sizePayload size aspect)
        after = placeLive (placeLive admittedLive (fieldOf 0xf6) size) (fieldOf 0xf9) aspect
    -- A fault model in place of FUN_00640e78's register-only stub, which the
    -- film arm calls last: it writes the live film code back through the stock
    -- write entry, so a selected film code other than the live one never holds
    -- and the assert pass refuses after the apply pass has written.
    filmPinAt = 0x00640e78 :: Integer
    liveFilm = admittedCode (fieldOf 0xfe)
    filmPin = B.concat (map u32le
      [ 0xe30000fe, 0xe3a01000 .|. liveFilm, 0xe3a02000 -- (0xfe, live film, 0)
      , 0xea000000 .|. (fromIntegral ((0x0061c91c - (filmPinAt + 12 + 8)) `div` 4) .&. 0x00ffffff) ])
    withFilmPin regions =
      [ (n, a, if n == "chain" then pokeAt (fromIntegral (filmPinAt - a)) filmPin b else b) | (n, a, b) <- regions ]
    restorePayload = placePayload (placePayload realRecallPayload (fieldOf 0xfe) 0) (fieldOf 0x104) 2
    restoreFile = encodeFile 7 0x0001 ((name26 "ALPHA", restorePayload) : replicate 6 (name26 "ALPHA", payloadReal))
    restoreAfterRefusal = Spec "c-select-restore-after-refusal"
      ("contract: with FUN_00640e78 modeled to write the live film code " ++ show liveFilm ++ " back, a slot with film 0 (PROVIA), DR code 2 and the halfwords 0x1234/0x5678 selected in M over the live fixture passes admission, has its base checkpointed at generation 8 in B and is applied, and the assert pass refuses 0xfe (stored 0, read-back " ++ show liveFilm ++ "); the kernel re-applies the codes it read before the apply pass and answers 14 with every live byte equal to the fixture and no selection written, and rb_select_trace records count 1, status 0x0e, selection 1, property 0xfe, stored 0, read-back " ++ show liveFilm ++ ", mode 0x24, one refused field and no field left unrestored")
      "rb_select" [1] (mModel { mCard = cardWith restoreFile B.empty, mExtra = withFilmPin (mExtra mModel) })
      (Expect "returned" 14
        [ ("live-state", 0, admittedLive)
        , traceClaim [1, 0x0e, 1, 0xfe, 0, liveFilm, 0x24, 1, 0, 0, 0]
        , ("card", 0, checkpointedFrom restorePayload 0x24 admittedLive) ])
      (("0x00640e78/" ++ show (B.length filmPin)) : "rb_select" : loadFns ++ realStockFns) (filter (/= "mode-state-c") realStubs)
    -- slots an unset slot's first Rename or Edit OK creates hold the live
    -- settings as saved and working, so in M they are selectable; the
    -- creation's generation 8 in B wins, so the base checkpoint lands at 9 in A
    -- and the selection at 10 in B
    seededBank name p = withRecord 1 (Populated name p p) (migrated 0x0001 validASlots)
    seededFile name p = encodeFileV2 8 (seededBank name p)
    seededSelected name p live = cardWith (checkpointOf 8 (seededBank name p) 0x24 live) (selectedOf 8 (seededBank name p) 0x24 live 2)
    renameCreatedRecall = Spec "c-select-rename-created-m"
      "contract: in M, Rename of unset slot 1 to DELTA over the v1 bank writes v2 generation 8 with slot 1 = DELTA whose saved and working payloads are the live settings the stock getter reads (not the per-slot defaults row 0x007cb804), and selecting CUSTOM 2 returns 0 with every live byte unchanged"
      "rbm_seq_rename_select" [1, 0x009a0000]
      (mModel { mCard = cardWith validA B.empty, mCase = name26 "DELTA" })
      (Expect "returned" 0
        [ ("card", 0, seededSelected (name26 "DELTA") admittedPayload admittedLive)
        , ("live-state", 0, admittedLive) ])
      ("rbm_seq_rename_select" : "rb_name_commit" : "rb_select" : "rb_stage_image" : "rb_write_back" : loadFns ++ realStockFns) realStubs
    editCreatedRecall = Spec "c-select-edit-created-m"
      "contract: in M, the first Edit OK on unset slot 1 (DR to code 2) writes v2 generation 8 with slot 1's saved and working payloads = the live settings with DR 2, and selecting CUSTOM 2 returns 0 and applies it: live DR reads 2 and every other live byte is unchanged"
      "rbm_seq_stage_field_select" [1, 0x104, 2]
      (mModel { mCard = cardWith validA B.empty })
      (Expect "returned" 0
        [ ("card", 0, seededSelected (name26 "") (placePayload admittedPayload (fieldOf 0x104) 2) admittedLive)
        , ("live-state", 0, placeLive admittedLive (fieldOf 0x104) 2) ])
      ("rbm_seq_stage_field_select" : "rb_select" : "rb_stage_load" : "rb_stage_field" : "rb_stage_commit" : "rb_stage_image" : "rb_write_back" : loadFns ++ realStockFns) realStubs
    -- the stock ISO list 0x00783de0's rows; in M (column 4) row 0's key 0 has
    -- cell 0 and every other row's key cell 1, and row 0x1b (AUTO) skips the
    -- matrix and is offered while FUN_00631edc(M, 0x14f) >= 2
    isoRows = [0x1a, 0x19, 0x17, 0x16, 0x15, 0x14, 0x13, 0x12, 0x11, 0x10, 0xf, 0xe, 0xd, 0xc, 0xb, 0xa, 9, 8, 7, 0, 0x1b] :: [Word32]
    isoRowState row = if row == 0 then 3 else 0 :: Word32
    isoEditRows = Spec "e-iso-edit-list-rows"
      "contract: in M, every row of the ISO edit list (hosting property 0x26e) takes the state the stock render probe FUN_00d52684 gives the same row of the stock ISO list (property 0x141): row 0 hidden (3), the other twenty rows, AUTO row 0x1b among them, offered (0), so the list shows one AUTO row"
      "rbm_seq_field_rows" [0x26e, 0x141, 0x00783528, 0x009a0000]
      mModel
      (Expect "returned" (fromIntegral (length isoRows))
        [ ("casebuf", 0, B.concat [u32le (isoRowState r) <> u32le (isoRowState r) | r <- isoRows]) ])
      ("rbm_seq_field_rows" : "rb_probe_state" : "0x00d52684/25140" : "0x0063d060/240" : "0x00631edc/292" : "0x0061d554/16" : "0x0061d018/32" : loadFns ++ realStockFns) realStubs
    -- ISO AUTO: property 0x141's row 0x1b, whose step word is the 0x7ffffffe
    -- sentinel. Stock leaves the mode's auto flag from table 0x007a0018 at 0
    -- (0x146 in modes 0x21-0x24), the ISO step 0x009568b4 at the AUTO-maximum
    -- step (0x14f's step for the code in 0x152) and 0x141 at the code whose
    -- step that is; 0x141 never holds 0x1b.
    fieldOf p = fromMaybe (error ("walked field missing: " ++ show p)) (find ((== p) . fProp) fields)
    codeIn payload p = let f = fieldOf p in (fromIntegral (B.index payload (fOff f)) .&. fMask f) `shiftR` fShift f
    rowsOf p = fromMaybe (error ("table missing: " ++ show p)) (M.lookup p tabs)
    stepOf p c = fromMaybe (error "code missing") (listToMaybe [ c2 | (c0 : _ : c2 : _) <- rowsOf p, c0 == c ])
    codeOfStep p s = fromMaybe (error "step missing") (listToMaybe [ c0 | (c0 : _ : c2 : _) <- rowsOf p, c2 == s ])
    -- live byte 0x9c carries 0x146 (bit 2), 0x149 (bit 3) and 0x148 (bit 7);
    -- the live table's 0x149 mask 0x0f covers bit 2, so 0x146 is placed last
    flagByte payload autoFlag =
      B.index (foldl (\b (p, c) -> placeLive b (fieldOf p) c) (B.replicate 0x1c2 0)
        [(0x148, codeIn payload 0x148), (0x149, codeIn payload 0x149), (0x146, autoFlag)]) 0x9c
    cellsIsoStep = 0x7c
    -- FUN_00641438's write in modes 0x21-0x25 (its lines 20-22): for r0 in
    -- 1..2 it sets property 0x161 to r0 through 0x0061c91c, and the ISO arms
    -- call it as (2, 0). The model replaces the routine's entry in the chain
    -- image; the rest of it is not executed.
    pipelineModelAt = 0x00641438 :: Integer
    pipelineModel = B.concat (map u32le
      [ 0xe306c838, 0xe340c095, 0xe59cc000 -- r12 = mode cell 0x00956838
      , 0xe24cc021, 0xe35c0005, 0x212fff1e -- mode - 0x21 >= 5: return
      , 0xe240c001, 0xe35c0002, 0x212fff1e -- r0 - 1 >= 2: return
      , 0xe1a01000, 0xe3000161, 0xe3a02000 -- (0x161, r0, 0)
      , 0xea000000 .|. (fromIntegral ((0x0061c91c - (pipelineModelAt + 48 + 8)) `div` 4) .&. 0x00ffffff) ])
    withPipelineModel regions =
      [ (n, a, if n == "chain" then pokeAt (fromIntegral (pipelineModelAt - a)) pipelineModel b else b) | (n, a, b) <- regions ]
    pokeAt o v b = B.take o b <> v <> B.drop (o + B.length v) b
    isoFns = ("0x00641438/" ++ show (B.length pipelineModel)) : loadFns ++ realStockFns
    isoStubs = filter (/= "pipeline-notify") realStubs
    -- a slot built by the bank editor before this fix, or snapshotted in manual
    -- ISO and edited to AUTO: 0x141 = 0x1b with the payload's 0x146 bit at 1,
    -- and 0x161 = 0, the defaults row's value, which the ISO arm's
    -- FUN_00641438(2, 0) overwrites. The AUTO maxima differ: 0x14f (mode 0's)
    -- code 0 and 0x152 (P/S/A/M's) code 2; the 0x14f commit writes 0x152.
    isoAutoPayload =
      putByte 0x0c (.&. 0x1f) (putByte 0x0b (\v -> (v .&. 0x1f) .|. 0x40)
        (putByte 0x25 (.&. 0x3f) (putByte 0x1f (.|. 0x80) (putByte 0x20 (const 0x1b) realRecallPayload))))
    isoAutoFile = encodeFile 7 0x0001 ((name26 "ALPHA", isoAutoPayload) : replicate 6 (name26 "ALPHA", payloadReal))
    -- live manual ISO 0x0c with the auto flag at 1 over the admitted fixture
    isoManualLive = placeLive admittedLive (fieldOf 0x141) 0x0c
    isoAutoStep = stepOf 0x14f (codeIn isoAutoPayload 0x152)
    isoModel m = m { mExtra = withPipelineModel (mExtra m) }
    liveByte p code off = B.singleton (B.index (placeLive admittedLive (fieldOf p) code) off)
    realIsoAuto (modeName, mode, modeHex) = Spec ("c-select-iso-auto-" ++ modeName)
      ("contract: in mode " ++ map toUpper modeName ++ " (mode cell " ++ modeHex ++ ", auto flag 0x146 in table 0x007a0018, AUTO maximum 0x152 in table 0x007a09fc) a slot carrying ISO AUTO (0x141 = 0x1b) with its payload 0x146 bit at 1, 0x161 = 0, 0x152 code 2 and 0x14f code 0 is selected into the stock AUTO state and returns 0: 0x146 reads 0, 0x152 holds its stored code 2, the ISO step 0x009568b4 is the step table 0x14f gives code 2, 0x141 is the code whose step that is, never 0x1b, and 0x161 holds 0 after the ISO arm's FUN_00641438(2, 0) wrote 2 (modeled); rb_select_trace keeps its initial magic and zero count")
      "rb_select" [1] (isoModel (model (fileA [])) { mCard = cardWith isoAutoFile B.empty, mLive = isoManualLive, mShootingMode = mode })
      (Expect "returned" 0
        [ ("live-state", 0x52, B.singleton (fromIntegral (codeOfStep 0x141 isoAutoStep)))
        , ("live-state", 0x9c, B.singleton (flagByte isoAutoPayload 0))
        , ("live-state", 0x6e, liveByte 0x161 0 0x6e)
        , ("live-state", 0x47, liveByte 0x152 (codeIn isoAutoPayload 0x152) 0x47)
        , ("cells", cellsIsoStep, u32le isoAutoStep)
        , traceClaim (replicate 11 0)
        , ("card", 0, selectedFrom isoAutoPayload mode isoManualLive) ])
      ("rb_select" : isoFns) isoStubs
    -- selection runs in P, S, A and M (mode cells 0x21-0x24) only: AUTO (0),
    -- SR+ (1), SP (0x18), Adv. (0x20) and every other mode cell refuse it
    modeRefused m = Spec ("c-select-mode-refused-" ++ showHex m "")
      ("contract: in mode cell " ++ hex (toInteger m) ++ " selection of an admitted slot is refused before the bank is read and before any write: the kernel answers 14, every live byte is unchanged, and rb_select_trace records count 1, status 0x0e, selection 1, property 0, mode " ++ hex (toInteger m) ++ ", no refused, unrestored or skipped field")
      "rb_select" [1] (mModel { mCard = cardWith realRecallFile B.empty, mShootingMode = m })
      (Expect "returned" 14
        [ ("live-state", 0, admittedLive)
        , traceClaim [1, 0x0e, 1, 0, 0, 0, m, 0, 0, 0, 0]
        , ("card", 0, cardWith realRecallFile B.empty) ])
      ("rb_select" : loadFns ++ realStockFns) realStubs
    -- FUN_00d52684's rule for manual ISO: with fewer than 2 available keys in
    -- the mode's column of the 0x141 matrix 0x00797c94 every ISO row but AUTO
    -- is hidden. Every P/S/A/M column has at least 2, so the fixture clears
    -- the M cells (column 4) of every row but one.
    matrixRowBytes = 0x5c :: Int
    clearMCells base keep rows regions =
      [ (n, a, if n == "menutab" then foldl (\b r -> pokeAt (fromIntegral (base - a) + r * matrixRowBytes + 4 + 4 * 4) (u32le 0) b) bytes [r | r <- [0 .. rows - 1], r /= keep] else bytes)
      | (n, a, bytes) <- regions ]
    -- ISO 0xc is key 0xe, row 18 of the 0x141 matrix
    isoCountFile = sizeFile (placePayload realRecallPayload (fieldOf 0x141) 0xc)
    isoCountSkipped = Spec "c-select-iso-count-skipped"
      "contract: in M with only the 0x141 matrix row of key 0xe (ISO 0xc) available in column M, FUN_00631edc(M, 0x141) is 1, so the stock list hides every row but AUTO; a slot with ISO 0xc, whose own cell is 1, selected over live ISO 0xb returns 0 with ISO and its maximum 0x152 skipped: live 0x141 stays 0xb, and rb_select_trace records count 1, status 0, selection 1, mode 0x24, no refused field, no restore, two skipped fields and 0x141 first"
      "rb_select" [1] (mModel { mCard = cardWith isoCountFile B.empty, mExtra = clearMCells 0x00797c94 18 28 (mExtra mModel) })
      (Expect "returned" 0
        [ liveAt admittedLive 0x141
        , traceClaim [1, 0, 1, 0, 0, 0, 0x24, 0, 0, 2, 0x141]
        , ("card", 0, selectedFrom (placePayload realRecallPayload (fieldOf 0x141) 0xc) 0x24 admittedLive) ])
      ("rb_select" : loadFns ++ realStockFns) realStubs
    -- 0x1c6 has no conversion table; its range matrix 0x0079b474 admits key 0
    -- in every column and keys -12..12 only in modes 0x1f, 0x20 and 0x21
    key1c6File = sizeFile (placePayload realRecallPayload (fieldOf 0x1c6) 0x80)
    key1c6Skipped = Spec "c-select-1c6-skipped"
      "contract: in M a slot storing 0x1c6 = 0x80 (key 1, stored as key + 0x7f by 0x006464cc) over live 0x7f is selected and returns 0 with 0x1c6 skipped, because FUN_00631b74(M, 0x1c6, 1) is 0: live 0x1c6 stays 0x7f, and rb_select_trace records count 1, status 0, selection 1, mode 0x24, no refused field, no restore, one skipped field and 0x1c6 first"
      "rb_select" [1] (mModel { mCard = cardWith key1c6File B.empty })
      (Expect "returned" 0
        [ liveAt admittedLive 0x1c6
        , traceClaim [1, 0, 1, 0, 0, 0, 0x24, 0, 0, 1, 0x1c6]
        , ("card", 0, selectedFrom (placePayload realRecallPayload (fieldOf 0x1c6) 0x80) 0x24 admittedLive) ])
      ("rb_select" : loadFns ++ realStockFns) realStubs
    -- a consistent AUTO slot (0x141 = 0x1b, 0x146 = 0) edited to manual ISO 0xc
    -- in the bank editor, then selected in S over a live AUTO state
    isoAutoSlot = putByte 0x1f (.&. 0x7f) isoAutoPayload
    isoAutoSlotFile = encodeFile 7 0x0001 ((name26 "ALPHA", isoAutoSlot) : replicate 6 (name26 "ALPHA", payloadReal))
    -- ISO 0xc, row 0xc of the ISO list 0x00783de0, which admits its key 0xe in S
    isoManualCode = 0x0c :: Word32
    isoManualPayload = putByte 0x1f (.|. 0x80) (putByte 0x20 (const (fromIntegral isoManualCode)) isoAutoSlot)
    isoManualBank = withRecord 0 (Populated (name26 "ALPHA") isoAutoSlot isoManualPayload) (migrated 0x0001 [(name26 "ALPHA", isoAutoSlot)])
    -- live ISO AUTO: the auto flag at 0 and 0x141 at the code of the AUTO-maximum step
    isoAutoLive = placeLive (placeLive admittedLive (fieldOf 0x141) (codeOfStep 0x141 isoAutoStep)) (fieldOf 0x146) 0
    realIsoManualAfterAuto = Spec "c-select-iso-manual-after-auto"
      "contract: editing an AUTO slot's ISO to code 0xc stages 0x146 = 1 with it (the new v2 generation's working payload carries 0x141 = 0xc and the +0x1f bit 7 set, while under DISABLE its saved payload stays the AUTO slot), and selecting that slot, which applies the working payload, in S over a live AUTO state ends in manual ISO: 0x146 reads 1, 0x141 reads 0xc, the ISO step 0x009568b4 is row 0xc's step, and the selection returns 0"
      "rbm_seq_stage_field_select" [0, 0x141, isoManualCode]
      (isoModel (model (fileA [])) { mCard = cardWith isoAutoSlotFile B.empty, mLive = isoAutoLive, mShootingMode = 0x22 })
      (Expect "returned" 0
        [ ("live-state", 0x52, B.singleton (fromIntegral isoManualCode))
        , ("live-state", 0x9c, B.singleton (flagByte isoManualPayload 1))
        , ("cells", cellsIsoStep, u32le (stepOf 0x141 isoManualCode))
        , ("card", 0, cardWith (checkpointOf 8 isoManualBank 0x22 isoAutoLive) (selectedOf 8 isoManualBank 0x22 isoAutoLive 1)) ])
      ("rbm_seq_stage_field_select" : "rb_select" : "rb_stage_load" : "rb_stage_field" : "rb_stage_commit" : "rb_stage_image" : "rb_write_back" : isoFns) isoStubs
    -- the slot Save Current writes in AUTO: the getter's 0x141 (FUN_0061e110
    -- case 0x141) is the code of the AUTO-maximum step and the payload's 0x146
    -- bit is 0
    savedAutoCode = codeOfStep 0x141 isoAutoStep
    savedAutoPayload = putByte 0x20 (const (fromIntegral savedAutoCode)) (putByte 0x1f (.&. 0x7f) isoAutoPayload)
    savedAutoFile = encodeFile 7 0x0001 ((name26 "ALPHA", savedAutoPayload) : replicate 6 (name26 "ALPHA", payloadReal))
    savedIsoAuto (modeName, mode, modeHex) = Spec ("c-select-iso-auto-saved-" ++ modeName)
      ("contract: in mode " ++ map toUpper modeName ++ " (mode cell " ++ modeHex ++ ") a slot Save Current wrote in ISO AUTO (0x141 = " ++ hex (toInteger savedAutoCode) ++ ", the code of the AUTO-maximum step, with the payload's 0x146 bit at 0 and 0x152 code 2) selected over live manual ISO 0xc commits ISO row 0x1b and returns 0: 0x146 reads 0, 0x152 holds 2, the ISO step 0x009568b4 is the step table 0x14f gives code 2, 0x141 is the code whose step that is, and rb_select_trace keeps its initial magic and zero count")
      "rb_select" [1] (isoModel (model (fileA [])) { mCard = cardWith savedAutoFile B.empty, mLive = isoManualLive, mShootingMode = mode })
      (Expect "returned" 0
        [ ("live-state", 0x52, B.singleton (fromIntegral savedAutoCode))
        , ("live-state", 0x9c, B.singleton (flagByte savedAutoPayload 0))
        , ("live-state", 0x47, liveByte 0x152 (codeIn savedAutoPayload 0x152) 0x47)
        , ("cells", cellsIsoStep, u32le isoAutoStep)
        , traceClaim (replicate 11 0)
        , ("card", 0, selectedFrom savedAutoPayload mode isoManualLive) ])
      ("rb_select" : isoFns) isoStubs
    -- RAW-only IMAGE QUALITY: row 4 of 0xfa (code 4) with 0x1d code 1. Its
    -- arm sets the RAW cell 0x0095689c to 1 and the size and aspect cells
    -- 0x00956888/0x00956898 from 0x00951550/0x00951558, which the fixture
    -- holds at 2 and 3, a pair no slot's size row in these cases produces.
    rawStockSize = 2 :: Word32
    rawStockAspect = 3 :: Word32
    withRawCells regions =
      [ (n, a, if n == "media-state" then pokeAt (0x00951550 - 0x00910140) (u32le rawStockSize) (pokeAt (0x00951558 - 0x00910140) (u32le rawStockAspect) b) else b)
      | (n, a, b) <- regions ]
    rawOnly place b = place (place b (fieldOf 0xfa) 4) (fieldOf 0x1d) 1
    rawLive = rawOnly placeLive admittedLive
    rawPayload = rawOnly placePayload realRecallPayload
    rawFile = sizeFile rawPayload
    rawOnlySizeSkipped = Spec "c-select-raw-only-size-skipped"
      "contract: in M a slot with RAW-only IMAGE QUALITY (0xfa row 4, 0x1d = 1) and IMAGE SIZE row 1 selected over a RAW-only live state skips the IMAGE SIZE pair, as FUN_00d52684(5, 0x3c) greys the IMAGE SIZE row while the RAW cell is 1: the size and aspect cells hold the 0xfa arm's 0x00951550/0x00951558 values 2 and 3, live 0xf6/0xf9 keep their codes, the selection returns 0, and rb_select_trace records count 1, status 0, selection 1, mode 0x24, no refused field, no restore, two skipped fields and 0xf6 first"
      "rb_select" [1] (mModel { mCard = cardWith rawFile B.empty, mLive = rawLive, mExtra = withRawCells (mExtra mModel) })
      (Expect "returned" 0
        [ ("cells", 0x50, u32le rawStockSize)
        , ("cells", 0x60, u32le rawStockAspect)
        , liveAt rawLive 0xf6
        , liveAt rawLive 0xf9
        , traceClaim [1, 0, 1, 0, 0, 0, 0x24, 0, 0, 2, 0xf6]
        , ("card", 0, selectedFrom rawPayload 0x24 rawLive) ])
      ("rb_select" : loadFns ++ realStockFns) realStubs
    -- a JPEG slot (0xfa row 0) with IMAGE SIZE row 2 and film 0, refused by
    -- the film pin over a RAW-only live state with IMAGE SIZE row 1
    rawRestorePayload = placePayload (placePayload restorePayload (fieldOf 0xf6) 0) (fieldOf 0xf9) 2
    rawRestoreFile = encodeFile 7 0x0001 ((name26 "ALPHA", rawRestorePayload) : replicate 6 (name26 "ALPHA", payloadReal))
    rawOnlyRestore = Spec "c-select-raw-only-restore"
      ("contract: with FUN_00640e78 modeled to write the live film code " ++ show liveFilm ++ " back, a JPEG slot with IMAGE SIZE row 2 and film 0 selected in M over a RAW-only live state is applied and refused on 0xfe; the restore re-commits IMAGE QUALITY row 4 and writes the live IMAGE SIZE codes without the IMAGE SIZE commit, so the kernel answers 14 with every live byte equal to the fixture, the size and aspect cells hold the 0xfa arm's values 2 and 3, and rb_select_trace records count 1, status 0x0e, selection 1, property 0xfe, stored 0, read-back " ++ show liveFilm ++ ", mode 0x24, one refused field and no field left unrestored")
      "rb_select" [1] (mModel { mCard = cardWith rawRestoreFile B.empty, mLive = rawLive, mExtra = withRawCells (withFilmPin (mExtra mModel)) })
      (Expect "returned" 14
        [ ("live-state", 0, rawLive)
        , ("cells", 0x50, u32le rawStockSize)
        , ("cells", 0x60, u32le rawStockAspect)
        , traceClaim [1, 0x0e, 1, 0xfe, 0, liveFilm, 0x24, 1, 0, 0, 0]
        , ("card", 0, checkpointedFrom rawRestorePayload 0x24 rawLive) ])
      (("0x00640e78/" ++ show (B.length filmPin)) : "rb_select" : loadFns ++ realStockFns) (filter (/= "mode-state-c") realStubs)
    -- In P (mode cell 0x21) the 0x14f arm, the 0x141 arm with ISO step
    -- 0x009568b4 at most 5 and the 0x104 arm with DR key 0 call FUN_0063d5fc,
    -- which sets 0x009568d0 to 0 and 0x1c6 to 0x7f (key 0). FUN_00633eac and
    -- FUN_006464cc set 0x009568d0 to a key and write 0x1c6 from it.
    resetAutoPayload = placePayload isoAutoPayload (fieldOf 0x1c6) 0x80
    resetAutoFile = encodeFile 7 0x0001 ((name26 "ALPHA", resetAutoPayload) : replicate 6 (name26 "ALPHA", payloadReal))
    resetAutoLive = placeLive isoManualLive (fieldOf 0x1c6) 0x7e
    isoAuto1c6Reset = Spec "c-select-1c6-reset-p"
      "contract: in P a slot carrying ISO AUTO and 0x1c6 = 0x80 (key 1, which FUN_00631b74(P, 0x1c6, 1) admits) selected over live 0x1c6 = 0x7e returns 0 with 0x1c6 skipped, so the reset the 0x14f and 0x141 arms run through FUN_0063d5fc stands: live 0x1c6 reads 0x7f, the ISO step is the AUTO-maximum step, and rb_select_trace records count 1, status 0, selection 1, mode 0x21, no refused field, no restore, one skipped field and 0x1c6 first"
      "rb_select" [1] (isoModel (model (fileA [])) { mCard = cardWith resetAutoFile B.empty, mLive = resetAutoLive, mShootingMode = 0x21 })
      (Expect "returned" 0
        [ ("live-state", 0x1a6, B.singleton 0x7f)
        , ("cells", cellsIsoStep, u32le isoAutoStep)
        , traceClaim [1, 0, 1, 0, 0, 0, 0x21, 0, 0, 1, 0x1c6]
        , ("card", 0, selectedFrom resetAutoPayload 0x21 resetAutoLive) ])
      ("rb_select" : isoFns) isoStubs
    resetManualPayload = placePayload isoManualPayload (fieldOf 0x1c6) 0x80
    resetManualFile = encodeFile 7 0x0001 ((name26 "ALPHA", resetManualPayload) : replicate 6 (name26 "ALPHA", payloadReal))
    isoManual1c6Live = placeLive isoAutoLive (fieldOf 0x1c6) 0x7e
    isoManual1c6Kept = Spec "c-select-1c6-manual-iso-p"
      ("contract: in P a slot carrying manual ISO 0xc (key " ++ show (stepOf 0x141 isoManualCode) ++ ", above 5, so the 0x141 arm does not reset 0x1c6) and 0x1c6 = 0x80 selected over a live AUTO state with 0x1c6 = 0x7e returns 0 with nothing skipped: the AUTO-maximum commit's reset is followed by the slot's 0x1c6 written through FUN_00633eac(1) and FUN_006464cc, so live 0x1c6 reads 0x80, 0x009568d0 holds key 1, 0x141 reads 0xc, and rb_select_trace keeps its initial magic and zero count")
      "rb_select" [1] (isoModel (model (fileA [])) { mCard = cardWith resetManualFile B.empty, mLive = isoManual1c6Live, mShootingMode = 0x21 })
      (Expect "returned" 0
        [ ("live-state", 0x1a6, B.singleton 0x80)
        , ("cells", 0x98, B.singleton 0x01)
        , ("live-state", 0x52, B.singleton (fromIntegral isoManualCode))
        , ("cells", cellsIsoStep, u32le (stepOf 0x141 isoManualCode))
        , traceClaim (replicate 11 0)
        , ("card", 0, selectedFrom resetManualPayload 0x21 isoManual1c6Live) ])
      ("rb_select" : isoFns) isoStubs
    putByte off f b = B.take off b <> B.pack [f (B.index b off)] <> B.drop (off + 1) b
    putWord off v b = B.take off b <> u32le v <> B.drop (off + 4) b
    traceOffset = fromIntegral (addr ks "rb_select_trace" - addr ks "__rb_text_end")
    loadFns = ["rb_load", "rb_load_bound", "rb_scan_file", "rb_open", "rb_map_completion", "rb_io_cb", "rb_memcpy", "rb_close_quiet", "rb_checksum_file", "rb_name_ok", "rb_codes_ok", "rb_code_ok", "rb_unbound_ok", "rb_extract", "rb_place",
      "rb_media_epoch", "rb_media_matches", "rb_media_sample", "rb_io_request_identity", "rb_media_worker_allows", "rb_media_request_post", "rb_media_guarded_open", "rb_media_complete", "rb_media_guarded_write", "rb_media_guarded_close",
      "rb_u32", "rb_defer_close", "rb_v2_class", "rb_tag_owned", "rb_header_ok", "rb_zero", "rb_payload_safe", "rb_size_row", "rb_walked_field", "rb_quality_raw_code", "rb_menu_values", "rb_row_offered", "rb_code_row", "rb_bytes_equal", "rb_seal",
      "0x0061d514/16", "0x00631a00/336"]
    saveFns = loadFns ++ ["rb_save_current", "rb_write_back", "rb_stage_image", "0x0061e110/25832", "0x006439e0/40"]
    bankStubs = ["task", "vol", "card", "tone-notify", "pipeline-notify", "display-notify", "mode-state-a", "mode-state-b", "mode-state-c"]
    badChecksumFile = putWord 0x18 (readU32 validA 0x18 + 1) validA
    badCodeFile = putChecksum (putByte 0x40 (\v -> (v .&. 0xe0) .|. 0x1f) validA)
    badUnboundFile = putByte 0x47 (.|. 0x04) validA
    emptyBankClaim = [("live-state", 0, liveFixture)]
    badChecksum = Spec "d-load-bad-checksum"
      "contract: a checksum mismatch refuses the whole bank with INVALID and leaves the card and live settings unchanged"
      "rb_load" [] ((model (fileA [])) { mCard = cardWith badChecksumFile B.empty })
      (Expect "returned" 5 (emptyBankClaim ++ [("card", 0, cardWith badChecksumFile B.empty)])
        )
      loadFns bankStubs
    badCode = Spec "d-load-bad-code"
      "contract: a checksum-valid v1 bank whose populated slot carries a code the conversion table does not admit (ISO 31 against its 0..12 set) cannot migrate: rb_load returns 15 (unsafe bank), publishes nothing and leaves both files and the live settings untouched"
      "rb_load" [] ((model (fileA [])) { mCard = cardWith badCodeFile B.empty })
      (Expect "returned" 15 (emptyBankClaim ++ [("card", 0, cardWith badCodeFile B.empty)])
        )
      loadFns bankStubs
    badUnbound = Spec "d-load-unbound"
      "contract: a file carrying a value in the unbound +0x27 bits 2-7 is refused whole: rb_load returns 5 even though every bound code is valid"
      "rb_load" [] ((model (fileA [])) { mCard = cardWith badUnboundFile B.empty })
      (Expect "returned" 5 (emptyBankClaim ++ [("card", 0, cardWith badUnboundFile B.empty)])
        )
      loadFns bankStubs
    editedPayload = putByte 5 (const 0x80) validPayload
    stageFile = writtenOver 8 0x0001 validASlots 0 (Populated (name26 "") validPayload editedPayload)
    stageOk = Spec "e-stage-ok"
      "contract: a staged field edit (DR to code 2) followed by OK under DISABLE persists into the new v2 generation in the target file with the edit in the working payload, the saved payload unchanged and the staged name, and leaves the old v1 file intact"
      "rbm_seq_stage_field_commit" [0, 0x104, 2, 0x009a0000] ((model (fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "returned" 0 [("card", 0, cardWith validA stageFile)]
        )
      ("rbm_seq_stage_field_commit" : "rb_stage_load" : "rb_stage_field" : "rb_stage_commit" : "rb_stage_image" : "rb_write_back" : loadFns) bankStubs
    stageCancel = Spec "e-stage-cancel"
      "contract: a staged field edit followed by CANCEL writes nothing: the card is byte-identical to the input and no new generation appears"
      "rbm_seq_stage_field_cancel" [0, 0x104, 2] ((model (fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "returned" 0 [("card", 0, cardWith validA B.empty)]
        )
      ("rbm_seq_stage_field_cancel" : "rb_stage_load" : "rb_stage_field" : "rb_stage_cancel" : loadFns) bankStubs
    resetFile = encodeFileV2 8 emptyBank
    resetCase = Spec "f-reset"
      "contract: reset clears the slot's record and its set-mask bit in the new v2 generation in the target file, and leaves the old v1 file intact"
      "rb_reset" [0] ((model (fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "returned" 0 [("card", 0, cardWith validA resetFile)]
        )
      ("rb_reset" : "rb_stage_image" : "rb_write_back" : loadFns) bankStubs
    renameFile = writtenOver 8 0x0001 validASlots 0 (Populated (name26 "DELTA") validPayload validPayload)
    renameCase = Spec "g-rename"
      "contract: renaming a set slot to DELTA persists the new name in the new v2 generation in the target file with the saved and working payloads unchanged, and leaves the old v1 file intact"
      "rb_name_commit" [0, 0x009a0000] ((model (fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive, mCase = name26 "DELTA" })
      (Expect "returned" 0 [("card", 0, cardWith validA renameFile)]
        )
      ("rb_name_commit" : "rb_stage_image" : "rb_write_back" : loadFns) bankStubs
    renameUnsetFile = writtenOver 8 0x0001 validASlots 1 (Populated (name26 "DELTA") extractPayload extractPayload)
    renameUnsetCase = Spec "g-rename-unset"
      "contract: renaming unset slot 1 to DELTA sets it: the new v2 generation in the target file carries DELTA with saved and working payloads of the live settings the stock getter reads, not the per-slot defaults row 0x007cb804, with set-mask bit 1 added, slot 0 unchanged, and leaves the old v1 file intact"
      "rb_name_commit" [1, 0x009a0000] ((model (fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive, mCase = name26 "DELTA" })
      (Expect "returned" 0 [("card", 0, cardWith validA renameUnsetFile)]
        )
      ("rb_name_commit" : "rb_stage_image" : "rb_write_back" : saveFns) bankStubs
    readOnlySave = Spec "h-write-read-only"
      "contract: a save against a read-only card (the write completion 8 maps to read-only) returns 2 and leaves both files untouched"
      "rb_save_current" [0, 0] ((model (("rbm_write_comp", 8) : fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "returned" 2 [("card", 0, cardWith validA B.empty)]
        )
      saveFns bankStubs
    fullCardSave = Spec "h-write-full"
      "contract: a save against a full card (the write completion 3 maps to full) returns 3 and leaves both files untouched"
      "rb_save_current" [0, 0] ((model (("rbm_write_comp", 3) : fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "returned" 3 [("card", 0, cardWith validA B.empty)]
        )
      saveFns bankStubs
    saveFile = writtenOver 8 0x0001 validASlots 0 (Populated (name26 "ALPHA") extractPayload extractPayload)
    partialFile = B.take 256 saveFile
    partialWrite = Spec "h-partial-write"
      "contract: a write that stops after 256 bytes returns 3 (full) and leaves exactly the first 256 bytes of the new v2 file at the target slot; the winner file is byte-identical"
      "rb_save_current" [0, 0] ((model (("rbm_write_short", 256) : fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "returned" 3 [("card", 0, cardWith validA partialFile)]
        )
      saveFns bankStubs
    powerLoss = Spec "h-power-loss"
      "contract: interruption halfway through the target-file write leaves the winning generation byte-identical"
      "rb_save_current" [0, 0] ((model (("rbm_trip_step", 2) : fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "memory_guard" 0x00005150 [("card", 0, cardWith validA (B.take 512 saveFile))]
        )
      saveFns bankStubs
    genExhaust = Spec "g2-save-gen-exhaust"
      "contract: a save against a winner at generation 0xffffffff must return 10 (generation exhausted) and truncate no file"
      "rb_save_current" [1, 0] ((model (fileA [])) { mCard = cardWith exhaust B.empty })
      (Expect "returned" 10 [("card", 0, cardWith exhaust B.empty)]
        )
      saveFns bankStubs
    -- The event shim runs at the worker boundary, before a mode-1 open or write.
    mediaMoveInScan = Spec "h-save-media-move-in-scan"
      "contract: a card change during bank discovery returns MEDIA_CHANGED without altering either file"
      "rb_save_current" [0, 0] ((model (("rbm_detach_before_stat", 1) : fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "returned" 13
        [ ("card", 0, cardWith validA B.empty)
        , ("live-state", 0, extractLive)
        ])
      saveFns bankStubs
    olderB = encodeFile 6 0x0002 (replicate 7 (name26 "BETA", validPayload))
    replacementA = encodeFile 11 0x0001 (replicate 7 (name26 "REPLACED", validPayload))
    replacementCard = cardWith replacementA olderB
    withReplacement card =
      B.take (2 * fileStride) card <> fileSlot validA
        <> B.take (2 * fileStride) replacementCard
    workerSwapModel boundary =
      (model (swapBoundaryCell boundary : fileA
        [ ("rbm_b_present", 1), ("rbm_b_size", 512)
        , ("rbm_swap_a_size", 512), ("rbm_swap_b_present", 1), ("rbm_swap_b_size", 512)
        , ("rbm_swap_card", fromIntegral (0x00980000 + cardBytes))
        ]))
        { mCard = withReplacement (cardWith validA olderB), mLive = extractLive }
    replacementClaims =
      [ ("card", 0, withReplacement replacementCard)
      , ("live-state", 0, extractLive)
      ]
    mediaMoveInOpen = Spec "h-save-media-move-before-truncate"
      "contract: replacement after dispatch and before destructive open returns MEDIA_CHANGED and preserves the replacement bank files, including the existing target's length"
      "rb_save_current" [0, 0] (workerSwapModel BeforeDestructiveOpen)
      (Expect "returned" 13 replacementClaims)
      saveFns bankStubs
    mediaMoveInWrite = Spec "h-save-media-move-mid-write"
      "contract: replacement after dispatch and before write returns MEDIA_CHANGED and preserves both replacement bank files and live settings"
      "rb_save_current" [0, 0] (workerSwapModel BeforeWrite)
      (Expect "returned" 13 replacementClaims)
      saveFns bankStubs
    closeDeferred = Spec "h-write-close-deferred-flush"
      "contract: a successful no-op write close leaves the handle for a later close; readback verification still completes the save and the durable generation preserves ALPHA"
      "rb_save_current" [0, 0] ((model (closeFaultCell NoopWriteClose : fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "returned" 0 [("card", 0, cardWith validA saveFile), ("live-state", 0, extractLive)])
      saveFns bankStubs
    twoDeferredReadback = Spec "h-load-two-deferred-readback"
      "contract: repeatedly discovering two bank files while closes are refused, then permitting closes for slot readback, returns the winning slot without exhausting handles or changing either file or live settings"
      "rbm_seq_read_after_deferred" []
      ((model (("rbm_close_noop", 1) : fileA [("rbm_b_present", 1), ("rbm_b_size", 512)]))
        { mCard = cardWith validA validB, mLive = extractLive })
      (Expect "returned" 0
        [ ("card", 0, cardWith validA validB)
        , ("live-state", 0, extractLive)
        , ("casebuf", 0, name26 "BETA")
        , ("casebuf", 0x20, validPayload)
        ])
      ("rbm_seq_read_after_deferred" : "rb_readback" : loadFns) bankStubs
    -- Repeated public operations expose deferred-close exhaustion as a refusal.
    gen9File = encodeFileV2 9 (withRecord 0 (Populated (name26 "ALPHA") extractPayload extractPayload) (migrated 0x0001 validASlots))
    openTagZero = Spec "h-open-zero-tag-flush"
      "contract: a delivered handle with a zero dispatch tag refuses the first load with IO; the next load and public readback succeed without changing either file"
      "rbm_seq_load_twice" [0x009a0000]
      ((model (("rbm_open_fault", 1) : fileA [])) { mCard = cardWith validA B.empty })
      (Expect "returned" 0
        [ ("card", 0, cardWith validA B.empty)
        , ("casebuf", 0, u32le 4)
        , ("casebuf", 0x20, name26 "ALPHA")
        , ("casebuf", 0x40, validPayload)
        ])
      ("rbm_seq_load_twice" : loadFns) bankStubs
    writeCloseOwned = Spec "h-write-close-tag-owned"
      "contract: a verify close with a successful callback but retained ownership permits the first save; the second save also succeeds and preserves both durable generations"
      "rbm_seq_save_twice" [0, 0x009a0000]
      ((model (closeFaultCell KeepVerifyClose : fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "returned" 0
        [ ("card", 0, cardWith gen9File saveFile)
        , ("casebuf", 0, u32le 0)
        , ("live-state", 0, extractLive)
        ])
      ("rbm_seq_save_twice" : saveFns) bankStubs
    writeCloseLate = Spec "h-write-close-late-release"
      "contract: a verify close that releases the handle without a callback refuses the first save with IO; the next save loads the written generation and succeeds without changing live settings"
      "rbm_seq_save_twice" [0, 0x009a0000]
      ((model (closeFaultCell LoseVerifyCallback : fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "returned" 0
        [ ("card", 0, cardWith gen9File saveFile)
        , ("casebuf", 0, u32le 4)
        , ("live-state", 0, extractLive)
        ])
      ("rbm_seq_save_twice" : saveFns) bankStubs
    -- v2 format and migration (recipe-design section 18.3) -------------------
    v2Builders =
      [ v2WorkingReadback, v2RenameKeepsSavedWorking, v2RecallAppliesWorking, v2HostRewrite
      , v2Mixed "v2-mixed-v2-newer" "a v1 A at generation 7 and a v2 B at generation 8: B wins" (v1One 7 "ALPHA", False) (v2One 8 "BRAVO", True) "BRAVO"
      , v2Mixed "v2-mixed-v1-newer" "a v2 A at generation 8 and a v1 B at generation 9: the migrated v1 B wins" (v2One 8 "INDIA", True) (v1One 9 "JULIET", False) "JULIET"
      , v2Mixed "v2-mixed-tie-a-v2" "a v2 A and a v1 B both at generation 9: the tie goes to A" (v2One 9 "ECHO", True) (v1One 9 "FOXTROT", False) "ECHO"
      , v2Mixed "v2-mixed-tie-a-v1" "a v1 A and a v2 B both at generation 9: the tie goes to the v1 A, published migrated" (v1One 9 "GOLF", False) (v2One 9 "HOTEL", True) "GOLF"
      , v2Mixed "v2-unsafe-older-ignored" "an unsafe v1 A (IMAGE SIZE row 0) at generation 6 and a valid v2 B at generation 7: the older unsafe file does not block the bank and B wins" (unsafeV1 6, False) (v2One 7 "BRAVO", True) "BRAVO"
      , v2Unsafe "v2-unsafe-working-blocks" "a valid v1 A at generation 7 and a v2 B at generation 8 whose populated slot's working payload has aspect 0 (IMAGE SIZE row 0)" (v1One 7 "ALPHA", False) (encodeFileV2 8 (bankOf [Populated (name26 "BRAVO") validPayload unsafePayload]), True)
      , v2Unsafe "v2-unsafe-saved-blocks" "a valid v1 A at generation 7 and a v2 B at generation 8 whose populated slot's saved payload has aspect 0 (IMAGE SIZE row 0) and whose working payload is safe" (v1One 7 "ALPHA", False) (encodeFileV2 8 (bankOf [Populated (name26 "BRAVO") unsafePayload validPayload]), True)
      , v2Unsafe "v2-unsafe-base-blocks" "a valid v1 A at generation 7 and a v2 B at generation 8 selecting slot 1 over a valid base whose payload has aspect 0" (v1One 7 "ALPHA", False) (encodeFileV2 8 (BankV2 1 0 (BaseValid CaptureP unsafePayload) [Populated (name26 "BRAVO") validPayload validPayload]), True)
      , v2Unsafe "v2-unsafe-tie-a-blocks" "an unsafe v1 A and a valid v2 B both at generation 8: the tie goes to A" (unsafeV1 8, False) (v2One 8 "BRAVO", True)
      , v2UnsafeWrite "v2-unsafe-v1-save-refused" "Save Current of slot 1" "rb_save_current" [1, 0] ("rb_save_current" : saveFns)
      , v2UnsafeWrite "v2-unsafe-v1-reset-refused" "Reset of slot 0" "rb_reset" [0] ("rb_reset" : "rb_stage_image" : "rb_write_back" : loadFns)
      , v2UnsafeWrite "v2-unsafe-v1-rename-refused" "Rename of slot 0 to DELTA" "rb_name_commit" [0, 0x009a0000] ("rb_name_commit" : "rb_stage_image" : "rb_write_back" : loadFns)
      , v2UnsafeLive "v2-save-unsafe-live-refused" "Save Current of slot 1" "rb_save_current" [1, 0] saveFns
      , v2UnsafeLive "v2-rename-unsafe-live-refused" "Rename of unset slot 1 to DELTA" "rb_name_commit" [1, 0x009a0000] ("rb_name_commit" : "rb_stage_image" : "rb_write_back" : saveFns)
      , v2Invalid "v2-invalid-selection-unpopulated" "selects CUSTOM 2 while slot 1 is unpopulated" (encodeFileV2 8 (BankV2 2 0 baseP [Populated (name26 "BRAVO") validPayload validPayload]))
      , v2Invalid "v2-invalid-selection-no-base" "selects CUSTOM 1 with the base invalid" (encodeFileV2 8 (BankV2 1 0 BaseInvalid [Populated (name26 "BRAVO") validPayload validPayload]))
      , v2Invalid "v2-invalid-selection-code" "has selection code 8" (putChecksum (putByte 0x20 (const 8) (v2One 8 "BRAVO")))
      , v2Invalid "v2-invalid-auto-code" "has auto-update code 2" (putChecksum (putByte 0x21 (const 2) (v2One 8 "BRAVO")))
      , v2Invalid "v2-invalid-base-valid-code" "has base-valid code 2" (putChecksum (putByte 0x22 (const 2) (v2One 8 "BRAVO")))
      , v2Invalid "v2-invalid-base-residue" "has a nonzero payload under an invalid base" (putChecksum (putByte 0x40 (const 1) (v2One 8 "BRAVO")))
      , v2Invalid "v2-invalid-working-unbound" "has an unbound bit in the working payload" (putChecksum (putByte (0x68 + 66 + 0x27) (.|. 4) (v2One 8 "BRAVO")))
      , v2Unsafe "v2-unsafe-working-quality" "a v2 B has a working IMAGE QUALITY/RAW pair the stock arm cannot produce" (v1One 7 "ALPHA", False) (encodeFileV2 8 (bankOf [Populated (name26 "BRAVO") validPayload (placePayload validPayload (fieldOf 0x1d) 1)]), True)
      , v2Invalid "v2-invalid-base-mode" "marks a valid base with capture mode byte 5, no P/S/A/M code" (putChecksum (putByte 0x23 (const 5) (encodeFileV2 8 (BankV2 1 0 baseP [Populated (name26 "BRAVO") validPayload validPayload]))))
      , v2Invalid "v2-invalid-enable-differs" "is ENABLE with a populated slot's working payload different from its saved payload" (encodeFileV2 8 (BankV2 0 1 BaseInvalid [Populated (name26 "BRAVO") validPayload distinctWorking]))
      , v2Invalid "v2-invalid-unpopulated-residue" "carries a nonzero name byte in unpopulated record 1" (putChecksum (putByte (0x68 + 106) (const 0x41) (v2One 8 "BRAVO")))
      , v2Mixed "v2-valid-selection-base" "a v1 A at generation 7 and a v2 B at generation 8 selecting CUSTOM 1 over a valid base captured in P: B wins" (v1One 7 "ALPHA", False) (encodeFileV2 8 (BankV2 1 0 baseP [Populated (name26 "BRAVO") validPayload validPayload]), True) "BRAVO"
      , v2GenExhaust, v2ResetSelected, v2RenameKeepsSelection, v2EnableEditSaves, v2ReadError
      , v2FailedReload "v2-reload-read-error" "rbm_read_comp" 4 4
      , v2FailedReload "v2-reload-media-change" "rbm_detach_before_stat" 1 13 ]
      ++ map v2TornLoad [0 .. 1023]
      ++ map v1TornLoad [0 .. 511]
      ++ map v2TornWrite [0 .. 1023]
    unsafePayload = placePayload validPayload (fieldOf 0xf9) 0
    distinctWorking = placePayload validPayload (fieldOf 0x104) 2
    baseP = BaseValid CaptureP validPayload
    bankOf = BankV2 0 0 BaseInvalid
    v1One gen name = encodeFile gen 0x0001 [(name26 name, validPayload)]
    v2One gen name = encodeFileV2 gen (bankOf [Populated (name26 name) validPayload validPayload])
    unsafeV1 gen = encodeFile gen 0x0001 [(name26 "ALPHA", unsafePayload)]
    sizeCells (a, aV2) (_, bV2) = (if aV2 then fileA2 else fileA) ((if bV2 then fileB2 else fileB) [])
    v2Mixed cid what a b winner = loadReadback cid
      ("contract: " ++ what ++ "; public readback of slot 0 returns " ++ winner ++ " with its working payload and neither file changes")
      0 (cardWith (fst a) (fst b)) (sizeCells a b) (readbackOf (name26 winner) validPayload)
    v2Unsafe cid what a b = Spec cid
      ("contract: " ++ what ++ ": the newest checksum-valid generation holds a payload no stock menu produces, so rb_load returns 15 (unsafe bank), publishes nothing and changes neither file")
      "rb_load" [] ((model (sizeCells a b)) { mCard = cardWith (fst a) (fst b) })
      (Expect "returned" 15 [("card", 0, cardWith (fst a) (fst b)), ("live-state", 0, liveFixture)])
      loadFns bankStubs
    v2UnsafeWrite cid what entry args fns = Spec cid
      ("contract: " ++ what ++ " over a v1 bank whose populated slot 0 stores aspect 0 (IMAGE SIZE row 0) is refused with 15 (unsafe bank, cannot migrate): neither file is rewritten and the live settings are unchanged")
      entry args ((model (fileA [])) { mCard = cardWith (unsafeV1 7) B.empty, mLive = extractLive, mCase = name26 "DELTA" })
      (Expect "returned" 15 [("card", 0, cardWith (unsafeV1 7) B.empty), ("live-state", 0, extractLive)])
      fns bankStubs
    unsafeLive = placeLive extractLive (fieldOf 0xf9) 0
    v2UnsafeLive cid what entry args fns = Spec cid
      ("contract: " ++ what ++ " over a valid v1 bank while live 0xf9 = 0 (IMAGE SIZE row 0) returns 5 before any write; both files and the live settings are unchanged")
      entry args ((model (fileA [])) { mCard = cardWith validA B.empty, mLive = unsafeLive, mCase = name26 "DELTA" })
      (Expect "returned" 5 [("card", 0, cardWith validA B.empty), ("live-state", 0, unsafeLive)])
      fns bankStubs
    v2Invalid cid what file = loadReadback cid
      ("contract: a v1 A at generation 7 and a checksum-valid v2 B at generation 8 that " ++ what ++ ": B is not a bank file, so readback of slot 0 returns A's ALPHA and neither file changes")
      0 (cardWith (v1One 7 "ALPHA") file) (fileA (fileB2 [])) (readbackOf (name26 "ALPHA") validPayload)
    v2Distinct = encodeFileV2 7 (bankOf [Populated (name26 "ALPHA") validPayload distinctWorking])
    v2WorkingReadback = loadReadback "v2-load-working-readback"
      "contract: a v2 A whose slot 0 holds distinct saved (DR 1) and working (DR 2) payloads loads, and public readback of slot 0 returns ALPHA with the working payload"
      0 (cardWith v2Distinct B.empty) (fileA2 []) (readbackOf (name26 "ALPHA") distinctWorking)
    v2RenameKeepsSavedWorking = Spec "v2-rename-keeps-saved-working"
      "contract: renaming slot 0 of that v2 bank to DELTA reloads it and writes generation 8 to B with the name changed and slot 0's distinct saved and working payloads both retained; A is unchanged"
      "rb_name_commit" [0, 0x009a0000] ((model (fileA2 [])) { mCard = cardWith v2Distinct B.empty, mLive = extractLive, mCase = name26 "DELTA" })
      (Expect "returned" 0 [("card", 0, cardWith v2Distinct (encodeFileV2 8 (bankOf [Populated (name26 "DELTA") validPayload distinctWorking])))])
      ("rb_name_commit" : "rb_stage_image" : "rb_write_back" : loadFns) bankStubs
    v2HostRewrite = Spec "v2-host-fixture-native-rewrite"
      "contract: rewriting the independently host-encoded seven-slot fixture changes only its generation and checksum in the nonwinner; all names and saved/working payloads survive"
      "rb_name_commit" [0, 0x009a0000]
      ((model (fileA2 [])) { mCard = cardWith hostBank B.empty, mCase = name26 "ALPHA" })
      (Expect "returned" 0 [("card", 0, cardWith hostBank (putChecksum (putWord 0x10 8 hostBank)))])
      ("rb_name_commit" : "rb_stage_image" : "rb_write_back" : loadFns) bankStubs
    v2RecallBank = bankOf [Populated (name26 "ALPHA") admittedPayload (placePayload admittedPayload (fieldOf 0x104) 2)]
    v2RecallFile = encodeFileV2 7 v2RecallBank
    v2RecallAppliesWorking = Spec "v2-select-applies-working"
      "contract: in M, selecting a v2 slot whose saved payload has DR 1 and whose working payload has DR 2 applies the working payload: live DR reads 2, every other live byte is unchanged, the selection returns 0, and the card holds the base checkpoint at generation 8 in B and the selection at generation 9 in A"
      "rb_select" [1] (mModel { mCells = M.fromList (fileA2 []), mCard = cardWith v2RecallFile B.empty })
      (Expect "returned" 0
        [ ("live-state", 0, placeLive admittedLive (fieldOf 0x104) 2)
        , ("card", 0, selectedCard 7 v2RecallBank 0x24 admittedLive 1) ])
      ("rb_select" : loadFns ++ realStockFns) realStubs
    v2Exhausted = encodeFileV2 0xffffffff (bankOf [Populated (name26 "BRAVO") validPayload validPayload])
    v2GenExhaust = Spec "v2-save-gen-exhaust"
      "contract: a save against a v2 winner at generation 0xffffffff returns 10 (generation exhausted) and changes neither file"
      "rb_save_current" [1, 0] ((model (fileA2 [])) { mCard = cardWith v2Exhausted B.empty, mLive = extractLive })
      (Expect "returned" 10 [("card", 0, cardWith v2Exhausted B.empty)])
      saveFns bankStubs
    selectedBank = BankV2 1 0 baseP [Populated (name26 "ALPHA") validPayload validPayload, Populated (name26 "BRAVO") validPayload validPayload]
    v2ResetSelected = Spec "v2-reset-selected-refused"
      "contract: resetting slot 0 of a v2 bank that selects CUSTOM 1 over a base captured in P is refused with 17 (the slot is the persisted selection): neither file changes"
      "rb_reset" [0] ((model (fileA2 [])) { mCard = cardWith (encodeFileV2 7 selectedBank) B.empty, mLive = extractLive })
      (Expect "returned" 17 [("card", 0, cardWith (encodeFileV2 7 selectedBank) B.empty)])
      ("rb_reset" : loadFns) bankStubs
    -- selection transactions (recipe-design section 18.4). In M over the
    -- admitted fixture, with the 0xfa arm's RAW-only cells: CUSTOM 1 moves DR
    -- to 2 and IMAGE SIZE to row 2; CUSTOM 2 moves sharpness to 2 and IMAGE
    -- QUALITY to RAW-only, under which its stored IMAGE SIZE row 3 is skipped.
    -- The bank wins in A at generation 7; generations alternate B, A, B, A.
    selA = placePayload (placePayload admittedPayload (fieldOf 0x104) 2) (fieldOf 0xf9) 2
    selB = rawOnly placePayload (placePayload (placePayload admittedPayload (fieldOf 0x10f) 2) (fieldOf 0xf9) 3)
    selBankOf b = bankOf [Populated (name26 "ALPHA") selA selA, Populated (name26 "BRAVO") b b]
    selBank = selBankOf selB
    selFile = encodeFileV2 7 selBank
    baseM = BaseValid CaptureM (payloadOfLive admittedLive)
    selGenOf bank g sel = encodeFileV2 g (bank { bSelection = sel, bBase = baseM })
    selGen = selGenOf selBank
    liveA = placeLive (placeLive admittedLive (fieldOf 0x104) 2) (fieldOf 0xf9) 2
    liveB = rawOnly placeLive (placeLive admittedLive (fieldOf 0x10f) 2)
    selModel cells = mModel
      { mCells = M.fromList (fileA2 cells), mCard = cardWith selFile B.empty
      , mExtra = withRawCells (mExtra mModel) }
    selFns = loadFns ++ realStockFns
    -- the dial boundaries rbm_seq_selects' dial ops and a pending settle run
    dialSelects = selectsWith ["rb_mode_leave", "rb_mode_resume", "rb_base_restore"] realStubs
    atMode m model' cells = (model' cells) { mShootingMode = m }
    selGenAt m g sel = encodeFileV2 g (selBank { bSelection = sel, bBase = BaseValid (captureOf m) (payloadOfLive admittedLive) })
    -- CUSTOM 2's selection write, the third, fails full while the dial reaches C
    raceToC = [("rbm_fail_write_at", 3), ("rbm_fail_write_code", 3), ("rbm_mode_at_write", 3),
               ("rbm_mode_race_value", 0x24), ("rbm_class_race_value", 3)]
    statusesClaim ss = ("casebuf", 0x100, B.concat (map u32le ss))
    dialObserved boundary step mode cls dr aspect effective =
      ("casebuf", (if boundary then 0x600 else 0x200) + 28 * step,
       B.concat (map u32le [mode, cls, dr, aspect]))
      : [("casebuf", (if boundary then 0x600 else 0x200) + 28 * step + 20,
          u32le 0 <> u32le effective)]
    restoreWrite = u32le (0xea000000 .|. (fromIntegral ((addr ms "rbm_restore_write" - (0x0061c91c + 8)) `div` 4) .&. 0x00ffffff))
    withRestoreWrite regions =
      [(n, a, if n == "chain" then pokeAt (fromIntegral (0x0061c91c - a)) restoreWrite b else b) | (n, a, b) <- regions]
    dialFault = selectsWith ["rb_mode_leave", "rb_mode_resume", "rb_base_restore"] realStubs
    faultModel cells = (selModel cells) { mExtra = withRestoreWrite (mExtra (selModel cells)) }
    refreshedLive = placeLive admittedLive (fieldOf 0x10f) 2
    refreshedGen g sel = encodeFileV2 g (selBank { bSelection = sel, bBase = BaseValid CaptureM (payloadOfLive refreshedLive) })
    pendingModel mode cells = (atMode mode selModel cells)
      { mLive = placeLive admittedLive (fieldOf 0x1c6) (if mode == 0x24 then 0x80 else 0x7f) }
    rawBaseLive = rawOnly placeLive admittedLive
    rawDialModel cells = (selModel cells) { mLive = rawBaseLive }
    rawDialGen g sel = encodeFileV2 g (selBank { bSelection = sel,
      bBase = BaseValid CaptureM (payloadOfLive rawBaseLive) })
    branchAt from to = 0xea000000 .|. (fromIntegral ((to - (from + 8)) `div` 4) .&. 0x00ffffff)
    refusalCaller kind =
      let target = addr ks (if kind == 0 then "rb_mode_dial_apply" else "rb_mode_pair_apply")
          -- Playback's ADR LR / B leaves the runner's tracked return at the saved caller frame.
          prefix = [0xe92d4830, 0xe28db00c] ++ [0xe24dd008 | kind == 2] ++ [0xe28fe000 | kind /= 0]
          callAt = 0x009b0000 + 4 * toInteger (length prefix)
          transfer = branchAt callAt target .|. (if kind == 0 then 0x01000000 else 0)
          suffix = if kind == 0 then [0xe3500000, 0x13e00000, 0x03a00077] else [0xe3a00077]
       in B.concat (map u32le (prefix ++ [transfer] ++ suffix ++ [0xe24bd00c, 0xe8bd8830]))
    refusalRegions =
      [ if RegionId name == RegionId "dispatcher"
          then (name, 0x00d38d98, failureRedraw <>
                B.replicate (fromIntegral (base - 0x00d38d98) - B.length failureRedraw) 0 <> bytes)
          else region
      | region@(name, base, bytes) <- mExtra (faultModel []) ]
    wrapperRefusal kind = Spec ("s-dial-wrapper-refusal-" ++ show kind)
      "contract: a refused outgoing base restore returns -1 through the stock-shaped handler frame without stock routing or destination continuation; real stock redraw publishes 0x400, CUSTOM 1 and its effective label remain in M, with C and both files unchanged"
      "rbm_seq_select_mode_refusal" [0x009b0000]
      ((faultModel []) { mExtra = ("prefix", 0x009b0000, refusalCaller kind) : refusalRegions })
      (Expect "returned" 0xffffffff
        ([("live-state", 0, liveA), ("card", 0, cardWith (selGen 9 1) (selGen 8 0)),
          ("media-state", 0x40290, u32le 0x400),
          ("media-state", 0x40288, u32le 0), ("media-state", 0x4028c, u32le 0)]
         ++ dialObserved False 0 0x24 1 2 2 1))
      ("rbm_seq_select_mode_refusal" : ("0x009b0000/" ++ show (B.length (refusalCaller kind))) :
       "0x00d38d98/132" : "0x006355dc/36" : "rb_selection_effective" : "rb_select" : selFns) realStubs
    selectsWith fns stubs cid what codes cells m expected claims = Spec cid ("contract: " ++ what)
      "rbm_seq_selects" [0x009a0000, fromIntegral (length codes), 0x009a0100]
      ((m cells) { mCase = B.pack codes })
      (Expect "returned" (last expected) (statusesClaim expected : claims))
      (fns ++ "rbm_seq_selects" : "rb_select" : selFns) stubs
    selects = selectsWith [] realStubs
    selectsFilmPinned = selectsWith ["0x00640e78/" ++ show (B.length filmPin)] (filter (/= "mode-state-c") realStubs)
    traceWord k v = ("kdata", traceOffset + 4 * k, u32le v)
    selectionBuilders =
      [ selects "s-select-a-b-none-restores-base"
          "CUSTOM 1, CUSTOM 2, then NOTHING SELECTED each return 0; every live byte ends equal to the captured base, the IMAGE SIZE cells hold the base row's pair (0, 1), and the card holds generation 11 (NOTHING SELECTED) in A and generation 10 (CUSTOM 2) in B, both with the base captured in M"
          [1, 2, 0] [] selModel [0, 0, 0]
          [ ("live-state", 0, admittedLive), ("cells", 0x50, u32le 0), ("cells", 0x60, u32le 1)
          , ("card", 0, cardWith (selGen 11 0) (selGen 10 2)) ]
      , selects "s-select-b-skips-hold-base"
          "CUSTOM 1 then CUSTOM 2 return 0: CUSTOM 2's RAW-only quality skips its IMAGE SIZE pair, so live 0xf6/0xf9 hold the base's row 1 rather than CUSTOM 1's row 2, live DR holds the base's code rather than CUSTOM 1's 2, every other live byte is the base with CUSTOM 2 applied, the size cells hold the 0xfa arm's 2 and 3, and rb_select_trace records selection 2 with two skipped fields and 0xf6 first"
          [1, 2] [] selModel [0, 0]
          [ ("live-state", 0, liveB), ("cells", 0x50, u32le rawStockSize), ("cells", 0x60, u32le rawStockAspect)
          , traceWord 3 2, traceWord 10 2, traceWord 11 0xf6
          , ("card", 0, cardWith (selGen 9 1) (selGen 10 2)) ]
      , selects "s-select-noop-reselect"
          "selecting CUSTOM 1 twice returns 0 both times; the second writes nothing and applies nothing: the card holds only the checkpoint (generation 8 in B) and the selection (generation 9 in A), and the live bytes are the base with CUSTOM 1 applied"
          [1, 1] [] selModel [0, 0]
          [ ("live-state", 0, liveA), ("card", 0, cardWith (selGen 9 1) (selGen 8 0)) ]
      , selects "s-select-none-noop"
          "NOTHING SELECTED while the base is live returns 0 and writes and applies nothing"
          [0] [] selModel [0]
          [ ("live-state", 0, admittedLive), ("card", 0, cardWith selFile B.empty) ]
      , selects "s-select-unset"
          "selecting CUSTOM 3, an unpopulated slot, returns 6 (unset) with no base checkpoint and no live write"
          [3] [] selModel [6]
          [ ("live-state", 0, admittedLive), ("card", 0, cardWith selFile B.empty) ]
      , selects "s-select-checkpoint-write-fails"
          "a full card (write completion 3) on the base checkpoint, the selection's first write, returns 3 before any live write: the live bytes and both files are unchanged"
          [1] [("rbm_fail_write_at", 1), ("rbm_fail_write_code", 3)] selModel [3]
          [ ("live-state", 0, admittedLive), ("card", 0, cardWith selFile B.empty) ]
      , selects "s-select-unsafe-live-base-refused"
          "from NOTHING SELECTED over live 0xf9 = 0 (IMAGE SIZE row 0), selecting CUSTOM 1 returns 5 before any write: no base checkpoint, the live bytes and both files are unchanged"
          [1] [] (\cells -> (selModel cells) { mLive = placeLive admittedLive (fieldOf 0xf9) 0 }) [5]
          [ ("live-state", 0, placeLive admittedLive (fieldOf 0xf9) 0), ("card", 0, cardWith selFile B.empty) ]
      , selects "s-select-mode-race-write"
          "with CUSTOM 1 active in M, the dial reaching C (mode class 3, record mode M) during CUSTOM 2's selection write, which fails full (completion 3), answers 16 and writes nothing live at C: the C record keeps the fixture, the ordinary bytes keep CUSTOM 2's applied state for the next arrival in P, S, A or M to settle, and the card keeps generation 9 (CUSTOM 1) in A and the checkpoint in B"
          [1, 2] raceToC selModel [0, 16]
          [ ("live-state", 0, liveB), ("cells", 0x50, u32le rawStockSize), ("cells", 0x60, u32le rawStockAspect)
          , ("media-state", 0x41424, u32le 3)
          , ("card", 0, cardWith (selGen 9 1) (selGen 8 0)) ]
      , dialSelects "s-dial-race-write-settles"
          "after that race, the dial back to M puts back the live codes read before CUSTOM 2, so every live byte and the IMAGE SIZE cells are CUSTOM 1's again and CUSTOM 1 is active: reselecting it writes nothing, and the card keeps generation 9 (CUSTOM 1) in A and the checkpoint in B"
          [1, 2, dialOp DialM, 1] raceToC selModel [0, 16, 0, 0]
          [ ("live-state", 0, liveA), ("cells", 0x50, u32le 0), ("cells", 0x60, u32le 2)
          , ("card", 0, cardWith (selGen 9 1) (selGen 8 0)) ]
      , dialSelects "s-dial-none-excursion"
          "with NOTHING SELECTED effective in M, dial moves to AUTO, C and back to M write nothing and leave every live byte and both files unchanged"
          [dialOp DialAuto, dialOp DialC, dialOp DialM] [] selModel [0, 0, 0]
          ([ ("live-state", 0, admittedLive), ("card", 0, cardWith selFile B.empty) ]
           ++ dialObserved False 0 0 0 1 1 0
           ++ dialObserved False 1 0x24 3 0 0 0
           ++ dialObserved False 2 0x24 0 1 1 0)
      , dialSelects "s-dial-p-c-p"
          "in P, CUSTOM 1 is selected over the base captured in P; the dial to C replaces the applied bank by that base before the class changes, so the C record and every ordinary byte equal the fixture while at C; the dial back to P resumes CUSTOM 1 without a write (the ordinary base equals the persisted one), and reselecting CUSTOM 1 is a no-op: live bytes are CUSTOM 1's and the card holds the checkpoint (generation 8 in B) and the selection (generation 9 in A)"
          [1, dialOp DialC, dialOp DialP, 1] [] (atMode 0x21 selModel) [0, 0, 0, 0]
          ([ ("live-state", 0, liveA), ("cells", 0, u32le 0x21)
          , ("card", 0, cardWith (selGenAt 0x21 9 1) (selGenAt 0x21 8 0)) ]
          ++ dialObserved False 1 0x24 3 0 0 0xffffffff
          ++ dialObserved True 1 0x21 1 1 1 0xffffffff
          ++ dialObserved False 2 0x21 0 2 2 1)
      ]
      ++ [ dialFault "s-dial-base-restore-refused"
          "a base DR write that reads back 0 refuses the dial to C with 14, restores CUSTOM 1, retains its effective selection, and changes neither C nor the card"
          [1, 0xa0, dialOp DialC] [] faultModel [0, 0, 14]
          ([("live-state", 0, liveA), ("card", 0, cardWith (selGen 9 1) (selGen 8 0))]
           ++ dialObserved False 2 0x24 1 2 2 1)
         , dialFault "s-dial-base-and-rollback-refused"
          "failed base and snapshot DR writes refuse the dial to C with 14 and hide the active-bank label; mode and C routing stay in M and the card is unchanged"
          [1, 0xa1, dialOp DialC, dialOp DialAuto] [] faultModel [0, 0, 14, 14]
          ([("live-state", 0, placeLive liveA (fieldOf 0x104) 0),
            ("card", 0, cardWith (selGen 9 1) (selGen 8 0))]
           ++ dialObserved False 2 0x24 1 0 2 0xffffffff
           ++ dialObserved False 3 0x24 1 0 2 0xffffffff)
         , dialFault "s-dial-resume-and-rollback-refused"
          "failed bank resume and ordinary rollback keep the label suspended; another dial and selection cannot route or checkpoint residual DR as an ordinary base"
          [1, dialOp DialAuto, 0xa1, dialOp DialM, dialOp DialP, 1] [] faultModel [0, 0, 0, 14, 14, 16]
          ([("live-state", 0, placeLive admittedLive (fieldOf 0x104) 0),
            ("card", 0, cardWith (selGen 9 1) (selGen 8 0))]
           ++ dialObserved False 1 0 0 1 1 0xffffffff
           ++ dialObserved False 3 0x24 0 0 1 0xffffffff
           ++ dialObserved False 4 0x24 0 0 1 0xffffffff
           ++ dialObserved False 5 0x24 0 0 1 0xffffffff)
         , dialSelects "s-dial-excursion-refresh-none"
          "an ordinary sharpness edit in AUTO is checkpointed as the refreshed base on return to M; CUSTOM 1 resumes, then NONE restores that edited ordinary base"
          [1, dialOp DialAuto, 0x90, dialOp DialM, 0] [] selModel [0, 0, 0, 0, 0]
          ([("live-state", 0, refreshedLive),
            ("casebuf", 0x200 + 28 * 2 + 16, u32le 2),
            ("casebuf", 0x600 + 28 * 3 + 16, u32le 2),
            ("casebuf", 0x200 + 28 * 3 + 16, u32le 3),
            ("casebuf", 0x200 + 28 * 4 + 16, u32le 2),
            ("card", 0, cardWith (refreshedGen 11 0) (refreshedGen 10 1))]
           ++ dialObserved False 1 0 0 1 1 0xffffffff
           ++ dialObserved False 3 0x24 0 2 2 1
           ++ dialObserved False 4 0x24 0 1 1 0)
         ]
      ++ map wrapperRefusal [0 .. 2 :: Int]
      ++ [ dialSelects "s-dial-raw-base-size-restored"
          "a RAW-only ordinary base regains its size/aspect bytes on AUTO entry without replacing RAW-only processing cells; resume and NONE preserve that base on disk"
          [1, dialOp DialAuto, dialOp DialM, 0] [] rawDialModel [0, 0, 0, 0]
          ([("live-state", 0, rawBaseLive), ("cells", 0x50, u32le rawStockSize),
            ("cells", 0x60, u32le rawStockAspect),
            ("card", 0, cardWith (rawDialGen 9 1) (rawDialGen 10 0))]
           ++ dialObserved False 1 0 0 1 1 0xffffffff
           ++ dialObserved False 3 0x24 0 1 1 0)
         ]
      ++ [ dialSelects ("s-dial-pending-cross-" ++ dialName from ++ "-" ++ dialName to)
          "a failed selection write followed by a different eligible arrival removes the old overlay under destination admission before resuming CUSTOM 1; no old-mode snapshot is replayed and C stays unchanged"
          [1, 2, dialOp to] raceToC (pendingModel (fst (dialCell from))) [0, 16, 0]
          ([("live-state", 0, placeLive liveA (fieldOf 0x1c6) 0x7f),
            ("card", 0, if from == DialM
              then cardWith (encodeFileV2 9 (selBank { bSelection = 1, bBase = BaseValid CaptureM (payloadOfLive (mLive (pendingModel 0x24 []))) }))
                   (encodeFileV2 8 (selBank { bSelection = 0, bBase = BaseValid CaptureM (payloadOfLive (mLive (pendingModel 0x24 []))) }))
              else cardWith (selGenAt 0x21 9 1) (selGenAt 0x21 8 0))]
           ++ dialObserved False 2 (fst (dialCell to)) 0 2 2 1)
         | (from, to) <- [(DialM, DialP), (DialP, DialM)] ]
      ++ [ dialSelects ("s-dial-pending-media-" ++ name)
          "media replacement after a failed selection write removes the old overlay on eligible return but does not replay or activate the old-card bank snapshot"
          [1, 2, mediaOp, dialOp DialM] raceToC selModel [0, 16, 0, 13]
          ([("live-state", 0, admittedLive),
            ("card", 0, cardWith (if mediaOp == 0xb0 then selGen 9 1 else B.empty) (selGen 8 0))]
           ++ dialObserved False 3 0x24 0 1 1 0xffffffff)
         | (name, mediaOp) <- [("remove", 0xb0), ("replace", 0xb1)] ]
      ++ [ dialFault ("s-dial-pending-cleanup-refused-" ++ dialName from ++ "-" ++ dialName to)
          "failed cross-mode base cleanup keeps the selection suspended and blocks a fresh selection before loading or writing a bank"
          [1, 2, 0xa1, dialOp to, 1] raceToC (atMode (fst (dialCell from)) faultModel) [0, 16, 0, 14, 16]
          ([("card", 0, cardWith (selGenAt (fst (dialCell from)) 9 1) (selGenAt (fst (dialCell from)) 8 0))]
           ++ dialObserved False 3 (fst (dialCell to)) 0 0 1 0xffffffff)
         | (from, to) <- [(DialM, DialP), (DialP, DialM)] ]
      ++ [ dialSelects ("s-dial-leave-" ++ name)
             ("with CUSTOM 1 active in M, the dial to " ++ name ++ " (mode cell " ++ hex (toInteger m) ++ ", class " ++ show c ++ ") puts back the persisted base before stock routes the new mode: every live byte, the C record and the inactive modes' family memories included, equals the fixture, and no file is written")
             [1, dialOp d] [] selModel [0, 0]
             ([ ("live-state", 0, admittedLive), ("cells", 0, u32le m), ("card", 0, cardWith (selGen 9 1) (selGen 8 0)) ]
              ++ [ ("media-state", 0x41424, u32le c) | c /= 0 ])
         | d <- excludedDials, let name = dialName d, let (m, c) = dialCell d ]
      ++ [ dialSelects ("s-dial-excursion-" ++ name)
             ("with CUSTOM 1 active in M, a dial excursion to " ++ name ++ " and back to M ends with CUSTOM 1 resumed without a write: live bytes are CUSTOM 1's, reselecting CUSTOM 1 is a no-op, and the card holds the checkpoint and the selection only")
             [1, dialOp d, dialOp DialM, 1] [] selModel [0, 0, 0, 0]
             ([ ("live-state", 0, liveA), ("card", 0, cardWith (selGen 9 1) (selGen 8 0)) ]
              ++ dialObserved True 1 0x24 1 1 1 0xffffffff
              ++ dialObserved False 1 (fst (dialCell d)) (snd (dialCell d))
                   (if d == DialC then 0 else 1) (if d == DialC then 0 else 1) 0xffffffff
              ++ dialObserved False 2 0x24 0 2 2 1)
         | d <- excludedDials, let name = dialName d ]
      ++ [ dialSelects ("s-dial-pair-" ++ dialName from ++ "-" ++ dialName to)
             ("with CUSTOM 1 selected in " ++ dialName from ++ " over the base captured there, the dial to " ++ dialName to ++ " removes and re-applies CUSTOM 1 around the stock mode change without a write: live bytes are CUSTOM 1's, the mode cell reads " ++ hex (toInteger (fst (dialCell to))) ++ ", reselecting CUSTOM 1 is a no-op, and the card holds the checkpoint and the selection only")
             [1, dialOp to, 1] [] (atMode (fst (dialCell from)) selModel) [0, 0, 0]
             ([ ("live-state", 0, liveA), ("cells", 0, u32le (fst (dialCell to)))
             , ("card", 0, cardWith (selGenAt (fst (dialCell from)) 9 1) (selGenAt (fst (dialCell from)) 8 0)) ]
             ++ dialObserved True 1 (fst (dialCell from)) 1 1 1 0xffffffff
             ++ dialObserved False 1 (fst (dialCell to)) 0 2 2 1)
         | from <- psamDials, to <- psamDials, from /= to ]
      ++ [ selects "s-select-write-fails-restores"
          "with CUSTOM 1 active, a full card (write completion 3) on CUSTOM 2's selection write returns 3 after CUSTOM 2 was applied and read back: the kernel re-applies the live codes read before the transaction, so every live byte and the IMAGE SIZE cells are CUSTOM 1's again, and the card keeps generation 9 (CUSTOM 1) in A and the checkpoint in B"
          [1, 2] [("rbm_fail_write_at", 3), ("rbm_fail_write_code", 3)] selModel [0, 3]
          [ ("live-state", 0, liveA), ("cells", 0x50, u32le 0), ("cells", 0x60, u32le 2)
          , ("card", 0, cardWith (selGen 9 1) (selGen 8 0)) ]
      , selects "s-select-readback-fails-restores"
          "with CUSTOM 1 active, CUSTOM 2's selection write lands with its last byte complemented, so its readback refuses with 3 (full): the live bytes are CUSTOM 1's again, A keeps generation 9 and B holds the corrupt bytes, which no load accepts"
          [1, 2] [("rbm_corrupt_write_at", 3)] selModel [0, 3]
          [ ("live-state", 0, liveA)
          , ("card", 0, cardWith (selGen 9 1) (B.take 1023 (selGen 10 2) <> B.singleton 0xff)) ]
      , selectsFilmPinned "s-select-live-readback-fails-restores"
          ("with FUN_00640e78 modeled to write the live film code " ++ show liveFilm ++ " back and CUSTOM 1 active, CUSTOM 2 stores film 0, so its assert pass refuses 0xfe: the selection answers 14, every live byte is CUSTOM 1's again with no field left unrestored, no selection is written, and rb_select_trace records status 14, selection 2 and property 0xfe")
          [1, 2] [] (\cells -> (selModel cells) { mCard = cardWith (encodeFileV2 7 (selBankOf selBPin)) B.empty, mExtra = withFilmPin (mExtra (selModel cells)) }) [0, 14]
          [ ("live-state", 0, liveA), traceWord 2 14, traceWord 3 2, traceWord 4 0xfe, traceWord 9 0
          , ("card", 0, cardWith (selGenOf (selBankOf selBPin) 9 1) (selGenOf (selBankOf selBPin) 8 0)) ]
      , selects "s-select-media-change-checkpoint"
          "a card replacement at the base checkpoint's write returns 13 (media changed) before any live write: the live bytes are unchanged and the replacement card's files stay as they were"
          [1] [swapBoundaryCell BeforeWrite, ("rbm_swap_a_size", 1024)]
          (\cells -> (selModel cells) { mCard = fileSlot selFile <> fileSlot B.empty <> fileSlot replacementV2 }) [13]
          [ ("live-state", 0, admittedLive), ("card", 0, fileSlot replacementV2 <> fileSlot B.empty <> fileSlot replacementV2) ]
      , selects "s-select-media-change-restores"
          "with CUSTOM 1 active, a card replacement at CUSTOM 2's selection write returns 13 after CUSTOM 2 was applied: the live bytes are CUSTOM 1's again and nothing is written to the replacement card"
          [1, 2] [("rbm_swap_write_at", 3), ("rbm_swap_a_size", 1024)]
          (\cells -> (selModel cells) { mCard = fileSlot selFile <> fileSlot B.empty <> fileSlot replacementV2 }) [0, 13]
          [ ("live-state", 0, liveA), ("card", 0, fileSlot replacementV2 <> fileSlot (selGen 8 0) <> fileSlot replacementV2) ]
      , selects "s-select-mode-race-checkpoint"
          "the shooting mode moving to S during the base checkpoint's write returns 16 (mode changed) before any live write: the live bytes are unchanged and B holds the checkpoint with the selection still NOTHING SELECTED"
          [1] [("rbm_mode_at_write", 1), ("rbm_mode_race_value", 0x22)] selModel [16]
          [ ("live-state", 0, admittedLive), ("cells", 0, u32le 0x22), ("card", 0, cardWith selFile (selGen 8 0)) ]
      , selects "s-select-mode-race-load"
          "with CUSTOM 1 active, the shooting mode moving to S during CUSTOM 2's bank load returns 16 before any live write: the live bytes stay CUSTOM 1's and neither file changes"
          [1, 2] [("rbm_mode_at_stat", 7), ("rbm_mode_race_value", 0x22)] selModel [0, 16]
          [ ("live-state", 0, liveA), ("card", 0, cardWith (selGen 9 1) (selGen 8 0)) ]
      , Spec "s-reset-selected-refused"
          "contract: after selecting CUSTOM 1, resetting slot 0 is refused with 17 (the slot is the persisted selection): the card keeps the selection and the live bytes stay CUSTOM 1's"
          "rbm_seq_selects_reset" [0x009a0000, 1, 0, 0x009a0100]
          ((selModel []) { mCase = B.pack [1] })
          (Expect "returned" 17 [statusesClaim [0], ("live-state", 0, liveA), ("card", 0, cardWith (selGen 9 1) (selGen 8 0))])
          ("rbm_seq_selects_reset" : "rbm_seq_selects" : "rb_select" : "rb_reset" : selFns) realStubs
      , selects "s-select-suspended-to-other"
          "a bank persisted with CUSTOM 1 selected over a base with DR 2 and IMAGE SIZE row 2 loads suspended: selecting CUSTOM 2 applies the persisted base and then CUSTOM 2, whose RAW-only quality skips its IMAGE SIZE pair, so live 0xf9 holds the base's 2 rather than the live 1, live DR holds CUSTOM 2's stored code, and B holds generation 8 selecting CUSTOM 2"
          [2] [] (\cells -> (selModel cells) { mCard = cardWith suspendedFile B.empty }) [0]
          [ ("live-state", 0, rawOnly placeLive (placeLive (placeLive admittedLive (fieldOf 0xf9) 2) (fieldOf 0x10f) 2))
          , ("card", 0, cardWith suspendedFile (encodeFileV2 8 (suspendedBank { bSelection = 2 }))) ]
      , selects "s-select-none-from-suspended"
          "a bank persisted with CUSTOM 1 selected over a base with DR 2 and IMAGE SIZE row 2 loads suspended: NOTHING SELECTED restores the persisted base, so live DR and 0xf9 read 2, and B holds generation 8 selecting NOTHING SELECTED"
          [0] [] (\cells -> (selModel cells) { mCard = cardWith suspendedFile B.empty }) [0]
          [ ("live-state", 0, placeLive (placeLive admittedLive (fieldOf 0x104) 2) (fieldOf 0xf9) 2)
          , ("card", 0, cardWith suspendedFile (encodeFileV2 8 (suspendedBank { bSelection = 0 }))) ]
      , rateSelected 1
          "with drive row 1 (DAT_00956844 = 1, live and stored 0x1b1 = 1), selecting CUSTOM 2 storing 0x214 = 7 over live 6 commits through the 0x214 arm, whose FUN_00646af0 writes 0x214 at row 1: live 0x214 reads 7, every other live byte, drive row 3's 0x216 and row 7's 0x215 included, is unchanged, and the card holds the checkpoint (generation 8 in B) and the selection (generation 9 in A)"
          []
      , rateSelected 3
          "with drive row 3 (DAT_00956844 = 3, live and stored 0x1b1 = 3), where the 0x214 arm's FUN_00646af0 writes 0x216 and not 0x214, selecting CUSTOM 2 storing 0x214 = 7 over live 6 writes 0x214 through 0x0061c91c: live 0x214 reads 7, every other live byte, row 3's 0x216 included, is unchanged, the displayed-rate cell DAT_00956b30 keeps 0, and the card holds the checkpoint (generation 8 in B) and the selection (generation 9 in A)"
          [("cells", 0x00956b30 - 0x00956838, u32le 0)]
      ]
    -- drive key k is 0x1b1 code k for k 1 and 3 (table 0x77a54c, column 3);
    -- the 0x1b1 arm sets DAT_00956844 to the key, so the slot and live both
    -- carry the drive the cell holds
    rateSelected key what claims = selects ("s-select-rate-drive-" ++ show key) what [2] [] model' [0]
      ([("live-state", 0, placeLive live (fieldOf 0x214) 7)] ++ claims ++ [("card", 0, cardWith (gen 9 2) (gen 8 0))])
      where
        live = placeLive admittedLive (fieldOf 0x1b1) key
        bank = selBankOf (placePayload (placePayload admittedPayload (fieldOf 0x1b1) key) (fieldOf 0x214) 7)
        gen g sel = encodeFileV2 g (bank { bSelection = sel, bBase = BaseValid CaptureM (payloadOfLive live) })
        model' cells = (selModel cells)
          { mCard = cardWith (encodeFileV2 7 bank) B.empty
          , mLive = live
          , mExtra = [ (n, a, if n == "cells" then pokeAt (0x00956844 - 0x00956838) (u32le key) b else b) | (n, a, b) <- mExtra (selModel cells) ] }
    selBPin = placePayload selB (fieldOf 0xfe) 0
    replacementV2 = encodeFileV2 11 (bankOf [Populated (name26 "REPLACED") admittedPayload admittedPayload])
    suspendedBank = selBank { bSelection = 1, bBase = BaseValid CaptureM (placePayload (placePayload (payloadOfLive admittedLive) (fieldOf 0x104) 2) (fieldOf 0xf9) 2) }
    suspendedFile = encodeFileV2 7 suspendedBank
    v2RenameKeepsSelection = Spec "v2-rename-keeps-selection"
      "contract: renaming slot 1 of a v2 bank that selects CUSTOM 1 over a base captured in P writes generation 8 to B with the name changed and the selection, auto-update policy and base unchanged"
      "rb_name_commit" [1, 0x009a0000] ((model (fileA2 [])) { mCard = cardWith (encodeFileV2 7 selectedBank) B.empty, mLive = extractLive, mCase = name26 "DELTA" })
      (Expect "returned" 0 [("card", 0, cardWith (encodeFileV2 7 selectedBank) (encodeFileV2 8 (withRecord 1 (Populated (name26 "DELTA") validPayload validPayload) selectedBank)))])
      ("rb_name_commit" : "rb_stage_image" : "rb_write_back" : loadFns) bankStubs
    enableBank = BankV2 0 1 BaseInvalid [Populated (name26 "ALPHA") validPayload validPayload]
    v2EnableEditSaves = Spec "v2-enable-edit-saves"
      "contract: under ENABLE a staged field edit (DR to code 2) followed by OK writes generation 8 to B with the edit in both the saved and the working payload"
      "rbm_seq_stage_field_commit" [0, 0x104, 2, 0x009a0000] ((model (fileA2 [])) { mCard = cardWith (encodeFileV2 7 enableBank) B.empty, mLive = extractLive })
      (Expect "returned" 0 [("card", 0, cardWith (encodeFileV2 7 enableBank) (encodeFileV2 8 (enableBank { bRecords = [Populated (name26 "") editedPayload editedPayload] })))])
      ("rbm_seq_stage_field_commit" : "rb_stage_load" : "rb_stage_field" : "rb_stage_commit" : "rb_stage_image" : "rb_write_back" : loadFns) bankStubs
    v2ReadError = Spec "v2-read-error-preserves"
      "contract: a read completion 4 during discovery fails a save over a v1 A and a newer v2 B with 4 (IO) before any write: both files and the live settings are unchanged"
      "rb_save_current" [0, 0] ((model (("rbm_read_comp", 4) : fileA (fileB2 []))) { mCard = cardWith validA (v2One 8 "BRAVO"), mLive = extractLive })
      (Expect "returned" 4 [("card", 0, cardWith validA (v2One 8 "BRAVO")), ("live-state", 0, extractLive)])
      saveFns bankStubs
    v2FailedReload cid fault value expected = Spec cid
      "contract: after loading the mixed-version winner, a failed discovery preserves its public resident name and both files"
      "0x009b0000" []
      ((model (fileA (fileB2 [])))
        { mCard = cardWith validA (v2One 8 "BRAVO")
        , mExtra = ("prefix", 0x009b0000, workload) : real
        })
      (Expect "returned" 0
        [ ("casebuf", 0, u32le expected)
        , ("casebuf", 0x20, name26 "BRAVO")
        , ("card", 0, cardWith validA (v2One 8 "BRAVO")) ])
      ("0x009b0000/64" : "rb_name_resident" : loadFns) bankStubs
      where
        workload = B.concat (map u32le
          [ 0xe92d4010, 0xe59fc034, 0xe12fff3c, 0xe59f1030
          , 0xe3a00000 .|. value, 0xe5810000, 0xe59fc020, 0xe12fff3c
          , 0xe1a04000, 0xe59f101c, 0xe5814000, 0xe2811020
          , 0xe3a00000, 0xe59fc010, 0xe12fff3c, 0xe8bd8010
          , fromIntegral (addr ks "rb_load"), fromIntegral (addr ms fault)
          , 0x009a0000, fromIntegral (addr ks "rb_name_resident") ])
    v1TornLoad n = Spec ("v1-torn-load-" ++ show n)
      ("contract: a complete v2 A wins over a newer v1 B truncated to " ++ show n ++ " bytes; public readback and both files remain unchanged")
      "rbm_seq_load_readback" [0, 0x009a0000, 0x009a0020]
      ((model (fileA2 [("rbm_b_present", 1), ("rbm_b_size", n)])) { mCard = cardWith (v2One 7 "ALPHA") (B.take (fromIntegral n) (v1One 8 "BRAVO")) })
      (Expect "returned" 0 (readbackOf (name26 "ALPHA") validPayload ++ [("card", 0, cardWith (v2One 7 "ALPHA") (B.take (fromIntegral n) (v1One 8 "BRAVO")))]))
      ("rbm_seq_load_readback" : "rb_readback" : loadFns) bankStubs
    v2TornLoad n = Spec ("v2-torn-load-" ++ show n)
      ("contract: a v1 A at generation 7 and a B holding the first " ++ show n ++ " bytes of a v2 generation 8 (a torn write of that length): B is not a bank file, so readback of slot 0 returns A's ALPHA and neither file changes")
      "rbm_seq_load_readback" [0, 0x009a0000, 0x009a0020]
      ((model (fileA [("rbm_b_present", 1), ("rbm_b_size", n)])) { mCard = cardWith validA (B.take (fromIntegral n) (v2One 8 "BRAVO")) })
      (Expect "returned" 0 (readbackOf (name26 "ALPHA") validPayload ++ [("card", 0, cardWith validA (B.take (fromIntegral n) (v2One 8 "BRAVO")))]))
      ("rbm_seq_load_readback" : "rb_readback" : loadFns) bankStubs
    v2TornWrite n = Spec ("v2-torn-write-" ++ show n)
      ("contract: a migrating save whose write stops after " ++ show n ++ " bytes returns 3 (full), leaves exactly the first " ++ show n ++ " bytes of the new v2 generation at B and the v1 winner A byte-identical")
      "rb_save_current" [0, 0] ((model ((if n == 0 then ("rbm_write_comp", 3) else ("rbm_write_short", n)) : fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "returned" 3 [("card", 0, cardWith validA (B.take (fromIntegral n) saveFile))])
      saveFns bankStubs

    -- No card, volume or file entry is admitted by the volatile suite.
    volKernelSpan = "0x" ++ showHex (addr ks "__rb_image_start") "" ++ "/"
      ++ show (addr ks "__rb_text_end" - addr ks "__rb_image_start")
    volStubs = filter (`notElem` ["card", "vol"]) realStubs
    volPlain = Model emptyCard admittedLive M.empty 1 0x24 B.empty real []
    volModel w names = volPlain
      { mCase = names, mExtra = ("prefix", 0x009b0000, w) : real }
    volStockFns = "0x0061d514/16" : "0x00631a00/336" : realStockFns
    volFns w = ("0x009b0000/" ++ show (B.length w)) : volKernelSpan : volStockFns
    volStatus k v = ("casebuf", 0x200 + 4 * k, u32le v)
    volUnchanged = [("card", 0, emptyCard), ("live-state", 0, admittedLive)]
    volReadbackW slot = volWorkload [(addr ks "rb_readback", [slot, 0x009a0000, 0x009a0020, 0])]
    volFresh slot = Spec ("vol-fresh-empty-readback-slot-" ++ show slot)
      ("contract: an independent volatile kernel holding no bank answers public readback of unset slot " ++ show slot ++ " with OK and zeros for its name and payload; the card region and the live settings are unchanged")
      "0x009b0000" []
      (volModel (volReadbackW slot) B.empty)
      (Expect "returned" 0
        (readbackOf (B.replicate 26 0) (B.replicate 0x28 0) ++ [volStatus 0 0] ++ volUnchanged))
      (volFns (volReadbackW slot)) volStubs
    volSaveW = volWorkload
      [ (addr ks "rb_save_current", [1, 0])
      , (addr ks "rb_load", [])
      , (addr ks "rb_readback", [1, 0x009a0000, 0x009a0020, 0]) ]
    volSaveReload = Spec "vol-save-current-reload-readback"
      "contract: on a fresh no-card kernel Save Current of slot 1 answers OK, the reload after it answers OK, and public readback returns an empty name with the saved payload (the live settings as the stock getter walks them); the card region and the live settings are unchanged"
      "0x009b0000" []
      (volModel volSaveW B.empty)
      (Expect "returned" 0
        (readbackOf (B.replicate 26 0) admittedPayload
          ++ [volStatus 0 0, volStatus 1 0, volStatus 2 0] ++ volUnchanged))
      (volFns volSaveW) volStubs
    volEdited = placePayload admittedPayload (fieldOf 0x104) 2
    volStageW = volWorkload
      [ (addr ks "rb_save_current", [1, 0])
      , (addr ks "rb_stage_load", [1, 0])
      , (addr ks "rb_stage_field", [0x104, 2])
      , (addr ks "rb_stage_commit", [1, 0x009a0100])
      , (addr ks "rb_readback", [1, 0x009a0000, 0x009a0020, 0])
      , (addr ks "rb_stage_load", [1, 0])
      , (addr ks "rb_stage_read", [0x104]) ]
    volStagedReentry = Spec "vol-staged-edit-reentry"
      "contract: after Save Current of slot 1, a staged DR edit committed as BRAVO answers OK; readback in a new visit returns that name and the edited working payload, and a second staged visit reads the edit back; the live settings and the card region are unchanged"
      "0x009b0000" []
      (volModel volStageW (B.replicate 0x100 0 <> name26 "BRAVO"))
      (Expect "returned" 2
        (readbackOf (name26 "BRAVO") volEdited
          ++ [volStatus 0 0, volStatus 1 0, volStatus 2 0, volStatus 3 0, volStatus 4 0, volStatus 5 0, volStatus 6 2]
          ++ volUnchanged))
      (volFns volStageW) volStubs
    volC1W = volWorkload
      [ (addr ks "rb_save_current", [0, 0])
      , (addr ks "rb_stage_load", [0, 0])
      , (addr ks "rb_stage_field", [0x104, 2])
      , (addr ks "rb_stage_commit", [0, 0x009a0100])
      , (addr ks "rb_select", [1, 0]) ]
    volNoneW = volWorkload
      [ (addr ks "rb_save_current", [0, 0])
      , (addr ks "rb_stage_load", [0, 0])
      , (addr ks "rb_stage_field", [0x104, 2])
      , (addr ks "rb_stage_commit", [0, 0x009a0100])
      , (addr ks "rb_select", [1, 0])
      , (addr ks "rb_select", [0, 0]) ]
    volSelectC1 = Spec "vol-select-c1-applies-working"
      "contract: in M over a freshly saved slot 0 whose working payload carries DR 2, selecting CUSTOM 1 answers OK and applies the working payload: live DR reads 2 and every other live byte is unchanged, and the card region stays untouched"
      "0x009b0000" []
      (volModel volC1W (B.replicate 0x100 0 <> name26 "ALPHA"))
      (Expect "returned" 0
        ([volStatus 0 0, volStatus 1 0, volStatus 2 0, volStatus 3 0, volStatus 4 0]
          ++ [("live-state", 0, placeLive admittedLive (fieldOf 0x104) 2), ("card", 0, emptyCard)]))
      (volFns volC1W) volStubs
    volSelectNone = Spec "vol-select-none-restores-base"
      "contract: in M, selecting CUSTOM 1 over the saved slot then NOTHING SELECTED answers OK twice and restores the ordinary base captured at the selection: every live byte reads the pre-selection fixture again and the card region stays untouched"
      "0x009b0000" []
      (volModel volNoneW (B.replicate 0x100 0 <> name26 "ALPHA"))
      (Expect "returned" 0
        ([volStatus 0 0, volStatus 1 0, volStatus 2 0, volStatus 3 0, volStatus 4 0, volStatus 5 0]
          ++ volUnchanged))
      (volFns volNoneW) volStubs
    volSelectW = volWorkload [(addr ks "rb_select", [1, 0])]
    volSelectAtC = Spec "vol-select-at-c-refused"
      "contract: at the C dial position the selection returns RB_AT_C; live settings and card bytes are unchanged"
      "0x009b0000" []
      ((volModel volSelectW B.empty) { mMode = 3 })
      (Expect "returned" 8 ([volStatus 0 8] ++ volUnchanged))
      (volFns volSelectW) volStubs
    volSelectUnset = Spec "vol-select-unset-refused"
      "contract: an empty volatile bank refuses CUSTOM 1 with 6 (unset slot) and starts no transaction; the card region and the live settings are unchanged"
      "0x009b0000" []
      (volModel volSelectW B.empty)
      (Expect "returned" 6 ([volStatus 0 6] ++ volUnchanged))
      (volFns volSelectW) volStubs
    volStaleW = volWorkload
      [ (addr ks "rb_stage_load", [0, 0])
      , (addr ks "rb_media_epoch_bump", [])
      , (addr ks "rb_stage_commit", [0, 0x009a0100])
      , (addr ks "rb_readback", [0, 0x009a0000, 0x009a0020, 1]) ]
    volStaleSession = Spec "vol-stale-session-refused"
      "contract: a staged visit opened before a media epoch change refuses its commit with 13 (media changed) and publishes nothing; the card region and the live settings are unchanged"
      "0x009b0000" []
      (volModel volStaleW (B.replicate 0x100 0 <> name26 "ALPHA"))
      (Expect "returned" 0
        (readbackOf (B.replicate 26 0) (B.replicate 0x28 0)
          ++ [volStatus 0 0, volStatus 2 13, volStatus 3 0] ++ volUnchanged))
      (volFns volStaleW) volStubs
    volRangeW = volWorkload
      [ (addr ks "rb_stage_load", [0, 0])
      , (addr ks "rb_stage_field", [0x141, 0x1f]) ]
    volInvalidStageCode = Spec "vol-invalid-stage-code-refused"
      "contract: staging unoffered ISO code 0x1f returns RB_RANGE; live settings and card bytes are unchanged"
      "0x009b0000" []
      (volModel volRangeW B.empty)
      (Expect "returned" 7 ([volStatus 0 0, volStatus 1 7] ++ [("live-state", 0, admittedLive), ("card", 0, emptyCard)]))
      (volFns volRangeW) volStubs
    volatileBuilders =
      [ volFresh 0, volFresh 6, volSaveReload, volStagedReentry
      , volSelectC1, volSelectNone, volSelectAtC, volSelectUnset
      , volStaleSession, volInvalidStageCode ]

runSpec :: BankMode -> FilePath -> FilePath -> FilePath -> Syms -> Syms -> B.ByteString -> Int -> Int -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Integer -> Spec -> IO Value
runSpec bankMode build out runner ks ms kbin ktextLen kdataLen mdataRaw fTable getter setter mediaImg tabImg mediaLo sp = do
  let dir = out </> spId sp
  createDirectoryIfMissing True dir
  let modeCell = B.replicate 0x24 0 <> u32le (mMode (spModel sp))
      mdata = overlay mdataRaw ([ (addr ms n - 0x970000, u32le v) | (n, v) <- M.toList (mCells (spModel sp))]
        ++ [(addr ms "rbm_field_count" - 0x970000, u32le (fromIntegral (B.length fTable `div` 20)))])
      mdata' = overlay (mdata <> B.replicate 0x1000 0) [(0x800, fTable)]
      overlay base ps = foldl (\b (o, v) -> B.take (fromIntegral o) b <> v <> B.drop (fromIntegral o + B.length v) b) base ps
      cardImg = mCard (spModel sp)
      cardLength = max cardBytes (B.length cardImg)
      liveImg = mLive (spModel sp)
      caseImg = mCase (spModel sp)
      extras = mExtra (spModel sp)
      patchMode b = B.take 0x41424 b <> u32le (mMode (spModel sp)) <> B.drop 0x41428 b
      patchCells b = u32le (mShootingMode (spModel sp)) <> B.drop 4 b
      extrasPatch = [(rid, a, if rid == "media-state" then patchMode b else if rid == "cells" then patchCells b else b) | (rid, a, b) <- extras]
      exitAddress = mediaLo + fromIntegral (B.length mediaImg)
      mediaImg' = overlay mediaImg [(a - mediaLo, bytes) | (GuestAddress a, bytes) <- mForeignCode (spModel sp)] <> u32le 0xe12fff1e
      arts = [("kernel.bin", kbin), ("media.bin", mediaImg'), ("getter.bin", getter), ("setter.bin", setter), ("model-data.bin", B.take 0x1000 mdata'), ("tables.bin", tabImg), ("card.bin", cardImg), ("live.bin", liveImg)]
        ++ (if B.null caseImg then [] else [("case.bin", caseImg)])
        ++ [(rid ++ ".bin", b) | (rid, _, b) <- extrasPatch, not (B.null b)]
  forM_ (arts ++ [("mode.bin", modeCell)]) $ \(n, b) -> B.writeFile (dir </> n) b
  let digests = M.fromList [(n, sha256Hex (sha256 b)) | (n, b) <- arts ++ [("mode.bin", modeCell)]]
  let reg :: String -> Integer -> Int -> String -> Maybe String -> Int -> Int -> Value
      kernelBase = addr ks "__rb_image_start"
      reg rid a sz perm art off len = object ["id" .= rid, "address" .= hex a, "size" .= (toInteger sz :: Integer), "permissions" .= (perm :: String),
        "initial" .= (if isJust art then object ["artifact" .= (takeWhile (/= '.') (fromJust art) :: String), "offset" .= (toInteger off :: Integer), "length" .= (toInteger len :: Integer)] else object ["zero" .= True])]
      regions =
        [ reg "ktext" kernelBase ktextLen "rx" (Just "kernel.bin") 0 ktextLen
        , reg "kdata" (kernelBase + fromIntegral ktextLen) kdataLen "rw" (Just "kernel.bin") ktextLen kdataLen
        , reg "media" mediaLo (B.length mediaImg') "rx" (Just "media.bin") 0 (B.length mediaImg')
        , reg "model-data" 0x00970000 0x1000 "rw" (Just "model-data.bin") 0 0x1000
        , reg "live-state" 0x0096cc64 0x1c2 "rw" (Just "live.bin") 0 0x1c2
        , reg "card" 0x00980000 cardLength "rw" (Just "card.bin") 0 (B.length cardImg)
        , reg "casebuf" 0x009a0000 0x2000 "rw" (if B.null caseImg then Nothing else Just "case.bin") 0 (B.length caseImg) ]
        ++ [reg rid a (max 4 (B.length b)) (extraPerms rid) (if B.null b then Nothing else Just (rid ++ ".bin")) 0 (B.length b) | (rid, a, b) <- extrasPatch]
      extraPerms rid
        | rid `elem` ["chain", "dispatcher", "conv-tables", "prefix"] = "rx"
        | rid `elem` ["conv-ptrs", "modetab"] = "r"
        | otherwise = "rw"
      -- Inlined helpers have no executable span.
      fns = map fnEntry (nub [n | n <- spFns sp, take 2 n == "0x" || hasK n || hasM n])
      fnEntry n
        | take 2 n == "0x" = case break (== '/') n of
            (a, '/' : s) -> object ["address" .= hex (hexI (drop 2 a)), "size" .= (read s :: Integer)]
            _ -> error ("raw function entry needs 0xADDR/SIZE: " ++ n)
        | otherwise = object ["address" .= hex (addrOf n), "size" .= (toInteger (sizeOf' n) :: Integer)]
      hasK n = case ks of Syms m -> M.member n m
      hasM n = case ms of Syms m -> M.member n m
      addrOf n = if hasK n then addr ks n else addr ms n
      sizeOf' n = if hasK n then sizeOf ks n else sizeOf ms n
      stubs = [let (a, i, o) = stubDef n (stubVal n) in object ["address" .= a, "inputs" .= bindObj i, "outputs" .= bindObj o, "side_effects" .= ("registers-only" :: String)] | n <- spStubs sp]
      stubVal n = fromMaybe 0 (lookup n (stubsOut n))
      stubsOut n = case n of
        "task" -> [("task", 3)]; "card" -> [("card", 0)]; "vol" -> [("vol", 0x43)]
        "live" -> [("live", 1)]; "store" -> [("store", 0)]; _ -> []
      bindObj bs = object [fromString k .= hex (toInteger v) | (k, v) <- bs]
      regs = object (["lr" .= hex exitAddress, "sp" .= hex 0x009a1ff0, "cpsr" .= hex 0x10]
        ++ [fromString ("r" ++ show i) .= hex (toInteger v) | (i, v) <- take 13 (zip [0 :: Int ..] (spArgs sp ++ repeat 0))])
      expected = object
        [ "status" .= eStatus (spExpect sp)
        , "registers" .= object ["r0" .= hex (fromIntegral (eR0 (spExpect sp)))]
        , "memory" .= claimsJson
        , "events" .= ([] :: [Value]) ]
      claimsJson = [object ["region" .= rid, "offset" .= (toInteger off :: Integer), "hex" .= hexB bs] | (rid, off, bs) <- eClaims (spExpect sp)]
      caseObj = object
        [ "schema_version" .= (1 :: Int), "id" .= spId sp, "origin" .= ("synthetic" :: String)
        , "provenance" .= spProv sp
        , "cpu" .= object ["architecture" .= ("arm" :: String), "model" .= ("cortex-r5f" :: String), "mode" .= ("arm" :: String), "endian" .= ("little" :: String)]
        , "artifacts" .= [object ["id" .= takeWhile (/= '.') n, "path" .= n, "size" .= (toInteger (B.length b) :: Integer), "sha256" .= M.findWithDefault "" n digests] | (n, b) <- arts ++ [("mode.bin", modeCell)]]
        , "entry" .= hex (addrOfEntry), "exit" .= hex exitAddress
        , "registers" .= regs
        , "regions" .= regions
        , "functions" .= fns, "stubs" .= stubs
        , "limits" .= object ["instructions" .= caseInstructions, "wall_ms" .= caseWallMs]
        , "expected" .= expected
        , "exclusions" .= (["Kernel behavior is evaluated over declared public output spans, not whole-region private residue or incidental stub event order", "Foreign media completion is synchronous; stock queue, scheduler and physical SD controller are not executed"] :: [String]) ]
      -- a selection runs the getter walk, up to two applies with their asserts
      -- and two verified generations per call; 4000000 instructions and
      -- 10000 ms are the runner's maxima
      selectionEntry = "select" `isInfixOf` spEntry sp
      longEntry = selectionEntry || bankMode == VolatileBank
      caseInstructions = if longEntry then 4000000 else 1000000 :: Int
      caseWallMs = if longEntry then 10000 else 4000 :: Int
      addrOfEntry = case spEntry sp of
        e | take 2 e == "0x" -> hexI e
        e -> if e `elem` M.keys (case ks of Syms m -> m) then addr ks e else addr ms e
  BL.writeFile (dir </> "case.json") (encode caseObj)
  (_, report) <- runCase runner (dir </> "case.json")
  BL.writeFile (dir </> "runner-report.json") (encode report)
  let claimedRegions = [RegionId rid | (rid, _, _) <- eClaims (spExpect sp)]
      regionIds = M.fromList
        [(GuestAddress (hexI (str (getRegionField region "address"))), rid)
        | region <- regions, let rid = RegionId (str (getRegionField region "id")), rid `elem` claimedRegions]
      getRegionField value key = fromMaybe Null (parseMaybe (withObject "region" (.: key)) value)
      observed = memMap regionIds report
      runnerResult = parseMaybe (withObject "report" (.: "runner")) report
      runnerStatus = fromMaybe "" (runnerResult >>= parseMaybe (withObject "runner" (.: "status")) :: Maybe String)
      observedR0 = runnerResult >>= parseMaybe (withObject "runner" (\o -> o .: "registers" >>= withObject "registers" (.: "r0"))) >>= parseHexV
      mismatches =
        ["status differs" | runnerStatus /= eStatus (spExpect sp)]
        ++ ["register r0: differs" | observedR0 /= Just (toInteger (eR0 (spExpect sp)))]
        ++ ["memory " ++ rid ++ " at " ++ hex (toInteger off) ++ ": differs" |
              (rid, off, bytes) <- eClaims (spExpect sp),
              maybe True (\actual -> B.take (B.length bytes) (B.drop off actual) /= bytes)
                (M.lookup (RegionId rid) observed)]
      status = if isNothing runnerResult
        then fromMaybe "invalid_case" (parseMaybe (withObject "report" (.: "status")) report :: Maybe String)
        else if null mismatches then "passed" else "failed"
      missing = if runnerStatus == "unknown_call" then missingSym ks ms report else Nothing
      result = object
        (["id" .= spId sp, "status" .= status, "mismatches" .= mismatches,
          "runner_status" .= runnerStatus]
          ++ maybe [] (\n -> ["missing_fn" .= n]) missing)
  BL.writeFile (dir </> "behavioral-result.json") (encode result)
  pure result

-- the undeclared call target of an unknown_call failure, mapped back to its
-- symbol from the pinned kernel/model maps
missingSym :: Syms -> Syms -> Value -> Maybe String
missingSym ks ms r = do
  tr <- parseMaybe (withObject "rep" (\o -> o .: "runner" >>= withObject "rn" (\rn -> rn .: "trace"))) r
  lastE <- case reverse (case tr of Array v -> toList v; _ -> []) of (x : _) -> Just x; [] -> Nothing
  pc <- parseMaybe (withObject "e" (\o -> o .: "pc")) lastE >>= parseHexV
  ins <- parseMaybe (withObject "e" (\o -> o .: "instruction")) lastE >>= parseHexV
  if (ins .&. 0x0e000000) == 0x0a000000
    then do
      let imm = fromIntegral (ins .&. 0xffffff) :: Integer
          d = (if imm >= 0x800000 then imm - 0x1000000 else imm) * 4
          target = fromIntegral pc + 8 + d
      symAt ks ms target
    else if (ins .&. 0x0fffffd0) == 0x012fff10
      then do
        let rm = ins .&. 15
            regName = if rm == 14 then "lr" else "r" ++ show rm
        rv <- parseMaybe (withObject "rep" (\o -> o .: "runner" >>= withObject "rn" (\rn -> rn .: "registers" >>= withObject "regs" (\rs -> rs .: fromString regName)))) r >>= parseHexV
        symAt ks ms rv
      else Nothing

symAt :: Syms -> Syms -> Integer -> Maybe String
symAt ks ms a =
  let hit (Syms m) = listToMaybe [n | (n, (b, z)) <- M.toList m, b == a, z >= 4, z `mod` 4 == 0, n /= "helpers"]
   in case hit ks of Just n -> Just n; Nothing -> hit ms

parseHexV :: Value -> Maybe Integer
parseHexV v = case str v of
  '0' : 'x' : rest -> Just (foldl (\acc c -> acc * 16 + fromIntegral (digitVal c)) 0 rest)
  _ -> Nothing
 where
  digitVal c
    | c >= '0' && c <= '9' = fromEnum c - fromEnum '0'
    | c >= 'a' && c <= 'f' = fromEnum c - fromEnum 'a' + 10
    | otherwise = 0

memMap :: M.Map GuestAddress RegionId -> Value -> M.Map RegionId B.ByteString
memMap regionIds r = M.fromList (fromMaybe [] (parseMaybe p =<< parseMaybe (withObject "rep" (\o -> o .: "runner" >>= withObject "rn" (\rn -> rn .: "memory"))) r))
  where
    p = withArray "mem" $ \a -> pure [(k, unhex v) | x <- toList a, Just (address, v) <- [row x], Just k <- [M.lookup address regionIds]]
    row x = parseMaybe (withObject "r" $ \o -> do
      address <- o .: "address" :: Parser String
      bytes <- o .: "bytes" :: Parser String
      pure (GuestAddress (hexI address), bytes)) x

shaHex :: B.ByteString -> String
shaHex b = replicate (64 - length h) '0' ++ h
  where h = foldr (\w acc -> showHex w acc) "" (map fromIntegral (B.unpack (B.take 32 b)) :: [Integer])

findingsText :: [Spec] -> [Value] -> String
findingsText specs results = unlines (headerLines ++ resultLines)
  where
    headerLines =
      [ "recipe-bank kernel runtime cases: findings"
      , ""
      , "Each case directory records its input artifacts, native runner report and behavioral result."
      , "Behavioral checks cover public returns, explicit readback buffers, live settings and durable card bytes."
      , "Private kernel/model state, stack residue and incidental stub event order are not expectations."
      , "Native region/function bounds, instruction limits and stub input contracts remain enforced."
      , ""
      , "The stock getter, setter, conversion tables, commit dispatcher and consumers execute from the candidate image."
      , "The two stock settings-copy wrappers are replaced by bx lr; journal/notification boundaries remain explicit stubs."
      , "The media ABI model completes calls synchronously with a four-handle pool."
      , "Faults select the destructive-open, write, write-close or verification-close boundary rather than a global call count."
      , "Worker-boundary swaps capture identity before the environment hook and check the production worker predicate before mutation."
      , ""
      , "Unexecuted boundaries: physical SD I/O, stock media queue/scheduler, out-of-order completion, full-system menu rendering and camera acceptance."
      , ""
      ]
    resultLines = ["case " ++ spId sp ++ ": " ++ st | (sp, r) <- zip specs results, let st = fromMaybe "?" (parseMaybe (withObject "r" (\o -> o .: "status" :: Parser String)) r)]

data BootScenario = Publish | ShortImage | CorruptHash | InvalidDestination | SourceNotLoaded | WaitRefused deriving (Eq, Show)

runBootCases :: Int -> FilePath -> FilePath -> FilePath -> IO [Value]
runBootCases jobs out runner package = do
  syms <- parseSyms (package </> "boot/boot.syms")
  source <- B.readFile (package </> "image/source-package.bin")
  id5 <- B.readFile (package </> "image/id5-final.bin")
  fragments <- forM [("entry", "rb_boot_entry"), ("sum", "rb_boot_sum"), ("check", "rb_boot_check"), ("publish", "rb_boot_publish")] $ \(file, sym) ->
    (\bytes -> ("boot-" ++ file, sym, bytes)) <$> B.readFile (package </> "boot" </> (file ++ ".bin"))
  let srcBase = addr syms "RB_SRC_BASE"
      image = B.drop 32 source
      carve = 0x00be1a00
      status = 0x00befffc
      exit = toInteger (readU32 id5 12)
      entries = takeWhile ((/= 0) . fst)
        [(toInteger (readU32 image i), readU32 image (i + 4)) | i <- [0x80, 0x88 .. 0x478]]
      targetSpans = compact 7 [(a, a + 4) | (a, _) <- sortOn fst entries]
      compact count spans
        | length spans <= count = spans
        | otherwise =
            let gaps = [(b - aEnd, i) | (i, ((_, aEnd), (b, _))) <- zip [0 ..] (zip spans (drop 1 spans)),
                         all (\(_, sym, bytes) -> let lo = addr syms sym; hi = lo + toInteger (B.length bytes) in hi <= aEnd || lo >= b) fragments]
                (_, at) = minimum gaps
                merged = (fst (spans !! at), snd (spans !! (at + 1)))
             in compact count (take at spans ++ [merged] ++ drop (at + 2) spans)
      initialFrame = B.concat [u32le (if i == 3 || i == 13 then fromIntegral exit else 0xa0 + fromIntegral i) | i <- [0 .. 13 :: Int]]
      hash bytes = foldl (\h i -> rotateR (h + readU32 bytes i) 7) 0 [0, 4 .. B.length bytes - 4]
  unless (B.length source == 32 + fromIntegral (readU32 source 8)) (fail "boot package length mismatch")
  mapBounded jobs (\scenario -> do
    let cid = case scenario of
          Publish -> "boot-publication-barrier"
          ShortImage -> "boot-refuse-short-image"
          CorruptHash -> "boot-refuse-hash"
          InvalidDestination -> "boot-refuse-destination"
          SourceNotLoaded -> "boot-refuse-source-highwater"
          WaitRefused -> "boot-refuse-loader-wait"
        dir = out </> cid
        replace off bytes original = B.take off original <> bytes <> B.drop (off + B.length bytes) original
        apply initial spans = foldl (\bytes (off, value) -> replace off value bytes) initial spans
        badImage = replace (0x80 + 8 * (length entries - 1)) (u32le 0x009a0000) image
        carried = case scenario of
          ShortImage -> replace 8 (u32le 0x100) source
          CorruptHash -> replace 12 (u32le (readU32 source 12 `xor` 1)) source
          InvalidDestination -> replace 12 (u32le (hash badImage)) (B.take 32 source <> badImage)
          _ -> source
        copied = scenario `elem` [Publish, CorruptHash, InvalidDestination]
        published = scenario == Publish
        runtime = if copied then B.drop 32 carried <> B.replicate (0xe600 - B.length image) 0 else B.replicate 0xe600 0
        expectedRuntime = if published then runtime else replace (fromIntegral (status - carve)) (u32le 2) runtime
        targets =
          [ ("target-" ++ show i, a, B.replicate (fromIntegral (b - a)) 0,
             apply (B.replicate (fromIntegral (b - a)) 0)
               [(fromIntegral (dest - a), u32le value) | (dest, value) <- entries, published, dest >= a, dest < b])
          | (i, (a, b)) <- zip [0 :: Int ..] targetSpans]
        sourcePresent = scenario `notElem` [SourceNotLoaded, WaitRefused]
        imageRegions =
          [("source", srcBase, carried, "rx" :: String) | sourcePresent] ++
          [ ("runtime", carve, B.replicate 0xe600 0, "rw")
          , ("reporter", 0x00914834, u32le (fromIntegral (if scenario == SourceNotLoaded then srcBase + toInteger (B.length source) - 0x1100001 else srcBase + toInteger (B.length source) - 0x1100000)), "rw")
          , ("frame", 0x009a0000, B.replicate 8 0 <> initialFrame <> B.replicate 8 0, "rw")
          , ("stock-entry", exit, B.take 4 (B.drop (fromIntegral (exit - 0xbf1a00)) id5), "rx")
          ] ++ [(n, addr syms sym, bytes, "rx") | (n, sym, bytes) <- fragments]
          ++ [(n, a, bytes, "rw") | (n, a, bytes, _) <- targets]
        fnSpans = [(addr syms sym, toInteger (B.length bytes)) | (_, sym, bytes) <- fragments]
          ++ [(addr syms "rb_store_entry", 60) | sourcePresent]
        waitEvent = object ["address" .= ("0x00724b68" :: String), "inputs" .= object ["r0" .= hex 0x47, "r1" .= hex 1000], "outputs" .= object ["r0" .= hex (if scenario == WaitRefused then 1 else 0)]]
        releaseEvent = object ["address" .= ("0x00724c00" :: String), "inputs" .= object ["r0" .= hex 0x47], "outputs" .= object ["r0" .= hex 0]]
        events = waitEvent : [releaseEvent | scenario /= WaitRefused]
        stub event = case event of Object o -> Object (o <> case object ["side_effects" .= ("registers-only" :: String)] of Object extra -> extra; _ -> error "stub object"); _ -> error "event object"
        claim n bytes = object ["region" .= n, "offset" .= (0 :: Int), "hex" .= hexB bytes]
        expectedRegisters =
          if published then object ["r0" .= hex 0, "pc" .= hex (addr syms "rb_store_entry" + 28)]
          else object (["lr" .= hex 0x0069d2e4, "sp" .= hex 0x009a0040]
            ++ [fromString ("r" ++ show i) .= hex (if i == 3 then exit else 0xa0 + toInteger i) | i <- [0 .. 12 :: Int]])
    createDirectoryIfMissing True dir
    digests <- forM imageRegions $ \(n, _, bytes, _) -> do
      B.writeFile (dir </> (n ++ ".bin")) bytes
      pure (n, sha256Hex (sha256 bytes))
    let c = object
          [ "schema_version" .= (1 :: Int), "id" .= cid, "origin" .= ("synthetic" :: String)
          , "provenance" .= ("Exact packaged boot fragments/source image; modeled launcher frame and zero-filled publication destinations. Scenario " ++ show scenario)
          , "cpu" .= object ["architecture" .= ("arm" :: String), "model" .= ("cortex-r5f" :: String), "mode" .= ("arm" :: String), "endian" .= ("little" :: String)]
          , "artifacts" .= [object ["id" .= n, "path" .= (n ++ ".bin"), "size" .= B.length bytes, "sha256" .= fromJust (lookup n digests)] | (n, _, bytes, _) <- imageRegions]
          , "entry" .= hex (addr syms "rb_boot_entry"), "exit" .= hex exit
          , "registers" .= object (["sp" .= hex 0x009a0040, "lr" .= hex exit, "cpsr" .= hex 0x10] ++
              [fromString ("r" ++ show i) .= hex (if i == 3 then exit else 0xa0 + toInteger i) | i <- [0 .. 12 :: Int]])
          , "regions" .= [object ["id" .= n, "address" .= hex a, "size" .= B.length bytes, "permissions" .= perm,
              "initial" .= object ["artifact" .= n, "offset" .= (0 :: Int), "length" .= B.length bytes]] | (n, a, bytes, perm) <- imageRegions]
          , "functions" .= [object ["address" .= hex a, "size" .= z] | (a, z) <- fnSpans]
          , "stubs" .= map stub events
          , "limits" .= object ["instructions" .= (1000000 :: Int), "wall_ms" .= (4000 :: Int)]
          , "expected" .= object ["status" .= (if published then "unsupported_instruction" else "unknown_call" :: String),
              "registers" .= expectedRegisters, "memory" .= (claim ("runtime" :: String) expectedRuntime : [claim n expected | (n, _, _, expected) <- targets]), "events" .= events]
          , "exclusions" .= (["stock semaphore wait/release are register-only ABI substitutions", "success stops at DSB after publication stores; cache, core and source-coherence acceptance require the full-system emulator", "refusals stop at the stock entry transfer, without executing that entry"] :: [String])
          ]
    BL.writeFile (dir </> "case.json") (encode c)
    (_, report) <- runCase runner (dir </> "case.json")
    pure (object ["id" .= cid, "status" .= fromMaybe "unknown" (parseMaybe (withObject "report" (.: "status")) report :: Maybe String), "report" .= report]))
    [Publish, ShortImage, CorruptHash, InvalidDestination, SourceNotLoaded, WaitRefused]

data AllocatorScenario = ExactFit | OneByteOverflow deriving (Eq)

runAllocatorCases :: Int -> FilePath -> FilePath -> FilePath -> IO [Value]
runAllocatorCases jobs out runner package = do
  id4 <- B.readFile (package </> "image/id4-final.bin")
  let code = B.take 84 (B.drop (0x72b110 - 0x61c1fc) id4)
      originalCursor = 0x00be18e0
  mapBounded jobs (\scenario -> do
    let fits = scenario == ExactFit
        cid = if fits then "carve-allocator-exact-fit" else "carve-allocator-overflow"
        dir = out </> cid
        bytes = u32le originalCursor <> u32le 9
        regions = [("code", 0x0072b110, code, "rx" :: String), ("allocator", 0x00829a08, bytes, "rw"),
                   ("stack", 0x009a0000, B.replicate 32 0, "rw"), ("exit", 0x009b0000, B.pack [0x1e,0xff,0x2f,0xe1], "rx")]
        expectedState = u32le (if fits then 0x00be19e0 else originalCursor) <> u32le (if fits then 9 else 12)
    createDirectoryIfMissing True dir
    digests <- forM regions $ \(n, _, raw, _) -> B.writeFile (dir </> (n ++ ".bin")) raw >> pure (n, sha256Hex (sha256 raw))
    let caseValue = object
          [ "schema_version" .= (1 :: Int), "id" .= cid, "origin" .= ("synthetic" :: String)
          , "provenance" .= ("Exact packaged X-M1 1.50 backing allocator. Cursor 0x00be18e0 plus request " ++ (if fits then "256" else "257") ++ " and the stock 32-byte reserve at the emitted 0x00be1a00 upper bound.")
          , "cpu" .= object ["architecture" .= ("arm" :: String), "model" .= ("cortex-r5f" :: String), "mode" .= ("arm" :: String), "endian" .= ("little" :: String)]
          , "artifacts" .= [object ["id" .= n, "path" .= (n ++ ".bin"), "size" .= B.length raw, "sha256" .= fromJust (lookup n digests)] | (n, _, raw, _) <- regions]
          , "entry" .= hex 0x0072b110, "exit" .= hex 0x009b0000
          , "registers" .= object (["r0" .= hex 0x00829a0c, "r1" .= hex (if fits then 256 else 257), "sp" .= hex 0x009a0010, "lr" .= hex 0x009b0000, "cpsr" .= hex 0x10]
              ++ [fromString ("r" ++ show i) .= hex 0 | i <- [2 .. 12 :: Int]])
          , "regions" .= [object ["id" .= n, "address" .= hex a, "size" .= B.length raw, "permissions" .= perm,
              "initial" .= object ["artifact" .= n, "offset" .= (0 :: Int), "length" .= B.length raw]] | (n, a, raw, perm) <- regions]
          , "functions" .= [object ["address" .= hex 0x0072b110, "size" .= (84 :: Int)]]
          , "stubs" .= ([] :: [Value])
          , "limits" .= object ["instructions" .= (1000 :: Int), "wall_ms" .= (4000 :: Int)]
          , "expected" .= object ["status" .= ("returned" :: String), "registers" .= object ["r0" .= hex (if fits then toInteger originalCursor else 0xffffffff)],
              "memory" .= [object ["region" .= ("allocator" :: String), "offset" .= (0 :: Int), "hex" .= hexB expectedState]], "events" .= ([] :: [Value])]
          , "exclusions" .= (["no heap population, exhaustion workload or carve writer ownership is established", "allocator behavior checks its public cursor/status words and return value; stack bytes are not expectations"] :: [String])
          ]
    BL.writeFile (dir </> "case.json") (encode caseValue)
    (_, report) <- runCase runner (dir </> "case.json")
    BL.writeFile (dir </> "runner-report.json") (encode report)
    let runnerResult = parseMaybe (withObject "report" (.: "runner")) report
        runnerStatus = runnerResult >>= parseMaybe (withObject "runner" (.: "status")) :: Maybe String
        actualR0 = runnerResult >>= parseMaybe (withObject "runner" (\o -> o .: "registers" >>= withObject "registers" (.: "r0"))) >>= parseHexV
        memory = memMap (M.singleton (GuestAddress 0x00829a08) (RegionId "allocator")) report
        mismatches :: [String]
        mismatches = ["status differs" | runnerStatus /= Just "returned"]
          ++ ["register r0: differs" | actualR0 /= Just (if fits then toInteger originalCursor else 0xffffffff)]
          ++ ["allocator output differs" | M.lookup (RegionId "allocator") memory /= Just expectedState]
        status = if isNothing runnerResult
          then fromMaybe "invalid_case" (parseMaybe (withObject "report" (.: "status")) report :: Maybe String)
          else if null mismatches then "passed" else "failed"
        result = object ["id" .= cid, "status" .= status, "mismatches" .= mismatches, "report" .= report]
    BL.writeFile (dir </> "behavioral-result.json") (encode result)
    pure result)
    [ExactFit, OneByteOverflow]
