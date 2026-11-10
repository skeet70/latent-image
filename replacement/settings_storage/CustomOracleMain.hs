{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import qualified BankRecordNative as Join
import Control.Exception (AsyncException, Exception, IOException, SomeException, evaluate, fromException, throwIO, try)
import Control.Monad (forM, forM_, unless)
import qualified ControlArtifact as Control
import CustomAdmission (admitCustomBoundaries, bankEntries)
import qualified CustomPreemptionNative as Preemption
import CustomNative
import Data.Aeson (Value(..), eitherDecodeStrict', object, toJSON, withObject, (.:), (.=))
import Data.Aeson.Types (Pair, parseEither)
import qualified Data.Aeson.KeyMap as K
import qualified Data.ByteString as B
import Data.Either (isLeft)
import Data.List (isPrefixOf, nub)
import Data.Maybe (listToMaybe, catMaybes)
import Data.IORef (newIORef, readIORef, writeIORef)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.CachedNativeBatch (BatchReuse(..), CacheBinding, cacheBindingEntry, cacheBindingManifestSha256, cacheOnlyRunner)
import Emulation.NativeObservation
import qualified Emulation.NativeEvidence as E
import Emulation.OwnedCase
import Emulation.ParallelCases (mapNativeBounded, readNativeJobs)
import Emulation.Sha256 (Sha256, sha256, sha256File, sha256Hex, parseSha256Hex)
import FujiAF.Host (ensure, readRegular)
import FujiAF.JSON (encodeCanonical)
import FujiAF.StorageBudget (checkStorageBudget)
import FujiModNativePaths (fujimodNativeCase)
import qualified Native as Bank
import PropertyFields (PropertyId(..))
import qualified Replacement.PropertyService.Artifact as Artifact
import Replacement.PropertyService.Classify (Class(..), Property(..))
import qualified Replacement.PropertyService.Oracle as Property
import qualified Replacement.PropertyService.Placement as Placement
import qualified Replacement.RecordStore.Cases as C
import qualified Replacement.RecordStore.Harness as H
import qualified Replacement.RecordStore.Carrier as RecordCarrier
import qualified SettingsEeNative as Ee
import StorageRun (NativeCache, NativeBatchFailure(..), openNativeCache, cacheJobs, cachedBatch, cacheExecutionValue, batchReuseLabel, runSpecs, runCachedSpecs, removeStaged, readResidueProof)
import System.Directory (canonicalizePath, createDirectoryIfMissing, doesFileExist, doesPathExist, getCurrentDirectory, makeAbsolute)
import System.Environment (getArgs, getExecutablePath, lookupEnv)
import System.Exit (ExitCode(..), exitFailure)
import System.FilePath ((</>))
import System.IO.Error (ioeGetFileName)
import System.Process (readProcessWithExitCode)
import Text.Read (readMaybe)

runnerArgument :: [String] -> IO FilePath
runnerArgument arguments = case dropWhile (/= "--runner") arguments of
  [] -> pure fujimodNativeCase
  [_] -> ioError (userError "--runner needs a value")
  _ : value : _ -> makeAbsolute value

stockRunnerArgument :: [String] -> FilePath -> IO FilePath
stockRunnerArgument arguments fallback = case dropWhile (/= "--stock-runner") arguments of
  [] -> pure fallback
  [_] -> ioError (userError "--stock-runner needs a value")
  _ : value : _ -> makeAbsolute value

data JoinedStockRunner = JoinedStockRunner FilePath (Maybe Sha256)

joinedStockRunnerArgument :: [String] -> FilePath -> IO JoinedStockRunner
joinedStockRunnerArgument arguments fallback = do
  path <- if "--joined-stock-runner" `elem` arguments
    then required arguments "--joined-stock-runner" >>= makeAbsolute
    else pure fallback
  digest <- if "--joined-stock-runner-cache-sha256" `elem` arguments
    then do
      ensure ("--joined-stock-runner" `elem` arguments) "cache-only joined runner requires its explicit frozen path"
      value <- required arguments "--joined-stock-runner-cache-sha256"
      Just <$> either fail pure (parseSha256Hex value)
    else pure Nothing
  pure (JoinedStockRunner path digest)

optionalArgument :: String -> [String] -> Maybe FilePath
optionalArgument flag arguments = case dropWhile (/= flag) arguments of
  _ : value : _ -> Just value
  _ -> Nothing

readSymbols :: FilePath -> IO (M.Map String Symbol)
readSymbols path = do
  rows <- map words . lines <$> readFile path
  let names = [name | [_,_,kind,name] <- rows,kind `elem` ["T","t","A","a"] || name `elem` ["SLOT_BYTES","TAIL_BYTES"]]
  ensure (not (null names) && length names == length (nub names)) "missing or duplicate custom symbols"
  parseSymbols names path

readFunctions :: FilePath -> IO [H.Function]
readFunctions path = do
  rows <- map words . lines <$> readFile path
  let functions = [H.Function address size | ("range":_:_:_:start:lengthBytes:_) <- rows,Just address <- [readMaybe start],Just size <- [readMaybe lengthBytes],address >= 0x61c400,address < 0x89ec28,address < 0x89aa90 || address >= 0x89abe0]
  ensure (not (null functions)) "decomp index has no image4 functions"
  pure (H.Function 0x89aa90 0x128 : H.Function 0x89abb8 0x20 : functions)

patchRuntimeStubs :: FilePath -> [CaseSpec] -> IO ()
patchRuntimeStubs directory specs = forM_ specs $ \spec -> do
  let path = directory </> caseId spec ++ ".case.json"
  bytes <- B.readFile path
  value <- either fail pure (eitherDecodeStrict' bytes)
  let stubs = [object ["address" .= hexWord32 address,"inputs" .= object [],"outputs" .= object ["r0" .= hexWord32 0],"side_effects" .= ("registers-only" :: String)] | address <- [0x724b68,0x724c00,0x42634,0x4263c]]
  case value of
    Object fields -> B.writeFile path (encodeCanonical (Object (K.insert "limits" (object ["instructions" .= (4000000::Int),"wall_ms" .= (10000::Int)]) (K.insert "stubs" (toJSON stubs) fields))))
    _ -> fail "native case is not an object"

data RoleCase = RoleCase
  { roleSpec :: CaseSpec
  , roleObservation :: CustomObservation
  , roleDirectory :: FilePath
  , roleBinding :: CacheBinding
  }

runRole :: NativeCache -> FilePath -> FilePath -> [CaseSpec] -> IO [RoleCase]
runRole cache runner directory specs = do
  createDirectoryIfMissing True directory
  E.withInputPool directory $ \pool -> mapNativeBounded (cacheJobs cache) (\(index,spec) -> do
    checkStorageBudget directory
    let batchDirectory=directory </> show index
    batch <- writeBatchWithInputPool pool batchDirectory [spec]
    patchRuntimeStubs batchDirectory [spec]
    attempt <- try (cachedBatch cache runner batch (CaseId (caseId spec)))
      :: IO (Either SomeException (BatchReuse,[(CaseId,NativeObservation)],Maybe CacheBinding))
    case attempt of
      Left exception -> do
        let failure = encodeCanonical (object
              ["schema_version" .= (1::Int),"case" .= caseId spec,"status" .= ("incomplete"::String)
              ,"error" .= show exception,"raw_cache" .= Null])
        B.writeFile (batchDirectory </> caseId spec ++ ".consumer-failure.json") failure
        B.writeFile (batchDirectory </> "summary.json") failure
        throwIO exception
      Right (reuse,observed,binding) -> do
        ensure (map (\(CaseId name,_) -> name) observed == [caseId spec]) "custom native observations differ in count/order"
        observation <- case observed of
          [(_,raw)] -> evaluate (projectCustomObservation raw)
          _ -> fail "custom native observation count differs"
        bound <- maybe (fail "successful custom case has no immutable raw-cache binding") pure binding
        batchBytes <- B.readFile batch
        let rawCache = object ["entry" .= cacheBindingEntry bound,"manifest_sha256" .= sha256Hex (cacheBindingManifestSha256 bound)]
        B.writeFile (batchDirectory </> "summary.json") (encodeCanonical (object
          ["batch_sha256" .= sha256Hex (sha256 batchBytes),"execution" .= batchReuseLabel reuse
          ,"raw_cache" .= rawCache
          ,"cases" .= [object ["case" .= caseId spec,"status" .= runnerStatusLabel (customObservationStatus observation)]]]))
        pure (RoleCase spec observation batchDirectory bound)) (zip [0::Int ..] specs)

finishRole :: RoleCase -> [String] -> IO ()
finishRole role differences = do
  let rawCache = object ["entry" .= cacheBindingEntry (roleBinding role),"manifest_sha256" .= sha256Hex (cacheBindingManifestSha256 (roleBinding role))]
      failure = encodeCanonical (object
        ["schema_version" .= (1::Int),"case" .= caseId (roleSpec role),"status" .= ("failed"::String)
        ,"differences" .= differences,"raw_cache" .= rawCache])
  if null differences
    then removeStaged (roleDirectory role)
    else B.writeFile (roleDirectory role </> caseId (roleSpec role) ++ ".consumer-failure.json") failure

mutateFunction :: Word32 -> M.Map String Symbol -> String -> B.ByteString -> Either String B.ByteString
mutateFunction base symbols name bytes = case M.lookup name symbols of
  Just (Symbol address size) | size >= 8 && address >= base && toInteger address + 8 <= toInteger base + toInteger (B.length bytes) ->
    let at = fromIntegral (address-base) in Right (B.take at bytes <> word32le 0xe3a00000 <> word32le 0xe12fff1e <> B.drop (at+8) bytes)
  _ -> Left ("cannot mutate custom function " ++ name)

defectArguments :: [String] -> [(String,FilePath,FilePath)]
defectArguments ("--defect":name:binary:symbols:rest) = (name,binary,symbols) : defectArguments rest
defectArguments (_:rest) = defectArguments rest
defectArguments [] = []

probePathObserved :: [CustomObservation] -> Bool
probePathObserved observations = case (find "recovery-boundary-probe",find "recovery-boundary-null") of
  (Just probed,Just normal) -> case (journal probed,journal normal) of
    (Just after,Just before) -> length after > length before && drop (length after-2) after == [127,126] && restored probed && restored normal
    _ -> False
  _ -> False
  where
    find name = lookup name [(C.caseName (customName c),observation) | (c,observation) <- zip customCases observations]
    restored observation = case customBytes observation CustomCaller of
      Just bytes -> B.take 52 bytes == payloadRecord 126 && B.take 52 (B.drop 0x100 bytes) == payloadRecord 126
      Nothing -> False
    journal observation = do
      complete <- customBytes observation CustomProbes
      let bytes = B.drop 0x2000 complete
      let count = fromIntegral (readWordAt bytes 0)
      if count > 0 && 4 + count * 4 <= B.length bytes then Just [readWordAt bytes (4 + index*4) | index <- [0..count-1]] else Nothing


data PropertyInputs = PropertyInputs
  { propertySource :: Artifact.CanonicalSource
  , propertyArtifact :: Artifact.LinkedArtifact
  , propertyStagedImage :: B.ByteString
  , propertyLinkedBytes :: B.ByteString
  , propertySymbolBytes :: B.ByteString
  , propertyElfBytes :: B.ByteString
  , propertyContractBytes :: B.ByteString
  , propertyImage5Bytes :: B.ByteString
  }

readPropertyInputs :: [String] -> B.ByteString -> B.ByteString -> IO PropertyInputs
readPropertyInputs args image loader = do
  [dat,loaderB,image5,plain,image2] <- forM
    [(64*1024*1024,"--dat"),(0x20000,"--loader-b"),(0x800000,"--image5"),(0x800000,"--plain"),(0x800000,"--image2")]
    (\(bound,flag) -> required args flag >>= readRegular bound)
  source <- either (fail . show) pure (Artifact.canonicalSource dat image loader loaderB image5 plain image2)
  carrier <- maybe (fail "canonical property hull has no carrier") pure (Placement.hullCarrier (Artifact.sourceHull source))
  linked <- required args "--property-linked" >>= readRegular (Placement.carrierBytes Placement.stage4Carrier)
  symbols <- required args "--property-symbols" >>= readRegular 0x10000
  elf <- required args "--property-elf" >>= readRegular 0x400000
  artifact <- either (fail . show) pure (Artifact.deriveArtifact Artifact.ProductionArtifact (Artifact.sourceProperties source) carrier (Artifact.ArtifactInput linked symbols elf))
  staged <- required args "--property-image4" >>= readRegular 0x300000
  rederived <- either (fail . show) pure (Artifact.stagedImage4Bytes source artifact)
  ensure (staged == rederived) "adopted property image differs from canonical linked-artifact staging"
  contractBytes <- required args "--property-contract" >>= readRegular 0x1000000
  contract <- either fail pure (eitherDecodeStrict' contractBytes :: Either String Value)
  either fail pure (parseEither (withObject "adopted property contract" (\fields -> do
    schema <- fields .: "schema"
    qualification <- fields .: "qualification"
    stagedSha <- fields .: "staged_image4_sha256"
    linkedSha <- fields .: "linked_sha256"
    symbolsSha <- fields .: "symbols_sha256"
    elfSha <- fields .: "elf_sha256"
    unless (schema == (2::Int) && qualification == ("conditional-owned-property-service-stage-4"::String)) (fail "property contract schema or qualification differs")
    unless (stagedSha == sha256Hex (sha256 staged) && linkedSha == sha256Hex (sha256 linked) && symbolsSha == sha256Hex (sha256 symbols) && elfSha == sha256Hex (sha256 elf)) (fail "property contract identities differ from adopted inputs")
    )) contract)
  pure (PropertyInputs source artifact staged linked symbols elf contractBytes image5)

newtype StorageControl = StorageControl String deriving (Eq, Ord)

data VariantArtifact
  = CombinedBankArtifact StorageControl FilePath FilePath
  | AdoptedPropertyArtifact StorageControl FilePath FilePath FilePath

data BankControlResult = BankControlResult
  { bankControlName :: StorageControl
  , bankControlHolds :: Bool
  , bankControlFailures :: [(String,[String])]
  , bankControlAppendArtifact :: Maybe Control.ControlArtifact
  }

data BankQualification = BankQualification
  { bankQualificationPassed :: Bool
  , bankQualificationReceipt :: Value
  , bankQualificationFailure :: Maybe FailedPredicate
  }

variantArguments :: [String] -> Either String [VariantArtifact]
variantArguments ("--property-defect":name:binary:symbols:elf:rest) = do
  control <- parseControl name
  (AdoptedPropertyArtifact control binary symbols elf:) <$> variantArguments rest
variantArguments ("--bank-defect":name:binary:symbols:rest) = do
  control <- parseControl name
  (CombinedBankArtifact control binary symbols:) <$> variantArguments rest
variantArguments ("--bank-defect":_) = Left "incomplete --bank-defect NAME BIN SYMBOLS"
variantArguments ("--property-defect":_) = Left "incomplete --property-defect NAME BIN SYMBOLS ELF"
variantArguments (_:rest) = variantArguments rest
variantArguments [] = Right []

parseControl :: String -> Either String StorageControl
parseControl name = do
  unless (name `elem` Bank.requiredControls) (Left ("unknown storage control " ++ name))
  pure (StorageControl name)

failedCases :: [(String,[String])] -> [(String,[String])]
failedCases = filter (not . null . snd)

observedControl :: StorageControl -> FilePath -> [(String,[String])] -> IO BankControlResult
observedControl control@(StorageControl name) directory rows = do
  let failures = failedCases rows
      blocked = [(label,filter ("status " `isPrefixOf`) problems) | (label,problems) <- rows]
  case firstCaseFailure directory blocked of
    Just refusal -> throwIO (QualificationRefusal (refusal
      {failedPredicateName=PredicateName ("control " ++ name ++ ": " ++ predicateText (failedPredicateName refusal))}))
    Nothing -> pure ()
  pure (BankControlResult control (not (null failures)) (take 10 failures) Nothing)

data JoinedQualification = JoinedQualification
  { joinedStockRows :: [(String,[String])]
  , joinedOwnedRows :: [(String,[String])]
  , joinedControlRows :: [BankControlResult]
  }

data JoinedRecordFault = OmitRecordAppend | OmitRecordCrc deriving (Eq, Ord, Show)

joinedRecordFaultName :: JoinedRecordFault -> String
joinedRecordFaultName OmitRecordAppend = "record-append-omitted"
joinedRecordFaultName OmitRecordCrc = "record-crc-omitted"

joinedRecordControlCases :: [String] -> B.ByteString -> B.ByteString -> [H.Function] -> PropertyInputs -> IO [(String,[CaseSpec])]
joinedRecordControlCases args image loader ranges inputs =
  forM [OmitRecordAppend,OmitRecordCrc] $ \fault -> do
    let name = joinedRecordFaultName fault
    (binaryPath,symbolPath) <- case [(binary,symbols) | (variant,binary,symbols) <- defectArguments args,variant == name] of
      [paths] -> pure paths
      _ -> fail ("missing or duplicate --defect " ++ name)
    binary <- readRegular 0x200000 binaryPath
    symbols <- readSymbols symbolPath
    either (fail . ((name ++ ": ") ++)) pure (admitCustomBoundaries 0x5d0000 binary symbols)
    joined <- either (fail . ((name ++ ": ") ++)) pure
      (Join.prepareJoinCases image loader ranges (propertyStagedImage inputs) (propertyArtifact inputs) binary symbols)
    let cases = filter ((== "bank-record-custom-program") . caseId) (Join.joinOwnedCases joined)
    ensure (not (null cases)) (name ++ ": no joined record control witness")
    pure (name,cases)

runJoinedQualification :: NativeCache -> [String] -> FilePath -> JoinedStockRunner -> FilePath -> B.ByteString -> B.ByteString -> [H.Function] -> B.ByteString -> M.Map String Symbol -> PropertyInputs -> IO JoinedQualification
runJoinedQualification cache args runner (JoinedStockRunner stockRunner cachedDigest) output image loader ranges binary symbols inputs = do
  joined <- either (fail . ("join: " ++)) pure
    (Join.prepareJoinCases image loader ranges (propertyStagedImage inputs) (propertyArtifact inputs) binary symbols)
  recordControls <- joinedRecordControlCases args image loader ranges inputs
  stock <- case cachedDigest of
    Nothing -> runSpecs cache stockRunner (output </> "joined-stock") (Join.joinStockCases joined)
    Just digest -> do
      frozen <- cacheOnlyRunner stockRunner digest
      runCachedSpecs cache frozen (output </> "joined-stock") (Join.joinStockCases joined)
  owned <- runSpecs cache runner (output </> "joined-owned") (Join.joinOwnedCases joined)
  controls <- forM (Join.joinControlCases joined ++ recordControls) $ \(name,cases) -> do
    ensure (not (null cases)) ("joined control has no witnesses: " ++ name)
    results <- runSpecs cache runner (output </> "joined-control-" ++ name) cases
    observedControl (StorageControl name) (output </> "joined-control-" ++ name) results
  pure (JoinedQualification stock owned controls)

joinedSubsetResult :: FilePath -> JoinedQualification -> QualificationResult
joinedSubsetResult output joined = QualificationResult passed receipt failure
  where
    stock = joinedStockRows joined
    owned = joinedOwnedRows joined
    controls = joinedControlRows joined
    passed = null (failedCases stock) && null (failedCases owned) && all bankControlHolds controls
    receipt = object
      [ "schema_version" .= (2::Int)
      , "qualification" .= scopeLabel JoinedNativeSubset
      , "stock_cases" .= length stock, "owned_cases" .= length owned
      , "stock_failures" .= failedCases stock, "owned_failures" .= failedCases owned
      , "controls" .= [object ["name" .= name,"holds" .= bankControlHolds result,"failures" .= bankControlFailures result]
          | result <- controls, let StorageControl name = bankControlName result]
      , "current_gates_completed" .= False
      , "scope" .= ("Joined bank/custom/record native subset only. The complete bank matrix, other controls, preemption, EE dispatcher, system and hardware qualification remain separate."::String)
      , "passed" .= passed
      ]
    failure = listToMaybe (catMaybes
      [firstCaseFailure (output </> "joined-stock") stock,firstCaseFailure (output </> "joined-owned") owned]
      ++ [controlFailure output name | result <- controls, not (bankControlHolds result), let StorageControl name = bankControlName result])

-- Each consumer constructs its own lazy case graph.
{-# NOINLINE withBankMatrix #-}
withBankMatrix :: Bank.NativeImage -> M.Map PropertyId Property -> Property.Fixture -> [Property.OracleCase] -> (Bank.StorageMatrix -> IO a) -> IO a
withBankMatrix source properties fixture grid consume =
  consume (Bank.storageMatrix source properties fixture grid)

runBankQualification :: NativeCache -> [String] -> FilePath -> FilePath -> JoinedStockRunner -> FilePath -> B.ByteString -> B.ByteString -> [H.Function] -> B.ByteString -> M.Map String Symbol -> PropertyInputs -> IO BankQualification
runBankQualification cache args runner stockRunner joinedStockRunner output image loader ranges binary symbols inputs = do
  let source = propertySource inputs
      artifact = propertyArtifact inputs
      propertyImage = propertyStagedImage inputs
      classified = Artifact.sourceProperties source
      properties = M.fromList [(propertyId property,property) | property <- classified]
      arms = [owned | Property _ _ _ (Owned owned) <- classified]
      fixture = Property.withConfigVariants arms (Property.fixture image)
      nativeImage = Bank.prepareNativeImage image
      fixtures = Bank.matrixFixtures (Bank.storageMatrix nativeImage properties fixture Property.caseGrid)
      entries = [(address,"settings_bank_" ++ role ++ "_entry") | (address,role) <- bankEntries]
      prepare adopted owned ownedSymbols = Bank.prepareComposedImage adopted (Bank.propertyMetadata artifact) owned ownedSymbols entries fixtures
      prepared = prepare propertyImage binary symbols
  stock <- withBankMatrix nativeImage properties fixture Property.caseGrid $ \matrix ->
    runSpecs cache stockRunner (output </> "bank-stock") (Bank.matrixCases matrix)
  owned <- withBankMatrix nativeImage properties fixture Property.caseGrid $ \matrix ->
    runSpecs cache runner (output </> "bank-owned") (map (Bank.ownedCase prepared) (Bank.matrixCases matrix))
  joined <- runJoinedQualification cache args runner joinedStockRunner output image loader ranges binary symbols inputs
  let joinedStock = joinedStockRows joined
      joinedOwned = joinedOwnedRows joined
      joinedControls = joinedControlRows joined
  refusalLinked <- required args "--property-refusal-linked" >>= readRegular (Placement.carrierBytes Placement.stage4Carrier)
  refusalSymbols <- required args "--property-refusal-symbols" >>= readRegular 0x10000
  refusalElf <- required args "--property-refusal-elf" >>= readRegular 0x400000
  carrier <- maybe (fail "canonical property hull has no carrier") pure (Placement.hullCarrier (Artifact.sourceHull source))
  refusalArtifact <- either (fail . show) pure (Artifact.deriveArtifact Artifact.ProductionArtifact classified carrier (Artifact.ArtifactInput refusalLinked refusalSymbols refusalElf))
  refusalImage <- either (fail . show) pure (Artifact.stagedImage4Bytes source refusalArtifact)
  refusalOwned <- maybe (fail "refusal property is not owned") pure (Property.modelled (Property.standingOf properties (PropertyId 1)))
  let refusalSpec = Bank.refusalCase (Bank.prepareNativeImage image) properties fixture refusalOwned
      preparedRefusal = Bank.prepareComposedImage refusalImage (Bank.propertyMetadata refusalArtifact) binary symbols entries fixtures
  stockRefusal <- runSpecs cache stockRunner (output </> "bank-stock-refusal") [refusalSpec]
  ownedRefusal <- runSpecs cache runner (output </> "bank-owned-refusal") [Bank.ownedCase preparedRefusal refusalSpec]
  variants <- either (fail . ("variants: " ++)) pure (variantArguments args)
  variantControls <- forM variants $ \variant -> withBankMatrix nativeImage properties fixture Property.caseGrid $ \matrix -> do
    let (control@(StorageControl name),path,symbolPath) = case variant of
          CombinedBankArtifact admittedControl binPath symbolsPath -> (admittedControl,binPath,symbolsPath)
          AdoptedPropertyArtifact admittedControl binPath symbolsPath _ -> (admittedControl,binPath,symbolsPath)
        selected = Bank.controlWitnesses name (Bank.matrixFields matrix ++ Bank.matrixHandoverReads matrix) (Bank.matrixMirrors matrix) (Bank.matrixSequences matrix)
    ensure (not (null selected)) ("storage control has no matrix witnesses: " ++ name)
    defect <- readRegular 0x200000 path
    (cases,appendArtifact) <- case variant of
      CombinedBankArtifact _ _ _ -> do
        defectSymbols <- readSymbols symbolPath
        ensure (defect /= binary) ("bank control is identical to candidate: " ++ name)
        either (fail . ((name ++ ": ") ++)) pure (admitCustomBoundaries 0x5d0000 defect defectSymbols)
        cases <- if control == StorageControl "application-appends-bank-as-area3"
          then Join.joinOwnedCases <$> either (fail . ((name ++ ": ") ++)) pure (Join.prepareJoinCases image loader ranges propertyImage artifact defect defectSymbols)
          else pure (map (Bank.ownedCase (prepare propertyImage defect defectSymbols)) selected)
        pure (cases,Nothing)
      AdoptedPropertyArtifact _ _ _ elfPath -> do
        rawSymbols <- readRegular 0x10000 symbolPath
        elf <- readRegular 0x400000 elfPath
        let input = Artifact.ArtifactInput defect rawSymbols elf
        if control == StorageControl "close-appends-area3"
          then do
            controlArtifact <- either (fail . ((name ++ ": ") ++)) pure (Control.deriveCloseAppendControl source input)
            controlImage <- either (fail . ((name ++ ": ") ++)) pure (Control.controlImage4 source controlArtifact)
            joinedControl <- either (fail . ((name ++ ": ") ++)) pure (Join.prepareControlJoinCases image loader ranges controlImage (Control.controlMetadata controlArtifact) binary symbols)
            pure (Join.joinOwnedCases joinedControl,Just controlArtifact)
          else do
            defectArtifact <- either (fail . ((name ++ ": ") ++) . show) pure (Artifact.deriveArtifact Artifact.ProductionArtifact classified carrier input)
            defectImage <- either (fail . ((name ++ ": ") ++) . show) pure (Artifact.stagedImage4Bytes source defectArtifact)
            ensure (defectImage /= propertyImage) ("property control is identical to candidate: " ++ name)
            let preparedDefect = Bank.prepareComposedImage defectImage (Bank.propertyMetadata defectArtifact) binary symbols entries fixtures
            pure (map (Bank.ownedCase preparedDefect) selected,Nothing)
    results <- runSpecs cache runner (output </> "bank-control-" ++ name) cases
    result <- observedControl control (output </> "bank-control-" ++ name) results
    pure result { bankControlAppendArtifact = appendArtifact }
  stockControls <- forM Bank.stockControls $ \control@(Bank.StockControl name _) -> withBankMatrix nativeImage properties fixture Property.caseGrid $ \matrix -> do
    let witnessesForControl = Bank.matrixMirrors matrix ++ Bank.matrixRequests matrix
    stockCases <- either (fail . ((name ++ ": ") ++)) pure (mapM (Bank.mutateStock control) witnessesForControl)
    ownedCases <- either (fail . ((name ++ ": ") ++)) pure (mapM (Bank.mutateStock control . Bank.ownedCase prepared) witnessesForControl)
    stockResults <- runSpecs cache runner (output </> "bank-stock-control-" ++ name) stockCases
    ownedResults <- runSpecs cache runner (output </> "bank-owned-control-" ++ name) ownedCases
    stockResult <- observedControl (StorageControl name) (output </> "bank-stock-control-" ++ name) stockResults
    ownedResult <- observedControl (StorageControl name) (output </> "bank-owned-control-" ++ name) ownedResults
    pure (BankControlResult (StorageControl name) (bankControlHolds stockResult && bankControlHolds ownedResult)
      ([(("stock/" ++ label),failures) | (label,failures) <- bankControlFailures stockResult]
      ++ [(("owned/" ++ label),failures) | (label,failures) <- bankControlFailures ownedResult]) Nothing)
  residueProof <- readResidueProof args
  let controls = variantControls ++ stockControls ++ [BankControlResult (StorageControl "full-export-tail-zero-invented") True [] Nothing | Just _ <- [residueProof]]
      names = [name | BankControlResult (StorageControl name) _ _ _ <- controls]
      missing = filter (`notElem` names) Bank.requiredControls
      allControls = controls ++ joinedControls
      passing = all (null . failedCases) [stock,owned,joinedStock,joinedOwned,stockRefusal,ownedRefusal] && null missing && all bankControlHolds allControls
      rows result = object ["name" .= name,"holds" .= bankControlHolds result,"failures" .= bankControlFailures result,"artifact_evidence" .= fmap Control.controlEvidence (bankControlAppendArtifact result)]
        where StorageControl name = bankControlName result
      receipt = object
        [ "property_image4_sha256" .= sha256Hex (sha256 propertyImage)
        , "property_linked_sha256" .= sha256Hex (sha256 (propertyLinkedBytes inputs))
        , "property_symbols_sha256" .= sha256Hex (sha256 (propertySymbolBytes inputs))
        , "property_elf_sha256" .= sha256Hex (sha256 (propertyElfBytes inputs))
        , "property_contract_sha256" .= sha256Hex (sha256 (propertyContractBytes inputs))
        , "stock_cases" .= length stock,"owned_cases" .= length owned
        , "joined_stock_cases" .= length joinedStock,"joined_owned_cases" .= length joinedOwned
        , "stock_failures" .= take 30 (failedCases stock),"owned_failures" .= take 30 (failedCases owned)
        , "joined_stock_failures" .= take 30 (failedCases joinedStock),"joined_owned_failures" .= take 30 (failedCases joinedOwned)
        , "stock_refusal_failures" .= failedCases stockRefusal,"owned_refusal_failures" .= failedCases ownedRefusal
        , "refusal_linked_sha256" .= sha256Hex (sha256 refusalLinked)
        , "refusal_symbols_sha256" .= sha256Hex (sha256 refusalSymbols)
        , "refusal_elf_sha256" .= sha256Hex (sha256 refusalElf)
        , "controls" .= map rows allControls,"missing_controls" .= missing
        , "full_export_control_receipt_sha256" .= residueProof,"passed" .= passing
        ]
  ensure (length names == length (nub names)) "duplicate storage controls"
  let failure=listToMaybe (catMaybes
        [firstCaseFailure (output </> directory) results
        | (directory,results) <- [("bank-stock",stock),("bank-owned",owned),("joined-stock",joinedStock)
          ,("joined-owned",joinedOwned),("bank-stock-refusal",stockRefusal),("bank-owned-refusal",ownedRefusal)]]
        ++ [controlFailure output name | name <- missing]
        ++ [controlFailure output name | BankControlResult (StorageControl name) False _ _ <- allControls])
  pure (BankQualification passing receipt failure)

data PreemptionQualification = PreemptionQualification
  { preemptionPassed :: Bool
  , preemptionReceipt :: Value
  , preemptionFailure :: Maybe FailedPredicate
  }

runPreemptionQualification :: NativeCache -> [String] -> FilePath -> FilePath -> B.ByteString -> B.ByteString -> [H.Function] -> B.ByteString -> M.Map String Symbol -> IO PreemptionQualification
runPreemptionQualification cache args runner output image loader ranges binary symbols = do
  prepared <- either (fail . ("preemption: " ++)) pure (Preemption.preparePreemptionCases image loader ranges binary symbols)
  let stockCases = Preemption.preemptionStockCases prepared
      ownedCases = Preemption.preemptionOwnedCases prepared
  ensure (not (null stockCases) && length stockCases == length ownedCases) "preemption fixture roles differ or are empty"
  stock <- runSpecs cache runner (output </> "preemption-stock") stockCases
  owned <- runSpecs cache runner (output </> "preemption-owned") ownedCases
  (path,symbolPath) <- case [(path,symbolPath) | (name,path,symbolPath) <- defectArguments args,name == "unlock-before-copy"] of
    [paths] -> pure paths
    _ -> fail "missing or duplicate --defect unlock-before-copy"
  mutant <- readRegular 0x200000 path
  mutantSymbols <- readSymbols symbolPath
  ensure (mutant /= binary) "unlock-before-copy control is identical to candidate"
  either (fail . ("preemption: " ++)) pure (admitCustomBoundaries 0x5d0000 mutant mutantSymbols)
  controlCases <- either (fail . ("preemption control: " ++)) pure (Preemption.preparePreemptionControl image loader ranges mutant mutantSymbols)
  ensure (not (null controlCases)) "unlock-before-copy control has no witnesses"
  controlRows <- runSpecs cache runner (output </> "preemption-control-unlock-before-copy") controlCases
  control <- observedControl (StorageControl "unlock-before-copy") (output </> "preemption-control-unlock-before-copy") controlRows
  let passed = null (failedCases stock) && null (failedCases owned) && bankControlHolds control
      receipt = object
        [ "stock_cases" .= length stock,"owned_cases" .= length owned
        , "stock_failures" .= failedCases stock,"owned_failures" .= failedCases owned
        , "control" .= object ["name" .= ("unlock-before-copy"::String),"holds" .= bankControlHolds control,"failures" .= bankControlFailures control,"linked_sha256" .= sha256Hex (sha256 mutant)]
        , "passed" .= passed
        ]
  let failure=listToMaybe (catMaybes [firstCaseFailure (output </> "preemption-stock") stock,firstCaseFailure (output </> "preemption-owned") owned]
        ++ [controlFailure output "unlock-before-copy" | not (bankControlHolds control)])
  pure (PreemptionQualification passed receipt failure)

data EeQualification = EeQualification
  { eeQualificationPassed :: Bool
  , eeQualificationReceipt :: Value
  , eeQualificationFailure :: Maybe FailedPredicate
  }

runEeQualification :: NativeCache -> FilePath -> FilePath -> B.ByteString -> B.ByteString -> [H.Function] -> B.ByteString -> M.Map String Symbol -> PropertyInputs -> IO EeQualification
runEeQualification cache runner output image loader ranges binary symbols inputs = do
  let image5=propertyImage5Bytes inputs
  prepared <- either (fail . ("ee: " ++)) pure (Ee.prepareEeCases image image5 loader ranges
    (propertyStagedImage inputs) (propertyArtifact inputs) binary symbols)
  let stockSpecs=Ee.eeStockCases prepared
      ownedSpecs=Ee.eeOwnedCases prepared
      controlSpecs=Ee.eeControlCases prepared
      requiredControls=[minBound..maxBound]
      actualControls=map fst controlSpecs
      validateSpecs specs = do
        ensure (not (null specs)) "EE native case family is empty"
        ensure (all ((==Returned) . caseExpectedStatus) specs) "EE native case expects a blocking status"
        ensure (length specs == length (nub (map caseId specs))) "EE native case IDs repeat within a family"
  ensure (length stockSpecs == length ownedSpecs) "EE native role case counts differ"
  ensure (length actualControls == length (nub actualControls)
    && all (`elem` actualControls) requiredControls) "EE native controls are missing or duplicated"
  validateSpecs stockSpecs
  validateSpecs ownedSpecs
  stock <- runSpecs cache runner (output </> "ee-stock") stockSpecs
  owned <- runSpecs cache runner (output </> "ee-owned") ownedSpecs
  controls <- forM controlSpecs $ \(control,specs) -> do
    validateSpecs specs
    let name=Ee.eeControlName control
    observed <- runSpecs cache runner (output </> "ee-control-" ++ name) specs
    result <- observedControl (StorageControl name) (output </> "ee-control-" ++ name) observed
    pure (bankControlHolds result,length observed,object
      ["name" .= name,"cases" .= length observed,"holds" .= bankControlHolds result
      ,"failures" .= bankControlFailures result])
  let passed=null (failedCases stock) && null (failedCases owned) && all (\(holds,_,_) -> holds) controls
      receipt=object
        [ "image5_sha256" .= sha256Hex (sha256 image5)
        , "stock_cases" .= length stock,"owned_cases" .= length owned
        , "stock_failures" .= failedCases stock,"owned_failures" .= failedCases owned
        , "required_controls" .= map Ee.eeControlName requiredControls
        , "controls" .= [value | (_,_,value) <- controls]
        , "observed_case_executions" .= (length stock+length owned+sum [count | (_,count,_) <- controls])
        , "scope" .= ("HOST observations execute the canonical d4fc74 dispatcher, real row conversion, cache publication, node/getter path and final property/bank application. The retained 634650 RAM publisher executes. Calls to 644d84 and 644e84 are retained white-balance service seams with declared settings-write fixtures, not pure device leaves or qualified helper internals. The d4d604 notification is a declared void service boundary. Ordered helper calls, arguments, cache values and final bank effects are qualified only under those declarations. No full helper, device-law, natural EE-route or hardware qualification."::String)
        , "passed" .= passed
        ]
  let failure=listToMaybe (catMaybes [firstCaseFailure (output </> "ee-stock") stock,firstCaseFailure (output </> "ee-owned") owned]
        ++ [controlFailure output (Ee.eeControlName control) | (control,(False,_,_)) <- zip actualControls controls])
  pure (EeQualification passed receipt failure)

data FocusedGate = PreemptionFocus | EeFocus | CustomLifecycleFocus
  deriving (Eq, Ord, Show, Enum, Bounded)

data QualificationScope = FullNative | JoinedNativeSubset | FocusedNative FocusedGate
  deriving (Eq, Show)

scopeLabel :: QualificationScope -> String
scopeLabel FullNative = "typed-settings-storage-composed-native-pair"
scopeLabel JoinedNativeSubset = "typed-settings-storage-joined-native-subset"
scopeLabel (FocusedNative gate) = "typed-settings-storage-focused-" ++ focusedGateName gate

focusedGateName :: FocusedGate -> String
focusedGateName PreemptionFocus = "preemption"
focusedGateName EeFocus = "ee"
focusedGateName CustomLifecycleFocus = "custom-lifecycle"

focusedScopeText :: FocusedGate -> String
focusedScopeText PreemptionFocus =
  "Focused cross-kind preemption gate only: the record-store entries execute at the unlock and callback boundaries with the unlock-before-copy control. The bank matrix, EE dispatcher, custom lifecycle, system and hardware qualification remain separate."
focusedScopeText EeFocus =
  "Focused EE dispatcher gate only: equal and different-old-value device-order cases execute through the canonical d4fc74 dispatcher with the declared service fixtures and the seven dispatcher controls. The bank matrix, cross-kind preemption, custom lifecycle, system and hardware qualification remain separate."
focusedScopeText CustomLifecycleFocus =
  "Focused custom-settings lifecycle gate only: record codec, prepare/check/default/recovery, commit and boot controls execute against the current compiled custom artifact. The bank matrix, cross-kind preemption, EE dispatcher, system and hardware qualification remain separate."

focusValues :: [String] -> Either String [String]
focusValues ("--focus":[]) = Left "--focus needs a value"
focusValues ("--focus":value:rest) = (value :) <$> focusValues rest
focusValues (_:rest) = focusValues rest
focusValues [] = Right []

parseFocusedGate :: String -> Either String FocusedGate
parseFocusedGate "preemption" = Right PreemptionFocus
parseFocusedGate "ee" = Right EeFocus
parseFocusedGate "custom-lifecycle" = Right CustomLifecycleFocus
parseFocusedGate other = Left ("--focus takes preemption, ee or custom-lifecycle, not " ++ show other)

selectScope :: [String] -> Either String QualificationScope
selectScope arguments = do
  values <- focusValues arguments
  gate <- case values of
    [] -> Right Nothing
    [value] -> Just <$> parseFocusedGate value
    _ -> Left "--focus accepts exactly one gate"
  let joined = "--joined-only" `elem` arguments
  case (gate,joined) of
    (Nothing,False) -> Right FullNative
    (Nothing,True) -> Right JoinedNativeSubset
    (Just selected,False) -> Right (FocusedNative selected)
    (Just _,_) -> Left "--joined-only and --focus select different execution scopes"

runQualification :: QualificationScope -> NativeCache -> [String] -> FilePath -> IO QualificationResult
runQualification scope cache args output = do
  runner <- runnerArgument args
  image <- required args "--image4" >>= readRegular 0x300000
  loader <- required args "--loader-a" >>= readRegular 0x20000
  ensure (sha256Hex (sha256 image) == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") "custom oracle image4 identity differs"
  ensure (sha256Hex (sha256 loader) == "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882") "custom oracle loader identity differs"
  ranges <- required args "--index" >>= readFunctions
  binary <- required args "--linked" >>= readRegular 0x200000
  symbols <- required args "--symbols" >>= readSymbols
  baseText <- required args "--base"
  base <- maybe (fail "invalid custom artifact base") pure (parseWord32 baseText)
  ensure (base == 0x5d0000) "custom artifact is outside its declared host carrier"
  either fail pure (admitCustomBoundaries base binary symbols)
  propertyInputs <- case scope of
    FocusedNative PreemptionFocus -> pure Nothing
    FocusedNative CustomLifecycleFocus -> pure Nothing
    _ -> Just <$> readPropertyInputs args image loader
  case scope of
    JoinedNativeSubset -> do
      stockRunner <- joinedStockRunnerArgument args runner
      inputs <- maybe (fail "joined qualification needs property inputs") pure propertyInputs
      joinedSubsetResult output <$> runJoinedQualification cache args runner stockRunner output image loader ranges binary symbols inputs
    FullNative -> do
      stockRunner <- stockRunnerArgument args runner
      joinedStockRunner <- joinedStockRunnerArgument args stockRunner
      inputs <- maybe (fail "complete qualification needs property inputs") pure propertyInputs
      runFullQualification cache args runner stockRunner joinedStockRunner output image loader ranges binary symbols base inputs
    FocusedNative gate -> runFocusedQualification gate cache args runner output image loader ranges binary symbols base propertyInputs

runFocusedQualification :: FocusedGate -> NativeCache -> [String] -> FilePath -> FilePath -> B.ByteString -> B.ByteString -> [H.Function] -> B.ByteString -> M.Map String Symbol -> Word32 -> Maybe PropertyInputs -> IO QualificationResult
runFocusedQualification gate cache args runner output image loader ranges binary symbols base propertyInputs = case gate of
  PreemptionFocus -> do
    result <- runPreemptionQualification cache args runner output image loader ranges binary symbols
    pure (focusedResult gate (preemptionPassed result) (preemptionReceipt result) (preemptionFailure result))
  EeFocus -> do
    inputs <- maybe (fail "EE dispatcher focus needs property inputs") pure propertyInputs
    result <- runEeQualification cache runner output image loader ranges binary symbols inputs
    pure (focusedResult gate (eeQualificationPassed result) (eeQualificationReceipt result) (eeQualificationFailure result))
  CustomLifecycleFocus -> do
    result <- runCustomQualification cache args runner output image loader ranges binary symbols base
    pure (focusedResult gate (customPassed result) (object
      [ "cases" .= [object ["name" .= name,"differences" .= differences] | (name,differences) <- customCaseRows result]
      , "controls" .= [object ["name" .= name,"holds" .= holds] | (name,holds) <- customControlRows result]
      ]) (customFailure result))

focusedResult :: FocusedGate -> Bool -> Value -> Maybe FailedPredicate -> QualificationResult
focusedResult gate passed receipt failure = QualificationResult passed (object
  [ "schema_version" .= (2::Int)
  , "qualification" .= scopeLabel (FocusedNative gate)
  , "gate" .= focusedGateName gate
  , "current_gates_completed" .= False
  , "gates" .= focusedGateValue gate passed
  , "scope" .= focusedScopeText gate
  , "passed" .= passed
  , focusedGateField gate receipt
  ]) failure

focusedGateValue :: FocusedGate -> Bool -> Value
focusedGateValue PreemptionFocus passed = object ["cross_kind_preemption" .= passed]
focusedGateValue EeFocus passed = object ["ee_dispatcher" .= passed]
focusedGateValue CustomLifecycleFocus passed = object ["custom_lifecycle" .= passed]

focusedGateField :: FocusedGate -> Value -> Pair
focusedGateField PreemptionFocus receipt = "cross_kind_preemption" .= receipt
focusedGateField EeFocus receipt = "ee_dispatcher" .= receipt
focusedGateField CustomLifecycleFocus receipt = "custom_lifecycle" .= receipt

runFullQualification :: NativeCache -> [String] -> FilePath -> FilePath -> JoinedStockRunner -> FilePath -> B.ByteString -> B.ByteString -> [H.Function] -> B.ByteString -> M.Map String Symbol -> Word32 -> PropertyInputs -> IO QualificationResult
runFullQualification cache args runner stockRunner joinedStockRunner output image loader ranges binary symbols base propertyInputs = do
  bank <- runBankQualification cache args runner stockRunner joinedStockRunner output image loader ranges binary symbols propertyInputs
  preemption <- runPreemptionQualification cache args runner output image loader ranges binary symbols
  ee <- runEeQualification cache runner output image loader ranges binary symbols propertyInputs
  custom <- runCustomQualification cache args runner output image loader ranges binary symbols base
  let passed = customPassed custom && bankQualificationPassed bank && preemptionPassed preemption && eeQualificationPassed ee
      receipt = object
        [ "schema_version" .= (2::Int),"qualification" .= ("typed-settings-storage-composed-native-pair"::String)
        , "image4_sha256" .= sha256Hex (sha256 image),"loader_sha256" .= sha256Hex (sha256 loader),"linked_sha256" .= sha256Hex (sha256 binary)
        , "owned_cache" .= RecordCarrier.cacheContract
        , "cases" .= [object ["name" .= name,"differences" .= differences] | (name,differences) <- customCaseRows custom]
        , "controls" .= [object ["name" .= name,"holds" .= holds] | (name,holds) <- customControlRows custom]
        , "bank_and_record" .= bankQualificationReceipt bank
        , "cross_kind_preemption" .= preemptionReceipt preemption
        , "ee_dispatcher" .= eeQualificationReceipt ee
        , "current_gates_completed" .= True
        , "gates" .= object ["current_input_admission" .= True,"bank_and_record" .= bankQualificationPassed bank
          ,"cross_kind_preemption" .= preemptionPassed preemption,"ee_dispatcher" .= eeQualificationPassed ee
          ,"custom_lifecycle" .= customLifecyclePassed custom,"custom_controls" .= all snd (customControlRows custom)]
        , "scope" .= ("The combined bank, custom-settings and record-store artifact executes with the adopted property image re-derived from canonical sources, BIN, symbols and ELF. The complete bank matrix and its 37 required controls are repeated after adoption. Joined cases execute actual custom refresh, record append and 600-byte persistence. Cross-kind preemption executes actual record-store entries at unlock and callback boundaries. Retained configuration/reset, scheduler and NAND model laws remain declared. The close-appends-area3 artifact is an isolated behavioral mutant, not production admission or stack evidence. No natural-route, device-law or hardware qualification."::String)
        , "passed" .= passed
        ]
  let failure=listToMaybe (catMaybes
        [bankQualificationFailure bank,preemptionFailure preemption,eeQualificationFailure ee,customFailure custom])
  pure (QualificationResult passed receipt failure)

data CustomQualification = CustomQualification
  { customPassed :: Bool
  , customLifecyclePassed :: Bool
  , customControlRows :: [(String,Bool)]
  , customCaseRows :: [(String,[String])]
  , customFailure :: Maybe FailedPredicate
  }

runCustomQualification :: NativeCache -> [String] -> FilePath -> FilePath -> B.ByteString -> B.ByteString -> [H.Function] -> B.ByteString -> M.Map String Symbol -> Word32 -> IO CustomQualification
runCustomQualification cache args runner output image loader ranges binary symbols base = do
  let stockSpecs = map (stockCase image loader ranges) customCases
  candidateSpecs <- either (fail . ("custom: " ++)) pure (mapM (candidateCase base binary symbols) stockSpecs)
  stockRun <- runRole cache runner (output </> "stock") stockSpecs
  candidateRun <- runRole cache runner (output </> "candidate") candidateSpecs
  let stock = map roleObservation stockRun
      candidate = map roleObservation candidateRun
      pairRows = [(c,original,owned,comparePair original owned ++ customInvariants c original ++ customInvariants c owned) | (c,original,owned) <- zip3 customCases stock candidate]
      paired = [differences | (_,_,_,differences) <- pairRows]
      controls = [("refresh-omitted","settings_custom_refresh",\c -> C.caseName (customName c) == "sequence-reload"),("correction-omitted","settings_custom_codec_check",\c -> inputRecords c == CorrectableRecord),("defaults-omitted","settings_custom_defaults",\c -> C.caseName (customName c) == "explicit-defaults-maximum")]
  forM_ (zip3 stockRun candidateRun pairRows) $ \(stockRole,candidateRole,(_,_,_,differences)) -> do
    finishRole stockRole differences
    finishRole candidateRole differences
  controlRows <- forM controls $ \(name,function,select) -> do
    mutated <- either fail pure (mutateFunction base symbols function binary)
    let chosen = [(c,spec,reference) | (c,spec,reference) <- zip3 customCases stockSpecs stock,select c]
    ensure (not (null chosen)) (name ++ ": no custom control witness")
    specs <- either (fail . ((name ++ ": ") ++)) pure (mapM (candidateCase base mutated symbols . (\(_,spec,_) -> spec)) chosen)
    observedRun <- runRole cache runner (output </> name) specs
    let observed = map roleObservation observedRun
    refused <- observeCustomControl output (output </> name) specs
      [CustomControlWitness (CaseId (caseId spec)) reference subject | ((_,spec,reference),subject) <- zip chosen observed]
    forM_ observedRun $ \role -> finishRole role (if refused then [] else ["control not observed"])
    pure (name,refused)
  defectRows <- forM ["accept-high-row-bits","drop-recovery"] $ \name -> do
    (binaryPath,symbolPath) <- case [(binaryPath,symbolPath) | (variant,binaryPath,symbolPath) <- defectArguments args,variant == name] of
      [paths] -> pure paths
      _ -> fail ("missing or duplicate --defect " ++ name)
    defectBinary <- readRegular 0x200000 binaryPath
    defectSymbols <- readSymbols symbolPath
    either (fail . ((name ++ ": ") ++)) pure (admitCustomBoundaries base defectBinary defectSymbols)
    let selected c = case (name,inputRecords c) of
          ("accept-high-row-bits",HighParityRecord _) -> True
          ("accept-high-row-bits",NewerHighParityRecord _) -> True
          ("drop-recovery",NewerRejectedRecords) -> True
          ("drop-recovery",BoundaryProbeRecords) -> customSteps c == [Boot,Read]
          _ -> False
        chosen = [(spec,reference) | (c,spec,reference) <- zip3 customCases stockSpecs stock,selected c]
    ensure (not (null chosen)) (name ++ ": no defect witnesses")
    specs <- either (fail . ((name ++ ": ") ++)) pure (mapM (candidateCase base defectBinary defectSymbols . fst) chosen)
    observedRun <- runRole cache runner (output </> name) specs
    let observed = map roleObservation observedRun
    refused <- observeCustomControl output (output </> name) specs
      [CustomControlWitness (CaseId (caseId spec)) reference subject | ((spec,reference),subject) <- zip chosen observed]
    forM_ observedRun $ \role -> finishRole role (if refused then [] else ["control not observed"])
    pure (name,refused)
  let boundaryControls = case M.lookup "settings_custom_store8" symbols of
        Nothing -> [("guarded-custom-store",False)]
        Just (Symbol address _) ->
          let offset = fromIntegral (address-base)
              changed = B.take offset binary <> word32le 0xe320f000 <> B.drop (offset+4) binary
          in [("guarded-custom-store",isLeft (admitCustomBoundaries base changed symbols))]
      probeControls = [("stock-recovery-probe-and-restoration",probePathObserved stock),("candidate-recovery-probe-and-restoration",probePathObserved candidate)]
      allControls = controlRows ++ defectRows ++ boundaryControls ++ probeControls
      passed = all null paired && all snd allControls
  pure (CustomQualification passed (all null paired) allControls
    [(C.caseName (customName c),differences) | (c,differences) <- zip customCases paired]
    (listToMaybe (catMaybes
      [firstCaseFailure (output </> "candidate") [(caseId spec,problems) | (spec,problems) <- zip candidateSpecs paired]]
      ++ [controlFailure output name | (name,False) <- allControls])))

newtype PredicateName = PredicateName String deriving Show

predicateText :: PredicateName -> String
predicateText (PredicateName text) = text

data FailedPredicate = FailedPredicate
  { failedPredicateName :: PredicateName
  , failedCase :: Maybe CaseId
  , failedArtifact :: Maybe FilePath
  , failedLog :: Maybe FilePath
  } deriving Show

newtype QualificationRefusal = QualificationRefusal FailedPredicate deriving Show
instance Exception QualificationRefusal

data QualificationResult = QualificationResult
  { qualificationPassed :: Bool
  , qualificationReceipt :: Value
  , qualificationFailure :: Maybe FailedPredicate
  }

firstCaseFailure :: FilePath -> [(String,[String])] -> Maybe FailedPredicate
firstCaseFailure directory rows = listToMaybe
  [ FailedPredicate (PredicateName problem) (Just (CaseId label))
      (Just (directory </> show index </> label ++ ".case.json"))
      (Just (directory </> show index </> label ++ ".native-result.json"))
  | (index,(label,problem:_)) <- zip [0::Int ..] rows
  ]

controlFailure :: FilePath -> String -> FailedPredicate
controlFailure output name = FailedPredicate
  (PredicateName ("control not observed: " ++ name)) Nothing Nothing (Just (output </> "results.json"))

observeCustomControl :: FilePath -> FilePath -> [CaseSpec] -> [CustomControlWitness] -> IO Bool
observeCustomControl output directory specs witnesses = case customControlKilled witnesses of
  Right killed -> pure killed
  Left NoCustomControlWitnesses -> fail "custom control has no witnesses"
  Left (CustomControlDidNotReturn identifier@(CaseId label) role status) ->
    let (root,labels)=case role of
          ReferenceControlRole -> (output </> "stock",[C.caseName (customName c) | c <- customCases])
          SubjectControlRole -> (directory,map caseId specs)
        rows=[(name,["control status " ++ runnerStatusLabel status | name == label]) | name <- labels]
        refusal=case firstCaseFailure root rows of
          Just failure -> failure
          Nothing -> FailedPredicate (PredicateName ("control status " ++ runnerStatusLabel status)) (Just identifier) Nothing Nothing
    in throwIO (QualificationRefusal refusal)

failureValue :: FailedPredicate -> Value
failureValue failure = object
  [ "predicate" .= take 4096 (predicateText (failedPredicateName failure))
  , "case_id" .= fmap (\(CaseId name) -> name) (failedCase failure)
  , "artifact" .= failedArtifact failure,"log" .= failedLog failure
  ]

newtype InputLabel = InputLabel String
data FrozenInput = FrozenInput InputLabel FilePath FilePath Sha256 | FrozenCachedRunner FilePath Sha256

frozenInputValue :: FrozenInput -> Value
frozenInputValue (FrozenInput (InputLabel label) configured canonical digest) = object
  ["role" .= label,"configured_path" .= configured,"path" .= canonical,"sha256" .= sha256Hex digest]
frozenInputValue (FrozenCachedRunner path digest) = object
  ["role" .= ("--joined-stock-runner"::String),"configured_path" .= path,"path" .= path
  ,"sha256" .= sha256Hex digest,"availability" .= ("cache-only"::String)]

scopeInputFlags :: QualificationScope -> [String]
scopeInputFlags FullNative = fullInputFlags
scopeInputFlags JoinedNativeSubset = fullInputFlags
scopeInputFlags (FocusedNative PreemptionFocus) = ["--image4","--loader-a","--index","--linked","--symbols"]
scopeInputFlags (FocusedNative EeFocus) =
  ["--image4","--loader-a","--index","--linked","--symbols","--dat","--loader-b","--image5"
  ,"--plain","--image2","--property-linked","--property-symbols","--property-elf","--property-image4"
  ,"--property-contract"]
scopeInputFlags (FocusedNative CustomLifecycleFocus) = ["--image4","--loader-a","--index","--linked","--symbols"]

fullInputFlags :: [String]
fullInputFlags =
  ["--image4","--loader-a","--index","--linked","--symbols","--dat","--loader-b","--image5"
  ,"--plain","--image2","--property-linked","--property-symbols","--property-elf","--property-image4"
  ,"--property-contract","--property-refusal-linked","--property-refusal-symbols","--property-refusal-elf"]

scopeDefectNames :: QualificationScope -> [String]
scopeDefectNames FullNative = fullDefectNames
scopeDefectNames JoinedNativeSubset = fullDefectNames
scopeDefectNames (FocusedNative PreemptionFocus) = ["unlock-before-copy"]
scopeDefectNames (FocusedNative EeFocus) = []
scopeDefectNames (FocusedNative CustomLifecycleFocus) = ["accept-high-row-bits","drop-recovery"]

fullDefectNames :: [String]
fullDefectNames = ["accept-high-row-bits","drop-recovery","unlock-before-copy"]
  ++ map joinedRecordFaultName [OmitRecordAppend,OmitRecordCrc]

scopeUsesVariants :: QualificationScope -> Bool
scopeUsesVariants FullNative = True
scopeUsesVariants JoinedNativeSubset = True
scopeUsesVariants (FocusedNative _) = False

freezeInputs :: QualificationScope -> [String] -> IO [FrozenInput]
freezeInputs scope args = do
  runner <- runnerArgument args
  stockRunner <- stockRunnerArgument args runner
  JoinedStockRunner joinedRunner joinedDigest <- joinedStockRunnerArgument args (case scope of
    FullNative -> stockRunner
    _ -> runner)
  paths <- forM (scopeInputFlags scope) (\flag -> (InputLabel flag,) <$> required args flag)
  variantPaths <- if scopeUsesVariants scope
    then do
      variants <- either (fail . ("variants: " ++)) pure (variantArguments args)
      pure (concatMap (\variant -> case variant of
        CombinedBankArtifact (StorageControl name) binary symbols ->
          [(InputLabel ("bank-control/" ++ name ++ "/bin"),binary),(InputLabel ("bank-control/" ++ name ++ "/symbols"),symbols)]
        AdoptedPropertyArtifact (StorageControl name) binary symbols elf ->
          [(InputLabel ("property-control/" ++ name ++ "/bin"),binary),(InputLabel ("property-control/" ++ name ++ "/symbols"),symbols),(InputLabel ("property-control/" ++ name ++ "/elf"),elf)]) variants)
    else pure []
  let defects=concat
        [[(InputLabel ("custom-control/" ++ name ++ "/bin"),binary),(InputLabel ("custom-control/" ++ name ++ "/symbols"),symbols)]
        | (name,binary,symbols) <- defectArguments args,name `elem` scopeDefectNames scope]
      proof=case dropWhile (/= "--full-export-control-result") args of
        _:path:_ -> [(InputLabel "--full-export-control-result",path)]
        _ -> []
      fullExport=concat
        [[(InputLabel flag,path)] | flag <- ["--full-export-control-producer","--full-export-control-correct","--full-export-control-mutant"], Just path <- [optionalArgument flag args]]
      stock=[(InputLabel "--stock-runner",stockRunner) | "--stock-runner" `elem` args]
      joinedStock=[(InputLabel "--joined-stock-runner",joinedRunner) | "--joined-stock-runner" `elem` args,joinedDigest == Nothing]
  oracle <- getExecutablePath
  frozen <- forM ((InputLabel "qualification-oracle",oracle):(InputLabel "--runner",runner):paths ++ variantPaths ++ defects ++ proof ++ fullExport ++ stock ++ joinedStock) $ \(label,path) -> do
    configured <- makeAbsolute path
    canonical <- canonicalizePath configured
    FrozenInput label configured canonical <$> sha256File canonical
  case joinedDigest of
    Nothing -> pure frozen
    Just digest -> do
      _ <- cacheOnlyRunner joinedRunner digest
      pure (frozen ++ [FrozenCachedRunner joinedRunner digest])

verifyFrozenInputs :: [FrozenInput] -> IO ()
verifyFrozenInputs frozen = forM_ frozen verify
 where
  verify (FrozenCachedRunner path digest) = do
    _ <- cacheOnlyRunner path digest
    pure ()
  verify (FrozenInput (InputLabel label) configured canonical digest) = do
    currentPath <- canonicalizePath configured
    current <- sha256File currentPath
    unless (currentPath == canonical && current == digest)
      (throwIO (QualificationRefusal (FailedPredicate
        (PredicateName ("input changed during qualification: " ++ label)) Nothing (Just configured) Nothing)))

resolveScratchPath :: FilePath -> IO FilePath
resolveScratchPath path = do
  workspace <- lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= maybe getCurrentDirectory pure
  root <- canonicalizePath (workspace </> "private-data/scratch")
  resolved <- makeAbsolute path >>= canonicalizePath
  ensure ((root ++ "/") `isPrefixOf` resolved) "qualification --out and --cache must be under private-data/scratch"
  pure resolved

prepareAttempt :: [String] -> IO (FilePath,NativeCache)
prepareAttempt args = do
  output <- required args "--out" >>= resolveScratchPath
  cacheRoot <- required args "--cache" >>= resolveScratchPath
  checkStorageBudget output
  exists <- doesPathExist output
  ensure (not exists) "custom oracle output already exists"
  createDirectoryIfMissing True output
  jobs <- readNativeJobs =<< case dropWhile (/= "--jobs") args of
    [] -> pure Nothing
    [_] -> fail "--jobs needs a value"
    _ : value : _ -> pure (Just value)
  cache <- openNativeCache jobs cacheRoot
  pure (output,cache)

runFullExportControl :: FilePath -> [String] -> IO [String]
runFullExportControl output args = do
  producer <- required args "--full-export-control-producer"
  correct <- required args "--full-export-control-correct"
  mutant <- required args "--full-export-control-mutant"
  let receipt = output </> "full-export-control-result.json"
  (code, _, err) <- readProcessWithExitCode producer [correct, mutant, receipt] ""
  case code of
    ExitSuccess -> pure ()
    _ -> ioError (userError ("full-export control producer failed: " ++ err))
  present <- doesFileExist receipt
  ensure present "full-export control receipt is missing"
  pure (args ++ ["--full-export-control-result", receipt])

exceptionFailure :: Maybe FilePath -> SomeException -> IO FailedPredicate
exceptionFailure output exception
  | Just asynchronous <- fromException exception :: Maybe AsyncException = throwIO asynchronous
  | Just (QualificationRefusal failure) <- fromException exception = pure failure
  | Just (NativeBatchFailure identifier path cause) <- fromException exception =
      pure (FailedPredicate (PredicateName ("native execution/admission: " ++ show cause))
        (Just identifier) (Just path) (fmap (</> "error.log") output))
  | Just fileFailure <- fromException exception :: Maybe IOException =
      pure (FailedPredicate (PredicateName (show fileFailure)) Nothing
        (ioeGetFileName fileFailure) (fmap (</> "error.log") output))
  | otherwise = pure (FailedPredicate (PredicateName (show exception)) Nothing Nothing (fmap (</> "error.log") output))

writeAttempt :: QualificationScope -> FilePath -> [String] -> [FrozenInput] -> Value -> QualificationResult -> IO Value
writeAttempt scope output args frozen execution result = do
  let failure=fmap failureValue (qualificationFailure result)
      fullExportFields=case optionalArgument "--full-export-control-result" args of
        Just path -> ["full_export_control_receipt" .= path
                     ,"full_export_control_correct_log" .= (path ++ ".correct.log")
                     ,"full_export_control_mutant_log" .= (path ++ ".mutant.log")]
        Nothing -> []
      metadata=
        ["execution" .= execution,"frozen_inputs" .= map frozenInputValue frozen
        ,"input_manifest_complete" .= not (null frozen),"first_failure" .= failure]
        ++ fullExportFields
  results <- case qualificationReceipt result of
    Object fields -> pure (Object (K.union (K.fromList metadata) fields))
    _ -> fail "qualification receipt is not an object"
  let resultsBytes=encodeCanonical results
      resultsPath=output </> "results.json"
      packetPath=output </> "review-packet.json"
      packet=object
        ["schema_version" .= (1::Int),"qualification" .= scopeLabel scope
        ,"passed" .= qualificationPassed result,"arguments" .= args,"frozen_inputs" .= map frozenInputValue frozen
        ,"input_manifest_complete" .= not (null frozen),"execution" .= execution
        ,"results" .= resultsPath,"results_sha256" .= sha256Hex (sha256 resultsBytes)
        ,"first_failure" .= failure,"current_comparison" .= qualificationReceipt result]
  let writeDocument path bytes = do
        written <- try (B.writeFile path bytes)
        case written of
          Right () -> pure ()
          Left exception -> do
            writeFailure <- exceptionFailure (Just output) exception
            throwIO (QualificationRefusal (writeFailure {failedArtifact=Just path}))
  writeDocument resultsPath resultsBytes
  writeDocument packetPath (encodeCanonical packet)
  pure (object
    ["schema_version" .= (1::Int),"qualification" .= scopeLabel scope,"passed" .= qualificationPassed result,"execution" .= execution
    ,"first_failure" .= failure,"results" .= resultsPath,"review_packet" .= packetPath])

writeErrorLog :: FilePath -> SomeException -> IO (Maybe FilePath)
writeErrorLog output exception = do
  let path=output </> "error.log"
  written <- try (B.writeFile path (encodeCanonical (object ["exception" .= show exception]) <> "\n"))
  case written of
    Right () -> pure (Just path)
    Left loggingFailure -> exceptionFailure Nothing loggingFailure >> pure Nothing

-- | Counts every generated native cohort from the same preparation path without running the
-- native runner. Used to bound a qualification's remaining calls, time and disk before a lease.
planQualification :: [String] -> IO ()
planQualification args = do
  image <- required args "--image4" >>= readRegular 0x300000
  loader <- required args "--loader-a" >>= readRegular 0x20000
  ensure (sha256Hex (sha256 image) == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") "custom oracle image4 identity differs"
  ensure (sha256Hex (sha256 loader) == "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882") "custom oracle loader identity differs"
  ranges <- required args "--index" >>= readFunctions
  binary <- required args "--linked" >>= readRegular 0x200000
  symbols <- required args "--symbols" >>= readSymbols
  baseText <- required args "--base"
  base <- maybe (fail "invalid custom artifact base") pure (parseWord32 baseText)
  ensure (base == 0x5d0000) "custom artifact is outside its declared host carrier"
  either fail pure (admitCustomBoundaries base binary symbols)
  inputs <- readPropertyInputs args image loader
  let source = propertySource inputs
      artifact = propertyArtifact inputs
      propertyImage = propertyStagedImage inputs
      classified = Artifact.sourceProperties source
      properties = M.fromList [(propertyId property,property) | property <- classified]
      arms = [owned | Property _ _ _ (Owned owned) <- classified]
      fixture = Property.withConfigVariants arms (Property.fixture image)
      matrix = Bank.storageMatrix (Bank.prepareNativeImage image) properties fixture Property.caseGrid
      fixtures = Bank.matrixFixtures matrix
      specs = Bank.matrixCases matrix
      entries = [(address,"settings_bank_" ++ role ++ "_entry") | (address,role) <- bankEntries]
      prepare adopted owned ownedSymbols = Bank.prepareComposedImage adopted (Bank.propertyMetadata artifact) owned ownedSymbols entries fixtures
      prepared = prepare propertyImage binary symbols
      witnesses name = Bank.controlWitnesses name (Bank.matrixFields matrix ++ Bank.matrixHandoverReads matrix) (Bank.matrixMirrors matrix) (Bank.matrixSequences matrix)
  joined <- either (fail . ("join: " ++)) pure (Join.prepareJoinCases image loader ranges propertyImage artifact binary symbols)
  joinedRecordControls <- joinedRecordControlCases args image loader ranges inputs
  carrier <- maybe (fail "canonical property hull has no carrier") pure (Placement.hullCarrier (Artifact.sourceHull source))
  variants <- either (fail . ("variants: " ++)) pure (variantArguments args)
  variantCohorts <- forM variants $ \variant -> do
    let (control@(StorageControl name),path,symbolPath) = case variant of
          CombinedBankArtifact admittedControl binPath symbolsPath -> (admittedControl,binPath,symbolsPath)
          AdoptedPropertyArtifact admittedControl binPath symbolsPath _ -> (admittedControl,binPath,symbolsPath)
        selected = witnesses name
    ensure (not (null selected)) ("storage control has no matrix witnesses: " ++ name)
    defect <- readRegular 0x200000 path
    cases <- case variant of
      CombinedBankArtifact _ _ _ -> do
        defectSymbols <- readSymbols symbolPath
        ensure (defect /= binary) ("bank control is identical to candidate: " ++ name)
        either (fail . ((name ++ ": ") ++)) pure (admitCustomBoundaries 0x5d0000 defect defectSymbols)
        if control == StorageControl "application-appends-bank-as-area3"
          then Join.joinOwnedCases <$> either (fail . ((name ++ ": ") ++)) pure (Join.prepareJoinCases image loader ranges propertyImage artifact defect defectSymbols)
          else pure (map (Bank.ownedCase (prepare propertyImage defect defectSymbols)) selected)
      AdoptedPropertyArtifact _ _ _ elfPath -> do
        rawSymbols <- readRegular 0x10000 symbolPath
        elf <- readRegular 0x400000 elfPath
        let input = Artifact.ArtifactInput defect rawSymbols elf
        if control == StorageControl "close-appends-area3"
          then do
            controlArtifact <- either (fail . ((name ++ ": ") ++)) pure (Control.deriveCloseAppendControl source input)
            controlImage <- either (fail . ((name ++ ": ") ++)) pure (Control.controlImage4 source controlArtifact)
            joinedControl <- either (fail . ((name ++ ": ") ++)) pure (Join.prepareControlJoinCases image loader ranges controlImage (Control.controlMetadata controlArtifact) binary symbols)
            pure (Join.joinOwnedCases joinedControl)
          else do
            defectArtifact <- either (fail . ((name ++ ": ") ++) . show) pure (Artifact.deriveArtifact Artifact.ProductionArtifact classified carrier input)
            defectImage <- either (fail . ((name ++ ": ") ++) . show) pure (Artifact.stagedImage4Bytes source defectArtifact)
            ensure (defectImage /= propertyImage) ("property control is identical to candidate: " ++ name)
            let preparedDefect = Bank.prepareComposedImage defectImage (Bank.propertyMetadata defectArtifact) binary symbols entries fixtures
            pure (map (Bank.ownedCase preparedDefect) selected)
    pure ("bank-control-" ++ name, length cases)
  stockControlCohorts <- fmap concat $ forM Bank.stockControls $ \control@(Bank.StockControl name _) -> do
    let witnessesForControl = Bank.matrixMirrors matrix ++ Bank.matrixRequests matrix
    stockCases <- either (fail . ((name ++ ": ") ++)) pure (mapM (Bank.mutateStock control) witnessesForControl)
    ownedCases <- either (fail . ((name ++ ": ") ++)) pure (mapM (Bank.mutateStock control . Bank.ownedCase prepared) witnessesForControl)
    pure [("bank-stock-control-" ++ name, length stockCases),("bank-owned-control-" ++ name, length ownedCases)]
  preparedPre <- either (fail . ("preemption: " ++)) pure (Preemption.preparePreemptionCases image loader ranges binary symbols)
  let preStock = Preemption.preemptionStockCases preparedPre
      preOwned = Preemption.preemptionOwnedCases preparedPre
  (prePath,preSymbolPath) <- case [(path,symbolPath) | (name,path,symbolPath) <- defectArguments args,name == "unlock-before-copy"] of
    [paths] -> pure paths
    _ -> fail "missing or duplicate --defect unlock-before-copy"
  preMutant <- readRegular 0x200000 prePath
  preSymbols <- readSymbols preSymbolPath
  ensure (preMutant /= binary) "unlock-before-copy control is identical to candidate"
  either (fail . ("preemption: " ++)) pure (admitCustomBoundaries 0x5d0000 preMutant preSymbols)
  preControl <- either (fail . ("preemption control: " ++)) pure (Preemption.preparePreemptionControl image loader ranges preMutant preSymbols)
  preparedEe <- either (fail . ("ee: " ++)) pure (Ee.prepareEeCases image (propertyImage5Bytes inputs) loader ranges propertyImage artifact binary symbols)
  let eeCohorts = [("ee-stock",length (Ee.eeStockCases preparedEe)),("ee-owned",length (Ee.eeOwnedCases preparedEe))]
        ++ [("ee-control-" ++ Ee.eeControlName control, length cases) | (control,cases) <- Ee.eeControlCases preparedEe]
      stockSpecs = map (stockCase image loader ranges) customCases
  candidateSpecs <- either (fail . ("custom: " ++)) pure (mapM (candidateCase base binary symbols) stockSpecs)
  customControlCohorts <- forM ["accept-high-row-bits","drop-recovery"] $ \name -> do
    (defectPath,defectSymbolPath) <- case [(binaryPath,symbolPath) | (variant,binaryPath,symbolPath) <- defectArguments args,variant == name] of
      [paths] -> pure paths
      _ -> fail ("missing or duplicate --defect " ++ name)
    defectBinary <- readRegular 0x200000 defectPath
    defectSymbols <- readSymbols defectSymbolPath
    either (fail . ((name ++ ": ") ++)) pure (admitCustomBoundaries base defectBinary defectSymbols)
    let selected c = case (name,inputRecords c) of
          ("accept-high-row-bits",HighParityRecord _) -> True
          ("accept-high-row-bits",NewerHighParityRecord _) -> True
          ("drop-recovery",NewerRejectedRecords) -> True
          ("drop-recovery",BoundaryProbeRecords) -> customSteps c == [Boot,Read]
          _ -> False
        chosen = [(c,spec) | (c,spec) <- zip customCases stockSpecs,selected c]
    ensure (not (null chosen)) (name ++ ": no custom control witness")
    controlSpecs <- either (fail . ((name ++ ": ") ++)) pure (mapM (candidateCase base defectBinary symbols . snd) chosen)
    pure ("custom-control-" ++ name,length controlSpecs)
  let cohorts =
        [("bank-stock",length specs),("bank-owned",length specs)
        ,("joined-stock",length (Join.joinStockCases joined)),("joined-owned",length (Join.joinOwnedCases joined))]
        ++ [("joined-control-" ++ name,length cases) | (name,cases) <- Join.joinControlCases joined ++ joinedRecordControls]
        ++ [("bank-stock-refusal",1),("bank-owned-refusal",1)]
        ++ variantCohorts ++ stockControlCohorts
        ++ [("preemption-stock",length preStock),("preemption-owned",length preOwned),("preemption-control-unlock-before-copy",length preControl)]
        ++ eeCohorts
        ++ [("stock",length stockSpecs),("candidate",length candidateSpecs)]
        ++ customControlCohorts
  let total = sum (map snd cohorts)
      proxySecondsPerBatch = 0.3003 :: Double
      proxyBytesPerBatch = 712000 :: Int
  B.putStr (encodeCanonical (object
    ["schema_version" .= (1::Int),"cohorts" .= [object ["name" .= name,"cases" .= count] | (name,count) <- cohorts]
    ,"total_cases" .= total
    ,"estimate" .= object
      ["basis" .= ("serial-27 partial: 12683 s / 42237 batch directories and 28G / 42237 batch directories; the directories were not all validated completed cache hits"::String)
      ,"rate_kind" .= ("proxy elapsed per batch directory, not measured completed cases"::String)
      ,"proxy_seconds_per_batch_directory" .= proxySecondsPerBatch
      ,"proxy_bytes_per_batch_directory" .= proxyBytesPerBatch
      ,"proxy_seconds" .= (fromIntegral total * proxySecondsPerBatch :: Double)
      ,"proxy_bytes" .= (total * proxyBytesPerBatch :: Int)]]) <> "\n")

main :: IO ()
main = do
  args <- getArgs
  scope <- either fail pure (selectScope args)
  case scope of
    FocusedNative _ -> ensure ("--joined-stock-runner" `notElem` args) "--joined-stock-runner requires the joined bank/custom/record stage"
    _ -> pure ()
  if "--plan" `elem` args
    then case scope of
      FullNative -> planQualification args
      JoinedNativeSubset -> fail "--joined-only is an execution subset, not a full qualification plan"
      FocusedNative gate -> fail ("--focus " ++ focusedGateName gate ++ " is an execution scope, not a full qualification plan")
    else runAttempt scope args

runAttempt :: QualificationScope -> [String] -> IO ()
runAttempt scope args = do
  prepared <- try (prepareAttempt args)
  case prepared of
    Left exception -> do
      failure <- exceptionFailure Nothing exception
      B.putStr (encodeCanonical (object
        ["schema_version" .= (1::Int),"passed" .= False,"first_failure" .= failureValue failure
        ,"results" .= Null,"review_packet" .= Null]) <> "\n")
      exitFailure
    Right (output,cache) -> do
      bindings <- newIORef []
      attempt <- try $ do
        frozen <- freezeInputs scope args
        writeIORef bindings frozen
        qualifiedArgs <- case scope of
          FocusedNative _ -> pure args
          _ -> runFullExportControl output args
        completed <- runQualification scope cache qualifiedArgs output
        verifyFrozenInputs frozen
        pure (qualifiedArgs,completed)
      frozen <- readIORef bindings
      execution <- cacheExecutionValue cache
      (attemptArgs,result) <- case attempt of
        Right completed -> pure completed
        Left exception -> do
          failure <- exceptionFailure (Just output) exception
          logPath <- writeErrorLog output exception
          let logged=case failedLog failure of
                Nothing -> failure {failedLog=logPath}
                Just path | path == output </> "error.log" -> failure {failedLog=logPath}
                _ -> failure
          pure (args,QualificationResult False (object
            ["schema_version" .= (2::Int),"qualification" .= scopeLabel scope
            ,"passed" .= False,"current_gates_completed" .= False]) (Just logged))
      published <- try (writeAttempt scope output attemptArgs frozen execution result)
      terminal <- case published of
        Right document -> pure document
        Left exception -> do
          failure <- exceptionFailure (Just output) exception
          logPath <- writeErrorLog output exception
          pure (object ["schema_version" .= (1::Int),"passed" .= False
            ,"current_gates_passed" .= qualificationPassed result,"execution" .= execution
            ,"first_failure" .= failureValue (failure {failedLog=logPath})
            ,"results" .= Null,"review_packet" .= Null])
      B.putStr (encodeCanonical terminal <> "\n")
      case published of
        Right _ | qualificationPassed result -> pure ()
        _ -> exitFailure
