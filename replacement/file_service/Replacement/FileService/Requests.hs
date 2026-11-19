{-# LANGUAGE OverloadedStrings #-}

-- | The request-path cases: an independent model of the stock posters, caller entries,
-- result-slot helpers, post, status translation, media readiness, mediamsg's routing, the
-- open interceptor and the media task, read from the bytes of their image-4 extents, gives
-- each case's results, ordered service calls, sent bytes and final state.
module Replacement.FileService.Requests
  ( code
  , scenarios
  ) where

import Data.Bits (shiftL, shiftR, testBit, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (elemIndex, findIndex)
import Data.Word (Word32, Word8)
import Replacement.FileService.Native

imageBase :: Word32
imageBase = 0x61c400

wordAt :: B.ByteString -> Word32 -> Word32
wordAt image address =
  sum [fromIntegral (B.index image (offset + index)) `shiftL` (8 * index) | index <- [0 .. 3]]
  where
    offset = fromIntegral (address - imageBase)

code :: Code
code = Code
  { codeImageBase = imageBase
  , codeExtents =
      [ ("result-code", Extent 0x71bc2c 0x71bcc8)
      , ("poster-code", Extent 0x71be74 0x71c504)
      , ("entry-code", Extent 0x71c634 0x71c81c)
      , ("translate-code", Extent 0x71d450 0x71d66c)
      , ("readiness-code", Extent 0x71ddf0 0x71ded4)
      , ("map-code", Extent 0x71ea34 0x71eba0)
      , ("tag-code", Extent 0x71ef18 0x71f078)
      , ("post-code", Extent 0x71fde4 0x71fe2c)
      , ("media-task-code", Extent 0x71fe7c 0x71ff3c)
      , ("mediamsg-code", Extent 0x721108 0x721744)
      , ("intercept-code", Extent 0x7217f0 0x721834)
      ]
  , codeStockFunctions =
      [ (0x71bc2c, 0x40), (0x71bc6c, 0x24), (0x71bc90, 0x38)
      , (0x71be74, 0x16c), (0x71bfe0, 0x154), (0x71c134, 0x17c), (0x71c2b0, 0x254)
      , (0x71c634, 0x14), (0x71c648, 0x28), (0x71c670, 0x34), (0x71c6a4, 0x24), (0x71c6c8, 0x34)
      , (0x71c6fc, 0x24), (0x71c720, 0x3c), (0x71c75c, 0x24), (0x71c780, 0x3c), (0x71c7bc, 0x24), (0x71c7e0, 0x3c)
      , (0x71d450, 0x21c), (0x71ddf0, 0xe4)
      , (0x71ea34, 0x78), (0x71eaac, 0x7c), (0x71eb28, 0x78), (0x71ef18, 0xdc), (0x71eff4, 0x84)
      , (0x71fde4, 0x48), (0x71fe7c, 0xc0)
      , (0x721108, 0xcc), (0x7211d4, 0xc8), (0x72129c, 0x124), (0x7213c0, 0x2c), (0x7213ec, 0x174)
      , (0x721560, 0x18), (0x721578, 0x1cc), (0x7217f0, 0x44)
      ]
  , codeRetained = [("copy-string", Extent 0x70e748 0x70e77c), ("copy-bytes", Extent 0x722054 0x722218)]
  , codeRetainedFunctions = [(0x70e748, 0x34), (0x722054, 0x124), (0x722178, 0x28), (0x7221a0, 0x28), (0x7221c8, 0x50)]
  }

-- Addresses ------------------------------------------------------------------

translateEntry, readinessEntry, acquireEntry, releaseEntry, waitEntry, doneEntry :: Word32
translateEntry = 0x71d450
readinessEntry = 0x71ddf0
acquireEntry = 0x71bc2c
releaseEntry = 0x71bc6c
waitEntry = 0x71bc90
doneEntry = 0x71c634

postEntry, mediaTaskEntry, routeEntry, interceptEntry :: Word32
postEntry = 0x71fde4
mediaTaskEntry = 0x71fe7c
routeEntry = 0x721578
interceptEntry = 0x7217f0

currentTask, semaphoreWait, semaphoreSignal, localRecord, localOpen, localPosition :: Word32
currentTask = 0x723cb8
semaphoreWait = 0x724b68
semaphoreSignal = 0x724c00
localRecord = 0x7111c4
localOpen = 0x711200
localPosition = 0x7111e0

deviceForLetter, mediaReport, send, sleep, wake, receive, ticks :: Word32
deviceForLetter = 0x70ec84
mediaReport = 0x726c74
send = 0x72503c
sleep = 0x7240c8
wake = 0x724144
receive = 0x71fe2c
ticks = 0x7257a8

callbackStub :: Word32
callbackStub = 0x380000

resultOwner, resultSlot, contextWord, contextBase :: Word32
resultOwner = 0x910134
resultSlot = 0x910138
contextWord = 0x910204
contextBase = 0xa00000

session, deferredBase, mediaBusy, mediaTicks, workerTable :: Word32
session = 0x910218
deferredBase = 0x9102e0
mediaBusy = 0x940ac0
mediaTicks = 0x96bfb8
workerTable = 0x814740

stringsBase, requestBase :: Word32
stringsBase = 0xc00000
requestBase = 0xc10000

forever, refused, notOwner, mediamsgTask, mediaTask, interceptor :: Word32
forever = 0xffffffff
refused = 0xffffff01
notOwner = 0xffffffc1
mediamsgTask = 0x65
mediaTask = 0x1c
interceptor = 0x7217f0

-- Services -----------------------------------------------------------------

service :: Word32 -> [(String, Word32)] -> Word32 -> Service
service address inputs result = Service address inputs [("r0", result)] [] []

waitOn :: Word32 -> Word32 -> Service
waitOn semaphore = service semaphoreWait [("r0", semaphore), ("r1", forever)]

signalOn :: Word32 -> Service
signalOn semaphore = service semaphoreSignal [("r0", semaphore)] 0

report :: Maybe Word32 -> [Service]
report = maybe [] (\reported -> [service mediaReport [("r0", 2), ("r1", reported)] 0])

complete :: Word32 -> Word32 -> Maybe Word32 -> [Service]
complete 0 _ _ = []
complete callback status value =
  [service callback ([("r0", status)] ++ maybe [] (\word -> [("r1", word)]) value) 0]

halfWords :: Word32 -> B.ByteString
halfWords value = B.take 2 (word32le value)

-- Status translation ---------------------------------------------------------

-- | The case block the jump table at 0x71d464 selects, decoded from its instructions: either
-- @mov r0,#2; mov r1,#R; bl report; mov r0,#S@ or @mov r0,#S@.
translation :: B.ByteString -> Word32 -> (Word32, Maybe Word32)
translation image raw
  | movR0 first == Just 2, Just reported <- movR1 (word 4), word 8 .&. 0xff000000 == 0xeb000000, Just status <- movR0 (word 12) =
      (status, Just reported)
  | Just status <- movR0 first = (status, Nothing)
  | otherwise = error ("translation case at " ++ hexWord target ++ " is not a status move")
  where
    target = if raw <= 38 then wordAt image (0x71d464 + 4 * raw) else 0x71d664
    word offset = wordAt image (target + offset)
    first = word 0
    movR0 instruction = if instruction .&. 0xffffff00 == 0xe3a00000 then Just (instruction .&. 0xff) else Nothing
    movR1 instruction = if instruction .&. 0xffffff00 == 0xe3a01000 then Just (instruction .&. 0xff) else Nothing

translateScenarios :: B.ByteString -> [Scenario]
translateScenarios image =
  [ plain ("translate-" ++ hexWord raw) []
      [Call translateEntry [0x2c18, raw] [] True]
      (report reported)
      [status]
  | raw <- [0 .. 40] ++ [0x7f, 0x80000000, 0xffffffff]
  , let (status, reported) = translation image raw
  ]

-- Media readiness ------------------------------------------------------------

data Media = NoContext | Context Word32 Word8

-- | 0x71ddf0: no context or device -1 refuses 5; otherwise the first set flag bit decides.
readiness :: Media -> Word32 -> (Word32, Maybe Word32)
readiness NoContext _ = (5, Nothing)
readiness (Context device flags) mode
  | device == 0xffffffff = (5, Nothing)
  | testBit flags 0 = (1, Just 0x50)
  | testBit flags 1 = if mode > 1 then (0, Nothing) else (5, Just 0x55)
  | testBit flags 2 = (5, Just 0x53)
  | testBit flags 3 = if mode == 0 then (2, Just 0x52) else (0, Nothing)
  | otherwise = (0, Nothing)

mediaRegions :: Media -> [DataRegion]
mediaRegions media =
  [ unchanged "context-device" (contextBase + 0x114) (word32le device)
  , unchanged "context-flags" (contextBase + 0x16bd8) (B.pack [flags, 0, 0, 0])
  ]
  where
    (device, flags) = case media of
      NoContext -> (0, 0)
      Context value bits -> (value, bits)

contextPointer :: Media -> Word32
contextPointer NoContext = 0
contextPointer _ = contextBase

unchanged :: String -> Word32 -> B.ByteString -> DataRegion
unchanged name address bytes = DataRegion name address bytes bytes

readinessScenarios :: [Scenario]
readinessScenarios =
  [ readinessCase name media mode
  | (name, media) <-
      [ (const "no-context", NoContext)
      , (const "device-unset", Context 0xffffffff 0)
      ]
  , mode <- [0]
  ]
    ++ [ readinessCase (\m -> "flags-" ++ hexWord (fromIntegral flags) ++ "-mode-" ++ show m) (Context 3 flags) mode
       | flags <- [0 .. 15] ++ [0x10, 0xf0, 0xf8]
       , mode <- [0, 1, 2, 3, 0x80000000]
       ]
  where
    readinessCase name media mode =
      let (status, reported) = readiness media mode
       in plain ("readiness-" ++ name mode) (unchanged "context" contextWord (word32le (contextPointer media)) : mediaRegions media)
            [Call readinessEntry [mode] [] True]
            (report reported)
            [status]

-- Result-slot helpers ----------------------------------------------------------

-- | Owner, then the slot's status and value words.
slotRegion :: (Word32, Word32, Word32) -> (Word32, Word32, Word32) -> DataRegion
slotRegion (owner, status, value) (owner', status', value') =
  DataRegion "result-slot" resultOwner (words32le [owner, status, value]) (words32le [owner', status', value'])

resultScenarios :: [Scenario]
resultScenarios =
  [ plain "acquire-granted" [slotRegion (0, 1, 2) (0x31, 1, 2)] [Call acquireEntry [] [] True]
      [waitOn 0x3a 0, service currentTask [] 0x31] [resultSlot]
  , plain "acquire-refused" [slotRegion (0x44, 1, 2) (0x44, 1, 2)] [Call acquireEntry [] [] True]
      [waitOn 0x3a 0xffffffe7] [0]
  , plain "release" [slotRegion (0x31, 1, 2) (0, 1, 2)] [Call releaseEntry [] [] True]
      [signalOn' 0x3a 0x12] [0x12]
  , plain "wait-owner" [slotRegion (0x31, 1, 2) (0x31, 1, 2)] [Call waitEntry [] [] True]
      [service currentTask [] 0x31, waitOn 0x3b 0xffffffe2] [0xffffffe2]
  , plain "wait-not-owner" [slotRegion (0x32, 1, 2) (0x32, 1, 2)] [Call waitEntry [] [] True]
      [service currentTask [] 0x31] [notOwner]
  , plain "done" [] [Call doneEntry [] [] True] [signalOn' 0x3b 0x7] [7]
  ]
  where
    signalOn' semaphore result = service semaphoreSignal [("r0", semaphore)] result

-- Post -------------------------------------------------------------------------

postScenarios :: [Scenario]
postScenarios =
  [ postCase "post-sent" 0 0x2c18 0x30 0x1c
  , postCase "post-refused" 0xffffffe0 0x2c2d 0x1d 0x50
  ]
  where
    postCase name result value source length' =
      let header = words32le [value .|. source `shiftL` 16 .|. mediamsgTask `shiftL` 24, length', requestBase + 0x10]
       in plain name [unchanged "request" requestBase header]
            [Call postEntry [requestBase] [] True]
            [Service send [("r0", mediamsgTask)] [("r0", result)] [AtRegister "r1" 0 header] []]
            [result]

-- Posters and caller entries -------------------------------------------------------

data Kind = Open | Read | Write | Close
  deriving (Eq)

-- | The six request words each entry passes, as callback, three request words, wait and
-- family; an open's third word is its open word. The wrappers at 0x71c648..0x71c81c fix
-- the callback, wait, word and family words as their bytes do.
request :: Word32 -> [Word32] -> [Word32] -> (Kind, [Word32])
request entry arguments stack = case entry of
  0x71c2b0 -> (Open, [a 0, a 1, a 2, s 0, a 3, s 1])
  0x71c648 -> (Open, [a 0, a 1, a 2, 1, 0, 0])
  0x71c670 -> (Open, [0, a 0, a 1, 1, 1, 0])
  0x71c6a4 -> (Open, [a 0, a 1, a 2, 0, 0, 0])
  0x71c6c8 -> (Open, [0, a 0, a 1, 0, 1, 0])
  0x71be74 -> (Read, full)
  0x71c6fc -> (Read, take 4 padded ++ [0, 0])
  0x71c720 -> (Read, [0, a 0, a 1, a 2, 1, 0])
  0x71bfe0 -> (Write, full)
  0x71c75c -> (Write, take 4 padded ++ [0, 0])
  0x71c780 -> (Write, [0, a 0, a 1, a 2, 1, 0])
  0x71c134 -> (Close, full)
  0x71c7bc -> (Close, take 4 padded ++ [0, 0])
  0x71c7e0 -> (Close, [0, a 0, a 1, a 2, 1, 0])
  _ -> error ("no request entry at " ++ hexWord entry)
  where
    padded = arguments ++ repeat 0
    a index = padded !! index
    s index = (stack ++ repeat 0) !! index
    full = take 4 padded ++ take 2 (stack ++ repeat 0)

data Env = Env
  { envTask :: Word32
  , envLocalMode :: Word32
  , envLocalMatch :: Word32
  , envMedia :: Media
  , envLetterDevice :: Word32
  , envAcquire :: Word32
  , envResult :: (Word32, Word32)
  , envSequence :: Word32
  , envPath :: B.ByteString
  , envName :: B.ByteString
  }

env :: Env
env = Env
  { envTask = 0x30
  , envLocalMode = 0
  , envLocalMatch = 0
  , envMedia = Context 7 0
  , envLetterDevice = 7
  , envAcquire = 0
  , envResult = (0, 0x200)
  , envSequence = 0x40
  , envPath = pathOf 47
  , envName = nameOf 19
  }

pathOf, nameOf :: Int -> B.ByteString
pathOf size = B.pack (take size (cycle [0x2f, 0x44, 0x43, 0x49, 0x4d, 0x2f, 0x31, 0x30, 0x30, 0x5f]))
nameOf size = B.pack (take size (cycle [0x44, 0x53, 0x43, 0x46, 0x30, 0x30, 0x30, 0x31, 0x2e]))

pathAddress, nameAddress :: Word32
pathAddress = stringsBase
nameAddress = stringsBase + 0x100

stringsRegion :: Env -> DataRegion
stringsRegion environment =
  unchanged "strings" stringsBase (padTo 0x100 (envPath environment <> B.singleton 0) <> envName environment <> B.singleton 0)
  where
    padTo size bytes = bytes <> B.replicate (size - B.length bytes) 0

-- | What one request does: its service calls in order, its result, the slot words afterwards
-- and the tag it allocated.
data Outcome = Outcome
  { outServices :: [Service]
  , outResult :: Word32
  , outSlot :: (Word32, Word32)
  , outTag :: Maybe Word32
  }

initialSlot :: (Word32, Word32)
initialSlot = (0x5a5a5a5a, 0x6b6b6b6b)

nextCandidate :: Word32 -> Word32
nextCandidate previous = let next = previous + 1 in if next >= 0x80000000 then 1 else next

refuses :: Word32 -> Bool
refuses task = task == mediaTask || task == 0

refusal :: [Service] -> Outcome
refusal made = Outcome made 0 initialSlot Nothing

-- | The header word the poster sends: code, source task byte and mediamsg.
headerWord :: Word32 -> Word32 -> Word32
headerWord requestCode task = requestCode .|. (task .&. 0xff) `shiftL` 16 .|. mediamsgTask `shiftL` 24

sent :: Word32 -> Word32 -> Word32 -> B.ByteString -> Int -> Service
sent requestCode task size payload defined =
  Service send [("r0", mediamsgTask)] [("r0", 0)]
    [AtRegister "r1" 0 (words32le [headerWord requestCode task, size]), AtIndirect "r1" 8 (B.take defined payload)]
    []

slotWords :: Bool -> [Word32]
slotWords False = [0, 0, 0]
slotWords True = [1, resultSlot, resultSlot + 4]

-- | The blocking tail: wait for the worker's completion, which writes the slot, then release.
awaited :: Env -> [Service]
awaited environment =
  [ service currentTask [] (envTask environment)
  , Service semaphoreWait [("r0", 0x3b), ("r1", forever)] [("r0", 0)] [] [AtAddress resultSlot 0 (words32le [status, value])]
  ]
  where
    (status, value) = envResult environment

acquired :: Env -> Either [Service] [Service]
acquired environment
  | envAcquire environment /= 0 = Left [waitOn 0x3a (envAcquire environment)]
  | otherwise = Right [waitOn 0x3a 0, service currentTask [] (envTask environment)]

poster :: Env -> Kind -> [Word32] -> Outcome
poster environment kind [callback, first, second, third, wait, family] = case kind of
  Open -> openRequest
  Read -> transfer True (pick 0x2c18 0x2c2e)
  Write -> transfer False (pick 0x2c19 0x2c2f)
  Close -> closeRequest
  where
    task = envTask environment
    blocking = wait /= 0
    pick primary alternate = if family == 1 then alternate else primary
    current = service currentTask [] task
    record = Service localRecord [] [("r0", 0)] [] [AtRegister "r0" 0 (word32le (envLocalMode environment))]
    (ready, reported) = readiness (envMedia environment) 0
    readied = report reported
    deviceWord = case envMedia environment of
      Context device _ -> device
      NoContext -> 0
    finish made tag = Outcome made tag (if blocking then envResult environment else initialSlot) Nothing
    released = [service semaphoreSignal [("r0", 0x3a)] 0]
    openRequest
      | blocking && refuses task = refusal ([current] ++ complete callback 9 (Just 0))
      | envLocalMode environment == 1 =
          let matched = envLocalMatch environment == 1
              value = if matched then 1 else 0
           in Outcome
                ( [current, record, service localOpen [("r0", first)] (envLocalMatch environment)]
                    ++ [service localPosition [("r0", 0)] 0 | matched]
                    ++ complete callback (if matched then 0 else 6) (Just value)
                )
                value initialSlot Nothing
      | ready /= 0 = refusal ([current, record] ++ readied ++ complete callback ready (Just 0))
      | otherwise =
          let path = envPath environment
              byte index = if index < B.length path then B.index path index else 0
              drive
                | byte 1 /= 0x3a = Right []
                | byte 0 >= 0x41 && byte 0 <= 0x5a = letter (byte 0)
                | byte 0 >= 0x61 && byte 0 <= 0x7a = letter (byte 0 - 32)
                | otherwise = Left ([], 8)
              letter value =
                let called = [service deviceForLetter [("r0", fromIntegral value)] (envLetterDevice environment)]
                 in if envLetterDevice environment == deviceWord then Right called else Left (called, 5)
              before = [current, record] ++ readied
           in case drive of
                Left (called, status) -> refusal (before ++ called ++ complete callback status (Just 0))
                Right called ->
                  case (blocking, acquired environment) of
                    (True, Left waited) -> refusal (before ++ called ++ waited ++ complete callback 9 (Just 0))
                    _ ->
                      let held = either (const []) id (acquired environment)
                          tag = nextCandidate (envSequence environment)
                          payload =
                            word32le callback <> B.take 0x30 (path <> B.replicate 0x30 0)
                              <> words32le ([0, second, tag] ++ slotWords blocking ++ [third])
                          defined = if B.length path >= 0x2f then 0x50 else 4 + B.length path + 1
                          posting =
                            before ++ called ++ (if blocking then held else [])
                              ++ [waitOn 0x3c 0, signalOn 0x3c, current, sent (pick 0x2c17 0x2c2d) task 0x50 payload defined]
                          status = fst (envResult environment)
                       in if blocking
                            then (finish (posting ++ awaited environment ++ released) (if status == 0 then tag else 0)) {outTag = Just tag}
                            else (finish posting tag) {outTag = Just tag}
    transfer local requestCode
      | local && envLocalMode environment == 0 && ready /= 0 = refusal ([record] ++ readied ++ complete callback ready (Just 0))
      | not local && ready /= 0 = refusal (readied ++ complete callback ready (Just 0))
      | otherwise =
          let before = (if local then [record] ++ (if envLocalMode environment == 0 then readied else []) else readied) ++ [current]
              payload = words32le ([callback, first, second, third] ++ slotWords blocking)
              posting held = before ++ held ++ [current, sent requestCode task 0x1c payload 0x1c]
              (status, value) = envResult environment
           in case (blocking, acquired environment) of
                (False, _) -> finish (posting []) 0
                (True, _) | refuses task -> refusal (before ++ complete callback 9 (Just 0))
                (True, Left waited) -> refusal (before ++ waited ++ complete callback 9 (Just 0))
                (True, Right held) -> finish (posting held ++ awaited environment ++ released) (if status == 0 then value else 0)
    closeRequest
      | envLocalMode environment == 1 = refusal ([record] ++ complete callback 0 Nothing)
      | ready /= 0 = refusal ([record] ++ readied ++ complete callback ready Nothing)
      | otherwise =
          let before = [record] ++ readied ++ [current]
              name = envName environment
              field = if third == 0 then B.singleton 0 else B.take 20 (name <> B.replicate 20 0)
              defined = if third == 0 then 13 else if B.length name >= 19 then 0x28 else 0xc + B.length name + 1
              payload = words32le [callback, first, second] <> field <> B.replicate (20 - B.length field) 0
                <> words32le (take 2 (slotWords blocking))
              posting held = before ++ held ++ [current, sent (pick 0x2c1a 0x2c30) task 0x28 payload defined]
           in case (blocking, acquired environment) of
                (False, _) -> finish (posting []) 0
                (True, _) | refuses task -> refusal (before ++ complete callback 9 Nothing)
                (True, Left waited) -> refusal (before ++ waited ++ complete callback 9 Nothing)
                (True, Right held) -> finish (posting held ++ awaited environment ++ released) (fst (envResult environment))
poster _ _ _ = error "a request has six words"

postingRegion :: Env -> Outcome -> DataRegion
postingRegion environment outcome =
  DataRegion "posting-state" resultOwner (state initialSlot [] (envSequence environment)) (state (outSlot outcome) tags sequence')
  where
    (tags, sequence') = case outTag outcome of
      Just tag -> ([tag], tag)
      Nothing -> ([], envSequence environment)
    state (status, value) held counter =
      words32le ([0, status, value] ++ replicate 32 0 ++ take 16 (held ++ repeat 0) ++ [counter, contextPointer (envMedia environment)])

posterCase :: String -> Env -> Word32 -> [Word32] -> [Word32] -> Scenario
posterCase name environment entry arguments stack =
  plain ("request-" ++ name)
    ([postingRegion environment outcome, stringsRegion environment] ++ mediaRegions (envMedia environment))
    [Call entry arguments stack True]
    (outServices outcome)
    [outResult outcome]
  where
    (kind, requested) = request entry arguments stack
    outcome = poster environment kind requested

posterScenarios :: [Scenario]
posterScenarios =
  [ posterCase "open-posted" env 0x71c2b0 [callbackStub, pathAddress, 2, 0] [1, 0]
  , posterCase "open-posted-alternate" env 0x71c2b0 [callbackStub, pathAddress, 2, 0] [0, 1]
  , posterCase "open-blocking" env 0x71c2b0 [callbackStub, pathAddress, 3, 1] [1, 0]
  , posterCase "open-blocking-failed" env {envResult = (4, 0x9)} 0x71c2b0 [callbackStub, pathAddress, 3, 1] [0, 0]
  , posterCase "open-blocking-media-task" env {envTask = mediaTask} 0x71c2b0 [callbackStub, pathAddress, 3, 1] [0, 0]
  , posterCase "open-blocking-no-task" env {envTask = 0} 0x71c2b0 [callbackStub, pathAddress, 3, 1] [0, 0]
  , posterCase "open-posted-media-task" env {envTask = mediaTask} 0x71c2b0 [callbackStub, pathAddress, 3, 0] [0, 0]
  , posterCase "open-slot-refused" env {envAcquire = 0xffffffe7} 0x71c2b0 [callbackStub, pathAddress, 3, 1] [0, 0]
  , posterCase "open-local-matched" env {envLocalMode = 1, envLocalMatch = 1} 0x71c2b0 [callbackStub, pathAddress, 3, 1] [0, 0]
  , posterCase "open-local-missing" env {envLocalMode = 1, envLocalMatch = 2} 0x71c2b0 [callbackStub, pathAddress, 3, 0] [0, 0]
  , posterCase "open-no-media" env {envMedia = NoContext} 0x71c2b0 [callbackStub, pathAddress, 3, 0] [0, 0]
  , posterCase "open-write-protected" env {envMedia = Context 7 4} 0x71c2b0 [callbackStub, pathAddress, 3, 0] [0, 0]
  , posterCase "open-no-media-silent" env {envMedia = NoContext} 0x71c2b0 [0, pathAddress, 3, 0] [0, 0]
  , posterCase "open-drive-upper" env {envPath = "B:/DCIM/100_FUJI/DSCF0001.JPG"} 0x71c2b0 [callbackStub, pathAddress, 1, 0] [0, 0]
  , posterCase "open-drive-lower" env {envPath = "b:/x"} 0x71c2b0 [callbackStub, pathAddress, 1, 0] [0, 0]
  , posterCase "open-drive-other" env {envPath = "C:/x", envLetterDevice = 8} 0x71c2b0 [callbackStub, pathAddress, 1, 0] [0, 0]
  , posterCase "open-drive-invalid" env {envPath = "1:/x"} 0x71c2b0 [callbackStub, pathAddress, 1, 0] [0, 0]
  , posterCase "open-sequence-wrap" env {envSequence = 0x7fffffff} 0x71c2b0 [callbackStub, pathAddress, 1, 0] [0, 0]
  , posterCase "open-async" env 0x71c648 [callbackStub, pathAddress, 5] []
  , posterCase "open-async-r3-ignored" env 0x71c648 [callbackStub, pathAddress, 5, 1] []
  , posterCase "open-wait" env 0x71c670 [pathAddress, 6] []
  , posterCase "open-unflagged" env 0x71c6a4 [callbackStub, pathAddress, 7] []
  , posterCase "open-unflagged-r3-ignored" env 0x71c6a4 [callbackStub, pathAddress, 7, 2] []
  , posterCase "open-unflagged-wait" env 0x71c6c8 [pathAddress, 8] []
  , posterCase "read-posted" env 0x71be74 [callbackStub, 0x51, 0x2000, 0xd00000] [0, 0]
  , posterCase "read-posted-alternate" env 0x71be74 [callbackStub, 0x51, 0x2000, 0xd00000] [0, 1]
  , posterCase "read-blocking" env 0x71be74 [callbackStub, 0x51, 0x2000, 0xd00000] [1, 0]
  , posterCase "read-blocking-failed" env {envResult = (3, 0x1000)} 0x71be74 [callbackStub, 0x51, 0x2000, 0xd00000] [1, 0]
  , posterCase "read-blocking-media-task" env {envTask = mediaTask} 0x71be74 [callbackStub, 0x51, 0x2000, 0xd00000] [1, 0]
  , posterCase "read-slot-refused" env {envAcquire = 1} 0x71be74 [callbackStub, 0x51, 0x2000, 0xd00000] [1, 0]
  , posterCase "read-no-media" env {envMedia = Context 0xffffffff 0} 0x71be74 [callbackStub, 0x51, 0x2000, 0xd00000] [0, 0]
  , posterCase "read-media-busy" env {envMedia = Context 7 1} 0x71be74 [callbackStub, 0x51, 0x2000, 0xd00000] [0, 0]
  , posterCase "read-local-no-media" env {envLocalMode = 1, envMedia = NoContext} 0x71be74 [callbackStub, 0x51, 0x2000, 0xd00000] [0, 0]
  , posterCase "read-async" env 0x71c6fc [callbackStub, 0x52, 0x100, 0xd00100] []
  , posterCase "read-wait" env 0x71c720 [0x52, 0x100, 0xd00100] []
  , posterCase "read-wait-silent-refusal" env {envMedia = Context 7 8} 0x71c720 [0x52, 0x100, 0xd00100] []
  , posterCase "write-posted" env 0x71bfe0 [callbackStub, 0x53, 0x400, 0xd00200] [0, 0]
  , posterCase "write-posted-alternate" env 0x71bfe0 [callbackStub, 0x53, 0x400, 0xd00200] [0, 1]
  , posterCase "write-blocking" env 0x71bfe0 [callbackStub, 0x53, 0x400, 0xd00200] [1, 0]
  , posterCase "write-blocking-failed" env {envResult = (8, 0x80)} 0x71bfe0 [callbackStub, 0x53, 0x400, 0xd00200] [1, 0]
  , posterCase "write-not-ready" env {envMedia = Context 7 8} 0x71bfe0 [callbackStub, 0x53, 0x400, 0xd00200] [0, 0]
  , posterCase "write-local-not-ready" env {envLocalMode = 1, envMedia = Context 7 2} 0x71bfe0 [callbackStub, 0x53, 0x400, 0xd00200] [0, 0]
  , posterCase "write-blocking-no-task" env {envTask = 0} 0x71bfe0 [callbackStub, 0x53, 0x400, 0xd00200] [1, 0]
  , posterCase "write-async" env 0x71c75c [callbackStub, 0x54, 0x10, 0xd00300] []
  , posterCase "write-wait" env 0x71c780 [0x54, 0x10, 0xd00300] []
  , posterCase "close-posted" env 0x71c134 [callbackStub, 0x55, 0x1, 0] [0, 0]
  , posterCase "close-posted-named" env 0x71c134 [callbackStub, 0x55, 0x1, nameAddress] [0, 0]
  , posterCase "close-posted-alternate" env 0x71c134 [callbackStub, 0x55, 0x1, 0] [0, 1]
  , posterCase "close-blocking" env {envResult = (0, 0x77)} 0x71c134 [callbackStub, 0x55, 0x2, nameAddress] [1, 0]
  , posterCase "close-blocking-failed" env {envResult = (6, 0)} 0x71c134 [callbackStub, 0x55, 0x2, 0] [1, 0]
  , posterCase "close-blocking-media-task" env {envTask = mediaTask} 0x71c134 [callbackStub, 0x55, 0x2, 0] [1, 0]
  , posterCase "close-slot-refused" env {envAcquire = 5} 0x71c134 [callbackStub, 0x55, 0x2, 0] [1, 0]
  , posterCase "close-local" env {envLocalMode = 1} 0x71c134 [callbackStub, 0x55, 0x2, 0] [1, 0]
  , posterCase "close-no-media" env {envMedia = NoContext} 0x71c134 [callbackStub, 0x55, 0x2, 0] [0, 0]
  , posterCase "close-async" env 0x71c7bc [callbackStub, 0x56, 0x3, nameAddress] []
  , posterCase "close-wait" env 0x71c7e0 [0x56, 0x3, 0] []
  ]

-- mediamsg -------------------------------------------------------------------

data Session = Session
  { sessionDeferred :: Word32
  , sessionClass :: Word32
  , sessionStatus :: Word32
  , sessionHandle :: Word32
  , sessionOpen :: Word32
  , sessionSaved :: Word32
  , sessionSet :: [Word32]
  , sessionSlots :: [B.ByteString]
  }

idle :: Session
idle = Session 0 0 0 0 0 0 (replicate 20 0) (replicate 48 (B.replicate 96 0))

sessionBytes :: Session -> B.ByteString
sessionBytes state =
  B.replicate 0x60 0x11
    <> words32le [sessionDeferred state, sessionClass state, sessionStatus state, sessionHandle state, sessionOpen state, sessionSaved state]
    <> words32le (sessionSet state)
    <> B.concat (sessionSlots state)

-- | A request header and its payload; a direct request sits at 'requestBase' with its
-- payload at +0x10, a replayed one in a private copy.
data Request = Request
  { requestCode :: Word32
  , requestSource :: Word32
  , requestDestination :: Word32
  , requestPayload :: B.ByteString
  , requestDirect :: Bool
  }

requestBytes :: Request -> B.ByteString
requestBytes item =
  words32le [headerOf item, fromIntegral (B.length (requestPayload item)), requestBase + 0x10, 0] <> requestPayload item

headerOf :: Request -> Word32
headerOf item = requestCode item .|. requestSource item `shiftL` 16 .|. requestDestination item `shiftL` 24

payloadWord :: Request -> Int -> Word32
payloadWord item index = wordFrom (requestPayload item) (4 * index)

wordFrom :: B.ByteString -> Int -> Word32
wordFrom bytes offset =
  sum [fromIntegral (if offset + index < B.length bytes then B.index bytes (offset + index) else 0) `shiftL` (8 * index) | index <- [0 .. 3]]

-- | Scripted answers: each send's status, each sleep's status with the open result the
-- interceptor records meanwhile.
data Script = Script
  { scriptSends :: [Word32]
  , scriptSleeps :: [(Word32, Word32, Word32)]
  }

type Routing = (Session, Script, [Service])

primaryOf :: Word32 -> Word32
primaryOf value = case value of
  0x2c2d -> 0x2c17
  0x2c2e -> 0x2c18
  0x2c2f -> 0x2c19
  0x2c30 -> 0x2c1a
  0x2c31 -> 0x2c1c
  0x2c32 -> 0x2c1e
  0x2c33 -> 0x2c08
  0x2c34 -> 0x2c13
  _ -> value

defer :: Routing -> Request -> (Routing, Word32)
defer (state, script, made) item = case findIndex (\slot -> wordFrom slot 0 .&. 0xffff == 0) (sessionSlots state) of
  Nothing -> ((state, script, made), refused)
  Just index ->
    let payload = requestPayload item
        size = B.length payload
        address = deferredBase + 96 * fromIntegral index
        header = words32le [headerOf item, fromIntegral size, if size == 0 then pointer else address + 16]
        pointer = if requestDirect item then requestBase + 0x10 else 0
        slot = header <> B.drop 12 (B.take 16 (sessionSlots state !! index)) <> payload <> B.drop (16 + size) (sessionSlots state !! index)
     in ((state {sessionSlots = replaceAt index (B.take 96 slot) (sessionSlots state), sessionDeferred = sessionDeferred state + 1}, script, made), 0)

forward :: Routing -> Request -> (Routing, Request, Word32)
forward (state, script, made) item =
  ((state, script {scriptSends = drop 1 (scriptSends script)}, made ++ [call]), routed, if status == 0 then 0 else refused)
  where
    routed = item {requestSource = mediamsgTask, requestDestination = mediaTask, requestCode = primaryOf (requestCode item)}
    status = case scriptSends script of
      first : _ -> first
      [] -> 0
    size = B.length (requestPayload item)
    call =
      Service send ([("r0", mediaTask)] ++ [("r1", requestBase) | requestDirect item]) [("r0", status)]
        ([AtRegister "r1" 0 (words32le [headerOf routed, fromIntegral size])] ++ [AtIndirect "r1" 8 (requestPayload item) | size > 0])
        []

replaceAt :: Int -> a -> [a] -> [a]
replaceAt index value values = take index values ++ [value] ++ drop (index + 1) values

-- | mediamsg's routing of one request at 0x721578, the request as it ends, and the result.
route :: Routing -> Request -> (Routing, Request, Word32)
route routing item = case requestCode item of
  0x2c01 -> detach
  0x2c17 -> open 1 2
  0x2c2d -> open 2 1
  0x2c1a -> close 1
  0x2c1b -> close 1
  0x2c1d -> close 1
  0x2c30 -> close 2
  value | value `elem` [0x2c2e, 0x2c2f, 0x2c31, 0x2c32, 0x2c33, 0x2c34] -> pass 2
  _ -> pass 1
  where
    (state, _, _) = routing
    deferred = let (next, result) = defer routing item in (next, item, result)
    pass blocked = if sessionClass state == blocked then deferred else forward routing item
    detach
      | sessionClass state == 1 = deferred
      | otherwise =
          let ((after, script, made), routed, result) = forward routing item
           in ((after {sessionStatus = 0, sessionHandle = 0, sessionOpen = 0, sessionSaved = 0, sessionSet = replicate 20 0, sessionClass = 0}, script, made), routed, result)
    open blocked class'
      | sessionClass state == blocked = let (next, _) = defer routing item in (next, item, 0)
      | otherwise =
          let (current, script, made) = routing
              installed = item {requestPayload = word32le interceptor <> B.drop 4 (requestPayload item)}
              saved = current {sessionSaved = payloadWord item 0}
              ((afterSend, script', made'), routed, result) = forward (saved, script, made) installed
              (sleepStatus, status, handle) = case scriptSleeps script' of
                first : _ -> first
                [] -> (0, 0, 0)
              slept = Service sleep [] [("r0", sleepStatus)] [] [AtAddress (session + 0x68) 0 (words32le [status, handle])]
              woken = afterSend {sessionStatus = status, sessionHandle = handle}
              script'' = script' {scriptSleeps = drop 1 (scriptSleeps script')}
           in if result /= 0
                then ((afterSend, script', made'), routed, result)
                else
                  if sleepStatus /= 0
                    then ((woken, script'', made' ++ [slept]), routed, refused)
                    else
                      if status /= 0
                        then ((woken, script'', made' ++ [slept]), routed, 0)
                        else
                          let set = sessionSet woken
                              recorded
                                | handle `elem` set = woken
                                | otherwise = case elemIndex 0 set of
                                    Just index -> woken {sessionSet = replaceAt index handle set, sessionOpen = sessionOpen woken + 1}
                                    Nothing -> error "stock stops with more than 20 open handles"
                           in ((recorded {sessionClass = class'}, script'', made' ++ [slept]), routed, 0)
    close blocked
      | sessionClass state == blocked = let (next, _) = defer routing item in (next, item, 0)
      | otherwise =
          let handle = payloadWord item 1
              ((afterSend, script, made), routed, result) = forward routing item
              set = sessionSet afterSend
              removed = case elemIndex handle set of
                Just index -> afterSend {sessionSet = replaceAt index 0 set, sessionOpen = sessionOpen afterSend - 1}
                Nothing -> afterSend
           in if result /= 0
                then ((afterSend, script, made), routed, result)
                else
                  if sessionOpen removed /= 0
                    then ((removed, script, made), routed, 0)
                    else (replay 0 (removed {sessionClass = 0}, script, made), routed, 0)
    replay index current@(present, _, _)
      | index >= 48 || sessionDeferred present == 0 = current
      | wordFrom slot 0 .&. 0xffff == 0 = replay (index + 1) current
      | otherwise =
          let (cleared, script, made) = current
              header = wordFrom slot 0
              size = fromIntegral (wordFrom slot 4)
              copy = Request (header .&. 0xffff) ((header `shiftR` 16) .&. 0xff) (header `shiftR` 24) (B.take size (B.drop 16 slot)) False
              emptied = cleared {sessionDeferred = sessionDeferred cleared - 1, sessionSlots = replaceAt index (B.replicate 96 0) (sessionSlots cleared)}
              (next, _, result) = route (emptied, script, made) copy
           in if result /= 0 then next else replay (index + 1) next
      where
        slot = sessionSlots present !! index

routeCase :: String -> Session -> Script -> Request -> Scenario
routeCase name initial script item =
  plain ("route-" ++ name)
    [ DataRegion "mediamsg-state" session (sessionBytes initial) (sessionBytes final)
    , DataRegion "request" requestBase (requestBytes item) (requestBytes routed)
    ]
    [Call routeEntry [requestBase] [] True]
    made
    [result]
  where
    ((final, _, made), routed, result) = route (initial, script, []) item

direct :: Word32 -> Word32 -> [Word32] -> Request
direct value source payload = Request value source mediamsgTask (words32le payload) True

sends :: [Word32] -> Script
sends values = Script values []

opened :: Word32 -> Word32 -> Word32 -> Script
opened sleepStatus status handle = Script [0] [(sleepStatus, status, handle)]

slotFor :: Request -> B.ByteString
slotFor item = B.take 96 (words32le [headerOf item, fromIntegral (B.length (requestPayload item)), 0xdead0000] <> B.replicate 4 0 <> requestPayload item <> B.replicate 96 0)

withSlots :: [(Int, Request)] -> Session -> Session
withSlots items state =
  state {sessionSlots = foldl (\slots (index, item) -> replaceAt index (slotFor item) slots) (sessionSlots state) items, sessionDeferred = sessionDeferred state + fromIntegral (length items)}

routeScenarios :: [Scenario]
routeScenarios =
  [ routeCase "read-idle" idle (sends [0]) (direct 0x2c18 0x30 [callbackStub, 0x51, 0x200, 0xd00000, 0, 0, 0])
  , routeCase "read-send-refused" idle (sends [5]) (direct 0x2c18 0x30 [callbackStub, 0x51, 0x200, 0xd00000, 0, 0, 0])
  , routeCase "alternate-read-primary-class" idle {sessionClass = 2} (sends []) (direct 0x2c2e 0x30 [callbackStub, 0x51, 0x200, 0xd00000, 0, 0, 0])
  , routeCase "read-alternate-class" idle {sessionClass = 1} (sends []) (direct 0x2c18 0x31 [callbackStub, 0x52, 0x200, 0xd00000, 1, resultSlot, resultSlot + 4])
  , routeCase "alternate-read-alternate-class" idle {sessionClass = 1} (sends [0]) (direct 0x2c2e 0x30 [callbackStub, 0x51, 0x200, 0xd00000, 0, 0, 0])
  , routeCase "alternate-write-idle" idle (sends [0]) (direct 0x2c2f 0x30 [callbackStub, 0x53, 0x10, 0xd00000, 0, 0, 0])
  , routeCase "attach-idle" idle (sends [0]) (direct 0x2c00 0x30 [callbackStub, 2])
  , routeCase "attach-alternate-class" idle {sessionClass = 1} (sends []) (direct 0x2c00 0x30 [])
  , routeCase "defer-second-slot" (withSlots [(0, direct 0x2c19 0x30 [1, 2, 3])] idle {sessionClass = 1}) (sends []) (direct 0x2c18 0x33 [callbackStub, 0x54, 0x10, 0xd00000, 0, 0, 0])
  , routeCase "defer-full" (withSlots [(index, direct 0x2c18 0x30 [fromIntegral index]) | index <- [0 .. 47]] idle {sessionClass = 1}) (sends []) (direct 0x2c18 0x34 [7])
  , routeCase "unknown-code-idle" idle (sends [0]) (direct 0x2c40 0x30 [1])
  , routeCase "open-idle" idle (opened 0 0 0x123) (direct 0x2c17 0x30 (callbackStub : replicate 19 0x41414141))
  , routeCase "open-handle-known" idle {sessionSet = replaceAt 3 0x123 (replicate 20 0), sessionOpen = 1, sessionClass = 2} (opened 0 0 0x123) (direct 0x2c17 0x30 [callbackStub, 0x2f])
  , routeCase "open-second-handle" idle {sessionSet = 0x99 : replicate 19 0, sessionOpen = 1, sessionClass = 2} (opened 0 0 0x124) (direct 0x2c17 0x30 [callbackStub, 0x2f])
  , routeCase "open-first-slot-free" idle {sessionSet = 0 : 0x99 : replicate 18 0, sessionOpen = 1, sessionClass = 2} (opened 0 0 0x125) (direct 0x2c17 0x30 [callbackStub, 0x2f])
  , routeCase "open-failed" idle (opened 0 4 0) (direct 0x2c17 0x30 [callbackStub, 0x2f])
  , routeCase "open-sleep-refused" idle (opened 0xfffffff7 0 0x123) (direct 0x2c17 0x30 [callbackStub, 0x2f])
  , routeCase "open-send-refused" idle (Script [1] []) (direct 0x2c17 0x30 [callbackStub, 0x2f])
  , routeCase "open-alternate-class" idle {sessionClass = 1} (sends []) (direct 0x2c17 0x30 [callbackStub, 0x2f])
  , routeCase "alternate-open-idle" idle (opened 0 0 0x200) (direct 0x2c2d 0x30 [callbackStub, 0x2f])
  , routeCase "alternate-open-primary-class" idle {sessionClass = 2} (sends []) (direct 0x2c2d 0x30 [callbackStub, 0x2f])
  , routeCase "close-one-of-two" idle {sessionSet = 0x123 : 0x124 : replicate 18 0, sessionOpen = 2, sessionClass = 2} (sends [0]) (direct 0x2c1a 0x30 [callbackStub, 0x124, 0])
  , routeCase "close-last-replays"
      (withSlots [(0, direct 0x2c2e 0x30 [callbackStub, 0x61, 0x20, 0xd00000, 0, 0, 0]), (2, direct 0x2c30 0x31 [callbackStub, 0x62, 0])] idle {sessionSet = 0x123 : replicate 19 0, sessionOpen = 1, sessionClass = 2})
      (sends [0, 0, 0]) (direct 0x2c1a 0x30 [callbackStub, 0x123, 0])
  , routeCase "close-last-replay-refused"
      (withSlots [(1, direct 0x2c2f 0x30 [callbackStub, 0x63, 0x20, 0xd00000, 0, 0, 0]), (5, direct 0x2c2e 0x31 [callbackStub, 0x64])] idle {sessionSet = 0x123 : replicate 19 0, sessionOpen = 1, sessionClass = 2})
      (sends [0, 2]) (direct 0x2c1b 0x30 [callbackStub, 0x123, 0])
  , routeCase "close-last-count-overstated"
      ((withSlots [(4, direct 0x2c32 0x30 [9])] idle {sessionSet = 0x123 : replicate 19 0, sessionOpen = 1, sessionClass = 2}) {sessionDeferred = 3})
      (sends [0, 0]) (direct 0x2c1d 0x30 [callbackStub, 0x123])
  , routeCase "close-unknown-handle" idle {sessionOpen = 0, sessionClass = 2} (sends [0]) (direct 0x2c1a 0x30 [callbackStub, 0x555])
  , routeCase "close-send-refused" idle {sessionSet = 0x123 : replicate 19 0, sessionOpen = 1, sessionClass = 2} (sends [3]) (direct 0x2c1a 0x30 [callbackStub, 0x123])
  , routeCase "close-alternate-class" idle {sessionClass = 1} (sends []) (direct 0x2c1a 0x30 [callbackStub, 0x123])
  , routeCase "alternate-close" idle {sessionSet = 0x200 : replicate 19 0, sessionOpen = 1, sessionClass = 1} (sends [0]) (direct 0x2c30 0x30 [callbackStub, 0x200])
  , routeCase "alternate-close-primary-class" idle {sessionClass = 2} (sends []) (direct 0x2c30 0x30 [callbackStub, 0x200])
  , routeCase "detach" idle {sessionSet = 0x123 : 0x124 : replicate 18 0, sessionOpen = 2, sessionClass = 2, sessionStatus = 4, sessionHandle = 0x124, sessionSaved = callbackStub} (sends [0]) (direct 0x2c01 0x30 [1])
  , routeCase "detach-send-refused" idle {sessionClass = 2} (sends [9]) (direct 0x2c01 0x30 [])
  , routeCase "detach-alternate-class" idle {sessionClass = 1} (sends []) (direct 0x2c01 0x30 [1])
  ]

-- Interceptor ----------------------------------------------------------------

interceptScenarios :: [Scenario]
interceptScenarios =
  [ interceptCase "intercept-callback" callbackStub 0 0x77
  , interceptCase "intercept-failed" callbackStub 6 0
  , interceptCase "intercept-no-callback" 0 0 0x78
  ]
  where
    interceptCase name saved status handle =
      plain name
        [DataRegion "mediamsg-state" session (sessionBytes idle {sessionSaved = saved}) (sessionBytes idle {sessionStatus = status, sessionHandle = handle, sessionSaved = 0})]
        [Call interceptEntry [status, handle] [] False]
        (complete saved status (Just handle) ++ [service wake [("r0", mediamsgTask)] 0])
        []

-- Media task -------------------------------------------------------------------

-- | The worker table at 0x814740, eight-byte rows of code and worker, ending at code 0.
workers :: B.ByteString -> [(Word32, Word32)]
workers image = takeWhile ((/= 0) . fst) [(wordAt image row, wordAt image (row + 4)) | row <- [workerTable, workerTable + 8 ..]]

workerRegion :: B.ByteString -> DataRegion
workerRegion image =
  unchanged "workers" workerTable (B.take (8 * (length (workers image) + 1)) (B.drop (fromIntegral (workerTable - imageBase)) image))

-- | Each received request's status and code, in order, the last one reaching a worker.
mediaTaskCase :: B.ByteString -> String -> [(Word32, Word32)] -> Scenario
mediaTaskCase image name received =
  Scenario
    { scenarioName = "media-task-" ++ name
    , scenarioEntry = mediaTaskEntry
    , scenarioData = [workerRegion image, DataRegion "media-busy" mediaBusy (word32le 0) (word32le 1)]
    , scenarioCalls = [Call mediaTaskEntry [] [] False]
    , scenarioServices = waitOn 0x3b 0 : concatMap receiveOf (zip [1 :: Int ..] received)
    , scenarioResults = []
    , scenarioStop = lookup (snd (last received)) (workers image)
    }
  where
    receiveOf (index, (status, value)) =
      Service receive [] [("r0", status)] [] [AtRegister "r0" 0 (halfWords value)]
        : [service ticks [("r0", mediaTicks)] 0 | index < length received]

mediaTaskScenarios :: B.ByteString -> [Scenario]
mediaTaskScenarios image =
  [mediaTaskCase image ("worker-" ++ hexWord value) [(0, value)] | (value, _) <- workers image]
    ++ [ mediaTaskCase image "receive-refused" [(0xfffffffc, 0x2c17), (0, 0x2c18)]
       , mediaTaskCase image "unknown-code" [(0, 0x2cff), (0, 0x2c1a)]
       , mediaTaskCase image "alternate-code" [(0, 0x2c2d), (1, 0x2c2d), (0, 0x2c12)]
       ]

-- Scenarios ----------------------------------------------------------------------

plain :: String -> [DataRegion] -> [Call] -> [Service] -> [Word32] -> Scenario
plain name regions calls made results =
  Scenario
    { scenarioName = name
    , scenarioEntry = case calls of
        first : _ -> callEntry first
        [] -> 0
    , scenarioData = regions
    , scenarioCalls = calls
    , scenarioServices = made
    , scenarioResults = results
    , scenarioStop = Nothing
    }

scenarios :: B.ByteString -> [Scenario]
scenarios image =
  translateScenarios image
    ++ readinessScenarios
    ++ resultScenarios
    ++ postScenarios
    ++ posterScenarios
    ++ routeScenarios
    ++ interceptScenarios
    ++ mediaTaskScenarios image
