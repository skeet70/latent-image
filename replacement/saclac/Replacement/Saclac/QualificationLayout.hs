{-# LANGUAGE OverloadedStrings #-}
-- | Compare the canonical DWARF layout of every type reachable from the
-- declared Root statics in two linked images.
--
-- The parser reads the textual tree emitted by @llvm-dwarfdump --debug-info@.
-- A type's canonical layout is its kind, byte size, alignment, ordered
-- members (name, offset, type), variant parts (discriminant and each
-- variant's discriminant value and layout), array bounds and pointee type.
-- Type names are deliberately not part of the layout: the qualification
-- build has a different crate hash, while field, variant and base-type names
-- are compared.
module Replacement.Saclac.QualificationLayout
  ( RootSpec(..)
  , Mismatch(..)
  , Report(..)
  , DwarfFile(..)
  , DwarfObject(..)
  , Die(..)
  , Attr(..)
  , parseDwarf
  , readDwarf
  , compareDwarf
  , checkLayouts
  , reportJson
  ) where

import Control.Monad (forM_, unless, when)
import Data.Aeson (Value, object, (.=))
import Data.Char (isDigit, isHexDigit, isSpace)
import Data.List (isPrefixOf, isSuffixOf, sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe)
import qualified Data.Set as S
import Data.Word (Word64)
import Numeric (readHex)
import System.Exit (ExitCode(..))
import System.Process (readProcessWithExitCode)

-- | A static whose type seeds the comparison, e.g. @OWNER@ in
-- @saclac/root/src/stock/composition.rs@.
data RootSpec = RootSpec
  { rootName :: String
  , rootFileSuffix :: String
  } deriving (Eq, Show)

data Mismatch = Mismatch
  { mismatchProduction :: String
  , mismatchQualification :: String
  , mismatchPath :: String
  , mismatchDetail :: String
  } deriving (Eq, Show)

data Report = Report
  { reportVerdict :: String
  , reportTypesCompared :: Int
  , reportMismatches :: [Mismatch]
  } deriving (Eq, Show)

data DwarfFile = DwarfFile
  { dwarfObjects :: [DwarfObject]
  } deriving (Eq, Show)

data DwarfObject = DwarfObject
  { objectLabel :: String
  , objectDies :: M.Map Word64 Die
  , objectRoots :: [Die]
  } deriving (Eq, Show)

data Die = Die
  { dieOffset :: Word64
  , dieTag :: String
  , dieAttrs :: M.Map String Attr
  , dieChildren :: [Die]
  } deriving (Eq, Show)

data Attr = Attr
  { attrText :: String
  , attrRef :: Maybe Word64
  , attrInt :: Maybe Integer
  , attrString :: Maybe String
  } deriving (Eq, Show)

-- ---------------------------------------------------------------------------
-- Parsing

parseDwarf :: String -> DwarfFile
parseDwarf input = DwarfFile (reverse (finishObject finalLabel finalEntries doneObjects))
  where
    ls = lines input
    (finalLabel, finalEntries, doneObjects) = foldl step ("input", [], []) ls

    step (label, entries, done) line
      | isObjectHeader line =
          let label' = objectLabelOf line
          in (label', [], finishObject label entries done)
      | Just (off, depth, tag) <- dieHeader line =
          (label, (off, depth, tag, M.empty) : entries, done)
      | Just (name, attribute) <- attrLine line =
          case entries of
            [] -> (label, entries, done)
            (off, depth, tag, attrs) : rest ->
              (label, (off, depth, tag, M.insert name attribute attrs) : rest, done)
      | otherwise = (label, entries, done)

    finishObject label entries done =
      let ordered = reverse entries
          (roots, _) = buildLevel 0 ordered
          dies = M.fromList [(dieOffset die, die) | die <- flatten roots]
      in if null ordered
           then done
           else DwarfObject label dies roots : done

isObjectHeader :: String -> Bool
isObjectHeader line = "file format" `isInfix` line && "):" `isInfix` line

objectLabelOf :: String -> String
objectLabelOf line =
  let (before, _) = breakOn "file format" line
  in trim (dropSuffix ":" (trim before))

dieHeader :: String -> Maybe (Word64, Int, String)
dieHeader line = do
  rest0 <- stripPrefix "0x" line
  let (hexDigits, rest1) = span isHexDigit rest0
  off <- parseHex hexDigits
  rest2 <- stripPrefix ":" rest1
  let spaces = length (takeWhile (== ' ') rest2)
      tag = dropWhile isSpace rest2
  if "DW_TAG_" `isPrefixOf` tag
    then Just (off, (spaces - 1) `div` 2, takeWhile (not . isSpace) tag)
    else Nothing

attrLine :: String -> Maybe (String, Attr)
attrLine line =
  let s = dropWhile isSpace line
  in if "DW_AT_" `isPrefixOf` s
       then
         let (name, rest) = break isSpace s
             value = dropWhile isSpace rest
         in if "(" `isPrefixOf` value && ")" `isSuffixOf` value
              then Just (name, parseAttrValue (init (drop 1 value)))
              else Just (name, Attr value Nothing Nothing Nothing)
       else Nothing

parseAttrValue :: String -> Attr
parseAttrValue raw =
  Attr
    { attrText = raw
    , attrRef = parseHexRef raw
    , attrInt = parseInteger raw
    , attrString = parseQuoted raw
    }

parseHexRef :: String -> Maybe Word64
parseHexRef s = do
  rest <- stripPrefix "0x" s
  let (hexDigits, _) = span isHexDigit rest
  parseHex hexDigits

parseInteger :: String -> Maybe Integer
parseInteger s
  | Just rest <- stripPrefix "-0x" s = negate <$> parseHexInteger rest
  | Just rest <- stripPrefix "-" s =
      if not (null rest) && all isDigit rest then Just (negate (read rest)) else Nothing
  | Just rest <- stripPrefix "0x" s = parseHexInteger rest
  | (first : _) <- s, isDigit first =
      let (digits, _) = span isDigit s in Just (read digits)
  | otherwise = Nothing

parseHexInteger :: String -> Maybe Integer
parseHexInteger s =
  let (hexDigits, _) = span isHexDigit s
  in case readHex hexDigits of
       ((value, _) : _) -> Just value
       [] -> Nothing

parseQuoted :: String -> Maybe String
parseQuoted s =
  case dropWhile (/= '"') s of
    [] -> Nothing
    (_ : rest) ->
      let (body, _) = break (== '"') rest
      in Just body

parseHex :: String -> Maybe Word64
parseHex s =
  if null s || not (all isHexDigit s)
    then Nothing
    else case readHex s of
      ((value, _) : _) -> Just value
      [] -> Nothing

buildLevel :: Int -> [(Word64, Int, String, M.Map String Attr)] -> ([Die], [(Word64, Int, String, M.Map String Attr)])
buildLevel _ [] = ([], [])
buildLevel depth entries@((off, d, tag, attrs) : rest)
  | d < depth = ([], entries)
  | d == depth =
      let (children, rest1) = buildLevel (depth + 1) rest
          (siblings, rest2) = buildLevel depth rest1
      in (Die off tag attrs children : siblings, rest2)
  | otherwise = ([], entries)

flatten :: [Die] -> [Die]
flatten dies = concatMap (\die -> die : flatten (dieChildren die)) dies

-- ---------------------------------------------------------------------------
-- Attribute access

attr :: String -> Die -> Maybe Attr
attr name die = M.lookup name (dieAttrs die)

intAttr :: String -> Die -> Maybe Integer
intAttr name die = attr name die >>= attrInt

strAttr :: String -> Die -> Maybe String
strAttr name die = attr name die >>= attrString

refAttr :: String -> Die -> Maybe Word64
refAttr name die = attr name die >>= attrRef

textAttr :: String -> Die -> Maybe String
textAttr name die = attr name die >>= Just . attrText

dieName :: Die -> Maybe String
dieName die = strAttr "DW_AT_name" die

-- ---------------------------------------------------------------------------
-- Comparison

data CmpState = CmpState
  { cmpVisited :: S.Set (Word64, Word64)
  , cmpCompared :: Int
  , cmpMismatches :: [Mismatch]
  }

type Cmp = CmpM

newtype CmpM a = CmpM { runCmpM :: CmpState -> (a, CmpState) }

instance Functor CmpM where
  fmap f (CmpM action) = CmpM $ \state ->
    let (value, state') = action state in (f value, state')

instance Applicative CmpM where
  pure value = CmpM $ \state -> (value, state)
  CmpM left <*> CmpM right = CmpM $ \state ->
    let (function, state1) = left state
        (value, state2) = right state1
    in (function value, state2)

instance Monad CmpM where
  CmpM action >>= next = CmpM $ \state ->
    let (value, state') = action state in runCmpM (next value) state'

getCmp :: Cmp CmpState
getCmp = CmpM $ \state -> (state, state)

modifyCmp :: (CmpState -> CmpState) -> Cmp ()
modifyCmp f = CmpM $ \state -> ((), f state)

execCmp :: Cmp a -> CmpState -> CmpState
execCmp action state = snd (runCmpM action state)

emptyCmp :: CmpState
emptyCmp = CmpState S.empty 0 []

addMismatch :: String -> String -> Cmp ()
addMismatch path detail =
  modifyCmp $ \s -> s { cmpMismatches = cmpMismatches s ++ [Mismatch "" "" path detail] }

compareMaybe :: Eq a => Show a => String -> String -> Maybe a -> Maybe a -> Cmp ()
compareMaybe path label left right =
  when (left /= right) $
    addMismatch path (label ++ " " ++ show left ++ " vs " ++ show right)

compareTag :: String -> Die -> Die -> Cmp Bool
compareTag path left right =
  if dieTag left == dieTag right
    then pure True
    else do
      addMismatch path ("tag " ++ dieTag left ++ " vs " ++ dieTag right)
      pure False

compareTypeRef :: DwarfObject -> DwarfObject -> String -> Die -> Die -> Cmp ()
compareTypeRef leftObj rightObj path left right =
  case (refAttr "DW_AT_type" left, refAttr "DW_AT_type" right) of
    (Just a, Just b) -> compareType leftObj rightObj path a b
    (Nothing, Nothing) -> pure ()
    (a, b) -> addMismatch path ("type reference " ++ show a ++ " vs " ++ show b)

compareType :: DwarfObject -> DwarfObject -> String -> Word64 -> Word64 -> Cmp ()
compareType leftObj rightObj path leftOff rightOff = do
  let (leftOff', leftDie) = stripType leftObj leftOff
      (rightOff', rightDie) = stripType rightObj rightOff
  case (leftDie, rightDie) of
    (Nothing, _) -> addMismatch path ("missing type DIE 0x" ++ hex leftOff)
    (_, Nothing) -> addMismatch path ("missing type DIE 0x" ++ hex rightOff)
    (Just a, Just b) -> do
      state <- getCmp
      let visited = cmpVisited state
          key = (leftOff', rightOff')
      if S.member key visited
        then pure ()
        else do
          modifyCmp $ \s -> s { cmpVisited = S.insert key (cmpVisited s), cmpCompared = cmpCompared s + 1 }
          compareDie leftObj rightObj path a b

stripType :: DwarfObject -> Word64 -> (Word64, Maybe Die)
stripType obj off = go (10 :: Int) off
  where
    go 0 current = (current, M.lookup current (objectDies obj))
    go n current =
      case M.lookup current (objectDies obj) of
        Nothing -> (current, Nothing)
        Just die
          | dieTag die `elem` qualifierTags ->
              case refAttr "DW_AT_type" die of
                Just next -> go (n - 1) next
                Nothing -> (current, Just die)
          | otherwise -> (current, Just die)
    qualifierTags =
      [ "DW_TAG_typedef"
      , "DW_TAG_const_type"
      , "DW_TAG_volatile_type"
      , "DW_TAG_restrict_type"
      , "DW_TAG_atomic_type"
      ]

compareDie :: DwarfObject -> DwarfObject -> String -> Die -> Die -> Cmp ()
compareDie leftObj rightObj path left right = do
  same <- compareTag path left right
  when same $ do
    compareMaybe path "byte_size" (intAttr "DW_AT_byte_size" left) (intAttr "DW_AT_byte_size" right)
    compareMaybe path "alignment" (intAttr "DW_AT_alignment" left) (intAttr "DW_AT_alignment" right)
    case dieTag left of
      "DW_TAG_base_type" -> do
        compareMaybe path "name" (dieName left) (dieName right)
        compareMaybe path "encoding" (textAttr "DW_AT_encoding" left) (textAttr "DW_AT_encoding" right)
      "DW_TAG_structure_type" -> compareAggregate leftObj rightObj path left right
      "DW_TAG_union_type" -> compareAggregate leftObj rightObj path left right
      "DW_TAG_class_type" -> compareAggregate leftObj rightObj path left right
      "DW_TAG_enumeration_type" -> compareEnumeration leftObj rightObj path left right
      "DW_TAG_array_type" -> compareArray leftObj rightObj path left right
      "DW_TAG_pointer_type" -> comparePointer leftObj rightObj path left right
      "DW_TAG_reference_type" -> comparePointer leftObj rightObj path left right
      "DW_TAG_rvalue_reference_type" -> comparePointer leftObj rightObj path left right
      "DW_TAG_subroutine_type" -> compareSubroutine leftObj rightObj path left right
      "DW_TAG_ptr_to_member_type" -> do
        compareTypeRef leftObj rightObj (path ++ ".class") left right
        case (refAttr "DW_AT_type" left, refAttr "DW_AT_type" right) of
          (Just a, Just b) -> compareType leftObj rightObj (path ++ ".member") a b
          (a, b) -> compareMaybe path "member type" a b
      "DW_TAG_unspecified_type" ->
        compareMaybe path "name" (dieName left) (dieName right)
      "DW_TAG_typedef" -> compareTypeRef leftObj rightObj path left right
      "DW_TAG_const_type" -> compareTypeRef leftObj rightObj path left right
      "DW_TAG_volatile_type" -> compareTypeRef leftObj rightObj path left right
      "DW_TAG_restrict_type" -> compareTypeRef leftObj rightObj path left right
      "DW_TAG_atomic_type" -> compareTypeRef leftObj rightObj path left right
      "DW_TAG_member" -> compareMember leftObj rightObj path left right
      "DW_TAG_variant_part" -> compareVariantPart leftObj rightObj path left right
      "DW_TAG_variant" -> compareVariant leftObj rightObj path left right
      "DW_TAG_subrange_type" -> compareSubrange path left right
      "DW_TAG_unspecified_parameters" -> pure ()
      other -> do
        case (refAttr "DW_AT_type" left, refAttr "DW_AT_type" right) of
          (Just a, Just b) -> compareType leftObj rightObj (path ++ ".type") a b
          (a, b) -> compareMaybe path "type reference" a b
        unless (null (dieChildren left) && null (dieChildren right)) $
          addMismatch path ("unsupported aggregate " ++ other)

compareAggregate :: DwarfObject -> DwarfObject -> String -> Die -> Die -> Cmp ()
compareAggregate leftObj rightObj path left right = do
  when (isDeclaration left || isDeclaration right) $
    addMismatch path ("declaration without a definition " ++ show (isDeclaration left) ++ " vs " ++ show (isDeclaration right))
  compareMembers leftObj rightObj path left right
  compareVariantParts leftObj rightObj path left right

isDeclaration :: Die -> Bool
isDeclaration die = textAttr "DW_AT_declaration" die == Just "true"

memberChildren :: Die -> [Die]
memberChildren = filter ((== "DW_TAG_member") . dieTag) . dieChildren

memberKey :: Die -> (Integer, String)
memberKey die =
  ( fromMaybe (-1) (intAttr "DW_AT_data_member_location" die)
  , fromMaybe "" (dieName die)
  )

compareMembers :: DwarfObject -> DwarfObject -> String -> Die -> Die -> Cmp ()
compareMembers leftObj rightObj path left right =
  let leftMembers = sortOn memberKey (memberChildren left)
      rightMembers = sortOn memberKey (memberChildren right)
  in compareZip path "member" leftMembers rightMembers (compareMember leftObj rightObj)

compareMember :: DwarfObject -> DwarfObject -> String -> Die -> Die -> Cmp ()
compareMember leftObj rightObj path left right = do
  compareMaybe path "name" (dieName left) (dieName right)
  compareMaybe path "offset" (intAttr "DW_AT_data_member_location" left) (intAttr "DW_AT_data_member_location" right)
  compareMaybe path "bit_size" (intAttr "DW_AT_bit_size" left) (intAttr "DW_AT_bit_size" right)
  compareMaybe path "data_bit_offset" (intAttr "DW_AT_data_bit_offset" left) (intAttr "DW_AT_data_bit_offset" right)
  compareTypeRef leftObj rightObj path left right

compareVariantParts :: DwarfObject -> DwarfObject -> String -> Die -> Die -> Cmp ()
compareVariantParts leftObj rightObj path left right =
  let leftParts = filter ((== "DW_TAG_variant_part") . dieTag) (dieChildren left)
      rightParts = filter ((== "DW_TAG_variant_part") . dieTag) (dieChildren right)
  in compareZip path "variant_part" leftParts rightParts (compareVariantPart leftObj rightObj)

compareVariantPart :: DwarfObject -> DwarfObject -> String -> Die -> Die -> Cmp ()
compareVariantPart leftObj rightObj path left right = do
  case (refAttr "DW_AT_discr" left, refAttr "DW_AT_discr" right) of
    (Just a, Just b) -> compareType leftObj rightObj (path ++ ".discriminant") a b
    (a, b) -> compareMaybe path "discriminant" a b
  let leftVariants = sortOn variantValue (filter ((== "DW_TAG_variant") . dieTag) (dieChildren left))
      rightVariants = sortOn variantValue (filter ((== "DW_TAG_variant") . dieTag) (dieChildren right))
  compareZip path "variant" leftVariants rightVariants (compareVariant leftObj rightObj)

variantValue :: Die -> Integer
variantValue die = fromMaybe (-1) (intAttr "DW_AT_discr_value" die)

compareVariant :: DwarfObject -> DwarfObject -> String -> Die -> Die -> Cmp ()
compareVariant leftObj rightObj path left right = do
  compareMaybe path "discr_value" (intAttr "DW_AT_discr_value" left) (intAttr "DW_AT_discr_value" right)
  compareMembers leftObj rightObj path left right

compareEnumeration :: DwarfObject -> DwarfObject -> String -> Die -> Die -> Cmp ()
compareEnumeration leftObj rightObj path left right = do
  compareTypeRef leftObj rightObj (path ++ ".underlying") left right
  let leftValues = sortOn enumValue (filter ((== "DW_TAG_enumerator") . dieTag) (dieChildren left))
      rightValues = sortOn enumValue (filter ((== "DW_TAG_enumerator") . dieTag) (dieChildren right))
  compareZip path "enumerator" leftValues rightValues $ \p a b -> do
    compareMaybe p "name" (dieName a) (dieName b)
    compareMaybe p "value" (intAttr "DW_AT_const_value" a) (intAttr "DW_AT_const_value" b)

enumValue :: Die -> Integer
enumValue die = fromMaybe (-1) (intAttr "DW_AT_const_value" die)

compareArray :: DwarfObject -> DwarfObject -> String -> Die -> Die -> Cmp ()
compareArray leftObj rightObj path left right = do
  compareTypeRef leftObj rightObj (path ++ ".element") left right
  let leftRanges = filter ((== "DW_TAG_subrange_type") . dieTag) (dieChildren left)
      rightRanges = filter ((== "DW_TAG_subrange_type") . dieTag) (dieChildren right)
  compareZip path "subrange" leftRanges rightRanges compareSubrange

compareSubrange :: String -> Die -> Die -> Cmp ()
compareSubrange path left right = do
  let leftLower = fromMaybe 0 (intAttr "DW_AT_lower_bound" left)
      rightLower = fromMaybe 0 (intAttr "DW_AT_lower_bound" right)
      leftCount = subrangeCount left
      rightCount = subrangeCount right
  when (leftLower /= rightLower) $
    addMismatch path ("lower_bound " ++ show leftLower ++ " vs " ++ show rightLower)
  when (leftCount /= rightCount) $
    addMismatch path ("count " ++ show leftCount ++ " vs " ++ show rightCount)
  compareMaybe path "byte_stride" (intAttr "DW_AT_byte_stride" left) (intAttr "DW_AT_byte_stride" right)

subrangeCount :: Die -> Integer
subrangeCount die =
  case intAttr "DW_AT_count" die of
    Just n -> n
    Nothing ->
      case (intAttr "DW_AT_upper_bound" die, intAttr "DW_AT_lower_bound" die) of
        (Just upper, lower) -> upper - fromMaybe 0 lower + 1
        (Nothing, _) -> -1

comparePointer :: DwarfObject -> DwarfObject -> String -> Die -> Die -> Cmp ()
comparePointer leftObj rightObj path left right = do
  compareMaybe path "address_class" (intAttr "DW_AT_address_class" left) (intAttr "DW_AT_address_class" right)
  case (refAttr "DW_AT_type" left, refAttr "DW_AT_type" right) of
    (Just a, Just b) -> compareType leftObj rightObj (path ++ ".pointee") a b
    (Nothing, Nothing) -> pure ()
    (a, b) -> addMismatch path ("pointee " ++ show a ++ " vs " ++ show b)

compareSubroutine :: DwarfObject -> DwarfObject -> String -> Die -> Die -> Cmp ()
compareSubroutine leftObj rightObj path left right = do
  compareTypeRef leftObj rightObj (path ++ ".return") left right
  compareMaybe path "prototyped" (textAttr "DW_AT_prototyped" left) (textAttr "DW_AT_prototyped" right)
  let leftParams = filter ((== "DW_TAG_formal_parameter") . dieTag) (dieChildren left)
      rightParams = filter ((== "DW_TAG_formal_parameter") . dieTag) (dieChildren right)
  compareZip path "parameter" leftParams rightParams $ \p a b ->
    compareTypeRef leftObj rightObj p a b

compareZip :: String -> String -> [Die] -> [Die] -> (String -> Die -> Die -> Cmp ()) -> Cmp ()
compareZip path label left right compareOne = do
  when (length left /= length right) $
    addMismatch path (label ++ " count " ++ show (length left) ++ " vs " ++ show (length right))
  forM_ (zip [0 :: Int ..] (zip left right)) $ \(index, (a, b)) ->
    compareOne (path ++ "." ++ label ++ "[" ++ show index ++ "]") a b

-- ---------------------------------------------------------------------------
-- Roots and reports

findRoot :: DwarfObject -> RootSpec -> Either String Die
findRoot obj spec =
  case filter matches (flatten (objectRoots obj)) of
    [die] -> Right die
    [] -> Left ("root " ++ rootName spec ++ " not found in " ++ objectLabel obj)
    _ -> Left ("root " ++ rootName spec ++ " is ambiguous in " ++ objectLabel obj)
  where
    matches die =
      dieTag die == "DW_TAG_variable"
        && dieName die == Just (rootName spec)
        && maybe False (rootFileSuffix spec `isSuffixOf`) (strAttr "DW_AT_decl_file" die)

rootType :: DwarfObject -> RootSpec -> Either String Word64
rootType obj spec = do
  die <- findRoot obj spec
  case refAttr "DW_AT_type" die of
    Just ref -> Right ref
    Nothing -> Left ("root " ++ rootName spec ++ " has no type in " ++ objectLabel obj)

compareDwarf :: String -> String -> DwarfFile -> DwarfFile -> [RootSpec] -> Report
compareDwarf productionName qualificationName production qualification roots =
  let initial = execCmp (mapM_ compareRoot roots) emptyCmp
      mismatches = map (withNames productionName qualificationName) (cmpMismatches initial)
      verdict
        | not (null mismatches) = if any isMissing mismatches then "missing" else "mismatch"
        | cmpCompared initial == 0 = "missing"
        | otherwise = "match"
  in Report verdict (cmpCompared initial) mismatches
  where
    compareRoot spec = do
      case pickObject production spec of
        Left err -> addMismatch (rootName spec) err
        Right leftObj ->
          case pickObject qualification spec of
            Left err -> addMismatch (rootName spec) err
            Right rightObj ->
              case (rootType leftObj spec, rootType rightObj spec) of
                (Left err, _) -> addMismatch (rootName spec) err
                (_, Left err) -> addMismatch (rootName spec) err
                (Right leftRef, Right rightRef) ->
                  compareType leftObj rightObj (rootName spec) leftRef rightRef

    isMissing mismatch = any (`isInfix` mismatchDetail mismatch) ["not found", "missing"]

withNames :: String -> String -> Mismatch -> Mismatch
withNames productionName qualificationName mismatch =
  mismatch { mismatchProduction = productionName, mismatchQualification = qualificationName }

pickObject :: DwarfFile -> RootSpec -> Either String DwarfObject
pickObject dwarf spec =
  case filter hasRoot (dwarfObjects dwarf) of
    [obj] -> Right obj
    [] -> Left ("root " ++ rootName spec ++ " not found in any object")
    _ -> Left ("root " ++ rootName spec ++ " is in more than one object")
  where
    hasRoot obj = case findRoot obj spec of Right _ -> True; Left _ -> False

-- ---------------------------------------------------------------------------
-- Driver

readDwarf :: FilePath -> FilePath -> IO (Either String DwarfFile)
readDwarf dwarfdump elf = do
  (code, out, err) <- readProcessWithExitCode dwarfdump ["--debug-info", elf] ""
  pure $ case code of
    ExitSuccess -> Right (parseDwarf out)
    ExitFailure n -> Left ("llvm-dwarfdump failed for " ++ elf ++ " (exit " ++ show n ++ "): " ++ take 500 err)

checkLayouts :: FilePath -> FilePath -> FilePath -> [RootSpec] -> IO Report
checkLayouts dwarfdump production qualification roots = do
  productionDwarf <- readDwarf dwarfdump production
  qualificationDwarf <- readDwarf dwarfdump qualification
  pure $ case (productionDwarf, qualificationDwarf) of
    (Left err, _) -> failed err
    (_, Left err) -> failed err
    (Right p, Right q) -> compareDwarf production qualification p q roots
  where
    failed err = Report "missing" 0 [Mismatch production qualification "input" err]

reportJson :: Report -> Value
reportJson report =
  object
    [ "verdict" .= reportVerdict report
    , "types_compared" .= reportTypesCompared report
    , "mismatches" .=
        [ object
            [ "production" .= mismatchProduction mismatch
            , "qualification" .= mismatchQualification mismatch
            , "path" .= mismatchPath mismatch
            , "detail" .= mismatchDetail mismatch
            ]
        | mismatch <- reportMismatches report
        ]
    ]

-- ---------------------------------------------------------------------------
-- Small string helpers

hex :: Word64 -> String
hex value = showHexWord value

showHexWord :: Word64 -> String
showHexWord value =
  let digits = "0123456789abcdef"
      go 0 acc = acc
      go n acc = go (n `div` 16) (digits !! fromIntegral (n `mod` 16) : acc)
  in if value == 0 then "0" else go value ""

stripPrefix :: String -> String -> Maybe String
stripPrefix prefix s =
  if prefix `isPrefixOf` s then Just (drop (length prefix) s) else Nothing

dropSuffix :: String -> String -> String
dropSuffix suffix s =
  if suffix `isSuffixOf` s then take (length s - length suffix) s else s

breakOn :: String -> String -> (String, String)
breakOn needle haystack = go "" haystack
  where
    go acc rest
      | needle `isPrefixOf` rest = (reverse acc, rest)
      | otherwise = case rest of
          [] -> (reverse acc, "")
          (c : cs) -> go (c : acc) cs

trim :: String -> String
trim = dropWhileEnd isSpace . dropWhile isSpace

dropWhileEnd :: (a -> Bool) -> [a] -> [a]
dropWhileEnd predicate = reverse . dropWhile predicate . reverse

isInfix :: String -> String -> Bool
isInfix needle haystack = any (needle `isPrefixOf`) (tails haystack)
  where
    tails [] = [[]]
    tails xs@(_ : rest) = xs : tails rest
