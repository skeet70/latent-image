module RenderStack
  ( RenderStackReport (..)
  , RenderEntryReport (..)
  , checkRenderStack
  ) where

import qualified ArmElf as ELF
import Control.Monad (unless)
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
  margin = budgetStockMargin taskOneBudget
  check root headroom = do
    depth <- either (Left . show) Right (stackDepth elf inRuntime root)
    unless (toInteger depth + toInteger margin <= toInteger headroom)
      (Left ("render stack bound: " ++ root ++ " uses " ++ show depth
        ++ " + stock margin " ++ show margin ++ " bytes, headroom " ++ show headroom))
    pure (RenderEntryReport depth headroom margin)
