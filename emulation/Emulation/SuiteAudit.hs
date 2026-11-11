{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | The static audit of stock-against-owned comparison suites.
--
-- One rule, applied per suite without executing a case:
--
-- * the entries bound are the entry rows of the image's own entry table;
-- * the entries entered are the distinct entry addresses of the suite's case
--   table, read as data;
-- * a stock role that maps bytes other than the firmware's at an entered entry
--   is reported.
--
-- The firmware is a declared input. Without it, the byte authentication is
-- @not run@ and the entered-versus-bound result is still reported.
module Emulation.SuiteAudit
  ( AuditStatus (..)
  , AuthStatus (..)
  , BoundKind (..)
  , BoundSpec (..)
  , CaseRow (..)
  , StockMapping (..)
  , SuiteSpec (..)
  , ImageSpec (..)
  , Gap (..)
  , GapKind (..)
  , SuiteAudit (..)
  , MissingEntry (..)
  , Unauthenticated (..)
  , Manifest (..)
  , parseManifest
  , parseCaseTable
  , parseGaps
  , readBoundEntries
  , auditOneSuite
  , auditTable
  , auditsJson
  , word32Hex
  , parseWord32
  , parseAddress
  , sha256HexBytes
  ) where

import Control.Monad (unless)
import Data.Aeson (Value (..), eitherDecodeStrict', object, toJSON)
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Char (digitToInt, isHexDigit, toLower)
import Data.List (find, nubBy, sortOn)
import Data.Maybe (mapMaybe)
import Data.Scientific (toBoundedInteger)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32)
import Emulation.Sha256 (sha256, sha256Hex, parseSha256Hex)
import FujiAF.Host (get, list, str)
import Numeric (readHex, showHex)

-- | The result of one suite audit.
data AuditStatus = AuditPassed | AuditFailed | AuditNotRun
  deriving (Eq, Show)

data AuthStatus = AuthRun | AuthNotRun
  deriving (Eq, Show)

data BoundKind = BoundLinkerMemory | BoundLinkerAsserts | BoundCarriers | BoundDeclared
  deriving (Eq, Show)

data BoundSpec = BoundSpec
  { boundKind :: BoundKind
  , boundSource :: FilePath
  , boundInclude :: [String]
  , boundExclude :: [String]
  , boundDomain :: Maybe String
  , boundGroups :: [String]
  , boundDeclared :: [(Word32, Word32, String)]
  , boundDerivedAt :: Maybe String
  , boundVerified :: Bool
  } deriving (Eq, Show)

data StockMapping
  = StockImage
  | StockDigest String
  | StockLiteral B.ByteString
  deriving (Eq, Show)

data CaseRow = CaseRow
  { caseName :: String
  , caseEntry :: Word32
  , caseExtent :: Word32
  , caseStock :: StockMapping
  } deriving (Eq, Show)

data ImageSpec = ImageSpec
  { imagePath :: FilePath
  , imageBase :: Word32
  , imageSha256 :: Maybe String
  } deriving (Eq, Show)

data SuiteSpec = SuiteSpec
  { suiteName :: String
  , suiteNote :: String
  , suiteImage :: Maybe ImageSpec
  , suiteBound :: BoundSpec
  , suiteCasesPath :: FilePath
  , suiteCasesSource :: String
  , suiteCasesDerivedAt :: Maybe String
  , suiteCasesVerified :: Bool
  , suiteGapsPath :: Maybe FilePath
  } deriving (Eq, Show)

data Manifest = Manifest
  { manifestSuites :: [SuiteSpec]
  } deriving (Eq, Show)

data GapKind = GapMissing | GapStock
  deriving (Eq, Show)

data Gap = Gap
  { gapEntry :: Word32
  , gapKind :: GapKind
  , gapReason :: String
  } deriving (Eq, Show)

data MissingEntry = MissingEntry
  { missingAddress :: Word32
  , missingSize :: Word32
  , missingName :: String
  } deriving (Eq, Show)

data Unauthenticated = Unauthenticated
  { unauthAddress :: Word32
  , unauthCase :: String
  , unauthMapped :: String
  , unauthExpected :: String
  } deriving (Eq, Show)

data SuiteAudit = SuiteAudit
  { auditedSuite :: String
  , auditBoundSource :: FilePath
  , auditCasesPath :: FilePath
  , auditCasesSource :: String
  , auditBoundLive :: Bool
  , auditBoundDerivedAt :: Maybe String
  , auditBoundVerified :: Bool
  , auditCasesDerivedAt :: Maybe String
  , auditCasesVerified :: Bool
  , auditBound :: [MissingEntry]
  , auditEntered :: [Word32]
  , auditEnteredBound :: [Word32]
  , auditMissing :: [MissingEntry]
  , auditUndeclaredMissing :: [MissingEntry]
  , auditUnauthenticated :: [Unauthenticated]
  , auditUndeclaredUnauthenticated :: [Unauthenticated]
  , auditDeclaredGaps :: [Gap]
  , auditAuthStatus :: AuthStatus
  , auditStatus :: AuditStatus
  , auditNote :: String
  , auditFirmware :: Maybe (FilePath, String)
  } deriving (Eq, Show)

-- Parsing -----------------------------------------------------------------

word32Hex :: Word32 -> String
word32Hex value = "0x" ++ replicate (8 - length text) '0' ++ text
  where text = showHex value ""

parseWord32 :: String -> Maybe Word32
parseWord32 text0 =
  let text = map toLower (dropPrefix text0)
   in case text of
        [] -> Nothing
        _ | all isHexDigit text ->
              case readHex text of
                [(value, "")]
                  | value <= toInteger (maxBound :: Word32) -> Just (fromInteger value)
                _ -> Nothing
        _ -> Nothing
  where
    dropPrefix ('0' : 'x' : rest) = rest
    dropPrefix rest = rest

parseAddress :: Value -> Maybe Word32
parseAddress (String text) = parseWord32 (T.unpack text)
parseAddress (Number value) = toBoundedInteger value :: Maybe Word32
parseAddress _ = Nothing

sha256HexBytes :: B.ByteString -> String
sha256HexBytes = sha256Hex . sha256

-- | The manifest is the suite registry. Unknown keys are refused so a typo
-- cannot silently drop a field.
parseManifest :: B.ByteString -> Either String Manifest
parseManifest bytes = do
  value <- eitherDecodeStrict' bytes
  case value of
    Object root -> do
      schema <- requireInt root "schema"
      unless (schema == (1 :: Int)) (Left "suite-audit manifest schema must be 1")
      suites <- case KM.lookup "suites" root of
        Just (Array rows) -> mapM parseSuite (V.toList rows)
        _ -> Left "suite-audit manifest needs a suites array"
      Right (Manifest suites)
    _ -> Left "suite-audit manifest must be a JSON object"

parseSuite :: Value -> Either String SuiteSpec
parseSuite (Object row) = do
  name <- requireText row "name"
  note <- optionalText row "note" ""
  image <- case KM.lookup "image" row of
    Nothing -> Right Nothing
    Just (Object imageRow) -> Just <$> parseImage imageRow
    Just _ -> Left (name ++ ": image must be an object")
  bound <- case KM.lookup "bound" row of
    Just (Object boundRow) -> parseBound name boundRow
    _ -> Left (name ++ ": bound must be an object")
  cases <- requireText row "cases"
  casesSource <- optionalText row "cases_source" cases
  casesDerivedAt <- optionalText row "cases_derived_at" ""
  casesVerified <- optionalBool row "cases_verified" False
  gapsText <- optionalText row "gaps" ""
  Right SuiteSpec
    { suiteName = name
    , suiteNote = note
    , suiteImage = image
    , suiteBound = bound
    , suiteCasesPath = cases
    , suiteCasesSource = casesSource
    , suiteCasesDerivedAt = if null casesDerivedAt then Nothing else Just casesDerivedAt
    , suiteCasesVerified = casesVerified
    , suiteGapsPath = if null gapsText then Nothing else Just gapsText
    }
parseSuite _ = Left "suite row must be an object"

parseImage :: KM.KeyMap Value -> Either String ImageSpec
parseImage row = do
  path <- requireText row "path"
  base <- requireAddress row "base"
  digestText <- optionalText row "sha256" ""
  digest <- if null digestText then Right Nothing else Just . sha256Hex <$> either Left Right (parseSha256Hex digestText)
  Right (ImageSpec path base digest)

parseBound :: String -> KM.KeyMap Value -> Either String BoundSpec
parseBound suite row = do
  kindText <- requireText row "kind"
  kind <- case kindText of
    "linker-memory" -> Right BoundLinkerMemory
    "linker-assert" -> Right BoundLinkerAsserts
    "carriers" -> Right BoundCarriers
    "declared" -> Right BoundDeclared
    _ -> Left (suite ++ ": unknown bound kind " ++ kindText)
  source <- requireText row "source"
  include <- optionalStrings row "include"
  exclude <- optionalStrings row "exclude"
  domainText <- optionalText row "domain" ""
  groups <- optionalStrings row "groups"
  derivedAt <- optionalText row "derived_at" ""
  verified <- optionalBool row "verified" False
  declared <- case kind of
    BoundDeclared -> case KM.lookup "entries" row of
      Just (Array entries) -> mapM parseDeclared (V.toList entries)
      _ -> Left (suite ++ ": declared bound needs entries")
    _ -> Right []
  Right (BoundSpec kind source include exclude (if null domainText then Nothing else Just domainText) groups declared (if null derivedAt then Nothing else Just derivedAt) verified)
  where
    parseDeclared (Object entry) = do
      address <- requireAddress entry "address"
      size <- requireAddress entry "size"
      name <- optionalText entry "name" (word32Hex address)
      Right (address, size, name)
    parseDeclared _ = Left (suite ++ ": declared entry must be an object")

parseCaseTable :: String -> B.ByteString -> Either String [CaseRow]
parseCaseTable suite bytes = do
  value <- eitherDecodeStrict' bytes
  case value of
    Object root -> do
      schema <- requireInt root "schema"
      unless (schema == (1 :: Int)) (Left (suite ++ ": case table schema must be 1"))
      let named = optionalText root "suite" ""
      tableSuite <- named
      unless (null tableSuite || tableSuite == suite)
        (Left (suite ++ ": case table names " ++ tableSuite))
      case KM.lookup "entries" root of
        Just (Array rows) -> mapM parseRow (V.toList rows)
        _ -> Left (suite ++ ": case table needs an entries array")
    _ -> Left (suite ++ ": case table must be an object")
  where
    parseRow (Object row) = do
      name <- optionalText row "case" ""
      entry <- requireAddress row "entry"
      extent <- case KM.lookup "extent" row of
        Nothing -> Right 4
        Just value -> maybe (Left (suite ++ ": case extent is not an address")) Right (parseAddress value)
      stock <- case KM.lookup "stock" row of
        Nothing -> Right StockImage
        Just (String "image") -> Right StockImage
        Just (Object stockRow) -> case (KM.lookup "sha256" stockRow, KM.lookup "hex" stockRow) of
          (Just (String digest), _) -> StockDigest . sha256Hex <$> either Left Right (parseSha256Hex (T.unpack digest))
          (_, Just (String hex)) -> StockLiteral <$> hexBytes (suite ++ ": stock hex") (T.unpack hex)
          _ -> Left (suite ++ ": stock object needs sha256 or hex")
        Just _ -> Left (suite ++ ": stock must be \"image\" or an object")
      Right (CaseRow name entry extent stock)
    parseRow _ = Left (suite ++ ": case row must be an object")

parseGaps :: B.ByteString -> Either String [Gap]
parseGaps bytes = do
  value <- eitherDecodeStrict' bytes
  case value of
    Object root -> case KM.lookup "gaps" root of
      Just (Array rows) -> mapM parseGap (V.toList rows)
      Nothing -> Right []
      _ -> Left "gaps must be an array"
    _ -> Left "gap file must be an object"
  where
    parseGap (Object row) = do
      entry <- requireAddress row "entry"
      kindText <- requireText row "kind"
      kind <- case kindText of
        "missing" -> Right GapMissing
        "stock" -> Right GapStock
        _ -> Left ("unknown gap kind " ++ kindText)
      reason <- requireText row "reason"
      Right (Gap entry kind reason)
    parseGap _ = Left "gap row must be an object"

hexBytes :: String -> String -> Either String B.ByteString
hexBytes label text
  | odd (length text) = Left (label ++ ": odd hex length")
  | not (all isHexDigit text) = Left (label ++ ": non-hex digit")
  | otherwise = Right (B.pack (pairs text))
  where
    pairs [] = []
    pairs (high : low : rest) = fromIntegral (digitToInt high * 16 + digitToInt low) : pairs rest
    pairs _ = []

-- Small JSON helpers ------------------------------------------------------

requireText :: KM.KeyMap Value -> String -> Either String String
requireText row key = case KM.lookup (K.fromString key) row of
  Just (String text) -> Right (T.unpack text)
  Just _ -> Left ("field " ++ key ++ " must be a string")
  Nothing -> Left ("missing field " ++ key)

optionalText :: KM.KeyMap Value -> String -> String -> Either String String
optionalText row key fallback = case KM.lookup (K.fromString key) row of
  Nothing -> Right fallback
  Just (String text) -> Right (T.unpack text)
  Just _ -> Left ("field " ++ key ++ " must be a string")

optionalBool :: KM.KeyMap Value -> String -> Bool -> Either String Bool
optionalBool row key fallback = case KM.lookup (K.fromString key) row of
  Nothing -> Right fallback
  Just (Bool value) -> Right value
  Just _ -> Left ("field " ++ key ++ " must be a boolean")

requireInt :: KM.KeyMap Value -> String -> Either String Int
requireInt row key = case KM.lookup (K.fromString key) row of
  Just (Number value) -> maybe (Left ("field " ++ key ++ " must be an integer")) Right (toBoundedInteger value)
  _ -> Left ("missing integer field " ++ key)

requireAddress :: KM.KeyMap Value -> String -> Either String Word32
requireAddress row key = case KM.lookup (K.fromString key) row of
  Just value -> maybe (Left ("field " ++ key ++ " is not a 32-bit address")) Right (parseAddress value)
  Nothing -> Left ("missing field " ++ key)

optionalStrings :: KM.KeyMap Value -> String -> Either String [String]
optionalStrings row key = case KM.lookup (K.fromString key) row of
  Nothing -> Right []
  Just (Array values) -> mapM asText (V.toList values)
  _ -> Left ("field " ++ key ++ " must be an array of strings")
  where
    asText (String text) = Right (T.unpack text)
    asText _ = Left ("field " ++ key ++ " must contain strings")

-- Bound entries -----------------------------------------------------------

-- | Read the entry rows the suite claims, from the declared source. The
-- source kind decides the parser; the result is sorted and deduplicated by
-- address.
readBoundEntries :: FilePath -> BoundSpec -> IO (Either String [MissingEntry])
readBoundEntries root spec
  | boundKind spec == BoundDeclared =
      pure (Right (map (\(address, size, name) -> MissingEntry address size name) (boundDeclared spec)))
  | otherwise = do
      bytes <- B.readFile (root ++ "/" ++ boundSource spec)
      pure $ case boundKind spec of
        BoundLinkerMemory -> parseLinkerMemory spec (BC.unpack bytes)
        BoundLinkerAsserts -> parseLinkerAsserts spec (BC.unpack bytes)
        BoundCarriers -> parseCarriers spec bytes
        BoundDeclared -> Right []

parseLinkerMemory :: BoundSpec -> String -> Either String [MissingEntry]
parseLinkerMemory spec text =
  let rows = mapMaybe memoryRow (lines text)
      selected = filter (selectedName spec . (\(_, _, name) -> name)) rows
   in Right (dedupe (map (\(address, size, name) -> MissingEntry address size name) selected))

memoryRow :: String -> Maybe (Word32, Word32, String)
memoryRow line =
  let trimmed = dropWhile (== ' ') line
      (name, rest) = span (\c -> c /= ' ' && c /= '\t') trimmed
      afterName = dropWhile (`elem` (" \t" :: String)) rest
   in if null name || take 2 afterName /= "(r" then Nothing else
        case (field "ORIGIN" afterName, field "LENGTH" afterName) of
          (Just origin, Just size) -> do
            address <- parseWord32 origin
            extent <- parseWord32 size
            pure (address, extent, name)
          _ -> Nothing
  where
    field key value = case breakOn (key ++ " = ") value of
      Just rest -> Just (takeWhile (\c -> c /= ',' && c /= ')' && c /= ' ') rest)
      Nothing -> Nothing

breakOn :: String -> String -> Maybe String
breakOn needle haystack = case find (prefixOf needle) (tails haystack) of
  Just match -> Just (drop (length needle) match)
  Nothing -> Nothing
  where
    prefixOf prefix candidate = take (length prefix) candidate == prefix
    tails [] = [[]]
    tails current@(_ : rest) = current : tails rest

parseLinkerAsserts :: BoundSpec -> String -> Either String [MissingEntry]
parseLinkerAsserts spec text =
  let rows = mapMaybe assertRow (lines text)
      selected = filter (selectedName spec . (\(_, _, name) -> name)) rows
   in Right (dedupe (map (\(address, size, name) -> MissingEntry address size name) selected))
  where
    assertRow line =
      case words (map (\c -> if c `elem` ("()," :: String) then ' ' else c) line) of
        ("ASSERT" : symbol : "==" : "ORIGIN" : region : _) ->
          case lookup region memory of
            Just (address, size) -> Just (address, size, symbol)
            Nothing -> Nothing
        ("ASSERT" : symbol : "==" : literal : _) ->
          case parseWord32 literal of
            Just address -> Just (address, 4, symbol)
            Nothing -> Nothing
        _ -> Nothing
    memory = [(name, (address, size)) | (address, size, name) <- mapMaybe memoryRow (lines text)]

parseCarriers :: BoundSpec -> B.ByteString -> Either String [MissingEntry]
parseCarriers spec bytes = do
  value <- eitherDecodeStrict' bytes
  domain <- maybe (Left "carriers bound needs a domain") Right (boundDomain spec)
  let rows = get (get (get value "domains") domain) "entries"
      entries = [ (address, 4, name)
                | row <- list rows
                , Just address <- [parseAddress (get row "address")]
                , let name = str (get row "symbol")
                , let group = str (get row "group")
                , null (boundGroups spec) || group `elem` boundGroups spec
                ]
      selected = filter (selectedName spec . (\(_, _, name) -> name)) entries
  pure (dedupe (map (\(address, size, name) -> MissingEntry address size name) selected))

selectedName :: BoundSpec -> String -> Bool
selectedName spec name =
  (null (boundInclude spec) || any (`isInfix` name) (boundInclude spec))
    && not (any (`isInfix` name) (boundExclude spec))
  where
    isInfix needle haystack = any (prefixOf needle) (tails haystack)
    prefixOf needle haystack = take (length needle) haystack == needle
    tails [] = [[]]
    tails current@(_ : rest) = current : tails rest

dedupe :: [MissingEntry] -> [MissingEntry]
dedupe = nubBy (\a b -> missingAddress a == missingAddress b) . sortOn missingAddress

-- Audit -------------------------------------------------------------------

-- | Apply the one rule to one suite. The firmware bytes are optional; when
-- absent, byte authentication is @not run@.
auditOneSuite :: Maybe (FilePath, B.ByteString) -> SuiteSpec -> [MissingEntry] -> [CaseRow] -> [Gap] -> SuiteAudit
auditOneSuite firmware spec bound rows gaps =
  SuiteAudit
    { auditedSuite = suiteName spec
    , auditBoundSource = boundSource (suiteBound spec)
    , auditCasesPath = suiteCasesPath spec
    , auditCasesSource = suiteCasesSource spec
    , auditBoundLive = boundKind (suiteBound spec) /= BoundDeclared
    , auditBoundDerivedAt = boundDerivedAt (suiteBound spec)
    , auditBoundVerified = boundVerified (suiteBound spec)
    , auditCasesDerivedAt = suiteCasesDerivedAt spec
    , auditCasesVerified = suiteCasesVerified spec
    , auditBound = bound
    , auditEntered = entered
    , auditEnteredBound = [address | address <- entered, address `elem` boundAddresses]
    , auditMissing = missing
    , auditUndeclaredMissing = [entry | entry <- missing, not (declared GapMissing (missingAddress entry))]
    , auditUnauthenticated = unauth
    , auditUndeclaredUnauthenticated = [bad | bad <- unauth, not (declared GapStock (unauthAddress bad))]
    , auditDeclaredGaps = [gap | gap <- gaps, declared (gapKind gap) (gapEntry gap)]
    , auditAuthStatus = authStatus
    , auditStatus = status
    , auditNote = suiteNote spec
    , auditFirmware = fmap (\(path, bytes) -> (path, sha256Hex (sha256 bytes))) firmware
    }
  where
    entered = nubBy (==) (sortOn id (map caseEntry rows))
    boundAddresses = map missingAddress bound
    missing = [entry | entry <- bound, missingAddress entry `notElem` entered]
    declared kind address = any (\gap -> gapKind gap == kind && gapEntry gap == address) gaps
    (authStatus, unauth) = case (firmware, suiteImage spec) of
      (Just (_, bytes), Just image) -> (AuthRun, concatMap (authenticate bytes image) rows)
      _ -> (AuthNotRun, [])
    status
      | not (null [entry | entry <- missing, not (declared GapMissing (missingAddress entry))]) = AuditFailed
      | not (null [bad | bad <- unauth, not (declared GapStock (unauthAddress bad))]) = AuditFailed
      | authStatus == AuthNotRun = AuditNotRun
      | otherwise = AuditPassed

authenticate :: B.ByteString -> ImageSpec -> CaseRow -> [Unauthenticated]
authenticate bytes image row =
  case sliceFirmware bytes image (caseEntry row) (caseExtent row) of
    Nothing ->
      [ Unauthenticated (caseEntry row) (caseName row) "outside the firmware image" "inside the firmware image" ]
    Just actual -> case caseStock row of
      StockImage -> []
      StockDigest expected
        | sha256Hex (sha256 actual) == expected -> []
        | otherwise -> [ Unauthenticated (caseEntry row) (caseName row) (sha256Hex (sha256 actual)) expected ]
      StockLiteral expected
        | actual == expected -> []
        | otherwise -> [ Unauthenticated (caseEntry row) (caseName row) (hexPrefix actual) (hexPrefix expected) ]

sliceFirmware :: B.ByteString -> ImageSpec -> Word32 -> Word32 -> Maybe B.ByteString
sliceFirmware bytes image address size =
  let offset = toInteger address - toInteger (imageBase image)
   in if offset < 0 || offset + toInteger size > toInteger (B.length bytes) then Nothing
        else Just (B.take (fromIntegral size) (B.drop (fromIntegral offset) bytes))

hexPrefix :: B.ByteString -> String
hexPrefix = concatMap byteHex . B.unpack . B.take 16
  where
    byteHex byte = let text = showHex byte "" in replicate (2 - length text) '0' ++ text

-- Output ------------------------------------------------------------------

auditTable :: [SuiteAudit] -> String
auditTable audits = unlines (header : map row audits)
  where
    header = pad 44 "suite" ++ pad 7 "bound" ++ pad 9 "entered" ++ pad 9 "entered*" ++ pad 9 "missing" ++ pad 9 "declared" ++ pad 13 "unauth" ++ pad 14 "bound-side" ++ pad 14 "cases-side" ++ "status"
    row audit =
      pad 44 (auditedSuite audit)
        ++ pad 7 (show (length (auditBound audit)))
        ++ pad 9 (show (length (auditEntered audit)))
        ++ pad 9 (show (length (auditEnteredBound audit)))
        ++ pad 9 (show (length (auditUndeclaredMissing audit)))
        ++ pad 9 (show (length (auditDeclaredGaps audit)))
        ++ pad 13 (show (length (auditUndeclaredUnauthenticated audit)))
        ++ pad 14 (boundSide audit)
        ++ pad 14 (casesSide audit)
        ++ statusText (auditStatus audit)
    boundSide audit
      | auditBoundLive audit = "live"
      | auditBoundVerified audit = "snap-tested"
      | otherwise = "snap-untested"
    casesSide audit
      | auditCasesVerified audit = "snap-tested"
      | otherwise = "snap-untested"
    statusText AuditPassed = "pass"
    statusText AuditFailed = "FAIL"
    statusText AuditNotRun = "not run"
    pad width text = take width (text ++ repeat ' ')

auditsJson :: [SuiteAudit] -> Value
auditsJson audits = object
  [ ("schema", Number 1)
  , ("suites", Array (V.fromList (map auditJson audits)))
  ]
  where
    text = T.pack
    address value = String (text (word32Hex value))
    auditJson audit = object
      [ ("suite", String (text (auditedSuite audit)))
      , ("bound_source", String (text (auditBoundSource audit)))
      , ("bound_side", String (text (if auditBoundLive audit then "live" else if auditBoundVerified audit then "snapshot-tested" else "snapshot-untested")))
      , ("bound_derived_at", maybe Null (String . text) (auditBoundDerivedAt audit))
      , ("cases_source", String (text (auditCasesSource audit)))
      , ("cases_path", String (text (auditCasesPath audit)))
      , ("cases_side", String (text (if auditCasesVerified audit then "snapshot-tested" else "snapshot-untested")))
      , ("cases_derived_at", maybe Null (String . text) (auditCasesDerivedAt audit))
      , ("bound", toJSON (length (auditBound audit)))
      , ("entered", toJSON (length (auditEntered audit)))
      , ("entered_bound", toJSON (length (auditEnteredBound audit)))
      , ("missing", toJSON
          [ object [("address", address (missingAddress entry)), ("name", String (text (missingName entry)))]
          | entry <- auditUndeclaredMissing audit ])
      , ("declared_missing", toJSON
          [ object [("address", address (missingAddress entry)), ("name", String (text (missingName entry))), ("reason", String (text (gapReason gap)))]
          | entry <- auditMissing audit
          , gap <- auditDeclaredGaps audit
          , gapEntry gap == missingAddress entry
          , gapKind gap == GapMissing ])
      , ("declared_gaps", toJSON
          [ object [("address", address (gapEntry gap)), ("kind", String (text (case gapKind gap of GapMissing -> "missing"; GapStock -> "stock"))), ("reason", String (text (gapReason gap)))]
          | gap <- auditDeclaredGaps audit ])
      , ("unauthenticated", toJSON
          [ object [("address", address (unauthAddress bad)), ("case", String (text (unauthCase bad))), ("mapped", String (text (unauthMapped bad))), ("expected", String (text (unauthExpected bad)))]
          | bad <- auditUndeclaredUnauthenticated audit ])
      , ("firmware", case auditFirmware audit of
          Nothing -> object [("present", Bool False)]
          Just (path, digest) -> object [("present", Bool True), ("path", String (text path)), ("sha256", String (text digest))])
      , ("authentication", String (text (case auditAuthStatus audit of AuthRun -> "run"; AuthNotRun -> "not-run")))
      , ("status", String (text (case auditStatus audit of AuditPassed -> "passed"; AuditFailed -> "failed"; AuditNotRun -> "not-run")))
      , ("note", String (text (auditNote audit)))
      ]
