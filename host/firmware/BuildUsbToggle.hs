{-# LANGUAGE LambdaCase #-}
-- Build a USB personality toggle extension (reports/usb-personality-toggle.md
-- section 6) on a base extension manifest: the base's image-5 write set, if
-- any, merged with the toggle's stub blob and hook word into one image-5 patch
-- set; every other base part copied unchanged. Current bytes are checked
-- against the pinned stock image 5 before anything is written.
module Main (main) where

import Control.Monad (forM, forM_, when)
import Data.Aeson (Value (Null))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Char (isDigit, isHexDigit, isLower)
import Data.List (isPrefixOf, nub, sort)
import Data.Time.Clock (getCurrentTime)
import Data.Time.Format (defaultTimeLocale, formatTime)
import Data.Word (Word32)
import qualified FujiAF.Host as H
import FujiAF.JSON (encodeCanonical)
import FujiAF.UsbToggle
import Numeric (readHex, showHex)
import System.Directory (canonicalizePath, createDirectory, doesPathExist, getCurrentDirectory)
import System.Environment (getArgs, lookupEnv)
import System.FilePath (addTrailingPathSeparator, isAbsolute, takeDirectory, takeFileName, (</>))

stockFirmware :: String
stockFirmware = "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"

stockId5 :: FilePath
stockId5 = "private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin"

usage :: String
usage = "usage: build-usb-toggle [--output-sha256 HEX] BASE_EXTENSION_JSON NEW_OUTPUT_DIRECTORY"

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    stub : symbols : rest -> case rest of
      ["--output-sha256", digest, b, o] -> do
        H.ensure (isSha256 digest) "--output-sha256: lowercase 64-digit SHA-256 required"
        build stub symbols digest b o
      [b, o] -> build stub symbols (replicate 64 '0') b o
      _ -> H.failH usage
    _ -> H.failH usage

data Part = Part {partStep :: Value, partSegment :: Maybe Integer, partFile :: Maybe (FilePath, B.ByteString)}

build :: FilePath -> FilePath -> String -> FilePath -> FilePath -> IO ()
build stubPath symbolsPath outputSha baseGiven outputGiven = do
  stub <- canonicalizePath stubPath >>= H.readRegular 4096
  symbolText <- canonicalizePath symbolsPath >>= H.readRegular (64 * 1024)
  H.ensure (lookup "usb_toggle" (parseSymbols (BC.unpack symbolText)) == Just extentStart) "symbol usb_toggle is not at 0x00bf4454"
  let callees = stubBlTargets stub
  H.ensure (not (null callees) && all (`elem` stubCallees) callees && sort (nub callees) == sort stubCallees) "stub BL targets differ from the four stock callees"
  id5 <- resolve stockId5 >>= canonicalizePath >>= H.readRegular (16 * 1024 * 1024)
  id5Sha <- H.sha256 id5
  H.ensure (id5Sha == image5Sha256) (stockId5 ++ " differs from its pinned SHA-256")
  let at a n = B.take n (B.drop (fromIntegral (a - image5Base)) id5)
      extent = at extentStart extentLength
  extentDigest <- H.sha256 extent
  H.ensure (extentDigest == extentSha256) "stock extent [0x00bf4454, 0x00bf44e4) differs from its pinned SHA-256"
  H.ensure (at hookSite 4 == le32 hookCurrent) "stock word at 0x00ecf900 is not bl 0x0069f8fc"
  toggle <- either H.failH pure (toggleWrites stub extent)

  basePath <- resolve baseGiven >>= canonicalizePath
  base <- H.readRegular (1024 * 1024) basePath >>= H.jsonRead
  H.fields base "schema_version id title base steps output_sha256" "base extension"
  H.ensure (H.num (H.get base "schema_version") == 1) "base extension schema_version must be 1"
  H.ensure (H.str (H.get (H.get base "base") "sha256") == stockFirmware) "base extension is not built on stock 1.50"
  parts <- forM (H.list (H.get base "steps")) (readPart (takeDirectory basePath))
  baseImage5 <- case [p | p <- parts, partSegment p == Just 5] of
    [] -> pure []
    [Part _ _ (Just (_, bytes))] -> H.jsonRead bytes >>= image5Writes
    _ -> H.failH "base extension has more than one image-5 patch set"
  merged <- either H.failH pure (mergeWrites baseImage5 toggle)
  forM_ merged $ \w -> H.ensure (at (wAddress w) (B.length (wCurrent w)) == wCurrent w) (wId w ++ ": current bytes differ from stock image 5")

  output <- newOutputDirectory outputGiven
  recorded <- formatTime defaultTimeLocale "%Y-%m-%dT%H:%M:%SZ" <$> getCurrentTime
  let baseId = H.str (H.get base "id")
      image5Name = "usb-toggle-image5.json"
      image5Bytes = encodeCanonical (patchSet recorded (baseId ++ "-USB-TOGGLE-IMAGE5") merged) <> BC.pack "\n"
  B.writeFile (output </> image5Name) image5Bytes
  image5Digest <- H.sha256 image5Bytes
  copied <- forM [p | p <- parts, partSegment p /= Just 5] $ \p -> case partFile p of
    Nothing -> pure (partSegment p, partStep p)
    Just (name, bytes) -> do
      let local = takeFileName name
      exists <- doesPathExist (output </> local)
      when exists (H.failH ("two base parts share the file name " ++ local))
      B.writeFile (output </> local) bytes
      pure (partSegment p, H.set "path" (H.s local) (partStep p))
  let image5Step = H.obj [("kind", H.s "image-patch-set"), ("path", H.s image5Name), ("sha256", H.s image5Digest)]
      steps = [s | (Just 4, s) <- copied] ++ [image5Step] ++ [s | (g, s) <- copied, g /= Just 4]
      extension =
        H.obj
          [ ("schema_version", H.n 1)
          , ("id", H.s (baseId ++ "-USB-TOGGLE"))
          , ("title", H.s (H.str (H.get base "title") ++ ", with the USB personality toggle"))
          , ("base", H.get base "base")
          , ("steps", H.arr steps)
          , ("output_sha256", H.s outputSha)
          ]
      manifestName = "usb-toggle.json"
  B.writeFile (output </> manifestName) (encodeCanonical extension <> BC.pack "\n")
  writeListing output baseId merged
  putStrLn (image5Name ++ " sha256 " ++ image5Digest)
  forM_ [(g, length [w | w <- merged, wGroup w == g], sum [differing w | w <- merged, wGroup w == g]) | g <- nub (map wGroup merged)] $ \(g, n, c) ->
    putStrLn ("image5 " ++ g ++ ": " ++ show n ++ " writes, " ++ show c ++ " changed bytes")
  putStrLn ("wrote " ++ (output </> manifestName))

readPart :: FilePath -> Value -> IO Part
readPart dir step = case H.get step "path" of
  Null -> pure (Part step Nothing Nothing)
  _ -> do
    let path = H.str (H.get step "path")
    bytes <- canonicalizePath (if isAbsolute path then path else dir </> path) >>= H.readRegular (16 * 1024 * 1024)
    digest <- H.sha256 bytes
    H.ensure (digest == H.str (H.get step "sha256")) ("base part " ++ path ++ " differs from its step SHA-256")
    segment <-
      if H.str (H.get step "kind") == "image-patch-set"
        then (\v -> Just (H.num (H.get (H.get v "image") "segment_id"))) <$> H.jsonRead bytes
        else pure Nothing
    pure (Part step segment (Just (path, bytes)))

image5Writes :: Value -> IO [Write]
image5Writes v = do
  let image = H.get v "image"
  H.ensure (H.num (H.get image "segment_id") == 5 && H.str (H.get image "decode_base") == "0x00bf1a00") "base image-5 patch set is not segment 5 at 0x00bf1a00"
  H.ensure (H.str (H.get image "decode_sha256") == image5Sha256) "base image-5 patch set names another decode"
  H.ensure (H.get image "output_base" == Null && H.get v "extension" == Null) "base image-5 patch set moves the load"
  blobs <- forM (H.list (H.get v "blobs")) $ \b -> do
    a <- hexWord (H.str (H.get b "address"))
    c <- unhex (H.str (H.get b "current_hex"))
    n <- unhex (H.str (H.get b "new_hex"))
    H.ensure (B.length c == B.length n && B.length c `mod` 4 == 0) "base blob lengths differ"
    pure (Write BlobWrite (H.str (H.get b "id")) (group (H.str (H.get b "role"))) (role (H.str (H.get b "role"))) a c n)
  ws <- forM (H.list (H.get v "words")) $ \w -> do
    a <- hexWord (H.str (H.get w "address"))
    c <- hexWord (H.str (H.get w "current"))
    n <- hexWord (H.str (H.get w "new"))
    pure (Write WordWrite ("word-" ++ hex32 a) (group (H.str (H.get w "role"))) (role (H.str (H.get w "role"))) a (le32 c) (le32 n))
  pure (blobs ++ ws)
  where
    group r = case break (== ':') r of (g, ':' : _) -> g; _ -> "BASE"
    role r = case break (== ':') r of (_, ':' : ' ' : rest) -> rest; _ -> r

patchSet :: String -> String -> [Write] -> Value
patchSet recorded identifier ws =
  H.obj
    [ ("schema_version", H.n 3)
    , ("id", H.s identifier)
    , ("tier", H.s "USB-TOGGLE")
    , ("recorded_at", H.s recorded)
    , ("target", H.obj [("model", H.s "X-M1"), ("firmware_version", H.s "1.50"), ("firmware_sha256", H.s stockFirmware)])
    , ( "image"
      , H.obj
          [ ("segment_id", H.n 5)
          , ("decode_base", H.s (hex32 image5Base))
          , ("decode_file", H.s stockId5)
          , ("decode_sha256", H.s image5Sha256)
          , ("file_offset_rule", H.s "output file_offset = address - 0x00bf1a00; stock source offset = address - 0x00bf1a00")
          , ("dump_correspondence", H.s "not established for this build")
          ]
      )
    , ( "free_space"
      , H.obj
          [ ("chosen_region", H.arr [H.s (hex32 extentStart), H.s (hex32 (extentStart + fromIntegral extentLength))])
          , ("stock_extent_sha256", H.s extentSha256)
          , ("ownership_evidence", H.s "reports/usb-personality-toggle.md section 5.2")
          ]
      )
    , ("array_source", Null)
    , ("order_rule", H.s "Address order; every write is in place.")
    , ("blobs", H.arr [blobJson w first | (w, first) <- numbered, wKind w == BlobWrite])
    , ("words", H.arr [wordJson w first | (w, first) <- numbered, wKind w == WordWrite])
    , ( "totals"
      , H.obj
          [ ("blob_words", H.n (toInteger (sum [B.length (wNew w) `div` 4 | w <- ws, wKind w == BlobWrite])))
          , ("word_writes", H.n (toInteger (length [w | w <- ws, wKind w == WordWrite])))
          , ("words", H.n (toInteger total))
          , ("bytes_written", H.n (toInteger (4 * total)))
          , ("prefix_bytes_added", H.n 0)
          , ("changed_bytes", H.n (toInteger (sum (map differing ws))))
          ]
      )
    , ("unchanged_surfaces", H.arr [H.s "stored adjustment byte 0xf7", H.s "boot USB key 0x0073ae10", H.s "handler B 0x00ecf9c8"])
    , ("container_effect", H.obj [("load", H.s (hex32 image5Base)), ("decoded_size", H.s "unchanged"), ("dat_extents", H.s "unchanged")])
    , ("emulation_case_parameters", Null)
    ]
  where
    total = sum [B.length (wNew w) `div` 4 | w <- ws]
    numbered = zip ws (scanl (+) 1 [B.length (wNew w) `div` 4 | w <- ws])
    offset a = hex32 (a - image5Base)
    blobJson w first =
      H.obj
        [ ("id", H.s (wId w))
        , ("role", H.s (wGroup w ++ ": " ++ wRole w))
        , ("address", H.s (hex32 (wAddress w)))
        , ("file_offset", H.s (offset (wAddress w)))
        , ("length", H.n (toInteger (B.length (wNew w))))
        , ("order_range", H.arr [H.n (toInteger first), H.n (toInteger (first + B.length (wNew w) `div` 4 - 1))])
        , ("current_hex", H.s (hexBytes (wCurrent w)))
        , ("new_hex", H.s (hexBytes (wNew w)))
        ]
    wordJson w order =
      H.obj
        [ ("order", H.n (toInteger order))
        , ("address", H.s (hex32 (wAddress w)))
        , ("file_offset", H.s (offset (wAddress w)))
        , ("current", H.s (hex32 (word32 (wCurrent w))))
        , ("new", H.s (hex32 (word32 (wNew w))))
        , ("role", H.s (wGroup w ++ ": " ++ wRole w))
        ]

writeListing :: FilePath -> String -> [Write] -> IO ()
writeListing output baseId ws = do
  let groups = nub (map wGroup ws)
      counts = [(g, length m, sum (map differing m)) | g <- groups, let m = [w | w <- ws, wGroup w == g]]
      row w = H.obj [("image", H.s "image5"), ("address", H.s (hex32 (wAddress w))), ("current", H.s (hexBytes (wCurrent w))), ("new", H.s (hexBytes (wNew w))), ("group", H.s (wGroup w)), ("role", H.s (wRole w))]
  B.writeFile (output </> "changed-words.json") $
    encodeCanonical (H.obj [("base", H.s baseId), ("counts", H.arr [H.obj [("image", H.s "image5"), ("group", H.s g), ("writes", H.n (toInteger n)), ("changed_bytes", H.n (toInteger c))] | (g, n, c) <- counts]), ("writes", H.arr (map row ws))]) <> BC.pack "\n"
  writeFile (output </> "changed-words.md") $
    unlines $
      ["# USB toggle image-5 changed words", "", "Base `" ++ baseId ++ "`.", "", "| Group | Writes | Changed bytes |", "| --- | ---: | ---: |"]
        ++ ["| " ++ g ++ " | " ++ show n ++ " | " ++ show c ++ " |" | (g, n, c) <- counts]
        ++ ["", "| Address | Current | New | Group | Role |", "| --- | --- | --- | --- | --- |"]
        ++ ["| `" ++ hex32 (wAddress w) ++ "` | `" ++ hexBytes (wCurrent w) ++ "` | `" ++ hexBytes (wNew w) ++ "` | " ++ wGroup w ++ " | " ++ wRole w ++ " |" | w <- ws]

parseSymbols :: String -> [(String, Word32)]
parseSymbols text = [(name, fromInteger v) | [address, _, name] <- map words (lines text), [(v, "")] <- [readHex address]]

differing :: Write -> Int
differing w = length (filter id (B.zipWith (/=) (wCurrent w) (wNew w)))

hexWord :: String -> IO Word32
hexWord = \case
  '0' : 'x' : digits | not (null digits), [(v, "")] <- readHex digits, v <= (0xffffffff :: Integer) -> pure (fromInteger v)
  other -> H.failH ("hex word expected, got " ++ show other)

unhex :: String -> IO B.ByteString
unhex text = B.pack <$> go text
  where
    go (a : b : rest) | [(v, "")] <- readHex [a, b] = (fromInteger v :) <$> go rest
    go [] = pure []
    go _ = H.failH "malformed hex blob"

word32 :: B.ByteString -> Word32
word32 b = sum [fromIntegral (B.index b i) * 256 ^ i | i <- [0 .. 3]]

le32 :: Word32 -> B.ByteString
le32 w = B.pack [fromIntegral ((w `div` (256 ^ i)) `mod` 256) | i <- [0 .. 3 :: Int]]

resolve :: FilePath -> IO FilePath
resolve path = do
  root <- lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= maybe getCurrentDirectory pure
  pure (if isAbsolute path then path else root </> path)

newOutputDirectory :: FilePath -> IO FilePath
newOutputDirectory given = do
  output <- resolve given
  root <- resolve "private-data" >>= canonicalizePath
  parent <- canonicalizePath (takeDirectory output)
  H.ensure (addTrailingPathSeparator root `isPrefixOf` addTrailingPathSeparator parent) "Output must be a new directory beneath private-data"
  H.ensure (takeFileName output `notElem` ["", ".", ".."]) "Output must name a new directory"
  exists <- doesPathExist output
  when exists (H.failH "Output directory already exists")
  createDirectory output
  pure output

isSha256 :: String -> Bool
isSha256 s = length s == 64 && all (\c -> isDigit c || (isHexDigit c && isLower c)) s

hex32 :: Word32 -> String
hex32 w = let digits = showHex w "" in "0x" ++ replicate (8 - length digits) '0' ++ digits

hexBytes :: B.ByteString -> String
hexBytes = concatMap (\x -> let d = showHex x "" in replicate (2 - length d) '0' ++ d) . B.unpack
