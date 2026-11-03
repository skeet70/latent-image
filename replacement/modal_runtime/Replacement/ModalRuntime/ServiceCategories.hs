{-# LANGUAGE GHC2021 #-}

-- | Retained-service seeds and their decompiled source-call closure, shared by
-- the controller manifest and the controller inventories.
module Replacement.ModalRuntime.ServiceCategories
  ( Category (..)
  , categoryName
  , Seed (..)
  , seeds
  , SourceLine (..)
  , SourceFunction (..)
  , sourceFunctions
  , classify
  ) where

import Data.Char (isAlphaNum, isHexDigit, isSpace)
import Data.List (isPrefixOf)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Numeric (readHex)
import StockInventory (GuestAddress (..))

newtype SourceLine = SourceLine Int deriving (Eq, Ord, Show)
data Category = SettingsPropertyGates | FrameEditorBuffers | Rendering | Persistence | CapturePlaybackDevice | Input | ControllerRuntime | Runtime | Unclassified deriving (Eq, Ord, Show)
data Seed = Seed GuestAddress Category String String

data SourceFunction = SourceFunction
  { sourceEntry :: GuestAddress
  , sourceLine :: SourceLine
  , sourceEndLine :: SourceLine
  , sourceCalls :: S.Set GuestAddress
  , sourcePrivateInputs :: [(SourceLine, String)]
  } deriving (Eq, Show)

categoryName :: Category -> String
categoryName category = case category of
  SettingsPropertyGates -> "settings-property-gates"
  FrameEditorBuffers -> "frame-editor-buffers"
  Rendering -> "rendering"
  Persistence -> "persistence"
  CapturePlaybackDevice -> "capture-playback-device"
  Input -> "input"
  ControllerRuntime -> "controller-runtime"
  Runtime -> "runtime"
  Unclassified -> "unclassified"

seeds :: [Seed]
seeds =
  [ Seed (GuestAddress address) category meaning citation
  | (addresses, category, meaning, citation) <-
      [ ([0x63a0f8,0x6394d0,0x6394e8,0x6334dc,0x632950,0x63270c,0x632a3c], SettingsPropertyGates, "property or mode gate/access", "reports/recipe-bank-ui-ownership.md:298-323; replacement/owned_menu/src/lifecycle.rs")
      , ([0x61c91c,0x61c93c,0x61d514,0x61e110,0x631b74,0x631edc,0x633eac,0x643d94,0x6439e0,0x6464cc,0xd4fc74], SettingsPropertyGates, "property read/write/conversion/availability or commit; not a durability claim", "menu/recipe_bank_allowlist.json:47-109,219-223")
      , ([0x61ca2c,0x61cd14,0x61ceac,0x61d0e4,0x61d154,0x61d1a4,0x61d1f4,0x61d244,0x61d294,0x61d2e4,0x61d334], FrameEditorBuffers, "menu-frame snapshot service", "E-MENU-FRAME-SNAPSHOT-NATIVE-150; reports/recipe-bank-ui-ownership.md:390-440")
      , ([0xd4ad38,0xd45ce4,0xd45cf8,0x722a50,0x722178,0x7221c8,0x61ce1c], FrameEditorBuffers, "editor operation, preparation, buffer copy/clear or saved-snapshot reader", "menu/recipe_bank_arm.S:254,268-272; reports/recipe-bank-ui-ownership.md:430")
      , ([0xd38d98,0xd38e1c,0xd52684,0x1166200,0x111236c,0x11302f8,0x11301ac,0x1109130,0x6d7540], Rendering, "notification, redraw, row availability or named rendering entry", "menu/recipe_bank_arm.S:228-245; reports/replacement-inventory-baseline.md:1676-1694")
      , ([0x71c648,0x71c6fc,0x71c75c,0x71c7bc,0x6a8484,0x6ad3f4], Persistence, "file operation or settings append", "reports/named-recipe-reach.md:190-205; reports/replacement-inventory-baseline.md:483")
      , ([0x7115a8,0x7116e8,0x71176c,0x71c634,0x71eaac,0x71eff4,0x71fde4], Persistence, "media worker continuation or media completion/mapping/tag/message operation", "menu/recipe_bank_allowlist.json:110-154")
      , ([0x6357dc,0x635800,0x635824,0x635b60,0x64257c], SettingsPropertyGates, "lifecycle mode or white-balance operation; device effects not closed", "reports/recipe-bank-ui-ownership.md:304-323")
      , ([0x6439ac,0xe00a80,0x1215480], CapturePlaybackDevice, "playback mode apply or REC/playback actor entry", "menu/recipe_bank_allowlist.json:90-94; reports/replacement-inventory-baseline.md:1676-1686")
      , ([0xc6f14c,0xc70528,0xdb5124], Input, "menu key handler or queue1 input adapter", "menu/recipe_bank_arm.S:232-233; reports/replacement-inventory-baseline.md:1690")
      , ([0xbfb028,0xd0cc68,0xd3d858,0xc12aac,0xd38f34,0xd350c4], ControllerRuntime, "nested modal dispatch, prehook, stage, transition, postlude or mode selection", "menu/recipe_bank_arm.S:225-230,256-275; E-MODAL-DISPATCHER-ABI-150")
      , ([0x72503c,0x725318,0x725dd8,0x726098,0x89cda4,0x89cdc4,0x89da3c,0xbf1c1c,0x4d978], Runtime, "queue, task, heap or loader division", "reports/replacement-inventory-baseline.md:8.12-8.18; reports/stock-heap-survey.md")
      ]
  , address <- addresses
  ]

-- | Functions of the pinned decompilation keyed by their `// ===== <entry>` header.
sourceFunctions :: String -> M.Map GuestAddress SourceFunction
sourceFunctions text = M.fromList [(sourceEntry function, function) | function <- map make (go numbered)]
 where
  numbered = zip [1..] (lines text)
  header (_, line) = case words line of
    ["//", "=====", digits, _] | length digits == 8, all isHexDigit digits, [(address, "")] <- readHex digits -> Just (GuestAddress address)
    _ -> Nothing
  go [] = []
  go (row:rest) = case header row of
    Nothing -> go rest
    Just entry -> let (body, remaining) = break (maybe False (const True) . header) rest in (entry, row, body) : go remaining
  make (entry, first, body) = SourceFunction entry (SourceLine (fst first)) (SourceLine (fst (foldl (\_ row -> row) first body)))
    (S.fromList [GuestAddress address | (_, line) <- body, (token,suffix) <- lexemes line, "(" `isPrefixOf` dropWhile isSpace suffix, "FUN_" `isPrefixOf` token, length token == 12, [(address, "")] <- [readHex (drop 4 token)], GuestAddress address /= entry])
    [(SourceLine lineNumber, token) | (lineNumber, line) <- first : body, token <- tokens line, private token]
  tokens = map fst . lexemes
  lexemes [] = []
  lexemes text' = case dropWhile (\c -> not (isAlphaNum c || c == '_')) text' of
    [] -> []
    rest -> let (token, suffix) = span (\c -> isAlphaNum c || c == '_') rest in (token,suffix) : lexemes suffix
  private token = token `elem` ["unaff_r11", "unaff_r4", "unaff_r5", "unaff_r6", "unaff_r7", "unaff_r8", "unaff_r9", "unaff_r10", "in_r12", "in_lr", "unaff_lr"] || "in_stack_" `isPrefixOf` token

-- | Depth-first source-call closure from `entry` that stops at seeds. Returns
-- the first path found to each reached seed and the reached addresses that are
-- absent from the decompilation or call-free leaves other than `entry` and
-- the callback targets.
classify :: M.Map GuestAddress SourceFunction -> S.Set GuestAddress -> GuestAddress -> ([(Category,GuestAddress,[GuestAddress])], S.Set GuestAddress)
classify functions callbackTargets entry = go S.empty [(entry,[entry])] M.empty S.empty
 where
  seedMap = M.fromList [(address,category) | Seed address category _ _ <- seeds]
  go _ [] witnesses unresolved = (M.elems witnesses, unresolved)
  go seen ((address,path):pending) witnesses unresolved
    | S.member address seen = go seen pending witnesses unresolved
    | Just category <- M.lookup address seedMap = go (S.insert address seen) pending (M.insertWith (\_ old -> old) address (category,address,path) witnesses) unresolved
    | Just function <- M.lookup address functions =
        let references = S.toList (sourceCalls function)
            more = [(target,path++[target]) | target <- references, not (S.member target seen)]
            leafUnknown = null references && address /= entry && not (S.member address callbackTargets)
        in go (S.insert address seen) (more++pending) witnesses (if leafUnknown then S.insert address unresolved else unresolved)
    | otherwise = go (S.insert address seen) pending witnesses (S.insert address unresolved)
