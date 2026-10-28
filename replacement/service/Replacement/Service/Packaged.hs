{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
-- | Packaged qualification for the retained row-availability service.
--
-- The receipt consumer binds the E2E result, the relinked manifest and symbol
-- table, and the package archive to the service contract. It accepts only the
-- owned call's entry, trampoline, stock call and trampoline return; stock calls
-- outside that interval are retained for review, not used as qualification.
module Replacement.Service.Packaged
  ( PackagedInputs (..)
  , PackagedStatus (..)
  , PackagedReport (..)
  , PackagedRow (..)
  , OwnedSequence (..)
  , qualifyPackaged
  , packagedStatusLabel
  , packagedReportValue
  ) where

import Data.Aeson (FromJSON (..), Value (..), eitherDecodeStrict', withObject, (.:), (.:?), (.!=))
import qualified Data.ByteString as B
import Data.Char (isHexDigit)
import Data.List (find, nub)
import qualified Data.Map.Strict as M
import Data.Maybe (listToMaybe)
import Data.Text (Text)
import qualified Data.Text as T
import Data.Word (Word32, Word64)
import Emulation.Sha256 (sha256File, sha256Hex)
import Emulation.Symbols (SymbolTable, parseSymbolTable, resolveSymbol)
import FujiAF.Host (arr, b, n, obj, s)
import Replacement.Service.Qualify
  ( serviceContractSha256
  , serviceContractValue
  , serviceRowExpected
  , serviceRowProperty
  , serviceRowToken
  , serviceRows
  , serviceStockTarget
  , rowStateWord
  )
import System.FilePath ((</>), takeFileName)

newtype TraceLabel = TraceLabel { traceLabelText :: String } deriving (Eq, Ord, Show)
newtype TraceAddress = TraceAddress { traceAddressWord :: Word32 } deriving (Eq, Ord, Show)

traceAddressText :: TraceAddress -> String
traceAddressText (TraceAddress value) = pad (showHexWord value)
  where
    pad text = replicate (8 - length text) '0' ++ text

showHexWord :: Word32 -> String
showHexWord value
  | value < 16 = [hexDigit (fromIntegral value)]
  | otherwise = showHexWord (value `div` 16) ++ [hexDigit (fromIntegral (value `mod` 16))]
  where
    hexDigit n = "0123456789abcdef" !! n

data DeclaredTraceSite = DeclaredTraceSite
  { declaredTraceLabel :: TraceLabel
  , declaredTraceAddress :: TraceAddress
  } deriving (Eq, Show)

instance FromJSON TraceLabel where
  parseJSON = withText "trace label" (pure . TraceLabel . T.unpack)

instance FromJSON TraceAddress where
  parseJSON = withText "trace address" parseAddress8

instance FromJSON DeclaredTraceSite where
  parseJSON = withObject "declared trace site" $ \site ->
    DeclaredTraceSite
      <$> site .: "label"
      <*> site .: "address"

data TraceHit = TraceHit
  { hitSite :: TraceLabel
  , hitCore :: Int
  , hitInstructions :: Word64
  , hitR0, hitR1, hitR2, hitR3 :: Word32
  } deriving (Eq, Show)

instance FromJSON TraceHit where
  parseJSON = withObject "trace hit" $ \hit ->
    TraceHit
      <$> hit .: "site"
      <*> hit .: "core"
      <*> hit .: "instructions"
      <*> (hit .: "r0" >>= parseRegister)
      <*> (hit .: "r1" >>= parseRegister)
      <*> (hit .: "r2" >>= parseRegister)
      <*> (hit .: "r3" >>= parseRegister)

data Observation = Observation
  { observationPhase :: String
  , observationHits :: [TraceHit]
  } deriving (Eq, Show)

instance FromJSON Observation where
  parseJSON = withObject "observation" $ \observation ->
    Observation
      <$> observation .: "phase"
      <*> observation .:? "trace_hits" .!= []

data E2EOutcome = OutcomeQualified | OutcomeControl | OutcomeOther String deriving (Eq, Show)

instance FromJSON E2EOutcome where
  parseJSON = withText "outcome" $ \outcome -> pure $ case T.unpack outcome of
    "candidate-qualified" -> OutcomeQualified
    "control-refused" -> OutcomeControl
    other -> OutcomeOther other

data RuntimeFacts = RuntimeFacts
  { runtimeBytes :: Int
  , runtimePublications :: Int
  , runtimeSymbolsSha :: String
  } deriving (Eq, Show)

instance FromJSON RuntimeFacts where
  parseJSON = withObject "runtime" $ \runtime ->
    RuntimeFacts
      <$> runtime .: "bytes"
      <*> runtime .: "publications"
      <*> runtime .: "symbols_sha256"

data RelinkFacts = RelinkFacts
  { relinkMode :: String
  , relinkSourceIdentity :: M.Map String String
  } deriving (Eq, Show)

instance FromJSON RelinkFacts where
  parseJSON = withObject "relink" $ \relink ->
    RelinkFacts
      <$> relink .: "mode"
      <*> relink .: "source_identity"

data StartFacts = StartFacts
  { startInstalledDat :: String
  , startCardPresent :: Bool
  } deriving (Eq, Show)

instance FromJSON StartFacts where
  parseJSON = withObject "emulator start" $ \start -> do
    inputs <- start .: "inputs"
    withObject "emulator inputs" parseFiles inputs
    where
      parseFiles inputs = do
        files <- inputs .: "inputs"
        withObject "emulator input files" (\value ->
          StartFacts
            <$> value .: "installed_dat"
            <*> value .:? "card_present" .!= False
          ) files

data MailboxOp = MailboxOp
  { mailboxOperation :: Int
  , mailboxStatus :: Int
  , mailboxPeerStatus :: Int
  , mailboxValidated :: Maybe Int
  , mailboxRestored :: Maybe Int
  } deriving (Eq, Show)

instance FromJSON MailboxOp where
  parseJSON = withObject "mailbox operation" $ \mailbox ->
    MailboxOp
      <$> mailbox .: "operation"
      <*> mailbox .: "status"
      <*> mailbox .: "peer_status"
      <*> mailbox .:? "validated"
      <*> mailbox .:? "restored"

data Mailbox = Mailbox
  { mailboxInstall :: MailboxOp
  , mailboxRollback :: MailboxOp
  } deriving (Eq, Show)

instance FromJSON Mailbox where
  parseJSON = withObject "mailbox" $ \mailbox ->
    Mailbox
      <$> mailbox .: "install"
      <*> mailbox .: "rollback"

data TrajectoryResult = TrajectoryResult
  { trajectoryName :: String
  , trajectoryResult :: String
  } deriving (Eq, Show)

instance FromJSON TrajectoryResult where
  parseJSON = withObject "trajectory result" $ \result ->
    TrajectoryResult
      <$> result .: "name"
      <*> result .: "result"

data E2E = E2E
  { e2eOutcome :: E2EOutcome
  , e2eRuntime :: RuntimeFacts
  , e2eRelink :: RelinkFacts
  , e2eStart :: StartFacts
  , e2eObservations :: [Observation]
  , e2eTrajectories :: [TrajectoryResult]
  , e2eUnload :: Maybe String
  , e2eMailbox :: Mailbox
  , e2eDeclaredSites :: [DeclaredTraceSite]
  } deriving (Eq, Show)

instance FromJSON E2E where
  parseJSON = withObject "e2e result" $ \result ->
    E2E
      <$> result .: "outcome"
      <*> result .: "runtime"
      <*> result .: "relink"
      <*> result .: "emulator_start"
      <*> result .: "observations"
      <*> result .: "trajectories"
      <*> result .:? "unload"
      <*> result .: "mailbox"
      <*> result .: "declared_trace_sites"

data PackageInput = PackageInput
  { packageInputPath :: String
  , packageInputSha :: String
  } deriving (Eq, Show)

instance FromJSON PackageInput where
  parseJSON = withObject "package input" $ \input ->
    PackageInput
      <$> input .: "path"
      <*> input .: "sha256"

data PackageManifest = PackageManifest
  { packageFirmwareSha :: String
  , packageExecutableSha :: String
  , packageExecutableStart :: TraceAddress
  , packageInputs :: [PackageInput]
  , packageInstallable :: Bool
  } deriving (Eq, Show)

instance FromJSON PackageManifest where
  parseJSON = withObject "package manifest" $ \manifest ->
    PackageManifest
      <$> manifest .: "firmware_sha256"
      <*> manifest .: "executable_sha256"
      <*> (manifest .: "executable_start" >>= parseAddressText)
      <*> manifest .: "inputs"
      <*> manifest .: "installable"

data Publication = Publication
  { publicationAddress :: TraceAddress
  , publicationOld :: String
  , publicationNew :: String
  , publicationPhase :: String
  } deriving (Eq, Show)

instance FromJSON Publication where
  parseJSON = withObject "publication" $ \publication ->
    Publication
      <$> (publication .: "address" >>= parseAddressText)
      <*> publication .: "old"
      <*> publication .: "new"
      <*> publication .: "phase"

data RelinkManifest = RelinkManifest
  { relinkManifestBase :: TraceAddress
  , relinkManifestRuntimeSha :: String
  , relinkManifestSourceIdentity :: M.Map String String
  , relinkManifestPublications :: [Publication]
  , relinkManifestSize :: Int
  } deriving (Eq, Show)

instance FromJSON RelinkManifest where
  parseJSON = withObject "relink manifest" $ \manifest ->
    RelinkManifest
      <$> (manifest .: "base" >>= parseAddressText)
      <*> manifest .: "runtime_sha256"
      <*> manifest .: "source_identity"
      <*> manifest .: "publications"
      <*> manifest .: "size"

data PackagedInputs = PackagedInputs
  { packagedReceiptDir :: FilePath
  , packagedPackageDir :: FilePath
  } deriving (Eq, Show)

data PackagedStatus = PackagedQualified | PackagedRefused deriving (Eq, Show)

data PackagedRow = PackagedRow
  { packagedRowToken :: Word32
  , packagedRowExpected :: Word32
  , packagedRowEntries :: Int
  , packagedRowReturned :: [Word32]
  , packagedRowNested :: Int
  } deriving (Eq, Show)

data OwnedSequence = OwnedSequence
  { ownedRow :: Word32
  , ownedCore :: Int
  , ownedEntry :: TraceHit
  , ownedTrampoline :: TraceHit
  , ownedStock :: TraceHit
  , ownedNested :: [TraceHit]
  , ownedReturn :: TraceHit
  } deriving (Eq, Show)

data PackagedIdentities = PackagedIdentities
  { packagedDatSha :: String
  , packagedArchiveSha :: String
  , packagedFactsSha :: String
  , packagedExecutableSha :: String
  , packagedInstalledDat :: String
  , packagedResultSha :: String
  , packagedManifestSha :: String
  , packagedSymbolsSha :: String
  , packagedRuntimeSha :: String
  , packagedOutcome :: String
  } deriving (Eq, Show)

data PackagedReport = PackagedReport
  { packagedStatus :: PackagedStatus
  , packagedRows :: [PackagedRow]
  , packagedSequences :: [OwnedSequence]
  , packagedRetainedHits :: [TraceHit]
  , packagedProblems :: [String]
  , packagedIdentities :: PackagedIdentities
  , packagedPhases :: [String]
  , packagedUnload :: Maybe String
  } deriving (Eq, Show)

packagedStatusLabel :: PackagedStatus -> String
packagedStatusLabel PackagedQualified = "qualified"
packagedStatusLabel PackagedRefused = "refused"

qualifyPackaged :: PackagedInputs -> IO (Either String PackagedReport)
qualifyPackaged (PackagedInputs receipt packageDirectory) = do
  resultPath <- pure (receipt </> "e2e-result.json")
  manifestPath <- pure (receipt </> "relinked" </> "ram-push-manifest.json")
  symbolsPath <- pure (receipt </> "relinked" </> "kernel.syms")
  packageManifestPath <- pure (packageDirectory </> "recipe-complete.json")
  result <- readJson resultPath
  manifest <- readJson manifestPath
  packageManifest <- readJson packageManifestPath
  symbolsBytes <- B.readFile symbolsPath
  resultSha <- digestFile resultPath
  manifestSha <- digestFile manifestPath
  symbolsSha <- digestFile symbolsPath
  let table = parseSymbolTable symbolsBytes
      package = lookupInput "bank.a" packageManifest
      facts = lookupInput "build-facts.txt" packageManifest
      identities = PackagedIdentities
        { packagedDatSha = packageFirmwareSha packageManifest
        , packagedArchiveSha = maybe "" packageInputSha package
        , packagedFactsSha = maybe "" packageInputSha facts
        , packagedExecutableSha = packageExecutableSha packageManifest
        , packagedInstalledDat = startInstalledDat (e2eStart result)
        , packagedResultSha = resultSha
        , packagedManifestSha = manifestSha
        , packagedSymbolsSha = symbolsSha
        , packagedRuntimeSha = relinkManifestRuntimeSha manifest
        , packagedOutcome = outcomeLabel (e2eOutcome result)
        }
      (sequences, retained, traceProblems) = analyzeObservations result
      rowResults = rowEvidence sequences
      rowProblems = rowProblemsFor rowResults
      siteProblems = validateDeclaredSites table (e2eDeclaredSites result)
      identityProblems = identityChecks packageManifest manifest (e2eRuntime result) symbolsSha (e2eStart result)
      lifecycleProblems = lifecycleChecks result
      outcomeChecks = case e2eOutcome result of
        OutcomeQualified -> []
        _ -> ["packaged run outcome is not candidate-qualified"]
      modeChecks = [ "packaged run is not an unreviewed relink"
                   | relinkMode (e2eRelink result) /= "unreviewed" ]
      problems = nub (outcomeChecks ++ modeChecks ++ identityProblems ++ siteProblems ++ traceProblems ++ rowProblems ++ lifecycleProblems)
  pure (Right PackagedReport
    { packagedStatus = if null problems then PackagedQualified else PackagedRefused
    , packagedRows = rowResults
    , packagedSequences = sequences
    , packagedRetainedHits = retained
    , packagedProblems = problems
    , packagedIdentities = identities
    , packagedPhases = nub (observationPhases result ++ map trajectoryName (e2eTrajectories result))
    , packagedUnload = e2eUnload result
    })
  where
    lookupInput name manifest = listToMaybe
      [ input | input <- packageInputs manifest, takeFileName (packageInputPath input) == name ]

readJson :: FromJSON a => FilePath -> IO (Either String a)
readJson path = do
  bytes <- B.readFile path
  pure (either (Left . ((path ++ ": ") ++)) Right (eitherDecodeStrict' bytes))

digestFile :: FilePath -> IO String
digestFile path = sha256Hex <$> sha256File path

outcomeLabel :: E2EOutcome -> String
outcomeLabel OutcomeQualified = "candidate-qualified"
outcomeLabel OutcomeControl = "control-refused"
outcomeLabel (OutcomeOther value) = value

observationPhases :: E2E -> [String]
observationPhases = map observationPhase . e2eObservations

entryLabel, trampolineLabel, returnLabel, stockLabel :: TraceLabel
entryLabel = TraceLabel "svc-rust-entry"
trampolineLabel = TraceLabel "svc-stock-trampoline"
returnLabel = TraceLabel "svc-stock-return"
stockLabel = TraceLabel "stock-row-probe"

selectedRows :: [ServiceRow]
selectedRows = filter ((== 0x110) . serviceRowProperty) serviceRows

selectedRow :: Word32 -> Maybe ServiceRow
selectedRow token = find ((== token) . serviceRowToken) selectedRows

rowProblemsFor :: [PackagedRow] -> [String]
rowProblemsFor rows = concat
  [ [ "selected row " ++ show (packagedRowToken row) ++ " was entered " ++ show (packagedRowEntries row) ++ " times; repeat entry requires at least two"
    | packagedRowEntries row < 2 ]
    ++ [ "selected row " ++ show (packagedRowToken row) ++ " returned "
           ++ show (packagedRowReturned row) ++ ", expected " ++ show (packagedRowExpected row)
       | any (/= packagedRowExpected row) (packagedRowReturned row) ]
    ++ [ "selected row " ++ show (packagedRowToken row) ++ " has no owned sequence"
       | packagedRowEntries row == 0 ]
  | row <- rows
  ]
    ++ [ "required selected row count is " ++ show (length selectedRows) ++ ", observed " ++ show (length rows)
       | length rows /= length selectedRows
       ]

rowEvidence :: [OwnedSequence] -> [PackagedRow]
rowEvidence sequences =
  [ PackagedRow
      { packagedRowToken = serviceRowToken row
      , packagedRowExpected = rowStateWord (serviceRowExpected row)
      , packagedRowEntries = length owned
      , packagedRowReturned = map (hitR0 . ownedReturn) owned
      , packagedRowNested = sum (map (length . ownedNested) owned)
      }
  | row <- selectedRows
  , let owned = [sequence | sequence <- sequences, ownedRow sequence == serviceRowToken row]
  ]

analyzeObservations :: E2E -> ([OwnedSequence], [TraceHit], [String])
analyzeObservations result =
  let (sequences, retained, problems) = foldl step ([], [], []) (e2eObservations result)
      step (seqs, outside, issues) observation =
        let (found, rest, problems) = analyzeHits (observationHits observation)
         in (seqs ++ found, outside ++ rest, issues ++ map ((observationPhase observation ++ ": ") ++) problems)
   in (sequences, retained, problems)

analyzeHits :: [TraceHit] -> ([OwnedSequence], [TraceHit], [String])
analyzeHits = go [] []
  where
    go sequences retained [] = (sequences, retained, [])
    go sequences retained (hit : rest)
      | hitSite hit == entryLabel =
          case parseSequence (hit : rest) of
            Left problem -> (sequences, retained, [problem])
            Right (sequence, remaining) -> go (sequences ++ [sequence]) retained remaining
      | otherwise = go sequences (retained ++ [hit]) rest

parseSequence :: [TraceHit] -> Either String (OwnedSequence, [TraceHit])
parseSequence (entry : rest) =
  case selectedRow (hitR1 entry) of
    Just row | hitR0 entry == 0x110 ->
      case expect trampolineLabel rest of
        Left problem -> Left problem
        Right (trampoline, rest1) ->
          case expect stockLabel rest1 of
            Left problem -> Left problem
            Right (stock, rest2) ->
              let (nested, rest3) = span isNested rest2
                  ordered = entry : trampoline : stock : nested
                  expected = rowStateWord (serviceRowExpected row)
               in case expect returnLabel rest3 of
                    Left problem -> Left problem
                    Right (returned, rest4) ->
                      let problems =
                            [ "svc-stock-trampoline arguments differ from svc-rust-entry"
                            | argumentPair trampoline /= argumentPair entry ]
                              ++ [ "stock-row-probe outer arguments differ from svc-rust-entry"
                                 | argumentPair stock /= argumentPair entry ]
                              ++ [ "0x110 owned call must contain one or two grounded FUN_00d52684(5,0x1a) nested calls"
                                 | length nested < 1 || length nested > 2 ]
                              ++ [ "owned service trace crosses cores"
                                 | length (nub (map hitCore (returned : ordered))) /= 1 ]
                              ++ [ "owned service trace instruction counts are not strictly increasing"
                                 | not (strictlyIncreasing (map hitInstructions (ordered ++ [returned]))) ]
                              ++ [ "svc-stock-return returned " ++ show (hitR0 returned) ++ ", expected " ++ show expected
                                 | hitR0 returned /= expected ]
                       in case problems of
                            [] -> Right
                              ( OwnedSequence
                                  { ownedRow = serviceRowToken row
                                  , ownedCore = hitCore entry
                                  , ownedEntry = entry
                                  , ownedTrampoline = trampoline
                                  , ownedStock = stock
                                  , ownedNested = nested
                                  , ownedReturn = returned
                                  }
                              , rest4
                              )
                            problem : _ -> Left problem
    _ -> Left "svc-rust-entry arguments are outside the five selected 0x110 rows"
parseSequence _ = Left "svc-rust-entry is missing"

expect :: TraceLabel -> [TraceHit] -> Either String (TraceHit, [TraceHit])
expect label (hit : rest)
  | hitSite hit == label = Right (hit, rest)
  | otherwise = Left ("expected " ++ traceLabelText label ++ ", observed " ++ traceLabelText (hitSite hit))
expect label [] = Left ("missing " ++ traceLabelText label)

isNested :: TraceHit -> Bool
isNested hit = hitSite hit == stockLabel && argumentPair hit == (5, 0x1a)

argumentPair :: TraceHit -> (Word32, Word32)
argumentPair hit = (hitR0 hit, hitR1 hit)

strictlyIncreasing :: [Word64] -> Bool
strictlyIncreasing values = and (zipWith (<) values (drop 1 values))

validateDeclaredSites :: SymbolTable -> [DeclaredTraceSite] -> [String]
validateDeclaredSites table sites =
  let addresses = [ (TraceLabel "svc-rust-entry", symbol "rb_service_field_row_state")
                  , (TraceLabel "svc-stock-trampoline", symbol "rb_call_stock_probe")
                  , (TraceLabel "svc-stock-return", symbolPlus "rb_call_stock_probe" 0x10)
                  , (TraceLabel "stock-row-probe", Right (TraceAddress serviceStockTarget))
                  ]
      declared = [ (declaredTraceLabel site, declaredTraceAddress site) | site <- sites ]
      duplicateLabels = duplicates (map fst declared)
      duplicateAddresses = duplicates (map snd declared)
      checks = concat
        [ case resolved of
            Left problem -> [problem]
            Right expected ->
              case lookup label declared of
                Nothing -> ["declared trace site is missing: " ++ traceLabelText label]
                Just actual | actual == expected -> []
                Just actual -> [ "declared trace address for " ++ traceLabelText label ++ " is "
                                   ++ traceAddressText actual ++ ", expected " ++ traceAddressText expected ]
        | (label, resolved) <- addresses
        ]
   in checks
        ++ [ "declared trace labels are not unique: " ++ show labels | not (null duplicateLabels), let labels = duplicateLabels ]
        ++ [ "declared trace addresses are not unique: " ++ show [ traceAddressText value | value <- duplicateAddresses ] | not (null duplicateAddresses) ]
  where
    symbol name = case resolveSymbol table name of
      Left problem -> Left problem
      Right address -> Right (TraceAddress address)
    symbolPlus name offset = case resolveSymbol table name of
      Left problem -> Left problem
      Right address
        | address > maxBound - offset -> Left ("runtime symbol " ++ name ++ " overflows the return offset")
        | otherwise -> Right (TraceAddress (address + offset))
    duplicates values = [ value | value <- nub values, length (filter (== value) values) > 1 ]

identityChecks :: PackageManifest -> RelinkManifest -> RuntimeFacts -> String -> StartFacts -> [String]
identityChecks package manifest runtime symbolsSha start =
  [ "package has no bank archive input" | lookupInput "bank.a" package == Nothing ]
    ++ [ "package has no build-facts input" | lookupInput "build-facts.txt" package == Nothing ]
    ++ [ "relinked symbols digest differs from the E2E symbols digest" | symbolsSha /= runtimeSymbolsSha runtime ]
    ++ [ "package firmware digest differs from the emulator start input" | packageFirmwareSha package /= startInstalledDat start ]
    ++ [ "emulator start input is not the cardless route" | startCardPresent start ]
    ++ [ "relinked runtime digest is empty" | null (relinkManifestRuntimeSha manifest) ]
    ++ [ "runtime publication count differs from the relink manifest" | runtimePublications runtime /= length (relinkManifestPublications manifest) ]
    ++ [ "runtime byte size differs from the relink manifest" | runtimeBytes runtime /= relinkManifestSize manifest ]
    ++ [ "relinked archive identity is missing" | M.lookup "rust_object_replacement_bank" (relinkManifestSourceIdentity manifest) == Nothing ]
    ++ [ "relinked toolchain identity is missing" | M.lookup "rust_toolchain_replacement_bank" (relinkManifestSourceIdentity manifest) == Nothing ]
    ++ [ "relinked archive identity differs from the package archive"
       | Just value <- [M.lookup "rust_object_replacement_bank" (relinkManifestSourceIdentity manifest)]
       , Just input <- [lookupInput "bank.a" package]
       , value /= packageInputSha input ]
    ++ [ "relinked toolchain identity differs from the package facts"
       | Just value <- [M.lookup "rust_toolchain_replacement_bank" (relinkManifestSourceIdentity manifest)]
       , Just input <- [lookupInput "build-facts.txt" package]
       , value /= packageInputSha input ]
  where
    lookupInput name value = listToMaybe
      [ input | input <- packageInputs value, takeFileName (packageInputPath input) == name ]

lifecycleChecks :: E2E -> [String]
lifecycleChecks result =
  [ "packaged trajectory did not match service-page-cancel"
  | not (trajectoryMatched "service-page-cancel") ]
    ++ [ "packaged trajectory did not match service-field-reopen"
       | not (trajectoryMatched "service-field-reopen") ]
    ++ [ "packaged trajectory has no matching service-after-unload observation"
       | not (any ((== "service-after-unload") . observationPhase) (e2eObservations result)) ]
    ++ [ "packaged trajectory has no matching post-unload stock observation"
       | not (any ((== "service-after-unload-stock") . observationPhase) (e2eObservations result)) ]
    ++ [ "post-unload stock observation has no stock probe activity"
       | not (postUnloadStockActivity result) ]
    ++ [ "packaged run has no unload outcome" | maybe True (null) (e2eUnload result) ]
    ++ mailboxProblems
  where
    trajectoryMatched name =
      any (\trajectory -> trajectoryName trajectory == name && trajectoryResult trajectory == "matched")
        (e2eTrajectories result)
    postUnloadStockActivity result' =
      any (\observation -> observationPhase observation == "service-after-unload-stock"
              && any ((== stockLabel) . hitSite) (observationHits observation))
          (e2eObservations result')
    mailbox = e2eMailbox result
    publications = runtimePublications (e2eRuntime result)
    install = mailboxInstall mailbox
    rollback = mailboxRollback mailbox
    mailboxProblems =
      [ "INSTALL mailbox is not a successful publication of every manifest word"
      | mailboxOperation install /= 1 || mailboxStatus install /= 0 || mailboxPeerStatus install /= 0
          || mailboxValidated install /= Just publications
      ]
      ++ [ "rollback mailbox is not a successful restoration of every publication"
         | mailboxOperation rollback /= 2 || mailboxStatus rollback /= 0 || mailboxPeerStatus rollback /= 0
             || mailboxRestored rollback /= Just publications
         ]

parseAddress8 :: Text -> Parser TraceAddress
parseAddress8 text
  | T.length text == 8
  , all isHexDigit (T.unpack text)
  , [(value, "")] <- readHex (T.unpack text) = pure (TraceAddress value)
  | otherwise = fail ("trace address must be eight hex digits without 0x: " ++ T.unpack text)
  where
    readHex value = case reads ("0x" ++ value) of
      [(number, "")] -> [(number, "")]
      _ -> []

parseAddressText :: Text -> Parser TraceAddress
parseAddressText text = case T.stripPrefix "0x" text of
  Just digits | not (null digits), all isHexDigit (T.unpack digits), length (T.unpack digits) <= 8 ->
    case reads ("0x" ++ T.unpack digits) of
      [(value, "")] -> pure (TraceAddress value)
      _ -> fail ("invalid hex address: " ++ T.unpack text)
  _ -> fail ("hex address requires 0x: " ++ T.unpack text)

parseRegister :: Value -> Parser Word32
parseRegister (String text) = parseAddressText text
parseRegister _ = fail "register value must be a hex string"

-- | The report keeps the existing service contract value and adds only the
-- packaged evidence this consumer owns.
packagedReportValue :: PackagedReport -> Value
packagedReportValue report =
  obj
    [ ("schema_version", n 1)
    , ("profile", s "service-packaged")
    , ("contract_sha256", s serviceContractSha256)
    , ("contract", serviceContractValue)
    , ("status", s (packagedStatusLabel (packagedStatus report)))
    , ("capability", b (packagedStatus report == PackagedQualified))
    , ("required_rows", n (fromIntegral (length selectedRows)))
    , ("qualified_rows", n (fromIntegral (length [ row | row <- packagedRows report, packagedRowEntries row >= 2, all (== packagedRowExpected row) (packagedRowReturned row) ])))
    , ("rows", arr (map rowValue (packagedRows report)))
    , ("sequences", arr (map sequenceValue (packagedSequences report)))
    , ("retained_stock_hits", arr (map hitValue (packagedRetainedHits report)))
    , ("identities", identitiesValue (packagedIdentities report))
    , ("phases", arr (map s (packagedPhases report)))
    , ("unload", maybe Null s (packagedUnload report))
    , ("problems", arr (map s (packagedProblems report)))
    , ("evidence_limits", arr (map s evidenceLimits))
    ]
  where
    rowValue row = obj
      [ ("property", n 272)
      , ("row", n (fromIntegral (packagedRowToken row)))
      , ("expected", n (fromIntegral (packagedRowExpected row)))
      , ("entries", n (fromIntegral (packagedRowEntries row)))
      , ("returned", arr (map (n . fromIntegral) (packagedRowReturned row)))
      , ("nested_calls", n (fromIntegral (packagedRowNested row)))
      ]
    sequenceValue sequence = obj
      [ ("row", n (fromIntegral (ownedRow sequence)))
      , ("core", n (fromIntegral (ownedCore sequence)))
      , ("entry_instructions", n (fromIntegral (hitInstructions (ownedEntry sequence))))
      , ("return_instructions", n (fromIntegral (hitInstructions (ownedReturn sequence))))
      , ("nested_calls", n (fromIntegral (length (ownedNested sequence))))
      , ("returned", n (fromIntegral (hitR0 (ownedReturn sequence))))
      ]
    hitValue hit = obj
      [ ("site", s (traceLabelText (hitSite hit)))
      , ("core", n (fromIntegral (hitCore hit)))
      , ("instructions", n (fromIntegral (hitInstructions hit)))
      , ("r0", s (traceAddressText (TraceAddress (hitR0 hit))))
      , ("r1", s (traceAddressText (TraceAddress (hitR1 hit))))
      ]
    identitiesValue identities = obj
      [ ("package_dat_sha256", s (packagedDatSha identities))
      , ("package_archive_sha256", s (packagedArchiveSha identities))
      , ("package_facts_sha256", s (packagedFactsSha identities))
      , ("package_executable_sha256", s (packagedExecutableSha identities))
      , ("installed_dat_sha256", s (packagedInstalledDat identities))
      , ("e2e_result_sha256", s (packagedResultSha identities))
      , ("relinked_manifest_sha256", s (packagedManifestSha identities))
      , ("relinked_symbols_sha256", s (packagedSymbolsSha identities))
      , ("runtime_sha256", s (packagedRuntimeSha identities))
      , ("outcome", s (packagedOutcome identities))
      ]
    evidenceLimits =
      [ "Only the five selected 0x110 rows are packaged qualification rows; stock probe calls outside each owned entry/return are retained for review."
      , "The 0x110 body's grounded FUN_00d52684(5,0x1a) nested call is allowed once or twice; any other stock event inside the owned interval refuses."
      , "rb_service_field_row_state tail-branches; rb_call_stock_probe+0x10 is the retained-call return."
      , "Page-visit cancellation is evidenced by service-page-cancel and service-field-reopen; cancellation inside the synchronous call and handback remain not applicable."
      , "No camera, hardware, visual or final-byte review is claimed."
      ]
