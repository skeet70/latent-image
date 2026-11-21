{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | frame-submission-check --stock RECEIPT --owned RECEIPT --out SUMMARY_JSON
--
-- Derives each role's stage-0 submitted work per frame step from its submission-observation receipt
-- (`RENDER_SUBMISSION` and `RENDER_FRAME_OBJECTS` in run-render-pairs.sh) and requires the roles to submit equal
-- operations in order ("Replacement.ModalRuntime.FrameSubmission"), with at least one frame the owned step rendered.
-- Writes the comparison and both roles' guest instructions per frame step. Exit 0 on equality, 1 otherwise.
module Main (main) where

import Control.Monad (forM, unless)
import qualified Data.Aeson as J
import Data.Aeson ((.=))
import qualified Data.ByteString.Lazy as BL
import qualified Data.ByteString as B
import Data.List (sort)
import Data.Maybe (mapMaybe)
import qualified Data.Text as T
import Data.Word (Word64)
import Numeric (showHex)
import Replacement.ModalRuntime.FrameSubmission
import Replacement.ModalRuntime.MenuEnginePair
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)

phases :: FilePath -> IO [PhaseSubmission]
phases directory = do
  readers <- receiptObservations directory
  forM readers $ \reader -> do
    hits <- readerFoldHits reader (\kept hit -> Right (if relevant hit then hit : kept else kept)) []
      >>= either (fail . ((directory ++ ": ") ++)) (pure . reverse)
    either (fail . ((directory ++ ": ") ++)) pure (phaseSubmission (readerPhase reader) hits)
 where
  relevant hit = case hitSite hit of
    OtherSite name -> name `elem` ["frame-step-entry", "frame-dispatch-return"] || submissionSite name /= Nothing
    _ -> False

statistics :: [Word64] -> J.Value
statistics [] = J.Null
statistics values = J.object
  [ "frames" .= length values, "mean" .= (fromIntegral (sum values) / fromIntegral (length values) :: Double)
  , "median" .= (sort values !! (length values `div` 2)), "min" .= minimum values, "max" .= maximum values ]

operationValue :: Operation -> J.Value
operationValue (Operation site values bytes) = J.object
  [ "site" .= show site, "values" .= map (\word -> "0x" ++ showHex word "") values
  , "bytes" .= concatMap (\byte -> let digits = showHex byte "" in replicate (2 - length digits) '0' ++ digits) (B.unpack bytes) ]

main :: IO ()
main = do
  arguments <- getArgs
  (stockPath, ownedPath, out) <- case arguments of
    ["--stock", stock, "--owned", owned, "--out", path] -> pure (stock, owned, path)
    _ -> hPutStrLn stderr "usage: frame-submission-check --stock RECEIPT --owned RECEIPT --out SUMMARY_JSON" >> exitFailure
  stock <- phases stockPath
  owned <- phases ownedPath
  let frames = concatMap phaseFrames
      count select = length . concatMap select . frames
      ownedFrames = length (filter ((== OwnedStep) . framePath) (frames owned))
      instructions role path = mapMaybe frameInstructions (filter ((== path) . framePath) (frames role))
      comparison = compareSubmission stock owned
      passed = comparison == Right Nothing && ownedFrames > 0
      summary = J.object
        [ "schema" .= (1 :: Int), "stock" .= stockPath, "owned" .= ownedPath
        , "phases" .= [ J.object ["phase" .= phaseLabel phase, "frames" .= length (phaseFrames phase)] | phase <- owned ]
        , "frames" .= length (frames owned), "owned_frames" .= ownedFrames
        , "calls" .= count frameCalls stock, "engine_operations" .= count frameEngine stock
        , "jobs" .= length [() | Operation JobSubmit _ _ <- concatMap frameCalls (frames stock)]
        , "instructions" .= J.object
            [ "stock" .= statistics (instructions stock StockStep)
            , "owned_rendered" .= statistics (instructions owned OwnedStep)
            , "stock_on_owned_frames" .= statistics
                [ value | (s, o) <- zip (frames stock) (frames owned), framePath o == OwnedStep, Just value <- [frameInstructions s] ] ]
        , "comparison" .= case comparison of
            Left problem -> J.object ["refused" .= problem]
            Right Nothing -> J.object ["equal" .= True]
            Right (Just mismatch) -> J.object
              [ "equal" .= False, "phase" .= mismatchPhase mismatch, "frame" .= mismatchFrame mismatch
              , "stream" .= show (mismatchStream mismatch), "index" .= mismatchIndex mismatch
              , "stock_operation" .= fmap operationValue (mismatchStock mismatch)
              , "owned_operation" .= fmap operationValue (mismatchOwned mismatch) ]
        , "passed" .= passed ]
  BL.writeFile out (J.encode summary <> "\n")
  putStrLn (T.unpack (T.intercalate " " [phaseLabel phase <> "=" <> T.pack (show (length (phaseFrames phase))) | phase <- owned]))
  putStrLn ("owned frames " ++ show ownedFrames ++ "; " ++ either ("refused: " ++) (maybe "equal" (("first difference: " ++) . show)) comparison)
  unless passed exitFailure
