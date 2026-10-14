-- Apply a patch-set manifest (reports/filmsim-patch-01.md) to its pinned
-- decoded image. Every ordered word write is checked against the manifest's
-- current bytes and the output-base file-offset rule before anything is
-- applied, the input must hash to the manifest's decode hash, and a write
-- whose current bytes disagree stops the run. The manifest is the write set's
-- only machine form, so the tool consumes its blobs and word list and nothing
-- else. A schema-3 extension block makes the constructed output the template
-- prefix followed by the input decode; every write's current bytes are then
-- checked against that constructed output, not against the input decode.
module Main where

import Control.Monad (forM_, unless)
import Data.Aeson (Value)
import qualified Data.ByteString as B
import Data.List (sortOn, (\\))
import qualified FujiAF.Host as H
import System.Environment (getArgs)

data PatchWord = PatchWord
  { pwOrder :: Int
  , pwOffset :: Int
  , pwCurrent :: B.ByteString
  , pwNew :: B.ByteString
  }

main :: IO ()
main = do
  arguments <- getArgs
  (manifestPath, inputPath, outputPath) <- case arguments of
    [manifest, input, output] -> pure (manifest, input, output)
    _ -> H.failH "usage: apply-patch-set PATCH_SET_JSON INPUT_IMAGE OUTPUT_IMAGE"
  manifestBytes <- H.readRegular (1024 * 1024) manifestPath
  value <- H.jsonRead manifestBytes
  let manifestKeys = H.keys value
      optionalKeys = ["status", "installable", "extension", "scan_contract", "file_contract", "service_job_map"]
      requiredKeys =
        words
          "schema_version id tier recorded_at target image free_space array_source order_rule blobs words totals unchanged_surfaces container_effect emulation_case_parameters"
  H.ensure (null (manifestKeys \\ (requiredKeys ++ optionalKeys))) "patch set: unknown top-level field"
  H.fields value (unwords (requiredKeys ++ [k | k <- optionalKeys, k `elem` manifestKeys])) "patch set"
  version <- H.integer (H.get value "schema_version") 1 3 "patch set schema_version"
  let identifier = H.str (H.get value "id")
      tier = H.str (H.get value "tier")
      imageBlock = H.get value "image"
      imageKeys = H.keys imageBlock
      imageRequired = words "segment_id decode_base decode_file decode_sha256 file_offset_rule dump_correspondence"
  H.ensure (null (imageKeys \\ (imageRequired ++ ["output_base"]))) "patch set image: unknown field"
  H.fields imageBlock (unwords (imageRequired ++ ["output_base" | "output_base" `elem` imageKeys])) "patch set image"
  decodeBase <- hexValue (H.str (H.get imageBlock "decode_base")) "patch set decode_base"
  outputBase <-
    if "output_base" `elem` imageKeys
      then hexValue (H.str (H.get imageBlock "output_base")) "patch set output_base"
      else pure decodeBase
  let decodeSha = H.str (H.get imageBlock "decode_sha256")
  template <- case "extension" `elem` manifestKeys of
    False -> do
      H.ensure (outputBase == decodeBase) "patch set output_base requires an extension block"
      pure B.empty
    True -> do
      let block = H.get value "extension"
      H.fields block "kind length current_hex" "patch set extension"
      H.ensure (H.str (H.get block "kind") == "prefix") "patch set extension kind must be prefix"
      size <- H.integer (H.get block "length") 1 0x10000 "patch set extension length"
      bytes <- hexBlob (H.str (H.get block "current_hex")) "patch set extension current_hex"
      H.ensure (fromIntegral size == B.length bytes) "patch set extension length must match current_hex"
      H.ensure (decodeBase - outputBase == size) "patch set extension length must equal decode_base - output_base"
      pure bytes
  H.ensure (B.null template || version == 3) "patch set extension requires schema_version 3"
  blobWrites <- concat <$> mapM (parseBlob outputBase) (H.list (H.get value "blobs"))
  wordWrites <- mapM (parseWord outputBase) (H.list (H.get value "words"))
  let writes = sortOn pwOrder (blobWrites ++ wordWrites)
  unless (map pwOrder writes == [1 .. length writes]) $
    H.failH "patch set write orders are not contiguous from 1"
  checkOverlap (map pwOffset (sortOn pwOffset writes))
  image <- H.readRegular (16 * 1024 * 1024) inputPath
  digest <- H.sha256 image
  let output = template <> image
  putStrLn (inputPath ++ ": " ++ show (B.length image) ++ " bytes, sha256 " ++ digest)
  unless (digest == decodeSha) $ H.failH "input image does not match the manifest's decode_sha256"
  unless (B.null template) $
    putStrLn
      ( "extension: prefix "
          ++ show (B.length template)
          ++ " bytes, output base 0x"
          ++ hex (fromIntegral outputBase)
          ++ ", output "
          ++ show (B.length output)
          ++ " bytes"
      )
  putStrLn ("patch set " ++ identifier ++ " (" ++ tier ++ "): applying " ++ show (length writes) ++ " ordered word writes")
  forM_ writes $ \w -> do
    let at = B.take 4 (B.drop (pwOffset w) output)
    unless (at == pwCurrent w) $
      H.failH
        ( "write order "
            ++ show (pwOrder w)
            ++ " at file offset 0x"
            ++ hex (pwOffset w)
            ++ ": current bytes disagree (image "
            ++ hexBytes at
            ++ ", manifest "
            ++ hexBytes (pwCurrent w)
            ++ ")"
        )
  let patched = foldl (\img w -> B.take (pwOffset w) img <> pwNew w <> B.drop (pwOffset w + 4) img) output writes
      differing = sum [length [() | i <- [0 .. 3], B.index (pwCurrent w) i /= B.index (pwNew w) i] | w <- writes]
      spans = mergeSpans [(pwOffset w, pwOffset w + 4) | w <- sortOn pwOffset writes]
  putStrLn ("words written: " ++ show (length writes) ++ " (orders 1.." ++ show (length writes) ++ "), bytes differing from the decode: " ++ show differing)
  putStrLn ("touched file offsets: " ++ unwords ["0x" ++ hex a ++ "..0x" ++ hex b | (a, b) <- spans])
  patchedDigest <- H.sha256 patched
  B.writeFile outputPath patched
  putStrLn ("wrote " ++ outputPath ++ ": " ++ show (B.length patched) ++ " bytes, sha256 " ++ patchedDigest)

parseBlob :: Integer -> Value -> IO [PatchWord]
parseBlob outputBase v = do
  H.fields v "id role address file_offset length order_range current_hex new_hex" "patch set blob"
  address <- hexValue (H.str (H.get v "address")) "blob address"
  offset <- hexValue (H.str (H.get v "file_offset")) "blob file_offset"
  H.ensure (offset == address - outputBase) "blob file_offset does not follow the output-base rule"
  size <- H.integer (H.get v "length") 4 0x10000 "blob length"
  orders <- case H.list (H.get v "order_range") of
    [a, b] -> do
      low <- H.integer a 1 100000 "blob order_range"
      high <- H.integer b 1 100000 "blob order_range"
      pure [low .. high]
    _ -> H.failH "blob order_range must be a two-element array"
  current <- hexBlob (H.str (H.get v "current_hex")) "blob current_hex"
  fresh <- hexBlob (H.str (H.get v "new_hex")) "blob new_hex"
  H.ensure (fromIntegral size == B.length current && B.length current == B.length fresh) "blob hex lengths must match the blob length"
  H.ensure (length orders * 4 == B.length current) "blob order_range must cover one order per word"
  pure
    [ PatchWord
        { pwOrder = fromIntegral order
        , pwOffset = fromIntegral offset + 4 * k
        , pwCurrent = B.take 4 (B.drop (4 * k) current)
        , pwNew = B.take 4 (B.drop (4 * k) fresh)
        }
    | (k, order) <- zip [0 :: Int ..] orders
    ]

parseWord :: Integer -> Value -> IO PatchWord
parseWord outputBase v = do
  H.fields v "order address file_offset current new role" "patch set word"
  order <- H.integer (H.get v "order") 1 100000 "word order"
  address <- hexValue (H.str (H.get v "address")) "word address"
  offset <- hexValue (H.str (H.get v "file_offset")) "word file_offset"
  H.ensure (offset == address - outputBase) "word file_offset does not follow the output-base rule"
  current <- hexValue (H.str (H.get v "current")) "word current"
  fresh <- hexValue (H.str (H.get v "new")) "word new"
  pure
    PatchWord
      { pwOrder = fromIntegral order
      , pwOffset = fromIntegral offset
      , pwCurrent = word32le current
      , pwNew = word32le fresh
      }

checkOverlap :: [Int] -> IO ()
checkOverlap (a : rest@(b : _)) = do
  H.ensure (a + 4 <= b) "patch set write words overlap"
  checkOverlap rest
checkOverlap _ = pure ()

mergeSpans :: [(Int, Int)] -> [(Int, Int)]
mergeSpans [] = []
mergeSpans ((a, b) : rest) = case mergeSpans rest of
  ((c, d) : spans) | c <= b -> (a, max b d) : spans
  spans -> (a, b) : spans

word32le :: Integer -> B.ByteString
word32le value = B.pack [fromIntegral ((value `div` shift) `mod` 256) | shift <- [1, 256, 65536, 16777216]]

hexValue :: String -> String -> IO Integer
hexValue text label = case text of
  '0' : 'x' : digits@(_ : _) | all (`elem` hexDigits) digits -> pure (foldl step 0 digits)
  _ -> H.failH (label ++ ": expected a 0x-prefixed hex value")
  where
    step acc c = acc * 16 + toInteger (hexDigit c)

hexBlob :: String -> String -> IO B.ByteString
hexBlob text label
  | odd (length text) = H.failH (label ++ ": even hex length required")
  | not (all (`elem` hexDigits) text) = H.failH (label ++ ": lowercase hex required")
  | otherwise = B.pack <$> go text
  where
    go [] = pure []
    go (a : b : rest) = (fromIntegral (hexDigit a * 16 + hexDigit b) :) <$> go rest
    go [_] = H.failH (label ++ ": odd hex length")

hexDigit :: Char -> Int
hexDigit c
  | '0' <= c && c <= '9' = fromEnum c - fromEnum '0'
  | otherwise = fromEnum c - fromEnum 'a' + 10

hexDigits :: String
hexDigits = "0123456789abcdef"

hexBytes :: B.ByteString -> String
hexBytes = concatMap (\b -> [hexDigitChar (fromIntegral b `div` 16), hexDigitChar (fromIntegral b `mod` 16)]) . B.unpack

hex :: Int -> String
hex n
  | n < 16 = [hexDigitChar n]
  | otherwise = hex (n `div` 16) ++ [hexDigitChar (n `mod` 16)]

hexDigitChar :: Int -> Char
hexDigitChar d = if d < 10 then toEnum (fromEnum '0' + d) else toEnum (fromEnum 'a' + d - 10)
