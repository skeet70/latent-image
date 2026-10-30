-- Stage the owned record-2 image that links the loader-B core-1 prefix, and
-- publish the prefix entry as loader B's header entry word.
module Main where

import Control.Monad (forM, forM_, unless, when)
import Data.List (sortOn)
import Data.Word (Word32)
import FujiAF.FirstStage
import qualified FujiAF.UpdateContainer as U
import SecondaryStageContract
import StageBuild

loaderB :: LoaderId
loaderB = LoaderId 1

prefixSymbols :: [String]
prefixSymbols =
  [ "xm1_loader_prefix_entry"
  , "xm1_loader_prefix_install"
  , "xm1_loader_prefix_clear_workspace"
  , "xm1_loader_prefix_completion_cell"
  , "xm1_loader_prefix_dispatch"
  , "xm1_loader_prefix_exit"
  ]

forbiddenExtents :: [(String, String)]
forbiddenExtents =
  [ ("fetch", "entry")
  , ("read", "entry_pool")
  , ("fetch", "decision")
  , ("fetch", "wait")
  ]

continuations :: [String]
continuations = ["xm1_loader_b_core0_continuation", "xm1_loader_b_core1_continuation"]

inside :: (Word32, Word32) -> Word32 -> Bool
inside range value = within (both fromIntegral range) (fromIntegral value)
  where
    both f (start, end) = (f start, f end)

declarations :: [Symbol] -> Either String StageDeclarations
declarations symbols = do
  secondary <- secondaryDeclarations symbols
  let address name = symbolAddress <$> lookupSymbol symbols name
  image <- (,) <$> address "xm1_secondary_entry" <*> address "__secondary_stage_end"
  forM_ prefixSymbols $ \name -> do
    location <- address name
    unless (inside image location) (Left (name ++ " is outside the owned record-2 image"))
  loader <- (,) <$> address "xm1_loader_b_image_start" <*> address "xm1_loader_b_image_end"
  forbidden <- forM forbiddenExtents $ \(kind, name) -> do
    let prefix = "xm1_loader_b_forbid_" ++ kind ++ "_" ++ name
    range@(start, end) <- (,) <$> address (prefix ++ "_start") <*> address (prefix ++ "_end")
    unless (fst loader <= start && start < end && end <= snd loader)
      (Left (prefix ++ " is empty or outside loader B"))
    pure (kind, name, range)
  let ordered = sortOn (\(_, _, range) -> range) forbidden
  forM_ (zip ordered (drop 1 ordered)) $ \((_, name, (_, end)), (_, next, (start, _))) ->
    when (end > start) (Left ("loader-B forbidden " ++ name ++ " overlaps " ++ next))
  retained <- forM continuations $ \name -> do
    location <- address name
    unless (inside loader location) (Left (name ++ " is outside loader B"))
    when (any (\(_, _, range) -> inside range location) forbidden) (Left (name ++ " is inside a forbidden extent"))
    pure (name, location)
  stockEntry <- address "xm1_loader_b_stock_entry"
  unless ([start | ("fetch", "entry", (start, _)) <- forbidden] == [stockEntry])
    (Left "the stock loader-B entry does not start the forbidden entry extent")
  stock <- armEntry stockEntry
  entry <- address "xm1_loader_prefix_entry" >>= armEntry
  publication <- loaderEntryPublication loaderB (fst loader) stock entry
  let summary =
        ["retained loader-B continuation " ++ name ++ " " ++ U.showHexWord location | (name, location) <- retained]
          ++ [ "candidate loader-B forbidden " ++ kind ++ " " ++ name ++ " [" ++ U.showHexWord start ++ "," ++ U.showHexWord end ++ ")"
             | (kind, name, (start, end)) <- forbidden
             ]
  pure
    secondary
      { declaredLoaderPatches = [publication]
      , declaredSummary = declaredSummary secondary ++ summary
      }

main :: IO ()
main = runStage (StageContract SecondRecord declarations)
