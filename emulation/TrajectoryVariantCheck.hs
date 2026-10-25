-- | Checks that a FujiMod trajectory variant asserts exactly what its retained trajectory file
-- asserts, differing at most in press timing (segment key pulses and step timing).
module Main (main) where

import qualified Data.ByteString as B
import Emulation.UiTrajectory (decodeTrajectories, variantMismatches)
import System.Directory (makeAbsolute)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>))

main :: IO ()
main = do
  arguments <- getArgs
  (retainedPath, variantPath) <- case arguments of
    [retained, variant] -> pure (retained, variant)
    _ -> ioError (userError "usage: trajectory-variant-check RETAINED.json VARIANT.json")
  workspace <- lookupEnv "BUILD_WORKSPACE_DIRECTORY"
  let resolve path = maybe (makeAbsolute path) (\root -> pure (root </> path)) workspace
  retained <- resolve retainedPath >>= B.readFile
  variant <- resolve variantPath >>= B.readFile
  case (,) <$> decodeTrajectories retained <*> decodeTrajectories variant of
    Left message -> putStrLn ("trajectory-variant-check: " ++ message) >> exitFailure
    Right (old, new) -> case variantMismatches old new of
      [] -> putStrLn ("trajectory-variant-check: " ++ variantPath ++ " asserts what " ++ retainedPath ++ " asserts")
      mismatches -> mapM_ (putStrLn . ("trajectory-variant-check: " ++)) mismatches >> exitFailure
