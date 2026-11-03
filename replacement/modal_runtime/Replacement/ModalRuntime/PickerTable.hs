{-# LANGUAGE OverloadedStrings #-}

-- | The owned picker table (`picker.rs` PickerTable expression): picker nodes
-- from census-5 and the decoded menu tables, checked against image4, and the
-- controller-75 key cells from image5, checked against the controller
-- manifest.
module Replacement.ModalRuntime.PickerTable
  ( Image (..)
  , NodeAddress (..)
  , PropertyId (..)
  , ActionAddress (..)
  , Controller (..)
  , State (..)
  , Event (..)
  , CellWord (..)
  , PickerNode (..)
  , PickerAction (..)
  , CellSpec (..)
  , PickerCell (..)
  , TableNode (..)
  , MenuTables (..)
  , Dispatch (..)
  , CensusKey (..)
  , Census (..)
  , ManifestCell (..)
  , ManifestController (..)
  , Manifest (..)
  , Inputs (..)
  , Generated (..)
  , keyController
  , contractCells
  , pickerDispatches
  , generate
  , rustTable
  , reportJson
  ) where

import Control.Monad (unless, when)
import Data.Aeson (FromJSON (..), (.!=), (.:), (.:?), (.=))
import qualified Data.Aeson as J
import Data.Aeson.Types (Parser)
import Data.Bits (shiftL, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Char (isHexDigit)
import Data.List (sortOn)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word16, Word32)
import Numeric (readHex, showHex)

-- * Images

-- | Bytes loaded at `imageBase`.
data Image = Image {imageBase :: Word32, imageBytes :: B.ByteString}

bytesIn :: Image -> Word32 -> Int -> Maybe B.ByteString
bytesIn (Image base bytes) address count
  | address < base || toInteger (address - base) + toInteger count > toInteger (B.length bytes) = Nothing
  | otherwise = Just (B.take count (B.drop (fromIntegral (address - base)) bytes))

littleEndian :: B.ByteString -> Word32
littleEndian = B.foldr (\byte value -> (value `shiftL` 8) .|. fromIntegral byte) 0

wordIn :: Image -> Word32 -> Maybe Word32
wordIn image address = littleEndian <$> bytesIn image address 4

halfIn :: Image -> Word32 -> Maybe Word16
halfIn image address = fromIntegral . littleEndian <$> bytesIn image address 2

hex :: Word32 -> String
hex value = "0x" ++ replicate (8 - length digits) '0' ++ digits where digits = showHex value ""

-- * Table types

newtype NodeAddress = NodeAddress Word32 deriving (Eq, Ord, Show)
newtype PropertyId = PropertyId Word16 deriving (Eq, Ord, Show)
newtype ActionAddress = ActionAddress Word32 deriving (Eq, Ord, Show)
newtype Controller = Controller Word16 deriving (Eq, Ord, Show)
newtype State = State Word16 deriving (Eq, Ord, Show)
newtype Event = Event Word16 deriving (Eq, Ord, Show)
newtype CellWord = CellWord Word32 deriving (Eq, Show)

data PickerNode = PickerNode {pickerNode :: NodeAddress, pickerProperty :: PropertyId}
  deriving (Eq, Show)

-- | Constructor names are the Rust `PickerAction` variants.
data PickerAction = Up | Down | Left' | Right' | Ok | Back | RouterTail
  deriving (Eq, Show, Enum, Bounded)

rustAction :: PickerAction -> String
rustAction action = case action of
  Left' -> "Left"
  Right' -> "Right"
  _ -> show action

data CellSpec = CellSpec
  { specController :: Controller
  , specState :: State
  , specEvent :: Event
  , specTarget :: ActionAddress
  , specAction :: PickerAction
  }
  deriving (Eq, Show)

data PickerCell = PickerCell
  { cellSpec :: CellSpec
  , cellWord :: CellWord
  , cellTarget :: ActionAddress
  }
  deriving (Eq, Show)

-- | The only controller the table may name.
keyController :: Controller
keyController = Controller 75

contractCells :: [CellSpec]
contractCells =
  [ cell 0 0 0xc6e378 Up
  , cell 0 2 0xc6eb44 Down
  , cell 0 4 0xc6f14c Left'
  , cell 0 6 0xc70528 Right'
  , cell 0 8 0xc72e10 Ok
  , cell 0 13 0xc76650 Back
  , cell 0 49 0xc7a514 RouterTail
  ]
 where
  cell s e target = CellSpec keyController (State s) (Event e) (ActionAddress target)

-- | The non-per-press census dispatch set that marks a picker key: the seven
-- controller-75 key actions, Status 0xca4488, BackGate 0xc59ea4 and BackSend
-- 0xc2f354.
pickerDispatches :: S.Set ActionAddress
pickerDispatches =
  S.fromList (map specTarget contractCells ++ map ActionAddress [0xca4488, 0xc59ea4, 0xc2f354])

-- * Inputs

parseHexWord :: String -> Maybe Word32
parseHexWord text = case text of
  '0' : 'x' : digits | not (null digits), length (dropWhile (== '0') digits) <= 8, all isHexDigit digits, [(value, "")] <- readHex digits -> Just (fromInteger value)
  _ -> Nothing

hexField :: J.Value -> Parser Word32
hexField value = do
  text <- parseJSON value
  maybe (fail ("hex word required: " ++ text)) pure (parseHexWord text)

hexBytes :: String -> Parser B.ByteString
hexBytes text = B.pack <$> go text
 where
  go (a : b : rest) | [(value, "")] <- readHex [a, b] = (fromInteger value :) <$> go rest
  go [] = pure []
  go _ = fail "raw_hex is not an even run of hex digits"

data TableNode = TableNode
  { tableAddress :: NodeAddress
  , tableRaw :: Maybe B.ByteString
  , tablePropertyOrClass :: Maybe Integer
  }

instance FromJSON TableNode where
  parseJSON = J.withObject "node" $ \o -> do
    address <- hexField =<< o .: "address"
    raw <- traverse hexBytes =<< o .:? "raw_hex"
    property <- o .:? "property_or_class"
    pure (TableNode (NodeAddress address) raw property)

newtype MenuTables = MenuTables {menuNodes :: [TableNode]}

instance FromJSON MenuTables where
  parseJSON = J.withObject "menu tables" $ \o -> MenuTables <$> o .: "nodes"

data Dispatch = Dispatch {dispatchAction :: ActionAddress, dispatchPerPress :: Bool}

instance FromJSON Dispatch where
  parseJSON = J.withObject "dispatch" $ \o -> Dispatch <$> (ActionAddress <$> (hexField =<< o .: "action")) <*> o .: "per_press"

data CensusKey = CensusKey
  { keyMenu :: Integer
  , keySetting :: Integer
  , keyNodes :: [NodeAddress]
  , keyDispatches :: [Dispatch]
  }

instance FromJSON CensusKey where
  parseJSON = J.withObject "census key" $ \o -> do
    (menu, setting) <- J.withObject "key" (\k -> (,) <$> k .: "menu_id" <*> k .: "setting_id") =<< o .: "key"
    nodes <- mapM (J.withObject "stock node" (\n -> NodeAddress <$> (hexField =<< n .: "address"))) =<< o .:? "stock_nodes" .!= []
    dispatches <- o .:? "dispatches" .!= []
    pure (CensusKey menu setting nodes dispatches)

newtype Census = Census [CensusKey]

instance FromJSON Census where
  parseJSON = J.withObject "census" $ \o -> do
    menus <- o .: "menus"
    Census <$> J.withObject "menus" (.: "keys") menus

data ManifestCell = ManifestCell {manifestState :: Word16, manifestEvent :: Word16, manifestSlot :: Word32, manifestWord :: Word32}

instance FromJSON ManifestCell where
  parseJSON = J.withObject "decoded cell" $ \o ->
    ManifestCell <$> o .: "state_coordinate" <*> o .: "event_coordinate" <*> o .: "action_slot" <*> (hexField =<< o .: "word")

data ManifestTable = CellsTable Word32 [ManifestCell] | ActionsTable Word32 [Word32] | OtherTable

instance FromJSON ManifestTable where
  parseJSON = J.withObject "table" $ \o -> do
    kind <- o .: "kind"
    case kind :: String of
      "Cells" -> CellsTable <$> (hexField =<< o .: "start") <*> o .: "decoded_cells"
      "Actions" -> ActionsTable <$> (hexField =<< o .: "start") <*> (mapM hexField =<< o .: "words")
      _ -> pure OtherTable

data ManifestController = ManifestController
  { manifestController :: Controller
  , manifestWidth :: Word16
  , manifestCells :: [(Word32, [ManifestCell])]
  , manifestActions :: [(Word32, [Word32])]
  }

instance FromJSON ManifestController where
  parseJSON = J.withObject "controller" $ \o -> do
    identity <- o .: "id"
    width <- o .: "width"
    tables <- o .: "tables"
    pure (ManifestController (Controller identity) width [(s, c) | CellsTable s c <- tables] [(s, w) | ActionsTable s w <- tables])

newtype Manifest = Manifest [ManifestController]

instance FromJSON Manifest where
  parseJSON = J.withObject "manifest" $ \o -> Manifest <$> o .: "controllers"

-- * Generation

data Inputs = Inputs
  { inputImage5 :: Image
  , inputImage4 :: Image
  , inputTables :: MenuTables
  , inputCensus :: Census
  , inputManifest :: Manifest
  }

data Generated = Generated
  { generatedNodes :: [PickerNode]
  , generatedCells :: [PickerCell]
  }

excludedSetting :: Integer
excludedSetting = 254

excludedNode :: NodeAddress
excludedNode = NodeAddress 0x7823a4

pickerMenu :: Integer
pickerMenu = 1

nodeRecordBytes :: Int
nodeRecordBytes = 44

generate :: [CellSpec] -> Inputs -> Either String Generated
generate specs inputs = do
  case [spec | spec <- specs, specController spec /= keyController] of
    [] -> Right ()
    CellSpec (Controller c) (State s) (Event e) _ _ : _ -> Left ("cell " ++ show c ++ "/" ++ show s ++ "/" ++ show e ++ ": controller is not 75")
  let Census keys = inputCensus inputs
      selected =
        [ key
        | key <- keys
        , keyMenu key == pickerMenu
        , S.fromList [dispatchAction d | d <- keyDispatches key, not (dispatchPerPress d)] == pickerDispatches
        , keySetting key /= excludedSetting
        , excludedNode `notElem` keyNodes key
        ]
  nodes <- sortOn pickerProperty <$> mapM (pickerNodeOf inputs) selected
  let properties = map pickerProperty nodes
  when (S.size (S.fromList properties) /= length properties) (Left "two picker nodes share a property")
  cells <- mapM (resolveCell (inputImage5 inputs) (inputManifest inputs)) specs
  pure (Generated nodes cells)

pickerNodeOf :: Inputs -> CensusKey -> Either String PickerNode
pickerNodeOf inputs key = do
  let at = "setting " ++ show (keySetting key)
  address <- case keyNodes key of
    [address] -> Right address
    other -> Left (at ++ ": " ++ show (length other) ++ " stock nodes, one required")
  let NodeAddress raw = address
  node <- case [n | n <- menuNodes (inputTables inputs), tableAddress n == address] of
    [n] -> Right n
    _ -> Left (at ++ ": node " ++ hex raw ++ " not once in menu tables")
  record <- case tableRaw node of
    Just bytes | B.length bytes == nodeRecordBytes -> Right bytes
    _ -> Left (at ++ ": node " ++ hex raw ++ " has no 44-byte record")
  let word2 = littleEndian (B.take 4 (B.drop 8 record))
  unless (tablePropertyOrClass node == Just (toInteger word2)) (Left (at ++ ": property_or_class differs from node word 2"))
  unless (toInteger word2 == keySetting key) (Left (at ++ ": node word 2 " ++ show word2 ++ " differs from the setting id"))
  when (word2 > 0xffff) (Left (at ++ ": property outside u16"))
  case bytesIn (inputImage4 inputs) raw nodeRecordBytes of
    Just stock | stock == record -> Right ()
    Just _ -> Left (at ++ ": node record differs from image4 at " ++ hex raw)
    Nothing -> Left (at ++ ": node record outside image4 at " ++ hex raw)
  pure (PickerNode address (PropertyId (fromIntegral word2)))

cellPointers, actionPointers, widthTable :: Word32
cellPointers = 0xcf58b8
actionPointers = 0xcfe2a8
widthTable = 0xcd5e78

resolveCell :: Image -> Manifest -> CellSpec -> Either String PickerCell
resolveCell image (Manifest controllers) spec@(CellSpec (Controller c) (State s) (Event e) (ActionAddress expected) _) = do
  let at = "cell " ++ show c ++ "/" ++ show s ++ "/" ++ show e
      c32 = fromIntegral c :: Word32
  controller <- case [m | m <- controllers, manifestController m == Controller c] of
    [m] -> Right m
    _ -> Left (at ++ ": controller not once in the manifest")
  (cellsStart, decoded) <- case manifestCells controller of
    [table] -> Right table
    _ -> Left (at ++ ": manifest has no single Cells table")
  (actionsStart, actionWords) <- case manifestActions controller of
    [table] -> Right table
    _ -> Left (at ++ ": manifest has no single Actions table")
  manifestCell <- case [m | m <- decoded, manifestState m == s, manifestEvent m == e] of
    [m] -> Right m
    _ -> Left (at ++ ": not once in the manifest's decoded cells")
  unless (manifestSlot manifestCell == manifestWord manifestCell .&. 0xffff) (Left (at ++ ": manifest action slot differs from its cell word"))
  manifestTarget <- case drop (fromIntegral (manifestSlot manifestCell)) actionWords of
    target : _ -> Right target
    [] -> Left (at ++ ": manifest action slot outside its Actions words")
  let width = manifestWidth controller
  unless (s < width) (Left (at ++ ": state outside the controller width"))
  let inImage5 :: String -> Maybe a -> Either String a
      inImage5 what = maybe (Left (at ++ ": " ++ what ++ " outside image5")) Right
  stockWidth <- inImage5 "width" (halfIn image (widthTable + 2 * c32))
  unless (stockWidth == width) (Left (at ++ ": image5 width differs from the manifest"))
  cellsPointer <- inImage5 "Cells pointer" (wordIn image (cellPointers + 4 * c32))
  actionsPointer <- inImage5 "Actions pointer" (wordIn image (actionPointers + 4 * c32))
  unless (cellsPointer == cellsStart) (Left (at ++ ": image5 Cells pointer differs from the manifest"))
  unless (actionsPointer == actionsStart) (Left (at ++ ": image5 Actions pointer differs from the manifest"))
  let index = fromIntegral e * fromIntegral width + fromIntegral s :: Word32
  word <- inImage5 "cell word" (wordIn image (cellsPointer + 4 * index))
  unless (word == manifestWord manifestCell) (Left (at ++ ": image5 cell word " ++ hex word ++ " differs from the manifest"))
  target <- inImage5 "action target" (wordIn image (actionsPointer + 4 * (word .&. 0xffff)))
  unless (target == manifestTarget) (Left (at ++ ": image5 action target " ++ hex target ++ " differs from the manifest"))
  unless (target == expected) (Left (at ++ ": stock action target " ++ hex target ++ " differs from the contract's " ++ hex expected))
  pure (PickerCell spec (CellWord word) (ActionAddress target))

-- * Output

rustTable :: [String] -> Generated -> String
rustTable header generated = unlines
  ( map ("// " ++) header
    ++ ["PickerTable {", "    nodes: &["]
    ++ [ "        PickerNode { node: NodeAddress(" ++ hex address ++ "), property: PropertyId(" ++ show p ++ ") },"
       | PickerNode (NodeAddress address) (PropertyId p) <- generatedNodes generated ]
    ++ ["    ],", "    cells: &["]
    ++ [ "        PickerCell { controller: " ++ show c ++ ", state: " ++ show s ++ ", event: " ++ show e
           ++ ", word: " ++ hex word ++ ", target: " ++ hex target ++ ", action: PickerAction::" ++ rustAction (specAction spec) ++ " },"
       | PickerCell spec (CellWord word) (ActionAddress target) <- generatedCells generated
       , let Controller c = specController spec; State s = specState spec; Event e = specEvent spec ]
    ++ ["    ],", "}"] )

reportJson :: [(String, String)] -> Generated -> J.Value
reportJson digests generated = J.object
  [ "schema" .= (2 :: Int)
  , "inputs_sha256" .= M.fromList digests
  , "node_selection" .= J.object
      [ "menu" .= pickerMenu
      , "non_per_press_dispatches" .= [hex a | ActionAddress a <- S.toList pickerDispatches]
      , "excluded_setting" .= excludedSetting
      , "excluded_node" .= let NodeAddress a = excludedNode in hex a
      ]
  , "nodes" .= [ J.object ["node" .= hex address, "property" .= p, "image4_record" .= ("equal" :: String)]
               | PickerNode (NodeAddress address) (PropertyId p) <- generatedNodes generated ]
  , "cells" .= [ J.object
                  [ "controller" .= c, "state" .= s, "event" .= e
                  , "word" .= hex word, "target" .= hex target
                  , "action" .= rustAction (specAction spec)
                  , "manifest" .= ("equal" :: String)
                  ]
               | PickerCell spec (CellWord word) (ActionAddress target) <- generatedCells generated
               , let Controller c = specController spec; State s = specState spec; Event e = specEvent spec ]
  ]
