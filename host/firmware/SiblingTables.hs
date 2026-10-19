{-# LANGUAGE OverloadedStrings #-}
-- The sibling tables behind spec/10 section 19.5.1, extracted from the pinned
-- X-M1 1.50 and X-T1 5.52 DATs, X70 1.10 section 4 and x70view's X70 recipe
-- values: tone records, colour matrices, chroma entries and the X-T1 Classic
-- Chrome selection, with the comparisons the section reports.
module SiblingTables (runTables) where

import Control.Monad (forM, unless, when)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.Types as AT
import Data.Bits (shiftL, xor, (.|.))
import qualified Data.ByteString as B
import Data.List (intercalate)
import qualified Data.Text as T
import Data.Word (Word16)
import Numeric (showFFloat, showHex)
import System.Exit (ExitCode (..))
import System.FilePath ((</>))

import qualified FujiAF.Host as H
import FujiAF.JSON (decodeStrict, encodeCanonical)
import FujiAF.XM1Develop (decodeField)

-- | A byte image mapped at a decode address.
data Image = Image {imageName :: String, imageBase :: Int, imageBytes :: B.ByteString}

hex :: Int -> String
hex n = "0x" ++ showHex n ""

u16At :: Image -> Int -> Either String Int
u16At im a
  | o >= 0 && o + 1 < B.length (imageBytes im) = Right (fromIntegral (B.index b o) .|. (fromIntegral (B.index b (o + 1)) `shiftL` 8))
  | otherwise = Left (imageName im ++ ": read outside the image at " ++ hex a)
 where
  o = a - imageBase im
  b = imageBytes im

u32At :: Image -> Int -> Either String Int
u32At im a = (\lo hi -> lo .|. (hi `shiftL` 16)) <$> u16At im a <*> u16At im (a + 2)

byteAt :: Image -> Int -> Either String Int
byteAt im a
  | o >= 0 && o < B.length (imageBytes im) = Right (fromIntegral (B.index (imageBytes im) o))
  | otherwise = Left (imageName im ++ ": read outside the image at " ++ hex a)
 where
  o = a - imageBase im

slice :: Image -> Int -> Int -> B.ByteString
slice im a n = B.take n (B.drop (a - imageBase im) (imageBytes im))

-- Pinned inputs and decodes -----------------------------------------------------

xm1DatSha, xt1DatSha, x70Section4Sha, x70RecipesSha :: String
xm1DatSha = "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"
xt1DatSha = "4cf56ba6abb4298a844422c016289228bb0acce0a2ae2c6755c05fffb38e92db"
x70Section4Sha = "9923d542f84f07196f0acdae181f0d536578c3f3f0419dba5960d9b055925acb"
x70RecipesSha = "d1fc35516b57528834149501487751738015d151502486a809b90d54f2604191"

-- | A compressed record of the complemented payload: payload offset, output
-- size, decode address and SHA-256 of the decode.
data Record = Record {recordName :: String, recordOffset :: Int, recordSize :: Int, recordBase :: Int, recordSha :: String}

xm1Id4, xm1Id5, xt1Stream0, xt1Stream1 :: Record
xm1Id4 = Record "X-M1 1.50 image 4" 0x160048 0x282800 0x61c400 "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
xm1Id5 = Record "X-M1 1.50 image 5" 0x2c0048 0x4f1e60 0xbf1a00 "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"
xt1Stream0 = Record "X-T1 5.52 stream-0016025c" 0x160048 0x3eca40 0x780400 "38042d1d8756c7269ff6d1b2c9ae31dc19fefd296989aa9d524b8af6ed1b169f"
xt1Stream1 = Record "X-T1 5.52 stream-0038025c" 0x380048 0x6344c0 0xeff120 "5eff6c793a875b9f4f4ac289cebc4dc521ed4d2d6ee554d68963850493bc3b04"

readPinned :: String -> String -> FilePath -> IO B.ByteString
readPinned label sha path = do
  bytes <- H.readRegularWith (pure ()) False (128 * 1024 * 1024) path
  digest <- H.sha256 bytes
  H.ensure (digest == sha) (label ++ " " ++ path ++ " has SHA-256 " ++ digest ++ ", expected " ++ sha)
  pure bytes

-- | DAT bytes from 532 on, complemented.
payloadOf :: B.ByteString -> B.ByteString
payloadOf = B.map (`xor` 0xff) . B.drop 532

decodeRecord :: FilePath -> FilePath -> FilePath -> Record -> IO Image
decodeRecord fflzd root payloadPath r = do
  let out = root </> (map (\c -> if c == ' ' then '-' else c) (recordName r) ++ ".bin")
  (timedOut, code, _, err) <- H.runBounded 120000000 (64 * 1024) fflzd [payloadPath, hex (recordOffset r), out, hex (recordSize r)] B.empty
  H.ensure (not timedOut && code == ExitSuccess) (recordName r ++ ": fflzd refused the record: " ++ show err)
  bytes <- B.readFile out
  digest <- H.sha256 bytes
  H.ensure (digest == recordSha r) (recordName r ++ " decodes to SHA-256 " ++ digest ++ ", expected " ++ recordSha r)
  pure (Image (recordName r) (recordBase r) bytes)

-- | The fallback adjustment image (payload 0xe0048, 0x10000 bytes) with its
-- product string at +0x15c and 0xaa55 at +0x202.
adjustmentImage :: String -> B.ByteString -> B.ByteString -> Either String Image
adjustmentImage model productName payload = do
  let im = Image (model ++ " fallback adjustment image") 0 (B.take 0x10000 (B.drop 0xe0048 payload))
  sig <- u16At im 0x202
  unless (B.length (imageBytes im) == 0x10000 && slice im 0x15c (B.length productName) == productName && sig == 0xaa55)
    (Left (model ++ ": no fallback adjustment image at payload 0xe0048"))
  pure im

-- Records ---------------------------------------------------------------------------

data ToneRecord = ToneRecord {trKey :: Int, trBytes :: B.ByteString}

-- | Tone records of 0x48 bytes after a count word: key u32, nine exponent
-- bytes at +4, 28 u16 points at +14.
toneTable :: Image -> Int -> Int -> Either String [ToneRecord]
toneTable im countAddr recordsAddr = do
  count <- u32At im countAddr
  unless (count > 0 && count <= 256) (Left (imageName im ++ ": tone count " ++ show count ++ " out of range"))
  forM [0 .. count - 1] $ \i -> do
    let a = recordsAddr + 0x48 * i
    key <- u32At im a
    let bytes = slice im a 0x48
    when (B.length bytes /= 0x48) (Left (imageName im ++ ": tone record outside the image"))
    pure (ToneRecord key bytes)

toneJson :: ToneRecord -> Either String Value
toneJson r = do
  let im = Image "tone record" 0 (trBytes r)
  es <- mapM (byteAt im) [4 .. 12]
  ps <- mapM (\k -> u16At im (14 + 2 * k)) [0 .. 27]
  pure (object ["key" .= trKey r, "exponents" .= es, "points" .= ps])

-- | Nine fields of matrix `id` read through the offset table at `table`
-- (18 bytes per id), checked against the fixed layout 0xacc3 + 18 (id - 1).
matrixIds :: Image -> Image -> Int -> [Int] -> Either String [(Int, [Word16])]
matrixIds code adj table ids = forM ids $ \i -> do
  offs <- mapM (\k -> u16At code (table + 18 * (i - 1) + 2 * k)) [0 .. 8]
  unless (offs == [0xacc3 + 18 * (i - 1) + 2 * k | k <- [0 .. 8]]) (Left (imageName code ++ ": matrix " ++ show i ++ " offsets differ from 0xacc3 + 18 (id - 1)"))
  vals <- mapM (u16At adj) offs
  pure (i, map fromIntegral vals)

-- | Four fields of chroma entry `id` through the offset table at `table`
-- (8 bytes per id), in storage order +0, +2, +4, +6.
chromaIds :: Image -> Image -> Int -> [(Int, Int)] -> Either String [(Int, [Word16])]
chromaIds code adj table ids = forM ids $ \(i, start) -> do
  offs <- mapM (\k -> u16At code (table + 8 * (i - 1) + 2 * k)) [0 .. 3]
  unless (offs == [start + 2 * k | k <- [0 .. 3]]) (Left (imageName code ++ ": chroma " ++ show i ++ " offsets differ from " ++ hex start))
  vals <- mapM (u16At adj) offs
  pure (i, map fromIntegral vals)

-- | Reading order of the chroma stage, +0, +4, +6, +2: Cr from R-Y, Cr from
-- B-Y, Cb from R-Y, Cb from B-Y.
chroma2x2 :: [Word16] -> [Int]
chroma2x2 [s0, s2, s4, s6] = map decodeField [s0, s4, s6, s2]
chroma2x2 _ = []

-- X70 recipe values -----------------------------------------------------------------

-- | Matrix and chroma values by selector from x70view recipes-srgb.json; its
-- chroma q10 lists are in the chroma stage's reading order.
x70Recipes :: Value -> Either String ([(Int, [Int])], [(Int, [Int])])
x70Recipes v = do
  sims <- AT.parseEither (A.withObject "recipes" (A..: "film_sims")) v :: Either String [Value]
  rows <- forM sims $ \s -> AT.parseEither (A.withObject "film" $ \o -> do
    cm <- o A..: "color_matrix"
    m <- (,) <$> cm A..: "selector" <*> cm A..: "q10"
    ch <- o A..: "chroma_matrix"
    c <- (,) <$> ch A..:? "selector" <*> ch A..: "q10"
    pure (m, c)) s
  let uniq xs = foldr (\x acc -> if fst x `elem` map fst acc then acc else x : acc) [] xs
  pure (uniq (map fst rows), uniq [(sel, q) | (_, (Just sel, q)) <- rows])

-- Matrix transfer ------------------------------------------------------------------

type M3 = [[Double]]

asMatrix :: [Word16] -> M3
asMatrix ws = [[fromIntegral (decodeField (ws !! (3 * r + c))) / 1024 | c <- [0 .. 2]] | r <- [0 .. 2]]

mul :: M3 -> M3 -> M3
mul a b = [[sum [a !! i !! k * b !! k !! j | k <- [0 .. 2]] | j <- [0 .. 2]] | i <- [0 .. 2]]

inverse :: M3 -> M3
inverse [[a, b, c], [d, e, f], [g, h, i]] =
  map (map (/ det)) [[e * i - f * h, c * h - b * i, b * f - c * e], [f * g - d * i, a * i - c * g, c * d - a * f], [d * h - e * g, b * g - a * h, a * e - b * d]]
 where
  det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
inverse m = m

maxError :: M3 -> M3 -> Double
maxError a b = maximum [abs (x - y) | (ra, rb) <- zip a b, (x, y) <- zip ra rb]

-- Command ------------------------------------------------------------------------------

runTables :: FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
runTables fflzd xm1Dat xt1Dat s4Path recipesPath out = do
  xm1 <- readPinned "X-M1 1.50 DAT" xm1DatSha xm1Dat
  xt1 <- readPinned "X-T1 5.52 DAT" xt1DatSha xt1Dat
  s4 <- readPinned "X70 1.10 section 4" x70Section4Sha s4Path
  recipesBytes <- readPinned "x70view recipes-srgb.json" x70RecipesSha recipesPath
  recipes <- either H.failH pure (decodeStrict recipesBytes)
  let xm1Payload = payloadOf xm1
      xt1Payload = payloadOf xt1
  (id4, id5, t0, t1) <- H.withTemporary "classic-chrome-tables" $ \root -> do
    let p1 = root </> "xm1-payload.bin"
        p2 = root </> "xt1-payload.bin"
    B.writeFile p1 xm1Payload
    B.writeFile p2 xt1Payload
    (,,,) <$> decodeRecord fflzd root p1 xm1Id4 <*> decodeRecord fflzd root p1 xm1Id5 <*> decodeRecord fflzd root p2 xt1Stream0 <*> decodeRecord fflzd root p2 xt1Stream1
  let x70 = Image "X70 1.10 section 4" 0x8a0400 s4
  result <- either H.failH pure $ do
    adjM <- adjustmentImage "X-M1" "X-M1" xm1Payload
    adjT <- adjustmentImage "X-T1" "X-T1" xt1Payload
    toneM <- toneTable id4 0x824bc4 0x824bc8
    toneX <- toneTable x70 0xb624e0 0xb624e8
    toneT <- toneTable t0 0xaf45e0 0xaf45e8
    matM <- matrixIds id5 adjM 0xd67f88 [1 .. 20]
    matT <- matrixIds t1 adjT 0x1467b44 [1 .. 20]
    chrM <- chromaIds id5 adjM 0xd61e14 [(i, 0xb077 + 8 * (i - 1)) | i <- [1 .. 10]]
    chrT <- chromaIds t1 adjT 0x1465d10 ([(i, 0xb077 + 8 * (i - 1)) | i <- [1 .. 10]] ++ [(i, 0xce9a + 8 * (i - 11)) | i <- [11 .. 15]])
    -- X-T1 Classic Chrome (X70 film index 8) selectors, sRGB row
    ccMatrixOffset <- u16At t1 (0x14685a0 + 2 * 8)
    ccMatrixId <- byteAt adjT ccMatrixOffset
    ccChromaOffset <- u16At t1 (0x14660d4 + 2 * 8)
    ccChromaId <- byteAt adjT ccChromaOffset
    unless (ccMatrixId == 9) (Left ("X-T1 Classic Chrome matrix id is " ++ show ccMatrixId))
    unless (ccChromaId == 11) (Left ("X-T1 Classic Chrome chroma id is " ++ show ccChromaId))
    ccChroma <- maybe (Left "chroma 11 missing") Right (lookup 11 chrT)
    (x70Mat, x70Chr) <- x70Recipes recipes
    pure (adjM, adjT, toneM, toneX, toneT, matM, matT, chrM, chrT, ccMatrixOffset, ccChromaOffset, ccChroma, x70Mat, x70Chr)
  let (_, _, toneM, toneX, toneT, matM, matT, chrM, chrT, ccMatrixOffset, ccChromaOffset, ccChroma, x70Mat, x70Chr) = result
      sameRecord a b = trBytes a == trBytes b
      xm1EqualsX70 = length toneM <= length toneX && and (zipWith sameRecord toneM toneX)
      xm1Keys = map trKey toneM
      absent = [k | k <- [37 .. 41], k `notElem` xm1Keys]
      xt1EqualsX70 = length toneT == length toneX && and (zipWith sameRecord toneT toneX)
      key38 t = [r | r <- t, trKey r == 38]
      key38Equal = case (key38 toneT, key38 toneX) of
        ([a], [b]) -> sameRecord a b
        _ -> False
      matDecoded i ms = maybe [] (map decodeField) (lookup i ms)
      xt1MatEq = [(i, matDecoded i matT == maybe [] id (lookup i x70Mat)) | i <- [1, 2, 3, 4, 5, 9]]
      xt1ChrEq = [(i, maybe [] chroma2x2 (lookup i chrT) == maybe [] id (lookup i x70Chr)) | i <- [1 .. 5] ++ [11]]
      m i ms = asMatrix (maybe [] id (lookup i ms))
      transfer =
        [ (name, maxError (m i matT `mul` inverse (m 1 matT) `mul` m 1 matM) (m i matM), maxError (m 1 matM `mul` inverse (m 1 matT) `mul` m i matT) (m i matM), maxError (m 1 matM) (m i matM), maxError (m i matT) (m i matM))
        | (name, i) <- [("Velvia", 2), ("ASTIA", 3), ("PRO Neg Std", 4), ("PRO Neg Hi", 5)]
        ]
      f3 x = showFFloat (Just 3) x ""
      compositionErrors = concat [[a, b] | (_, a, b, _, _) <- transfer]
      worseThanReuse = [n | (n, a, b, r, _) <- transfer, min a b > r]
  H.ensure (xm1EqualsX70 && null [k | (k, i) <- zip xm1Keys [0 ..], k /= i]) "X-M1 tone records do not equal X70 records 0-36"
  tonesJson <- either H.failH pure $ do
    a <- mapM toneJson toneM
    b <- mapM toneJson toneX
    c <- mapM toneJson toneT
    pure (a, b, c)
  let (jm, jx, jt) = tonesJson
      matObj ms = object [K.fromString (show i) .= map decodeField ws | (i, ws) <- ms]
      chrObj cs = object [K.fromString (show i) .= chroma2x2 ws | (i, ws) <- cs]
      hashes = [("xm1_150", xm1DatSha), ("xt1_552", xt1DatSha), ("x70_section4", x70Section4Sha), ("x70_recipes", x70RecipesSha)]
  B.writeFile out . (<> "\n") . encodeCanonical $
    object
      [ "schema_version" .= (1 :: Int)
      , "inputs" .= object [K.fromString k .= T.pack v | (k, v) <- hashes]
      , "decodes" .= object [K.fromString (recordName r) .= object ["payload_offset" .= hex (recordOffset r), "base" .= hex (recordBase r), "sha256" .= recordSha r] | r <- [xm1Id4, xm1Id5, xt1Stream0, xt1Stream1]]
      , "tone"
          .= object
            [ "record_bytes" .= (0x48 :: Int)
            , "xm1_150" .= object ["count_addr" .= hex 0x824bc4, "records_addr" .= hex 0x824bc8, "records" .= jm]
            , "x70_110" .= object ["count_addr" .= hex 0xb624e0, "records_addr" .= hex 0xb624e8, "records" .= jx, "classic_chrome_key" .= (38 :: Int)]
            , "xt1_552" .= object ["count_addr" .= hex 0xaf45e0, "records_addr" .= hex 0xaf45e8, "records" .= jt]
            ]
      , "matrices" .= object ["xm1_150" .= object ["ids" .= matObj matM], "xt1_552" .= object ["ids" .= matObj matT]]
      , "chroma" .= object ["order" .= ("Cr from R-Y, Cr from B-Y, Cb from R-Y, Cb from B-Y" :: T.Text), "xm1_150" .= object ["ids" .= chrObj chrM], "xt1_552" .= object ["ids" .= chrObj chrT]]
      , "xt1_classic_chrome_chroma" .= object ["image_offset" .= hex 0xce9a, "selector_offset" .= hex ccChromaOffset, "values" .= chroma2x2 ccChroma]
      , "xt1_classic_chrome_matrix" .= object ["id" .= (9 :: Int), "selector_offset" .= hex ccMatrixOffset]
      , "matrix_transfer" .= [object ["film" .= (n :: String), "film_provia_xm1" .= a, "xm1_provia_film" .= b, "reuse_xm1_provia" .= r, "copy_xtrans2" .= c] | (n, a, b, r, c) <- transfer]
      ]
  mapM_ putStrLn $
    [ "X-M1 1.50 tone records: " ++ show (length toneM) ++ " at image-4 0x824bc8 (count at 0x824bc4), keys 0-" ++ show (length toneM - 1) ++ "; " ++ (if xm1EqualsX70 then "each equals" else "differ from") ++ " X70 1.10 record of the same index (0x48 bytes)"
    , "X70 1.10 tone records: " ++ show (length toneX) ++ " at section-4 0xb624e8; keys absent from the X-M1 table: " ++ intercalate ", " (map show absent) ++ "; Classic Chrome uses key 38"
    , "X-T1 5.52 tone records: " ++ show (length toneT) ++ " at stream-0016025c 0xaf45e8 (count at 0xaf45e0); " ++ (if xt1EqualsX70 then "table equals" else "table differs from") ++ " the X70's; key 38 " ++ (if key38Equal then "equals" else "differs from") ++ " the X70's"
    , "X-T1 5.52 matrices equal to the X70's: " ++ intercalate ", " [show i ++ (if e then " yes" else " NO") | (i, e) <- xt1MatEq]
    , "X-T1 5.52 chroma entries equal to the X70's: " ++ intercalate ", " [show i ++ (if e then " yes" else " NO") | (i, e) <- xt1ChrEq]
    , "X-T1 5.52 Classic Chrome (film index 8, sRGB): matrix selector 0x14685a0 reads adjustment " ++ hex ccMatrixOffset ++ " = 9; chroma selector 0x14660d4 reads adjustment " ++ hex ccChromaOffset ++ " = 11; chroma offset table 0x1465d10 entry 11 lists 0xce9a, +2, +4, +6"
    , "Matrix transfer (X-Trans II film matrix composed with the inverse X-Trans II PROVIA matrix and the X-M1 PROVIA matrix, both orders) against the X-M1's own film matrix, max element error: "
        ++ intercalate "; " [n ++ " " ++ f3 a ++ " / " ++ f3 b ++ " (reuse X-M1 PROVIA " ++ f3 r ++ ")" | (n, a, b, r, _) <- transfer]
    , "Composition errors range " ++ f3 (minimum compositionErrors) ++ "-" ++ f3 (maximum compositionErrors) ++ "; worse than reusing X-M1 PROVIA for " ++ intercalate ", " worseThanReuse
    , "wrote " ++ out
    ]
  H.ensure (and (map snd xt1MatEq) && and (map snd xt1ChrEq) && xt1EqualsX70 && key38Equal) "an X-T1 value differs from the X70's"
