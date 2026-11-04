{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | menu-engine-pair-check --tables MENU-TABLES.json --package-plan BANK-PLAN.json --stock RECEIPT --owned RECEIPT
--
-- Checks one owned menu engine guest pair against the menu tables composed with the installed
-- package's rows: each role's e2e observations by "Replacement.ModalRuntime.MenuEnginePair", then
-- the two roles against each other.
module Main (main) where

import qualified ArmElf as ELF
import Control.Monad (forM, unless)
import qualified Data.Aeson as J
import qualified Data.Aeson.Types as J
import qualified Data.ByteString as B
import Data.Char (isDigit)
import Data.List (sortOn, stripPrefix)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import qualified EngineStack as S
import Numeric (readHex)
import RamPushLayout (modalEngineBankVariant, runtimeAllocationSizeFor)
import Replacement.ModalRuntime.MenuEngineClass (menuTree)
import Replacement.ModalRuntime.MenuEnginePair
import Replacement.ModalRuntime.MenuEngineTree (TablesDocument, composePackage, parsePackagePlan)
import System.Directory (listDirectory)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import System.IO (hPutStrLn, stderr)

-- | The receipt's observations; segments after its `ram-unload` phase are marked unloaded.
observations :: FilePath -> IO [Observation]
observations directory = do
  resultBytes <- B.readFile (directory </> "e2e-result.json")
  phases <- either (fail . ((directory </> "e2e-result.json: ") ++)) pure
    (J.eitherDecodeStrict' resultBytes >>= J.parseEither (J.withObject "e2e result" $ \fields ->
      fields J..: "phases" >>= mapM (J.withObject "phase" (J..: "name"))))
  names <- listDirectory directory
  let numbered = [(read digits :: Int, name) | name <- names, Just rest <- [stripPrefix "e2e-observation-" name]
        , let digits = takeWhile isDigit rest, not (null digits), drop (length digits) rest == ".json"]
  forM (sortOn fst numbered) $ \(_, name) -> do
    bytes <- B.readFile (directory </> name)
    either (fail . ((directory </> name ++ ": ") ++)) pure (J.eitherDecodeStrict' bytes >>= J.parseEither (parseObservation (unloadedPhases phases)))

main :: IO ()
main = do
  arguments <- getArgs
  (tablesPath, planPath, stockPath, ownedPath) <- case arguments of
    ["--tables", tables, "--package-plan", plan, "--stock", stock, "--owned", owned] -> pure (tables, plan, stock, owned)
    _ -> hPutStrLn stderr "usage: menu-engine-pair-check --tables MENU-TABLES.json --package-plan BANK-PLAN.json --stock RECEIPT --owned RECEIPT" >> exitFailure
  stockTables <- B.readFile tablesPath >>= either fail pure . (J.eitherDecodeStrict' :: B.ByteString -> Either String TablesDocument)
  plan <- B.readFile planPath >>= either fail pure . J.eitherDecodeStrict'
  overlay <- either (fail . ("package plan: " ++)) pure (parsePackagePlan plan)
  document <- either (fail . ("package overlay: " ++)) pure (composePackage overlay stockTables)
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
  -- The bound is the owned build's: its relinked ELF and manifest sit in the owned receipt.
  manifestBytes <- B.readFile (ownedPath </> "relinked" </> "ram-push-manifest.json")
  base <- either fail pure (J.eitherDecodeStrict' manifestBytes >>= J.parseEither (J.withObject "manifest" $ \fields -> do
    baseText <- fields J..: "base" :: J.Parser String
    case stripPrefix "0x" baseText of
      Just digits | [(value, "")] <- readHex digits -> pure (value :: Word32)
      _ -> fail "manifest base is not hex"))
  elf <- B.readFile (ownedPath </> "relinked" </> "kernel.elf") >>= either fail pure . ELF.parseElf
  let size = runtimeAllocationSizeFor modalEngineBankVariant
  bound <- either fail pure (S.checkEngineStack S.taskOneBudget elf (\address -> base <= address && address - base < size))
  let margin = S.budgetStockMargin S.taskOneBudget
      required = S.reportEngine bound + S.reportModal bound + margin
  putStrLn ("stack: engine " ++ show (S.reportEngine bound) ++ " + modal dispatcher " ++ show (S.reportModal bound)
    ++ " + stock margin " ++ show margin ++ " = " ++ show required ++ " bytes")
  stacks <- forM [(StockRole, stock), (OwnedRole, owned)] $ \(role, segments) -> case checkStack required segments of
    Left failure -> hPutStrLn stderr (show role ++ " stack: " ++ failure) >> pure False
    Right report -> do
      putStrLn (show role ++ " stack: lowest bound floor " ++ maybe "none" show (stackFloor report)
        ++ ", measured high-water mark " ++ maybe "none" show (stackHighWater report))
      pure True
  paired <- case comparePair stock owned of
    Left failure -> hPutStrLn stderr ("pair: " ++ failure) >> pure False
    Right () -> putStrLn "pair: engine reads and 0x1d posts equal" >> pure True
  unless (and (paired : results ++ stacks)) exitFailure
