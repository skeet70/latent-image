{-# LANGUAGE OverloadedStrings #-}
-- Cross-language interop fixtures for the recipe bank file codec (1.50,
-- host-only). `encode` writes a bank file through the Haskell codec from
-- literal slot names and payloads; `check` decodes a firmware-written file
-- against the same literals. The literals are typed here, not produced by
-- either codec.
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
import System.IO (hFlush, stdout)

-- Literal fixture values. Payloads are the 0x28-byte record images; the two
-- halfwords at +0x00/+0x02 are bound fields (properties 0x1cb/0x1cc) and are
-- carried at full width, so distinct values there pin the payload start, and
-- the rest of each payload is the body's defaults so every table-bound field
-- keeps a code its conversion table carries. Names pin the name-block start at
-- file offset 0x20 + 7*0x28 = 0x138.
fixtureNames :: [String]
fixtureNames = ["ALPHA", "BRAVO", "CHARLIE", "DELTA", "ECHO", "FOXTROT", "GOLF"]

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

-- | The body's C-payload defaults (reports/custom-settings.fields.json,
-- reset_after_jig_cycle.default_block, sha256 1787a253...): the base every
-- fixture payload rides, because not every conversion table carries code 0 for
-- every field (table 0x776f34 has col0 {1,3,4}, so property 0x137 refuses a
-- zero payload and the storage kernel answers RB_BAD_DATA).
fixtureDefaults :: B.ByteString
fixtureDefaults =
  B.pack
    [ 0x00, 0x00, 0x00, 0x00, 0x7f, 0x22, 0x22, 0x09, 0x49, 0x49, 0x69, 0x49, 0x49, 0x09, 0x09, 0x09
    , 0x49, 0x29, 0x09, 0x69, 0x09, 0x09, 0xa9, 0x40, 0x43, 0x22, 0x03, 0x02, 0x82, 0x82, 0x8f, 0x8f
    , 0x88, 0x30, 0x63, 0x60, 0x30, 0x07, 0x40, 0x00
    ]

fixtureDefaultsSha256 :: String
fixtureDefaultsSha256 = "1787a2535e60bbcbc13f515b1f786786a38e5482269e8d4a65f4e4739c10cd81"

fixturePayloads :: [B.ByteString]
fixturePayloads =
  [ hw `B.append` B.drop (B.length hw) fixtureDefaults
  | hw <- fixtureHalfwords
  ]

fixtureGeneration :: Integer
fixtureGeneration = 7

fixtureMask :: Integer
fixtureMask = 0x7f

fileSlots :: [FileSlot]
fileSlots = zipWith FileSlot fixtureNames fixturePayloads

hex :: B.ByteString -> String
hex = concatMap (\w -> let g = showHexByte w in g) . B.unpack
 where
  showHexByte w =
    let digits = "0123456789abcdef"
        hi = digits !! (fromIntegral w `div` 16)
        lo = digits !! (fromIntegral w `mod` 16)
     in [hi, lo]

literalManifest :: String -> Value
literalManifest digest =
  obj
    [ ("schema", s "recipe-bank-interop-fixtures-1")
    , ("file_sha256", s digest)
    , ("generation", FujiAF.Host.n fixtureGeneration)
    , ("slot_valid_mask", FujiAF.Host.n fixtureMask)
    , ("slot_count", ni (length fixtureNames))
    , ("name_block_offset", ni (0x20 + 7 * FujiAF.RecipeBank.payloadBytes))
    , ( "slots"
      , arr
          [ obj
              [ ("index", ni i)
              , ("name", s name)
              , ("payload_hex", s (hex payload))
              ]
          | (i, (name, payload)) <- zip [0 :: Int ..] (zip fixtureNames fixturePayloads)
          ]
      )
    ]
 where
  ni :: Int -> Value
  ni = FujiAF.Host.n . fromIntegral

encodeMode :: FilePath -> IO ()
encodeMode outDir = do
  createDirectoryIfMissing True outDir
  defaultsDigest <- sha256 fixtureDefaults
  ensure
    (defaultsDigest == fixtureDefaultsSha256)
    ("fixture defaults hash " ++ defaultsDigest ++ ", not the pinned block")
  let blob = either (error . ("encodeFileBlob: " ++)) id (encodeFileBlob fileSlots)
      file = either (error . ("encodeBankFile: " ++)) id
        (encodeBankFile (BankHeader (fromIntegral fixtureGeneration) (fromIntegral fixtureMask)) blob)
  B.writeFile (outDir </> "host-bank.bin") file
  digest <- sha256 file
  B.writeFile (outDir </> "host-bank-manifest.json") (encodeCanonical (literalManifest digest) <> BC.pack "\n")
  putStrLn ("wrote " ++ outDir </> "host-bank.bin" ++ " sha256 " ++ digest)

checkMode :: FilePath -> FilePath -> IO ()
checkMode file manifestPath = do
  bytes <- B.readFile file
  manifestBytes <- B.readFile manifestPath
  manifest <- jsonRead manifestBytes
  case decodeBankFile bytes of
    Left reason -> do
      putStrLn ("decode refused: " ++ reason)
      exitFailure
    Right (header, slots) -> do
      let problems =
            [ "generation: " ++ show (bankGeneration header)
            | bankGeneration header /= fromIntegral (num (get manifest "generation"))
            ]
              ++ [ "slot mask: " ++ show (bankSlotValid header)
                 | bankSlotValid header /= fromIntegral (num (get manifest "slot_valid_mask"))
                 ]
              ++ concat
                  [ [ "slot " ++ show i ++ " name: " ++ show (fileSlotName slot)
                    | fileSlotName slot /= str (get row "name")
                    ]
                    ++ [ "slot " ++ show i ++ " payload: " ++ hex (fileSlotPayload slot)
                       | hex (fileSlotPayload slot) /= str (get row "payload_hex")
                       ]
                  | (i, (slot, row)) <- zip [0 :: Int ..] (zip slots (list (get manifest "slots")))
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
