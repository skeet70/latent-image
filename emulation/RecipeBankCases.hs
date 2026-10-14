{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
-- Recipe-bank kernel behavior cases. Each case's memory image is composed
-- from pinned inputs; every behavior expectation is derived from the storage
-- contract before execution. The stack region and the kernel's operational
-- cells carry no contract fact: phase 1 observes them and phase 2 pins them
-- as OBSERVED RESIDUE, never as behavior claims.
--
-- The d3-* family runs the composed candidate's own slices instead: the stock
-- commit dispatcher entered with property 0xfe and the film row token, the
-- prefix-resident compiled-recipe hook, and the consumer cells its arms write.
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
  , mCase :: B.ByteString            -- bytes at 0x009a0000 (name buffers)
  , mExtra :: [(String, Integer, B.ByteString)] -- named regions at RAM addresses
  }

data Expect = Expect
  { eStatus :: String, eR0 :: Word32
  , eClaims :: [(String, Int, B.ByteString)]
  }

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
  let candidateDir = fromMaybe "private-data/analysis/x-m1-1.50/recipes/runtime/combined-candidate" (opt "candidate")
  createDirectoryIfMissing True out
  ks <- parseSyms (build </> "kernel.syms")
  ms <- parseSyms (build </> "model.syms")
  kbin <- B.readFile (build </> "kernel.bin")
  ktext <- B.readFile (build </> "kernel-text.bin")
  mdataRaw <- B.readFile (build </> "model-data.bin")
  id4 <- B.readFile id4Path
  id5 <- B.readFile id5Path
  fieldsRaw <- B.readFile fieldsPath
  let ktextLen = B.length ktext
      kdataLen = B.length kbin - ktextLen
      fields = parseFields fieldsRaw
      fTable = B.concat [B.concat [u32le (fProp f), u32le (fromIntegral (fOff f)), u32le (fromIntegral (fShift f)), u32le (fromIntegral (fBytes f)), u32le (fMask f)] | f <- fields]
      mediaSpans = ["rbm_low_close", "rbm_read", "rbm_write", "rbm_close", "rbm_open", "rbm_stat", "rbm_index_free", "rbm_mapping_delete", "rbm_resolve", "rbm_tag_release"]
      mediaLo = minimum [addr ms n | n <- mediaSpans]
      mediaHi = maximum [addr ms n + sizeOf ms n | n <- mediaSpans ++ ["rbm_event_hook", "rbm_detach_event", "rbm_attach_event", "rbm_media_eval", "rbm_tag_release"]]
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
  specs <- pure (specSet ks ms fields tabs hostBank (realStock id4 id5) ++ d3SpecSet id4Final id5Final)
  results <- forM specs $ \sp -> do
    let loop s fuel = do
          r <- runSpec build out runner ks ms kbin ktextLen kdataLen mdataRaw fTable getter setter mediaImg tabImg mediaLo s
          let missing = parseMaybe (withObject "r" (\o -> o .: "missing_fn" >>= withText "t" (pure . T.unpack))) r
          case missing of
            Just n | fuel > 0 && n `notElem` spFns s -> loop (s { spFns = spFns s ++ [n] }) (fuel - 1)
            _ -> pure r
    loop sp (48 :: Int)
  BL.writeFile (out </> "results.json") (encode (object ["cases" .= results, "compiled_recipe_candidate" .= object ["dir" .= candidateDir, "id4_final_sha256" .= d3Id4, "id5_final_sha256" .= d3Id5]]))
  writeFile (out </> "findings.txt") (findingsText specs results)
  putStrLn ("ran " ++ show (length specs) ++ " cases")
  unless (all ((== Just ("passed" :: String)) . parseMaybe (withObject "result" (.: "status"))) results) exitFailure

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
-- flag set. The synthetic getter and setter retire: 0x0061e110 and 0x00624694
-- are the real code inside the chain.
realStock :: B.ByteString -> B.ByteString -> [(String, Integer, B.ByteString)]
realStock id4 id5 =
  [ ("chain", 0x0061c91c, chain)
  , ("dispatcher", 0x00d4d604, B.take 20608 (B.drop (0x00d4d604 - 0x00bf1a00) id5))
  , ("conv-tables", 0x00774f48, B.take 0x68dc (B.drop (0x774f48 - 0x61c400) id4))
  , ("conv-ptrs", 0x0081b09c, B.take 0x860 (B.drop (0x81b09c - 0x61c400) id4))
  , ("cells", 0x00956838, poke 0 (u32le 0x21) (B.replicate 0x1000 0))
  , ("modetab", 0x007a0018, B.take 0x1400 (B.drop (0x7a0018 - 0x61c400) id4))
  , ("menutab", 0x00780000, B.take 0x20000 (B.drop (0x780000 - 0x61c400) id4))
  , ("media-state", 0x00910140, B.replicate 0x42100 0)
  , ("mirror", 0x0089ec00, poke 0x1c8 (u32le 1) (B.replicate 0x200 0))
  ]
  where
    chain = poke (0x61d9ec - 0x61c91c) bxlr (poke (0x61d8dc - 0x61c91c) bxlr raw)
    raw = B.take 0x2c000 (B.drop (0x61c91c - 0x61c400) id4)
    bxlr = B.pack [0x1e, 0xff, 0x2f, 0xe1]
    poke o v b = B.take o b <> v <> B.drop (o + B.length v) b

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
      (Model (B.replicate 0x660 0) (B.replicate 0x1c2 0xaa) M.empty 1 B.empty (d3Extras id4f id5f))
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
    model st = Model emptyCard liveFixture (M.fromList st) 1 B.empty real
    builders =
      [ \_ -> blankLoad, \_ -> validLoad, \_ -> bothLoad, \_ -> hostLoad
      , \_ -> atCRecall, \_ -> unsetRecall, \_ -> genExhaust, \_ -> extractSave
      , \_ -> realRecall, \_ -> realConsumers, \_ -> realE9, \_ -> realRefused, \_ -> badChecksum, \_ -> badCode, \_ -> badUnbound
      , \_ -> stageOk, \_ -> stageCancel, \_ -> resetCase, \_ -> renameCase
      , \_ -> readOnlySave, \_ -> fullCardSave, \_ -> partialWrite, \_ -> powerLoss
      , \_ -> mediaMoveInScan, \_ -> mediaMoveInWrite, \_ -> closeDeferred
      , \_ -> openTagZero, \_ -> writeCloseOwned, \_ -> writeCloseLate ]
    hostLoad = Spec "a-load-host-fixture"
      "contract: cross-language direction A - the C kernel loads a bank file encoded by the Haskell codec (host-bank.bin, literal names ALPHA..GOLF and literal halfword payloads at +0x00/+0x02) and accepts its header, checksum and all seven slots; the resident image equals the host file byte for byte"
      "rb_load" [] ((model (fileA [])) { mCard = cardWith hostBank B.empty })
      (Expect "returned" 0 [("kdata", fromIntegral (addr ks "rb_resident"), hostBank), ("card", 0, cardWith hostBank B.empty)]
        )
      loadFns ["task", "vol", "card", "live", "store"]
    extractLive = liveReal
    extractPayload = payloadReal
    extractSave = Spec "b-save-extract-codes"
      "contract: the fixed extraction, recipeOf 1 3 4 4 3 2 3 0xb 5 (DR/highlight/shadow/colour/sharp/NR/WB/ISO/film): the save's file payload carries the literal shifted bytes +0x05=0x40 +0x07=0x80 +0x08=0x60 +0x09=0x80 +0x10=0x40 +0x17=0x60 +0x20=0x0b +0x21=0x03 +0x24=0x05 and nothing else, and the live state (C record row 0, ordinary row 1, the wireless 0xa8/0xa9 bytes) is byte-identical after the save - the old zero-on-shift extraction would write zeros at the shifted fields and the C-preservation claim would still pass, so the payload claim is the distinguishing assertion"
      "rb_save_current" [1, 0x009a0000]
      ((model []) { mLive = extractLive, mCard = emptyCard })
      (Expect "returned" 0 [("card", 0, cardWith (encodeFile 1 0x0002 [(name26 "", B.replicate 0x28 0), (name26 "", extractPayload)]) B.empty),
        ("live-state", 0, extractLive)]
        )
      saveFns bankStubs
    blankLoad = Spec "a-load-blank"
      "contract: rb_load on a card with no bank file returns 9 (no-bank) and publishes the empty bank"
      "rb_load" [] (model []) (Expect "returned" 9 [("kdata", fromIntegral (addr ks "rb_resident"), B.replicate 512 0)])
      ["rb_load", "rb_scan_file", "rb_open", "rb_map_completion", "rb_io_cb", "rb_memcpy"] ["task", "vol", "card", "live", "store"]
    validA = encodeFile 7 0x0001 (replicate 7 (name26 "ALPHA", validPayload))
    fileA st = ("rbm_a_present", 1) : ("rbm_a_size", 512) : st
    validLoad = Spec "a-load-valid-a"
      "contract: rb_load with one valid RECIPEA.BIN returns 0 and the resident image equals that file byte for byte; the A scan opens, stats, reads 512 and closes"
      "rb_load" [] ((model (fileA [])) { mCard = cardWith validA B.empty })
      (Expect "returned" 0 [("kdata", fromIntegral (addr ks "rb_resident"), validA), ("card", 0, cardWith validA B.empty)]
        )
      loadFns ["task", "vol", "card", "live", "store"]
    validB = encodeFile 9 0x0002 (replicate 7 (name26 "BETA", validPayload))
    bothLoad = Spec "a-load-b-newer"
      "contract: with both files valid the higher generation wins; B carries generation 9 against A's 7, so the resident image equals RECIPEB.BIN"
      "rb_load" [] ((model (fileA [("rbm_b_present", 1), ("rbm_b_size", 512)])) { mCard = cardWith validA validB })
      (Expect "returned" 0 [("kdata", fromIntegral (addr ks "rb_resident"), validB), ("card", 0, cardWith validA validB)]
        )
      loadFns ["task", "vol", "card", "live", "store"]
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
      [ "0x00d4d604/20", "0x00d4fc74/10768", "0x0061c91c/16"
      , "0x0061c93c/16", "0x0061d8dc/80", "0x0061d9ec/80"
      , "0x0061e110/25832", "0x00624620/44", "0x00624694/49968", "0x006309c4/112"
      , "0x0063396c/32", "0x00633b6c/32", "0x00633b8c/32"
      , "0x00633cac/32", "0x00633d0c/32", "0x0063458c/32", "0x0063460c/32"
      , "0x006347b0/32", "0x006347d0/32", "0x006439e0/40", "0x00643d94/48"
      , "0x00643dc4/40", "0x00643e9c/40", "0x00643fb8/76", "0x00644848/476"
      , "0x0061cd14/264", "0x0061d154/80", "0x0061d534/16", "0x00631b74/484", "0x00632860/192", "0x00632920/20", "0x00632934/28", "0x00632950/116", "0x0063386c/32", "0x006338ec/32", "0x0063390c/32", "0x0063392c/32", "0x0063394c/32", "0x0063398c/32", "0x006339ac/32", "0x00633a0c/32", "0x00633a2c/32", "0x00633a4c/32", "0x00633a8c/32", "0x00633aac/32", "0x00633acc/32", "0x00633aec/32", "0x00633b0c/32", "0x00633b4c/32", "0x00633c0c/32", "0x00633c8c/32", "0x00633ccc/32", "0x00633cec/32", "0x00633e0c/32", "0x00633e8c/32", "0x00633eac/32", "0x006340cc/32", "0x0063412c/32", "0x0063432c/32", "0x0063462c/36", "0x00634830/32", "0x00634850/32", "0x00634890/32", "0x006348b0/32", "0x006348d0/32", "0x00635aac/36", "0x00635ad0/36", "0x006394d0/24", "0x0063d3bc/220", "0x0063d5fc/24", "0x0063d614/116", "0x0063d688/1180", "0x0063db24/80", "0x0063ead0/144", "0x006425ec/80", "0x00643c4c/152", "0x00643e30/40", "0x00643ec4/68", "0x00643f08/40", "0x00644004/80", "0x00644054/44", "0x006440b4/212", "0x00644188/56", "0x006441c0/244", "0x006442b4/56", "0x006442ec/116", "0x00644360/244", "0x006464cc/40", "0x006464f4/44", "0x00646574/48", "0x00646af0/116", "0x00646c04/152" ]
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
      "contract: the bound-not-walked 0xe9 row (payload +0x0a bits 5-7) survives the validation and the remembered-payload write path: the loaded file carries 0xe0 at +0x0a, a staged commit of slot 0 writes the new generation whose payload keeps those bits (no walk row carries them and normalisation does not mask them), and the resident equals the written file"
      "rbm_seq_stage_roundtrip" [0, 0x009a0000]
      ((model (fileA [])) { mCard = cardWith e9RecallFile B.empty })
      (Expect "returned" 0
        [ ("card", 0, cardWith e9RecallFile e9Reread)
        , ("kdata", fromIntegral (addr ks "rb_resident"), e9Reread) ])
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
    recallApply = Spec "c-recall-apply"
      "contract: modeled recall of a set slot updates ordinary row 1 to the literal payload and leaves the card untouched; stock processing-cache application is outside this model"
      "rb_recall" [0] ((model (fileA [])) { mCard = cardWith recallFile B.empty, mLive = B.replicate 0x1c2 0 })
      (Expect "returned" 0
        [("live-state", 0, B.take 0x168 (B.replicate 0x1c2 0) <> recallPayload <> B.drop 0x190 (B.replicate 0x1c2 0)), ("card", 0, cardWith recallFile B.empty)]
        )
      ("rb_recall" : "rb_walk_payload_to_live" : loadFns) bankStubs
    putByte off f b = B.take off b <> B.pack [f (B.index b off)] <> B.drop (off + 1) b
    putWord off v b = B.take off b <> u32le v <> B.drop (off + 4) b
    loadFns = ["rb_load", "rb_scan_file", "rb_open", "rb_map_completion", "rb_io_cb", "rb_memcpy", "rb_read", "rb_read_count", "rb_close_quiet", "rb_checksum_file", "rb_name_ok", "rb_codes_ok", "rb_code_ok", "rb_unbound_ok", "rb_extract", "rb_place",
      "0x0061d514/16", "0x00631a00/336"]
    saveFns = loadFns ++ ["rb_save_current", "rb_write_back", "rb_stage_image", "0x0061e110/25832", "0x006439e0/40"]
    bankStubs = ["task", "vol", "card", "tone-notify", "pipeline-notify", "display-notify", "mode-state-a", "mode-state-b", "mode-state-c"]
    badChecksumFile = putWord 0x18 (readU32 validA 0x18 + 1) validA
    badCodeFile = putByte 0x40 (\v -> (v .&. 0xe0) .|. 0x1f) validA
    badUnboundFile = putByte 0x47 (.|. 0x04) validA
    emptyBankClaim = [("kdata", fromIntegral (addr ks "rb_resident"), B.replicate 512 0)]
    badChecksum = Spec "d-load-bad-checksum"
      "contract: a file whose checksum word does not match its bytes is refused whole: rb_load returns 5 (invalid) with bytes seen, publishes the empty bank and leaves the card untouched"
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
    readOnlySave = Spec "h-write-read-only"
      "contract: a save against a read-only card (the write completion 8 maps to read-only) returns 2 and leaves both files untouched"
      "rb_save_current" [0, 0x009a0000] ((model (("rbm_write_comp", 8) : fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "returned" 2 [("card", 0, cardWith validA B.empty)]
        )
      saveFns bankStubs
    fullCardSave = Spec "h-write-full"
      "contract: a save against a full card (the write completion 3 maps to full) returns 3 and leaves both files untouched"
      "rb_save_current" [0, 0x009a0000] ((model (("rbm_write_comp", 3) : fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "returned" 3 [("card", 0, cardWith validA B.empty)]
        )
      saveFns bankStubs
    saveFile = encodeFile 8 0x0001 ((name26 "", extractPayload) : replicate 6 (name26 "ALPHA", extractPayload))
    partialFile = B.take 256 saveFile <> B.replicate 256 0
    partialWrite = Spec "h-partial-write"
      "contract: a write that stops at the half-way boundary returns 3 (full) and leaves exactly the first half of the new file at the target slot; the winner file is byte-identical"
      "rb_save_current" [0, 0x009a0000] ((model (("rbm_write_short", 256) : fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "returned" 3 [("card", 0, cardWith validA partialFile)]
        )
      saveFns bankStubs
    powerLoss = Spec "h-power-loss"
      "contract: a cut at the mid-write boundary (model trip step 2, the write's half-way boundary) stops the run at the same half-written target and leaves the winner file byte-identical; r0 pins the fault site's residue register"
      "rb_save_current" [0, 0x009a0000] ((model (("rbm_trip_step", 2) : fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "memory_guard" 0x00970008 [("card", 0, cardWith validA partialFile)]
        )
      saveFns bankStubs
    genExhaust = Spec "g2-save-gen-exhaust"
      "contract: a save against a winner at generation 0xffffffff must return 10 (generation exhausted) and truncate no file"
      "rb_save_current" [1, 0x009a0000] ((model (fileA [])) { mCard = cardWith exhaust B.empty })
      (Expect "returned" 10 [("card", 0, cardWith exhaust B.empty)]
        )
      saveFns bankStubs
    -- the two media-identity races of the write-back: the twelfth media call is
    -- the write-back's B scan open and the fourteenth is the destructive write, so
    -- rbm_event_step (the model's detach/attach shim, which bumps the epoch at
    -- the start of the numbered media call) places the card change inside the
    -- scan and inside the write
    mediaMoveInScan = Spec "h-save-media-move-in-scan"
      "contract: a card change at the write-back's B scan (epoch bumped by the tenth media call) returns 13 (media-changed) and writes nothing: the winner file and the resident bank are the load's, and the changed-epoch open releases nothing"
      "rb_save_current" [0, 0x009a0000] ((model (("rbm_event_step", 12) : fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "returned" 13
        [ ("card", 0, cardWith validA B.empty)
        , ("kdata", fromIntegral (addr ks "rb_resident"), validA)
        ])
      saveFns bankStubs
    mediaMoveInWrite = Spec "h-save-media-move-mid-write"
      "contract: a card change at the destructive write (epoch bumped by the twelfth media call, after the open and before its callback) returns 13 (media-changed) with the target file carrying the written generation and the resident bank still the winner's: nothing is published across an identity"
      "rb_save_current" [0, 0x009a0000] ((model (("rbm_event_step", 14) : fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "returned" 13
        [ ("card", 0, cardWith validA saveFile)
        , ("kdata", fromIntegral (addr ks "rb_resident"), validA)
        ])
      saveFns bankStubs
    closeDeferred = Spec "h-write-close-deferred-flush"
      "contract: a close that completes with a success callback and a zero return (the state-1 no-op at the fifteenth media call, the write-back's close) releases nothing, so the handle is deferred and the next open re-dispatches its close; the write-back still succeeds and the tag table at region offset 0x80 (0x009101c0) ends all-zero, which is the release the flush performed"
      "rb_save_current" [0, 0x009a0000] ((model (("rbm_close_noop_at", 15) : fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "returned" 0
        [ ("card", 0, cardWith validA saveFile)
        , ("kdata", fromIntegral (addr ks "rb_resident"), saveFile)
        , ("media-state", 0x80, B.replicate 64 0)
        ])
      saveFns bankStubs
    -- FILE-FAILURE-OWNERSHIP-01: the failure and ownership shapes of the
    -- media identity, driven through the kernel's real request and release
    -- code. rb_open flushes the deferred list before it posts, rb_close_quiet
    -- defers whenever the tag table still owns the handle, and rb_flush_pending
    -- drops a deferred handle whose late completion already released it. The
    -- tag table at 0x009101c0 (media-state +0x80) is the release oracle: all
    -- zero means every identity the flow took was released exactly once.
    -- rbm_stale_close counts a close posted to a freed handle (the double
    -- release shape) and rbm_release_guard a release applied to an unowned
    -- slot; both must stay zero. The two entry sequences expose the first
    -- entry's status in rbm_seq_first and the second entry's as the result.
    gen9File = encodeFile 9 0x0001 ((name26 "", extractPayload) : replicate 6 (name26 "ALPHA", extractPayload))
    openTagZero = Spec "h-open-zero-tag-flush"
      "contract: an open whose dispatch-tag allocation returns zero while the worker still delivers a nonzero handle is deferred, not leaked - the first load answers 4 (RB_IO) with the tag still allocated, the second load's first open re-dispatches the close through the flush and releases it, the second load then publishes the file, and the tag table ends all-zero with no stale close and no unowned release"
      "rbm_seq_load_twice" []
      ((model (("rbm_open_tag_zero_at", 1) : fileA [])) { mCard = cardWith validA B.empty })
      (Expect "returned" 0
        [ ("kdata", fromIntegral (addr ks "rb_resident"), validA)
        , ("card", 0, cardWith validA B.empty)
        , ("media-state", 0x80, B.replicate 64 0)
        , ("model-data", fromIntegral (addr ms "rbm_seq_first" - 0x970000), u32le 4)
        , ("model-data", fromIntegral (addr ms "rbm_stale_close" - 0x970000), u32le 0)
        , ("model-data", fromIntegral (addr ms "rbm_release_guard" - 0x970000), u32le 0)
        ])
      ("rbm_seq_load_twice" : loadFns) bankStubs
    writeCloseOwned = Spec "h-write-close-tag-owned"
      "contract: the write-back's verify close (the nineteenth media call) completes with a success callback and a dispatched return yet leaves its tag allocated, so the kernel defers the handle; the write-back still publishes (the close status was success), and the next entry's first open flushes the deferred close through the worker, releases the tag and then loads the published file - first entry 0, second entry 0, tag table all-zero, no stale close"
      "rbm_seq_save_twice" [0, 0x009a0000]
      ((model (("rbm_close_keep_at", 19) : fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "returned" 0
        [ ("card", 0, cardWith gen9File saveFile)
        , ("kdata", fromIntegral (addr ks "rb_resident"), gen9File)
        , ("media-state", 0x80, B.replicate 64 0)
        , ("model-data", fromIntegral (addr ms "rbm_seq_first" - 0x970000), u32le 0)
        , ("model-data", fromIntegral (addr ms "rbm_stale_close" - 0x970000), u32le 0)
        , ("model-data", fromIntegral (addr ms "rbm_release_guard" - 0x970000), u32le 0)
        ])
      ("rbm_seq_save_twice" : saveFns) bankStubs
    writeCloseLate = Spec "h-write-close-late-release"
      "contract: the write-back's verify close returns without its callback while the worker's release ran, so the kernel defers the handle and answers 4 (RB_IO) without publishing; the next entry's flush finds the tag already free and drops the entry instead of re-dispatching a close (no stale close, no unowned release), then loads the written target and completes the second save - first entry 4, second entry 0, tag table all-zero"
      "rbm_seq_save_twice" [0, 0x009a0000]
      ((model (("rbm_close_late_at", 19) : fileA [])) { mCard = cardWith validA B.empty, mLive = extractLive })
      (Expect "returned" 0
        [ ("card", 0, cardWith gen9File saveFile)
        , ("kdata", fromIntegral (addr ks "rb_resident"), gen9File)
        , ("media-state", 0x80, B.replicate 64 0)
        , ("model-data", fromIntegral (addr ms "rbm_seq_first" - 0x970000), u32le 4)
        , ("model-data", fromIntegral (addr ms "rbm_stale_close" - 0x970000), u32le 0)
        , ("model-data", fromIntegral (addr ms "rbm_release_guard" - 0x970000), u32le 0)
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
      liveImg = mLive (spModel sp)
      caseImg = mCase (spModel sp)
      extras = mExtra (spModel sp)
      patchMode = overlay (B.replicate 0x42100 0) [(0x41424, u32le (mMode (spModel sp)))]
      extrasPatch = [(rid, a, if rid == "media-state" then patchMode else b) | (rid, a, b) <- extras]
      mediaImg' = mediaImg
      arts = [("kernel.bin", kbin), ("media.bin", mediaImg'), ("getter.bin", getter), ("setter.bin", setter), ("model-data.bin", B.take 0x1000 mdata'), ("tables.bin", tabImg), ("card.bin", cardImg), ("live.bin", liveImg)]
        ++ (if B.null caseImg then [] else [("case.bin", caseImg)])
        ++ [(rid ++ ".bin", b) | (rid, _, b) <- extrasPatch, not (B.null b)]
  forM_ (arts ++ [("mode.bin", modeCell)]) $ \(n, b) -> B.writeFile (dir </> n) b
  digests <- M.fromList <$> mapM (\(n, b) -> (,) n <$> sha256 b) (arts ++ [("mode.bin", modeCell)])
  let reg :: String -> Integer -> Int -> String -> Maybe String -> Int -> Int -> Value
      reg rid a sz perm art off len = object ["id" .= rid, "address" .= hex a, "size" .= (toInteger sz :: Integer), "permissions" .= (perm :: String),
        "initial" .= (if isJust art then object ["artifact" .= (takeWhile (/= '.') (fromJust art) :: String), "offset" .= (toInteger off :: Integer), "length" .= (toInteger len :: Integer)] else object ["zero" .= True])]
      regions =
        [ reg "ktext" 0x011dcca0 ktextLen "rx" (Just "kernel.bin") 0 ktextLen
        , reg "kdata" (0x011dcca0 + fromIntegral ktextLen) kdataLen "rw" (Just "kernel.bin") ktextLen kdataLen
        , reg "media" mediaLo (B.length mediaImg') "rx" (Just "media.bin") 0 (B.length mediaImg')
        , reg "model-data" 0x00970000 0x1000 "rw" (Just "model-data.bin") 0 0x1000
        , reg "live-state" 0x0096cc64 0x1c2 "rw" (Just "live.bin") 0 0x1c2
        , reg "card" 0x00980000 0x660 "rw" (Just "card.bin") 0 0x660
        , reg "casebuf" 0x009a0000 0x2000 "rw" (if B.null caseImg then Nothing else Just "case.bin") 0 (B.length caseImg) ]
        ++ [reg rid a (max 4 (B.length b)) (extraPerms rid) (if B.null b then Nothing else Just (rid ++ ".bin")) 0 (B.length b) | (rid, a, b) <- extrasPatch]
      extraPerms rid
        | rid `elem` ["chain", "dispatcher", "conv-tables", "prefix"] = "rx"
        | rid `elem` ["conv-ptrs", "modetab"] = "r"
        | otherwise = "rw"
      fns = [fnEntry n | n <- spFns sp, hasK n || hasM n || take 2 n == "0x"]
      fnEntry n
        | take 2 n == "0x" = case break (== '/') n of
            (a, '/' : s) -> object ["address" .= hex (hexI (drop 2 a)), "size" .= (read s :: Integer)]
            _ -> error ("raw function entry needs 0xADDR/SIZE: " ++ n)
        | otherwise = object ["address" .= hex (addrOf n), "size" .= (toInteger (sizeOf' n) :: Integer)]
      hasK n = n `elem` map fst (M.toList (case ks of Syms m -> m))
      hasM n = n `elem` map fst (M.toList (case ms of Syms m -> m))
      addrOf n = if hasK n then addr ks n else addr ms n
      sizeOf' n = if hasK n then sizeOf ks n else sizeOf ms n
      stubs = [let (a, i, o) = stubDef n (stubVal n) in object ["address" .= a, "inputs" .= bindObj i, "outputs" .= bindObj o, "side_effects" .= ("registers-only" :: String)] | n <- spStubs sp]
      stubVal n = fromMaybe 0 (lookup n (stubsOut n))
      stubsOut n = case n of
        "task" -> [("task", 3)]; "card" -> [("card", 0)]; "vol" -> [("vol", 0x43)]
        "live" -> [("live", 1)]; "store" -> [("store", 0)]; _ -> []
      bindObj bs = object [fromString k .= hex (toInteger v) | (k, v) <- bs]
      regs = object (["lr" .= hex 0x0071f890, "sp" .= hex 0x009a1ff0, "cpsr" .= hex 0x10]
        ++ [fromString ("r" ++ show i) .= hex (toInteger v) | (i, v) <- take 13 (zip [0 :: Int ..] (spArgs sp ++ repeat 0))])
      expected ph2 evs = object
        [ "status" .= eStatus (spExpect sp)
        , "registers" .= object ["r0" .= hex (fromIntegral (eR0 (spExpect sp)))]
        , "memory" .= (claimsJson ++ ph2)
        , "events" .= evs ]
      claimsJson = [object ["region" .= rid, "offset" .= (toInteger (off - regionBase rid) :: Integer), "hex" .= hexB bs] | (rid, off, bs) <- eClaims (spExpect sp)]
      regionBase "kdata" = fromIntegral ktextLen + 0x011dcca0
      regionBase _ = 0
      caseObj ph2 evs = object
        [ "schema_version" .= (1 :: Int), "id" .= spId sp, "origin" .= ("synthetic" :: String)
        , "provenance" .= spProv sp
        , "cpu" .= object ["architecture" .= ("arm" :: String), "model" .= ("cortex-r5f" :: String), "mode" .= ("arm" :: String), "endian" .= ("little" :: String)]
        , "artifacts" .= [object ["id" .= takeWhile (/= '.') n, "path" .= n, "size" .= (toInteger (B.length b) :: Integer), "sha256" .= M.findWithDefault "" n digests] | (n, b) <- arts ++ [("mode.bin", modeCell)]]
        , "entry" .= hex (addrOfEntry), "exit" .= hex 0x0071f890
        , "registers" .= regs
        , "regions" .= regions
        , "functions" .= fns, "stubs" .= stubs
        , "limits" .= object ["instructions" .= (1000000 :: Int), "wall_ms" .= (4000 :: Int)]
        , "expected" .= expected ph2 evs
        , "exclusions" .= (["OBSERVED RESIDUE (no behavior claim): stack region, kernel operational cells and the stub event sequence pinned from phase 1"] :: [String]) ]
      addrOfEntry = case spEntry sp of
        e | take 2 e == "0x" -> hexI e
        e -> if e `elem` M.keys (case ks of Syms m -> m) then addr ks e else addr ms e
  BL.writeFile (dir </> "case.json") (encode (caseObj [] (toJSON ([] :: [Value]))))
  (_c1, r1) <- runCase runner (dir </> "case.json")
  let observed = memMap r1
      evPin = fromMaybe (toJSON ([] :: [Value])) (parseMaybe (withObject "report" (\o -> o .: "runner" >>= withObject "runner" (.: "events"))) r1)
      pinned = ["stack", "kdata", "model-data", "media-state", "casebuf", "defaults", "mode-cell", "mirror", "live-state", "cells"]
      residue = concat [gapPins k v | (k, v) <- M.toList observed, k `elem` pinned, not (B.null v)]
      gapPins k v =
        let spans = sort [(fromInteger (toInteger off - toInteger (regionBase k)), B.length bs) | (rid, off, bs) <- eClaims (spExpect sp), rid == k]
            gaps [] = [(0, B.length v)]
            gaps ((s, l) : rest) =
              [(a, b) | (a, b) <- [(0, s)], b > a]
                ++ go (s + l) rest
            go e [] = [(e, B.length v) | B.length v > e]
            go e ((s, l) : rest) = [(e, s) | s > e] ++ go (s + l) rest
         in [ object ["region" .= k, "offset" .= (fromIntegral a :: Integer), "hex" .= hexB (B.take (b - a) (B.drop a v))]
            | (a, b) <- gaps spans, b > a
            ]
  BL.writeFile (dir </> "case.json") (encode (caseObj residue evPin))
  (_c2, r2) <- runCase runner (dir </> "case.json")
  let mism = fromMaybe [] (parseMaybe (withArray "m" (pure . mapMaybe strV . toList)) =<< lookupV "mismatches" r2)
      strV (String t) = Just (T.unpack t)
      strV _ = Nothing
      lookupV k v = parseMaybe (withObject "r" (.: k)) v
      status = fromMaybe "??" (parseMaybe (withObject "r" (\o -> o .: "status" :: Parser String)) r2)
      runnerStatus = fromMaybe "" (parseMaybe (withObject "r" (\o -> o .: "runner" >>= withObject "rn" (\rn -> rn .: "status" >>= withText "t" (pure . T.unpack)))) r2)
      missing = if runnerStatus == "unknown_call" then missingSym ks ms r2 else Nothing
  pure (object (["id" .= spId sp, "status" .= status, "mismatches" .= mism]
    ++ maybe [] (\n -> ["missing_fn" .= n]) missing))

-- the undeclared call target of an unknown_call failure, mapped back to its
-- symbol from the pinned kernel/model maps
missingSym :: Syms -> Syms -> Value -> Maybe String
missingSym ks ms r = do
  tr <- parseMaybe (withObject "rep" (\o -> o .: "runner" >>= withObject "rn" (\rn -> rn .: "trace"))) r
  lastE <- case reverse (case tr of Array v -> toList v; _ -> []) of (x : _) -> Just x; [] -> Nothing
  pc <- parseMaybe (withObject "e" (\o -> o .: "pc")) lastE >>= parseHexV
  ins <- parseMaybe (withObject "e" (\o -> o .: "instruction")) lastE >>= parseHexV
  if (ins .&. 0x0f000000) == 0x0b000000
    then do
      let imm = fromIntegral (ins .&. 0xffffff) :: Integer
          d = (if imm >= 0x800000 then imm - 0x1000000 else imm) * 4
          target = fromIntegral pc + 8 + d
      symAt ks ms target
    else if (ins .&. 0x0ffffff0) == 0x012fff30
      then do
        let rm = ins .&. 15
            regName = if rm == 14 then "lr" else "r" ++ show rm
        rv <- parseMaybe (withObject "rep" (\o -> o .: "runner" >>= withObject "rn" (\rn -> rn .: "registers" >>= withObject "regs" (\rs -> rs .: fromString regName)))) r >>= parseHexV
        symAt ks ms rv
      else Nothing

symAt :: Syms -> Syms -> Integer -> Maybe String
symAt ks ms a =
  let hit (Syms m) = listToMaybe [n | (n, (b, z)) <- sortOn (\(_, (b, z)) -> (max z 1, b)) (M.toList m), b <= a, a < b + max z 1]
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

memMap :: Value -> M.Map String B.ByteString
memMap r = M.fromList (fromMaybe [] (parseMaybe p =<< parseMaybe (withObject "rep" (\o -> o .: "runner" >>= withObject "rn" (\rn -> rn .: "memory"))) r))
  where
    p = withArray "mem" $ \a -> pure [(k, unhex v) | x <- toList a, Just (k, v) <- [row x]]
    row x = parseMaybe (withObject "r" $ \o -> do
      a <- o .: "address" :: Parser String
      b <- o .: "bytes" :: Parser String
      pure (regionName a, b)) x
    regionName a = case hexI a of
      v | v == 0x009b0000 -> "stack"
        | v == 0x011dcca0 -> "ktext"
        | v >= 0x011dcca0 -> "kdata"
        | v == 0x00970000 -> "model-data"
        | v == 0x00910140 -> "media-state"
        | v == 0x009a0000 -> "casebuf"
        | v == 0x007cb804 -> "defaults"
        | v == 0x0089ec00 -> "mirror"
        | v == 0x00956838 -> "cells"
        | v == 0x0096cc64 -> "live-state"
        | v == 0x00951564 -> "mode-cell"
      _ -> a

shaHex :: B.ByteString -> String
shaHex b = replicate (64 - length h) '0' ++ h
  where h = foldr (\w acc -> showHex w acc) "" (map fromIntegral (B.unpack (B.take 32 b)) :: [Integer])

findingsText :: [Spec] -> [Value] -> String
findingsText specs results = unlines (headerLines ++ resultLines)
  where
    headerLines =
      [ "recipe-bank kernel runtime cases: findings"
      , ""
      , "Blob and model hashes are recorded in results.json provenance per run;"
      , "re-measure on any menu/ change."
      , ""
      , "Unexercised consumers:"
      , "- on-camera staged-edit navigation, rename/editor, refusal rendering,"
      , "  probe activation/lifecycle, and global arm-level stock-page routing."
      , "- probe token contract (RB_UI_TOKEN_EDIT_HIDDEN_ROW 0x82, from the"
      , "  availability lane): state 3 (hidden) on RB_UI_CLASS_EDIT 0x277, the nine"
      , "  field hosts 0x267..0x26f and the seven submenu pages 0x270..0x276; every"
      , "  other token on those pages stays state 0 (submenus' verb rows 0x40..0x5b"
      , "  included). A page carrying a 0x82 placeholder draws and counts only its"
      , "  parameter/verb rows with 0x82 excluded from the count, scroll and cursor."
      , "  Recall rows: class 0x285, tokens 0x60..0x66, one rb_meta_op_state reload"
      , "  per visit after a cache drop; at-C cell 0x00951564 read live every row."
      , "- dialog expectations (Main's review additions): the per-item OK/CANCEL"
      , "  dialog raises on a row-routed edit (reachable once the item-session"
      , "  snapshot fix lands in the staged-edit module) and the Save Current/Reset"
      , "  confirmations raise from the submenu pages with the 0x82 placeholder"
      , "  excluded from count, scroll and cursor there."
      , ""
      , "Compiled-recipe consumer family (d3-*):"
      , "- the composed candidate's own id4-final and id5-final slices at their"
      , "  runtime addresses: the prefix-resident hook and its tables, the stock"
      , "  commit dispatcher with the patched case-0xfe site, the chain, the"
      , "  pinned conversion tables and pointer block, the consumer cell block"
      , "  and the settings mirror. The three render cases enter the dispatcher"
      , "  with the film rows the body's nine-shot render check selected"
      , "  (PROVIA 0, PRO Neg Std 5, PRO Neg Hi 6); the two recipe cases enter"
      , "  with the compiled rows (2 and 4), so the hook walks its row table and"
      , "  issues the eight nested property commits."
      , "- claims are the consumer cells the film and parameter arms write (the"
      , "  film cell 0x009568b8 and the seven parameter cells) and the record"
      , "  bytes the stock getter reads; each expectation is read from the"
      , "  candidate's own tables and prefix row tables at case-build time."
      , "- outside this family: the hook's instruction fetch from the prefix,"
      , "  the placement, the menu draw, the camera's rendering and any"
      , "  persistence."
      , ""
      , "Declared substitution limits of the model:"
      , "- the walked field table comes from the authoritative map"
      , "  (reports/custom-settings.fields.json, the same rows the storage kernel"
      , "  is generated from): 83 rows, the two halfword properties 0x1cb/0x1cc at"
      , "  +0x00/+0x02 first, 0x1a one bit at +0x27 bit 1, and the unbound mask"
      , "  (+0x27 bits 2-7) derived with the rows."
      , "- getter/setter remain synthetic row-model replacements with aliases"
      , "  0x1c6 at 0x0096ce0a and 0xe8 at 0x0096ce05 (the stock ordinary-branch"
      , "  bytes: setter arms 0x62d728 and 0x627b60 write those two addresses)."
      , "  The runner admits the stock setter's BFI forms; these cases do not"
      , "  execute the stock bytes."
      , "- the save and load paths run the stock chain itself: the snapshot walk"
      , "  reads every walked row through the stock getter 0x0061e110 (ordinary"
      , "  branch, the live block at 0x0096cc64), the code scan runs the stock"
      , "  walker 0x00631a00 through 0x0061d514, and the live resolver 0x006439e0"
      , "  is the stock function; the chain, dispatcher, conversion tables and"
      , "  pointer block are regions at their stock addresses and the load/store"
      , "  wrappers 0x0061d9ec/0x0061d8dc are one bx lr word each in place (their"
      , "  copy primitive 0x00722054 is refused by the runner). CLZ (the kernel's"
      , "  not-found test) is evaluated by the runner because the R5F model"
      , "  interrupts on it."
      , "- the walk fixture is derived from the getter's ordinary-branch arms"
      , "  (offset, mask, shift per row), not from the payload's record map: the"
      , "  layouts differ for some rows (0x1d, 0xfa, 0x1b1). Every slot of every"
      , "  fixture card carries valid codes, because the kernel validates all"
      , "  seven slots and row 0x137's table 0x776f34 admits no zero."
      , "- c-recall-real-bytes passes: the recall executes the stock write entry"
      , "  0x0061c91c, the commit dispatcher 0x00d4fc74 (all executed property"
      , "  arms) and the reader 0x0061c93c with the real code, returns 0 (every"
      , "  walked row read back its stored code through the kernel's assert"
      , "  pass), and its consumers are observed: the live halfword pair at"
      , "  0x0096cc74/0x0096cc76 carries the payload's +0x00/+0x02 codes and the"
      , "  film cell 0x009568b8 carries the film code's col2 (9). The executed"
      , "  arm closure was mapped evidence-first: the extended chain (0x2c000"
      , "  from 0x0061c91c), the cell blocks 0x0094f000..0x00952200 folded into"
      , "  media-state and 0x00956838..0x00957838, the menu data at"
      , "  0x00780000..0x007a1400, and 67 additional stock entries in the"
      , "  row-path set; two callees outside the mapped window (the journal"
      , "  forwarder 0x0072b858 and 0x0004357c) are register-only stubs and are"
      , "  the only substitutions on the arm path."
      , "- c-recall-real-consumers passes: the eight written rows' processing and"
      , "  menu consumers are observed effective - WB mode 0x00956864 = 3,"
      , "  sharpness 0x009568a0 = 1, film 0x009568b8 = 9, DR 0x009568bc = 1,"
      , "  colour 0x009568c0 = 3, noise reduction 0x0095699c = 2, highlight"
      , "  0x00956b40 = 3 and shadow 0x00956b44 = 4, each the col2 of the"
      , "  committed row whose col0 is the payload's stored code - with the"
      , "  recall still reporting 0."
      , "- c-recall-real-e9-preserved passes: the bound-not-walked 0xe9 row"
      , "  (payload +0x0a bits 5-7 = 0xe0) survives validation and the remembered"
      , "  payload path; the staged load/commit writes the new generation whose"
      , "  payload keeps those bits (no walk row carries them, normalisation does"
      , "  not mask them) and the resident equals the written file."
      , "- c-recall-real-refused passes: 0x1b1 stored as 0xb (row 9 of table"
      , "  0x77a54c, whose col2 is the 0x7ffffffe not-found sentinel) cannot hold"
      , "  its code - the arm's convert round trip writes 0 at the first apply"
      , "  and again at the re-apply - so the kernel answers 14"
      , "  (RB_APPLY_REFUSED); the failing field's live byte at 0x0096cce7 holds"
      , "  the arm's last write (0) and the fields other arms rewrite (0x1d,"
      , "  0xfa) still read back their codes. The old c-recall-apply modeled"
      , "  expectation is superseded and retired."
      , "- a-load-host-fixture passes against the refreshed canonical fixture"
      , "  (host-bank.bin sha256 029700fb19df5145c0339185f29dc755e05a03d501ffa3d5-"
      , "  9a01e26cd46fc9a8): the host codec encodes slot payloads over the body's"
      , "  defaults record, so the kernel loads the host file resident byte for"
      , "  byte. The earlier 5 (invalid) was the pre-fix empty-slot code 0, refused"
      , "  by table 0x776f34; the refresh was unblocked by the interop binary's"
      , "  missing -threaded flag (the non-threaded runtime deadlocked its bounded"
      , "  wait4), not by an encoder change."
      , "- media-task serialization is modeled by in-call completion; no scheduler runs."
      , "- the close worker's 0x71d66c media-evaluation call is modeled (no epoch"
      , "  move); the epoch moves only at the 0x71fa9c/0x71f7dc device-event shims."
      , "- the tag table at 0x009101c0 and the tag-to-index/index/low tables at"
      , "  0x00910140+ are observable model state; pre-dispatch close shapes release"
      , "  nothing and leave the handle for the next open to flush."
      , "- the media-identity races are modeled by the numbered-call fixtures:"
      , "  rbm_event_step bumps the epoch at its media call (the open/read/write/"
      , "  close/stat entries and the device-event shims), rbm_close_noop_at"
      , "  applies the state-1 no-op close at its call, and rbm_trip_step stops the"
      , "  run at a write boundary. h-save-media-move-in-scan,"
      , "  h-save-media-move-mid-write and h-write-close-deferred-flush use them"
      , "  against the write-back's scan, write and close."
      , "- the failure and ownership shapes of the media identity (the kernel's"
      , "  real request and release code: rb_open's flush before it posts,"
      , "  rb_close_quiet's defer-when-the-tag-is-owned, rb_flush_pending's"
      , "  drop-when-already-released) are driven by three more numbered-call"
      , "  fixtures: rbm_open_tag_zero(_at) returns a zero dispatch tag while the"
      , "  worker still delivers a nonzero callback handle, rbm_close_keep(_at)"
      , "  completes with a success callback while the worker release is skipped,"
      , "  and rbm_close_late_at runs the worker release with the callback lost to"
      , "  the caller's await. h-open-zero-tag-flush, h-write-close-tag-owned and"
      , "  h-write-close-late-release drive them through two entries of"
      , "  rb_load/rb_save_current; the tag table at 0x009101c0 (media-state"
      , "  +0x80) is the release oracle (all-zero), rbm_release_guard counts a"
      , "  release applied to an unowned slot, rbm_stale_close counts a close"
      , "  posted to a freed handle (the double-release shapes), and rbm_seq_first"
      , "  exposes the first entry's status. The model's hosts at the stock base"
      , "  addresses (0x0071c2b0 open, 0x0071be74 read, 0x0071bfe0 write,"
      , "  0x0071c134 close, 0x0071ce84 stat, 0x0071eff4 release) stand in for the"
      , "  media worker bytes: every request completes inside the call, so the"
      , "  stock media task, its queue and semaphore wait, the close worker chain"
      , "  (0x711768 -> 0x71f1d4 -> 0x71eff4) and the evaluation at 0x71d66c do not"
      , "  execute, and the handle pool is four slots against the stock 16. Named"
      , "  but unexecuted boundaries: allocation exhaustion, queue or semaphore"
      , "  wait failure, a true out-of-order late completion, and the raw"
      , "  completion mapping of every status except 0/3/0xe/6/8."
      ]
    resultLines = ["case " ++ spId sp ++ ": " ++ st | (sp, r) <- zip specs results, let st = fromMaybe "?" (parseMaybe (withObject "r" (\o -> o .: "status" :: Parser String)) r)]
