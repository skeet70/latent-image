{-# LANGUAGE OverloadedStrings #-}

-- | Checks layout, definite ELF address references, direct transfers and evidenced stack budgets.
-- Literal and MOVW/MOVT scalars are not classified as pointers. Owned memory safety rests on the compiler, bounded types, source boundaries and subsystem comparisons.
module BankAllowlist
  ( Allowlist (..), StockCall (..), StockRegion (..)
  , parseAllowlist, Finding (..), Reason (..), LayoutRule (..), Region (..)
  , Layout (..), EngineExtents (..), readLayout, regionOf, writable, layoutFindings
  , Verdict (..), Report (..), allowlistAnalysisIdentity, checkBank, checkBankFile
  , AddressRange(..), InterfaceLayout(..), checkImageInterfaces
  , violations, declared, renderFinding, reportValue
  ) where

import A32
import ArmElf
import Control.Monad (unless, when)
import Data.Aeson (FromJSON (..), Value (..), withObject, (.:), (.:?))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (Parser)
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import Data.Char (isHexDigit)
import Data.List (find, groupBy, nub, sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (isJust)
import qualified Data.Set as S
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32)
import qualified EngineStack
import qualified RenderStack
import FujiAF.JSON (decodeStrictIntegers)
import Numeric (readHex, showHex)
import RamPushLayout (BankVariant, PublicationGroup (MenuEnginePublication, RenderPublication), hasPublicationGroup, bankVariantFromIdentity, bankVariantTag, modalRenderBankVariant, modalValueBankVariant, menuEngineRegionExtentFor, menuEngineRegionStartFor, runtimeAllocationSizeFor, runtimeBankEnd, runtimeDataBase, runtimeExecutableBase, runtimeInstallerBase)

data Allowlist = Allowlist
  { stockCalls :: [StockCall]
  , stockRegions :: [StockRegion]
  }

data StockCall = StockCall {stockCallAddress :: Word32, stockCallName :: String, stockCallSource :: String}

data StockRegion = StockRegion {stockRegionAddress :: Word32, stockRegionBytes :: Word32, stockRegionName :: String, stockRegionSource :: String}

parseAllowlist :: B.ByteString -> Either String Allowlist
parseAllowlist bytes = do
  document <- decodeStrictIntegers bytes
  closed "allowlist" document ["schema", "stock_calls", "stock_regions"]
  schema <- member document "schema"
  unless (schema == Number 2) (Left "allowlist: schema 2 required")
  calls <- list "stock_calls" document >>= mapM parseCall
  stores <- list "stock_regions" document >>= mapM parseStore
  unless (length (nub (map stockCallAddress calls)) == length calls) (Left "allowlist: duplicate stock call address")
  pure (Allowlist calls stores)
 where
  parseCall value = do
    closed "stock call" value ["address", "name", "source"]
    address <- member value "address" >>= hexWord
    unless (address .&. 3 == 0) (Left "allowlist: stock entry must be ARM aligned")
    StockCall address <$> (member value "name" >>= text) <*> (member value "source" >>= text)
  parseStore value = do
    closed "stock region" value ["address", "bytes", "name", "source"]
    address <- member value "address" >>= hexWord
    size <- member value "bytes" >>= byteCount
    unless (toInteger address + toInteger size <= 0x100000000) (Left "allowlist: stock region wraps")
    StockRegion address size <$> (member value "name" >>= text) <*> (member value "source" >>= text)
  member (Object object) key = maybe (Left ("allowlist: missing " ++ key)) Right (KM.lookup (K.fromString key) object)
  member _ key = Left ("allowlist: object required around " ++ key)
  closed label (Object object) keys =
    unless (S.fromList (map K.toString (KM.keys object)) == S.fromList keys) (Left ("allowlist: " ++ label ++ " needs exactly " ++ unwords keys))
  closed label _ _ = Left ("allowlist: " ++ label ++ " must be an object")
  list key document = member document key >>= \value -> case value of
    Array values -> Right (V.toList values)
    _ -> Left ("allowlist: " ++ key ++ " must be a list")
  text (String value) | not (T.null value) = Right (T.unpack value)
  text _ = Left "allowlist: nonempty string required"
  byteCount (Number n) | n >= 1 && n <= 0xffffffff && n == fromIntegral (truncate n :: Integer) = Right (truncate n)
  byteCount _ = Left "allowlist: positive 32-bit region size required"
  hexWord (String value) = case T.unpack value of
    '0' : 'x' : digits | length digits == 8 && all isHexDigit digits, [(word, "")] <- readHex digits -> Right word
    _ -> Left "allowlist: eight-digit 0x word required"
  hexWord _ = Left "allowlist: hex word string required"

data Verdict = Violation | DeclaredBy String deriving (Eq, Show)

data Finding = Finding
  { findingAddress :: Word32, findingFunction :: String, findingVerdict :: Verdict
  , findingReason :: Reason, findingDetail :: String
  } deriving (Eq, Show)

data Reason
  = LayoutProblem LayoutRule
  | UndeclaredStockTarget Word32
  | ExternalAddress Word32
  | StackBudgetViolation
  | ReportReason
  deriving (Eq, Show)

data Report = Report
  { reportFindings :: [Finding]
  , reportProvenBranches :: Int, reportInstructions :: Int, reportFunctions :: Int
  }

violations, declared :: Report -> [Finding]
violations = filter ((== Violation) . findingVerdict) . reportFindings
declared = filter ((/= Violation) . findingVerdict) . reportFindings

renderFinding :: Finding -> String
renderFinding finding = hex (findingAddress finding) ++ " " ++ findingFunction finding ++ ": " ++ findingDetail finding
  ++ case findingVerdict finding of Violation -> ""; DeclaredBy reason -> " [declared: " ++ reason ++ "]"

reportValue :: Report -> Value
reportValue report = Object (KM.fromList
  [ ("result", String (if null (violations report) then "pass" else "fail"))
  , ("analysis", String (T.pack allowlistAnalysisIdentity))
  , ("functions", count (reportFunctions report)), ("instructions", count (reportInstructions report))
  , ("proven_branches", count (reportProvenBranches report))
  , ("violations", Array (V.fromList (map item (violations report))))
  , ("declared", Array (V.fromList (map item (declared report))))
  ])
 where
  count = Number . fromIntegral
  item finding = Object (KM.fromList
    ([ ("address", String (T.pack (hex (findingAddress finding)))), ("function", String (T.pack (findingFunction finding)))
     , ("detail", String (T.pack (findingDetail finding))) ]
     ++ case findingVerdict finding of Violation -> []; DeclaredBy reason -> [("reason", String (T.pack reason))]))

hex :: Word32 -> String
hex value = let digits = showHex value "" in "0x" ++ replicate (8 - length digits) '0' ++ digits

allowlistAnalysisIdentity :: String
allowlistAnalysisIdentity = "bank-interface-check-r3"

instance FromJSON Report where
  parseJSON = withObject "bank allowlist report" $ \fields -> do
    result <- fields .: "result" :: Parser T.Text
    unless (result == "pass" || result == "overridden") (fail "allowlist report result is not pass or overridden")
    functions <- fields .: "functions"
    instructions <- fields .: "instructions"
    branches <- fields .: "proven_branches"
    violationFindings <- fields .: "violations" >>= mapM parseFinding
    declaredFindings <- fields .: "declared" >>= mapM parseFinding
    pure (Report (violationFindings ++ declaredFindings) branches instructions functions)
   where
    parseFinding = withObject "allowlist finding" $ \fields -> do
      addressText <- fields .: "address" :: Parser String
      address <- case readHex (drop 2 addressText) of [(value, "")] -> pure value; _ -> fail "invalid finding address"
      function <- fields .: "function"
      detail <- fields .: "detail"
      reason <- fields .:? "reason"
      pure (Finding address function (maybe Violation DeclaredBy reason) ReportReason detail)

-- * Layout

-- | What a layout finding breaks.
data LayoutRule
  = SectionOutsideLayout
  | SectionNotProgbits
  | BankCodeStart
  | EngineRegion
  | SectionExtent String
  | BankImageOverlap
  | GeneratedEnd
  | GeneratedBundleOverlap
  | InstallerPlacement
  | RuntimeExtent
  | InstallerSectionOutside
  | WritableExecutableSegment
  | ThumbCode
  deriving (Eq, Show)

data Layout = Layout
  { allocationBase :: Word32
  , allocationEnd :: Word32
  , textStart :: Word32
  , textEnd :: Word32
  , dataStart :: Word32
  , imageEnd :: Word32
  , generatedStart :: Word32
  , bankEnd :: Word32
  , installerStart :: Word32
  , runtimeEnd :: Word32
  , -- | The menu engine code and the writable data that follows it.
    engine :: Maybe EngineExtents
  , layoutBankVariant :: BankVariant
  }

data EngineExtents = EngineExtents
  { engineCodeStart :: Word32
  , engineCodeEnd :: Word32
  , engineDataStart :: Word32
  , engineDataEnd :: Word32
  }
  deriving (Eq, Show)

data Region
  = AllocationHeader
  | BankCode
  | BankData
  | EngineData
  | AllocationGap
  | GeneratedArea
  | ExecutableBundle
  | InstallerArea
  | AllocationTail
  | StockImage String
  | DeviceSpace
  | StockMemory
  deriving (Eq, Show)


deviceSpaceStart :: Word32
deviceSpaceStart = 0x80000000

-- | The decoded images the installed loader runs: image 4, image 5 and the
-- plain image (the extents emitRamPush writes into the allocation header).
stockRegion :: Word32 -> Region
stockRegion address
  | address >= deviceSpaceStart = DeviceSpace
  | within 0x0061c400 0x00282800 = StockImage "image 4"
  | within 0x00bf1a00 0x004d0020 = StockImage "image 5"
  | within 0x01100000 0x00080000 = StockImage "plain image"
  | otherwise = StockMemory
 where
  within start size = start <= address && toInteger address < toInteger start + size


regionOf :: Layout -> Word32 -> Region
regionOf layout address
  | address < allocationBase layout || address >= allocationEnd layout = stockRegion address
  | Just region <- engine layout, engineCodeStart region <= address && address < engineCodeEnd region = BankCode
  | Just region <- engine layout, engineDataStart region <= address && address < engineDataEnd region = EngineData
  | address < textStart layout = AllocationHeader
  | address < textEnd layout = BankCode
  | address >= dataStart layout && address < imageEnd layout = BankData
  | address >= generatedStart layout && address < bankEnd layout = GeneratedArea
  | address >= bankEnd layout && address < installerStart layout = ExecutableBundle
  | address >= installerStart layout && address < runtimeEnd layout = InstallerArea
  | address >= runtimeEnd layout = AllocationTail
  | otherwise = AllocationGap

writable :: Region -> Bool
writable region = region == BankData || region == EngineData || region == GeneratedArea

readLayout :: Elf -> Either String Layout
readLayout elf = do
  base <- absolute "RAM_PUSH_BASE"
  end <- absolute "RAM_PUSH_BANK_END"
  start <- symbol "__rb_image_start"
  text <- symbol "__rb_text_end"
  image <- symbol "__rb_image_end"
  installer <- symbol "__ram_push_installer_start"
  runtime <- symbol "__ram_push_end"
  count <- absolute "RAM_PUSH_LINK_PUBLICATION_COUNT"
  tag <- case [symbolValue s | s <- elfSymbols elf, symbolName s == "RAM_PUSH_BANK_VARIANT", symbolSection s == Absolute] of
    [] -> Right Nothing
    [1] -> Right (Just (bankVariantTag modalRenderBankVariant))
    [2] -> Right (Just (bankVariantTag modalValueBankVariant))
    _ -> Left "layout: unsupported bank variant identity"
  variant <- bankVariantFromIdentity tag (fromIntegral count)
  let allocationSize = runtimeAllocationSizeFor variant
  dataSection <- case [s | s <- elfSections elf, sectionName s == ".data"] of
    [s] -> Right s
    _ -> Left "layout: exactly one .data section required"
  unless (toInteger base + toInteger allocationSize <= 0xffffffff) (Left "layout: allocation leaves the address space")
  engineRegion <- if hasPublicationGroup MenuEnginePublication variant
    then Just <$> (EngineExtents
      <$> symbol "__rb_menu_engine_start"
      <*> symbol "__rb_menu_engine_end"
      <*> symbol "__rb_menu_engine_data_start"
      <*> symbol "__rb_menu_engine_data_end")
    else pure Nothing
  pure
    Layout
      { allocationBase = base
      , allocationEnd = base + allocationSize
      , textStart = start
      , textEnd = text
      , dataStart = sectionAddress dataSection
      , imageEnd = image
      , generatedStart = runtimeDataBase base
      , bankEnd = end
      , installerStart = installer
      , runtimeEnd = runtime
      , engine = engineRegion
      , layoutBankVariant = variant
      }
 where
  absolute name = case [s | s <- elfSymbols elf, symbolName s == name, symbolSection s == Absolute] of
    [s] -> Right (symbolValue s)
    _ -> Left ("layout: absolute symbol " ++ name ++ " required")
  symbol name = case [s | s <- elfSymbols elf, symbolName s == name, symbolSection s /= Undefined] of
    [s] -> Right (symbolValue s)
    _ -> Left ("layout: symbol " ++ name ++ " required")

bankSections, installerSections :: [String]
bankSections = [".text", ".data"]
installerSections = [".ram_push.text", ".ram_push.publications", ".ram_push.rendezvous", ".ram_push.mailbox"]

engineSections :: Layout -> [String]
engineSections layout
  | isJust (engine layout) = [".menu_engine", ".menu_engine_data"]
  | otherwise = []

layoutFindings :: Elf -> Layout -> [Finding]
layoutFindings elf layout =
  concat
    [ [ violation SectionOutsideLayout (sectionAddress s) ("allocated section " ++ sectionName s ++ " is outside the bank and installer layout")
      | s <- allocated
      , sectionName s `notElem` bankSections ++ installerSections ++ engineSections layout
      ]
    , [ violation SectionNotProgbits (sectionAddress s) ("section " ++ sectionName s ++ " is not PROGBITS")
      | s <- allocated
      , sectionType s /= 1
      ]
    , check BankCodeStart (textStart layout == runtimeExecutableBase (allocationBase layout)) (textStart layout) "bank code does not start at the allocation's executable offset"
    , case engine layout of
        Nothing -> []
        Just region -> concat
          [ check EngineRegion (engineCodeStart region == menuEngineRegionStartFor (layoutBankVariant layout) (allocationBase layout) && engineCodeStart region >= runtimeEnd layout
              && engineCodeStart region <= engineCodeEnd region && engineDataStart region >= engineCodeEnd region
              && engineDataStart region <= engineDataEnd region
              && engineDataEnd region <= menuEngineRegionStartFor (layoutBankVariant layout) (allocationBase layout) + menuEngineRegionExtentFor (layoutBankVariant layout) && engineDataEnd region <= allocationEnd layout)
              (engineCodeStart region) "menu engine region differs from the layout"
          , sectionCheck ".menu_engine" $ \s ->
              sectionAddress s == engineCodeStart region && sectionAddress s + sectionSize s == engineCodeEnd region
                && sectionExecutable s && not (sectionWritable s)
          , sectionCheck ".menu_engine_data" $ \s ->
              sectionAddress s == engineDataStart region && sectionAddress s + sectionSize s == engineDataEnd region
                && sectionWritable s && not (sectionExecutable s)
          ]
    , sectionCheck ".text" $ \s ->
        sectionAddress s == textStart layout && sectionAddress s + sectionSize s == textEnd layout
          && sectionExecutable s && not (sectionWritable s)
    , sectionCheck ".data" $ \s ->
        sectionAddress s >= textEnd layout && sectionAddress s + sectionSize s == imageEnd layout
          && sectionWritable s && not (sectionExecutable s)
    , check BankImageOverlap (textStart layout <= textEnd layout && textEnd layout <= dataStart layout && dataStart layout <= imageEnd layout && imageEnd layout <= generatedStart layout) (imageEnd layout) "bank image extents overlap or reverse"
    , check GeneratedEnd (bankEnd layout == runtimeBankEnd (allocationBase layout)) (bankEnd layout) "generated bank data has the wrong writable end"
    , check GeneratedBundleOverlap (generatedStart layout <= bankEnd layout && bankEnd layout <= installerStart layout) (bankEnd layout) "generated data overlaps the executable bundle"
    , check InstallerPlacement (installerStart layout == runtimeInstallerBase (allocationBase layout)) (installerStart layout) "installer has the wrong placement after the executable bundle"
    , check RuntimeExtent (installerStart layout <= runtimeEnd layout && runtimeEnd layout <= allocationEnd layout) (runtimeEnd layout) "runtime exceeds or reverses the installer extent"
    , [ violation InstallerSectionOutside (sectionAddress s) ("installer section " ++ sectionName s ++ " lies outside the installer extent")
      | s <- allocated
      , sectionName s `elem` installerSections
      , sectionAddress s < installerStart layout || sectionAddress s + sectionSize s > runtimeEnd layout
      ]
    , [ violation WritableExecutableSegment (segmentAddress g) "loadable segment is both writable and executable"
      | g <- elfSegments elf
      , loadSegment g
      , segmentWritable g && segmentExecutable g
      ]
    , [ violation ThumbCode (symbolValue s) "Thumb code in the bank"
      | s <- elfSymbols elf
      , symbolName s == "$t"
      , symbolValue s >= textStart layout && symbolValue s < imageEnd layout
      ]
    ]
 where
  allocated = filter sectionAllocated (elfSections elf)
  violation rule address = Finding address "<layout>" Violation (LayoutProblem rule)
  check rule ok address message = [violation rule address message | not ok]
  sectionCheck name predicate = case [s | s <- allocated, sectionName s == name] of
    [s] -> check (SectionExtent name) (predicate s) (sectionAddress s) ("section " ++ name ++ " has the wrong extent or permissions")
    _ -> [violation (SectionExtent name) 0 ("exactly one " ++ name ++ " section required")]


-- | Exact aliases share one function extent.
data Function = Function {functionName :: String, functionStart :: Word32, functionEnd :: Word32}

data AddressRange = AddressRange Word32 Word32 deriving (Eq, Show)
data InterfaceLayout = InterfaceLayout
  { interfaceAllocation :: AddressRange
  , interfaceCode :: [AddressRange]
  , interfaceSections :: [String]
  , interfaceLayoutProblems :: [Finding]
  }

checkBank :: Allowlist -> Elf -> Either String Report
checkBank allowlist elf = do
  layout <- readLayout elf
  let ranges = AddressRange (textStart layout) (textEnd layout) : maybe [] (\r -> [AddressRange (engineCodeStart r) (engineCodeEnd r)]) (engine layout)
      inside address = any (\(AddressRange low high) -> low <= address && address < high) ranges
      stack = case engine layout of
        Nothing -> []
        Just _ -> let result = do
                        _ <- EngineStack.checkEngineStack EngineStack.taskOneBudget elf inside
                        when (hasPublicationGroup RenderPublication (layoutBankVariant layout)) $ do
                          _ <- RenderStack.checkRenderStack elf inside
                          pure ()
                   in case result of Left problem -> [Finding (textStart layout) "<stack>" Violation StackBudgetViolation problem]; Right () -> []
  report <- checkImageInterfaces allowlist elf (InterfaceLayout
    (AddressRange (allocationBase layout) (allocationEnd layout)) ranges
    (bankSections ++ engineSections layout) (layoutFindings elf layout))
  pure report { reportFindings = reportFindings report ++ stack }

checkImageInterfaces :: Allowlist -> Elf -> InterfaceLayout -> Either String Report
checkImageInterfaces allowlist elf layout = do
  let layoutProblems = interfaceLayoutProblems layout
  relocations <- if null layoutProblems then addressRelocations elf else pure []
  unless (not (null relocations) || not (null layoutProblems)) (Left "retained ELF address relocations required; relink with --emit-relocs")
  let ranges = [(low,high) | AddressRange low high <- interfaceCode layout]
      inside address = any (\(lo, hi) -> lo <= address && address < hi) ranges
      symbols = [s | s <- elfSymbols elf, symbolKind s == FunctionSymbol, symbolSize s > 0, inside (symbolValue s)]
      groups = groupBy (\a b -> symbolValue a == symbolValue b && symbolSize a == symbolSize b)
        (sortOn (\s -> (symbolValue s, symbolSize s, symbolName s)) symbols)
      functions = [Function (symbolName s) (symbolValue s) (symbolValue s + symbolSize s) | s : _ <- groups]
      mappings = M.fromList [(symbolValue s, take 2 (symbolName s)) | s <- elfSymbols elf, take 2 (symbolName s) `elem` ["$a", "$d", "$t"]]
      isCode address = isJust (owner address) && maybe False ((== "$a") . snd) (M.lookupLE address mappings)
      owner address = find (\f -> functionStart f <= address && address < functionEnd f) functions
      code = [(address, word) | (lo, hi) <- ranges, address <- addresses lo hi, isCode address, Just word <- [wordAt elf address]]
      belongs target = inside target && isCode target
      entry target = target `elem` map stockCallAddress (stockCalls allowlist)
      region target = any (\s -> stockRegionAddress s <= target && toInteger target < toInteger (stockRegionAddress s) + toInteger (stockRegionBytes s)) (stockRegions allowlist)
      AddressRange allocationLow allocationHigh = interfaceAllocation layout
      owned target = allocationLow <= target && target < allocationHigh
      ownedSite address = any (\section -> sectionName section `elem` interfaceSections layout
        && sectionAddress section <= address && toInteger address + 4 <= toInteger (sectionAddress section) + toInteger (sectionSize section)) (elfSections elf)
      finding address reason detail verdict = Finding address (maybe "<unowned code>" functionName (owner address)) verdict reason detail
      external address target
        | owned target = []
        | entry target || region target = [finding address (ExternalAddress target) ("declared stock address " ++ hex target) (DeclaredBy "stock entry or region")]
        | otherwise = [finding address (ExternalAddress target) ("external address " ++ hex target ++ " is not a declared stock entry or region") Violation]
      transfer address target =
        [finding address (UndeclaredStockTarget target) ("direct transfer leaves owned code for undeclared entry " ++ hex target) Violation
        | not (belongs target) && not (entry target)]
      directSites = S.fromList [addressSite relocation | relocation <- relocations, addressKind relocation == DirectReference]
      inspect (address, word)
        | S.member address directSites = []
        | otherwise = maybe [] (transfer address) (directTarget address word)
      references = concat
        [ case addressKind relocation of
            DataReference -> external (addressSite relocation) (addressTarget relocation)
            DirectReference -> transfer (addressSite relocation) (addressTarget relocation)
        | relocation <- relocations
        , ownedSite (addressSite relocation) ]
      extents = [Finding (functionStart f) "<layout>" Violation (LayoutProblem (SectionExtent "function")) "function extent leaves owned code"
        | f <- functions, not (any (\(lo, hi) -> lo <= functionStart f && functionEnd f <= hi) ranges)]
      found = layoutProblems ++ extents ++ concatMap inspect code ++ references
  when (null functions) (Left "bank: no function symbols in owned code")
  pure (Report found
    (length [() | (address, word) <- code, Just target <- [directTarget address word], belongs target || entry target]) (length code) (length functions))
 where
  addresses lo hi = if hi < lo + 4 then [] else [lo, lo + 4 .. hi - 4]

checkBankFile :: B.ByteString -> B.ByteString -> Either String Report
checkBankFile allowlistBytes elfBytes = do
  allowlist <- parseAllowlist allowlistBytes
  elf <- parseElf elfBytes
  checkBank allowlist elf
