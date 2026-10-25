-- The shareable text recipe format for the editable custom bank (issue #76):
-- one file per recipe, the camera's field names as keys, parsed at this
-- boundary into closed value types and mapped to the bank's stored codes.
-- Host-only; no camera, no container access.
--
-- The stored codes are the col0 words of the values' rows in the pinned 1.50
-- conversion tables (the addresses FujiAF.RecipeBankFields carries), byte-read
-- from the pinned image-4 decode f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16.
-- The rows reports/recipe-patch-d3.md section 6.1 and reports/recipe-design.md
-- section 2 pin carry the same codes.
module FujiAF.RecipeText
  ( -- * Recipe values
    FilmValue (..)
  , DynamicRange (..)
  , Tone (..)
  , WhiteBalance (..)
  , IsoValue (..)
  , RecipeFields (..)
  , TextRecipe (..)
    -- * Text format
  , parseRecipeText
  , renderRecipeText
    -- * Bank mapping
  , fillRecipe
  , exportRecipeText
  , preparedBase
  , safePayload
  ) where

import Control.Monad (foldM, forM, unless, when)
import Data.Char (isSpace, toLower)
import Data.List (dropWhileEnd, intercalate)
import Data.Maybe (isJust, isNothing)
import qualified Data.ByteString as B
import Data.Word (Word16, Word8)

import FujiAF.RecipeBank
  ( Recipe
  , SlotName
  , StoredCode (..)
  , BankPayload
  , bankPayloadContents
  , canonicalBankPayload
  , encodeSlotPayloadOver
  , payloadBytes
  , payloadUnsafeReason
  , PayloadDomain
  , withImageSizeRow
  , recipeFromPairs
  , recipePairs
  , slotName
  , slotNameText
  , slotRecipe
  )

-- values ---------------------------------------------------------------

-- | The film simulations the bank's FILM SIMULATION list offers.
data FilmValue
  = ProviaStandard
  | VelviaVivid
  | AstiaSoft
  | BlackAndWhite
  | Sepia
  | ProNegStd
  | ProNegHi
  deriving (Eq, Ord, Show, Enum, Bounded)

-- | The DYNAMIC RANGE values the bank offers.
data DynamicRange = RangeAuto | Range100 | Range200 | Range400
  deriving (Eq, Ord, Show, Enum, Bounded)

-- | A signed tone step, the -2..+2 side the menus show.
data Tone = ToneMinus2 | ToneMinus1 | Tone0 | TonePlus1 | TonePlus2
  deriving (Eq, Ord, Show, Enum, Bounded)

-- | The WHITE BALANCE modes the bank offers.
data WhiteBalance
  = BalanceAuto
  | BalanceCustom
  | BalanceFine
  | BalanceShade
  | BalanceFluorescent1
  | BalanceFluorescent2
  | BalanceFluorescent3
  | BalanceIncandescent
  deriving (Eq, Ord, Show, Enum, Bounded)

-- | The ISO values the bank's edit list offers. The list's second AUTO row
-- (stored code 0x1b, which opens the ISO AUTO CONTROL submenu) and that
-- submenu's rows (stored codes 1-6) are not modelled, the same boundary
-- schemas/recipe-set.schema.json draws.
data IsoValue
  = IsoAuto
  | IsoL100
  | Iso200
  | Iso250
  | Iso320
  | Iso400
  | Iso500
  | Iso640
  | Iso800
  | Iso1000
  | Iso1250
  | Iso1600
  | Iso2000
  | Iso2500
  | Iso3200
  | Iso4000
  | Iso5000
  | Iso6400
  | IsoH12800
  | IsoH25600
  deriving (Eq, Ord, Show, Enum, Bounded)

-- | The recipe fields a text file spells; a field it omits takes its stored
-- code from the declared payload base.
data RecipeFields = RecipeFields
  { fieldFilm :: Maybe FilmValue
  , fieldRange :: Maybe DynamicRange
  , fieldHighlight :: Maybe Tone
  , fieldShadow :: Maybe Tone
  , fieldColour :: Maybe Tone
  , fieldSharpness :: Maybe Tone
  , fieldNoiseReduction :: Maybe Tone
  , fieldWhiteBalance :: Maybe WhiteBalance
  , fieldIso :: Maybe IsoValue
  }
  deriving (Eq, Show)

emptyFields :: RecipeFields
emptyFields = RecipeFields Nothing Nothing Nothing Nothing Nothing Nothing Nothing Nothing Nothing

-- | One parsed recipe: the slot name the file declares (or the caller's
-- default) and the fields it spells.
data TextRecipe = TextRecipe
  { textName :: SlotName
  , textFields :: RecipeFields
  }
  deriving (Eq, Show)

-- keys -----------------------------------------------------------------

-- | The camera's field names, as the format writes them.
nameKey, filmKey, rangeKey, highlightKey, shadowKey, colourKey, sharpnessKey, noiseKey, balanceKey, isoKey :: String
nameKey = "Name"
filmKey = "Film Simulation"
rangeKey = "Dynamic Range"
highlightKey = "Highlight Tone"
shadowKey = "Shadow Tone"
colourKey = "Color"
sharpnessKey = "Sharpness"
noiseKey = "Noise Reduction"
balanceKey = "White Balance"
isoKey = "ISO"

-- rendering ------------------------------------------------------------

renderFilm :: FilmValue -> String
renderFilm value = case value of
  ProviaStandard -> "PROVIA/STANDARD"
  VelviaVivid -> "Velvia/VIVID"
  AstiaSoft -> "ASTIA/SOFT"
  BlackAndWhite -> "B&W"
  Sepia -> "SEPIA"
  ProNegStd -> "PRO Neg. Std"
  ProNegHi -> "PRO Neg. Hi"

renderRange :: DynamicRange -> String
renderRange value = case value of
  RangeAuto -> "AUTO"
  Range100 -> "100"
  Range200 -> "200"
  Range400 -> "400"

renderTone :: Tone -> String
renderTone value = case value of
  ToneMinus2 -> "-2"
  ToneMinus1 -> "-1"
  Tone0 -> "0"
  TonePlus1 -> "+1"
  TonePlus2 -> "+2"

renderBalance :: WhiteBalance -> String
renderBalance value = case value of
  BalanceAuto -> "AUTO"
  BalanceCustom -> "CUSTOM"
  BalanceFine -> "FINE"
  BalanceShade -> "SHADE"
  BalanceFluorescent1 -> "FLUORESCENT LIGHT-1"
  BalanceFluorescent2 -> "FLUORESCENT LIGHT-2"
  BalanceFluorescent3 -> "FLUORESCENT LIGHT-3"
  BalanceIncandescent -> "INCANDESCENT"

renderIso :: IsoValue -> String
renderIso value = case value of
  IsoAuto -> "AUTO"
  IsoL100 -> "L100"
  Iso200 -> "200"
  Iso250 -> "250"
  Iso320 -> "320"
  Iso400 -> "400"
  Iso500 -> "500"
  Iso640 -> "640"
  Iso800 -> "800"
  Iso1000 -> "1000"
  Iso1250 -> "1250"
  Iso1600 -> "1600"
  Iso2000 -> "2000"
  Iso2500 -> "2500"
  Iso3200 -> "3200"
  Iso4000 -> "4000"
  Iso5000 -> "5000"
  Iso6400 -> "6400"
  IsoH12800 -> "H12800"
  IsoH25600 -> "H25600"

-- | The canonical text: the name line, then the fields the recipe spells in
-- the format's order.
renderRecipeText :: TextRecipe -> String
renderRecipeText recipe = unlines ((nameKey ++ ": " ++ slotNameText (textName recipe)) : rendered)
 where
  fields = textFields recipe
  rendered =
    concat
      [ one filmKey (renderFilm <$> fieldFilm fields)
      , one rangeKey (renderRange <$> fieldRange fields)
      , one highlightKey (renderTone <$> fieldHighlight fields)
      , one shadowKey (renderTone <$> fieldShadow fields)
      , one colourKey (renderTone <$> fieldColour fields)
      , one sharpnessKey (renderTone <$> fieldSharpness fields)
      , one noiseKey (renderTone <$> fieldNoiseReduction fields)
      , one balanceKey (renderBalance <$> fieldWhiteBalance fields)
      , one isoKey (renderIso <$> fieldIso fields)
      ]
  one label value = maybe [] (\text -> [label ++ ": " ++ text]) value

-- stored codes ---------------------------------------------------------

filmStored :: FilmValue -> StoredCode
filmStored value = StoredCode $ case value of
  ProviaStandard -> 0x00
  AstiaSoft -> 0x01
  VelviaVivid -> 0x03
  ProNegStd -> 0x09
  ProNegHi -> 0x0a
  BlackAndWhite -> 0x0b
  Sepia -> 0x0c

filmOfStored :: StoredCode -> Maybe FilmValue
filmOfStored (StoredCode code) = case code of
  0x00 -> Just ProviaStandard
  0x01 -> Just AstiaSoft
  0x03 -> Just VelviaVivid
  0x09 -> Just ProNegStd
  0x0a -> Just ProNegHi
  0x0b -> Just BlackAndWhite
  0x0c -> Just Sepia
  _ -> Nothing

rangeStored :: DynamicRange -> StoredCode
rangeStored value = StoredCode $ case value of
  RangeAuto -> 0
  Range100 -> 1
  Range200 -> 2
  Range400 -> 3

rangeOfStored :: StoredCode -> Maybe DynamicRange
rangeOfStored (StoredCode code) = case code of
  0 -> Just RangeAuto
  1 -> Just Range100
  2 -> Just Range200
  3 -> Just Range400
  _ -> Nothing

toneLevel :: Tone -> Int
toneLevel value = case value of
  ToneMinus2 -> -2
  ToneMinus1 -> -1
  Tone0 -> 0
  TonePlus1 -> 1
  TonePlus2 -> 2

toneOfLevel :: Int -> Tone
toneOfLevel level = case level of
  -2 -> ToneMinus2
  -1 -> ToneMinus1
  0 -> Tone0
  1 -> TonePlus1
  _ -> TonePlus2

-- | Highlight, shadow and sharpness: the menu order HARD..SOFT stores the
-- rows 0..4, so +2 stores 0.
hardnessStored :: Tone -> StoredCode
hardnessStored value = StoredCode (fromIntegral (2 - toneLevel value))

hardnessOfStored :: StoredCode -> Maybe Tone
hardnessOfStored (StoredCode code) = case code of
  0 -> Just TonePlus2
  1 -> Just TonePlus1
  2 -> Just Tone0
  3 -> Just ToneMinus1
  4 -> Just ToneMinus2
  _ -> Nothing

-- | Noise reduction: the node offers the rows {4,3,2,1,0} as HIGH..LOW and
-- each row stores its own index, so +2 stores 4.
levelStored :: Tone -> StoredCode
levelStored value = StoredCode (fromIntegral (2 + toneLevel value))

levelOfStored :: StoredCode -> Maybe Tone
levelOfStored (StoredCode code) = case code of
  0 -> Just ToneMinus2
  1 -> Just ToneMinus1
  2 -> Just Tone0
  3 -> Just TonePlus1
  4 -> Just TonePlus2
  _ -> Nothing

-- | Colour's rows {+2, +1, 0, -1, -2} store {3, 4, 0, 6, 5}
-- (reports/recipe-design.md section 2).
colourStored :: Tone -> StoredCode
colourStored value = StoredCode $ case value of
  TonePlus2 -> 3
  TonePlus1 -> 4
  Tone0 -> 0
  ToneMinus1 -> 6
  ToneMinus2 -> 5

colourOfStored :: StoredCode -> Maybe Tone
colourOfStored (StoredCode code) = case code of
  3 -> Just TonePlus2
  4 -> Just TonePlus1
  0 -> Just Tone0
  6 -> Just ToneMinus1
  5 -> Just ToneMinus2
  _ -> Nothing

-- | WHITE BALANCE's offered rows store {AUTO 0, CUSTOM 9, FINE 1, SHADE 2,
-- FLUORESCENT LIGHT-1..3 3..5, INCANDESCENT 6}.
balanceStored :: WhiteBalance -> StoredCode
balanceStored value = StoredCode $ case value of
  BalanceAuto -> 0x00
  BalanceCustom -> 0x09
  BalanceFine -> 0x01
  BalanceShade -> 0x02
  BalanceFluorescent1 -> 0x03
  BalanceFluorescent2 -> 0x04
  BalanceFluorescent3 -> 0x05
  BalanceIncandescent -> 0x06

balanceOfStored :: StoredCode -> Maybe WhiteBalance
balanceOfStored (StoredCode code) = case code of
  0x00 -> Just BalanceAuto
  0x09 -> Just BalanceCustom
  0x01 -> Just BalanceFine
  0x02 -> Just BalanceShade
  0x03 -> Just BalanceFluorescent1
  0x04 -> Just BalanceFluorescent2
  0x05 -> Just BalanceFluorescent3
  0x06 -> Just BalanceIncandescent
  _ -> Nothing

-- | ISO's offered rows store their own index; AUTO stores 0, L100 7 and the
-- numbered rows 8..23, then H12800 25 and H25600 26.
isoStored :: IsoValue -> StoredCode
isoStored value = StoredCode $ case value of
  IsoAuto -> 0x00
  IsoL100 -> 0x07
  Iso200 -> 0x08
  Iso250 -> 0x09
  Iso320 -> 0x0a
  Iso400 -> 0x0b
  Iso500 -> 0x0c
  Iso640 -> 0x0d
  Iso800 -> 0x0e
  Iso1000 -> 0x0f
  Iso1250 -> 0x10
  Iso1600 -> 0x11
  Iso2000 -> 0x12
  Iso2500 -> 0x13
  Iso3200 -> 0x14
  Iso4000 -> 0x15
  Iso5000 -> 0x16
  Iso6400 -> 0x17
  IsoH12800 -> 0x19
  IsoH25600 -> 0x1a

isoOfStored :: StoredCode -> Maybe IsoValue
isoOfStored (StoredCode code) = case code of
  0x00 -> Just IsoAuto
  0x07 -> Just IsoL100
  0x08 -> Just Iso200
  0x09 -> Just Iso250
  0x0a -> Just Iso320
  0x0b -> Just Iso400
  0x0c -> Just Iso500
  0x0d -> Just Iso640
  0x0e -> Just Iso800
  0x0f -> Just Iso1000
  0x10 -> Just Iso1250
  0x11 -> Just Iso1600
  0x12 -> Just Iso2000
  0x13 -> Just Iso2500
  0x14 -> Just Iso3200
  0x15 -> Just Iso4000
  0x16 -> Just Iso5000
  0x17 -> Just Iso6400
  0x19 -> Just IsoH12800
  0x1a -> Just IsoH25600
  _ -> Nothing

-- | The nine fields' stored codes a recipe spells, in the format's order.
textCodes :: RecipeFields -> [(Word16, String, Maybe StoredCode)]
textCodes fields =
  [ (0xfe, filmKey, filmStored <$> fieldFilm fields)
  , (0x104, rangeKey, rangeStored <$> fieldRange fields)
  , (0x110, highlightKey, hardnessStored <$> fieldHighlight fields)
  , (0x111, shadowKey, hardnessStored <$> fieldShadow fields)
  , (0x10e, colourKey, colourStored <$> fieldColour fields)
  , (0x10f, sharpnessKey, hardnessStored <$> fieldSharpness fields)
  , (0xef, noiseKey, levelStored <$> fieldNoiseReduction fields)
  , (0xed, balanceKey, balanceStored <$> fieldWhiteBalance fields)
  , (0x141, isoKey, isoStored <$> fieldIso fields)
  ]

-- | The fields a recipe takes from the declared payload base.
baseFields :: RecipeFields -> [String]
baseFields fields =
  concat
    [ missing filmKey (fieldFilm fields)
    , missing rangeKey (fieldRange fields)
    , missing highlightKey (fieldHighlight fields)
    , missing shadowKey (fieldShadow fields)
    , missing colourKey (fieldColour fields)
    , missing sharpnessKey (fieldSharpness fields)
    , missing noiseKey (fieldNoiseReduction fields)
    , missing balanceKey (fieldWhiteBalance fields)
    , missing isoKey (fieldIso fields)
    ]
 where
  missing label spelled = [label | isNothing spelled]

-- | The stored codes of a text recipe over the declared payload base: every
-- field it names takes its own code and every field it omits takes the
-- base's, with the omitted fields' names as the report. The base is read as
-- it stands, so a caller that needs the canonical form (unbound bits
-- cleared) canonicalizes it first.
fillRecipe :: B.ByteString -> TextRecipe -> Either String (Recipe, [String])
fillRecipe base recipe = do
  baseRecipe <- slotRecipe base
  let baseCodes = recipePairs baseRecipe
      wanted = textCodes (textFields recipe)
  pairs <- forM wanted $ \(property, label, requested) -> case requested of
    Just code -> Right (property, code)
    Nothing -> case lookup property baseCodes of
      Just code -> Right (property, code)
      Nothing -> Left ("the declared payload base carries no " ++ label ++ " code")
  cooked <- recipeFromPairs pairs
  pure (cooked, baseFields (textFields recipe))

-- | The declared base with one offered IMAGE SIZE menu row placed, in the
-- canonical form a stored payload has. The storage kernel refuses a payload
-- whose pair no menu offers, so a base of its own may not carry one.
preparedBase :: PayloadDomain -> Int -> B.ByteString -> Either String B.ByteString
preparedBase domain row base = do
  placed <- withImageSizeRow domain row base
  canonical <- canonicalBankPayload placed
  case payloadUnsafeReason domain canonical of
    Just reason -> Left reason
    Nothing -> pure (bankPayloadContents canonical)

-- | A payload the storage kernel's payload-safety walk accepts: the same
-- conversion-table rows, IMAGE SIZE pair and IMAGE QUALITY/RAW pair the stock
-- menus produce.
safePayload :: PayloadDomain -> B.ByteString -> Either String BankPayload
safePayload domain bytes = do
  canonical <- canonicalBankPayload bytes
  case payloadUnsafeReason domain canonical of
    Just reason -> Left reason
    Nothing -> pure canonical

-- | The canonical text of a slot payload over the declared base: the name and
-- exactly the fields whose stored code the payload does not take from the
-- base. A payload that differs from the base outside the nine fields, or that
-- carries a code with no text spelling, is refused rather than approximated.
exportRecipeText :: B.ByteString -> SlotName -> B.ByteString -> Either String (String, [String])
exportRecipeText base name payload = do
  fields <- payloadFields base payload
  pure (renderRecipeText (TextRecipe name fields), baseFields fields)

payloadFields :: B.ByteString -> B.ByteString -> Either String RecipeFields
payloadFields base payload = do
  baseRecipe <- slotRecipe base
  recipe <- slotRecipe payload
  rebuilt <- encodeSlotPayloadOver base recipe
  unless (rebuilt == payload) $
    Left
      ( "payload bytes "
          ++ show [k | k <- [0 .. payloadBytes - 1], B.index payload k /= B.index rebuilt k]
          ++ " differ from the declared base outside the recipe fields"
      )
  let codes = recipePairs recipe
      baseCodes = recipePairs baseRecipe
      one label property decode = case (lookup property codes, lookup property baseCodes) of
        (Just code, Just baseCode)
          | code == baseCode -> Right Nothing
          | otherwise ->
              case decode code of
                Just value -> Right (Just value)
                Nothing -> Left (label ++ " stored code " ++ show (codeWord code) ++ " has no text spelling")
        _ -> Left ("the declared payload base or the payload carries no " ++ label ++ " code")
  RecipeFields
    <$> one filmKey 0xfe filmOfStored
    <*> one rangeKey 0x104 rangeOfStored
    <*> one highlightKey 0x110 hardnessOfStored
    <*> one shadowKey 0x111 hardnessOfStored
    <*> one colourKey 0x10e colourOfStored
    <*> one sharpnessKey 0x10f hardnessOfStored
    <*> one noiseKey 0xef levelOfStored
    <*> one balanceKey 0xed balanceOfStored
    <*> one isoKey 0x141 isoOfStored

codeWord :: StoredCode -> Word8
codeWord (StoredCode code) = code

-- parsing --------------------------------------------------------------

-- | Parses one recipe file's text. `defaultName` is the slot name a file
-- without a Name line takes. Every failure names the line and the field it is
-- about.
parseRecipeText :: String -> String -> Either String TextRecipe
parseRecipeText defaultName contents = do
  (named, fields) <- foldM step (Nothing, emptyFields) (zip [1 :: Int ..] (lines contents))
  resolved <- case named of
    Just name -> Right name
    Nothing -> slotName defaultName
  pure (TextRecipe resolved fields)
 where
  step state (lineNo, raw) = case trim raw of
    "" -> Right state
    ('#' : _) -> Right state
    line -> atLine lineNo (stepLine state line)

  stepLine (named, fields) line = do
    (rawKey, rawValue) <- maybe (Left ("syntax: expected \"Field: Value\", got " ++ show line)) Right (splitPair line)
    let key = normalizeKey rawKey
        label = trim rawKey
        raw = trim rawValue
    when (null raw) (Left ("empty value for field " ++ show label))
    case key of
      "name" -> do
        name <- slotName raw
        when (isJust named) (Left ("duplicate field " ++ show nameKey))
        pure (Just name, fields)
      _
        | key `elem` ["film", "filmsimulation"] -> set (assign label (fieldFilm fields) (\v -> fields {fieldFilm = Just v}) (parseChoice filmKey filmOffered filmAliases filmUnsupported raw))
        | key `elem` ["dr", "dynamicrange"] -> set (assign label (fieldRange fields) (\v -> fields {fieldRange = Just v}) (parseChoice rangeKey rangeOffered rangeAliases [] raw))
        | key `elem` ["highlight", "highlighttone"] -> set (assign label (fieldHighlight fields) (\v -> fields {fieldHighlight = Just v}) (parseTone highlightKey raw))
        | key `elem` ["shadow", "shadowtone"] -> set (assign label (fieldShadow fields) (\v -> fields {fieldShadow = Just v}) (parseTone shadowKey raw))
        | key `elem` ["color", "colour"] -> set (assign label (fieldColour fields) (\v -> fields {fieldColour = Just v}) (parseTone colourKey raw))
        | key `elem` ["sharpness"] -> set (assign label (fieldSharpness fields) (\v -> fields {fieldSharpness = Just v}) (parseTone sharpnessKey raw))
        | key `elem` ["nr", "noisereduction"] -> set (assign label (fieldNoiseReduction fields) (\v -> fields {fieldNoiseReduction = Just v}) (parseTone noiseKey raw))
        | key `elem` ["wb", "whitebalance"] -> set (assign label (fieldWhiteBalance fields) (\v -> fields {fieldWhiteBalance = Just v}) (parseChoice balanceKey balanceOffered balanceAliases balanceUnsupported raw))
        | key `elem` ["iso"] -> set (assign label (fieldIso fields) (\v -> fields {fieldIso = Just v}) (parseChoice isoKey isoOffered isoAliases isoUnsupported raw))
        | Just reason <- lookup key unsupportedKeys ->
            Left ("field " ++ show label ++ " is unsupported: " ++ reason)
        | otherwise -> Left ("unknown field " ++ show label)
   where
    set fieldsResult = do
      fields' <- fieldsResult
      pure (named, fields')

-- | One field's assignment, refusing a second spelling of the same field.
assign :: String -> Maybe a -> (a -> RecipeFields) -> Either String a -> Either String RecipeFields
assign label spelled update parsed = do
  when (isJust spelled) (Left ("duplicate field " ++ show label))
  value <- parsed
  pure (update value)

atLine :: Int -> Either String a -> Either String a
atLine lineNo = either (Left . (prefix ++)) Right
 where
  prefix = "line " ++ show lineNo ++ ": "

splitPair :: String -> Maybe (String, String)
splitPair line = case break isSeparator line of
  (key, _ : value) | not (null (trim key)) -> Just (trim key, trim value)
  _ -> Nothing
 where
  isSeparator c = c == ':' || c == '='

trim :: String -> String
trim = dropWhileEnd isSpace . dropWhile isSpace

normalizeKey :: String -> String
normalizeKey = filter (`notElem` (" ._-()" :: String)) . map toLower

normalizeValue :: String -> String
normalizeValue = filter (`notElem` (" ._%()" :: String)) . map toLower

-- | One spelled value: the spellings that mean it, the spellings this body
-- has no setting for, and two failure shapes for everything else. A numeric
-- spelling the field does not offer is reported out of range.
parseChoice :: String -> String -> [(String, a)] -> [(String, String)] -> String -> Either String a
parseChoice label offered aliases unavailable raw =
  case lookup spelled aliases of
    Just value -> Right value
    Nothing -> case lookup spelled unavailable of
      Just reason -> Left (show raw ++ " is unsupported for " ++ label ++ ": " ++ reason)
      Nothing
        | Just _ <- numericValue spelled -> Left (show raw ++ " is out of range: " ++ label ++ " takes " ++ offered)
        | otherwise -> Left (show raw ++ " is not a " ++ label ++ " value (offered: " ++ offered ++ ")")
 where
  spelled = normalizeValue raw

parseTone :: String -> String -> Either String Tone
parseTone label raw =
  case lookup spelled toneAliases of
    Just tone -> Right tone
    Nothing -> case numericValue spelled of
      Just level
        | level >= -2 && level <= 2 -> Right (toneOfLevel level)
        | otherwise -> Left (show raw ++ " is out of range: " ++ label ++ " takes " ++ toneOffered)
      Nothing -> Left (show raw ++ " is not a " ++ label ++ " value (offered: " ++ toneOffered ++ ")")
 where
  spelled = normalizeValue raw

numericValue :: String -> Maybe Int
numericValue raw = case raw of
  ('+' : rest) -> whole rest
  ('-' : rest) -> negate <$> whole rest
  _ -> whole raw
 where
  whole digits = case reads digits of
    [(number, "")] -> Just number
    _ -> Nothing

offeredList :: (a -> String) -> [a] -> String
offeredList render values = intercalate ", " (map render values)

filmOffered, rangeOffered, toneOffered, balanceOffered, isoOffered :: String
filmOffered = offeredList renderFilm [minBound ..]
rangeOffered = offeredList renderRange [minBound ..]
toneOffered = offeredList renderTone [minBound ..]
balanceOffered = offeredList renderBalance [minBound ..]
isoOffered = offeredList renderIso [minBound ..]

-- | Spellings that mean one offered value. They cover the camera's own names
-- and the X70 project's `key=value` spellings where both mean the same thing
-- on X-M1 1.50.
filmAliases :: [(String, FilmValue)]
filmAliases =
  [ ("provia/standard", ProviaStandard)
  , ("provia", ProviaStandard)
  , ("velvia/vivid", VelviaVivid)
  , ("velvia", VelviaVivid)
  , ("astia/soft", AstiaSoft)
  , ("astia", AstiaSoft)
  , ("b&w", BlackAndWhite)
  , ("bw", BlackAndWhite)
  , ("monochromestd", BlackAndWhite)
  , ("sepia", Sepia)
  , ("pronegstd", ProNegStd)
  , ("proneghi", ProNegHi)
  ]

rangeAliases :: [(String, DynamicRange)]
rangeAliases =
  [ ("auto", RangeAuto)
  , ("drauto", RangeAuto)
  , ("100", Range100)
  , ("dr100", Range100)
  , ("200", Range200)
  , ("dr200", Range200)
  , ("400", Range400)
  , ("dr400", Range400)
  ]

toneAliases :: [(String, Tone)]
toneAliases =
  [ ("-2", ToneMinus2)
  , ("soft", ToneMinus2)
  , ("low", ToneMinus2)
  , ("-1", ToneMinus1)
  , ("mediumsoft", ToneMinus1)
  , ("mediumlow", ToneMinus1)
  , ("0", Tone0)
  , ("standard", Tone0)
  , ("mid", Tone0)
  , ("+1", TonePlus1)
  , ("1", TonePlus1)
  , ("mediumhard", TonePlus1)
  , ("mediumhigh", TonePlus1)
  , ("+2", TonePlus2)
  , ("2", TonePlus2)
  , ("hard", TonePlus2)
  , ("high", TonePlus2)
  ]

balanceAliases :: [(String, WhiteBalance)]
balanceAliases =
  [ ("auto", BalanceAuto)
  , ("custom", BalanceCustom)
  , ("fine", BalanceFine)
  , ("shade", BalanceShade)
  , ("fluorescentlight1", BalanceFluorescent1)
  , ("fluorescentlight-1", BalanceFluorescent1)
  , ("fluorescent-1", BalanceFluorescent1)
  , ("fluorescent1", BalanceFluorescent1)
  , ("fluorescentlight2", BalanceFluorescent2)
  , ("fluorescentlight-2", BalanceFluorescent2)
  , ("fluorescent-2", BalanceFluorescent2)
  , ("fluorescent2", BalanceFluorescent2)
  , ("fluorescentlight3", BalanceFluorescent3)
  , ("fluorescentlight-3", BalanceFluorescent3)
  , ("fluorescent-3", BalanceFluorescent3)
  , ("fluorescent3", BalanceFluorescent3)
  , ("incandescent", BalanceIncandescent)
  ]

isoAliases :: [(String, IsoValue)]
isoAliases =
  [ ("auto", IsoAuto)
  , ("l100", IsoL100)
  , ("100", IsoL100)
  , ("200", Iso200)
  , ("250", Iso250)
  , ("320", Iso320)
  , ("400", Iso400)
  , ("500", Iso500)
  , ("640", Iso640)
  , ("800", Iso800)
  , ("1000", Iso1000)
  , ("1250", Iso1250)
  , ("1600", Iso1600)
  , ("2000", Iso2000)
  , ("2500", Iso2500)
  , ("3200", Iso3200)
  , ("4000", Iso4000)
  , ("5000", Iso5000)
  , ("6400", Iso6400)
  , ("h12800", IsoH12800)
  , ("12800", IsoH12800)
  , ("h25600", IsoH25600)
  , ("25600", IsoH25600)
  ]

-- | Spellings this body has a name for but no setting. The X70 project
-- spells several of them.
filmUnsupported, balanceUnsupported, isoUnsupported :: [(String, String)]
filmUnsupported =
  [ ("classicchrome", "the X-M1 1.50 does not implement Classic Chrome")
  , ("eterna", "the X-M1 1.50 does not implement Eterna")
  , ("monochromeyellow", "filtered monochrome is not offered on the X-M1 1.50")
  , ("monochromered", "filtered monochrome is not offered on the X-M1 1.50")
  , ("monochromegreen", "filtered monochrome is not offered on the X-M1 1.50")
  ]
balanceUnsupported =
  [ ("daylight", "the X-M1 1.50 has no DAYLIGHT preset")
  , ("cloudy", "the X-M1 1.50 has no CLOUDY preset")
  , ("underwater", "the X-M1 1.50 has no UNDERWATER preset")
  ]
isoUnsupported =
  [ ("auto1", "the X70 Auto ISO presets are not an X-M1 1.50 setting")
  , ("auto2", "the X70 Auto ISO presets are not an X-M1 1.50 setting")
  , ("auto3", "the X70 Auto ISO presets are not an X-M1 1.50 setting")
  ]

-- | Keys the format refuses, with the reason the body does not carry them.
unsupportedKeys :: [(String, String)]
unsupportedKeys =
  [ ("kelvin", "Kelvin white balance is not an X-M1 1.50 setting")
  , ("wbshift", shiftReason)
  , ("whitebalanceshift", shiftReason)
  , ("autoiso1base", autoReason)
  , ("autoiso1max", autoReason)
  , ("autoiso1minshutter", autoReason)
  , ("autoiso2base", autoReason)
  , ("autoiso2max", autoReason)
  , ("autoiso2minshutter", autoReason)
  , ("autoiso3base", autoReason)
  , ("autoiso3max", autoReason)
  , ("autoiso3minshutter", autoReason)
  , ("grain", "grain is not implemented on the X-M1 1.50")
  , ("grainroughness", "grain is not implemented on the X-M1 1.50")
  , ("grainsize", "grain is not implemented on the X-M1 1.50")
  , ("colorchrome", "Color Chrome is not implemented on the X-M1 1.50")
  , ("colourchrome", "Color Chrome is not implemented on the X-M1 1.50")
  , ("colorchromeeffect", "Color Chrome is not implemented on the X-M1 1.50")
  , ("colorchromefxblue", "Color Chrome is not implemented on the X-M1 1.50")
  , ("clarity", "clarity is not implemented on the X-M1 1.50")
  , ("drangepriority", "D RANGE PRIORITY is not implemented on the X-M1 1.50")
  , ("ev", "exposure compensation is not one of the bank's recipe fields")
  , ("exposurecompensation", "exposure compensation is not one of the bank's recipe fields")
  ]
 where
  shiftReason = "the bank record cannot store the WB shift grid's two axes (reports/recipe-design.md section 2)"
  autoReason = "the X70 Auto ISO presets are not an X-M1 1.50 setting"
