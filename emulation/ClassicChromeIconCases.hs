{-# LANGUAGE ScopedTypeVariables #-}
-- Native-harness cases for the Classic Chrome icon (reports/classic-chrome-icon.md)
-- and the combined Classic Chrome flash candidate, X-M1 1.50.
--
-- Four sides run the same stock functions:
--   stock     the pinned stock decodes and the stock FONT_ICON.FDL window
--   cc        the reviewed Classic Chrome candidate decodes, image 2 with the CC
--             0x83e record, the stock font window
--   combined  the cc side with the icon write set of FujiAF.ClassicChromeIcon
--             (image-4 words, the 0x83d record, the font writes), the live-view
--             image-5 words (host/firmware/classic-chrome-liveview-image5.json)
--             and the frames-remaining fix's three image-5 words
--   qtile     the combined side with the Q-tile write set of
--             FujiAF.ClassicChromeQTile (three Q label words and the GVFF tile
--             in the font window, the Q glyph 0x37c record pointer in image 2)
-- The frames family adds two image-5 states of a task preempted inside
-- FUN_00e42414 while the fix is published: pre1 executes the stock cmp and the
-- fix's two movs, pre2 the stock cmp and movne and the fix's movls.
--
-- Every expectation is computed here from byte reads of the side's images
-- before any run. Stack bytes, and the unclaimed bytes of the font-object and
-- raster-context regions the open and configure cases build, are pinned from a
-- discovery run: they are residue the exact-final-memory contract requires,
-- not claims. The loader and sizing cases start from the final bytes of their
-- side's open or configure case; the validator cases from the class their
-- side's mapper case returned.
--
-- Families:
--   open-*      plain FUN_01197b98 (FDL font object) on the font window
--   config-*    plain FUN_0119fcbc (raster context) on the font window
--   fdl1-*      plain FUN_01197f98 glyph loader over the font object
--   fdl2-*      plain FUN_0119f74c glyph loader over the raster context
--   size-*      plain FUN_011a023c per-mille sizing read
--   text-*      plain FUN_01179b0c text resolution; glyph-* plain FUN_0117e15c
--   row-*       id5 FUN_00bf9360 label row getter
--   val-*       id5 FUN_00bf93e0 label value getter
--   map-*       id5 FUN_00e42414 film col2 to frames-remaining size class
--   valid-*     id5 FUN_00ed5868 size-class validator over the mapped class
--   qrec-*      plain FUN_0117e048/0a4/100/15c Q glyph record accessors
--   kind-*      plain FUN_01179a50 label kind of each Q-row label
--   qdraw-*     plain FUN_011121d4 Q label drawer, the generic draw
--               FUN_01179da0 a register-only stub recording the drawn size
module Main where

import Control.Concurrent
import Control.Exception (SomeException, evaluate, try)
import Control.Monad
import Data.Aeson (Value)
import Data.Bits
import qualified Data.ByteString as B
import Data.Char (digitToInt, isHexDigit)
import Data.IORef
import Data.List (intercalate, sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, isJust)
import qualified Data.Set as S
import Data.Word
import Emulation.NativeCase (runCase)
import FujiAF.ClassicChromeIcon (FontWrite (..), Image4Write (..), iconFontWrites, iconGlyphRecord, iconImage4Writes, iconTailEnd, label83dRecord, qLabelsArray, qLabelsOffset, qValuesArray)
import FujiAF.ClassicChromeQTile (Image2Write (..), QGlyph (..), qGlyphLabel, qGlyphRecordAddress, qTileAddress, qTileFontWrites, qTileGvff, qTileImage2Writes, qTileOffset)
import FujiAF.FdlFont (FontOffset (..), parseFdlFont)
import FujiAF.Gvff (Gvff (..), decodeGvff)
import FujiAF.Host
import FujiAF.JSON (decodeStrict, encodeCanonical)
import Numeric (readHex, showHex)
import System.Directory
import System.Environment
import System.Exit
import System.FilePath

-- ---------------------------------------------------------------------------
-- Bytes
-- ---------------------------------------------------------------------------

data Img = Img {iName :: String, iDesc :: String, iBase :: Word32, iBytes :: B.ByteString, iSha :: String}

mkImg :: String -> String -> Word32 -> B.ByteString -> IO Img
mkImg name desc base bytes = Img name desc base bytes <$> sha256 bytes

loadImg :: String -> FilePath -> Word32 -> String -> IO Img
loadImg name path base pin = do
  img <- B.readFile path >>= mkImg name path base
  ensure (iSha img == pin) (path ++ ": SHA-256 " ++ iSha img ++ " differs from the pin " ++ pin)
  pure img

sl :: Img -> Word32 -> Int -> B.ByteString
sl img a len
  | a < iBase img || off + len > B.length (iBytes img) = error (iName img ++ ": slice " ++ h32 a ++ "+" ++ show len ++ " outside the image")
  | otherwise = B.take len (B.drop off (iBytes img))
  where
    off = fromIntegral (a - iBase img)

wd :: Img -> Word32 -> Word32
wd img a = le32At (sl img a 4) 0

hw :: Img -> Word32 -> Word32
hw img a = le16At (sl img a 2) 0

le32At :: B.ByteString -> Int -> Word32
le32At bs o = foldr (\k acc -> acc .|. (fromIntegral (B.index bs (o + k)) `shiftL` (8 * k))) 0 [0 .. 3]

le16At :: B.ByteString -> Int -> Word32
le16At bs o = fromIntegral (B.index bs o) .|. (fromIntegral (B.index bs (o + 1)) `shiftL` 8)

le32 :: Word32 -> B.ByteString
le32 v = B.pack [fromIntegral (v `shiftR` k) | k <- [0, 8, 16, 24]]

le16 :: Word32 -> B.ByteString
le16 v = B.pack [fromIntegral v, fromIntegral (v `shiftR` 8)]

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

patch :: Img -> Word32 -> B.ByteString -> B.ByteString
patch img a new = let o = fromIntegral (a - iBase img) in B.take o (iBytes img) <> new <> B.drop (o + B.length new) (iBytes img)

-- ---------------------------------------------------------------------------
-- Case model
-- ---------------------------------------------------------------------------

-- Initial region bytes: zero, literal bytes, or a slice of a named blob.
data Init = IZero | IBytes B.ByteString | ISlice Img Int

data Reg = Reg {rgId :: String, rgAddr :: Word32, rgSize :: Int, rgPerm :: String, rgInit :: Init, rgSrc :: String}

data Stub = Stub Word32 [(String, Word32)] [(String, Word32)]

data Case = Case
  { cId :: String
  , cFamily :: String
  , cProv :: String
  , cEntry :: Word32
  , cArgs :: [(String, Word32)]
  , cSp :: Word32
  , cRegs :: [Reg]
  , cFns :: [(Word32, Int)]
  , cStubs :: [Stub]
  , cExpR :: [(String, Word32)]
  , cClaims :: [(String, Int, B.ByteString)]
  , cEvents :: [Stub]
  , cPin :: [String]
  , cExpect :: String
  , cObserve :: String
  , cExcl :: [String]
  , cInputs :: [(String, String)]
  }

exitA, stackBase :: Word32
exitA = 0x00001080
stackBase = 0x00003000

stackSize :: Int
stackSize = 0x400

-- The stack region with optional argument words at SP.
stackReg :: Word32 -> [Word32] -> Reg
stackReg sp args =
  Reg "stack" stackBase stackSize "rw" (IBytes (B.replicate off 0 <> B.concat (map le32 args))) ("harness stack, SP " ++ h32 sp ++ (if null args then "" else ", stacked arguments " ++ unwords (map h32 args)))
  where
    off = fromIntegral (sp - stackBase)

allRegs :: Case -> [Reg]
allRegs c = cRegs c ++ [Reg "exit" exitA 4 "rx" IZero "exit sentinel"]

codeReg :: String -> Img -> Word32 -> Int -> Reg
codeReg rid img a len = Reg rid a len "rx" (imgInit img a len) (iName img ++ " [" ++ h32 a ++ ", " ++ h32 (a + fromIntegral len) ++ ")")

dataReg :: String -> String -> Img -> Word32 -> Int -> Reg
dataReg rid perm img a len = Reg rid a len perm (imgInit img a len) (iName img ++ " [" ++ h32 a ++ ", " ++ h32 (a + fromIntegral len) ++ ")")

-- Font and slot windows are shared artifacts; a slice of a larger decode is
-- its own artifact.
imgInit :: Img -> Word32 -> Int -> Init
imgInit img a len
  | B.length (iBytes img) <= 0x80000 = ISlice img (fromIntegral (a - iBase img))
  | otherwise = IBytes (sl img a len)

synthReg :: String -> String -> Word32 -> B.ByteString -> String -> Reg
synthReg rid perm a bytes note = Reg rid a (B.length bytes) perm (IBytes bytes) note

regNames :: [String]
regNames = map (("r" ++) . show) [0 :: Int .. 12]

initialRegs :: Case -> [(String, Word32)]
initialRegs c = [(r, fromMaybe 0 (lookup r (cArgs c))) | r <- regNames] ++ [("sp", cSp c), ("lr", exitA), ("cpsr", 0x10)]

expectedRegs :: Case -> [(String, Word32)]
expectedRegs c =
  M.toList (M.fromList ([(r, v) | (r, v) <- initialRegs c, r `elem` ["r4", "r5", "r6", "r7", "r8", "r9", "r10", "r11", "sp"]] ++ [("pc", exitA)] ++ cExpR c))

regsJ :: [(String, Word32)] -> Value
regsJ rs = obj [(k, s (h32 v)) | (k, v) <- rs]

stubJ :: Stub -> Value
stubJ (Stub a i o) = obj [("address", s (h32 a)), ("inputs", regsJ i), ("outputs", regsJ o), ("side_effects", s "registers-only")]

eventJ :: Stub -> Value
eventJ (Stub a i o) = obj [("address", s (h32 a)), ("inputs", regsJ i), ("outputs", regsJ o)]

initialBytes :: Reg -> B.ByteString
initialBytes r = case rgInit r of
  IZero -> B.replicate (rgSize r) 0
  IBytes x -> x <> B.replicate (rgSize r - B.length x) 0
  ISlice img off -> B.take (rgSize r) (B.drop off (iBytes img))

-- Artifact files by content; one per image or literal byte string.
data Env = Env {eOut :: FilePath, eRunner :: FilePath, eArts :: MVar (M.Map String (String, String))}

artifactFor :: Env -> String -> B.ByteString -> IO (String, String)
artifactFor env key bytes = modifyMVar (eArts env) $ \seen -> case M.lookup key seen of
  Just x -> pure (seen, x)
  Nothing -> do
    digest <- sha256 bytes
    let aid = "a-" ++ take 24 digest
        path = eOut env </> aid ++ ".bin"
    present <- doesFileExist path
    old <- if present then B.readFile path else pure B.empty
    when (old /= bytes) (B.writeFile path bytes)
    pure (M.insert key (aid, digest) seen, (aid, digest))

-- Artifact id, digest, artifact size, slice offset and slice length per region.
type RegArt = Maybe (String, String, Int, Int, Int)

regionArt :: Env -> Reg -> IO RegArt
regionArt env r = case rgInit r of
  IZero -> pure Nothing
  IBytes x
    | B.all (== 0) x -> pure Nothing
    | otherwise -> do
        (aid, d) <- artifactFor env ("bytes:" ++ hexBs x) x
        pure (Just (aid, d, B.length x, 0, B.length x))
  ISlice img off -> do
    (aid, d) <- artifactFor env ("img:" ++ iSha img) (iBytes img)
    pure (Just (aid, d, B.length (iBytes img), off, rgSize r))

provenanceText :: Case -> String
provenanceText c =
  cProv c
    ++ " Expected before execution: "
    ++ cExpect c
    ++ " Inputs: "
    ++ intercalate "; " [k ++ " " ++ v | (k, v) <- cInputs c]
    ++ ". Regions: "
    ++ intercalate "; " [rgId r ++ " " ++ h32 (rgAddr r) ++ " " ++ rgPerm r ++ " = " ++ rgSrc r | r <- allRegs c]
    ++ ". The unclaimed bytes of "
    ++ intercalate ", " ("stack" : cPin c)
    ++ " are pinned from a discovery run of this case and are residue, not claims."

caseValue :: Case -> [(Reg, RegArt)] -> [(String, Int, B.ByteString)] -> Value
caseValue c regArts memory =
  obj
    [ ("schema_version", n 1)
    , ("id", s (cId c))
    , ("origin", s "stock")
    , ("provenance", s (provenanceText c))
    , ("cpu", obj [("architecture", s "arm"), ("model", s "cortex-r5f"), ("mode", s "arm"), ("endian", s "little")])
    , ("artifacts", arr [obj [("id", s aid), ("path", s (aid ++ ".bin")), ("size", n (toInteger len)), ("sha256", s d)] | (aid, (d, len)) <- M.toList uniqueArts])
    , ("entry", s (h32 (cEntry c)))
    , ("exit", s (h32 exitA))
    , ("registers", regsJ (initialRegs c))
    , ("regions", arr (map regionJ regArts))
    , ("functions", arr [obj [("address", s (h32 a)), ("size", n (toInteger z))] | (a, z) <- sortOn fst (cFns c)])
    , ("stubs", arr (map stubJ (cStubs c)))
    , ("limits", obj [("instructions", n 200000), ("wall_ms", n 5000)])
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
    uniqueArts = M.fromList [(aid, (d, len)) | (_, Just (aid, d, len, _, _)) <- regArts]
    regionJ (r, art) =
      obj
        [ ("id", s (rgId r))
        , ("address", s (h32 (rgAddr r)))
        , ("size", n (toInteger (rgSize r)))
        , ("permissions", s (rgPerm r))
        , ("initial", maybe (obj [("zero", b True)]) (\(aid, _, _, off, len) -> obj [("artifact", s aid), ("offset", n (toInteger off)), ("length", n (toInteger len))]) art)
        ]

commonExclusions :: [String]
commonExclusions =
  [ "No camera, USB device or full-system emulator is used: the case reads host files and runs the bounded Unicorn Cortex-R5F engine only."
  , "The R5F engine stands for the inferred R4F-class core only inside the admitted instruction set: no coprocessor, MPU, TCM, cache, interrupt, exception or timing behaviour is modelled and the A32 SDIV/UDIV refusal is unchanged."
  , "One function invocation with declared inputs is executed; callers, task context, drawing and rasterization are outside the case."
  , "Patched-image bytes are the candidate decodes with the icon write set applied by this generator, not bytes read from a body."
  ]

finalsOf :: Case -> Value -> IO (M.Map String B.ByteString)
finalsOf c report = do
  let mem = list (get (get report "runner") "memory")
  ensure (length mem == length (allRegs c)) (cId c ++ ": report has no final memory (" ++ str (get report "status") ++ " " ++ str (get report "reason") ++ " " ++ str (get (get report "runner") "status") ++ " " ++ str (get (get report "runner") "reason") ++ ")")
  pure (M.fromList [(rgId r, unhex (str (get m "bytes"))) | (r, m) <- zip (allRegs c) mem])

-- Spans of a region outside the claims whose final bytes differ from the initial ones.
residue :: Case -> Reg -> B.ByteString -> [(String, Int, B.ByteString)]
residue c r final =
  [ (rgId r, a, B.take (z - a) (B.drop a final))
  | (lo, hi) <- gaps 0 (sortOn fst [(o, o + B.length x) | (rid, o, x) <- cClaims c, rid == rgId r])
  , a : _ <- [[i | i <- [lo .. hi - 1], B.index final i /= B.index ini i]]
  , let z = 1 + last [i | i <- [a .. hi - 1], B.index final i /= B.index ini i]
  ]
  where
    ini = initialBytes r
    size = rgSize r
    gaps e [] = [(e, size) | size > e]
    gaps e ((lo, hi) : rest) = [(e, lo) | lo > e] ++ gaps (max e hi) rest

data Outcome = Outcome {oCase :: Case, oStatus :: String, oRunner :: String, oR0 :: String, oInstr :: Integer, oMismatch :: [String], oFinals :: M.Map String B.ByteString}

runOne :: Env -> Case -> IO Outcome
runOne env c = do
  regArts <- forM (allRegs c) $ \r -> (,) r <$> regionArt env r
  let path = eOut env </> cId c ++ ".json"
  B.writeFile path (encodeCanonical (caseValue c regArts (cClaims c)))
  (_, r1) <- runCase (eRunner env) path
  own <- finalsOf c r1
  let pins = concat [residue c r (own M.! rgId r) | r <- allRegs c, rgId r `elem` ("stack" : cPin c)]
  r2 <-
    if null pins
      then pure r1
      else do
        B.writeFile path (encodeCanonical (caseValue c regArts (cClaims c ++ pins)))
        snd <$> runCase (eRunner env) path
  B.writeFile (eOut env </> cId c ++ "-report.json") (encodeCanonical r2)
  finals <- finalsOf c r2
  let runner = get r2 "runner"
  pure
    Outcome
      { oCase = c
      , oStatus = str (get r2 "status")
      , oRunner = str (get runner "status")
      , oR0 = str (get (get runner "registers") "r0")
      , oInstr = num (get runner "instructions")
      , oMismatch = map str (list (get r2 "mismatches"))
      , oFinals = finals
      }

-- Runs the cases on a fixed number of workers; results keep the input order.
runAll :: Env -> Int -> [Case] -> IO [Outcome]
runAll env workers cases = do
  queue <- newMVar (zip [0 :: Int ..] cases)
  results <- newIORef M.empty
  dones <- forM [1 .. workers] $ \_ -> do
    done <- newEmptyMVar
    _ <- forkIO $ do
      let loop = do
            next <- modifyMVar queue (\q -> pure (drop 1 q, take 1 q))
            case next of
              [] -> pure ()
              (i, c) : _ -> do
                r <- try (runOne env c >>= evaluate)
                case r of
                  Left (e :: SomeException) -> atomicModifyIORef' results (\m -> (M.insert i (Left (cId c ++ ": " ++ show e)) m, ()))
                  Right o -> do
                    atomicModifyIORef' results (\m -> (M.insert i (Right o) m, ()))
                    putStrLn (oStatus o ++ " " ++ cId c ++ " runner=" ++ oRunner o ++ " r0=" ++ oR0 o ++ " instructions=" ++ show (oInstr o) ++ concatMap (" " ++) (oMismatch o))
                loop
      loop
      putMVar done ()
    pure done
  mapM_ takeMVar dones
  m <- readIORef results
  forM (M.elems m) (either failH pure)

-- ---------------------------------------------------------------------------
-- Pins, inputs, sides
-- ---------------------------------------------------------------------------

stockId4Sha, stockId5Sha, stockPlainSha, stockId2Sha, stockDatSha, ccDatSha, ccId4Sha, ccId5Sha, ccPlainSha, loaderSha :: String
stockId4Sha = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
stockId5Sha = "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"
stockPlainSha = "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d"
stockId2Sha = "236d12a11c8b386f71192675c12398358840327f21e392e6eaa91ddfd6e020a5"
stockDatSha = "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"
ccDatSha = "16c830ad71b9755d22368ffaeacd6b8b4d78376c9568ac3f9717f45d48292c89"
ccId4Sha = "00493fab3f7c67fddf3899e8f38395def2b593b93e4dcb73206bfb6cc2ddff6a"
ccId5Sha = "e740aeb28ce070a659583bd0bb5cfd9ecc5846ac302564741fcdf37b9b2089bb"
ccPlainSha = "7e60e04e2dde4cecea4d575e02ae64b8f6053af3f459c42b5222f980e3c301fa"
loaderSha = "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"

-- Contract digests of the icon bytes.
recordSha, qValuesSha, qLabelsSha :: String
recordSha = "d8f69314a25cf8f5a3e0cf340aaf1996f9102283168e41b123116d8c5e600429"
qValuesSha = "01670314392c2618797a47aadebdad5fe6df4e55c564e6266d8df8c1d077b312"
qLabelsSha = "56e79304725b4d70987cf95d6480725af8a0b9fa24559d92db88834bab277073"

-- FONT_ICON.FDL: media 0x01060000, window 0x80000, RAM 0x003bc000.
-- English language slot: media 0x01120000, window 0x20000, RAM 0x0039c000.
fontRam, slotRam :: Word32
fontRam = 0x003bc000
slotRam = 0x0039c000

datWindow :: B.ByteString -> Int -> Int -> B.ByteString
datWindow dat media len = B.map complement (B.take len (B.drop (media + 0x25c) dat))

data Opts = Opts
  { oOut :: FilePath
  , oRunnerPath :: FilePath
  , oCandBuild :: FilePath
  , oCandDat :: FilePath
  , oPrivate :: FilePath
  , oLiveview :: FilePath
  , oFrames :: Maybe FilePath
  , oCombinedDecodes :: [(String, FilePath)]
  , oCombinedDat :: Maybe FilePath
  , oQTileDat :: Maybe FilePath
  , oWorkers :: Int
  }

defaults :: Opts
defaults =
  Opts
    { oOut = "private-data/analysis/x-m1-1.50/native-case/classic-chrome-icon"
    , oRunnerPath = "bazel-bin/emulation/native-runner"
    , oCandBuild = "private-data/analysis/classic-chrome-patch/candidate-build-1"
    , oCandDat = "private-data/analysis/classic-chrome-patch/candidate-pinned-dat/FWUP0002.DAT"
    , oPrivate = "private-data"
    , oLiveview = "host/firmware/classic-chrome-liveview-image5.json"
    , oFrames = Nothing
    , oCombinedDecodes = []
    , oCombinedDat = Nothing
    , oQTileDat = Nothing
    , oWorkers = 8
    }

parseOpts :: Opts -> [String] -> IO Opts
parseOpts o [] = pure o
parseOpts o (flag : v : rest) = case flag of
  "--out" -> parseOpts o {oOut = v} rest
  "--runner" -> parseOpts o {oRunnerPath = v} rest
  "--candidate-build" -> parseOpts o {oCandBuild = v} rest
  "--candidate-dat" -> parseOpts o {oCandDat = v} rest
  "--private" -> parseOpts o {oPrivate = v} rest
  "--liveview" -> parseOpts o {oLiveview = v} rest
  "--frames" -> parseOpts o {oFrames = Just v} rest
  "--combined-id4" -> parseOpts o {oCombinedDecodes = oCombinedDecodes o ++ [("id4", v)]} rest
  "--combined-id5" -> parseOpts o {oCombinedDecodes = oCombinedDecodes o ++ [("id5", v)]} rest
  "--combined-plain" -> parseOpts o {oCombinedDecodes = oCombinedDecodes o ++ [("plain", v)]} rest
  "--combined-dat" -> parseOpts o {oCombinedDat = Just v} rest
  "--q-tile-dat" -> parseOpts o {oQTileDat = Just v} rest
  "--workers" -> parseOpts o {oWorkers = read v} rest
  _ -> failH ("unknown option " ++ flag)
parseOpts _ [flag] = failH ("option " ++ flag ++ " needs a value")

data Side = Side {sdTag :: String, sdId4, sdId5, sdPlain, sdId2, sdFont, sdSlot :: Img}

imgInput :: Img -> (String, String)
imgInput img = (iName img, iDesc img ++ " (SHA-256 " ++ iSha img ++ ", base " ++ h32 (iBase img) ++ ")")

-- The 0x83e record of the reviewed candidate.
ccRecord :: B.ByteString
ccRecord =
  let units = map (fromIntegral . fromEnum) "CLASSIC CHROME" ++ [0]
   in B.concat (map le16 units) <> B.replicate (56 - 2 * length units) 0 <> B.concat (map le16 [40, 0, 30, 36, 48])

-- Image-5 word fragments in the overlay schema: address, current and new word.
type Fragment = [(Word32, Word32, Word32)]

loadFragment :: FilePath -> IO Fragment
loadFragment path = do
  v <- B.readFile path >>= either failH pure . decodeStrict
  let hex32 x = case readHex (drop 2 (str x)) of
        [(w, "")] -> pure (fromInteger w)
        _ -> failH (path ++ ": " ++ str x ++ " is not a hexadecimal word")
  ensure (str (get (get v "image") "decode_base") == "0x00bf1a00" && str (get (get v "image") "decode_sha256") == ccId5Sha) (path ++ ": not a fragment over the CC candidate's image 5")
  forM (list (get v "words")) $ \w -> (,,) <$> hex32 (get w "address") <*> hex32 (get w "current") <*> hex32 (get w "new")

liveviewSha :: String
liveviewSha = "45211718825328620ab27882b55555ae1299354eaeee9abe797533e4529ca015"

-- The frames-remaining fix (contract, combined candidate additions):
-- cmp r0,#13; movhi r0,#6; movls r0,#0 over the tail of FUN_00e42414.
framesFix :: Fragment
framesFix = [(0x00e424ac, 0xe350000c, 0xe350000d), (0x00e424b0, 0x13a00006, 0x83a00006), (0x00e424b4, 0x03a00000, 0x93a00000)]

applyFragment :: String -> Img -> Fragment -> IO B.ByteString
applyFragment what img ws = do
  forM_ ws $ \(addr, cur, _) -> ensure (wd img addr == cur) (what ++ ": current word at " ++ h32 addr ++ " is " ++ h32 (wd img addr) ++ ", not " ++ h32 cur)
  pure (foldl (\acc (addr, _, new) -> let o = fromIntegral (addr - iBase img) in B.take o acc <> le32 new <> B.drop (o + 4) acc) (iBytes img) ws)

data World = World {wStock, wCc, wComb, wQ :: Side, wStockFont :: Img, wPre :: [(String, Img)]}

loadWorld :: Opts -> IO World
loadWorld o = do
  let p = oPrivate o
      a = p </> "analysis/x-m1-1.50"
  id4 <- loadImg "id4-stock" (a </> "lz/id4-0061c400.bin") 0x0061c400 stockId4Sha
  id5 <- loadImg "id5-stock" (a </> "lz/id5-00bf1a00.bin") 0x00bf1a00 stockId5Sha
  plain <- loadImg "plain-stock" (a </> "lz/plain-01100000.bin") 0x01100000 stockPlainSha
  id2 <- loadImg "id2-stock" (a </> "filmsim/id2-00130000.bin") 0x00130000 stockId2Sha
  c4 <- loadImg "id4-cc" (oCandBuild o </> "patched-id4.bin") 0x0061c200 ccId4Sha
  c5 <- loadImg "id5-cc" (oCandBuild o </> "patched-id5.bin") 0x00bf1a00 ccId5Sha
  cp <- loadImg "plain-cc" (oCandBuild o </> "patched-plain.bin") 0x01100000 ccPlainSha
  dat <- B.readFile (p </> "firmware/x-m1-1.50/FWUP0002.DAT")
  datSha <- sha256 dat
  ensure (datSha == stockDatSha) ("stock FWUP0002.DAT: SHA-256 " ++ datSha)
  cdat <- B.readFile (oCandDat o)
  cdatSha <- sha256 cdat
  ensure (cdatSha == ccDatSha) (oCandDat o ++ ": SHA-256 " ++ cdatSha)
  let fontBytes = datWindow dat 0x01060000 0x80000
      slotBytes = datWindow dat 0x01120000 0x20000
  ensure (datWindow cdat 0x01060000 0x80000 == fontBytes) "the candidate DAT's FONT_ICON.FDL window differs from the stock DAT's"
  ensure (datWindow cdat 0x01120000 0x20000 == slotBytes) "the candidate DAT's English slot differs from the stock DAT's"
  ensure (B.map complement (B.take 66 (B.drop 0x00eeba64 cdat)) == ccRecord) "the candidate DAT's 0x83e record is not the CC record"
  ensure (datWindow dat id2Media (B.length (iBytes id2)) == iBytes id2) "the stock DAT's image-2 window differs from the stock image-2 decode"
  font <- mkImg "font-stock" "FONT_ICON.FDL window of the stock DAT, media 0x01060000 + 0x80000, complemented" fontRam fontBytes
  slot <- mkImg "slot-en" "English language slot of the stock DAT, media 0x01120000 + 0x20000, complemented" slotRam slotBytes
  ensure (B.take 8 fontBytes == B.pack (map (fromIntegral . fromEnum) "FDL Font")) "font window magic"
  -- Icon font.
  parsed <- either failH pure (parseFdlFont fontBytes)
  writes <- either failH pure (iconFontWrites parsed)
  forM_ writes $ \fw -> do
    let FontOffset off = fwOffset fw
    ensure (B.take (B.length (fwOld fw)) (B.drop off fontBytes) == fwOld fw) (fwRole fw ++ ": old bytes differ")
  let at off = [fwNew fw | fw <- writes, let FontOffset o' = fwOffset fw, o' == off]
  ensure (at 0x3c21d == [B.pack [0xf3, 0xf2, 0x03, 0x01]]) "the U+F073 directory write differs from the contract"
  ensure (map (B.take 2) (at 0x7c26b) == [le16 0x320]) "the U+F073 per-mille write differs from the contract"
  ensure (at 0x7cfe0 == [iconGlyphRecord] && at 0x7d160 == [qValuesArray] && at 0x7d1b0 == [qLabelsArray] && length writes == 5) "the font write set differs from the contract placement"
  forM_ [(iconGlyphRecord, recordSha, "record"), (qValuesArray, qValuesSha, "Q values"), (qLabelsArray, qLabelsSha, "Q labels")] $ \(x, pin, what) -> do
    d <- sha256 x
    ensure (d == pin) (what ++ ": SHA-256 " ++ d ++ " differs from the contract " ++ pin)
  let combFontBytes = foldl (\acc fw -> let FontOffset off = fwOffset fw in B.take off acc <> fwNew fw <> B.drop (off + B.length (fwNew fw)) acc) fontBytes writes
  -- Image 4.
  forM_ iconImage4Writes $ \w -> ensure (wd c4 (i4Address w) == i4Old w) (i4Role w ++ ": old word differs")
  let i4bytes = foldl (\acc w -> let o' = fromIntegral (i4Address w - 0x0061c200) in B.take o' acc <> le32 (i4New w) <> B.drop (o' + 4) acc) (iBytes c4) iconImage4Writes
  -- Image 5: the live-view words, then the frames-remaining fix.
  lvBytes <- B.readFile (oLiveview o)
  lvSha <- sha256 lvBytes
  ensure (lvSha == liveviewSha) (oLiveview o ++ ": SHA-256 " ++ lvSha ++ " differs from the contract " ++ liveviewSha)
  live <- loadFragment (oLiveview o)
  ensure (length live == 5 && all (\(addr, _, _) -> addr >= 0x00eee960 && addr < 0x00eee974) live) (oLiveview o ++ ": not the five words 0x00eee960..0x00eee970")
  forM_ (oFrames o) $ \path -> do
    fr <- loadFragment path
    ensure (fr == framesFix) (path ++ ": words differ from the contract's frames-remaining fix")
  lv5 <- applyFragment (oLiveview o) c5 live >>= mkImg "t" "" 0x00bf1a00
  i5bytes <- applyFragment "frames-remaining fix" lv5 framesFix
  -- Image 2.
  ensure (sl id2 0x0037b808 66 == B.replicate 66 0) "id2: the stock 0x83e record is not 66 zero bytes"
  ensure (B.length label83dRecord == 66 && le16At label83dRecord 0 == 0xf073 && le16At label83dRecord 2 == 0) "label83dRecord: not the one-unit U+F073 record"
  ensure (map (le16At label83dRecord) [56, 58, 60, 62, 64] == [540, 24, 30, 36, 48]) "label83dRecord: layout differs from the contract"
  c2 <- mkImg "id2-cc" "the stock image 2 with the CC 0x83e record at 0x0037b808" 0x00130000 (patch id2 0x0037b808 ccRecord)
  let i2bytes = patch c2 0x0037b7c6 label83dRecord
  -- The combined images: built here, or read from --combined-id4/-id5/-plain and
  -- --combined-dat and required to equal the built ones.
  let built =
        [ ("id4", 0x0061c200, i4bytes, iDesc c4 ++ " with the five iconImage4Writes words")
        , ("id5", 0x00bf1a00, i5bytes, iDesc c5 ++ " with the five live-view words of " ++ oLiveview o ++ " and the three frames-remaining fix words")
        , ("plain", 0x01100000, iBytes cp, iDesc cp)
        , ("id2", 0x00130000, i2bytes, "the stock image 2 with the CC 0x83e record at 0x0037b808 and label83dRecord at 0x0037b7c6")
        , ("font", fontRam, combFontBytes, "the stock FONT_ICON.FDL window with the iconFontWrites set applied")
        , ("slot", slotRam, slotBytes, iDesc slot)
        ]
  external <- combinedInputs o (B.length (iBytes id2))
  comb <- forM built $ \(kind, base, bytes, desc) -> case lookup kind external of
    Nothing -> mkImg (kind ++ "-combined") desc base bytes
    Just (path, other) -> do
      ensure (other == bytes) (path ++ ": differs from the combined " ++ kind ++ " built from the decodes and the contract writes" ++ firstDiff bytes other)
      mkImg (kind ++ "-combined") (path ++ ", equal to " ++ desc) base other
  [i4, i5, cpComb, i2, ifont, _] <- pure comb
  pre1 <- applyFragment "pre1" c5 (drop 1 framesFix) >>= mkImg "id5-pre1" (iDesc c5 ++ " with the fix's movhi and movls (stock cmp)") 0x00bf1a00
  pre2 <- applyFragment "pre2" c5 (drop 2 framesFix) >>= mkImg "id5-pre2" (iDesc c5 ++ " with the fix's movls (stock cmp and movne)") 0x00bf1a00
  ensure (wd i4 0x0078b020 == 0x00439160 && wd i4 0x0078b024 == 0x004391b0) "Q node pointers differ from the contract"
  -- The Q-tile side: the combined font window with qTileFontWrites, the
  -- combined image 2 with the Q glyph 0x37c record pointer.
  qWrites <- either failH pure (qTileFontWrites (iBytes ifont))
  let FontOffset labelsAt = qLabelsOffset
      FontOffset tileAt = qTileOffset
  ensure (map fwOffset qWrites == map FontOffset [labelsAt + 0x50, labelsAt + 0x60, labelsAt + 0x70, tileAt] && qTileOffset == iconTailEnd && map fwNew (take 3 qWrites) == map le32 [0xff000779, 0xff000778, 0xff00037c] && map fwNew (drop 3 qWrites) == [qTileGvff]) "the Q-tile font writes differ from the contract"
  forM_ qTileImage2Writes $ \w -> ensure (wd i2 (i2Address w) == i2Old w && i2Address w == 0x0022d28c && i2New w == qTileAddress) "the Q-tile image-2 write differs from the contract"
  let qFontBytes = foldl (\acc fw -> let FontOffset off = fwOffset fw in B.take off acc <> fwNew fw <> B.drop (off + B.length (fwNew fw)) acc) (iBytes ifont) qWrites
      qId2Bytes = foldl (\acc w -> let off = fromIntegral (i2Address w - 0x00130000) in B.take off acc <> le32 (i2New w) <> B.drop (off + 4) acc) (iBytes i2) qTileImage2Writes
  qWindows <- forM (oQTileDat o) $ \path -> do
    dat <- B.readFile path
    pure [("id2", datWindow dat id2Media (B.length qId2Bytes)), ("font", datWindow dat 0x01060000 0x80000)]
  let qImg kind bytes desc = case qWindows >>= lookup kind of
        Nothing -> mkImg (kind ++ "-qtile") desc (iBase (if kind == "id2" then i2 else ifont)) bytes
        Just other -> do
          ensure (other == bytes) (kind ++ " window of " ++ fromMaybe "" (oQTileDat o) ++ " differs from the Q-tile " ++ kind ++ " built from the combined side and the contract writes" ++ firstDiff bytes other)
          mkImg (kind ++ "-qtile") (fromMaybe "" (oQTileDat o) ++ " " ++ kind ++ " window, equal to " ++ desc) (iBase (if kind == "id2" then i2 else ifont)) other
  qi2 <- qImg "id2" qId2Bytes ("the combined image 2 with the Q glyph 0x37c record pointer at 0x0022d28c set to " ++ h32 qTileAddress)
  qfont <- qImg "font" qFontBytes "the combined font window with the qTileFontWrites set applied"
  let stock = Side "stock" id4 id5 plain id2 font slot
      cc = Side "cc" c4 c5 cp c2 font slot
      combined = Side "combined" i4 i5 cpComb i2 ifont slot
      qtile = Side "qtile" i4 i5 cpComb qi2 qfont slot
  pure (World stock cc combined qtile font [("pre1", pre1), ("pre2", pre2)])

-- Image 2 in the DAT: complemented bytes at media 0x00ca0000 + 0x25c for RAM 0x00130000.
id2Media :: Int
id2Media = 0x00ca0000

-- Decodes of an integrator's combined DAT, and the DAT's uncompressed windows.
combinedInputs :: Opts -> Int -> IO [(String, (FilePath, B.ByteString))]
combinedInputs o id2Len = do
  decodes <- forM (oCombinedDecodes o) $ \(kind, path) -> do
    bytes <- B.readFile path
    pure (kind, (path, bytes))
  windows <- forM (oCombinedDat o) $ \path -> do
    dat <- B.readFile path
    pure
      [ ("id2", (path ++ " image-2 window", datWindow dat id2Media id2Len))
      , ("font", (path ++ " FONT_ICON.FDL window", datWindow dat 0x01060000 0x80000))
      , ("slot", (path ++ " English slot", datWindow dat 0x01120000 0x20000))
      ]
  pure (decodes ++ fromMaybe [] windows)

firstDiff :: B.ByteString -> B.ByteString -> String
firstDiff x y
  | B.length x /= B.length y = " (length " ++ show (B.length y) ++ ", built " ++ show (B.length x) ++ ")"
  | otherwise = case [k | k <- [0 .. B.length x - 1], B.index x k /= B.index y k] of
      (k : _) -> " (first difference at offset " ++ hx (fromIntegral k) ++ ")"
      [] -> ""

-- Plain code the font cases execute is identical on the stock and CC images;
-- in the text code the CC images differ only in the 0x83d -> 0x83e gate words.
checkPlain :: World -> IO ()
checkPlain w = do
  let st = sdPlain (wStock w)
      cc = sdPlain (wCc w)
  forM_ [(0x01197b98, 0x011984e4), (0x0119eed8, 0x0119ef94), (0x0119f74c, 0x011a0340), (0x01178aac, 0x01178ab0)] $ \(lo, hi) ->
    ensure (sl st lo (fromIntegral (hi - lo)) == sl cc lo (fromIntegral (hi - lo))) ("plain [" ++ h32 lo ++ ", " ++ h32 hi ++ ") differs between stock and CC")
  let textDiffs = [a | (lo, hi) <- [(0x01179b0c, 0x01179b84), (0x0117deb0, 0x0117e1b8)], a <- [lo, lo + 4 .. hi - 4], wd st a /= wd cc a]
  ensure (all (\a -> wd st a == 0xe300383d && wd cc a == 0xe300383e) textDiffs && 0x01179b14 `elem` textDiffs) ("plain text code: CC differs from stock outside the gate words at " ++ unwords (map h32 textDiffs))

-- ---------------------------------------------------------------------------
-- FDL model
-- ---------------------------------------------------------------------------

commandsOff, dirOff :: Word32
commandsOff = 0x3dced
dirOff = 0x51

glyphCount :: Word32
glyphCount = 0xf726

data Load = Load {ldPresent :: Bool, ldCursor, ldCount :: Word32, ldAdv :: (Word32, Word32), ldFlags :: Word32, ldRead :: Int}
  deriving (Eq)

-- The X-M1 loaders' directory_format 0x100 path, horizontal: count = end -
-- begin of the code's and the next code's directory words, zero is absent;
-- flags&1 reads u16 adv_x, adv_y; flags&2 skips four bytes; nonzero flags read
-- a u16 length that replaces the count. Cursor and count are font offsets.
loadModel :: Img -> Word32 -> Load
loadModel font c
  | c >= glyphCount || count0 == 0 = Load False 0 0 (0, 0) flags 0
  | otherwise = Load True cur3 cnt3 adv flags readLen
  where
    w0 = wd font (fontRam + dirOff + 4 * c)
    w1 = wd font (fontRam + dirOff + 4 * (c + 1))
    begin = w0 .&. 0xffffff
    flags = w0 `shiftR` 24
    count0 = (w1 .&. 0xffffff) - begin
    cur0 = commandsOff + begin
    at o = hw font (fontRam + o)
    (adv, cur1, cnt1) = if testBit flags 0 then ((at cur0, at (cur0 + 2)), cur0 + 4, count0 - 4) else ((0, 0), cur0, count0)
    (cur2, cnt2) = if testBit flags 1 then (cur1 + 4, cnt1 - 4) else (cur1, cnt1)
    (cur3, cnt3) = if flags == 0 then (cur2, cnt2) else (cur2 + 2, at cur2)
    readLen = if flags == 0 then 0 else fromIntegral (cur3 - cur0)

-- The stratified census: every code present on either font, the property-10
-- range U+F000..U+F725, the first and last code of every absent run, and the
-- range ends.
censusCodes :: Img -> Img -> [Word32]
censusCodes a b = S.toList (S.fromList (presentCodes ++ [0xf000 .. 0xf725] ++ runEnds ++ [0, glyphCount - 1]))
  where
    presentAny c = ldPresent (loadModel a c) || ldPresent (loadModel b c)
    flags = [(c, presentAny c) | c <- [0 .. glyphCount - 1]]
    presentCodes = [c | (c, True) <- flags]
    runEnds = concat [[c, c'] | (c, c') <- absentRuns flags]
    absentRuns xs = case dropWhile snd xs of
      [] -> []
      ys@((first, _) : _) -> let (run, rest) = span (not . snd) ys in (first, fst (last run)) : absentRuns rest

-- ---------------------------------------------------------------------------
-- 1. FDL open, configure and loaders
-- ---------------------------------------------------------------------------

structBase, ctxBase, requestBase, outBase :: Word32
structBase = 0x20000000
ctxBase = 0x21000000
requestBase = 0x22000000
outBase = 0x23000000

ctxSize :: Int
ctxSize = 0x83ec

fontLen :: Word32
fontLen = 0x7cfd1

requestBytes :: B.ByteString
requestBytes =
  B.concat (map le32 [0, 30, 30, 0, 0, 0, 0, 0, 0, 0])
    <> B.replicate (0x3c - 0x28) 0
    <> le32 fontRam
    <> le32 ctxBase
    <> le32 (fromIntegral ctxSize)

fontHeader :: Img -> (Word32, Word32)
fontHeader font = (hw font (fontRam + 0x1c), wd font (fontRam + 0x3a))

openCase :: Side -> Case
openCase sd =
  Case
    { cId = "open-" ++ sdTag sd
    , cFamily = "1 FDL open"
    , cProv = "Plain FUN_01197b98 (FDL font object constructor, caller of the loader FUN_01197f98's object) on the " ++ sdTag sd ++ " plain image: r0 = font data 0x003bc000, r1 = registry size 0x7cfd1 (plain font registry 0x011804a4 ICON entry), r2 = a zeroed 0x100-byte object at 0x20000000."
    , cEntry = 0x01197b98
    , cArgs = [("r0", fontRam), ("r1", fontLen), ("r2", structBase)]
    , cSp = 0x00003400
    , cRegs = [stackReg 0x00003400 [], codeReg "code" (sdPlain sd) 0x01197b98 0x2b8, dataReg "font-header" "r" (sdFont sd) fontRam 0x40, synthReg "object" "rw" structBase (B.replicate 0x100 0) "font object, zero"]
    , cFns = [(0x01197b98, 0x2b8)]
    , cStubs = []
    , cExpR = [("r0", structBase)]
    , cClaims = [("object", 8, le32 fontRam), ("object", 0x14, le32 count), ("object", 0x2c, le32 commandsOff), ("object", 0x34, le32 0x100), ("object", 0x38, le32 dirOff), ("object", 0x3c, le16 0xffff)]
    , cEvents = []
    , cPin = ["object"]
    , cExpect = "r0 = the object; +0x08 = data 0x003bc000, +0x14 = glyph count " ++ hx count ++ " (header +0x1c), +0x2c = commands 0x3dced (+0x32), +0x34 = directory format 0x100 (+0x38), +0x38 = directory 0x51 (+0x2e), +0x3c = 0xffff, read from the " ++ iName (sdFont sd) ++ " header."
    , cObserve = "font object"
    , cExcl = ["Only the 0x40-byte header is mapped: the constructor reads no directory, command or property byte on the directory_format 0x100 path."]
    , cInputs = [imgInput (sdPlain sd), imgInput (sdFont sd)]
    }
  where
    count = fst (fontHeader (sdFont sd))

configCase :: Side -> Img -> Case
configCase sd loader =
  Case
    { cId = "config-" ++ sdTag sd
    , cFamily = "1 raster configure"
    , cProv = "Plain FUN_0119fcbc (raster context configure, caller of the loader FUN_0119f74c's context and of FUN_011a023c's re-configure) on the " ++ sdTag sd ++ " plain image with its property finder FUN_0119eed8 and the kernel byte copy 0x0004357c from loader-40000.bin. r0 = request block 0x22000000: +0 = 0 (horizontal), width = height = 30, style 0, stroke 0, padding 0, no fallback, storage mode 0, data +0x3c = 0x003bc000, context +0x40 = 0x21000000, capacity +0x44 = 0x83ec. The whole font window is mapped at 0x003bc000."
    , cEntry = 0x0119fcbc
    , cArgs = [("r0", requestBase)]
    , cSp = 0x00003400
    , cRegs =
        [ stackReg 0x00003400 []
        , codeReg "code-config" (sdPlain sd) 0x0119fcbc 0x544
        , codeReg "code-property" (sdPlain sd) 0x0119eed8 0xbc
        , dataReg "scales" "r" (sdPlain sd) 0x011a2ad4 12
        , codeReg "code-copy" loader 0x0004357c 0x30
        , dataReg "font" "r" (sdFont sd) fontRam 0x80000
        , synthReg "request" "rw" requestBase requestBytes "raster request"
        , synthReg "context" "rw" ctxBase (B.replicate ctxSize 0) "raster context 0x83ec bytes, zero"
        ]
    , cFns = [(0x0119fcbc, 0x544), (0x0119eed8, 0xbc), (0x0004357c, 0x30)]
    , cStubs = []
    , cExpR = [("r0", 0)]
    , cClaims =
        [ ("context", 0x48, le32 fontRam)
        , ("context", 0x54, le32 count)
        , ("context", 0x68, le32 dirOff)
        , ("context", 0x6c, le32 commandsOff)
        , ("context", 0x70, le32 4)
        , ("context", 0x74, le32 0x100)
        , ("context", 0x78, le32 dirOff)
        , ("context", 0x7c, le16 0xffff)
        , ("context", 0x83d4, le32 props)
        , ("context", 0x83d8, le16 propCount)
        , ("context", 0x83dc, le32 prop10)
        , ("context", 0x83e0, le16 (hw font (fontRam + prop10 + 6)) <> le16 (hw font (fontRam + prop10 + 8)))
        , ("context", 0x83e8, le32 (prop10 + 14))
        ]
    , cEvents = []
    , cPin = ["context"]
    , cExpect = "r0 = 0; context +0x48 = data 0x003bc000, +0x54 = glyph count " ++ hx count ++ ", +0x68 = +0x78 = directory 0x51, +0x6c = commands 0x3dced, +0x70 = format 4, +0x74 = directory format 0x100, +0x7c = 0xffff, +0x83d4 = properties " ++ hx props ++ " (count " ++ hx propCount ++ "), +0x83dc = property 10 at " ++ hx prop10 ++ ", range " ++ hx (hw font (fontRam + prop10 + 6)) ++ ".." ++ hx (hw font (fontRam + prop10 + 8)) ++ ", per-mille table +0x83e8 = " ++ hx (prop10 + 14) ++ " (header and property table read from " ++ iName font ++ ")."
    , cObserve = "raster context"
    , cExcl = ["The request's scale table read and the context's scaled sizes are residue; only the font fields the loader and the sizing read use are claimed."]
    , cInputs = [imgInput (sdPlain sd), imgInput font, imgInput loader]
    }
  where
    font = sdFont sd
    (count, props) = fontHeader font
    propCount = hw font (fontRam + props)
    prop10 = propertyOffset font 10

-- Property records from properties + 2: u16 key, u16 length, payload of that
-- length; the finder returns the record's offset.
propertyOffset :: Img -> Word32 -> Word32
propertyOffset font key = go (props + 2) (hw font (fontRam + props))
  where
    props = snd (fontHeader font)
    go _ 0 = 0
    go at k = if hw font (fontRam + at) == key then at else go (at + 4 + hw font (fontRam + at + 2)) (k - 1)

data LoaderKind = Fdl1 | Fdl2 deriving (Eq)

loaderCase :: Side -> LoaderKind -> B.ByteString -> Word32 -> Case
loaderCase sd kind object c =
  Case
    { cId = (if kind == Fdl1 then "fdl1-" else "fdl2-") ++ sdTag sd ++ "-" ++ showHex c ""
    , cFamily = if kind == Fdl1 then "1 FDL loader FUN_01197f98" else "1 FDL loader FUN_0119f74c"
    , cProv =
        (if kind == Fdl1 then "Plain FUN_01197f98 over the font object final bytes of open-" else "Plain FUN_0119f74c over the raster context head [0x21000000, +0x130) final bytes of config-")
          ++ sdTag sd
          ++ " (horizontal: object vertical flag 0), r1 = code "
          ++ hx c
          ++ ", r2/r3/[sp]/[sp+4] = four u16 outputs at 0x23000000 seeded 0xee. Mapped font bytes: the code's and the next code's directory words"
          ++ (if isJust record then " and the 16 bytes from the glyph record" else "")
          ++ ", from "
          ++ iName font
          ++ "."
    , cEntry = entry
    , cArgs = [("r0", base), ("r1", c), ("r2", outBase), ("r3", outBase + 2)]
    , cSp = 0x000033f0
    , cRegs =
        [ stackReg 0x000033f0 [outBase + 4, outBase + 6]
        , codeReg "code" (sdPlain sd) entry size
        , synthReg (objId) "rw" base object (if kind == Fdl1 then "font object from open-" ++ sdTag sd else "raster context head from config-" ++ sdTag sd)
        , synthReg "out" "rw" outBase (B.replicate 8 0xee) "outputs adv_x, adv_y, vertical x, vertical y, 0xee"
        , dataReg "directory" "r" font (fontRam + dirOff + 4 * c) 8
        ]
          ++ [dataReg "record" "r" font (fontRam + off) len | Just (off, len) <- [record]]
    , cFns = [(entry, size)]
    , cStubs = []
    , cExpR = [("r0", if ldPresent ld then 1 else 0)]
    , cClaims =
        if ldPresent ld
          then
            [ (objId, countAt, le32 (ldCount ld) <> le32 (ldCursor ld))
            , ("out", 0, le16 (fst (ldAdv ld)) <> le16 (snd (ldAdv ld)) <> le32 0)
            ]
              ++ [(objId, 0x128, B.pack [0, 0]) | kind == Fdl2]
          else []
    , cEvents = []
    , cPin = []
    , cExpect =
        if ldPresent ld
          then "r0 = 1, count " ++ hx (ldCount ld) ++ ", cursor " ++ hx (ldCursor ld) ++ ", adv " ++ show (ldAdv ld) ++ ", vertical outputs 0 (directory flags " ++ hx (ldFlags ld) ++ " read from " ++ iName font ++ ")."
          else "r0 = 0, absent (zero directory span read from " ++ iName font ++ "), no output written."
    , cObserve = "loader"
    , cExcl = ["The loader is entered directly with the horizontal object; the vertical-alternate path, the rasterizer and drawing are outside the case."]
    , cInputs = [imgInput (sdPlain sd), imgInput font]
    }
  where
    font = sdFont sd
    ld = loadModel font c
    (entry, size, base, objId, countAt) =
      if kind == Fdl1 then (0x01197f98, 0x54c, structBase, "object", 0x60) else (0x0119f74c, 0x570, ctxBase, "context", 0xa0)
    record =
      if ldPresent ld && ldFlags ld /= 0
        then let off = commandsOff + ((wd font (fontRam + dirOff + 4 * c)) .&. 0xffffff) in Just (off, min 16 (0x80000 - fromIntegral off))
        else Nothing

-- ---------------------------------------------------------------------------
-- 5. Per-mille sizing read
-- ---------------------------------------------------------------------------

sizeCase :: Side -> B.ByteString -> B.ByteString -> Word32 -> Case
sizeCase sd ctxHead ctxProps c =
  Case
    { cId = "size-" ++ sdTag sd ++ "-" ++ showHex c ""
    , cFamily = "5 per-mille sizing"
    , cProv = "Plain FUN_011a023c (size adjust for a glyph of a format-4 font with property 10) on the " ++ sdTag sd ++ " plain image: r0 = the config-" ++ sdTag sd ++ " request block (height 30, context +0x40 = 0x21000000), r1 = code " ++ hx c ++ ", r2 = adjusted flag cell 0x23000000 seeded 0xee. The context head [0x21000000, +0x80) and property fields [0x210083d4, +0x18) are the config case's final bytes. The divide 0x0089aa90 and the re-configure FUN_0119fcbc are registers-only stubs."
    , cEntry = 0x011a023c
    , cArgs = [("r0", requestBase), ("r1", c), ("r2", outBase)]
    , cSp = 0x00003400
    , cRegs =
        [ stackReg 0x00003400 []
        , codeReg "code" (sdPlain sd) 0x011a023c 0x104
        , synthReg "request" "rw" requestBase requestBytes "raster request"
        , synthReg "context-head" "r" ctxBase ctxHead ("raster context head from config-" ++ sdTag sd)
        , synthReg "context-props" "r" (ctxBase + 0x83d4) ctxProps ("raster context property fields from config-" ++ sdTag sd)
        , synthReg "adjusted" "rw" outBase (B.replicate 4 0xee) "adjusted flag, 0xee"
        ]
          ++ [dataReg "per-mille" "r" font (fontRam + pmAt) 2 | inRange]
    , cFns = [(0x011a023c, 0x104)]
    , cStubs = [divStub | inRange] ++ [confStub | inRange]
    , cExpR = [("r0", 0)]
    , cClaims = ("adjusted", 0, le32 (if inRange then 1 else 0)) : [("request", 4, le32 q <> le32 q) | inRange]
    , cEvents = if inRange then [divStub, confStub] else []
    , cPin = []
    , cExpect =
        if inRange
          then "per-mille " ++ show pm ++ " at font " ++ hx pmAt ++ " (read from " ++ iName font ++ "): the divide is called with r0 = 1000*30 - 1 + " ++ show pm ++ " = " ++ show dividend ++ ", r1 = " ++ show pm ++ "; request width = height = " ++ show q ++ "; adjusted = 1; the re-configure returns 0 and so does the function."
          else "code outside U+F000..U+F725: adjusted = 0, no divide, no re-configure, r0 = 0."
    , cObserve = "sizing"
    , cExcl = ["The divide and the re-configure are stubs; their real bodies are outside the case. The stub returns are inputs: the divide returns the truncated quotient, the re-configure 0."]
    , cInputs = [imgInput (sdPlain sd), imgInput font]
    }
  where
    font = sdFont sd
    prop10 = propertyOffset font 10
    first = hw font (fontRam + prop10 + 6)
    lastC = hw font (fontRam + prop10 + 8)
    inRange = c >= first && c <= lastC
    pmAt = prop10 + 14 + 2 * (c - first)
    pm = hw font (fontRam + pmAt)
    dividend = 30 * 1000 - 1 + pm
    q = dividend `div` pm
    divStub = Stub 0x0089aa90 [("r0", dividend), ("r1", pm)] [("r0", q)]
    confStub = Stub 0x0119fcbc [("r0", requestBase)] [("r0", 0)]

-- ---------------------------------------------------------------------------
-- 2. Text resolution
-- ---------------------------------------------------------------------------

recordsBase :: Word32
recordsBase = 0x0037b784

-- FUN_01179b0c's dispatch read from the side's images.
textModel :: Side -> Word32 -> Word32
textModel sd i
  | i <= gate = locate
  | i + 0x01000000 <= 0x8c5 = glyphEntry i
  | otherwise = 0
  where
    plain = sdPlain sd
    gate = wd plain 0x01179b14 .&. 0xfff
    locate
      | i - 1 > 0x83d = 0
      | i > 0x83b = recordsBase + 66 * (i - 0x83c)
      | otherwise =
          let e = wd plain 0x01178aac + 8 * i
              r = wd (sdSlot sd) (e + 4)
           in if r == 0 then 0 else wd (sdSlot sd) r
    glyphEntry = glyphModel sd

glyphModel :: Side -> Word32 -> Word32
glyphModel sd i = wd (sdId2 sd) (glyphEntryAddr i + 8)

glyphEntryAddr :: Word32 -> Word32
glyphEntryAddr i = 0x0022a8b4 + 12 * (i + 0x01000000)

utf16Text :: B.ByteString -> String
utf16Text bs = concatMap unit (takeWhile (/= 0) [le16At bs (2 * k) | k <- [0 .. B.length bs `div` 2 - 1]])
  where
    unit u = if u >= 0x20 && u < 0x7f then [toEnum (fromIntegral u)] else "<" ++ showHex u "" ++ ">"

textCase :: Side -> Bool -> Word32 -> Case
textCase sd direct i =
  Case
    { cId = (if direct then "glyph-" else "text-") ++ sdTag sd ++ "-" ++ showHex i ""
    , cFamily = if direct then "2 glyph table" else "2 text resolution"
    , cProv = (if direct then "Plain FUN_0117e15c (glyph-id text table)" else "Plain FUN_01179b0c (text pointer accessor) with the record locator FUN_0117dfd8, the entry-pointer accessor FUN_0117deb0 and FUN_0117e15c") ++ " on the " ++ sdTag sd ++ " plain image, r0 = " ++ hx i ++ ", r1 = 0. Image 2 is " ++ iDesc (sdId2 sd) ++ "; the English slot is mapped at 0x0039c000."
    , cEntry = if direct then 0x0117e15c else 0x01179b0c
    , cArgs = [("r0", i)]
    , cSp = 0x00003400
    , cRegs =
        [stackReg 0x00003400 [], codeReg "code-glyph" plain 0x0117e15c 0x5c]
          ++ ( if direct
                 then []
                 else
                   [ codeReg "code-accessor" plain 0x01179b0c 120
                   , codeReg "code-locator" plain 0x0117dfd8 112
                   , codeReg "code-entry" plain 0x0117deb0 48
                   , dataReg "table-pointer" "r" plain 0x01178aac 4
                   , dataReg "records" "r" (sdId2 sd) recordsBase 198
                   , dataReg "slot" "r" (sdSlot sd) slotRam 0x20000
                   ]
             )
          ++ [dataReg "glyph-entry" "r" (sdId2 sd) (glyphEntryAddr i) 12 | glyphId]
    , cFns = [(0x0117e15c, 0x5c)] ++ (if direct then [] else [(0x01179b0c, 120), (0x0117dfd8, 112), (0x0117deb0, 48)])
    , cStubs = []
    , cExpR = [("r0", out)]
    , cClaims = []
    , cEvents = []
    , cPin = []
    , cExpect = "r0 = " ++ h32 out ++ note
    , cObserve = "text"
    , cExcl = ["The returned pointer is the result; metrics, drawing and the other gate sites are outside the case."]
    , cInputs = [imgInput plain, imgInput (sdId2 sd), imgInput (sdSlot sd)]
    }
  where
    plain = sdPlain sd
    glyphId = i + 0x01000000 <= 0x8c4
    out = if direct then glyphModel sd i else textModel sd i
    note
      | out == 0 = " (refused by the accessor's gate " ++ hx (wd plain 0x01179b14 .&. 0xfff) ++ ")."
      | out >= recordsBase && out < recordsBase + 198 = ", the inline record holding \"" ++ utf16Text (sl (sdId2 sd) out 56) ++ "\" (read from " ++ iName (sdId2 sd) ++ ")."
      | glyphId = ", glyph table entry " ++ h32 (glyphEntryAddr i) ++ " +8 of " ++ iName (sdId2 sd) ++ "."
      | otherwise = ", the English slot text \"" ++ utf16Text (sl (sdSlot sd) out 64) ++ "\" (table word " ++ h32 (wd plain 0x01178aac) ++ " + 8*id, read from the slot)."

-- ---------------------------------------------------------------------------
-- 3/4. Label getters
-- ---------------------------------------------------------------------------

nodeTable :: Word32
nodeTable = 0x0081babc

data Node = Node {ndName :: String, ndAddr :: Word32, ndIndex :: Word32}

nodes :: [Node]
nodes = [Node "shoot" 0x007823a4 0xbd, Node "fn" 0x00785d74 0x102, Node "type9" 0x0077d184 0x68, Node "raw" 0x0078bd5c 0x1b9, Node "q" 0x0078b000 0x201]

-- A pointer's image: image 4 or the font window loaded at 0x003bc000.
ptrImg :: Side -> Word32 -> Img
ptrImg sd a = if a >= fontRam && a < fontRam + 0x80000 then sdFont sd else sdId4 sd

nodeValues :: Side -> Node -> [Word32]
nodeValues sd nd = takeWhile (/= 0x7ffffffe) [wd img (v + 8 * k) | k <- [0 ..]]
  where
    v = wd (sdId4 sd) (ndAddr nd + 0x20)
    img = ptrImg sd v

nodeLabel :: Side -> Node -> Word32 -> Word32 -> Word32
nodeLabel sd nd row field = wd (ptrImg sd l) (l + 16 * row + 4 * field)
  where
    l = wd (sdId4 sd) (ndAddr nd + 0x24)

-- FUN_00bf93e0 with the node's own key: first row whose value matches, else row 0.
valueRow :: Side -> Node -> Word32 -> Word32
valueRow sd nd v = case [k | (k, x) <- zip [0 ..] (nodeValues sd nd), x == v] of
  (k : _) -> k
  [] -> 0

getterRegs :: Side -> Node -> [Reg]
getterRegs sd nd =
  [ dataReg "node-table" "r" (sdId4 sd) (nodeTable + 4 * ndIndex nd) 4
  , dataReg "node" "r" (sdId4 sd) (ndAddr nd) 48
  , dataReg "values" "r" (ptrImg sd vals) vals (8 * (length (nodeValues sd nd) + 1))
  , dataReg "labels" "r" (ptrImg sd labs) labs (16 * (length (nodeValues sd nd) + 1))
  ]
  where
    vals = wd (sdId4 sd) (ndAddr nd + 0x20)
    labs = wd (sdId4 sd) (ndAddr nd + 0x24)

rowCase :: Side -> Node -> Word32 -> Word32 -> Case
rowCase sd nd row field =
  Case
    { cId = "row-" ++ sdTag sd ++ "-" ++ ndName nd ++ "-r" ++ show row ++ "-f" ++ show field
    , cFamily = if ndName nd == "q" then "4 Q row getter" else "3 label row getter"
    , cProv = "Id5 FUN_00bf9360 (label row getter) on the " ++ sdTag sd ++ " image 5 for node " ++ h32 (ndAddr nd) ++ " (index " ++ hx (ndIndex nd) ++ " of table 0x0081babc), r1 = row " ++ show row ++ ", r2 = field " ++ show field ++ "; node, values and labels arrays from " ++ iName (sdId4 sd) ++ (if labs >= fontRam then " and the labels array from " ++ iName (sdFont sd) else "") ++ "."
    , cEntry = 0x00bf9360
    , cArgs = [("r0", ndIndex nd), ("r1", row), ("r2", field)]
    , cSp = 0x00003400
    , cRegs = [stackReg 0x00003400 [], codeReg "code" (sdId5 sd) 0x00bf9360 0x80] ++ getterRegs sd nd
    , cFns = [(0x00bf9360, 0x80)]
    , cStubs = []
    , cExpR = [("r0", out)]
    , cClaims = []
    , cEvents = []
    , cPin = []
    , cExpect = "r0 = " ++ hx out ++ " (labels " ++ h32 labs ++ " + 16*" ++ show row ++ " + 4*" ++ show field ++ ")."
    , cObserve = "label id"
    , cExcl = ["The returned string id is the result; its drawing is outside the case."]
    , cInputs = [imgInput (sdId5 sd), imgInput (sdId4 sd)] ++ [imgInput (sdFont sd) | labs >= fontRam]
    }
  where
    out = nodeLabel sd nd row field
    labs = wd (sdId4 sd) (ndAddr nd + 0x24)

valCase :: Side -> Node -> Word32 -> Word32 -> Case
valCase sd nd v field =
  Case
    { cId = "val-" ++ sdTag sd ++ "-" ++ ndName nd ++ "-v" ++ showHex v "" ++ "-f" ++ show field
    , cFamily = if ndName nd == "q" then "4 Q value getter" else "3 label value getter"
    , cProv = "Id5 FUN_00bf93e0 (label value getter) with its counter FUN_00bf9794 on the " ++ sdTag sd ++ " image 5 for node " ++ h32 (ndAddr nd) ++ " (index " ++ hx (ndIndex nd) ++ "), r1 = the node's key " ++ hx key ++ ", r2 = value " ++ hx v ++ ", r3 = field " ++ show field ++ "."
    , cEntry = 0x00bf93e0
    , cArgs = [("r0", ndIndex nd), ("r1", key), ("r2", v), ("r3", field)]
    , cSp = 0x00003400
    , cRegs = [stackReg 0x00003400 [], codeReg "code-getter" (sdId5 sd) 0x00bf93e0 0x1c8, codeReg "code-counter" (sdId5 sd) 0x00bf9794 0xc8] ++ getterRegs sd nd
    , cFns = [(0x00bf93e0, 0x1c8), (0x00bf9794, 0xc8)]
    , cStubs = []
    , cExpR = [("r0", out)]
    , cClaims = []
    , cEvents = []
    , cPin = []
    , cExpect = "r0 = " ++ hx out ++ " (value " ++ hx v ++ " is " ++ (if v `elem` nodeValues sd nd then "row " ++ show row else "absent, row 0") ++ " of " ++ show (nodeValues sd nd) ++ ")."
    , cObserve = "label id"
    , cExcl = ["The returned string id is the result; the walk over other nodes for a foreign key is not exercised."]
    , cInputs = [imgInput (sdId5 sd), imgInput (sdId4 sd)]
    }
  where
    key = wd (sdId4 sd) (ndAddr nd + 8)
    row = valueRow sd nd v
    out = nodeLabel sd nd row field

-- ---------------------------------------------------------------------------
-- Comparisons over the expectations
-- ---------------------------------------------------------------------------

compareSides :: World -> IO [String]
compareSides w = do
  let cc = wCc w
      comb = wComb w
      stock = wStock w
      stockFont = wStockFont w
      combFont = sdFont comb
      fonts = censusCodes stockFont combFont
      full = [c | c <- [0 .. glyphCount - 1], loadModel stockFont c /= loadModel combFont c]
  ensure (full == [0xf072, 0xf073]) ("loader model, all codes: stock and combined fonts differ at " ++ show (map hx full))
  let f073 = loadModel combFont 0xf073
      f072 = loadModel combFont 0xf072
  ensure (ldPresent f073 && ldCount f073 == 0x172 && ldCursor f073 == 0x7cfe6 && ldAdv f073 == (0, 0)) "U+F073 combined model"
  ensure (not (ldPresent (loadModel stockFont 0xf073)) && not (ldPresent (loadModel stockFont 0xf072))) "U+F072 and U+F073 stock model"
  -- Getters: every row and value present on both CC and combined nodes.
  let cells nd sd = [(("row", r, f), nodeLabel sd nd r f) | r <- [0 .. fromIntegral (length (nodeValues sd nd)) - 1], f <- [0 .. 3]] ++ [(("val", v, f), nodeLabel sd nd (valueRow sd nd v) f) | v <- nodeValues sd nd, f <- [0 .. 3]]
      diffs nd = M.toList (M.filter id (M.intersectionWith (/=) (M.fromList (cells nd cc)) (M.fromList (cells nd comb))))
      expectDiff "raw" = [(("row", 5, 1), True), (("val", 0xa, 1), True)]
      expectDiff "q" = []
      expectDiff _ = [(("row", 7, 0), True), (("row", 7, 1), True), (("val", 2, 0), True), (("val", 2, 1), True)]
  forM_ nodes $ \nd -> ensure (diffs nd == expectDiff (ndName nd)) (ndName nd ++ ": CC/combined cells differ at " ++ show [(k, hx r, f) | ((k, r, f), _) <- diffs nd])
  forM_ (init nodes) $ \nd -> forM_ (expectDiff (ndName nd)) $ \((k, r, f), _) ->
    ensure (nodeLabel comb nd (if k == "row" then r else valueRow comb nd r) f == 0x83d) (ndName nd ++ ": combined " ++ k ++ " " ++ hx r ++ " field " ++ show f ++ " is not 0x83d")
  let q = last nodes
      stockQ = [0xff00077b, 0xff00077c, 0xff000773, 0xff000777, 0xff00077a]
  ensure (cells q stock == cells q cc) "Q node: stock and CC cells differ"
  ensure ([nodeLabel stock q r 0 | r <- [0 .. 4]] == stockQ && length (nodeValues stock q) == 5) "stock Q rows 0-4 are not the five GVFF ids"
  ensure (nodeValues comb q == [0, 3, 1, 7, 0xc, 5, 6, 2]) "combined Q values"
  ensure ([nodeLabel comb q r 0 | r <- [0 .. 7]] == stockQ ++ [0x49a, 0x496, 0x83d]) "combined Q labels"
  pure
    [ show (length fonts) ++ " census codes; the loader model over all " ++ show glyphCount ++ " codes differs between the stock and combined fonts at U+F072 and U+F073 only."
    , "U+F072 on the combined font: present, directory flags " ++ hx (ldFlags f072) ++ ", count " ++ hx (ldCount f072) ++ ", cursor " ++ hx (ldCursor f072) ++ " (stock: absent)."
    , "U+F073 on the combined font: present, directory flags " ++ hx (ldFlags f073) ++ ", count " ++ hx (ldCount f073) ++ ", cursor " ++ hx (ldCursor f073) ++ ", advance " ++ show (ldAdv f073) ++ " (stock: absent)."
    ]

-- ---------------------------------------------------------------------------
-- 7. Q tiles: glyph record accessors, label kind and the Q label drawer
-- ---------------------------------------------------------------------------

-- The Q glyphs of the stock tiles, the two PRO Neg tiles and the Classic
-- Chrome tile (glyph 0x37c).
qTileGlyphs :: [QGlyph]
qTileGlyphs = map QGlyph [0x77b, 0x77c, 0x773, 0x777, 0x77a, 0x779, 0x778, 0x37c]

-- The record word an accessor returns: width and height halfwords of word 0,
-- the type word, the pointer word.
data QField = QWidth | QHeight | QType | QPointer
  deriving (Eq, Show)

qFieldEntry :: QField -> Word32
qFieldEntry f = case f of
  QWidth -> 0x0117e048
  QHeight -> 0x0117e0a4
  QType -> 0x0117e100
  QPointer -> 0x0117e15c

qRecordWord :: Side -> QGlyph -> QField -> Word32
qRecordWord sd g f = case f of
  QWidth -> wd img rec .&. 0xffff
  QHeight -> wd img rec `shiftR` 16
  QType -> wd img (rec + 4)
  QPointer -> wd img (rec + 8)
  where
    img = sdId2 sd
    rec = qGlyphRecordAddress g

qrecCase :: Side -> QField -> QGlyph -> Case
qrecCase sd f g@(QGlyph n) =
  Case
    { cId = "qrec-" ++ sdTag sd ++ "-" ++ map toLowerAscii (drop 1 (show f)) ++ "-" ++ showHex n ""
    , cFamily = "7 Q glyph record accessors"
    , cProv = "Plain FUN_" ++ drop 2 (h32 (qFieldEntry f)) ++ " (Q glyph record " ++ map toLowerAscii (drop 1 (show f)) ++ ") on the " ++ sdTag sd ++ " plain image, r0 = " ++ h32 label ++ "; the record is image-2 " ++ h32 (qGlyphRecordAddress g) ++ " of " ++ iName (sdId2 sd) ++ "."
    , cEntry = qFieldEntry f
    , cArgs = [("r0", label)]
    , cSp = 0x00003400
    , cRegs = [stackReg 0x00003400 [], codeReg "code" (sdPlain sd) (qFieldEntry f) 0x5c, dataReg "glyph-entry" "r" (sdId2 sd) (qGlyphRecordAddress g) 12]
    , cFns = [(qFieldEntry f, 0x5c)]
    , cStubs = []
    , cExpR = [("r0", out)]
    , cClaims = []
    , cEvents = []
    , cPin = []
    , cExpect = "r0 = " ++ hx out ++ " (record " ++ h32 (qGlyphRecordAddress g) ++ " read from " ++ iName (sdId2 sd) ++ ")."
    , cObserve = "record word"
    , cExcl = ["The returned record word is the result; the drawing of the record's GVFF block is outside the case."]
    , cInputs = [imgInput (sdPlain sd), imgInput (sdId2 sd)]
    }
  where
    label = qGlyphLabel g
    out = qRecordWord sd g f

toLowerAscii :: Char -> Char
toLowerAscii c = if c >= 'A' && c <= 'Z' then toEnum (fromEnum c + 32) else c

-- FUN_01179a50's label kind read from the side's images: 1 text (string id at
-- or below the gate, or 0xfe000000), 2 bitmap (0xfd000000/1, or a Q record of
-- another type), 5 and 6 the Q record types 1 and 2, 0 otherwise.
kindModel :: Side -> Word32 -> Word32
kindModel sd i
  | i == 0xfe000000 || i <= gate = 1
  | i + 0x03000000 <= 1 = 2
  | i + 0x01000000 <= 0x8c5 = case wd (sdId2 sd) (glyphEntryAddr i + 4) of
      1 -> 5
      2 -> 6
      _ -> 2
  | otherwise = 0
  where
    gate = wd (sdPlain sd) 0x01179a58 .&. 0xfff

kindCase :: Side -> Word32 -> Case
kindCase sd i =
  Case
    { cId = "kind-" ++ sdTag sd ++ "-" ++ showHex i ""
    , cFamily = "8 Q label kind"
    , cProv = "Plain FUN_01179a50 (label kind) with FUN_0117e100 on the " ++ sdTag sd ++ " plain image, r0 = " ++ h32 i ++ "."
    , cEntry = 0x01179a50
    , cArgs = [("r0", i)]
    , cSp = 0x00003400
    , cRegs = [stackReg 0x00003400 [], codeReg "code-kind" (sdPlain sd) 0x01179a50 0x64, codeReg "code-type" (sdPlain sd) 0x0117e100 0x5c] ++ [dataReg "glyph-entry" "r" (sdId2 sd) (glyphEntryAddr i) 12 | qGlyph]
    , cFns = [(0x01179a50, 0x64), (0x0117e100, 0x5c)]
    , cStubs = []
    , cExpR = [("r0", out)]
    , cClaims = []
    , cEvents = []
    , cPin = []
    , cExpect = "r0 = " ++ hx out ++ (if qGlyph then " (Q record type " ++ hx (wd (sdId2 sd) (glyphEntryAddr i + 4)) ++ ")." else " (string id at or below the gate " ++ hx (wd (sdPlain sd) 0x01179a58 .&. 0xfff) ++ ").")
    , cObserve = "label kind"
    , cExcl = ["The returned kind is the result; the renderer it selects is outside the case."]
    , cInputs = [imgInput (sdPlain sd), imgInput (sdId2 sd)]
    }
  where
    qGlyph = i + 0x01000000 <= 0x8c4
    out = kindModel sd i

-- FUN_011121d4 with r0 = r1 = 1 (cell coordinates 0, 0), r2 = the label, r3
-- = 1: a Q glyph draws at its record's width and height, a string id at its
-- layout's +0x3c width and +0x3e height (FUN_0117dce0, FUN_0117dd48), read
-- here from the side's image 2. The draw FUN_01179da0 is a register-only stub
-- whose inputs r0-r3 are the position and the size.
drawSize :: Side -> Word32 -> (Word32, Word32)
drawSize sd i
  | i + 0x01000000 <= 0x8c5 = (qRecordWord sd (QGlyph (fromIntegral (i .&. 0xffff))) QWidth, qRecordWord sd (QGlyph (fromIntegral (i .&. 0xffff))) QHeight)
  | otherwise = (hw img (layout + 4), hw img (layout + 6))
  where
    img = sdId2 sd
    layout = if i > 0x83b then recordsBase + 66 * (i - 0x83c) + 0x38 else 0x00130000 + 10 * i

qdrawCase :: Side -> Word32 -> Case
qdrawCase sd i =
  Case
    { cId = "qdraw-" ++ sdTag sd ++ "-" ++ showHex i ""
    , cFamily = "9 Q label drawer"
    , cProv = "Plain FUN_011121d4 (Q label drawer) on the " ++ sdTag sd ++ " plain image, r0 = 1, r1 = 1, r2 = " ++ h32 i ++ ", r3 = 1; " ++ (if qGlyph then "the Q glyph record accessors FUN_0117e048 and FUN_0117e0a4 over image-2 " ++ h32 (glyphEntryAddr i) else "the layout accessors FUN_0117ddb0, FUN_0117dce0 and FUN_0117dd48 over image-2 " ++ h32 layoutAt) ++ "; FUN_01179da0 is a register-only stub."
    , cEntry = 0x011121d4
    , cArgs = [("r0", 1), ("r1", 1), ("r2", i), ("r3", 1)]
    , cSp = 0x00003400
    , cRegs =
        [stackReg 0x00003400 [], codeReg "code-drawer" plain 0x011121d4 0x198]
          ++ ( if qGlyph
                 then [codeReg "code-width" plain 0x0117e048 0x5c, codeReg "code-height" plain 0x0117e0a4 0x5c, dataReg "glyph-entry" "r" (sdId2 sd) (glyphEntryAddr i) 12]
                 else [codeReg "code-layout" plain 0x0117dce0 0x138, dataReg "layout" "r" (sdId2 sd) layoutAt 10]
             )
    , cFns = [(0x011121d4, 0x198)] ++ (if qGlyph then [(0x0117e048, 0x5c), (0x0117e0a4, 0x5c)] else [(0x0117dce0, 0x68), (0x0117dd48, 0x68), (0x0117ddb0, 0x68)])
    , cStubs = [drawStub]
    , cExpR = [("r0", 0)]
    , cClaims = []
    , cEvents = [drawStub]
    , cPin = []
    , cExpect = "one FUN_01179da0 call with r0 = 0, r1 = 0, r2 = " ++ show w ++ ", r3 = " ++ show h ++ " (" ++ (if qGlyph then "the Q record's width and height" else "the label layout's +0x3c and +0x3e halfwords") ++ "), then r0 = 0."
    , cObserve = "drawn size"
    , cExcl = ["The draw call's position and size registers are the result; the stacked style, id and flag words, the renderer and the pixels are outside the case."]
    , cInputs = [imgInput plain, imgInput (sdId2 sd)]
    }
  where
    plain = sdPlain sd
    qGlyph = i + 0x01000000 <= 0x8c4
    (w, h) = drawSize sd i
    layoutAt = if i > 0x83b then recordsBase + 66 * (i - 0x83c) + 0x38 else 0x00130000 + 10 * i
    drawStub = Stub 0x01179da0 [("r0", 0), ("r1", 0), ("r2", w), ("r3", h)] [("r0", 0)]

-- Generator-side checks of the Q-tile side against the combined side.
checkQTile :: World -> IO [String]
checkQTile w = do
  let comb = wComb w
      qt = wQ w
      stock = wStock w
      q = last nodes
      stockQ = [0xff00077b, 0xff00077c, 0xff000773, 0xff000777, 0xff00077a]
      diffAt a b = [k | k <- [0 .. B.length (iBytes a) - 1], B.index (iBytes a) k /= B.index (iBytes b) k]
      id2Diff = diffAt (sdId2 comb) (sdId2 qt)
      fontDiff = diffAt (sdFont comb) (sdFont qt)
      FontOffset labelsAt = qLabelsOffset
      FontOffset tileAt = qTileOffset
      inFontWrites k = k >= labelsAt + 0x50 && k < labelsAt + 0x80 || k >= tileAt && k < tileAt + B.length qTileGvff
      pointer = qRecordWord qt (QGlyph 0x37c) QPointer
  ensure (nodeValues qt q == nodeValues comb q) "Q-tile Q values differ from the combined side's"
  ensure ([nodeLabel qt q r 0 | r <- [0 .. 7]] == stockQ ++ [0xff000779, 0xff000778, 0xff00037c]) "Q-tile Q labels"
  ensure ([nodeLabel qt q r f | r <- [0 .. 7], f <- [1 .. 3]] == [nodeLabel comb q r f | r <- [0 .. 7], f <- [1 .. 3]]) "Q-tile Q label fields 1-3 differ from the combined side's"
  ensure (all (\k -> k >= 0x0022d28c - 0x00130000 && k < 0x0022d290 - 0x00130000) id2Diff && not (null id2Diff)) ("Q-tile image 2 differs from the combined side outside the 0x37c pointer word: " ++ show (take 4 id2Diff))
  ensure (all inFontWrites fontDiff) "Q-tile font window differs from the combined side outside the Q label words and the tile"
  ensure (and [qRecordWord stock g f == qRecordWord qt g f | g <- qTileGlyphs, g /= QGlyph 0x37c, f <- [QWidth, QHeight, QType, QPointer]]) "a stock Q tile record differs between the stock and Q-tile sides"
  ensure (and [(qRecordWord qt g QWidth, qRecordWord qt g QHeight, qRecordWord qt g QType) == (80, 48, 1) | g <- qTileGlyphs]) "a Q-row glyph record is not 80 x 48 type 1"
  ensure (pointer == qTileAddress) "the 0x37c record does not point at the tile"
  tile <- either failH pure (decodeGvff (B.drop (fromIntegral (pointer - fontRam)) (iBytes (sdFont qt))))
  ensure (gvffBounds tile == [0, 0, 80, -48] && B.take (gvffLength tile) (B.drop (fromIntegral (pointer - fontRam)) (iBytes (sdFont qt))) == qTileGvff) "the block at the 0x37c pointer is not the encoded tile"
  forM_ [QGlyph 0x778, QGlyph 0x779] $ \g -> do
    let p = qRecordWord qt g QPointer
    block <- either (failH . (("Q glyph " ++ show g ++ ": ") ++)) pure (decodeGvff (B.drop (fromIntegral (p - 0x00130000)) (iBytes (sdId2 qt))))
    ensure (gvffBounds block == [0, 0, 80, -48]) ("Q glyph " ++ show g ++ ": bounds " ++ show (gvffBounds block))
  pure
    [ "Q-tile side: Q labels rows 5-7 are 0xff000779, 0xff000778, 0xff00037c; image 2 differs from the combined side in " ++ show (length id2Diff) ++ " bytes of the 0x37c pointer word; the font window in " ++ show (length fontDiff) ++ " bytes of the Q label words and the " ++ show (B.length qTileGvff) ++ "-byte tile."
    , "Q glyph 0x37c points at " ++ h32 pointer ++ ", a GVFF block with bounds (0, 0, 80, -48) equal to the encoded tile; Q glyphs 0x778 and 0x779 are stock 80 x 48 type-1 tiles in image 2."
    ]

-- ---------------------------------------------------------------------------
-- 6. Frames-remaining size class and its validator
-- ---------------------------------------------------------------------------

mapperEntry, validatorEntry :: Word32
mapperEntry = 0x00e42414
validatorEntry = 0x00ed5868

mapperInputs :: [Word32]
mapperInputs = [0 .. 15] ++ [0x7fffffff, 0x80000000, 0xffffffff]

-- Report section 9.4: col2 0, 1, 2, 3, 5, 6, 11, 12 -> 0; 4, 7, 8, 9, 10 -> 2;
-- anything else -> 6. The fix maps 13 to 0 and keeps every other class.
stockClass, fixedClass :: Word32 -> Word32
stockClass x
  | x `elem` [0, 1, 2, 3, 5, 6, 11, 12] = 0
  | x `elem` [4, 7, 8, 9, 10] = 2
  | otherwise = 6
fixedClass x = if x == 13 then 0 else stockClass x

-- ARM condition codes over NZCV.
condHolds :: Word32 -> (Bool, Bool, Bool, Bool) -> Bool
condHolds cond (nf, zf, cf, vf) = case cond of
  0 -> zf
  1 -> not zf
  2 -> cf
  3 -> not cf
  4 -> nf
  5 -> not nf
  6 -> vf
  7 -> not vf
  8 -> cf && not zf
  9 -> not cf || zf
  10 -> nf == vf
  11 -> nf /= vf
  12 -> not zf && nf == vf
  13 -> zf || nf /= vf
  _ -> True

-- FUN_00e42414 read from an image: after the 8-byte prologue, triples
-- `cmp r0,#k; moveq r0,#c; beq epilogue` and then a tail `cmp r0,#k` with
-- two conditional `mov r0,#imm` before the epilogue `add sp,r11,#0`.
mapperModel :: Img -> Word32 -> Word32
mapperModel img x = go (mapperEntry + 8)
  where
    isCmp v = v .&. 0xffffff00 == 0xe3500000
    isMov v = v .&. 0x0fffff00 == 0x03a00000
    go a
      | isCmp w0 && w1 .&. 0xffffff00 == 0x03a00000 && w2 .&. 0xff000000 == 0x0a000000 =
          if x == w0 .&. 0xff then w1 .&. 0xff else go (a + 12)
      | isCmp w0 && isMov w1 && isMov w2 && w3 == 0xe28bd000 =
          let k = w0 .&. 0xff
              res = x - k
              flags = (testBit res 31, res == 0, x >= k, testBit ((x `xor` k) .&. (x `xor` res)) 31)
              step r v = if condHolds (v `shiftR` 28) flags then v .&. 0xff else r
           in step (step x w1) w2
      | otherwise = error (iName img ++ ": FUN_00e42414 has no compare chain at " ++ h32 a)
      where
        [w0, w1, w2, w3] = [wd img (a + 4 * k) | k <- [0 .. 3]]

inputTag :: Word32 -> String
inputTag x = if x > 0xff then showHex x "" else let t = showHex x "" in replicate (2 - length t) '0' ++ t

mapCase :: String -> Img -> Word32 -> Case
mapCase tag img x =
  Case
    { cId = "map-" ++ tag ++ "-" ++ inputTag x
    , cFamily = "6 frames size class FUN_00e42414"
    , cProv = "Id5 FUN_00e42414 (film col2 to size class; FUN_00e4361c calls it at 0x00e43960 with block +0x10 and stores the class at its struct +0xc) on " ++ iDesc img ++ ", r0 = col2 " ++ hx x ++ "."
    , cEntry = mapperEntry
    , cArgs = [("r0", x)]
    , cSp = 0x00003400
    , cRegs = [stackReg 0x00003400 [], codeReg "code" img mapperEntry 0xb0]
    , cFns = [(mapperEntry, 0xb0)]
    , cStubs = []
    , cExpR = [("r0", out)]
    , cClaims = []
    , cEvents = []
    , cPin = []
    , cExpect = "r0 = class " ++ show out ++ " (compare chain read from " ++ iName img ++ "; stock class " ++ show (stockClass x) ++ ", fixed class " ++ show (fixedClass x) ++ ")."
    , cObserve = "size class"
    , cExcl = ["The mapper is entered directly; FUN_00e4361c's loop, the frames-remaining table 0x009432a8 and the second caller 0x00e43e48 are outside the case."]
    , cInputs = [imgInput img]
    }
  where
    out = mapperModel img x

blockBase :: Word32
blockBase = 0x20000000

validCase :: String -> Img -> Word32 -> Word32 -> Case
validCase tag img x cls =
  Case
    { cId = "valid-" ++ tag ++ "-" ++ inputTag x
    , cFamily = "6 frames size-class validator FUN_00ed5868"
    , cProv = "Id5 FUN_00ed5868 (FUN_00ed5b0c's first check; FUN_00ed5b0c returns 0 when it returns -1) on " ++ iDesc img ++ ", r0 = a 0x38-byte struct at 0x20000000, zero except +0xc = " ++ show cls ++ ", the class case map-" ++ tag ++ "-" ++ inputTag x ++ " returned for col2 " ++ hx x ++ "."
    , cEntry = validatorEntry
    , cArgs = [("r0", blockBase)]
    , cSp = 0x00003400
    , cRegs = [stackReg 0x00003400 [], codeReg "code" img validatorEntry 0x78, synthReg "struct" "r" blockBase (B.replicate 12 0 <> le32 cls <> B.replicate 0x28 0) "frames struct, zero except +0xc"]
    , cFns = [(validatorEntry, 0x78)]
    , cStubs = []
    , cExpR = [("r0", out)]
    , cClaims = []
    , cEvents = []
    , cPin = []
    , cExpect = "r0 = " ++ h32 out ++ " (+0 = 0 selects the field checks; class " ++ show cls ++ (if cls <= 5 then " <= 5 and +0x10, +0x14, +0x34 = 0 in range: accepted)." else " > 5: rejected, -1).")
    , cObserve = "validator"
    , cExcl = ["The struct's fields other than +0xc are 0 and in range; the values FUN_00e4361c stores there are not modelled."]
    , cInputs = [imgInput img]
    }
  where
    out = if cls <= 5 then 0 else 0xffffffff

-- Generator-side checks of the mapper over the sides' images.
checkFrames :: World -> IO [String]
checkFrames w = do
  let table img = map (mapperModel img) mapperInputs
      stockT = map stockClass mapperInputs
      fixedT = map fixedClass mapperInputs
  ensure (table (sdId5 (wStock w)) == stockT && table (sdId5 (wCc w)) == stockT) "FUN_00e42414 on the stock or CC image differs from report section 9.4"
  ensure (table (sdId5 (wComb w)) == fixedT) "FUN_00e42414 on the combined image is not the stock table with 13 -> 0"
  forM_ (wPre w) $ \(tag, img) -> ensure (and (zipWith3 (\v a b -> v == a || v == b) (table img) stockT fixedT)) (tag ++ ": a class is neither the stock nor the fixed one")
  pure ["FUN_00e42414 " ++ tag ++ ": " ++ unwords [inputTag x ++ "->" ++ show c | (x, c) <- zip mapperInputs (table img)] | (tag, img) <- [("stock", sdId5 (wStock w)), ("cc", sdId5 (wCc w)), ("combined", sdId5 (wComb w))] ++ wPre w]

-- ---------------------------------------------------------------------------

main :: IO ()
main = do
  o <- getArgs >>= parseOpts defaults
  w <- loadWorld o
  checkPlain w
  notes <- concat <$> sequence [compareSides w, checkFrames w, checkQTile w]
  mapM_ putStrLn notes
  let out = oOut o
      images = out </> "images"
      comb = wComb w
  createDirectoryIfMissing True images
  stale <- filter (\f -> takeExtension f == ".json" || (take 2 f == "a-" && takeExtension f == ".bin")) <$> listDirectory out
  mapM_ (removeFile . (out </>)) stale
  imageNotes <- forM [("patched-id4.bin", sdId4 comb), ("patched-id5.bin", sdId5 comb), ("patched-plain.bin", sdPlain comb), ("id2.bin", sdId2 comb), ("font-window.bin", sdFont comb), ("q-tile-id2.bin", sdId2 (wQ w)), ("q-tile-font-window.bin", sdFont (wQ w))] $ \(file, img) -> do
    B.writeFile (images </> file) (iBytes img)
    pure (file ++ ": SHA-256 " ++ iSha img ++ ", base " ++ h32 (iBase img) ++ ", " ++ iDesc img ++ ".")
  mapM_ putStrLn imageNotes
  runner <- makeAbsolute (oRunnerPath o)
  arts <- newMVar M.empty
  let env = Env out runner arts
      fontSides = [wCc w, comb]
      allSides = [wStock w, wCc w, comb]
      mapSides = [(sdTag sd, sdId5 sd) | sd <- allSides] ++ wPre w
  loader <- loadImg "loader-40000" (oPrivate o </> "analysis/x-m1-1.50/decode/loader-40000.bin") 0x00040000 loaderSha
  -- Stage A: object and context builders, and the size-class mapper.
  stageA <- runAll env (oWorkers o) (concat [[openCase sd, configCase sd loader] | sd <- fontSides] ++ [mapCase tag img x | (tag, img) <- mapSides, x <- mapperInputs])
  let finalOf cid rid = case [oFinals x M.! rid | x <- stageA, cId (oCase x) == cid, oStatus x == "passed"] of
        (f : _) -> pure f
        [] -> failH (cid ++ " did not pass; the dependent cases cannot start")
  built <- forM fontSides $ \sd -> do
    object <- finalOf ("open-" ++ sdTag sd) "object"
    ctx <- finalOf ("config-" ++ sdTag sd) "context"
    pure (sd, object, ctx)
  validCases <- forM [(sdTag sd, sdId5 sd, x) | sd <- allSides, x <- mapperInputs] $ \(tag, img, x) ->
    case [oR0 r | r <- stageA, cId (oCase r) == cId (mapCase tag img x), oStatus r == "passed"] of
      [r0] | [(cls, "")] <- readHex (drop 2 r0) -> pure (validCase tag img x cls)
      _ -> failH ("map-" ++ tag ++ "-" ++ inputTag x ++ " did not pass; its validator case cannot start")
  let codes = censusCodes (wStockFont w) (sdFont comb)
      loaderCases = concat [[loaderCase sd Fdl1 object c, loaderCase sd Fdl2 (B.take 0x130 ctx) c] | (sd, object, ctx) <- built, c <- codes]
      sizeCases = [sizeCase sd (B.take 0x80 ctx) (B.take 0x18 (B.drop 0x83d4 ctx)) c | (sd, _, ctx) <- built, c <- [0xf073, 0xf06a, 0xf072, 0xe120]]
      textIds = [0x83d, 0x83e, 0x489, 0x48a, 0x49a, 0x496, 0x48b, 0xff00077b, 0xff00077c, 0xff000773, 0xff000777, 0xff00077a]
      textCases = [textCase sd False i | sd <- allSides, i <- textIds] ++ [textCase sd True i | sd <- allSides, i <- [0xff00077b, 0xff00077c, 0xff000773, 0xff000777, 0xff00077a]]
      getterSides nd = if ndName nd == "q" then allSides ++ [wQ w] else fontSides
      getterCases =
        concat
          [ [rowCase sd nd r f | r <- [0 .. fromIntegral (length (nodeValues sd nd)) - 1], f <- [0 .. 3]]
              ++ [valCase sd nd v f | v <- S.toList (S.fromList (nodeValues sd nd ++ [2, 0xd])), f <- [0 .. 3]]
          | nd <- nodes
          , sd <- getterSides nd
          ]
      qSides = [comb, wQ w]
      qrecCases = [qrecCase sd f g | sd <- [wStock w, comb, wQ w], f <- [QWidth, QHeight, QType, QPointer], g <- qTileGlyphs]
      kindCases = [kindCase sd (nodeLabel sd (last nodes) r 0) | sd <- qSides, r <- [0 .. 7]]
      qdrawCases = [qdrawCase sd (nodeLabel sd (last nodes) r 0) | sd <- qSides, r <- [0 .. 7]]
  stageB <- runAll env (oWorkers o) (validCases ++ textCases ++ getterCases ++ qrecCases ++ kindCases ++ qdrawCases ++ sizeCases ++ loaderCases)
  let outcomes = stageA ++ stageB
      row r = "| " ++ cId (oCase r) ++ " | " ++ h32 (cEntry (oCase r)) ++ " | " ++ intercalate ", " [k ++ "=" ++ hx v | (k, v) <- cArgs (oCase r)] ++ " | " ++ cExpect (oCase r) ++ " | " ++ oStatus r ++ ", " ++ oRunner r ++ ", r0 " ++ oR0 r ++ ", " ++ show (oInstr r) ++ " instructions |"
      families = M.toList (M.fromListWith (\(a, b) (c, d) -> (a + c, b + d)) [(cFamily (oCase r), (if oStatus r == "passed" then 1 else 0, 1 :: Int)) | r <- outcomes])
      familyRows = ["| " ++ f ++ " | " ++ show p ++ "/" ++ show t ++ " |" | (f, (p, t)) <- families]
  writeFile (out </> "results.md") (unlines (notes ++ [""] ++ imageNotes ++ ["", "| Family | Passed |", "| --- | --- |"] ++ familyRows ++ ["", "| Case | Entry | Inputs | Expected before execution | Observed |", "| --- | --- | --- | --- | --- |"] ++ map row outcomes))
  mapM_ putStrLn familyRows
  B.writeFile
    (out </> "results.json")
    ( encodeCanonical
        ( arr
            [ obj
                [ ("id", s (cId (oCase r)))
                , ("family", s (cFamily (oCase r)))
                , ("entry", s (h32 (cEntry (oCase r))))
                , ("arguments", regsJ (cArgs (oCase r)))
                , ("expected", s (cExpect (oCase r)))
                , ("inputs", obj [(k, s v) | (k, v) <- cInputs (oCase r)])
                , ("status", s (oStatus r))
                , ("runner_status", s (oRunner r))
                , ("r0", s (oR0 r))
                , ("instructions", n (oInstr r))
                , ("mismatches", arr (map s (oMismatch r)))
                ]
            | r <- outcomes
            ]
        )
    )
  let failed = [r | r <- outcomes, oStatus r /= "passed"]
  putStrLn (show (length outcomes - length failed) ++ "/" ++ show (length outcomes) ++ " cases passed")
  unless (null failed) exitFailure
