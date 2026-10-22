{-# LANGUAGE ScopedTypeVariables #-}
-- Native-harness cases for the individual Classic Chrome patch sites
-- (reports/classic-chrome-patch.md sections 4 and 9), X-M1 1.50.
--
-- Every case executes pinned stock bytes or the patched decodes of a
-- build-classic-chrome output in the bounded runner, one stock function
-- invocation per case. Expected registers and claimed memory are computed here
-- from byte reads of the images, the patch contract words and the parameter
-- files, before any run. The stack region, and for the plain request builder
-- the request bytes outside the claimed word, are pinned from a discovery run:
-- they are residue the exact-final-memory contract requires, not claims. The
-- patched request-builder cases take that residue from the stock case with the
-- same input, so their unclaimed request bytes assert equality with stock.
--
-- Families:
--   conv-*      FUN_00738fe8 / FUN_00736a74 converters, inputs 0..15, 0xffffffff
--   validator-* FUN_00eee958 on stock and, with --liveview-id5, the live-view image 5
--   devcode-*   FUN_00d78a80 -> FUN_00d78418, develop record +0x104
--   marker-*    FUN_00d78538 -> FUN_00d7851c, develop record +0x110
--   filmmode-*  FUN_00eab258;  saturation-*  FUN_00eaac74
--   stub-*      cc_colour, cc_chroma, cc_tone on both patched builds
--   rawcommit-* FUN_00d46010 property 0x22b; rawrow-* FUN_00d588b8 property 0x22b
--   rawreq-*    plain FUN_012546a8 request word 2
--   offer-*     FUN_00bfa800 list open on the stock, RC0 and patched arrays
--   label-*     plain FUN_01179b0c; locator-* plain 0x0117dfd8
--
-- RAM mode (--ram-set, repeatable): the same families on the RC0 decodes with
-- a CC RAM-push manifest's publications applied, the runtime's CC code mapped
-- rx and its data block r at their link addresses, with the flash cases'
-- expectations. The stock request-builder cases run too as the residue source.
module Main where

import Control.Monad
import Data.Aeson (Value)
import Data.Bits
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Char (digitToInt, isHexDigit)
import Data.IORef
import Data.List (intercalate, sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe)
import Data.Word
import Emulation.ClassicChromeRam
import Emulation.NativeCase (runCase)
import FujiAF.Host
import FujiAF.JSON (decodeStrict, encodeCanonical)
import Numeric (showHex)
import System.Directory
import System.Environment
import System.Exit
import System.FilePath

-- ---------------------------------------------------------------------------
-- Inputs
-- ---------------------------------------------------------------------------

data Img = Img {iName :: String, iPath :: FilePath, iBase :: Word32, iBytes :: B.ByteString, iSha :: String}

loadImg :: String -> FilePath -> Word32 -> Maybe String -> IO Img
loadImg name path base pin = do
  bytes <- B.readFile path
  digest <- sha256 bytes
  forM_ pin $ \p -> ensure (digest == p) (path ++ ": SHA-256 " ++ digest ++ " differs from the pin " ++ p)
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

le16At :: B.ByteString -> Int -> Word32
le16At bs o = fromIntegral (B.index bs o) .|. (fromIntegral (B.index bs (o + 1)) `shiftL` 8)

le32 :: Word32 -> B.ByteString
le32 v = B.pack [fromIntegral (v `shiftR` k) | k <- [0, 8, 16, 24]]

le16 :: Word32 -> B.ByteString
le16 v = B.pack [fromIntegral v, fromIntegral (v `shiftR` 8)]

le64 :: Word32 -> B.ByteString
le64 v = le32 v <> le32 0

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

-- The words the patch contract places at a site; refuses an image that differs.
checkWords :: Img -> Word32 -> [Word32] -> IO ()
checkWords img a ws =
  forM_ (zip [0 ..] ws) $ \(k, w) -> do
    let got = wd img (a + 4 * k)
    ensure (got == w) (iName img ++ ": word at " ++ h32 (a + 4 * k) ++ " is " ++ h32 got ++ ", contract " ++ h32 w)

-- ---------------------------------------------------------------------------
-- Parameter files (schema latent-image.classic-chrome-parameters.v1)
-- ---------------------------------------------------------------------------

data Params = Params {pPath :: FilePath, pSha :: String, pMatrix :: [Word32], pChroma :: [Word32], pHead :: [Word32], pCurve :: [Word32]}

loadParams :: FilePath -> IO Params
loadParams path = do
  raw <- B.readFile path
  digest <- sha256 raw
  v <- either failH pure (decodeStrict raw)
  ensure (str (get v "schema") == "latent-image.classic-chrome-parameters.v1") (path ++ ": schema")
  let ints = map (fromIntegral . num) . list
      matrix = concatMap ints (list (get v "colour_matrix"))
      chroma = ints (get v "chroma")
      tone = get v "tone_record"
  ensure (length matrix == 9 && length chroma == 4) (path ++ ": matrix/chroma shape")
  ensure (length (ints (get tone "head")) == 9 && length (ints (get tone "curve")) == 28) (path ++ ": tone shape")
  pure (Params path digest matrix chroma (ints (get tone "head")) (ints (get tone "curve")))

-- The 100-byte data block of the contract, from a parameter file.
dataBlock :: Params -> B.ByteString
dataBlock p =
  BC.pack "CCD1"
    <> B.concat (map le16 (pMatrix p))
    <> B.concat (map le16 (chromaOutputOrder p))
    <> B.replicate 2 0
    <> B.pack (map fromIntegral (pHead p))
    <> B.singleton 0
    <> B.concat (map le16 (pCurve p))
    <> B.replicate 2 0

-- Storage order +0,+2,+4,+6 to FUN_00d60cc4's output order +0,+4,+6,+2.
chromaOutputOrder :: Params -> [Word32]
chromaOutputOrder p = let c = pChroma p in [c !! 0, c !! 2, c !! 3, c !! 1]

-- ---------------------------------------------------------------------------
-- Case model
-- ---------------------------------------------------------------------------

data Reg = Reg {rgId :: String, rgAddr :: Word32, rgSize :: Int, rgPerm :: String, rgInit :: B.ByteString, rgSrc :: String}

data Stub = Stub Word32 [(String, Word32)] [(String, Word32)]

data Case = Case
  { cId :: String
  , cFamily :: String
  , cProv :: String
  , cEntry :: Word32
  , cArgs :: [(String, Word32)]
  , cRegs :: [Reg]
  , cFns :: [(Word32, Int)]
  , cStubs :: [Stub]
  , cExpR :: [(String, Word32)]
  , cClaims :: [(String, Int, B.ByteString)]
  , cEvents :: [Stub]
  , cPin :: [String]
  , cPinFrom :: Maybe String
  , cExpect :: String
  , cExcl :: [String]
  , cBudget :: Integer
  , cInputs :: [(String, String)]
  }

exitA, stackBase, stackTop :: Word32
exitA = 0x00001080
stackBase = 0x00003000
stackTop = 0x00003400

baseRegs :: [Reg]
baseRegs =
  [ Reg "stack" stackBase 0x400 "rw" B.empty "zero-filled harness stack, SP 0x00003400"
  , Reg "exit" exitA 4 "rx" B.empty "exit sentinel"
  ]

codeReg :: String -> Img -> Word32 -> Int -> Reg
codeReg rid img a len = Reg rid a len "rx" (sl img a len) (iName img ++ " [" ++ h32 a ++ ", " ++ h32 (a + fromIntegral len) ++ ")")

dataReg :: String -> String -> Img -> Word32 -> Int -> Reg
dataReg rid perm img a len = Reg rid a len perm (sl img a len) (iName img ++ " [" ++ h32 a ++ ", " ++ h32 (a + fromIntegral len) ++ ")")

synthReg :: String -> String -> Word32 -> B.ByteString -> String -> Reg
synthReg rid perm a bytes note = Reg rid a (B.length bytes) perm bytes note

allRegs :: Case -> [Reg]
allRegs c = cRegs c ++ baseRegs

regNames :: [String]
regNames = map (("r" ++) . show) [0 :: Int .. 12]

initialRegs :: Case -> [(String, Word32)]
initialRegs c = [(r, fromMaybe 0 (lookup r (cArgs c))) | r <- regNames] ++ [("sp", stackTop), ("lr", exitA), ("cpsr", 0x10)]

-- Callee-saved r4-r11 and SP restored, return to the exit address.
expectedRegs :: Case -> [(String, Word32)]
expectedRegs c =
  M.toList (M.fromList ([(r, v) | (r, v) <- initialRegs c, r `elem` ["r4", "r5", "r6", "r7", "r8", "r9", "r10", "r11", "sp"]] ++ [("pc", exitA)] ++ cExpR c))

regsJ :: [(String, Word32)] -> Value
regsJ rs = obj [(k, s (h32 v)) | (k, v) <- rs]

stubJ :: Stub -> Value
stubJ (Stub a i o) = obj [("address", s (h32 a)), ("inputs", regsJ i), ("outputs", regsJ o), ("side_effects", s "registers-only")]

eventJ :: Stub -> Value
eventJ (Stub a i o) = obj [("address", s (h32 a)), ("inputs", regsJ i), ("outputs", regsJ o)]

data Env = Env {eOut :: FilePath, eRunner :: FilePath, eArts :: IORef (M.Map B.ByteString (String, String)), eFinals :: IORef (M.Map String (M.Map String B.ByteString))}

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
    ++ " Expected before execution: "
    ++ cExpect c
    ++ " Inputs: "
    ++ intercalate "; " [k ++ " " ++ v | (k, v) <- cInputs c]
    ++ ". Regions: "
    ++ intercalate "; " [rgId r ++ " " ++ h32 (rgAddr r) ++ " " ++ rgPerm r ++ " = " ++ rgSrc r | r <- allRegs c]
    ++ ". Stack bytes are pinned from a discovery run of this case"
    ++ (if null (cPin c) then "" else "; the unclaimed bytes of " ++ intercalate ", " (cPin c) ++ " are pinned from the discovery run of " ++ fromMaybe "this case" (cPinFrom c))
    ++ " and are residue, not claims."

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
    , ("limits", obj [("instructions", n (cBudget c)), ("wall_ms", n 5000)])
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
        , ("permissions", s (rgPerm r))
        , ("initial", maybe (obj [("zero", b True)]) (\(aid, _) -> obj [("artifact", s aid), ("offset", n 0), ("length", n (toInteger (B.length (rgInit r))))]) art)
        ]

commonExclusions :: [String]
commonExclusions =
  [ "No camera, USB device or full-system emulator is used: the case reads host files and runs the bounded Unicorn Cortex-R5F engine only."
  , "The R5F engine stands for the inferred R4F-class core only inside the admitted instruction set: no coprocessor, MPU, TCM, cache, interrupt, exception or timing behaviour is modelled and the A32 SDIV/UDIV refusal is unchanged."
  , "One function invocation with declared inputs is executed; callers, task context and the rest of the pipeline are outside the case."
  , "Patched-image bytes are the build-classic-chrome decodes named in the inputs, not bytes read from a body."
  ]

-- Final bytes of every region from a native-case report.
finalsOf :: Case -> Value -> IO (M.Map String B.ByteString)
finalsOf c report = do
  let mem = list (get (get report "runner") "memory")
  ensure (length mem == length (allRegs c)) (cId c ++ ": report has no final memory (" ++ str (get report "status") ++ " " ++ str (get report "reason") ++ ")")
  pure (M.fromList [(rgId r, unhex (str (get m "bytes"))) | (r, m) <- zip (allRegs c) mem])

initialBytes :: Reg -> B.ByteString
initialBytes r = rgInit r <> B.replicate (rgSize r - B.length (rgInit r)) 0

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

data Outcome = Outcome {oCase :: Case, oStatus :: String, oRunner :: String, oR0 :: String, oInstr :: Integer, oMismatch :: [String]}

runOne :: Env -> Case -> IO Outcome
runOne env c = do
  regArts <- forM (allRegs c) $ \r -> if B.null (rgInit r) then pure (r, Nothing) else (,) r . Just <$> artifactFor env (rgInit r)
  let path = eOut env </> cId c ++ ".json"
  B.writeFile path (encodeCanonical (caseValue c regArts (cClaims c)))
  (_, r1) <- runCase (eRunner env) path
  own <- finalsOf c r1
  source <- case cPinFrom c of
    Nothing -> pure own
    Just other -> do
      store <- readIORef (eFinals env)
      maybe (failH (cId c ++ ": residue source " ++ other ++ " has not run")) pure (M.lookup other store)
  let pins = concat [residue c r (if rgId r == "stack" then own M.! "stack" else source M.! rgId r) | r <- allRegs c, rgId r `elem` ("stack" : cPin c)]
  B.writeFile path (encodeCanonical (caseValue c regArts (cClaims c ++ pins)))
  (_, r2) <- runCase (eRunner env) path
  B.writeFile (eOut env </> cId c ++ "-report.json") (encodeCanonical r2)
  finals <- finalsOf c r2
  modifyIORef' (eFinals env) (M.insert (cId c) finals)
  let runner = get r2 "runner"
  pure
    Outcome
      { oCase = c
      , oStatus = str (get r2 "status")
      , oRunner = str (get runner "status")
      , oR0 = str (get (get runner "registers") "r0")
      , oInstr = num (get runner "instructions")
      , oMismatch = map str (list (get r2 "mismatches"))
      }

-- ---------------------------------------------------------------------------
-- Pins and paths
-- ---------------------------------------------------------------------------

stockId4Sha, stockId5Sha, stockPlainSha, stockId2Sha, rc0Id4Sha, payloadSha :: String
stockId4Sha = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
stockId5Sha = "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"
stockPlainSha = "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d"
stockId2Sha = "236d12a11c8b386f71192675c12398358840327f21e392e6eaa91ddfd6e020a5"
rc0Id4Sha = "a4d169bd961ec7df34e3b9b7108b624b79680c3cc50297e434e0b3607fb65551"
payloadSha = "8c6f806ef4554128e5be887a76e438a91b7d6cb030c940fc1c8fd6d32d5926c0"

data RamOpt = RamOpt {roTag :: String, roParams :: FilePath, roManifest :: FilePath, roRuntime :: FilePath, roSyms :: FilePath}

data Opts = Opts
  { oOut :: Maybe FilePath
  , oRunnerPath :: FilePath
  , oSynthBuild :: FilePath
  , oCandBuild :: FilePath
  , oSynthParams :: FilePath
  , oCandParams :: FilePath
  , oStubs :: FilePath
  , oPrivate :: FilePath
  , oRams :: [RamOpt]
  , oLiveview :: Maybe FilePath
  }

defaults :: Opts
defaults =
  Opts
    { oOut = Nothing
    , oRunnerPath = "bazel-bin/emulation/native-runner"
    , oSynthBuild = "private-data/analysis/classic-chrome-patch/synthetic-build-1"
    , oCandBuild = "private-data/analysis/classic-chrome-patch/candidate-build-1"
    , oSynthParams = "host/firmware/classic-chrome-synthetic.json"
    , oCandParams = "private-data/analysis/classic-chrome/candidate.json"
    , oStubs = "bazel-bin/host/firmware"
    , oPrivate = "private-data/analysis/x-m1-1.50"
    , oRams = []
    , oLiveview = Nothing
    }

outDir :: Opts -> FilePath
outDir o = fromMaybe (if null (oRams o) then "private-data/analysis/x-m1-1.50/native-case/classic-chrome/sites" else "private-data/analysis/x-m1-1.50/native-case/classic-chrome/sites-ram") (oOut o)

parseOpts :: Opts -> [String] -> IO Opts
parseOpts o [] = pure o
parseOpts o ("--ram-set" : tag : params : manifest : runtime : syms : rest) = parseOpts o {oRams = oRams o ++ [RamOpt tag params manifest runtime syms]} rest
parseOpts o (flag : v : rest) = case flag of
  "--out" -> parseOpts o {oOut = Just v} rest
  "--runner" -> parseOpts o {oRunnerPath = v} rest
  "--synthetic-build" -> parseOpts o {oSynthBuild = v} rest
  "--candidate-build" -> parseOpts o {oCandBuild = v} rest
  "--synthetic-params" -> parseOpts o {oSynthParams = v} rest
  "--candidate-params" -> parseOpts o {oCandParams = v} rest
  "--stubs" -> parseOpts o {oStubs = v} rest
  "--private" -> parseOpts o {oPrivate = v} rest
  "--liveview-id5" -> parseOpts o {oLiveview = Just v} rest
  _ -> failH ("unknown option " ++ flag)
parseOpts _ [flag] = failH ("option " ++ flag ++ " needs a value")

data World = World
  { wId4, wId5, wPlain, wId2, wRc0 :: Img
  , wP4, wP5, wPPlain :: Img
  , wC4 :: Img
  , wAdj :: B.ByteString
  , wSynth, wCand :: Params
  , wCapture, wSettings :: B.ByteString
  , wRams :: [RamWorld]
  , wLv5 :: Maybe Img
  }

-- The RC0 decodes with one manifest's publications applied, and its runtime.
data RamWorld = RamWorld {rwTag :: String, rwPayload :: RamPayload, rwParams :: Params, rwId4, rwId5, rwPlain, rwId2 :: Img}

loadRam :: Img -> Img -> Img -> Img -> RamOpt -> IO RamWorld
loadRam rc0 id5 plain id2 ro = do
  let tag = roTag ro
  ensure (take 4 tag == "ram-" && all (\x -> x `elem` ['a' .. 'z'] || x `elem` ['0' .. '9'] || x == '-') tag && length tag <= 24) (tag ++ ": a RAM set tag is ram- followed by lowercase letters, digits and hyphens")
  params <- loadParams (roParams ro)
  rp <- loadRamPayload (roManifest ro) (roRuntime ro) (roSyms ro)
  ensure (runtimeSlice rp (dataRange rp) == dataBlock params) (roRuntime ro ++ ": cc_ram_data differs from the contract encoding of " ++ roParams ro)
  let asRam img = RamImage (iName img) (iBase img) (iBytes img)
  applied <- applyPublications rp [RamImage (iName rc0) 0x0061c200 (B.replicate 0x140 0 <> iBytes rc0), asRam id5, asRam plain, asRam id2]
  imgs <- forM (zip3 applied [rc0, id5, plain, id2] ["id4", "id5", "plain", "id2"]) $ \(ri, src, kind) -> do
    digest <- sha256 (riBytes ri)
    pure (Img (tag ++ "-" ++ kind) (iPath src ++ " with the publications of " ++ roManifest ro) (riBase ri) (riBytes ri) digest)
  case imgs of
    [r4, r5, rp', r2] -> pure (RamWorld tag rp params r4 r5 rp' r2)
    _ -> failH (tag ++ ": RAM images")

loadWorld :: Opts -> IO World
loadWorld o = do
  let p = oPrivate o
  id4 <- loadImg "id4-stock" (p </> "lz/id4-0061c400.bin") 0x0061c400 (Just stockId4Sha)
  id5 <- loadImg "id5-stock" (p </> "lz/id5-00bf1a00.bin") 0x00bf1a00 (Just stockId5Sha)
  plain <- loadImg "plain-stock" (p </> "lz/plain-01100000.bin") 0x01100000 (Just stockPlainSha)
  id2 <- loadImg "id2-stock" (p </> "filmsim/id2-00130000.bin") 0x00130000 (Just stockId2Sha)
  rc0 <- loadImg "id4-rc0" (p </> "update/prefix/id4-patched.bin") 0x0061c340 (Just rc0Id4Sha)
  p4 <- loadImg "id4-patched-synthetic" (oSynthBuild o </> "patched-id4.bin") 0x0061c200 Nothing
  p5 <- loadImg "id5-patched" (oSynthBuild o </> "patched-id5.bin") 0x00bf1a00 Nothing
  pp <- loadImg "plain-patched" (oSynthBuild o </> "patched-plain.bin") 0x01100000 Nothing
  c4 <- loadImg "id4-patched-candidate" (oCandBuild o </> "patched-id4.bin") 0x0061c200 Nothing
  c5 <- loadImg "id5-patched-candidate" (oCandBuild o </> "patched-id5.bin") 0x00bf1a00 Nothing
  cp <- loadImg "plain-patched-candidate" (oCandBuild o </> "patched-plain.bin") 0x01100000 Nothing
  ensure (iSha c5 == iSha p5 && iSha cp == iSha pp) "the candidate build's id5/plain differ from the synthetic build's"
  payload <- B.readFile (p </> "payload.bin")
  pd <- sha256 payload
  ensure (pd == payloadSha) ("payload.bin: SHA-256 " ++ pd)
  let adj = B.take 0xd000 (B.drop 0xe0048 payload)
  ensure (B.take 4 (B.drop 0x15c adj) == BC.pack "X-M1" && le16At adj 0x202 == 0xaa55) "fallback adjustment image markers"
  synth <- loadParams (oSynthParams o)
  cand <- loadParams (oCandParams o)
  capture <- B.readFile (oStubs o </> "classic-chrome-capture.bin")
  settings <- B.readFile (oStubs o </> "classic-chrome-settings.bin")
  ensure (B.length capture == 96 && B.length settings == 84) "stub blob sizes"
  rams <- mapM (loadRam rc0 id5 plain id2) (oRams o)
  lv5 <- forM (oLiveview o) $ \path -> loadImg "id5-liveview" path 0x00bf1a00 Nothing
  pure (World id4 id5 plain id2 rc0 p4 p5 pp c4 adj synth cand capture settings rams lv5)

-- Contract words at every site the cases execute, on the stock and patched images.
checkContract :: World -> IO ()
checkContract w = do
  let pair img4 a old new = checkWords (wId4 w) a old >> checkWords img4 a new
      p5 a old new = checkWords (wId5 w) a old >> checkWords (wP5 w) a new
  forM_ [wP4 w, wC4 w] $ \img4 -> do
    ensure (sl img4 0x00738fe8 96 == wCapture w) (iName img4 ++ ": capture blob differs from classic-chrome-capture.bin")
    ensure (sl img4 0x00736a74 84 == wSettings w) (iName img4 ++ ": settings blob differs from classic-chrome-settings.bin")
    checkWords img4 0x00738fe8 [0xe350000d, 0x83a00000, 0xe12fff1e]
    checkWords img4 0x00736a74 [0xe350000d, 0x83a00000, 0xe12fff1e]
    pair img4 0x0077ac48 [2] [0xd]
    pair img4 0x0077ac50 [2] [0xd]
    pair img4 0x0079608c [2] [0xd]
    forM_ [0x00796094, 0x00796098, 0x007960a0, 0x007960a4, 0x007960a8, 0x007960ac, 0x007960e4] $ \a -> pair img4 a [0] [1]
    forM_ [0x007823c4, 0x00785d94, 0x0077d1a4] $ \a -> pair img4 a [0x0078319c] [0x0061c340]
    forM_ [0x007823c8, 0x00785d98, 0x0077d1a8] $ \a -> pair img4 a [0x0078d4f8] [0x0061c2b0]
    checkWords img4 0x0061c340 [0, 0, 3, 0, 1, 0, 7, 0, 0xc, 0, 5, 0, 6, 0, 2, 0, 0x7ffffffe, 0]
  ensure (sl (wP4 w) 0x0061c200 100 == dataBlock (wSynth w)) "synthetic build: data block differs from the contract encoding of the synthetic set"
  ensure (sl (wC4 w) 0x0061c200 100 == dataBlock (wCand w)) "candidate build: data block differs from the contract encoding of candidate.json"
  checkWords (wId4 w) 0x00738fe8 [0xe52db004, 0xe28db000, 0xe3500000]
  checkWords (wId4 w) 0x00736a74 [0xe52db004, 0xe28db000, 0xe3500000]
  checkWords (wRc0 w) 0x007823c4 [0x0061c340]
  checkWords (wRc0 w) 0x0061c340 [0, 0, 3, 0, 1, 0, 7, 0, 0xc, 0, 5, 0, 6, 0, 0x7ffffffe, 0]
  p5 0x00d7851c [0xe52db004, 0xe28db000, 0xe3a03000] [0xe351000d, 0x03a0300d, 0x13a03000]
  p5 0x00d7852c [0xe28bd000] [0xe12fff1e]
  p5 0x00eab2e0 [0xea00000b] [0xea00000f]
  p5 0x00eab2f0 [0x00eab324, 0x00eab32c] [0x00eab334, 0x00eab334]
  p5 0x00eab324 [0xe30f0fff, 0xea000008, 0xe30f0fff] [0xe353000c, 0x03a00c06, 0x13a00000]
  p5 0x00eaac88 [0xea00004e] [0xea00004b]
  p5 0x00eaacb0 [0x00eaadb8, 0x00eaadc0] [0x00eaadb0, 0x00eaadb0]
  p5 0x00eaadb0 [0xe3000301, 0xea00000a, 0xe3000302, 0xea000008, 0xe3000303, 0xea000006] [0xe2430007, 0xe2800c03, 0xea000009, 0xe353000d, 0x0affffc0, 0xe3a00000]
  p5 0x00d461ec [0xe5832008, 0xe2466005, 0xe3560004, 0x8a000004] [0xe356000a, 0x03a02008, 0xe5832008, 0xea000004]
  p5 0x00d5ada4
    [0xe3530000, 0x050b3018, 0x0a000164, 0xe3530005, 0x03a03001, 0x050b3018, 0x0a000160, 0xe3530003, 0x03a03002, 0x050b3018, 0x0a00015c, 0xe3530006, 0x03a03004, 0x050b3018, 0x0a000158, 0xe3530007, 0x03a03003]
    [0xe3530005, 0x03a03001, 0x0a00000b, 0xe3530003, 0x03a03002, 0x0a000008, 0xe3530006, 0x03a03004, 0x0a000005, 0xe3530007, 0x03a03003, 0x0a000002, 0xe3530008, 0x03a0300a, 0x13a03000, 0xe50b3018, 0xea000156]
  checkWords (wPlain w) 0x012548a0 [0xe5943008, 0xe2433003, 0xe3530004, 0x979ff103, 0xea000004, 0x012548f0, 0x012548c8, 0x012548dc, 0x01254904, 0x01254918, 0xe3033a7c, 0xe3403151, 0xe3a02000, 0xe58327a4, 0xea000012, 0xe3033a7c]
  checkWords (wPPlain w) 0x012548a0 [0xe5943008, 0xe3a02000, 0xe3530003, 0x03a02001, 0xe3530005, 0x03a02003, 0xe3530006, 0x03a0200b, 0xe3530007, 0x03a0200c, 0xe3530008, 0x03a0200d, 0xe3033a7c, 0xe3403151, 0xe58327a4, 0xea000011]
  checkWords (wPlain w) 0x01179b14 [0xe300383d]
  checkWords (wPPlain w) 0x01179b14 [0xe300383e]
  -- The live-view fragment: five words of FUN_00eee958 over the patched image 5, nothing else.
  forM_ [0 .. 12 :: Word32] $ \k -> let arm = wd (wId5 w) (wd (wId5 w) (0x00eee96c + 4 * k)) in ensure (arm .&. 0xffff0f00 == 0xe3a00000) ("id5: FUN_00eee958 arm " ++ show k ++ " is not mov rX,#imm: " ++ h32 arm)
  forM_ (wLv5 w) $ \l5 -> do
    let lo = 0x00eee960 - 0x00bf1a00
        hi = lo + 20
        outside img = B.take lo (iBytes img) <> B.drop hi (iBytes img)
    checkWords (wP5 w) 0x00eee960 [0xe350000c, 0x979ff100, 0xea00000c, 0x00eee9b0, 0x00eee9bc]
    checkWords l5 0x00eee960 [0xe350000d, 0x95810000, 0x93a00000, 0x9a000041, 0xea00000a]
    ensure (B.length (iBytes l5) == B.length (iBytes (wP5 w)) && outside l5 == outside (wP5 w)) (iPath l5 ++ ": differs from the patched image 5 outside [0x00eee960, 0x00eee974)")
  ensure (sl (wId2 w) 0x0037b808 66 == B.replicate 66 0) "id2: the stock 0x83e record is not 66 zero bytes"

-- Words the RAM cases' expectations rest on, read from the RAM images.
checkRam :: World -> RamWorld -> IO ()
checkRam w rw = do
  let r4 = rwId4 rw
  checkWords r4 0x0061c340 [0, 0, 3, 0, 1, 0, 7, 0, 0xc, 0, 5, 0, 6, 0, 2, 0, 0x7ffffffe, 0]
  forM_ [0x007823c4, 0x00785d94, 0x0077d1a4] $ \a -> checkWords r4 a [0x0061c340]
  forM_ [0x007823c8, 0x00785d98, 0x0077d1a8] $ \a -> checkWords r4 a [0x0061c2b0]
  forM_ [0x0077ac48, 0x0077ac50, 0x0079608c, 0x00796094, 0x00796098, 0x007960a0, 0x007960a4, 0x007960a8, 0x007960ac, 0x007960e4] $ \a -> checkWords r4 a [wd (wP4 w) a]
  -- RAW values through RAW labels; RC0's words from 0x0061c3f8 stay unpublished.
  let window = [0x0061c264, 0x0061c268 .. 0x0061c3f4]
  ensure (all (\a -> wd r4 a == wd (wP4 w) a) window) (rwTag rw ++ ": the RAM prefix arrays [0x0061c264, 0x0061c3f8) differ from the flash build's at " ++ unwords [h32 a ++ " " ++ h32 (wd r4 a) ++ "/" ++ h32 (wd (wP4 w) a) | a <- window, wd r4 a /= wd (wP4 w) a])
  checkWords (rwPlain rw) 0x01179b14 [0xe300383e]
  ensure (sl (rwId2 rw) 0x0037b808 66 == ccRecord) (rwTag rw ++ ": the RAM image-2 0x83e record is not the contract record")

-- The images a family case runs on; a RAM side also maps its runtime.
data Side = Side {sdTag :: String, sdId4, sdId5, sdPlain :: Img, sdPatched :: Bool, sdRam :: Maybe RamWorld}

flashSides :: World -> [Side]
flashSides w = [Side "stock" (wId4 w) (wId5 w) (wPlain w) False Nothing, Side "patched" (wP4 w) (wP5 w) (wPPlain w) True Nothing]

ramSides :: World -> [Side]
ramSides w = [Side (rwTag rw) (rwId4 rw) (rwId5 rw) (rwPlain rw) True (Just rw) | rw <- wRams w]

ramRegs :: RamWorld -> [Reg]
ramRegs rw =
  [ Reg "runtime-text" ta tl "rx" (runtimeSlice rp (ta, tl)) ("runtime " ++ rpRuntimePath rp ++ " CC code [" ++ h32 ta ++ ", " ++ h32 (ta + fromIntegral tl) ++ ")")
  , Reg "runtime-data" (rpData rp) 100 "r" (runtimeSlice rp (dataRange rp)) ("runtime " ++ rpRuntimePath rp ++ " cc_ram_data [" ++ h32 (rpData rp) ++ ", " ++ h32 (rpData rp + 100) ++ ")")
  ]
  where
    rp = rwPayload rw
    (ta, tl) = textRange rp

-- On a RAM side: the runtime regions and the named entry functions when any
-- entry runs, and the payload's provenance, exclusion and inputs.
onRam :: Side -> [CcEntry] -> Case -> Case
onRam side entries c = case sdRam side of
  Nothing -> c
  Just rw ->
    let rp = rwPayload rw
     in c
          { cProv = cProv c ++ " RAM images: the RC0 decodes (image 4 at 0x0061c340 with zeros from 0x0061c200, stock images 5, plain and 2) with the " ++ show (length (rpPublications rp)) ++ " publications of " ++ rpManifestPath rp ++ " applied in manifest order; runtime linked at " ++ h32 (rpBase rp) ++ (if null entries then "" else ", entered at " ++ intercalate ", " [ccSymbol e ++ " " ++ h32 (entryAddress rp e) | e <- entries]) ++ "."
          , cRegs = cRegs c ++ (if null entries then [] else ramRegs rw)
          , cFns = cFns c ++ map (entryFunction rp) entries
          , cExcl = cExcl c ++ ["RAM-image bytes are the RC0 decodes with the manifest's publications applied by this generator, not bytes read from a body; of the runtime only its CC code and data block are mapped."]
          , cInputs = cInputs c ++ [("manifest", rpManifestPath rp ++ " (SHA-256 " ++ rpManifestSha rp ++ ")"), ("runtime", rpRuntimePath rp ++ " (SHA-256 " ++ rpRuntimeSha rp ++ ")")]
          }

imgInput :: Img -> (String, String)
imgInput img = (iName img, iPath img ++ " (SHA-256 " ++ iSha img ++ ", base " ++ h32 (iBase img) ++ ")")

-- ---------------------------------------------------------------------------
-- 1. Converters
-- ---------------------------------------------------------------------------

convInputs :: [Word32]
convInputs = [0 .. 15] ++ [0xffffffff]

-- Stock FUN_00738fe8/FUN_00736a74 over table 0x00817180 of {value, key} pairs:
-- input 0 returns entry 0's value; otherwise the first entry from 1 whose key
-- matches, scanning while the key is non-negative; otherwise 0.
convStock :: Img -> Word32 -> Word32
convStock img x
  | x == 0 = wd img 0x00817180
  | otherwise = go 1
  where
    go i =
      let key = wd img (0x00817180 + 8 * i + 4)
       in if key .&. 0x80000000 /= 0 then 0 else if key == x then wd img (0x00817180 + 8 * i) else go (i + 1)

convCases :: World -> [Side] -> [Case]
convCases w sides =
  [ onRam side [CcConvert] Case
      { cId = "conv-" ++ fname ++ "-" ++ tag ++ "-" ++ inTag x
      , cFamily = "1 converters"
      , cProv = fname' ++ " on the " ++ tag ++ " image 4, entered with r0 = the film value (callers 0x00eb1a34 and 0x00739930 pass it in r0 and read r0)."
      , cEntry = fa
      , cArgs = [("r0", x)]
      , cRegs = [codeReg "code" img fa fsize, dataReg "table" "r" img 0x00817180 120]
      , cFns = [(fa, fsize)]
      , cStubs = []
      , cExpR = [("r0", out)]
      , cClaims = []
      , cEvents = []
      , cPin = []
      , cPinFrom = Nothing
      , cExpect = "r0 = " ++ hx out ++ " (" ++ rule ++ ")."
      , cExcl = ["Only the converter's return value is exercised; the settings object and capture record it feeds are outside the case."]
      , cBudget = 10000
      , cInputs = [imgInput img]
      }
  | (fname, fname', fa, fsize) <- [("capture", "FUN_00738fe8 (canonical +0x9c to capture +0x508)", 0x00738fe8, 100), ("settings", "FUN_00736a74 (live col2 to settings object +0x3c)", 0x00736a74, 104)]
  , side <- sides
  , let (tag, img, patched) = (sdTag side, sdId4 side, sdPatched side)
  , x <- convInputs
  , let out = if patched then (if x <= 13 then x else 0) else convStock (wId4 w) x
  , let rule = if patched then "contract body cmp r0,#13; movhi r0,#0: 0..13 pass, else 0" else "table 0x00817180 read from the stock image: identity 0..12, else 0"
  ]

inTag :: Word32 -> String
inTag x = if x == 0xffffffff then "ffffffff" else let t = showHex x "" in replicate (2 - length t) '0' ++ t

-- ---------------------------------------------------------------------------
-- 1b. Movie/live-view film validator FUN_00eee958
-- ---------------------------------------------------------------------------

validatorInputs :: [Word32]
validatorInputs = [0 .. 19] ++ [0x7fffffff, 0x80000000, 0xffffffff]

-- Stock: for film <= 12 the jump table 0x00eee96c sends it to an arm
-- `mov rX, #k; str rX, [r1]; mov r0, #0`, read here; else the default arm
-- stores 0 and returns 0x84.
validatorStock :: Img -> Word32 -> (Word32, Word32)
validatorStock img x
  | x <= 12 = (wd img (wd img (0x00eee96c + 4 * x)) .&. 0xff, 0)
  | otherwise = (0, 0x84)

validatorLiveview :: Word32 -> (Word32, Word32)
validatorLiveview x = if x <= 13 then (x, 0) else (0, 0x84)

outBase :: Word32
outBase = 0x20000000

validatorCases :: World -> [Case]
validatorCases w =
  [ Case
      { cId = "validator-" ++ tag ++ "-" ++ inTag x
      , cFamily = "1b movie/live-view film validator"
      , cProv = "FUN_00eee958 on the " ++ tag ++ " image 5, entered as FUN_00edccd8, FUN_00edd120, FUN_00eef1e4 and FUN_00eef4b0 call it: r0 = film value (config +0x5c or struct +0x14), r1 = the caller's local, which the caller passes to FUN_00d78a80 and FUN_00d78538; no caller reads r0. The local is filled with 0xa5."
      , cEntry = 0x00eee958
      , cArgs = [("r0", x), ("r1", outBase)]
      , cRegs = [codeReg "code" img 0x00eee958 0x12c, synthReg "out" "rw" outBase (B.replicate 4 0xa5) "caller local, 4 bytes of 0xa5"]
      , cFns = [(0x00eee958, 0x12c)]
      , cStubs = []
      , cExpR = [("r0", ret)]
      , cClaims = [("out", 0, le32 out)]
      , cEvents = []
      , cPin = []
      , cPinFrom = Nothing
      , cExpect = "local = " ++ hx out ++ ", r0 = " ++ hx ret ++ " (" ++ rule ++ ")."
      , cExcl = ["Only the validator is exercised; FUN_00d78a80 and FUN_00d78538 on its output are the devcode- and marker- families, and the record build around them the builder cases' lv path."]
      , cBudget = 10000
      , cInputs = [imgInput img]
      }
  | (tag, img, lv) <- ("stock", wId5 w, False) : [("liveview", l5, True) | Just l5 <- [wLv5 w]]
  , x <- validatorInputs
  , let (out, ret) = if lv then validatorLiveview x else validatorStock (wId5 w) x
  , let rule = if lv then "fragment cmp r0,#13; strls r0,[r1]; movls r0,#0; bls epilogue; b default arm: 0..13 pass, else 0 with 0x84" else "jump table 0x00eee96c read from the stock image: identity 0..12, else 0 with 0x84"
  ]

-- ---------------------------------------------------------------------------
-- 2. Develop dispatch and marker
-- ---------------------------------------------------------------------------

recordBase :: Word32
recordBase = 0x20000000

sentinelRecord :: B.ByteString
sentinelRecord = B.replicate 0x200 0xa5

devCode :: Word32 -> Word32
devCode col2 = case col2 of
  6 -> 1
  4 -> 2
  7 -> 2
  8 -> 2
  9 -> 2
  10 -> 2
  5 -> 3
  3 -> 4
  1 -> 5
  11 -> 6
  12 -> 7
  _ -> 0

devCases :: [Side] -> [Case]
devCases sides =
  concat
    [ [ onRam side [] Case
          { cId = "devcode-" ++ tag ++ "-col2-" ++ inTag col2
          , cFamily = "2 develop dispatch"
          , cProv = "FUN_00d78a80 on the " ++ tag ++ " image 5, which adds 0x104 to the develop record and calls FUN_00d78418(record + 0x104, col2). r0 = develop record, r1 = film value col2 (request[9]); the record is filled with 0xa5."
          , cEntry = 0x00d78a80
          , cArgs = [("r0", recordBase), ("r1", col2)]
          , cRegs = codeRegs img
          , cFns = [(0x00d78a80, 20), (0x00d78418, 148)]
          , cStubs = []
          , cExpR = []
          , cClaims = [("record", 0x104, le32 (devCode col2))]
          , cEvents = []
          , cPin = []
          , cPinFrom = Nothing
          , cExpect = "record +0x104 = " ++ hx (devCode col2) ++ " (stock mapping 0<-{0,2,13,other}, 1<-6, 2<-{4,7,8,9,0xa}, 3<-5, 4<-3, 5<-1, 6<-0xb, 7<-0xc on both images), every other record byte 0xa5."
          , cExcl = ["Only the dispatch store is exercised; readers of +0x104 are outside the case."]
          , cBudget = 10000
          , cInputs = [imgInput img]
          }
      , onRam side [CcMarker] Case
          { cId = "marker-" ++ tag ++ "-col2-" ++ inTag col2
          , cFamily = "2 develop marker"
          , cProv = "FUN_00d78538 on the " ++ tag ++ " image 5, which adds 0x110 to the develop record and calls the setter FUN_00d7851c with the film value in r1. r0 = develop record, r1 = col2; the record is filled with 0xa5."
          , cEntry = 0x00d78538
          , cArgs = [("r0", recordBase), ("r1", col2)]
          , cRegs = codeRegs img
          , cFns = [(0x00d78538, 20), (0x00d7851c, 28)]
          , cStubs = []
          , cExpR = []
          , cClaims = [("record", 0x110, le32 mark)]
          , cEvents = []
          , cPin = []
          , cPinFrom = Nothing
          , cExpect = "record +0x110 = " ++ hx mark ++ (if patched then " (contract setter: 0xd when r1 = 0xd, else 0)" else " (stock setter stores constant 0)") ++ ", every other record byte 0xa5."
          , cExcl = ["The +0x110 reader 0x00d671d4 and the stage hooks are outside the case."]
          , cBudget = 10000
          , cInputs = [imgInput img]
          }
      ]
    | side <- sides
    , let (tag, img, patched) = (sdTag side, sdId5 side, sdPatched side)
    , col2 <- [0 .. 13]
    , let mark = if patched && col2 == 13 then 13 else 0
    ]
  where
    codeRegs img =
      [ codeReg "code-dispatch" img 0x00d78418 (0x00d7854c - 0x00d78418)
      , codeReg "code-wrapper" img 0x00d78a80 20
      , synthReg "record" "rw" recordBase sentinelRecord "develop record, 0x200 bytes of 0xa5"
      ]

-- ---------------------------------------------------------------------------
-- 3. FilmMode and Saturation
-- ---------------------------------------------------------------------------

filmModeGated :: Word32 -> Bool
filmModeGated m = m `elem` ([0, 2, 3, 4, 0xe, 0xf, 0x19, 0x1a, 0x1d, 0x1e, 0x1f, 0x2c] ++ [0x21 .. 0x24])

filmModeStock :: Word32 -> Word32 -> Word32
filmModeStock mode col2
  | not (filmModeGated mode) = 0xffff
  | otherwise = case col2 of
      1 -> 0x120
      3 -> 0x200
      4 -> 0xffff
      5 -> 0xffff
      11 -> 0x500
      12 -> 0x501
      _ | col2 >= 7 && col2 <= 10 -> 0xffff
      _ -> 0

filmModePatched :: Word32 -> Word32 -> Word32
filmModePatched mode col2
  | filmModeGated mode && col2 == 13 = 0x600
  | otherwise = filmModeStock mode col2

satArmA, satArmB :: Word32 -> Bool
satArmA m = m `elem` ([2, 3, 4, 0x1d, 0x1f, 0x2c] ++ [0x21 .. 0x24])
satArmB m = m `elem` [0, 0xe, 0xf, 0x19, 0x1a, 0x1e, 0x20, 0x27, 0x29, 0x2a]

satModeArm :: Word32 -> Word32 -> Word32 -> Word32
satModeArm mode e504 col2
  | satArmA mode = case e504 of
      2 -> 0x100
      3 -> 0x80
      4 -> 0x400
      5 -> 0x180
      _ -> 0
  | satArmB mode = if col2 == 1 || col2 == 3 then 0x8000 else 0
  | otherwise = 0xffff

saturationStock :: Word32 -> Word32 -> Word32 -> Word32
saturationStock mode e504 col2
  | col2 `elem` [0, 1, 3, 11, 12] = satModeArm mode e504 col2
  | col2 == 4 || col2 == 7 = 0x300
  | col2 == 5 = 0x310
  | col2 `elem` [8, 9, 10] = 0x300 + col2 - 7
  | otherwise = 0

saturationPatched :: Word32 -> Word32 -> Word32 -> Word32
saturationPatched mode e504 col2
  | col2 == 13 = satModeArm mode e504 col2
  | otherwise = saturationStock mode e504 col2

captureRecord :: Word32 -> Word32 -> Word32 -> B.ByteString
captureRecord mode e504 col2 = B.replicate 0x4b4 0 <> le32 mode <> B.replicate (0x504 - 0x4b8) 0 <> le32 e504 <> le32 col2 <> B.replicate 4 0

metaCases :: [Side] -> [Case]
metaCases sides =
  [ mk "filmmode" "FUN_00eab258 (FilmMode, maker-note +0xf0)" 0x00eab258 0x104 side mode 0 col2 (if patched then filmModePatched mode col2 else filmModeStock mode col2) note
  | side <- sides
  , let patched = sdPatched side
  , (mode, cols) <- [(0, [0 .. 15]), (5, [0, 13])]
  , col2 <- cols
  , let note = case (patched, sdRam side) of
          (_, Just _) -> "stock table with the default arm branching to cc_ram_filmmode: col2 0xd in a gated mode gives 0x600"
          (True, _) -> "stock table with the contract arms F1-F6: col2 0xd in a gated mode gives 0x600"
          _ -> "stock switch read from image 5"
  ]
    ++ [ mk "saturation" "FUN_00eaac74 (Saturation, maker-note +0x40)" 0x00eaac74 0x17c side mode e504 col2 (if patched then saturationPatched mode e504 col2 else saturationStock mode e504 col2) note
       | side <- sides
       , let patched = sdPatched side
       , (mode, e504, cols) <- [(0x24, 4, [0 .. 15]), (0, 4, [0 .. 15]), (5, 4, [0, 13])]
       , col2 <- cols
       , let note = case (patched, sdRam side) of
               (_, Just _) -> "stock switch with the default arm branching to cc_ram_saturation: col2 0xd takes the mode arm col2 0 takes"
               (True, _) -> "stock switch with the contract arms S1-S9: col2 0xd takes the mode arm col2 0 takes"
               _ -> "stock switch read from image 5"
       ]
  where
    mk fam fname fa fsize side mode e504 col2 out note =
      let (tag, img) = (sdTag side, sdId5 side)
       in onRam side [if fam == "filmmode" then CcFilmMode else CcSaturation] Case
        { cId = fam ++ "-" ++ tag ++ "-m" ++ inTag mode ++ "-col2-" ++ inTag col2
        , cFamily = "3 metadata"
        , cProv = fname ++ " on the " ++ tag ++ " image 5. r0 = capture record with +0x4b4 = mode " ++ hx mode ++ ", +0x504 = " ++ hx e504 ++ ", +0x508 = col2 " ++ hx col2 ++ ", other bytes 0."
        , cEntry = fa
        , cArgs = [("r0", recordBase)]
        , cRegs = [codeReg "code" img fa fsize, synthReg "capture" "r" recordBase (captureRecord mode e504 col2) "capture record"]
        , cFns = [(fa, fsize)]
        , cStubs = []
        , cExpR = [("r0", out)]
        , cClaims = []
        , cEvents = []
        , cPin = []
        , cPinFrom = Nothing
        , cExpect = "r0 = " ++ hx out ++ " (" ++ note ++ "; mode " ++ hx mode ++ (if fam == "filmmode" then (if filmModeGated mode then " is in the FilmMode mode set" else " is outside the FilmMode mode set") else (if satArmA mode then " takes the +0x504 arm" else if satArmB mode then " takes the col2 arm" else " is outside both arms")) ++ ")."
        , cExcl = ["The maker-note writer that stores r0 and the exiftool naming of the value are outside the case."]
        , cBudget = 10000
        , cInputs = [imgInput img]
        }

-- ---------------------------------------------------------------------------
-- 4. Stage hook stubs
-- ---------------------------------------------------------------------------

adjBase, adjPtrCell, destBase :: Word32
adjBase = 0x30000000
adjPtrCell = 0x0096ca50
destBase = 0x21000000

data StubKind = StubColour | StubChroma | StubTone

-- A build whose stage stubs the cases enter: per stub its symbol, entry, code
-- and data-block regions and functions.
data StubHost = StubHost
  { shTag :: String
  , shId4, shId5 :: Img
  , shParams :: Params
  , shHead :: String
  , shSite :: StubKind -> (String, Word32, [Reg], [(Word32, Int)])
  , shWrap :: Case -> Case
  }

flashStubHosts :: World -> [StubHost]
flashStubHosts w = [host bt img4 params | (bt, img4, params) <- [("synthetic", wP4 w, wSynth w), ("candidate", wC4 w, wCand w)]]
  where
    host bt img4 params = StubHost bt img4 (wP5 w) params ("Stage hook stub in isolation on the " ++ bt ++ " build's patched image 4 (data block from " ++ pPath params ++ ", SHA-256 " ++ pSha params ++ ").") (site img4) id
    site img4 k =
      let capture = [codeReg "code-capture" img4 0x00738fe8 100, dataReg "datablock" "r" img4 0x0061c200 100]
       in case k of
            StubColour -> ("cc_colour", 0x00738ff4, capture, [(0x00738ff4, 0x0073904c - 0x00738ff4)])
            StubChroma -> ("cc_chroma", 0x00739010, capture, [(0x00739010, 0x0073904c - 0x00739010)])
            StubTone -> ("cc_tone", 0x00736a80, [codeReg "code-settings" img4 0x00736a74 104, dataReg "datablock" "r" img4 0x0061c200 100], [(0x00736a80, 0x00736adc - 0x00736a80)])

ramStubHost :: Side -> RamWorld -> StubHost
ramStubHost side rw = StubHost (rwTag rw) (rwId4 rw) (rwId5 rw) (rwParams rw) ("Stage hook stub in isolation in the " ++ rwTag rw ++ " runtime (data block cc_ram_data from " ++ pPath (rwParams rw) ++ ", SHA-256 " ++ pSha (rwParams rw) ++ ").") site (onRam side [])
  where
    rp = rwPayload rw
    entry e es = (ccSymbol e, entryAddress rp e, ramRegs rw, map (entryFunction rp) es)
    site k = case k of
      StubColour -> entry CcColour [CcColour, CcChroma]
      StubChroma -> entry CcChroma [CcChroma]
      StubTone -> entry CcTone [CcTone]

stubCases :: World -> [StubHost] -> [Case]
stubCases w hosts = concat [build h | h <- hosts]
  where
    adj = wAdj w
    adjField f = le16At adj (fromIntegral f)
    record mark = B.replicate 0x110 0 <> le32 mark <> B.replicate (0x200 - 0x114) 0
    dest = B.replicate 0x100 0xee
    readerRegs img4 =
      [ codeReg "code-adjget" img4 0x0073ed6c 40
      , codeReg "code-byteget" img4 0x0069f8fc 36
      , codeReg "code-memcpy" img4 0x00722054 292
      , synthReg "adjptr" "r" adjPtrCell (le32 adjBase) "adjustment-image pointer cell 0x0096ca50 = 0x30000000"
      , Reg "adj" adjBase 0xd000 "r" adj "fallback adjustment image, payload.bin [0xe0048, 0xed048)"
      ]
    readerFns = [(0x0073ed6c, 40), (0x0069f8fc, 36), (0x00722054, 292)]
    colourId = 0 :: Word32
    toneKey img4 = wd img4 (0x00824bc8 + 72)
    toneCount img4 = wd img4 0x00824bc4
    build h =
      map (shWrap h) $ concat
        [ [ base
              { cId = "stub-colour-" ++ bt ++ "-m" ++ inTag mark
              , cProv = provHead ++ " " ++ colourName ++ " (" ++ h32 colourEntry ++ ", hook at 0x00d67284) entered with r0 = 72-byte destination, r1 = colour id-1 = " ++ show colourId ++ ", r5 = develop record whose +0x110 = " ++ hx mark ++ "; the stock reader FUN_00d670a0 and its callees are mapped over the fallback adjustment image."
              , cEntry = colourEntry
              , cArgs = [("r0", destBase), ("r1", colourId), ("r5", recordBase)]
              , cRegs =
                  colourRegs
                    ++ [ synthReg "record" "r" recordBase (record mark) "develop record"
                       , synthReg "dest" "rw" destBase dest "destination, 0xee"
                       , codeReg "code-reader" id5 0x00d670a0 276
                       , dataReg "fieldtab" "r" id5 0x00d67f88 360
                       ]
                    ++ readerRegs img4
              , cFns = colourFns ++ [(0x00d670a0, 276)] ++ readerFns
              , cClaims = [("dest", 0, colourOut)]
              , cExpect =
                  if mark == 13
                    then "the nine matrix halfwords of the parameter file (" ++ pPath params ++ ") zero-extended to u64 at the destination, 72 bytes, the rest 0xee."
                    else "the stock reader's output: nine u64 of the adjustment fields named by table 0x00d67f88 for id-1 " ++ show colourId ++ " (fields " ++ intercalate "," (map hx colourFields) ++ "), the rest 0xee."
              }
          , base
              { cId = "stub-chroma-" ++ bt ++ "-m" ++ inTag mark
              , cProv = provHead ++ " " ++ chromaName ++ " (" ++ h32 chromaEntry ++ ", hooks at 0x00d60f60 and 0x00d64ef4) entered with r0 = 32-byte destination, r1 = chroma id-1 = " ++ show colourId ++ ", r4 = develop record whose +0x110 = " ++ hx mark ++ "; the stock reader FUN_00d60cc4 and its callees are mapped over the fallback adjustment image."
              , cEntry = chromaEntry
              , cArgs = [("r0", destBase), ("r1", colourId), ("r4", recordBase)]
              , cRegs =
                  chromaRegs
                    ++ [ synthReg "record" "r" recordBase (record mark) "develop record"
                       , synthReg "dest" "rw" destBase dest "destination, 0xee"
                       , codeReg "code-reader" id5 0x00d60cc4 152
                       , dataReg "fieldtab" "r" id5 0x00d61e14 0x58
                       ]
                    ++ readerRegs img4
              , cFns = chromaFns ++ [(0x00d60cc4, 152)] ++ readerFns
              , cClaims = [("dest", 0, chromaOut)]
              , cExpect =
                  if mark == 13
                    then "the parameter file's chroma fields in storage order +0,+4,+6,+2 (" ++ intercalate "," (map hx (chromaOutputOrder params)) ++ ") zero-extended to u64, 32 bytes, the rest 0xee."
                    else "the stock reader's output: four u64 of the adjustment fields at table 0x00d61e14 entry " ++ show colourId ++ " +0,+4,+6,+2 (fields " ++ intercalate "," (map hx chromaFields) ++ "), the rest 0xee."
              }
          , base
              { cId = "stub-tone-" ++ bt ++ "-m" ++ inTag mark
              , cProv = provHead ++ " " ++ toneName ++ " (" ++ h32 toneEntry ++ ", hooks at 0x00d677ac and 0x00d677bc) entered with r0 = curve id " ++ hx (toneKey img4) ++ " (the key of record 1 of table 0x00824bc8), r1 = nine-byte head destination, r2 = 28-halfword curve destination, r7 = develop record whose +0x110 = " ++ hx mark ++ "; the stock reader FUN_00690924 is mapped over the image-4 tone table."
              , cEntry = toneEntry
              , cArgs = [("r0", toneKey img4), ("r1", destBase), ("r2", destBase + 0x10), ("r7", recordBase)]
              , cRegs =
                  toneRegs
                    ++ [ synthReg "record" "r" recordBase (record mark) "develop record"
                       , synthReg "dest" "rw" destBase dest "destination, 0xee"
                       , codeReg "code-reader" img4 0x00690924 592
                       , dataReg "tonetab" "r" img4 0x00824bc4 (4 + 72 * fromIntegral (toneCount img4))
                       ]
              , cFns = toneFns ++ [(0x00690924, 592)]
              , cClaims = [("dest", 0, toneHead), ("dest", 0x10, toneCurve)]
              , cExpect =
                  if mark == 13
                    then "the parameter file's nine tone head bytes at r1 and its 28 curve halfwords at r2, the rest 0xee."
                    else "the stock reader's output: head bytes +4..+12 and curve halfwords +14..+0x45 of the table record with key " ++ hx (toneKey img4) ++ ", the rest 0xee."
              }
          ]
        | mark <- [13, 0]
        , let cc = mark == 13
        , let colourFields = [le16At (sl id5 (0x00d67f88 + 18 * colourId) 18) (2 * k) | k <- [0 .. 8]]
        , let colourOut = B.concat (map le64 (if cc then pMatrix params else map adjField colourFields))
        , let chromaFields = [le16At (sl id5 (0x00d61e14 + 8 * colourId) 8) o | o <- [0, 4, 6, 2]]
        , let chromaOut = B.concat (map le64 (if cc then chromaOutputOrder params else map adjField chromaFields))
        , let toneRec = toneRecord img4 (toneKey img4)
        , let toneHead = if cc then B.pack (map fromIntegral (pHead params)) else B.take 9 (B.drop 4 toneRec)
        , let toneCurve = if cc then B.concat (map le16 (pCurve params)) else B.take 56 (B.drop 14 toneRec)
        ]
      where
        (bt, img4, id5, params) = (shTag h, shId4 h, shId5 h, shParams h)
        (colourName, colourEntry, colourRegs, colourFns) = shSite h StubColour
        (chromaName, chromaEntry, chromaRegs, chromaFns) = shSite h StubChroma
        (toneName, toneEntry, toneRegs, toneFns) = shSite h StubTone
        provHead = shHead h
        base =
          Case
            { cId = ""
            , cFamily = "4 stage stubs"
            , cProv = ""
            , cEntry = 0
            , cArgs = []
            , cRegs = []
            , cFns = []
            , cStubs = []
            , cExpR = []
            , cClaims = []
            , cEvents = []
            , cPin = []
            , cPinFrom = Nothing
            , cExpect = ""
            , cExcl =
                [ "The stub is entered directly, not through its bl site; the stage function around the site and the stock post-processing (sign fill, gains, blend) are outside the case."
                , "The marker-0 case executes the stub's tail branch into the stock reader with the reader's own return to the exit address; the reader's id selection by the stage is outside the case."
                ]
            , cBudget = 100000
            , cInputs = [imgInput img4, imgInput id5, ("parameters", pPath params ++ " (SHA-256 " ++ pSha params ++ ")"), ("payload", "payload.bin (SHA-256 " ++ payloadSha ++ ")")]
            }

-- The 72-byte tone table record with the given key, or record 0 when absent.
toneRecord :: Img -> Word32 -> B.ByteString
toneRecord img key =
  let count = fromIntegral (wd img 0x00824bc4) :: Int
      recs = [sl img (0x00824bc8 + 72 * fromIntegral i) 72 | i <- [0 .. count - 1]]
   in case [r | r <- recs, le32At r 0 == key] ++ take 1 recs of
        (r : _) -> r
        [] -> error "tone table is empty"

-- ---------------------------------------------------------------------------
-- 5. RAW conversion sites
-- ---------------------------------------------------------------------------

rawStruct :: Word32
rawStruct = 0x00954d78

-- FUN_00631a00(prop, index, 0, 3) over the property's conversion table:
-- (value, flag) with flag 0 when the index names a row whose col2 is not 0x7ffffffe.
convIndex :: Img -> Word32 -> Word32 -> (Word32, Word32)
convIndex img prop row = go 0
  where
    tbl = wd img (0x0081b09c + 4 * prop)
    go i =
      let e = tbl + 12 * i
          ws = [wd img (e + 4 * k) | k <- [0 .. 2]]
       in if all (== 0xffff) ws
            then (0, 1)
            else if i == row then (if ws !! 2 == 0x7ffffffe then (0, 1) else (ws !! 2, 0)) else go (i + 1)

convTableLen :: Img -> Word32 -> Int
convTableLen img prop = go 0
  where
    tbl = wd img (0x0081b09c + 4 * prop)
    go i = if all (\k -> wd img (tbl + 12 * i + 4 * k) == 0xffff) [0 .. 2] then 12 * (fromIntegral i + 1) else go (i + 1)

rawCases :: [Side] -> [Case]
rawCases sides =
  [ onRam side [CcRawCommit] Case
      { cId = "rawcommit-" ++ tag ++ "-row-" ++ inTag row
      , cFamily = "5 RAW commit"
      , cProv = "FUN_00d46010 on the " ++ tag ++ " image 5 (commit, called from FUN_00cc453c), property 0x22b arm, with its FUN_0061d514 conversion through FUN_00631a00 on image 4. r0 = 0x22b, r1 = row token " ++ show row ++ "; field record 0x00954d78 seeded +0x08 = 0xa5a5a5a5 (colour) and +0x1c = 8 (a mono value), other bytes 0."
      , cEntry = 0x00d46010
      , cArgs = [("r0", 0x22b), ("r1", row)]
      , cRegs =
          [ codeReg "code-commit" img5 0x00d46010 0x2f8
          , codeReg "code-thunk" img4 0x0061d514 16
          , codeReg "code-conv" img4 0x00631a00 0x150
          , dataReg "convptr" "r" img4 (0x0081b09c + 4 * 0x22b) 4
          , dataReg "convtab" "r" img4 (wd img4 (0x0081b09c + 4 * 0x22b)) (convTableLen img4 0x22b)
          , synthReg "flag" "rw" 0x0089edc8 (le32 0) "converter not-found flag 0x0089edc8"
          , synthReg "fields" "rw" rawStruct rawSeed "RAW field record 0x00954d78"
          ]
      , cFns = [(0x00d46010, 0x2f8), (0x0061d514, 16), (0x00631a00, 0x150)]
      , cStubs = []
      , cExpR = []
      , cClaims = [("fields", 0x08, le32 colour), ("fields", 0x1c, le32 mono), ("flag", 0, le32 flag)]
      , cEvents = []
      , cPin = []
      , cPinFrom = Nothing
      , cExpect = "fields +0x08 = " ++ hx colour ++ ", +0x1c = " ++ hx mono ++ ", converter flag " ++ hx flag ++ " (rows 0,1,2,4,3 -> colour 0,5,3,6,7 and mono 8 cleared; rows 5..9 -> mono 6..10, colour 0; other rows -> both 0" ++ (if patched then "; contract: row 10 -> colour 8" else "") ++ "; flag from the 0x22b table read from image 4)."
      , cExcl = ["The record's later transfer to the develop request and the list draw are outside the case."]
      , cBudget = 20000
      , cInputs = [imgInput img5, imgInput img4]
      }
  | side <- sides
  , let (tag, img5, img4, patched) = (sdTag side, sdId5 side, sdId4 side, sdPatched side)
  , row <- [0 .. 10]
  , let (colour, mono) = rawCommit patched row
  , let flag = snd (convIndex img4 0x22b row)
  ]
    ++ [ onRam side [CcRawHighlight] Case
           { cId = "rawrow-" ++ tag ++ "-colour-" ++ inTag colour
           , cFamily = "5 RAW highlight"
           , cProv = "FUN_00d588b8 on the " ++ tag ++ " image 5 (selection query), property 0x22b arm: r2 = 0x22b, r0 = r1 = 0; field record 0x00954d78 with +0x08 = colour " ++ show colour ++ " and +0x1c = 0 (no mono value)."
           , cEntry = 0x00d588b8
           , cArgs = [("r2", 0x22b)]
           , cRegs = [codeReg "code-query" img5 0x00d588b8 10904, synthReg "fields" "r" rawStruct (B.replicate 8 0 <> le32 colour <> B.replicate 0x24 0) "RAW field record 0x00954d78"]
           , cFns = [(0x00d588b8, 10904)]
           , cStubs = []
           , cExpR = [("r0", row)]
           , cClaims = []
           , cEvents = []
           , cPin = []
           , cPinFrom = Nothing
           , cExpect = "r0 = row " ++ show row ++ " (colour 0,5,3,6,7 -> rows 0,1,2,4,3, else row 0" ++ (if patched then "; contract: colour 8 -> row 10" else "") ++ ")."
           , cExcl = ["Only the 0x22b arm of the query is executed."]
           , cBudget = 20000
           , cInputs = [imgInput img5]
           }
       | side <- sides
       , let (tag, img5, patched) = (sdTag side, sdId5 side, sdPatched side)
       , colour <- [0 .. 8]
       , let row = rawRow patched colour
       ]
    ++ [ onRam side [CcRawRequest] Case
           { cId = "rawreq-" ++ tag ++ "-film-" ++ inTag film
           , cFamily = "5 RAW request"
           , cProv = "Plain FUN_012546a8 on the " ++ tag ++ " plain image (RAW develop request builder, from FUN_01205afc) with its memset FUN_007221c8/FUN_007221a0 on image 4. The settings record pointer *(0x014ea86c) = 0x00954d78; the record has +0x08 = " ++ show film ++ " and every other field 0. The request block 0x01514218 (52 bytes) is seeded 0xee."
           , cEntry = 0x012546a8
           , cArgs = []
           , cRegs =
               [ codeReg "code-builder" plainImg 0x012546a8 (0x01255d00 - 0x012546a8)
               , codeReg "code-memset" img4 0x007221a0 0x78
               , synthReg "recptr" "r" 0x014ea86c (le32 rawStruct) "settings pointer cell 0x014ea6e4 + 0x188"
               , synthReg "settings" "r" rawStruct (B.replicate 8 0 <> le32 film <> B.replicate 0x34 0) "RAW settings record"
               , synthReg "request" "rw" 0x01514218 (B.replicate 0x34 0xee) "develop request 0x01514218"
               ]
           , cFns = [(0x012546a8, 0x8d0), (0x007221a0, 0x28), (0x007221c8, 0x50)]
           , cStubs = []
           , cExpR = []
           , cClaims = [("request", 8, le32 word2)]
           , cEvents = []
           , cPin = ["request"]
           , cPinFrom = if patched then Just ("rawreq-stock-film-" ++ inTag film) else Nothing
           , cExpect = "request word 2 (0x01514220) = " ++ hx word2 ++ " (3->1, 5->3, 6->0xb, 7->0xc, else 0" ++ (if patched then "; contract: 8->0xd; every other request byte equal to the stock case with the same record" else "") ++ ")."
           , cExcl = ["The request's handoff through FUN_01254f8c and FUN_00ea1838 to capture +0x508 is outside the case."]
           , cBudget = 50000
           , cInputs = [imgInput plainImg, imgInput img4]
           }
       | side <- sides
       , let (tag, plainImg, img4, patched) = (sdTag side, sdPlain side, sdId4 side, sdPatched side)
       , film <- [0 .. 9]
       , let word2 = rawRequest patched film
       ]
  where
    rawSeed = B.replicate 8 0 <> le32 0xa5a5a5a5 <> B.replicate 0x10 0 <> le32 8 <> B.replicate 0x10 0

rawCommit :: Bool -> Word32 -> (Word32, Word32)
rawCommit patched row
  | row >= 5 && row <= 9 = (0, row + 1)
  | Just c <- lookup row [(0, 0), (1, 5), (2, 3), (4, 6), (3, 7)] = (c, 0)
  | patched && row == 10 = (8, 0)
  | otherwise = (0, 0)

rawRow :: Bool -> Word32 -> Word32
rawRow patched colour = fromMaybe 0 (lookup colour ([(0, 0), (5, 1), (3, 2), (6, 4), (7, 3)] ++ [(8, 10) | patched]))

rawRequest :: Bool -> Word32 -> Word32
rawRequest patched film = fromMaybe 0 (lookup film ([(3, 1), (5, 3), (6, 0xb), (7, 0xc)] ++ [(8, 0xd) | patched]))

-- ---------------------------------------------------------------------------
-- 6. Film-simulation offer list
-- ---------------------------------------------------------------------------

offerNode, offerGlobals, offerState :: Word32
offerNode = 0x007823a4
offerGlobals = 0x009571d0
offerState = 0x00956838

data Offer = Offer {ofTokens :: [Word32], ofStates :: [Word32], ofGreyed :: Bool}

-- The open's states from the image bytes: each named token's col2 (conversion
-- table 0x0077ac30, column 2) is looked up in the availability matrix
-- 0x00795c98 at the mode's column of list 0x00792fac; cells 1 and 2 allow.
-- Fewer than two allowed values in the column greys every entry.
offerModel :: Img -> Word32 -> Offer
offerModel img mode = Offer tokens states greyed
  where
    values = wd img (offerNode + 0x20)
    tokens = takeWhile (/= 0x7ffffffe) [wd img (values + 8 * i) | i <- [0 ..]]
    conv = wd img (0x0081b09c + 4 * 0xfe)
    col2 t = wd img (conv + 12 * t + 8)
    column = case [i | i <- [0 .. 21], wd img (0x00792fac + 4 * i) == mode] of
      (i : _) -> i
      [] -> error ("mode " ++ hx mode ++ " has no column")
    rows = takeWhile (\r -> wd img r /= 0x7ffffffe) [0x00795c98 + 0x5c * i | i <- [0 ..]]
    cell key = case [r | r <- rows, wd img r == key] of
      (r : _) -> wd img (r + 4 + 4 * column)
      [] -> error ("col2 " ++ hx key ++ " has no matrix row")
    allowed v = v == 1 || v == 2
    greyed = length (filter (\r -> allowed (wd img (r + 4 + 4 * column))) rows) < 2
    states = [if greyed || not (allowed (cell (col2 t))) then 3 else 0 | t <- tokens]

offerGlobalsSeed :: B.ByteString
offerGlobalsSeed = le32 offerNode <> B.replicate 360 0xff

offerGlobalsFinal :: Offer -> B.ByteString
offerGlobalsFinal o =
  B.concat (map le32 (head8 ++ slots ++ tail4))
  where
    named = fromIntegral (length (ofTokens o))
    shown = fromIntegral (length (filter (< 3) (ofStates o)))
    ff = 0xffffffff
    head8 = if ofGreyed o then [offerNode, ff, ff, ff, ff, ff, named, 0] else [offerNode, ff, 0, 0, shown, 0, named, shown]
    slots = take 0x4f (ofStates o ++ repeat 0)
    tail4 = if ofGreyed o then [ff, ff, ff, ff] else [0, 0, ff, 0]

-- Sides with the name of their label and values arrays.
flashOfferSides :: World -> [(Side, String)]
flashOfferSides w = [(stock, "stock"), (Side "rc0" (wRc0 w) (wId5 w) (wPlain w) False Nothing, "RC0 (installed decode)"), (patched, "patched")]
  where
    (stock, patched) = case flashSides w of
      [a, c] -> (a, c)
      _ -> error "flash sides"

offerCases :: [(Side, String)] -> [Case]
offerCases sides =
  [ onRam side [] Case
      { cId = "offer-" ++ tag ++ "-mode-" ++ inTag mode
      , cFamily = "6 offer list"
      , cProv = "The FILMSIM-EMUL-01 list-open ABI (FUN_00bfa800 kind 1, call site 0x00bfa3e4-0x00bfa3f0: r0 = 1, r1 = 1, r2 = 21) on the shooting-menu node 0x007823a4 over the " ++ arrays ++ " arrays, mode cell DAT_00956838 = " ++ hx mode ++ " (column " ++ show column ++ " of list 0x00792fac). DAT_00956840, DAT_00956844, DAT_009568b8 and DAT_009575fc are 0. The list block 0x009571d0 is seeded with the node pointer and 0xff. FUN_00bfa234 (event post) is a registers-only stub returning r0 = 0."
      , cEntry = 0x00bfa800
      , cArgs = [("r0", 1), ("r1", 1), ("r2", 21)]
      , cRegs =
          [ codeReg "code-walk" img5 0x00bf9794 5124
          , codeReg "code-probe" img5 0x00d52684 36612
          , codeReg "code-thunks" img4 0x0061d018 1356
          , codeReg "code-queries" img4 0x00631a00 1536
          , dataReg "conv-table" "r" img4 0x0077ac30 168
          , dataReg "conv-pointers" "r" img4 0x0081b0b0 1000
          , dataReg "tables" "r" img4 0x00792fac 25976
          , dataReg "node" "r" img4 offerNode 48
          , dataReg "values" "r" img4 values (8 * (length (ofTokens o) + 1))
          , synthReg "globals" "rw" offerGlobals offerGlobalsSeed "list block 0x009571d0, node pointer then 0xff"
          , synthReg "state-block" "rw" offerState (le32 mode <> B.replicate 128 0) "mode and selection cells 0x00956838"
          , synthReg "conv-flag" "rw" 0x0089edc8 (le32 0) "converter flag"
          , synthReg "case5-cell" "rw" 0x009575fc (le32 0) "case-5 tail gate"
          ]
      , cFns =
          [ (0x0061d018, 32), (0x0061d514, 16), (0x0061d534, 16), (0x0061d554, 16), (0x00631a00, 336), (0x00631b74, 484), (0x00631edc, 292)
          , (0x00bf9794, 200), (0x00bf985c, 48), (0x00bf988c, 260), (0x00bf9990, 156), (0x00bf9a2c, 48), (0x00bfa800, 920), (0x00d52684, 25140), (0x00d588b8, 10904)
          ]
      , cStubs = [postStub]
      , cExpR = [("r0", if ofGreyed o then 0 else 1)]
      , cClaims = [("globals", 0, offerGlobalsFinal o)]
      , cEvents = [postStub | ofGreyed o]
      , cPin = []
      , cPinFrom = Nothing
      , cExpect =
          "named rows " ++ show (ofTokens o) ++ ", states " ++ show (ofStates o) ++ ", named count " ++ show (length (ofTokens o)) ++ ", displayed count " ++ show (length (filter (< 3) (ofStates o)))
            ++ (if ofGreyed o then "; fewer than two values allowed in the column, so every state is 3, the walk posts event 0xf through FUN_00bfa234 and returns its r0 (0) with the selection words untouched" else "; the walk returns 1")
            ++ " (states derived from the conversion table, the mode list and the availability matrix read from " ++ iName img4 ++ ")."
      , cExcl =
          [ "Rendering and the menu draw are not modelled: the closure computes per-entry states and counts only; the label arrays are inputs no executed function reads."
          , "FUN_00bfa234 (event post) is a registers-only stub; what the posted event does is outside the case."
          , "The executed bytes' correspondence to installed bytes rests on E-DUMP-RAM-150 for the stock images and on the build output for the patched images."
          ]
      , cBudget = 400000
      , cInputs = [imgInput img5, imgInput img4]
      }
  | (side, arrays) <- sides
  , let (tag, img4, img5) = (sdTag side, sdId4 side, sdId5 side)
  , mode <- [0, 5]
  , let o = offerModel img4 mode
  , let values = wd img4 (offerNode + 0x20)
  , column : _ <- [[i | i <- [0 .. 21 :: Int], wd img4 (0x00792fac + 4 * fromIntegral i) == mode]]
  ]
  where
    postStub = Stub 0x00bfa234 [("r0", 0xf), ("r1", 1), ("r2", 0x15), ("r3", 0)] [("r0", 0)]

-- ---------------------------------------------------------------------------
-- 7. Label
-- ---------------------------------------------------------------------------

recordsBase :: Word32
recordsBase = 0x0037b784

ccRecord :: B.ByteString
ccRecord =
  let units = map (fromIntegral . fromEnum) "CLASSIC CHROME" ++ [0]
   in B.concat (map le16 units) <> B.replicate (56 - 2 * length units) 0 <> B.concat (map le16 [40, 0, 30, 36, 48])

-- Per side: image 2, the special records the case maps, and their provenance
-- and region notes.
flashLabelSources :: World -> [(Side, Img, B.ByteString, String, String)]
flashLabelSources w = case flashSides w of
  [stock, patched] ->
    [ (stock, wId2 w, stockRecs, "the stock image 2", "id2-stock [0x0037b784, 0x0037b84a)")
    , (patched, wId2 w, B.take 132 stockRecs <> ccRecord, "the stock image 2 with the 0x83e record replaced by the contract record (UTF-16LE \"CLASSIC CHROME\", zero unit, zero fill to 56 bytes, layout halfwords 40, 0, 30, 36, 48), built by this generator", "image 2 [0x0037b784, 0x0037b84a) with the CC 0x83e record")
    ]
  _ -> error "flash sides"
  where
    stockRecs = sl (wId2 w) recordsBase 198

ramLabelSource :: Side -> RamWorld -> (Side, Img, B.ByteString, String, String)
ramLabelSource side rw = (side, rwId2 rw, sl (rwId2 rw) recordsBase 198, "the RAM image 2 " ++ iName (rwId2 rw) ++ " (the stock image 2 with the manifest's image-2 publications)", iName (rwId2 rw) ++ " [0x0037b784, 0x0037b84a)")

labelCases :: [(Side, Img, B.ByteString, String, String)] -> [Case]
labelCases sources =
  [ mk kind source labelId
  | source <- sources
  , kind <- ["label", "locator"]
  , labelId <- [0x83c, 0x83d, 0x83e, 0x83f]
  ]
  where
    locate i = if i >= 0x83c && i <= 0x83e then recordsBase + 66 * (i - 0x83c) else 0
    accessor patched i = if i <= (if patched then 0x83e else 0x83d) then locate i else 0
    mk kind (side, id2, recs, provSource, regSource) labelId =
      let (tag, plainImg, patched) = (sdTag side, sdPlain side, sdPatched side)
          out = if kind == "label" then accessor patched labelId else locate labelId
          text = if out == 0 then "" else recordText (B.drop (fromIntegral (out - recordsBase)) recs)
       in onRam side [] Case
            { cId = kind ++ "-" ++ tag ++ "-" ++ showHex labelId ""
            , cFamily = "7 label"
            , cProv =
                (if kind == "label" then "Plain text accessor FUN_01179b0c with its record locator 0x0117dfd8" else "Plain record locator 0x0117dfd8 entered directly")
                  ++ " on the " ++ tag ++ " plain image, r0 = string id " ++ hx labelId ++ ", r1 = 0. Records 0x0037b784 (three 66-byte special records) come from "
                  ++ provSource
                  ++ "."
            , cEntry = if kind == "label" then 0x01179b0c else 0x0117dfd8
            , cArgs = [("r0", labelId)]
            , cRegs =
                [ codeReg "code-textid" plainImg 0x01179b0c 120
                , codeReg "code-locator" plainImg 0x0117dfd8 112
                , Reg "records" recordsBase 198 "r" recs regSource
                ]
            , cFns = [(0x01179b0c, 120) | kind == "label"] ++ [(0x0117dfd8, 112)]
            , cStubs = []
            , cExpR = [("r0", out)]
            , cClaims = []
            , cEvents = []
            , cPin = []
            , cPinFrom = Nothing
            , cExpect =
                "r0 = " ++ h32 out
                  ++ (if out == 0 then " (refused" ++ (if kind == "label" then " by the accessor's gate 0x01179b14 #" ++ (if patched then "0x83e" else "0x83d") else " by the locator's id-1 gate #0x83d") ++ ")" else ", the record whose inline text is \"" ++ text ++ "\"")
                  ++ "."
            , cExcl = ["The returned pointer is the result; drawing, metrics and the other 24 gate sites are outside the case."]
            , cBudget = 10000
            , cInputs = [imgInput plainImg, imgInput id2]
            }

recordText :: B.ByteString -> String
recordText bs = map (toEnum . fromIntegral) (takeWhile (/= 0) [le16At bs (2 * k) | k <- [0 .. 27]])

-- ---------------------------------------------------------------------------

main :: IO ()
main = do
  o <- getArgs >>= parseOpts defaults
  w <- loadWorld o
  checkContract w
  ensure (recordText (B.take 66 ccRecord) == "CLASSIC CHROME" && B.length ccRecord == 66) "CC record encoding"
  let offerKey img mode = let x = offerModel img mode in (ofTokens x, ofStates x, ofGreyed x)
  forM_ (wRams w) $ \rw -> do
    checkRam w rw
    forM_ [0, 5] $ \mode -> ensure (offerKey (rwId4 rw) mode == offerKey (wP4 w) mode) (rwTag rw ++ ": the offer model in mode " ++ show mode ++ " differs from the flash build's")
  let out = outDir o
  createDirectoryIfMissing True out
  runner <- makeAbsolute (oRunnerPath o)
  arts <- newIORef M.empty
  finals <- newIORef M.empty
  let env = Env out runner arts finals
      fs = flashSides w
      rs = ramSides w
      flashCases = convCases w fs ++ validatorCases w ++ devCases fs ++ metaCases fs ++ stubCases w (flashStubHosts w) ++ rawCases fs ++ offerCases (flashOfferSides w) ++ labelCases (flashLabelSources w)
      ramPairs = zip rs (wRams w)
      ramCases =
        convCases w rs ++ devCases rs ++ metaCases rs
          ++ stubCases w [ramStubHost side rw | (side, rw) <- ramPairs]
          ++ rawCases (take 1 fs ++ rs)
          ++ offerCases [(side, "RAM (RC0 with the CC publications)") | side <- rs]
          ++ labelCases [ramLabelSource side rw | (side, rw) <- ramPairs]
      cases = if null (wRams w) then flashCases else ramCases
  outcomes <- forM cases $ \c -> do
    r <- runOne env c
    putStrLn (oStatus r ++ " " ++ cId c ++ " runner=" ++ oRunner r ++ " r0=" ++ oR0 r ++ " instructions=" ++ show (oInstr r) ++ concatMap (" " ++) (oMismatch r))
    pure r
  let row r = "| " ++ cId (oCase r) ++ " | " ++ cFamily (oCase r) ++ " | " ++ cExpect (oCase r) ++ " | " ++ oStatus r ++ ", " ++ oRunner r ++ ", r0 " ++ oR0 r ++ ", " ++ show (oInstr r) ++ " instructions |"
  writeFile (out </> "results.md") (unlines (["| Case | Family | Expected before execution | Observed |", "| --- | --- | --- | --- |"] ++ map row outcomes))
  B.writeFile
    (out </> "results.json")
    ( encodeCanonical
        ( arr
            [ obj
                [ ("id", s (cId (oCase r)))
                , ("family", s (cFamily (oCase r)))
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
