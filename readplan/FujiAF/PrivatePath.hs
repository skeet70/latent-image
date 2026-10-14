-- Location checks for the private paths both readplan tools take. Each check exits the program
-- with a message when the path is refused.
module FujiAF.PrivatePath
  ( privateArea
  , requirePrivateLocation
  , requirePrivateDirectory
  , requireNewPrivateFile
  ) where

import Control.Monad (filterM, unless, when)
import Data.Bits ((.&.))
import Data.List (isPrefixOf)
import System.Directory (canonicalizePath, doesPathExist)
import System.Exit (die)
import System.FilePath (addTrailingPathSeparator, isAbsolute, takeDirectory, (</>))
import System.Posix.Files (fileMode, fileOwner, getSymbolicLinkStatus, isDirectory, isSymbolicLink)
import System.Posix.User (getEffectiveUserID)

-- The ignored directory at a working-tree root that holds private research artifacts.
privateArea :: FilePath
privateArea = "private-data"

-- Accepts an absolute path that, with symlinks resolved, lies outside every Git working tree or
-- under the private-data directory of the innermost tree that contains it.
requirePrivateLocation :: FilePath -> IO ()
requirePrivateLocation path = do
  unless (isAbsolute path) (die ("absolute path required: " ++ path))
  resolved <- canonicalizePath path
  roots <- filterM (\directory -> doesPathExist (directory </> ".git")) (ancestors resolved)
  case roots of
    [] -> pure ()
    (root : _) ->
      unless (addTrailingPathSeparator (root </> privateArea) `isPrefixOf` addTrailingPathSeparator resolved)
        (die (path ++ " is inside the Git working tree at " ++ root ++ " and outside its " ++ privateArea ++ "/"))

requirePrivateDirectory :: FilePath -> IO ()
requirePrivateDirectory path = do
  requirePrivateLocation path
  exists <- doesPathExist path
  unless exists (die (path ++ " does not exist"))
  status <- getSymbolicLinkStatus path
  uid <- getEffectiveUserID
  unless (isDirectory status && not (isSymbolicLink status) && fileOwner status == uid
           && fileMode status .&. 0o077 == 0)
    (die (path ++ " must be an owned non-symlink directory with no group or other access"))

-- A path that does not exist yet, inside an existing private directory.
requireNewPrivateFile :: FilePath -> IO ()
requireNewPrivateFile path = do
  unless (isAbsolute path) (die ("absolute path required: " ++ path))
  exists <- doesPathExist path
  when exists (die ("output must be a new path: " ++ path))
  requirePrivateDirectory (takeDirectory path)
  requirePrivateLocation path

ancestors :: FilePath -> [FilePath]
ancestors path
  | parent == path = [path]
  | otherwise = path : ancestors parent
  where parent = takeDirectory path
