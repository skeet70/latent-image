{-# LANGUAGE OverloadedStrings #-}
-- Compiles x70view X-M1 1.50 C with the pinned EXR II GCC and links function
-- sections at their stock addresses through x70view's bindings.
module FujiAF.Matching.Build
  ( Region (..)
  , regionName
  , parseRegion
  , LinkUnit (..)
  , parseUnit
  , Compiler (..)
  , compilerName
  , parseCompiler
  , Address (..)
  , parseAddress
  , Digest
  , digest
  , digestText
  , parseDigest
  , Tools (..)
  , toolsFor
  , runOk
  , die
  , trim
  , ownerPath
  , checkUpstream
  , Bindings
  , loadBindings
  , prepareWork
  , annotationFree
  , Compiled (..)
  , compileUnit
  , Root (..)
  , objectSections
  , Placement (..)
  , LinkedRoot (..)
  , LinkFailure (..)
  , linkFailureText
  , linkRoots
  , textSections
  , stockAt
  ) where

import Control.Monad (forM, unless, when)
import qualified Crypto.Hash.SHA256 as SHA256
import Data.Aeson ((.:))
import qualified Data.Aeson as A
import qualified Data.Aeson.Types as A
import Data.Bits (shiftR)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.Char (isHexDigit, isSpace)
import Data.List (dropWhileEnd, isPrefixOf, isSuffixOf, nub, sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (catMaybes, fromMaybe, isJust, isNothing, listToMaybe, mapMaybe)
import Data.Word (Word32)
import FujiAF.Matching.Compare
import Numeric (readHex, showHex)
import System.Directory
  ( canonicalizePath, createDirectory, createDirectoryIfMissing, createFileLink
  , doesPathExist, listDirectory )
import System.Environment (lookupEnv)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath (takeDirectory, takeFileName, (</>))
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

parseDigest :: String -> Either String Digest
parseDigest t
  | length t == 64 && all (`elem` ("0123456789abcdef" :: String)) t = Right (Digest t)
  | otherwise = Left ("bad SHA-256 " ++ t)

-- Processes ------------------------------------------------------------------

data Tools = Tools { tGcc :: M.Map Compiler FilePath, tPrefix :: FilePath }

-- The binutils prefix comes from a TARGET-ld path.
toolsFor :: M.Map Compiler FilePath -> FilePath -> IO Tools
toolsFor gccs ldPath = do
  ld <- canonicalizePath ldPath
  unless ("-ld" `isSuffixOf` takeFileName ld) (die "--ld must name a TARGET-ld executable")
  resolved <- traverse canonicalizePath gccs
  pure (Tools resolved (take (length ld - 2) ld))

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

-- Creates the fresh work directory with src/ linked to x70view's decomp/ and
-- an empty api.h on the quote path, and returns its canonical path.
prepareWork :: FilePath -> FilePath -> IO FilePath
prepareWork out x70 = do
  exists <- doesPathExist out
  when exists (die (out ++ " exists; choose a fresh output directory"))
  createDirectoryIfMissing True (takeDirectory out)
  createDirectory out
  work <- canonicalizePath out
  createFileLink (x70 </> "decomp") (work </> "src")
  createDirectory (work </> "api-stub")
  writeFile (work </> "api-stub" </> "api.h") ""
  pure work

trim :: String -> String
trim = dropWhileEnd isSpace . dropWhile isSpace

-- bazel run starts in the runfiles tree; owner paths are relative to the shell.
ownerPath :: IO (FilePath -> FilePath)
ownerPath = maybe id (</>) <$> lookupEnv "BUILD_WORKING_DIRECTORY"

-- The checkout must be at the frozen revision with a clean decomp/.
checkUpstream :: FilePath -> String -> IO ()
checkUpstream x70 revision = do
  observed <- trim <$> runOk x70 "git" ["rev-parse", "HEAD"]
  when (observed /= revision) (die ("x70view HEAD " ++ observed ++ " is not the frozen revision " ++ revision))
  dirty <- runOk x70 "git" ["status", "--porcelain", "--", "decomp"]
  unless (null dirty) (die ("x70view decomp/ has local changes:\n" ++ dirty))

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
        (first : _) | Just body <- annotation "flags:" first -> words body
        _ -> ["-O1", "-fno-omit-frame-pointer"]
  compiler <- maybe (Right Gcc464) (parseCompiler . trim) (listToMaybe (mapMaybe (annotation "compiler:") ls))
  pure (compiler, flags)

annotation :: String -> String -> Maybe String
annotation key line =
  let t = trim line
      inner = trim (drop 2 (take (length t - 2) t))
  in if "/*" `isPrefixOf` t && "*/" `isSuffixOf` t && key `isPrefixOf` inner
       then Just (drop (length key) inner)
       else Nothing

-- The source's lines without the flags and compiler annotations among its
-- first three.
annotationFree :: B.ByteString -> [B.ByteString]
annotationFree text =
  let (headLines, rest) = splitAt 3 (BC.lines text)
      isAnnotation l = any (\k -> isJust (annotation k (BC.unpack l))) ["flags:", "compiler:"]
  in filter (not . isAnnotation) headLines ++ rest

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

-- Compilation ------------------------------------------------------------------

data Compiled = Compiled
  { cObject :: FilePath
  , cCompiler :: Compiler
  , cFlags :: [String]
  , cDependencies :: [(FilePath, Digest)]
  , cSections :: [Section]
  , cSymbols :: [Symbol]
  , cRelocations :: String
  }

-- Compiles SOURCE (relative to WORK, where src/ links to x70view's decomp/)
-- into LABEL.o. EXTRA flags follow x70view's; a forced release replaces the
-- source's annotation.
compileUnit :: Tools -> Maybe Compiler -> LinkUnit -> FilePath -> String -> FilePath -> [String] -> IO (Either String Compiled)
compileUnit tools force unit work label source extra = do
  text <- B.readFile (work </> source)
  (annotated, fileFlags) <- either die pure (sourceSettings text)
  let compiler = fromMaybe annotated force
      object = label ++ ".o"
      depfile = label ++ ".d"
      flags = commonFlags ++ fileFlags ++ unitDefines unit ++ ["-fdata-sections", "-iquote", "api-stub"] ++ extra
  gcc <- maybe (die ("missing --gcc " ++ compilerName compiler)) pure (M.lookup compiler (tGcc tools))
  compiled <- runEither work gcc (flags ++ ["-MD", "-MF", depfile, "-c", source, "-o", object])
  case compiled of
    Left e -> pure (Left e)
    Right _ -> do
      deps <- dependencies work depfile
      sections <- parseSections <$> runOk work (binutil tools "readelf") ["-W", "-S", object]
      symbols <- parseSymbols <$> runOk work (binutil tools "readelf") ["-W", "-s", object]
      relocs <- runOk work (binutil tools "readelf") ["-W", "-r", object]
      pure (Right (Compiled object compiler flags deps sections symbols relocs))

-- Make-style dependency list written by -MD.
dependencies :: FilePath -> FilePath -> IO [(FilePath, Digest)]
dependencies work depfile = do
  text <- readFile (work </> depfile)
  let tokens = filter (\t -> t /= "\\" && not (":" `isSuffixOf` t)) (words (drop 1 (dropWhile (/= ':') text)))
  forM tokens $ \t -> (,) t . digest <$> B.readFile (work </> t)

-- Function names of the object's nonempty .text.NAME sections.
textSections :: Compiled -> [String]
textSections c = [drop 6 (secName s) | s <- cSections c, ".text." `isPrefixOf` secName s, secSize s > 0]

objectSections :: Compiled -> [(String, Int)]
objectSections c = [(secName s, secSize s) | s <- cSections c]

-- Linking ------------------------------------------------------------------------

-- A function section and the stock entry it is linked at.
data Root = Root { rootName :: String, rootAddress :: Address }

-- Relocation offset, kind, target symbol and resolved address.
data Placement = Placement Int RelocKind String Address

data LinkedRoot = LinkedRoot
  { lrRoot :: Root
  , lrBytes :: B.ByteString
  , lrLiteral :: Int
  , lrFields :: [RelocatedField]
  , lrPlacements :: [Placement]
  }

data LinkFailure = MissingSection String | Unresolved [String] | LinkError String

linkFailureText :: LinkFailure -> String
linkFailureText f = case f of
  MissingSection s -> "the object has no " ++ s
  Unresolved syms -> "no unique x70view binding for " ++ unwords syms
  LinkError e -> "link failed: " ++ take 2000 e

-- Name, defining section of this object, that section's placement, and the
-- symbol's address. Undefined symbols resolve through PROVIDE; sections of
-- this object that a relocation reaches are placed at their bound address.
data Target = Target String (Maybe String) (Maybe Address) (Maybe Address)

-- A section's address comes from a root, its "@stem:section" placement, or a
-- bound symbol it defines (section anchors such as .LANCHOR0 have no binding
-- of their own).
resolveTarget :: Bindings -> String -> M.Map String Address -> M.Map String Address -> (Int -> String) -> Symbol -> Target
resolveTarget bindings stem roots derived sectionName s =
  case (symIsSection s, symShndx s) of
    (True, Just ndx) ->
      let sec = sectionName ndx
          at = sectionAt sec
      in Target sec (Just sec) at at
    (False, Just ndx) ->
      let sec = sectionName ndx
          placed = case named of
            Just (Address a) | not (M.member sec roots) -> Just (Address (a - symValue s))
            _ -> sectionAt sec
      in Target (symName s) (Just sec) placed ((`offsetBy` symValue s) <$> placed)
    _ -> Target (symName s) Nothing Nothing named
  where
    named = bound bindings stem s
    sectionAt sec = listToMaybe (catMaybes [M.lookup sec roots, M.lookup ("@" ++ stem ++ ":" ++ sec) bindings, M.lookup sec derived])

bound :: Bindings -> String -> Symbol -> Maybe Address
bound bindings stem s = listToMaybe (mapMaybe (`M.lookup` bindings) [stem ++ ":" ++ symName s, symName s])

-- Links every root's .text.NAME at its entry in one ELF, with the object
-- sections its relocations reach at their bound addresses, and returns each
-- root's linked bytes. Reached text sections are linked with their own
-- relocations resolved but are not returned.
linkRoots :: Tools -> Bindings -> String -> FilePath -> String -> Compiled -> [Root] -> IO (Either LinkFailure [LinkedRoot])
linkRoots tools bindings stem work label c roots = do
  let rootSections = M.fromList [(".text." ++ rootName r, rootAddress r) | r <- roots]
      bySymbol = M.fromList [(symIndex s, s) | s <- cSymbols c]
      sectionName i = maybe ("#" ++ show i) secName (listToMaybe [s | s <- cSections c, secIndex s == i])
      sectionNamed n = listToMaybe [s | s <- cSections c, secName s == n]
      derived = M.mapMaybe (\as -> case nub as of [a] -> Just a; _ -> Nothing) $ M.fromListWith (++)
        [ (sectionName ndx, [Address (a - symValue s)])
        | s <- cSymbols c, not (symIsSection s), Just ndx <- [symShndx s]
        , Just (Address a) <- [bound bindings stem s] ]
      -- Relocations without a symbol (R_ARM_V4BX) mark instructions, not values.
      relocsOf sec = [rel | rel <- parseRelocs sec (cRelocations c), relSymbol rel /= 0]
      targetsOf sec = forM (relocsOf sec) $ \rel ->
        maybe (die (cObject c ++ ": relocation names missing symbol " ++ show (relSymbol rel)))
          (pure . resolveTarget bindings stem rootSections derived sectionName)
          (M.lookup (relSymbol rel) bySymbol)
      -- Every text section reached from the roots, transitively.
      reach done [] = pure done
      reach done (sec : rest)
        | M.member sec done = reach done rest
        | otherwise = do
            ts <- targetsOf sec
            let next = [s | Target _ (Just s) (Just _) _ <- ts, ".text." `isPrefixOf` s]
            reach (M.insert sec ts done) (rest ++ next)
  case [n | n <- M.keys rootSections, isNothing (sectionNamed n)] of
    (missing : _) -> pure (Left (MissingSection missing))
    [] -> do
      processed <- reach M.empty (M.keys rootSections)
      let targets = concat (M.elems processed)
          unresolved = nub [n | Target n _ _ Nothing <- targets]
          placed = M.fromListWith (++)
            [(sec, [a]) | Target _ (Just sec) (Just a) _ <- targets, not (M.member sec rootSections)]
          conflicting = [sec | (sec, as) <- M.toList placed, length (nub as) > 1]
          provides = M.fromList [(n, a) | Target n Nothing _ (Just a) <- targets]
          rootOutputs = zip [(0 :: Int) ..] roots
      case () of
        _ | not (null unresolved) -> pure (Left (Unresolved unresolved))
          | not (null conflicting) -> pure (Left (LinkError ("inconsistent placements for " ++ unwords conflicting)))
          | otherwise -> do
              let script = unlines $
                    ["SECTIONS {"]
                    ++ [ "  .root_" ++ show i ++ " " ++ show (rootAddress r) ++ " : { " ++ cObject c ++ "(.text." ++ rootName r ++ ") }"
                       | (i, r) <- rootOutputs ]
                    ++ [ "  .placed_" ++ show i ++ " " ++ show a ++ " : { " ++ cObject c ++ "(" ++ sec ++ ") }"
                       | (i, (sec, a : _)) <- zip [(0 :: Int) ..] (M.toList placed) ]
                    ++ ["  /DISCARD/ : { *(*) }", "}"]
                    ++ ["PROVIDE(" ++ n ++ " = " ++ show a ++ ");" | (n, a) <- M.toList provides]
                  ld = label ++ ".ld"
                  elf = label ++ ".elf"
              writeFile (work </> ld) script
              linked <- runEither work (binutil tools "ld") ["-T", ld, "-o", elf, cObject c]
              case linked of
                Left e -> pure (Left (LinkError e))
                Right _ -> fmap Right $ forM rootOutputs $ \(i, r) -> do
                  let sec = ".text." ++ rootName r
                      relocs = relocsOf sec
                      ts = M.findWithDefault [] sec processed
                      bin = label ++ "." ++ rootName r ++ ".bin"
                  _ <- runOk work (binutil tools "objcopy") ["-O", "binary", "-j", ".root_" ++ show i, elf, bin]
                  bytes <- B.readFile (work </> bin)
                  text <- maybe (die ("no " ++ sec)) pure (sectionNamed sec)
                  pure LinkedRoot
                    { lrRoot = r
                    , lrBytes = bytes
                    , lrLiteral = literalBytes text (cSymbols c)
                    , lrFields = [RelocatedField (relOffset rel) (relKind rel) | rel <- relocs]
                    , lrPlacements = [Placement (relOffset rel) (relKind rel) n a | (rel, Target n _ _ (Just a)) <- zip relocs ts]
                    }

-- Stock bytes of a region image at an address, or Nothing outside it.
stockAt :: (Address, B.ByteString) -> Address -> Int -> Maybe B.ByteString
stockAt (Address base, image) (Address a) n
  | a < base = Nothing
  | otherwise =
      let offset = fromIntegral (a - base)
      in if offset + n <= B.length image then Just (B.take n (B.drop offset image)) else Nothing
