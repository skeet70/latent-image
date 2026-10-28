{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (Value (..), encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.List (intercalate)
import qualified Data.Text as T
import Emulation.Sha256 (sha256, sha256Hex)
import Replacement.Service.Packaged
import System.Directory (createDirectoryIfMissing, getTemporaryDirectory, removePathForcibly)
import System.Exit (exitFailure)
import System.FilePath ((</>))

main :: IO ()
main = do
  root <- (</> "latent-image-packaged-qualification-tests") <$> getTemporaryDirectory
  createDirectoryIfMissing True root
  removePathForcibly root
  createDirectoryIfMissing True root
  baseline root
  refuses root "missing Rust entry" (baselineHits, declaredSites, "candidate-qualified")
    (dropFirstEntry)
  refuses root "wrong order" (baselineHits, declaredSites, "candidate-qualified")
    (swapFirstTwo)
  refuses root "wrong entry arguments" (baselineHits, declaredSites, "candidate-qualified")
    (wrongArguments)
  refuses root "missing return" (baselineHits, declaredSites, "candidate-qualified")
    removeFirstReturn
  refuses root "wrong symbol binding" (baselineHits, badSites, "candidate-qualified")
    id
  refuses root "control outcome" (baselineHits, declaredSites, "control-refused")
    id
  removePathForcibly root
  putStrLn "packaged qualification tests: passed"

baseline :: FilePath -> IO ()
baseline root = do
  report <- run root baselineHits declaredSites "candidate-qualified"
  check "a conforming wire fixture qualifies" (packagedStatus report == PackagedQualified)

refuses :: FilePath -> String -> ([Value], [Value], String) -> ([Value] -> [Value]) -> IO ()
refuses root label (hits, sites, outcome) change = do
  report <- run root (change hits) sites outcome
  check (label ++ " is refused") (packagedStatus report == PackagedRefused)

check :: String -> Bool -> IO ()
check label ok = unless ok (putStrLn ("FAIL " ++ label) >> exitFailure) >> putStrLn ("ok " ++ label)

run :: FilePath -> [Value] -> [Value] -> String -> IO PackagedReport
run root hits sites outcome = do
  let directory = root </> outcome
      receipt = directory </> "receipt"
      package = directory </> "package"
  removePathForcibly directory
  createDirectoryIfMissing True (receipt </> "relinked")
  createDirectoryIfMissing True package
  writeFile (receipt </> "relinked" </> "kernel.syms") symbols
  BL.writeFile (receipt </> "relinked" </> "ram-push-manifest.json") (encode relinkManifest)
  BL.writeFile (receipt </> "e2e-result.json") (encode (e2eResult hits sites outcome))
  BL.writeFile (package </> "recipe-complete.json") (encode packageManifest)
  outcomeResult <- qualifyPackaged (PackagedInputs receipt package)
  case outcomeResult of
    Left problem -> fail problem
    Right report -> pure report

symbols :: String
symbols = intercalate "\n"
  [ "00a86878 00000010 T rb_service_field_row_state"
  , "00a867a0 00000014 T rb_call_stock_probe"
  , "00d52684 00006234 T FUN_00d52684"
  , ""
  ]

symbolsSha :: String
symbolsSha = sha256Hex (sha256 (BC.pack symbols))

declaredSites :: [Value]
declaredSites =
  [ site "svc-rust-entry" "00a86878"
  , site "svc-stock-trampoline" "00a867a0"
  , site "svc-stock-return" "00a867b0"
  , site "stock-row-probe" "00d52684"
  ]

badSites :: [Value]
badSites = site "svc-rust-entry" "00a8687c" : drop 1 declaredSites

site :: String -> String -> Value
site label address = object ["label" .= label, "address" .= address]

baselineHits :: [Value]
baselineHits = concat [observed row iteration | row <- [0 .. 4], iteration <- [0 .. 1]]

observed :: Int -> Int -> [Value]
observed row iteration =
  [ hit "svc-rust-entry" (base + 0) 0x110 row
  , hit "svc-stock-trampoline" (base + 1) 0x110 row
  , hit "stock-row-probe" (base + 2) 0x110 row
  , hit "stock-row-probe" (base + 3) 5 0x1a
  , hit "stock-row-probe" (base + 4) 5 0x1a
  , hit "svc-stock-return" (base + 5) 0 0
  ]
  where
    base = 1000 + row * 100 + iteration * 10

hit :: String -> Int -> Int -> Int -> Value
hit label instructions r0 r1 =
  object
    [ "site" .= label
    , "core" .= (0 :: Int)
    , "instructions" .= instructions
    , "r0" .= hex r0
    , "r1" .= hex r1
    , "r2" .= hex (0 :: Int)
    , "r3" .= hex (0 :: Int)
    ]

hex :: Int -> String
hex value = "0x" ++ replicate (8 - length digits) '0' ++ digits
  where digits = showHex value

showHex :: Int -> String
showHex value
  | value < 16 = [digits !! value]
  | otherwise = showHex (value `div` 16) ++ [digits !! (value `mod` 16)]
  where digits = "0123456789abcdef"

dropFirstEntry :: [Value] -> [Value]
dropFirstEntry = removeFirst "svc-rust-entry"

removeFirstReturn :: [Value] -> [Value]
removeFirstReturn = removeFirst "svc-stock-return"

swapFirstTwo :: [Value] -> [Value]
swapFirstTwo (first : second : rest) = second : first : rest
swapFirstTwo values = values

wrongArguments :: [Value] -> [Value]
wrongArguments (value : rest) =
  case value of
    Object objectValue ->
      Object (KM.insert (K.fromString "r1") (String (T.pack "0x00000009")) objectValue) : rest
    _ -> value : rest
wrongArguments [] = []

removeFirst :: String -> [Value] -> [Value]
removeFirst label (value : rest)
  | field value "site" == Just (String (text label)) = rest
  | otherwise = value : removeFirst label rest
removeFirst _ [] = []

field :: Value -> String -> Maybe Value
field (Object objectValue) name = KM.lookup (K.fromString name) objectValue
field _ _ = Nothing

text :: String -> T.Text
text = T.pack

e2eResult :: [Value] -> [Value] -> String -> Value
e2eResult hits sites outcome =
  object
    [ "schema" .= (1 :: Int)
    , "outcome" .= outcome
    , "runtime" .= object
        [ "bytes" .= (100 :: Int)
        , "publications" .= (1 :: Int)
        , "symbols_sha256" .= symbolsSha
        , "manifest" .= "relinked/ram-push-manifest.json"
        ]
    , "relink" .= object
        [ "mode" .= ("unreviewed" :: String)
        , "source_identity" .= object
            [ "rust_object_replacement_bank" .= archiveSha
            , "rust_toolchain_replacement_bank" .= factsSha
            ]
        ]
    , "emulator_start" .= object
        [ "inputs" .= object
            [ "inputs" .= object
                [ "installed_dat" .= datSha
                , "card_present" .= False
                ]
            ]
        ]
    , "observations" .=
        [ object ["phase" .= ("service-field-list" :: String), "trace_hits" .= hits]
        , object ["phase" .= ("service-after-unload" :: String), "trace_hits" .= ([] :: [Value])]
        , object [ "phase" .= ("service-after-unload-stock" :: String)
                 , "trace_hits" .= [hit "stock-row-probe" 9000 0 0]
                 ]
        ]
    , "trajectories" .=
        [ object ["name" .= ("service-page-cancel" :: String), "result" .= ("matched" :: String)]
        , object ["name" .= ("service-field-reopen" :: String), "result" .= ("matched" :: String)]
        ]
    , "unload" .= ("1 old words verified; allocation retained until power-off" :: String)
    , "mailbox" .= object
        [ "install" .= operation 1 1
        , "rollback" .= operation 2 1
        ]
    , "declared_trace_sites" .= sites
    ]

operation :: Int -> Int -> Value
operation op count =
  object
    [ "operation" .= op
    , "status" .= (0 :: Int)
    , "peer_status" .= (0 :: Int)
    , "validated" .= count
    , "restored" .= count
    ]

relinkManifest :: Value
relinkManifest =
  object
    [ "base" .= ("0x00a85000" :: String)
    , "runtime_sha256" .= runtimeSha
    , "source_identity" .= object
        [ "rust_object_replacement_bank" .= archiveSha
        , "rust_toolchain_replacement_bank" .= factsSha
        ]
    , "publications" .=
        [ object
            [ "address" .= ("0x011122c0" :: String)
            , "old" .= ("0xe300383d" :: String)
            , "new" .= ("0xe3003849" :: String)
            , "phase" .= ("setup" :: String)
            ]
        ]
    , "size" .= (100 :: Int)
    ]

packageManifest :: Value
packageManifest =
  object
    [ "firmware_sha256" .= datSha
    , "executable_sha256" .= executableSha
    , "executable_start" .= ("0x00be1f00" :: String)
    , "installable" .= False
    , "inputs" .=
        [ object ["path" .= ("runfiles/bank.a" :: String), "sha256" .= archiveSha]
        , object ["path" .= ("runfiles/build-facts.txt" :: String), "sha256" .= factsSha]
        ]
    ]

archiveSha, factsSha, datSha, executableSha, runtimeSha :: String
archiveSha = replicate 64 'a'
factsSha = replicate 64 'b'
datSha = replicate 64 'c'
executableSha = replicate 64 'd'
runtimeSha = replicate 64 'e'
