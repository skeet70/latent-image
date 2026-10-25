-- | recipe-bank-text: imports shareable text recipes into the bank file and
-- exports occupied slots back to the same text (issue #76; the README
-- section "Share recipes as text" documents the format). Host-only: the user
-- copies the generated RECIPEA.BIN to the card, and no camera operation is
-- involved.
--
--   recipe-bank-text import --image4 PATH --image-size ROW BASE OUT_DIR RECIPE...
--   recipe-bank-text export --image4 PATH --image-size ROW BASE BANK OUT_DIR
--
-- IMAGE4 is the pinned image-4 decode whose conversion tables define the
-- payload-safety rules the camera's storage kernel applies. ROW is the IMAGE
-- SIZE menu row every slot carries (an offered row such as 1 for L 3:2): the
-- kernel refuses a bank whose slots carry a size/aspect pair no menu offers,
-- so the row is explicit and replaces the base's own pair. BASE is the
-- declared payload base, given as the 80 hex characters of one 0x28-byte
-- payload or as a path to the file that holds it. Every field a recipe does
-- not spell takes its stored code from BASE, and the report names those
-- fields.
module Main where

import Control.Monad (forM_, unless, when)
import qualified Data.ByteString as B
import Data.Char (isHexDigit, toLower)
import Data.Either (isLeft)
import Data.List (intercalate)
import Data.Word (Word32, Word8)
import System.Directory (createDirectoryIfMissing, doesFileExist)
import System.Environment (getArgs)
import System.Exit (die)
import System.FilePath ((</>), takeBaseName)

import FujiAF.RecipeBank
import FujiAF.RecipeText

data Settings = Settings
  { settingImage4 :: FilePath
  , settingImage4Base :: Word32
  , settingImageSize :: Int
  }

usage :: String
usage =
  unlines
    [ "usage: recipe-bank-text import --image4 PATH --image-size ROW BASE OUT_DIR RECIPE..."
    , "       recipe-bank-text export --image4 PATH --image-size ROW BASE BANK OUT_DIR"
    , "  --image4 PATH      the pinned image-4 decode whose tables define the payload-safety rules"
    , "  --image-size ROW   the offered IMAGE SIZE menu row every slot carries (1 is L 3:2)"
    , "  --image4-base HEX  the decode's load address (default 0x0061c400)"
    ]

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ("import" : rest) -> do
      (settings, paths) <- parseSettings rest
      case paths of
        (baseArg : outDir : recipePaths@(_ : _)) -> importRun settings baseArg outDir recipePaths
        _ -> die usage
    ("export" : rest) -> do
      (settings, paths) <- parseSettings rest
      case paths of
        [baseArg, bankPath, outDir] -> exportRun settings baseArg bankPath outDir
        _ -> die usage
    _ -> die usage

parseSettings :: [String] -> IO (Settings, [String])
parseSettings = go Nothing 0x0061c400 Nothing
 where
  go _ base row ("--image4" : value : rest) = go (Just value) base row rest
  go image4 _ row ("--image4-base" : value : rest) = do
    parsed <- either die pure (hex32 value)
    go image4 parsed row rest
  go image4 base _ ("--image-size" : value : rest) = go image4 base (Just value) rest
  go image4 base row rest = do
    path <- maybe (die usage) pure image4
    rowText <- maybe (die usage) pure row
    size <- either die pure (imageSizeRow rowText)
    pure (Settings path base size, rest)

imageSizeRow :: String -> Either String Int
imageSizeRow = number

number :: String -> Either String Int
number text = case text of
  ('0' : 'x' : digits) | not (null digits) && all isHexDigit digits -> pure (digitsValue 16 digits)
  _ | not (null text) && all isDigit' text -> pure (digitsValue 10 text)
  _ -> Left ("bad number " ++ show text)
 where
  digitsValue baseValue = foldl (\acc c -> acc * baseValue + digit c) 0
  digit c
    | isDigit' c = fromEnum c - fromEnum '0'
    | toLower c >= 'a' && toLower c <= 'f' = 10 + fromEnum (toLower c) - fromEnum 'a'
    | otherwise = 0

isDigit' :: Char -> Bool
isDigit' c = c >= '0' && c <= '9'

hex32 :: String -> Either String Word32
hex32 text
  | length text > 8 || null text || not (all isHexDigit text) = Left ("bad hex number " ++ show text)
  | otherwise = Right (foldl (\acc c -> acc * 16 + fromIntegral (digit c)) 0 text)
 where
  digit c
    | isDigit' c = fromEnum c - fromEnum '0'
    | otherwise = 10 + fromEnum (toLower c) - fromEnum 'a'

data Imported = Imported
  { importedName :: SlotName
  , importedPayload :: BankPayload
  , importedFromBase :: [String]
  }

loadDomain :: Settings -> IO PayloadDomain
loadDomain settings = do
  image <- B.readFile (settingImage4 settings)
  either die pure (payloadDomainFromImage4 (settingImage4Base settings) image)

preparedBaseFor :: Settings -> PayloadDomain -> FilePath -> IO B.ByteString
preparedBaseFor settings domain baseArg = do
  raw <- loadBase baseArg
  either die pure (preparedBase domain (settingImageSize settings) raw)

importRun :: Settings -> FilePath -> FilePath -> [FilePath] -> IO ()
importRun settings baseArg outDir recipePaths = do
  when (length recipePaths > bankCapacity) $
    die ("import: the bank holds " ++ show bankCapacity ++ " slots, got " ++ show (length recipePaths) ++ " recipes")
  domain <- loadDomain settings
  base <- preparedBaseFor settings domain baseArg
  results <- mapM (importOne domain base) recipePaths
  let slots = scanl nextSlot 1 results
  mapM_ putStrLn (zipWith3 reportImport recipePaths slots results)
  let accepted = [imported | Right imported <- results]
  when (null accepted) $ die "import: no recipe was imported"
  records <-
    either die pure $
      bankRecordsFromList (map recordOf accepted ++ replicate (bankCapacity - length accepted) Unpopulated)
  file <-
    either die pure $
      encodeBankFileFor ClassicChromeFilmEncoding (BankState (BankGeneration 1) NothingSelected Disabled BaseInvalid records)
  createDirectoryIfMissing True outDir
  B.writeFile (outDir </> bankFileA) file
 where
  nextSlot slot result = either (const slot) (const (slot + 1)) result
  recordOf imported = Populated (importedName imported) (importedPayload imported) (importedPayload imported)

reportImport :: FilePath -> Int -> Either String Imported -> String
reportImport path slot result = case result of
  Left reason -> "REJECTED " ++ path ++ ": " ++ reason
  Right imported ->
    "IMPORTED C"
      ++ show slot
      ++ " from "
      ++ path
      ++ " as "
      ++ show (slotNameText (importedName imported))
      ++ "; from base: "
      ++ namesOrNone (importedFromBase imported)

importOne :: PayloadDomain -> B.ByteString -> FilePath -> IO (Either String Imported)
importOne domain base path = do
  contents <- readFile path
  pure $ do
    recipe <- parseRecipeText (takeBaseName path) contents
    (cooked, fromBase) <- fillRecipe base recipe
    payload <- encodeSlotPayloadOver base cooked >>= safePayload domain
    pure (Imported (textName recipe) payload fromBase)

exportRun :: Settings -> FilePath -> FilePath -> FilePath -> IO ()
exportRun settings baseArg bankPath outDir = do
  domain <- loadDomain settings
  base <- preparedBaseFor settings domain baseArg
  file <- B.readFile bankPath
  decoded <- either die pure (decodeBankFile file)
  when (decodedFilmEncoding decoded /= ClassicChromeFilmEncoding) $
    die "export: legacy D3 bank encoding; Classic Chrome banks use format version 3"
  let state = decodedState decoded
      occupied =
        [ (k, name, bankPayloadContents saved)
        | (k, Populated name saved _) <- zip [1 :: Int ..] (bankRecordList (bankRecords state))
        ]
  when (null occupied) $ die ("export: " ++ bankPath ++ " has no occupied slot")
  createDirectoryIfMissing True outDir
  let results = [(k, name, checked name saved) | (k, name, saved) <- occupied]
      checked name saved = do
        payload <- safePayload domain saved
        exportRecipeText base name (bankPayloadContents payload)
  forM_ results $ \(k, name, result) -> case result of
    Left reason -> putStrLn ("SKIPPED C" ++ show k ++ ": " ++ reason)
    Right (text, fromBase) -> do
      let fileName = "C" ++ show k ++ ".txt"
      writeFile (outDir </> fileName) text
      putStrLn
        ( "EXPORTED C"
            ++ show k
            ++ " to "
            ++ fileName
            ++ " as "
            ++ show (slotNameText name)
            ++ "; from base: "
            ++ namesOrNone fromBase
        )
  when (all (\(_, _, result) -> isLeft result) results) $
    die "export: no slot could be exported"

namesOrNone :: [String] -> String
namesOrNone [] = "none"
namesOrNone names = intercalate ", " names

-- | Reads BASE: 80 hex characters, or a file holding one 0x28-byte payload.
loadBase :: FilePath -> IO B.ByteString
loadBase argument = do
  raw <-
    if length argument == 2 * payloadBytes && all isHexDigit argument
      then pure (B.pack (hexBytes argument))
      else do
        exists <- doesFileExist argument
        unless exists $
          die ("BASE " ++ show argument ++ " is neither " ++ show (2 * payloadBytes) ++ " hex characters nor a file")
        B.readFile argument
  when (B.length raw /= payloadBytes) $
    die ("BASE must be one " ++ show payloadBytes ++ "-byte payload, got " ++ show (B.length raw) ++ " bytes")
  pure raw

hexBytes :: String -> [Word8]
hexBytes (high : low : rest) = fromIntegral (hexDigit high * 16 + hexDigit low) : hexBytes rest
hexBytes _ = []

hexDigit :: Char -> Int
hexDigit c
  | isDigit' c = fromEnum c - fromEnum '0'
  | c >= 'A' && c <= 'F' = 10 + fromEnum c - fromEnum 'A'
  | otherwise = 10 + fromEnum c - fromEnum 'a'
