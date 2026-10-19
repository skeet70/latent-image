-- Classic Chrome RAM payload for the native cases (reports/classic-chrome-patch.md
-- section 8.2): a RAM-push manifest's publications applied to the RC0 decodes,
-- and the CC code and data extents of the linked runtime.
module Emulation.ClassicChromeRam
  ( CcEntry (..)
  , ccSymbol
  , Phase (..)
  , Publication (..)
  , RamPayload (..)
  , RamImage (..)
  , loadRamPayload
  , applyPublications
  , entryAddress
  , entryFunction
  , textRange
  , dataRange
  , runtimeSlice
  ) where

import Control.Monad
import Data.Bits
import qualified Data.ByteString as B
import Data.List (sort)
import qualified Data.Map.Strict as M
import Data.Maybe (mapMaybe)
import Data.Word
import FujiAF.Host
import FujiAF.JSON (decodeStrictIntegers)
import Numeric (readHex, showHex)

-- The entry symbols of menu/classic_chrome_ram.S.
data CcEntry
  = CcConvert
  | CcMarker
  | CcFilmMode
  | CcSaturation
  | CcRawCommit
  | CcRawHighlight
  | CcRawRequest
  | CcColour
  | CcChroma
  | CcTone
  deriving (Eq, Ord, Enum, Bounded, Show)

ccSymbol :: CcEntry -> String
ccSymbol e = case e of
  CcConvert -> "cc_ram_convert"
  CcMarker -> "cc_ram_marker"
  CcFilmMode -> "cc_ram_filmmode"
  CcSaturation -> "cc_ram_saturation"
  CcRawCommit -> "cc_ram_raw_commit"
  CcRawHighlight -> "cc_ram_raw_highlight"
  CcRawRequest -> "cc_ram_raw_request"
  CcColour -> "cc_ram_colour"
  CcChroma -> "cc_ram_chroma"
  CcTone -> "cc_ram_tone"

-- Local code symbols inside an entry's extent: cc_ram_chroma falls through into cc_ram_copy.
innerSymbols :: [String]
innerSymbols = ["cc_ram_copy"]

data BranchKind = Branch | BranchLink

-- Contract activation words A1 and A2: site, kind, target entry.
contractBranches :: [(Word32, BranchKind, CcEntry)]
contractBranches =
  [ (0x00738fe8, Branch, CcConvert)
  , (0x00736a74, Branch, CcConvert)
  , (0x00d7851c, Branch, CcMarker)
  , (0x00eab2e0, Branch, CcFilmMode)
  , (0x00eaac88, Branch, CcSaturation)
  , (0x00d461ec, Branch, CcRawCommit)
  , (0x00d5ada4, Branch, CcRawHighlight)
  , (0x012548a4, Branch, CcRawRequest)
  , (0x00d67284, BranchLink, CcColour)
  , (0x00d60f60, BranchLink, CcChroma)
  , (0x00d64ef4, BranchLink, CcChroma)
  , (0x00d677ac, BranchLink, CcTone)
  , (0x00d677bc, BranchLink, CcTone)
  ]

encodeBranch :: BranchKind -> Word32 -> Word32 -> Word32
encodeBranch kind at target = op .|. (((target - (at + 8)) `shiftR` 2) .&. 0xffffff)
  where
    op = case kind of Branch -> 0xea000000; BranchLink -> 0xeb000000

data Phase = Setup | Activation deriving (Eq, Show)

data Publication = Publication {pubIndex :: Integer, pubPhase :: Phase, pubAddress :: Word32, pubOld :: Word32, pubNew :: Word32}

data RamPayload = RamPayload
  { rpManifestPath :: FilePath
  , rpManifestSha :: String
  , rpRuntimePath :: FilePath
  , rpRuntimeSha :: String
  , rpBase :: Word32
  , rpRuntime :: B.ByteString
  , rpPublications :: [Publication]
  , rpEntries :: M.Map CcEntry (Word32, Word32)
  , rpData :: Word32
  }

data RamImage = RamImage {riName :: String, riBase :: Word32, riBytes :: B.ByteString}

h32 :: Word32 -> String
h32 v = let t = showHex v "" in "0x" ++ replicate (8 - length t) '0' ++ t

hexWord :: String -> String -> IO Word32
hexWord label text = case text of
  '0' : 'x' : digits | length digits == 8, [(v, "")] <- readHex digits -> pure v
  _ -> failH (label ++ ": 0x-prefixed eight-digit hex word required, got " ++ show text)

-- llvm-nm --defined-only [--print-size] lines: address [size] type name.
data Sym = Sym {symAddress :: Word32, symType :: String, symName :: String}

parseSyms :: String -> [Sym]
parseSyms = mapMaybe line . lines
  where
    line l = case words l of
      [a, t, nm] -> sym a t nm
      [a, _, t, nm] -> sym a t nm
      _ -> Nothing
    sym a t nm = case readHex a of
      [(v, "")] -> Just (Sym v t nm)
      _ -> Nothing

-- Reads the manifest, the runtime image it names by digest and the runtime's
-- symbol table. Refuses a publication that is unaligned, duplicated or inside
-- the allocation, and a contract branch word whose target is not its entry symbol.
loadRamPayload :: FilePath -> FilePath -> FilePath -> IO RamPayload
loadRamPayload manifestPath runtimePath symsPath = do
  raw <- B.readFile manifestPath
  manifestSha <- sha256 raw
  m <- either (failH . ((manifestPath ++ ": ") ++)) pure (decodeStrictIntegers raw)
  base <- hexWord "manifest base" (str (get m "base"))
  runtime <- B.readFile runtimePath
  runtimeSha <- sha256 runtime
  ensure (runtimeSha == str (get m "runtime_sha256")) (runtimePath ++ ": SHA-256 " ++ runtimeSha ++ " differs from the manifest's runtime_sha256")
  ensure (toInteger (B.length runtime) == num (get m "size")) (runtimePath ++ ": length differs from the manifest size")
  let size = fromIntegral (B.length runtime) :: Word32
      inside a = a >= base && a - base < size
  pubs <- forM (list (get m "publications")) $ \p -> do
    a <- hexWord "publication address" (str (get p "address"))
    old <- hexWord "publication old" (str (get p "old"))
    new <- hexWord "publication new" (str (get p "new"))
    phase <- case str (get p "phase") of
      "setup" -> pure Setup
      "activation" -> pure Activation
      other -> failH ("publication " ++ h32 a ++ ": phase " ++ show other)
    ensure (a .&. 3 == 0) ("publication " ++ h32 a ++ ": unaligned")
    ensure (not (inside a)) ("publication " ++ h32 a ++ ": inside the allocation")
    pure (Publication (num (get p "index")) phase a old new)
  let addresses = sort (map pubAddress pubs)
  ensure (and (zipWith (/=) addresses (drop 1 addresses))) (manifestPath ++ ": duplicate publication address")
  ensure (all ((== Setup) . pubPhase) (takeWhile ((== Setup) . pubPhase) pubs) && all ((== Activation) . pubPhase) (dropWhile ((== Setup) . pubPhase) pubs)) (manifestPath ++ ": a setup publication follows an activation publication")
  syms <- parseSyms <$> readFile symsPath
  let named nm = [symAddress x | x <- syms, symName x == nm]
      one nm = case named nm of
        [v] | inside v -> pure v
        _ -> failH (symsPath ++ ": " ++ nm ++ " is not one symbol inside the allocation")
      boundaries = sort [symAddress x | x <- syms, symType x `notElem` ["A", "a"], symName x `notElem` innerSymbols, inside (symAddress x)]
  starts <- forM [minBound .. maxBound] $ \e -> (,) e <$> one (ccSymbol e)
  dataStart <- one "cc_ram_data"
  ensure (named "cc_ram_data_end" == [dataStart + 100] && dataStart - base + 100 <= size) (symsPath ++ ": cc_ram_data_end is not cc_ram_data + 100 inside the allocation")
  let end a = case filter (> a) boundaries of
        nxt : _ -> nxt
        [] -> base + size
      entries = M.fromList [(e, (a, end a)) | (e, a) <- starts]
  forM_ contractBranches $ \(site, kind, e) -> case [p | p <- pubs, pubAddress p == site] of
    [p] -> ensure (pubNew p == encodeBranch kind site (fst (entries M.! e))) ("publication " ++ h32 site ++ ": new word " ++ h32 (pubNew p) ++ " is not a branch to " ++ ccSymbol e)
    _ -> failH (manifestPath ++ ": no publication at contract site " ++ h32 site)
  pure (RamPayload manifestPath manifestSha runtimePath runtimeSha base runtime pubs entries dataStart)

-- Applies the publications in manifest order; each must lie in exactly one
-- image and find its old word there.
applyPublications :: RamPayload -> [RamImage] -> IO [RamImage]
applyPublications rp images0 = foldM step images0 (rpPublications rp)
  where
    step images p = do
      let a = pubAddress p
          hit im = a >= riBase im && fromIntegral (a - riBase im) + 4 <= B.length (riBytes im)
      case [i | (i, im) <- zip [0 :: Int ..] images, hit im] of
        [i] -> do
          let im = images !! i
              off = fromIntegral (a - riBase im)
              cur = foldl' (\acc k -> acc .|. (fromIntegral (B.index (riBytes im) (off + k)) `shiftL` (8 * k))) 0 [0 .. 3]
              bytes = B.take off (riBytes im) <> B.pack [fromIntegral (pubNew p `shiftR` k) | k <- [0, 8, 16, 24]] <> B.drop (off + 4) (riBytes im)
          ensure (cur == pubOld p) ("publication " ++ show (pubIndex p) ++ " at " ++ h32 a ++ ": " ++ riName im ++ " holds " ++ h32 cur ++ ", manifest old " ++ h32 (pubOld p))
          pure (take i images ++ [im {riBytes = bytes}] ++ drop (i + 1) images)
        _ -> failH ("publication " ++ show (pubIndex p) ++ " at " ++ h32 a ++ " is not in exactly one image")

entryAddress :: RamPayload -> CcEntry -> Word32
entryAddress rp e = fst (rpEntries rp M.! e)

-- [entry, next non-inner symbol): the entry's code including inner labels.
entryFunction :: RamPayload -> CcEntry -> (Word32, Int)
entryFunction rp e = let (a, z) = rpEntries rp M.! e in (a, fromIntegral (z - a))

-- The span of every entry function, the runtime's CC code.
textRange :: RamPayload -> (Word32, Int)
textRange rp = (lo, fromIntegral (hi - lo))
  where
    lo = minimum (map fst (M.elems (rpEntries rp)))
    hi = maximum (map snd (M.elems (rpEntries rp)))

dataRange :: RamPayload -> (Word32, Int)
dataRange rp = (rpData rp, 100)

runtimeSlice :: RamPayload -> (Word32, Int) -> B.ByteString
runtimeSlice rp (a, len) = B.take len (B.drop (fromIntegral (a - rpBase rp)) (rpRuntime rp))
