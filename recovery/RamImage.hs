{-# LANGUAGE BangPatterns #-}
module RamImage
  ( DecoderExecutable (..), NativeBootstrapFile (..), RecoveryIntent (..), buildRamImage
  ) where

import BootCard
import Control.Exception (bracket)
import Control.Monad (forM, forM_)
import Data.Bits ((.&.), (.|.), complement, shiftR, xor)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.List (isPrefixOf, nub, sortOn)
import Data.Word (Word32)
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import RamBootstrapPin (BootstrapDigest (..), expectedBootstrapDigest)
import RecoveryFirmware
import qualified UpdaterRoute as Updater
import System.Directory (canonicalizePath, createDirectory, removeDirectoryRecursive)
import System.Exit (ExitCode (..))
import System.FilePath ((</>), addTrailingPathSeparator, takeDirectory, takeFileName)

newtype DecoderExecutable = DecoderExecutable FilePath
newtype NativeBootstrapFile = NativeBootstrapFile FilePath
newtype RegionBytes = RegionBytes Int deriving (Eq, Show)
newtype InstructionOffset = InstructionOffset Int deriving (Eq, Show)
newtype Crc32 = Crc32 Word32
data RecoveryIntent = BootOnly | NativeUpdater deriving (Eq)

data Region = CopyRegion RamAddress B.ByteString | ZeroRegion RamAddress RegionBytes
data LiveTable = LiveTable B.ByteString RamAddress RegionBytes
data RegionLayout = MeaningfulBytes | SectorPaddedBytes
data PlainCompletionEvent = Event51 | Event52
data WordPatch = WordPatch FileOffset Word32
data BranchKind = Call | Jump | JumpNotEqual

branchOpcode :: BranchKind -> Word32
branchOpcode Call = 0xeb000000
branchOpcode Jump = 0xea000000
branchOpcode JumpNotEqual = 0x1a000000

eventInstruction :: PlainCompletionEvent -> Word32
eventInstruction Event51 = 0xe3a00033
eventInstruction Event52 = 0xe3a00034

checked :: Either String a -> IO a
checked = either H.failH pure

mediaSlice :: Firmware -> FileOffset -> ByteCount -> IO B.ByteString
mediaSlice firmware offset count = B.map complement <$> slice (firmwareMain firmware) offset count

replaceWords :: [WordPatch] -> B.ByteString -> B.ByteString
replaceWords patches bytes = B.concat (pieces 0 (sortOn position patches))
  where
    position (WordPatch (FileOffset offset) _) = offset
    pieces cursor [] = [B.drop cursor bytes]
    pieces cursor (WordPatch (FileOffset offset) value:rest) =
      B.take (offset-cursor) (B.drop cursor bytes) : U.putWord32 value : pieces (offset+4) rest

crc32 :: B.ByteString -> Crc32
crc32 = Crc32 . complement . B.foldl' byte 0xffffffff
  where
    byte !acc value =
      let index = fromIntegral ((acc `xor` fromIntegral value) .&. 0xff)
      in (acc `shiftR` 8) `xor` U.word32At crcTable (4*index)

crcTable :: B.ByteString
crcTable = B.concat [U.putWord32 (bits 8 value) | value <- [0..255]]
  where
    bits :: Int -> Word32 -> Word32
    bits 0 !value = value
    bits n !value = bits (n-1) ((value `shiftR` 1) `xor` if value .&. 1 == 0 then 0 else 0xedb88320)

segmentEntry :: Firmware -> SegmentKind -> IO U.SegmentEntry
segmentEntry firmware kind = case [entry | Segment candidate entry <- firmwareSegments firmware, candidate == kind] of
  [entry] -> pure entry
  _ -> H.failH "Missing or ambiguous firmware segment"

validateMediaLayout :: Firmware -> IO ()
validateMediaLayout firmware = do
  let ranges = (0,0x80000) : (0x140000,0x400) : [(fromIntegral (U.segFlash e),fromIntegral (U.segFlashLen e)) | Segment _ e <- firmwareSegments firmware]
      ordered = sortOn fst ranges :: [(Integer,Integer)]
  H.ensure (all (\(a,n) -> n > 0 && a+n <= fromIntegral (B.length (firmwareMain firmware))) ordered) "Empty or unbounded firmware media range"
  H.ensure (and [a+n <= b | ((a,n),(b,_)) <- zip ordered (drop 1 ordered)]) "Overlapping firmware media components"

nativeImage :: Firmware -> IO B.ByteString
nativeImage firmware = do
  bytes <- mediaSlice firmware (FileOffset 0x40000) (ByteCount 0x40000)
  digest <- H.sha256 bytes
  H.ensure (digest == "c1c0255e6ac971a26151a083cf38f45061a8c621904fcfdf5aef61a5f0567b6f") "Firmware native records are not the bound X-M1 execution family"
  H.ensure (U.word32At bytes 0x20 == 0x1800) "Native reset literal changed"
  let patches = [(0x1d4c,0xeb000172,0xe3a00000),(0x1d58,0xeb00016f,0xe3a00000),(0x1d5c,0xeb00013b,0xe3a00000),(0x1f5c,0xebffff98,0xe320f000),(0x1f7c,0xebffff4f,0xe3a00000)]
  forM_ patches $ \(offset,expected,_) -> H.ensure (U.word32At bytes offset == expected) "Native preload patch instruction changed"
  pure (replaceWords [WordPatch (FileOffset offset) value | (offset,_,value) <- patches] bytes)

liveTable :: Firmware -> IO LiveTable
liveTable firmware = do
  table <- mediaSlice firmware (FileOffset 0x140000) (ByteCount 0x400)
  let word = U.word32At table
      pointer offset = do
        let address = word offset
        H.ensure (address >= 0x4061c010 && address < 0x4061c400 && address `mod` 4 == 0) "Section-table pointer is outside its bound table"
        pure (fromIntegral (address-0x4061c000))
  entries <- pointer 0
  extentDescriptor <- pointer 4
  publication <- pointer 12
  H.ensure (entries == 0x40 && word 8 == 0) "Unrecognized native section-table header"
  H.ensure (word publication == 0x1234) "Unrecognized native publication structure"
  _ <- slice table (FileOffset publication) (ByteCount 32)
  _ <- slice table (FileOffset extentDescriptor) (ByteCount 12)
  extents <- pointer extentDescriptor
  let destination = word (extentDescriptor+4)
      plainLength = word (extentDescriptor+8)
      walk offset acc = do
        H.ensure (offset+4 <= B.length table) "Unterminated plain-image extent list"
        if word offset == 0xffffffff then pure (reverse acc,offset+4) else do
          _ <- slice table (FileOffset offset) (ByteCount 24)
          let ident = word offset
              base = word (offset+4)
              count = word (offset+8)
          H.ensure (count > 0 && base >= destination && toInteger base+toInteger count <= toInteger destination+toInteger plainLength) "Plain-image extent is outside its declared window"
          H.ensure (word (offset+12) == 0 && word (offset+16) == 0) "Unrecognized or already-published plain-image extent"
          walk (offset+24) ((ident,base,count):acc)
  (runs,extentEnd) <- walk extents []
  H.ensure (not (null runs) && length (nub [ident | (ident,_,_) <- runs]) == length runs) "Empty or ambiguous plain-image extent list"
  let sortedRuns = sortOn (\(_,base,_) -> base) runs
  H.ensure (and [toInteger a+toInteger n <= toInteger b | ((_,a,n),(_,b,_)) <- zip sortedRuns (drop 1 sortedRuns)]) "Overlapping plain-image extents"
  let structures = sortOn fst [(0,16),(entries,8*36+4),(publication,32),(extentDescriptor,12),(extents,extentEnd-extents)]
      end = maximum [a+n | (a,n) <- structures]
      used index = any (\(a,n) -> index >= a && index < a+n) structures
  H.ensure (and [a+n <= b | ((a,n),(b,_)) <- zip structures (drop 1 structures)]) "Overlapping section-table structures"
  H.ensure (all (\index -> used index || B.index table index == 0) [0..B.length table-1]) "Unbound nonzero section-table fields"
  pure (LiveTable (replaceWords [WordPatch (FileOffset 8) 1] (B.take end table)) (RamAddress destination) (RegionBytes (fromIntegral plainLength)))

rawComponent :: Firmware -> SegmentKind -> IO (RamAddress,B.ByteString)
rawComponent firmware kind = do
  entry <- segmentEntry firmware kind
  H.ensure (U.segSize entry > 0 && U.segSize entry <= U.segFlashLen entry && U.segFlag entry == 0 && U.segBss entry == 0 && U.segBssLen entry == 0) "Unrecognized raw component declaration"
  H.ensure (U.segFlashLen entry `mod` 0x20000 == 0) "Raw component has a partial NAND-block window"
  bytes <- mediaSlice firmware (FileOffset (fromIntegral (U.segFlash entry))) (ByteCount (fromIntegral (U.segFlashLen entry)))
  if kind `elem` [PrimaryKernel,CompanionKernel] then do
    H.ensure (B.length bytes >= 32) "Truncated kernel header"
    H.ensure (map (U.word32At bytes) [0,4,8,12,16,20,24,28] == [0,U.segLoad entry,U.segSize entry,U.segLoad entry+32,0x40000000 .|. (U.segLoad entry+U.segSize entry),0,0,0xaaaaffff]) "Kernel header disagrees with its section-table declaration"
    H.ensure (toInteger (U.segLoad entry)+toInteger (U.segSize entry) < 0x2000000) "Kernel address overflow"
  else pure ()
  pure (RamAddress (U.segLoad entry),bytes)

decodeComponent :: DecoderExecutable -> FilePath -> Firmware -> SegmentKind -> IO (RamAddress,B.ByteString)
decodeComponent (DecoderExecutable decoder) work firmware kind = do
  entry <- segmentEntry firmware kind
  H.ensure (U.segFlag entry == 1 && U.segWord7 entry == 0x20) "Unrecognized compressed component declaration"
  H.ensure (U.segFlashLen entry >= 20) "Truncated compressed frame"
  bytes <- mediaSlice firmware (FileOffset (fromIntegral (U.segFlash entry))) (ByteCount 20)
  let outputSize = U.word32At bytes 0
      total = U.word32At bytes 4
      stem = "segment-" ++ show (fromEnum kind)
      input = work </> stem ++ ".frame"
      output = work </> stem ++ ".decoded"
  H.ensure (outputSize > 0 && outputSize <= 16*1024*1024 && total >= 20 && total <= U.segFlashLen entry && total <= 8*1024*1024 && U.word32At bytes 8 == (total+0x3fff) `div` 0x4000 && U.word32At bytes 12 == 0x4000 && U.word32At bytes 16 == 4) "Invalid compressed frame header"
  frame <- mediaSlice firmware (FileOffset (fromIntegral (U.segFlash entry))) (ByteCount (fromIntegral total))
  B.writeFile input frame
  (limited,code,_,err) <- H.runBounded 30000000 65536 decoder [input,"0",output,show outputSize] B.empty
  H.ensure (not limited && code == ExitSuccess) ("Strict component decode refused: " ++ BC.unpack err)
  decoded <- H.readRegular (fromIntegral outputSize) output
  H.ensure (B.length decoded == fromIntegral outputSize) "Strict decode output length mismatch"
  pure (RamAddress (U.segLoad entry),decoded)

branchTarget :: B.ByteString -> InstructionOffset -> BranchKind -> IO InstructionOffset
branchTarget bytes (InstructionOffset offset) kind = do
  let opcode = branchOpcode kind
  H.ensure (offset >= 0 && offset+4 <= B.length bytes) "Branch instruction is outside application"
  let instruction = U.word32At bytes offset
      immediate = toInteger (instruction .&. 0xffffff)
      signed = if immediate >= 0x800000 then immediate-0x1000000 else immediate
      target = toInteger offset+8+4*signed
  H.ensure (instruction .&. 0xff000000 == opcode && target >= 0 && target+4 <= toInteger (B.length bytes) && target `mod` 4 == 0) "Unbound application branch target"
  pure (InstructionOffset (fromInteger target))

matches :: B.ByteString -> Int -> [Maybe Word32] -> Bool
matches bytes offset wanted = offset >= 0 && offset+4*length wanted <= B.length bytes && and [maybe True (== U.word32At bytes (offset+4*i)) value | (i,value) <- zip [0..] wanted]

uniquePattern :: B.ByteString -> [Maybe Word32] -> IO InstructionOffset
uniquePattern bytes patternWords = scan 0 Nothing
  where
    end = B.length bytes-4*length patternWords
    scan !offset found
      | offset > end = maybe (H.failH "Missing application loader structure") pure found
      | matches bytes offset patternWords = case found of
          Nothing -> scan (offset+4) (Just (InstructionOffset offset))
          Just _ -> H.failH "Ambiguous application loader structure"
      | otherwise = scan (offset+4) found

exact :: [Word32] -> [Maybe Word32]
exact = map Just

checkGetter :: B.ByteString -> InstructionOffset -> IO ()
checkGetter bytes (InstructionOffset offset) = do
  H.ensure (matches bytes offset (exact [0xe92d4800,0xe28db004] ++ [Nothing] ++ exact [0xe1a03000,0xe5933008,0xe1a00003,0xe8bd8800])) "Unrecognized preloaded-flag getter"
  InstructionOffset table <- branchTarget bytes (InstructionOffset (offset+8)) Call
  H.ensure (matches bytes table (exact [0xe52db004,0xe28db000,0xe30c3000,0xe3403061,0xe1a00003,0xe28bd000,0xe8bd0800,0xe12fff1e])) "Unbound application section-table getter"

patchApplication :: RamAddress -> B.ByteString -> IO B.ByteString
patchApplication (RamAddress loadAddress) bytes = do
  InstructionOffset loader5 <- uniquePattern bytes (exact [0xe92d49f0,0xe28db018,0xe24dd00c,0xe3a00005])
  InstructionOffset loader6 <- uniquePattern bytes (exact [0xe92d48f0,0xe28db014,0xe24dd008,0xe3a00006])
  InstructionOffset path <- uniquePattern bytes (exact [0xe3a03001,0xe58d3000,0xe1a00007,0xe1a01008,0xe3a02000,0xe34423e4,0xe3a03000] ++ [Nothing] ++ exact [0xe1a04000])
  H.ensure (path > loader5 && path < loader6 && loader6-loader5 <= 0x400) "Image-5 source-read path is outside the bound loader"
  let candidates = [off | off <- [loader5+16,loader5+20..path-12], matches bytes (off+4) [Just 0xe3500000], U.word32At bytes off .&. 0xff000000 == 0xeb000000, U.word32At bytes (off+8) .&. 0xff000000 == 0x1a000000]
  incoming <- forM candidates $ \off -> do
    target <- branchTarget bytes (InstructionOffset (off+8)) JumpNotEqual
    pure (off,target)
  flagCall <- case [off | (off,InstructionOffset target) <- incoming,target == path] of
    [off] -> pure off
    _ -> H.failH "Image-5 preload branch is missing or ambiguous"
  getter <- branchTarget bytes (InstructionOffset flagCall) Call
  checkGetter bytes getter
  sourceReader <- branchTarget bytes (InstructionOffset (path+28)) Call
  segmentGetter <- branchTarget bytes (InstructionOffset (loader5+16)) Call
  resourcePatches <- forM [0xe3a00002,0xe3a00003] $ \identifierInstruction -> do
    InstructionOffset resource <- uniquePattern bytes (exact [0xe92d4800,0xe28db004,0xe24dd008,identifierInstruction] ++ [Nothing] ++ exact [0xe2503000,0x1a000000,0xeafffffe,0xe5930014,0xe5932018,0xe30f1fff,0xe3401001,0xe0821001,0xe5932004,0xe3a03001,0xe58d3000,0xe1a008a0,0xe1a018a1,0xe3a03000] ++ [Nothing] ++ exact [0xe24bd004,0xe8bd8800])
    resourceGetter <- branchTarget bytes (InstructionOffset (resource+16)) Call
    resourceReader <- branchTarget bytes (InstructionOffset (resource+76)) Call
    H.ensure (resourceGetter == segmentGetter && resourceReader == sourceReader) "Resource loader is not bound to the application descriptor/source reader"
    pure (WordPatch (FileOffset (resource+76)) 0xe3a00000)
  H.ensure (matches bytes (loader6+16) ([Nothing] ++ exact [0xe1a05000] ++ [Nothing] ++ exact [0xe1a04000,0xe3550000,0x1a000000,0xeafffffe] ++ [Nothing] ++ exact [0xe3500000] ++ [Nothing])) "Unrecognized image-6 loader prefix"
  plainGetter <- branchTarget bytes (InstructionOffset (loader6+44)) Call
  H.ensure (plainGetter == getter) "Image-6 loader uses a different preload flag"
  descriptorGetter <- branchTarget bytes (InstructionOffset (loader6+24)) Call
  let InstructionOffset descriptorOffset = descriptorGetter
  H.ensure (matches bytes descriptorOffset (exact [0xe92d4800,0xe28db004] ++ [Nothing] ++ exact [0xe1a03000,0xe5933004,0xe1a00003,0xe8bd8800])) "Unrecognized plain-image descriptor getter"
  descriptorTable <- branchTarget bytes (InstructionOffset (descriptorOffset+8)) Call
  let InstructionOffset flagOffset = getter
  flagTable <- branchTarget bytes (InstructionOffset (flagOffset+8)) Call
  H.ensure (descriptorTable == flagTable) "Plain-image descriptor is not from the bound section table"
  InstructionOffset preserved <- branchTarget bytes (InstructionOffset (loader6+52)) JumpNotEqual
  H.ensure (preserved == loader6+116) "Image-6 preload branch does not preserve the plain image"
  _ <- slice bytes (FileOffset preserved) (ByteCount 128)
  event <- case U.word32At bytes (preserved+8*4) of
    0xe3a00033 -> pure Event51
    0xe3a00034 -> pure Event52
    _ -> H.failH "Unrecognized plain-image completion event"
  let progress = eventInstruction event
      plainPath = exact [0xe5953018,0xe30f6fff,0xe3406001,0xe0836006,0xe1a068a6,0xe3a00024,0xe3e01000] ++ [Nothing] ++ exact [progress,0xe3e01000] ++ [Nothing] ++ exact [0xe3560000,0xda000008,0xe3a04000,0xe3a0700e,0xe1a00007] ++ [Nothing] ++ exact [0xe2844001,0xe1a00884] ++ [Nothing] ++ exact [0xe1560004,0x1afffff8,progress] ++ [Nothing] ++ exact [0xe3a00024] ++ [Nothing] ++ exact [0xe3a04000,0xe5950018] ++ [Nothing] ++ exact [0xe1a00004,0xe24bd014,0xe8bd88f0]
  H.ensure (matches bytes preserved plainPath) "Unrecognized image-6 completion path"
  H.ensure (matches bytes (loader6+56) (exact [0xe5950014,0xe5953018,0xe30f1fff,0xe3401001,0xe0831001,0xe5942004,0xe3a03001,0xe58d3000,0xe1a008a0,0xe1a018a1] ++ [Nothing,Nothing,Nothing] ++ exact [0xe1a04000] ++ [Nothing])) "Unrecognized image-6 source-read path"
  plainReader <- branchTarget bytes (InstructionOffset (loader6+104)) Call
  H.ensure (plainReader == sourceReader) "Image-5 bypass does not target the bound plain-image source reader"
  InstructionOffset plainJoin <- branchTarget bytes (InstructionOffset (loader6+112)) Jump
  H.ensure (plainJoin == preserved+27*4) "Image-6 source-read path does not join its completion path"
  completion <- branchTarget bytes (InstructionOffset (preserved+28*4)) Call
  loopCompletion <- branchTarget bytes (InstructionOffset (preserved+19*4)) Call
  H.ensure (loopCompletion == completion) "Image-6 completion callbacks disagree"
  let movw = U.word32At bytes (loader6+96)
      movt = U.word32At bytes (loader6+100)
      immediate word = (word .&. 0xfff) .|. ((word `shiftR` 4) .&. 0xf000)
      literal = toInteger (immediate movw)+0x10000*toInteger (immediate movt)
      InstructionOffset callback = completion
  H.ensure (movw .&. 0xfff0f000 == 0xe3003000 && movt .&. 0xfff0f000 == 0xe3403000 && literal == toInteger loadAddress+toInteger callback) "Application load address disagrees with its linked completion callback"
  H.ensure (toInteger loadAddress <= 0x61c40c && 0x61c410 <= toInteger loadAddress+toInteger (B.length bytes)) "Native application handoff word is not loaded"
  H.ensure (matches bytes (loader6-12) (exact [0xe1a00004,0xe24bd018,0xe8bd89f0])) "Image-5 loader does not end before image-6 loader"
  forM_ [7,10,16,19,23,25,28] $ \index -> do
    _ <- branchTarget bytes (InstructionOffset (preserved+index*4)) Call
    pure ()
  pure (replaceWords (WordPatch (FileOffset (path+28)) 0xe3a00000 : resourcePatches) bytes)

copyRegion :: RamAddress -> B.ByteString -> Region
copyRegion (RamAddress destination) bytes =
  let leading = fromIntegral (destination `mod` 512)
      total = leading+B.length bytes
      trailing = (512-total `mod` 512) `mod` 512
  in CopyRegion (RamAddress (destination-fromIntegral leading)) (B.concat [B.replicate leading 0,bytes,B.replicate trailing 0])

regionRange :: Region -> (Integer,Integer)
regionRange region = case region of
  CopyRegion (RamAddress address) bytes -> (toInteger address,toInteger (B.length bytes))
  ZeroRegion (RamAddress address) (RegionBytes count) -> (toInteger address,toInteger count)

validateRegions :: RegionLayout -> [Region] -> IO ()
validateRegions layout regions = do
  H.ensure (not (null regions) && length regions <= 16) "RAM rescue descriptor count exceeds protocol"
  forM_ regions $ \region -> do
    let (address,count) = regionRange region
        alignment = case (layout,region) of
          (_,ZeroRegion {}) -> 4
          (SectorPaddedBytes,CopyRegion {}) -> 512
          (MeaningfulBytes,CopyRegion {}) -> 1
    H.ensure (address >= 0 && count > 0 && count `mod` alignment == 0 && address+count <= 0x2000000 && (address+count <= 0xc0000 || address >= 0xc2000)) "RAM rescue descriptor exceeds RAM or overlaps bootstrap relocation"
    case region of ZeroRegion {} -> H.ensure (address `mod` 4 == 0) "Unaligned BSS destination"; CopyRegion {} -> pure ()
  let ordered = sortOn fst (map regionRange regions)
  H.ensure (and [a+n <= b | ((a,n),(b,_)) <- zip ordered (drop 1 ordered)]) "RAM rescue destination regions overlap"

normalizeRegions :: [Region] -> IO [Region]
normalizeRegions original = do
  validateRegions MeaningfulBytes original
  let copies = [copyRegion address bytes | CopyRegion address bytes <- original]
      copyRanges = map regionRange copies
      trim pieces (copyStart,copyCount) = concatMap (subtractRange copyStart (copyStart+copyCount)) pieces
      subtractRange copyStart copyEnd (start,count)
        | copyEnd <= start || copyStart >= start+count = [(start,count)]
        | otherwise = [(a,b-a) | (a,b) <- [(start,min copyStart (start+count)),(max copyEnd start,start+count)], a < b]
      zeroRanges = concat [foldl trim [regionRange region] copyRanges | region@ZeroRegion {} <- original]
      zeros = [ZeroRegion (RamAddress (fromInteger address)) (RegionBytes (fromInteger count)) | (address,count) <- zeroRanges]
      normalized = copies ++ zeros
  validateRegions SectorPaddedBytes normalized
  pure normalized

metadata :: CardSector -> [Region] -> B.ByteString
metadata (CardSector firstLba) regions = replaceWords [WordPatch (FileOffset 12) checksum] sector
  where
    bodyCount = sum [B.length bytes | CopyRegion _ bytes <- regions]
    (_,descriptors) = foldl descriptor (firstLba,[]) regions
    descriptor (lba,acc) region = case region of
      CopyRegion (RamAddress destination) bytes ->
        let Crc32 crc = crc32 bytes
        in (lba+B.length bytes `div` 512,acc ++ [[1,0x40000000 .|. destination,fromIntegral (B.length bytes),fromIntegral lba,crc]])
      ZeroRegion (RamAddress destination) (RegionBytes count) -> (lba,acc ++ [[2,0x40000000 .|. destination,fromIntegral count,0,0]])
    wordsOut = [0x31524d58,1,fromIntegral (length regions),0,fromIntegral firstLba,fromIntegral bodyCount,0,0] ++ concat descriptors
    encoded = B.concat (map U.putWord32 wordsOut)
    sector = encoded <> B.replicate (512-B.length encoded) 0
    Crc32 checksum = crc32 sector

privateOutput :: FilePath -> IO FilePath
privateOutput given = do
  output <- resolve given
  root <- resolve "private-data" >>= canonicalizePath
  parent <- canonicalizePath (takeDirectory output)
  H.ensure (takeFileName output `notElem` ["", ".", ".."] && addTrailingPathSeparator root `isPrefixOf` addTrailingPathSeparator parent) "Output must be a new directory beneath private-data"
  pure output

buildRamImage :: RecoveryIntent -> DecoderExecutable -> NativeBootstrapFile -> FilePath -> FilePath -> IO ()
buildRamImage intent decoder (NativeBootstrapFile bootstrapPath) input givenOutput = do
  firmware <- readFirmware input
  H.ensure (firmwareIdentity firmware == IdentityDigest "346c64fd9cacc439e930d9ad58bb3d699b74633e1c7cdd053c6e2cf0f89c08b1") "Firmware identity is not the bound X-M1 identity record"
  validateMediaLayout firmware
  native <- nativeImage firmware
  configSource <- mediaSlice firmware (FileOffset 0) (ByteCount 0x1000)
  config <- checked (stockConfig (B.replicate 0x48 0 <> configSource))
  LiveTable table plainAddress (RegionBytes plainCount) <- liveTable firmware
  plainEntry <- segmentEntry firmware PlainImage
  H.ensure (U.segFlag plainEntry == 0 && U.segFlashLen plainEntry == fromIntegral plainCount && U.segFlash plainEntry `mod` 0x20000 == 0 && plainCount > 0 && plainCount `mod` 0x20000 == 0) "Plain-image extent descriptor disagrees with segment-6 window"
  plain <- mediaSlice firmware (FileOffset (fromIntegral (U.segFlash plainEntry))) (ByteCount plainCount)
  raw <- forM [PrimaryKernel,CompanionKernel,Resource2,Resource3] $ \kind -> do
    (address,bytes) <- rawComponent firmware kind
    pure (CopyRegion address bytes)
  bootstrap <- H.readRegular 4096 bootstrapPath
  H.ensure (B.length bootstrap == 4096 && B.all (== 0) (B.drop 0xa00 bootstrap) && B.all (== 0) (B.take 512 (B.drop 0x800 bootstrap))) "Linked RAM bootstrap layout differs"
  bootstrapDigest <- H.sha256 bootstrap
  let BootstrapDigest expected = expectedBootstrapDigest
  H.ensure (bootstrapDigest == expected) "RAM bootstrap is not the current linked asset"
  output <- privateOutput givenOutput
  createDirectory output
  let work = output </> "work"
  bracket (createDirectory work) (const (removeDirectoryRecursive work)) $ \_ -> do
    (address4,decoded4) <- decodeComponent decoder work firmware Application4
    (address5,decoded5) <- decodeComponent decoder work firmware Application5
    (runtime4,runtime5) <- case intent of
      BootOnly -> pure (decoded4,decoded5)
      NativeUpdater -> do
        binding <- Updater.bindUpdaterRoute firmware (address4,decoded4) (address5,decoded5)
        putStrLn ("Ordinary updater startup bound at " ++ show (Updater.updaterBootPollerAddress binding))
        putStrLn ("Body version policy: " ++ show (Updater.updaterVersionPolicy binding))
        pure (Updater.updaterImage4 binding,Updater.updaterImage5 binding)
    patched4 <- patchApplication address4 runtime4
    entry4 <- segmentEntry firmware Application4
    entry5 <- segmentEntry firmware Application5
    let RamAddress load4 = address4
        RamAddress load5 = address5
        RamAddress plainLoad = plainAddress
    H.ensure (load4 `mod` 4 == 0 && load5 `mod` 4 == 0 && toInteger load4+toInteger (B.length decoded4) == toInteger (U.segBss entry4) && toInteger (U.segBss entry4)+toInteger (U.segBssLen entry4) == toInteger load5) "Application-4 decoded/BSS boundary disagrees with application-5 load"
    H.ensure (toInteger plainLoad+toInteger plainCount == toInteger (U.segBss entry5)) "Application-5 BSS does not follow the plain-image window"
    bss <- forM [Application4,Application5] $ \kind -> do
      entry <- segmentEntry firmware kind
      H.ensure (U.segBssLen entry > 0) "Missing application BSS declaration"
      pure (ZeroRegion (RamAddress (U.segBss entry)) (RegionBytes (fromIntegral (U.segBssLen entry))))
    let original = [CopyRegion (RamAddress 0) native,CopyRegion (RamAddress 0x61c000) table] ++ raw ++ [CopyRegion address4 patched4,CopyRegion address5 runtime5,CopyRegion plainAddress plain] ++ bss
    regions <- normalizeRegions original
    sector <- checked (case intent of BootOnly -> firmwareRecoveryBodyLba bootstrap; NativeUpdater -> firmwareRecoveryBodyLbaFat32 bootstrap)
    let params = metadata sector regions
        entry = B.concat [B.take 0x800 bootstrap,params,B.drop 0xa00 bootstrap]
        body = B.concat [bytes | CopyRegion _ bytes <- regions]
        restore = firmwareBytes firmware
    image <- checked (case intent of BootOnly -> firmwareRecoveryCard config entry body restore; NativeUpdater -> firmwareRecoveryCardFat32 config entry body restore)
    forM_ [("FFSYS.BIN",configBytes config),("FFBOOT.BIN",entry),("FFRECOV.BIN",body),("FWUP0002.DAT",restore),("firmware-ram-recovery-sd.img",image)] $ \(name,bytes) -> do
      B.writeFile (output </> name) bytes
      digest <- H.sha256 bytes
      putStrLn (name ++ ": " ++ show (B.length bytes) ++ " bytes, sha256 " ++ digest)
  putStrLn "Firmware-derived RAM rescue card generated. Restore DAT is unchanged. No camera or card-device I/O was performed."
  putStrLn "RAM handoff does not establish application readiness or authorize an automatic persistent update."
