{-# LANGUAGE OverloadedStrings #-}
-- Whole-translation-unit rebuild gate and two-step patch check: every function
-- an x70view translation unit owns must rebuild to its stock extent, and a
-- patched copy of the unit may change stock bytes only inside the intended
-- function's extent.
module Main (main) where

import Control.Monad (forM, forM_, unless, when)
import Data.Aeson ((.:), (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Types as A
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.List (isPrefixOf, sort)
import qualified Data.Map.Strict as M
import Data.Maybe (isJust)
import FujiAF.Matching.Build
import FujiAF.Matching.Compare
import System.Directory (canonicalizePath, copyFile, createDirectory)
import System.Environment (getArgs)
import System.FilePath (takeBaseName, takeDirectory, takeFileName, (</>))

data Owned = Owned { owName :: String, owAddress :: Address, owSize :: Int, owDigest :: Digest }

data Manifest = Manifest
  { mRevision :: String
  , mUnit :: LinkUnit
  , mRegion :: Region
  , mBase :: Address
  , mImageDigest :: Digest
  , mSource :: FilePath
  , mSourceDigest :: Digest
  , mFunctions :: [Owned]
  }

orFail :: Either String a -> A.Parser a
orFail = either fail pure

instance A.FromJSON Manifest where
  parseJSON = A.withObject "unit gate" $ \o -> do
    upstream <- o .: "upstream"
    Manifest
      <$> upstream .: "revision"
      <*> (o .: "unit" >>= orFail . parseUnit)
      <*> (o .: "region" >>= orFail . parseRegion)
      <*> (o .: "region_base" >>= orFail . parseAddress)
      <*> (o .: "region_sha256" >>= orFail . parseDigest)
      <*> o .: "upstream_source"
      <*> (o .: "upstream_source_sha256" >>= orFail . parseDigest)
      <*> (o .: "functions" >>= mapM (A.withObject "function" $ \x -> Owned
            <$> x .: "name"
            <*> (x .: "address" >>= orFail . parseAddress)
            <*> x .: "size"
            <*> (x .: "stock_sha256" >>= orFail . parseDigest)))

-- Command line ---------------------------------------------------------------

data Patch = Patch
  { pSource :: FilePath
  , pFunction :: String
  , pImage :: FilePath
  }

data Options = Options
  { oManifest :: FilePath
  , oX70view :: FilePath
  , oOut :: FilePath
  , oGcc :: M.Map Compiler FilePath
  , oLd :: FilePath
  , oImage :: FilePath
  , oPatchSource :: Maybe FilePath
  , oPatchFunction :: Maybe String
  , oPatchedImage :: Maybe FilePath
  -- Stock image with UDF at the patched function's entry.
  , oTrapImage :: Maybe FilePath
  }

usage :: String
usage = unwords
  [ "usage: unit-gate --manifest JSON --x70view DIR --out NEW_DIR --gcc VERSION=PATH... --ld TARGET-ld --image PATH"
  , "[--patch-source C --patch-function NAME --patched-image OUT [--trap-image OUT]]" ]

parseOptions :: [String] -> Either String Options
parseOptions = go (Options "" "" "" M.empty "" "" Nothing Nothing Nothing Nothing)
  where
    go o [] = do
      when (any null [oManifest o, oX70view o, oOut o, oLd o, oImage o]) (Left usage)
      let patchParts = [isJust (oPatchSource o), isJust (oPatchFunction o), isJust (oPatchedImage o)]
      when (or patchParts && not (and patchParts)) (Left usage)
      when (isJust (oTrapImage o) && not (and patchParts)) (Left usage)
      pure o
    go o ("--manifest" : v : rest) = go o {oManifest = v} rest
    go o ("--x70view" : v : rest) = go o {oX70view = v} rest
    go o ("--out" : v : rest) = go o {oOut = v} rest
    go o ("--ld" : v : rest) = go o {oLd = v} rest
    go o ("--image" : v : rest) = go o {oImage = v} rest
    go o ("--patch-source" : v : rest) = go o {oPatchSource = Just v} rest
    go o ("--patch-function" : v : rest) = go o {oPatchFunction = Just v} rest
    go o ("--patched-image" : v : rest) = go o {oPatchedImage = Just v} rest
    go o ("--trap-image" : v : rest) = go o {oTrapImage = Just v} rest
    go o ("--gcc" : v : rest) = do
      let (k, p) = break (== '=') v
      c <- parseCompiler k
      go o {oGcc = M.insert c (drop 1 p) (oGcc o)} rest
    go _ (a : _) = Left ("unexpected argument " ++ a ++ "\n" ++ usage)

patchOf :: Options -> Maybe Patch
patchOf o = Patch <$> oPatchSource o <*> oPatchFunction o <*> oPatchedImage o

-- Building one copy of the unit ------------------------------------------------

data Build = Build
  { bCompiled :: Compiled
  , bRoots :: [LinkedRoot]
  , bResults :: [(Owned, B.ByteString, ByteComparison)]
  }

-- Compiles and links every function of the unit at its stock entry. The
-- object must emit exactly the owned functions and no allocated data.
buildUnit :: Tools -> Bindings -> String -> FilePath -> String -> FilePath -> [String] -> Manifest -> (Address, B.ByteString)
          -> IO (Either String Build)
buildUnit tools bindings stem work label source extra m region = do
  compiled <- compileUnit tools Nothing (mUnit m) work label source extra
  case compiled of
    Left e -> pure (Left ("compile failed: " ++ take 2000 e))
    Right c -> do
      let emitted = sort (textSections c)
          owned = sort (map owName (mFunctions m))
          dataSections = [s | (s, size) <- objectSections c, size > 0, any (`isPrefixOf` s) [".data", ".rodata", ".bss"]]
      if emitted /= owned
        then pure (Left ("emitted functions " ++ unwords emitted ++ " differ from the owned set " ++ unwords owned))
        else if not (null dataSections)
          then pure (Left ("allocated data sections are not compared: " ++ unwords dataSections))
          else do
            linked <- linkRoots tools bindings stem work label c [Root (owName f) (owAddress f) | f <- mFunctions m]
            case linked of
              Left failure -> pure (Left (linkFailureText failure))
              Right roots -> do
                results <- forM (zip (mFunctions m) roots) $ \(f, root) -> do
                  stock <- maybe (die (owName f ++ ": emitted section runs past the image")) pure
                    (stockAt region (owAddress f) (B.length (lrBytes root)))
                  pure (f, lrBytes root, compareLinked (lrFields root) (owSize f) (lrBytes root) stock)
                pure (Right (Build c roots results))

-- Report helpers ------------------------------------------------------------------

functionJson :: (Owned, B.ByteString, ByteComparison) -> A.Value
functionJson (f, bytes, comparison) = A.object
  [ "name" .= owName f
  , "address" .= show (owAddress f)
  , "stock_body_bytes" .= owSize f
  , "emitted_bytes" .= B.length bytes
  , "emitted_sha256" .= digestText (digest bytes)
  , "result" .= comparisonDetail comparison
  , "exact" .= (comparison == Exact) ]

buildJson :: Build -> A.Value
buildJson b = A.object
  [ "compiler" .= ("GCC " ++ compilerName (cCompiler (bCompiled b)))
  , "flags" .= unwords (cFlags (bCompiled b))
  , "generated_api_stub" .= any ((== "api-stub/api.h") . fst) (cDependencies (bCompiled b))
  , "functions" .= map functionJson (bResults b)
  , "relocations" .= length (concatMap lrPlacements (bRoots b))
  , "dependencies" .= [A.object ["path" .= p, "sha256" .= digestText d] | (p, d) <- cDependencies (bCompiled b)] ]

-- Byte offsets where two equal-length images differ.
differences :: B.ByteString -> B.ByteString -> [Int]
differences a b = [i | i <- [0 .. B.length a - 1], B.index a i /= B.index b i]

udf :: B.ByteString
udf = B.pack [0xf0, 0x00, 0xf0, 0xe7]

main :: IO ()
main = do
  parsed <- getArgs >>= either die pure . parseOptions
  owner <- ownerPath
  let o = parsed
        { oX70view = owner (oX70view parsed), oOut = owner (oOut parsed), oImage = owner (oImage parsed)
        , oPatchSource = owner <$> oPatchSource parsed, oPatchedImage = owner <$> oPatchedImage parsed
        , oTrapImage = owner <$> oTrapImage parsed }
  m <- A.eitherDecodeFileStrict (oManifest o) >>= either die pure
  x70 <- canonicalizePath (oX70view o)
  checkUpstream x70 (mRevision m)
  source <- B.readFile (x70 </> mSource m)
  when (digest source /= mSourceDigest m) (die (mSource m ++ ": source digest differs from the manifest"))
  image <- B.readFile (oImage o)
  when (digest image /= mImageDigest m) (die (oImage o ++ ": digest differs from the manifest's " ++ regionName (mRegion m)))
  let region = (mBase m, image)
  forM_ (mFunctions m) $ \f ->
    when (fmap digest (stockAt region (owAddress f) (owSize f)) /= Just (owDigest f))
      (die (owName f ++ ": stock body digest differs from the manifest"))
  tools <- toolsFor (oGcc o) (oLd o)
  bindings <- loadBindings x70 (mUnit m)
  work <- prepareWork (oOut o) x70
  let stem = takeBaseName (mSource m)
      inTree = "src" </> drop (length ("decomp/" :: String)) (mSource m)
  -- Step 1: the unmodified unit rebuilds every owned function exactly.
  stock <- buildUnit tools bindings stem work "stock" inTree [] m region
  gate <- case stock of
    Left e -> pure (Left e)
    Right b | all (\(_, _, c) -> c == Exact) (bResults b) -> pure (Right b)
            | otherwise -> pure (Left "an owned function does not rebuild exactly")
  forM_ (either (const []) bResults stock) $ \(f, bytes, c) ->
    putStrLn (concat ["gate  ", show (owAddress f), "  ", owName f, "  ", show (B.length bytes), "/", show (owSize f), " B  ", comparisonDetail c])
  -- Step 2: the patched unit changes stock bytes only inside the intended extent.
  patchReport <- case (gate, patchOf o) of
    (Right _, Just p) -> do
      intended <- maybe (die (pFunction p ++ " is not owned by the unit")) pure
        (lookup (pFunction p) [(owName f, f) | f <- mFunctions m])
      createDirectory (work </> "patched")
      let patchedSource = "patched" </> takeFileName (mSource m)
      copyFile (pSource p) (work </> patchedSource)
      patchedText <- B.readFile (pSource p)
      built <- buildUnit tools bindings stem work "patched" patchedSource ["-iquote", takeDirectory inTree] m region
      b <- either (\e -> die ("patched unit: " ++ e)) pure built
      patchedBytes <- maybe (die ("patched unit emitted no " ++ pFunction p)) pure
        (lookup (pFunction p) [(owName f, bytes) | (f, bytes, _) <- bResults b])
      let others = [(f, c) | (f, _, c) <- bResults b, owName f /= pFunction p]
          Address entry = owAddress intended
          Address base = mBase m
          offset = fromIntegral (entry - base)
          patchedImage = B.concat [B.take offset image, patchedBytes, B.drop (offset + B.length patchedBytes) image]
          changed = differences image patchedImage
          outside = [i | i <- changed, i < offset || i >= offset + owSize intended]
      unless (all ((== Exact) . snd) others) (die "patched unit: another owned function no longer rebuilds exactly")
      when (B.length patchedBytes > owSize intended)
        (die ("patched " ++ pFunction p ++ " is " ++ show (B.length patchedBytes) ++ " bytes, larger than its "
              ++ show (owSize intended) ++ "-byte stock extent"))
      when (null changed) (die "the patch changes no stock bytes")
      unless (null outside) (die ("the patch changes " ++ show (length outside) ++ " bytes outside the intended extent"))
      B.writeFile (pImage p) patchedImage
      forM_ (oTrapImage o) $ \trap ->
        B.writeFile trap (B.concat [B.take offset image, udf, B.drop (offset + 4) image])
      putStrLn (concat [ "patch ", show (owAddress intended), "  ", pFunction p, "  ", show (B.length patchedBytes), "/"
                       , show (owSize intended), " B, ", show (length changed), " image bytes changed, all inside the extent" ])
      pure $ Just $ A.object
        [ "function" .= pFunction p
        , "patched_source_sha256" .= digestText (digest patchedText)
        , "build" .= buildJson b
        , "emitted_bytes" .= B.length patchedBytes
        , "stock_extent_bytes" .= owSize intended
        , "stock_bytes_retained_in_extent" .= (owSize intended - B.length patchedBytes)
        , "changed_image_bytes" .= length changed
        , "changed_bytes_outside_extent" .= length outside
        , "patched_image_sha256" .= digestText (digest patchedImage)
        , "trap_image" .= isJust (oTrapImage o) ]
    _ -> pure Nothing
  let report = A.object
        [ "x70view_revision" .= mRevision m
        , "source" .= mSource m
        , "gate" .= either (const False) (const True) gate
        , "gate_detail" .= either id (const "every owned function rebuilds exactly") gate
        , "stock_build" .= either (const A.Null) buildJson stock
        , "patch" .= patchReport ]
  BL.writeFile (work </> "report.json") (A.encode report)
  case gate of
    Left e -> die ("gate failed: " ++ e)
    Right _ -> putStrLn ("gate passed; report " ++ work </> "report.json")
