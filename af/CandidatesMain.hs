module Main (main) where

import Control.Exception (IOException, bracket, catch, throwIO)
import Control.Monad (forM_, unless)
import Data.List (intercalate)
import qualified Data.ByteString.Char8 as BC
import qualified FujiAF.Host as Host
import System.Environment (getArgs)
import System.Exit (ExitCode(..))
import System.IO (Handle, hClose, hFlush, hGetLine, hPutStrLn)
import System.Process
import System.Timeout (timeout)
import Text.Read (readMaybe)

newtype Position = Position Integer deriving (Eq, Ord, Show)
newtype Score = Score Integer deriving (Show)
newtype CurveName = CurveName String
newtype SampleCount = SampleCount Integer
newtype Travel = Travel Integer
data Candidate = A1 | A2 deriving (Show)
data Curve = Curve CurveName Position Position [(Position, Score)]
data Failure = Budget | FlatCurve | InvalidObservation | Cancelled deriving (Show, Read)
data Outcome = Focused Position | Rejected Failure

number :: String -> IO Integer
number text = maybe (fail "invalid curve integer") pure (readMaybe text)

loadCurve :: FilePath -> IO Curve
loadCurve path = do
  rows <- map words . lines . BC.unpack <$> Host.readRegular (128 * 1024) path
  case rows of
    ["synthetic", name, optimum, initial] : values -> do
      best <- Position <$> number optimum
      start <- Position <$> number initial
      points <- mapM point values
      let positions = map fst points
      unless (length points >= 3 && length points <= 4096 && head positions == Position 0
        && last positions >= Position 125 && last positions <= Position 65535 && and (zipWith (<) positions (drop 1 positions))
        && best >= head positions && best <= last positions && start >= head positions && start <= last positions)
        (fail "unsupported focus curve bounds")
      pure (Curve (CurveName name) best start points)
    _ -> fail "expected tagged synthetic curve"
  where
    point [p, s] = do
      position <- number p
      score <- number s
      unless (position >= 0 && position <= 65535 && score >= 0 && score <= 1000000) (fail "curve domain")
      pure (Position position, Score score)
    point _ = fail "expected position and score"

interpolate :: [(Position, Score)] -> Position -> IO Score
interpolate ((Position x, Score y) : rest@((Position x', Score y') : _)) target@(Position p)
  | x <= p && p <= x' = pure (Score (y + ((y' - y) * (p - x)) `quot` (x' - x)))
  | otherwise = interpolate rest target
interpolate _ _ = fail "candidate requested unsupported position"

runCandidate :: FilePath -> Candidate -> Curve -> IO (Outcome, SampleCount, Travel)
runCandidate binary candidate (Curve _ _ (Position start) points) =
  bracket open close $ \(input, output, process) -> do
    let Position high = fst (last points)
    hPutStrLn input (unwords ["CANDIDATE1", show candidate, show high])
    hFlush input
    result <- timeout 30000000 (loop input output start 0 0)
    answer <- maybe (fail "candidate host deadline") pure result
    hClose input
    status <- waitForProcess process
    unless (status == ExitSuccess) (fail "candidate adapter failed")
    pure answer
  where
    open = do
      handles <- createProcess (proc binary []) {std_in = CreatePipe, std_out = CreatePipe}
      case handles of
        (Just input, Just output, _, process) -> pure (input, output, process)
        _ -> fail "missing candidate pipes"
    close (input, output, process) = do
      terminateProcess process
      _ <- waitForProcess process
      mapM_ (\h -> hClose h `catchClosed` pure ()) [input, output]
    loop :: Handle -> Handle -> Integer -> Integer -> Integer -> IO (Outcome, SampleCount, Travel)
    loop input output previous count travel
      | count > 64 = fail "candidate sample budget"
      | otherwise = do
        fields <- words <$> hGetLine output
        case fields of
          ["OBSERVE", _, text] -> do
            position <- number text
            Score score <- interpolate points (Position position)
            hPutStrLn input (show score)
            hFlush input
            loop input output position (count + 1) (travel + abs (position - previous))
          ["FINISH", text, samples] -> do
            position <- number text
            reported <- number samples
            unless (reported == count && position == previous) (fail "candidate final accounting mismatch")
            pure (Focused (Position position), SampleCount count, Travel travel)
          ["FAIL", reason, samples] -> do
            reported <- number samples
            unless (reported == count) (fail "candidate failure accounting mismatch")
            failure <- maybe (fail "unknown candidate failure") pure (readMaybe reason)
            pure (Rejected failure, SampleCount count, Travel travel)
          _ -> fail "invalid candidate action"

catchClosed :: IO () -> IO () -> IO ()
catchClosed action fallback = catch action handler
  where
    handler :: IOException -> IO ()
    handler _ = fallback

main :: IO ()
main = do
  args <- getArgs
  case args of
    binary : paths@(_ : _) -> do
      putStrLn "origin,curve,candidate,sample_count,simulated_travel_units,final_position_error_units,outcome"
      forM_ paths $ \path -> do
        curve@(Curve (CurveName name) (Position optimum) _ _) <- loadCurve path
        forM_ [A1, A2] $ \candidate -> do
          (outcome, SampleCount count, Travel travel) <- runCandidate binary candidate curve
          let (err, status) = case outcome of
                Focused (Position position) -> (show (abs (position - optimum)), "focused")
                Rejected reason -> ("", show reason)
          putStrLn (intercalate "," ["simulated", name, show candidate, show count, show travel, err, status])
    _ -> throwIO (userError "usage: candidate-compare DECISION_BINARY SYNTHETIC.curve ...")
