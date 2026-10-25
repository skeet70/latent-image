-- | Behavioural checks for the shareable text recipe format and its CLI: the
-- round trip, every rejection the parser and the exporter define, the two
-- compiled recipes reports/recipe-patch-d3.md section 6.1 documents, and the
-- mixed-set acceptance of issue #76.
module Main where

import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import Data.List (isInfixOf)
import Data.Word (Word16, Word8)
import System.Directory (createDirectory, getTemporaryDirectory, makeAbsolute, removeDirectoryRecursive, removeFile)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>))
import System.IO (hClose, openTempFile)
import System.Process (CreateProcess (..), proc, readCreateProcessWithExitCode)

import FujiAF.RecipeBank
import FujiAF.RecipeText

check :: String -> Bool -> IO ()
check label passed = unless passed (putStrLn ("FAIL: " ++ label) >> exitFailure)

fixture :: Either String a -> a
fixture = either (error . ("invalid fixture: " ++)) id

-- | A declared payload base for the checks: every byte 0x11 in its canonical
-- form. Its nine field codes are all values the format spells (film ASTIA,
-- dynamic range AUTO, the three tone fields HARD, colour MID, sharpness HARD,
-- noise reduction LOW, white balance FINE, ISO 1600), and they differ from the
-- recipes below, so a spelled field appears in the exported text.
testBase :: B.ByteString
testBase = bankPayloadContents (fixture (canonicalBankPayload (B.replicate payloadBytes 0x11)))

zeroBase :: B.ByteString
zeroBase = B.replicate payloadBytes 0

-- | The payload one recipe produces, with the declared base filled in.
payloadOf :: B.ByteString -> TextRecipe -> B.ByteString
payloadOf base recipe =
  fixture (encodeSlotPayloadOver base (fst (fixture (fillRecipe base recipe))))

recipeOf :: [(Word16, Word8)] -> Recipe
recipeOf pairs = fixture (recipeFromPairs [(property, StoredCode code) | (property, code) <- pairs])

-- recipe-patch-d3.md section 6.1, recipe A: WARM STANDARD. The report's table
-- is byte-read as rows and stored codes.
warmStandardText :: String
warmStandardText =
  unlines
    [ "Film Simulation: PROVIA/STANDARD"
    , "Dynamic Range: 200"
    , "Highlight Tone: +1"
    , "Shadow Tone: -1"
    , "Color: +2"
    , "Sharpness: 0"
    , "Noise Reduction: -1"
    , "White Balance: AUTO"
    , "ISO: 400"
    ]

warmStandardCodes :: [(Word16, Word8)]
warmStandardCodes =
  [ (0xfe, 0x00)
  , (0x104, 0x02)
  , (0x110, 0x01)
  , (0x111, 0x03)
  , (0x10e, 0x03)
  , (0x10f, 0x02)
  , (0xef, 0x01)
  , (0xed, 0x00)
  , (0x141, 0x0b)
  ]

-- recipe-patch-d3.md section 6.1, recipe B: the second Velvia entry.
secondVelviaText :: String
secondVelviaText =
  unlines
    [ "Film Simulation: Velvia/VIVID"
    , "Dynamic Range: 100"
    , "Highlight Tone: 0"
    , "Shadow Tone: 0"
    , "Color: +1"
    , "Sharpness: -1"
    , "Noise Reduction: -2"
    , "White Balance: AUTO"
    , "ISO: 200"
    ]

secondVelviaCodes :: [(Word16, Word8)]
secondVelviaCodes =
  [ (0xfe, 0x03)
  , (0x104, 0x01)
  , (0x110, 0x02)
  , (0x111, 0x02)
  , (0x10e, 0x04)
  , (0x10f, 0x03)
  , (0xef, 0x00)
  , (0xed, 0x00)
  , (0x141, 0x08)
  ]

rejectedWith :: String -> Either String a -> Bool
rejectedWith needle result = case result of
  Left reason -> needle `isInfixOf` reason
  Right _ -> False

main :: IO ()
main = do
  [binaryArg] <- getArgs
  realRecipeChecks
  roundTripChecks
  rejectionChecks
  exportRejectionChecks
  binary <- makeAbsolute binaryArg
  cliChecks binary
  putStrLn "recipe-text: checks passed"

realRecipeChecks :: IO ()
realRecipeChecks = do
  let warm = fixture (parseRecipeText "warmstandard" warmStandardText)
      second = fixture (parseRecipeText "secondvelvia" secondVelviaText)
  check
    "WARM STANDARD takes the file name as its slot name"
    (textName warm == fixture (slotName "warmstandard"))
  check
    "WARM STANDARD imports to the report's stored codes"
    (fillRecipe testBase warm == Right (recipeOf warmStandardCodes, []))
  check
    "the second Velvia entry imports to the report's stored codes"
    (fillRecipe testBase second == Right (recipeOf secondVelviaCodes, []))
  check
    "WARM STANDARD exports as the same text"
    ( exportRecipeText testBase (textName warm) (payloadOf testBase warm)
        == Right ("Name: warmstandard\n" ++ warmStandardText, [])
    )
  check
    "the second Velvia entry round-trips byte for byte"
    (reimportedPayload testBase (payloadOf testBase second) == payloadOf testBase second)

-- | Import, export and import a payload over the declared base.
reimportedPayload :: B.ByteString -> B.ByteString -> B.ByteString
reimportedPayload base payload = case exportRecipeText base (fixture (slotName "slot")) payload of
  Left reason -> error reason
  Right (text, _) ->
    payloadOf base (fixture (parseRecipeText "slot" text))

roundTripChecks :: IO ()
roundTripChecks = do
  let partial =
        unlines
          [ "Name: PORTRA 4000"
          , "Film Simulation: Velvia/VIVID"
          , "Highlight Tone: -2"
          , "White Balance: FLUORESCENT LIGHT-1"
          , "ISO: H25600"
          ]
      parsed = fixture (parseRecipeText "fixture" partial)
      omitted = ["Dynamic Range", "Shadow Tone", "Color", "Sharpness", "Noise Reduction"]
      payload = payloadOf testBase parsed
  check "a partial recipe fills the omitted fields from the base" (snd (fixture (fillRecipe testBase parsed)) == omitted)
  check "a partial recipe exports as the same canonical text" (exportRecipeText testBase (textName parsed) payload == Right (partial, omitted))
  check "a partial recipe re-imports byte for byte" (reimportedPayload testBase payload == payload)
  let isoOnly = fixture (parseRecipeText "fixture" "ISO: 6400\n")
  check
    "the base report names every field the text omits"
    ( snd (fixture (fillRecipe testBase isoOnly))
        == ["Film Simulation", "Dynamic Range", "Highlight Tone", "Shadow Tone", "Color", "Sharpness", "Noise Reduction", "White Balance"]
    )
  let spelledBaseField = fixture (parseRecipeText "fixture" "Name: MID\nWhite Balance: AUTO\n")
  check
    "a field spelled with the base's own code is canonicalized out on export"
    ( exportRecipeText zeroBase (textName spelledBaseField) (payloadOf zeroBase spelledBaseField)
        == Right ("Name: MID\n", ["Film Simulation", "Dynamic Range", "Highlight Tone", "Shadow Tone", "Color", "Sharpness", "Noise Reduction", "White Balance", "ISO"])
    )

rejections :: [(String, String, String)]
rejections =
  [ ("unknown field", "Focus Mode: AF-S\n", "unknown field")
  , ("Kelvin white balance", "Kelvin: 5600\n", "is unsupported")
  , ("WB shift", "WB Shift: +2R -5B\n", "is unsupported")
  , ("X70 Auto ISO preset", "auto_iso1_base: 200\n", "is unsupported")
  , ("grain", "Grain: STRONG\n", "is unsupported")
  , ("Color Chrome", "Color Chrome: WEAK\n", "is unsupported")
  , ("clarity", "Clarity: +1\n", "is unsupported")
  , ("exposure compensation", "ev: -1/3\n", "is unsupported")
  , ("unsupported film", "Film Simulation: Classic Chrome\n", "is unsupported for Film Simulation")
  , ("unsupported white balance", "White Balance: DAYLIGHT\n", "is unsupported for White Balance")
  , ("unsupported ISO preset", "ISO: auto1\n", "is unsupported for ISO")
  , ("unknown white balance", "White Balance: SHADEE\n", "is not a White Balance value")
  , ("unknown tone name", "Highlight Tone: MEDIUM\n", "is not a Highlight Tone value")
  , ("out-of-range tone", "Highlight Tone: +3\n", "out of range")
  , ("out-of-range dynamic range", "Dynamic Range: 800\n", "out of range")
  , ("out-of-range ISO", "ISO: 51200\n", "out of range")
  , ("syntax error", "Highlight Tone -1\n", "syntax")
  , ("empty value", "Film Simulation:\n", "empty value")
  , ("duplicate field", "ISO: 200\nISO: 400\n", "duplicate field")
  , ("duplicate name", "Name: A\nName: B\n", "duplicate field")
  , ("name too long", "Name: WARM STANDARD\n", "exceeds")
  ]

rejectionChecks :: IO ()
rejectionChecks =
  forM_ rejections $ \(label, contents, needle) ->
    check ("rejects " ++ label) (rejectedWith needle (parseRecipeText "fixture" contents))

exportRejectionChecks :: IO ()
exportRejectionChecks = do
  let unspelled =
        recipeOf
          [ (0xfe, 0x00)
          , (0x104, 0x00)
          , (0x110, 0x00)
          , (0x111, 0x00)
          , (0x10e, 0x00)
          , (0x10f, 0x00)
          , (0xef, 0x00)
          , (0xed, 0x00)
          , (0x141, 0x1b)
          ]
      unspelledPayload = fixture (encodeSlotPayloadOver testBase unspelled)
  check
    "refuses a stored code with no text spelling"
    ( rejectedWith "has no text spelling" (exportRecipeText testBase (fixture (slotName "slot")) unspelledPayload)
    )
  let warm = fixture (parseRecipeText "warmstandard" warmStandardText)
      payload = payloadOf testBase warm
      poked = B.concat [B.singleton 0xff, B.drop 1 payload]
  check
    "refuses a payload that differs from the base outside the recipe fields"
    ( rejectedWith "differ from the declared base outside the recipe fields" (exportRecipeText testBase (textName warm) poked)
    )

cliChecks :: FilePath -> IO ()
cliChecks binary = do
  temporary <- getTemporaryDirectory
  (root, handle) <- openTempFile temporary "recipe-text-test"
  hClose handle
  removeFile root
  createDirectory root
  let hexBase = concat (replicate payloadBytes "11")
      outDir = root </> "out"
      exportDir = root </> "exported"
      secondDir = root </> "second"
      run arguments = readCreateProcessWithExitCode (proc binary arguments) {cwd = Just root} ""
  writeFile (root </> "warmstandard.txt") warmStandardText
  writeFile (root </> "chrome.txt") "Film Simulation: Classic Chrome\n"
  writeFile (root </> "tone.txt") "Highlight Tone: +3\n"
  (importCode, importOut, importErr) <-
    run ["import", hexBase, outDir, "warmstandard.txt", "chrome.txt", "tone.txt"]
  check ("mixed set imports the valid recipe: " ++ show importErr) (importCode == ExitSuccess)
  check ("mixed set reports C1: " ++ show importOut) ("IMPORTED C1 from warmstandard.txt as \"warmstandard\"" `isInfixOf` importOut)
  check ("mixed set reports the unsupported film: " ++ show importOut) ("REJECTED chrome.txt:" `isInfixOf` importOut && "Classic Chrome" `isInfixOf` importOut)
  check ("mixed set reports the out-of-range value: " ++ show importOut) ("REJECTED tone.txt:" `isInfixOf` importOut && "out of range" `isInfixOf` importOut)
  check ("mixed set reports no field from the base: " ++ show importOut) ("from base: none" `isInfixOf` importOut)
  bank <- B.readFile (outDir </> bankFileA)
  let decoded = decodeBankFile bank
      state = decodedState (fixture decoded)
      occupied = [(k, name, bankPayloadContents saved) | (k, Populated name saved _) <- zip [0 :: Int ..] (bankRecordList (bankRecords state))]
  check "the generated bank decodes with its checksum" (case decoded of Right _ -> True; Left _ -> False)
  check "the generated bank is generation 1" (bankGeneration state == BankGeneration 1)
  check "the generated bank has one populated slot" (length occupied == 1)
  check
    "the populated slot carries the recipe's payload"
    (occupied == [(0, fixture (slotName "warmstandard"), payloadOf testBase (fixture (parseRecipeText "warmstandard" warmStandardText)))])
  (exportCode, exportOut, exportErr) <- run ["export", hexBase, outDir </> bankFileA, exportDir]
  check ("the bank exports: " ++ show exportErr) (exportCode == ExitSuccess)
  check ("the bank reports its slot: " ++ show exportOut) ("EXPORTED C1" `isInfixOf` exportOut)
  exported <- readFile (exportDir </> "C1.txt")
  check "the exported text is the canonical recipe" (exported == "Name: warmstandard\n" ++ warmStandardText)
  (againCode, againOut, againErr) <- run ["import", hexBase, secondDir, exportDir </> "C1.txt"]
  check ("the exported text imports again: " ++ show againErr) (againCode == ExitSuccess)
  check ("the exported text reports its slot: " ++ show againOut) ("IMPORTED C1" `isInfixOf` againOut)
  again <- B.readFile (secondDir </> bankFileA)
  let againState = decodedState (fixture (decodeBankFile again))
      againOccupied = [bankPayloadContents saved | Populated _ saved _ <- bankRecordList (bankRecords againState)]
  check "export followed by import reproduces the slot byte for byte" (againOccupied == [payloadOf testBase (fixture (parseRecipeText "warmstandard" warmStandardText))])
  removeDirectoryRecursive root
