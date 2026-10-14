-- One bounded raw storage read of the two area-2 banks over the queue-7
-- action 37 path: C0 open, the session-start cache snapshot, the gate
-- enable and readback of probe-ram, the group-2 state precondition, the
-- map's start word and entry count, the two area-2 map entries, then per
-- bank the disarm, the staging field writes, one trigger, the result poll
-- and the bank capture, and on every exit path the disarm, the 0x81..0x8a
-- and 0xf7 restores, their readback and C4.
--
-- The evidence is private-data/analysis/x-m1-1.50/end-setting/followup/
-- raw-media-read.md and the third independent pass over it. A config write
-- 0x1a0002 function 3 to adjustment index 0x80 with the whitelisted value
-- 0x35 posts queue-7 row 34, whose action 37 reads eight raw device blocks
-- into the fixed staging window 0x4b2ece00 with the block selector from cache
-- 0x82 under the bound block + count <= 0x400. The bytes leave the camera
-- with the ordinary 0x200001 function 2 RAM read. Nothing here writes media.
--
-- Which blocks those are comes from the map, not from the encoded bases.
-- The descriptor-layer area read resolves its map-relative index with
-- FUN_006fca60, u16(map + 0x10 + 2 * index) + u32(map + 0) for an index
-- below the entry count u32(map + 8), and reads that physical block through
-- FUN_006f1a74 as block * PPB + page; action 37's own raw-media read feeds
-- FUN_006f1a74 the same block * PPB with no resolution at all. So cache 0x82
-- names the map-resolved blocks entry + start, one request per bank, where
-- start is the map object's own start word and not a constant, and the
-- encoded bases 0x3d6/0x3d7 are what those entries give only under the
-- identity map. The map is runtime state rebuilt at install, so the count,
-- the start word and the entries are read per run and never assumed.
--
-- What the run leaves behind: action 37 opens the storage driver at 0xdbcfe0
-- and closes it at 0xdbd030 with no refcount and no already-open guard, so
-- the driver is shut down afterwards and the owner power-cycles before using
-- the body normally, including after a failed run. The staging window is the
-- head of the frame-buffer pool rather than private scratch, so the session
-- must have no capture, live view or playback in flight.
--
-- The limits live in types wherever a value enters: the only values index
-- 0x80 can take are the trigger 0x35 and the disarm 0x33, the only config
-- function 3 start indices are 0x80..0x8a and 0xf7 and every such request is
-- the 37-byte length-1 form with one appended byte, so no request can be
-- zero-length or span 0x80, the run's own write to cache 0x81 passes zero
-- and a restore write there cannot scan the whitelist because only a
-- request starting at 0x80 is scanned, cache 0x82 is built from a
-- BlockSelector whose constructor is not exported and whose only builder
-- refuses anything above 0x7f, and the map start is a MapStart whose only
-- builder accepts raw blocks of the device. The staging requests come from
-- the fixed per-bank walks bankSlices over
-- [0x4b2ece00 + bankOffset, 0x4b2ece00 + bankOffset + 0x20000), so no
-- staging address leaves the window.
--
-- The result sentinel 0xff is written to cache 0x83 before the trigger and
-- the poll accepts only its transition to 0, 1 or 2 before the first staging
-- byte is read, because the handler fills the window page by page and nothing
-- orders these reads against it. The group-2 state halfword at 0x015c20b0 is
-- a precondition and not a safety gate: state 4 sends action 37 to action 3,
-- so the read cannot run there, while the protection against the destructive
-- rows is that this command cannot emit their values. A stop before the
-- cache snapshot writes only the disarm, because nothing had been changed
-- yet.
module FujiAF.ReadMedia
  ( CacheIndex(..), cacheIndexOf
  , TriggerValue(..), triggerByte, triggerStatus
  , BlockSelector, mkBlockSelector, blockSelectorByte
  , CacheSnapshot(..), cacheSnapshotOf, restorePlan
  , stagingBase, stagingBytes, stagingSliceLength, blockBytes
  , bankSlices, bankSliceCount, mediaBankCount, mediaSliceCount
  , stateAddress, mapPointerAddress, mapEntryIndexes, mapEntryTableOffset
  , mapEntryAddress, mapStartOffset, mapStartAddress, MapStart, mkMapStart, mapStartBlock
  , mapEntryCountOffset, mapEntryCountAddress, mapHeaderBytes
  , resultSentinelByte, spareGateByte
  , mediaPollCap, mediaPollSpacingUs
  , mediaConfigSteps, mediaAggregateDeadlineMs
  , mediaJournalFixedBytes, mediaJournalPerRequestBytes, mediaJournalCapBytes
  , mediaFsyncEvery
  , cacheWriteFrame, decodeCacheWriteReply
  , mediaReadFrame, mediaReadReplyBytes, decodeMediaReadReply
  , BankPlan(..), deriveBankPlan, deriveBankPlans
  , runReadMedia
  ) where

import Control.Concurrent (threadDelay)
import Control.Exception
import Control.Monad (forM, forM_, unless, when)
import qualified Data.ByteString as B
import Data.IORef
import Data.List (nub)
import Data.Word (Word8)
import Foreign.Ptr (castPtr)
import Numeric (showHex)
import System.FilePath ((</>))
import System.Posix.Files (getFdStatus, isRegularFile)
import System.Posix.IO
import System.Posix.Types (Fd)
import System.Posix.Unistd (fileSynchronise)
import FujiAF.Transport
import FujiAF.Read
  ( ReadWindow(..), decodeReadReply, readOpcode, readReplyBytes
  , readRequestBytes, readRequestFrame, readWindowLength
  , replyError1Offset, replyError2Offset
  , replyFunctionOffset, replyMailboxOffset, replyLengthOffset
  , replyOffsetOffset, replyPageOffset, replyReservedOffset
  , sessionFixedTransfers, word32At, word32le )
import FujiAF.Probe
  ( configReplyBytes, configWriteBytes, configWriteFunction, configMailbox
  , configWriteLength, debugDataOffset, debugFunction, debugOpcode
  , debugRequestBytes, gateByteOf, gateReadbackEnabled, gateReadbackRestored
  , probeName, readbackIndex, transfersPerRequest )
import FujiAF.SHA256 (sha256)
import qualified FujiAF.Broker as P

-- A config function 3 start index. The whole writable set is here: the
-- whitelist scan byte, the staging fields and the RAM-read gate. The values
-- 0x84..0x8a are restore-only, the snapshot bytes the run writes back and
-- never sets.
data CacheIndex
  = TriggerIndex | SpareGateIndex | BlockIndex | ResultIndex
  | RestoreIndex0 | RestoreIndex1 | RestoreIndex2 | RestoreIndex3
  | RestoreIndex4 | RestoreIndex5 | RestoreIndex6
  | RamGateIndex
  deriving (Eq, Ord, Show, Read, Enum, Bounded)

cacheIndexOf :: CacheIndex -> Int
cacheIndexOf TriggerIndex = 0x80
cacheIndexOf SpareGateIndex = 0x81
cacheIndexOf BlockIndex = 0x82
cacheIndexOf ResultIndex = 0x83
cacheIndexOf RestoreIndex0 = 0x84
cacheIndexOf RestoreIndex1 = 0x85
cacheIndexOf RestoreIndex2 = 0x86
cacheIndexOf RestoreIndex3 = 0x87
cacheIndexOf RestoreIndex4 = 0x88
cacheIndexOf RestoreIndex5 = 0x89
cacheIndexOf RestoreIndex6 = 0x8a
cacheIndexOf RamGateIndex = 0xf7

-- The only two values that can reach index 0x80: the flag-1 trigger that
-- posts row 34 and the flag-0 disarm that makes any later request at 0x80
-- inert. Every other whitelist value is unreachable from this command.
data TriggerValue = TriggerArm | TriggerDisarm
  deriving (Eq, Show, Read)

triggerByte :: TriggerValue -> Word8
triggerByte TriggerArm = 0x35
triggerByte TriggerDisarm = 0x33

-- The reply each 0x80 write must carry. The scan posts the trigger and the
-- handler answers (0,0) with its async marker set; the disarm posts nothing
-- and answers (1,0xffff).
triggerStatus :: TriggerValue -> (Int,Int)
triggerStatus TriggerArm = (0,0)
triggerStatus TriggerDisarm = (1,0xffff)

-- The block selector cache 0x82 carries: block = selector << 3 with count 8,
-- so selector <= 0x7f is the bound block + count <= 0x400. The constructor is
-- not exported, so no value above 0x7f exists in the type.
newtype BlockSelector = BlockSelector Word8
  deriving (Eq, Show, Read)

mkBlockSelector :: Int -> Either String BlockSelector
mkBlockSelector value
  | value < 0 = Left ("block selector below zero: "++show value)
  | value > 0x7f = Left ("block selector above 0x7f would exceed the 0x400-block device: "++show value)
  | otherwise = Right (BlockSelector (fromIntegral value))

blockSelectorByte :: BlockSelector -> Word8
blockSelectorByte (BlockSelector value) = value

-- The session-start bytes of the staging fields and the RAM gate, read before
-- anything is written and written back on every exit path.
data CacheSnapshot = CacheSnapshot { snapshotFields :: [Word8], snapshotGate :: Word8 }
  deriving (Eq, Show)

cacheSnapshotOf :: [Word8] -> Either String CacheSnapshot
cacheSnapshotOf window
  | length window /= readWindowLength = Left ("cache snapshot window must be exactly "++show readWindowLength++" bytes")
  | otherwise = Right (CacheSnapshot [window !! cacheIndexOf index | index <- [SpareGateIndex .. RestoreIndex6]] (window !! cacheIndexOf RamGateIndex))

restorePlan :: CacheSnapshot -> [(CacheIndex, Word8)]
restorePlan snapshot =
  zip [SpareGateIndex .. RestoreIndex6] (snapshotFields snapshot) ++ [(RamGateIndex, snapshotGate snapshot)]

-- The staging window action 37 fills: eight raw blocks of 0x20000 bytes with
-- the spare gate at 0, so the window is 0x100000 bytes. Where a bank's block
-- sits inside that window is derived from its map entry and the map's start
-- word, not assumed.
stagingBase, stagingBytes, stagingSliceLength, blockBytes :: Int
stagingBase = 0x4b2ece00
stagingBytes = 0x100000
stagingSliceLength = 248
blockBytes = 0x20000

-- The 248-byte walk of one bank's block at its in-window offset: 248 is the
-- largest request the 0x200001 function 2 handler returns complete (it
-- advertises 284 = 0x24 + 248), and the last request carries the 128-byte
-- remainder.
bankSlices :: Int -> [(Int,Int)]
bankSlices offset =
  [ (stagingBase + offset + index*stagingSliceLength, min stagingSliceLength (blockBytes - index*stagingSliceLength))
  | index <- [0 .. bankSliceCount - 1] ]

bankSliceCount :: Int
bankSliceCount = (blockBytes + stagingSliceLength - 1) `div` stagingSliceLength

-- The group-2 state halfword and the map object. FUN_006fca60 resolves an
-- index below the entry count u32(map + 8) to u16(map + 0x10 + 2 * index) +
-- u32(map + 0): the count at +8, the entry table at +0x10 and the start word
-- at +0, all runtime state (raw-media-read.md section 5). The device holds
-- mediaDeviceBlocks raw blocks, the device-size word of the geometry record
-- at 0x82988c (record 3 of 0x829844 + dev*0x18, dev pinned at 3 by the
-- driver open at 0xdbcfe0), and every resolved block is the start plus a
-- u16, so a start outside those blocks resolves nothing this read could
-- fetch.
stateAddress, mapPointerAddress, mapEntryTableOffset, mapStartOffset, mapEntryCountOffset, mapHeaderBytes, mediaDeviceBlocks :: Int
stateAddress = 0x015c20b0
mapPointerAddress = 0x00919a20
mapEntryTableOffset = 0x10
mapStartOffset = 0
mapEntryCountOffset = 8
mapHeaderBytes = mapEntryCountOffset + 4
mediaDeviceBlocks = 0x400

mapEntryAddress :: Int -> Int -> Int
mapEntryAddress mapBase index = mapBase + mapEntryTableOffset + 2*index

mapStartAddress :: Int -> Int
mapStartAddress mapBase = mapBase + mapStartOffset

mapEntryCountAddress :: Int -> Int
mapEntryCountAddress mapBase = mapBase + mapEntryCountOffset

-- The map start word FUN_006fca60 adds to every entry. It names a raw block
-- of the device, so the constructor is not exported and the only builder
-- accepts exactly those blocks.
newtype MapStart = MapStart Int
  deriving (Eq, Show)

mkMapStart :: Int -> Either String MapStart
mkMapStart value
  | value < 0 || value >= mediaDeviceBlocks =
      Left ("map start 0x"++showHex value ""++" is not a raw block of the 0x"++showHex mediaDeviceBlocks ""++"-block device")
  | otherwise = Right (MapStart value)

mapStartBlock :: MapStart -> Int
mapStartBlock (MapStart value) = value

mapEntryIndexes :: [Int]
mapEntryIndexes = [0x56, 0x57]

mediaBankCount :: Int
mediaBankCount = length mapEntryIndexes

mediaSliceCount :: Int
mediaSliceCount = mediaBankCount * bankSliceCount

resultSentinelByte, spareGateByte :: Word8
resultSentinelByte = 0xff
spareGateByte = 0

-- The poll is bounded twice: by count and by the aggregate deadline. The
-- firmware request carries a 2000 ms timeout, so 1024 polls spaced 5 ms
-- cover it with margin even on a slow host.
mediaPollCap :: Int
mediaPollCap = 1024

mediaPollSpacingUs :: Int
mediaPollSpacingUs = 5000

-- 5000 ms per transfer as every command: four transfers per config or RAM
-- step, per staging slice and per poll, plus the session's fixed eight. One
-- request per bank, so the poll bound is per bank.
mediaConfigSteps :: Int
mediaConfigSteps = 31

mediaAggregateDeadlineMs :: Int
mediaAggregateDeadlineMs = transferTimeoutMs
  *(sessionFixedTransfers + transfersPerRequest*mediaConfigSteps
    + transfersPerRequest*mediaSliceCount + transfersPerRequest*mediaPollCap*mediaBankCount)

-- The cap bounds each journal file. One staging slice costs about 3000
-- journal bytes over its four transfers and one summary line; the poll
-- allowance covers the full poll bound of both banks.
mediaJournalFixedBytes, mediaJournalPerRequestBytes, mediaJournalCapBytes :: Int
mediaJournalFixedBytes = 65536
mediaJournalPerRequestBytes = 4096
mediaJournalCapBytes = mediaJournalFixedBytes + mediaJournalPerRequestBytes*(mediaSliceCount + mediaPollCap*mediaBankCount)

-- The capture file is made durable every this many slices and at the end.
mediaFsyncEvery :: Int
mediaFsyncEvery = 256

-- Config function 3 (0x1a0002 function 3): function at +0x10, mb +0x14,
-- page +0x18, in-page offset +0x1c, length +0x20, one appended data byte.
-- The length is a constant here and the index is a CacheIndex, so no frame
-- can be zero-length, span an index, or name a byte this command may not
-- write.
cacheWriteFrame :: CacheIndex -> Word8 -> [Word8]
cacheWriteFrame index value = concat
  [ word32le readOpcode, replicate 12 0
  , word32le configWriteFunction, word32le configMailbox
  , word32le 0, word32le (cacheIndexOf index)
  , word32le configWriteLength, [value] ]

decodeCacheWriteReply :: CacheIndex -> (Int,Int) -> [Word8] -> Either String ()
decodeCacheWriteReply index expected bytes
  | length bytes /= configReplyBytes = Left ("reply length "++show (length bytes)++" differs from the requested "++show configReplyBytes)
  | word32At 0 bytes /= readOpcode = Left "reply opcode differs from the request"
  | word32At replyReservedOffset bytes /= 0 = Left "reply reserved word is nonzero"
  | word32At replyFunctionOffset bytes /= configWriteFunction = Left "reply function differs from the request"
  | word32At replyMailboxOffset bytes /= configMailbox = Left "reply mailbox differs from the request"
  | echoed /= requested = Left ("echoed page/offset/length "++show echoed++" differs from the request "++show requested)
  | status /= expected = Left ("status "++show status++" differs from the expected "++show expected)
  | otherwise = Right ()
  where
    echoed = (word32At replyPageOffset bytes, word32At replyOffsetOffset bytes, word32At replyLengthOffset bytes)
    requested = (0, cacheIndexOf index, configWriteLength)
    status = (word32At replyError1Offset bytes, word32At replyError2Offset bytes)

-- 0x200001 function 2 with the address at +0x1c and the length at +0x20, the
-- request shape probe-straps and dump-ram use. The reply is 0x24 + length
-- bytes with the address and length echoed at +0x1c and +0x20.
mediaReadFrame :: Int -> Int -> [Word8]
mediaReadFrame address size = concat
  [ word32le debugOpcode, replicate 12 0
  , word32le debugFunction, word32le 0, word32le 0
  , word32le address, word32le size ]

mediaReadReplyBytes :: Int -> Int
mediaReadReplyBytes size = debugDataOffset + size

decodeMediaReadReply :: Int -> Int -> [Word8] -> Either String [Word8]
decodeMediaReadReply address size bytes
  | length bytes /= mediaReadReplyBytes size = Left ("reply length "++show (length bytes)++" differs from the requested "++show (mediaReadReplyBytes size))
  | word32At 0 bytes /= debugOpcode = Left "reply opcode differs from the request"
  | word32At replyReservedOffset bytes /= 0 = Left "reply reserved word is nonzero"
  | word32At replyFunctionOffset bytes /= debugFunction = Left "reply function differs from the request"
  | otherwise = case status of
      (0,0) | echoed == (address,size) -> Right (take size (drop debugDataOffset bytes))
            | otherwise -> Left ("echoed address/length "++show echoed++" differs from the request "++show (address,size))
      (9,3) -> Left "rejected arguments: error1 9, error2 3"
      (1,0xffff) -> Left "closed gate: error1 1, error2 0xffff"
      (first,second) -> Left ("status the handler cannot produce: error1 "++show first++", error2 "++show second)
  where
    echoed = (word32At 0x1c bytes, word32At 0x20 bytes)
    status = (word32At replyError1Offset bytes, word32At replyError2Offset bytes)

-- One area-2 bank as its map names it: the entry read for the declared index
-- plus the map's start word resolves to the raw block holding the bank, one
-- request covers that block's eight-block window and the bank is captured at
-- its own offset inside the window.
data BankPlan = BankPlan
  { bankEntryIndex :: Int
  , bankEntry :: Int
  , bankBlock :: Int
  , bankSelector :: BlockSelector
  , bankOffset :: Int
  } deriving (Eq, Show)

deriveBankPlan :: MapStart -> Int -> (Int,Int) -> Either String BankPlan
deriveBankPlan start entryCount (index,entry)
  | index >= entryCount = Left ("map entry 0x"++showHex index ""++" is at or above the map's entry count 0x"++showHex entryCount "")
  | entry == 0xffff = Left ("map entry 0x"++showHex index ""++" is 0xffff: no physical block is mapped for that bank")
  | otherwise = do
      selector <- mkBlockSelector (block `div` 8)
      pure (BankPlan index entry block selector ((block `mod` 8)*blockBytes))
  where
    block = entry + mapStartBlock start

-- Two banks need two blocks; one map naming one raw block twice is not a map
-- that holds two banks.
deriveBankPlans :: MapStart -> Int -> [(Int,Int)] -> Either String [BankPlan]
deriveBankPlans start entryCount entries = do
  plans <- mapM (deriveBankPlan start entryCount) entries
  let blocks = map bankBlock plans
  if length blocks == length (nub blocks) then Right plans
    else Left "two area-2 map entries name the same raw block"

runReadMedia :: Clock -> UsbApi -> FilePath -> IO Int
runReadMedia clock api dir = runReadMediaCore clock api dir `catch` \(e::SomeException) -> do
  _ <- try (recordWithin mediaJournalCapBytes dir "session-uncertain" (take 4096 (displayException e))) :: IO (Either SomeException ())
  throwIO e

data MediaLink = MediaLink { linkSession :: Session, linkSid :: Int, linkInput :: Int, linkOutput :: Int }

mediaRefuse :: Session -> String -> [Word8] -> String -> IO a
mediaRefuse s label response reason = do
  note s "read-media-refused" (label++";reply="++P.hexBytes response++";"++reason)
  P.reject reason

mediaPrecondition :: Session -> String -> IO a
mediaPrecondition s reason = do
  note s "read-media-precondition" reason
  P.reject ("precondition failure: "++reason)

cacheWrite :: MediaLink -> CacheIndex -> Word8 -> IO ()
cacheWrite link index value = do
  let s = linkSession link
  _ <- tx s (P.controlTransfer (P.control 0xd0 configWriteBytes (linkSid link))) 12
  _ <- tx s (P.BulkOut (linkOutput link) (cacheWriteFrame index value)) configWriteBytes
  _ <- tx s (P.controlTransfer (P.control 0xd1 configReplyBytes (linkSid link))) 12
  bytes <- tx s (P.BulkIn (linkInput link) configReplyBytes) configReplyBytes
  either (mediaRefuse s ("cache-write-"++show (cacheIndexOf index)) bytes) pure
    (decodeCacheWriteReply index (0,0) bytes)
  note s "cache-write-complete" ("index=0x"++showHex (cacheIndexOf index) ""++";value="++show value++";status=(0,0)")

triggerWrite :: MediaLink -> TriggerValue -> IO ()
triggerWrite link value = do
  let s = linkSession link
  _ <- tx s (P.controlTransfer (P.control 0xd0 configWriteBytes (linkSid link))) 12
  _ <- tx s (P.BulkOut (linkOutput link) (cacheWriteFrame TriggerIndex (triggerByte value))) configWriteBytes
  _ <- tx s (P.controlTransfer (P.control 0xd1 configReplyBytes (linkSid link))) 12
  bytes <- tx s (P.BulkIn (linkInput link) configReplyBytes) configReplyBytes
  either (mediaRefuse s "trigger" bytes) pure
    (decodeCacheWriteReply TriggerIndex (triggerStatus value) bytes)
  note s "cache-write-complete" ("index=0x80;value="++show (triggerByte value)++";status="++show (triggerStatus value))

windowRead :: MediaLink -> String -> IO [Word8]
windowRead link label = do
  let s = linkSession link
      window = ReadWindow 0 readbackIndex label
  _ <- tx s (P.controlTransfer (P.control 0xd0 readRequestBytes (linkSid link))) 12
  _ <- tx s (P.BulkOut (linkOutput link) (readRequestFrame window)) readRequestBytes
  _ <- tx s (P.controlTransfer (P.control 0xd1 readReplyBytes (linkSid link))) 12
  bytes <- tx s (P.BulkIn (linkInput link) readReplyBytes) readReplyBytes
  either (mediaRefuse s label bytes) pure (decodeReadReply window bytes)

ramRead :: MediaLink -> String -> Int -> Int -> IO [Word8]
ramRead link label address size = do
  let s = linkSession link
  _ <- tx s (P.controlTransfer (P.control 0xd0 debugRequestBytes (linkSid link))) 12
  _ <- tx s (P.BulkOut (linkOutput link) (mediaReadFrame address size)) debugRequestBytes
  _ <- tx s (P.controlTransfer (P.control 0xd1 (mediaReadReplyBytes size) (linkSid link))) 12
  bytes <- tx s (P.BulkIn (linkInput link) (mediaReadReplyBytes size)) (mediaReadReplyBytes size)
  either (mediaRefuse s label bytes) pure (decodeMediaReadReply address size bytes)

u16At :: Int -> [Word8] -> Int
u16At at bytes = fromIntegral (bytes !! at) + 256*fromIntegral (bytes !! (at+1))

runReadMediaCore :: Clock -> UsbApi -> FilePath -> IO Int
runReadMediaCore clock api dir = do
  s <- newSession clock api dir mediaJournalCapBytes mediaAggregateDeadlineMs
  held <- newHeld
  openedSession <- newIORef Nothing
  snapshotRef <- newIORef Nothing
  completed <- newIORef (0::Int)
  note s "prepared" ("read-media;banks="++show mediaBankCount++";staging=0x"++probeName stagingBase
                     ++";bank-bytes="++show blockBytes++";slices="++show mediaSliceCount++";poll-cap="++show mediaPollCap)
  mask $ \restore -> do
    result <- try $ restore $ do
      device <- acquireDevice s held
      let input = inputEndpoint (descriptors device)
          output = outputEndpoint (descriptors device)
      _ <- tx s (P.controlTransfer (P.control 0xc0 8 0)) 12
      reply <- tx s (P.BulkIn input 8) 8
      -- C0 bytes 2..7 are opaque; every D1 length here is fixed by its handler.
      let sid = fromIntegral (reply!!0) + 256*fromIntegral (reply!!1)
          link = MediaLink s sid input output
      writeIORef openedSession (Just (device,sid))
      -- The session-start snapshot comes before the first write, so every
      -- restore on every exit path writes a value that was read, never a
      -- guess.
      snapshotBytes <- windowRead link "cache-snapshot"
      snapshot <- either (mediaRefuse s "cache-snapshot" snapshotBytes) pure (cacheSnapshotOf snapshotBytes)
      writeIORef snapshotRef (Just snapshot)
      writePrivateBytes (dir </> "cache-snapshot.bin") (B.pack snapshotBytes)
      note s "cache-snapshot" ("sha256="++sha256 (B.pack snapshotBytes)++";fields="++P.hexBytes (snapshotFields snapshot)++";gate="++show (snapshotGate snapshot))
      cacheWrite link RamGateIndex 1
      enabled <- windowRead link gateReadbackEnabled
      writePrivateBytes (dir </> (gateReadbackEnabled++".bin")) (B.pack enabled)
      note s "readback-complete" (gateReadbackEnabled++";sha256="++sha256 (B.pack enabled))
      unless (gateByteOf enabled == 1) (mediaRefuse s gateReadbackEnabled enabled "gate enable is not visible in the config readback")
      -- The precondition comes first among the RAM reads and the first stop
      -- decision: state 4 sends action 37 to action 3, so the read cannot
      -- run there.
      stateBytes <- ramRead link "group-2-state" stateAddress 2
      let state = word32At 0 stateBytes
      note s "group-2-state" ("state="++show state)
      when (state == 4) (mediaPrecondition s "group-2 state 4 disables the raw-media read")
      pointerBytes <- ramRead link "map-pointer" mapPointerAddress 4
      let mapBase = word32At 0 pointerBytes
      note s "map-pointer" ("map=0x"++probeName mapBase)
      headerBytes <- ramRead link "map-header" (mapStartAddress mapBase) mapHeaderBytes
      mapStart <- either (mediaRefuse s "map-header" headerBytes) pure (mkMapStart (word32At 0 headerBytes))
      let entryCount = word32At mapEntryCountOffset headerBytes
      note s "map-header" ("start=0x"++probeName (mapStartBlock mapStart)++";count=0x"++showHex entryCount "")
      entryReplies <- forM mapEntryIndexes $ \index -> do
        bytes <- ramRead link ("map-entry-0x"++showHex index "") (mapEntryAddress mapBase index) 2
        pure (index, bytes)
      let entries = [(index, u16At 0 bytes) | (index,bytes) <- entryReplies]
      plans <- either (mediaRefuse s "map-entries" (concatMap snd entryReplies)) pure (deriveBankPlans mapStart entryCount entries)
      forM_ (zip [0::Int ..] plans) $ \(position,plan) ->
        note s "media-plan" ("bank="++show position
          ++";index=0x"++showHex (bankEntryIndex plan) ""
          ++";entry=0x"++showHex (bankEntry plan) ""
          ++";block=0x"++showHex (bankBlock plan) ""
          ++";selector=0x"++showHex (blockSelectorByte (bankSelector plan)) ""
          ++";offset=0x"++probeName (bankOffset plan))
      forM_ (zip [0::Int ..] plans) $ \(position,plan) ->
        captureBank link dir position plan completed
    -- The disarm, the restores and their readback run on every exit path,
    -- including a stop and a timeout, each attempted once and none skipped
    -- because an earlier one failed. The protocol close follows them.
    errors <- newIORef ([] :: [String])
    let attempt label action = action `catch` \(e::SomeException) -> modifyIORef' errors (++[label++": "++displayException e])
    opened <- readIORef openedSession
    snapshot <- readIORef snapshotRef
    case opened of
      Nothing -> note s "cleanup-unavailable" "unknown SID; no invented C4"
      Just (device,sid) -> do
        let link = MediaLink s sid (inputEndpoint (descriptors device)) (outputEndpoint (descriptors device))
        attempt "staging disarm" (triggerWrite link TriggerDisarm)
        case snapshot of
          Nothing -> note s "restore-skipped" "no cache snapshot; nothing had been written"
          Just snap -> do
            forM_ (restorePlan snap) $ \(index,value) ->
              attempt ("restore 0x"++showHex (cacheIndexOf index) "") (cacheWrite link index value)
            attempt "gate restore readback" $ do
              payload <- windowRead link gateReadbackRestored
              writePrivateBytes (dir </> (gateReadbackRestored++".bin")) (B.pack payload)
              note s "readback-complete" (gateReadbackRestored++";sha256="++sha256 (B.pack payload))
              unless (gateByteOf payload == snapshotGate snap) (P.reject "gate restore is not visible in the config readback")
        attempt "protocol close" $ do
          _ <- tx s (P.controlTransfer (P.control 0xc4 2 sid)) 12
          _ <- tx s (P.BulkIn (inputEndpoint (descriptors device)) 2) 2
          note s "close-reply" "opaque bytes; stock restoration unobserved"
    releaseHeld held api (\label action -> attempt label (bounded s label action))
    failures <- readIORef errors
    done <- readIORef completed
    case (result,failures) of
      (_, _:_) -> P.reject ("cleanup uncertain: "++show failures)
      (Left (e::SomeException),[]) -> throwIO e
      (Right (),[]) -> note s "read-media-complete-stock-unverified" (show done++" slices") >> pure (mediaBankCount*blockBytes)

-- One action-37 request for one bank: the disarm precedes the staging fields
-- and makes a zero-length or repeated request at 0x80 inert, 0x81 is written
-- 0 which keeps the spare stride off, the erase dispatcher's case selector
-- outside 1..5 and the window at 0x100000 bytes, and only the bank's own
-- 0x20000-byte block is walked and captured.
captureBank :: MediaLink -> FilePath -> Int -> BankPlan -> IORef Int -> IO ()
captureBank link dir position plan completed = do
  let s = linkSession link
      capturePath = dir </> ("bank-"++show position++".bin")
  triggerWrite link TriggerDisarm
  cacheWrite link SpareGateIndex spareGateByte
  cacheWrite link BlockIndex (blockSelectorByte (bankSelector plan))
  cacheWrite link ResultIndex resultSentinelByte
  triggerWrite link TriggerArm
  result <- pollResult link position 0
  note s "result-terminal" ("bank="++show position++";result="++show result)
  unless (result == 0) (mediaRefuse s "result" [] ("the raw read did not complete: result code "++show result))
  bracket (openCapture capturePath) closeFd $ \fd -> do
    forM_ (zip [0::Int ..] (bankSlices (bankOffset plan))) $ \(index,(address,size)) -> do
      payload <- ramRead link ("slice-"++show position++"-"++show index) address size
      appendCapture fd payload
      time <- nowNS (sessionClock s)
      done <- modifyIORef' completed (+1) >> readIORef completed
      note s "slice-complete" ("bank="++show position++";index="++show index++";address=0x"++probeName address
                              ++";length="++show size++";sha256="++sha256 (B.pack payload)++";time="++show time)
      when (done `mod` mediaFsyncEvery == 0) (fileSynchronise fd)
    fileSynchronise fd
  captured <- B.readFile capturePath
  note s "bank-complete" ("bank="++show position++";bytes="++show (B.length captured)++";sha256="++sha256 captured)

-- The poll reads the cache window and accepts only the sentinel's transition
-- to a terminal result code, so a stale code from an earlier run cannot look
-- like completion and no staging byte is read before the handler is done.
pollResult :: MediaLink -> Int -> Int -> IO Int
pollResult link position count
  | count >= mediaPollCap = P.reject ("result code never left the sentinel after "++show mediaPollCap++" polls")
  | otherwise = do
      when (count > 0) (threadDelay mediaPollSpacingUs)
      payload <- windowRead link ("result-poll-"++show position++"-"++show count)
      let code = fromIntegral (payload !! cacheIndexOf ResultIndex) :: Int
      note (linkSession link) "result-poll" ("bank="++show position++";poll="++show count++";result="++show code)
      case code of
        0 -> pure 0
        1 -> pure 1
        2 -> pure 2
        0xff -> pollResult link position (count+1)
        other -> P.reject ("result byte left the sentinel to "++show other)

-- The capture file is created exclusively and appended per slice.
openCapture :: FilePath -> IO Fd
openCapture path = do
  fd <- openFd path WriteOnly defaultFileFlags {creat=Just 0o600,exclusive=True,nofollow=True,cloexec=True}
  status <- getFdStatus fd
  unless (isRegularFile status) (P.reject "capture must be regular")
  pure fd

appendCapture :: Fd -> [Word8] -> IO ()
appendCapture fd payload = B.useAsCStringLen (B.pack payload) $ \(buffer,size) -> do
  count <- fdWriteBuf fd (castPtr buffer) (fromIntegral size)
  unless (count == fromIntegral size) (P.reject "short capture write")
