-- SPDX-License-Identifier: AGPL-3.0-only
-- Index and boundary check over an extracted x70view `decomp/` tree (X-M1
-- 1.50). Definition recognition and symbol resolution follow x70view's
-- decomp/tools/cdefs.py, coverage_sources.py and build_symbols.py.
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (SomeException, displayException, try)
import Control.Monad (filterM, foldM, forM, forM_, unless)
import qualified Crypto.Hash.SHA256 as SHA256
import Data.Aeson (Value (..))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Builder as BB
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import qualified Data.ByteString.Unsafe as BU
import Data.Char (chr, isAlpha, isAlphaNum, isHexDigit)
import Data.IORef (modifyIORef', newIORef, readIORef, writeIORef)
import Data.List (sort, sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe)
import qualified Data.Scientific as Sci
import qualified Data.Set as S
import qualified Data.Text as T
import qualified Data.Vector as V
import qualified Data.Vector.Unboxed as U
import Data.Word (Word32)
import FujiAF.JSON (decodeStrictIntegers)
import Numeric (readHex, showHex)
import System.Directory (canonicalizePath, doesDirectoryExist, doesFileExist, listDirectory)
import System.Environment (getArgs)
import System.Exit (exitFailure, exitWith, ExitCode (..))
import System.FilePath (makeRelative, takeBaseName, takeDirectory, takeExtension, takeFileName, (</>))
import System.IO (hPutStrLn, stderr, stdout)

newtype Address = Address Word32 deriving (Eq, Ord)

hexAddress :: Address -> String
hexAddress (Address a) = "0x" ++ pad (showHex a "")
 where pad s = replicate (8 - length s) '0' ++ s

data BuildUnit = Kernel0 | Kernel1 | App deriving (Eq, Ord, Enum, Bounded)

unitName :: BuildUnit -> String
unitName Kernel0 = "kernel0"
unitName Kernel1 = "kernel1"
unitName App = "app"

unitSources, unitSymbols :: BuildUnit -> FilePath
unitSources Kernel0 = "firmwares/xm1-150/kernel0/src"
unitSources Kernel1 = "firmwares/xm1-150/kernel1/src"
unitSources App = "firmwares/xm1-150/app"
unitSymbols Kernel0 = "firmwares/xm1-150/symbols/kernel0.txt"
unitSymbols Kernel1 = "firmwares/xm1-150/symbols/kernel1.txt"
unitSymbols App = "firmwares/xm1-150/symbols/app"

-- function-ranges.json `unit` values each build unit links.
rangeUnits :: BuildUnit -> [T.Text]
rangeUnits Kernel0 = ["kernel0"]
rangeUnits Kernel1 = ["kernel1"]
rangeUnits App = ["main", "overlay44", "overlay46", "overlay47"]

data Range = Range
  { rUnit :: T.Text
  , rSection :: T.Text
  , rStart :: Address
  , rSize :: Word32
  , rSha :: T.Text
  , rStock :: T.Text
  , rName :: T.Text
  }

data Marker = Marker {mOffset :: Int, mReason :: B.ByteString}

data Def = Def
  { dName :: B.ByteString
  , dHead :: Int
  , dEnd :: Int
  , dAdjacent :: Maybe Marker
  }

data Parsed = Parsed
  { pDefs :: [Def]
  , pIncludes :: [B.ByteString]
  , pFlags :: [B.ByteString]
  , pNewlines :: U.Vector Int
  -- Names the build treats as NONMATCHING when this file is the translation unit.
  , pMarked :: M.Map B.ByteString Marker
  }

data Status = Exact | Nonmatching | Unranged | Unplaced deriving (Eq)

statusName :: Status -> String
statusName Exact = "exact"
statusName Nonmatching = "nonmatching"
statusName Unranged = "unranged"
statusName Unplaced = "unplaced"

data Entry = Entry
  { eUnit :: BuildUnit
  , eAddress :: Maybe Address
  , eRange :: Maybe Range
  , eName :: B.ByteString
  , eFile :: FilePath
  , eFirstLine :: Int
  , eHeadLine :: Int
  , eEndLine :: Int
  , eStatus :: Status
  , eMarker :: Maybe (FilePath, Int, B.ByteString)
  }

------------------------------------------------------------------------------
-- Lexical clean-up: comments and literals become spaces, newlines are kept.

data SpanKind = Comment | Literal deriving (Eq)

at :: B.ByteString -> Int -> Char
at t i
  | i >= 0 && i < B.length t = chr (fromIntegral (BU.unsafeIndex t i))
  | otherwise = '\0'

lexSpans :: B.ByteString -> [(Int, Int, SpanKind)]
lexSpans t = go 0
 where
  n = B.length t
  go i
    | i >= n = []
    | c == '/' && at t (i + 1) == '*' =
        let (before, rest) = B.breakSubstring (C.pack "*/") (B.drop (i + 2) t)
            e = if B.null rest then n else i + 2 + B.length before + 2
         in (i, e, Comment) : go e
    | c == '/' && at t (i + 1) == '/' =
        let e = maybe n (+ i) (C.elemIndex '\n' (B.drop i t)) in (i, e, Comment) : go e
    | c == '"' || c == '\'' = let e = literalEnd c (i + 1) in (i, e, Literal) : go e
    | otherwise = go (i + 1)
   where
    c = at t i
  literalEnd q j
    | j >= n = n
    | at t j == q = j + 1
    | at t j == '\n' = j
    | at t j == '\\' = literalEnd q (j + 2)
    | otherwise = literalEnd q (j + 1)

blankRanges :: B.ByteString -> [(Int, Int)] -> B.ByteString
blankRanges t rs = BL.toStrict (BB.toLazyByteString (go 0 rs))
 where
  go pos [] = BB.byteString (B.drop pos t)
  go pos ((s, e) : rest)
    | s < pos = go pos rest
    | otherwise =
        BB.byteString (B.take (s - pos) (B.drop pos t))
          <> BB.byteString (C.map (\ch -> if ch == '\n' then '\n' else ' ') (B.take (e - s) (B.drop s t)))
          <> go e rest

attributeRanges :: B.ByteString -> [(Int, Int)]
attributeRanges t = go 0
 where
  key = C.pack "__attribute__"
  go from =
    let (before, rest) = B.breakSubstring key (B.drop from t)
     in if B.null rest
          then []
          else
            let s = from + B.length before
                o = skipSpace t (s + B.length key)
             in if at t o == '(' then let e = balance (o + 1) (1 :: Int) in (s, e) : go e else go (s + B.length key)
  balance i depth
    | depth == 0 || i >= B.length t = i
    | at t i == '(' = balance (i + 1) (depth + 1)
    | at t i == ')' = balance (i + 1) (depth - 1)
    | otherwise = balance (i + 1) depth

isSpaceC, isWord, isIdentStart :: Char -> Bool
isSpaceC ch = ch `elem` [' ', '\t', '\n', '\r', '\f', '\v']
isWord ch = ch < '\128' && (isAlphaNum ch || ch == '_')
isIdentStart ch = ch < '\128' && (isAlpha ch || ch == '_')

skipSpace :: B.ByteString -> Int -> Int
skipSpace t i = if i < B.length t && isSpaceC (at t i) then skipSpace t (i + 1) else i

skipBlank :: B.ByteString -> Int -> Int
skipBlank t i = if at t i == ' ' || at t i == '\t' then skipBlank t (i + 1) else i

identEnd :: B.ByteString -> Int -> Int
identEnd t i = if isWord (at t i) then identEnd t (i + 1) else i

slice :: B.ByteString -> Int -> Int -> B.ByteString
slice t s e = B.take (e - s) (B.drop s t)

nextLineStart :: B.ByteString -> Int -> Maybe Int
nextLineStart t r
  | r >= B.length t = Nothing
  | r == 0 || at t (r - 1) == '\n' = Just r
  | otherwise = (\k -> r + k + 1) <$> C.elemIndex '\n' (B.drop r t)

------------------------------------------------------------------------------
-- Definition heads (cdefs._HEAD and _definition_brace).

notNames :: S.Set B.ByteString
notNames = S.fromList (map C.pack ["if", "while", "for", "switch", "return", "sizeof", "else", "do", "__attribute__"])

-- (name start, name end, opening parenthesis) of the first head starting at p.
matchHead :: B.ByteString -> Int -> Maybe (Int, Int, Int)
matchHead t p = scan (p + 1) False
 where
  n = B.length t
  scan i sawNewline
    | i >= n = Nothing
    | sawNewline && i - (p + 1) > 4000 = Nothing
    | otherwise = case candidate i of
        Just found -> Just found
        Nothing
          | at t i `elem` (";{}()=" :: String) -> Nothing
          | otherwise -> scan (i + 1) (sawNewline || at t i == '\n')
  candidate i
    | not (at t i == '(' || (isIdentStart (at t i) && not (isWord (at t (i - 1))))) = Nothing
    | otherwise =
        let j = pointerGroups i
         in if isIdentStart (at t j) && (j > i || not (isWord (at t (i - 1))))
              then
                let k = identEnd t j
                    w = skipSpace t k
                 in if at t w == '(' && at t (skipSpace t (w + 1)) /= '*' then Just (j, k, w) else Nothing
              else Nothing
  pointerGroups i
    | at t i == '(' =
        let a = skipSpace t (i + 1)
         in if at t a == '*' then pointerGroups (skipSpace t (stars a)) else i
    | otherwise = i
  stars i = if at t i == '*' then stars (i + 1) else i

parameterEnd :: B.ByteString -> Int -> Int -> Maybe Int
parameterEnd t start limit = go (start + 1) (1 :: Int)
 where
  go i depth
    | depth == 0 = Just i
    | i >= limit = Nothing
    | otherwise = case at t i of
        '(' -> go (i + 1) (depth + 1)
        ')' -> go (i + 1) (depth - 1)
        ch | ch `elem` (";{}" :: String) -> Nothing
        _ -> go (i + 1) depth

definitionBrace :: B.ByteString -> B.ByteString -> Int -> Maybe Int
definitionBrace t prefix paren = do
  let limit = min (B.length t) (paren + 1 + 4000)
  i0 <- parameterEnd t paren limit
  i1 <- foldM (\i _ -> do
                  let a = skipSpace t i
                  unless (a < limit && at t a == ')') Nothing
                  let b = skipSpace t (a + 1)
                  unless (b < limit && at t b == '(') Nothing
                  parameterEnd t b limit)
              i0 [1 .. C.count '(' prefix]
  let i = skipSpace t i1
  if at t i == '{' then Just i else Nothing

bodyEnd :: B.ByteString -> Int -> Int
bodyEnd t open = go open (0 :: Int)
 where
  go i depth
    | i >= B.length t = B.length t
    | at t i == '{' = go (i + 1) (depth + 1)
    | at t i == '}' = if depth == 1 then i + 1 else go (i + 1) (depth - 1)
    | otherwise = go (i + 1) depth

prefixWords :: B.ByteString -> [B.ByteString]
prefixWords = filter (not . B.null) . C.splitWith (not . isWord)

-- (name, head offset, opening brace) for every definition in cleaned text.
heads :: B.ByteString -> [(B.ByteString, Int, Int)]
heads t = go 0
 where
  go r = case nextLineStart t r of
    Nothing -> []
    Just ls ->
      let p = skipBlank t ls
          following = go (ls + 1)
       in if not (isIdentStart (at t p))
            then following
            else case matchHead t p of
              Nothing -> following
              Just (j, k, w) ->
                let name = slice t j k
                    prefix = slice t p j
                    resume = go (w + 1)
                 in if S.member name notNames || any (`elem` map C.pack ["return", "else", "case"]) (prefixWords prefix)
                      then resume
                      else case definitionBrace t prefix w of
                        Nothing -> resume
                        Just brace -> (name, p, brace) : resume

-- SM_STATE(a, b) { and SM_STEP(a) { from hostapd's state-machine macros.
stateMachineHeads :: B.ByteString -> [(B.ByteString, Int, Int)]
stateMachineHeads t = go 0
 where
  go r = case nextLineStart t r of
    Nothing -> []
    Just ls ->
      let p = skipSpace t ls
          rest = go (ls + 1)
       in case macro p of
            Just (name, brace) -> (name, p, brace) : rest
            Nothing -> rest
  word i = let e = identEnd t i in if e > i then Just (slice t i e, e) else Nothing
  lit s i = if C.pack s `B.isPrefixOf` B.drop i t then Just (i + length s) else Nothing
  close i = do
    a <- lit ")" (skipSpace t i)
    let b = skipSpace t a
    if at t b == '{' then Just b else Nothing
  macro p =
    ( do
        a <- lit "SM_STATE(" p
        (m, b) <- word (skipSpace t a)
        c <- lit "," (skipSpace t b)
        (s, d) <- word (skipSpace t c)
        brace <- close d
        Just (B.concat [C.pack "sm_", m, C.pack "_", s, C.pack "_Enter"], brace)
    )
      `orElse` ( do
                   a <- lit "SM_STEP(" p
                   (m, b) <- word (skipSpace t a)
                   brace <- close b
                   Just (B.concat [C.pack "sm_", m, C.pack "_Step"], brace)
               )
  orElse (Just x) _ = Just x
  orElse Nothing y = y

stripB :: B.ByteString -> B.ByteString
stripB = C.dropWhileEnd isSpaceC . C.dropWhile isSpaceC

lastIndexOf :: B.ByteString -> B.ByteString -> Maybe Int
lastIndexOf needle hay = go (B.length hay - B.length needle)
 where
  go i
    | i < 0 = Nothing
    | needle `B.isPrefixOf` B.drop i hay = Just i
    | otherwise = go (i - 1)

-- The NONMATCHING comment that ends immediately before a definition head.
markerBefore :: B.ByteString -> Int -> Maybe Marker
markerBefore raw headOffset = do
  let before = C.dropWhileEnd isSpaceC (B.take headOffset raw)
  unless (C.pack "*/" `B.isSuffixOf` before) Nothing
  open <- lastIndexOf (C.pack "/*") before
  let comment = stripB (B.drop (open + 2) (B.take (B.length before - 2) before))
      key = C.pack "NONMATCHING"
  unless (key `B.isPrefixOf` comment) Nothing
  Just (Marker open (stripB (C.dropWhile (== ':') (B.drop (B.length key) comment))))

-- build_step.compare_result's marker pattern
-- `/\*\s*NONMATCHING:\s*(.*?)\s*\*/\s*\n[^\n(]*?\b(\w+)\(`: the comment names the
-- first `identifier(` on the next nonblank line, a prototype as well as a definition.
buildMarkers :: B.ByteString -> M.Map B.ByteString Marker
buildMarkers raw = M.fromList (go 0)
 where
  n = B.length raw
  key = C.pack "NONMATCHING:"
  go from =
    let (before, rest) = B.breakSubstring (C.pack "/*") (B.drop from raw)
        s = from + B.length before
        a = skipSpace raw (s + 2)
        (body, close) = B.breakSubstring (C.pack "*/") (B.drop (a + B.length key) raw)
        e = a + B.length key + B.length body + 2
     in if B.null rest
          then []
          else
            if not (key `B.isPrefixOf` B.drop a raw) || B.null close
              then go (s + 2)
              else case lastNewline e (skipSpace raw e) >>= firstCall of
                Just (name, resume) -> (name, Marker s (stripB body)) : go resume
                Nothing -> go (s + 2)
  lastNewline lo hi = case [i | i <- [hi - 1, hi - 2 .. lo], at raw i == '\n'] of
    i : _ -> Just (i + 1)
    [] -> Nothing
  firstCall i
    | i >= n || at raw i == '\n' || at raw i == '(' = Nothing
    | isWord (at raw i) && not (isWord (at raw (i - 1))) =
        let k = identEnd raw i in if at raw k == '(' then Just (slice raw i k, k + 1) else firstCall k
    | otherwise = firstCall (i + 1)

includesOf :: B.ByteString -> [B.ByteString]
includesOf t = go 0
 where
  go r = case nextLineStart t r of
    Nothing -> []
    Just ls ->
      let rest = go (ls + 1)
          a = skipSpace t ls
       in if at t a /= '#'
            then rest
            else
              let b = skipSpace t (a + 1)
               in if not (C.pack "include" `B.isPrefixOf` B.drop b t)
                    then rest
                    else
                      let c = skipSpace t (b + 7)
                          close = case at t c of
                            '<' -> Just '>'
                            '"' -> Just '"'
                            _ -> Nothing
                       in case close of
                            Nothing -> rest
                            Just _ ->
                              let name = C.takeWhile (\ch -> ch /= '>' && ch /= '"' && ch /= '\n') (B.drop (c + 1) t)
                                  e = c + 1 + B.length name
                               in if not (B.null name) && at t e `elem` (">\"" :: String) then name : rest else rest

flagsOf :: B.ByteString -> [B.ByteString]
flagsOf raw = go (B.take 2000 raw)
 where
  key = C.pack "/* flags:"
  go t =
    let (_, rest) = B.breakSubstring key t
     in if B.null rest
          then []
          else
            let body = C.dropWhile isSpaceC (B.drop (B.length key) rest)
                line = C.takeWhile (/= '\n') body
                (value, close) = B.breakSubstring (C.pack "*/") line
             in if B.null close then go (B.drop (B.length key) rest) else value : go (B.drop (B.length key) rest)

parseSource :: B.ByteString -> Parsed
parseSource raw =
  let spans = lexSpans raw
      commentless = blankRanges raw [(s, e) | (s, e, Comment) <- spans]
      literalless = blankRanges raw [(s, e) | (s, e, _) <- spans]
      clean = blankRanges literalless (attributeRanges literalless)
      found = sortOn (\(_, h, _) -> h) (heads clean ++ stateMachineHeads commentless)
      defs = [Def name h (bodyEnd clean brace) (markerBefore raw h) | (name, h, brace) <- found]
   in Parsed defs (includesOf commentless) (flagsOf raw) (U.fromList (C.elemIndices '\n' raw)) (buildMarkers raw)

lineOf :: Parsed -> Int -> Int
lineOf parsed offset = 1 + search 0 (U.length nl)
 where
  nl = pNewlines parsed
  -- count of newline offsets below `offset`
  search lo hi
    | lo >= hi = lo
    | otherwise = let mid = (lo + hi) `div` 2 in if nl U.! mid < offset then search (mid + 1) hi else search lo mid

------------------------------------------------------------------------------
-- Symbols (build_symbols.read with every entry committed).

data SymEntry = SymEntry {sAddress :: Address, sName :: B.ByteString, sSeed :: Bool}

readSymbols :: FilePath -> IO (M.Map B.ByteString Address)
readSymbols path = do
  isDir <- doesDirectoryExist path
  isFile <- doesFileExist path
  files <- if isDir then map (path </>) . sort . filter ((== ".txt") . takeExtension) <$> listDirectory path else pure [path | isFile]
  entries <- concat <$> forM files (\file -> do
    text <- B.readFile file
    let seed = C.pack "00-" `B.isPrefixOf` C.pack (takeFileName file)
    pure [ SymEntry (Address value) name seed
         | line <- C.lines text
         , let ws = C.words (C.takeWhile (/= '#') line)
         , a : name : _ <- [ws]
         , all isHexDigit (C.unpack a)
         , (value, "") : _ <- [readHex (C.unpack a)]
         ])
  let deleted = S.fromList [sAddress e | e <- entries, sName e == C.pack "-", not (sSeed e)]
      kept = [e | e <- entries, sName e /= C.pack "-", not (sSeed e && S.member (sAddress e) deleted)]
      named = S.fromList [sAddress e | e <- kept, not (sSeed e)]
      active = [e | e <- kept, not (sSeed e && S.member (sAddress e) named)]
      byName = M.fromListWith (flip (++)) [(sName e, [e]) | e <- active]
      winner candidates = case (filter (not . sSeed) candidates, candidates) of
        (e : _, _) | length (S.fromList (map sAddress candidates)) > 1 -> e
        (_, e : _) -> e
        (_, []) -> error "symbol name without entries"
      symbols = M.map (sAddress . winner) byName
      seeds = M.fromList [(sName e, sAddress e) | e <- reverse kept, sSeed e, S.member (sAddress e) named]
  pure (M.union symbols (M.difference seeds symbols))

------------------------------------------------------------------------------
-- function-ranges.json

field :: T.Text -> KM.KeyMap Value -> Either String Value
field k o = maybe (Left ("function-ranges row lacks " ++ T.unpack k)) Right (KM.lookup (K.fromText k) o)

textField :: T.Text -> KM.KeyMap Value -> Either String T.Text
textField k o = field k o >>= \v -> case v of
  String s -> Right s
  _ -> Left ("function-ranges " ++ T.unpack k ++ " is not a string")

number :: T.Text -> KM.KeyMap Value -> Either String Word32
number k o = field k o >>= \v -> case v of
  Number s | Just i <- Sci.toBoundedInteger s -> Right i
  _ -> Left ("function-ranges " ++ T.unpack k ++ " is not a 32-bit integer")

readRanges :: FilePath -> IO [Range]
readRanges path = do
  bytes <- B.readFile path
  let rows = do
        document <- decodeStrictIntegers bytes
        functions <- case document of
          Object o | Just (Array a) <- KM.lookup (K.fromText (T.pack "functions")) o -> Right (V.toList a)
          _ -> Left "function-ranges.json has no functions array"
        forM functions $ \row -> case row of
          Object o ->
            Range <$> textField (T.pack "unit") o <*> textField (T.pack "section") o <*> (Address <$> number (T.pack "address") o)
              <*> number (T.pack "size") o <*> textField (T.pack "sha256") o <*> textField (T.pack "stock_sha256") o <*> textField (T.pack "name") o
          _ -> Left "function-ranges row is not an object"
  either fail pure rows

------------------------------------------------------------------------------
-- Index

listSources :: FilePath -> IO [FilePath]
listSources dir = do
  exists <- doesDirectoryExist dir
  names <- if exists then sort <$> listDirectory dir else pure []
  fmap concat . forM names $ \name -> do
    let path = dir </> name
    isDir <- doesDirectoryExist path
    if isDir then listSources path else pure [path | takeExtension name == ".c"]

stemOf :: FilePath -> B.ByteString
stemOf path = last (splitOn (C.pack "__") (C.pack (takeBaseName path)))
 where
  splitOn sep s = let (a, b) = B.breakSubstring sep s in if B.null b then [a] else a : splitOn sep (B.drop (B.length sep) b)

buildIndex :: FilePath -> [Range] -> IO [Entry]
buildIndex root ranges = do
  cache <- newIORef M.empty
  real <- newIORef M.empty
  let parseFile path = do
        known <- M.lookup path <$> readIORef cache
        case known of
          Just parsed -> pure parsed
          Nothing -> do
            parsed <- parseSource <$> B.readFile path
            modifyIORef' cache (M.insert path parsed)
            pure parsed
      realPath path = do
        known <- M.lookup path <$> readIORef real
        case known of
          Just r -> pure r
          Nothing -> do
            r <- canonicalizePath path
            modifyIORef' real (M.insert path r)
            pure r
  fmap concat . forM [minBound .. maxBound] $ \unit -> do
    symbols <- readSymbols (root </> unitSymbols unit)
    sources <- listSources (root </> unitSources unit)
    let unitRanges = M.fromListWith (flip (++)) [(rStart r, [r]) | r <- ranges, rUnit r `elem` rangeUnits unit]
        resolve stem name = maybe (M.lookup name symbols) Just (M.lookup (B.concat [stem, C.pack ":", name]) symbols)
    found <- fmap concat . forM sources $ \source -> do
      parsed <- parseFile source
      let stem = stemOf source
          search = [root </> "include", takeDirectory source]
            ++ [root </> C.unpack (B.drop 6 arg) | flags <- pFlags parsed, arg <- C.words flags, C.pack "-Isrc/" `B.isPrefixOf` arg]
          own = [(source, parsed, d, resolve stem (dName d), source, parsed) | d <- pDefs parsed]
      start <- realPath source
      included <- walk parseFile realPath search (S.singleton start) [(source, parsed)]
      let fromHeaders = [(header, hp, d, Just a, source, parsed) | (header, hp) <- included, d <- pDefs hp, Just a <- [resolve stem (dName d)]]
      pure (own ++ fromHeaders)
    writeIORef cache M.empty
    let entry (file, parsed, d, address, unitFile, unitParsed) =
          let range = case fromMaybe [] (address >>= (`M.lookup` unitRanges)) of
                r : _ -> Just r
                [] -> Nothing
              marker = M.lookup (dName d) (pMarked unitParsed)
              status
                | Nothing <- address = Unplaced
                | Just _ <- marker = Nonmatching
                | Nothing <- range = Unranged
                | otherwise = Exact
           in Entry unit address range (dName d) (makeRelative root file)
                (lineOf parsed (maybe (dHead d) mOffset (dAdjacent d))) (lineOf parsed (dHead d))
                (lineOf parsed (max (dHead d) (dEnd d - 1))) status
                ((\m -> (makeRelative root unitFile, lineOf unitParsed (mOffset m), mReason m)) <$> marker)
        marked e = eStatus e == Nonmatching
        keep new old = if marked old && not (marked new) then old else new
    pure (M.elems (M.fromListWith keep [((f, dHead d, a), entry x) | x@(f, _, d, a, _, _) <- found]))
 where
  walk parseFile realPath search visited0 start = go visited0 start []
   where
    go _ [] acc = pure (reverse acc)
    go visited ((parent, item) : pending) acc = do
      let parentDir = takeDirectory parent
      (visited', found) <- foldM (\(seen, out) name -> do
          let candidates = [d </> C.unpack name | d <- parentDir : search]
          existing <- filterM doesFileExist candidates
          case existing of
            [] -> pure (seen, out)
            child : _ -> do
              r <- realPath child
              if S.member r seen
                then pure (seen, out)
                else do
                  header <- parseFile child
                  pure (S.insert r seen, out ++ [(child, header)]))
        (visited, []) (pIncludes item)
      go visited' (pending ++ found) (reverse found ++ acc)

------------------------------------------------------------------------------
-- Output

tsv :: [String] -> BB.Builder
tsv cells = mconcat (zipWith (\i c -> (if i == (0 :: Int) then mempty else BB.char7 '\t') <> BB.stringUtf8 c) [0 ..] cells) <> BB.char7 '\n'

showAddress :: Maybe Address -> String
showAddress = maybe "-" hexAddress

decimal :: Maybe Address -> String
decimal = maybe "-" (\(Address a) -> show a)

cell :: String -> String
cell = map (\ch -> if ch == '\t' || ch == '\n' then ' ' else ch)

indexRows :: [Range] -> M.Map BuildUnit (M.Map B.ByteString Address) -> [Entry] -> BB.Builder
indexRows ranges symbolTables entries =
  mconcat
    [ tsv ["range", T.unpack (rUnit r), T.unpack (rSection r), hexAddress (rStart r), decimal (Just (rStart r)), show (rSize r), T.unpack (rSha r), T.unpack (rName r)]
    | r <- ranges
    ]
    <> mconcat
      [ tsv ["sym", unitName unit, hexAddress a, C.unpack key]
      | (unit, table) <- M.toList symbolTables
      , let starts = S.fromList [rStart r | r <- ranges, rUnit r `elem` rangeUnits unit]
      , (key, a) <- M.toList table
      , S.member a starts
      ]
    <> mconcat
      [ tsv
          [ "def", unitName (eUnit e), maybe "-" (T.unpack . rUnit) (eRange e), maybe "-" (T.unpack . rSection) (eRange e)
          , showAddress (eAddress e), decimal (eAddress e), C.unpack (eName e), "decomp/" ++ eFile e
          , show (eFirstLine e), show (eHeadLine e), show (eEndLine e), statusName (eStatus e)
          , maybe "-" (\(f, l, _) -> "decomp/" ++ f ++ ":" ++ show l) (eMarker e), maybe "" (\(_, _, r) -> cell (C.unpack r)) (eMarker e)
          ]
      | e <- entries
      ]

------------------------------------------------------------------------------
-- Boundary check

data Image = Image {iSection :: T.Text, iPath :: FilePath, iOffset :: Int, iSize :: Int, iBase :: Word32}

parseNumber :: String -> Maybe Integer
parseNumber ('0' : 'x' : digits) | not (null digits), all isHexDigit digits, (value, "") : _ <- readHex digits = Just value
parseNumber digits | not (null digits), all (`elem` ['0' .. '9']) digits = Just (read digits)
parseNumber _ = Nothing

-- SECTION=PATH:OFFSET:SIZE:BASE
parseImage :: String -> Either String Image
parseImage spec = case break (== '=') spec of
  (section, '=' : rest) -> case splitColons rest of
    [path, offset, size, base]
      | Just o <- parseNumber offset, Just s <- parseNumber size, Just b <- parseNumber base ->
          Right (Image (T.pack section) path (fromInteger o) (fromInteger s) (fromInteger b))
    _ -> Left ("bad image spec " ++ spec)
  _ -> Left ("bad image spec " ++ spec)
 where
  splitColons s = case break (== ':') s of
    (a, ':' : b) -> a : splitColons b
    (a, _) -> [a]

hexText :: B.ByteString -> String
hexText = concatMap (\w -> let s = showHex w "" in if length s == 1 then '0' : s else s) . B.unpack

boundary :: [Range] -> [Image] -> IO Bool
boundary ranges images = do
  loaded <- forM images $ \image -> do
    bytes <- B.readFile (iPath image)
    let section = B.take (iSize image) (B.drop (iOffset image) bytes)
    unless (B.length section == iSize image) (fail ("image " ++ iPath image ++ " is shorter than its section"))
    pure (iSection image, (image, section))
  let table = M.fromList loaded
      sectionRows = [(s, T.unpack (rStock r)) | r <- ranges, let s = rSection r]
      stockOf s = S.toList (S.fromList [h | (s', h) <- sectionRows, s' == s])
      check r = case M.lookup (rSection r) table of
        Nothing -> Left "no image"
        Just (image, bytes) ->
          let Address a = rStart r
              off = fromIntegral a - fromIntegral (iBase image) :: Integer
           in if off < 0 || off + fromIntegral (rSize r) > fromIntegral (B.length bytes)
                then Left "outside image"
                else Right (hexText (SHA256.hash (B.take (fromIntegral (rSize r)) (B.drop (fromInteger off) bytes))))
      results = [(r, check r) | r <- ranges]
      matched = [r | (r, Right h) <- results, h == T.unpack (rSha r)]
      mismatched = [(r, h) | (r, Right h) <- results, h /= T.unpack (rSha r)]
      unmapped = [(r, why) | (r, Left why) <- results]
  forM_ (M.toList table) $ \(s, (image, bytes)) -> do
    let h = hexText (SHA256.hash bytes)
    putStrLn (unwords ["section", T.unpack s, iPath image, "sha256", h, if [h] == stockOf s then "matches stock_sha256" else "DIFFERS from stock_sha256 " ++ unwords (stockOf s)])
  forM_ (S.toList (S.fromList (map rSection ranges))) $ \s -> do
    let inSection = filter ((== s) . rSection)
    putStrLn (unwords ["rows", T.unpack s, show (length (inSection ranges)), "matched", show (length (inSection matched))
                      , "mismatched", show (length (filter ((== s) . rSection . fst) mismatched)), "unmapped", show (length (filter ((== s) . rSection . fst) unmapped))])
  putStrLn (unwords ["total", show (length ranges), "matched", show (length matched), "mismatched", show (length mismatched), "unmapped", show (length unmapped)])
  forM_ mismatched $ \(r, h) -> putStrLn (unwords ["MISMATCH", T.unpack (rSection r), hexAddress (rStart r), show (rSize r), T.unpack (rName r), "ranges", T.unpack (rSha r), "ours", h])
  forM_ unmapped $ \(r, why) -> putStrLn (unwords ["UNMAPPED", T.unpack (rSection r), hexAddress (rStart r), show (rSize r), T.unpack (rName r), why])
  pure (null mismatched && null unmapped)

------------------------------------------------------------------------------

usage :: String
usage =
  unlines
    [ "usage: xm1-decomp-index index DECOMP_ROOT"
    , "       xm1-decomp-index boundary DECOMP_ROOT SECTION=PATH:OFFSET:SIZE:BASE ..."
    , "DECOMP_ROOT is an extracted x70view decomp/ directory holding firmwares/xm1-150 and include."
    ]

rangesPath :: FilePath -> FilePath
rangesPath root = root </> "firmwares/xm1-150/function-ranges.json"

main :: IO ()
main = do
  result <- try $ do
    args <- getArgs
    case args of
      ["index", root] -> do
        ranges <- readRanges (rangesPath root)
        symbols <- M.fromList <$> forM [minBound .. maxBound] (\u -> (,) u <$> readSymbols (root </> unitSymbols u))
        entries <- buildIndex root ranges
        BB.hPutBuilder stdout (indexRows ranges symbols entries)
        pure True
      "boundary" : root : specs@(_ : _) -> do
        ranges <- readRanges (rangesPath root)
        images <- either fail pure (mapM parseImage specs)
        boundary ranges images
      _ -> hPutStrLn stderr usage >> exitWith (ExitFailure 2)
  case result of
    Right True -> pure ()
    Right False -> exitWith (ExitFailure 1)
    Left err -> hPutStrLn stderr (displayException (err :: SomeException)) >> exitFailure
