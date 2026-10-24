module Main where

import Control.Exception
import Control.Monad (forM_, unless, when)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Bits (shiftR, xor, (.&.))
import Data.IORef
import Data.List (isInfixOf)
import Data.Word (Word8, Word64)
import System.Directory
import System.FilePath
import System.Posix.Process (getProcessID)
import FujiAF.Transport
import FujiAF.ReadBulk
import qualified FujiAF.Broker as P

check :: String -> Bool -> IO ()
check label yes = unless yes (error label)

le32 :: Int -> [Word8]
le32 value = [fromIntegral ((value `shiftR` (8*i)) .&. 0xff) | i <- [0..3]]

be32 :: Int -> [Word8]
be32 value = reverse (le32 value)

u32 :: Int -> [Word8] -> Int
u32 at bytes = sum [fromIntegral byte * 256^index | (index,byte) <- zip [0..3::Int] (take 4 (drop at bytes))]

hex :: [Word8] -> String
hex = P.hexBytes

-- Wire shapes written out from the documented layouts.
sid :: Int
sid = 0x2b61

control :: Word8 -> Int -> P.Transfer
control op size = P.ControlWrite 0x43 0 0 0
  [op,0,0,0,0,0,0,0, fromIntegral (size `div` 256), fromIntegral (size `mod` 256), fromIntegral (sid `div` 256), fromIntegral (sid `mod` 256)]

identityFrame, window0Frame, queryFrame :: [Word8]
identityFrame = [0,0,0x15,0] ++ replicate 12 0 ++ [4,0,0,0] ++ replicate 12 0
window0Frame = [0x02,0x00,0x1a,0x00] ++ replicate 12 0 ++ [0x02,0,0,0, 0x01,0,0,0, 0,0,0,0, 0,0,0,0, 0x00,0x01,0,0]
queryFrame = [0x06,0x00,0x45,0x00] ++ replicate 12 0 ++ [0x80,0,0,0] ++ replicate 12 0

ramFrame :: Int -> Int -> [Word8]
ramFrame address size = [0x01,0x00,0x20,0x00] ++ replicate 12 0 ++ [0x02,0,0,0] ++ replicate 8 0 ++ le32 address ++ le32 size

-- E1: command byte, zero, big-endian offset, big-endian length, big-endian session.
e1Frame :: Int -> Int -> [Word8]
e1Frame offset size = [0xe1,0] ++ be32 offset ++ be32 size ++ [fromIntegral (sid `div` 256), fromIntegral (sid `mod` 256)]

memoryByte :: Int -> Word8
memoryByte address = fromIntegral ((address `xor` (address `shiftR` 9) `xor` (address `shiftR` 19)) .&. 0xff)

-- The synthetic body: the query publishes pointer and capacity, E1 serves
-- min(length, capacity) bytes from pointer + offset (mod 2^32).
data Fake = Fake
  { fakeGate :: Word8
  , fakePointer :: Int
  , fakeCapacity :: Int
  , fakeQueryStatus :: (Int,Int)
  , fakeShortChunk :: Maybe Int
  , fakeJournal :: Maybe FilePath
  }

fake :: Fake
fake = Fake 1 0x4b2ece00 0x100000 (0,0) Nothing Nothing

data Pending = NoReply | Reply [Word8] | AwaitOut Int

-- The device and the log of every transfer it received.
fakeApi :: Fake -> IO (UsbApi, IORef [P.Transfer])
fakeApi f = do
  seen <- newIORef []
  pending <- newIORef NoReply
  request <- newIORef []
  published <- newIORef Nothing
  bulkCount <- newIORef (0::Int)
  let transfer packet _ = do
        modifyIORef' seen (++[packet])
        -- Every outgoing frame is durable before it reaches the device.
        forM_ (fakeJournal f) $ \dir -> case packet of
          P.ControlWrite _ _ _ _ frame -> journaled dir frame
          P.BulkOut _ frame -> journaled dir frame
          _ -> pure ()
        case packet of
          P.ControlWrite 0x43 0 0 0 frame@(op:_) -> do
            case op of
              0xc0 -> writeIORef pending (Reply ([fromIntegral (sid `mod` 256), fromIntegral (sid `div` 256),0,0,0,0,1,0]))
              0xd0 -> writeIORef pending (AwaitOut (fromIntegral (frame!!8)*256 + fromIntegral (frame!!9)))
              0xd1 -> readIORef request >>= d1 >>= writeIORef pending . Reply
              0xe1 -> do
                source <- readIORef published
                case source of
                  Nothing -> writeIORef pending (Reply [])
                  Just (pointer,capacity) -> do
                    let offset = foldl (\a b -> a*256 + fromIntegral b) 0 (take 4 (drop 2 frame))
                        size = foldl (\a b -> a*256 + fromIntegral b) 0 (take 4 (drop 6 frame))
                        start = (pointer + offset) .&. 0xffffffff
                    index <- atomicModifyIORef' bulkCount (\n -> (n+1,n))
                    let served = if fakeShortChunk f == Just index then min size capacity - 64 else min size capacity
                    writeIORef pending (Reply [memoryByte ((start + i) .&. 0xffffffff) | i <- [0 .. served-1]])
              0xc4 -> writeIORef pending (Reply [0,0])
              _ -> throwIO (userError "fake: unknown control")
            pure (12,[])
          P.BulkOut _ bytes -> do
            writeIORef request bytes
            writeIORef pending NoReply
            pure (length bytes,[])
          P.BulkIn _ count -> do
            state <- readIORef pending
            writeIORef pending NoReply
            case state of
              Reply bytes -> let sent = take count bytes in pure (length sent, sent)
              _ -> pure (0,[])
          _ -> throwIO (userError "fake: unsupported transfer")
      d1 frame
        | frame == identityFrame = pure ([0,0,0x15,0] ++ replicate 12 0 ++ [88,0,45,0,77,0,49,0] ++ replicate 104 0)
        | frame == window0Frame = pure (le32 0x1a0002 ++ le32 0 ++ le32 0 ++ le32 0 ++ le32 2 ++ le32 1 ++ le32 0 ++ le32 0 ++ le32 256
                                         ++ [if i == 0xf7 then fakeGate f else 0x5a | i <- [0..255::Int]])
        | frame == queryFrame = do
            let (e1,e2) = fakeQueryStatus f
            when ((e1,e2) == (0,0)) (writeIORef published (Just (fakePointer f, fakeCapacity f)))
            pure (le32 0x450006 ++ le32 e1 ++ le32 e2 ++ le32 0 ++ le32 0 ++ le32 (if (e1,e2) == (0,0) then fakeCapacity f else 0) ++ le32 0 ++ le32 0)
        | take 4 frame == [0x01,0x00,0x20,0x00] = do
            source <- readIORef published
            let address = u32 0x1c frame
                size = u32 0x20 frame
                state = maybe (replicate 8 0) (\(p,c) -> le32 p ++ le32 c) source
                byteAt a | a >= 0x01469878 && a < 0x01469880 = state !! (a - 0x01469878)
                         | otherwise = memoryByte a
            pure (le32 0x200001 ++ le32 0 ++ le32 0 ++ le32 0 ++ le32 2 ++ le32 0 ++ le32 0 ++ le32 address ++ le32 size ++ map byteAt [address .. address+size-1])
        | otherwise = throwIO (userError ("fake: unknown request "++hex frame))
      journaled dir frame = do
        outgoing <- B.readFile (dir </> "outgoing.v1")
        unless (C.pack (hex frame) `B.isInfixOf` outgoing) (throwIO (userError ("frame sent before it was journaled: "++hex frame)))
  pure (UsbApi (\_ -> pure ()) (\_ -> pure [Candidate "001:2" 0x04cb 0xff80 (Descriptors 1 0 0 255 0 0 0x81 2 512 512 0 1 [])])
                (\_ _ -> pure ()) (\_ _ -> pure ()) transfer (\_ _ -> pure ()) (\_ -> pure ()) (\_ -> pure ()), seen)

steppingClock :: IO Clock
steppingClock = do
  now <- newIORef (1000000000 :: Word64)
  pure (TimedClock (atomicModifyIORef' now (\t -> (t+1000,t))) (\ms -> modifyIORef' now (+ fromIntegral ms*1000000)))

window :: Int -> Int -> BulkWindow
window address size = either error id (mkBulkWindow address size)

main :: IO ()
main = do
  pid <- getProcessID
  root <- (</> ("fuji-af-read-bulk-test-"++show pid)) <$> getTemporaryDirectory
  createDirectory root
  counter <- newIORef (0::Int)
  let session = do
        n <- atomicModifyIORef' counter (\k -> (k+1,k))
        let dir = root </> show n
        ensurePrivateDirectory dir
        pure dir
      runOn f w = do
        dir <- session
        (api,seen) <- fakeApi f { fakeJournal = Just dir }
        clock <- steppingClock
        outcome <- try (runReadBulk w clock api dir)
        transfers <- readIORef seen
        pure (dir, outcome :: Either SomeException ReadBulkReport, transfers)
      e1s transfers = [frame | P.ControlWrite _ _ _ _ frame@(0xe1:_) <- transfers]
      closed transfers = control 0xc4 2 `elem` transfers

  -- Window validation.
  check "accepts the image-5 window" (either (const False) (const True) (mkBulkWindow 0x00bf1a00 0x400000))
  check "accepts the alias range end" (either (const False) (const True) (mkBulkWindow 0x5ff00000 0x100000))
  check "accepts the DRAM range end" (either (const False) (const True) (mkBulkWindow 0x1fffffc0 0x40))
  forM_ [ (0x00bf1a02, 0x100, "unaligned address"), (0x00bf1a00, 0x104, "length not a 64-byte multiple")
        , (0x00bf1a00, 0, "empty window"), (0x1fffff00, 0x200, "crosses the DRAM end")
        , (0x3fffff00, 0x100, "between the ranges"), (0x80000000, 0x100, "boot ROM")
        , (0xfff50000, 0x40, "peripheral"), (0x00000000, 0x20000040, "above the length cap") ] $ \(a,l,label) ->
    check ("refuses "++label) (either (const True) (const False) (mkBulkWindow a l))

  -- The CLI shape.
  check "parses ADDRESS LENGTH" (readBulkArguments ["0x00bf1a00","0x400000"] == Just (0x00bf1a00,0x400000,Nothing))
  check "parses an OUTDIR" (readBulkArguments ["0x0","0x40","/tmp/x"] == Just (0,0x40,Just "/tmp/x"))
  forM_ [["0x0"],["12","0x40"],["0x0","64"],["0x0","0x40","--x"],["0x0","0x40","/a","/b"],["0x123456789","0x40"]] $ \tokens ->
    check ("refuses arguments "++show tokens) (readBulkArguments tokens == Nothing)

  -- Transport shapes: bulk IN up to one chunk, every other transfer 292.
  check "admits a chunk-sized bulk IN" (supportedTransfer (P.BulkIn 0x81 0x100000))
  check "refuses a larger bulk IN" (not (supportedTransfer (P.BulkIn 0x81 0x100001)))
  check "refuses a 293-byte bulk OUT" (not (supportedTransfer (P.BulkOut 2 (replicate 293 0))))

  -- A two-and-a-half-chunk window: every byte from its own address.
  let span' = window 0x00bf1a00 0x280000
  (dir,outcome,transfers) <- runOn fake span'
  report <- either throwIO pure outcome
  captured <- B.readFile (dir </> "ram-00bf1a00-00280000.bin")
  check "the capture holds the window's bytes" (captured == B.pack [memoryByte a | a <- [0x00bf1a00 .. 0x00bf1a00+0x280000-1]])
  check "three chunks reported" (reportChunks report == 3 && reportBytes report == 0x280000 && reportCapacity report == 0x100000)
  check "one E1 per chunk at offsets from 0x4b2ece00"
    (e1s transfers == [ e1Frame ((a - 0x4b2ece00) .&. 0xffffffff) l | (a,l) <- [(0x00bf1a00,0x100000),(0x00cf1a00,0x100000),(0x00df1a00,0x80000)] ])
  check "bulk OUT frames are the identity, window 0, the query and the source read only"
    ([frame | P.BulkOut _ frame <- transfers] == [identityFrame, window0Frame, queryFrame, ramFrame 0x01469878 8])
  check "the session closes" (closed transfers)
  journal <- readFile (dir </> "journal.v1")
  check "each chunk is journaled with its digest" (length (filter ("chunk-complete" `isInfixOf`) (lines journal)) == 3)

  -- The alias range reads through the same offset arithmetic.
  (aliasDir,aliasOutcome,_) <- runOn fake (window 0x40bf1a00 0x1000)
  _ <- either throwIO pure aliasOutcome
  aliasCaptured <- B.readFile (aliasDir </> "ram-40bf1a00-00001000.bin")
  check "the alias capture holds its addresses' bytes" (aliasCaptured == B.pack [memoryByte a | a <- [0x40bf1a00 .. 0x40bf1a00+0x1000-1]])

  -- The 0x108000 capacity is accepted; chunks stay 1 MiB.
  (_,wideOutcome,wideTransfers) <- runOn fake { fakeCapacity = 0x108000 } (window 0x00100000 0x180000)
  _ <- either throwIO pure wideOutcome
  check "1 MiB chunks under the 0x108000 capacity" (map (\f -> u32 0 (reverse (take 4 (drop 6 f)))) (e1s wideTransfers) == [0x100000,0x80000])

  -- Stops: nothing past the failing step, no retry, C4 still sent.
  let stopsBeforeE1 label f = do
        (_,o,t) <- runOn f span'
        check (label++" stops the run") (either (const True) (const False) o)
        check (label++" sends no E1") (null (e1s t))
        check (label++" still closes") (closed t)
        pure t
  closedGate <- stopsBeforeE1 "a closed gate" fake { fakeGate = 0 }
  check "a closed gate sends no query" (queryFrame `notElem` [frame | P.BulkOut _ frame <- closedGate])
  _ <- stopsBeforeE1 "another source pointer" fake { fakePointer = 0x42e40000 }
  _ <- stopsBeforeE1 "an unexpected capacity" fake { fakeCapacity = 0x80000 }
  _ <- stopsBeforeE1 "a refused query" fake { fakeQueryStatus = (1,0xffff) }
  (_,shortOutcome,shortTransfers) <- runOn fake { fakeShortChunk = Just 1 } span'
  check "a short chunk stops the run" (either (const True) (const False) shortOutcome)
  check "a short chunk is not retried and no later chunk is sent" (length (e1s shortTransfers) == 2)
  check "a short chunk still closes" (closed shortTransfers)

  removeDirectoryRecursive root
  putStrLn "read-bulk tests: ok"
