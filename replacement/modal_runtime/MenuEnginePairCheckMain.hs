{-# LANGUAGE GHC2021 #-}

-- | menu-engine-pair-check --tables MENU-TABLES.json --stock RECEIPT --owned RECEIPT
--
-- Checks one owned menu engine guest pair: each role's e2e observations by
-- "Replacement.ModalRuntime.MenuEnginePair", then the two roles against each other.
module Main (main) where

import Control.Monad (forM, unless)
import qualified Data.Aeson as J
import qualified Data.Aeson.Types as J
import qualified Data.ByteString as B
import Data.Char (isDigit)
import Data.List (sortOn, stripPrefix)
import qualified Data.Map.Strict as M
import Replacement.ModalRuntime.MenuEngineClass (menuTree)
import Replacement.ModalRuntime.MenuEnginePair
import Replacement.ModalRuntime.MenuEngineTree (TablesDocument)
import System.Directory (listDirectory)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import System.IO (hPutStrLn, stderr)

observations :: FilePath -> IO [Observation]
observations directory = do
  names <- listDirectory directory
  let numbered = [(read digits :: Int, name) | name <- names, Just rest <- [stripPrefix "e2e-observation-" name]
        , let digits = takeWhile isDigit rest, not (null digits), drop (length digits) rest == ".json"]
  forM (sortOn fst numbered) $ \(_, name) -> do
    bytes <- B.readFile (directory </> name)
    either (fail . ((directory </> name ++ ": ") ++)) pure (J.eitherDecodeStrict' bytes >>= J.parseEither parseObservation)

main :: IO ()
main = do
  arguments <- getArgs
  (tablesPath, stockPath, ownedPath) <- case arguments of
    ["--tables", tables, "--stock", stock, "--owned", owned] -> pure (tables, stock, owned)
    _ -> hPutStrLn stderr "usage: menu-engine-pair-check --tables MENU-TABLES.json --stock RECEIPT --owned RECEIPT" >> exitFailure
  document <- B.readFile tablesPath >>= either fail pure . (J.eitherDecodeStrict' :: B.ByteString -> Either String TablesDocument)
  let tree = menuTree document
  stock <- observations stockPath
  owned <- observations ownedPath
  unless (not (null stock) && not (null owned)) (fail "a role has no e2e observations")
  results <- forM [(StockRole, stock), (OwnedRole, owned)] $ \(role, segments) -> case checkRole tree role segments of
    Left failure -> hPutStrLn stderr (show role ++ ": " ++ failure) >> pure False
    Right summary -> do
      putStrLn (show role ++ ": " ++ show (summaryEvents summary) ++ " stage events, "
        ++ show (summaryFallbackEntries summary) ++ " fallback-site entries")
      mapM_ (\(class_, count) -> putStrLn ("  " ++ class_ ++ ": " ++ show count)) (M.toList (summaryClasses summary))
      pure True
  paired <- case comparePair stock owned of
    Left failure -> hPutStrLn stderr ("pair: " ++ failure) >> pure False
    Right () -> putStrLn "pair: engine reads and 0x1d posts equal" >> pure True
  unless (and (paired : results)) exitFailure
