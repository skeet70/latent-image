{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | The owned menu engine's 'Tree' and record side table, as Rust expressions, from the menu
-- tables JSON of "Replacement.ModalRuntime.MenuTables".
module Replacement.ModalRuntime.MenuEngineTree
  ( TablesDocument (..)
  , RegistryEntry (..)
  , TableNode (..)
  , TableRow (..)
  , TableLabel (..)
  , Generated (..)
  , image4Pin
  , stringsPin
  , generate
  ) where

import Control.Monad (forM, unless, when)
import Data.Aeson ((.:), (.:?))
import qualified Data.Aeson as J
import qualified Data.Aeson.Types as J
import Data.Char (toLower)
import Data.List (intercalate)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Numeric (readHex, showHex)

image4Pin :: String
image4Pin = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"

stringsPin :: String
stringsPin = "68ea4d03b50267f0adbb03f37662d81ae0b4cc41658c4a8663b2209e75ce3e63"

newtype Address = Address Word32 deriving (Eq, Ord, Show)

data RegistryEntry = RegistryEntry {registrySlot :: Word32, registryAddress :: Maybe Address} deriving (Eq, Show)

data TableRow = TableRow {rowToken :: Word32, rowChild :: Maybe Address} deriving (Eq, Show)

data TableLabel = TableLabel {labelPreview :: Word32, labelList :: Word32, labelHelp :: Word32, labelName :: Word32}
  deriving (Eq, Show)

data TableNode = TableNode
  { nodeAddress :: Address
  , nodeId :: Word32
  , nodeType :: Word32
  , nodeClass :: Word32
  , nodeRowsPerPage :: Word32
  , nodeNoWrap :: Word32
  , nodeNoAvailability :: Word32
  , nodeRemember :: Word32
  , nodeParent :: Maybe Address
  , nodeWord10 :: Word32
  , nodeRows :: Maybe [TableRow]
  , nodeLabels :: Maybe [TableLabel]
  }
  deriving (Eq, Show)

data TablesDocument = TablesDocument
  { documentImage4 :: String
  , documentStrings :: String
  , documentRegistry :: [RegistryEntry]
  , documentNodes :: [TableNode]
  }
  deriving (Eq, Show)

hexWord :: String -> J.Parser Word32
hexWord text = case text of
  '0' : 'x' : digits | [(value, "")] <- readHex digits -> pure value
  _ -> fail ("not a hex word: " ++ text)

address :: String -> J.Parser Address
address text = Address <$> hexWord text

-- | A walk is a list, null, or an object naming why it is unresolved; the last refuses.
walkOf :: (J.Value -> J.Parser a) -> J.Value -> J.Parser (Maybe [a])
walkOf item value = case value of
  J.Null -> pure Nothing
  J.Array _ -> Just <$> (mapM item =<< J.parseJSON value)
  _ -> fail "unresolved walk"

instance J.FromJSON TablesDocument where
  parseJSON = J.withObject "menu tables" $ \fields -> do
    schema <- fields .: "schema"
    unless (schema == (1 :: Int)) (fail "menu tables schema is not 1")
    inputs <- fields .: "inputs"
    TablesDocument
      <$> inputs .: "image4_sha256"
      <*> inputs .: "strings_sha256"
      <*> (mapM registryEntry =<< fields .: "registry")
      <*> (mapM tableNode =<< fields .: "nodes")
   where
    registryEntry = J.withObject "registry slot" $ \slot -> do
      unresolved <- slot .:? "unresolved"
      when (unresolved /= (Nothing :: Maybe String)) (fail "unreadable registry slot")
      RegistryEntry <$> slot .: "id" <*> (traverse address =<< slot .: "address")
    tableNode = J.withObject "node" $ \node -> do
      at <- address =<< node .: "address"
      unresolved <- node .:? "unresolved"
      when (unresolved /= (Nothing :: Maybe String)) (fail ("unresolved node " ++ show at))
      TableNode at
        <$> node .: "id"
        <*> node .: "type"
        <*> node .: "property_or_class"
        <*> node .: "kind"
        <*> node .: "word4"
        <*> node .: "word5"
        <*> node .: "flag"
        <*> (traverse address =<< node .: "parent")
        <*> node .: "word10"
        <*> (walkOf tableRow =<< node .: "rows")
        <*> (walkOf tableLabel =<< node .: "labels")
    tableRow = J.withObject "row" $ \row -> TableRow <$> row .: "row" <*> (traverse address =<< row .: "child")
    tableLabel = J.withObject "label" $ \label -> do
      let string key = (hexWord =<<) . (.: "id") =<< label .: key
      TableLabel <$> string "preview_icon" <*> string "list_icon" <*> string "help" <*> string "name"

-- | The tree text carries its identity; the records text names the same identity.
data Generated = Generated {generatedTree :: String, generatedRecords :: String, generatedIdentity :: String}
  deriving (Eq, Show)

hex :: Word32 -> String
hex value = "0x" ++ showHex value ""

identityBytes :: String -> String
identityBytes identity = "TreeIdentity([" ++ intercalate ", " ["0x" ++ pair | pair <- pairs identity] ++ "])"
 where
  pairs (high : low : more) = [high, low] : pairs more
  pairs _ = []

flag :: String -> String -> String -> Word32 -> Either String String
flag what clear set value = case value of
  0 -> Right clear
  1 -> Right set
  _ -> Left (what ++ " is neither 0 nor 1: " ++ show value)

registrySlots :: Word32
registrySlots = 0x86

-- | The first argument hashes the tree body; the identity is its lowercase hex SHA-256.
generate :: (String -> String) -> String -> TablesDocument -> Either String Generated
generate digest tablesSha document = do
  unless (documentImage4 document == image4Pin) (Left "menu tables name another image 4")
  unless (documentStrings document == stringsPin) (Left "menu tables name another strings slot")
  unless (map registrySlot (documentRegistry document) == [0 .. registrySlots - 1]) (Left "registry is not slots 0..0x85 in order")
  let nodes = documentNodes document
      refs = M.fromList (zip (map nodeAddress nodes) [0 :: Int ..])
      refOf what (Address at) = maybe (Left (what ++ " " ++ hex at ++ " is not a decoded node")) Right (M.lookup (Address at) refs)
  when (M.size refs /= length nodes) (Left "duplicate node address")
  when (length nodes > 0xffff) (Left "more nodes than NodeRef holds")
  nodeLines <- forM nodes $ \node -> do
    let Address at = nodeAddress node
    when (nodeRowsPerPage node == 0) (Left ("node " ++ hex at ++ " has zero rows per page"))
    when (nodeWord10 node > 0xffff) (Left ("node " ++ hex at ++ " word 10 exceeds a halfword"))
    wrap <- flag "word4" "Wrap::Wraps" "Wrap::Stops" (nodeNoWrap node)
    availability <- flag "word5" "Availability::Queried" "Availability::Fixed" (nodeNoAvailability node)
    remember <- flag "flag" "Remember::Off" "Remember::On" (nodeRemember node)
    parent <- traverse (refOf "parent") (nodeParent node)
    rows <- traverse (mapM (\row -> (,) (rowToken row) <$> traverse (refOf "row child") (rowChild row))) (nodeRows node)
    let some = maybe "None" (\items -> "Some(&[" ++ intercalate ", " items ++ "])")
        rowText (token, child) = "Row { token: RowToken::new(" ++ hex token ++ "), child: " ++ maybe "None" (\c -> "Some(NodeRef::new(" ++ show c ++ "))") child ++ " }"
        labelText label =
          "Label { preview_icon: StringId::new(" ++ hex (labelPreview label) ++ "), list_icon: StringId::new(" ++ hex (labelList label)
            ++ "), help: StringId::new(" ++ hex (labelHelp label) ++ "), name: StringId::new(" ++ hex (labelName label) ++ ") }"
    pure $
      "        Node { id: NodeId::new(" ++ hex (nodeId node) ++ "), node_type: NodeType::new(" ++ hex (nodeType node)
        ++ "), class: MenuClass::new(" ++ hex (nodeClass node) ++ "), rows_per_page: RowsPerPage::new(" ++ hex (nodeRowsPerPage node)
        ++ "), wrap: " ++ wrap ++ ", availability: " ++ availability ++ ", remember: " ++ remember
        ++ ", parent: " ++ maybe "None" (\p -> "Some(NodeRef::new(" ++ show p ++ "))") parent
        ++ ", word10: NodeWord10::new(" ++ hex (nodeWord10 node) ++ "), rows: " ++ some (map rowText <$> rows)
        ++ ", labels: " ++ some (map labelText <$> nodeLabels node) ++ " }, // " ++ hex at
  registryLines <- forM (documentRegistry document) $ \entry -> case registryAddress entry of
    Nothing -> Right ("        None, // " ++ hex (registrySlot entry))
    Just at -> (\ref -> "        Some(NodeRef::new(" ++ show ref ++ ")), // " ++ hex (registrySlot entry)) <$> refOf "registry slot" at
  let header =
        [ "// Generated by //replacement/modal_runtime:menu-engine-tree from menu tables sha256 " ++ map toLower tablesSha
        , "// (image 4 sha256 " ++ image4Pin ++ "); not for redistribution." ]
      body = unlines (header ++ ["Tree {", "    nodes: &["] ++ nodeLines ++ ["    ],", "    registry: ["] ++ registryLines ++ ["    ],", "}"])
      identity = digest body
      tree = case reverse (lines body) of
        closing : rest -> unlines (reverse rest ++ ["    identity: " ++ identityBytes identity ++ ",", closing])
        [] -> body
      records = unlines
        ( header
            ++ ["RecordTable {", "    tree: " ++ identityBytes identity ++ ",", "    records: &["]
            ++ ["        RecordAddress::new(" ++ hex at ++ "), // " ++ show index | (index, TableNode {nodeAddress = Address at}) <- zip [0 :: Int ..] nodes]
            ++ ["    ],", "}"] )
  pure (Generated tree records identity)
