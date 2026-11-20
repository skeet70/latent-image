{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | frame-presentation-check --stock RECEIPT --owned RECEIPT
--
-- Derives each role's presented-frame sequence from its frame-observation receipt
-- (`RENDER_FRAME_OBJECTS` in run-render-pairs.sh), checks each role's publication laws, then the
-- two roles against each other ("Replacement.ModalRuntime.FramePresentation").
module Main (main) where

import Control.Monad (forM, unless)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.Text as T
import Replacement.ModalRuntime.FramePresentation
import Replacement.ModalRuntime.MenuEnginePair
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import System.IO (hPutStrLn, stderr)

framePhases :: FrameRole -> FilePath -> IO [PhaseFrames]
framePhases role directory = do
  result <- B.readFile (directory </> "e2e-result.json") >>= either fail pure . J.eitherDecodeStrict'
  objects <- either (fail . ((directory ++ ": ") ++)) pure (declaredObjects role result)
  readers <- receiptObservations directory
  forM readers $ \reader -> do
    hits <- readerFoldHits reader (\kept hit -> Right (if isFrameSite hit then hit : kept else kept)) []
      >>= either (fail . ((directory ++ ": ") ++)) (pure . reverse)
    let observation = Observation (readerPhase reader) (readerResidency reader) (readerReads reader) hits Nothing
        built = PhaseFrames (readerPhase reader) <$> frameSteps role objects observation
          <*> displayRecordOf observation <*> frameRecordOf role observation
    either (fail . ((directory ++ ": ") ++)) pure built
 where
  isFrameSite hit = case hitSite hit of
    OtherSite name -> "frame-" `T.isPrefixOf` name
    _ -> False

summary :: String -> [PhaseFrames] -> String
summary name phases = name ++ ": " ++ unwords
  [ T.unpack (phaseName phase) ++ "=" ++ show (length (phaseSteps phase)) ++ "/" ++ either (const "?") (show . length) (presentedFrames phase)
  | phase <- phases ]

main :: IO ()
main = do
  arguments <- getArgs
  (stockPath, ownedPath) <- case arguments of
    ["--stock", stock, "--owned", owned] -> pure (stock, owned)
    _ -> hPutStrLn stderr "usage: frame-presentation-check --stock RECEIPT --owned RECEIPT" >> exitFailure
  stock <- framePhases StockFrames stockPath
  owned <- framePhases OwnedFrames ownedPath
  putStrLn (summary "stock steps/presented" stock)
  putStrLn (summary "owned steps/presented" owned)
  verdicts <- forM [("stock role", checkRoleFrames StockFrames stock), ("owned role", checkRoleFrames OwnedFrames owned), ("pair", comparePresentation stock owned)] $ \(name, verdict) -> do
    putStrLn (name ++ ": " ++ either ("refused: " ++) (const "passed") verdict)
    pure verdict
  unless (all (either (const False) (const True)) verdicts) exitFailure
