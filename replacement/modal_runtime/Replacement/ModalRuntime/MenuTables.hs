{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Stock X-M1 1.50 menu tables read from the image-4 load bytes: the node registry, every node
-- reachable from it, the values and labels arrays, English label text and the property
-- conversion tables.
module Replacement.ModalRuntime.MenuTables
  ( Address (..)
  , NodeId (..)
  , PropertyId (..)
  , StringId (..)
  , Image
  , StringSlot
  , image4
  , stringSlot
  , image4Base
  , registryAddress
  , registrySlots
  , conversionPointers
  , conversionBound
  , stringSlotBase
  , stringTable
  , stringCount
  , Unresolved (..)
  , Walk (..)
  , SlotEntry (..)
  , RegistrySlot (..)
  , Role (..)
  , Reference (..)
  , Row (..)
  , StringRef (..)
  , Label (..)
  , NodeRecord (..)
  , NodeEntry (..)
  , ConversionRow (..)
  , TableStatus (..)
  , PropertyEntry (..)
  , MenuTables (..)
  , decodeMenuTables
  , resolveString
  , limits
  , menuTablesJson
  ) where

import Data.Aeson (object, (.=))
import qualified Data.Aeson as J
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Char (chr)
import Data.List (intercalate)
import qualified Data.Map.Strict as M
import Data.Maybe (mapMaybe)
import qualified Data.Set as S
import Data.Word (Word32)
import Numeric (showHex)

newtype Address = Address Word32 deriving (Eq, Ord, Show)
newtype NodeId = NodeId Word32 deriving (Eq, Ord, Show)
newtype PropertyId = PropertyId Word32 deriving (Eq, Ord, Show)
newtype StringId = StringId Word32 deriving (Eq, Ord, Show)

-- | Image-4 bytes linked at 'image4Base'.
newtype Image = Image B.ByteString

-- | One language slot linked at 'stringSlotBase'.
newtype StringSlot = StringSlot B.ByteString

image4 :: B.ByteString -> Image
image4 = Image

stringSlot :: B.ByteString -> StringSlot
stringSlot = StringSlot

image4Base :: Address
image4Base = Address 0x0061c400

registryAddress :: Address
registryAddress = Address 0x0081babc

-- | id4 FUN_0061d038 walks 0x0081babc..0x0081bcd0 inclusive.
registrySlots :: Word32
registrySlots = 0x86

conversionPointers :: Address
conversionPointers = Address 0x0081b09c

-- | id4 FUN_00631a00 rejects a property above 0x286.
conversionBound :: PropertyId
conversionBound = PropertyId 0x286

stringSlotBase :: Address
stringSlotBase = Address 0x0039c000

-- | English table base selected by plain FUN_0117e1b8; entries are {attr, line-record pointer}.
stringTable :: Address
stringTable = Address 0x0039f754

stringCount :: Word32
stringCount = 0x83c

data Unresolved = PointerOutsideImage | RunsPastImageEnd deriving (Eq, Ord, Show, Enum, Bounded)

-- | A terminated array behind a pointer word.
data Walk a = NullPointer | Walked [a] | WalkUnresolved Unresolved deriving (Eq, Show)

data SlotEntry = EmptySlot | SlotNode Address | SlotUnreadable deriving (Eq, Show)

data RegistrySlot = RegistrySlot { slotId :: NodeId, slotEntry :: SlotEntry } deriving (Eq, Show)

data Role = ChildOf | ParentOf deriving (Eq, Ord, Show)

-- | The node at 'referenceFrom' names this node as a row child or as its parent.
data Reference = Reference { referenceFrom :: Address, referenceRole :: Role } deriving (Eq, Ord, Show)

data Row = Row { rowToken :: Word32, rowChild :: Maybe Address } deriving (Eq, Show)

data StringRef = StringRef { stringRefId :: StringId, stringRefText :: Maybe String } deriving (Eq, Show)

data Label = Label { labelPreview :: StringRef, labelList :: StringRef, labelHelp :: StringRef, labelName :: StringRef }
  deriving (Eq, Show)

data NodeRecord = NodeRecord
  { nodeRaw :: B.ByteString
  , nodeIdOf :: NodeId
  , nodeType :: Word32
  , nodePropertyOrClass :: Word32
  , nodeKind :: Word32
  , nodeWord4 :: Word32
  , nodeWord5 :: Word32
  , nodeFlag :: Word32
  , nodeParent :: Maybe Address
  , nodeValues :: Maybe Address
  , nodeLabels :: Maybe Address
  , nodeWord10 :: Word32
  , nodeRows :: Walk Row
  , nodeLabelRows :: Walk Label
  }
  deriving (Eq, Show)

data NodeEntry = NodeEntry
  { entryAddress :: Address
  , entryRegistryIds :: [NodeId]
  , entryReferences :: [Reference]
  , entryRecord :: Either Unresolved NodeRecord
  }
  deriving (Eq, Show)

data ConversionRow = ConversionRow { conversionStored :: Word32, conversionColumn1 :: Word32, conversionEffective :: Word32 }
  deriving (Eq, Show)

data TableStatus = BeyondConverterBound | NullTablePointer | PointerSlotUnreadable | Table Address (Walk ConversionRow)
  deriving (Eq, Show)

data PropertyEntry = PropertyEntry { propertyId :: PropertyId, propertyUsers :: [Address], propertyTable :: TableStatus }
  deriving (Eq, Show)

data MenuTables = MenuTables { tablesRegistry :: [RegistrySlot], tablesNodes :: [NodeEntry], tablesProperties :: [PropertyEntry] }
  deriving (Eq, Show)

offsetIn :: Address -> Int -> Address -> Int -> Maybe Int
offsetIn (Address base) size (Address address) width
  | address >= base && offset + width <= size = Just offset
  | otherwise = Nothing
 where
  offset = fromIntegral (address - base)

le32 :: B.ByteString -> Int -> Word32
le32 bytes offset = foldr (\index value -> (value `shiftL` 8) .|. fromIntegral (B.index bytes (offset + index))) 0 [0 .. 3]

le16 :: B.ByteString -> Int -> Word32
le16 bytes offset = fromIntegral (B.index bytes offset) .|. (fromIntegral (B.index bytes (offset + 1)) `shiftL` 8)

imageBytes :: Image -> Address -> Int -> Maybe B.ByteString
imageBytes (Image bytes) address width = (\offset -> B.take width (B.drop offset bytes)) <$> offsetIn image4Base (B.length bytes) address width

imageWord :: Image -> Address -> Maybe Word32
imageWord (Image bytes) address = le32 bytes <$> offsetIn image4Base (B.length bytes) address 4

advance :: Address -> Word32 -> Address
advance (Address address) delta = Address (address + delta)

pointer :: Word32 -> Maybe Address
pointer 0 = Nothing
pointer word = Just (Address word)

-- | Reads fixed-size records from a pointer until one satisfies the terminator.
walk :: Image -> Maybe Address -> Int -> (B.ByteString -> Bool) -> (B.ByteString -> a) -> Walk a
walk _ Nothing _ _ _ = NullPointer
walk image (Just start) width terminator decode = case imageBytes image start width of
  Nothing -> WalkUnresolved PointerOutsideImage
  Just _ -> go start []
 where
  go address acc = case imageBytes image address width of
    Nothing -> WalkUnresolved RunsPastImageEnd
    Just record
      | terminator record -> Walked (reverse acc)
      | otherwise -> go (advance address (fromIntegral width)) (decode record : acc)

recordWords :: B.ByteString -> [Word32]
recordWords record = [le32 record offset | offset <- [0, 4 .. B.length record - 4]]

nodeSize :: Int
nodeSize = 0x2c

decodeNode :: Image -> StringSlot -> Address -> Either Unresolved NodeRecord
decodeNode image slot address = case imageBytes image address nodeSize of
  Nothing -> Left PointerOutsideImage
  Just raw -> case recordWords raw of
    [identity, kind', property, rows, word4, word5, flag, parent, values, labels, word10] -> Right NodeRecord
      { nodeRaw = raw
      , nodeIdOf = NodeId identity
      , nodeType = kind'
      , nodePropertyOrClass = property
      , nodeKind = rows
      , nodeWord4 = word4
      , nodeWord5 = word5
      , nodeFlag = flag
      , nodeParent = pointer parent
      , nodeValues = pointer values
      , nodeLabels = pointer labels
      , nodeWord10 = word10
      , nodeRows = walk image (pointer values) 8 (\record -> recordWords record == [0x7ffffffe, 0]) decodeRow
      , nodeLabelRows = walk image (pointer labels) 16 (all (== 0xffffffff) . recordWords) (decodeLabel slot)
      }
    _ -> Left PointerOutsideImage
 where
  decodeRow record = case recordWords record of
    [token, child] -> Row token (pointer child)
    _ -> Row 0 Nothing

decodeLabel :: StringSlot -> B.ByteString -> Label
decodeLabel slot record = case map (\word -> StringRef (StringId word) (resolveString slot (StringId word))) (recordWords record) of
  [preview, list, help, name] -> Label preview list help name
  _ -> Label blank blank blank blank
 where
  blank = StringRef (StringId 0) Nothing

slotWord :: StringSlot -> Address -> Maybe Word32
slotWord (StringSlot bytes) address = le32 bytes <$> offsetIn stringSlotBase (B.length bytes) address 4

-- | UTF-16LE text from a pointer into the slot, up to its zero unit; Nothing when the string
-- leaves the slot.
slotText :: StringSlot -> Address -> Maybe String
slotText (StringSlot bytes) start = offsetIn stringSlotBase (B.length bytes) start 2 >>= units []
 where
  units acc offset
    | offset + 2 > B.length bytes = Nothing
    | unit == 0 = Just (utf16 (reverse acc))
    | otherwise = units (unit : acc) (offset + 2)
   where
    unit = le16 bytes offset
  utf16 (high : low : rest)
    | high >= 0xd800 && high < 0xdc00 && low >= 0xdc00 && low < 0xe000 =
        chr (fromIntegral (0x10000 + ((high - 0xd800) `shiftL` 10) + (low - 0xdc00))) : utf16 rest
  utf16 (unit : rest)
    | unit >= 0xd800 && unit < 0xe000 = '\xfffd' : utf16 rest
    | otherwise = chr (fromIntegral unit) : utf16 rest
  utf16 [] = []

-- | The English text of a string id below 'stringCount': the attr word's high halfword is the
-- line count, each 12-byte line record starts with a UTF-16LE pointer, lines join with newlines.
resolveString :: StringSlot -> StringId -> Maybe String
resolveString slot (StringId identity)
  | identity >= stringCount = Nothing
  | otherwise = do
      attr <- slotWord slot entry
      records <- slotWord slot (advance entry 4)
      let count = attr `shiftR` 16
      if count == 0
        then Nothing
        else intercalate "\n" <$> mapM (\index -> slotWord slot (Address (records + 12 * index)) >>= pointer >>= slotText slot) [0 .. count - 1]
 where
  entry = advance stringTable (8 * identity)

decodeMenuTables :: Image -> StringSlot -> MenuTables
decodeMenuTables image slot = MenuTables registry nodes properties
 where
  registry =
    [ RegistrySlot (NodeId identity) (maybe SlotUnreadable (maybe EmptySlot SlotNode . pointer) (imageWord image (advance registryAddress (4 * identity))))
    | identity <- [0 .. registrySlots - 1] ]
  registered = M.fromListWith (flip (++)) [(address, [identity]) | RegistrySlot identity (SlotNode address) <- registry]
  records = explore M.empty (M.keys registered)
  explore seen [] = seen
  explore seen (address : pending)
    | M.member address seen = explore seen pending
    | otherwise = explore (M.insert address record seen) (neighbours record ++ pending)
   where
    record = decodeNode image slot address
  neighbours (Left _) = []
  neighbours (Right record) = maybe [] pure (nodeParent record) ++ children record
  children record = case nodeRows record of
    Walked rows -> mapMaybe rowChild rows
    _ -> []
  references = M.fromListWith S.union
    ( [(child, S.singleton (Reference address ChildOf)) | (address, Right record) <- M.toList records, child <- children record]
    ++ [(parent, S.singleton (Reference address ParentOf)) | (address, Right record) <- M.toList records, Just parent <- [nodeParent record]] )
  nodes =
    [ NodeEntry address (M.findWithDefault [] address registered) (maybe [] S.toList (M.lookup address references)) record
    | (address, record) <- M.toList records ]
  users = M.fromListWith (flip (++)) [(PropertyId (nodePropertyOrClass record), [address]) | (address, Right record) <- M.toList records]
  properties = [PropertyEntry property addresses (tableOf property) | (property, addresses) <- M.toList users]
  tableOf property@(PropertyId number)
    | property > conversionBound = BeyondConverterBound
    | otherwise = case imageWord image (advance conversionPointers (4 * number)) of
        Nothing -> PointerSlotUnreadable
        Just 0 -> NullTablePointer
        Just table -> Table (Address table) (walk image (Just (Address table)) 12 ((== [0xffff, 0xffff, 0xffff]) . recordWords) conversionRow)
  conversionRow record = case recordWords record of
    [stored, column1, effective] -> ConversionRow stored column1 effective
    _ -> ConversionRow 0 0 0

hex :: Word32 -> String
hex value = "0x" ++ showHex value ""

addressText :: Address -> String
addressText (Address address) = hex address

hexBytes :: B.ByteString -> String
hexBytes = concatMap (\byte -> [digit (byte `shiftR` 4), digit (byte .&. 0xf)]) . B.unpack
 where
  digit nibble = "0123456789abcdef" !! fromIntegral nibble

unresolvedText :: Unresolved -> String
unresolvedText reason = case reason of
  PointerOutsideImage -> "pointer outside image"
  RunsPastImageEnd -> "runs past image end without terminator"

walkJson :: (a -> J.Value) -> Walk a -> J.Value
walkJson _ NullPointer = J.Null
walkJson encode (Walked items) = J.toJSON (map encode items)
walkJson _ (WalkUnresolved reason) = object ["unresolved" .= unresolvedText reason]

walkLength :: Walk a -> Maybe Int
walkLength (Walked items) = Just (length items)
walkLength _ = Nothing

-- | Decoding rules recorded in the output.
limits :: [String]
limits =
  [ "registry: 0x86 slots at 0x0081babc..0x0081bcd0 (ids 0..0x85), the bound of the id4 FUN_0061d038 walk at 0x0061d058..0x0061d084 (r3 = 0x0081bab8, r1 = r3 + 0x218, pre-increment load); a zero slot is empty"
  , "nodes: registry nodes plus every node reached through row children and parent pointers; a record not wholly inside image 4 [0x0061c400, end) is unresolved"
  , "values: 8-byte {row, child} pairs until {0x7ffffffe, 0} (id5 FUN_00bf9794 at 0x00bf97d4..0x00bf97e8); a pair with row 0x7ffffffe and a nonzero child is a row"
  , "labels: 16-byte {preview_icon, list_icon, help, name} records until a record of four 0xffffffff words; id5 FUN_00bf9360 indexes labels by row without a terminator, so a count differing from the rows is recorded"
  , "strings: English slot linked at 0x0039c000, table 0x0039f754 of 0x83c entries {attr, line-record pointer}; attr bits 16-31 are the line count; line records are 12 bytes starting with a UTF-16LE pointer; lines join with a newline; ids >= 0x83c, including Q glyph ids 0xff000000|n, are not in this table and get no text"
  , "properties: word 2 of every resolved node is looked up, including class numbers on page nodes; pointers *(0x0081b09c + 4*property) for properties 0..0x286 (id4 FUN_00631a00 bound); rows of three words until {0xffff, 0xffff, 0xffff} (same function)"
  , "walks are bounded by the image extent only"
  ]

menuTablesJson :: String -> String -> MenuTables -> J.Value
menuTablesJson imageSha stringsSha tables = object
  [ "schema" .= (1 :: Int)
  , "inputs" .= object ["image4_sha256" .= imageSha, "strings_sha256" .= stringsSha]
  , "limits" .= limits
  , "registry" .= map slotJson (tablesRegistry tables)
  , "nodes" .= map nodeJson (tablesNodes tables)
  , "properties" .= [tableJson entry address rows | entry@PropertyEntry {propertyTable = Table address rows} <- tablesProperties tables]
  , "properties_without_table" .= mapMaybe missingJson (tablesProperties tables)
  ]
 where
  slotJson (RegistrySlot (NodeId identity) entry) = object ["id" .= identity, case entry of
    EmptySlot -> "address" .= J.Null
    SlotNode address -> "address" .= addressText address
    SlotUnreadable -> "unresolved" .= unresolvedText PointerOutsideImage]
  stringJson (StringRef (StringId identity) text) = object ["id" .= hex identity, "text" .= text]
  rowJson (Row token child) = object ["row" .= token, "child" .= fmap addressText child]
  labelJson label = object
    [ "preview_icon" .= stringJson (labelPreview label), "list_icon" .= stringJson (labelList label)
    , "help" .= stringJson (labelHelp label), "name" .= stringJson (labelName label) ]
  referenceJson (Reference from role) = object ["node" .= addressText from, "as" .= (case role of ChildOf -> "row child" :: String; ParentOf -> "parent")]
  nodeJson entry = object
    ( [ "address" .= addressText (entryAddress entry)
      , "registry_ids" .= [identity | NodeId identity <- entryRegistryIds entry]
      , "referenced_by" .= map referenceJson (entryReferences entry) ]
    ++ either (\reason -> ["unresolved" .= unresolvedText reason]) recordFields (entryRecord entry) )
  recordFields record =
    [ "raw_hex" .= hexBytes (nodeRaw record)
    , "id" .= (\(NodeId identity) -> identity) (nodeIdOf record)
    , "type" .= nodeType record
    , "property_or_class" .= nodePropertyOrClass record
    , "kind" .= nodeKind record
    , "word4" .= nodeWord4 record
    , "word5" .= nodeWord5 record
    , "flag" .= nodeFlag record
    , "parent" .= fmap addressText (nodeParent record)
    , "values_address" .= fmap addressText (nodeValues record)
    , "labels_address" .= fmap addressText (nodeLabels record)
    , "word10" .= nodeWord10 record
    , "rows" .= walkJson rowJson (nodeRows record)
    , "labels" .= walkJson labelJson (nodeLabelRows record)
    , "row_label_mismatch" .= case (walkLength (nodeRows record), walkLength (nodeLabelRows record)) of
        (Just rows, Just labels) | rows /= labels -> object ["rows" .= rows, "labels" .= labels]
        _ -> J.Null
    ]
  tableJson entry address rows = object
    [ "property" .= (\(PropertyId number) -> number) (propertyId entry)
    , "table_address" .= addressText address
    , "used_by" .= map addressText (propertyUsers entry)
    , "rows" .= walkJson (\row -> object ["stored" .= conversionStored row, "col1" .= conversionColumn1 row, "effective" .= conversionEffective row]) rows ]
  missingJson entry = case propertyTable entry of
    Table _ _ -> Nothing
    status -> Just (object
      [ "property" .= (\(PropertyId number) -> number) (propertyId entry)
      , "used_by" .= map addressText (propertyUsers entry)
      , "reason" .= (case status of
          BeyondConverterBound -> "above converter bound 0x286" :: String
          NullTablePointer -> "null table pointer"
          _ -> unresolvedText PointerOutsideImage) ])
