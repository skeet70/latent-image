-- | recipe-bank-text: imports shareable text recipes into the bank file and
-- exports occupied slots back to the same text (issue #76; the README
-- section "Share recipes as text" documents the format). Host-only: the user
-- copies the generated RECIPEA.BIN to the card, and no camera operation is
-- involved.
--
--   recipe-bank-text import BASE OUT_DIR RECIPE...
--   recipe-bank-text export BASE BANK OUT_DIR
--
-- BASE is the declared payload base, given as the 80 hex characters of one
-- 0x28-byte payload or as a path to the file that holds it. Every field a
-- recipe does not spell takes its stored code from BASE, and the report names
-- those fields.
module Main where

import Control.Monad (forM_, unless, when)
import qualified Data.ByteString as B
import Data.Char (isHexDigit)
import Data.Either (isLeft)
import Data.List (intercalate)
import Data.Word (Word8)
import System.Directory (createDirectoryIfMissing, doesFileExist)
import System.Environment (getArgs)
import System.Exit (die)
import System.FilePath ((</>), takeBaseName)

import FujiAF.RecipeBank
import FujiAF.RecipeText

usage :: String
usage =
  unlines
    [ "usage: recipe-bank-text import BASE OUT_DIR RECIPE..."
    , "       recipe-bank-text export BASE BANK OUT_DIR"
    ]

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ("import" : baseArg : outDir : recipePaths@(_ : _)) -> importRun baseArg outDir recipePaths
    ["export", baseArg, bankPath, outDir] -> exportRun baseArg bankPath outDir
    _ -> die usage

data Imported = Imported
  { importedName :: SlotName
  , importedPayload :: BankPayload
  , importedFromBase :: [String]
  }

importRun :: FilePath -> FilePath -> [FilePath] -> IO ()
importRun baseArg outDir recipePaths = do
  when (length recipePaths > bankCapacity) $
    die ("import: the bank holds " ++ show bankCapacity ++ " slots, got " ++ show (length recipePaths) ++ " recipes")
  base <- loadBase baseArg
  results <- mapM (importOne base) recipePaths
  let slots = scanl nextSlot 1 results
  mapM_ putStrLn (zipWith3 reportImport recipePaths slots results)
  let accepted = [imported | Right imported <- results]
  when (null accepted) $ die "import: no recipe was imported"
  records <-
    either die pure $
      bankRecordsFromList (map recordOf accepted ++ replicate (bankCapacity - length accepted) Unpopulated)
  file <-
    either die pure $
      encodeBankFile (BankState (BankGeneration 1) NothingSelected Disabled BaseInvalid records)
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

importOne :: B.ByteString -> FilePath -> IO (Either String Imported)
importOne base path = do
  contents <- readFile path
  pure $ do
    recipe <- parseRecipeText (takeBaseName path) contents
    (cooked, fromBase) <- fillRecipe base recipe
    payload <- encodeSlotPayloadOver base cooked >>= canonicalBankPayload
    pure (Imported (textName recipe) payload fromBase)

exportRun :: FilePath -> FilePath -> FilePath -> IO ()
exportRun baseArg bankPath outDir = do
  base <- loadBase baseArg
  file <- B.readFile bankPath
  decoded <- either die pure (decodeBankFile file)
  let state = decodedState decoded
      occupied =
        [ (k, name, saved)
        | (k, Populated name saved _) <- zip [1 :: Int ..] (bankRecordList (bankRecords state))
        ]
  when (null occupied) $ die ("export: " ++ bankPath ++ " has no occupied slot")
  createDirectoryIfMissing True outDir
  let results = [(k, name, exportRecipeText base name (bankPayloadContents saved)) | (k, name, saved) <- occupied]
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

-- | Reads BASE: 80 hex characters, or a file holding one 0x28-byte payload,
-- in the canonical form every stored payload has.
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
  either die (pure . bankPayloadContents) (canonicalBankPayload raw)

hexBytes :: String -> [Word8]
hexBytes (high : low : rest) = fromIntegral (hexDigit high * 16 + hexDigit low) : hexBytes rest
hexBytes _ = []

hexDigit :: Char -> Int
hexDigit c
  | c >= '0' && c <= '9' = fromEnum c - fromEnum '0'
  | c >= 'A' && c <= 'F' = 10 + fromEnum c - fromEnum 'A'
  | otherwise = 10 + fromEnum c - fromEnum 'a'
