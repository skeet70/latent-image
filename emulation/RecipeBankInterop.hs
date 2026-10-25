{-# LANGUAGE OverloadedStrings #-}
-- Host-only cross-language fixtures for the X-M1 1.50 bank codec.
module Main where

import FujiAF.Host
import FujiAF.JSON (encodeCanonical)
import FujiAF.RecipeBank
import Data.Aeson (Value(..))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import System.Directory
import System.Environment
import System.Exit
import System.FilePath

fixtureNames :: [String]
fixtureNames = ["ALPHA", "BRAVO", "CHARLIE", "DELTA", "ECHO", "FOXTROT", "GOLF"]

-- Properties 0x1cb/0x1cc are full-width halfwords at payload +0/+2.
fixtureHalfwords :: [B.ByteString]
fixtureHalfwords =
  [ B.pack [0x34, 0x12, 0x78, 0x56]
  , B.pack [0x01, 0x00, 0x02, 0x00]
  , B.pack [0xff, 0xff, 0xff, 0xff]
  , B.pack [0xef, 0xbe, 0xad, 0xde]
  , B.pack [0x00, 0x80, 0x00, 0x40]
  , B.pack [0x11, 0x22, 0x33, 0x44]
  , B.pack [0xfe, 0xca, 0x00, 0x01]
  ]

-- reports/custom-settings.fields.json, reset_after_jig_cycle.default_block.
fixtureDefaults :: B.ByteString
fixtureDefaults =
  B.pack
    [ 0x00, 0x00, 0x00, 0x00, 0x7f, 0x22, 0x22, 0x09, 0x49, 0x49, 0x69, 0x49, 0x49, 0x09, 0x09, 0x09
    , 0x49, 0x29, 0x09, 0x69, 0x09, 0x09, 0xa9, 0x40, 0x43, 0x22, 0x03, 0x02, 0x82, 0x82, 0x8f, 0x8f
    , 0x88, 0x30, 0x63, 0x60, 0x30, 0x07, 0x40, 0x00
    ]

fixtureDefaultsSha256 :: String
fixtureDefaultsSha256 = "1787a2535e60bbcbc13f515b1f786786a38e5482269e8d4a65f4e4739c10cd81"

-- IMAGE SIZE row 1 is (size 0, aspect 1) at 0x007a00cc + 16.
-- The aspect occupies payload +0x1e bits 5-6.
fixtureSeed :: B.ByteString
fixtureSeed = B.take 0x1e fixtureDefaults <> B.singleton 0xaf <> B.drop 0x1f fixtureDefaults

fixtureState :: Either String BankState
fixtureState = do
  records <- sequence
    [ do
        named <- slotName name
        payload <- canonicalBankPayload (hw <> B.drop (B.length hw) fixtureSeed)
        pure (Populated named payload payload)
    | (name, hw) <- zip fixtureNames fixtureHalfwords
    ]
  bank <- bankRecordsFromList records
  pure (BankState (BankGeneration 7) NothingSelected Disabled BaseInvalid bank)

hex :: B.ByteString -> String
hex = concatMap showHexByte . B.unpack
 where
  showHexByte w =
    let digits = "0123456789abcdef"
        hi = digits !! (fromIntegral w `div` 16)
        lo = digits !! (fromIntegral w `mod` 16)
     in [hi, lo]

ni :: Int -> Value
ni = FujiAF.Host.n . fromIntegral

stateManifestFields :: BankState -> [(String, Value)]
stateManifestFields state =
  [ ("generation", FujiAF.Host.n (fromIntegral generation))
  , ("slot_valid_mask", FujiAF.Host.n (sum [2 ^ i | (i, Populated {}) <- zip [0 :: Int ..] records]))
  , ("slot_count", ni (length records))
  , ("selection", case bankSelection state of
        NothingSelected -> ni 0
        Custom slot -> ni (fromEnum slot + 1))
  , ("auto_update", s (case bankAutoUpdate state of Disabled -> "DISABLE"; Enabled -> "ENABLE"))
  , ("base", case bankBase state of
        BaseInvalid -> obj [("valid", Bool False), ("capture_mode", Null), ("payload_hex", Null)]
        BaseValid mode payload -> obj
          [("valid", Bool True), ("capture_mode", s (show mode)), ("payload_hex", payloadHex payload)])
  , ("layout", obj
        [ ("version", ni 2)
        , ("file_bytes", ni 1024)
        , ("header_bytes", ni 64)
        , ("body_bytes", ni 782)
        , ("selection_offset", ni 0x20)
        , ("auto_update_offset", ni 0x21)
        , ("base_valid_offset", ni 0x22)
        , ("base_capture_mode_offset", ni 0x23)
        , ("base_payload_offset", ni 0x40)
        , ("records_offset", ni 0x68)
        , ("record_bytes", ni 106)
        , ("name_bytes", ni 26)
        , ("payload_bytes", ni 40)
        , ("padding_offset", ni 0x34e)
        ])
  , ("slots", arr
        [ obj
            ([ ("index", ni i)
             , ("name_offset", ni (0x68 + 106 * i))
             , ("saved_payload_offset", ni (0x68 + 106 * i + 26))
             , ("working_payload_offset", ni (0x68 + 106 * i + 66))
             ] ++ recordFields record)
        | (i, record) <- zip [0 :: Int ..] records
        ])
  ]
 where
  BankGeneration generation = bankGeneration state
  records = bankRecordList (bankRecords state)
  payloadHex = s . hex . bankPayloadContents
  recordFields Unpopulated =
    [("populated", Bool False), ("name", Null), ("saved_payload_hex", Null), ("working_payload_hex", Null)]
  recordFields (Populated named saved working) =
    [("populated", Bool True), ("name", s (slotNameText named)), ("saved_payload_hex", payloadHex saved), ("working_payload_hex", payloadHex working)]

literalManifest :: BankState -> String -> Value
literalManifest state digest =
  obj
    ([ ("schema", s "recipe-bank-interop-fixtures-2")
     , ("file_sha256", s digest)
     , ("provenance", obj
          [ ("defaults_source", s "reports/custom-settings.fields.json#reset_after_jig_cycle.default_block")
          , ("defaults_sha256", s fixtureDefaultsSha256)
          , ("image4_sha256", s "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")
          , ("image_size_row", ni 1)
          , ("image_size_pair_table", s "0x007a00cc")
          , ("size_code", ni 0)
          , ("aspect_code", ni 1)
          , ("halfword_literals", arr (map (s . hex) fixtureHalfwords))
          , ("unbound_bits", s "cleared using the generated payload field map")
          ])
     ] ++ stateManifestFields state)

encodeMode :: FilePath -> IO ()
encodeMode outDir = do
  defaultsDigest <- sha256 fixtureDefaults
  ensure
    (defaultsDigest == fixtureDefaultsSha256)
    ("fixture defaults hash " ++ defaultsDigest ++ ", not the pinned block")
  state <- either failH pure fixtureState
  file <- either failH pure (encodeBankFile state)
  createDirectoryIfMissing True outDir
  B.writeFile (outDir </> "host-bank.bin") file
  digest <- sha256 file
  B.writeFile (outDir </> "host-bank-manifest.json") (encodeCanonical (literalManifest state digest) <> BC.pack "\n")
  putStrLn ("wrote " ++ outDir </> "host-bank.bin" ++ " sha256 " ++ digest)

checkMode :: FilePath -> FilePath -> IO ()
checkMode file manifestPath = do
  bytes <- B.readFile file
  manifest <- jsonRead =<< B.readFile manifestPath
  ensure (get manifest "schema" == s "recipe-bank-interop-fixtures-2") "v2 interop manifest required"
  state <- case decodeBankFile bytes of
    Left reason -> failH ("decode refused: " ++ reason)
    Right (DecodedV1 _) -> failH "interop check requires a v2 file"
    Right (DecodedV3 _) -> failH "interop check requires the legacy D3 film encoding"
    Right (DecodedV2 decoded) -> pure decoded
  let problems =
        [ key ++ ": decoded metadata differs from manifest"
        | (key, expected) <- stateManifestFields state
        , get manifest key /= expected
        ]
  if null problems
    then putStrLn ("interop check passed: " ++ file)
    else do
      mapM_ putStrLn problems
      exitFailure

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ["encode", outDir] -> encodeMode outDir
    ["check", file, manifest] -> checkMode file manifest
    _ -> do
      putStrLn "usage: recipe-bank-interop encode OUT_DIR | check FILE MANIFEST"
      exitFailure
