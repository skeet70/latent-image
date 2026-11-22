{-# LANGUAGE OverloadedStrings #-}

-- | The volume-worker cases: an independent model of the stock attach (0x2c00), detach
-- (0x2c01), armed format (0x2c11) and format (0x2c12) workers, read from the bytes of
-- [0x7120d8,0x712700), gives each case's result, the ordered calls into the retained volume,
-- format, level and flagged-media services and the callback, and the final payload,
-- media-state words and context.
module Replacement.FileService.Volume
  ( code
  , scenarios
  ) where

import Control.Monad (forM_, when)
import Data.Bits (complement, shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32, Word8)
import Replacement.FileService.Native

code :: Code
code = Code
  { codeImageBase = imageBase
  , codeExtents =
      [ ("attach-code", Extent 0x7120d8 0x7122dc)
      , ("detach-code", Extent 0x7122dc 0x7123b4)
      , ("format-armed-code", Extent 0x7123b4 0x712580)
      , ("format-code", Extent 0x712580 0x712700)
      ]
  , codeStockFunctions = [(0x7120d8, 0x204), (0x7122dc, 0xd8), (0x7123b4, 0x1cc), (0x712580, 0x180)]
  , codeRetained = [("fill-memory", Extent 0x70e664 0x70e6c4), ("copy-memory", Extent 0x70e6c4 0x70e748)]
  , codeRetainedFunctions = [(0x70e664, 0x60), (0x70e6c4, 0x84)]
  }

imageBase :: Word32
imageBase = 0x61c400

-- Addresses ------------------------------------------------------------------

attachWorker, detachWorker, formatArmedWorker, formatWorker :: Word32
attachWorker = 0x7120d8
detachWorker = 0x7122dc
formatArmedWorker = 0x7123b4
formatWorker = 0x712580

deviceForLetter, volumeRecord, volumeRelease, volumeSwitch, volumeOther :: Word32
deviceForLetter = 0x70ec84
volumeRecord = 0x71f4b8
volumeRelease = 0x71f4d8
volumeSwitch = 0x71f418
volumeOther = 0x71f4f8

volumeAttach, volumeReattach, volumeDetach, volumeFormat, volumeReformat :: Word32
volumeAttach = 0x71f7dc
volumeReattach = 0x71fcbc
volumeDetach = 0x71fa9c
volumeFormat = 0x71e100
volumeReformat = 0x71e1bc

snapshot, setLevel, flaggedPrepare, catalogRebuild, flaggedSelect, noVolumeStatus :: Word32
snapshot = 0x712b4c
setLevel = 0x6fae80
flaggedPrepare = 0x6f535c
catalogRebuild = 0x719de0
flaggedSelect = 0x6f5330
noVolumeStatus = 0x6ed770

callbackStub, mediaState, attachState, defaults, contextBase, tailBase, requestBase, payloadBase :: Word32
callbackStub = 0x380000
mediaState = 0x910204
attachState = 0x910208
defaults = 0x814558
contextBase = 0xa00000
tailBase = contextBase + 0x16bd8
requestBase = 0xc10000
payloadBase = requestBase + 0x10

contextBytes, tailBytes, defaultsBytes :: Int
contextBytes = 0x3e00
tailBytes = 0x98
defaultsBytes = 216

workerTable :: Word32
workerTable = 0x814740

-- Model ------------------------------------------------------------------------

-- | Scripted answers of the retained services and the initial media-state words.
data Env = Env
  { envVolume :: Word32
  , envContext :: Word32
  , envAttachState :: Word32
  , envRecords :: [Word32]
  , envOthers :: [Word32]
  , envStatus :: Word32
  }

env :: Env
env = Env
  { envVolume = 1
  , envContext = contextBase
  , envAttachState = 0
  , envRecords = [0, 0]
  , envOthers = []
  , envStatus = 0
  }

-- | Guest memory by byte, the answers 0x71f4f8 has left and the calls made.
data Machine = Machine {mMemory :: M.Map Word32 Word8, mOthers :: [Word32], mServices :: [Service]}

newtype S a = S {runS :: Env -> Machine -> (a, Machine)}

instance Functor S where
  fmap f (S g) = S (\e m -> let (a, m') = g e m in (f a, m'))

instance Applicative S where
  pure a = S (\_ m -> (a, m))
  S f <*> S g = S (\e m -> let (h, m') = f e m; (a, m'') = g e m' in (h a, m''))

instance Monad S where
  S g >>= k = S (\e m -> let (a, m') = g e m in runS (k a) e m')

ask :: S Env
ask = S (\e m -> (e, m))

gets :: (Machine -> a) -> S a
gets f = S (\_ m -> (f m, m))

modify :: (Machine -> Machine) -> S ()
modify f = S (\_ m -> ((), f m))

made :: Service -> S ()
made service = modify (\m -> m {mServices = mServices m ++ [service]})

call :: Word32 -> [(String, Word32)] -> Word32 -> S Word32
call address inputs result = made (Service address inputs [("r0", result)] [] []) >> pure result

load :: Word32 -> Int -> S Word32
load address count = gets (\m -> sum [fromIntegral (M.findWithDefault 0 (address + fromIntegral i) (mMemory m)) `shiftL` (8 * i) | i <- [0 .. count - 1]])

rd32, rd8 :: Word32 -> S Word32
rd32 address = load address 4
rd8 address = load address 1

put :: Word32 -> Int -> Word32 -> S ()
put address count value =
  modify (\m -> m {mMemory = foldr (\i -> M.insert (address + fromIntegral i) (fromIntegral (value `shiftR` (8 * i)))) (mMemory m) [0 .. count - 1]})

wr32, wr16, wr8 :: Word32 -> Word32 -> S ()
wr32 address = put address 4
wr16 address = put address 2
wr8 address = put address 1

bytesAt :: Word32 -> Int -> S B.ByteString
bytesAt address count = gets (\m -> B.pack [M.findWithDefault 0 (address + fromIntegral i) (mMemory m) | i <- [0 .. count - 1]])

context :: S Word32
context = rd32 mediaState

callback :: Word32 -> Word32 -> S ()
callback 0 _ = pure ()
callback function value = () <$ call function [("r0", value)] 0

-- | The request header and payload 0x71f7dc, 0x71fcbc and 0x71fa9c receive in r0.
requestReads :: Int -> S [MemoryRow]
requestReads payloadSize = do
  header <- bytesAt requestBase 12
  payload <- bytesAt payloadBase payloadSize
  pure [AtRegister "r0" 0 header, AtIndirect "r0" 8 payload]

withRequest :: Word32 -> Int -> Word32 -> [MemoryRow] -> S Word32
withRequest address payloadSize result writes = do
  rows <- requestReads payloadSize
  made (Service address [("r0", requestBase)] [("r0", result)] rows writes)
  pure result

level :: S ()
level = () <$ call setLevel [("r0", 0), ("r1", 10)] 0

release :: Word32 -> S ()
release volume = () <$ call volumeRelease [("r0", volume)] (0x7000 + volume)

attachPayload :: Int
attachPayload = 0x44

-- | 0x7120d8; it returns 0.
attachW :: S Word32
attachW = do
  let p = payloadBase
  function <- rd32 p
  letter <- rd8 (p + 0x38)
  e <- ask
  volume <- call deviceForLetter [("r0", letter)] (envVolume e)
  if volume == 0xffffffff
    then callback function volume >> level >> pure 0
    else do
      present <- context
      attached <-
        if present == 0
          then pure False
          else do
            forM_ (zip [0 ..] (envRecords e)) $ \(slot, record) -> do
              _ <- call volumeRecord [("r0", slot)] record
              when (record /= 0) $ do
                descriptor <- rd32 (p + 4)
                when ((descriptor == record && slot /= volume) || (descriptor /= record && slot == volume)) (release slot)
            descriptor <- rd32 (p + 4)
            current <- context
            let forget = wr32 mediaState 0 >> release volume
            if descriptor == current
              then do
                owner <- rd32 (descriptor + 0x114)
                when (owner /= volume) forget
              else do
                owner <- rd32 (current + 0x114)
                if owner == volume then forget else () <$ call volumeSwitch [("r0", owner), ("r1", volume)] (0x8000 + volume)
            now <- context
            state <- rd32 attachState
            mark <- rd32 (now + 0x2210)
            pure (now /= 0 && state /= 0xffffffff && mark == 1)
      residue <-
        if attached
          then withRequest volumeReattach attachPayload 0x6200 []
          else do
            flagged <- rd32 (p + 0x3c)
            when (flagged == 1) (() <$ call flaggedPrepare [] 0)
            wr32 p 0
            _ <- withRequest volumeAttach attachPayload 0x6100 [AtAddress mediaState 0 (word32le contextBase)]
            wr32 mediaState contextBase
            destination <- (+ 0x118) <$> context
            forM_ [0 .. fromIntegral defaultsBytes - 1] $ \i -> rd8 (defaults + i) >>= wr8 (destination + i)
            now <- context
            wr32 (now + 0x1f0) 9
            pure (destination + fromIntegral defaultsBytes)
      flagged <- rd32 (p + 0x3c)
      final <-
        if flagged /= 1
          then pure residue
          else do
            now <- context
            flags <- rd32 (now + 0x16bd8)
            if flags .&. complement 0x10 == 0
              then call catalogRebuild [("r0", 0), ("r1", 0)] 0x6400
              else call flaggedSelect [("r0", 0), ("r1", 1)] 0x6500
      callback function final
      level
      pure 0

-- | 0x7122dc; it returns 0.
detachW :: S Word32
detachW = do
  let p = payloadBase
  function <- rd32 p
  wr32 p 0
  repeating <- rd32 (p + 4)
  initial <- call noVolumeStatus [] 0xffffff01
  present <- context
  let step = do
        now <- context
        volume <- rd32 (now + 0x114)
        when (now /= 0) (wr8 (now + 0x16c6a) 0 >> wr8 (now + 0x16c6b) 0)
        _ <- withRequest volumeDetach 8 0x6300 []
        release volume
        other <- gets (\m -> case mOthers m of answer : _ -> answer; [] -> error "a case scripts too few 0x71f4f8 answers")
        modify (\m -> m {mOthers = drop 1 (mOthers m)})
        _ <- call volumeOther [("r0", volume)] other
        if other == 0xffffffff
          then pure other
          else do
            residue <- call volumeSwitch [("r0", volume), ("r1", other)] (0x8000 + other)
            after <- context
            wr32 (after + 0x114) other
            if repeating /= 0 then step else pure residue
  residue <- if present == 0 then pure initial else step
  wr32 p function
  callback function residue
  pure 0

-- | 0x7123b4 (armed, through 0x71e100) and 0x712580 (through 0x71e1bc); each returns its status.
formatW :: Bool -> S Word32
formatW armed = do
  let p = payloadBase
  function <- rd32 p
  wr32 p 0
  letter <- rd8 (p + 4)
  e <- ask
  volume <- call deviceForLetter [("r0", letter)] (envVolume e)
  now <- context
  status <-
    if now == 0 || volume == 0xffffffff
      then pure 5
      else do
        wr8 (now + 0x16c6a) 0
        wr8 (now + 0x16c6b) 0
        status <- call (if armed then volumeFormat else volumeReformat) [("r0", requestBase), ("r1", volume)] (envStatus e)
        when (status == 0) (reset armed)
        pure status
  callback function status
  pure status

reset :: Bool -> S ()
reset armed = do
  c <- context
  forM_ [0x1fc, 0x2168, 0x216c, 0x2148, 0x214c, 0x2150, 0x2154, 0x2158, 0x215c, 0x2164, 0x2160] $ \offset -> wr32 (c + offset) 0
  forM_ [0 .. 1999] $ \i -> wr8 (c + 0x35a4 + i) 0
  c' <- context
  forM_ [0 .. 3999] $ \i -> wr8 (c' + 0x2604 + i) 0
  c'' <- context
  forM_ [(0x21bc, 0), (0x21c0, 0), (0x2180, 100), (0x2184, 100), (0x2168, 100), (0x2178, 1000), (0x217c, 1000), (0x218c, 1001), (0x2190, 1001)] $ \(offset, value) ->
    wr32 (c'' + offset) value
  when armed $ do
    wr32 (c'' + 0x2214) 0
    count <- rd8 (c'' + 0x2600)
    forM_ (takeWhile (< count) [0 ..]) $ \i -> do
      let record = c'' + 0x21d0 + 16 * i
      wr32 record 0
      forM_ [4, 6, 8] $ \half -> wr16 (record + half) 0
  _ <- call snapshot [] 0
  done <- context
  wr32 (done + 0x21cc) 2

-- Cases ------------------------------------------------------------------------------

data Worker = Worker {workerEntry :: Word32, workerCode :: Word32, workerPayload :: Int, workerModel :: S Word32}

attachWorkerW, detachWorkerW, formatArmedWorkerW, formatWorkerW :: Worker
attachWorkerW = Worker attachWorker 0x2c00 attachPayload attachW
detachWorkerW = Worker detachWorker 0x2c01 8 detachW
formatArmedWorkerW = Worker formatArmedWorker 0x2c11 8 (formatW True)
formatWorkerW = Worker formatWorker 0x2c12 8 (formatW False)

patterned :: Word32 -> Int -> B.ByteString
patterned seed count = B.pack [fromIntegral (seed + 13 * fromIntegral i) | i <- [0 .. count - 1]]

-- | One case: payload words, then context words over a patterned context.
workerCase :: B.ByteString -> String -> Worker -> Env -> [(Int, Word32)] -> [(Word32, Word32)] -> Scenario
workerCase image name worker e payloadWords contextWords =
  Scenario
    { scenarioName = "volume-" ++ name
    , scenarioEntry = workerEntry worker
    , scenarioData = [DataRegion region address (slice initial) (slice final) | (region, address, bytes) <- layout, let slice memory = extract memory address bytes]
    , scenarioCalls = [Call (workerEntry worker) [requestBase, workerTable, workerCode worker, workerEntry worker] [] True]
    , scenarioServices = mServices end
    , scenarioResults = [result]
    , scenarioStop = Nothing
    }
  where
    size = workerPayload worker
    payload = foldl (\b (offset, value) -> B.take offset b <> word32le value <> B.drop (offset + 4) b) (B.replicate size 0) payloadWords
    request = words32le [workerCode worker .|. 0x65 `shiftL` 16 .|. 0x1c `shiftL` 24, fromIntegral size, payloadBase, 0] <> payload
    defaultBytes = B.take defaultsBytes (B.drop (fromIntegral (defaults - imageBase)) image)
    layout =
      [ ("request", requestBase, B.length request)
      , ("media-state", mediaState, 8)
      , ("context", contextBase, contextBytes)
      , ("context-tail", tailBase, tailBytes)
      , ("defaults", defaults, defaultsBytes)
      ]
    place address bytes memory = foldl (\acc (i, b) -> M.insert (address + i) b acc) memory (zip [0 ..] (B.unpack bytes))
    initial =
      foldl (\memory (offset, value) -> place (contextBase + offset) (word32le value) memory) base contextWords
    base =
      place requestBase request
        . place mediaState (words32le [envContext e, envAttachState e])
        . place contextBase (patterned 0x21 contextBytes)
        . place tailBase (patterned 0x47 tailBytes)
        . place defaults defaultBytes
        $ M.empty
    extract memory address count = B.pack [M.findWithDefault 0 (address + fromIntegral i) memory | i <- [0 .. count - 1]]
    (result, end) = runS (workerModel worker) e (Machine initial (envOthers e) [])
    final = mMemory end

letterB :: Word32
letterB = 0x42

scenarios :: B.ByteString -> [Scenario]
scenarios image =
  [ attach "unknown-letter" env {envVolume = 0xffffffff} (attachRequest contextBase 0) []
  , attach "unknown-letter-silent" env {envVolume = 0xffffffff} (attachRequest contextBase 0 ++ [(0, 0)]) []
  , attach "no-context" env {envContext = 0} (attachRequest contextBase 0) []
  , attach "release-and-drop" env {envRecords = [contextBase, 0x55]} (attachRequest contextBase 0) [(0x114, 0)]
  , attach "in-place-reattach" env {envRecords = [0x55, contextBase]} (attachRequest contextBase 0) [(0x114, 1), (0x2210, 1)]
  , attach "switch-reattach-rebuild" env (attachRequest 0x1234 1) [(0x114, 0), (0x2210, 1), (0x16bd8, 0x10)]
  , attach "foreign-drop-select" env {envRecords = [0x1234, 0]} (attachRequest 0x1234 1) [(0x114, 1), (0x2210, 1), (0x16bd8, 0x11)]
  , attach "detached-state" env {envAttachState = 0xffffffff} (attachRequest contextBase 0) [(0x114, 1), (0x2210, 1)]
  , attach "unmarked-rebuild" env (attachRequest contextBase 1) [(0x114, 1), (0x2210, 0), (0x16bd8, 0)]
  , detach "no-context" env {envContext = 0} [(0, callbackStub)]
  , detach "once" env {envOthers = [0]} [(0, callbackStub)]
  , detach "none-left" env {envOthers = [0xffffffff]} [(0, callbackStub)]
  , detach "repeat" env {envOthers = [0, 0xffffffff]} [(0, callbackStub), (4, 1)]
  , detach "silent" env {envOthers = [0]} []
  , formatArmed "no-context" env {envContext = 0} formatRequest []
  , formatArmed "unknown-letter" env {envVolume = 0xffffffff} formatRequest []
  , formatArmed "failed" env {envStatus = 39} formatRequest [(0x114, 1)]
  , formatArmed "three-slots" env formatRequest [(0x2600, 3)]
  , formatArmed "no-slots-silent" env [(4, letterB)] [(0x2600, 0)]
  , format "no-context" env {envContext = 0} formatRequest []
  , format "failed" env {envStatus = 2} formatRequest []
  , format "reset" env formatRequest [(0x2600, 3)]
  ]
  where
    attach name = workerCase image ("attach-" ++ name) attachWorkerW
    detach name e request = workerCase image ("detach-" ++ name) detachWorkerW e request [(0x114, 1)]
    formatArmed name = workerCase image ("format-armed-" ++ name) formatArmedWorkerW
    format name = workerCase image ("format-" ++ name) formatWorkerW
    attachRequest descriptor flagged = [(0, callbackStub), (4, descriptor), (0x38, letterB), (0x3c, flagged)]
    formatRequest = [(0, callbackStub), (4, letterB)]
