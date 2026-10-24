{-# LANGUAGE OverloadedStrings #-}
-- MATCHING-C-01: compile frozen x70view C translation units with the pinned
-- EXR II GCC, link each function section at its stock address and compare the
-- linked bytes with the owner's decoded X-M1 1.50 images.
module Main (main) where

import Control.Monad (forM, forM_, unless, when)
import Data.Aeson ((.:), (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Aeson.Types as A
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.List (isPrefixOf)
import qualified Data.Map.Strict as M
import FujiAF.Matching.Build
import FujiAF.Matching.Compare
import System.Directory (canonicalizePath, createDirectoryIfMissing)
import System.Environment (getArgs)
import System.FilePath (takeBaseName, takeDirectory, takeFileName, (</>))

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
        <*> (x .: "sha256" >>= orFail . parseDigest)) v
      pure (r, image)
    functions <- o .: "functions" >>= mapM (A.withObject "function" $ \x -> Frozen
      <$> (x .: "region" >>= orFail . parseRegion)
      <*> (x .: "unit" >>= orFail . parseUnit)
      <*> (x .: "address" >>= orFail . parseAddress)
      <*> x .: "size"
      <*> x .: "name"
      <*> (x .: "stock_sha256" >>= orFail . parseDigest)
      <*> x .: "upstream_source"
      <*> (x .: "upstream_source_sha256" >>= orFail . parseDigest))
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
  -- Per-function copies of the unit that differ only in their flags and
  -- compiler annotations.
  , oAnnotations :: M.Map String FilePath
  }

usage :: String
usage = "usage: matching-pilot --manifest JSON --x70view DIR --out NEW_DIR --gcc VERSION=PATH... --ld TARGET-ld --image REGION=PATH... [--force-gcc VERSION] [--annotation NAME=C_FILE...]"

parseOptions :: [String] -> Either String Options
parseOptions = go (Options "" "" "" M.empty "" M.empty Nothing M.empty)
  where
    go o [] = do
      when (any null [oManifest o, oX70view o, oOut o, oLd o]) (Left usage)
      pure o
    go o ("--manifest" : v : rest) = go o {oManifest = v} rest
    go o ("--x70view" : v : rest) = go o {oX70view = v} rest
    go o ("--out" : v : rest) = go o {oOut = v} rest
    go o ("--ld" : v : rest) = go o {oLd = v} rest
    go o ("--annotation" : v : rest) = do
      let (k, p) = break (== '=') v
      when (null k || null p) (Left usage)
      go o {oAnnotations = M.insert k (drop 1 p) (oAnnotations o)} rest
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

-- Per-function pilot -------------------------------------------------------------

data Outcome
  = Compared ByteComparison
  | NotLinked LinkFailure
  | CompileFailed String

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
  NotLinked f -> linkFailureText f
  CompileFailed e -> "compile failed: " ++ take 2000 e

data Result = Result
  { rFunction :: Frozen
  , rCompiled :: Maybe Compiled
  , rLinked :: Maybe LinkedRoot
  , rOutcome :: Outcome
  }

pilot :: Tools -> Maybe Compiler -> M.Map String FilePath -> FilePath -> M.Map Region (Address, B.ByteString) -> FilePath -> Frozen -> IO Result
pilot tools force annotations x70 images work f = do
  unless ("decomp/" `isPrefixOf` fSource f) (die (fSource f ++ ": upstream source must lie under decomp/"))
  source <- B.readFile (x70 </> fSource f)
  when (digest source /= fSourceDigest f) (die (fSource f ++ ": source digest differs from the frozen sample"))
  region <- maybe (die ("no image for " ++ regionName (fRegion f))) pure (M.lookup (fRegion f) images)
  when (fmap digest (stockAt region (fAddress f) (fSize f)) /= Just (fStockDigest f))
    (die (fName f ++ ": stock body digest differs from the frozen sample"))
  let sourceInTree = "src" </> drop (length ("decomp/" :: String)) (fSource f)
  (compileSource, extra) <- case M.lookup (fName f) annotations of
    Nothing -> pure (sourceInTree, [])
    Just path -> do
      annotated <- B.readFile path
      unless (annotationFree annotated == annotationFree source)
        (die (path ++ ": differs from " ++ fSource f ++ " outside its flags and compiler annotations"))
      let copy = "annotated" </> fName f </> takeFileName (fSource f)
      createDirectoryIfMissing True (work </> takeDirectory copy)
      B.writeFile (work </> copy) annotated
      pure (copy, ["-iquote", takeDirectory sourceInTree])
  compiled <- compileUnit tools force (fUnit f) work (fName f) compileSource extra
  case compiled of
    Left e -> pure (Result f Nothing Nothing (CompileFailed e))
    Right c -> do
      bindings <- loadBindings x70 (fUnit f)
      linked <- linkRoots tools bindings (takeBaseName (fSource f)) work (fName f) c [Root (fName f) (fAddress f)]
      case linked of
        Left failure -> pure (Result f (Just c) Nothing (NotLinked failure))
        Right [root] -> do
          let emitted = lrBytes root
          stock <- maybe (die (fName f ++ ": emitted section runs past the end of the " ++ regionName (fRegion f) ++ " image"))
            pure (stockAt region (fAddress f) (B.length emitted))
          pure (Result f (Just c) (Just root) (Compared (compareLinked (lrFields root) (fSize f) emitted stock)))
        Right _ -> die "one root links to one section"

-- Report ---------------------------------------------------------------------------

resultJson :: Result -> A.Value
resultJson r =
  let f = rFunction r
      emitted = maybe 0 (B.length . lrBytes) (rLinked r)
      deps = maybe [] cDependencies (rCompiled r)
  in A.object
    [ "region" .= regionName (fRegion f)
    , "name" .= fName f
    , "address" .= show (fAddress f)
    , "compiler" .= fmap (("GCC " ++) . compilerName . cCompiler) (rCompiled r)
    , "flags" .= fmap (unwords . cFlags) (rCompiled r)
    , "generated_api_stub" .= any ((== "api-stub/api.h") . fst) deps
    , "class" .= classText (outcomeClass (rOutcome r))
    , "detail" .= outcomeDetail (rOutcome r)
    , "stock_body_bytes" .= fSize f
    , "emitted_bytes" .= emitted
    , "emitted_literal_bytes" .= maybe 0 lrLiteral (rLinked r)
    , "emitted_beyond_body_bytes" .= max 0 (emitted - fSize f)
    , "stock_body_bytes_not_emitted" .= max 0 (fSize f - emitted)
    , "stock_sha256" .= digestText (fStockDigest f)
    , "emitted_sha256" .= fmap (digestText . digest . lrBytes) (rLinked r)
    , "relocations" .= maybe [] (map placementJson . lrPlacements) (rLinked r)
    , "dependencies" .= [A.object ["path" .= p, "sha256" .= digestText d] | (p, d) <- deps]
    ]

placementJson :: Placement -> A.Value
placementJson (Placement o k n t) = A.object ["offset" .= o, "kind" .= relocKindName k, "symbol" .= n, "target" .= show t]

summaryLine :: Result -> String
summaryLine r =
  let f = rFunction r
      compiler = maybe "-" (compilerName . cCompiler) (rCompiled r)
  in concat [ pad 9 (regionName (fRegion f)), show (fAddress f), "  ", pad 30 (fName f)
            , "GCC ", compiler, "  ", pad 11 (classText (outcomeClass (rOutcome r)))
            , show (maybe 0 (B.length . lrBytes) (rLinked r)), "/", show (fSize f), " B  ", outcomeDetail (rOutcome r) ]
  where pad k t = t ++ replicate (k - length t) ' '

main :: IO ()
main = do
  parsed <- getArgs >>= either die pure . parseOptions
  owner <- ownerPath
  let o = parsed { oX70view = owner (oX70view parsed), oOut = owner (oOut parsed)
                 , oImages = M.map owner (oImages parsed) }
  manifest <- A.eitherDecodeFileStrict (oManifest o) >>= either die pure
  x70 <- canonicalizePath (oX70view o)
  checkUpstream x70 (mRevision manifest)
  tools <- toolsFor (oGcc o) (oLd o)
  images <- fmap M.fromList $ forM (M.toList (mRegions manifest)) $ \(r, ri) -> do
    path <- maybe (die ("missing --image " ++ regionName r)) pure (M.lookup r (oImages o))
    bytes <- B.readFile path
    when (digest bytes /= riDigest ri) (die (path ++ ": digest differs from the manifest's " ++ regionName r))
    pure (r, (riBase ri, bytes))
  work <- prepareWork (oOut o) x70
  results <- mapM (pilot tools (oForce o) (M.map owner (oAnnotations o)) x70 images work) (mFunctions manifest)
  let count c = length [() | r <- results, outcomeClass (rOutcome r) == c]
      report = A.object
        [ "x70view_revision" .= mRevision manifest
        , "forced_compiler" .= fmap compilerName (oForce o)
        , "annotation_overrides" .= M.keys (oAnnotations o)
        , "exact" .= count ClassExact
        , "normalized" .= count ClassNormalized
        , "failure" .= count ClassFailure
        , "functions" .= map resultJson results ]
  BL.writeFile (work </> "report.json") (A.encode report)
  forM_ results (putStrLn . summaryLine)
  putStrLn (concat [ "exact ", show (count ClassExact), ", normalized ", show (count ClassNormalized)
                   , ", failure ", show (count ClassFailure), "; report ", work </> "report.json" ])
