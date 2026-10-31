{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (bracket)
import Control.Monad (forM_)
import Data.Aeson (FromJSON (..), withObject, (.:))
import Data.Aeson.Types (parseEither)
import Data.Bits ((.|.), shiftL, shiftR)
import qualified Data.ByteString as B
import Data.List (mapAccumL)
import Data.Unique (hashUnique, newUnique)
import Data.Word (Word32)
import Emulation.EmulatorSlot (reexecInSlot, slotPolicyFromEnvironment)
import qualified FujiAF.Host as H
import qualified FujiAF.JSON as J
import Numeric.Natural (Natural)
import System.Directory (canonicalizePath, copyFile, createDirectory, getTemporaryDirectory, removeDirectoryRecursive)
import System.Environment (getArgs, getExecutablePath, lookupEnv, setEnv, unsetEnv)
import System.Exit (ExitCode (..), exitWith)
import System.FilePath ((</>))
import System.Posix.Files (setFileMode)
import System.Posix.Process (getProcessID)
import System.Posix.User (getEffectiveUserID)
import System.Process (createProcess, proc, waitForProcess)

newtype Address = Address Word32 deriving (Eq, Show, FromJSON)
newtype ByteCount = ByteCount Natural deriving (Eq, Show, FromJSON)
newtype Count = Count Natural deriving (Eq, Ord, Show, FromJSON)
newtype SchemaVersion = SchemaVersion Natural deriving (Eq, Show, FromJSON)
newtype EnginePath = EnginePath FilePath
newtype ArmWord = ArmWord Word32

data Origin = CpuAttempt | MediaDma | SdDma | Fill | Lz deriving (Eq, Show)
data Phase = Reset | RomLoad | NativeReset | NativeLoad | Rollback | Close deriving (Eq, Show)
data Stage = BeforeLoad | PreDecode | ImageDecode | AfterLoad deriving (Eq, Show)
data Status = Complete | Unsupported | NativeHalt | Budget | EngineError | RouteMismatch deriving (Eq, Show)

instance FromJSON Origin where
  parseJSON value = do
    text <- parseJSON value
    case (text :: String) of
      "cpu_attempt" -> pure CpuAttempt
      "media_dma" -> pure MediaDma
      "sd_dma" -> pure SdDma
      "fill" -> pure Fill
      "lz" -> pure Lz
      _ -> fail "unknown write origin"

instance FromJSON Phase where
  parseJSON value = do
    text <- parseJSON value
    case (text :: String) of
      "reset" -> pure Reset
      "rom_load" -> pure RomLoad
      "native_reset" -> pure NativeReset
      "native_load" -> pure NativeLoad
      "rollback" -> pure Rollback
      "close" -> pure Close
      _ -> fail "unknown execution phase"

instance FromJSON Stage where
  parseJSON value = do
    text <- parseJSON value
    case (text :: String) of
      "before_load" -> pure BeforeLoad
      "pre_decode" -> pure PreDecode
      "image_decode" -> pure ImageDecode
      "after_load" -> pure AfterLoad
      _ -> fail "unknown write stage"

instance FromJSON Status where
  parseJSON value = do
    text <- parseJSON value
    case (text :: String) of
      "complete" -> pure Complete
      "unsupported" -> pure Unsupported
      "native_halt" -> pure NativeHalt
      "budget" -> pure Budget
      "engine_error" -> pure EngineError
      "route_mismatch" -> pure RouteMismatch
      _ -> fail "unknown engine status"

data Event = Event
  { eventOrigin :: Origin
  , eventPhase :: Phase
  , eventStage :: Stage
  , eventPc :: Address
  , eventAddress :: Address
  , eventBytes :: ByteCount
  , overlapAddress :: Address
  , overlapBytes :: ByteCount
  , overlapUnchanged :: Bool
  } deriving (Eq, Show)

instance FromJSON Event where
  parseJSON = withObject "prefix write event" $ \o -> Event
    <$> o .: "origin" <*> o .: "phase" <*> o .: "stage" <*> o .: "pc"
    <*> o .: "address" <*> o .: "bytes" <*> o .: "overlap_address"
    <*> o .: "overlap_bytes" <*> o .: "overlap_unchanged"

data PrefixWrites = PrefixWrites
  { prefixAddress :: Address
  , prefixBytes :: ByteCount
  , auditComplete :: Bool
  , loadEntered :: Bool
  , decodeEntered :: Bool
  , loadReturned :: Bool
  , cpuAttempts :: Count
  , deviceCommits :: Count
  , events :: [Event]
  } deriving (Show)

instance FromJSON PrefixWrites where
  parseJSON = withObject "prefix write audit" $ \o -> PrefixWrites
    <$> o .: "address" <*> o .: "bytes" <*> o .: "complete"
    <*> o .: "load_entered" <*> o .: "decode_entered" <*> o .: "load_returned"
    <*> o .: "cpu_attempts" <*> o .: "device_commits" <*> o .: "events"

data Report = Report
  { schemaVersion :: SchemaVersion
  , status :: Status
  , prefixWrites :: PrefixWrites
  , fillBytes :: ByteCount
  } deriving (Show)

instance FromJSON Report where
  parseJSON = withObject "engine report" $ \o -> do
    statistics <- o .: "statistics"
    filled <- withObject "device statistics" (.: "fill_bytes") statistics
    Report <$> o .: "schema_version" <*> o .: "status" <*> o .: "prefix_writes" <*> pure filled

prefix :: Address
prefix = Address 0x0061c1fc

-- Literal pools follow the terminating instruction.
data Register = R0 | R1 | R2 | R3 | R4 deriving (Enum)
data Instruction = Literal Register Word32 | Arm ArmWord

rom :: [Instruction] -> B.ByteString
rom instructions = B.pack bytes <> B.replicate (16384 - length bytes) 0
  where
    literals = [value | Literal _ value <- instructions]
    instructionCount = length instructions
    (_, code) = mapAccumL emit (0 :: Int) (zip [0 :: Int ..] instructions)
    emit poolIndex (index, instruction) = case instruction of
      Arm word -> (poolIndex, word)
      Literal register _ ->
        let displacement = 4 * (instructionCount + poolIndex - index) - 8
            opcode = 0xe59f0000 .|. (fromIntegral (fromEnum register) `shiftL` 12) .|. fromIntegral displacement
        in (poolIndex + 1, ArmWord opcode)
    bytes = concatMap littleEndian (code ++ map ArmWord literals)
    littleEndian (ArmWord value) = [fromIntegral (value `shiftR` shift) | shift <- [0, 8, 16, 24]]

arm :: Word32 -> Instruction
arm = Arm . ArmWord

halt :: Instruction
halt = arm 0xe7f0def0

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ["--admit", engine, root] -> do
      unsetEnv "XM1_EMULATOR_SLOT"
      userBus
      policy <- slotPolicyFromEnvironment
      reexecInSlot policy ["--guarded", engine, root]
      ioError (userError "slot admission returned without re-exec")
    ["--guarded", engine, root] -> do
      policy <- slotPolicyFromEnvironment
      reexecInSlot policy arguments
      runCases (EnginePath engine) root
    [engine] -> bracket privateDirectory removeDirectoryRecursive $ \root -> do
      enginePath <- canonicalizePath engine
      executable <- getExecutablePath
      let launcher = root </> "audit-tests"
      copyFile executable launcher
      setFileMode launcher 0o700
      (_, _, _, process) <- createProcess (proc launcher ["--admit", enginePath, root])
      waitForProcess process >>= exitWith
    _ -> ioError (userError "usage: recovery-bootstrap-audit-tests ENGINE")

privateDirectory :: IO FilePath
privateDirectory = do
  temporary <- getTemporaryDirectory
  base <- maybe temporary id <$> lookupEnv "TEST_TMPDIR"
  pid <- getProcessID
  unique <- hashUnique <$> newUnique
  let root = base </> ("recovery-bootstrap-audit-" ++ show pid ++ "-" ++ show unique)
  createDirectory root
  setFileMode root 0o700
  canonicalizePath root

userBus :: IO ()
userBus = do
  uid <- getEffectiveUserID
  let runtime = "/run/user/" ++ show uid
  forM_ [("XDG_RUNTIME_DIR", runtime), ("DBUS_SESSION_BUS_ADDRESS", "unix:path=" ++ runtime ++ "/bus")] $ \(name, value) -> do
    existing <- lookupEnv name
    case existing of
      Nothing -> setEnv name value
      Just _ -> pure ()

invoke :: EnginePath -> FilePath -> String -> [Instruction] -> IO Report
invoke (EnginePath engine) root name instructions = do
  let directory = root </> name
      input = directory </> "input"
      output = directory </> "output"
  forM_ [directory, input, output] $ \path -> createDirectory path >> setFileMode path 0o700
  B.writeFile (input </> "rom.bin") (rom instructions)
  B.writeFile (input </> "media.bin") (B.singleton 0xff)
  B.writeFile (input </> "card.img") (B.replicate 512 0)
  (timedOut, code, _, _) <- H.runBounded 10_000_000 (64 * 1024) engine
    [input, "media", "none", output, "4096", "2"] B.empty
  H.ensure (not timedOut && code == ExitFailure 2) (name ++ ": bounded synthetic guest must retain a non-complete report")
  bytes <- H.readRegular (1024 * 1024) (output </> "report.json")
  value <- checked (J.decodeStrictIntegers bytes)
  report <- checked (parseEither parseJSON value)
  let audit = prefixWrites report
  H.ensure (schemaVersion report == SchemaVersion 2) (name ++ ": report schema")
  H.ensure (prefixAddress audit == prefix && prefixBytes audit == ByteCount 516) (name ++ ": canonical prefix extent")
  H.ensure (not (loadEntered audit) && not (decodeEntered audit) && not (loadReturned audit)) (name ++ ": no native load entered")
  H.ensure (all (\event -> eventPhase event == Reset && eventStage event == BeforeLoad) (events audit)) (name ++ ": reset write provenance")
  H.ensure (all (\event -> case eventPc event of
    Address pc -> pc `mod` 4 == 0 && pc < fromIntegral (4 * length instructions)) (events audit)) (name ++ ": write PCs belong to declared ARM program")
  pure report
  where
    checked result = either (ioError . userError) pure result

expectEvents :: String -> Report -> [Event] -> IO ()
expectEvents name report expected = do
  H.ensure (status report == Unsupported && auditComplete (prefixWrites report)) (name ++ ": complete audit survives unsupported instruction")
  H.ensure (events (prefixWrites report) == expected) (name ++ ": observed writes differ: " ++ show report)
  putStrLn ("ok   " ++ name)

cpuEvent :: Word32 -> Word32 -> Natural -> Word32 -> Natural -> Bool -> Event
cpuEvent pc address bytes overlap count same = Event CpuAttempt Reset BeforeLoad
  (Address pc) (Address address) (ByteCount bytes) (Address overlap) (ByteCount count) same

runCases :: EnginePath -> FilePath -> IO ()
runCases engine root = do
  same <- invoke engine root "same-value"
    [Literal R0 0x0061c1fc, arm 0xe3a01000, arm 0xe5801000, arm 0xe5801000, halt]
  expectEvents "same-value CPU attempts" same
    [cpuEvent 8 0x0061c1fc 4 0x0061c1fc 4 True, cpuEvent 12 0x0061c1fc 4 0x0061c1fc 4 True]
  H.ensure (cpuAttempts (prefixWrites same) == Count 2 && deviceCommits (prefixWrites same) == Count 0) "same-value attempt counters"

  alias <- invoke engine root "ram-alias"
    [ Literal R0 0x0061c1fc, arm 0xe3a0105a, arm 0xe5801000
    , Literal R2 0x4061c1fc, arm 0xe5821000, arm 0xe5901000, arm 0xe5821000, halt
    ]
  expectEvents "RAM aliases share values and canonical overlap" alias
    [ cpuEvent 8 0x0061c1fc 4 0x0061c1fc 4 False
    , cpuEvent 16 0x4061c1fc 4 0x0061c1fc 4 True
    , cpuEvent 24 0x4061c1fc 4 0x0061c1fc 4 True
    ]
  H.ensure (cpuAttempts (prefixWrites alias) == Count 3 && deviceCommits (prefixWrites alias) == Count 0) "alias attempt counters"

  crossing <- invoke engine root "crossing-intervals"
    [ Literal R0 0x0061c1fb, arm 0xe3a01000, arm 0xe5801000
    , Literal R0 0x0061c3ff, arm 0xe5801000, halt
    ]
  expectEvents "partial overlaps preserve original intervals" crossing
    [cpuEvent 8 0x0061c1fb 4 0x0061c1fc 3 True, cpuEvent 16 0x0061c3ff 4 0x0061c3ff 1 True]

  outside <- invoke engine root "adjacent-intervals"
    [Literal R0 0x0061c1f8, arm 0xe3a01000, arm 0xe5801000, Literal R0 0x0061c400, arm 0xe5801000, halt]
  expectEvents "half-open prefix excludes adjacent stores" outside []
  H.ensure (cpuAttempts (prefixWrites outside) == Count 0 && deviceCommits (prefixWrites outside) == Count 0) "adjacent stores do not count"

  high <- invoke engine root "unrelated-high-address"
    [Literal R0 0x8061c1fc, arm 0xe3a01000, arm 0xe5801000, halt]
  H.ensure (status high `elem` [Unsupported, EngineError] && auditComplete (prefixWrites high)) "unmapped high address terminates with intact audit"
  H.ensure (null (events (prefixWrites high)) && cpuAttempts (prefixWrites high) == Count 0 && deviceCommits (prefixWrites high) == Count 0) "unrelated high address is not a RAM alias"
  putStrLn "ok   unrelated high address"

  overflow <- invoke engine root "event-overflow"
    [Literal R0 0x0061c1fc, arm 0xe3a01000, arm 0xe5801000, arm 0xeafffffd]
  let overflowAudit = prefixWrites overflow
  H.ensure (status overflow == EngineError && not (auditComplete overflowAudit)) "event overflow fails closed"
  H.ensure (length (events overflowAudit) == 512 && cpuAttempts overflowAudit > Count 512 && deviceCommits overflowAudit == Count 0) "event overflow retains bounded history and total attempts"
  H.ensure (all (== cpuEvent 8 0x0061c1fc 4 0x0061c1fc 4 True) (events overflowAudit)) "overflow history retains actual same-value attempts"
  putStrLn "ok   event overflow"

  filled <- invoke engine root "same-value-fill" fillProgram
  let fillAudit = prefixWrites filled
      committed = filter ((== Fill) . eventOrigin) (events fillAudit)
      readback = filter ((== CpuAttempt) . eventOrigin) (events fillAudit)
  H.ensure (status filled == Unsupported && auditComplete fillAudit) "fill audit survives unsupported endpoint"
  H.ensure (fillBytes filled == ByteCount 1032 && deviceCommits fillAudit == Count 2 && cpuAttempts fillAudit == Count 1) "two actual fill commits and guest readback store"
  H.ensure (map eventOrigin (events fillAudit) == [Fill, Fill, CpuAttempt]) "fill commits precede guest readback"
  H.ensure (map overlapUnchanged committed == [False, True]) "second fill commits identical prefix-overlap bytes"
  H.ensure (all (\event -> eventAddress event == prefix && eventBytes event == ByteCount 516 && overlapAddress event == prefix && overlapBytes event == ByteCount 516) committed) "fill commit extents"
  H.ensure (case readback of
    [event] -> overlapUnchanged event && eventAddress event == prefix && eventBytes event == ByteCount 4 && overlapAddress event == prefix && overlapBytes event == ByteCount 4
    _ -> False) "guest reads filled final word equal to filled first word"
  putStrLn "ok   same-value modeled fill and guest readback"

fillProgram :: [Instruction]
fillProgram =
  [ Literal R2 0x00010000
  , Literal R1 0x5a
  ] ++ [arm (0xe5821000 .|. offset) | offset <- [0, 4 .. 28]] ++
  [ Literal R3 0xfff02000
  , arm 0xe5832000
  , Literal R0 0x0061c1fc
  , arm 0xe5830004
  , Literal R4 516
  , arm 0xe5834008
  , Literal R4 0x01522400
  , arm 0xe583402c
  , arm 0xe3a04005
  , arm 0xe5834028
  , arm 0xe5934024
  , arm 0xe3a04005
  , arm 0xe5834028
  , arm 0xe5934024
  , arm 0xe5901200
  , arm 0xe5801000
  , halt
  ]
