{-# LANGUAGE OverloadedStrings #-}
module BootCard
  ( Config, configBytes, stockConfig, StageMarker (..), stageMarkerConfig, powerHoldConfig, bootCard, stagedRecoveryCard, stagedRepairCard, recoveryCard, recoveryCardFat32
  , BootPayloadBytes, bootPayloadByteCount, maxBootPayloadBytes
  , verifyCard, verifyStagedRecoveryCard, verifyStagedRepairCard, verifyRecoveryCard
  , recoveryReportBaseLba, recoveryReportSectorCount, recoveryReportSectors
  , BootFile (..), FatKind (..), Volume (..), mountRom, readRomFile
  , Mode2Load, mode2ConfigBytes, mode2PayloadBytes, readRawRom
  , ConfigWordOffset (..), ConfigDifference (..), rawConfigDifferences
  , CardSector (..), firmwareRecoveryBodyLba, firmwareRecoveryCard, verifyFirmwareRecoveryCard
  , firmwareRecoveryBodyLbaFat32, firmwareRecoveryCardFat32
  ) where

import Control.Monad (unless)
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import qualified Data.ByteString as B
import qualified Data.IntSet as S
import Data.Word (Word32)

newtype ConfigWordOffset = ConfigWordOffset Int deriving (Eq, Show)
newtype RawSectorCount = RawSectorCount Int
newtype CardSector = CardSector Int deriving (Eq, Show)

newtype BootPayloadBytes = BootPayloadBytes { bootPayloadByteCount :: Int }

maxBootPayloadBytes :: BootPayloadBytes
maxBootPayloadBytes = BootPayloadBytes ((2048-2)*512)

payloadSectorCount :: B.ByteString -> Either String RawSectorCount
payloadSectorCount payload = do
  let size = B.length payload
  require (size > 0 && size `mod` 512 == 0 && size <= bootPayloadByteCount maxBootPayloadBytes) "payload must be nonempty, sector-aligned and end before LBA 2048"
  pure (RawSectorCount (size `div` 512))

data ConfigDifference = ConfigDifference
  { configWordOffset :: ConfigWordOffset
  , stockWord :: Word32
  , rawWord :: Word32
  } deriving (Eq, Show)
data Mode2Load = Mode2Load
  { mode2ConfigBytes :: B.ByteString
  , mode2PayloadBytes :: B.ByteString
  } deriving (Eq, Show)

newtype Config = Config { configBytes :: B.ByteString } deriving (Eq, Show)
data FatKind = Fat12 | Fat16 | Fat32 deriving (Eq, Show)
data Volume = Volume { fatKind :: FatKind, firstFat :: Int, rootLocation :: Int, dataStart :: Int, clusterSectors :: Int, clusterLimit :: Int, rootEntries :: Int } deriving (Eq, Show)
data BootFile = SystemConfig | BootPayload | RecoveryStage | RestoreContainer deriving (Eq, Show)

shortName :: BootFile -> B.ByteString
shortName SystemConfig = "FFSYS   BIN"
shortName BootPayload = "FFBOOT  BIN"
shortName RecoveryStage = "FFRECOV BIN"
shortName RestoreContainer = "FWUP0002DAT"

require :: Bool -> String -> Either String ()
require ok message = unless ok (Left message)

slice :: B.ByteString -> Int -> Int -> Either String B.ByteString
slice bytes off count = do
  require (off >= 0 && count >= 0 && off <= B.length bytes - count) "read outside image"
  pure (B.take count (B.drop off bytes))

le :: B.ByteString -> Int -> Int -> Either String Int
le bytes off count = do
  value <- slice bytes off count
  pure (sum [fromIntegral w `shiftL` (8*i) | (i,w) <- zip [0..] (B.unpack value)])

putLE :: Int -> Int -> B.ByteString
putLE count value = B.pack [fromIntegral (value `shiftR` (8*i)) | i <- [0..count-1]]

patch :: B.ByteString -> [(Int,B.ByteString)] -> B.ByteString
patch = foldl (\bytes (off,value) -> B.take off bytes <> value <> B.drop (off+B.length value) bytes)

stockConfig :: B.ByteString -> Either String Config
stockConfig payload = do
  halves <- mapM (\off -> slice payload off 512) [0x48,0x848]
  mapM_ (\(base,half) -> mapM_ (\i -> do copy <- slice payload (base+512*i) 512; require (copy == half) "stock config unit copies differ") [1..3]) (zip [0x48,0x848] halves)
  let bytes = B.concat halves
  mapM_ (\(off,wanted) -> do got <- le bytes off 4; require (got == wanted) "stock config consumer field mismatch")
    ([(0,0x00ffaa55),(4,0x01234567),(8,0x89abcdef),(0x10,0x300),(0x14,0x11),(0x18,2),(0x54,0x1a0000),(0x330,0x22312131),(0x334,0x605),(0x348,7)] ++ [(off,0) | off <- [0xb8,0xc0,0xc8,0xd0]])
  let pairs = [(0xfffb2004,0x5f5f),(0xfffb2008,1),(0xfffb200c,0x11),(0xfffb2010,0x1111),(0xfffb2014,0x1111),(0xfff98100,0x50000100),(0xfff98104,0)]
  mapM_ (\(i,(address,value)) -> do a <- le bytes (0x34c+8*i) 4; v <- le bytes (0x350+8*i) 4; require (a == address && v == value) "stock config pair write mismatch") (zip [0..] pairs)
  pure (Config bytes)

-- Where in 0x80000304 the LED lights.
-- AfterApply: the +0x348 table runs last (0x800006cc-0x80000714), after the clock,
-- memory and +0x33c SD-divider steps, before mode 1 opens ffboot.bin.
-- BeforeMemoryWait: slot +0xb8 (0x80000510) enables 0xfff88084 after the 0xfff88080,
-- 0x80000b50 and 0xfff88038 waits; slots +0xc8/+0xd0 (0x80000600-0x80000628) set up and
-- light the LED after the 0xfff01000 register writes, before the 0xfff01090 wait.
-- Nonzero +0xb8 and +0xc8 enable the +0x158 and +0x15c cycle delays.
data StageMarker = AfterApply | BeforeMemoryWait deriving (Eq, Show)

-- 0xfff893c0 = 0x00010000 is the lit state of the blink payload's long gap on this body.
ledSetup, ledLit :: (Int,Int)
ledSetup = (0xfff893c4,0x00050001)
ledLit = (0xfff893c0,0x00010000)

writePairs :: [(Int,Int)] -> B.ByteString
writePairs writes = B.concat [putLE 4 address <> putLE 4 value | (address,value) <- writes]

-- +0x10 bit 0 is stored to 0xfff88084 at 0x80000330; bit 1 is the argument of the
-- ffboot-failure teardown 0x80000b3c, which also stores it to 0xfff88084.
stageMarkerConfig :: StageMarker -> Config -> Either String Config
stageMarkerConfig marker (Config bytes) = do
  control <- le bytes 0x10 4
  count <- le bytes 0x348 4
  optionalSlots <- slice bytes 0xb8 0x20
  require (control == 0x300 && count == 7 && B.all (== 0) optionalSlots) "stage marker requires the stock control word, pair count and empty optional slots"
  case marker of
    AfterApply -> do
      let tableEnd = 0x34c+8*count
          extension = writePairs [(0xfff88084,1),ledSetup,ledLit]
      tableTail <- slice bytes tableEnd (B.length extension)
      require (B.all (== 0) tableTail) "stage marker pairs would overwrite nonzero configuration bytes"
      pure (Config (patch bytes [(0x10,putLE 4 0x302),(0x348,putLE 4 (count+3)),(tableEnd,extension)]))
    BeforeMemoryWait ->
      pure (Config (patch bytes [(0x10,putLE 4 0x302),(0xb8,writePairs [(0xfff88084,1),(0,0),ledSetup,ledLit])]))

-- With bit 0 set, the 0x80000330 store repeats the 1 written at 0x800000ec, so on the
-- success path 0xfff88084 bit 0 stays 1 through the ffboot.bin read and handoff.
powerHoldConfig :: Config -> Either String Config
powerHoldConfig (Config bytes) = do
  control <- le bytes 0x10 4
  require (control .&. 1 == 0) "power hold requires +0x10 bit 0 clear"
  pure (Config (patch bytes [(0x10,putLE 4 (control .|. 1))]))

-- ROM mount: 0x80001ae0 and 0x80001ba0, not a filesystem-library mount.
mountRom :: B.ByteString -> Either String Volume
mountRom image = do
  first <- slice image 0 512
  require (B.drop 510 first == "\x55\xaa") "sector zero signature absent"
  let tagged b = B.take 2 (B.drop 54 b) == "FA" || B.take 2 (B.drop 82 b) == "FA"
  start <- if tagged first then pure 0 else do
    typ <- le first (0x1be+4) 1
    require (typ /= 0) "first partition absent"
    le first (0x1be+8) 4
  boot <- slice image (start*512) 512
  require (B.drop 510 boot == "\x55\xaa" && tagged boot) "volume signature or FA tag absent"
  spc <- le boot 13 1
  reserved <- le boot 14 2
  fats <- le boot 16 1
  entries <- le boot 17 2
  total16 <- le boot 19 2
  total <- if total16 /= 0 then pure total16 else le boot 32 4
  fat16 <- le boot 22 2
  fatSize <- if fat16 /= 0 then pure fat16 else le boot 36 4
  require (spc > 0 && spc .&. (spc-1) == 0 && reserved > 0 && fats > 0 && fatSize > 0 && entries `mod` 16 == 0) "invalid BPB geometry"
  let metadata = reserved + fats*fatSize + entries `div` 16
  require (total > metadata && start+total <= B.length image `div` 512) "volume exceeds image or has no data"
  let limit = (total-metadata) `div` spc + 2
      kind | limit < 0xff7 = Fat12 | limit < 0xfff7 = Fat16 | otherwise = Fat32
      capacity = case kind of Fat12 -> fatSize*512*2 `div` 3; Fat16 -> fatSize*256; Fat32 -> fatSize*128
      fat = start+reserved
      dat = start+metadata
  require (limit <= capacity) "FAT too short for data clusters"
  root <- if kind == Fat32 then le boot 44 4 else pure (fat+fats*fatSize)
  require (kind /= Fat32 || (entries == 0 && root >= 2 && root < limit)) "invalid FAT32 root"
  pure (Volume kind fat root dat spc limit entries)

nextCluster :: B.ByteString -> Volume -> Int -> Either String Int
nextCluster image vol cluster = do
  require (cluster >= 2 && cluster < clusterLimit vol) "invalid cluster"
  let base = firstFat vol * 512
  value <- case fatKind vol of
    Fat12 -> do w <- le image (base+cluster+cluster `div` 2) 2; pure ((if odd cluster then w `shiftR` 4 else w) .&. 0xfff)
    Fat16 -> le image (base+cluster*2) 2
    Fat32 -> (.&. 0xfffffff) <$> le image (base+cluster*4) 4
  let end = case fatKind vol of Fat12 -> 0xff8; Fat16 -> 0xfff8; Fat32 -> 0xffffff8
  pure (if value >= end then 0 else value)

clusterBytes :: B.ByteString -> Volume -> Int -> Either String B.ByteString
clusterBytes image vol cluster = do
  require (cluster >= 2 && cluster < clusterLimit vol) "cluster outside data region"
  slice image ((dataStart vol+(cluster-2)*clusterSectors vol)*512) (clusterSectors vol*512)

chainBytes :: B.ByteString -> Volume -> Int -> Int -> Either String B.ByteString
chainBytes image vol start wanted = B.concat . reverse <$> go S.empty start wanted []
  where
    go seen cluster remaining chunks
      | remaining <= 0 = pure chunks
      | otherwise = do
          require (cluster /= 0 && S.notMember cluster seen) "short or cyclic cluster chain"
          chunk <- clusterBytes image vol cluster
          next <- nextCluster image vol cluster
          go (S.insert cluster seen) next (remaining-B.length chunk) (B.take remaining chunk:chunks)

readRomFile :: B.ByteString -> Volume -> BootFile -> Either String B.ByteString
readRomFile image vol file = do
  directory <- if fatKind vol == Fat32 then rootChain S.empty (rootLocation vol) else slice image (rootLocation vol*512) (rootEntries vol*32)
  findEntry directory 0
  where
    name = shortName file
    rootChain seen cluster = do
      require (S.notMember cluster seen) "cyclic root chain"
      chunk <- clusterBytes image vol cluster
      next <- nextCluster image vol cluster
      rest <- if next == 0 then pure B.empty else rootChain (S.insert cluster seen) next
      pure (chunk <> rest)
    findEntry bytes off = do
      entry <- slice bytes off 32
      require (B.head entry /= 0) "short name not found"
      attr <- le entry 11 1
      if B.head entry /= 0xe5 && attr .&. 8 == 0 && B.take 11 entry == name then do
        require (attr .&. 16 == 0) "file is directory"
        low <- le entry 26 2
        high <- le entry 20 2
        size <- le entry 28 4
        chainBytes image vol (low .|. (high `shiftL` 16)) size
      else findEntry bytes (off+32)

-- Mode 2 reads config+0x18, masked to 18 bits, then stages from LBA 2.
readRawRom :: B.ByteString -> Either String Mode2Load
readRawRom image = do
  config <- slice image 0 1024
  count <- RawSectorCount . (.&. 0x3ffff) <$> le config 0x18 4
  payload <- stageRawPayload image count
  pure (Mode2Load config payload)

stageRawPayload :: B.ByteString -> RawSectorCount -> Either String B.ByteString
stageRawPayload image (RawSectorCount blocks) = slice image 1024 (blocks*512)

rawConfigDifferences :: Config -> Mode2Load -> [ConfigDifference]
rawConfigDifferences config loaded =
  [ ConfigDifference (ConfigWordOffset off) old new
  | off <- [0,4..1020]
  , let old = word (configBytes config) off
        new = word (mode2ConfigBytes loaded) off
  , old /= new
  ]
  where
    word bytes off = sum
      [ fromIntegral (B.index bytes (off+i)) `shiftL` (8*i) | i <- [0..3] ]

rawConfiguration :: Config -> RawSectorCount -> B.ByteString
rawConfiguration config (RawSectorCount blocks) = patch (configBytes config)
  [ (0x18,putLE 4 blocks)
  , (0x1c2,"\x04")
  , (0x1c6,putLE 4 2048)
  , (510,"\x55\xaa")
  ]

bootCard :: Config -> B.ByteString -> Either String B.ByteString
bootCard config payload = do
  count@(RawSectorCount blocks) <- payloadSectorCount payload
  let start = 2048; total = 32768; fatSize = 128; entries = 512
      boot = patch (B.replicate 512 0)
        [(0,"\xeb\x3c\x90XM1DIAG "),(11,putLE 2 512),(13,putLE 1 1),(14,putLE 2 1),(16,putLE 1 2),(17,putLE 2 entries),(19,putLE 2 total),(21,"\xf8"),(22,putLE 2 fatSize),(24,putLE 2 32),(26,putLE 2 64),(28,putLE 4 start),(36,"\x80\x00\x29"),(39,putLE 4 0x150),(43,"XM1DIAG    FAT16   "),(510,"\x55\xaa")]
      -- Stock bytes remain in MBR fields the ROM does not consume.
      rawConfig = rawConfiguration config count
      fatEntries = [0xfff8,0xffff,3,0xffff] ++ [if cluster == blocks+3 then 0xffff else cluster+1 | cluster <- [4..blocks+3]]
      fatBytes = B.concat (map (putLE 2) fatEntries)
      fat = fatBytes <> B.replicate (fatSize*512-B.length fatBytes) 0
      entry name cluster size = patch (B.replicate 32 0) [(0,name),(11,"\x20"),(26,putLE 2 cluster),(28,putLE 4 size)]
      root = entry "FFSYS   BIN" 2 1024 <> entry "FFBOOT  BIN" 4 (B.length payload) <> B.replicate (entries*32-64) 0
      prefix = rawConfig <> payload <> B.replicate ((start-2-blocks)*512) 0 <> boot <> fat <> fat <> root <> configBytes config <> payload
      image = prefix <> B.replicate ((start+total+1)*512-B.length prefix) 0
  verifyCard config payload image
  pure image

verifyCard :: Config -> B.ByteString -> B.ByteString -> Either String ()
verifyCard config payload image = do
  count <- payloadSectorCount payload
  vol <- mountRom image
  sys <- readRomFile image vol SystemConfig
  stage <- readRomFile image vol BootPayload
  require (sys == configBytes config && B.length sys == 1024) "ROM-read config differs"
  require (stage == payload) "ROM-read payload differs"
  raw <- readRawRom image
  require (mode2ConfigBytes raw == rawConfiguration config count) "raw-mode configuration differs"
  require (mode2PayloadBytes raw == payload) "raw-mode staged payload differs"
  pure ()

stagedRecoveryCard :: Config -> B.ByteString -> B.ByteString -> Either String B.ByteString
stagedRecoveryCard config bootstrap stage = do
  count@(RawSectorCount bootstrapBlocks) <- payloadSectorCount bootstrap
  stageCount <- payloadSectorCount stage
  let RawSectorCount stageBlocks = stageCount
  require (bootstrapBlocks == 1) "staged bootstrap must occupy one sector"
  require (stageBlocks == 512 && B.length stage == 0x40000) "recovery stage must occupy 512 sectors"
  let start = 2048; total = 32768; fatSize = 128; entries = 512
      boot = patch (B.replicate 512 0)
        [(0,"\xeb\x3c\x90XM1STAGE"),(11,putLE 2 512),(13,putLE 1 1),(14,putLE 2 1),(16,putLE 1 2),(17,putLE 2 entries),(19,putLE 2 total),(21,"\xf8"),(22,putLE 2 fatSize),(24,putLE 2 32),(26,putLE 2 64),(28,putLE 4 start),(36,"\x80\x00\x29"),(39,putLE 4 0x150),(43,"XM1STAGE   FAT16   "),(510,"\x55\xaa")]
      rawConfig = rawConfiguration config count
      stageStart = 5
      stageChain = fatChain stageStart stageBlocks
      fatEntries = [0xfff8,0xffff,3,0xffff,0xffff] ++ stageChain
      fatBytes = B.concat (map (putLE 2) fatEntries)
      fat = fatBytes <> B.replicate (fatSize*512-B.length fatBytes) 0
      entry name cluster size = patch (B.replicate 32 0) [(0,name),(11,"\x20"),(26,putLE 2 cluster),(28,putLE 4 size)]
      root = entry (shortName SystemConfig) 2 1024
          <> entry (shortName BootPayload) 4 (B.length bootstrap)
          <> entry (shortName RecoveryStage) stageStart (B.length stage)
          <> B.replicate (entries*32-96) 0
      rawPrefix = rawConfig <> bootstrap <> stage
          <> B.replicate ((start-2-bootstrapBlocks-stageBlocks)*512) 0
      prefix = rawPrefix <> boot <> fat <> fat <> root <> configBytes config <> bootstrap <> stage
      image = prefix <> B.replicate ((start+total+1)*512-B.length prefix) 0
  verifyStagedRecoveryCard config bootstrap stage image
  pure image

verifyStagedRecoveryCard :: Config -> B.ByteString -> B.ByteString -> B.ByteString -> Either String ()
verifyStagedRecoveryCard config bootstrap stage image = do
  vol <- mountRom image
  sys <- readRomFile image vol SystemConfig
  first <- readRomFile image vol BootPayload
  second <- readRomFile image vol RecoveryStage
  raw <- readRawRom image
  rawStage <- slice image (3*512) (B.length stage)
  require (sys == configBytes config) "staged FAT configuration differs"
  require (first == bootstrap) "staged FAT bootstrap differs"
  require (second == stage) "staged FAT recovery body differs"
  require (mode2ConfigBytes raw == rawConfiguration config (RawSectorCount 1)) "staged raw configuration differs"
  require (mode2PayloadBytes raw == bootstrap) "staged raw bootstrap differs"
  require (rawStage == stage) "staged raw recovery body differs"

stagedRepairCard :: Config -> B.ByteString -> B.ByteString -> B.ByteString -> Either String B.ByteString
stagedRepairCard config bootstrap stage repair = do
  RawSectorCount bootstrapBlocks <- payloadSectorCount bootstrap
  RawSectorCount stageBlocks <- payloadSectorCount stage
  RawSectorCount repairBlocks <- payloadSectorCount repair
  require (bootstrapBlocks == 1) "persistent bootstrap must occupy one sector"
  require (stageBlocks == 512 && B.length stage == 0x40000) "persistent stage must occupy 512 sectors"
  require (repairBlocks `elem` [256,768] && B.length repair `elem` [0x20000,0x60000]) "persistent repair body must occupy 256 or 768 sectors"
  let start = 2048; total = 32768; fatSize = 128; entries = 512
      body = stage <> repair
      bodyBlocks = stageBlocks + repairBlocks
      boot = patch (B.replicate 512 0)
        [(0,"\xeb\x3c\x90XM1FIX  "),(11,putLE 2 512),(13,putLE 1 1),(14,putLE 2 1),(16,putLE 1 2),(17,putLE 2 entries),(19,putLE 2 total),(21,"\xf8"),(22,putLE 2 fatSize),(24,putLE 2 32),(26,putLE 2 64),(28,putLE 4 start),(36,"\x80\x00\x29"),(39,putLE 4 0x150),(43,"XM1FIX     FAT16   "),(510,"\x55\xaa")]
      rawConfig = rawConfiguration config (RawSectorCount bootstrapBlocks)
      bodyStart = 5
      bodyChain = fatChain bodyStart bodyBlocks
      fatEntries = [0xfff8,0xffff,3,0xffff,0xffff] ++ bodyChain
      fatBytes = B.concat (map (putLE 2) fatEntries)
      fat = fatBytes <> B.replicate (fatSize*512-B.length fatBytes) 0
      entry name cluster size = patch (B.replicate 32 0) [(0,name),(11,"\x20"),(26,putLE 2 cluster),(28,putLE 4 size)]
      root = entry (shortName SystemConfig) 2 1024
          <> entry (shortName BootPayload) 4 (B.length bootstrap)
          <> entry (shortName RecoveryStage) bodyStart (B.length body)
          <> B.replicate (entries*32-96) 0
      rawPrefix = rawConfig <> bootstrap <> body
          <> B.replicate ((start-2-bootstrapBlocks-bodyBlocks)*512) 0
      prefix = rawPrefix <> boot <> fat <> fat <> root <> configBytes config <> bootstrap <> body
      image = prefix <> B.replicate ((start+total+1)*512-B.length prefix) 0
  verifyStagedRepairCard config bootstrap stage repair image
  pure image

verifyStagedRepairCard :: Config -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Either String ()
verifyStagedRepairCard config bootstrap stage repair image = do
  vol <- mountRom image
  sys <- readRomFile image vol SystemConfig
  first <- readRomFile image vol BootPayload
  body <- readRomFile image vol RecoveryStage
  raw <- readRawRom image
  rawStage <- slice image (3*512) (B.length stage)
  rawRepair <- slice image ((3*512)+B.length stage) (B.length repair)
  require (sys == configBytes config) "persistent FAT configuration differs"
  require (first == bootstrap) "persistent FAT bootstrap differs"
  require (body == stage <> repair) "persistent FAT body differs"
  require (mode2ConfigBytes raw == rawConfiguration config (RawSectorCount 1)) "persistent raw configuration differs"
  require (mode2PayloadBytes raw == bootstrap) "persistent raw bootstrap differs"
  require (rawStage == stage) "persistent raw recovery stage differs"
  require (rawRepair == repair) "persistent raw repair block differs"

recoveryPartitionStart, recoveryPartitionSectors, recoveryClusterSectors, recoveryFatSectors, recoveryRootEntries :: Int
recoveryPartitionStart = 2048
recoveryPartitionSectors = 131072
recoveryClusterSectors = 4
recoveryFatSectors = 128
recoveryRootEntries = 512

recoveryReportBaseLba, recoveryReportSectorCount :: Int
recoveryReportBaseLba = recoveryPartitionStart + recoveryPartitionSectors
recoveryReportSectorCount = 4

clustersFor :: Int -> Int -> Int
clustersFor bytes clusterSize = (bytes + clusterSize - 1) `div` clusterSize

padTo :: Int -> B.ByteString -> B.ByteString
padTo alignment bytes = bytes <> B.replicate ((alignment - B.length bytes `mod` alignment) `mod` alignment) 0

fatChain :: Int -> Int -> [Int]
fatChain start count =
  [ if index + 1 == count then 0xffff else start + index + 1
  | index <- [0..count-1]
  ]

data FirmwareLayout = FirmwareLayout [Int] [Int] CardSector

firmwareLayout :: [Int] -> Either String FirmwareLayout
firmwareLayout sizes = do
  require (length sizes == 4 && all (> 0) sizes) "firmware recovery requires four nonempty files"
  let counts = map (`clustersFor` (recoveryClusterSectors*512)) sizes
      starts = take 4 (scanl (+) 2 counts)
      metadata = 1 + 2*recoveryFatSectors + recoveryRootEntries `div` 16
      limit = (recoveryPartitionSectors-metadata) `div` recoveryClusterSectors + 2
      end = 2 + sum counts
      body = recoveryPartitionStart + metadata + (starts !! 2-2)*recoveryClusterSectors
  require (end <= limit && end <= recoveryFatSectors*256) "firmware recovery files exceed FAT16 volume"
  pure (FirmwareLayout starts counts (CardSector body))

recoveryFat32Reserved, recoveryFat32FatSectors :: Int
recoveryFat32Reserved = 32
recoveryFat32FatSectors = 1024

firmwareLayoutFat32 :: [Int] -> Either String FirmwareLayout
firmwareLayoutFat32 sizes = do
  require (length sizes == 4 && all (\size -> size > 0 && size <= recoveryPartitionSectors*512) sizes)
    "firmware recovery requires four nonempty files within volume capacity"
  let counts = map (`clustersFor` 512) sizes
      starts = take 4 (scanl (+) 3 counts)
      metadata = recoveryFat32Reserved + 2*recoveryFat32FatSectors
      limit = recoveryPartitionSectors - metadata + 2
      end = 3 + sum counts
      body = recoveryPartitionStart + metadata + starts !! 2 - 2
  require (limit-2 > 65525 && end <= limit && limit <= recoveryFat32FatSectors*128)
    "firmware recovery files exceed FAT32 volume"
  pure (FirmwareLayout starts counts (CardSector body))

firmwareRecoveryBodyLbaFat32 :: B.ByteString -> Either String CardSector
firmwareRecoveryBodyLbaFat32 bootstrap = do
  require (B.length bootstrap == 4096) "firmware bootstrap must occupy exactly 4096 bytes"
  FirmwareLayout _ _ sector <- firmwareLayoutFat32 [1024,B.length bootstrap,512,1]
  pure sector

firmwareRecoveryCardFat32 :: Config -> B.ByteString -> B.ByteString -> B.ByteString -> Either String B.ByteString
firmwareRecoveryCardFat32 config bootstrap body restore = do
  count <- payloadSectorCount bootstrap
  require (B.length bootstrap == 4096 && B.length (configBytes config) == 1024) "firmware entry/configuration size mismatch"
  require (not (B.null body) && B.length body `mod` 512 == 0) "firmware body must be nonempty and sector-aligned"
  let files = [configBytes config,bootstrap,body,restore]
  FirmwareLayout _ _ _ <- firmwareLayoutFat32 (map B.length files)
  image <- recoveryFat32Image config count bootstrap
    (zip [SystemConfig,BootPayload,RecoveryStage,RestoreContainer] files)
  verifyFirmwareRecoveryCard config bootstrap body restore image
  pure image

firmwareRecoveryBodyLba :: B.ByteString -> Either String CardSector
firmwareRecoveryBodyLba bootstrap = do
  require (B.length bootstrap == 4096) "firmware bootstrap must occupy exactly 4096 bytes"
  FirmwareLayout _ _ sector <- firmwareLayout [1024,B.length bootstrap,512,1]
  pure sector

firmwareRecoveryCard :: Config -> B.ByteString -> B.ByteString -> B.ByteString -> Either String B.ByteString
firmwareRecoveryCard config bootstrap body restore = do
  count <- payloadSectorCount bootstrap
  require (B.length bootstrap == 4096 && B.length (configBytes config) == 1024) "firmware entry/configuration size mismatch"
  require (not (B.null body) && B.length body `mod` 512 == 0) "firmware body must be nonempty and sector-aligned"
  let files = [configBytes config,bootstrap,body,restore]
      sizes = map B.length files
  FirmwareLayout starts counts _ <- firmwareLayout sizes
  let boot = patch (B.replicate 512 0)
        [ (0,"\xeb\x3c\x90XM1RECV "), (11,putLE 2 512), (13,putLE 1 recoveryClusterSectors)
        , (14,putLE 2 1), (16,putLE 1 2), (17,putLE 2 recoveryRootEntries)
        , (21,"\xf8"), (22,putLE 2 recoveryFatSectors), (24,putLE 2 32), (26,putLE 2 64)
        , (28,putLE 4 recoveryPartitionStart), (32,putLE 4 recoveryPartitionSectors)
        , (36,"\x80\x00\x29"), (39,putLE 4 0x150), (43,"XM1RECOVERYFAT16   "), (510,"\x55\xaa")
        ]
      fatBytes = B.concat (map (putLE 2) ([0xfff8,0xffff] <> concat (zipWith fatChain starts counts)))
      fat = fatBytes <> B.replicate (recoveryFatSectors*512-B.length fatBytes) 0
      entry file cluster size = patch (B.replicate 32 0)
        [(0,shortName file),(11,"\x20"),(26,putLE 2 cluster),(28,putLE 4 size)]
      directory = B.concat (zipWith3 entry [SystemConfig,BootPayload,RecoveryStage,RestoreContainer] starts sizes)
      root = directory <> B.replicate (recoveryRootEntries*32-B.length directory) 0
      volumePrefix = B.concat ([boot,fat,fat,root] ++ map (padTo (recoveryClusterSectors*512)) files)
      rawPrefix = rawConfiguration config count <> bootstrap
        <> B.replicate (recoveryPartitionStart*512-1024-B.length bootstrap) 0
      image = B.concat [rawPrefix,volumePrefix,B.replicate (recoveryPartitionSectors*512-B.length volumePrefix) 0,B.replicate (recoveryReportSectorCount*512) 0]
  verifyFirmwareRecoveryCard config bootstrap body restore image
  pure image

verifyFirmwareRecoveryCard :: Config -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Either String ()
verifyFirmwareRecoveryCard config bootstrap body restore image = do
  require (B.length bootstrap == 4096 && B.length body > 0 && B.length body `mod` 512 == 0) "firmware recovery entry/body size mismatch"
  vol <- mountRom image
  FirmwareLayout starts counts (CardSector bodyLba) <- case fatKind vol of
    Fat16 -> do
      require (clusterSectors vol == recoveryClusterSectors) "firmware recovery FAT16 cluster geometry differs"
      firmwareLayout (map B.length [configBytes config,bootstrap,body,restore])
    Fat32 -> do
      require (B.length image == (recoveryReportBaseLba+recoveryReportSectorCount)*512 &&
        B.length image <= 128*1024*1024) "firmware recovery FAT32 image capacity differs"
      require (clusterSectors vol == 1 && rootLocation vol == 2 &&
        clusterLimit vol-2 > 65525 &&
        firstFat vol == recoveryPartitionStart+recoveryFat32Reserved &&
        dataStart vol == recoveryPartitionStart+recoveryFat32Reserved+2*recoveryFat32FatSectors)
        "firmware recovery FAT32 geometry differs"
      firmwareLayoutFat32 (map B.length [configBytes config,bootstrap,body,restore])
    Fat12 -> Left "firmware recovery volume must be FAT16 or FAT32"
  actual <- mapM (readRomFile image vol) [SystemConfig,BootPayload,RecoveryStage,RestoreContainer]
  require (actual == [configBytes config,bootstrap,body,restore]) "firmware recovery FAT files differ"
  raw <- readRawRom image
  require (mode2ConfigBytes raw == rawConfiguration config (RawSectorCount 8) && mode2PayloadBytes raw == bootstrap) "firmware recovery raw bootstrap differs"
  contiguous <- slice image (bodyLba*512) (B.length body)
  require (contiguous == body) "firmware recovery raw body differs"
  let root = if fatKind vol == Fat32 then dataStart vol*512+32 else rootLocation vol*512
  low <- le image (root+2*32+26) 2
  high <- le image (root+2*32+20) 2
  let actualStart = low .|. (high `shiftL` 16)
  require (actualStart == starts !! 2 && dataStart vol+(actualStart-2)*clusterSectors vol == bodyLba) "firmware recovery body starts at unexpected LBA"
  mapM_ (\cluster -> do
    next <- nextCluster image vol cluster
    require (next == if cluster+1 == actualStart+counts !! 2 then 0 else cluster+1) "firmware recovery body is not contiguous") [actualStart..actualStart+counts !! 2-1]
  reports <- recoveryReportSectors image
  require (all (B.all (== 0)) reports) "firmware recovery report sectors are not initially empty"

recoveryCard :: Config -> B.ByteString -> B.ByteString -> B.ByteString -> Either String B.ByteString
recoveryCard config rawPayload payload restore = do
  count <- payloadSectorCount rawPayload
  require (B.length payload >= B.length rawPayload && B.take (B.length rawPayload) payload == rawPayload)
    "mode-1 payload must begin with the raw-mode refusal payload"
  require (not (B.null restore)) "restore container must be nonempty"
  let clusterSize = recoveryClusterSectors * 512
      sizes = map B.length [configBytes config, payload, restore]
      counts = map (`clustersFor` clusterSize) sizes
      starts = take 3 (scanl (+) 2 counts)
      finalCluster = last starts + last counts
      fatCapacity = recoveryFatSectors * 256
  require (all (> 0) counts && finalCluster <= fatCapacity) "recovery files exceed FAT16 cluster capacity"
  let boot = patch (B.replicate 512 0)
        [ (0,"\xeb\x3c\x90XM1RECV ")
        , (11,putLE 2 512)
        , (13,putLE 1 recoveryClusterSectors)
        , (14,putLE 2 1)
        , (16,putLE 1 2)
        , (17,putLE 2 recoveryRootEntries)
        , (19,putLE 2 0)
        , (21,"\xf8")
        , (22,putLE 2 recoveryFatSectors)
        , (24,putLE 2 32)
        , (26,putLE 2 64)
        , (28,putLE 4 recoveryPartitionStart)
        , (32,putLE 4 recoveryPartitionSectors)
        , (36,"\x80\x00\x29")
        , (39,putLE 4 0x150)
        , (43,"XM1RECOVERYFAT16   ")
        , (510,"\x55\xaa")
        ]
      fatValues = [0xfff8,0xffff] <> concat (zipWith fatChain starts counts)
      fatBytes = B.concat (map (putLE 2) fatValues)
      fat = fatBytes <> B.replicate (recoveryFatSectors*512-B.length fatBytes) 0
      entry file cluster size = patch (B.replicate 32 0)
        [ (0,shortName file), (11,"\x20"), (26,putLE 2 cluster), (28,putLE 4 size) ]
      rootEntries = zipWith3 entry [SystemConfig,BootPayload,RestoreContainer] starts sizes
      root = B.concat rootEntries <> B.replicate (recoveryRootEntries*32-32*length rootEntries) 0
      fileData = B.concat (map (padTo clusterSize) [configBytes config,payload,restore])
      volumePrefix = boot <> fat <> fat <> root <> fileData
      volume = volumePrefix <> B.replicate (recoveryPartitionSectors*512-B.length volumePrefix) 0
      rawPrefix = rawConfiguration config count <> rawPayload
        <> B.replicate (recoveryPartitionStart*512-1024-B.length rawPayload) 0
      image = rawPrefix <> volume <> B.replicate (recoveryReportSectorCount*512) 0
  require (B.length rawPrefix == recoveryPartitionStart*512) "raw-mode payload overlaps recovery FAT volume"
  verifyRecoveryCard config rawPayload payload restore image
  pure image

recoveryCardFat32 :: Config -> B.ByteString -> B.ByteString -> B.ByteString -> Either String B.ByteString
recoveryCardFat32 config rawPayload payload restore = do
  count <- payloadSectorCount rawPayload
  require (B.length payload >= B.length rawPayload && B.take (B.length rawPayload) payload == rawPayload)
    "mode-1 payload must begin with the raw-mode refusal payload"
  require (not (B.null restore)) "restore container must be nonempty"
  image <- recoveryFat32Image config count rawPayload
    (zip [SystemConfig,BootPayload,RestoreContainer] [configBytes config,payload,restore])
  verifyRecoveryCard config rawPayload payload restore image
  pure image

recoveryFat32Image :: Config -> RawSectorCount -> B.ByteString -> [(BootFile,B.ByteString)] -> Either String B.ByteString
recoveryFat32Image config count rawPayload namedFiles = do
  let files = map snd namedFiles
      reserved = recoveryFat32Reserved
      fatSectors = recoveryFat32FatSectors
      clusterSize = 512
      sizes = map B.length files
  require (not (null files) && length files <= 15 &&
    all (\size -> size > 0 && size <= recoveryPartitionSectors*512) sizes)
    "recovery files exceed FAT32 volume capacity"
  let
      counts = map (`clustersFor` clusterSize) sizes
      starts = take (length files) (scanl (+) 3 counts)
      finalCluster = 3 + sum counts
      metadataSectors = reserved + 2*fatSectors
      clusterLimit = recoveryPartitionSectors - metadataSectors + 2
      fatCapacity = fatSectors * 128
  require (all (> 0) counts && finalCluster <= clusterLimit && clusterLimit <= fatCapacity)
    "recovery files exceed FAT32 cluster capacity"
  let boot = patch (B.replicate 512 0)
        [ (0,"\xeb\x3c\x90XM1RECV ")
        , (11,putLE 2 512)
        , (13,putLE 1 1)
        , (14,putLE 2 reserved)
        , (16,putLE 1 2)
        , (17,putLE 2 0)
        , (19,putLE 2 0)
        , (21,"\xf8")
        , (22,putLE 2 0)
        , (24,putLE 2 32)
        , (26,putLE 2 64)
        , (28,putLE 4 recoveryPartitionStart)
        , (32,putLE 4 recoveryPartitionSectors)
        , (36,putLE 4 fatSectors)
        , (44,putLE 4 2)
        , (48,putLE 2 1)
        , (50,putLE 2 6)
        , (64,"\x80")
        , (66,"\x29")
        , (67,putLE 4 0x150)
        , (71,"XM1RECOVERY")
        , (82,"FAT32   ")
        , (510,"\x55\xaa")
        ]
      usedClusters = 1 + sum counts
      freeClusters = clusterLimit - 2 - usedClusters
      fsInfo = patch (B.replicate 512 0)
        [ (0,putLE 4 0x41615252)
        , (484,putLE 4 0x61417272)
        , (488,putLE 4 freeClusters)
        , (492,putLE 4 finalCluster)
        , (508,putLE 4 0xaa550000)
        ]
      reservedBytes = patch (B.replicate (reserved*512) 0)
        [ (0,boot), (512,fsInfo), (6*512,boot), (7*512,fsInfo) ]
      chain start clusterCount =
        [ if index + 1 == clusterCount then 0x0fffffff else start + index + 1
        | index <- [0..clusterCount-1]
        ]
      fatValues = [0x0ffffff8,0xffffffff,0x0fffffff] <> concat (zipWith chain starts counts)
      fatBytes = B.concat (map (putLE 4) fatValues)
      fat = fatBytes <> B.replicate (fatSectors*512-B.length fatBytes) 0
      entry file cluster size = patch (B.replicate 32 0)
        [ (0,shortName file)
        , (11,"\x20")
        , (20,putLE 2 (cluster `shiftR` 16))
        , (26,putLE 2 cluster)
        , (28,putLE 4 size)
        ]
      label = patch (B.replicate 32 0) [(0,"XM1RECOVERY"),(11,"\x08")]
      rootEntries = zipWith3 entry (map fst namedFiles) starts sizes
      root = label <> B.concat rootEntries <> B.replicate (clusterSize-32*(1+length rootEntries)) 0
      fileData = B.concat (map (padTo clusterSize) files)
      volumePrefix = reservedBytes <> fat <> fat <> root <> fileData
      volume = volumePrefix <> B.replicate (recoveryPartitionSectors*512-B.length volumePrefix) 0
      rawPrefix = rawConfiguration config count <> rawPayload
        <> B.replicate (recoveryPartitionStart*512-1024-B.length rawPayload) 0
      image = rawPrefix <> volume <> B.replicate (recoveryReportSectorCount*512) 0
  require (B.length rawPrefix == recoveryPartitionStart*512) "raw-mode payload overlaps recovery FAT volume"
  vol <- mountRom image
  require (fatKind vol == Fat32) "recovery volume is not FAT32"
  pure image

verifyRecoveryCard :: Config -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Either String ()
verifyRecoveryCard config rawPayload payload restore image = do
  vol <- mountRom image
  sys <- readRomFile image vol SystemConfig
  stage <- readRomFile image vol BootPayload
  container <- readRomFile image vol RestoreContainer
  raw <- readRawRom image
  require (sys == configBytes config) "recovery FAT configuration differs"
  require (stage == payload) "recovery FAT payload differs"
  require (container == restore) "recovery FAT restore container differs"
  require (mode2ConfigBytes raw == rawConfiguration config (RawSectorCount (B.length rawPayload `div` 512)))
    "recovery raw-mode configuration differs"
  require (mode2PayloadBytes raw == rawPayload) "recovery raw-mode refusal payload differs"
  reports <- recoveryReportSectors image
  require (all (B.all (== 0)) reports) "recovery report sectors are not initially empty"

recoveryReportSectors :: B.ByteString -> Either String [B.ByteString]
recoveryReportSectors image =
  mapM (\lba -> slice image (lba*512) 512)
    [recoveryReportBaseLba .. recoveryReportBaseLba+recoveryReportSectorCount-1]
