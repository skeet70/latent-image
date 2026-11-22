{-# LANGUAGE OverloadedStrings #-}

-- | Stock-against-owned cases for the DCF numbering: an independent model of the stock
-- entries in image 4, read from the bytes of [0x70f790,0x70f8b8), [0x70faa0,0x7101e4),
-- [0x71141c,0x7115a4), [0x7159f8,0x715a58) and [0x716184,0x71657c), gives each case's
-- results, the ordered calls into the retained services and the final context, path and
-- output bytes.
module Replacement.DcfNumbering.Cases
  ( code
  , entries
  , scenarios
  ) where

import Data.Bits (countTrailingZeros, shiftL, (.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Char (isAsciiLower, isAsciiUpper, isDigit)
import qualified Data.Map.Strict as M
import Data.Word (Word32, Word8)
import Replacement.FileService.Native

imageBase :: Word32
imageBase = 0x61c400

code :: Code
code = Code
  { codeImageBase = imageBase
  , codeExtents =
      [ ("prefix-mode-and-name-check", Extent 0x70f790 0x70f8b8)
      , ("numbering-and-name", Extent 0x70faa0 0x7101e4)
      , ("written-file", Extent 0x71141c 0x7115a4)
      , ("profile-pair", Extent 0x7159f8 0x715a58)
      , ("folders-and-last-used", Extent 0x716184 0x71657c)
      ]
  , codeStockFunctions =
      [ (0x70f790, 176), (0x70f840, 120), (0x70faa0, 200), (0x70fb68, 148), (0x70fbfc, 1512)
      , (0x71141c, 392), (0x7159f8, 52), (0x715a2c, 44), (0x716184, 416), (0x716324, 600)
      ]
  , codeRetained = []
  , codeRetainedFunctions = []
  }

setPrefixMode, checkName, applyContinuous, readNumbersEntry, prepareName, noteWritten, readProfile, writeProfile, createFolder, lastUsedEntry :: Word32
setPrefixMode = 0x70f790
checkName = 0x70f840
applyContinuous = 0x70faa0
readNumbersEntry = 0x70fb68
prepareName = 0x70fbfc
noteWritten = 0x71141c
readProfile = 0x7159f8
writeProfile = 0x715a2c
createFolder = 0x716184
lastUsedEntry = 0x716324

-- | The owned veneer symbol at each stock entry.
entries :: [(String, Word)]
entries =
  [ ("dcf_entry_" ++ name, fromIntegral address)
  | (name, address) <-
      [ ("set_prefix_mode", setPrefixMode), ("check_name", checkName)
      , ("apply_continuous_numbering", applyContinuous), ("read_numbers", readNumbersEntry)
      , ("prepare_name", prepareName), ("note_written", noteWritten), ("read_profile", readProfile)
      , ("write_profile", writeProfile), ("create_folder", createFolder), ("last_used", lastUsedEntry)
      ]
  ]

-- Retained services ------------------------------------------------------------------

mediaStatusS, currentDriveS, groupIndexS, reportS, drivePrefixS, copyBytesS, formatDecimalS, configurationS :: Word32
mediaStatusS = 0x71ddf0
currentDriveS = 0x71f708
groupIndexS = 0x715f0c
reportS = 0x726c74
drivePrefixS = 0x712d08
copyBytesS = 0x70e6c4
formatDecimalS = 0x70eaa0
configurationS = 0x69f8fc

profileDirectoryS, profileFileS, storeDirectoryS, storeFileS, resetProfileS, snapshotS :: Word32
profileDirectoryS = 0x6a8040
profileFileS = 0x6a801c
storeDirectoryS = 0x6a802c
storeFileS = 0x6a8008
resetProfileS = 0x6a8050
snapshotS = 0x712b4c

fillS, formatNumberS, copyMemoryS, openDirectoryS, entryClusterS, pathGroupsS, translateS :: Word32
fillS = 0x43550
formatNumberS = 0x722218
copyMemoryS = 0x722054
openDirectoryS = 0x701e18
entryClusterS = 0x713460
pathGroupsS = 0x7160b8
translateS = 0x71d450

-- Guest layout -------------------------------------------------------------------------

contextSlot, contextBase, pathBase, textBase, outputBase :: Word32
contextSlot = 0x910204
contextBase = 0xa00000
pathBase = 0xb00000
textBase = 0xb10000
outputBase = 0xb20000

-- | The mapped spans of the media context, relative to its base.
contextSpans :: [(String, Word32, Int)]
contextSpans =
  [ ("context-files", 0x0, 0x200)
  , ("context-catalog", 0x2160, 0x1460)
  , ("context-sequence", 0x127d0, 0x8)
  , ("context-numbering", 0x16bd0, 0xa0)
  ]

-- | Stock-only reads: the folder template behind the literal at 0x716320 and `\DCIM\`.
stockData :: [(String, Word32, Int)]
stockData = [("folder-template", 0x774d44, 20), ("dcim-text", 0x781f60, 8)]

pathBytes, textBytes, outputBytes :: Int
pathBytes = 0x40
textBytes = 0x20
outputBytes = 8

prefixOffset, writtenFiles, maximumDirectory, maximumFile, largestFirstGroup, counters, selectedGroups, directoryGroups, counterCount, directoryClusters :: Word32
prefixOffset = 0x0
writtenFiles = 0x1fc
maximumDirectory = 0x2168
maximumFile = 0x216c
largestFirstGroup = 0x21b8
counters = 0x21d0
selectedGroups = 0x2210
directoryGroups = 0x2218
counterCount = 0x2600
directoryClusters = 0x2604

sequenceCount, sequenceLatch, prefixMode, lastSequence, profileEnabled :: Word32
sequenceCount = 0x127d4
sequenceLatch = 0x16bd0
prefixMode = 0x16bd4
lastSequence = 0x16c64
profileEnabled = 0x16c6c

counterAt :: Word32 -> Word32
counterAt group = contextBase + counters + 16 * group

-- Scripted answers ------------------------------------------------------------------------

data Env = Env
  { envMedia :: Word32
  , envDrive :: Word32
  , envGroups :: Word32 -> Word32
  , envConfiguration :: Word32 -> Word32
  , envProfile :: Word32 -> (Word32, Word32)
  , envOpen :: Word32
  , envCluster :: Word32
  , envPathMask :: Word32
  }

env :: Env
env = Env
  { envMedia = 0
  , envDrive = 0x43
  , envGroups = \mask -> if mask == 0 then 0xff else lowestBit mask
  , envConfiguration = \key -> 0x40 + (key .&. 0x1f)
  , envProfile = const (100, 0)
  , envOpen = 0
  , envCluster = 0x1234
  , envPathMask = 1
  }

lowestBit :: Word32 -> Word32
lowestBit mask = fromIntegral (countTrailingZeros mask)

-- Model ----------------------------------------------------------------------------------------

data World = World {wMemory :: M.Map Word32 Word8, wServices :: [Service]}

newtype S a = S {runS :: Env -> World -> (a, World)}

instance Functor S where
  fmap f (S g) = S (\e w -> let (a, w') = g e w in (f a, w'))

instance Applicative S where
  pure a = S (\_ w -> (a, w))
  S f <*> S g = S (\e w -> let (h, w') = f e w; (a, w'') = g e w' in (h a, w''))

instance Monad S where
  S g >>= k = S (\e w -> let (a, w') = g e w in runS (k a) e w')

ask :: S Env
ask = S (\e w -> (e, w))

made :: Service -> S ()
made service = S (\_ w -> ((), w {wServices = wServices w ++ [service]}))

byteAt :: Word32 -> S Word32
byteAt address = S (\_ w -> (fromIntegral (M.findWithDefault 0 address (wMemory w)), w))

halfAt :: Word32 -> S Word32
halfAt address = (\low high -> low + high `shiftL` 8) <$> byteAt address <*> byteAt (address + 1)

wordAt :: Word32 -> S Word32
wordAt address = (\low high -> low + high `shiftL` 16) <$> halfAt address <*> halfAt (address + 2)

bytesAt :: Word32 -> Word32 -> S B.ByteString
bytesAt address count = B.pack . map fromIntegral <$> mapM byteAt [address .. address + count - 1]

store :: Word32 -> B.ByteString -> S ()
store address bytes =
  S (\_ w -> ((), w {wMemory = foldr (uncurry M.insert) (wMemory w) (zip [address ..] (B.unpack bytes))}))

setByte, setHalf, setWord :: Word32 -> Word32 -> S ()
setByte address value = store address (B.singleton (fromIntegral value))
setHalf address value = store address (B.take 2 (word32le value))
setWord address value = store address (word32le value)

decimal :: Word32 -> Word32 -> B.ByteString
decimal digits value = C.pack (reverse (take (fromIntegral digits) (reverse (show value) ++ repeat '0')))

-- | A service returning r0 that reads and writes no memory.
plain :: Word32 -> [(String, Word32)] -> Word32 -> S Word32
plain address inputs result = made (Service address inputs [("r0", result)] [] []) >> pure result

effect :: Word32 -> [(String, Word32)] -> S ()
effect address inputs = made (Service address inputs [] [] [])

mediaStatus :: S Word32
mediaStatus = ask >>= plain mediaStatusS [("r0", 0)] . envMedia

currentDrive :: S Word32
currentDrive = ask >>= plain currentDriveS [] . envDrive

groupIndex :: Word32 -> S Word32
groupIndex mask = ask >>= \e -> plain groupIndexS [("r0", mask)] (envGroups e mask)

reportLimit :: S ()
reportLimit = effect reportS [("r0", 2), ("r1", 0x51)]

drivePrefix :: Word32 -> S ()
drivePrefix path = do
  e <- ask
  let bytes = B.pack [fromIntegral (envDrive e), 0x3a, 0]
  made (Service drivePrefixS [("r0", path)] [] [] [AtRegister "r0" 0 bytes])
  store path bytes

-- | 0x70e6c4 copying @bytes@; the source address is an input unless it is the owned or
-- stock `\DCIM\` text.
copyBytes :: Word32 -> Maybe Word32 -> B.ByteString -> S ()
copyBytes destination source bytes = do
  let count = fromIntegral (B.length bytes)
  made
    ( Service copyBytesS ([("r0", destination)] ++ [("r1", address) | Just address <- [source]] ++ [("r2", count)]) []
        [AtRegister "r1" 0 bytes] [AtRegister "r0" 0 bytes]
    )
  store destination bytes

copyFrom :: Word32 -> Word32 -> Word32 -> S ()
copyFrom destination source count = bytesAt source count >>= copyBytes destination (Just source)

formatDecimal :: Word32 -> Word32 -> Word32 -> S ()
formatDecimal destination value digits = do
  let bytes = decimal digits value
  made (Service formatDecimalS [("r0", destination), ("r1", value), ("r2", digits), ("r3", 1)] [] [] [AtRegister "r0" 0 bytes])
  store destination bytes

configuration :: Word32 -> S Word32
configuration key = ask >>= \e -> plain configurationS [("r0", key)] (envConfiguration e key)

profilePair :: Word32 -> Word32 -> S (Word32, Word32)
profilePair drive group = do
  e <- ask
  let (directory, file) = envProfile e group
  _ <- plain profileDirectoryS [("r0", drive), ("r1", group)] directory
  _ <- plain profileFileS [("r0", drive), ("r1", group)] file
  pure (directory .&. 0xffff, file .&. 0xffff)

storePair :: Word32 -> Word32 -> Word32 -> Word32 -> S ()
storePair drive group directory file = do
  effect storeDirectoryS [("r0", drive), ("r1", group), ("r2", directory), ("r3", file)]
  effect storeFileS [("r0", drive), ("r1", group), ("r2", file)]

-- Entries ------------------------------------------------------------------------------------------

-- | 0x70f790.
modeM :: Word32 -> S ()
modeM mode = case lookup mode [(0, (0, 0)), (1, (2, 1)), (2, (2, 2))] of
  Just (latch, word) -> setByte (contextBase + sequenceLatch) latch >> setWord (contextBase + prefixMode) word
  Nothing -> pure ()

-- | 0x70f840.
checkM :: Word32 -> Word32 -> S Word32
checkM name count = do
  bytes <- bytesAt name count
  pure (if count == 0 || C.all accepted bytes then 0 else 8)
  where
    accepted c = isAsciiUpper c || isAsciiLower c || isDigit c || c == '_'

-- | 0x70faa0.
continuousM :: Word32 -> S ()
continuousM enabled = do
  status <- mediaStatus
  drive <- currentDrive
  if status == 0 && drive /= 0
    then
      if enabled == 1
        then
          let loop group = do
                count <- byteAt (contextBase + counterCount)
                if count > group
                  then do
                    directory <- halfAt (counterAt group + 4)
                    file <- halfAt (counterAt group + 6)
                    storePair drive group directory file
                    loop (group + 1)
                  else pure ()
           in loop 0
        else effect resetProfileS []
    else pure ()
  setWord (contextBase + profileEnabled) enabled

blockEnd, blockStart :: Word32 -> Word32
blockEnd directory = ((directory `mod` 10) * 1000 + 999) .&. 0xffff
blockStart directory = ((directory `mod` 10) * 1000 + 1) .&. 0xffff

-- | 0x716324.
lastUsedM :: Word32 -> Word32 -> S (Word32, Word32)
lastUsedM drive group = do
  directory0 <- halfAt (counterAt group + 4)
  file0 <- halfAt (counterAt group + 6)
  maximum0 <- wordAt (contextBase + maximumDirectory)
  folder100 <- wordAt (contextBase + directoryClusters + 400)
  let (directory1, file1)
        | directory0 > 99 = (directory0, file0)
        | maximum0 <= 100 && folder100 == 0 = (100, 0)
        | otherwise = let d = maximum0 .&. 0xffff in (d, blockEnd d)
  enabled <- wordAt (contextBase + profileEnabled)
  (pd, pf) <-
    if enabled == 0
      then pure (100, 0)
      else do
        (pd, pf) <- profilePair drive group
        if directory1 < pd || (directory1 == pd && pf > file1)
          then do
            owner <- byteAt (contextBase + directoryGroups + pd)
            if owner /= group && owner /= 0xff
              then do
                d <- halfAt (contextBase + maximumDirectory)
                pure (d, blockEnd d)
              else pure (pd, pf)
          else pure (pd, pf)
  let (directory2, file2)
        | directory1 < pd = (pd, pf)
        | directory1 == pd && pf > file1 = (directory1, pf)
        | otherwise = (directory1, file1)
  if group /= 0
    then pure (directory2, file2)
    else do
      maximum1 <- wordAt (contextBase + maximumDirectory)
      largest <- wordAt (contextBase + largestFirstGroup)
      if directory2 < maximum1
        then let d = maximum1 .&. 0xffff in pure (d, blockEnd d)
        else
          if directory2 == maximum1 && directory2 /= largest && (directory2 > 100 || folder100 /= 0)
            then pure (directory2, blockEnd directory2)
            else pure (directory2, file2)

-- | 0x70fb68.
readNumbersM :: S (Either Word32 (Word32, Word32))
readNumbersM = do
  mask <- wordAt (contextBase + selectedGroups)
  status <- mediaStatus
  if status /= 0
    then pure (Left status)
    else do
      drive <- currentDrive
      if drive == 0
        then pure (Left 8)
        else do
          group <- groupIndex mask
          if group == 0xff then pure (Left 8) else Right <$> lastUsedM drive group

folderAfter :: Word32 -> Word32 -> Word32
folderAfter previous maximum0 = (if previous < maximum0 then maximum0 + 1 else previous + 1) .&. 0xffff

restart :: S Word32
restart = setHalf (contextBase + sequenceCount) 1 >> pure 1

exhausted :: Word32 -> S Bool
exhausted sequence0 = (\latch -> latch /= 0 && sequence0 > 99) <$> byteAt (contextBase + sequenceLatch)

newFolder :: Word32 -> S (Maybe Word32)
newFolder previous = do
  next <- folderAfter previous <$> wordAt (contextBase + maximumDirectory)
  if next >= 1000 then reportLimit >> pure Nothing else pure (Just next)

-- | The allocation at 0x70fcb4..0x71003c: folder, file and sequence, or the limit.
allocateM :: (Word32, Word32) -> Word32 -> S (Maybe (Word32, Word32, Word32))
allocateM (previous, last0) sequence0
  | previous `mod` 10 == last0 `div` 1000 = do
      let file = (last0 + 1) .&. 0xffff
      full <- if file `mod` 1000 == 0 then pure True else exhausted sequence0
      if not full
        then pure (Just (previous, file, sequence0))
        else newFolder previous >>= maybe (pure Nothing) (\d -> restart >>= \s -> pure (Just (d, blockStart d, s)))
  | otherwise = do
      let stepped = (last0 + 1) .&. 0xffff
      first <-
        if stepped > 9999
          then newFolder previous >>= maybe (pure Nothing) (\d -> restart >>= \s -> pure (Just (d, 1, s, True)))
          else pure (Just (previous, if stepped `mod` 1000 == 0 then (last0 + 2) .&. 0xffff else stepped, sequence0, False))
      case first of
        Nothing -> pure Nothing
        Just (directory, file, sequence1, wrapped)
          | file `div` 1000 == directory `mod` 10 -> pure (Just (directory, file, sequence1))
          | wrapped || file `div` 1000 > directory `mod` 10 -> do
              start <- folderAfter previous <$> wordAt (contextBase + maximumDirectory)
              let block = (file `mod` 10000) `div` 1000
                  aligned = until (\d -> d `mod` 10 == block) (\d -> (d + 1) .&. 0xffff) start
              if aligned >= 1000
                then reportLimit >> pure Nothing
                else restart >>= \s -> pure (Just (aligned, file, s))
          | otherwise -> do
              full <- exhausted sequence1
              if full
                then newFolder previous >>= maybe (pure Nothing) (\d -> restart >>= \s -> pure (Just (d, blockStart d, s)))
                else pure (Just (directory, blockStart previous, sequence1))

-- | 0x70fbfc.
prepareM :: Word32 -> Word32 -> Word32 -> S Word32
prepareM path suffix prefix = do
  status <- mediaStatus
  if status /= 0
    then pure status
    else do
      group <- wordAt (contextBase + selectedGroups) >>= groupIndex
      if group == 0xff
        then pure 8
        else do
          latch <- byteAt (contextBase + sequenceLatch)
          if latch == 2
            then do
              s <- halfAt (contextBase + sequenceCount)
              setHalf (contextBase + sequenceCount) (s + 1)
              setByte (contextBase + sequenceLatch) 1
            else pure ()
          sequence0 <- halfAt (contextBase + sequenceCount)
          numbers <- readNumbersM
          case numbers of
            Left failure -> pure failure
            Right pair -> do
              allocated <- allocateM pair sequence0
              case allocated of
                Nothing -> pure 6
                Just (directory, file, sequence1) -> do
                  drivePrefix path
                  copyBytes (path + 2) Nothing "\\DCIM\\"
                  formatDecimal (path + 8) directory 3
                  ownSuffix <- pure (counterAt group + 0xa)
                  suffixSource <-
                    if suffix == 0 then pure ownSuffix else (\ok -> if ok == 0 then suffix else ownSuffix) <$> checkM suffix 5
                  copyFrom (path + 0xb) suffixSource 5
                  setByte (path + 0x10) 0x5c
                  if prefix /= 0
                    then do
                      ok <- checkM prefix 4
                      copyFrom (path + 0x11) (if ok == 0 then prefix else contextBase + prefixOffset) 4
                    else do
                      mode <- wordAt (contextBase + prefixMode)
                      let sequenced keys = do
                            mapM_ (\(key, offset) -> configuration key >>= setByte (path + offset)) (zip keys [0x11, 0x12])
                            formatDecimal (path + 0x13) sequence1 2
                      case mode of
                        0 -> copyFrom (path + 0x11) (contextBase + prefixOffset) 4
                        1 -> sequenced [0x142, 0x143]
                        2 -> sequenced [0x144, 0x145]
                        _ -> pure ()
                  formatDecimal (path + 0x15) file 4
                  setByte (path + 0x19) 0
                  pure 0

-- | Bytes the scripted directory service writes at entry + 0x24 and the cluster service reads.
entryRecord :: B.ByteString
entryRecord = B.pack [0x30 .. 0x4f]

-- | 0x716184.
createM :: Word32 -> Word32 -> S Word32
createM number suffix = do
  made (Service fillS [("r1", 0), ("r2", 30)] [] [] [AtRegister "r0" 0 (B.replicate 30 0)])
  if (number - 100) .&. 0xffff >= 900
    then pure 8
    else do
      drive <- currentDrive
      if drive == 0
        then pure 8
        else do
          let digits = decimal 3 number
          made (Service formatNumberS [("r1", number), ("r2", 3), ("r3", 1)] [] [] [AtRegister "r0" 0 digits])
          suffixBytes <- bytesAt suffix 5
          made (Service copyMemoryS [("r1", suffix), ("r2", 5)] [] [AtRegister "r1" 0 suffixBytes] [AtRegister "r0" 0 suffixBytes])
          occupied <- wordAt (contextBase + directoryClusters + 4 * number)
          if occupied /= 0
            then pure 8
            else do
              e <- ask
              let request = B.pack [fromIntegral drive, 0x3a] <> "\\DCIM\\" <> digits <> suffixBytes <> "\\" <> B.replicate 31 0
                  status = envOpen e
              made
                ( Service openDirectoryS [("r2", 0)] [("r0", status)] [AtRegister "r0" 0 request]
                    [AtRegister "r1" 0x24 entryRecord | status == 0]
                )
              if status /= 0
                then pure ()
                else do
                  made (Service entryClusterS [] [("r0", envCluster e)] [AtRegister "r0" 0 entryRecord] [])
                  setWord (contextBase + directoryClusters + 4 * number) (envCluster e)
                  made (Service pathGroupsS [] [] [AtRegister "r0" 0 (word32le 0), AtRegister "r1" 0 (B.take 18 request)] [AtRegister "r0" 0 (word32le (envPathMask e))])
                  group <- groupIndex (envPathMask e)
                  if group == 0xff
                    then pure ()
                    else do
                      setByte (contextBase + directoryGroups + number) group
                      known <- halfAt (counterAt group + 4)
                      if known < number
                        then do
                          setHalf (counterAt group + 4) number
                          setHalf (counterAt group + 6) 0
                          if group == 0 then setWord (contextBase + largestFirstGroup) number else pure ()
                        else pure ()
                  maximum0 <- wordAt (contextBase + maximumDirectory)
                  if number > maximum0
                    then setWord (contextBase + maximumDirectory) number >> setWord (contextBase + maximumFile) 0
                    else pure ()
              plain translateS [("r0", 0), ("r1", status)] (if status == 0 then 0 else 0x100 + status)

-- | 0x71141c.
writtenM :: Word32 -> S ()
writtenM slot = do
  let entry = contextBase + 16 * slot
  directory <- halfAt (entry + 4)
  file <- halfAt (entry + 6)
  group <- byteAt (contextBase + directoryGroups + directory)
  known <- halfAt (counterAt group + 4)
  knownFile <- halfAt (counterAt group + 6)
  if not (directory > known || (directory == known && knownFile < file))
    then pure ()
    else do
      setHalf (counterAt group + 4) directory
      setHalf (counterAt group + 6) file
      wordAt (counterAt group) >>= setWord (counterAt group) . (+ 1)
      drive <- currentDrive
      storePair drive group directory file
      halfAt (contextBase + sequenceCount) >>= setHalf (contextBase + lastSequence)
      wordAt (contextBase + writtenFiles) >>= setWord (contextBase + writtenFiles) . (+ 1)
      maximum0 <- wordAt (contextBase + maximumDirectory)
      maximumFile0 <- wordAt (contextBase + maximumFile)
      if maximum0 < directory || (maximum0 == directory && maximumFile0 < file)
        then setWord (contextBase + maximumDirectory) directory >> setWord (contextBase + maximumFile) file
        else pure ()
      maximum1 <- wordAt (contextBase + maximumDirectory)
      owner <- byteAt (contextBase + directoryGroups + maximum1)
      if owner == 0
        then pure ()
        else do
          drive1 <- currentDrive
          (persisted, _) <- profilePair drive1 0
          enabled <- wordAt (contextBase + profileEnabled)
          let next0 = (maximum1 + 1) .&. 0xffff
              next = if enabled == 1 && next0 < persisted then persisted else next0
          _ <- createM next (counterAt 0 + 0xa)
          pure ()
      effect snapshotS []

-- Cases ---------------------------------------------------------------------------------------------

-- | A context, path and text arrangement; unnamed fields are zero.
data Setup = Setup
  { sCounters :: [(Word32, Word32, Word32, Word32)]
  , sSuffixes :: [(Word32, B.ByteString)]
  , sMaximum :: (Word32, Word32)
  , sLargest :: Word32
  , sMask :: Word32
  , sGroups :: [(Word32, Word32)]
  , sCount :: Word32
  , sClusters :: [(Word32, Word32)]
  , sSequence :: Word32
  , sLatch :: Word32
  , sMode :: Word32
  , sProfileEnabled :: Word32
  , sWritten :: Word32
  , sSlots :: [(Word32, Word32, Word32)]
  , sText :: B.ByteString
  }

setup :: Setup
setup = Setup
  { sCounters = []
  , sSuffixes = [(0, "_FUJI"), (1, "_TEST")]
  , sMaximum = (0, 0)
  , sLargest = 0
  , sMask = 1
  , sGroups = []
  , sCount = 1
  , sClusters = []
  , sSequence = 0
  , sLatch = 0
  , sMode = 0
  , sProfileEnabled = 0
  , sWritten = 0
  , sSlots = []
  , sText = ""
  }

initialWorld :: Setup -> World
initialWorld s = World memory []
  where
    memory =
      M.fromList $
        [ (address, 0) | (_, offset, size) <- contextSpans, address <- [contextBase + offset .. contextBase + offset + fromIntegral size - 1] ]
          ++ [(pathBase + index, 0xee) | index <- [0 .. fromIntegral pathBytes - 1]]
          ++ [(textBase + index, 0) | index <- [0 .. fromIntegral textBytes - 1]]
          ++ [(outputBase + index, 0xcc) | index <- [0 .. fromIntegral outputBytes - 1]]
          ++ placed
    placed =
      concat
        [ bytesFrom contextBase "DSCF"
        , concat [bytesFrom (counterAt g) (word32le count <> B.take 2 (word32le d) <> B.take 2 (word32le f)) | (g, count, d, f) <- sCounters s]
        , concat [bytesFrom (counterAt g + 0xa) text | (g, text) <- sSuffixes s]
        , bytesFrom (contextBase + maximumDirectory) (word32le (fst (sMaximum s)) <> word32le (snd (sMaximum s)))
        , bytesFrom (contextBase + largestFirstGroup) (word32le (sLargest s))
        , bytesFrom (contextBase + selectedGroups) (word32le (sMask s))
        , concat [bytesFrom (contextBase + directoryGroups + n) (B.singleton (fromIntegral g)) | (n, g) <- sGroups s]
        , bytesFrom (contextBase + counterCount) (B.singleton (fromIntegral (sCount s)))
        , concat [bytesFrom (contextBase + directoryClusters + 4 * n) (word32le c) | (n, c) <- sClusters s]
        , bytesFrom (contextBase + sequenceCount) (B.take 2 (word32le (sSequence s)))
        , bytesFrom (contextBase + sequenceLatch) (B.singleton (fromIntegral (sLatch s)))
        , bytesFrom (contextBase + prefixMode) (word32le (sMode s))
        , bytesFrom (contextBase + profileEnabled) (word32le (sProfileEnabled s))
        , bytesFrom (contextBase + writtenFiles) (word32le (sWritten s))
        , concat [bytesFrom (contextBase + 16 * i + 4) (B.take 2 (word32le d) <> B.take 2 (word32le f)) | (i, d, f) <- sSlots s]
        , bytesFrom textBase (sText s)
        ]
    bytesFrom address bytes = zip [address ..] (B.unpack bytes)

regionsOf :: B.ByteString -> World -> World -> [DataRegion]
regionsOf image initial final =
  [DataRegion "context-slot" contextSlot (word32le contextBase) (word32le contextBase)]
    ++ [ DataRegion name (contextBase + offset) (slice initial (contextBase + offset) size) (slice final (contextBase + offset) size)
       | (name, offset, size) <- contextSpans
       ]
    ++ [ DataRegion name address (slice initial address size) (slice final address size)
       | (name, address, size) <- [("path", pathBase, pathBytes), ("text", textBase, textBytes), ("outputs", outputBase, outputBytes)]
       ]
    ++ [ let bytes = B.take size (B.drop (fromIntegral (address - imageBase)) image) in DataRegion name address bytes bytes
       | (name, address, size) <- stockData
       ]
  where
    slice world address size = B.pack [M.findWithDefault 0 (address + fromIntegral index) (wMemory world) | index <- [0 .. size - 1]]

-- | One driver call and the model of the entry it makes; a returning entry yields r0.
data Step = Step Word32 [Word32] (S (Maybe Word32))

returning :: Word32 -> [Word32] -> S Word32 -> Step
returning entry arguments model = Step entry arguments (Just <$> model)

void :: Word32 -> [Word32] -> S () -> Step
void entry arguments model = Step entry arguments (Nothing <$ model)

scenario :: B.ByteString -> String -> Env -> Setup -> [Step] -> Scenario
scenario image name e s steps =
  Scenario
    { scenarioName = name
    , scenarioEntry = case steps of (Step entry _ _ : _) -> entry; [] -> 0
    , scenarioData = regionsOf image initial final
    , scenarioCalls = [Call entry arguments [] (result /= Nothing) | (Step entry arguments _, result) <- zip steps results]
    , scenarioServices = wServices final
    , scenarioResults = [value | Just value <- results]
    , scenarioStop = Nothing
    }
  where
    initial = initialWorld s
    (results, final) = runS (mapM (\(Step _ _ model) -> model) steps) e initial

-- | Writes the pair a model returns to the output words, as the pointer-taking entries do.
outputs :: S (Word32, Word32) -> S ()
outputs model = model >>= \(directory, file) -> setHalf outputBase directory >> setHalf (outputBase + 2) file

prepare :: Word32 -> Word32 -> Step
prepare suffix prefix = returning prepareName [pathBase, suffix, prefix] (prepareM pathBase suffix prefix)

scenarios :: B.ByteString -> [Scenario]
scenarios image =
  [ scenario image "name-empty-card" env setup [prepare 0 0]
  , scenario image "name-next-in-folder" env setup {sCounters = [(0, 4, 103, 3095)], sMaximum = (103, 3095), sLargest = 103} [prepare 0 0]
  , scenario image "name-block-full" env setup {sCounters = [(0, 4, 103, 3999)], sMaximum = (103, 3999), sLargest = 103} [prepare 0 0]
  , scenario image "name-file-wrap" env setup {sCounters = [(0, 4, 103, 9999)], sMaximum = (103, 9999), sLargest = 103} [prepare 0 0]
  , scenario image "name-past-card-maximum" env setup {sCounters = [(0, 4, 103, 3999)], sMaximum = (250, 1), sLargest = 103} [prepare 0 0]
  , scenario image "name-folder-limit" env setup {sCounters = [(0, 4, 999, 9999)], sMaximum = (999, 9999), sLargest = 999} [prepare 0 0]
  , scenario image "name-limit-after-wrap" env setup {sMask = 2, sCount = 2, sCounters = [(1, 4, 998, 9999)], sMaximum = (999, 1)} [prepare 0 0]
  , scenario image "name-block-above-folder" env setup {sCounters = [(0, 4, 100, 3095)], sMaximum = (100, 3095), sLargest = 100} [prepare 0 0]
  , scenario image "name-block-above-limit" env setup {sMask = 2, sCount = 2, sCounters = [(1, 4, 101, 3095)], sMaximum = (998, 1)} [prepare 0 0]
  , scenario image "name-block-below-folder" env setup {sCounters = [(0, 4, 105, 2010)], sMaximum = (105, 2010), sLargest = 105} [prepare 0 0]
  , scenario image "name-step-over-thousand" env setup {sCounters = [(0, 4, 105, 1999)], sMaximum = (105, 1999), sLargest = 105} [prepare 0 0]
  , scenario image "name-caller-text" env setup {sText = "_TEST\0\0\0ABCD"} [prepare textBase (textBase + 8)]
  , scenario image "name-caller-text-invalid" env setup {sText = "_T-ST\0\0\0AB.D"} [prepare textBase (textBase + 8)]
  , scenario image "name-second-group" env setup {sMask = 6, sCount = 2, sCounters = [(0, 1, 101, 1001), (1, 2, 120, 7)], sMaximum = (120, 7), sGroups = [(120, 1)]} [prepare 0 0]
  , scenario image "name-sequenced-a" env setup {sMode = 1, sLatch = 2, sSequence = 6} [prepare 0 0, prepare 0 0]
  , scenario image "name-sequenced-b" env setup {sMode = 2, sLatch = 2, sSequence = 41, sCounters = [(0, 1, 102, 2050)], sMaximum = (102, 2050)} [prepare 0 0]
  , scenario image "name-sequence-exhausted" env setup {sMode = 1, sLatch = 2, sSequence = 99, sCounters = [(0, 1, 100, 5)], sMaximum = (100, 5)} [prepare 0 0]
  , scenario image "name-sequence-exhausted-below" env setup {sMode = 2, sLatch = 1, sSequence = 120, sCounters = [(0, 1, 105, 2010)], sMaximum = (105, 2010)} [prepare 0 0]
  , scenario image "name-unknown-mode" env setup {sMode = 5} [prepare 0 0]
  , scenario image "name-media-status" env {envMedia = 0x17} setup [prepare 0 0]
  , scenario image "name-no-group" env setup {sMask = 0} [prepare 0 0]
  , scenario image "name-no-drive" env {envDrive = 0} setup [prepare 0 0]
  , scenario image "name-profile-ahead" env {envProfile = const (105, 5010)} setup {sProfileEnabled = 1, sCounters = [(0, 4, 103, 3096)], sMaximum = (103, 3096), sLargest = 103} [prepare 0 0]
  , scenario image "name-profile-foreign-folder" env {envProfile = const (130, 10)} setup {sProfileEnabled = 1, sCounters = [(0, 4, 103, 3096)], sMaximum = (125, 4), sLargest = 103, sGroups = [(130, 1)]} [prepare 0 0]
  , scenario image "name-profile-behind" env {envProfile = const (101, 1500)} setup {sProfileEnabled = 1, sCounters = [(0, 4, 103, 3096)], sMaximum = (103, 3096), sLargest = 103} [prepare 0 0]
  , scenario image "name-then-close-then-name" env setup {sCounters = [(0, 4, 103, 3095)], sMaximum = (103, 3095), sLargest = 103, sSlots = [(3, 103, 3096)]}
      [prepare 0 0, void noteWritten [3] (writtenM 3), prepare 0 0]
  , scenario image "read-numbers" env setup {sCounters = [(0, 4, 103, 3095)], sMaximum = (103, 3095), sLargest = 103}
      [returning readNumbersEntry [outputBase, outputBase + 2] (readNumbersM >>= either pure (\(d, f) -> setHalf outputBase d >> setHalf (outputBase + 2) f >> pure 0))]
  , scenario image "read-numbers-no-drive" env {envDrive = 0} setup
      [returning readNumbersEntry [outputBase, outputBase + 2] (readNumbersM >>= either pure (const (pure 0)))]
  , scenario image "last-used-empty-folder-100" env setup {sClusters = [(100, 7)]}
      [returning lastUsedEntry [0x43, 0, outputBase, outputBase + 2] (outputs (lastUsedM 0x43 0) >> pure 0)]
  , scenario image "last-used-maximum-unmatched" env setup {sCounters = [(0, 1, 104, 4002)], sMaximum = (104, 4002), sLargest = 101}
      [returning lastUsedEntry [0x43, 0, outputBase, outputBase + 2] (outputs (lastUsedM 0x43 0) >> pure 0)]
  , scenario image "last-used-other-group" env {envProfile = const (150, 3)} setup {sProfileEnabled = 7, sCounters = [(1, 1, 50, 0)], sMaximum = (140, 9)}
      [returning lastUsedEntry [0x43, 1, outputBase, outputBase + 2] (outputs (lastUsedM 0x43 1) >> pure 0)]
  , scenario image "check-name" env setup {sText = "Ab_9-xyz"}
      [ returning checkName [textBase, 4] (checkM textBase 4)
      , returning checkName [textBase, 5] (checkM textBase 5)
      , returning checkName [textBase, 0] (checkM textBase 0)
      ]
  , scenario image "prefix-modes" env setup {sLatch = 1, sMode = 1}
      [void setPrefixMode [2] (modeM 2), void setPrefixMode [9] (modeM 9), void setPrefixMode [0] (modeM 0), void setPrefixMode [1] (modeM 1)]
  , scenario image "continuous-on" env setup {sCount = 3, sCounters = [(0, 1, 103, 3096), (1, 1, 120, 12), (2, 0, 0, 0)]}
      [void applyContinuous [1] (continuousM 1)]
  , scenario image "continuous-off" env setup {sProfileEnabled = 1} [void applyContinuous [0] (continuousM 0)]
  , scenario image "continuous-other-value" env setup [void applyContinuous [2] (continuousM 2)]
  , scenario image "continuous-no-card" env {envMedia = 3} setup [void applyContinuous [1] (continuousM 1)]
  , scenario image "profile-read" env {envProfile = \g -> (110 + g, 0x12345)} setup
      [void readProfile [0x43, 2, outputBase, outputBase + 2] (outputs (profilePair 0x43 2))]
  , scenario image "profile-write" env setup [void writeProfile [0x43, 1, 117, 7042] (storePair 0x43 1 117 7042)]
  , scenario image "written-advances" env setup {sCounters = [(0, 4, 103, 3095)], sMaximum = (103, 3095), sSequence = 4, sSlots = [(2, 103, 3096)], sWritten = 9}
      [void noteWritten [2] (writtenM 2)]
  , scenario image "written-older" env setup {sCounters = [(0, 4, 103, 3095)], sMaximum = (103, 3095), sSlots = [(0, 103, 3001)]}
      [void noteWritten [0] (writtenM 0)]
  , scenario image "written-same-number" env setup {sCounters = [(0, 4, 103, 3095)], sMaximum = (103, 3095), sSlots = [(0, 103, 3095)]}
      [void noteWritten [0] (writtenM 0)]
  , scenario image "written-new-folder-below-maximum" env setup {sCounters = [(0, 4, 103, 3999)], sMaximum = (110, 5), sSlots = [(1, 104, 4001)]}
      [void noteWritten [1] (writtenM 1)]
  , scenario image "written-foreign-maximum" env setup {sSlots = [(0, 104, 4001)], sGroups = [(104, 1)], sCount = 2}
      [void noteWritten [0] (writtenM 0)]
  , scenario image "written-foreign-profile-ahead" env {envProfile = const (140, 2)} setup {sProfileEnabled = 1, sSlots = [(0, 104, 4001)], sGroups = [(104, 1)], sCount = 2}
      [void noteWritten [0] (writtenM 0)]
  , scenario image "create-folder" env setup [returning createFolder [107, counterAt 0 + 0xa] (createM 107 (counterAt 0 + 0xa))]
  , scenario image "create-folder-second-group" env {envPathMask = 2} setup {sCounters = [(1, 0, 150, 3)], sMaximum = (170, 2)}
      [returning createFolder [160, counterAt 1 + 0xa] (createM 160 (counterAt 1 + 0xa))]
  , scenario image "create-folder-unknown-group" env {envPathMask = 0} setup [returning createFolder [130, textBase] (createM 130 textBase)]
  , scenario image "create-folder-existing" env setup {sClusters = [(107, 9)]} [returning createFolder [107, counterAt 0 + 0xa] (createM 107 (counterAt 0 + 0xa))]
  , scenario image "create-folder-range" env setup
      [ returning createFolder [99, counterAt 0 + 0xa] (createM 99 (counterAt 0 + 0xa))
      , returning createFolder [1000, counterAt 0 + 0xa] (createM 1000 (counterAt 0 + 0xa))
      , returning createFolder [999, counterAt 0 + 0xa] (createM 999 (counterAt 0 + 0xa))
      ]
  , scenario image "create-folder-no-drive" env {envDrive = 0} setup [returning createFolder [107, counterAt 0 + 0xa] (createM 107 (counterAt 0 + 0xa))]
  , scenario image "create-folder-open-fails" env {envOpen = 5} setup [returning createFolder [107, counterAt 0 + 0xa] (createM 107 (counterAt 0 + 0xa))]
  , scenario image "create-folder-below-counter" env setup {sCounters = [(0, 3, 140, 4)], sMaximum = (140, 4)}
      [returning createFolder [120, counterAt 0 + 0xa] (createM 120 (counterAt 0 + 0xa))]
  ]
