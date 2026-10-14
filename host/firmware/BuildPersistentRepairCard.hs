module Main (main) where

import BootCard
import Control.Exception (SomeException, displayException, try)
import Control.Monad (forM_)
import Data.Bits (complement)
import qualified Data.ByteString as B
import Data.List (isPrefixOf)
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import ConditionalBlock14RecoveryPin (ConditionalDigest (..), expectedConditionalDigest)
import ConditionalBlock14RecoveryCodedPin (CodedConditionalDigest (..), expectedCodedConditionalDigest)
import PersistentBlock14ClassifierPin (ClassifierDigest (..), expectedClassifierDigest)
import MediaViewDiagnosticPin (MediaViewDigest (..), expectedMediaViewDigest)
import PhysicalBlock14RecoveryPin (PhysicalRecoveryDigest (..), expectedPhysicalRecoveryDigest)
import PhysicalRepairBootstrapPin (PhysicalBootstrapDigest (..), expectedPhysicalBootstrapDigest)
import PersistentRepairBootstrapPin (BootstrapDigest (..), expectedBootstrapDigest)
import PersistentRepairPin (RepairDigest (..), expectedRepairDigest)
import System.Directory (canonicalizePath, createDirectory, getCurrentDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>), addTrailingPathSeparator, isAbsolute, takeDirectory, takeFileName)
import System.IO (hPutStrLn, stderr)

newtype EntryBootstrap = EntryBootstrap B.ByteString
newtype NativeStage = NativeStage B.ByteString
newtype RepairBlock = RepairBlock B.ByteString

data Inputs = Inputs Operation Config EntryBootstrap NativeStage RepairBlock RepairBlock

data Operation = Repair | Classify | Conditional | CodedConditional | MediaView | PhysicalRepair

data InstalledSource = CapturedInstalled FilePath | ReconstructedRecorder

data Command
  = Build Operation FilePath FilePath InstalledSource FilePath FilePath FilePath
  | Verify Operation FilePath FilePath InstalledSource FilePath FilePath FilePath

checked :: Either String a -> IO a
checked = either H.failH pure

workspaceRoot :: IO FilePath
workspaceRoot = lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= maybe getCurrentDirectory pure

resolve :: FilePath -> IO FilePath
resolve path = do
  root <- workspaceRoot
  pure (if isAbsolute path then path else root </> path)

readPinned :: Int -> FilePath -> String -> IO B.ByteString
readPinned limit given wanted = do
  path <- resolve given
  bytes <- H.readRegular limit path
  digest <- H.sha256 bytes
  H.ensure (digest == wanted) (path ++ ": pinned SHA-256 mismatch")
  pure bytes

mediaBytes :: B.ByteString -> B.ByteString
mediaBytes = B.map complement . B.take 0x24a0020 . B.drop 0x25c

blockAt :: Int -> B.ByteString -> B.ByteString
blockAt index = B.take 0x20000 . B.drop (index * 0x20000)

assembleStage :: B.ByteString -> B.ByteString -> NativeStage
assembleStage staged stub = NativeStage (B.concat
  [ B.take 0x20 staged
  , U.putWord32 0x6000
  , B.take (0x1fdc-0x24) (B.drop 0x24 staged)
  , U.putWord32 0xeb001037
  , B.take (0x6000-0x1fe0) (B.drop 0x1fe0 staged)
  , stub
  , B.drop (0x6000+B.length stub) staged
  ])

inputs :: Operation -> FilePath -> FilePath -> InstalledSource -> FilePath -> FilePath -> IO Inputs
inputs operation stockPath prefixPath installedPath stubPath entryPath = do
  stock <- readPinned (64*1024*1024) stockPath "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"
  parsed <- checked (U.parseContainer stock)
  H.ensure (all U.checkPassed (U.checks parsed ++ U.carriedChecks parsed ++ U.stockChecks parsed)) "official DAT contract failed"
  config <- checked (stockConfig (B.map complement (B.take 0x1048 (B.drop 0x214 stock))))
  let staged = B.map complement (B.take 0x40000 (B.drop (0x214+0x40048) stock))
  H.ensure (B.length staged == 0x40000 && B.all (== 0xff) (B.take 0x1000 (B.drop 0x6000 staged))) "native repair gap changed"

  prefix <- readPinned (64*1024*1024) prefixPath "acb0fbbb9507421f48a6254920cafb5ce20e1fd6bf821bc80c912c1f99c29ca8"
  let prefixMedia = mediaBytes prefix
      repair = blockAt 14 prefixMedia
  H.ensure (B.length prefixMedia == 0x24a0020) "RC0 media span changed"
  repairDigest <- H.sha256 repair
  H.ensure (repairDigest == "94deaec641a11cf045c8761752b218d9b52af9e2e015e3b8dcc8e33708956bf5") "repair block changed"
  installedRepair <- case installedPath of
    CapturedInstalled path -> do
      installed <- readPinned (64*1024*1024) path "72a78c5595f40f88af26c4ed429529a756f759aa9b8e5c5704a745613f9ed0a8"
      let installedMedia = mediaBytes installed
          changedBlocks = [index | index <- [0..293], blockAt index prefixMedia /= blockAt index installedMedia]
      H.ensure (B.length installedMedia == 0x24a0020) "installed media span changed"
      H.ensure (changedBlocks == [14,71]) "RC0-to-installed changed-block set changed"
      pure (blockAt 14 installedMedia)
    ReconstructedRecorder -> do
      H.ensure (case operation of PhysicalRepair -> True; _ -> False) "derived recorder is physical-repair only"
      let substitutions =
            [ (0x1f120, 0xffdc0446, 0xa9c90446)
            , (0x1f124, 0x04d8ebff, 0x04d8eb34)
            , (0x1f480, 0x28024381, 0x280d4381)
            , (0x1f484, 0x0803c8da, 0xebffffda)
            ]
      forM_ substitutions $ \(offset, original, _) ->
        H.ensure (U.word32At repair offset == original) "RC0 rollback word changed"
      pure (foldl (\bytes (offset, _, replacement) ->
        B.take offset bytes <> U.putWord32 replacement <> B.drop (offset+4) bytes) repair substitutions)
  let changedOffsets = [index | index <- [0..0x1ffff], B.index repair index /= B.index installedRepair index]
  H.ensure (changedOffsets == [127266,127267,127268,128130,128133,128134,128135]) "repair block byte delta changed"
  let RepairDigest expectedRepair = expectedRepairDigest
      ClassifierDigest expectedClassifier = expectedClassifierDigest
      ConditionalDigest expectedConditional = expectedConditionalDigest
      CodedConditionalDigest expectedCodedConditional = expectedCodedConditionalDigest
      MediaViewDigest expectedMediaView = expectedMediaViewDigest
      PhysicalRecoveryDigest expectedPhysicalRecovery = expectedPhysicalRecoveryDigest
      PhysicalBootstrapDigest expectedPhysicalEntry = expectedPhysicalBootstrapDigest
      expectedStub = case operation of
        Repair -> expectedRepair
        Classify -> expectedClassifier
        Conditional -> expectedConditional
        CodedConditional -> expectedCodedConditional
        MediaView -> expectedMediaView
        PhysicalRepair -> expectedPhysicalRecovery
      BootstrapDigest expectedDefaultEntry = expectedBootstrapDigest
      expectedEntry = case operation of
        PhysicalRepair -> expectedPhysicalEntry
        _ -> expectedDefaultEntry
  stub <- readPinned 0x1000 stubPath expectedStub
  H.ensure (B.length stub > 0x200 && B.length stub <= 0x1000 && B.length stub `mod` 4 == 0) "persistent repair stub exceeds native gap"
  entry <- readPinned 512 entryPath expectedEntry
  H.ensure (B.length entry == 512) "persistent entry bootstrap must occupy one sector"
  let NativeStage native = assembleStage staged stub
  finalStage <- case operation of
    PhysicalRepair -> do
      H.ensure (B.all (== 0xff) (B.take 0x8000 (B.drop 0x8000 native))
             && B.all (== 0xff) (B.take 0x8000 (B.drop 0x16000 native))) "physical reference holes changed"
      pure (NativeStage (B.take 0x8000 native <> B.take 0x8000 repair
                     <> B.take 0x6000 (B.drop 0x10000 native)
                     <> B.take 0x8000 (B.drop 0x8000 repair)
                     <> B.drop 0x1e000 native))
    _ -> pure (NativeStage native)
  pure (Inputs operation config (EntryBootstrap entry) finalStage (RepairBlock repair) (RepairBlock installedRepair))

carriedBody :: Operation -> RepairBlock -> B.ByteString
carriedBody operation (RepairBlock repair) = case operation of
  PhysicalRepair -> B.replicate 0x48000 0xff <> B.drop 0x10000 repair <> B.replicate 0x8000 0xff
  _ -> repair

canonicalImage :: Inputs -> IO B.ByteString
canonicalImage (Inputs operation config (EntryBootstrap entry) (NativeStage stage) repair _) =
  checked (stagedRepairCard config entry stage (carriedBody operation repair))

verifyInputs :: Inputs -> B.ByteString -> IO ()
verifyInputs (Inputs operation config (EntryBootstrap entry) (NativeStage stage) repair _) image = do
  checked (verifyStagedRepairCard config entry stage (carriedBody operation repair) image)
  H.ensure (B.length image == 17826304) "persistent recovery image size changed"
  H.ensure (U.word32At stage 0x20 == 0x6000) "persistent guard vector missing"
  H.ensure (U.word32At stage 0x1fdc == 0xeb001037) "persistent callback call missing"
  raw <- checked (readRawRom image)
  H.ensure (rawConfigDifferences config raw ==
    [ ConfigDifference (ConfigWordOffset 0x18) 2 1
    , ConfigDifference (ConfigWordOffset 0x1c0) 0 0x40000
    , ConfigDifference (ConfigWordOffset 0x1c4) 0 0x08000000
    , ConfigDifference (ConfigWordOffset 0x1fc) 0x01000101 0xaa550101
    ]) "persistent raw configuration changed"

privateOutput :: FilePath -> IO FilePath
privateOutput given = do
  output <- resolve given
  H.ensure (takeFileName output `notElem` ["", ".", ".."]) "output must name a new directory"
  root <- workspaceRoot
  guard <- canonicalizePath (root </> "private-data")
  parent <- canonicalizePath (takeDirectory output)
  H.ensure (addTrailingPathSeparator guard `isPrefixOf` addTrailingPathSeparator parent) "output must be beneath private-data"
  pure output

writeArtifact :: FilePath -> (FilePath, B.ByteString) -> IO ()
writeArtifact directory (name, bytes) = do
  let path = directory </> name
  B.writeFile path bytes
  saved <- H.readRegular (18*1024*1024) path
  H.ensure (saved == bytes) "saved artifact differs from emitted bytes"
  digest <- H.sha256 saved
  putStrLn (path ++ ": " ++ show (B.length saved) ++ " bytes, sha256 " ++ digest)

run :: Command -> IO ()
run command = case command of
  Build operation stock prefix installed stub entry outputPath -> do
    prepared@(Inputs _ config (EntryBootstrap boot) (NativeStage stage) (RepairBlock repair) (RepairBlock recorder)) <-
      inputs operation stock prefix installed stub entry
    image <- canonicalImage prepared
    verifyInputs prepared image
    output <- privateOutput outputPath
    createDirectory output
    let imageName = case operation of
          Repair -> "persistent-repair-sd.img"
          Classify -> "persistent-block14-classifier-sd.img"
          Conditional -> "conditional-block14-recovery-sd.img"
          CodedConditional -> "conditional-block14-recovery-coded-sd.img"
          MediaView -> "media-view-diagnostic-sd.img"
          PhysicalRepair -> "physical-block14-recovery-sd.img"
    let operationArtifacts = case operation of
          PhysicalRepair -> [("recorder-block-14.bin", recorder)]
          _ -> []
    forM_ ([("ffsys.bin",configBytes config),("ffboot.bin",boot),("ffrecov.bin",stage <> carriedBody operation (RepairBlock repair)),("repair-block-14.bin",repair),(imageName,image)] ++ operationArtifacts) (writeArtifact output)
    case operation of
      Repair -> do
        putStrLn "Success indication: solid red during one-block program, then three long red pulses, then normal UI."
        putStrLn "Repeating short red pulses are refusal or programming failure; do not power-cycle after that indication."
      Classify -> do
        putStrLn "Read-only result: one long pulse is recorder block, two is unknown, three is RC0 block."
        putStrLn "Repeating fast pulses are a pre-read refusal; repeating slow pulses are a read error."
      Conditional -> do
        putStrLn "Conditional result: three long pulses means exact RC0 before or after one bounded write."
        putStrLn "One long pulse means exact recorder after the write; two means unknown before any write."
        putStrLn "Repeating fast or slow pulses require preserved power."
      CodedConditional -> do
        putStrLn "Packet repeats: long solid sync, long dark separator, then one through nine long pulses."
        putStrLn "1 not ready; 2 initial read error; 3 initial unknown; 4 preexisting RC0; 5 carried-source refusal."
        putStrLn "6 post-write read error; 7 post-write RC0; 8 post-write recorder; 9 post-write unknown."
      MediaView -> do
        putStrLn "Three packet types repeat: four-delay sync is status, eight-delay sync is boot physical offset plus one, twelve-delay sync is absolute application delta plus one."
        putStrLn "Status 9 is the expected one-block forward RC0 split; 10 is a larger forward RC0 split, 11 is a reverse RC0 split, and 12 is an equal-map RC0 result."
        putStrLn "Status 8 is an application-physical read failure; 13 is application-physical recorder, and 14 is other application-physical content. Statuses 1-7 stop before that classification."
        putStrLn "All fields for statuses 8-14 are emitted from validated mappings. This operation performs no internal-media or card write."
      PhysicalRepair -> do
        putStrLn "Three packet types repeat: four-delay sync is status, eight-delay sync is boot physical offset plus one, twelve-delay sync is final physical delta plus one."
        putStrLn "Status 20 is exact RC0 readback at physical block 14; 21 is exact RC0 after a stock bad-block remap. Statuses 15-19 and 22-29 are refusal, writer failure or non-RC0 readback."
        putStrLn "The write path requires both validated mappings to equal physical block 14 and byte-exact recorder content under the storage lock. It restores the four pinned changed words in the read buffer, compares the complete result to the carried RC0 reference, and submits that buffer once to the stock lower-layer programmer."
  Verify operation stock prefix installed stub entry imagePath -> do
    prepared <- inputs operation stock prefix installed stub entry
    expected <- canonicalImage prepared
    path <- resolve imagePath
    image <- H.readRegular (18*1024*1024) path
    H.ensure (image == expected) "persistent recovery image differs from canonical bytes"
    verifyInputs prepared image
    digest <- H.sha256 image
    putStrLn ("persistent card layout verified; sha256 " ++ digest)
parseCommand :: [String] -> IO Command
parseCommand args = case args of
  ["build",stock,prefix,installed,stub,entry,output] -> pure (Build Repair stock prefix (CapturedInstalled installed) stub entry output)
  ["verify",stock,prefix,installed,stub,entry,image] -> pure (Verify Repair stock prefix (CapturedInstalled installed) stub entry image)
  ["build-classifier",stock,prefix,installed,stub,entry,output] -> pure (Build Classify stock prefix (CapturedInstalled installed) stub entry output)
  ["verify-classifier",stock,prefix,installed,stub,entry,image] -> pure (Verify Classify stock prefix (CapturedInstalled installed) stub entry image)
  ["build-conditional",stock,prefix,installed,stub,entry,output] -> pure (Build Conditional stock prefix (CapturedInstalled installed) stub entry output)
  ["verify-conditional",stock,prefix,installed,stub,entry,image] -> pure (Verify Conditional stock prefix (CapturedInstalled installed) stub entry image)
  ["build-coded-conditional",stock,prefix,installed,stub,entry,output] -> pure (Build CodedConditional stock prefix (CapturedInstalled installed) stub entry output)
  ["verify-coded-conditional",stock,prefix,installed,stub,entry,image] -> pure (Verify CodedConditional stock prefix (CapturedInstalled installed) stub entry image)
  ["build-media-view",stock,prefix,installed,stub,entry,output] -> pure (Build MediaView stock prefix (CapturedInstalled installed) stub entry output)
  ["verify-media-view",stock,prefix,installed,stub,entry,image] -> pure (Verify MediaView stock prefix (CapturedInstalled installed) stub entry image)
  ["build-physical",stock,prefix,installed,stub,entry,output] -> pure (Build PhysicalRepair stock prefix (CapturedInstalled installed) stub entry output)
  ["verify-physical",stock,prefix,installed,stub,entry,image] -> pure (Verify PhysicalRepair stock prefix (CapturedInstalled installed) stub entry image)
  ["build-physical-from-rc0",stock,prefix,stub,entry,output] -> pure (Build PhysicalRepair stock prefix ReconstructedRecorder stub entry output)
  _ -> H.failH "usage: build-persistent-repair-card MODE STOCK_DAT RC0_DAT INSTALLED_DAT LINKED_STAGE_BIN LINKED_ENTRY_BIN OUTPUT; build-physical-from-rc0 STOCK_DAT RC0_DAT LINKED_STAGE_BIN LINKED_ENTRY_BIN OUTPUT"

main :: IO ()
main = do
  result <- try (getArgs >>= parseCommand >>= run) :: IO (Either SomeException ())
  case result of
    Left failure -> hPutStrLn stderr (displayException failure) >> exitFailure
    Right () -> pure ()
