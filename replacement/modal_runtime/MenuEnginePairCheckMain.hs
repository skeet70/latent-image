{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | menu-engine-pair-check [--render [--render-frame-record RECORD] ...] --tables MENU-TABLES.json --package-plan BANK-PLAN.json --stock RECEIPT --owned RECEIPT
--
-- Checks one owned menu engine guest pair against the menu tables composed with the installed
-- package's rows: each role's e2e observations by "Replacement.ModalRuntime.MenuEnginePair", then
-- the two roles against each other.
module Main (main) where

import qualified ArmElf as ELF
import Control.Monad (forM, forM_, unless, when)
import qualified Data.Aeson as J
import qualified Data.Aeson.Types as J
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString.Lazy as BL
import qualified Data.ByteString as B
import Data.List (nub, sort, stripPrefix)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import qualified EngineStack as S
import Emulation.ObservationStream (countObservationDrawersValue, loadReceiptObservationValues)
import qualified RenderStack as R
import Numeric (readHex)
import RamPushLayout (BankVariant, PublicationAddress (..), menuEngineHookAddress, menuValueHookAddress, modalEngineBankVariant, modalRenderBankVariant, modalRenderFrameBankVariant, modalValueBankVariant, parseBankVariantTag, renderHookAddresses, runtimeAllocationSizeFor, safePublicationOrderForVariant)
import Replacement.ModalRuntime.MenuEngineClass (MenuTree, menuTree)
import Replacement.ModalRuntime.MenuEnginePair
import Replacement.ModalRuntime.MenuEngineTree (TablesDocument, composePackage, parsePackagePlan)
import Replacement.ModalRuntime.RenderPair
import qualified Replacement.ModalRuntime.RenderStackEvidence as E
import System.Directory (doesFileExist)
import System.Environment (getArgs)
import System.Exit (exitFailure, exitSuccess)
import System.FilePath ((</>))
import System.IO (hPutStrLn, stderr)

data RenderSelection = OwnedRender RenderConfig | RetainedRender RenderConfig deriving (Eq)

mapSelection :: (RenderConfig -> RenderConfig) -> RenderSelection -> RenderSelection
mapSelection f (OwnedRender config) = OwnedRender (f config)
mapSelection f (RetainedRender config) = RetainedRender (f config)

sessionHits :: FilePath -> IO (Either String [Hit])
sessionHits directory = do
  let path = directory </> "emulator-session.json"
  present <- doesFileExist path
  if not present then pure (Right []) else do
    bytes <- B.readFile path
    pure (parseSessionClockHits bytes)

checkLifetime :: FilePath -> IO ()
checkLifetime directory = do
  bytes <- B.readFile (directory </> "e2e-modal-observation.json")
  lifetime <- either fail pure (J.eitherDecodeStrict' bytes)
  either fail pure (E.checkLifetimeCoverage lifetime)

runRetained :: RenderConfig -> Bool -> MenuTree -> TablesDocument -> TablesDocument -> FilePath -> FilePath -> IO Bool
runRetained config write tree stockTables document stockPath ownedPath = do
  stock <- receiptObservations stockPath
  owned <- receiptObservations ownedPath
  unless (not (null stock) && not (null owned)) (fail "a role has no e2e observations")
  results <- forM [(StockRole, stock), (OwnedRole, owned)] $ \(role, segments) -> do
    checked <- checkRoleReader tree role segments
    case checked of
      Left failure -> hPutStrLn stderr (show role ++ ": " ++ failure) >> pure False
      Right summary -> do
        putStrLn (show role ++ ": " ++ show (summaryEvents summary) ++ " stage events, "
          ++ show (summaryFallbackEntries summary) ++ " fallback-site entries")
        pure True
  manifestBytes <- B.readFile (ownedPath </> "relinked" </> "ram-push-manifest.json")
  base <- either fail pure (J.eitherDecodeStrict' manifestBytes >>= J.parseEither (J.withObject "manifest" $ \fields -> do
    baseText <- fields J..: "base" :: J.Parser String
    case stripPrefix "0x" baseText of
      Just digits | [(value, "")] <- readHex digits -> pure (value :: Word32)
      _ -> fail "manifest base is not hex"))
  variant <- either fail pure (J.eitherDecodeStrict' manifestBytes >>= J.parseEither validateEngineManifest)
  elf <- B.readFile (ownedPath </> "relinked" </> "kernel.elf") >>= either fail pure . ELF.parseElf
  let size = runtimeAllocationSizeFor variant
  valueLifecycle <- checkValue variant owned
  bound <- either fail pure (S.checkEngineStack S.taskOneBudget elf (\address -> base <= address && address - base < size))
  let margin = S.budgetStockMargin S.taskOneBudget
      required = S.reportEngine bound + S.reportModal bound + margin
  stacks <- forM [(StockRole, stock), (OwnedRole, owned)] $ \(role, segments) -> do
    checked <- checkStackReader required segments
    case checked of
      Left failure -> hPutStrLn stderr (show role ++ " stack: " ++ failure) >> pure False
      Right report -> do
        putStrLn (show role ++ " stack: lowest bound floor " ++ maybe "none" show (stackFloor report)
          ++ ", measured high-water mark " ++ maybe "none" show (stackHighWater report))
        pure True
  paired <- do
    checked <- comparePairReader stock owned
    case checked of
      Left failure -> hPutStrLn stderr ("pair: " ++ failure) >> pure False
      Right () -> putStrLn "pair: engine reads and 0x1d posts equal" >> pure True
  stockHits <- sessionHits stockPath >>= either fail pure
  ownedHits <- sessionHits ownedPath >>= either fail pure
  forM_ [(StockRole,stockPath,stock),(OwnedRole,ownedPath,owned)] $ \(_,path,segments) -> do
    value <- B.readFile (path </> "e2e-result.json") >>= either fail pure . J.eitherDecodeStrict'
    either fail pure (J.parseEither (validateRetainedRenderDeclarations config) value)
    when write (writeRenderReceipt Nothing path segments)
  let renderFailure problem = hPutStrLn stderr ("render: " ++ problem) >> pure False
  unless (map readerPhase stock == map readerPhase owned) (fail "render segment phases differ")
  coverage <- checkRenderCoverageReader config stock owned
  rendered <- case coverage of
    Left problem -> renderFailure problem
    Right () -> do
      clock <- checkConstantClockInputsReader (ClockEvidenceReader stockHits stock) (ClockEvidenceReader ownedHits owned)
      adapters <- forM (stock ++ owned) $ \reader -> readerFoldHits reader adapterStep ()
      case (clock, sequence adapters) of
        (Left problem, _) -> renderFailure problem
        (_, Left problem) -> renderFailure problem
        (Right (), Right _) -> do
          stockDrawsResult <- collectDrawsReader config stockTables document StockRole stock
          case stockDrawsResult of
            Left problem -> renderFailure problem
            Right stockDraws -> do
              drawsChecked <- compareDrawsReaderRole StockRole config stockTables document stockDraws owned
              case drawsChecked of
                Left problem -> renderFailure problem
                Right () -> do
                  stockStackResult <- checkRetainedRenderStackObservationsReader config stock
                  ownedStackResult <- checkRetainedRenderStackObservationsReader config owned
                  banksResult <- discoveredBanksReaderRole StockRole config stockTables document owned
                  case (stockStackResult, ownedStackResult, banksResult) of
                    (Left problem, _, _) -> renderFailure problem
                    (_, Left problem, _) -> renderFailure problem
                    (_, _, Left problem) -> renderFailure problem
                    (Right stockStack, Right ownedStack, Right ownedBanks) ->
                      case discoveredBanks config stockDraws of
                        Left problem -> renderFailure problem
                        Right stockBanks -> do
                          let banks = sort (nub (ownedBanks ++ stockBanks))
                          putStrLn ("render: " ++ show (renderCoverage config) ++ " stock-stack=" ++ show stockStack
                            ++ " owned-stack=" ++ show ownedStack ++ " banks=" ++ show banks)
                          pure True
  pure (and (results ++ stacks ++ [paired, rendered, valueLifecycle]))
 where
  adapterStep () hit = if noRenderAdapter hit then Right () else Left "retained render reached an owned adapter"

noRenderAdapter :: Hit -> Bool
noRenderAdapter hit = case hitSite hit of
  OtherSite label -> label `notElem` ["render-tree-adapter","render-picker-adapter","render-pane-adapter"]
  _ -> True

checkValue :: BankVariant -> [ObservationReader] -> IO Bool
checkValue variant segments
  | variant /= modalValueBankVariant = pure True
  | otherwise = do
      checked <- checkValueLifecycleReader segments
      case checked of
        Left failure -> hPutStrLn stderr ("value lifecycle: " ++ failure) >> pure False
        Right () -> putStrLn "value lifecycle: owned begin, apply, confirm, cancel and handback executed without the replaced stock arm" >> pure True

validateEngineManifest :: J.Value -> J.Parser BankVariant
validateEngineManifest = J.withObject "engine manifest" $ \fields -> do
  publications <- fields J..: "publications" >>= mapM (J.withObject "publication" $ \publication -> do
    address <- PublicationAddress <$> (publication J..: "address" >>= hexWord)
    old <- publication J..: "old" >>= hexWord
    pure (address,old))
  let variant = if menuValueHookAddress `elem` map fst publications then modalValueBankVariant else modalEngineBankVariant
  _ <- either fail pure (safePublicationOrderForVariant variant (map fst publications))
  unless (all (\(address,_) -> address `notElem` renderHookAddresses) publications) (fail "engine manifest publishes an owned render hook")
  unless ([old | (address,old) <- publications, address == menuEngineHookAddress] == [0xe92d4818]) (fail "engine manifest lacks the stock engine entry preimage")
  unless (variant /= modalValueBankVariant || [old | (address,old) <- publications, address == menuValueHookAddress] == [0x00d51b58]) (fail "value manifest lacks the stock IMAGE SIZE arm preimage")
  size <- fields J..: "size"
  unless (size == runtimeAllocationSizeFor variant) (fail "engine manifest allocation differs from its publication variant")
  pure variant
 where
  hexWord text = case stripPrefix "0x" text of
    Just digits | length digits == 8, [(value,"")] <- readHex digits -> pure (value :: Word32)
    _ -> fail "publication word requires eight hex digits"

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ["--count-drawers", directory] -> countDrawers directory
    _ -> runCheck arguments

countDrawers :: FilePath -> IO ()
countDrawers directory = do
  bytes <- B.readFile (directory </> "e2e-result.json")
  value <- either (ioError . userError) pure (J.eitherDecodeStrict' bytes)
  verified <- either (ioError . userError) pure =<< loadReceiptObservationValues directory value
  counts <- mapM (\(_, document) -> either (ioError . userError) pure =<< countObservationDrawersValue directory document) verified
  print (sum counts)

runCheck :: [String] -> IO ()
runCheck arguments = do
  (render, writeReceipt, frameRecord, tablesPath, planPath, stockPath, ownedPath) <- either fail pure (options arguments)
  publication <- forM frameRecord $ \path ->
    B.readFile path >>= either fail pure . (\bytes -> J.eitherDecodeStrict' bytes >>= J.parseEither parseFramePublication)
  stockTables <- B.readFile tablesPath >>= either fail pure . (J.eitherDecodeStrict' :: B.ByteString -> Either String TablesDocument)
  plan <- B.readFile planPath >>= either fail pure . J.eitherDecodeStrict'
  overlay <- either (fail . ("package plan: " ++)) pure (parsePackagePlan plan)
  document <- either (fail . ("package overlay: " ++)) pure (composePackage overlay stockTables)
  let tree = menuTree document
  case render of
    Just (RetainedRender config) -> do
      ok <- runRetained config writeReceipt tree stockTables document stockPath ownedPath
      unless ok exitFailure
      exitSuccess
    _ -> pure ()
  stock <- receiptObservations stockPath
  owned <- receiptObservations ownedPath
  unless (not (null stock) && not (null owned)) (fail "a role has no e2e observations")
  results <- forM [(StockRole, stock), (OwnedRole, owned)] $ \(role, segments) -> do
    checked <- checkRoleReader tree role segments
    case checked of
     Left failure -> hPutStrLn stderr (show role ++ ": " ++ failure) >> pure False
     Right summary -> do
      putStrLn (show role ++ ": " ++ show (summaryEvents summary) ++ " stage events, "
        ++ show (summaryFallbackEntries summary) ++ " fallback-site entries")
      let classes = M.toList (summaryClasses summary)
          label coverage = (if coverageResidency coverage == EngineResident then "resident" else "unloaded")
            ++ (if coverageOwned coverage then " owned " else " fallback ") ++ coverageClass coverage
          resident isEngine = [count | (coverage, count) <- classes, coverageResidency coverage == EngineResident, coverageOwned coverage == isEngine]
      mapM_ (\(coverage, count) -> putStrLn ("  " ++ label coverage ++ ": " ++ show count)) classes
      putStrLn ("  while resident: " ++ show (sum (resident True)) ++ " owned events in " ++ show (length (resident True))
        ++ " classes, " ++ show (sum (resident False)) ++ " fallback events in " ++ show (length (resident False)) ++ " classes")
      pure True
  -- The bound is the owned build's: its relinked ELF and manifest sit in the owned receipt.
  manifestBytes <- B.readFile (ownedPath </> "relinked" </> "ram-push-manifest.json")
  base <- either fail pure (J.eitherDecodeStrict' manifestBytes >>= J.parseEither (J.withObject "manifest" $ \fields -> do
    baseText <- fields J..: "base" :: J.Parser String
    case stripPrefix "0x" baseText of
      Just digits | [(value, "")] <- readHex digits -> pure (value :: Word32)
      _ -> fail "manifest base is not hex"))
  elf <- B.readFile (ownedPath </> "relinked" </> "kernel.elf") >>= either fail pure . ELF.parseElf
  variant <- case render of
    Nothing -> either fail pure (J.eitherDecodeStrict' manifestBytes >>= J.parseEither validateEngineManifest)
    Just _ -> do
      tagged <- either fail pure (J.eitherDecodeStrict' manifestBytes >>= J.parseEither (J.withObject "manifest" (J..: "bank_variant")))
      selected <- either fail pure (parseBankVariantTag tagged)
      unless (selected `elem` [modalRenderBankVariant, modalRenderFrameBankVariant]) (fail "render manifest is neither the render nor the render-frame variant")
      unless ((selected == modalRenderFrameBankVariant) == (publication /= Nothing)) (fail "--render-frame-record names the render-frame variant's record, and only that variant's")
      pure selected
  let size = runtimeAllocationSizeFor variant
  valueLifecycle <- checkValue variant owned
  bound <- either fail pure (S.checkEngineStack S.taskOneBudget elf (\address -> base <= address && address - base < size))
  let margin = S.budgetStockMargin S.taskOneBudget
      required = S.reportEngine bound + S.reportModal bound + margin
  putStrLn ("stack: engine " ++ show (S.reportEngine bound) ++ " + modal dispatcher " ++ show (S.reportModal bound)
    ++ " + stock margin " ++ show margin ++ " = " ++ show required ++ " bytes")
  stacks <- forM [(StockRole, stock), (OwnedRole, owned)] $ \(role, segments) -> do
    checked <- checkStackReader required segments
    case checked of
     Left failure -> hPutStrLn stderr (show role ++ " stack: " ++ failure) >> pure False
     Right report -> do
      putStrLn (show role ++ " stack: lowest bound floor " ++ maybe "none" show (stackFloor report)
        ++ ", measured high-water mark " ++ maybe "none" show (stackHighWater report))
      pure True
  pairChecked <- comparePairReader stock owned
  paired <- case pairChecked of
    Left failure -> hPutStrLn stderr ("pair: " ++ failure) >> pure False
    Right () -> putStrLn "pair: engine reads and 0x1d posts equal" >> pure True
  rendered <- case render of
    Nothing -> pure True
    Just (RetainedRender _) -> pure True
    Just (OwnedRender config) -> do
      forM_ [(StockRole,stockPath,stock,Nothing),(OwnedRole,ownedPath,owned,publication)] $ \(role,path,segments,frame) -> do
        value <- B.readFile (path </> "e2e-result.json") >>= either fail pure . J.eitherDecodeStrict'
        either fail pure (J.parseEither (validateRenderDeclarations config role frame) value)
        writeRenderReceipt frame path segments
      renderBounds <- either fail pure (R.checkRenderStack elf (\address -> base <= address && address - base < size))
      forM_ [stockPath, ownedPath] checkLifetime
      checked <- (if map readerPhase stock /= map readerPhase owned
        then pure (Left "render segment phases differ")
        else do
          coverage <- checkRenderCoverageReader config stock owned
          case coverage of
            Left failure -> pure (Left failure)
            Right () -> do
              stockDrawsResult <- collectDrawsReader config stockTables document StockRole stock
              case stockDrawsResult of
                Left failure -> pure (Left failure)
                Right stockDraws -> do
                  drawsChecked <- compareDrawsReader config stockTables document stockDraws owned
                  case drawsChecked of
                    Left failure -> pure (Left failure)
                    Right () -> do
                      stockStackResult <- checkRenderStackObservationsReader config renderBounds stockTables document stock
                      ownedStackResult <- checkRenderStackObservationsReader config renderBounds stockTables document owned
                      banksResult <- discoveredBanksReader config stockTables document owned
                      let stockBanksResult = discoveredBanks config stockDraws
                      case (stockStackResult, ownedStackResult, banksResult, stockBanksResult) of
                        (Left failure, _, _, _) -> pure (Left failure)
                        (_, Left failure, _, _) -> pure (Left failure)
                        (_, _, Left failure, _) -> pure (Left failure)
                        (_, _, _, Left failure) -> pure (Left failure)
                        (Right stockStack, Right ownedStack, Right ownedBanks, Right stockBanks) ->
                          pure (Right (stockStack, ownedStack, sort (nub (stockBanks ++ ownedBanks)))))
      case checked of
        Left failure -> hPutStrLn stderr ("render: " ++ failure) >> pure False
        Right (stockStack,ownedStack,banks) -> do
          putStrLn ("render: " ++ show (renderCoverage config) ++ " changed-word lower-bound reports "
            ++ show (stockStack,ownedStack))
          BL.writeFile (ownedPath </> "render-banks.json") (J.encode banks <> "\n")
          pure True
  unless (and (valueLifecycle : rendered : paired : results ++ stacks)) exitFailure

options :: [String] -> Either String (Maybe RenderSelection,Bool,Maybe FilePath,FilePath,FilePath,FilePath,FilePath)
options = go Nothing False Nothing
 where
  defaults = RenderConfig [0..18] Nothing 0xa4ce00 DrawsRequired
  go selection _write frame ("--write-render-receipt":rest) = go selection True frame rest
  go Nothing write frame ("--render":rest) = go (Just (OwnedRender defaults)) write frame rest
  go Nothing write frame ("--retained-render":rest) = go (Just (RetainedRender (defaults {renderCoverage = RetainedObservation}))) write frame rest
  go (Just selection@(OwnedRender _)) write Nothing ("--render-frame-record":record:rest) = go (Just selection) write (Just record) rest
  go (Just selection) write frame ("--render-draw-free":rest) = go (Just (mapSelection (\config -> config {renderCoverage = DeclaredDrawFree}) selection)) write frame rest
  go (Just selection) write frame ("--render-banks":banks:rest) = do
    parsed <- J.eitherDecodeStrict' (B.pack (map (fromIntegral . fromEnum) banks))
    go (Just (mapSelection (\config -> config {renderBanks = parsed}) selection)) write frame rest
  go (Just selection) write frame ("--render-clock-resource":resource:rest) = case stripPrefix "0x" resource of
    Just digits | [(value,"")] <- readHex digits -> go (Just (mapSelection (\config -> config {renderClock = Just (ResourceId value)}) selection)) write frame rest
    _ -> Left "render clock resource requires a hex word"
  go (Just selection) write frame ("--render-stack-base":address:rest) = case stripPrefix "0x" address of
    Just digits | [(value,"")] <- readHex digits -> go (Just (mapSelection (\config -> config {renderStackBase = value}) selection)) write frame rest
    _ -> Left "render stack base requires a hex word"
  go selection write frame ["--tables",tables,"--package-plan",plan,"--stock",stock,"--owned",owned] = Right (selection,write,frame,tables,plan,stock,owned)
  go _ _ _ _ = Left "usage: menu-engine-pair-check [--render [--render-frame-record RECORD]|--retained-render [--write-render-receipt] [--render-draw-free] [--render-banks JSON] [--render-clock-resource 0xID] [--render-stack-base 0xBASE]] --tables TABLES --package-plan PLAN --stock RECEIPT --owned RECEIPT"

-- | `publication` is the role's render-frame package, if it installed one.
writeRenderReceipt :: Maybe FramePublication -> FilePath -> [ObservationReader] -> IO ()
writeRenderReceipt publication directory segments = do
  fronts <- either fail pure =<< osdInputsReader publication segments
  producers <- if all (\segment -> "main-shadow" `notElem` map fst (readerReads segment)) segments then pure []
    else either fail pure =<< mainProducerInputsReader segments
  labels <- fmap concat $ forM (zip [0 :: Int ..] segments) $ \(index,segment) -> do
    bytes <- B.readFile (directory </> ("e2e-observation-" ++ show index ++ ".json"))
    label <- either fail pure (J.eitherDecodeStrict' bytes >>= J.parseEither (J.withObject "observation" (\fields -> fields J..:? "capture" :: J.Parser (Maybe String))))
    pure [(name,(input,lookup (readerPhase segment) producers)) | Just name <- [label], Just input <- [lookup (readerPhase segment) fronts]]
  value <- B.readFile (directory </> "e2e-result.json") >>= either fail pure . J.eitherDecodeStrict'
  annotated <- either fail pure (J.parseEither (J.withObject "e2e result" $ \fields -> do
    captures <- fields J..: "menu_receipts" :: J.Parser [J.Value]
    updated <- forM captures $ J.withObject "capture" $ \capture -> do
      label <- capture J..: "label"
      (input,producer) <- maybe (fail "capture lacks complete OSD front inputs") pure (lookup label labels)
      when (not (null producers) && producer == Nothing) (fail "capture lacks main producer inputs")
      pure (J.Object (maybe id (KM.insert "main_producer") producer (KM.insert "osd_front" input capture)))
    pure (J.Object (KM.insert "menu_receipts" (J.toJSON updated) fields))) value)
  BL.writeFile (directory </> "e2e-result-render.json") (J.encode annotated <> "\n")
