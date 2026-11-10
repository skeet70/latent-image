{-# LANGUAGE GHC2021 #-}
-- | The oracle runs with cwd at the real bazel-out bin directory.
module Main (main) where

import qualified Data.ByteString as B
import System.Directory (canonicalizePath, doesFileExist, setCurrentDirectory)
import System.Environment (getArgs, getExecutablePath)
import System.FilePath (takeDirectory, takeFileName, (</>))
import System.Posix.Process (executeFile)

main :: IO ()
main = do
  executable <- getExecutablePath
  let runfiles = executable ++ ".runfiles" </> "_main"
  args <- resolveControlArgs runfiles =<< getArgs
  let relative = executable ++ ".runfiles" </> "_main" </> "replacement" </> "settings_storage" </> "custom-oracle"
  oracle <- canonicalizePath relative
  magic <- B.take 4 <$> B.readFile oracle
  if magic /= B.pack [0x7f, 0x45, 0x4c, 0x46]
    then ioError (userError ("custom oracle is not an ELF executable: " ++ oracle))
    else do
      setCurrentDirectory (takeDirectoryN 3 oracle)
      executeFile oracle True args Nothing

resolveControlArgs :: FilePath -> [String] -> IO [String]
resolveControlArgs root = go
  where
    go ("--full-export-control-producer":path:rest) = do
      resolved <- resolve path
      rest' <- go rest
      pure ("--full-export-control-producer":resolved:rest')
    go ("--full-export-control-correct":path:rest) = do
      resolved <- resolve path
      rest' <- go rest
      pure ("--full-export-control-correct":resolved:rest')
    go ("--full-export-control-mutant":path:rest) = do
      resolved <- resolve path
      rest' <- go rest
      pure ("--full-export-control-mutant":resolved:rest')
    go (arg:rest) = (arg:) <$> go rest
    go [] = pure []
    resolve path = do
      let candidate = root </> "replacement" </> "settings_storage" </> takeFileName path
      present <- doesFileExist candidate
      pure (if present then candidate else path)

takeDirectoryN :: Int -> FilePath -> FilePath
takeDirectoryN n path
  | n <= 0 = path
  | otherwise = takeDirectoryN (n - 1) (takeDirectory path)
