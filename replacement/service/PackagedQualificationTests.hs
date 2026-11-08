{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless, when)
import Data.Aeson (Value (..), encode, object, toJSON, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.List (isPrefixOf)
import qualified Data.Text as T
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host (withTemporary)
import FujiAF.RamPush (Address (..), RamPushEntries (..), afRingPayload, payloadAllocationSize, payloadEntries)
import Replacement.Service.Packaged
import Numeric (showHex)
import System.Directory (createDirectoryIfMissing, listDirectory, removeFile)
import System.Environment (getArgs)
import System.Posix.Files (setFileMode)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import Data.Word (Word32)

data Variant = Variant
  { variantHits :: [Value]
  , variantReopenHits :: [Value]
  , variantSites :: [Value]
  , variantOutcome :: String
  , variantAfterHits :: [Value]
  , variantUnrelatedHits :: [Value]
  , variantOmitSymbols :: Bool
  , variantControlFile :: Bool
  , variantStartDat :: Maybe String
  , variantMutatePackage :: Maybe (FilePath -> IO ())
  , variantForm :: ObservationForm
  }

-- | How the receipt names its observations: the legacy inline array, or the current reference to an
-- observation document plus its trace-hit sidecar.
data ObservationForm = InlineForm | ReferenceForm ReferenceDefect

data ReferenceDefect
  = ReferenceClean
  | ReferenceChangedEvent
  | ReferenceBadDigest
  | ReferenceBadBytes
  | ReferenceBadTraceCount
  | ReferenceBadTraceDigest
  | ReferenceBadInlineTraceCount
  | ReferenceMissingFile
  | ReferenceTruncatedFile
  | ReferenceEscapingPath
  deriving (Eq)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ["--fixture-dir", directory] -> emitFixture directory
    [] -> withTemporary "packaged-qualification" $ \root -> do
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
      refuses root "start DAT differs from typed reference" (baselineVariant { variantStartDat = Just (digest (BC.pack "other-reference")) })
      refuses root "owned entry after service-after-unload" (baselineVariant { variantAfterHits = afterUnloadOwned })
      refuses root "owned entry in a later unrelated phase" (baselineVariant { variantUnrelatedHits = afterUnloadOwned })
      referenceChecks root
      putStrLn "packaged qualification tests: passed"
    _ -> fail "usage: packaged-qualification-tests [--fixture-dir DIR]"

-- | Writes both receipt forms for out-of-process CLI checks.
emitFixture :: FilePath -> IO ()
emitFixture directory = do
  createDirectoryIfMissing True directory
  writeFixture (directory </> "referenced") (baselineVariant { variantForm = ReferenceForm ReferenceClean })
  writeFixture (directory </> "inline") baselineVariant
  putStrLn ("synthetic fixtures written to " ++ directory)

writeFixture :: FilePath -> Variant -> IO ()
writeFixture directory variant = do
  outcome <- load directory variant
  case outcome of
    Right report
      | packagedStatus report == PackagedQualified -> pure ()
      | otherwise -> fail ("fixture did not qualify: " ++ show (packagedProblems report))
    Left problem -> fail problem

baselineVariant :: Variant
baselineVariant = Variant baselineHits reopenHits declaredSites "candidate-qualified" [] [] False False Nothing Nothing InlineForm

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

referenceChecks :: FilePath -> IO ()
referenceChecks root = do
  inline <- load root baselineVariant
  referenced <- load root (baselineVariant { variantForm = ReferenceForm ReferenceClean })
  case (inline, referenced) of
    (Right inlineReport, Right referencedReport) -> do
      check "a referenced receipt qualifies" (packagedStatus referencedReport == PackagedQualified)
      check "inline and referenced receipts report the same service evidence"
        (sameEvidence inlineReport referencedReport)
    (Left problem, _) -> fail problem
    (_, Left problem) -> fail problem
  refusesReference root "changed service event in a referenced receipt" ReferenceChangedEvent
  refusesReference root "changed observation digest" ReferenceBadDigest
  refusesReference root "changed observation byte count" ReferenceBadBytes
  refusesReference root "changed trace-hit count" ReferenceBadTraceCount
  refusesReference root "changed trace-hit digest" ReferenceBadTraceDigest
  refusesReference root "changed inline trace-hit count" ReferenceBadInlineTraceCount
  refusesReference root "missing observation document" ReferenceMissingFile
  refusesReference root "truncated observation document" ReferenceTruncatedFile
  refusesReference root "observation path outside the receipt" ReferenceEscapingPath

sameEvidence :: PackagedReport -> PackagedReport -> Bool
sameEvidence left right =
  and
    [ packagedStatus left == packagedStatus right
    , packagedRows left == packagedRows right
    , packagedSequences left == packagedSequences right
    , packagedRetainedHits left == packagedRetainedHits right
    , packagedProblems left == packagedProblems right
    , packagedPhases left == packagedPhases right
    ]

refusesReference :: FilePath -> String -> ReferenceDefect -> IO ()
refusesReference root label defect = do
  outcome <- load root (baselineVariant { variantForm = ReferenceForm defect })
  case outcome of
    Left problem -> fail (label ++ " was not a reportable refusal: " ++ problem)
    Right report -> check (label ++ " is refused") (packagedStatus report == PackagedRefused)

check :: String -> Bool -> IO ()
check label ok = unless ok (putStrLn ("FAIL " ++ label) >> exitFailure) >> putStrLn ("ok " ++ label)

load :: FilePath -> Variant -> IO (Either String PackagedReport)
load root variant = do
  let receipt = root </> "receipt"
      package = root </> "package"
  createDirectoryIfMissing True (receipt </> "relinked")
  createDirectoryIfMissing True (package </> "kernel")
  clearReceipt receipt
  privateWrite (receipt </> "relinked" </> "kernel.syms") symbols
  privateWrite (receipt </> "relinked" </> "runtime.bin") runtime
  privateWriteLazy (receipt </> "relinked" </> "ram-push-manifest.json") (encode relinkManifest)
  privateWrite (package </> "recipe-complete.DAT") datBytes
  privateWrite (package </> "kernel" </> "kernel.bin") kernelBytes
  createDirectoryIfMissing True (package </> "runfiles")
  privateWrite (package </> "runfiles" </> "bank.a") archiveBytes
  privateWrite (package </> "runfiles" </> "build-facts.txt") factsBytes
  privateWrite (root </> "reference.DAT") referenceBytes
  privateWriteLazy (package </> "recipe-complete.json") (encode packageManifest)
  case variantMutatePackage variant of
    Nothing -> pure ()
    Just mutation -> mutation package
  observations <- observationsValue receipt variant
  privateWriteLazy (receipt </> "e2e-result.json") (encode (e2eResult variant observations))
  when (variantControlFile variant) $
    privateWriteLazy (receipt </> "e2e-control.json") (encode (object ["outcome" .= ("control-refused" :: String)]))
  reference <- loadReferenceDat (root </> "reference.DAT")
  qualifyPackaged (PackagedInputs receipt package reference)

clearReceipt :: FilePath -> IO ()
clearReceipt directory = do
  names <- listDirectory directory
  mapM_ (removeFile . (directory </>))
    [ name
    | name <- names
    , "e2e-observation-" `isPrefixOf` name
        || name `elem` ["e2e-control.json", "e2e-result.json", "e2e-failure.json"]
    ]

namedObservations :: Variant -> [(String, [Value])]
namedObservations variant =
  [ ("service-field-list", variantHits variant)
  , ("service-field-reopen", variantReopenHits variant)
  , ("service-after-unload", [])
  , ("service-after-unload-stock", hit "stock-row-probe" 9000 0 0 : variantAfterHits variant)
  , ("service-unrelated-after-unload", variantUnrelatedHits variant)
  ]

observationsValue :: FilePath -> Variant -> IO Value
observationsValue receipt variant = case variantForm variant of
  InlineForm ->
    pure (toJSON [object ["phase" .= phase, "trace_hits" .= hits] | (phase, hits) <- namedObservations variant])
  ReferenceForm defect ->
    toJSON <$> mapM (writeObservation receipt defect) (zip [0 :: Int ..] (namedObservations variant))

-- | Writes one observation document in the producer's current form, then returns the receipt
-- reference. A sidecar reference names the sidecar; an inline document reference names the
-- document itself. A defect applies to the observation its case names.
writeObservation :: FilePath -> ReferenceDefect -> (Int, (String, [Value])) -> IO Value
writeObservation receipt defect (index, (phase, hits)) = do
  let stem = "e2e-observation-" ++ show index
      chosen = if index == 0 then defectHits defect hits else hits
      sidecar = stem ++ ".trace-hits.jsonl"
      document = stem ++ ".json"
      useSidecar = length chosen > 16
  documentTrace <-
    if useSidecar
      then do
        let rows = BL.toStrict (mconcat [encode value <> "\n" | value <- chosen])
            digestText' = if index == 0 && defect == ReferenceBadTraceDigest then replicate 64 '0' else digest rows
            count = length chosen + (if index == 0 && defect == ReferenceBadTraceCount then 1 else 0)
        privateWrite (receipt </> sidecar) rows
        pure (object ["schema" .= (1 :: Int), "path" .= sidecar, "sha256" .= digestText', "count" .= count])
      else pure (toJSON chosen)
  let documentBytes = BL.toStrict (encode (object ["phase" .= phase, "trace_hits" .= documentTrace]))
      documentDigest = digest documentBytes
      selfTrace =
        object
          [ "schema" .= (1 :: Int)
          , "path" .= document
          , "sha256" .= documentDigest
          , "count" .= (length chosen + (if index == 3 && defect == ReferenceBadInlineTraceCount then 1 else 0))
          ]
      referenceTrace = if useSidecar then documentTrace else selfTrace
      reference =
        object
          [ "phase" .= phase
          , "path" .= document
          , "sha256" .= documentDigest
          , "bytes" .= B.length documentBytes
          , "trace_hits" .= referenceTrace
          ]
  case (index == 0, defect) of
    (True, ReferenceMissingFile) -> pure reference
    (True, ReferenceTruncatedFile) -> do
      privateWrite (receipt </> document) (B.take (B.length documentBytes `div` 2) documentBytes)
      pure reference
    (True, ReferenceBadDigest) -> pure (replaceField "sha256" (String (T.replicate 64 "0")) reference)
    (True, ReferenceBadBytes) -> pure (replaceField "bytes" (toJSON (B.length documentBytes + 1)) reference)
    (True, ReferenceEscapingPath) -> do
      privateWrite (receipt </> document) documentBytes
      pure (replaceField "path" (String (T.pack ("../" ++ document))) reference)
    _ -> do
      privateWrite (receipt </> document) documentBytes
      pure reference

defectHits :: ReferenceDefect -> [Value] -> [Value]
defectHits ReferenceChangedEvent = changedEvent
defectHits _ = id

-- | Changes one service event so the receipt stays internally consistent and only the service
-- predicate refuses it.
changedEvent :: [Value] -> [Value]
changedEvent [] = []
changedEvent (value : rest)
  | field value "site" == Just (String "svc-stock-return") = setField value "r0" (String "0x00000009") : rest
  | otherwise = value : changedEvent rest

replaceField :: String -> Value -> Value -> Value
replaceField name value (Object fields) = Object (KM.insert (K.fromString name) value fields)
replaceField _ _ other = other

setField :: Value -> String -> Value -> Value
setField (Object fields) name value = Object (KM.insert (K.fromString name) value fields)
setField other _ _ = other

privateWrite :: FilePath -> B.ByteString -> IO ()
privateWrite path bytes = B.writeFile path bytes >> setFileMode path 0o600

privateWriteLazy :: FilePath -> BL.ByteString -> IO ()
privateWriteLazy path bytes = BL.writeFile path bytes >> setFileMode path 0o600

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

referenceBytes :: B.ByteString
referenceBytes = BC.pack "boot-install-reference"

datSha, kernelSha, runtimeSha, archiveSha, factsSha, stockSha, referenceSha :: String
datSha = digest datBytes
kernelSha = digest kernelBytes
runtimeSha = digest runtime
archiveSha = digest archiveBytes
factsSha = digest factsBytes
stockSha = digest (BC.pack "stock-reference")
referenceSha = digest referenceBytes

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
    , "stock_sha256" .= stockSha
    , "firmware_sha256" .= datSha
    , "executable_sha256" .= kernelSha
    , "executable_start" .= ("0x00be1f00" :: String)
    , "installable" .= False
    , "inputs" .=
        [ object ["path" .= ("runfiles/bank.a" :: String), "sha256" .= archiveSha]
        , object ["path" .= ("runfiles/build-facts.txt" :: String), "sha256" .= factsSha]
        ]
    ]

e2eResult :: Variant -> Value -> Value
e2eResult variant observations =
  object
    [ "schema" .= (1 :: Int)
    , "scope" .= ("synthetic packaged-qualification fixture; no guest execution" :: String)
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
                [ "installed_dat" .= maybe referenceSha id (variantStartDat variant)
                , "card_present" .= False
                ]
            ]
        ]
    , "observations" .= observations
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
