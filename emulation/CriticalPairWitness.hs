module Main where

import Control.Monad (when)
import qualified Data.Aeson as J
import Data.Bits (complement)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.List (isPrefixOf)
import qualified Emulation.CriticalPairWitness as W
import Emulation.EmulatorSlot (reexecInSlot, slotPolicyFromEnvironment)
import qualified Emulation.InterfaceInitializer as I
import Emulation.ViewerRuntime (Workspace (..), Runtime (..), DefaultRuntime (..), freshRuntime, resolve, workspace)
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import System.Directory (canonicalizePath, doesFileExist, setCurrentDirectory)
import System.Environment (getArgs)
import System.Exit (ExitCode (..))
import System.FilePath ((</>), splitDirectories, takeDirectory, takeFileName)
import System.IO (stderr)
import System.Posix.Files (setFileMode)
import System.Process (showCommandForUser)

newtype Decoder = Decoder FilePath
newtype SourceDat = SourceDat FilePath
newtype CandidateDat = CandidateDat FilePath
newtype LinkedImage = LinkedImage FilePath
newtype LinkedSymbols = LinkedSymbols FilePath
newtype Output = Output FilePath
newtype ReceiptPath = ReceiptPath FilePath

data Inputs = Retained SourceDat | Owned SourceDat CandidateDat LinkedImage LinkedSymbols
data Launch = Launch FilePath FilePath FilePath

data Command = Prepare Inputs Decoder Output (Maybe Launch) | Consume Inputs Decoder Output ReceiptPath | Compare Inputs Decoder Output ReceiptPath ReceiptPath | Help

usage :: String
usage = unlines
  [ "critical-pair-witness prepare retained SOURCE_DAT DECODER OUTPUT"
  , "critical-pair-witness prepare owned SOURCE_DAT CANDIDATE_DAT IMAGE SYMBOLS DECODER OUTPUT"
  , "critical-pair-witness launch retained SOURCE_DAT DECODER OUTPUT DRIVER RUN BOOT_ROM"
  , "critical-pair-witness launch owned SOURCE_DAT CANDIDATE_DAT IMAGE SYMBOLS DECODER OUTPUT DRIVER RUN BOOT_ROM"
  , "critical-pair-witness consume retained SOURCE_DAT DECODER SCRATCH RECEIPT"
  , "critical-pair-witness consume owned SOURCE_DAT CANDIDATE_DAT IMAGE SYMBOLS DECODER SCRATCH RECEIPT"
  , "critical-pair-witness compare SOURCE_DAT CANDIDATE_DAT IMAGE SYMBOLS DECODER SCRATCH RETAINED_RECEIPT OWNED_RECEIPT"
  , "OUTPUT must be a new directory under private-data/scratch or private-data/verify-candidate/runs."
  , "SCRATCH must be an existing directory under either private output root."
  , "Preparation and independent consumption do not launch an emulator."
  ]

parse :: [String] -> Either String Command
parse ["--help"] = Right Help
parse ["prepare","retained",source,decoder,output] = Right (Prepare (Retained (SourceDat source)) (Decoder decoder) (Output output) Nothing)
parse ["prepare","owned",source,candidate,image,symbols,decoder,output] = Right (Prepare (Owned (SourceDat source) (CandidateDat candidate) (LinkedImage image) (LinkedSymbols symbols)) (Decoder decoder) (Output output) Nothing)
parse ["launch","retained",source,decoder,output,driver,run,rom] = Right (Prepare (Retained (SourceDat source)) (Decoder decoder) (Output output) (Just (Launch driver run rom)))
parse ["launch","owned",source,candidate,image,symbols,decoder,output,driver,run,rom] = Right (Prepare (Owned (SourceDat source) (CandidateDat candidate) (LinkedImage image) (LinkedSymbols symbols)) (Decoder decoder) (Output output) (Just (Launch driver run rom)))
parse ["consume","retained",source,decoder,scratch,receipt] = Right (Consume (Retained (SourceDat source)) (Decoder decoder) (Output scratch) (ReceiptPath receipt))
parse ["consume","owned",source,candidate,image,symbols,decoder,scratch,receipt] = Right (Consume (Owned (SourceDat source) (CandidateDat candidate) (LinkedImage image) (LinkedSymbols symbols)) (Decoder decoder) (Output scratch) (ReceiptPath receipt))
parse ["compare",source,candidate,image,symbols,decoder,scratch,retainedReceipt,ownedReceipt] = Right (Compare (Owned (SourceDat source) (CandidateDat candidate) (LinkedImage image) (LinkedSymbols symbols)) (Decoder decoder) (Output scratch) (ReceiptPath retainedReceipt) (ReceiptPath ownedReceipt))
parse _ = Left usage

main :: IO ()
main = do
  args <- getArgs
  command <- either H.failH pure (parse args)
  Workspace root <- workspace Nothing
  setCurrentDirectory root
  case command of
    Help -> putStr usage
    Prepare inputs decoder output launch -> do
      case launch of
        Nothing -> pure ()
        Just _ -> slotPolicyFromEnvironment >>= (`reexecInSlot` args)
      root <- freshOutput output
      (firmware,contract) <- derive inputs decoder root
      let contractPath = root </> "peer-witness-contract.json"
          receipt = root </> "peer-witness.json"
      L.writeFile contractPath (J.encode (W.contractValue contract))
      setFileMode contractPath 0o600
      case launch of
        Nothing -> putStrLn ("critical pair private contract prepared: " ++ contractPath)
        Just selected -> launchDriver selected firmware root contractPath receipt contract
    Consume inputs decoder output (ReceiptPath receipt) -> do
      root <- scratchOutput output
      H.withTemporaryAt root "critical-pair-consume-" $ \scratch -> do
        (_,contract) <- derive inputs decoder scratch
        consume contract receipt
    Compare inputs@(Owned source _ _ _) decoder output (ReceiptPath first) (ReceiptPath second) -> do
      root <- scratchOutput output
      H.withTemporaryAt root "critical-pair-compare-" $ \scratch -> do
        (_,retained) <- derive (Retained source) decoder scratch
        (_,owned) <- derive inputs decoder scratch
        a <- H.readRegular W.maxReceiptBytes first
        b <- H.readRegular W.maxReceiptBytes second
        either (H.failH . show) pure (W.compareReceipts (retained,a) (owned,b))
        putStrLn "critical pair retained/owned caller and effect comparison accepted"
    Compare {} -> H.failH "critical comparison requires an owned artifact"

privateRoot :: FilePath -> IO Bool
privateRoot path = do
  root <- workspace Nothing
  private <- canonicalizePath (resolve root "private-data")
  let candidate = splitDirectories path
      admitted relative = splitDirectories (private </> relative) `isPrefixOf` candidate
  pure (admitted "scratch" || admitted "verify-candidate/runs")

freshOutput :: Output -> IO FilePath
freshOutput (Output output) = do
  root <- workspace Nothing
  parent <- canonicalizePath (takeDirectory (resolve root output))
  admitted <- privateRoot (parent </> takeFileName output)
  H.ensure admitted "critical output must be under this workspace's private scratch or verification runs"
  Runtime resolved <- freshRuntime root (Just (Runtime output)) (UniqueRuntime "critical-pair")
  pure resolved

scratchOutput :: Output -> IO FilePath
scratchOutput (Output output) = do
  resolved <- canonicalizePath output
  admitted <- privateRoot resolved
  H.ensure admitted "critical scratch must be under this workspace's private output roots"
  pure resolved

derive :: Inputs -> Decoder -> FilePath -> IO (FilePath,W.Contract)
derive inputs decoder scratch = do
  let sourcePath = case inputs of Retained (SourceDat source) -> source; Owned (SourceDat source) _ _ _ -> source
  source <- H.readRegular (64*1024*1024) sourcePath
  sourceDecoded <- decodeImage4 decoder scratch "original" source
  case inputs of
    Retained _ -> do
      contract <- either H.failH pure (W.deriveContract I.Retained source sourceDecoded source sourceDecoded B.empty B.empty)
      firmware <- canonicalizePath sourcePath
      pure (firmware,contract)
    Owned _ (CandidateDat candidatePath) (LinkedImage imagePath) (LinkedSymbols symbolsPath) -> do
      candidate <- H.readRegular (64*1024*1024) candidatePath
      decoded <- decodeImage4 decoder scratch "candidate" candidate
      image <- H.readRegular 0x1400 imagePath
      symbols <- H.readRegular (1024*1024) symbolsPath
      contract <- either H.failH pure (W.deriveContract I.Owned source sourceDecoded candidate decoded image symbols)
      firmware <- canonicalizePath candidatePath
      pure (firmware,contract)

decodeImage4 :: Decoder -> FilePath -> String -> B.ByteString -> IO B.ByteString
decodeImage4 (Decoder decoderPath) scratch label dat = do
  parsed <- either H.failH pure (U.parseContainer dat)
  let record = U.partRecord4 (U.parsedParts parsed)
      size = U.recDecodedSize (U.recHeader record)
      input = scratch </> (label ++ "-record4.bin")
      output = scratch </> (label ++ "-image4.bin")
  H.ensure (size > 0 && size <= 16*1024*1024) "critical image4 decoded extent is outside its bound"
  B.writeFile input (B.map complement (U.serializeRecord record))
  decoder <- canonicalizePath decoderPath
  (bounded,status,out,err) <- H.runBounded 120000000 (1024*1024) decoder [input,"0",output,show size] B.empty
  B.writeFile (scratch </> (label ++ "-decoder.stdout.log")) out
  B.writeFile (scratch </> (label ++ "-decoder.stderr.log")) err
  H.ensure (not bounded && status == ExitSuccess) "critical image4 decoder failed or exceeded its wall/output bounds"
  decoded <- H.readRegular (16*1024*1024) output
  H.ensure (B.length decoded == fromIntegral size) "critical decoder output extent differs from its DAT record"
  pure decoded

consume :: W.Contract -> FilePath -> IO ()
consume contract path = do
  bytes <- H.readRegular W.maxReceiptBytes path
  either (H.failH . show) pure (W.validateReceipt contract bytes)
  putStrLn ("critical pair receipt independently accepted: " ++ path)

launchDriver :: Launch -> FilePath -> FilePath -> FilePath -> FilePath -> W.Contract -> IO ()
launchDriver (Launch driverPath runPath romPath) firmware output contractPath receipt contract = do
  driver <- canonicalizePath driverPath
  run <- canonicalizePath runPath
  rom <- canonicalizePath romPath
  bytes <- H.readRegular 0x4000 rom
  H.ensure (B.length bytes == 0x4000) "critical launch requires an exact 16 KiB captured boot ROM"
  let args = ["--run",run,"--update",firmware,"--boot-rom",rom,"--output",output,"--peer-witness",contractPath]
  putStrLn (showCommandForUser driver args)
  (bounded,status,out,err) <- H.runBounded 120000000 (1024*1024) driver args B.empty
  B.writeFile (output </> "driver.stdout.log") out
  B.writeFile (output </> "driver.stderr.log") err
  B.putStr out
  B.hPut stderr err
  exists <- doesFileExist receipt
  when exists (consume contract receipt)
  H.ensure (not bounded && status == ExitSuccess && exists) "critical driver failed, exceeded its bounds or produced no receipt"
