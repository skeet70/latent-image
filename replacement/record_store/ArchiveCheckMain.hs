{-# LANGUAGE GHC2021 #-}
module Main (main) where

import Control.Monad (forM_, unless)
import Data.List (isInfixOf, isPrefixOf)
import qualified Data.Set as Set
import System.Environment (getArgs)
import System.Exit (ExitCode(..), die)
import System.Process (readProcessWithExitCode)

newtype NmTool = NmTool FilePath
newtype ArchivePath = ArchivePath FilePath
newtype ArchiveMember = ArchiveMember String
newtype Symbol = Symbol String deriving (Eq, Ord, Show)
data Entry = Entry { member :: ArchiveMember, symbol :: Symbol }

main :: IO ()
main = do
  args <- getArgs
  case args of
    nm : archives@(_ : _) -> forM_ archives (checkArchive (NmTool nm) . ArchivePath)
    _ -> die "usage: archive-check LLVM_NM ARCHIVE..."

checkArchive :: NmTool -> ArchivePath -> IO ()
checkArchive (NmTool nm) (ArchivePath archive) = do
  undefinedEntries <- entries ["--undefined-only"]
  definedEntries <- entries ["--defined-only"]
  let owned = filter (ownedMember . member) undefinedEntries
      builtins = Set.fromList [symbol entry | entry <- definedEntries, builtinMember (member entry)]
      forbidden = [symbol entry | entry <- owned, isAeabi (symbol entry) || Set.member (symbol entry) builtins]
  unless (not (null owned)) $ die (archive ++ ": no owned crate dependencies found")
  unless (null forbidden) $ die (archive ++ ": forbidden guest runtime dependencies: " ++ show forbidden)
  putStrLn (archive ++ ": owned crate references no AEABI or compiler-builtins symbol")
  where
    entries flags = do
      (status, out, err) <- readProcessWithExitCode nm (["--print-file-name"] ++ flags ++ [archive]) ""
      unless (status == ExitSuccess) $ die (archive ++ ": llvm-nm failed: " ++ err)
      traverse parseEntry (filter (not . null) (lines out))

parseEntry :: String -> IO Entry
parseEntry line = case words line of
  [owner, "U", name] -> pure (Entry (ArchiveMember owner) (Symbol name))
  [owner, _, _, name] -> pure (Entry (ArchiveMember owner) (Symbol name))
  _ -> die ("unrecognized llvm-nm row: " ++ line)

ownedMember :: ArchiveMember -> Bool
ownedMember (ArchiveMember owner) = ".xm1_record_store." `isInfixOf` owner

builtinMember :: ArchiveMember -> Bool
builtinMember (ArchiveMember owner) = ":compiler_builtins-" `isInfixOf` owner

isAeabi :: Symbol -> Bool
isAeabi (Symbol name) = "__aeabi_" `isPrefixOf` name
