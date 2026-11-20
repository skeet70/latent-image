module RenderStack
  ( RenderStackReport (..)
  , RenderEntryReport (..)
  , checkRenderStack
  , checkRenderFrameStack
  , renderFrameHeadroom
  ) where

import qualified ArmElf as ELF
import Control.Monad (forM_, unless)
import Data.Word (Word32)
import EngineStack (StackBudget (..), stackDepth, taskOneBudget)

data RenderEntryReport = RenderEntryReport
  { entryDepth :: Word32
  , entryHeadroom :: Word32
  , entryStockMargin :: Word32
  }
  deriving (Eq, Show)

data RenderStackReport = RenderStackReport
  { reportTree :: RenderEntryReport
  , reportPicker :: RenderEntryReport
  , reportPane :: RenderEntryReport
  }
  deriving (Eq, Show)

-- Stock caller frames are byte-cited in display-render-increment-r1.md, prerequisite 2.
checkRenderStack :: ELF.Elf -> (Word32 -> Bool) -> Either String RenderStackReport
checkRenderStack elf inRuntime = do
  bankPrefix <- either (Left . show) Right (stackDepth elf inRuntime "rb_bank_draw_root")
  unless (bankPrefix < 4096 - 96) (Left "render stack bound: bank root prefix exhausts task 33")
  RenderStackReport
    <$> check "rb_render_tree_entry" 3416
    <*> check "rb_render_picker_entry" 3720
    <*> check "rb_render_pane_entry" (4096 - 96 - bankPrefix)
 where
  check = checkEntry elf inRuntime

-- | Task 73's 0x1000-byte stack less the 64 bytes its task body (24) and dispatcher `0x6d6ea8` (40) hold when
-- they enter `0x6d7920` (`I4:6d7540`, `6d6ea8`, `6d6f04`).
renderFrameHeadroom :: Word32
renderFrameHeadroom = 4096 - 64

-- | Bytes of the stock frame `rb_render_frame_entry` builds before either call (`I4:6d7920-6d7928`).
renderFrameEntryBytes :: Word32
renderFrameEntryBytes = 48

-- | The owned frame step on task 73. `rb_render_frame_apply` calls no stock code, so the whole entry is bounded by
-- the headroom alone; the stock services run from `rb_render_frame_commit` and `rb_render_frame_complete` above
-- the entry frame and keep the retained-stock margin. The scanout bridge and its one-shot run on interrupt stacks
-- in place of `0x6d80f0` and `0x6d7f30` and are bounded by `0x6d80f0`'s 32-byte frame (`I4:6d80f0-6d80f8`).
checkRenderFrameStack :: ELF.Elf -> (Word32 -> Bool) -> Either String RenderEntryReport
checkRenderFrameStack elf inRuntime = do
  whole <- depth "rb_render_frame_entry"
  unless (whole <= renderFrameHeadroom)
    (Left ("render stack bound: rb_render_frame_entry uses " ++ show whole ++ " bytes, headroom " ++ show renderFrameHeadroom))
  forM_ ["rb_render_frame_commit", "rb_render_frame_complete"] $ \name -> do
    used <- depth name
    unless (toInteger renderFrameEntryBytes + toInteger used + toInteger margin <= toInteger renderFrameHeadroom)
      (Left ("render stack bound: " ++ name ++ " uses " ++ show used ++ " + entry " ++ show renderFrameEntryBytes
        ++ " + stock margin " ++ show margin ++ " bytes, headroom " ++ show renderFrameHeadroom))
  forM_ ["rb_render_frame_scanout_bridge", "rb_render_frame_scanout_delivery"] $ \name -> do
    used <- depth name
    unless (used <= renderInterruptBytes)
      (Left ("render stack bound: " ++ name ++ " uses " ++ show used ++ " bytes, interrupt bound " ++ show renderInterruptBytes))
  pure (RenderEntryReport whole renderFrameHeadroom margin)
 where
  margin = budgetStockMargin taskOneBudget
  depth name = either (Left . show) Right (stackDepth elf inRuntime name)

renderInterruptBytes :: Word32
renderInterruptBytes = 32

checkEntry :: ELF.Elf -> (Word32 -> Bool) -> String -> Word32 -> Either String RenderEntryReport
checkEntry elf inRuntime root headroom = do
  depth <- either (Left . show) Right (stackDepth elf inRuntime root)
  unless (toInteger depth + toInteger margin <= toInteger headroom)
    (Left ("render stack bound: " ++ root ++ " uses " ++ show depth
      ++ " + stock margin " ++ show margin ++ " bytes, headroom " ++ show headroom))
  pure (RenderEntryReport depth headroom margin)
 where
  margin = budgetStockMargin taskOneBudget
