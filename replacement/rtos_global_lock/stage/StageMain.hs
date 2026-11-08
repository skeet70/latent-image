{-# LANGUAGE LambdaCase #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Stages the owned global-lock extents into a pinned X-M1 1.50 DAT.
module Main (main) where

import qualified Data.Aeson as J
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as L
import qualified Data.Text as T
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.Host as H
import Replacement.RtosGlobalLock.Stage
import Replacement.RtosOwnFirst.Regions (Domain (..))
import System.Directory (createDirectory, doesPathExist)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))

flags :: [String]
flags = ["--owned-a-bin", "--owned-a-symbols", "--owned-b-bin", "--owned-b-symbols"]

main :: IO ()
main = do
  arguments <- getArgs
  ownedABin <- readInput arguments "--owned-a-bin"
  ownedASymbols <- readInput arguments "--owned-a-symbols"
  ownedBBin <- readInput arguments "--owned-b-bin"
  ownedBSymbols <- readInput arguments "--owned-b-symbols"
  let linkedA = Linked ownedABin ownedASymbols
      linkedB = Linked ownedBBin ownedBSymbols
  case positional arguments of
    ["controls", datPath, loaderAPath, loaderBPath] -> controlMode linkedA linkedB datPath loaderAPath loaderBPath
    ["compose", expected, datPath, loaderAPath, loaderBPath, output] -> composeMode linkedA linkedB expected datPath loaderAPath loaderBPath output
    ["plan", basePlan, candidatePath, loaderAPath, loaderBPath, output] -> planMode basePlan candidatePath loaderAPath loaderBPath output
    [datPath, loaderAPath, loaderBPath, output] -> stageMode linkedA linkedB datPath loaderAPath loaderBPath output
    _ ->
      H.failH
        "usage: stage-global-lock [Bazel-supplied inputs] (SOURCE_DAT LOADER_A LOADER_B NEW_OUTPUT_DIRECTORY | controls SOURCE_DAT LOADER_A LOADER_B | compose BASE_SHA256 SOURCE_DAT LOADER_A LOADER_B NEW_OUTPUT_DIRECTORY | plan BASE_PLAN CANDIDATE_DAT LOADER_A LOADER_B NEW_PLAN)"

stageMode :: Linked -> Linked -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
stageMode linkedA linkedB datPath loaderAPath loaderBPath output = do
  source <- readSource datPath loaderAPath loaderBPath
  either (H.failH . show) pure (requirePinned source)
  writeStaged linkedA linkedB source output

composeMode :: Linked -> Linked -> String -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
composeMode linkedA linkedB expected datPath loaderAPath loaderBPath output = do
  dat <- H.readRegularWith (pure ()) False (64 * 1024 * 1024) datPath
  H.ensure (digest dat == expected) (datPath ++ " is not the expected base")
  loaderA <- H.readRegularWith (pure ()) False 0x20000 loaderAPath
  loaderB <- H.readRegularWith (pure ()) False 0x20000 loaderBPath
  source <- either (H.failH . show) pure (canonicalSource dat loaderA loaderB)
  writeStaged linkedA linkedB source output

writeStaged :: Linked -> Linked -> Source -> FilePath -> IO ()
writeStaged linkedA linkedB source output = do
  admitted <- either (H.failH . show) pure (admit source linkedA linkedB)
  staged <- either (H.failH . show) pure (stage source admitted)
  exists <- doesPathExist output
  H.ensure (not exists) (output ++ " already exists")
  createDirectory output
  B.writeFile (output </> "candidate.DAT") (stagedDat staged)
  B.writeFile (output </> "loader-a.bin") (stagedLoader staged DomainA)
  B.writeFile (output </> "loader-b.bin") (stagedLoader staged DomainB)
  L.writeFile (output </> "contract.json") (J.encode (contractValue staged) <> L.singleton 10)
  putStrLn ("staged " ++ output)

controlMode :: Linked -> Linked -> FilePath -> FilePath -> FilePath -> IO ()
controlMode linkedA linkedB datPath loaderAPath loaderBPath = do
  source <- readSource datPath loaderAPath loaderBPath
  swapped <- readSwapped datPath loaderAPath loaderBPath
  let Linked bin symbols = linkedA
      gapped = B.take 0x54 bin <> B.singleton 1 <> B.drop 0x55 bin
      moved = moveSymbol symbols "xm1_rtos_global_lock_try" 0x412d0
      extra = symbols <> "00050000 00000004 T extra\n"
      controls =
        [ ("baseline-stage", isRight (admit source linkedA linkedB >>= stage source))
        , ("gap-byte", refuses (\case OwnedByteOutsideExtents DomainA _ -> True; _ -> False) (admit source (Linked gapped symbols) linkedB))
        , ("moved-symbol", refuses (\case EntryNotAtStart DomainA _ -> True; SymbolMissing DomainA _ -> True; _ -> False) (admit source (Linked bin moved) linkedB))
        , ("symbol-outside", refuses (\case SymbolOutsideExtents DomainA _ -> True; _ -> False) (admit source (Linked bin extra) linkedB))
        , ("swapped-loaders", refuses (\case LoaderMismatch _ -> True; _ -> False) (swapped >>= \s -> admit s linkedA linkedB))
        ]
  mapM_ (\(name, holds) -> putStrLn (name ++ ": " ++ if holds then "ok" else "FAILED")) controls
  if all snd controls then pure () else exitFailure
  where
    refuses predicate result = case result of Left err -> predicate err; Right _ -> False

planMode :: FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
planMode basePlanPath candidatePath loaderAPath loaderBPath output = do
  base <- either (H.failH . ("base plan: " ++)) pure . J.eitherDecodeStrict' =<< B.readFile basePlanPath
  dat <- B.readFile candidatePath
  loaderA <- B.readFile loaderAPath
  loaderB <- B.readFile loaderBPath
  let starts = [hexText start | domain <- [DomainA, DomainB], (start, _) <- extentTable domain]
      guards =
        [ J.object
            [ "domain" J..= label domain
            , "start" J..= hexText start
            , "end" J..= hexText end
            , "sha256" J..= digest (slice domain start end loaderA loaderB)
            ]
        | domain <- [DomainA, DomainB]
        , (start, end) <- extentTable domain
        ]
      existing = case base of
        J.Object o -> case KM.lookup "guarded" o of
          Just (J.Array values) -> foldr (:) [] values
          _ -> []
        _ -> []
      updated = case base of
        J.Object o ->
          J.Object
            ( KM.insert "dat_sha256" (J.String (T.pack (digest dat)))
                (KM.insert "guarded" (J.toJSON (filter (not . (`elem` starts) . startOf) existing ++ guards)) o)
            )
        _ -> base
  H.ensure (not (any ((`elem` starts) . startOf) existing)) "base plan already guards a global-lock extent"
  L.writeFile output (J.encode updated <> L.singleton 10)
  putStrLn ("plan " ++ output)
  where
    startOf (J.Object o) = case KM.lookup "start" o of
      Just (J.String text) -> T.unpack text
      _ -> ""
    startOf _ = ""

slice :: Domain -> Word32 -> Word32 -> B.ByteString -> B.ByteString -> B.ByteString
slice domain start end loaderA loaderB =
  let loader = if domain == DomainA then loaderA else loaderB
      base = if domain == DomainA then 0x40000 else 0xd0000
   in B.take (fromIntegral (end - start)) (B.drop (fromIntegral (start - base)) loader)

label :: Domain -> String
label DomainA = "a"
label DomainB = "b"

digest :: B.ByteString -> String
digest = SHA.sha256Hex . SHA.sha256

hexText :: Word32 -> String
hexText value = "0x" ++ showHexWord value

readSource :: FilePath -> FilePath -> FilePath -> IO Source
readSource datPath loaderAPath loaderBPath = do
  dat <- H.readRegularWith (pure ()) False (64 * 1024 * 1024) datPath
  loaderA <- H.readRegularWith (pure ()) False 0x20000 loaderAPath
  loaderB <- H.readRegularWith (pure ()) False 0x20000 loaderBPath
  either (H.failH . show) pure (canonicalSource dat loaderA loaderB)

readSwapped :: FilePath -> FilePath -> FilePath -> IO (Either StageError Source)
readSwapped datPath loaderAPath loaderBPath = do
  dat <- H.readRegularWith (pure ()) False (64 * 1024 * 1024) datPath
  loaderA <- H.readRegularWith (pure ()) False 0x20000 loaderAPath
  loaderB <- H.readRegularWith (pure ()) False 0x20000 loaderBPath
  pure (canonicalSource dat loaderB loaderA)

moveSymbol :: B.ByteString -> String -> Word32 -> B.ByteString
moveSymbol symbols name address =
  C.unlines
    [ if C.unpack (C.drop 20 line) == name then C.pack (showHexWord address) <> C.drop 8 line else line
    | line <- C.lines symbols
    ]

isRight :: Either a b -> Bool
isRight (Right _) = True
isRight (Left _) = False

readInput :: [String] -> String -> IO B.ByteString
readInput arguments flag = case dropWhile (/= flag) arguments of
  [] -> H.failH (flag ++ " is required")
  [_] -> H.failH (flag ++ " needs a value")
  _ : value : _ -> B.readFile value

positional :: [String] -> [String]
positional (flag : _ : rest) | flag `elem` flags = positional rest
positional rest = rest

showHexWord :: Word32 -> String
showHexWord value = pad (go value "")
  where
    go 0 acc = if null acc then "0" else acc
    go n acc = go (n `div` 16) (("0123456789abcdef" !! fromIntegral (n `mod` 16)) : acc)
    pad text = replicate (8 - length text) '0' ++ text
