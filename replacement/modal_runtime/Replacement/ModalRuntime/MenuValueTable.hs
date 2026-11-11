{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | The owned value transaction's tables, as Rust expressions, from the pinned
-- image-4 bytes and the menu-tables document of
-- "Replacement.ModalRuntime.MenuTables". Every row is checked against the
-- existing decoder; the IMAGE SIZE pairs and the intended token 1/2 values are
-- checked against the image.
module Replacement.ModalRuntime.MenuValueTable
  ( ValueDocument (..)
  , PropertyTable (..)
  , image4Pin
  , tablesPin
  , pairTableAddress
  , imageSizeTokens
  , sizeProperty
  , aspectProperty
  , changeMarkerProperty
  , Generated (..)
  , generateTables
  , generate
  , render
  ) where

import Control.Monad (forM_, unless)
import Data.Aeson ((.:))
import qualified Data.Aeson as J
import Data.Char (toLower)
import Data.List (intercalate, nubBy)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Numeric (showHex)
import Replacement.ModalRuntime.MenuTables

image4Pin :: String
image4Pin = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"

tablesPin :: String
tablesPin = "7fa536b6f4ab72205be57f407ce7224fa0bbc397aca2061ad2fac3e14ba2021d"

pairTableAddress :: Word32
pairTableAddress = 0x007a00cc

-- | The tokens every IMAGE SIZE list offers.
imageSizeTokens :: [Word32]
imageSizeTokens = [1, 2, 3, 5, 6, 7, 9, 10, 11]

sizeProperty, aspectProperty, changeMarkerProperty :: Word32
sizeProperty = 0xf6
aspectProperty = 0xf9
changeMarkerProperty = 0x1e5

data PropertyTable = PropertyTable
  { tableProperty :: Word32
  , tableRows :: [ConversionRow]
  }
  deriving (Eq, Show)

data Generated = Generated
  { generatedConversions :: [(Word32, [ConversionRow])]
  , generatedPairs :: [(Word32, Word32, Word32)]
  }
  deriving (Eq, Show)

data ValueDocument = ValueDocument
  { documentImage4 :: String
  , documentProperties :: [PropertyTable]
  , documentNullProperties :: [Word32]
  }
  deriving (Eq, Show)

instance J.FromJSON ValueDocument where
  parseJSON = J.withObject "menu tables" $ \fields -> do
    schema <- fields .: "schema"
    unless (schema == (1 :: Int)) (fail "menu tables schema is not 1")
    inputs <- fields .: "inputs"
    ValueDocument
      <$> inputs .: "image4_sha256"
      <*> (mapM propertyTable =<< fields .: "properties")
      <*> (mapM nullProperty =<< fields .: "properties_without_table")
   where
    propertyTable = J.withObject "property" $ \property ->
      PropertyTable <$> property .: "property" <*> (mapM row =<< property .: "rows")
    row = J.withObject "row" $ \value ->
      ConversionRow <$> value .: "stored" <*> value .: "col1" <*> value .: "effective"
    nullProperty = J.withObject "property" $ \property -> property .: "property"

hex :: Word32 -> String
hex value = "0x" ++ showHex value ""

rowText :: ConversionRow -> String
rowText row = "crate::menu_value::ConversionRow { stored: crate::menu_value::ChoiceCode::new("
  ++ hex (conversionStored row) ++ "), column1: crate::menu_value::ChoiceCode::new("
  ++ hex (conversionColumn1 row) ++ "), effective: crate::menu_value::ChoiceCode::new("
  ++ hex (conversionEffective row) ++ ") }"

pairText :: (Word32, Word32, Word32) -> String
pairText (token, size, aspect) = "    (crate::menu_value::RowToken::new(" ++ hex token
  ++ "), crate::menu_value::ImageSizePair { size: crate::menu_value::SizeCode::new(" ++ hex size
  ++ "), aspect: crate::menu_value::AspectCode::new(" ++ hex aspect ++ ") }),"

-- | The stored code a conversion table's effective column maps to.
effectiveToStored :: [ConversionRow] -> Word32 -> Maybe Word32
effectiveToStored rows key =
  case [conversionStored row | row <- rows, conversionEffective row == key] of
    stored : _ -> Just stored
    [] -> Nothing

decodePairs :: Image -> Either String [(Word32, Word32, Word32)]
decodePairs image = mapM pair [0 .. 11]
 where
  pair token = case (imageWord image (Address (pairTableAddress + 16 * token)), imageWord image (Address (pairTableAddress + 16 * token + 4))) of
    (Just size, Just aspect) -> Right (token, size, aspect)
    _ -> Left ("IMAGE SIZE pair " ++ show token ++ " is outside the image")

generateTables :: Image -> ValueDocument -> Either String Generated
generateTables image document = do
  unless (documentImage4 document == image4Pin) (Left "menu tables name another image 4")
  let properties = documentProperties document
  forM_ properties $ \table -> do
    let PropertyTable number rows = table
    case conversionTable image (PropertyId number) of
      Table _ (Walked decoded)
        | decoded == rows -> Right ()
        | otherwise -> Left ("property " ++ show number ++ " rows differ from the image-4 decode")
      other -> Left ("property " ++ show number ++ " does not decode from image 4: " ++ show other)
  forM_ (documentNullProperties document) $ \number ->
    unless (conversionTable image (PropertyId number) == NullTablePointer) $
      Left ("property " ++ show number ++ " is not a null pointer in image 4")
  aspectRows <- requiredTable (PropertyId aspectProperty)
  markerRows <- requiredTable (PropertyId changeMarkerProperty)
  pairs <- decodePairs image
  sizeTable <- maybe (Left "the size property has no conversion table") Right (lookup sizeProperty [(number, rows) | PropertyTable number rows <- properties])
  let pairOf token = lookup token [(t, (size, aspect)) | (t, size, aspect) <- pairs]
  forM_ [(1, 0, 1), (2, 0, 2)] $ \(token, size, aspect) ->
    unless (pairOf token == Just (size, aspect)) $
      Left ("IMAGE SIZE token " ++ show token ++ " is not the intended pair " ++ show (size, aspect))
  unless (effectiveToStored sizeTable 0 == Just 0 && effectiveToStored sizeTable 1 == Just 1 && effectiveToStored sizeTable 2 == Just 2) $
    Left "the size conversion is not the intended 0, 1, 2 identity"
  unless (effectiveToStored aspectRows 1 == Just 1 && effectiveToStored aspectRows 2 == Just 2) $
    Left "the aspect conversion is not the intended 1, 2 identity"
  let tables = M.fromList
        ([(tableProperty table, tableRows table) | table <- properties]
          ++ [(aspectProperty, aspectRows), (changeMarkerProperty, markerRows)])
  pure (Generated (M.toAscList tables) pairs)
 where
  requiredTable property@(PropertyId number) = case conversionTable image property of
    Table _ (Walked rows) -> Right rows
    other -> Left ("property " ++ show number ++ " has no conversion table: " ++ show other)

generate :: String -> Image -> ValueDocument -> Either String String
generate tablesSha image document = render tablesSha <$> generateTables image document

render :: String -> Generated -> String
render tablesSha (Generated conversions pairs) = unlines
  ( [ "// Generated by //replacement/modal_runtime:menu-value-table from menu tables sha256 " ++ map toLower tablesSha
    , "// (image 4 sha256 " ++ image4Pin ++ "); not for redistribution."
    ]
    ++ concat
       [ [ "#[link_section = \".menu_engine.rodata.value_tables\"]"
         , "static CONVERSION_ROWS_" ++ show number ++ ": [crate::menu_value::ConversionRow; " ++ show (length rows) ++ "] = ["
         , "    " ++ intercalate ", " (map rowText rows)
         , "];"
         ]
       | (number, rows) <- uniqueRows
       ]
    ++ [ "#[link_section = \".menu_engine.rodata.value_tables\"]"
       , "static CONVERSIONS: [(crate::menu_value::PropertyId, &[crate::menu_value::ConversionRow]); " ++ show (length conversions) ++ "] = ["
       ]
    ++ [ "    (crate::menu_value::PropertyId::new(" ++ hex number ++ "), &CONVERSION_ROWS_" ++ show shared ++ "),"
       | (number, rows) <- conversions
       , (shared, sharedRows) <- uniqueRows
       , rows == sharedRows
       ]
    ++ [ "];"
       , "#[link_section = \".menu_engine.rodata.value_tables\"]"
       , "static IMAGE_SIZES: [(crate::menu_value::RowToken, crate::menu_value::ImageSizePair); " ++ show (length pairs) ++ "] = ["
       ]
    ++ map pairText pairs
    ++ [ "];"
       , "#[used]"
       , "#[link_section = \".menu_engine.rodata.value_tables\"]"
       , "pub static TABLES: crate::menu_value::ValueTables = crate::menu_value::ValueTables::new("
       , "    &CONVERSIONS,"
       , "    &IMAGE_SIZES,"
       , "    Some(crate::menu_value::ImageSizeRule {"
       , "        size_property: crate::menu_value::PropertyId::new(" ++ hex sizeProperty ++ "),"
       , "        aspect_property: crate::menu_value::PropertyId::new(" ++ hex aspectProperty ++ "),"
       , "        change_property: crate::menu_value::PropertyId::new(0x161),"
       , "        change_property_1f: crate::menu_value::PropertyId::new(0x165),"
       , "        marker_property: crate::menu_value::PropertyId::new(0x1e5),"
       , "    }),"
       , ");"
       ] )
 where
  uniqueRows = nubBy (\(_, left) (_, right) -> left == right) conversions
