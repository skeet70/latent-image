{-# LANGUAGE OverloadedStrings #-}
-- MATCHING-C-01: compile frozen x70view C translation units with the pinned
-- EXR II GCC, link each function section at its stock address and compare the
-- linked bytes with the owner's decoded X-M1 1.50 images.
module Main (main) where

import Control.Monad (forM, forM_, unless, when)
import qualified Crypto.Hash.SHA256 as SHA256
import Data.Aeson ((.:), (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Aeson.Types as A
import Data.Bits (shiftR)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.Char (isHexDigit, isSpace)
import Data.List (dropWhileEnd, isPrefixOf, isSuffixOf, nub, sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, listToMaybe, mapMaybe)
import Data.Word (Word32)
import FujiAF.Matching.Compare
import Numeric (readHex, showHex)
import System.Directory
  ( canonicalizePath, createDirectory, createDirectoryIfMissing, createFileLink
  , doesPathExist, listDirectory )
import System.Environment (getArgs, lookupEnv)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath (takeBaseName, takeDirectory, takeFileName, (</>))
import System.IO (hPutStrLn, stderr)
import System.Process (CreateProcess (..), proc, readCreateProcessWithExitCode)

-- Identifiers ----------------------------------------------------------------

data Region = LoaderA | LoaderB | Image4 | Image5 | Plain
  deriving (Eq, Ord, Show, Enum, Bounded)

regionName :: Region -> String
regionName r = case r of
  LoaderA -> "loader-a"
  LoaderB -> "loader-b"
  Image4 -> "image-4"
  Image5 -> "image-5"
  Plain -> "plain"

parseRegion :: String -> Either String Region
parseRegion t = maybe (Left ("unknown region " ++ t)) Right
  (lookup t [(regionName r, r) | r <- [minBound .. maxBound]])

-- x70view link units: the application unit spans images 4/5 and the plain
-- overlays; each loader is its own kernel unit.
data LinkUnit = App | Kernel0 | Kernel1 deriving (Eq, Show)

parseUnit :: String -> Either String LinkUnit
parseUnit t
  | t == "main" || "overlay4" `isPrefixOf` t = Right App
  | t == "kernel0" = Right Kernel0
  | t == "kernel1" = Right Kernel1
  | otherwise = Left ("unknown x70view unit " ++ t)

-- x70view decomp/tools/build_target.py, firmware xm1-150.
unitDefines :: LinkUnit -> [String]
unitDefines u = case u of
  App -> ["-Isrc/firmwares/xm1-150/app"]
  Kernel0 -> ["-DKERNEL_CORE=0", "-Isrc/firmwares/xm1-150/kernel0/src"]
  Kernel1 -> ["-DKERNEL_CORE=1", "-Isrc/firmwares/xm1-150/kernel1/src"]

data Compiler = Gcc464 | Gcc447 deriving (Eq, Ord, Show)

compilerName :: Compiler -> String
compilerName c = case c of Gcc464 -> "4.6.4"; Gcc447 -> "4.4.7"

parseCompiler :: String -> Either String Compiler
parseCompiler t = case t of
  "4.6.4" -> Right Gcc464
  "4.4.7" -> Right Gcc447
  _ -> Left ("no pinned GCC " ++ t)

newtype Address = Address Word32 deriving (Eq, Ord)

instance Show Address where
  show (Address a) = let h = showHex a "" in "0x" ++ replicate (8 - length h) '0' ++ h

parseAddress :: String -> Either String Address
parseAddress t = case readHex (if "0x" `isPrefixOf` t then drop 2 t else t) of
  [(v, "")] | v <= (0xffffffff :: Integer) -> Right (Address (fromInteger v))
  _ -> Left ("bad address " ++ t)

offsetBy :: Address -> Word32 -> Address
offsetBy (Address a) k = Address (a + k)

newtype Digest = Digest String deriving (Eq, Show)

digest :: B.ByteString -> Digest
digest = Digest . concatMap byte . B.unpack . SHA256.hash
  where byte w = let h = showHex w "" in if length h == 1 then '0' : h else h

digestText :: Digest -> String
digestText (Digest d) = d

-- Frozen sample --------------------------------------------------------------

data RegionImage = RegionImage { riBase :: Address, riDigest :: Digest }

data Frozen = Frozen
  { fRegion :: Region
  , fUnit :: LinkUnit
  , fAddress :: Address
  , fSize :: Int
  , fName :: String
  , fStockDigest :: Digest
  , fSource :: FilePath
  , fSourceDigest :: Digest
  }

data Manifest = Manifest
  { mRevision :: String
  , mRegions :: M.Map Region RegionImage
  , mFunctions :: [Frozen]
  }

orFail :: Either String a -> A.Parser a
orFail = either fail pure

instance A.FromJSON Manifest where
  parseJSON = A.withObject "manifest" $ \o -> do
    upstream <- o .: "upstream"
    revision <- upstream .: "revision"
    regionObject <- o .: "regions"
    regions <- forM (KM.toList regionObject) $ \(k, v) -> do
      r <- orFail (parseRegion (K.toString k))
      image <- A.withObject "region" (\x -> RegionImage
        <$> (x .: "base" >>= orFail . parseAddress)
        <*> (Digest <$> x .: "sha256")) v
      pure (r, image)
    functions <- o .: "functions" >>= mapM (A.withObject "function" $ \x -> Frozen
      <$> (x .: "region" >>= orFail . parseRegion)
      <*> (x .: "unit" >>= orFail . parseUnit)
      <*> (x .: "address" >>= orFail . parseAddress)
      <*> x .: "size"
      <*> x .: "name"
      <*> (Digest <$> x .: "stock_sha256")
      <*> x .: "upstream_source"
      <*> (Digest <$> x .: "upstream_source_sha256"))
    pure (Manifest revision (M.fromList regions) functions)

-- Command line ---------------------------------------------------------------

data Options = Options
  { oManifest :: FilePath
  , oX70view :: FilePath
  , oOut :: FilePath
  , oGcc :: M.Map Compiler FilePath
  , oLd :: FilePath
  , oImages :: M.Map Region FilePath
  -- Replaces every source's annotated GCC release.
  , oForce :: Maybe Compiler
  }

usage :: String
usage = "usage: matching-pilot --manifest JSON --x70view DIR --out NEW_DIR --gcc VERSION=PATH... --ld TARGET-ld --image REGION=PATH... [--force-gcc VERSION]"

parseOptions :: [String] -> Either String Options
parseOptions = go (Options "" "" "" M.empty "" M.empty Nothing)
  where
    go o [] = do
      when (any null [oManifest o, oX70view o, oOut o, oLd o]) (Left usage)
      pure o
    go o ("--manifest" : v : rest) = go o {oManifest = v} rest
    go o ("--x70view" : v : rest) = go o {oX70view = v} rest
    go o ("--out" : v : rest) = go o {oOut = v} rest
    go o ("--ld" : v : rest) = go o {oLd = v} rest
    go o ("--force-gcc" : v : rest) = do
      c <- parseCompiler v
      go o {oForce = Just c} rest
    go o ("--gcc" : v : rest) = do
      let (k, p) = break (== '=') v
      c <- parseCompiler k
      go o {oGcc = M.insert c (drop 1 p) (oGcc o)} rest
    go o ("--image" : v : rest) = do
      let (k, p) = break (== '=') v
      r <- parseRegion k
      go o {oImages = M.insert r (drop 1 p) (oImages o)} rest
    go _ (a : _) = Left ("unexpected argument " ++ a ++ "\n" ++ usage)

-- Processes ------------------------------------------------------------------

data Tools = Tools { tGcc :: M.Map Compiler FilePath, tPrefix :: FilePath }

binutil :: Tools -> String -> FilePath
binutil t name = tPrefix t ++ name

runEither :: FilePath -> FilePath -> [String] -> IO (Either String String)
runEither cwd exe args = do
  (code, out, err) <- readCreateProcessWithExitCode (proc exe args) {cwd = Just cwd} ""
  pure $ case code of
    ExitSuccess -> Right out
    ExitFailure _ -> Left (unwords (exe : args) ++ "\n" ++ err)

runOk :: FilePath -> FilePath -> [String] -> IO String
runOk cwd exe args = runEither cwd exe args >>= either die pure

die :: String -> IO a
die message = hPutStrLn stderr message >> exitFailure

trim :: String -> String
trim = dropWhileEnd isSpace . dropWhile isSpace

-- x70view compilation settings -----------------------------------------------

-- x70view decomp/tools/build_toolchain.py COMMON with the exr2 CPU flags.
commonFlags :: [String]
commonFlags =
  [ "-marm", "-march=armv7-a", "-mfloat-abi=soft", "-fno-common", "-ffunction-sections"
  , "-fno-asynchronous-unwind-tables", "-Isrc/include", "-Isrc/generated" ]

-- A first-line "/* flags: ... */" replaces the default optimization flags; a
-- "/* compiler: V */" within the first three lines selects the release.
sourceSettings :: B.ByteString -> Either String (Compiler, [String])
sourceSettings text = do
  let ls = map BC.unpack (take 3 (BC.lines text))
      flags = case ls of
        (first : _) | Just body <- comment "flags:" first -> words body
        _ -> ["-O1", "-fno-omit-frame-pointer"]
  compiler <- maybe (Right Gcc464) (parseCompiler . trim) (listToMaybe (mapMaybe (comment "compiler:") ls))
  pure (compiler, flags)
  where
    comment key line =
      let t = trim line
          inner = trim (drop 2 (take (length t - 2) t))
      in if "/*" `isPrefixOf` t && "*/" `isSuffixOf` t && key `isPrefixOf` inner
           then Just (drop (length key) inner)
           else Nothing

-- x70view symbol bindings -----------------------------------------------------

-- Names with more than one bound address are absent and resolve to nothing.
type Bindings = M.Map String Address

symbolFiles :: FilePath -> LinkUnit -> IO [FilePath]
symbolFiles x70 u = case u of
  App -> do
    let dir = x70 </> "decomp/firmwares/xm1-150/symbols/app"
    names <- listDirectory dir
    pure [dir </> f | f <- names, ".txt" `isSuffixOf` f]
  Kernel0 -> pure [x70 </> "decomp/firmwares/xm1-150/symbols/kernel0.txt"]
  Kernel1 -> pure [x70 </> "decomp/firmwares/xm1-150/symbols/kernel1.txt"]

-- "ADDRESS NAME" rows plus "@stem:section" placements from native-bindings.json.
loadBindings :: FilePath -> LinkUnit -> IO Bindings
loadBindings x70 u = do
  files <- symbolFiles x70 u
  rows <- concat <$> forM files (\f -> do
    text <- B.readFile f
    pure [ (name, a) | line <- BC.lines text
         , (h : name : _) <- [words (BC.unpack line)]
         , all isHexDigit h, Right a <- [parseAddress h] ])
  sections <- sectionBindings x70
  let grouped = M.fromListWith (++) [(name, [a]) | (name, a) <- rows ++ sections]
  pure (M.mapMaybe (\as -> case nub as of [a] -> Just a; _ -> Nothing) grouped)

sectionBindings :: FilePath -> IO [(String, Address)]
sectionBindings x70 = do
  bytes <- BL.readFile (x70 </> "decomp/firmwares/xm1-150/native-bindings.json")
  value <- either die pure (A.eitherDecode bytes)
  either die pure $ flip A.parseEither value $ A.withObject "bindings" $ \o -> do
    rows <- o .: "section_bindings"
    forM rows $ A.withObject "section" $ \r -> do
      b <- r .: "binding"
      (,) <$> b .: "name" <*> (Address <$> b .: "target_value")

-- ELF object facts from binutils readelf --------------------------------------

data Section = Section { secIndex :: Int, secName :: String, secSize :: Int }

data Symbol = Symbol
  { symIndex :: Int, symValue :: Word32, symIsSection :: Bool
  , symShndx :: Maybe Int, symName :: String }

data Reloc = Reloc { relOffset :: Int, relKind :: RelocKind, relSymbol :: Int }

readHexInt :: String -> Maybe Int
readHexInt t = case readHex t of [(v, "")] -> Just v; _ -> Nothing

readInt :: String -> Maybe Int
readInt t = case reads t of [(v, "")] -> Just v; _ -> Nothing

parseSections :: String -> [Section]
parseSections out =
  [ Section i name size
  | line <- lines out
  , let (pre, rest) = break (== ']') line
  , Just i <- [readInt (trim (drop 1 (dropWhile (/= '[') pre)))]
  , (name : _ : _ : _ : sz : _) <- [words (drop 1 rest)]
  , Just size <- [readHexInt sz] ]

parseSymbols :: String -> [Symbol]
parseSymbols out =
  [ Symbol i (fromIntegral v) (typ == "SECTION") (readInt ndx) (unwords nameWords)
  | line <- lines out
  , (num : val : _size : typ : _bind : _vis : ndx : nameWords) <- [words line]
  , ":" `isSuffixOf` num
  , Just i <- [readInt (init num)]
  , Just v <- [readHexInt val] ]

parseRelocs :: String -> String -> [Reloc]
parseRelocs section out = go (lines out) False
  where
    header = "Relocation section '.rel" ++ section ++ "'"
    go [] _ = []
    go (l : ls) inside
      | "Relocation section" `isPrefixOf` l = go ls (header `isPrefixOf` l)
      | inside, (off : info : typ : _) <- words l, Just o <- readHexInt off, Just i <- readHexInt info =
          Reloc o (relocKind typ) (i `shiftR` 8) : go ls inside
      | otherwise = go ls inside

-- Mapping symbols ($a, $d) split code from literal-pool bytes.
literalBytes :: Section -> [Symbol] -> Int
literalBytes text syms =
  let marks = sortOn fst [ (fromIntegral (symValue s) :: Int, symName s) | s <- syms
                         , symShndx s == Just (secIndex text), symName s `elem` ["$a", "$d"] ]
      spans = zip marks (map fst (drop 1 marks) ++ [secSize text])
  in sum [ end - start | ((start, kind), end) <- spans, kind == "$d" ]

-- Relocation targets ------------------------------------------------------------

-- Name, defining section of this object, that section's placement, and the
-- symbol's address. Undefined symbols resolve through PROVIDE; sections of
-- this object that a relocation reaches are placed at their bound address.
data Target = Target String (Maybe String) (Maybe Address) (Maybe Address)

targetName :: Target -> String
targetName (Target name _ _ _) = name

targetAddress :: Target -> Maybe Address
targetAddress (Target _ _ _ a) = a

resolveTarget :: Bindings -> String -> String -> Address -> (Int -> String) -> Symbol -> Target
resolveTarget bindings stem textSection entry sectionName s =
  case (symIsSection s, symShndx s) of
    (True, Just ndx) ->
      let sec = sectionName ndx
          at = sectionAt sec
      in Target sec (Just sec) at at
    (False, Just ndx) ->
      let sec = sectionName ndx
          placed = case named of
            Just (Address a) | sec /= textSection -> Just (Address (a - symValue s))
            _ -> sectionAt sec
      in Target (symName s) (Just sec) placed ((`offsetBy` symValue s) <$> placed)
    _ -> Target (symName s) Nothing Nothing named
  where
    named = listToMaybe (mapMaybe (`M.lookup` bindings) [stem ++ ":" ++ symName s, symName s])
    sectionAt sec
      | sec == textSection = Just entry
      | otherwise = M.lookup ("@" ++ stem ++ ":" ++ sec) bindings

-- Per-function pilot -------------------------------------------------------------

data Outcome
  = Compared ByteComparison
  | Unresolved [String]
  | CompileFailed String
  | LinkFailed String

data OutcomeClass = ClassExact | ClassNormalized | ClassFailure deriving (Eq)

outcomeClass :: Outcome -> OutcomeClass
outcomeClass o = case o of
  Compared Exact -> ClassExact
  Compared (Normalized _) -> ClassNormalized
  _ -> ClassFailure

classText :: OutcomeClass -> String
classText c = case c of
  ClassExact -> "exact"
  ClassNormalized -> "normalized"
  ClassFailure -> "failure"

outcomeDetail :: Outcome -> String
outcomeDetail o = case o of
  Compared c -> comparisonDetail c
  Unresolved syms -> "no unique x70view binding for " ++ unwords syms
  CompileFailed e -> "compile failed: " ++ take 2000 e
  LinkFailed e -> "link failed: " ++ take 2000 e

-- Relocation offset, kind, target symbol and resolved address.
data Placement = Placement Int RelocKind String Address

data Result = Result
  { rFunction :: Frozen
  , rCompiler :: Compiler
  , rFlags :: [String]
  , rDependencies :: [(FilePath, Digest)]
  , rEmitted :: Int
  , rLiteral :: Int
  , rPlacements :: [Placement]
  , rEmittedDigest :: Maybe Digest
  , rOutcome :: Outcome
  }

pilot :: Tools -> Maybe Compiler -> FilePath -> M.Map Region (Address, B.ByteString) -> FilePath -> Frozen -> IO Result
pilot tools force x70 images work f = do
  let stem = takeBaseName (fSource f)
      object = fName f ++ ".o"
      depfile = fName f ++ ".d"
      textSection = ".text." ++ fName f
      sourceInTree = "src" </> drop (length ("decomp/" :: String)) (fSource f)
  unless ("decomp/" `isPrefixOf` fSource f) (die (fSource f ++ ": upstream source must lie under decomp/"))
  source <- B.readFile (x70 </> fSource f)
  when (digest source /= fSourceDigest f) (die (fSource f ++ ": source digest differs from the frozen sample"))
  (regionBase, image) <- maybe (die ("no image for " ++ regionName (fRegion f))) pure (M.lookup (fRegion f) images)
  let Address entry = fAddress f
      Address base = regionBase
      offset = fromIntegral (entry - base)
      body = B.take (fSize f) (B.drop offset image)
  when (entry < base || digest body /= fStockDigest f)
    (die (fName f ++ ": stock body digest differs from the frozen sample"))
  (annotated, fileFlags) <- either die pure (sourceSettings source)
  let compiler = fromMaybe annotated force
  gcc <- maybe (die ("missing --gcc " ++ compilerName compiler)) pure (M.lookup compiler (tGcc tools))
  let flags = commonFlags ++ fileFlags ++ unitDefines (fUnit f) ++ ["-fdata-sections", "-iquote", "api-stub"]
      initial = Result f compiler flags
  compiled <- runEither work gcc (flags ++ ["-MD", "-MF", depfile, "-c", sourceInTree, "-o", object])
  case compiled of
    Left e -> pure (initial [] 0 0 [] Nothing (CompileFailed e))
    Right _ -> do
      deps <- dependencies work depfile
      sections <- parseSections <$> runOk work (binutil tools "readelf") ["-W", "-S", object]
      symbols <- parseSymbols <$> runOk work (binutil tools "readelf") ["-W", "-s", object]
      relocs <- parseRelocs textSection <$> runOk work (binutil tools "readelf") ["-W", "-r", object]
      text <- maybe (die (object ++ " has no " ++ textSection)) pure
        (listToMaybe [s | s <- sections, secName s == textSection])
      bindings <- loadBindings x70 (fUnit f)
      let bySymbol = M.fromList [(symIndex s, s) | s <- symbols]
          sectionName i = maybe ("#" ++ show i) secName (listToMaybe [s | s <- sections, secIndex s == i])
      targets <- forM relocs $ \r ->
        maybe (die (object ++ ": relocation names missing symbol " ++ show (relSymbol r)))
          (pure . resolveTarget bindings stem textSection (fAddress f) sectionName)
          (M.lookup (relSymbol r) bySymbol)
      let unresolved = nub [targetName t | t <- targets, targetAddress t == Nothing]
          placed = M.fromListWith (++)
            [(sec, [a]) | Target _ (Just sec) (Just a) _ <- targets, sec /= textSection]
          conflicting = [sec | (sec, as) <- M.toList placed, length (nub as) > 1]
          provides = M.fromList [(name, a) | Target name Nothing _ (Just a) <- targets]
          withObject = initial deps (secSize text) (literalBytes text symbols)
      case () of
        _ | not (null unresolved) -> pure (withObject [] Nothing (Unresolved unresolved))
          | not (null conflicting) ->
              pure (withObject [] Nothing (LinkFailed ("inconsistent placements for " ++ unwords conflicting)))
          | otherwise -> do
              let script = unlines $
                    ["SECTIONS {", "  .pilot_text " ++ show (fAddress f) ++ " : { " ++ object ++ "(" ++ textSection ++ ") }"]
                    ++ [ "  .pilot_" ++ show i ++ " " ++ show a ++ " : { " ++ object ++ "(" ++ sec ++ ") }"
                       | (i, (sec, a : _)) <- zip [(0 :: Int) ..] (M.toList placed) ]
                    ++ ["  /DISCARD/ : { *(*) }", "}"]
                    ++ ["PROVIDE(" ++ name ++ " = " ++ show a ++ ");" | (name, a) <- M.toList provides]
                  ld = fName f ++ ".ld"
                  elf = fName f ++ ".elf"
                  bin = fName f ++ ".bin"
              writeFile (work </> ld) script
              linked <- runEither work (binutil tools "ld") ["-T", ld, "-o", elf, object]
              case linked of
                Left e -> pure (withObject [] Nothing (LinkFailed e))
                Right _ -> do
                  _ <- runOk work (binutil tools "objcopy") ["-O", "binary", "-j", ".pilot_text", elf, bin]
                  emitted <- B.readFile (work </> bin)
                  let stock = B.take (B.length emitted) (B.drop offset image)
                      placements = [ Placement (relOffset r) (relKind r) name a
                                   | (r, Target name _ _ (Just a)) <- zip relocs targets ]
                      fields = [RelocatedField (relOffset r) (relKind r) | r <- relocs]
                  when (B.length stock /= B.length emitted)
                    (die (fName f ++ ": emitted section runs past the end of the " ++ regionName (fRegion f) ++ " image"))
                  pure (withObject placements (Just (digest emitted))
                    (Compared (compareLinked fields (fSize f) emitted stock)))

-- Make-style dependency list written by -MD.
dependencies :: FilePath -> FilePath -> IO [(FilePath, Digest)]
dependencies work depfile = do
  text <- readFile (work </> depfile)
  let tokens = filter (\t -> t /= "\\" && not (":" `isSuffixOf` t)) (words (drop 1 (dropWhile (/= ':') text)))
  forM tokens $ \t -> (,) t . digest <$> B.readFile (work </> t)

-- Report ---------------------------------------------------------------------------

resultJson :: Result -> A.Value
resultJson r =
  let f = rFunction r
  in A.object
    [ "region" .= regionName (fRegion f)
    , "name" .= fName f
    , "address" .= show (fAddress f)
    , "compiler" .= ("GCC " ++ compilerName (rCompiler r))
    , "flags" .= unwords (rFlags r)
    , "generated_api_stub" .= any ((== "api-stub/api.h") . fst) (rDependencies r)
    , "class" .= classText (outcomeClass (rOutcome r))
    , "detail" .= outcomeDetail (rOutcome r)
    , "stock_body_bytes" .= fSize f
    , "emitted_bytes" .= rEmitted r
    , "emitted_literal_bytes" .= rLiteral r
    , "emitted_beyond_body_bytes" .= max 0 (rEmitted r - fSize f)
    , "stock_body_bytes_not_emitted" .= max 0 (fSize f - rEmitted r)
    , "stock_sha256" .= digestText (fStockDigest f)
    , "emitted_sha256" .= fmap digestText (rEmittedDigest r)
    , "relocations" .= [ A.object ["offset" .= o, "kind" .= relocKindName k, "symbol" .= n, "target" .= show t]
                       | Placement o k n t <- rPlacements r ]
    , "dependencies" .= [A.object ["path" .= p, "sha256" .= digestText d] | (p, d) <- rDependencies r]
    ]

summaryLine :: Result -> String
summaryLine r =
  let f = rFunction r
  in concat [ pad 9 (regionName (fRegion f)), show (fAddress f), "  ", pad 30 (fName f)
            , "GCC ", compilerName (rCompiler r), "  ", pad 11 (classText (outcomeClass (rOutcome r)))
            , show (rEmitted r), "/", show (fSize f), " B  ", outcomeDetail (rOutcome r) ]
  where pad k t = t ++ replicate (k - length t) ' '

main :: IO ()
main = do
  parsed <- getArgs >>= either die pure . parseOptions
  -- bazel run starts in the runfiles tree; owner paths are relative to the shell.
  owner <- maybe id (</>) <$> lookupEnv "BUILD_WORKING_DIRECTORY"
  let o = parsed { oX70view = owner (oX70view parsed), oOut = owner (oOut parsed)
                 , oImages = M.map owner (oImages parsed) }
  manifest <- A.eitherDecodeFileStrict (oManifest o) >>= either die pure
  exists <- doesPathExist (oOut o)
  when exists (die (oOut o ++ " exists; choose a fresh output directory"))
  createDirectoryIfMissing True (takeDirectory (oOut o))
  createDirectory (oOut o)
  work <- canonicalizePath (oOut o)
  x70 <- canonicalizePath (oX70view o)
  revision <- trim <$> runOk x70 "git" ["rev-parse", "HEAD"]
  when (revision /= mRevision manifest)
    (die ("x70view HEAD " ++ revision ++ " is not the frozen revision " ++ mRevision manifest))
  dirty <- runOk x70 "git" ["status", "--porcelain", "--", "decomp"]
  unless (null dirty) (die ("x70view decomp/ has local changes:\n" ++ dirty))
  ld <- canonicalizePath (oLd o)
  unless ("-ld" `isSuffixOf` takeFileName ld) (die "--ld must name a TARGET-ld executable")
  gccs <- traverse canonicalizePath (oGcc o)
  let tools = Tools gccs (take (length ld - 2) ld)
  images <- fmap M.fromList $ forM (M.toList (mRegions manifest)) $ \(r, ri) -> do
    path <- maybe (die ("missing --image " ++ regionName r)) pure (M.lookup r (oImages o))
    bytes <- B.readFile path
    when (digest bytes /= riDigest ri) (die (path ++ ": digest differs from the manifest's " ++ regionName r))
    pure (r, (riBase ri, bytes))
  createFileLink (x70 </> "decomp") (work </> "src")
  createDirectory (work </> "api-stub")
  writeFile (work </> "api-stub" </> "api.h") ""
  results <- mapM (pilot tools (oForce o) x70 images work) (mFunctions manifest)
  let count c = length [() | r <- results, outcomeClass (rOutcome r) == c]
      report = A.object
        [ "x70view_revision" .= revision
        , "forced_compiler" .= fmap compilerName (oForce o)
        , "exact" .= count ClassExact
        , "normalized" .= count ClassNormalized
        , "failure" .= count ClassFailure
        , "functions" .= map resultJson results ]
  BL.writeFile (work </> "report.json") (A.encode report)
  forM_ results (putStrLn . summaryLine)
  putStrLn (concat [ "exact ", show (count ClassExact), ", normalized ", show (count ClassNormalized)
                   , ", failure ", show (count ClassFailure), "; report ", work </> "report.json" ])
