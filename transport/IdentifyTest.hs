module Main where
import Control.Exception
import Control.Monad (unless, forM_, when)
import Data.IORef
import Data.List (isInfixOf)
import Data.Word (Word8, Word64)
import System.Directory
import System.FilePath
import System.Posix.Process (getProcessID)
import FujiAF.Transport
import FujiAF.Identify
import qualified FujiAF.Broker as P

check :: String -> Bool -> IO ()
check label yes = unless yes (error label)

syntheticDescriptors :: Descriptors
syntheticDescriptors = Descriptors 1 0 0 255 0 0 0x81 2 512 512 0 1 []

syntheticDevice :: Candidate
syntheticDevice = Candidate "001:2.3" 0x04cb 0xff80 syntheticDescriptors

identityReply :: [Word8]
identityReply = [0,0,0x15,0] ++ replicate 12 0 ++ [88,0,45,0,77,0,49,0] ++ replicate 104 0

-- The eight service transfers the operation must emit, in order.
literalPlan :: [P.Transfer]
literalPlan =
  [ P.ControlWrite 0x43 0 0 0 [0xc0,0,0,0,0,0,0,0,0,8,0,0]
  , P.BulkIn 0x81 8
  , P.ControlWrite 0x43 0 0 0 [0xd0,0,0,0,0,0,0,0,0,0x20,0x12,0x34]
  , P.BulkOut 2 ([0,0,0x15,0] ++ replicate 12 0 ++ [4,0,0,0] ++ replicate 12 0)
  , P.ControlWrite 0x43 0 0 0 [0xd1,0,0,0,0,0,0,0,0,0x80,0x12,0x34]
  , P.BulkIn 0x81 128
  , P.ControlWrite 0x43 0 0 0 [0xc4,0,0,0,0,0,0,0,0,2,0x12,0x34]
  , P.BulkIn 0x81 2 ]

successSteps :: [P.Step]
successSteps = zipWith P.Step literalPlan
  [ P.Transferred 12, P.Reply [0x34,0x12,0,0,0,0,1,0]
  , P.Transferred 12, P.Transferred 32
  , P.Transferred 12, P.Reply identityReply
  , P.Transferred 12, P.Reply [0,0] ]

data Observed = Observed
  { observedResult :: Either SomeException ProductModel
  , observedPackets :: [P.Transfer]
  , observedEvents :: [String]
  }

isClose :: P.Transfer -> Bool
isClose (P.ControlWrite _ _ _ _ (0xc4:_)) = True
isClose (P.BulkIn _ 2) = True
isClose _ = False

-- The close pair has its own script so a session stop cannot consume a
-- scripted window transfer in its place.
runCase :: FilePath -> [Candidate] -> [P.Step] -> Word64 -> Bool -> IO Observed
runCase dir inventory steps tickStep releaseFails = do
  createDirectoryIfMissing True dir
  events <- newIORef ([]::[String])
  emitted <- newIORef ([]::[P.Transfer])
  clockRef <- newIORef (0::Word64)
  script <- newIORef (take (length steps-2) steps)
  closeScript <- newIORef (drop (length steps-2) steps)
  let event s = modifyIORef' events (++[s])
      clock = Clock (readIORef clockRef)
      api = UsbApi
        (\_ -> event "init")
        (\_ -> event "list" >> pure inventory)
        (\_ _ -> event "open")
        (\_ _ -> event "claim")
        (\packet _ -> do
          event "transfer"
          modifyIORef' emitted (++[packet])
          modifyIORef' clockRef (+tickStep)
          let active = if isClose packet then closeScript else script
          remainingSteps <- readIORef active
          case remainingSteps of
            [] -> P.reject "script exhausted"
            P.Step expected outcome:rest -> do
              writeIORef active rest
              check ("packet differs from the audited sequence: "++show packet) (packet == expected)
              case outcome of
                P.Transferred n -> pure (n,[])
                P.Reply bytes -> pure (length bytes,bytes)
                P.LostReply -> P.reject "injected lost reply")
        (\_ _ -> event "release" >> when releaseFails (P.reject "release failed"))
        (\_ -> event "close")
        (\_ -> event "exit")
  result <- try (runIdentify clock api dir)
  Observed result <$> readIORef emitted <*> readIORef events

cleanupEvents :: Observed -> [String]
cleanupEvents = filter (`elem` ["release","close","exit"]) . observedEvents

replaceAt :: Int -> a -> [a] -> [a]
replaceAt at value values = take at values ++ [value] ++ drop (at+1) values

main :: IO ()
main = do
  process <- getProcessID
  temporary <- getTemporaryDirectory
  let base = temporary </> ("fuji-af-identify-tests-"++show process)
  createDirectory base

  check "a session with no eligible device is refused" (selectDevice [] == Left NoEligibleDevice)
  check "two eligible devices are refused" (selectDevice [syntheticDevice,syntheticDevice] == Left (MultipleEligibleDevices 2))
  check "an oversized inventory is refused" (selectDevice (replicate 65 syntheticDevice) == Left (InventoryCapExceeded 65))
  check "a foreign vendor is not eligible" (selectDevice [syntheticDevice{vid=0x1d6b,pid=2}] == Left NoEligibleDevice)
  check "the single eligible device is selected among others"
    (selectDevice [syntheticDevice{vid=0x1d6b,pid=2},syntheticDevice] == Right syntheticDevice)
  let relocated = syntheticDevice{descriptors=syntheticDescriptors{inputEndpoint=0x82,outputEndpoint=1}}
  check "endpoints come from the live descriptors" (selectDevice [relocated] == Right relocated)
  forM_ [ syntheticDescriptors{interfaceNumber=1}
        , syntheticDescriptors{alternateSetting=1}
        , syntheticDescriptors{inputEndpoint=2}
        , syntheticDescriptors{outputEndpoint=0x81}
        , syntheticDescriptors{inputPacketSize=0}
        , syntheticDescriptors{configuration=0}
        , syntheticDescriptors{extraEndpoints=[(0x83,2,16,8)]}
        , syntheticDescriptors{extraEndpoints=replicate 7 (0x83,3,16,8)} ] $ \profile ->
    check ("unsupported descriptor profile refused: "++show profile)
      (selectDevice [syntheticDevice{descriptors=profile}] == Left (UnsupportedProfile profile))

  success <- runCase (base </> "success") [syntheticDevice] successSteps 0 False
  check "the identity query returns the decoded model"
    (either (const False) (==ProductModel "X-M1") (observedResult success))
  check "exactly the audited eight transfers are emitted" (observedPackets success == literalPlan)
  check "every host resource is released once" (cleanupEvents success == ["release","close","exit"])
  outgoing <- readFile (base </> "success" </> "outgoing.v1")
  check "every outgoing frame is durable before dispatch"
    (all (\p -> case p of
            P.ControlWrite _ _ _ _ bytes -> P.hexBytes bytes `isInfixOf` outgoing
            P.BulkOut _ bytes -> P.hexBytes bytes `isInfixOf` outgoing
            P.BulkIn _ _ -> True) literalPlan)
  journal <- readFile (base </> "success" </> "journal.v1")
  check "the raw identity reply is journaled" (P.hexBytes identityReply `isInfixOf` journal)

  opaque <- runCase (base </> "opaque-open") [syntheticDevice]
    (replaceAt 1 (P.Step (P.BulkIn 0x81 8) (P.Reply [0x34,0x12,0xa5,0x5a,0,0,1,0])) successSteps) 0 False
  check "unknown C0 bytes are carried without interpretation"
    (either (const False) (==ProductModel "X-M1") (observedResult opaque))

  -- (label, script, transfers emitted before the stop, close attempted)
  let stops =
        [ ("short identity reply", replaceAt 5 (P.Step (P.BulkIn 0x81 128) (P.Reply [0])) successSteps, 6, True)
        , ("wrong identity opcode", replaceAt 5 (P.Step (P.BulkIn 0x81 128) (P.Reply (1:drop 1 identityReply))) successSteps, 6, True)
        , ("nonzero identity status", replaceAt 5 (P.Step (P.BulkIn 0x81 128) (P.Reply (take 4 identityReply ++ [1,0,0,0] ++ drop 8 identityReply))) successSteps, 6, True)
        , ("other product string", replaceAt 5 (P.Step (P.BulkIn 0x81 128) (P.Reply (take 16 identityReply ++ [88,0,45,0,69,0,50,0] ++ replicate 104 0))) successSteps, 6, True)
        , ("lost identity reply", replaceAt 5 (P.Step (P.BulkIn 0x81 128) P.LostReply) successSteps, 6, True)
        , ("partial control write", replaceAt 0 (P.Step (literalPlan !! 0) (P.Transferred 4)) successSteps, 1, False)
        , ("short C0 reply", replaceAt 1 (P.Step (P.BulkIn 0x81 8) (P.Reply (replicate 7 0))) successSteps, 2, False)
        , ("session tail off the constant", replaceAt 1 (P.Step (P.BulkIn 0x81 8) (P.Reply [0x34,0x12,0,0,0,0,0,127])) successSteps, 2, True) ]
  forM_ (zip [(0::Int)..] stops) $ \(index,(label,steps,dispatched,closes)) -> do
    observed <- runCase (base </> ("stop-"++show index)) [syntheticDevice] steps 0 False
    check (label++" stops the session") (either (const True) (const False) (observedResult observed))
    check (label++" emits the planned prefix and at most one close, without a retry")
      (observedPackets observed == take dispatched literalPlan ++ (if closes then drop 6 literalPlan else []))
    check (label++" releases every resource once") (cleanupEvents observed == ["release","close","exit"])

  late <- runCase (base </> "late") [syntheticDevice] successSteps 5100000000 False
  check "a transfer beyond its budget stops the session"
    (either (const True) (const False) (observedResult late))
  check "a late transfer still releases every resource once" (cleanupEvents late == ["release","close","exit"])

  refused <- runCase (base </> "no-device") [] successSteps 0 False
  check "a session without the camera stops before any transfer"
    (either (const True) (const False) (observedResult refused) && null (observedPackets refused))
  check "a refused selection still exits the context" (cleanupEvents refused == ["exit"])

  releaseFailure <- runCase (base </> "release-failure") [syntheticDevice] successSteps 0 True
  check "a failed release makes the outcome uncertain"
    (either (isInfixOf "cleanup uncertain" . displayException) (const False) (observedResult releaseFailure))

  let journalFailure = base </> "journal-failure"
  createDirectoryIfMissing True journalFailure
  createDirectory (journalFailure </> "outgoing.v1")
  blocked <- runCase journalFailure [syntheticDevice] successSteps 0 False
  check "an unwritable journal stops before the first dispatch"
    (either (const True) (const False) (observedResult blocked) && null (observedPackets blocked))

  removeDirectoryRecursive base
  putStrLn "identify: emitted bytes, device selection, stop-on-mismatch, no-retry, journal-before-dispatch, deadline and cleanup checks passed"
