{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless, when)
import Data.Aeson (Value (..), encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import qualified Data.Text as T
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host (withTemporary)
import FujiAF.RamPush (Address (..), RamPushEntries (..), afRingPayload, payloadAllocationSize, payloadEntries)
import Replacement.Service.Packaged
import Numeric (showHex)
import System.Directory (createDirectoryIfMissing)
import System.Exit (exitFailure)
import System.FilePath ((</>))

data Variant = Variant
  { variantHits :: [Value]
  , variantReopenHits :: [Value]
  , variantSites :: [Value]
  , variantOutcome :: String
  , variantAfterHits :: [Value]
  , variantUnrelatedHits :: [Value]
  , variantOmitSymbols :: Bool
  , variantControlFile :: Bool
  , variantMutatePackage :: Maybe (FilePath -> IO ())
  }

main :: IO ()
main = withTemporary "packaged-qualification" $ \root -> do
  baselineCheck root
  refuses root "missing Rust entry" (baselineVariant { variantHits = dropFirstEntry baselineHits })
  refuses root "wrong order" (baselineVariant { variantHits = swapFirstTwo baselineHits })
  refuses root "wrong entry arguments" (baselineVariant { variantHits = wrongArguments baselineHits })
  refuses root "missing return" (baselineVariant { variantHits = removeFirstReturn baselineHits })
  refuses root "missing reopen row entry" (baselineVariant { variantReopenHits = dropFirstEntry reopenHits })
  refuses root "valid rows with an orphan owned event" (baselineVariant { variantHits = appendOrphan baselineHits })
  refuses root "wrong symbol binding" (baselineVariant { variantSites = badSites })
  refuses root "control outcome" (baselineVariant { variantOutcome = "control-refused" })
  refuses root "missing runtime symbol metadata" (baselineVariant { variantOmitSymbols = True })
  refuses root "sibling control result" (baselineVariant { variantControlFile = True })
  refuses root "mutated package DAT" (baselineVariant { variantMutatePackage = Just mutatePackage })
  refuses root "owned entry after service-after-unload" (baselineVariant { variantAfterHits = afterUnloadOwned })
  refuses root "owned entry in a later unrelated phase" (baselineVariant { variantUnrelatedHits = afterUnloadOwned })
  putStrLn "packaged qualification tests: passed"

baselineVariant :: Variant
baselineVariant = Variant baselineHits reopenHits declaredSites "candidate-qualified" [] [] False False Nothing

baselineCheck :: FilePath -> IO ()
baselineCheck root = do
  outcome <- load root baselineVariant
  case outcome of
    Left problem -> fail problem
    Right report -> check "a conforming independent wire fixture qualifies" (packagedStatus report == PackagedQualified)

refuses :: FilePath -> String -> Variant -> IO ()
refuses root label variant = do
  outcome <- load root variant
  case outcome of
    Left _ -> putStrLn ("ok " ++ label ++ " is refused")
    Right report -> check (label ++ " is refused") (packagedStatus report == PackagedRefused)

check :: String -> Bool -> IO ()
check label ok = unless ok (putStrLn ("FAIL " ++ label) >> exitFailure) >> putStrLn ("ok " ++ label)

load :: FilePath -> Variant -> IO (Either String PackagedReport)
load root variant = do
  let receipt = root </> "receipt"
      package = root </> "package"
  createDirectoryIfMissing True (receipt </> "relinked")
  createDirectoryIfMissing True (package </> "kernel")
  B.writeFile (receipt </> "relinked" </> "kernel.syms") symbols
  B.writeFile (receipt </> "relinked" </> "runtime.bin") runtime
  BL.writeFile (receipt </> "relinked" </> "ram-push-manifest.json") (encode relinkManifest)
  B.writeFile (package </> "recipe-complete.DAT") datBytes
  B.writeFile (package </> "kernel" </> "kernel.bin") kernelBytes
  createDirectoryIfMissing True (package </> "runfiles")
  B.writeFile (package </> "runfiles" </> "bank.a") archiveBytes
  B.writeFile (package </> "runfiles" </> "build-facts.txt") factsBytes
  BL.writeFile (package </> "recipe-complete.json") (encode packageManifest)
  case variantMutatePackage variant of
    Nothing -> pure ()
    Just mutation -> mutation package
  BL.writeFile (receipt </> "e2e-result.json") (encode (e2eResult variant))
  when (variantControlFile variant) $
    BL.writeFile (receipt </> "e2e-control.json") (encode (object ["outcome" .= ("control-refused" :: String)]))
  qualifyPackaged (PackagedInputs receipt package)

symbols :: B.ByteString
symbols = B.intercalate (BC.pack "\n")
  [ BC.pack "00a86878 00000010 T rb_service_field_row_state"
  , BC.pack "00a867a0 00000014 T rb_call_stock_probe"
  , BC.pack "00d52684 00006234 T FUN_00d52684"
  , BC.pack ""
  ]

runtime :: B.ByteString
runtime = B.replicate size 0

size :: Int
size = payloadAllocationSize afRingPayload

base :: Word32
base = 0x00a85000

entries :: RamPushEntries
entries = payloadEntries afRingPayload (Address base)

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
baselineHits = concat [observed row 0 | row <- [0 .. 4]]

reopenHits :: [Value]
reopenHits = concat [observed row 1 | row <- [0 .. 4]]

observed :: Int -> Int -> [Value]
observed row iteration =
  [ hit "svc-rust-entry" (start + 0) 0x110 row
  , hit "svc-stock-trampoline" (start + 1) 0x110 row
  , hit "stock-row-probe" (start + 2) 0x110 row
  , hit "stock-row-probe" (start + 3) 5 0x1a
  , hit "stock-row-probe" (start + 4) 5 0x1a
  , hit "svc-stock-return" (start + 5) 0 0
  ]
  where
    start = 1000 + iteration * 1000 + row * 100

afterUnloadOwned :: [Value]
afterUnloadOwned =
  [ hit "svc-rust-entry" 9500 0x110 0
  , hit "svc-stock-trampoline" 9501 0x110 0
  , hit "stock-row-probe" 9502 0x110 0
  , hit "stock-row-probe" 9503 5 0x1a
  , hit "svc-stock-return" 9504 0 0
  ]

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
  where digits = showHex value ""

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
    Object objectValue -> Object (KM.insert (K.fromString "r1") (String (T.pack "0x00000009")) objectValue) : rest
    _ -> value : rest
wrongArguments [] = []

appendOrphan :: [Value] -> [Value]
appendOrphan hits = hits ++ [hit "svc-stock-trampoline" 9001 0x110 0]

removeFirst :: String -> [Value] -> [Value]
removeFirst label (value : rest)
  | field value "site" == Just (String (T.pack label)) = rest
  | otherwise = value : removeFirst label rest
removeFirst _ [] = []

field :: Value -> String -> Maybe Value
field (Object objectValue) name = KM.lookup (K.fromString name) objectValue
field _ _ = Nothing

mutatePackage :: FilePath -> IO ()
mutatePackage package = B.writeFile (package </> "recipe-complete.DAT") (datBytes <> BC.pack "!")

datBytes :: B.ByteString
datBytes = BC.pack "recipe-complete-dat"

kernelBytes :: B.ByteString
kernelBytes = BC.pack "kernel-bin"

archiveBytes :: B.ByteString
archiveBytes = BC.pack "bank-archive"

factsBytes :: B.ByteString
factsBytes = BC.pack "build-facts"

datSha, kernelSha, runtimeSha, archiveSha, factsSha :: String
datSha = digest datBytes
kernelSha = digest kernelBytes
runtimeSha = digest runtime
archiveSha = digest archiveBytes
factsSha = digest factsBytes

digest :: B.ByteString -> String
digest = sha256Hex . sha256

relinkManifest :: Value
relinkManifest =
  object
    [ "schema" .= (1 :: Int)
    , "base" .= hex (fromIntegral base)
    , "size" .= size
    , "runtime_sha256" .= runtimeSha
    , "entry" .= object
        [ "install" .= hexWord (installEntry entries)
        , "rollback" .= hexWord (rollbackEntry entries)
        , "status" .= hexWord (statusEntry entries)
        ]
    , "publications" .= [publication index | index <- [0 .. publicationTotal - 1]]
    , "source_identity" .= object
        [ "rust_object_replacement_bank" .= archiveSha
        , "rust_toolchain_replacement_bank" .= factsSha
        ]
    ]

publicationTotal :: Int
publicationTotal = 6

publication :: Int -> Value
publication index =
  object
    [ "index" .= index
    , "phase" .= (if index < 3 then "setup" else "activation" :: String)
    , "address" .= hex (0x011122c0 + 4 * index)
    , "old" .= ("0x00000000" :: String)
    , "new" .= ("0x00000000" :: String)
    , "core" .= ("cpu0" :: String)
    ]

hexWord :: Address -> String
hexWord (Address value) = hex (fromIntegral value)

packageManifest :: Value
packageManifest =
  object
    [ "schema" .= ("recipe-executable-package-1" :: String)
    , "firmware_sha256" .= datSha
    , "executable_sha256" .= kernelSha
    , "executable_start" .= ("0x00be1f00" :: String)
    , "installable" .= False
    , "inputs" .=
        [ object ["path" .= ("runfiles/bank.a" :: String), "sha256" .= archiveSha]
        , object ["path" .= ("runfiles/build-facts.txt" :: String), "sha256" .= factsSha]
        ]
    ]

e2eResult :: Variant -> Value
e2eResult variant =
  object
    [ "schema" .= (1 :: Int)
    , "outcome" .= variantOutcome variant
    , "runtime" .=
        object
          ([ "bytes" .= size
           , "publications" .= publicationTotal
           , "manifest" .= ("relinked/ram-push-manifest.json" :: String)
           ] ++ [ "symbols_sha256" .= digest symbols | not (variantOmitSymbols variant) ])
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
        [ object ["phase" .= ("service-field-list" :: String), "trace_hits" .= variantHits variant]
        , object ["phase" .= ("service-field-reopen" :: String), "trace_hits" .= variantReopenHits variant]
        , object ["phase" .= ("service-after-unload" :: String), "trace_hits" .= ([] :: [Value])]
        , object [ "phase" .= ("service-after-unload-stock" :: String)
                 , "trace_hits" .= ([hit "stock-row-probe" 9000 0 0] ++ variantAfterHits variant)
                 ]
        , object [ "phase" .= ("service-unrelated-after-unload" :: String)
                 , "trace_hits" .= variantUnrelatedHits variant
                 ]
        ]
    , "trajectories" .=
        [ object ["name" .= ("service-page-cancel" :: String), "result" .= ("matched" :: String)]
        , object ["name" .= ("service-field-reopen" :: String), "result" .= ("matched" :: String)]
        ]
    , "unload" .= ("publication rollback observed" :: String)
    , "mailbox" .= object
        [ "install" .= operation 1
        , "rollback" .= operation 2
        ]
    , "declared_trace_sites" .= variantSites variant
    ]

operation :: Int -> Value
operation op =
  object
    [ "operation" .= op
    , "status" .= (0 :: Int)
    , "peer_status" .= (0 :: Int)
    , "validated" .= publicationTotal
    , "restored" .= publicationTotal
    ]
