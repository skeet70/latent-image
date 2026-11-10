{-# LANGUAGE GHC2021 #-}
module CustomAdmission (admitCustomBoundaries, admitCustomBoundaryEncodings, customEntries, recordEntries, bankEntries, retiredSettingsRegions, CustomStoreWidth(..), customStoreName, customStoreWords, customStoreIndex, BankApplyElfInput(..), BankApplyBinding(..), BankApplyBoundary, admitBankApplyElf, bankApplyEntry, bankApplyEnd, bankApplyCallReturn, bankApplyBinding, bankApplyRoots) where

import qualified A32 as A
import qualified ArmElf as E
import Control.Monad (foldM, forM_, unless)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Bits (complement, (.&.))
import Data.List (isPrefixOf, sortOn)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import Emulation.OwnedCase (Symbol(..), readWordAt)
import Emulation.Sha256 (Sha256, sha256, sha256Hex)
import qualified FujiAF.FirstStage as F
import Replacement.PropertyService.Placement (Carrier(..), GuestAddress(..))
import Replacement.PropertyService.Artifact (storageStoreName, storageStoreWords, storageOutputName, storageOutputWords, storageStatusWords, mirrorBoundaries)
import qualified Replacement.RecordStore.Carrier as RecordCarrier

customEntries :: [(Word32, String)]
customEntries = [(0x6a8abc,"boot"),(0x6a865c,"read"),(0x6a86c4,"refresh"),(0x6a877c,"defaults"),(0x6a8484,"prepare"),(0x6a84d4,"check")]

recordEntries :: [(Word32, String)]
recordEntries = [(0x6ac2b4,"open"),(0x6ac328,"register"),(0x6ac478,"close"),(0x6ac4c8,"read_latest"),(0x6ac7d4,"append"),(0x6ac984,"erase_all"),(0x6acab8,"collect"),(0x6acda0,"read_at"),(0x6ae0fc,"zero")]

bankEntries :: [(Word32, String)]
bankEntries = [(0x630a34,"initialize"),(0x61db04,"copy"),(0x61db3c,"capture"),(0x61db18,"restore"),(0x61daa8,"exchange"),(0x61d8dc,"write_body"),(0x61d92c,"write_active_body"),(0x61d944,"write_header"),(0x61d994,"read_custom"),(0x61d9ec,"read_body"),(0x61da3c,"read_header"),(0x6309c4,"apply")]

retiredSettingsRegions :: [(Word32,Word32)]
retiredSettingsRegions =
  [ (0x630a34,0x630b14),(0x61db04,0x61db18),(0x61db3c,0x61db5c),(0x61db18,0x61db3c)
  , (0x61daa8,0x61db04),(0x61d8dc,0x61d92c),(0x61d92c,0x61d944),(0x61d944,0x61d994)
  , (0x61d994,0x61d9ec),(0x61d9ec,0x61da3c),(0x61da3c,0x61da8c),(0x6309c4,0x630a34)
  , (0x6a8484,0x6a86a4),(0x6a86c4,0x6a8bb4),(0x6ac2b4,0x6ae114)
  ]

-- The separate host carrier uses structural record-entry checks, not the property decoded-store proof.
admitCustomBoundaries :: Word32 -> B.ByteString -> M.Map String Symbol -> Either String ()
admitCustomBoundaries base bytes symbols = do
  unless (base == 0x5d0000) (Left "custom host carrier differs")
  admitCustomBoundaryEncodings base [Carrier (GuestAddress 0x5d0000) (GuestAddress 0x5e0000)] bytes symbols

-- Placement approval belongs to the caller.
admitCustomBoundaryEncodings :: Word32 -> [Carrier] -> B.ByteString -> M.Map String Symbol -> Either String ()
admitCustomBoundaryEncodings base carriers bytes symbols = do
  unless (not (B.null bytes) && B.length bytes `mod` 4 == 0 && base `mod` 4 == 0
    && toInteger base + toInteger (B.length bytes) <= 0x100000000)
    (Left "unaligned or empty custom artifact")
  unless (not (null carriers)) (Left "custom carriers are empty")
  forM_ carriers $ \(Carrier (GuestAddress low) (GuestAddress high)) ->
    unless (low >= base && low < high && low `mod` 4 == 0 && high `mod` 4 == 0)
      (Left "invalid custom carrier")
  let ordered = sortOn (\(Carrier (GuestAddress low) _) -> low) carriers
  forM_ (zip ordered (drop 1 ordered)) $ \(Carrier _ (GuestAddress high), Carrier (GuestAddress low) _) ->
    unless (high <= low) (Left "custom carriers overlap")
  Symbol textEnd _ <- required "__custom_text_end"
  unless (textEnd `mod` 4 == 0 && inside textEnd 0
    && any (\(Carrier (GuestAddress low) (GuestAddress high)) -> low < textEnd && textEnd <= high) carriers)
    (Left "custom text end leaves supplied carriers")
  unless (M.notMember "record_store_cache_tag" symbols) (Left "obsolete record-store cache-tag binding remains")
  let cacheBindings = M.fromList [(C.unpack name,address) | (name,address) <- RecordCarrier.cacheSymbols]
  forM_ (M.toList cacheBindings) $ \(name,target) -> do
    Symbol address size <- required name
    unless (address == target && size == 0) (Left (name ++ ": record cache/service binding differs"))
  forM_ (M.toList symbols) $ \(name,Symbol address size) -> do
    let physical = address .&. complement 0x40000000
        cacheAddress = physical >= 0x89f8f0 && physical < 0x89f8f8
          || physical >= 0x89fa60 && physical < 0x89fa68
          || physical >= 0x8a0080 && physical < 0x8e008c
    unless (not cacheAddress || M.lookup name cacheBindings == Just address && size == 0) (Left (name ++ ": undeclared cache ownership alias"))
  forM_ (M.toList symbols) $ \(name, Symbol address size) ->
    unless (size == 0 || inside address size
      && (inCarrier address size || M.lookup name fixedEntries == Just (address,size)))
      (Left (name ++ ": sized symbol leaves custom carriers"))
  forM_ ownedEntries $ \(address,name,entryName) -> do
    Symbol target size <- required name
    unless (inCarrier target size && toInteger target + toInteger size <= toInteger textEnd
      && target `mod` 4 == 0) (Left (name ++ ": missing owned function"))
    Symbol entryAddress _ <- required entryName
    unless (entryAddress == address) (Left (entryName ++ ": fixed entry moved"))
    tailTo entryName target
  tailTo "settings_custom_config_read" 0x69f8fc
  tailTo "settings_custom_reset_modes" 0x6a6fd8
  forM_ [minBound .. maxBound] $ \width -> exact (customStoreName width) (customStoreWords width)
  forM_ [("xm1_bank_property_set",0x624694),("xm1_bank_current_selection",0x6439e0),("xm1_bank_custom_refresh",0x6a86c4)] $ \(name,target) -> do
    Symbol address size <- required name
    unless (address == target && size == 0) (Left (name ++ ": bank ABI target differs"))
  forM_ (M.lookup "xm1_bank_record_append" symbols) $ \(Symbol address size) ->
    unless (address == 0x6ac7d4 && size == 0) (Left "bank record-append control target differs")
  forM_ [minBound .. maxBound] $ \width -> exact (storageStoreName width) (storageStoreWords width)
  forM_ [minBound .. maxBound] $ \width -> exact (storageOutputName width) (storageOutputWords width)
  exact "xm1_storage_status32" storageStatusWords
  forM_ mirrorBoundaries $ \(name,external,target) -> do
    Symbol bound boundSize <- required external
    unless (bound == target && boundSize == 0) (Left (external ++ ": retained target differs"))
    Symbol address size <- required name
    unless (size == 16 && inside address size && inCarrier address size && wordsAt address 2 == [0xe92d4800,0xe1a0b00d]
      && A.decode (address+8) (readWordAt bytes (fromIntegral (address+8-base))) == A.Branch (A.Cond 14) True target
      && wordsAt (address+12) 1 == [0xe8bd8800]) (Left (name ++ ": retained call boundary differs"))
  where
    required name = maybe (Left (name ++ ": missing symbol")) Right (M.lookup name symbols)
    ownedEntries =
      [(address,"settings_custom_" ++ role,"settings_custom_" ++ role ++ "_entry") | (address,role) <- customEntries]
      ++ [(address,"record_store_" ++ role,"record_store_entry_" ++ role) | (address,role) <- recordEntries]
      ++ [(address,"xm1_storage_" ++ role,"settings_bank_" ++ role ++ "_entry") | (address,role) <- bankEntries]
    fixedEntries = M.fromList [(entryName,(address,4)) | (address,_,entryName) <- ownedEntries]
    inCarrier address size = size > 0 && any (\(Carrier (GuestAddress low) (GuestAddress high)) ->
      low <= address && toInteger address + toInteger size <= toInteger high) carriers
    inside :: Word32 -> Word32 -> Bool
    inside address size = address >= base && toInteger address + toInteger size <= toInteger base + toInteger (B.length bytes)
    wordsAt address count = [readWordAt bytes (fromIntegral (address - base) + offset) | offset <- take count [0,4..]]
    tailTo name target = do
      Symbol address size <- required name
      unless (size == 4 && inside address size && A.decode address (readWordAt bytes (fromIntegral (address-base))) == A.Branch (A.Cond 14) False target) (Left (name ++ ": tail boundary differs"))
    exact name expected = do
      Symbol address size <- required name
      unless (size == 4 * fromIntegral (length expected) && inside address size && inCarrier address size
        && wordsAt address (length expected) == expected) (Left (name ++ ": storage boundary differs"))

data CustomStoreWidth = CustomStoreByte | CustomStoreHalf | CustomStoreWord deriving (Eq, Show, Enum, Bounded)

customStoreName :: CustomStoreWidth -> String
customStoreName width = case width of
  CustomStoreByte -> "settings_custom_store8"
  CustomStoreHalf -> "settings_custom_store16"
  CustomStoreWord -> "settings_custom_store32"

customStoreIndex :: CustomStoreWidth -> Word32
customStoreIndex width = case width of
  CustomStoreByte -> 16
  CustomStoreHalf -> 18
  CustomStoreWord -> 8

customStoreWords :: CustomStoreWidth -> [Word32]
customStoreWords width = case width of
  CustomStoreByte ->
    [ 0xe30f2834, 0xe3402089, 0xe0402002, 0xe352003c, 0x3a00000a
    , 0xe30f2874, 0xe3402089, 0xe0402002, 0xe3520034, 0x3a000005
    , 0xe302295c, 0xe344209c, 0xe0402002, 0xe3520028, 0x3a000000
    , 0xe7f000f0, 0xe5c01000, 0xe12fff1e
    ]
  CustomStoreHalf ->
    [ 0xe3100001, 0x1a00000e
    , 0xe30f2838, 0xe3402089, 0xe0402002, 0xe3520033, 0x3a00000a
    , 0xe30f2874, 0xe3402089, 0xe0402002, 0xe3520033, 0x3a000005
    , 0xe302295c, 0xe344209c, 0xe0402002, 0xe3520027, 0x3a000000
    , 0xe7f000f0, 0xe1c010b0, 0xe12fff1e
    ]
  CustomStoreWord ->
    [ 0xe3100003, 0x1a000004, 0xe30f2834, 0xe3402089, 0xe0402002
    , 0xe3520039, 0x3a000000, 0xe7f000f0, 0xe5801000, 0xe12fff1e
    ]

data BankApplyElfInput = BankApplyElfInput
  { bankApplyImageBase :: GuestAddress
  , bankApplyCarriers :: [Carrier]
  , bankApplyLinkedBytes :: B.ByteString
  , bankApplySymbolBytes :: B.ByteString
  , bankApplyElfBytes :: B.ByteString
  }

data BankApplyBinding = BankApplyBinding
  { bankApplyElfDigest :: Sha256
  , bankApplyLinkedDigest :: Sha256
  , bankApplySymbolsDigest :: Sha256
  , bankApplyBindingBase :: GuestAddress
  , bankApplyBindingCarriers :: [Carrier]
  } deriving (Eq)

instance Show BankApplyBinding where
  show binding = show (sha256Hex (bankApplyElfDigest binding),sha256Hex (bankApplyLinkedDigest binding)
    ,sha256Hex (bankApplySymbolsDigest binding),bankApplyBindingBase binding,bankApplyBindingCarriers binding)

data BankApplyBoundary = BankApplyBoundary
  { bankApplyEntry :: GuestAddress
  , bankApplyEnd :: GuestAddress
  , bankApplyCallReturns :: S.Set GuestAddress
  , bankApplyBinding :: BankApplyBinding
  , bankApplyRootEntries :: S.Set GuestAddress
  } deriving (Eq, Show)

bankApplyCallReturn :: BankApplyBoundary -> GuestAddress -> Bool
bankApplyCallReturn boundary address = S.member address (bankApplyCallReturns boundary)

bankApplyRoots :: BankApplyBoundary -> [GuestAddress]
bankApplyRoots = S.toAscList . bankApplyRootEntries

data AdmittedBankInstruction = AdmittedBankInstruction GuestAddress A.Instruction
data AdmittedBankFunction = AdmittedBankFunction
  { admittedBankEntry :: GuestAddress
  , admittedBankEnd :: GuestAddress
  , admittedBankInstructions :: [AdmittedBankInstruction]
  }

data MappingKind = ArmMapping | ThumbMapping | DataMapping deriving (Eq)

admitBankApplyElf :: BankApplyElfInput -> Either String BankApplyBoundary
admitBankApplyElf input = do
  elf <- E.parseElf (bankApplyElfBytes input)
  symbols <- F.parseSymbols (bankApplySymbolBytes input)
  let candidates = [symbol | symbol <- E.elfSymbols elf,E.symbolKind symbol == E.FunctionSymbol,E.symbolSize symbol > 0]
  functions <- mapM (admitBankFunction input elf symbols) candidates
  let extents=S.toAscList (S.fromList [(admittedBankEntry owned,admittedBankEnd owned) | owned <- functions])
  forM_ (zip extents (drop 1 extents)) $ \((_,previousEnd),(nextEntry,_)) ->
    unless (previousEnd <= nextEntry) (Left "bank apply ELF sized functions overlap without being exact aliases")
  function <- uniqueBankElf "bank apply ELF function" [owned | (symbol,owned) <- zip candidates functions,E.symbolName symbol == "xm1_storage_apply"]
  unless ([C.unpack kind | row <- C.lines (bankApplySymbolBytes input),
    [_,_,kind,name] <- [C.words row], C.unpack name == "xm1_storage_apply"] == ["T"])
    (Left "bank apply nm definition is not global text")
  let returns = S.fromList [GuestAddress (address+4)
        | AdmittedBankInstruction (GuestAddress address) instruction <- admittedBankInstructions function
        ,instruction == A.Branch (A.Cond 14) True 0x624694]
      roots = closeRoots (S.singleton (admittedBankEntry function))
      closeRoots current =
        let next=S.union current (S.fromList [admittedBankEntry owned | owned <- functions
              ,any (tailTo current) (admittedBankInstructions owned)])
        in if next == current then current else closeRoots next
      tailTo current (AdmittedBankInstruction _ instruction) = case instruction of
        A.Branch _ False target -> S.member (GuestAddress target) current
        _ -> False
      binding = BankApplyBinding (sha256 (bankApplyElfBytes input)) (sha256 (bankApplyLinkedBytes input))
        (sha256 (bankApplySymbolBytes input)) (bankApplyImageBase input) (bankApplyCarriers input)
  unless (not (S.null returns)) (Left "bank apply function has no admitted property-set call")
  pure (BankApplyBoundary (admittedBankEntry function) (admittedBankEnd function) returns binding roots)

uniqueBankElf :: String -> [a] -> Either String a
uniqueBankElf label items = case items of
  [item] -> Right item
  _ -> Left (label ++ " is absent or ambiguous")

admitBankFunction :: BankApplyElfInput -> E.Elf -> [F.Symbol] -> E.Symbol -> Either String AdmittedBankFunction
admitBankFunction input elf symbols function = do
  named <- F.lookupSymbol symbols (E.symbolName function)
  let entry = E.symbolValue function
      size = E.symbolSize function
      end = toInteger entry + toInteger size
      GuestAddress base = bankApplyImageBase input
  unless (E.symbolKind function == E.FunctionSymbol && size > 0
    && entry `mod` 4 == 0 && size `mod` 4 == 0 && end <= 0xffffffff
    && F.symbolAddress named == entry && F.symbolSize named == size)
    (Left "bank apply ELF and sized nm function differ")
  unless ([C.unpack kind | row <- C.lines (bankApplySymbolBytes input),
    [_,_,kind,name] <- [C.words row], C.unpack name == E.symbolName function] `elem` [["T"],["t"],["W"],["w"]])
    (Left "bank apply ancestor nm definition is not text")
  let inCarrier = any (\(Carrier (GuestAddress low) (GuestAddress high)) ->
        low <= entry && end <= toInteger high && low < high) (bankApplyCarriers input)
      fixedEntries = [(address,"settings_bank_" ++ role ++ "_entry") | (address,role) <- bankEntries]
        ++ [(address,"settings_custom_" ++ role ++ "_entry") | (address,role) <- customEntries]
        ++ [(address,"record_store_entry_" ++ role) | (address,role) <- recordEntries]
      fixedEntry = size == 4 && (entry,E.symbolName function) `elem` fixedEntries
  unless (inCarrier || fixedEntry) (Left "bank apply ancestor function leaves approved carriers and fixed veneers")
  sectionIndex <- case E.symbolSection function of
    E.InSection index -> Right index
    _ -> Left "bank apply function has no ELF section"
  section <- case drop sectionIndex (E.elfSections elf) of
    item : _ | sectionIndex >= 0 -> Right item
    _ -> Left "bank apply function section is absent"
  unless (E.sectionType section == 1 && E.sectionAllocated section
    && E.sectionExecutable section && not (E.sectionWritable section)
    && E.sectionAddress section <= entry
    && end <= toInteger (E.sectionAddress section) + toInteger (E.sectionSize section))
    (Left "bank apply section is not bounded read-only executable code")
  segment <- uniqueBankElf "bank apply ELF load segment"
    [item | item <- E.elfSegments elf, E.loadSegment item,
      E.segmentAddress item <= entry,
      end <= toInteger (E.segmentAddress item) + toInteger (E.segmentFileSize item)]
  unless (E.segmentExecutable segment && not (E.segmentWritable segment)
    && E.segmentFlags segment .&. 4 /= 0
    && E.segmentFileSize segment <= E.segmentMemorySize segment)
    (Left "bank apply load segment is not read-only executable code")
  unless (all (\item -> not (E.loadSegment item) || not (E.segmentWritable item)
    || toInteger (E.segmentAddress item) >= end
    || toInteger (E.segmentAddress item) + toInteger (E.segmentMemorySize item) <= toInteger entry)
    (E.elfSegments elf)) (Left "bank apply function has a writable load alias")
  unless (base <= entry && end <= toInteger base + toInteger (B.length (bankApplyLinkedBytes input)))
    (Left "bank apply function leaves published bytes")
  let count = fromIntegral size
      linked = B.take count (B.drop (fromIntegral (entry-base)) (bankApplyLinkedBytes input))
      sectionBytes = B.take count (B.drop (fromIntegral (E.sectionOffset section) + fromIntegral (entry-E.sectionAddress section)) (E.elfBytes elf))
      segmentBytes = B.take count (B.drop (fromIntegral (E.segmentOffset segment) + fromIntegral (entry-E.segmentAddress segment)) (E.elfBytes elf))
  unless (B.length linked == count && linked == sectionBytes && linked == segmentBytes)
    (Left "bank apply ELF function bytes differ from publication")
  let markers = [(symbol,kind) | symbol <- E.elfSymbols elf,
        E.symbolSection symbol == E.InSection sectionIndex,
        Just kind <- [mappingKind (E.symbolName symbol)]]
  forM_ markers $ \(symbol,kind) -> do
    let address = E.symbolValue symbol
    unless (E.symbolKind symbol == E.OtherSymbol && E.symbolSize symbol == 0
      && address >= E.sectionAddress section
      && toInteger address < toInteger (E.sectionAddress section) + toInteger (E.sectionSize section)
      && (kind /= ArmMapping || address `mod` 4 == 0)
      && (kind /= ThumbMapping || address `mod` 2 == 0))
      (Left "bank apply ELF mapping leaves its section or has an invalid shape")
  mappings <- foldM insertMapping M.empty [(E.symbolValue symbol,kind) | (symbol,kind) <- markers]
  (_,initial) <- maybe (Left "bank apply function lacks an ARM mapping") Right (M.lookupLE entry mappings)
  unless (initial == ArmMapping) (Left "bank apply entry is not A32 code")
  let finish = fromInteger end
      internal = [(address,kind) | (address,kind) <- M.toAscList mappings, address > entry, address < finish]
      spans = zip ((entry,initial):internal) (map fst internal ++ [finish])
  forM_ spans $ \((low,kind),high) ->
    unless (low `mod` 4 == 0 && high `mod` 4 == 0 && kind /= ThumbMapping)
      (Left "bank apply function has an unaligned or Thumb mapping")
  let instructions =
        [AdmittedBankInstruction (GuestAddress address) (A.decode address (readWordAt linked (fromIntegral (address-entry))))
        | ((low,ArmMapping),high) <- spans,address <- [low,low+4 .. high-4]]
  unless (not fixedEntry || case instructions of
    [AdmittedBankInstruction _ (A.Branch (A.Cond 14) False _)] -> True
    _ -> False) (Left "bank apply ancestor fixed veneer is not one unconditional tail branch")
  pure (AdmittedBankFunction (GuestAddress entry) (GuestAddress finish) instructions)
  where
    mappingKind name
      | name == "$a" || "$a." `isPrefixOf` name = Just ArmMapping
      | name == "$t" || "$t." `isPrefixOf` name = Just ThumbMapping
      | name == "$d" || "$d." `isPrefixOf` name = Just DataMapping
      | otherwise = Nothing
    insertMapping mappings (address,kind) = case M.lookup address mappings of
      Just previous | previous /= kind -> Left "bank apply ELF mappings conflict"
      _ -> Right (M.insert address kind mappings)
