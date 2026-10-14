-- Synthetic round trips and owner-input stock reproduction use separate targets.
-- Stock mode requires pinned inputs and compares the emitted record byte for byte.
module Main where

import Control.Monad (forM, forM_, unless)
import Data.Bits (shiftR)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified FujiAF.Host as H
import qualified FujiAF.Lz.Encode as Encode
import qualified FujiAF.Lz.Format as Format
import Data.List (find)
import Data.Word (Word32, Word64, Word8)
import System.Directory (canonicalizePath, doesFileExist)
import System.Environment (getArgs)
import System.Exit (ExitCode (..))
import System.FilePath ((</>), isRelative, takeDirectory)

-- One pinned image and the shipped record that carries it, as an absolute
-- path already resolved through its symlinks.
data Pinned = Pinned
  { pinnedImage :: FilePath
  , pinnedPayload :: FilePath
  , pinnedOffset :: Int
  , pinnedWindow :: Word32
  }

data TestMode = Synthetic | Stock

main :: IO ()
main = do
  arguments <- getArgs
  let (mode, remaining) = case arguments of
        "--synthetic" : rest -> (Synthetic, rest)
        _ -> (Stock, arguments)
  (decoder, manifest, explicit) <- case collect remaining of
    (Just path, manifest', pinned') -> pure (path, manifest', pinned')
    _ -> H.failH "usage: lz-round-trip [--synthetic] --decoder FFLZD [--pinned-manifest MANIFEST | --pinned IMAGE PAYLOAD OFFSET WINDOW]..."
  -- Explicit inputs override the target's default manifest.
  pinned <- case (mode, manifest, explicit) of
    (Synthetic, Nothing, []) -> pure []
    (Synthetic, _, _) -> H.failH "synthetic mode does not accept pinned stock inputs"
    (Stock, _, cases@(_ : _)) -> mapM (resolvePinned ".") cases
    (Stock, Just path, []) -> manifestPinned path
    (Stock, Nothing, []) -> H.failH "stock mode requires --pinned-manifest MANIFEST or --pinned IMAGE PAYLOAD OFFSET WINDOW"
  H.withTemporary "fuji-af-lz-roundtrip-" $ \root -> do
    forM_ (zip [0 :: Int ..] corpus) $ \(index, (name, image)) -> do
      record <- encode name corpusWindow image
      checkStructure name image record
      decoded <- decodeRecord decoder root ("corpus-" ++ show index) record
      H.ensure (decoded == image) (name ++ ": decoder output differs from the input")
      putStrLn (name ++ ": round trip " ++ show (B.length image) ++ " bytes")
    forM_ (zip [0 :: Int ..] geometryFamily) $ \(index, (name, image)) -> do
      record <- encode name corpusWindow image
      checkStructure name image record
      decoded <- decodeRecord decoder root ("geometry-" ++ show index) record
      H.ensure (decoded == image) (name ++ ": decoder output differs from the input")
    putStrLn ("geometry sweep: " ++ show (length geometryFamily) ++ " cases round trip")
    feedAlignedSweep
    windowRefusals
    forM_ (zip [0 :: Int ..] pinned) $ \(index, Pinned imagePath payloadPath offset window) -> do
      image <- H.readRegular (16 * 1024 * 1024) imagePath
      payload <- H.readRegular (64 * 1024 * 1024) payloadPath
      stock <- stockRecord payloadPath payload offset
      mine <- encode imagePath window image
      H.ensure (null (Format.verifyRecord mine)) (imagePath ++ ": shipped record fails verifyRecord: " ++ unwords (Format.verifyRecord mine))
      H.ensure (mine == stock) (imagePath ++ ": encoder differs from the stock record at byte " ++ firstDifference mine stock)
      stockImage <- decodeAt decoder root ("stock-" ++ show index) payloadPath offset (B.length image)
      H.ensure (stockImage == image) (imagePath ++ ": the stock record does not decode to the pinned image")
      mineImage <- decodeRecord decoder root ("mined-" ++ show index) mine
      H.ensure (mineImage == image) (imagePath ++ ": the re-encoded record does not decode to the pinned image")
      putStrLn
        ( imagePath
            ++ ": stock record reproduced byte for byte inside its window ("
            ++ show (100 * B.length stock `div` fromIntegral window)
            ++ " percent used), both records decode to the image"
        )
    let pinnedCount = length pinned
    unless (null pinned) $ putStrLn
      ( "pinned stock images: "
          ++ show pinnedCount
          ++ (if pinnedCount == 1 then " stock record" else " stock records")
          ++ " reproduced byte for byte and decoded back"
      )
  putStrLn $ case mode of
    Synthetic -> "lz-synthetic-round-trip passed"
    Stock -> "lz-round-trip passed"

encode :: String -> Word32 -> B.ByteString -> IO B.ByteString
encode name window image = case Encode.encodeRecord window image of
  Left Encode.EmptyImage -> H.failH (name ++ ": empty image")
  Left Encode.ImageTooLarge -> H.failH (name ++ ": image too large")
  Left Encode.WindowExceeded -> H.failH (name ++ ": record exceeds its flash window")
  Left Encode.FeedAlignedTotalLen -> H.failH (name ++ ": total_len lands on a feed boundary")
  Left Encode.InvalidTokenization -> H.failH (name ++ ": encoder produced an invalid tokenization")
  Right record -> pure record

-- The stock record: the header's total_len bytes at the payload offset.
stockRecord :: FilePath -> B.ByteString -> Int -> IO B.ByteString
stockRecord payloadPath payload offset = do
  let slice = B.drop offset payload
  header <- maybe (H.failH (payloadPath ++ ": no header at the offset")) pure (Format.parseHeader slice)
  let len = fromIntegral (Format.headerTotalLen header)
  H.ensure (len >= Format.headerSize && len <= B.length slice) (payloadPath ++ ": total_len leaves the file")
  pure (B.take len slice)

decodeAt :: FilePath -> FilePath -> String -> FilePath -> Int -> Int -> IO B.ByteString
decodeAt decoder root name payloadPath offset expected = do
  let outPath = root </> (name ++ ".out")
  (stopped, status, _, errors) <-
    H.runBounded 20000000 (1024 * 1024) decoder [payloadPath, show offset, outPath, show expected] B.empty
  H.ensure (not stopped && status == ExitSuccess) (name ++ ": fflzd rejected the record: " ++ C.unpack errors)
  B.readFile outPath

decodeRecord :: FilePath -> FilePath -> String -> B.ByteString -> IO B.ByteString
decodeRecord decoder root name record = do
  let recordPath = root </> (name ++ ".record")
  B.writeFile recordPath record
  decodeAt decoder root name recordPath 0 (outSizeOf record)

outSizeOf :: B.ByteString -> Int
outSizeOf record = case Format.parseHeader record of
  Just header -> fromIntegral (Format.headerOutSize header)
  Nothing -> 0

firstDifference :: B.ByteString -> B.ByteString -> String
firstDifference mine stock = case find (\i -> byteAt i mine /= byteAt i stock) [0 .. maxLen - 1] of
  Just i -> show i ++ " (mine 0x" ++ hex (byteAt i mine) ++ ", stock 0x" ++ hex (byteAt i stock) ++ ")"
  Nothing -> "lengths " ++ show (B.length mine) ++ " and " ++ show (B.length stock)
  where
    maxLen = max (B.length mine) (B.length stock)
    byteAt i bytes = if i < B.length bytes then B.index bytes i else 0
    hex w = let s = showHexInt (fromIntegral w) in if length s == 1 then '0' : s else s

showHexInt :: Int -> String
showHexInt n
  | n < 16 = [digit n]
  | otherwise = showHexInt (n `div` 16) ++ [digit (n `mod` 16)]
  where
    digit d = if d < 10 then toEnum (fromEnum '0' + d) else toEnum (fromEnum 'a' + d - 10)

-- The emitted record must satisfy the format contract and the stock token
-- invariants: header arithmetic including total_len off the feed grid,
-- contiguous legal tokens, every token inside one feed page, every page
-- boundary a token end, matches with 3 <= dist <= 2047, 1 <= len <= dist,
-- and a short match only as a page-edge tail.
checkStructure :: String -> B.ByteString -> B.ByteString -> IO ()
checkStructure name image record = do
  H.ensure (null (Format.verifyRecord record)) (name ++ ": verifyRecord: " ++ unwords (Format.verifyRecord record))
  header <- maybe (H.failH (name ++ ": unparseable header")) pure (Format.parseHeader record)
  let stream = B.drop Format.headerSize record
      totalLen = fromIntegral (Format.headerTotalLen header)
      chunk = fromIntegral (Format.headerChunk header)
      pages = fromIntegral (Format.headerPages header)
  H.ensure (fromIntegral (Format.headerOutSize header) == B.length image) (name ++ ": out_size is not the image length")
  H.ensure (chunk == 0x4000) (name ++ ": chunk word is not 0x4000")
  H.ensure (Format.headerMode header == 4) (name ++ ": mode word is not 4")
  H.ensure (totalLen == Format.headerSize + B.length stream) (name ++ ": total_len is not the record length")
  H.ensure (pages == (totalLen + chunk - 1) `div` chunk) (name ++ ": page count is not ceil(total_len/chunk)")
  H.ensure (totalLen `mod` chunk /= 0) (name ++ ": total_len lands on a feed boundary")
  tokens <- either (H.failH . ((name ++ ": stream rejected: ") ++)) pure (Format.tokenize stream)
  let spans = walk Format.headerSize [t | (_, t) <- tokens]
      walk pos (t : ts) = (pos, t, tokenLen t) : walk (pos + tokenLen t) ts
      walk _ [] = []
      bounds = Format.pageBoundaries header
  forM_ spans $ \(start, token, len) -> do
    case token of
      Format.LiteralRun lenWord bytes -> do
        H.ensure (B.length bytes == fromIntegral lenWord && lenWord >= 1 && lenWord <= 0x7f) (name ++ ": bad literal run")
        H.ensure (bytes == B.take (fromIntegral lenWord) (B.drop (start + 1 - Format.headerSize) stream)) (name ++ ": literal bytes differ from the stream")
      Format.Match distance matchLen -> do
        let d = fromIntegral (Format.distanceValue distance) :: Int
            l = fromIntegral (Format.matchLenValue matchLen) :: Int
        H.ensure (d >= 3 && l >= 1 && l <= 15 && l <= d) (name ++ ": match outside the stock domain")
        unless
          (l >= 3 || any (\bound -> start == bound || start + len == bound) bounds)
          (H.failH (name ++ ": short match away from a feed boundary"))
    forM_ bounds $ \bound ->
      H.ensure (not (start < bound && bound < start + len)) (name ++ ": token crosses feed boundary " ++ show bound)
  forM_ bounds $ \bound ->
    H.ensure (any (\(start, _, len) -> start + len == bound) spans) (name ++ ": no token ends at feed boundary " ++ show bound)
  where
    tokenLen t = case t of
      Format.LiteralRun len _ -> 1 + fromIntegral len
      Format.Match _ _ -> 2

-- The F1 regression family: a short noise prefix then a long match-free
-- region, with the prefix length sweeping the run starts across the feed-page
-- geometry. Every case must round trip through the decoder.
geometryFamily :: [(String, B.ByteString)]
geometryFamily =
  [ ("geometry-" ++ show prefixLen, noise (fromIntegral (prefixLen * 3 + 1)) prefixLen <> noise 991 (25000 + prefixLen `mod` 7))
  | prefixLen <- [0 :: Int .. 512]
  ]

-- total_len on a feed boundary is refused rather than emitted. The sweep
-- checks both halves: every accepted record is off the grid, and the refusal
-- arm is reachable.
feedAlignedSweep :: IO ()
feedAlignedSweep = do
  results <- forM [15900 :: Int .. 16900] $ \len -> do
    let image = noise (fromIntegral (len * 5 + 3)) len
    pure (len, Encode.encodeRecord corpusWindow image)
  forM_ results $ \(len, result) -> case result of
    Right record -> do
      let header = case Format.parseHeader record of
            Just h -> h
            Nothing -> error "feed-aligned sweep: unparseable header"
      H.ensure (fromIntegral (Format.headerTotalLen header) `mod` (0x4000 :: Int) /= 0) ("feed-aligned sweep: length " ++ show len ++ " emitted an aligned total_len")
    Left Encode.FeedAlignedTotalLen -> pure ()
    Left other -> H.failH ("feed-aligned sweep: length " ++ show len ++ " failed unexpectedly: " ++ show other)
  let refusals = [len | (len, Left Encode.FeedAlignedTotalLen) <- results]
  unless (not (null refusals)) (H.failH "feed-aligned sweep: the refusal arm was never reached")
  putStrLn ("feed-aligned sweep: " ++ show (length refusals) ++ " lengths refused, the rest emitted off the grid")

windowRefusals :: IO ()
windowRefusals = do
  let image = noise 17 200
  case Encode.encodeRecord 32 image of
    Left Encode.WindowExceeded -> pure ()
    _ -> H.failH "window check: an oversized record was not refused"
  case Encode.encodeRecord 0 image of
    Left Encode.WindowExceeded -> pure ()
    _ -> H.failH "window check: a zero window was not refused"
  putStrLn "window check: oversized records refused"

collect :: [String] -> (Maybe FilePath, Maybe FilePath, [Pinned])
collect ("--decoder" : path : rest) = let (d, m, p) = collect rest in (d `orElse` Just path, m, p)
collect ("--pinned-manifest" : path : rest) = let (d, _, p) = collect rest in (d, Just path, p)
collect ("--pinned" : image : payload : offset : window : rest) =
  let (d, m, p) = collect rest in (d, m, Pinned image payload (read offset) (read window) : p)
collect [] = (Nothing, Nothing, [])
collect _ = error "lz-round-trip: unparsed arguments"

orElse :: Maybe a -> Maybe a -> Maybe a
orElse (Just x) _ = Just x
orElse Nothing y = y

-- The stock images from the checked-in manifest: blank lines and # comments
-- are ignored, every other line is IMAGE PAYLOAD OFFSET WINDOW with
-- workspace-relative paths and decimal or 0x-prefixed hex offsets.
manifestPinned :: FilePath -> IO [Pinned]
manifestPinned path = do
  resolved <- canonicalizePath path
  text <- C.unpack <$> H.readRegular (1024 * 1024) resolved
  root <- workspaceRoot resolved
  entries <- mapM (manifestEntry resolved) (zip [1 :: Int ..] (lines text))
  let pinned = [entry | Just entry <- entries]
  H.ensure (not (null pinned)) (resolved ++ ": lists no pinned stock images")
  mapM (resolvePinned root) pinned

manifestEntry :: FilePath -> (Int, String) -> IO (Maybe Pinned)
manifestEntry path (number, line) = case words (takeWhile (/= '#') line) of
  [] -> pure Nothing
  [image, payload, offset, window] -> case (reads offset, reads window) of
    ([(o, "")], [(w, "")]) -> pure (Just (Pinned image payload o w))
    _ -> H.failH (say number ++ ": OFFSET and WINDOW must be decimal or 0x-prefixed hex")
  _ -> H.failH (say number ++ ": expected IMAGE PAYLOAD OFFSET WINDOW")
  where
    say position = path ++ ":" ++ show position

-- A workspace-relative artifact path, made absolute against root and resolved
-- through its symlinks: payload.bin is a symlink and readRegular refuses one.
resolvePinned :: FilePath -> Pinned -> IO Pinned
resolvePinned root pinned = do
  image <- resolveArtifact (pinnedImage pinned)
  payload <- resolveArtifact (pinnedPayload pinned)
  pure pinned {pinnedImage = image, pinnedPayload = payload}
  where
    resolveArtifact artifact = do
      let path = if isRelative artifact then root </> artifact else artifact
      present <- doesFileExist path
      unless present $
        H.failH
          ( path
              ++ ": pinned stock artifact is absent, so the encoder's comparison against the vendor's own streams cannot run."
              ++ " The default artifacts are the private-data/ paths lz/pinned-stock-images.txt names;"
              ++ " --pinned IMAGE PAYLOAD OFFSET WINDOW names them explicitly instead"
          )
      canonicalizePath path

-- The innermost ancestor of the resolved manifest that holds MODULE.bazel.
workspaceRoot :: FilePath -> IO FilePath
workspaceRoot path = search (takeDirectory path)
  where
    search directory = do
      marker <- doesFileExist (directory </> "MODULE.bazel")
      if marker || takeDirectory directory == directory
        then pure directory
        else search (takeDirectory directory)

newtype Seed = Seed Word64

nextByte :: Seed -> (Word8, Seed)
nextByte (Seed s) =
  let s' = s * 6364136223846793005 + 1442695040888963407
   in (fromIntegral (s' `shiftR` 33), Seed s')

randomBytes :: Word64 -> Int -> [Word8]
randomBytes seed n = take n (unfold (Seed seed))
  where
    unfold s = let (b, s') = nextByte s in b : unfold s'

corpusWindow :: Word32
corpusWindow = 8 * 1024 * 1024

-- A deterministic corpus: constant runs, short-period motifs, sparse fills,
-- repeated blocks, pointer-like words and uniform noise at sizes around the
-- literal-run cap, the window and the feed-page geometry.
corpus :: [(String, B.ByteString)]
corpus =
  [ (name ++ "-" ++ show size, generator (fromIntegral (size * 7 + index)) size)
    | (index, name, generator) <- generators
    , size <- sizes
  ]
  where
    generators =
      [ (1, "const", constant)
      , (2, "periodic", periodic)
      , (3, "sparse", sparse)
      , (4, "blocks", blocks)
      , (5, "words", words4)
      , (6, "noise", noise)
      , (7, "zeros", zeros)
      ]
    sizes = [1, 2, 3, 4, 5, 6, 7, 8, 127, 128, 129, 255, 256, 2047, 2048, 2049, 4095, 4096, 4097, 16381, 16382, 16383, 16384, 16385, 16400, 20000, 50000]

constant :: Word64 -> Int -> B.ByteString
constant seed size = case randomBytes seed 1 of
  baseByte : _ -> B.pack (zipWith mutate [0 :: Int ..] (replicate size baseByte))
  [] -> B.empty
  where
    mutate i b = if i `mod` 211 == 210 then b + 1 else b

periodic :: Word64 -> Int -> B.ByteString
periodic seed size = B.pack (take size (cycle motif))
  where
    motif = take 5 (randomBytes (seed + 1) 5)

sparse :: Word64 -> Int -> B.ByteString
sparse seed size = B.pack (zipWith pick (randomBytes seed size) (randomBytes (seed + 99) size))
  where
    pick r b = if r < 5 then b else 0

blocks :: Word64 -> Int -> B.ByteString
blocks seed size = B.pack (take size (concat (zipWith mutate [0 :: Int ..] (repeat block))))
  where
    block = randomBytes (seed + 7) 191
    mutate i = if i `mod` 3 == 2 then patch (i * 13) else id
    patch j (x : xs) = (x + fromIntegral j) : xs
    patch _ [] = []

words4 :: Word64 -> Int -> B.ByteString
words4 seed size = B.pack (take size (concatMap word (randomBytes (seed + 13) (size `div` 4 + 1))))
  where
    word r = replicate 4 (16 * (r `mod` 16))

noise :: Word64 -> Int -> B.ByteString
noise seed size = B.pack (randomBytes (seed + 29) size)

zeros :: Word64 -> Int -> B.ByteString
zeros _ size = B.replicate size 0
