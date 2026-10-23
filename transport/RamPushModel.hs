-- The in-process X-M1 camera the RAM-push operations run against: RAM, the
-- callback slot, the bootstrap window and the installer mailbox, answering
-- each transfer as the body does.
module RamPushModel where

import Control.Monad (forM_, unless, when)
import Data.Bits (shiftR)
import qualified Data.ByteString as B
import Data.IORef
import qualified Data.Map.Strict as Map
import Data.Word (Word8, Word32)
import FujiAF.RamPush
import FujiAF.SHA256 (sha256)
import FujiAF.Transport
import qualified FujiAF.Broker as P

check :: String -> Bool -> IO ()
check label yes = unless yes (error label)

word32le :: Word32 -> [Word8]
word32le value = [fromIntegral (value `shiftR` shift) | shift <- [0,8,16,24]]

u32 :: Int -> [Word8] -> Word32
u32 offset bytes = sum [fromIntegral byte * 256^index | (index,byte) <- zip [0..3::Int] (take 4 (drop offset bytes))]

base :: Word32
base = 0x00a80000

mailboxAddress :: Word32
mailboxAddress = base + 0xa280

publicationOrder :: [Int]
publicationOrder = [0..35] ++ [55..68] ++ [88] ++ [36..53] ++ [97..252]
  ++ [75,76,72,73,74,69,70,71,77,78,79,80,81,82,83,89,90,91,92,93,94,95,96,84,85,86,87,54]

publications :: [Publication]
publications =
  [ Publication logical (if position < setupCount then Setup else Activation)
      (Address (0x00630000 + fromIntegral (4*logical)))
      (WordValue (0xe1000000 + fromIntegral logical))
      (WordValue (0xea000000 + fromIntegral logical)) Both
  | (position,logical) <- zip [0::Int ..] publicationOrder ]

runtime :: B.ByteString
runtime = B.pack [fromIntegral (index `mod` 251) | index <- [0..reviewedAllocationSize-1]]

manifest :: RamPushManifest
manifest = RamPushManifest "/manifest.json" "/runtime.bin" (replicate 64 'a')
  [("menu/recipe_bank_hook.c",replicate 64 'b')] (Address base) (B.length runtime)
  (sha256 runtime) runtime (RamPushEntries (Address (base+0xa200)) (Address (base+0xa204)) (Address (base+0xa208))) publications

allocation :: RamAllocState
allocation = RamAllocState (Address base) reviewedAllocationSize 0x4003 callbackTableAddress callbackStockWord bootstrapBytes (sha256 (B.replicate reviewedAllocationSize 0)) "X-M1" 1

data Pending = Open | Close | Identity | Config | ReadMem Word32 Int | WriteMem Word32 [Word8] | Query Word32 deriving (Eq,Show)

data Model = Model
  { modelApi :: UsbApi
  , modelMemory :: IORef (Map.Map Word32 Word8)
  , modelHostWrites :: IORef [(Word32,[Word8])]
  , modelInternalWrites :: IORef [(Word32,Word32)]
  , modelFreed :: IORef Bool
  , modelIdentity :: IORef [Word8]
  , modelConfig :: IORef [Word8]
  , modelWriteReplies :: IORef Int
  , modelWriteFailures :: IORef [Int]
  , modelInstallStatus :: IORef Word32
  , modelSpinReport :: IORef (Word32,Word32,Word32)
  , modelUsbCalls :: IORef Int
  }

writeBytes :: IORef (Map.Map Word32 Word8) -> Word32 -> [Word8] -> IO ()
writeBytes memory address bytes = modifyIORef' memory $ \contents ->
  foldl (\updated (offset,byte) -> Map.insert (address+fromIntegral offset) byte updated) contents (zip [0::Int ..] bytes)

readBytes :: IORef (Map.Map Word32 Word8) -> Word32 -> Int -> IO [Word8]
readBytes memory address count = do
  contents <- readIORef memory
  pure [Map.findWithDefault 0 (address+fromIntegral offset) contents | offset <- [0..count-1]]

writeWord :: IORef (Map.Map Word32 Word8) -> Word32 -> Word32 -> IO ()
writeWord memory address value = writeBytes memory address (word32le value)

configReply :: [Word8]
configReply = concatMap word32le [0x1a0002,0,0,0,2,1,0,0,256]
  ++ [if index == 0xf7 then 1 else 0 | index <- [0..255::Int]]

identityReply :: [Word8]
identityReply = [0,0,0x15,0] ++ replicate 12 0 ++ [88,0,45,0,77,0,49,0] ++ replicate 104 0

-- | Spin report is (spin bound, CPU0 ACK-wait spins, CPU0 PARKED-wait spins).
mailbox :: Word32 -> Word32 -> Word32 -> (Word32,Word32,Word32) -> [Word8]
mailbox nonce operation status (bound,ackSpins,parkSpins) = concatMap word32le
  [0x31504d52,installerAbiVersion,nonce,operation,status,count publicationCount,if operation == 1 then count setupCount else 0,if operation == 1 then count activationCount else 0,if operation == 2 then count publicationCount else 0,0x4003,nonce,0x8003,0,count publicationCount,bound,ackSpins,nonce,nonce,nonce,parkSpins,41,0,0,0,0,0]
  where count = fromIntegral

measuredSpins :: (Word32,Word32,Word32)
measuredSpins = (0x80000,3,17)

newModel :: Maybe Int -> IO Model
newModel failWriteReply = do
  memory <- newIORef Map.empty
  writeWord memory (unAddress callbackTablePointerAddress) (unAddress callbackTableAddress)
  writeWord memory (unAddress callbackSlotAddress) (unWordValue callbackStockWord)
  forM_ publications $ \publication -> writeWord memory (unAddress (publicationAddress publication)) (unWordValue (publicationOld publication))
  pending <- newIORef Open
  writes <- newIORef []
  internal <- newIORef []
  freed <- newIORef False
  writeReplies <- newIORef (0::Int)
  writeFailures <- newIORef (maybe [] (:[]) failWriteReply)
  installStatus <- newIORef 0
  spinReport <- newIORef measuredSpins
  clock <- newIORef (0::Int)
  identity <- newIORef identityReply
  config <- newIORef configReply
  let descriptors' = Descriptors 1 0 0 255 0 0 0x81 2 512 512 0 1 []
      candidate = Candidate "001:2.3" 0x04cb 0xff80 descriptors'
      advance = modifyIORef' clock (+1)
      parse frame
        | frame == P.identityPacket = Identity
        | length frame == 36 && u32 0 frame == 0x1a0002 = Config
        | length frame == 36 && u32 0 frame == 0x200001 && u32 16 frame == 2 = ReadMem (u32 28 frame) (fromIntegral (u32 32 frame))
        | length frame >= 36 && u32 0 frame == 0x200001 && u32 16 frame == 3 =
            WriteMem (u32 28 frame) (take (fromIntegral (u32 32 frame)) (drop 36 frame))
        | length frame >= 32 && u32 0 frame == 0x450006 = Query (u32 16 frame)
        | otherwise = error ("unknown fake frame "++P.hexBytes frame)
      runQuery tag = do
        slot <- u32 0 <$> readBytes memory (unAddress callbackSlotAddress) 4
        if slot == unAddress bootstrapAddress then do
          bytes <- B.pack <$> readBytes memory (unAddress bootstrapAddress) 32
          if bytes == bootstrapBytes
            then do
              writeWord memory (unAddress queryOutputAddress) base
              writeWord memory (unAddress queryOutputAddress+4) 0x4003
            else if bytes == peerPrepareBytes
              then writeWord memory (unAddress queryOutputAddress) 0
              else when (bytes == freeBootstrapBytes) (writeIORef freed True)
        else if slot == unAddress (installEntry (manifestEntries manifest)) then do
          status <- readIORef installStatus
          spins <- readIORef spinReport
          writeBytes memory mailboxAddress (mailbox tag 1 status spins)
          writeWord memory (unAddress queryOutputAddress) mailboxAddress
          writeWord memory (unAddress queryOutputAddress+4) (fromIntegral mailboxBytes)
          when (status == 0) $ forM_ publications $ \publication -> do
            writeWord memory (unAddress (publicationAddress publication)) (unWordValue (publicationNew publication))
            modifyIORef' internal (++[(unAddress (publicationAddress publication),unWordValue (publicationNew publication))])
        else if slot == unAddress (rollbackEntry (manifestEntries manifest)) then do
          spins <- readIORef spinReport
          writeBytes memory mailboxAddress (mailbox tag 2 0 spins)
          writeWord memory (unAddress queryOutputAddress) mailboxAddress
          writeWord memory (unAddress queryOutputAddress+4) (fromIntegral mailboxBytes)
          forM_ (reverse publications) $ \publication -> do
            writeWord memory (unAddress (publicationAddress publication)) (unWordValue (publicationOld publication))
            modifyIORef' internal (++[(unAddress (publicationAddress publication),unWordValue (publicationOld publication))])
        else pure ()
        writeIORef pending (Query tag)
      response request count = case request of
        Open -> pure ([0x61,0x2b,0,0,0,0,1,0],False)
        Close -> pure ([0,0],False)
        Identity -> do bytes <- readIORef identity; pure (bytes,False)
        Config -> do bytes <- readIORef config; pure (bytes,False)
        ReadMem address bytes -> do
          check "X-M1 RAM read exceeds its 248-byte payload" (bytes <= 248)
          payload <- readBytes memory address bytes
          pure (concatMap word32le [0x200001,0,0,0,2,0,0,address,fromIntegral bytes]++payload,False)
        WriteMem address bytes -> do
          ordinal <- readIORef writeReplies
          failures <- readIORef writeFailures
          modifyIORef' writeReplies (+1)
          pure (concatMap word32le [0x200001,0,0,0,3,0,0,address,fromIntegral (length bytes)],ordinal `elem` failures)
        Query _tag -> do
          slot <- u32 0 <$> readBytes memory (unAddress callbackSlotAddress) 4
          if slot == unAddress (installEntry (manifestEntries manifest)) ||
             slot == unAddress (rollbackEntry (manifestEntries manifest))
            then pure (take 32 (concatMap word32le [0x00450006,0,0,0,0,fromIntegral mailboxBytes] ++ repeat 0),False)
          else if slot == unAddress bootstrapAddress
            then pure (take count (word32le 0x00450006 ++ word32le 1 ++ word32le 0xffff ++ repeat 0),False)
            else pure (replicate count 0,False)
      api = UsbApi (\_ -> advance) (\_ -> advance >> pure [candidate]) (\_ _ -> advance) (\_ _ -> advance)
        (\transfer _ -> do
          advance
          case transfer of
            P.ControlWrite _ _ _ _ bytes -> do
              case bytes of 0xc0:_ -> writeIORef pending Open; 0xc4:_ -> writeIORef pending Close; _ -> pure ()
              pure (12,[])
            P.BulkOut _ frame -> do
              if length frame >= 0xa4
                then writeBytes memory (unAddress bootstrapAddress) (take 32 (drop 0x84 frame))
                else writeBytes memory (unAddress bootstrapAddress) (replicate 32 0)
              let request = parse frame
              case request of
                WriteMem address bytes -> do
                  writeBytes memory address bytes
                  modifyIORef' writes (++[(address,bytes)])
                  writeIORef pending request
                Query tag -> runQuery tag
                _ -> writeIORef pending request
              pure (length frame,[])
            P.BulkIn _ count -> do
              request <- readIORef pending
              (bytes,failNow) <- response request count
              when failNow (P.reject "injected write-reply loss")
              check ("fake reply length for "++show request) (length bytes == count)
              pure (count,bytes))
        (\_ _ -> advance) (\_ -> advance) (\_ -> advance)
  pure (Model api memory writes internal freed identity config writeReplies writeFailures installStatus spinReport clock)
