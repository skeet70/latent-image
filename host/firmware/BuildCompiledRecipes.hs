-- build-compiled-recipes: compiles the D3/Q combined recipe rows for the
-- pinned X-M1 1.50 artifacts (reports/recipe-patch-d3.md). Host-only; no
-- camera, no container write.
--
--   build-compiled-recipes RECIPES.json IMAGE4.bin IMAGE5.bin IMAGE2.bin HOOK.bin OUT_DIR
--
-- RECIPES.json takes the envelope of schemas/recipe-set.schema.json
-- (schema_version 1, tier RC1, origin host-authored, the pinned target, and
-- exactly two recipes), with two deviations this artifact's shape forces:
-- `name` is optional and at most one recipe carries it (the artifact owns one
-- compiled name record, id 0x83d), and parameter values are the menu-name
-- tokens the pinned evidence binds, not the schema's tone integers (the 1.50
-- owner record binds no menu's sign side). A `help` line is refused: help text
-- would need the recipe-id record class above 0x83e, which this artifact does
-- not carry.
--
-- IMAGE4/IMAGE5/IMAGE2 are the pinned stock decodes (their SHA-256 are pinned
-- below and a mismatch is refused). HOOK.bin is menu/recipe_compiled.S
-- assembled and linked with menu/recipe_compiled.ld; its 19 words are checked
-- against the patch/ABI contract below before anything is emitted.
--
-- Value mapping: every recipe field's menu-name token resolves to a conversion
-- table row through the pinned bindings, and the pinned 1.50 conversion tables
-- read out of IMAGE4 at compile time confirm the row's stored code and its
-- reverse lookup. A token with no pinned row, a table that disagrees with a
-- pinned code, an ambiguous code and an unknown input field are all refused;
-- nothing is approximated.
--
-- Outputs in OUT_DIR:
--   patch-set-image4.json  the schema-3 manifest: the 516-byte prefix (values
--                          array, shooting-list labels, Q labels, the two row
--                          tables, the property table, the hook), the two
--                          film conversion-row rewrites, the fourteen PRO Neg
--                          availability cells and the eight node pointer words
--   patch-set-image5.json  the case-0xfe site word at 0x00d50a48
--   id2-patched.bin        the image-2 decode carrying the compiled name in
--                          the id-0x83d record's inline buffer (the input,
--                          unmodified, when no recipe is named)
--
-- stdout carries the layout, the link defsyms, the resolved rows and the free
-- window space a further patch can claim.
module Main where

import Control.Monad (forM, forM_, unless)
import Data.Aeson (Value)
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import qualified Data.ByteString as B
import Data.List ((\\))
import Data.Word (Word16, Word32, Word8)
import qualified FujiAF.Host as H
import FujiAF.JSON (encodeCanonical)
import FujiAF.LabelRecord (writeLabelName)
import FujiAF.RecipeBank
  ( ConversionTable (..)
  , RecipeField (..)
  , StoredCode (..)
  , TableRow (..)
  , armBranchWord
  , codeToRow
  , encodeLabelsArray
  , encodeValuesArray
  , readConversionTable
  , recipeFields
  , rowToCode
  )
import FujiAF.UpdateContainer (MediaImage (..))
import System.Directory (createDirectoryIfMissing)
import System.Environment (getArgs)

-- pinned artifacts --------------------------------------------------------

firmwareSha256 :: String
firmwareSha256 = "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"

image4Sha256, image5Sha256, image2Sha256 :: String
image4Sha256 = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
image5Sha256 = "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"
image2Sha256 = "236d12a11c8b386f71192675c12398358840327f21e392e6eaa91ddfd6e020a5"

image4Base, image5Base, image2Base :: Word32
image4Base = 0x0061c400
image5Base = 0x00bf1a00
image2Base = 0x00130000

image4File, image5File, image2File :: String
image4File = "private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin"
image5File = "private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin"
image2File = "private-data/analysis/x-m1-1.50/filmsim/id2-00130000.bin"

recordedAt :: String
recordedAt = "2026-10-09T20:00:00Z"

-- prefix layout -----------------------------------------------------------

-- The combined content is 508 bytes and only the 516-byte window
-- [0x0061c1fc, 0x0061c400) holds it, so the load drops the 64-byte base
-- convention and takes the widening report's corrected-floor shape
-- (reports/filmsim-prefix-widening.md section 6). Nothing overlaps: the
-- blocks run end to end from the floor and the slack stays at the top.
prefixBase, valuesAddress, labelsAddress, qLabelsAddress, rowTablesAddress, propsAddress, hookAddress, prefixContentEnd, windowEnd :: Word32
prefixBase = 0x0061c1fc
valuesAddress = prefixBase
labelsAddress = valuesAddress + 80
qLabelsAddress = labelsAddress + 160
rowTablesAddress = qLabelsAddress + 160
propsAddress = rowTablesAddress + 16
hookAddress = propsAddress + 16
prefixContentEnd = hookAddress + fromIntegral hookBytes
windowEnd = 0x0061c400

hookBytes :: Int
hookBytes = 76

-- the case-0xfe site in image 5 and the stock commit the hook re-enters
siteAddress, menuCommit, siteCurrentWord :: Word32
siteAddress = 0x00d50a48
menuCommit = 0x00d4fc74
siteCurrentWord = 0xe1a00004

-- the film conversion table rows the two recipes rewrite in place: the only
-- rows no stock values array names (reports/recipe-patch-d3.md section 1),
-- re-checked against IMAGE4 before the rewrite is emitted
filmTableAddress, filmRowAAddress, filmRowBAddress :: Word32
filmTableAddress = 0x0077ac30
filmRowAAddress = filmTableAddress + 24
filmRowBAddress = filmTableAddress + 48

filmRowA, filmRowB :: Int
filmRowA = 2
filmRowB = 4

filmCodeA, filmCodeB :: Word8
filmCodeA = 0x0d
filmCodeB = 0x0e

-- the six stock values arrays a row token could arrive from
stockValuesArrays :: [Word32]
stockValuesArrays = [0x0078319c, 0x0077ddf4, 0x00786724, 0x0078a0e8, 0x00780008, 0x00782348]

-- the node values/labels pointer words the prefix growth retargets: the three
-- list nodes that share the shooting-list arrays, then the Q cell (labels
-- before values per node)
pointerWords :: [(Word32, Word32, String)]
pointerWords =
  [ (0x007823c8, labelsAddress, "shooting-menu node 0x007823a4 labels pointer -> the nine-row shooting labels")
  , (0x007823c4, valuesAddress, "shooting-menu node 0x007823a4 values pointer -> the nine-row values array")
  , (0x00785d98, labelsAddress, "Fn-submenu node 0x00785d74 labels pointer -> the nine-row shooting labels")
  , (0x00785d94, valuesAddress, "Fn-submenu node 0x00785d74 values pointer -> the nine-row values array")
  , (0x0077d1a8, labelsAddress, "node 0x0077d184 labels pointer -> the nine-row shooting labels")
  , (0x0077d1a4, valuesAddress, "node 0x0077d184 values pointer -> the nine-row values array")
  , (0x0078b024, qLabelsAddress, "Q cell node 0x0078b000 labels pointer -> the nine-row Q labels")
  , (0x0078b020, valuesAddress, "Q cell node 0x0078b000 values pointer -> the shared nine-row values array")
  ]

-- RC0's fourteen availability cells: the key 0xb/0xc rows of the property-0xfe
-- matrix, word writes of 1 in the ASTIA/Velvia column pattern (columns 1, 2,
-- 4, 5, 6, 7, 21), byte-pinned from reports/filmsim-patch-01.md section 2
-- (orders 49-62). Without them the mode-apply path substitutes the mode
-- default for a chosen PRO Neg value; the combined build carries them so both
-- PRO Neg rows keep the behaviour RC0 shipped.
matrixCells :: [(Word32, Word32, String)]
matrixCells =
  [ (0x00795db4, 1, "availability cell: 0xfe matrix key 11 (PRO Neg Std), column 1")
  , (0x00795db8, 1, "availability cell: 0xfe matrix key 11 (PRO Neg Std), column 2")
  , (0x00795dc0, 1, "availability cell: 0xfe matrix key 11 (PRO Neg Std), column 4")
  , (0x00795dc4, 1, "availability cell: 0xfe matrix key 11 (PRO Neg Std), column 5")
  , (0x00795dc8, 1, "availability cell: 0xfe matrix key 11 (PRO Neg Std), column 6")
  , (0x00795dcc, 1, "availability cell: 0xfe matrix key 11 (PRO Neg Std), column 7")
  , (0x00795e04, 1, "availability cell: 0xfe matrix key 11 (PRO Neg Std), column 21")
  , (0x00795e10, 1, "availability cell: 0xfe matrix key 12 (PRO Neg Hi), column 1")
  , (0x00795e14, 1, "availability cell: 0xfe matrix key 12 (PRO Neg Hi), column 2")
  , (0x00795e1c, 1, "availability cell: 0xfe matrix key 12 (PRO Neg Hi), column 4")
  , (0x00795e20, 1, "availability cell: 0xfe matrix key 12 (PRO Neg Hi), column 5")
  , (0x00795e24, 1, "availability cell: 0xfe matrix key 12 (PRO Neg Hi), column 6")
  , (0x00795e28, 1, "availability cell: 0xfe matrix key 12 (PRO Neg Hi), column 7")
  , (0x00795e60, 1, "availability cell: 0xfe matrix key 12 (PRO Neg Hi), column 21")
  ]

-- compiled rows and labels -------------------------------------------------

-- The nine-entry list order, byte-pinned to the D3 build's arrays: the five
-- stock rows, the two RC0 PRO Neg rows, then the two compiled recipe rows.
listRows :: [Int]
listRows = [0, 3, 1, 7, 0xc, 5, 6, 2, 4]

nameRecordId :: Word32
nameRecordId = 0x83d

data BaseMode = BaseMode
  { bmToken :: String
  , bmRow :: Int
  , bmQuad :: (Word32, Word32, Word32, Word32)
  , bmQGlyph :: Word32
  }
  deriving (Eq, Show)

-- The stock label quadruples and Q glyphs, byte-read (reports/recipe-patch-d3.md
-- section 4.3; the Q glyphs in reports/filmsim-patch-02.md section 2.1).
baseModes :: [BaseMode]
baseModes =
  [ BaseMode "PROVIA/STANDARD" 0 (0x48a, 0x489, 0x48c, 0x48b) 0xff00077b
  , BaseMode "Velvia/VIVID" 3 (0x48e, 0x48d, 0x490, 0x48f) 0xff00077c
  , BaseMode "ASTIA/SOFT" 1 (0x492, 0x491, 0x494, 0x493) 0xff000773
  , BaseMode "B&W" 7 (0x49f, 0x49e, 0x4a2, 0x4a0) 0xff000777
  , BaseMode "SEPIA" 0xc (0x4b0, 0x4af, 0x4b2, 0x4b1) 0xff00077a
  ]

-- The two PRO Neg rows RC0 ships: row token, shooting-list quadruple, Q word 0.
proNegRows :: [(Int, (Word32, Word32, Word32, Word32), Word32)]
proNegRows =
  [ (5, (0x49b, 0x49a, 0x499, 0x49c), 0x49a)
  , (6, (0x497, 0x496, 0x495, 0x498), 0x496)
  ]

-- value bindings ----------------------------------------------------------

data Binding
  = BindRow Int (Maybe Word8) -- conversion row, pinned stored code when one is byte-read
  | BindCode Word8 -- pinned stored code; the row comes from the conversion table
  deriving (Eq, Show)

data ParamField = ParamField
  { pfKey :: String
  , pfProperty :: Word16
  , pfBindings :: [(String, Binding)]
  }
  deriving (Eq, Show)

-- The row-table byte order the hook walks, matching the property halfword
-- order below. Token-to-row bindings are byte-pinned from the pinned node map
-- (private-data/analysis/x-m1-1.50/filmsim/nodes-id4.txt: each property's
-- setting node carries its menu row tokens and its decoded label names) and
-- from reports/recipe-patch-d3.md section 6.1's byte-read rows. Expected
-- stored codes are byte-pinned from reports/recipe-design.md section 2
-- (colour's rows {+2, +1, 0, -1, -2} store {3, 4, 0, 6, 5}; white balance's
-- CUSTOM row stores 9) and its noise-reduction note. The pinned conversion
-- tables read out of IMAGE4 confirm every binding's code and reverse lookup at
-- compile, and a token outside these bindings is refused.
paramFields :: [ParamField]
paramFields =
  [ ParamField "dynamic_range" 0x104 [("AUTO", BindRow 0 (Just 0)), ("100", BindRow 1 (Just 1)), ("200", BindRow 2 (Just 2)), ("400", BindRow 3 (Just 3))]
  , ParamField "highlight_tone" 0x110 hardness
  , ParamField "shadow_tone" 0x111 hardness
  , ParamField "color" 0x10e level
  , ParamField "sharpness" 0x10f hardness
  , ParamField "noise_reduction" 0x00ef noiseReduction
  , ParamField "white_balance" 0x00ed whiteBalance
  , ParamField "iso" 0x141 iso
  ]

hardness :: [(String, Binding)]
hardness =
  [ ("HARD", BindRow 0 Nothing)
  , ("MEDIUM HARD", BindRow 1 Nothing)
  , ("STANDARD", BindRow 2 Nothing)
  , ("MEDIUM SOFT", BindRow 3 Nothing)
  , ("SOFT", BindRow 4 Nothing)
  ]

level :: [(String, Binding)]
level =
  [ ("HIGH", BindRow 0 (Just 3))
  , ("MEDIUM HIGH", BindRow 1 (Just 4))
  , ("MID", BindRow 2 (Just 0))
  , ("MEDIUM LOW", BindRow 3 (Just 6))
  , ("LOW", BindRow 4 (Just 5))
  ]

noiseReduction :: [(String, Binding)]
noiseReduction =
  [ ("HIGH", BindRow 4 Nothing)
  , ("MEDIUM HIGH", BindRow 3 Nothing)
  , ("STANDARD", BindRow 2 Nothing)
  , ("MEDIUM LOW", BindRow 1 (Just 1))
  , ("LOW", BindRow 0 (Just 0))
  ]

-- White balance's menu row tokens skip rows 2 and 3 (the node map's value
-- line for node 0x0077f868); INCANDESCENT's row token is 9.
whiteBalance :: [(String, Binding)]
whiteBalance =
  [ ("AUTO", BindRow 0 (Just 0))
  , ("CUSTOM", BindRow 1 (Just 9))
  , ("FINE", BindRow 4 Nothing)
  , ("SHADE", BindRow 5 Nothing)
  , ("FLUORESCENT-1", BindRow 6 Nothing)
  , ("FLUORESCENT-2", BindRow 7 Nothing)
  , ("FLUORESCENT-3", BindRow 8 Nothing)
  , ("INCANDESCENT", BindRow 9 Nothing)
  ]

-- ISO's menu row tokens are the node map's value line for node 0x00783de0,
-- in menu order H25600 down to AUTO; the menu's second AUTO row (token 0x1b)
-- is a child row and is not modelled.
iso :: [(String, Binding)]
iso =
  [ ("AUTO", BindRow 0 (Just 0))
  , ("L100", BindRow 7 Nothing)
  , ("200", BindRow 8 (Just 8))
  , ("250", BindRow 9 Nothing)
  , ("320", BindRow 10 Nothing)
  , ("400", BindRow 11 (Just 11))
  , ("500", BindRow 12 Nothing)
  , ("640", BindRow 13 Nothing)
  , ("800", BindRow 14 Nothing)
  , ("1000", BindRow 15 Nothing)
  , ("1250", BindRow 16 Nothing)
  , ("1600", BindRow 17 Nothing)
  , ("2000", BindRow 18 Nothing)
  , ("2500", BindRow 19 Nothing)
  , ("3200", BindRow 20 Nothing)
  , ("4000", BindRow 21 Nothing)
  , ("5000", BindRow 22 Nothing)
  , ("6400", BindRow 23 Nothing)
  , ("H12800", BindRow 25 Nothing)
  , ("H25600", BindRow 26 Nothing)
  ]

-- patch/ABI contract ------------------------------------------------------

data HookSlot
  = HookConst Word32
  | HookMov Word32 Word32 Bool -- destination register, address, is-movt
  | HookBl Word32
  deriving (Eq, Show)

-- menu/recipe_compiled.S as 19 words: the case arm's ABI (every register the
-- hook touches is pushed and popped, the site instruction is re-executed as
-- `mov r0, r4`), the independent derivation of the two table addresses, and
-- the single re-entry target. A HOOK.bin that differs in any slot is refused.
hookContract :: [HookSlot]
hookContract =
  [ HookConst 0xe92d4ff8 -- push {r3-r11, lr}
  , HookConst 0xe3550004 -- cmp r5, #4
  , HookMov 4 rowTablesAddress False
  , HookMov 4 rowTablesAddress True
  , HookConst 0x02844008 -- addeq r4, r4, #8
  , HookConst 0x0a000001 -- beq 1f
  , HookConst 0xe3550002 -- cmp r5, #2
  , HookConst 0x1a000007 -- bne 2f
  , HookMov 6 propsAddress False
  , HookMov 6 propsAddress True
  , HookConst 0xe3a07008 -- mov r7, #8
  , HookConst 0xe0d600b2 -- ldrh r0, [r6], #2
  , HookConst 0xe4d41001 -- ldrb r1, [r4], #1
  , HookBl menuCommit
  , HookConst 0xe2577001 -- subs r7, r7, #1
  , HookConst 0x1afffffa -- bne 3b
  , HookConst 0xe8bd4ff8 -- pop {r3-r11, lr}
  , HookConst 0xe1a00004 -- mov r0, r4
  , HookConst 0xe12fff1e -- bx lr
  ]

hookSlotWord :: Int -> HookSlot -> Word32
hookSlotWord _ (HookConst word) = word
hookSlotWord _ (HookMov rd address isMovt) =
  let imm16 = if isMovt then (address `shiftR` 16) .&. 0xffff else address .&. 0xffff
      base = (if isMovt then 0xe3400000 else 0xe3000000) .|. (rd `shiftL` 12)
   in base .|. (((imm16 `shiftR` 12) .&. 0xf) `shiftL` 16) .|. (imm16 .&. 0xfff)
hookSlotWord slot (HookBl to) =
  let delta = toInteger to - toInteger (hookAddress + fromIntegral (4 * slot)) - 8
   in 0xeb000000 .|. (fromIntegral (shiftR delta 2) .&. 0xffffff)

-- input -------------------------------------------------------------------

data RecipeInput = RecipeInput
  { riId :: String
  , riName :: Maybe String
  , riBaseToken :: String
  , riParams :: [(String, String)]
  }
  deriving (Eq, Show)

data ResolvedField = ResolvedField
  { rfKey :: String
  , rfProperty :: Word16
  , rfToken :: String
  , rfRow :: Int
  , rfCode :: Word8
  , rfEffective :: Word32
  }
  deriving (Eq, Show)

data ResolvedRecipe = ResolvedRecipe
  { rrInput :: RecipeInput
  , rrBase :: BaseMode
  , rrBaseCode :: Word8
  , rrBaseEffective :: Word32
  , rrFields :: [ResolvedField]
  }
  deriving (Eq, Show)

parseRecipeSet :: Value -> IO [RecipeInput]
parseRecipeSet value = do
  H.fields value "schema_version tier origin target recipes" "recipe set"
  version <- H.integer (H.get value "schema_version") 1 1 "recipe set schema_version"
  unless (version == 1) $ H.failH "recipe set: schema_version 1 required"
  unless (H.str (H.get value "tier") == "RC1") $ H.failH "recipe set: tier RC1 required"
  unless (H.str (H.get value "origin") == "host-authored") $ H.failH "recipe set: origin host-authored required"
  let target = H.get value "target"
  H.fields target "model firmware_version firmware_sha256" "recipe set target"
  unless (H.str (H.get target "model") == "X-M1") $ H.failH "recipe set: target model X-M1 required"
  unless (H.str (H.get target "firmware_version") == "1.50") $ H.failH "recipe set: firmware_version 1.50 required"
  unless (H.str (H.get target "firmware_sha256") == firmwareSha256) $
    H.failH "recipe set: firmware_sha256 does not name the pinned 1.50 firmware"
  recipes <- mapM parseRecipe (H.list (H.get value "recipes"))
  unless (length recipes == 2) $
    H.failH "recipe set: exactly two recipes required (the artifact carries film rows 2 and 4)"
  let identifiers = map riId recipes
  unless (length identifiers == length (unique identifiers)) $ H.failH "recipe set: recipe ids must be unique"
  let names = [name | recipe <- recipes, name <- maybe [] pure (riName recipe)]
  unless (length names <= 1) $
    H.failH "recipe set: at most one recipe may carry a name (the artifact owns one compiled name record, id 0x83d)"
  pure recipes

parseRecipe :: Value -> IO RecipeInput
parseRecipe value = do
  let names = H.keys value
  unless (null (names \\ ["id", "name", "film_simulation", "parameters"])) $
    H.failH "recipe: only id, name, film_simulation and parameters are accepted"
  unless (null (["id", "film_simulation", "parameters"] \\ names)) $
    H.failH "recipe: id, film_simulation and parameters are required"
  let identifier = H.str (H.get value "id")
  unless (validId identifier) $
    H.failH ("recipe: id " ++ show identifier ++ " must match ^[a-z0-9][a-z0-9-]{0,31}$")
  let name = if "name" `elem` names then Just (H.str (H.get value "name")) else Nothing
  mapM_ checkName name
  let parameters = H.get value "parameters"
  H.fields parameters "dynamic_range highlight_tone shadow_tone color sharpness noise_reduction white_balance iso" "recipe parameters"
  values <- forM paramFields $ \field -> pure (pfKey field, H.str (H.get parameters (pfKey field)))
  let balanceBlock = H.get parameters "white_balance"
  H.fields balanceBlock "mode" "recipe white_balance"
  let tokens = [(key, if key == "white_balance" then H.str (H.get balanceBlock "mode") else token) | (key, token) <- values]
  pure (RecipeInput identifier name (H.str (H.get value "film_simulation")) tokens)

checkName :: String -> IO ()
checkName name = do
  unless (not (null name) && length name <= 27) $
    H.failH ("recipe name " ++ show name ++ ": 1 to 27 characters required (the id-0x83d buffer)")
  unless (all nameChar name) $
    H.failH ("recipe name " ++ show name ++ ": letters, digits, space and . / + - only")

nameChar :: Char -> Bool
nameChar c =
  ('A' <= c && c <= 'Z') || ('a' <= c && c <= 'z') || ('0' <= c && c <= '9') || c `elem` " ./+-"

validId :: String -> Bool
validId identifier = case identifier of
  [] -> False
  (c : rest) -> first c && all restChar rest && length identifier <= 32
 where
  first ch = ('a' <= ch && ch <= 'z') || ('0' <= ch && ch <= '9')
  restChar ch = first ch || ch == '-'

unique :: Eq a => [a] -> [a]
unique [] = []
unique (x : xs) = x : unique (filter (/= x) xs)

-- resolution --------------------------------------------------------------

unStored :: StoredCode -> Word8
unStored (StoredCode code) = code

tableFor :: [(Word16, ConversionTable)] -> Word16 -> Either String ConversionTable
tableFor tables property = case lookup property tables of
  Just table -> Right table
  Nothing -> Left ("no conversion table bound for property " ++ hex16 property)

rowEffectiveAt :: ConversionTable -> Int -> Either String Word32
rowEffectiveAt table row = case drop row (tableRows table) of
  (entry : _) -> Right (rowEffective entry)
  [] -> Left ("conversion table " ++ hex32 (tableAddress table) ++ ": row " ++ show row ++ " out of range")

resolveField :: [(Word16, ConversionTable)] -> ParamField -> String -> Either String ResolvedField
resolveField tables field token = do
  binding <- case lookup token (pfBindings field) of
    Just binding -> Right binding
    Nothing ->
      Left
        ( pfKey field
            ++ ": value "
            ++ show token
            ++ " has no pinned row binding (pinned: "
            ++ show (map fst (pfBindings field))
            ++ "); refusing rather than guessing"
        )
  table <- tableFor tables (pfProperty field)
  (row, code) <- case binding of
    BindRow row expected -> do
      code <- unStored <$> rowToCode table row
      case expected of
        Just want
          | want /= code ->
              Left
                ( pfKey field
                    ++ ": value "
                    ++ show token
                    ++ " pins row "
                    ++ show row
                    ++ " to code "
                    ++ show want
                    ++ " but the pinned table carries code "
                    ++ show code
                )
        _ -> pure ()
      back <- codeToRow table (StoredCode code)
      unless (back == row) $
        Left (pfKey field ++ ": code " ++ show code ++ " resolves back to row " ++ show back ++ ", not row " ++ show row)
      pure (row, code)
    BindCode want -> do
      row <- codeToRow table (StoredCode want)
      pure (row, want)
  effective <- rowEffectiveAt table row
  pure (ResolvedField (pfKey field) (pfProperty field) token row code effective)

resolveBase :: [(Word16, ConversionTable)] -> String -> Either String (BaseMode, Word8, Word32)
resolveBase tables token = do
  base <- case [m | m <- baseModes, bmToken m == token] of
    [m] -> Right m
    [] -> Left ("film_simulation: value " ++ show token ++ " is not one of " ++ show (map bmToken baseModes))
    _ -> Left ("film_simulation: value " ++ show token ++ " is bound twice")
  table <- tableFor tables 0x00fe
  code <- unStored <$> rowToCode table (bmRow base)
  back <- codeToRow table (StoredCode code)
  unless (back == bmRow base) $
    Left ("film_simulation: code " ++ show code ++ " resolves back to row " ++ show back ++ ", not row " ++ show (bmRow base))
  effective <- rowEffectiveAt table (bmRow base)
  pure (base, code, effective)

resolveRecipe :: [(Word16, ConversionTable)] -> RecipeInput -> Either String ResolvedRecipe
resolveRecipe tables recipe = do
  (base, code, effective) <- resolveBase tables (riBaseToken recipe)
  fields <- mapM one paramFields
  pure (ResolvedRecipe recipe base code effective fields)
 where
  one field = case lookup (pfKey field) (riParams recipe) of
    Just token -> resolveField tables field token
    Nothing -> Left (pfKey field ++ ": value missing")

-- serialization -----------------------------------------------------------

le16 :: Word16 -> B.ByteString
le16 w = B.pack [fromIntegral (w .&. 0xff), fromIntegral (w `shiftR` 8)]

leWord :: B.ByteString -> Int -> Word32
leWord bytes i = foldl' (\acc k -> acc .|. (fromIntegral (B.index bytes (i + k)) `shiftL` (8 * k))) 0 [0 .. 3 :: Int]

blockWords :: B.ByteString -> [Word32]
blockWords bytes = [leWord bytes i | i <- [0, 4 .. B.length bytes - 4]]

rowBytes :: ResolvedRecipe -> B.ByteString
rowBytes recipe = B.pack [fromIntegral (rfRow field) | field <- rrFields recipe]

shootingQuads :: [ResolvedRecipe] -> [[Word32]]
shootingQuads recipes =
  [quadOf base | base <- baseModes]
    ++ [[a, b, c, d] | (_, (a, b, c, d), _) <- proNegRows]
    ++ [recipeQuad recipe | recipe <- recipes]
 where
  quadOf base = case bmQuad base of (a, b, c, d) -> [a, b, c, d]
  recipeQuad recipe = case bmQuad (rrBase recipe) of
    (a, b, c, d) -> [a, b, c, if hasName recipe then nameRecordId else d]
  hasName recipe = case riName (rrInput recipe) of
    Just _ -> True
    Nothing -> False

qRecords :: [ResolvedRecipe] -> [[Word32]]
qRecords recipes =
  [[bmQGlyph base, 0, 0, 0] | base <- baseModes]
    ++ [[glyph, 0, 0, 0] | (_, _, glyph) <- proNegRows]
    ++ [[bmQGlyph (rrBase recipe), 0, 0, 0] | recipe <- recipes]

data Block = Block
  { blId :: String
  , blRole :: String
  , blAddress :: Word32
  , blBytes :: B.ByteString
  }
  deriving (Eq, Show)

prefixBlocks :: B.ByteString -> [ResolvedRecipe] -> Either String [Block]
prefixBlocks hook recipes = do
  labels <- encodeLabelsArray (shootingQuads recipes)
  qLabels <- encodeLabelsArray (qRecords recipes)
  pure
    [ Block "values-array" "relocation: the nine {row, child} pairs and the {0x7ffffffe, 0} terminator" valuesAddress (encodeValuesArray [(fromIntegral row, 0) | row <- listRows])
    , Block "shooting-labels" "relocation: the nine {preview icon, list icon, help, name} quadruples and the 16-byte -1 terminator" labelsAddress labels
    , Block "q-labels" "new values: the nine Q records (word 0 only, words 1-3 zero) and the 16-byte -1 terminator" qLabelsAddress qLabels
    , Block "row-tables" "new values: the two recipe row tables, one row byte per applied property" rowTablesAddress (B.concat (map rowBytes recipes))
    , Block "property-table" "new values: the eight property halfwords the hook walk consumes" propsAddress (B.concat [le16 (pfProperty field) | field <- paramFields])
    , Block "commit-hook" "new code: the case-0xfe commit hook (menu/recipe_compiled.S)" hookAddress hook
    ]

filmRowWords :: [ResolvedRecipe] -> Either String [(Word32, Word32, String)]
filmRowWords recipes = case recipes of
  [recipeA, recipeB] ->
    Right
      [ (filmRowAAddress, fromIntegral filmCodeA, "recipe A (film row 2) col0 stored code")
      , (filmRowAAddress + 4, 0x7ffffffe, "recipe A (film row 2) col1 marker, unchanged")
      , (filmRowAAddress + 8, rrBaseEffective recipeA, "recipe A (film row 2) col2 = its base mode's downstream value")
      , (filmRowBAddress, fromIntegral filmCodeB, "recipe B (film row 4) col0 stored code")
      , (filmRowBAddress + 4, 0x7ffffffe, "recipe B (film row 4) col1 marker, unchanged")
      , (filmRowBAddress + 8, rrBaseEffective recipeB, "recipe B (film row 4) col2 = its base mode's downstream value")
      ]
  _ -> Left "internal: exactly two resolved recipes required"

bodyWords :: [ResolvedRecipe] -> Either String [(Word32, Word32, String)]
bodyWords recipes = do
  film <- filmRowWords recipes
  pure (film ++ matrixCells ++ pointerWords)

-- images and words --------------------------------------------------------

readWord32 :: B.ByteString -> Word32 -> Word32 -> Word32
readWord32 image base address =
  foldl' (\acc i -> acc .|. (fromIntegral (B.index image (fromIntegral (address - base) + i)) `shiftL` (8 * i))) 0 [0 .. 3 :: Int]

checkHash :: String -> B.ByteString -> String -> IO ()
checkHash label bytes expected = do
  actual <- H.sha256 bytes
  H.ensure (actual == expected) (label ++ ": sha256 " ++ actual ++ " does not match the pinned " ++ expected)

changedBytes :: Word32 -> Word32 -> Int
changedBytes old fresh = length [() | s <- [0, 8, 16, 24], ((old `shiftR` s) .&. 0xff) /= ((fresh `shiftR` s) .&. 0xff)]

valuesRowTokens :: B.ByteString -> Word32 -> Word32 -> Either String [Word32]
valuesRowTokens image base address = go 0
 where
  go :: Int -> Either String [Word32]
  go k
    | k >= 128 = Left ("values array " ++ hex32 address ++ ": no 0x7ffffffe terminator")
    | token == 0x7ffffffe = Right []
    | otherwise = (token :) <$> go (k + 1)
   where
    token = readWord32 image base (address + fromIntegral (8 * k))

hexDigitChar :: Int -> Char
hexDigitChar d = "0123456789abcdef" !! d

hex :: Word32 -> String
hex w = [hexDigitChar (fromIntegral ((w `shiftR` s) .&. 0xf)) | s <- [28, 24, 20, 16, 12, 8, 4, 0]]

hex16 :: Word16 -> String
hex16 w = "0x" ++ [hexDigitChar (fromIntegral ((w `shiftR` s) .&. 0xf)) | s <- [12, 8, 4, 0]]

hex32 :: Word32 -> String
hex32 w = "0x" ++ hex w

hex8 :: Word8 -> String
hex8 w = "0x" ++ [hexDigitChar (fromIntegral ((w `shiftR` s) .&. 0xf)) | s <- [4, 0]]

hexBytes :: B.ByteString -> String
hexBytes = concatMap byte . B.unpack
 where
  byte b = [hexDigitChar (fromIntegral b `div` 16), hexDigitChar (fromIntegral b `mod` 16)]

-- manifest ----------------------------------------------------------------

blobJson :: Int -> Block -> Value
blobJson order0 block = H.obj
  [ ("id", H.s (blId block))
  , ("role", H.s (blRole block))
  , ("address", H.s (hex32 (blAddress block)))
  , ("file_offset", H.s (hex32 (blAddress block - prefixBase)))
  , ("length", H.n (fromIntegral (B.length bytes)))
  , ("order_range", H.arr [H.n (fromIntegral order0), H.n (fromIntegral (order0 + B.length bytes `div` 4 - 1))])
  , ("current_hex", H.s (hexBytes (B.replicate (B.length bytes) 0)))
  , ("new_hex", H.s (hexBytes bytes))
  ]
 where
  bytes = blBytes block

wordJson :: Int -> Word32 -> Word32 -> Word32 -> String -> Value
wordJson order address current fresh role = H.obj
  [ ("order", H.n (fromIntegral order))
  , ("address", H.s (hex32 address))
  , ("file_offset", H.s (hex32 (address - prefixBase)))
  , ("current", H.s (hex32 current))
  , ("new", H.s (hex32 fresh))
  , ("role", H.s role)
  ]

wordJson5 :: Int -> Word32 -> Word32 -> Word32 -> String -> Value
wordJson5 order address current fresh role = H.obj
  [ ("order", H.n (fromIntegral order))
  , ("address", H.s (hex32 address))
  , ("file_offset", H.s (hex32 (address - image5Base)))
  , ("current", H.s (hex32 current))
  , ("new", H.s (hex32 fresh))
  , ("role", H.s role)
  ]

targetJson :: Value
targetJson = H.obj
  [ ("model", H.s "X-M1")
  , ("firmware_version", H.s "1.50")
  , ("firmware_sha256", H.s firmwareSha256)
  ]

image4Json :: Value
image4Json = H.obj
  [ ("segment_id", H.n 4)
  , ("decode_base", H.s (hex32 image4Base))
  , ("output_base", H.s (hex32 prefixBase))
  , ("decode_file", H.s image4File)
  , ("decode_sha256", H.s image4Sha256)
  , ("file_offset_rule", H.s ("output file_offset = address - " ++ hex32 prefixBase ++ "; source offset in the stock decode = address - " ++ hex32 image4Base))
  , ("dump_correspondence", H.s "the input is the pinned stock image-4 decode; E-DUMP-RAM-150 correspondence applies to its stock-body bytes and the prefix region is zero at rest in every captured representation")
  ]

extensionJson :: Value
extensionJson = H.obj
  [ ("kind", H.s "prefix")
  , ("length", H.n 516)
  , ("current_hex", H.s (concat (replicate 516 "00")))
  ]

freeSpaceJson :: Value
freeSpaceJson = H.obj
  [ ("chosen_region", H.arr [H.s (hex32 prefixBase), H.s (hex32 prefixContentEnd)])
  , ("window", H.arr [H.s (hex32 prefixBase), H.s (hex32 windowEnd)])
  , ("window_bytes", H.n 516)
  , ("window_floor_evidence", H.s "byte-read table-block tail (reports/filmsim-prefix-widening.md section 1): the last plain-extent terminator sits at 0x0061c1f8 and 0x0061c1fc reads 0, so the free window is [0x0061c1fc, 0x0061c400) = 516 bytes")
  , ("running_count", runningCount)
  , ("residual_slack", H.arr [H.s (hex32 prefixContentEnd), H.s (hex32 windowEnd)])
  , ("residual_slack_bytes", H.n 8)
  , ("geometry_invariant", H.s ("load + decoded_size = " ++ hex32 prefixBase ++ " + 0x00282a04 = 0x0089ec00 = stock bss start; load + segment_size = " ++ hex32 prefixBase ++ " + 0x00282a2c = 0x0089ec28"))
  , ("bootstrap_note", H.s "the 68 bytes [0x0061c1fc, 0x0061c240) are inside this artifact's content, not free bootstrap space: the values array covers [0x0061c1fc, 0x0061c24c) and the shooting labels begin at 0x0061c24c. Free space after this artifact is [0x0061c3f8, 0x0061c400), 8 bytes")
  , ("ownership_evidence", H.s "reports/filmsim-patch-01.md section 5 extended downward to the corrected floor (reports/filmsim-prefix-widening.md sections 1-5, E-FILMSIM-PREFIX-WIDENING-150); static-inference, not hardware-observed")
  , ("coverage_limit", H.s "as in FILMSIM-PATCH-01: a later table reload or an undiscovered covering consumer falsifies the window's ownership")
  ]
 where
  runningCount =
    H.arr
      [ item "values array (9 rows + terminator)" valuesAddress 80 80 436
      , item "shooting labels (9 quadruples + terminator)" labelsAddress 160 240 276
      , item "Q labels (9 records + terminator)" qLabelsAddress 160 400 116
      , item "row tables (2 x 8 bytes)" rowTablesAddress 16 416 100
      , item "property table (8 halfwords)" propsAddress 16 432 84
      , item "commit hook (19 words)" hookAddress 76 508 8
      ]
  item :: String -> Word32 -> Int -> Int -> Int -> Value
  item name address bytes used free =
    H.obj
      [ ("item", H.s name)
      , ("range", H.arr [H.s (hex32 address), H.s (hex32 (address + fromIntegral bytes))])
      , ("bytes", H.n (fromIntegral bytes))
      , ("used_total", H.n (fromIntegral used))
      , ("free_after", H.n (fromIntegral free))
      ]

arraySourceJson :: Value
arraySourceJson = H.obj
  [ ("file", H.s image4File)
  , ("sha256", H.s image4Sha256)
  , ("note", H.s "the five stock rows and the two PRO Neg rows are byte-identical to the RC0 arrays (reports/recipe-patch-d3.md section 4.3) and their Q records to the stock Q glyphs plus the patch-02 string ids 0x49a and 0x496; the two recipe rows and their Q records are new values with no stock source. Current prefix bytes are the zero creation template, equal to the table preload at those RAM addresses.")
  ]

orderRuleJson :: Value
orderRuleJson = H.s "the 127 prefix words (values array, shooting labels, Q labels, row tables, property table, hook) as six blob writes in address order, then the two film conversion-row rewrites in row order (3 words each), then the fourteen PRO Neg availability cells (key 11 then key 12, columns ascending), then per node the labels pointer before the values pointer (shooting menu, Fn submenu, node 0x0077d184, Q cell). Offline prefix creation precedes all 155 writes; this manifest is not a live-write input."

totalsJson :: Int -> Int -> Value
totalsJson changedBody changedAll = H.obj
  [ ("words", H.n 155)
  , ("prefix_words", H.n 127)
  , ("body_words", H.n 28)
  , ("bytes_written", H.n 620)
  , ("prefix_bytes_added", H.n 516)
  , ("changed_bytes_in_preserved_body", H.n (fromIntegral changedBody))
  , ("changed_bytes_against_constructed_output", H.n (fromIntegral changedAll))
  , ("film_row_words", H.n 6)
  , ("matrix_words", H.n 14)
  , ("pointer_words", H.n 8)
  ]

unchangedSurfacesJson :: Value
unchangedSurfacesJson = H.arr (map H.s items)
 where
  items =
    [ "FILM 1-3 bracket children 0x00786774 / 0x00786da8 / 0x007898d4 and their values and labels arrays: stock five rows, no RC0b bracket scope incorporated"
    , "the FILM 1-3 availability matrices 0x00791514 / 0x00791964 / 0x007953f8 (every row and cell)"
    , "RAW-conversion list 0x0078bd5c and unlabelled nine-row list 0x0078d2d0"
    , "film conversion table 0x0077ac30 rows 0, 1, 3 and 5-12, and the table-pointer words at 0x0081b09c + 4*property"
    , "the availability descriptor list 0x0079944c and every 0xfe availability matrix row and cell except the fourteen key 0xb/0xc cells named in the write set"
    , "the stock values and labels arrays now unreachable at 0x0078319c, 0x0078d4f8, 0x00782348, 0x00789e2c and the bracket arrays"
    , "the label accessor gates and the id-0x83c and id-0x83e records: no accessor-gate edit, 0x83e stays the stock empty record"
    , "every image-5 word except the case-0xfe site word, and every image-2 byte except the id-0x83d inline buffer"
    ]

containerEffectJson :: Value
containerEffectJson = H.obj
  [ ("image4_load", H.s (hex32 prefixBase))
  , ("decoded_size", H.s "0x00282a04")
  , ("segment_size", H.s "0x00282a2c")
  , ("bss_base", H.s "0x0089ec00")
  , ("bss_length", H.s "0x00352e00")
  , ("bss_end", H.s "0x00bf1a00")
  , ("embedded_header_address", H.s "0x0061c400")
  , ("embedded_header", H.s "unchanged, now at output offset 0x204")
  , ("trailer_load", H.s (hex32 prefixBase))
  , ("trailer_size", H.s "0x00282a2c")
  , ("record_total_len", H.s "encoder-measured; header word 1 includes the 20-byte header")
  , ("record_chunk_count", H.s "ceil(total_len / 0x4000)")
  , ("flash_window", H.s "0x00160000")
  , ("dat_extents", H.s "unchanged")
  , ("image5_site", H.s "the four-byte site word rides patch-set-image5.json and rebuild-update replace-record5")
  , ("image2_name", H.s "the id-0x83d name write rides rebuild-update write-label or replace-image2 with id2-patched.bin")
  ]

emulationCasesJson :: Value
emulationCasesJson = H.obj
  [ ("carried", H.s "none in this manifest; the five exercise cases below are the emit contract for the runtime worker's harness")
  , ("cases", H.arr (map H.s cases))
  ]
 where
  cases =
    [ "row-path-a: menu commit FUN_00d4fc74(0xfe, values entry {2, 0}); the hook reads the recipe A row table at 0x0061c38c and issues eight nested FUN_00d4fc74 calls (0x104, 0x110, 0x111, 0x10e, 0x10f, 0xef, 0xed, 0x141), then the stock arm continues with mov r0, r4 and mov r1, r8 intact"
    , "row-path-b: menu commit FUN_00d4fc74(0xfe, values entry {4, 0}); same shape with the recipe B row table at 0x0061c394, the property walk still starting at 0x0061c39c"
    , "stock-rows: menu commit FUN_00d4fc74(0xfe, values entry in {0, 3, 1, 7, 0xc, 5, 6}); the hook returns after its push and pop with no property call and no register change"
    , "reverse-lookup: FUN_00631a00 col0 scan over 0x0077ac30 for stored codes 0x0d and 0x0e returns row indices 2 and 4"
    , "q-consumer: FUN_00bf93e0 over Q node 0x0078b000's arrays returns record word 0 for rows 2 and 4 (0xff00077b and 0xff00077c), with the first five Q glyphs unchanged"
    ]

manifestImage4 :: [Block] -> [(Int, Word32, Word32, Word32, String)] -> Int -> Int -> Value
manifestImage4 blocks writes changedBody changedAll = H.obj
  [ ("schema_version", H.n 3)
  , ("id", H.s "RECIPE-D3-PATCH-01")
  , ("tier", H.s "RC1")
  , ("status", H.s "source-not-built-not-installable")
  , ("installable", H.b False)
  , ("recorded_at", H.s recordedAt)
  , ("target", targetJson)
  , ("image", image4Json)
  , ("extension", extensionJson)
  , ("free_space", freeSpaceJson)
  , ("array_source", arraySourceJson)
  , ("order_rule", orderRuleJson)
  , ("blobs", H.arr [blobJson start block | (start, block) <- zip (orderStarts blocks) blocks])
  , ("words", H.arr [wordJson order address current fresh role | (order, address, current, fresh, role) <- writes])
  , ("totals", totalsJson changedBody changedAll)
  , ("unchanged_surfaces", unchangedSurfacesJson)
  , ("container_effect", containerEffectJson)
  , ("emulation_case_parameters", emulationCasesJson)
  ]

orderStarts :: [Block] -> [Int]
orderStarts blocks = scanl (\acc block -> acc + B.length (blBytes block) `div` 4) 1 blocks

manifestImage5 :: Word32 -> Word32 -> Value
manifestImage5 current fresh = H.obj
  [ ("schema_version", H.n 3)
  , ("id", H.s "RECIPE-D3-PATCH-01-IMAGE5")
  , ("tier", H.s "RC1")
  , ("status", H.s "source-not-built-not-installable")
  , ("installable", H.b False)
  , ("recorded_at", H.s recordedAt)
  , ("target", targetJson)
  , ("image", image5Json)
  , ("free_space", freeSpaceJson5)
  , ("array_source", arraySourceJson5)
  , ("order_rule", H.s "one word write: the case-0xfe site instruction becomes the branch to the compiled-recipe hook")
  , ("blobs", H.arr [])
  , ("words", H.arr [wordJson5 1 siteAddress current fresh "code change: case 0xfe's first instruction becomes bl to the compiled-recipe hook"])
  , ("totals", totalsJson5)
  , ("unchanged_surfaces", unchangedSurfacesJson5)
  , ("container_effect", containerEffectJson5)
  , ("emulation_case_parameters", emulationCasesJson)
  ]

image5Json :: Value
image5Json = H.obj
  [ ("segment_id", H.n 5)
  , ("decode_base", H.s (hex32 image5Base))
  , ("decode_file", H.s image5File)
  , ("decode_sha256", H.s image5Sha256)
  , ("file_offset_rule", H.s ("file_offset = address - " ++ hex32 image5Base))
  , ("dump_correspondence", H.s "the input is the pinned stock image-5 decode; the site word is byte-read at decode offset 0x0015f048")
  ]

freeSpaceJson5 :: Value
freeSpaceJson5 = H.obj
  [ ("chosen_region", H.arr [H.s (hex32 siteAddress), H.s (hex32 (siteAddress + 4))])
  , ("note", H.s "no decode-prefix placement: the only image-5 write is the site word in the preserved body, and the image-5 record keeps its decoded size and segment entry")
  ]

arraySourceJson5 :: Value
arraySourceJson5 = H.obj
  [ ("file", H.s image5File)
  , ("sha256", H.s image5Sha256)
  , ("note", H.s "the site word's current bytes are read from the pinned decode and checked against 0xe1a00004 (mov r0, r4)")
  ]

totalsJson5 :: Value
totalsJson5 = H.obj
  [ ("words", H.n 1)
  , ("bytes_written", H.n 4)
  , ("record5_note", H.s "the record-5 stream moves inside its window [0x002c0000, 0x005a0000) as rebuild-update replace-record5 measures it; record-5 header word 1 (stream length) always moves with the stream, word 2 (chunk count) only when ceil(total_len / 0x4000) changes, and the main checksum word moves with the container")
  ]

unchangedSurfacesJson5 :: Value
unchangedSurfacesJson5 = H.arr (map H.s ["every image-5 word except the site word at 0x00d50a48", "the image-5 decode size, segment entry and stage header"])

containerEffectJson5 :: Value
containerEffectJson5 = H.obj
  [ ("record5_window", H.s "[0x002c0000, 0x005a0000)")
  , ("decoded_size", H.s "0x004f1e60 (unchanged)")
  , ("segment_size", H.s "0x004f1e60 (unchanged, equal to the decoded size; the 0x28 excess over the decoded size is image 4's relation only)")
  , ("dat_extents", H.s "unchanged outside the record-5 stream window and the record-5 header and main checksum words")
  ]

-- summary -----------------------------------------------------------------

summary :: [ResolvedRecipe] -> [Block] -> String -> String
summary recipes blocks hookDigest = unlines (concat sections)
 where
  sections =
    [ ["build-compiled-recipes: RECIPE-D3-PATCH-01 (RC1), " ++ show (length recipes) ++ " compiled rows", ""]
    , ["prefix layout (base " ++ hex32 prefixBase ++ ", load + decoded_size = 0x0089ec00):"]
    , [ "  " ++ hex32 (blAddress block) ++ "  " ++ pad (B.length (blBytes block)) ++ "  " ++ blId block
      | block <- blocks
      ]
    , ["  " ++ hex32 prefixContentEnd ++ "  " ++ pad 8 ++ "  zero fill (free window slack)", ""]
    , ["entry and link:"]
    , ["  hook entry      " ++ hex32 hookAddress ++ " (" ++ show hookBytes ++ " bytes, sha256 " ++ hookDigest ++ ")"]
    , ["  link            --defsym=RC_BASE=" ++ hex32 hookAddress ++ "; rc_row_tables " ++ hex32 rowTablesAddress ++ ", rc_props " ++ hex32 propsAddress ++ ", menu_commit 0x00d4fc74 (menu/recipe_compiled_layout.S)", ""]
    , ["compiled rows (row-table byte order " ++ show (map pfKey paramFields) ++ "):"]
    , concat
        [ [ "  recipe " ++ riId (rrInput recipe) ++ maybe "" (\n -> " (" ++ n ++ ")") (riName (rrInput recipe)) ++ ", base " ++ riBaseToken (rrInput recipe) ++ ", film row " ++ show (bmRow (rrBase recipe)) ++ " -> code " ++ hex8 (if index == 0 then filmCodeA else filmCodeB)]
          ++ [ "    " ++ padKey (rfKey f) ++ " " ++ rfToken f ++ " -> row " ++ show (rfRow f) ++ ", code " ++ hex8 (rfCode f) ++ ", effective " ++ show (rfEffective f)
             | f <- rrFields recipe
             ]
        | (index, recipe) <- zip [0 :: Int ..] recipes
        ]
    , [""]
    , ["row tables: " ++ hexBytes (B.concat (map rowBytes recipes))]
    , ["outputs: patch-set-image4.json, patch-set-image5.json, id2-patched.bin"]
    , ["inputs: " ++ image4File ++ ", " ++ image5File ++ ", " ++ image2File]
    , ["free window after this artifact: [" ++ hex32 prefixContentEnd ++ ", " ++ hex32 windowEnd ++ "), 8 bytes"]
    ]
  pad :: Int -> String
  pad n = let s = show n in replicate (4 - length s) ' ' ++ s
  padKey s = s ++ replicate (16 - length s) ' '

-- main --------------------------------------------------------------------

main :: IO ()
main = do
  arguments <- getArgs
  (recipesPath, image4Path, image5Path, image2Path, hookPath, outDir) <- case arguments of
    [a, b, c, d, e, f] -> pure (a, b, c, d, e, f)
    _ -> H.failH "usage: build-compiled-recipes RECIPES.json IMAGE4.bin IMAGE5.bin IMAGE2.bin HOOK.bin OUT_DIR"
  recipeBytes <- H.readRegular (1024 * 1024) recipesPath
  image4 <- H.readRegular (16 * 1024 * 1024) image4Path
  image5 <- H.readRegular (16 * 1024 * 1024) image5Path
  image2 <- H.readRegular (16 * 1024 * 1024) image2Path
  hook <- H.readRegular 4096 hookPath
  checkHash "image4" image4 image4Sha256
  checkHash "image5" image5 image5Sha256
  checkHash "image2" image2 image2Sha256
  recipeValue <- H.jsonRead recipeBytes
  recipesInput <- parseRecipeSet recipeValue
  let orderedFields = filter ((/= 0x00fe) . fieldProperty) recipeFields ++ filter ((== 0x00fe) . fieldProperty) recipeFields
  tables <- forM orderedFields $ \field -> do
    table <- either H.failH pure (readConversionTable image4Base image4 (fieldTable field))
    pure (fieldProperty field, table)
  recipes <- either H.failH pure (mapM (resolveRecipe tables) recipesInput)
  hookDigest <- H.sha256 hook
  unless (B.length hook == hookBytes) $
    H.failH ("hook blob: " ++ show (B.length hook) ++ " bytes, the patch/ABI contract is " ++ show hookBytes)
  forM_ (zip [0 ..] hookContract) $ \(slot, expected) -> do
    let actual = readWord32 hook 0 (fromIntegral (4 * slot))
        want = hookSlotWord slot expected
    H.ensure (actual == want) ("hook blob: slot " ++ show slot ++ " is 0x" ++ hex actual ++ ", the patch/ABI contract requires 0x" ++ hex want)
  -- the in-place film-row rewrite is only sound while rows 2 and 4 stay
  -- unnamed by every stock values array and codes 0x0d/0x0e stay free
  filmTable <- either H.failH pure (tableFor tables 0x00fe)
  let stockCodes = map (unStored . rowCode) (tableRows filmTable)
  unless (all (`notElem` [filmCodeA, filmCodeB]) stockCodes) $
    H.failH "film conversion table: stored code 0x0d or 0x0e is already taken, the compiled rows need both free"
  forM_ stockValuesArrays $ \address -> do
    tokens <- either H.failH pure (valuesRowTokens image4 image4Base address)
    unless (all (`notElem` [fromIntegral filmRowA, fromIntegral filmRowB]) tokens) $
      H.failH ("values array " ++ hex32 address ++ " already names row 2 or row 4, the compiled rows would collide")
  blocks <- either H.failH pure (prefixBlocks hook recipes)
  body <- either H.failH pure (bodyWords recipes)
  let wordsWithCurrent =
        [ (order, address, readWord32 image4 image4Base address, fresh, role)
        | (order, (address, fresh, role)) <- zip [128 ..] body
        ]
      changedBody = sum [changedBytes current fresh | (_, _, current, fresh, _) <- wordsWithCurrent]
      changedAll = changedBody + sum [changedBytes 0 word | block <- blocks, word <- blockWords (blBytes block)]
  createDirectoryIfMissing True outDir
  B.writeFile (outDir ++ "/patch-set-image4.json") (encodeCanonical (manifestImage4 blocks wordsWithCurrent changedBody changedAll))
  let siteCurrent = readWord32 image5 image5Base siteAddress
  H.ensure (siteCurrent == siteCurrentWord) ("image5: site word at " ++ hex32 siteAddress ++ " is 0x" ++ hex siteCurrent ++ ", not the stock mov r0, r4")
  siteFresh <- either H.failH pure (armBranchWord siteAddress hookAddress)
  B.writeFile (outDir ++ "/patch-set-image5.json") (encodeCanonical (manifestImage5 siteCurrent siteFresh))
  patchedId2 <- case [name | recipe <- recipesInput, name <- maybe [] pure (riName recipe)] of
    [] -> pure image2
    [name] -> case writeLabelName image2Base (MediaImage image2) name of
      Left err -> H.failH ("image-2 name write: " ++ err)
      Right (MediaImage bytes) -> pure bytes
    _ -> H.failH "recipe set: at most one compiled name"
  B.writeFile (outDir ++ "/id2-patched.bin") patchedId2
  putStrLn (summary recipes blocks hookDigest)
  putStrLn ("wrote " ++ outDir ++ "/patch-set-image4.json, " ++ outDir ++ "/patch-set-image5.json, " ++ outDir ++ "/id2-patched.bin")
