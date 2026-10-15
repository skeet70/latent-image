{-# LANGUAGE OverloadedStrings #-}
module Main where

import BootCard
import Control.Monad (forM_, unless)
import Data.Bits (complement, shiftL, shiftR)
import qualified Data.ByteString as B
import Data.Either (isLeft)
import Data.Word (Word32)
import RecoveryStageReport
import System.Environment (getArgs)
import System.IO (IOMode (ReadMode), withBinaryFile)

word :: Int -> Int -> B.ByteString
word count value = B.pack [fromIntegral (value `shiftR` (8*i)) | i <- [0..count-1]]

change :: B.ByteString -> Int -> B.ByteString -> B.ByteString
change bytes off value = B.take off bytes <> value <> B.drop (off+B.length value) bytes

fields :: B.ByteString -> [(Int,Int)] -> B.ByteString
fields = foldl (\bytes (off,value) -> change bytes off (word 4 value))

putWord32 :: Word32 -> B.ByteString
putWord32 value = B.pack [fromIntegral (value `shiftR` (8*i)) | i <- [0..3]]

word32At :: B.ByteString -> Int -> Word32
word32At bytes offset = sum [fromIntegral (B.index bytes (offset+i)) `shiftL` (8*i) | i <- [0..3]]

stageRecord :: Word32 -> B.ByteString
stageRecord stage =
  let header = change (B.replicate 512 0) 0 "XM1STAGE"
      body = foldl (\bytes (offset,value) -> change bytes offset (putWord32 value)) header
        [ (8,1), (12,stage), (16,complement stage), (20,1), (24,0xeb34a9c9)
        , (28,1), (32,1), (36,1), (40,0x00030000), (44,7), (504,0x504d4f43) ]
      checksum = negate (sum [word32At body offset | offset <- [0,4..504]])
  in change body 508 (putWord32 checksum)

checked :: Either String a -> IO a
checked = either (ioError . userError) pure

main :: IO ()
main = do
  args <- getArgs
  payload <- case args of
    [path] -> withBinaryFile path ReadMode (\handle -> B.hGet handle 513)
    _ -> ioError (userError "payload path required")
  unless (B.length payload == 512) (ioError (userError "diagnostic payload must occupy one sector"))
  -- Synthetic configuration satisfies the stock extraction oracle.
  let pairs = [(0xfffb2004,0x5f5f),(0xfffb2008,1),(0xfffb200c,0x11),(0xfffb2010,0x1111),(0xfffb2014,0x1111),(0xfff98100,0x50000100),(0xfff98104,0)]
      config = fields (B.replicate 1024 0)
        ([(0,0x00ffaa55),(4,0x01234567),(8,0x89abcdef),(0x10,0x300),(0x14,0x11),(0x18,2),(0x54,0x1a0000),(0x1fc,0x01000101),(0x330,0x22312131),(0x334,0x605),(0x348,7)]
          ++ concat [[(0x34c+8*i,address),(0x350+8*i,value)] | (i,(address,value)) <- zip [0..] pairs])
      first = B.take 512 config
      second = B.drop 512 config
      fixture = B.replicate 0x48 0 <> B.concat (replicate 4 first) <> B.concat (replicate 4 second)
  cfg <- checked (stockConfig fixture)
  card <- checked (bootCard cfg payload)
  volume <- checked (mountRom card)
  raw <- checked (readRawRom card)
  superVolume <- checked (mountRom (B.drop (2048*512) card))
  let restore = B.replicate 4096 0xa5
  recovery <- checked (recoveryCard cfg payload payload restore)
  recoveryVolume <- checked (mountRom recovery)
  initialReports <- checked (recoveryReportSectors recovery)
  recovery32 <- checked (recoveryCardFat32 cfg payload payload restore)
  recovery32Volume <- checked (mountRom recovery32)
  recovery32Reports <- checked (recoveryReportSectors recovery32)
  let observedRecovery = change recovery ((recoveryReportBaseLba+2)*512) (stageRecord 2)
  observedReports <- checked (recoveryReportSectors observedRecovery)
  let decodedReports = decodeStageReports observedReports
      corruptedReports = decodeStageReports
        (take 2 observedReports ++ [change (observedReports !! 2) 24 "\x00"] ++ drop 3 observedReports)
  let chainPayload :: Int -> B.ByteString
      chainPayload size = B.concat [B.replicate 512 (fromIntegral sector) | sector <- [0..size `div` 512-1]]
  let stagedBody = chainPayload 0x40000
  stagedRecovery <- checked (stagedRecoveryCard cfg payload stagedBody)
  stagedVolume <- checked (mountRom stagedRecovery)
  stagedRaw <- checked (readRawRom stagedRecovery)
  let repairBlock = B.replicate 0x20000 0x5a
  persistentRecovery <- checked (stagedRepairCard cfg payload stagedBody repairBlock)
  persistentVolume <- checked (mountRom persistentRecovery)
  persistentRaw <- checked (readRawRom persistentRecovery)
  let firmwareBootstrap = chainPayload 4096
      firmwareBody = stagedBody <> repairBlock
      firmwareRestore = restore <> "unchanged DAT tail"
  firmwareRecovery <- checked (firmwareRecoveryCardFat32 cfg firmwareBootstrap firmwareBody firmwareRestore)
  firmwareVolume <- checked (mountRom firmwareRecovery)
  firmwareRaw <- checked (readRawRom firmwareRecovery)
  CardSector firmwareBodyLba <- checked (firmwareRecoveryBodyLbaFat32 firmwareBootstrap)
  let firmwareRootOffset = dataStart firmwareVolume*512
      firmwareFatOffset = firstFat firmwareVolume*512
      firmwareBodyCluster = firmwareBodyLba-dataStart firmwareVolume+2
      verifyFirmware = verifyFirmwareRecoveryCard cfg firmwareBootstrap firmwareBody firmwareRestore
  extentCases <- mapM (\size -> do
    let staged = chainPayload size
    image <- checked (bootCard cfg staged)
    mounted <- checked (mountRom image)
    loaded <- checked (readRawRom image)
    pure ("both ROM routes stage " ++ show size ++ " bytes", readRomFile image mounted BootPayload == Right staged && mode2PayloadBytes loaded == staged))
    [0x40000,bootPayloadByteCount maxBootPayloadBytes]
  let fragmented = change
        (change (change (change card (2049*512+4) (word 2 5)) (2049*512+10) (word 2 0xffff)) (2338*512) (B.replicate 512 0))
        (2340*512) second
      legacyMbr = change (change (change (B.replicate 512 0) 0x1c2 "\x04") 0x1c6 (word 4 2048)) 510 "\x55\xaa"
      legacyCard = change card 0 (legacyMbr <> B.replicate 512 0)
      cases =
        [ ("mode 1 reads unchanged stock configuration", readRomFile card volume SystemConfig == Right config)
        , ("mode 1 stages the supplied diagnostic", readRomFile card volume BootPayload == Right payload)
        , ("mode 2 stages the same diagnostic", mode2PayloadBytes raw == payload)
        , ("raw configuration changes only the count and three MBR-overlap words", rawConfigDifferences cfg raw ==
            [ ConfigDifference (ConfigWordOffset 0x18) 2 1
            , ConfigDifference (ConfigWordOffset 0x1c0) 0 0x00040000
            , ConfigDifference (ConfigWordOffset 0x1c4) 0 0x08000000
            , ConfigDifference (ConfigWordOffset 0x1fc) 0x01000101 0xaa550101
            ])
        , ("ROM follows a fragmented configuration chain", readRomFile fragmented volume SystemConfig == Right config)
        , ("ROM mounts a sector-zero FAT volume", readRomFile (B.drop (2048*512) card) superVolume BootPayload == Right payload)
        , ("raw loader masks count to eighteen bits", fmap mode2PayloadBytes (readRawRom (change card 0x18 (word 4 0x40001))) == Right payload)
        , ("zero raw count stages nothing", fmap mode2PayloadBytes (readRawRom (change card 0x18 (word 4 0))) == Right B.empty)
        , ("legacy FAT-only card is not dual-path verified", isLeft (verifyCard cfg payload legacyCard))
        , ("zero raw count cannot pass diagnostic verification", isLeft (verifyCard cfg payload (change card 0x18 (word 4 0))))
        , ("additional raw sector cannot pass diagnostic verification", isLeft (verifyCard cfg payload (change card 0x18 (word 4 2))))
        , ("raw payload corruption rejected", isLeft (verifyCard cfg payload (change card 1024 "\x00")))
        , ("raw configuration-tail corruption rejected", isLeft (verifyCard cfg payload (change card 0x330 "\x00")))
        , ("raw pair-address corruption rejected", isLeft (verifyCard cfg payload (change card 0x34c "\x00")))
        , ("MBR geometry edit cannot silently alter raw configuration", isLeft (verifyCard cfg payload (change card 0x1ce "\xff")))
        , ("truncated raw configuration rejected", isLeft (readRawRom (B.take 1023 card)))
        , ("truncated raw payload rejected", isLeft (readRawRom (B.take 1535 card)))
        , ("missing MBR signature rejected", isLeft (verifyCard cfg payload (change card 510 "\x00")))
        , ("absent partition rejected", isLeft (verifyCard cfg payload (change card 0x1c2 "\x00")))
        , ("invalid volume signature rejected", isLeft (verifyCard cfg payload (change card (2048*512+510) "\x00")))
        , ("missing FAT tag rejected", isLeft (verifyCard cfg payload (change card (2048*512+54) "\x00")))
        , ("zero cluster size rejected", isLeft (verifyCard cfg payload (change card (2048*512+13) "\x00")))
        , ("short configuration file rejected", isLeft (verifyCard cfg payload (change card (2305*512+28) (word 4 512))))
        , ("empty FAT payload rejected", isLeft (verifyCard cfg payload (change card (2305*512+60) (word 4 0))))
        , ("directory cannot substitute for configuration file", isLeft (verifyCard cfg payload (change card (2305*512+11) "\x10")))
        , ("ROM rejects configuration cluster-high bits outside this FAT16 volume", isLeft (verifyCard cfg payload (change card (2305*512+20) (word 2 1))))
        , ("ROM rejects payload cluster-high bits outside this FAT16 volume", isLeft (verifyCard cfg payload (change card (2305*512+52) (word 2 1))))
        , ("file exceeding its cluster chain rejected", isLeft (verifyCard cfg payload (change card (2305*512+28) (word 4 4096))))
        , ("cyclic FAT chain rejected", isLeft (verifyCard cfg payload (change (change card (2305*512+28) (word 4 4096)) (2049*512+6) (word 2 2))))
        , ("FAT configuration corruption rejected", isLeft (verifyCard cfg payload (change card (2337*512) "\x00")))
        , ("short FAT payload image rejected", isLeft (verifyCard cfg payload (B.take (2339*512) card)))
        , ("short diagnostic sector refused", isLeft (bootCard cfg (B.take 511 payload)))
        , ("unaligned payload refused", isLeft (bootCard cfg (payload <> "\x00")))
        , ("empty payload refused", isLeft (bootCard cfg B.empty))
        , ("payload overlapping FAT partition refused", isLeft (bootCard cfg (B.replicate (bootPayloadByteCount maxBootPayloadBytes+512) 0)))
        , ("zeroed stock configuration tail refused", isLeft (stockConfig (B.take 0x848 fixture <> B.replicate 2048 0)))
        , ("inconsistent stock configuration copies refused", isLeft (stockConfig (change fixture 0x248 "x")))
        , ("short FAT chain rejected", isLeft (verifyCard cfg payload (change card (2049*512+4) (word 2 0))))
        , ("recovery FAT carries the exact restore DAT", readRomFile recovery recoveryVolume RestoreContainer == Right restore)
        , ("recovery FAT carries the mode-1 payload", readRomFile recovery recoveryVolume BootPayload == Right payload)
        , ("recovery raw route carries only the guarded payload", fmap mode2PayloadBytes (readRawRom recovery) == Right payload)
        , ("recovery report sectors start empty", all (B.all (== 0)) initialReports)
        , ("manual-compatible recovery volume is FAT32", fatKind recovery32Volume == Fat32)
        , ("FAT32 recovery carries the exact restore DAT", readRomFile recovery32 recovery32Volume RestoreContainer == Right restore)
        , ("FAT32 recovery carries the mode-1 payload", readRomFile recovery32 recovery32Volume BootPayload == Right payload)
        , ("FAT32 recovery raw route carries only the guarded payload", fmap mode2PayloadBytes (readRawRom recovery32) == Right payload)
        , ("FAT32 recovery report sectors start empty", all (B.all (== 0)) recovery32Reports)
        , ("firmware carrier exposes all four files byte-for-byte",
            mapM (readRomFile firmwareRecovery firmwareVolume) [SystemConfig,BootPayload,RecoveryStage,RestoreContainer] ==
              Right [config,firmwareBootstrap,firmwareBody,firmwareRestore])
        , ("firmware carrier has real FAT32 data capacity and root cluster two",
            fatKind firmwareVolume == Fat32 && clusterSectors firmwareVolume == 1 &&
              rootLocation firmwareVolume == 2 && clusterLimit firmwareVolume-2 > 65525)
        , ("firmware carrier occupies its bounded partition and report sectors",
            B.length firmwareRecovery == (recoveryReportBaseLba+recoveryReportSectorCount)*512 &&
              B.length firmwareRecovery <= 128*1024*1024)
        , ("firmware raw bootstrap transport stages exactly eight sectors",
            mode2PayloadBytes firmwareRaw == firmwareBootstrap &&
              word32At (mode2ConfigBytes firmwareRaw) 0x18 == 8 &&
              rawConfigDifferences cfg firmwareRaw ==
                [ ConfigDifference (ConfigWordOffset 0x18) 2 8
                , ConfigDifference (ConfigWordOffset 0x1c0) 0 0x00040000
                , ConfigDifference (ConfigWordOffset 0x1c4) 0 0x08000000
                , ConfigDifference (ConfigWordOffset 0x1fc) 0x01000101 0xaa550101
                ])
        , ("firmware metadata addresses the complete contiguous raw body",
            B.take (B.length firmwareBody) (B.drop (firmwareBodyLba*512) firmwareRecovery) == firmwareBody)
        , ("firmware verifier rejects raw bootstrap changes",
            isLeft (verifyFirmware (change firmwareRecovery 1024 "x")))
        , ("firmware verifier rejects body transport changes",
            isLeft (verifyFirmware (change firmwareRecovery (firmwareBodyLba*512) "x")))
        , ("firmware verifier rejects DAT changes",
            isLeft (verifyFirmware (change firmwareRecovery ((firmwareBodyLba*512)+B.length firmwareBody) "x")))
        , ("firmware verifier rejects missing report capacity",
            isLeft (verifyFirmware (B.take (B.length firmwareRecovery-512) firmwareRecovery)))
        , ("firmware verifier rejects excess card capacity",
            isLeft (verifyFirmware (firmwareRecovery <> B.replicate 512 0)))
        , ("firmware verifier rejects noncontiguous body chains",
            isLeft (verifyFirmware (change firmwareRecovery (firmwareFatOffset+firmwareBodyCluster*4) (word 4 (firmwareBodyCluster+2)))))
        , ("firmware ROM reader rejects cyclic root chains",
            isLeft (readRomFile (change firmwareRecovery (firmwareFatOffset+8) (word 4 2)) firmwareVolume BootPayload))
        , ("firmware ROM reader rejects roots beyond the data region",
            isLeft (readRomFile (change firmwareRecovery (firmwareFatOffset+8) (word 4 (clusterLimit firmwareVolume))) firmwareVolume BootPayload))
        , ("firmware ROM reader rejects file clusters beyond the data region",
            isLeft (readRomFile (change (change firmwareRecovery (firmwareRootOffset+3*32+26)
              (word 2 (clusterLimit firmwareVolume))) (firmwareRootOffset+3*32+20)
              (word 2 (clusterLimit firmwareVolume `shiftR` 16))) firmwareVolume RecoveryStage))
        , ("firmware ROM mount rejects out-of-bounds FAT32 root",
            isLeft (mountRom (change firmwareRecovery (2048*512+44) (word 4 (clusterLimit firmwareVolume)))))
        , ("firmware carrier refuses wrong bootstrap lengths",
            isLeft (firmwareRecoveryCardFat32 cfg payload firmwareBody firmwareRestore) &&
              isLeft (firmwareRecoveryBodyLbaFat32 payload))
        , ("firmware carrier refuses empty and unaligned bodies",
            isLeft (firmwareRecoveryCardFat32 cfg firmwareBootstrap B.empty firmwareRestore) &&
              isLeft (firmwareRecoveryCardFat32 cfg firmwareBootstrap (firmwareBody <> "x") firmwareRestore))
        , ("firmware carrier refuses empty restore containers",
            isLeft (firmwareRecoveryCardFat32 cfg firmwareBootstrap firmwareBody B.empty))
        , ("firmware carrier refuses files beyond actual data capacity",
            isLeft (firmwareRecoveryCardFat32 cfg firmwareBootstrap firmwareBody (B.replicate (64*1024*1024) 0)))
        , ("staged FAT route exposes the one-sector entry", readRomFile stagedRecovery stagedVolume BootPayload == Right payload)
        , ("staged FAT route exposes the complete recovery body", readRomFile stagedRecovery stagedVolume RecoveryStage == Right stagedBody)
        , ("staged raw ROM load stops after the one-sector entry", mode2PayloadBytes stagedRaw == payload)
        , ("staged verifier rejects a changed raw recovery body", isLeft (verifyStagedRecoveryCard cfg payload stagedBody (change stagedRecovery (3*512) "x")))
        , ("staged verifier rejects a changed FAT recovery body", isLeft (verifyStagedRecoveryCard cfg payload stagedBody (change stagedRecovery (2340*512) "x")))
        , ("persistent FAT route carries stage then exact repair block",
            readRomFile persistentRecovery persistentVolume RecoveryStage == Right (stagedBody <> repairBlock))
        , ("persistent raw ROM load stops after the one-sector entry", mode2PayloadBytes persistentRaw == payload)
        , ("persistent verifier rejects a changed raw repair block",
            isLeft (verifyStagedRepairCard cfg payload stagedBody repairBlock (change persistentRecovery (3*512+0x40000) "x")))
        , ("persistent verifier rejects a changed FAT repair block",
            isLeft (verifyStagedRepairCard cfg payload stagedBody repairBlock (change persistentRecovery (2852*512) "x")))
        , ("stage report identifies the last completed stage",
            decodedReports == [StageAbsent 0,StageAbsent 1,StageValid (StageRecord 2 0xeb34a9c9 1 1 1 0x00030000 7 0 0),StageAbsent 3])
        , ("torn stage report is invalid",
            case corruptedReports !! 2 of StageInvalid 2 _ -> True; _ -> False)
        ]
  let allCases = cases ++ extentCases
  forM_ allCases $ \(name,ok) -> unless ok (ioError (userError ("assertion failed: " ++ name)))
  putStrLn (show (length allCases) ++ " behavioral checks passed: synthetic config, built payload, modeled FAT and raw ROM loads")
