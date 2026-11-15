{-# LANGUAGE OverloadedStrings #-}
module ShimCatalog where

import qualified ArmElf as E
import qualified BuildInventory as I
import Control.Monad (forM, unless, when)
import Data.Aeson
import Data.Aeson.Types (Parser)
import qualified Data.ByteString as B
import Data.Char (isAlphaNum, isHexDigit)
import Data.List (intercalate, isPrefixOf, sortOn)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Text (Text, unpack)
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.Host as H
import qualified LinkedReferences
import System.Directory (doesFileExist, listDirectory)
import System.FilePath ((</>), isAbsolute, splitDirectories)

newtype UnitId = UnitId String deriving (Eq, Ord, Show)
newtype ImageId = ImageId String deriving (Eq, Ord, Show)
newtype ShimId = ShimId String deriving (Eq, Ord, Show)
newtype SymbolName = SymbolName String deriving (Eq, Ord, Show)
newtype Digest = Digest String deriving (Eq, Ord, Show)
newtype ContractPath = ContractPath FilePath deriving (Eq, Ord, Show)
newtype GuestAddress = GuestAddress Word32 deriving (Eq, Ord, Show)
data Truth = Established | Refuted | Unestablished deriving (Eq, Show)
data ShimState = Required | ConditionMet | Removed deriving (Eq, Show)
data Ownership = NotOwned | PartlyOwned | Owned deriving (Eq, Show)
data UnitClass = LinkedUnit | ReferenceUnit | ModuleUnit deriving (Eq, Show)
data ImageClass = Production | ReferenceImage | Qualification deriving (Eq, Show)
data Kind = StockExport | Veneer | RetainedImport | CallbackRegistry | KernelSlot | LayoutView | AssemblySeam | OwnedBridge deriving (Eq, Show, Enum, Bounded)
data EvidenceKind = Export | Import | Layout | Publication | RetainedEntry | Reference deriving (Eq, Show)
data Predicates = Predicates { callerClosure :: Truth, sameImageComparison :: Truth, directOwnedApi :: Truth } deriving (Eq, Show)
data ComparisonReceipt = ComparisonReceipt ContractPath Digest Digest deriving (Eq, Show)
data ComparisonProof = ComparisonProof ImageId Digest [UnitId] Predicates Bool deriving (Eq, Show)
data Evidence = Evidence { evidenceImage :: Maybe ImageId, evidenceKind :: EvidenceKind, evidenceSymbols :: [SymbolName], evidenceAddresses :: [GuestAddress], evidenceTargets :: [GuestAddress], evidenceContract :: Maybe ContractPath, evidenceRequirement :: String } deriving (Eq, Show)
data Row = Row { rowId :: ShimId, rowShim :: String, rowKind :: Kind, rowOther :: String, rowOwned :: Ownership, rowRemoval :: String, rowState :: ShimState, rowEvidence :: [Evidence], rowPredicates :: Predicates, rowComparison :: Maybe ComparisonReceipt, rowConsumers :: [UnitId] } deriving (Eq, Show)
data Unit = Unit { unitId :: UnitId, unitSummary :: String, unitClass :: UnitClass, unitRows :: [Row] } deriving (Eq, Show)
data PinnedArtifact = PinnedArtifact ContractPath Digest deriving (Eq, Show)
data Image = Image { imageId :: ImageId, imageElf :: ContractPath, imageUnits :: [UnitId], imageClass :: ImageClass, imageContracts :: [ContractPath], imageExports :: [SymbolName], imageImports :: [SymbolName], imageExpected :: Maybe Digest, imageQualified :: [PinnedArtifact] } deriving (Eq, Show)
data ReferenceArtifact = ReferenceArtifact ImageId [UnitId] ContractPath String deriving (Eq, Show)
data Manifest = Manifest [Image] [ReferenceArtifact] deriving (Eq, Show)
data ErrorCode = MissingDeclaration | MissingExport | UnexpectedImport | StaleArtifact | FalseRemoval | ContradictorySeam | UnboundSeam | InvalidCatalog | MissingArtifact deriving (Eq, Show)
data Refusal = Refusal ErrorCode String deriving (Eq, Show)
data RowOutcome = RowOutcome ShimId Truth ShimState deriving (Eq, Show)
data ImageOutcome = ImageOutcome ImageId Digest [RowOutcome] deriving (Eq, Show)

nonempty :: String -> Text -> Parser String
nonempty label value = let text = unpack value in if null text then fail (label ++ " cannot be empty") else pure text
identifier :: String -> Text -> Parser String
identifier label value = do
  text <- nonempty label value
  unless (all (\c -> isAlphaNum c || c `elem` ("_-./:+" :: String)) text) (fail (label ++ " contains unsupported characters"))
  pure text
instance FromJSON UnitId where parseJSON = withText "unit id" (fmap UnitId . identifier "unit id")
instance FromJSON ImageId where
  parseJSON = withText "image id" $ \v -> do
    text <- identifier "image id" v
    unless ("//" `isPrefixOf` text && ':' `elem` text) (fail "Bazel image label required")
    pure (ImageId text)
instance FromJSON ShimId where parseJSON = withText "shim id" (fmap ShimId . identifier "shim id")
instance FromJSON SymbolName where parseJSON = withText "symbol" (fmap SymbolName . nonempty "symbol")
instance FromJSON GuestAddress where parseJSON value = GuestAddress <$> parseJSON value
instance FromJSON Digest where
  parseJSON = withText "sha256" $ \v -> do
    let text = unpack v
    unless (length text == 64 && all isHexDigit text) (fail "SHA256 required")
    pure (Digest text)
instance FromJSON ContractPath where
  parseJSON = withText "contract path" $ \v -> do
    text <- nonempty "contract path" v
    unless (not (isAbsolute text) && all (`notElem` ["..", "."]) (splitDirectories text)) (fail "repository-relative path required")
    pure (ContractPath text)
closed :: String -> [(Text,a)] -> Value -> Parser a
closed name entries = withText name $ \text -> maybe (fail ("unknown " ++ name)) pure (lookup text entries)
instance FromJSON Truth where parseJSON = closed "predicate" [("true",Established),("false",Refuted),("unestablished",Unestablished)]
instance FromJSON ShimState where parseJSON = closed "shim state" [("required",Required),("condition met",ConditionMet),("removed",Removed)]
instance FromJSON Ownership where parseJSON = closed "ownership" [("no",NotOwned),("partly",PartlyOwned),("yes",Owned)]
instance FromJSON UnitClass where parseJSON = closed "unit classification" [("linked",LinkedUnit),("reference-only",ReferenceUnit),("module",ModuleUnit)]
instance FromJSON ImageClass where parseJSON = closed "image classification" [("production",Production),("reference-only",ReferenceImage),("qualification",Qualification)]
kindName :: Kind -> String
kindName kind = case kind of
  StockExport -> "stock-ABI export"
  Veneer -> "veneer or carrier at a stock address"
  RetainedImport -> "trampoline into retained stock"
  CallbackRegistry -> "callback registry for guest words"
  KernelSlot -> "kernel interface slot"
  LayoutView -> "guest-memory layout view"
  AssemblySeam -> "retained stock seam in assembly"
  OwnedBridge -> "C-ABI bridge between two owned units"
instance FromJSON Kind where parseJSON = withText "shim kind" $ \text -> maybe (fail "unknown shim kind") pure (lookup (unpack text) [(kindName kind,kind) | kind <- [minBound .. maxBound]])
instance FromJSON EvidenceKind where parseJSON = closed "evidence kind" [("export",Export),("retained-import",Import),("layout",Layout),("publication",Publication),("retained-entry",RetainedEntry),("reference-only",Reference)]
instance FromJSON Predicates where parseJSON = withObject "predicates" $ \o -> Predicates <$> o .: "caller_closure" <*> o .: "same_image_comparison" <*> o .: "direct_owned_api"
instance FromJSON ComparisonReceipt where parseJSON = withObject "comparison receipt" $ \o -> ComparisonReceipt <$> o .: "path" <*> o .: "sha256" <*> o .: "image_sha256"
instance FromJSON ComparisonProof where
  parseJSON = withObject "same-image comparison proof" $ \o -> do
    version <- o .: "schema" :: Parser Int
    unless (version == 1) (fail "unsupported comparison proof")
    ComparisonProof <$> o .: "image" <*> o .: "image_sha256" <*> o .: "compared_units" <*> o .: "predicates" <*> o .: "accepted"
instance FromJSON Evidence where parseJSON = withObject "evidence" $ \o -> Evidence <$> o .:? "image" <*> o .: "kind" <*> o .:? "symbols" .!= [] <*> o .:? "addresses" .!= [] <*> o .:? "retained_targets" .!= [] <*> o .:? "contract" <*> o .: "requirement"
instance FromJSON Row where parseJSON = withObject "shim" $ \o -> Row <$> o .: "id" <*> o .: "shim" <*> o .: "kind" <*> o .: "other_side" <*> o .: "owned" <*> o .: "removal" <*> o .: "state" <*> o .: "evidence" <*> o .: "predicates" <*> o .:? "comparison_receipt" <*> o .:? "consumers" .!= []
instance FromJSON Unit where parseJSON = withObject "unit" $ \o -> Unit <$> o .: "unit" <*> o .: "summary" <*> o .: "classification" <*> o .: "rows"
instance FromJSON PinnedArtifact where parseJSON = withObject "qualified artifact" $ \o -> PinnedArtifact <$> o .: "path" <*> o .: "sha256"
instance FromJSON Image where parseJSON = withObject "image" $ \o -> Image <$> o .: "id" <*> o .: "elf" <*> o .: "units" <*> o .: "classification" <*> o .:? "contracts" .!= [] <*> o .:? "required_exports" .!= [] <*> o .:? "allowed_imports" .!= [] <*> o .:? "expected_sha256" <*> o .:? "qualified_artifacts" .!= []
instance FromJSON ReferenceArtifact where parseJSON = withObject "reference artifact" $ \o -> ReferenceArtifact <$> o .: "id" <*> o .: "units" <*> o .: "contract" <*> o .: "reason"
instance FromJSON Manifest where
  parseJSON = withObject "image manifest" $ \o -> do
    version <- o .: "version" :: Parser Int
    unless (version == 1) (fail "unsupported shim manifest version")
    Manifest <$> o .: "images" <*> o .:? "references" .!= []

truthName :: Truth -> String
truthName Established = "true"
truthName Refuted = "false"
truthName Unestablished = "unestablished"
stateName :: ShimState -> String
stateName Required = "required"
stateName ConditionMet = "condition met"
stateName Removed = "removed"
removalTruth :: Predicates -> Truth
removalTruth (Predicates caller compared direct)
  | Refuted `elem` [caller,compared,direct] = Refuted
  | all (== Established) [caller,compared,direct] = Established
  | otherwise = Unestablished

readJson :: FromJSON a => FilePath -> IO a
readJson path = readInput (16*1024*1024) path >>= either (H.failH . ((path ++ ": ") ++)) pure . eitherDecodeStrict'
readInput :: Int -> FilePath -> IO B.ByteString
readInput = H.readRegularWith (pure ()) False
loadUnits :: FilePath -> IO [Unit]
loadUnits root = do
  names <- listDirectory (root </> "replacement")
  fmap concat $ forM (sortOn id names) $ \name -> do
    let path = root </> "replacement" </> name </> "shims.json"
    exists <- doesFileExist path
    if exists then do
      unit <- readJson path
      H.ensure (unitId unit == UnitId name) "unit declaration location differs"
      pure [unit]
    else pure []

validateCatalog :: [Unit] -> Manifest -> Either Refusal ()
validateCatalog units (Manifest images references) = do
  unique "unit" (map unitId units)
  unique "image" (map imageId images)
  unique "shim" (map rowId rows)
  unique "linked image path" (map imageElf images)
  forM_ images $ \image -> do
    unique "image unit" (imageUnits image)
    forM_ (imageUnits image) $ \owner -> require (owner `elem` map unitId units) MissingDeclaration (show (imageId image,owner))
  forM_ references $ \(ReferenceArtifact target owners _ reason) -> do
    require (not (null reason)) InvalidCatalog (show target)
    require (target `notElem` map imageId images) ContradictorySeam (show target)
    forM_ owners $ \owner -> require (owner `elem` map unitId units) MissingDeclaration (show (target,owner))
  forM_ units $ \unit -> do
    when (unitClass unit == LinkedUnit) $ require (any (elem (unitId unit) . imageUnits) images) MissingDeclaration (show (unitId unit))
    forM_ (unitRows unit) $ \row -> do
      forM_ (rowConsumers row) $ \consumer -> require (consumer `elem` map unitId units) MissingDeclaration (show (rowId row,consumer))
      require (not (null (rowEvidence row))) UnboundSeam (show (rowId row))
      require (not (null (rowRemoval row))) InvalidCatalog (show (rowId row))
      let removal = removalTruth (rowPredicates row)
      when (rowState row /= Required) $ require (removal == Established) FalseRemoval (show (rowId row))
      require (rowState row /= ConditionMet && (removal /= Established || rowState row == Removed)) FalseRemoval (show (rowId row))
      when (sameImageComparison (rowPredicates row) == Established) $ require (rowComparison row /= Nothing) FalseRemoval (show (rowId row))
      forM_ (rowEvidence row) $ \evidence -> do
        require (not (null (evidenceRequirement evidence))) UnboundSeam (show (rowId row))
        case evidenceKind evidence of
          Reference -> do
            require (evidenceImage evidence == Nothing && evidenceContract evidence /= Nothing) ContradictorySeam (show (rowId row))
            require (rowState row == Required) FalseRemoval (show (rowId row))
          _ -> do
            target <- maybe (Left (Refusal UnboundSeam (show (rowId row)))) Right (evidenceImage evidence)
            image <- maybe (Left (Refusal MissingDeclaration (show target))) Right (lookup target [(imageId i,i) | i <- images])
            require (unitId unit `elem` imageUnits image) ContradictorySeam (show (rowId row,target))
            if evidenceKind evidence `elem` [Export,Import]
              then require (not (null (evidenceSymbols evidence))) UnboundSeam (show (rowId row))
              else require (evidenceContract evidence /= Nothing && (not (null (evidenceSymbols evidence)) || not (null (evidenceAddresses evidence)) || not (null (evidenceTargets evidence)))) UnboundSeam (show (rowId row))
  where
    rows = concatMap unitRows units
    unique label values = require (length values == S.size (S.fromList values)) InvalidCatalog ("duplicate " ++ label)
    forM_ = flip mapM_

require :: Bool -> ErrorCode -> String -> Either Refusal ()
require yes code detail = unless yes (Left (Refusal code detail))

checkImage :: [Unit] -> Image -> Digest -> E.Elf -> Either Refusal ImageOutcome
checkImage units image digest elf = do
  case imageExpected image of
    Just expected -> require (expected == digest) StaleArtifact (show (imageId image))
    Nothing -> Left (Refusal StaleArtifact ("missing expected identity: " ++ show (imageId image)))
  linkedReferences <- referenceTargets elf
  forM_ (imageExports image) $ \name -> require (exported name) MissingExport (show (imageId image,name))
  forM_ actualImports $ \name -> do
    require (name `elem` imageImports image) UnexpectedImport (show (imageId image,name))
    require (any (elem name . evidenceSymbols . snd) active) MissingDeclaration (show (imageId image,name))
  forM_ active $ \(row,evidence) -> do
    let absent = rowState row == Removed
    when absent $ forM_ (rowConsumers row) $ \consumer -> require (consumer `elem` imageUnits image) FalseRemoval (show (rowId row,consumer))
    forM_ (evidenceSymbols evidence) $ \name -> do
      let present = case evidenceKind evidence of
            Export -> exported name
            Import -> any (\symbol -> SymbolName (E.symbolName symbol) == name && case E.symbolSection symbol of
              E.Undefined -> symbol `elem` E.globalSymbols elf
              E.Absolute -> symbol `elem` E.globalSymbols elf || S.member (GuestAddress (E.symbolValue symbol)) linkedReferences
              _ -> False) (E.elfSymbols elf)
            _ -> defined name
      require (present /= absent) (if absent then FalseRemoval else MissingExport) (show (rowId row,imageId image,name))
    forM_ (evidenceAddresses evidence) $ \(GuestAddress address) -> require ((E.byteAt elf address /= Nothing) /= absent) (if absent then FalseRemoval else UnboundSeam) (show (rowId row,address))
    forM_ (evidenceTargets evidence) $ \target -> require ((S.member target linkedReferences) /= absent) (if absent then FalseRemoval else UnboundSeam) (show (rowId row,target))
    case rowComparison row of
      Just (ComparisonReceipt _ _ compared) | sameImageComparison (rowPredicates row) == Established -> require (compared == digest) StaleArtifact (show (rowId row))
      _ -> pure ()
  let declared = S.fromList (concatMap (evidenceSymbols . snd) active)
      seamNames = [SymbolName text | symbol <- E.globalSymbols elf, let text = E.symbolName symbol, not (rustItem text), E.symbolSection symbol /= E.Undefined, E.symbolSection symbol /= E.Absolute]
  forM_ seamNames $ \name -> require (S.member name declared) MissingDeclaration (show (imageId image,name))
  let declaredTargets = S.fromList (concatMap (evidenceTargets . snd) active ++ [GuestAddress (E.symbolValue symbol) | symbol <- E.globalSymbols elf, E.symbolSection symbol == E.Absolute, SymbolName (E.symbolName symbol) `elem` imageImports image, S.member (SymbolName (E.symbolName symbol)) declared])
  forM_ (S.toList linkedReferences) $ \target@(GuestAddress address) -> require (allocated address || S.member target declaredTargets) UnexpectedImport (show (imageId image,target))
  forM_ [(row,evidence) | unit <- units, unitId unit `elem` imageUnits image, row <- unitRows unit, evidence <- rowEvidence row, evidenceKind evidence == Reference] $ \(row,evidence) ->
    forM_ (evidenceSymbols evidence) $ \name -> require (not (M.member name symbols)) ContradictorySeam (show (rowId row,imageId image,name))
  pure (ImageOutcome (imageId image) digest [RowOutcome (rowId row) (removalTruth (rowPredicates row)) (rowState row) | row <- participating])
  where
    symbols = M.fromList [(SymbolName (E.symbolName symbol),symbol) | symbol <- E.elfSymbols elf, not (null (E.symbolName symbol))]
    defined name = maybe False ((/= E.Undefined) . E.symbolSection) (M.lookup name symbols)
    exported name = any (\symbol -> SymbolName (E.symbolName symbol) == name && case E.symbolSection symbol of
      E.InSection index -> index >= 0 && index < length (E.elfSections elf) && let section = E.elfSections elf !! index in E.sectionAllocated section && E.sectionAddress section <= E.symbolValue symbol && toInteger (E.symbolValue symbol) < toInteger (E.sectionAddress section) + toInteger (E.sectionSize section)
      _ -> False) (E.globalSymbols elf)
    actualImports = [SymbolName (E.symbolName symbol) | symbol <- E.globalSymbols elf, E.symbolSection symbol `elem` [E.Undefined,E.Absolute], not (null (E.symbolName symbol))]
    participating = [row | unit <- units, unitId unit `elem` imageUnits image, row <- unitRows unit, any ((== Just (imageId image)) . evidenceImage) (rowEvidence row)]
    active = [(row,evidence) | row <- participating, evidence <- rowEvidence row, evidenceImage evidence == Just (imageId image)]
    rustItem text = any (`isPrefixOf` text) ["_R","_ZN"]
    allocated = LinkedReferences.allocatedAddress elf
    forM_ = flip mapM_

referenceTargets :: E.Elf -> Either Refusal (S.Set GuestAddress)
referenceTargets elf = either (Left . Refusal UnboundSeam) (Right . S.map GuestAddress) (LinkedReferences.linkedReferences elf)

checkAll :: FilePath -> [Unit] -> Manifest -> IO (Either Refusal [ImageOutcome])
checkAll root = checkAllWithBuildRoot root root

checkAllWithBuildRoot :: FilePath -> FilePath -> [Unit] -> Manifest -> IO (Either Refusal [ImageOutcome])
checkAllWithBuildRoot buildRoot root units manifest@(Manifest images references) = case validateCatalog units manifest of
  Left refusal -> pure (Left refusal)
  Right () -> do
    inventory <- I.discover buildRoot
    let inventoryCheck = do
          mapM_ (\(I.InventoryUnit owner) -> require (UnitId owner `elem` map unitId units) MissingDeclaration ("firmware build unit: " ++ owner)) (I.firmwareUnits inventory)
          mapM_ (\(I.InventoryImage target) -> require (ImageId target `elem` map imageId images) MissingDeclaration ("Root composition input: " ++ target)) (I.rootImages inventory)
    referenceChecks <- mapM (\(ReferenceArtifact _ _ path _) -> present (resolve path)) references
    referenceRows <- mapM checkEvidence [evidence | unit <- units, row <- unitRows unit, evidence <- rowEvidence row, evidenceKind evidence == Reference]
    checks <- forM images $ \image -> do
      contracts <- mapM (present . resolve) (imageContracts image)
      pins <- mapM checkPin (imageQualified image)
      rowChecks <- mapM checkEvidence [evidence | unit <- units, row <- unitRows unit, evidence <- rowEvidence row, evidenceImage evidence == Just (imageId image)]
      comparisons <- mapM (checkComparison image) [receipt | unit <- units, row <- unitRows unit, Just receipt <- [rowComparison row], any ((== Just (imageId image)) . evidenceImage) (rowEvidence row)]
      let path = resolve (imageElf image)
      exists <- doesFileExist path
      if not exists then pure (Left (Refusal MissingArtifact path)) else do
        bytes <- readInput (256*1024*1024) path
        let digest = digestBytes bytes
        pure $ do
          sequence_ contracts
          sequence_ pins
          sequence_ rowChecks
          sequence_ comparisons
          elf <- either (Left . Refusal InvalidCatalog) Right (E.parseElf bytes)
          checkImage units image digest elf
    pure (inventoryCheck >> sequence_ referenceChecks >> sequence_ referenceRows >> sequence checks)
  where
    resolve (ContractPath path) = root </> path
    present path = do
      exists <- doesFileExist path
      pure (require exists MissingArtifact path)
    checkPin (PinnedArtifact path expected) = do
      exists <- doesFileExist (resolve path)
      if not exists then pure (Left (Refusal MissingArtifact (resolve path))) else do
        actual <- digestBytes <$> readInput (256*1024*1024) (resolve path)
        pure (require (actual == expected) StaleArtifact (resolve path))
    checkComparison image (ComparisonReceipt path expected expectedImage) = do
      pin <- checkPin (PinnedArtifact path expected)
      case pin of
        Left refusal -> pure (Left refusal)
        Right () -> do
          bytes <- readInput (16*1024*1024) (resolve path)
          pure $ do
            proof <- either (Left . Refusal FalseRemoval) Right (eitherDecodeStrict' bytes)
            let ComparisonProof target digest compared predicates accepted = proof
            require (target == imageId image && digest == expectedImage) StaleArtifact (resolve path)
            require (accepted && S.size (S.fromList compared) >= 2 && S.fromList compared == S.fromList (imageUnits image) && removalTruth predicates == Established) FalseRemoval (resolve path)
    checkEvidence evidence = maybe (pure (Right ())) (present . resolve) (evidenceContract evidence)

digestBytes :: B.ByteString -> Digest
digestBytes = Digest . SHA.sha256Hex . SHA.sha256

inspectArtifacts :: FilePath -> Manifest -> IO Value
inspectArtifacts root (Manifest images _) = do
  artifacts <- forM images $ \image -> do
    let ContractPath path = imageElf image
        ImageId target = imageId image
    bytes <- readInput (256*1024*1024) (root </> path)
    elf <- either H.failH pure (E.parseElf bytes)
    references <- either (H.failH . show) pure (referenceTargets elf)
    let Digest digest = digestBytes bytes
    pure (object ["image" .= target,"sha256" .= digest,"symbols" .= map symbolReport (E.elfSymbols elf),"global_symbols" .= map E.symbolName (E.globalSymbols elf),"reference_targets" .= [address | GuestAddress address <- S.toList references],"external_targets" .= [address | GuestAddress address <- S.toList references, not (LinkedReferences.allocatedAddress elf address)]])
  pure (object ["admission_performed" .= False,"artifacts" .= artifacts])
  where
    symbolReport symbol = object ["name" .= E.symbolName symbol,"value" .= E.symbolValue symbol,"kind" .= show (E.symbolKind symbol),"section" .= show (E.symbolSection symbol)]

renderRegister :: [Unit] -> Manifest -> String
renderRegister units (Manifest images references) = unlines $
  ["# Shim register", "", "[Replacement index](README.md). Generated from owning-unit `shims.json` declarations and `shim-images.json` by `//replacement/shim_check:shim-check`. Edit declaration data, not this presentation.", "", "A removal requires established caller closure, direct owned API use and a qualified comparison of both sides in the same checked image. Ownership alone establishes none of these predicates. Link admission is not removal acceptance. Reference-only families remain required and are not checked production coverage.", "", "## Linked compositions", "", "| Image | Classification | Owning units | Bound families |", "| --- | --- | --- | --- |"] ++
  [table [imageName (imageId image),show (imageClass image),intercalate ", " (map unitName (imageUnits image)),show (length [() | unit <- units, row <- unitRows unit, any ((== Just (imageId image)) . evidenceImage) (rowEvidence row)])] | image <- images] ++
  ["", "## Nonlinked and frozen references", "", "| Artifact | Owning units | Classification requirement |", "| --- | --- | --- |"] ++
  [table [imageName target,intercalate ", " (map unitName owners),reason] | ReferenceArtifact target owners _ reason <- references] ++
  ["", "## Owned on both sides", "", "These rows are derived from the C-ABI bridge declarations. Each removal predicate remains independent of the ownership column.", "", "| Unit | Family | State | Removal predicate |", "| --- | --- | --- | --- |"] ++
  [table [unitName (unitId unit),rowShim row,stateName (rowState row),truthName (removalTruth (rowPredicates row))] | unit <- units, row <- unitRows unit, rowKind row == OwnedBridge] ++
  [""] ++ concatMap renderUnit (sortOn unitId units)
  where
    renderUnit unit = ["## " ++ unitName (unitId unit), "", unitSummary unit, "", "Classification: `" ++ unitClassName (unitClass unit) ++ "`. [Declarations](" ++ unitName (unitId unit) ++ "/shims.json).", "", "| Shim | Kind | Other side | Owned now | Removal condition | State | Removal predicate | Evidence |", "| --- | --- | --- | --- | --- | --- | --- | --- |"] ++ map renderRow (unitRows unit) ++ [""]
    renderRow row = table [rowShim row,kindName (rowKind row),rowOther row,ownedName (rowOwned row),rowRemoval row,stateName (rowState row),truthName (removalTruth (rowPredicates row)),intercalate "; " (map renderEvidence (rowEvidence row))]
    renderEvidence evidence = case evidenceImage evidence of
      Nothing -> "reference-only: " ++ evidenceRequirement evidence
      Just target -> imageName target ++ " (" ++ show (evidenceKind evidence) ++ ", " ++ show (length (evidenceSymbols evidence)) ++ " symbols, " ++ show (length (evidenceTargets evidence)) ++ " definite references)"
    table fields = "| " ++ intercalate " | " (map escape fields) ++ " |"
    escape = concatMap (\c -> if c == '|' then "&#124;" else [c])
    unitName (UnitId name) = name
    imageName (ImageId name) = "`" ++ name ++ "`"
    ownedName NotOwned = "no"
    ownedName PartlyOwned = "partly"
    ownedName Owned = "yes"
    unitClassName LinkedUnit = "linked"
    unitClassName ReferenceUnit = "reference-only"
    unitClassName ModuleUnit = "module"

report :: Either Refusal [ImageOutcome] -> Value
report (Left (Refusal code detail)) = object ["link_admitted" .= False,"code" .= show code,"detail" .= detail]
report (Right outcomes) = object ["link_admitted" .= True,"removal_closed" .= (not (null outcomes) && all imageClosed outcomes),"images" .= map imageReport outcomes]
  where
    imageClosed (ImageOutcome _ _ rows) = all (\(RowOutcome _ _ state) -> state == Removed) rows
    imageReport (ImageOutcome (ImageId name) (Digest digest) rows) = object ["image" .= name,"sha256" .= digest,"remaining_shims" .= length [() | RowOutcome _ _ state <- rows,state /= Removed],"rows" .= map rowReport rows]
    rowReport (RowOutcome (ShimId name) truth state) = object ["id" .= name,"removal" .= truthName truth,"state" .= stateName state]
