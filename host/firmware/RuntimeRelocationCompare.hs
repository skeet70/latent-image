-- | Source-bound comparison of the composed223 RAM-push runtime format.
-- Section addresses come from ELF; generated spans come from the bank plan and
-- BuildRecipeBank.relocatedD3. No byte is excluded from the comparison.
module RuntimeRelocationCompare
  ( comparePackages
  , PackageBytes (..)
  , Comparison (..)
  , comparisonValue
  ) where

import qualified A32 as A
import qualified ArmElf as E
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (FromJSON (..), Value, eitherDecodeStrict', object, withObject, withText, (.:), (.=))
import Data.Aeson.Types (Parser)
import Data.Bits (complement, shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (nub, sort, sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (mapMaybe)
import qualified Data.Text as T
import Data.Word (Word32)
import Numeric (readHex, showHex)
import qualified RamPushLayout as L
import qualified RuntimeRelocationProvenance as P

newtype Address = Address Word32 deriving (Eq, Ord, Show)
newtype ComponentName = ComponentName String deriving (Eq, Ord, Show)
data Phase = Setup | Activation deriving (Eq, Show)
data PlanRegion = MenuValues | MenuLabels | MenuParent | NameRecords | EditHalf | Stock deriving (Eq, Show)
data Mode = Arm | Data | Fixed | Publications | Metadata deriving (Eq, Show)
data DifferenceKind = Identical | Unclassified | LiteralPointer | WideAddress | ArmBranch | LiteralLoad | PublicationWide | PublicationBranch | PublicationPointer deriving (Eq, Show, Enum, Bounded)
data GeneratedShape = NodePointers | ChildPointers | ScalarData deriving (Eq, Show)
data PointerProof = ElfPointer P.AbsoluteRelocation | GeneratedPointer GeneratedShape Int | NameShowLiteral Address Address deriving (Eq, Show)
data PublicationPointerOrigin = BankPlanField | RelocatedD3Field | ClassicChromeField deriving (Eq, Show)
data PublicationPointerRule = PublicationPointerRule PublicationPointerOrigin Address
data Publication = Publication
  { publicationIndex :: Int
  , publicationAddress :: Address
  , publicationOld :: Word32
  , publicationNew :: Word32
  , publicationPhase :: Phase
  } deriving (Eq, Show)
data EntryKind = InstallEntry | RollbackEntry | StatusEntry deriving (Eq, Show)
data EntryPoint = EntryPoint EntryKind Address
data Manifest = Manifest Address Int [Publication] [EntryPoint]
data PlanWrite = PlanWrite PlanRegion Address B.ByteString B.ByteString String
newtype Plan = Plan [PlanWrite]
data Image = Image E.Elf B.ByteString Manifest Plan
-- | Four inputs per package: kernel.elf, runtime.bin, manifest, bank-plan.
data PackageBytes = PackageBytes
  { packageElf :: B.ByteString
  , packageRuntime :: B.ByteString
  , packageManifest :: B.ByteString
  , packagePlan :: B.ByteString
  }
data Component = Component ComponentName Address Int Mode deriving (Eq, Show)
data Mapping = Mapping Component Component deriving (Eq, Show)
data Evidence = Evidence ComponentName Address Address Int B.ByteString B.ByteString DifferenceKind (Maybe (Address, Address))
data Comparison = Comparison
  { comparisonAccepted :: Bool
  , comparisonMappings :: [Value]
  , comparisonDifferences :: [Value]
  , comparisonRawDifferences :: [Value]
  , comparisonCapture :: [Value]
  , comparisonSourceCompanions :: [Value]
  , comparisonGeneratedOverlays :: [Value]
  , comparisonPointerFields :: [Value]
  , comparisonPublicationPointerFields :: [Value]
  , comparisonProblems :: [String]
  , comparisonCounts :: Value
  }

hex :: Word32 -> String
hex value = "0x" ++ replicate (8 - length digits) '0' ++ digits where digits = showHex value ""
addressValue :: Address -> String
addressValue (Address value) = hex value
add :: Address -> Int -> Address
add (Address address) offset = Address (address + fromIntegral offset)
distance :: Address -> Address -> Int
distance (Address high) (Address low) = fromIntegral high - fromIntegral low

parseWord :: T.Text -> Parser Word32
parseWord text = case T.stripPrefix (T.pack "0x") text of
  Just digits -> bounded (readHex (T.unpack digits) :: [(Integer, String)])
  Nothing -> bounded (reads (T.unpack text) :: [(Integer, String)])
 where
  bounded [(value, "")] | value >= 0 && value <= toInteger (maxBound :: Word32) = pure (fromInteger value)
  bounded _ = fail "unsigned 32-bit address/word required"
instance FromJSON Address where parseJSON = withText "address" (fmap Address . parseWord)
instance FromJSON Phase where
  parseJSON = withText "publication phase" $ \text -> case T.unpack text of
    "setup" -> pure Setup
    "activation" -> pure Activation
    _ -> fail "unknown publication phase"
instance FromJSON Publication where
  parseJSON = withObject "publication" $ \o -> Publication <$> o .: "index" <*> o .: "address"
    <*> (o .: "old" >>= parseWord) <*> (o .: "new" >>= parseWord) <*> o .: "phase"
instance FromJSON Manifest where
  parseJSON = withObject "manifest" $ \o -> do
    schema <- o .: "schema" :: Parser Int
    unless (schema == 1) (fail "manifest schema 1 required")
    entries <- o .: "entry" >>= withObject "entry" (\e -> sequence
      [EntryPoint InstallEntry <$> e .: "install", EntryPoint RollbackEntry <$> e .: "rollback", EntryPoint StatusEntry <$> e .: "status"])
    Manifest <$> o .: "base" <*> o .: "size" <*> o .: "publications" <*> pure entries
instance FromJSON PlanRegion where
  parseJSON = withText "plan region" $ \text -> case T.unpack text of
    "menu_values" -> pure MenuValues
    "menu_labels" -> pure MenuLabels
    "menu_parent" -> pure MenuParent
    "name_records" -> pure NameRecords
    "edit_half" -> pure EditHalf
    "id4_code" -> pure Stock
    "image5" -> pure Stock
    "plain_sites" -> pure Stock
    _ -> fail "unknown bank plan region"
instance FromJSON PlanWrite where
  parseJSON = withObject "plan write" $ \o -> do
    fresh <- o .: "new" >>= parseBytes
    old <- o .: "current" >>= parseBytes
    unless (B.length fresh == B.length old) (fail "plan old/new byte lengths differ")
    PlanWrite <$> o .: "region" <*> o .: "address" <*> pure fresh <*> pure old <*> o .: "role"
   where
    parseBytes encoded = maybe (fail "plan bytes must be even-length hexadecimal") pure (decodeHex encoded)
instance FromJSON Plan where
  parseJSON = withObject "bank plan" $ \o -> do
    schema <- o .: "schema" :: Parser T.Text
    unless (schema == T.pack "recipe-bank-plan-1") (fail "recipe-bank-plan-1 required")
    Plan <$> o .: "writes"

decodeHex :: T.Text -> Maybe B.ByteString
decodeHex = fmap B.pack . go . T.unpack
 where
  go [] = Just []
  go (a : b : rest) = case readHex [a, b] of
    [(value, "")] -> (value :) <$> go rest
    _ -> Nothing
  go _ = Nothing

parseImage :: PackageBytes -> Either String Image
parseImage (PackageBytes elfBytes runtime manifestBytes planBytes) = do
  elf <- E.parseElf elfBytes
  manifest@(Manifest base size publications entries) <- eitherDecodeStrict' manifestBytes
  plan <- eitherDecodeStrict' planBytes
  unless (size == fromIntegral L.runtimeAllocationSize && size == B.length runtime) (Left "runtime allocation extent mismatch")
  unless (toIntegerAddress base + toInteger size <= 0x100000000) (Left "runtime extent wraps")
  unless (sort (map publicationIndex publications) == [0 .. L.publicationCount - 1]) (Left "publication indices are not the composed223 permutation")
  unless (length (nub (map publicationAddress publications)) == L.publicationCount) (Left "duplicate publication address")
  forM_ entries $ \(EntryPoint kind address) -> do
    let name = case kind of InstallEntry -> "ram_push_install"; RollbackEntry -> "ram_push_rollback"; StatusEntry -> "ram_push_status"
    symbol elf name >>= \value -> unless (value == address) (Left ("manifest entry mismatch: " ++ name))
  pure (Image elf runtime manifest plan)
 where
  toIntegerAddress (Address value) = toInteger value

symbol :: E.Elf -> String -> Either String Address
symbol elf name = case [Address (E.symbolValue s) | s <- E.elfSymbols elf, E.symbolName s == name, E.symbolSection s /= E.Undefined] of
  [address] -> Right address
  _ -> Left ("unique ELF symbol required: " ++ name)

imageBase :: Image -> Address
imageBase (Image _ _ (Manifest base _ _ _) _) = base
imageBytes :: Image -> B.ByteString
imageBytes (Image _ bytes _ _) = bytes
imageElf :: Image -> E.Elf
imageElf (Image elf _ _ _) = elf
imagePublications :: Image -> [Publication]
imagePublications (Image _ _ (Manifest _ _ publications _) _) = publications

-- An absent publication has an unchanged companion only when the plan carries it.
installedCompanion :: Image -> Address -> Maybe Word32
installedCompanion image@(Image _ _ _ (Plan writes)) address =
  case [publicationNew publication | publication <- imagePublications image, publicationAddress publication == address] of
    [value] -> Just value
    [] -> case [word fresh 0 | PlanWrite Stock site fresh old _ <- writes, site == address, fresh == old, B.length fresh == 4] of
      [value] -> Just value
      _ -> Nothing
    _ -> Nothing
slice :: Image -> Address -> Int -> Either String B.ByteString
slice image address size = do
  let offset = distance address (imageBase image)
  unless (offset >= 0 && size >= 0 && offset + size <= B.length (imageBytes image)) (Left ("runtime span outside allocation: " ++ addressValue address))
  pure (B.take size (B.drop offset (imageBytes image)))
word :: B.ByteString -> Int -> Word32
word bytes offset = sum [fromIntegral (B.index bytes (offset + index)) `shiftL` (8 * index) | index <- [0 .. 3]]
wordBytes :: Word32 -> B.ByteString
wordBytes value = B.pack [fromIntegral (value `shiftR` (8 * index)) | index <- [0 .. 3]]

components :: Image -> Either String [Component]
components image@(Image elf _ _ (Plan writes)) = do
  let sections = filter E.sectionAllocated (E.elfSections elf)
      expected = [".text", ".data", ".ram_push.text", ".ram_push.publications", ".ram_push.rendezvous", ".ram_push.mailbox"]
  unless (sort (map E.sectionName sections) == sort expected) (Left "allocated ELF section set changed")
  ccStart <- symbol elf "cc_ram_data"
  ccEnd <- symbol elf "cc_ram_data_end"
  unless (distance ccEnd ccStart == 100 && any (\section -> E.sectionName section == ".data" && Address (E.sectionAddress section) <= ccStart && ccEnd <= Address (E.sectionAddress section + E.sectionSize section)) sections) (Left "Classic Chrome generated data extent changed")
  sectionComponents <- forM sections $ \section -> do
    unless (E.sectionType section == 1 && E.sectionSize section > 0) (Left "allocated section must be nonempty PROGBITS")
    unless (E.sectionExecutable section == (E.sectionName section `elem` [".text",".ram_push.text"])) (Left "ELF executable section flags changed")
    let name = E.sectionName section
        address = Address (E.sectionAddress section)
        size = fromIntegral (E.sectionSize section)
        mode | name == ".ram_push.publications" = Publications
             | E.sectionExecutable section = Arm
             | otherwise = Data
    actual <- slice image address size
    unless (mode == Publications || and
      [ byte == B.index (E.elfBytes elf) (fromIntegral (E.sectionOffset section) + index)
      | (index, byte) <- zip [0 ..] (B.unpack actual)
      , let here = add address index
      , not (ccStart <= here && here < ccEnd)
      ]) (Left ("runtime differs from ELF outside generated overlays: " ++ name))
    pure (Component (ComponentName ("elf:" ++ name)) address size mode)
  generated <- forM (zip [0 :: Int ..] [w | w@(PlanWrite region _ _ _ _) <- writes, region /= Stock]) $ \(index, PlanWrite region address bytes _ role) -> do
    actual <- slice image address (B.length bytes)
    unless (actual == bytes) (Left ("runtime differs from bank plan: " ++ role))
    pure (Component (ComponentName ("plan:" ++ show index ++ ":" ++ show region ++ ":" ++ role)) address (B.length bytes) Data)
  nameAddress <- symbol elf "__rb_name_records"
  unless ([() | PlanWrite NameRecords address bytes _ _ <- writes, address == nameAddress, B.length bytes == 14 * 66] == [()]) (Left "complete fourteen-record name component required")
  bankEnd <- symbol elf "RAM_PUSH_BANK_END"
  unless (bankEnd == add nameAddress 0x400) (Left "name reservation changed")
  let generatedD3 = [Component (ComponentName ("d3:" ++ name)) (add bankEnd offset) size mode
        | (name, offset, size, mode) <- [("values",0,88,Data),("labels",88,176,Data),("q-labels",264,176,Data),("rows",440,16,Data),("properties",456,16,Data),("hook",472,76,Arm),("film-table",548,180,Data)]]
      allComponents = sortOn componentStart (sectionComponents ++ generated ++ generatedD3 ++
        [Component (ComponentName "header") (imageBase image) 0x480 Metadata, Component (ComponentName "fixed-store") (add (imageBase image) 0x480) 60 Fixed])
  forM_ allComponents $ \component -> do
    _ <- slice image (componentStart component) (componentSize component)
    unless (componentSize component > 0) (Left "empty component")
  forM_ (zip allComponents (drop 1 allComponents)) $ \(a, b) ->
    unless (componentEnd a <= componentStart b) (Left ("overlapping runtime components: " ++ show (componentName a, componentName b)))
  pure allComponents

-- BuildRecipeBank.encodeCustomNode/encodeListNode and encodeValuesArray.
generatedShape :: PlanRegion -> String -> Either String GeneratedShape
generatedShape region role = case region of
  NameRecords -> pure ScalarData
  MenuLabels -> pure ScalarData
  MenuValues -> case role of
    "slot list values" -> pure ChildPointers
    "sub-list values, one row per field" -> pure ChildPointers
    "slot list node header" -> pure NodePointers
    "select list node header" -> pure NodePointers
    "sub-list node header" -> pure NodePointers
    "bank name editor node header, rows over the stock charset arrays" -> pure NodePointers
    _ -> Left ("unknown generated menu layout: " ++ role)
  MenuParent -> case role of
    "parent values array with the two items" -> pure ChildPointers
    "parent labels array with the two items" -> pure ScalarData
    _ -> Left ("unknown generated parent layout: " ++ role)
  EditHalf -> case role of
    "edit screen node header" -> pure NodePointers
    "dialog node header" -> pure NodePointers
    "field-list node header" -> pure NodePointers
    "edit screen rows" -> pure ChildPointers
    "dialog rows" -> pure ChildPointers
    "field-list values" -> pure ChildPointers
    "edit screen row labels" -> pure ScalarData
    "dialog row labels" -> pure ScalarData
    "seven stock-mode film edit label quadruples" -> pure ScalarData
    _ -> Left ("unknown generated edit layout: " ++ role)
  Stock -> Left "stock publication is not a generated runtime component"

-- menu/recipe_bank_hook.c:31 uses RB_NAME_RECORD + (3 + slot) * 0x42.
-- The generator defines RB_NAME_RECORD as RamPushLayout.runtimeNameBase.
nameShowPointer :: Image -> [Component] -> Either String (Address,PointerProof)
nameShowPointer image owned = do
  function <- case [s | s <- E.elfSymbols (imageElf image), E.symbolName s == "rb_name_show", E.symbolSection s /= E.Undefined] of
    [s] | E.symbolKind s == E.FunctionSymbol && E.symbolSize s >= 0x98 -> pure s
    _ -> Left "named rb_name_show function with literal extent required"
  let start = Address (E.symbolValue function)
      consumer@(Address consumerAddress) = add start 0x0c
      literal = add start 0x94
  owner <- case [component | component <- owned, componentName component == ComponentName "elf:.text", contains component start, add literal 4 <= componentEnd component] of
    [component] -> pure component
    _ -> Left "rb_name_show consumer/literal outside ELF text"
  let modes = armMap image owner
  forM_ [0 .. 3] $ \offset -> do
    isCode <- armAt modes (add consumer offset)
    unless isCode (Left "rb_name_show consumer is not A32 code")
  instruction <- word <$> slice image consumer 4 <*> pure 0
  case A.decode consumerAddress instruction of
    A.Load (A.Cond 14) A.Word False (A.Reg 3) rn (A.OffsetImmediate offset) A.OffsetOnly
      | rn == A.pc && add consumer (8 + fromIntegral offset) == literal -> pure ()
    _ -> Left "rb_name_show consumer must load literal +0x94 with A32 LDR at +0x0c"
  let Address base = imageBase image
      names = Address (L.runtimeNameBase base)
      expected = add names 0xc6
  linkedNames <- symbol (imageElf image) "__rb_name_records"
  unless (linkedNames == names) (Left "rb_name_show generated name base differs from runtimeNameBase")
  bytes <- slice image literal 4
  unless (Address (word bytes 0) == expected) (Left "rb_name_show literal must equal generated names +0xc6")
  pure (literal,NameShowLiteral consumer expected)

pointerProofs :: Image -> [Component] -> Either String (M.Map Address PointerProof)
pointerProofs image@(Image elf _ _ (Plan writes)) owned = do
  relocations <- P.absoluteRelocations elf
  ccStart <- symbol elf "cc_ram_data"
  ccEnd <- symbol elf "cc_ram_data_end"
  elfFields <- forM relocations $ \relocation -> do
    let address = Address (P.relocationSite relocation)
    unless (address < ccStart || ccEnd <= address) (Left "R_ARM_ABS32 overlaps generated Classic Chrome scalars")
    pure (address,ElfPointer relocation)
  generated <- concat <$> forM [w | w@(PlanWrite region _ _ _ _) <- writes, region /= Stock] (\(PlanWrite region address bytes _ role) -> do
    shape <- generatedShape region role
    offsets <- case shape of
      NodePointers -> unless (B.length bytes == 44) (Left "generated node extent changed") >> pure [28,32,36]
      ChildPointers -> unless (B.length bytes >= 8 && B.length bytes `mod` 8 == 0) (Left "generated values-pair extent changed") >> pure [4,12 .. B.length bytes - 4]
      ScalarData -> pure []
    pure [(add address offset,GeneratedPointer shape offset) | offset <- offsets])
  d3Start <- symbol elf "RAM_PUSH_BANK_END"
  nameField <- nameShowPointer image owned
  let d3Fields = [(add d3Start offset,GeneratedPointer ChildPointers offset) | offset <- [4,12 .. 84]]
      fields = elfFields ++ generated ++ d3Fields ++ [nameField]
      table = M.fromList fields
  unless (M.size table == length fields) (Left "duplicate pointer provenance")
  let codeMaps = M.fromList [(componentName component,armMap image component) | component <- owned]
  forM_ fields $ \(address@(Address value),_) -> do
    owner <- case [component | component <- owned, contains component address, add address 4 <= componentEnd component] of
      [component] | value .&. 3 == 0 -> pure component
      _ -> Left "pointer field outside aligned component bytes"
    modes <- maybe (Left "pointer component code map missing") pure (M.lookup (componentName owner) codeMaps)
    isCode <- armAt modes address
    unless (not isCode) (Left "absolute pointer relocation overlaps A32 instruction")
    _ <- slice image address 4
    pure ()
  pure table

pointerProofMatches :: (Address -> Address -> Bool) -> PointerProof -> PointerProof -> Bool
pointerProofMatches related (ElfPointer left) (ElfPointer right) =
  let a = P.relocationSymbol left
      b = P.relocationSymbol right
   in E.symbolName a == E.symbolName b && E.symbolKind a == E.symbolKind b && E.symbolSize a == E.symbolSize b
      && P.relocationSymbolSection left == P.relocationSymbolSection right
      && P.relocationAddend left == P.relocationAddend right
      && related (Address (E.symbolValue a)) (Address (E.symbolValue b))
pointerProofMatches _ (GeneratedPointer shape offset) (GeneratedPointer shape' offset') = shape == shape' && offset == offset'
pointerProofMatches related (NameShowLiteral consumer target) (NameShowLiteral consumer' target') = related consumer consumer' && related target target'
pointerProofMatches _ _ _ = False

pointerFieldValues :: Image -> M.Map Address PointerProof -> [Value]
pointerFieldValues image fields =
  [object ["runtime_base" .= addressValue (imageBase image),"site" .= addressValue site,"target" .= hex target,"provenance" .= source proof]
  | (site,proof) <- M.toAscList fields
  , let target = word (imageBytes image) (distance site (imageBase image))
  ]
 where
  source (ElfPointer relocation) = object ["kind" .= ("R_ARM_ABS32" :: String),"symbol" .= E.symbolName (P.relocationSymbol relocation),"symbol_section" .= P.relocationSymbolSection relocation,"addend" .= hex (P.relocationAddend relocation)]
  source (GeneratedPointer shape offset) = object ["kind" .= ("generated-schema" :: String),"shape" .= show shape,"field_offset" .= offset]
  source (NameShowLiteral consumer expected) = object ["kind" .= ("literal-consumer-schema" :: String),"function" .= ("rb_name_show" :: String),"consumer" .= addressValue consumer,"expected_target" .= addressValue expected,"source" .= ("menu/recipe_bank_hook.c:31; RamPushLayout.runtimeNameBase" :: String)]

componentName :: Component -> ComponentName
componentName (Component name _ _ _) = name
componentStart :: Component -> Address
componentStart (Component _ start _ _) = start
componentSize :: Component -> Int
componentSize (Component _ _ size _) = size
componentEnd :: Component -> Address
componentEnd component = add (componentStart component) (componentSize component)
componentMode :: Component -> Mode
componentMode (Component _ _ _ mode) = mode
contains :: Component -> Address -> Bool
contains component address = componentStart component <= address && address < componentEnd component

mapComponents :: [Component] -> [Component] -> Either String [Mapping]
mapComponents left right = do
  unless (sort (map componentName left) == sort (map componentName right)) (Left "component identities changed")
  forM left $ \a -> case filter ((== componentName a) . componentName) right of
    [b] | componentSize a == componentSize b && componentMode a == componentMode b -> pure (Mapping a b)
    _ -> Left ("component size/mode changed: " ++ show (componentName a))

relates :: Image -> Image -> [Mapping] -> Address -> Address -> Bool
relates left right mappings a b = case nub candidates of
  [expected] -> b == expected
  [] -> not (inside left a) && not (inside right b) && a == b
  _ -> False
 where
  candidates = [add (componentStart y) (distance a (componentStart x)) | Mapping x y <- mappings, contains x a]
    ++ [componentEnd y | Mapping x y <- mappings, componentEnd x == a]
  inside image address = imageBase image <= address && address < add (imageBase image) (B.length (imageBytes image))

-- ELF mapping symbols delimit code and literal pools inside executable sections.
armMap :: Image -> Component -> Either String (M.Map Address Bool)
armMap image component
  | componentMode component /= Arm = pure (M.singleton (componentStart component) False)
  | componentName component == ComponentName "d3:hook" = pure (M.singleton (componentStart component) True)
  | otherwise = do
      unless (null thumb) (Left "Thumb mapping in A32 component")
      unless (all (\(address, mode) -> M.lookup address table == Just mode) markers) (Left "conflicting ELF mapping symbols")
      pure table
 where
  symbols = [s | s <- E.elfSymbols (imageElf image), contains component (Address (E.symbolValue s))]
  thumb = [s | s <- symbols, E.symbolName s == "$t"]
  markers = [(Address (E.symbolValue s), isArm) | s <- symbols, (name,isArm) <- [("$a",True),("$d",False)], E.symbolName s == name]
  table = M.fromList markers

armAt :: Either String (M.Map Address Bool) -> Address -> Either String Bool
armAt modes address = do
  table <- modes
  maybe (Left ("missing A32/data mapping at " ++ addressValue address)) (pure . snd) (M.lookupLE address table)

-- BuildRecipeBank's registry/dispatch words and relocatedD3 pointer list;
-- ClassicChrome.rawNodeValuesWord/rawNodeLabelsWord and shootingValuesWords.
publicationPointerSchemas :: Image -> Either String (M.Map Address PublicationPointerRule)
publicationPointerSchemas image@(Image _ _ _ (Plan writes)) =
  M.fromList . concat <$> forM (imagePublications image) (\publication -> do
    let site@(Address address) = publicationAddress publication
        Address base = imageBase image
        d3 offset = pure [(site,PublicationPointerRule RelocatedD3Field (Address (L.runtimeBankEnd base + offset)))]
        fixed target = pure [(site,PublicationPointerRule ClassicChromeField (Address target))]
    case bankRole address of
      Just expectedRole -> case [fresh | PlanWrite Stock actual fresh _ role <- writes, actual == site, role == expectedRole, B.length fresh == 4] of
        [fresh] -> pure [(site,PublicationPointerRule BankPlanField (Address (word fresh 0)))]
        _ -> Left ("bank publication pointer schema missing from plan at " ++ addressValue site)
      Nothing -> case address of
        0x007823c8 -> d3 88
        0x007823c4 -> d3 0
        0x00785d98 -> d3 88
        0x00785d94 -> d3 0
        0x0077d1a8 -> d3 88
        0x0077d1a4 -> d3 0
        0x0078b024 -> d3 264
        0x0078b020 -> d3 0
        0x0081b494 -> d3 548
        0x0078bd80 -> fixed 0x0061c388
        0x0078bd7c -> fixed 0x0061c264
        0x0061c384 -> fixed 0
        _ -> pure [])
 where
  bankRole address
    | address `elem` [0x0061eac4,0x0061eac8 .. 0x0061eae4] = Just "getter dispatch arm"
    | address `elem` [0x00625064,0x00625068 .. 0x00625084] = Just "setter dispatch arm"
    | address `elem` [0x0081ba38,0x0081ba3c .. 0x0081ba58] = Just "conversion pointer"
    | otherwise = lookup address
        [(0x0078eab8,"parent node +0x20 values pointer")
        ,(0x0078eabc,"parent node +0x24 labels pointer")
        ,(0x0081bc40,"registry entry for the slot page, the resolver root of the verb pages")
        ,(0x0081bb60,"registry entry for the confirmation dialog")
        ,(0x0081bc64,"registry entry for the editor node")
        ,(0x0081bc84,"registry entry for the edit screen")
        ,(0x00d04084,"modal action table word, the interposed entry")
        ,(0x00d04188,"modal75 RIGHT action66 bank navigation")
        ,(0x00d04114,"modal75 LEFT action37 bank navigation")]

publicationPointerValues :: Image -> M.Map Address PublicationPointerRule -> [Value]
publicationPointerValues image rules =
  [object ["runtime_base" .= addressValue (imageBase image),"site" .= addressValue site,"table_position" .= position,"actual_target" .= hex (publicationNew publication),"expected_target" .= addressValue expected,"source" .= show origin]
  | (position,publication) <- zip [0 :: Int ..] (imagePublications image)
  , let site = publicationAddress publication
  , Just (PublicationPointerRule origin expected) <- [M.lookup site rules]]

publicationChecks :: Image -> Either String Value
publicationChecks image = do
  let elf = imageElf image
      publications = imagePublications image
  start <- symbol elf "__ram_push_publications_start"
  end <- symbol elf "__ram_push_publications_end"
  unless (distance end start == L.publicationCount * 16) (Left "publication table extent changed")
  forM_ (zip [0 :: Int ..] publications) $ \(position, publication) -> do
    bytes <- slice image (add start (16 * position)) 16
    unless ([word bytes n | n <- [0,4,8,12]] ==
      [let Address value = publicationAddress publication in value, publicationOld publication, publicationNew publication, if publicationPhase publication == Setup then 0 else 1])
      (Left ("manifest/runtime publication mismatch at physical record " ++ show position))
    unless (publicationPhase publication == if position < L.publicationSetupCount then Setup else Activation) (Left "publication setup/activation boundary changed")
  let legacy = sortOn publicationIndex (filter ((< 97) . publicationIndex) publications)
  header <- slice image (imageBase image) 0x480
  unless (B.take 0x80 header == B.concat (map wordBytes [0x0061c400,0x00282800,0x00bf1a00,0x004d0020,0x01100000,0x00080000]) <> B.replicate 104 0) (Left "runtime header allowlist changed")
  unless (B.take 0x400 (B.drop 0x80 header) == B.concat [wordBytes address <> wordBytes (publicationNew p) | p <- legacy, let Address address = publicationAddress p] <> B.replicate (0x400 - 8 * length legacy) 0) (Left "legacy publication header differs from manifest/table")
  (position, capture) <- case [(position, p) | (position, p) <- zip [0 :: Int ..] publications, publicationAddress p == Address 0x00e9e038] of
    [entry] -> pure entry
    _ -> Left "unique capture publication required"
  target <- symbol elf "rb_capture_develop_wait"
  unless (publicationOld capture == 0xe92d4870 && publicationPhase capture == Activation) (Left "capture old word/phase changed")
  let field = add start (16 * position + 8)
  captureBytes <- slice image field 4
  let captureWord = word captureBytes 0
  unless (any (\s -> E.symbolName s == "rb_capture_develop_wait" && E.symbolKind s == E.FunctionSymbol && E.symbolSize s > 0) (E.elfSymbols elf)) (Left "capture target must be an ELF function")
  text <- case [section | section <- E.elfSections elf, E.sectionName section == ".text"] of
    [section] -> pure (Component (ComponentName "elf:.text") (Address (E.sectionAddress section)) (fromIntegral (E.sectionSize section)) Arm)
    _ -> Left "capture A32 text section missing"
  unless (contains text target && let Address address = target in address .&. 3 == 0) (Left "capture target outside aligned A32 text")
  targetIsArm <- armAt (armMap image text) target
  unless targetIsArm (Left "capture target is mapped as data")
  case A.decode 0x00e9e038 captureWord of
    A.Branch (A.Cond 14) False actual | Address actual == target -> pure ()
    _ -> Left "capture publication must be unconditional A32 B to rb_capture_develop_wait"
  pure (object ["table_record" .= addressValue (add start (16 * position)), "table_word" .= addressValue field, "installed_site" .= hex 0x00e9e038, "word" .= hex captureWord, "manifest_word" .= hex (publicationNew capture), "kind" .= ("B/AL" :: String), "target" .= addressValue target, "symbol" .= ("rb_capture_develop_wait" :: String)])

-- MOVW/MOVT evidence requires adjacent instructions with the same condition and register.
widePair :: Address -> Word32 -> Word32 -> Maybe Word32
widePair (Address address) low high = case (A.decode address low, A.decode (address + 4) high) of
  (A.MoveWide c r l, A.MoveTop c' r' h) | c == c' && r == r' -> Just (l .|. (h `shiftL` 16))
  _ -> Nothing

comparePackages :: PackageBytes -> PackageBytes -> Either String Comparison
comparePackages leftBytes rightBytes = do
  left <- parseImage leftBytes
  right <- parseImage rightBytes
  leftComponents <- components left
  rightComponents <- components right
  mappings <- mapComponents leftComponents rightComponents
  leftPointers <- pointerProofs left leftComponents
  rightPointers <- pointerProofs right rightComponents
  leftPublicationPointers <- publicationPointerSchemas left
  rightPublicationPointers <- publicationPointerSchemas right
  let sectionShape image = sort [(E.sectionName s,E.sectionType s,E.sectionFlags s,E.sectionSize s) | s <- E.elfSections (imageElf image), E.sectionAllocated s]
  unless (sectionShape left == sectionShape right) (Left "ELF section attributes changed")
  ccLeft <- symbol (imageElf left) "cc_ram_data"
  ccRight <- symbol (imageElf right) "cc_ram_data"
  captures <- mapM publicationChecks [left, right]
  let related = relates left right mappings
  unless (E.elfEntry (imageElf left) /= 0 && related (Address (E.elfEntry (imageElf left))) (Address (E.elfEntry (imageElf right)))) (Left "ELF entry changed")
  let symbolRows image = sortOn (\(name, size, kind, section, address) -> (name, size, show kind, show section, address)) (symbolsUnsorted image)
      symbolsUnsorted image = [(E.symbolName s, E.symbolSize s, E.symbolKind s, E.symbolSection s, Address (E.symbolValue s)) | s <- E.elfSymbols (imageElf image), E.symbolSection s /= E.Undefined]
      symbolProblems = ["ELF symbol correspondence changed" | not (length (symbolRows left) == length (symbolRows right) && and [n == n' && z == z' && k == k' && s == s' && related a b | ((n,z,k,s,a),(n',z',k',s',b)) <- zip (symbolRows left) (symbolRows right)])]
  evidence <- concat <$> mapM (compareComponent left right mappings leftPointers rightPointers leftPublicationPointers rightPublicationPointers) mappings
  let paddingProblems = paddingCheck leftComponents left ++ paddingCheck rightComponents right
      problems = symbolProblems ++ paddingProblems ++ ["unclassified semantic difference at " ++ addressValue a | Evidence _ a _ _ _ _ kind _ <- evidence, kind == Unclassified]
      raw = rawDifferences left right mappings
      changedBytes = sum [length [() | (a,b) <- B.zip before after, a /= b] | Evidence _ _ _ _ before after _ _ <- evidence]
  pure Comparison
    { comparisonAccepted = null problems
    , comparisonMappings = map (mappingValue (imageBase left) (imageBase right)) mappings
    , comparisonDifferences = map evidenceValue evidence
    , comparisonRawDifferences = raw
    , comparisonCapture = captures
    , comparisonSourceCompanions = concatMap sourceCompanionValues [left,right]
    , comparisonGeneratedOverlays = [object ["name" .= ("cc_ram_data" :: String), "left" .= addressValue ccLeft, "right" .= addressValue ccRight, "bytes" .= (100 :: Int), "containing_component" .= ("elf:.data" :: String)]]
    , comparisonPointerFields = pointerFieldValues left leftPointers ++ pointerFieldValues right rightPointers
    , comparisonPublicationPointerFields = publicationPointerValues left leftPublicationPointers ++ publicationPointerValues right rightPublicationPointers
    , comparisonProblems = problems
    , comparisonCounts = object
        ["runtime_bytes_each" .= B.length (imageBytes left), "components" .= length mappings, "semantic_fields" .= length evidence, "semantic_differing_bytes" .= changedBytes, "raw_differing_words" .= length raw, "raw_differing_bytes" .= length [() | (a,b) <- B.zip (imageBytes left) (imageBytes right), a /= b], "unclassified_fields" .= length [() | Evidence _ _ _ _ _ _ kind _ <- evidence, kind == Unclassified], "publication_records_checked_each" .= L.publicationCount, "pointer_fields_checked_left" .= M.size leftPointers, "pointer_fields_checked_right" .= M.size rightPointers, "publication_pointer_fields_checked_left" .= M.size leftPublicationPointers, "publication_pointer_fields_checked_right" .= M.size rightPublicationPointers
        ,"classifications" .= [object ["kind" .= show kind,"fields" .= length entries] | kind <- [minBound .. maxBound], let entries = [entry | entry@(Evidence _ _ _ _ _ _ actual _) <- evidence, actual == kind], not (null entries)]]
    }
 where
  paddingCheck owned image =
    ["nonzero unmapped byte at " ++ addressValue address | (offset, byte) <- zip [0 :: Int ..] (B.unpack (imageBytes image)), let address = add (imageBase image) offset, byte /= 0, not (any (`contains` address) owned)]

sourceCompanionValues :: Image -> [Value]
sourceCompanionValues image@(Image _ _ _ (Plan writes)) =
  [object ["runtime_base" .= addressValue (imageBase image), "site" .= addressValue address, "word" .= hex value, "role" .= role, "source" .= ("bank-plan unchanged stock instruction" :: String)]
  | PlanWrite Stock address fresh old role <- writes
  , fresh == old, B.length fresh == 4
  , let value = word fresh 0
  , case A.decode (let Address site = address in site) value of A.MoveWide {} -> True; A.MoveTop {} -> True; _ -> False
  , any (\publication -> publicationAddress publication == add address 4 || add (publicationAddress publication) 4 == address) (imagePublications image)
  ]

compareComponent :: Image -> Image -> [Mapping] -> M.Map Address PointerProof -> M.Map Address PointerProof -> M.Map Address PublicationPointerRule -> M.Map Address PublicationPointerRule -> Mapping -> Either String [Evidence]
compareComponent left right mappings leftPointers rightPointers leftPublicationPointers rightPublicationPointers (Mapping x y) = do
  before <- slice left (componentStart x) (componentSize x)
  after <- slice right (componentStart y) (componentSize y)
  go before after 0
 where
  related = relates left right mappings
  leftModes = armMap left x
  rightModes = armMap right y
  evidence offset size kind targets before after = Evidence (componentName x) (add (componentStart x) offset) (add (componentStart y) offset) size (B.take size (B.drop offset before)) (B.take size (B.drop offset after)) kind targets
  go before after offset
    | offset >= B.length before = pure []
    | otherwise = do
        let a@(Address av) = add (componentStart x) offset
            b@(Address bv) = add (componentStart y) offset
            aligned = av .&. 3 == 0 && bv .&. 3 == 0 && offset + 4 <= B.length before
        isArm <- armAt leftModes a
        isArm' <- armAt rightModes b
        unless (isArm == isArm') (Left ("code/data boundary changed at " ++ addressValue a))
        when (isArm && not aligned) (Left ("A32 code gap at " ++ addressValue a))
        let leftProof = M.lookup a leftPointers
            rightProof = M.lookup b rightPointers
        when (isArm && (leftProof /= Nothing || rightProof /= Nothing)) (Left "absolute pointer relocation overlaps A32 instruction")
        when aligned $ forM_ [0 .. 3] $ \index -> do
          leftMode <- armAt leftModes (add a index)
          rightMode <- armAt rightModes (add b index)
          unless (leftMode == isArm && rightMode == isArm') (Left ("code/data gap inside word at " ++ addressValue a))
        let size = if aligned then 4 else 1
            old = if aligned then word before offset else fromIntegral (B.index before offset)
            new = if aligned then word after offset else fromIntegral (B.index after offset)
            canPair = isArm && offset + 8 <= B.length before
            pair = if canPair then (,) <$> widePair a old (word before (offset + 4)) <*> widePair b new (word after (offset + 4)) else Nothing
        pairArm <- if canPair then (&&) <$> armAt leftModes (add a 4) <*> armAt rightModes (add b 4) else pure False
        let classify
              | componentMode x == Fixed = if old == new then (Identical, Nothing) else (Unclassified, Nothing)
              | componentMode x == Publications = publicationField offset a b old new
              | componentMode x == Metadata = metadataField offset a b old new
              | isArm = instructionField a b old new
              | leftProof /= Nothing || rightProof /= Nothing =
                  case (leftProof,rightProof) of
                    (Just proof,Just proof') | aligned && pointerProofMatches related proof proof' && related (Address old) (Address new) -> (LiteralPointer, Just (Address old,Address new))
                    _ -> (Unclassified, Just (Address old,Address new))
              | old == new = (Identical, Nothing)
              | otherwise = (Unclassified, Nothing)
        case pair of
          Just (oldTarget,newTarget) | pairArm -> do
            forM_ [4 .. 7] $ \index -> do
              leftMode <- armAt leftModes (add a index)
              rightMode <- armAt rightModes (add b index)
              unless (leftMode && rightMode) (Left ("code/data gap inside MOVW/MOVT at " ++ addressValue a))
            let unchanged = B.take 8 (B.drop offset before) == B.take 8 (B.drop offset after)
                mask = complement 0x000f0fff
                stable = old .&. mask == new .&. mask && word before (offset + 4) .&. mask == word after (offset + 4) .&. mask
                valid = stable && related (Address oldTarget) (Address newTarget)
                kind = if valid then WideAddress else Unclassified
                entries = [evidence offset 8 kind (Just (Address oldTarget,Address newTarget)) before after | not unchanged || not valid]
            rest <- go before after (offset + 8)
            pure (entries ++ rest)
          _ -> do
            let (kind, targets) = classify
                entries = [evidence offset size kind targets before after | old /= new || kind == Unclassified]
            rest <- go before after (offset + size)
            pure (entries ++ rest)
  instructionField a@(Address av) b@(Address bv) old new = case (A.decode av old, A.decode bv new) of
    (A.Branch c link target, A.Branch c' link' target') | c == c' && link == link' && related (Address target) (Address target') -> (ArmBranch, Just (Address target,Address target'))
    (A.Branch {}, _) -> (Unclassified, Nothing)
    (A.Load c A.Word False rd rn (A.OffsetImmediate offset) A.OffsetOnly, A.Load c' A.Word False rd' rn' (A.OffsetImmediate offset') A.OffsetOnly)
      | rn == A.pc && rn' == A.pc && c == c' && rd == rd' && related (add a (8 + fromIntegral offset)) (add b (8 + fromIntegral offset')) && old .&. 0xff7ff000 == new .&. 0xff7ff000 -> (LiteralLoad, Just (add a (8 + fromIntegral offset),add b (8 + fromIntegral offset')))
    (A.Load _ _ _ _ rn _ _, _) | rn == A.pc -> (Unclassified, Nothing)
    _ | old == new -> (Identical, Nothing)
      | otherwise -> (Unclassified, Nothing)
  publicationField offset _ _ old new
    | offset `mod` 16 == 8 = installedWord (offset `div` 16) old new
    | old == new = (Identical, Nothing)
    | otherwise = (Unclassified, Nothing)
  metadataField offset _ _ old new
    | offset >= 0x80 && offset < 0x80 + 97 * 8 && offset `mod` 8 == 4 =
        let logical = (offset - 0x80) `div` 8
            positions = [position | (position,p) <- zip [0 :: Int ..] (imagePublications left), publicationIndex p == logical]
         in case positions of [position] -> installedWord position old new; _ -> (Unclassified, Nothing)
    | old == new = (Identical, Nothing)
    | otherwise = (Unclassified, Nothing)
  installedWord position old new =
    let p = imagePublications left !! position
        q = imagePublications right !! position
        site@(Address address) = publicationAddress p
        adjacent image = installedCompanion image (add site 4)
        previous image = installedCompanion image (add site (-4))
        pairTargets = case (adjacent left, adjacent right, previous left, previous right) of
          (Just hi,Just hi',_,_) | Just target <- widePair site old hi, Just target' <- widePair site new hi' -> Just (target,target')
          (_,_,Just lo,Just lo') -> (,) <$> widePair (add site (-4)) lo old <*> widePair (add site (-4)) lo' new
          _ -> Nothing
     in if publicationAddress p /= publicationAddress q || publicationOld p /= publicationOld q || publicationPhase p /= publicationPhase q || publicationIndex p /= publicationIndex q then (Unclassified, Nothing)
        else if M.member site leftPublicationPointers || M.member (publicationAddress q) rightPublicationPointers then
          case (M.lookup site leftPublicationPointers,M.lookup (publicationAddress q) rightPublicationPointers) of
            (Just (PublicationPointerRule origin expected),Just (PublicationPointerRule origin' expected'))
              | origin == origin' && Address old == expected && Address new == expected' && related expected expected' -> (PublicationPointer,Just (Address old,Address new))
            _ -> (Unclassified,Just (Address old,Address new))
        else case pairTargets of
          Just (target,target') | related (Address target) (Address target') && old .&. complement 0x000f0fff == new .&. complement 0x000f0fff -> (PublicationWide, Just (Address target,Address target'))
          Just (target,target') -> (Unclassified,Just (Address target,Address target'))
          _ -> case (A.decode address old, A.decode address new) of
            (A.Branch c link target, A.Branch c' link' target') | c == c' && link == link' && related (Address target) (Address target') -> (PublicationBranch, Just (Address target,Address target'))
            (A.Branch {},_) -> (Unclassified,Nothing)
            _ | old == new -> (Identical, Nothing)
              | otherwise -> (Unclassified, Nothing)

mappingValue :: Address -> Address -> Mapping -> Value
mappingValue leftBase rightBase (Mapping a b) = object ["component" .= name, "left" .= addressValue (componentStart a), "right" .= addressValue (componentStart b), "bytes" .= componentSize a, "left_offset" .= distance (componentStart a) leftBase, "right_offset" .= distance (componentStart b) rightBase, "address_delta" .= distance (componentStart b) (componentStart a), "component_offset_delta" .= (distance (componentStart b) rightBase - distance (componentStart a) leftBase), "mode" .= show (componentMode a)]
 where ComponentName name = componentName a

evidenceValue :: Evidence -> Value
evidenceValue (Evidence (ComponentName name) a b size before after kind targets) = object
  ["component" .= name, "left" .= addressValue a, "right" .= addressValue b, "bytes" .= size, "left_bytes" .= bytesHex before, "right_bytes" .= bytesHex after, "classification" .= show kind, "differing_byte_indices" .= [index | (index,(x,y)) <- zip [0 :: Int ..] (B.zip before after), x /= y], "targets" .= fmap (\(x,y) -> [addressValue x,addressValue y]) targets]
bytesHex :: B.ByteString -> String
bytesHex = concatMap (\byte -> let digits = showHex byte "" in replicate (2 - length digits) '0' ++ digits) . B.unpack

rawDifferences :: Image -> Image -> [Mapping] -> [Value]
rawDifferences left right mappings =
  [object ["offset" .= offset, "left_word" .= hex (word before offset), "right_word" .= hex (word after offset), "bytes" .= changed]
  | offset <- [0,4 .. B.length before - 4], let changed = mapMaybe (byteEvidence offset) [0 .. 3], not (null changed)]
 where
  before = imageBytes left
  after = imageBytes right
  byteEvidence offset index
    | B.index before here == B.index after here = Nothing
    | otherwise = Just (object ["index" .= index, "left_owner" .= owner True (add (imageBase left) here), "right_owner" .= owner False (add (imageBase right) here), "classification" .= classification])
   where
    here = offset + index
    a = add (imageBase left) here
    b = add (imageBase right) here
    samePosition = any (\(Mapping x y) -> contains x a && contains y b && distance a (componentStart x) == distance b (componentStart y)) mappings
    classification :: String
    classification = if samePosition then "semantic-field" else "moved-component-or-zero-padding"
  owner isLeft address = case [name | Mapping x y <- mappings, let component = if isLeft then x else y, contains component address, let ComponentName name = componentName component] of
    [name] -> name
    _ -> "zero-padding"

comparisonValue :: Comparison -> Value
comparisonValue report = object
  ["schema" .= ("runtime-relocation-comparison-1" :: String), "accepted" .= comparisonAccepted report, "counts" .= comparisonCounts report, "components" .= comparisonMappings report, "generated_overlays" .= comparisonGeneratedOverlays report, "pointer_fields" .= comparisonPointerFields report, "publication_pointer_fields" .= comparisonPublicationPointerFields report, "semantic_differences" .= comparisonDifferences report, "physical_differences" .= comparisonRawDifferences report, "capture_publications" .= comparisonCapture report, "unchanged_stock_companions" .= comparisonSourceCompanions report, "problems" .= comparisonProblems report]
