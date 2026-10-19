{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
-- Recipe-bank cases observe public returns, readback buffers, live settings
-- and card bytes. Foreign media requests complete synchronously in the model.
module Main where

import Control.Monad
import FujiAF.Host (sha256, str)
import Data.Aeson hiding (encodeFile)
import Data.Aeson.Types (Parser, parseEither, parseMaybe)
import qualified Data.ByteString as B
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
import Emulation.NativeCase (runCase)
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

-- bank file codec, built independently of menu/recipe_bank_storage.c
checksum :: B.ByteString -> Word32
checksum f = complement (sum [fromIntegral (complement (if i >= 0x18 && i < 0x1c then 0 else B.index f i) :: Word8) | i <- [0 .. 511]])
  where complement w = maxBound - w

encodeFile :: Word32 -> Word16 -> [(B.ByteString, B.ByteString)] -> B.ByteString
encodeFile gen mask slots = putU32 0x18 ck file0
  where
    hdr = B.concat [B.pack (map (fromIntegral . fromEnum) "FJRB"), u16le 1, u16le 0x20, u16le 150, u16le 7, u16le 12, u16le 0x28, u32le gen, u32le 462, u32le 0, u16le mask, u16le 0]
    payloads = B.concat [maybe (B.replicate 0x28 0) snd (at i) | i <- [0 .. 6]]
    names = B.concat [maybe (B.replicate 26 0) fst (at i) | i <- [0 .. 6]]
    at i = if i < length slots then Just (slots !! i) else Nothing
    body = payloads <> names <> B.replicate 18 0
    file0 = hdr <> body
    ck = checksum file0
    putU16 o v b = B.take o b <> u16le v <> B.drop (o + 2) b
    putU32 o v b = B.take o b <> u32le v <> B.drop (o + 4) b

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

data Model = Model
  { mCard :: B.ByteString            -- 0x660 bytes at 0x00980000
  , mLive :: B.ByteString            -- 0x1c2 bytes at 0x0096cc64
  , mCells :: M.Map String Word32    -- model-data cells by symbol
  , mMode :: Word32
  , mShootingMode :: Word32          -- the mode cell 0x00956838 (0x21 P, 0x22 S)
  , mCase :: B.ByteString            -- bytes at 0x009a0000 (name buffers)
  , mExtra :: [(String, Integer, B.ByteString)] -- named regions at RAM addresses
  }

data Expect = Expect
  { eStatus :: String, eR0 :: Word32
  , eClaims :: [(String, Int, B.ByteString)]
  }

newtype GuestAddress = GuestAddress Integer deriving (Eq, Ord)
newtype RegionId = RegionId String deriving (Eq, Ord)

data BankOperation = BankSaveCurrent | BankReset | BankRename | BankRecall

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
  "out-window-stub" -> ("0x0004357c", [], [("r0", v)])
  _ -> error ("unknown stub " ++ n)


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
  d3Digests <- mapM sha256 [id4Final, id5Final]
  (d3Id4, d3Id5) <- case d3Digests of
    [a, b] -> pure (a, b)
    _ -> fail "compiled-recipe candidate: two image digests required"
  putStrLn ("kernel .text " ++ show ktextLen ++ " .data " ++ show kdataLen ++ " media " ++ show (B.length mediaImg) ++ " fields " ++ show (length fields))
  putStrLn
    ( "compiled-recipe candidate "
        ++ candidateDir
        ++ ": id4-final "
        ++ d3Id4
        ++ ", id5-final "
        ++ d3Id5
    )
  specs <- pure (specSet ks ms fields tabs hostBank (realStock (GuestAddress id4Base) id4Raw id5) ++ d3SpecSet id4Final id5Final)
  results <- forM specs $ \sp -> do
    let loop s fuel = do
          r <- runSpec build out runner ks ms kbin ktextLen kdataLen mdataRaw fTable getter setter mediaImg tabImg mediaLo s
          let missing = parseMaybe (withObject "r" (\o -> o .: "missing_fn" >>= withText "t" (pure . T.unpack))) r
          case missing of
            Just n | fuel > 0 && n `notElem` spFns s -> loop (s { spFns = spFns s ++ [n] }) (fuel - 1)
            _ -> pure r
    loop sp (48 :: Int)
  bootResults <- maybe (pure []) (\package -> (++) <$> runBootCases out runner package <*> runAllocatorCases out runner package) (opt "boot-package")
  let allResults = results ++ bootResults
  BL.writeFile (out </> "results.json") (encode (object ["cases" .= allResults, "compiled_recipe_candidate" .= object ["dir" .= candidateDir, "id4_final_sha256" .= d3Id4, "id5_final_sha256" .= d3Id5]]))
  writeFile (out </> "findings.txt") (findingsText specs results ++ if null bootResults then "" else "\nBoot cases use register-only stock semaphore ABI boundaries and stop at DSB after publication stores. Cache/core/source-coherence acceptance is not claimed. Allocator cases execute the packaged stock backing allocator at its new capacity boundary.\n")
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
-- block with the mode cell 0x21, the settings mirror with the not-found
-- flag set and the stock per-slot defaults row. The menu and mode tables share
-- one region. The synthetic getter and setter retire: 0x0061e110 and 0x00624694
-- are the real code inside the chain.
realStock :: GuestAddress -> B.ByteString -> B.ByteString -> [(String, Integer, B.ByteString)]
realStock (GuestAddress id4Base) id4Raw id5 =
  [ ("chain", id4Base, chain)
  , ("dispatcher", 0x00d4d604, B.take 20608 (B.drop (0x00d4d604 - 0x00bf1a00) id5))
  , ("conv-tables", 0x00774f48, B.take 0x68dc (B.drop (0x774f48 - 0x61c400) id4))
  , ("conv-ptrs", 0x0081b09c, B.take 0x860 (B.drop (0x81b09c - 0x61c400) id4))
  , ("cells", 0x00956838, poke 0 (u32le 0x21) (B.replicate 0x1000 0))
  , ("menutab", 0x00780000, B.take (0x7a1460 - 0x780000) (B.drop (0x780000 - 0x61c400) id4))
  , ("defaults-row", defaultsRowAddress, B.take 0x28 (B.drop (fromIntegral defaultsRowAddress - 0x61c400) id4))
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

defaultsRowAddress :: Integer
defaultsRowAddress = 0x007cb804

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
  , ("modetab", 0x7a0018, at4 0x7a0018 0x1400)
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
      (Model (B.replicate 0x660 0) (B.replicate 0x1c2 0xaa) M.empty 1 0x21 B.empty (d3Extras id4f id5f))
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

specSet :: Syms -> Syms -> [Field] -> M.Map Word32 [[Word32]] -> B.ByteString -> [(String, Integer, B.ByteString)] -> [Spec]
specSet ks ms fields tabs hostBank real = map ($ ()) builders
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
    -- excludes zero, so it carries the smallest admitted code (1); the nine
    -- recipe rows carry the literal codes the extraction case names
    fieldCode f = fromMaybe (if fProp f == 0x137 then 1 else 0) (lookup (fProp f) extractCodes)
    payloadReal = foldl (\b f -> placePayload b f (fieldCode f)) (B.replicate 0x28 0) fields
    liveReal = foldl (\b f -> placeLive b f (fieldCode f)) (B.replicate 0x1c2 0) fields
    -- the payload the save's walk emits for the live fixture, and the live
    -- block the stock getter reads it from
    validPayload = payloadReal
    emptyCard = B.replicate 0x660 0
    cardWith a b = B.take 0x220 (a <> B.replicate 0x220 0) <> B.take 0x220 (b <> B.replicate 0x220 0) <> B.replicate 0x220 0
    model st = Model emptyCard liveFixture (M.fromList st) 1 0x21 B.empty real
    builders = map (\operation _ -> staleMediaOperation operation)
      [BankRename, BankSaveCurrent, BankReset, BankRecall] ++
      existingBuilders
    existingBuilders =
      [ \_ -> stageMediaChanged, \_ -> blankLoad, \_ -> validLoad, \_ -> bothLoad, \_ -> hostLoad
      , \_ -> atCRecall, \_ -> unsetRecall, \_ -> genExhaust, \_ -> extractSave
      , \_ -> realRecall, \_ -> realConsumers, \_ -> realE9, \_ -> realRefused
      , \_ -> realIsoAuto ("s", 0x22, "0x22"), \_ -> realIsoAuto ("m", 0x24, "0x24"), \_ -> realIsoAutoMode0, \_ -> realIsoAutoNoFlag, \_ -> realIsoManualAfterAuto
      , \_ -> badChecksum, \_ -> badCode, \_ -> badUnbound
      , \_ -> stageOk, \_ -> stageCancel, \_ -> resetCase, \_ -> renameCase, \_ -> renameUnsetCase
      , \_ -> readOnlySave, \_ -> fullCardSave, \_ -> partialWrite, \_ -> powerLoss
      , \_ -> mediaMoveInScan, \_ -> mediaMoveInOpen, \_ -> mediaMoveInWrite, \_ -> closeDeferred
      , \_ -> twoDeferredReadback, \_ -> openTagZero, \_ -> writeCloseOwned, \_ -> writeCloseLate ]
    staleMediaOperation operation =
      let (suffix, target) = case operation of
            BankSaveCurrent -> ("save", "rb_save_current")
            BankReset -> ("reset", "rb_reset")
            BankRename -> ("rename", "rb_name_commit")
            BankRecall -> ("recall", "rb_recall")
          identityArgument = case operation of
            BankRename -> 0xe1a02004
            _ -> 0xe1a01004
          workload = B.concat (map u32le
            [ 0xe92d4010, 0xe59fc024, 0xe12fff3c, 0xe1a04000
            , 0xe59fc01c, 0xe12fff3c, 0xe3a00000, 0xe59f1018
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
      "contract: public readback returns all seven literal names and halfword-bearing payloads from the independently Haskell-encoded bank without changing card bytes"
      "rbm_seq_load_all_readback" [0x009a0000, 0x009a0100] ((model (fileA [])) { mCard = cardWith hostBank B.empty })
      (Expect "returned" 0
        [("casebuf", 0, B.take 182 (B.drop 0x138 hostBank)), ("casebuf", 0x100, B.take 280 (B.drop 0x20 hostBank)), ("card", 0, cardWith hostBank B.empty)])
      ("rbm_seq_load_all_readback" : "rb_readback" : loadFns) bankStubs
    extractLive = liveReal
    extractPayload = payloadReal
    extractSave = Spec "b-save-extract-codes"
      "contract: the fixed extraction, recipeOf 1 3 4 4 3 2 3 0xb 5 (DR/highlight/shadow/colour/sharp/NR/WB/ISO/film): the save's file payload carries the literal shifted bytes +0x05=0x40 +0x07=0x80 +0x08=0x60 +0x09=0x80 +0x10=0x40 +0x17=0x60 +0x20=0x0b +0x21=0x03 +0x24=0x05 and nothing else, and the live state (C record row 0, ordinary row 1, the wireless 0xa8/0xa9 bytes) is byte-identical after the save - the old zero-on-shift extraction would write zeros at the shifted fields and the C-preservation claim would still pass, so the payload claim is the distinguishing assertion"
      "rb_save_current" [1, 0]
      ((model []) { mLive = extractLive, mCard = emptyCard })
      (Expect "returned" 0 [("card", 0, cardWith (encodeFile 1 0x0002 [(name26 "", B.replicate 0x28 0), (name26 "", extractPayload)]) B.empty),
        ("live-state", 0, extractLive)]
        )
      saveFns bankStubs
    blankLoad = Spec "a-load-blank"
      "contract: rb_load on a card with no bank file returns 9 (no-bank) without changing card bytes or live settings"
      "rb_load" [] (model []) (Expect "returned" 9 [("card", 0, emptyCard), ("live-state", 0, liveFixture)])
      loadFns bankStubs
    validA = encodeFile 7 0x0001 (replicate 7 (name26 "ALPHA", validPayload))
    fileA st = ("rbm_a_present", 1) : ("rbm_a_size", 512) : st
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
    atCRecall = Spec "c-recall-at-c"
      "contract: recall at mode-class 3 (dial at C) returns 8 and leaves the card untouched"
      "rb_recall" [2] ((model []) { mMode = 3 })
      (Expect "returned" 8 [("card", 0, emptyCard)])
      ["rb_recall"] []
    unsetRecall = Spec "c-recall-unset"
      "contract: recall of an unset slot returns 6 (unset) with no record change; the scan runs but the mask bit is clear"
      "rb_recall" [1] ((model (fileA [])) { mCard = cardWith validA B.empty })
      (Expect "returned" 6 [("card", 0, cardWith validA B.empty)])
      ("rb_recall" : loadFns) bankStubs
    exhaust = encodeFile 0xffffffff 0x0001 (replicate 7 (name26 "ALPHA", validPayload))
    recallPayload = extractPayload
    recallFile = encodeFile 7 0x0001 [(name26 "ALPHA", recallPayload)]
    -- the real-byte recall (the storage lane's real-stock contract): the
    -- kernel walks every one of the 83 fields through the stock getter and
    -- setter and re-reads them through the stock reader, so status 0 is the
    -- aggregate verdict that every field, the halfwords included, holds its
    -- stored code. The halfword pair rides the setter's ordinary branch to
    -- the live block head at 0x0096cc74/0x0096cc76 (the record pair at
    -- 0x0096cdcc/ce is the slot != 1 branch this path never takes).
    realRecallPayload = u16le 0x1234 <> u16le 0x5678 <> B.drop 4 extractPayload
    realRecallFile = encodeFile 7 0x0001 ((name26 "ALPHA", realRecallPayload) : replicate 6 (name26 "ALPHA", payloadReal))
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
      , "0x0061cd14/264", "0x0061d154/80", "0x0061d534/16", "0x0061d544/16", "0x00631d58/388", "0x00631b74/484", "0x00632860/192", "0x00632920/20", "0x00632934/28", "0x00632950/116", "0x0063386c/32", "0x006338ec/32", "0x0063390c/32", "0x0063392c/32", "0x0063394c/32", "0x0063398c/32", "0x006339ac/32", "0x00633a0c/32", "0x00633a2c/32", "0x00633a4c/32", "0x00633a8c/32", "0x00633aac/32", "0x00633acc/32", "0x00633aec/32", "0x00633b0c/32", "0x00633b4c/32", "0x00633c0c/32", "0x00633c8c/32", "0x00633ccc/32", "0x00633cec/32", "0x00633e0c/32", "0x00633e8c/32", "0x00633eac/32", "0x006340cc/32", "0x0063412c/32", "0x0063432c/32", "0x0063462c/36", "0x00634830/32", "0x00634850/32", "0x00634890/32", "0x006348b0/32", "0x006348d0/32", "0x00635aac/36", "0x00635ad0/36", "0x006394d0/24", "0x0063d3bc/220", "0x0063d5fc/24", "0x0063d614/116", "0x0063d688/1180", "0x0063db24/80", "0x0063ead0/144", "0x006425ec/80", "0x00643c4c/152", "0x00643e30/40", "0x00643ec4/68", "0x00643f08/40", "0x00644004/80", "0x00644054/44", "0x006440b4/212", "0x00644188/56", "0x006441c0/244", "0x006442b4/56", "0x006442ec/116", "0x00644360/244", "0x006464cc/40", "0x006464f4/44", "0x00646574/48", "0x00646af0/116", "0x00646c04/152" ]
    realStubs = ["task", "vol", "card", "tone-notify", "pipeline-notify", "display-notify", "mode-state-a", "mode-state-b", "mode-state-c", "journal-stub", "out-window-stub"]
    realRecall = Spec "c-recall-real-bytes"
      "contract: recall through the real stock getter, setter, writer, reader and commit dispatcher: the status is 0 (the kernel's own re-read of all 83 walked fields agrees), the live halfwords at 0x0096cc74/0x0096cc76 carry the payload's +0x00/+0x02 codes through the setter's ordinary branch, and the film cell 0x009568b8 carries the recalled film code's col2 from the table at 0x0077ac30 (code 5 maps to col2 9)"
      "rb_recall" [0] ((model (fileA [])) { mCard = cardWith realRecallFile B.empty, mLive = B.replicate 0x1c2 0 })
      (Expect "returned" 0
        [ ("live-state", 0x10, B.take 4 realRecallPayload)
        , ("cells", 0x80, u32le 9)
        , ("card", 0, cardWith realRecallFile B.empty)
        ])
      ("rb_recall" : loadFns ++ realStockFns) realStubs
    -- RECIPE-STATE-APPLY-01 closure: the grouped consumer cells, the e9 row
    -- and the refusal fixture on the passing recall's real-byte setup
    payloadCode f = fromIntegral ((fromIntegral (B.index realRecallPayload (fOff f)) .&. fMask f) `shiftR` fShift f)
    col2For p = case M.lookup p tabs of
      Just rows -> let c = payloadCode (fromMaybe (error "row missing") (find ((== p) . fProp) fields))
                    in fromMaybe 0 (listToMaybe [ c2 | (c0 : _ : c2 : _) <- rows, c0 == c ])
      Nothing -> 0
    cellFor p off = ("cells", off, u32le (col2For p))
    realConsumers = Spec "c-recall-real-consumers"
      "contract: every walked row whose arm carries a consumer runs with the real code: the eight cell writers observed effective are WB mode 0x00956864, sharpness 0x009568a0, film 0x009568b8, DR 0x009568bc, colour 0x009568c0, noise reduction 0x0095699c, highlight 0x00956b40 and shadow 0x00956b44, each holding the col2 of the committed row whose col0 is the payload's stored code; the recall still reports 0"
      "rb_recall" [0] ((model (fileA [])) { mCard = cardWith realRecallFile B.empty, mLive = B.replicate 0x1c2 0 })
      (Expect "returned" 0
        [ cellFor 0xed 0x2c, cellFor 0x10f 0x68, cellFor 0xfe 0x80, cellFor 0x104 0x84
        , cellFor 0x10e 0x88, cellFor 0xef 0x164, cellFor 0x110 0x308, cellFor 0x111 0x30c
        , ("card", 0, cardWith realRecallFile B.empty) ])
      ("rb_recall" : loadFns ++ realStockFns) realStubs
    e9Payload = putByte 0x0a (.|. 0xe0) realRecallPayload
    e9RecallFile = encodeFile 7 0x0001 ((name26 "ALPHA", e9Payload) : replicate 6 (name26 "ALPHA", payloadReal))
    e9Reread = encodeFile 8 0x0001 ((name26 "", e9Payload) : replicate 6 (name26 "ALPHA", payloadReal))
    realE9 = Spec "c-recall-real-e9-preserved"
      "contract: the remembered payload's 0xe9 bits survive staged commit in the durable new generation"
      "rbm_seq_stage_roundtrip" [0, 0x009a0000]
      ((model (fileA [])) { mCard = cardWith e9RecallFile B.empty })
      (Expect "returned" 0
        [ ("card", 0, cardWith e9RecallFile e9Reread) ])
      ("rbm_seq_stage_roundtrip" : "rb_stage_load" : "rb_stage_commit" : "rb_stage_image" : "rb_write_back" : loadFns ++ realStockFns) realStubs
    refusedPayload = putByte 0x23 (\v -> (v .&. 0xf0) .|. 0x0b) realRecallPayload
    refusedRecallFile = encodeFile 7 0x0001 ((name26 "ALPHA", refusedPayload) : replicate 6 (name26 "ALPHA", payloadReal))
    realRefused = Spec "c-recall-real-refused"
      "contract: the assert pass is what makes recall status 0 mean every field holds its code. 0x1b1 stored as 0xb (row 9 of table 0x77a54c, whose col2 is the 0x7ffffffe not-found sentinel) cannot hold its code: the arm's convert round trip through the sentinel writes 0 on the first apply and again on the re-apply, so the kernel answers 14 (RB_APPLY_REFUSED); the failing field's live byte holds the arm's last write and the fields other arms rewrite (0x1d, 0xfa) still read back their codes"
      "rb_recall" [0] ((model (fileA [])) { mCard = cardWith refusedRecallFile B.empty, mLive = B.replicate 0x1c2 0 })
      (Expect "returned" 14
        [ ("live-state", 0x83, B.pack [0])
        , ("live-state", 0x80, B.pack [0])
        , ("live-state", 0x46, B.pack [0])
        , ("card", 0, cardWith refusedRecallFile B.empty) ])
      ("rb_recall" : loadFns ++ realStockFns) realStubs
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
    -- live manual ISO 0x0c with the auto flag at 1
    isoManualLive = putByte 0x9c (const 0x04) (putByte 0x52 (const 0x0c) (B.replicate 0x1c2 0))
    isoAutoStep = stepOf 0x14f (codeIn isoAutoPayload 0x152)
    isoModel m = m { mExtra = withPipelineModel (mExtra m) }
    liveByte p code off = B.singleton (B.index (placeLive (B.replicate 0x1c2 0) (fieldOf p) code) off)
    realIsoAuto (modeName, mode, modeHex) = Spec ("c-recall-iso-auto-" ++ modeName)
      ("contract: in mode " ++ map toUpper modeName ++ " (mode cell " ++ modeHex ++ ", auto flag 0x146 in table 0x007a0018, AUTO maximum 0x152 in table 0x007a09fc) a slot carrying ISO AUTO (0x141 = 0x1b) with its payload 0x146 bit at 1, 0x161 = 0, 0x152 code 2 and 0x14f code 0 recalls into the stock AUTO state and returns 0: 0x146 reads 0, 0x152 holds its stored code 2 although the 0x14f commit wrote 0 into it, the ISO step 0x009568b4 is the step table 0x14f gives code 2, 0x141 is the code whose step that is, never 0x1b, and 0x161 holds 0 after the ISO arm's FUN_00641438(2, 0) wrote 2 (modeled); rb_recall_trace keeps its initial magic and zero count")
      "rb_recall" [0] (isoModel (model (fileA [])) { mCard = cardWith isoAutoFile B.empty, mLive = isoManualLive, mShootingMode = mode })
      (Expect "returned" 0
        [ ("live-state", 0x52, B.singleton (fromIntegral (codeOfStep 0x141 isoAutoStep)))
        , ("live-state", 0x9c, B.singleton (flagByte isoAutoPayload 0))
        , ("live-state", 0x6e, liveByte 0x161 0 0x6e)
        , ("live-state", 0x47, liveByte 0x152 (codeIn isoAutoPayload 0x152) 0x47)
        , ("cells", cellsIsoStep, u32le isoAutoStep)
        , ("kdata", traceOffset, B.pack [0x52, 0x43, 0x42, 0x52] <> B.replicate 32 0)
        , ("card", 0, cardWith isoAutoFile B.empty) ])
      ("rb_recall" : isoFns) isoStubs
    -- mode 0, the AUTO dial: flag 0x142, AUTO maximum 0x14f, ISO property 0x157
    -- (tables 0x007a0018, 0x007a09fc, 0x007a13c8); live manual, 0x142 = 1. DR
    -- (0x104, +0x05 bits 6-7) is stored as 0: in mode 0 the stored code 1 reads
    -- back 0 and refuses the recall whatever the ISO.
    isoManualLiveMode0 = putByte 0x9d (const 0x01) isoManualLive
    isoAutoPayloadMode0 = putByte 0x05 (.&. 0x3f) isoAutoPayload
    isoAutoFileMode0 = encodeFile 7 0x0001 ((name26 "ALPHA", isoAutoPayloadMode0) : replicate 6 (name26 "ALPHA", payloadReal))
    realIsoAutoMode0 = Spec "c-recall-iso-auto-mode-0"
      "contract: in mode 0 (the AUTO dial) an ISO AUTO slot recalls into mode 0's stock AUTO state and returns 0: the commit of row 0x1b sets the flag 0x142 to 0, the ISO step 0x009568b4 to the step table 0x14f gives the stored 0x14f code 0, and FUN_00644848 writes the matching code into 0x157, which the assert reads in place of 0x141; rb_recall_trace keeps its initial magic and zero count"
      "rb_recall" [0] (isoModel (model (fileA [])) { mCard = cardWith isoAutoFileMode0 B.empty, mLive = isoManualLiveMode0, mShootingMode = 0 })
      (Expect "returned" 0
        [ ("live-state", 0x9d, liveByte 0x142 0 0x9d)
        , ("cells", cellsIsoStep, u32le (stepOf 0x14f (codeIn isoAutoPayloadMode0 0x14f)))
        , ("kdata", traceOffset, B.pack [0x52, 0x43, 0x42, 0x52] <> B.replicate 32 0)
        , ("card", 0, cardWith isoAutoFileMode0 B.empty) ])
      ("rb_recall" : isoFns) isoStubs
    realIsoAutoNoFlag = Spec "c-recall-iso-auto-no-flag"
      "contract: in mode cell 0x25, whose table 0x007a0018 entry is 0 (no auto flag), a slot carrying ISO AUTO is refused before any field is applied: the recall returns 14 (RB_APPLY_REFUSED), every live byte is unchanged and rb_recall_trace records count 1, status 0x0e, slot 0, property 0x141, stored 0x1b, read-back 0x0c, mode 0x25 and one refused field"
      "rb_recall" [0] (isoModel (model (fileA [])) { mCard = cardWith isoAutoFile B.empty, mLive = isoManualLive, mShootingMode = 0x25 })
      (Expect "returned" 14
        [ ("live-state", 0, isoManualLive)
        , ("kdata", traceOffset, B.concat (map u32le [0x52424352, 1, 0x0e, 0, 0x141, 0x1b, 0x0c, 0x25, 1]))
        , ("card", 0, cardWith isoAutoFile B.empty) ])
      ("rb_recall" : isoFns) isoStubs
    -- a consistent AUTO slot (0x141 = 0x1b, 0x146 = 0) edited to manual ISO 6
    -- in the bank editor, then recalled in S over a live AUTO state
    isoAutoSlot = putByte 0x1f (.&. 0x7f) isoAutoPayload
    isoAutoSlotFile = encodeFile 7 0x0001 ((name26 "ALPHA", isoAutoSlot) : replicate 6 (name26 "ALPHA", payloadReal))
    isoManualPayload = putByte 0x1f (.|. 0x80) (putByte 0x20 (const 0x06) isoAutoSlot)
    isoManualFile = encodeFile 8 0x0001 ((name26 "ALPHA", isoManualPayload) : replicate 6 (name26 "ALPHA", payloadReal))
    isoAutoLive = putByte 0x52 (const (fromIntegral (codeOfStep 0x141 isoAutoStep))) (B.replicate 0x1c2 0)
    realIsoManualAfterAuto = Spec "c-recall-iso-manual-after-auto"
      "contract: editing an AUTO slot's ISO to code 6 stages 0x146 = 1 with it (the new generation's payload carries 0x141 = 6 and the +0x1f bit 7 set), and recalling that slot in S over a live AUTO state ends in manual ISO: 0x146 reads 1, 0x141 reads 6, the ISO step 0x009568b4 is row 6's step, and the recall returns 0"
      "rbm_seq_stage_field_recall" [0, 0x141, 6]
      (isoModel (model (fileA [])) { mCard = cardWith isoAutoSlotFile B.empty, mLive = isoAutoLive, mShootingMode = 0x22 })
      (Expect "returned" 0
        [ ("live-state", 0x52, B.singleton 6)
        , ("live-state", 0x9c, B.singleton (flagByte isoManualPayload 1))
        , ("cells", cellsIsoStep, u32le (stepOf 0x141 6))
        , ("card", 0, cardWith isoAutoSlotFile isoManualFile) ])
      ("rbm_seq_stage_field_recall" : "rb_recall" : "rb_stage_load" : "rb_stage_field" : "rb_stage_commit" : "rb_stage_image" : "rb_write_back" : isoFns) isoStubs
    recallApply = Spec "c-recall-apply"
      "contract: modeled recall of a set slot updates ordinary row 1 to the literal payload and leaves the card untouched; stock processing-cache application is outside this model"
      "rb_recall" [0] ((model (fileA [])) { mCard = cardWith recallFile B.empty, mLive = B.replicate 0x1c2 0 })
      (Expect "returned" 0
        [("live-state", 0, B.take 0x168 (B.replicate 0x1c2 0) <> recallPayload <> B.drop 0x190 (B.replicate 0x1c2 0)), ("card", 0, cardWith recallFile B.empty)]
        )
      ("rb_recall" : "rb_walk_payload_to_live" : loadFns) bankStubs
    putByte off f b = B.take off b <> B.pack [f (B.index b off)] <> B.drop (off + 1) b
    putWord off v b = B.take off b <> u32le v <> B.drop (off + 4) b
    traceOffset = fromIntegral (addr ks "rb_recall_trace" - addr ks "__rb_text_end")
    loadFns = ["rb_load", "rb_scan_file", "rb_open", "rb_map_completion", "rb_io_cb", "rb_memcpy", "rb_read_count", "rb_close_quiet", "rb_checksum_file", "rb_name_ok", "rb_codes_ok", "rb_code_ok", "rb_unbound_ok", "rb_extract", "rb_place",
      "0x0061d514/16", "0x00631a00/336"]
    saveFns = loadFns ++ ["rb_save_current", "rb_write_back", "rb_stage_image", "0x0061e110/25832", "0x006439e0/40"]
    bankStubs = ["task", "vol", "card", "tone-notify", "pipeline-notify", "display-notify", "mode-state-a", "mode-state-b", "mode-state-c"]
    badChecksumFile = putWord 0x18 (readU32 validA 0x18 + 1) validA
    badCodeFile = putByte 0x40 (\v -> (v .&. 0xe0) .|. 0x1f) validA
    badUnboundFile = putByte 0x47 (.|. 0x04) validA
    emptyBankClaim = [("live-state", 0, liveFixture)]
    badChecksum = Spec "d-load-bad-checksum"
      "contract: a checksum mismatch refuses the whole bank with INVALID and leaves the card and live settings unchanged"
      "rb_load" [] ((model (fileA [])) { mCard = cardWith badChecksumFile B.empty })
      (Expect "returned" 5 (emptyBankClaim ++ [("card", 0, cardWith badChecksumFile B.empty)])
        )
      loadFns bankStubs
    badCode = Spec "d-load-bad-code"
      "contract: a file carrying a code the conversion table does not admit (ISO 31 against its 0..12 set) is refused whole: rb_load returns 5 and the live settings are never touched"
      "rb_load" [] ((model (fileA [])) { mCard = cardWith badCodeFile B.empty })
      (Expect "returned" 5 (emptyBankClaim ++ [("card", 0, cardWith badCodeFile B.empty)])
        )
      loadFns bankStubs
    badUnbound = Spec "d-load-unbound"
      "contract: a file carrying a value in the unbound +0x27 bits 2-7 is refused whole: rb_load returns 5 even though every bound code is valid"
      "rb_load" [] ((model (fileA [])) { mCard = cardWith badUnboundFile B.empty })
      (Expect "returned" 5 (emptyBankClaim ++ [("card", 0, cardWith badUnboundFile B.empty)])
        )
      loadFns bankStubs
    editedPayload = putByte 5 (const 0x80) validPayload
    stageFile = encodeFile 8 0x0001 ((name26 "", editedPayload) : replicate 6 (name26 "ALPHA", extractPayload))
    stageOk = Spec "e-stage-ok"
      "contract: a staged field edit (DR to code 2) followed by OK persists into the new-generation target file with the edit and the staged name, and leaves the old file intact"
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
    resetFile = encodeFile 8 0x0000 ((name26 "", B.replicate 0x28 0) : replicate 6 (name26 "ALPHA", validPayload))
    resetCase = Spec "f-reset"
      "contract: reset clears the slot's name and payload and its set-mask bit in the new-generation target file, and leaves the old file intact"
      "rb_reset" [0] ((model (fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "returned" 0 [("card", 0, cardWith validA resetFile)]
        )
      ("rb_reset" : "rb_stage_image" : "rb_write_back" : loadFns) bankStubs
    renameFile = encodeFile 8 0x0001 ((name26 "DELTA", validPayload) : replicate 6 (name26 "ALPHA", validPayload))
    renameCase = Spec "g-rename"
      "contract: renaming a set slot to DELTA persists the new name in the new-generation target file with the payload unchanged, and leaves the old file intact"
      "rb_name_commit" [0, 0x009a0000] ((model (fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive, mCase = name26 "DELTA" })
      (Expect "returned" 0 [("card", 0, cardWith validA renameFile)]
        )
      ("rb_name_commit" : "rb_stage_image" : "rb_write_back" : loadFns) bankStubs
    defaultsRow = fromMaybe (error "defaults-row region missing") (listToMaybe [b | ("defaults-row", _, b) <- real])
    renameUnsetFile = encodeFile 8 0x0003 ((name26 "ALPHA", validPayload) : (name26 "DELTA", defaultsRow) : replicate 5 (name26 "ALPHA", validPayload))
    renameUnsetCase = Spec "g-rename-unset"
      "contract: renaming unset slot 1 to DELTA sets it: the new-generation target file carries DELTA over the stock per-slot defaults row 0x007cb804 with set-mask bit 1 added, the other slots unchanged, and leaves the old file intact"
      "rb_name_commit" [1, 0x009a0000] ((model (fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive, mCase = name26 "DELTA" })
      (Expect "returned" 0 [("card", 0, cardWith validA renameUnsetFile)]
        )
      ("rb_name_commit" : "rb_stage_image" : "rb_write_back" : loadFns) bankStubs
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
    saveFile = encodeFile 8 0x0001 (replicate 7 (name26 "ALPHA", extractPayload))
    partialFile = B.take 256 saveFile <> B.replicate 256 0
    partialWrite = Spec "h-partial-write"
      "contract: a write that stops at the half-way boundary returns 3 (full) and leaves exactly the first half of the new file at the target slot; the winner file is byte-identical"
      "rb_save_current" [0, 0] ((model (("rbm_write_short", 256) : fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "returned" 3 [("card", 0, cardWith validA partialFile)]
        )
      saveFns bankStubs
    powerLoss = Spec "h-power-loss"
      "contract: interruption halfway through the target-file write leaves the winning generation byte-identical"
      "rb_save_current" [0, 0] ((model (("rbm_trip_step", 2) : fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "memory_guard" 0x00005150 [("card", 0, cardWith validA partialFile)]
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
      B.take 0x440 card <> B.take 0x220 (validA <> B.replicate 0x220 0)
        <> B.take 0x440 replacementCard
    workerSwapModel boundary =
      (model (swapBoundaryCell boundary : fileA
        [ ("rbm_b_present", 1), ("rbm_b_size", 512)
        , ("rbm_swap_a_size", 512), ("rbm_swap_b_present", 1), ("rbm_swap_b_size", 512)
        , ("rbm_swap_card", 0x00980660)
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
    gen9File = encodeFile 9 0x0001 (replicate 7 (name26 "ALPHA", extractPayload))
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
    _ = ()

runSpec :: FilePath -> FilePath -> FilePath -> Syms -> Syms -> B.ByteString -> Int -> Int -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Integer -> Spec -> IO Value
runSpec build out runner ks ms kbin ktextLen kdataLen mdataRaw fTable getter setter mediaImg tabImg mediaLo sp = do
  let dir = out </> spId sp
  createDirectoryIfMissing True dir
  let modeCell = B.replicate 0x24 0 <> u32le (mMode (spModel sp))
      mdata = overlay mdataRaw ([ (addr ms n - 0x970000, u32le v) | (n, v) <- M.toList (mCells (spModel sp))]
        ++ [(addr ms "rbm_field_count" - 0x970000, u32le (fromIntegral (B.length fTable `div` 20)))])
      mdata' = overlay (mdata <> B.replicate 0x1000 0) [(0x800, fTable)]
      overlay base ps = foldl (\b (o, v) -> B.take (fromIntegral o) b <> v <> B.drop (fromIntegral o + B.length v) b) base ps
      cardImg = mCard (spModel sp)
      cardLength = max 0x660 (B.length cardImg)
      liveImg = mLive (spModel sp)
      caseImg = mCase (spModel sp)
      extras = mExtra (spModel sp)
      patchMode = overlay (B.replicate 0x42100 0) [(0x41424, u32le (mMode (spModel sp)))]
      patchCells b = u32le (mShootingMode (spModel sp)) <> B.drop 4 b
      extrasPatch = [(rid, a, if rid == "media-state" then patchMode else if rid == "cells" then patchCells b else b) | (rid, a, b) <- extras]
      exitAddress = mediaLo + fromIntegral (B.length mediaImg)
      mediaImg' = mediaImg <> u32le 0xe12fff1e
      arts = [("kernel.bin", kbin), ("media.bin", mediaImg'), ("getter.bin", getter), ("setter.bin", setter), ("model-data.bin", B.take 0x1000 mdata'), ("tables.bin", tabImg), ("card.bin", cardImg), ("live.bin", liveImg)]
        ++ (if B.null caseImg then [] else [("case.bin", caseImg)])
        ++ [(rid ++ ".bin", b) | (rid, _, b) <- extrasPatch, not (B.null b)]
  forM_ (arts ++ [("mode.bin", modeCell)]) $ \(n, b) -> B.writeFile (dir </> n) b
  digests <- M.fromList <$> mapM (\(n, b) -> (,) n <$> sha256 b) (arts ++ [("mode.bin", modeCell)])
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
        | rid `elem` ["conv-ptrs", "modetab", "defaults-row"] = "r"
        | otherwise = "rw"
      fns = map fnEntry (nub (spFns sp))
      fnEntry n
        | take 2 n == "0x" = case break (== '/') n of
            (a, '/' : s) -> object ["address" .= hex (hexI (drop 2 a)), "size" .= (read s :: Integer)]
            _ -> error ("raw function entry needs 0xADDR/SIZE: " ++ n)
        | otherwise = object ["address" .= hex (addrOf n), "size" .= (toInteger (sizeOf' n) :: Integer)]
      hasK n = n `elem` map fst (M.toList (case ks of Syms m -> m))
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
        , "limits" .= object ["instructions" .= (1000000 :: Int), "wall_ms" .= (4000 :: Int)]
        , "expected" .= expected
        , "exclusions" .= (["Kernel behavior is evaluated over declared public output spans, not whole-region private residue or incidental stub event order", "Foreign media completion is synchronous; stock queue, scheduler and physical SD controller are not executed"] :: [String]) ]
      addrOfEntry = case spEntry sp of
        e | take 2 e == "0x" -> hexI e
        e -> if e `elem` M.keys (case ks of Syms m -> m) then addr ks e else addr ms e
  BL.writeFile (dir </> "case.json") (encode caseObj)
  (_, report) <- runCase runner (dir </> "case.json")
  BL.writeFile (dir </> "runner-report.json") (encode report)
  let regionIds = M.fromList
        [(GuestAddress (hexI (str (getRegionField region "address"))), RegionId (str (getRegionField region "id"))) | region <- regions]
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

runBootCases :: FilePath -> FilePath -> FilePath -> IO [Value]
runBootCases out runner package = do
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
  forM [Publish, ShortImage, CorruptHash, InvalidDestination, SourceNotLoaded, WaitRefused] $ \scenario -> do
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
      (n,) <$> sha256 bytes
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
    pure (object ["id" .= cid, "status" .= fromMaybe "unknown" (parseMaybe (withObject "report" (.: "status")) report :: Maybe String), "report" .= report])

data AllocatorScenario = ExactFit | OneByteOverflow deriving (Eq)

runAllocatorCases :: FilePath -> FilePath -> FilePath -> IO [Value]
runAllocatorCases out runner package = do
  id4 <- B.readFile (package </> "image/id4-final.bin")
  let code = B.take 84 (B.drop (0x72b110 - 0x61c1fc) id4)
      originalCursor = 0x00be18e0
  forM [ExactFit, OneByteOverflow] $ \scenario -> do
    let fits = scenario == ExactFit
        cid = if fits then "carve-allocator-exact-fit" else "carve-allocator-overflow"
        dir = out </> cid
        bytes = u32le originalCursor <> u32le 9
        regions = [("code", 0x0072b110, code, "rx" :: String), ("allocator", 0x00829a08, bytes, "rw"),
                   ("stack", 0x009a0000, B.replicate 32 0, "rw"), ("exit", 0x009b0000, B.pack [0x1e,0xff,0x2f,0xe1], "rx")]
        expectedState = u32le (if fits then 0x00be19e0 else originalCursor) <> u32le (if fits then 9 else 12)
    createDirectoryIfMissing True dir
    digests <- forM regions $ \(n, _, raw, _) -> B.writeFile (dir </> (n ++ ".bin")) raw >> ((n,) <$> sha256 raw)
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
    pure result
