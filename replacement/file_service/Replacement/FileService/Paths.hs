{-# LANGUAGE OverloadedStrings #-}

-- | The path-worker cases: an independent model of the stock file delete (0x2c0a), filename
-- update (0x2c03), directory delete (0x2c0b), image remove (0x2c25), read-only (0x2c0c) and
-- attribute update (0x2c04) workers, read from the bytes of [0x7118fc,0x7120d8), gives each
-- case's result, the ordered calls into the retained request, path, catalog, device and
-- filesystem services, and the final payload.
module Replacement.FileService.Paths
  ( code
  , scenarios
  ) where

import Data.Bits (shiftL, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word32, Word8)
import Replacement.FileService.Native
import Replacement.FileService.Requests (translation)

code :: Code
code = Code
  { codeImageBase = 0x61c400
  , codeExtents =
      [ ("worker-code", Extent 0x7118fc 0x7120d8)
      , ("translate-code", Extent 0x71d450 0x71d66c)
      ]
  , codeStockFunctions =
      [ (0x7118fc, 0xb4), (0x7119b0, 0x1a0), (0x711b50, 0x78), (0x711bc8, 0x2dc), (0x711ea4, 0xb4), (0x711f58, 0x180)
      , (0x71d450, 0x21c)
      ]
  , codeRetained = []
  , codeRetainedFunctions = []
  }

-- Addresses ------------------------------------------------------------------

fileDeleteWorker, filenameWorker, directoryDeleteWorker, imageRemoveWorker, readOnlyWorker, attributeWorker :: Word32
fileDeleteWorker = 0x7118fc
filenameWorker = 0x7119b0
directoryDeleteWorker = 0x711b50
imageRemoveWorker = 0x711bc8
readOnlyWorker = 0x711ea4
attributeWorker = 0x711f58

deleteFileRequest, deleteDirectoryRequest, readOnlyRequest, catalogRebuild, catalogReload, mediaRefresh, mediaReport :: Word32
deleteFileRequest = 0x71df30
deleteDirectoryRequest = 0x71e08c
readOnlyRequest = 0x71e288
catalogRebuild = 0x719de0
catalogReload = 0x71985c
mediaRefresh = 0x71d66c
mediaReport = 0x726c74

splitPath, pathWildcard, directoryNumber, entryUpdate, entryUpdateOne :: Word32
splitPath = 0x71543c
pathWildcard = 0x71d1b8
directoryNumber = 0x712dac
entryUpdate = 0x714940
entryUpdateOne = 0x713ef4

deviceState, fileNumber, extensionId, extensionKind, catalogFind, fsDelete :: Word32
deviceState = 0x6f52e4
fileNumber = 0x713360
extensionId = 0x713264
extensionKind = 0x713488
catalogFind = 0x71a93c
fsDelete = 0x6ff84c

callbackStub, contextBase, requestBase, payloadBase, pathAddress, workerTable :: Word32
callbackStub = 0x380000
contextBase = 0xa00000
requestBase = 0xc10000
payloadBase = requestBase + 0x10
pathAddress = payloadBase + 4
workerTable = 0x814740

contextBytes, payloadBytes :: Int
contextBytes = 0x3600
payloadBytes = 0x60

-- Model ------------------------------------------------------------------------

-- | Scripted answers of the retained services and the initial context.
data Env = Env
  { envContext :: Bool
  , envFilenameCancel :: Word8
  , envMetadataCancel :: Word8
  , envDevice :: Word32
  , envRecords :: [(Word32, Word32)]
  , envRequest :: Word32
  , envRebuild :: Word32
  , envReload :: Word32
  , envSplit :: Word32
  , envWildcard :: Word32
  , envNumber :: Word32
  , envUpdate :: Word32
  , envState :: Word8
  , envFile :: Word32
  , envFind :: Word32
  , envEntryDirectory :: Word32
  , envEntryFile :: Word32
  , envEntryFlags :: (Word8, Word8, Word8)
  , envExtension :: Word32
  , envKind :: Word32
  , envDelete :: Word32
  }

env :: Env
env = Env
  { envContext = True
  , envFilenameCancel = 0
  , envMetadataCancel = 0
  , envDevice = 7
  , envRecords = [(0x67, 0xb00040)]
  , envRequest = 0
  , envRebuild = 0
  , envReload = 0
  , envSplit = 0
  , envWildcard = 0
  , envNumber = 0x67
  , envUpdate = 0
  , envState = 0
  , envFile = 3097
  , envFind = 0
  , envEntryDirectory = 0x67
  , envEntryFile = 3097
  , envEntryFlags = (0, 1, 0)
  , envExtension = 2
  , envKind = 1
  , envDelete = 0
  }

-- | The payload and the ordered service calls.
data M = M {mPayload :: B.ByteString, mServices :: [Service]}

newtype S a = S {runS :: Env -> B.ByteString -> M -> (a, M)}

instance Functor S where
  fmap f (S g) = S (\e i m -> let (a, m') = g e i m in (f a, m'))

instance Applicative S where
  pure a = S (\_ _ m -> (a, m))
  S f <*> S g = S (\e i m -> let (h, m') = f e i m; (a, m'') = g e i m' in (h a, m''))

instance Monad S where
  S g >>= k = S (\e i m -> let (a, m') = g e i m in runS (k a) e i m')

ask :: S Env
ask = S (\e _ m -> (e, m))

image :: S B.ByteString
image = S (\_ i m -> (i, m))

gets :: (M -> a) -> S a
gets f = S (\_ _ m -> (f m, m))

modify :: (M -> M) -> S ()
modify f = S (\_ _ m -> ((), f m))

made :: Service -> S ()
made service = modify (\m -> m {mServices = mServices m ++ [service]})

-- | A service returning r0 that reads no memory and writes none.
plain :: Word32 -> [(String, Word32)] -> Word32 -> S Word32
plain address inputs result = made (Service address inputs [("r0", result)] [] []) >> pure result

word :: Int -> S Word32
word offset = gets (\m -> wordAt (mPayload m) offset)

wordAt :: B.ByteString -> Int -> Word32
wordAt bytes offset = sum [fromIntegral (B.index bytes (offset + index)) `shiftL` (8 * index) | index <- [0 .. 3]]

byte :: Int -> S Word32
byte offset = gets (\m -> fromIntegral (B.index (mPayload m) offset))

setWord :: Int -> Word32 -> S ()
setWord offset value = modify (\m -> m {mPayload = B.take offset (mPayload m) <> word32le value <> B.drop (offset + 4) (mPayload m)})

setByte :: Int -> Word32 -> S ()
setByte offset value = modify (\m -> m {mPayload = B.take offset (mPayload m) <> B.singleton (fromIntegral value) <> B.drop (offset + 1) (mPayload m)})

-- | 0x71d450 through its jump table, with the media report the case block makes.
translate :: Word32 -> S Word32
translate raw = do
  bytes <- image
  let (status, reported) = translation bytes raw
  mapM_ (\value -> made (Service mediaReport [("r0", 2), ("r1", value)] [("r0", 0)] [] [])) reported
  pure status

-- | blx with the status in r0, skipped for a null callback.
complete :: Word32 -> Word32 -> S ()
complete 0 _ = pure ()
complete callback status = made (Service callback [("r0", status)] [("r0", 0)] [] [])

rebuild :: Word32 -> S Word32
rebuild mode = ask >>= plain catalogRebuild [("r0", mode), ("r1", 0)] . envRebuild

refresh :: S ()
refresh = ask >>= \e -> made (Service mediaRefresh [("r0", envDevice e), ("r1", 0)] [("r0", 0)] [] [])

-- | The context word at +0x2604 + 4 * number.
record :: Word32 -> S Word32
record n = maybe 0 id . lookup n . envRecords <$> ask

-- | The callback load and clear every worker begins with.
begin :: S Word32
begin = do
  callback <- word 0
  setWord 0 0
  pure callback

-- Split frames -----------------------------------------------------------------

-- | The bytes the scripted path splitter writes to the directory and name buffers.
splitDirectory, splitName :: B.ByteString
splitDirectory = "100_FUJI\0"
splitName = "DSCF3097JPG\0"

splitWord :: Word32
splitWord = 4

-- | 0x71543c(path, directory, name, &word, 0, &flag), then the wildcard test at 0x71d1b8 when
-- @wildcardAfter@; returns the split result and the wildcard answer.
split :: Bool -> S (Word32, Word32)
split wildcardAfter = do
  e <- ask
  made
    ( Service splitPath [("r0", pathAddress)] [("r0", envSplit e)] [AtRegister "sp" 0 (word32le 0)]
        [AtRegister "r1" 0 splitDirectory, AtRegister "r2" 0 splitName, AtRegister "r3" 0 (word32le splitWord), AtIndirect "sp" 4 (word32le 0)]
    )
  wild <- if wildcardAfter then wildcard else pure 0
  pure (envSplit e, wild)

wildcard :: S Word32
wildcard = ask >>= plain pathWildcard [("r0", pathAddress)] . envWildcard

number :: S Word32
number = do
  e <- ask
  made (Service directoryNumber [("r1", splitWord)] [("r0", envNumber e)] [AtRegister "r0" 0 splitDirectory] [])
  pure (envNumber e)

-- | The tests at 0x711a54..0x711a88 and 0x711ffc..0x712030: a usable record for a nonzero
-- number above 99 whose word is neither 0 nor -1.
usable :: Word32 -> S (Maybe Word32)
usable 0 = pure Nothing
usable n = do
  r <- record n
  pure (if r == 0 || r == 0xffffffff || n <= 99 then Nothing else Just r)

update :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> S Word32
update address r mode value wild = do
  e <- ask
  made (Service address [("r0", r), ("r2", mode), ("r3", value)] [("r0", envUpdate e)] [AtRegister "r1" 0 splitName, AtRegister "sp" 0 (words32le [wild, 1])] [])
  pure (envUpdate e)

-- Workers ------------------------------------------------------------------------

-- | 0x7118fc and 0x711ea4: the cancel byte, the request helper on the header, then a rebuild
-- after 0, a failure rebuild after anything but 6; returns 0.
requestW :: Word32 -> (Env -> Word8) -> S Word32
requestW helper cancelByte = do
  callback <- begin
  e <- ask
  if not (envContext e)
    then finish callback 5
    else
      if cancelByte e == 1
        then complete callback 11 >> pure 0
        else do
          status <- plainRead helper [("r0", requestBase)] (envRequest e)
          result <-
            if status == 0
              then rebuild 1 >>= translate
              else (if status /= 6 then rebuild 0 >> pure () else pure ()) >> pure status
          finish callback result
  where
    finish callback status = setWord 0 callback >> complete callback status >> pure 0

-- | A request helper call, which finds the payload's callback word clear.
plainRead :: Word32 -> [(String, Word32)] -> Word32 -> S Word32
plainRead address inputs result = made (Service address inputs [("r0", result)] [AtAddress payloadBase 0 (word32le 0)] []) >> pure result

-- | 0x711b50: no cancel byte; the helper receives the worker table row in r1; returns the
-- status.
directoryDeleteW :: S Word32
directoryDeleteW = do
  callback <- begin
  e <- ask
  status <-
    if not (envContext e)
      then pure 5
      else do
        status <- plainRead deleteDirectoryRequest [("r0", requestBase), ("r1", workerTable)] (envRequest e)
        if status == 0 then rebuild 1 >>= translate else pure status
  setWord 0 callback
  complete callback status
  pure status

-- | The tail at 0x711b0c and 0x7120b0.
finishRaw :: Word32 -> Word32 -> S Word32
finishRaw callback raw = do
  setWord 0 callback
  status <- translate raw
  complete callback status
  pure 0

-- | 0x7119b0.
filenameW :: S Word32
filenameW = do
  callback <- begin
  keep <- byte 0x34
  e <- ask
  if not (envContext e)
    then finishRaw callback 2
    else
      if envFilenameCancel e == 1
        then complete callback 11 >> pure 0
        else do
          (splitRaw, wild) <- split True
          if splitRaw /= 0
            then finishRaw callback splitRaw
            else do
              n <- number
              found <- usable n
              let tailAt raw = if keep /= 0 then pure raw else rebuild 1
              raw <- case found of
                Nothing -> refresh >> tailAt 0
                Just r -> do
                  updated <- update entryUpdate r 0 0 wild
                  refresh
                  if updated == 22 || updated == 0
                    then tailAt updated
                    else do
                      if updated /= 7 then rebuild 0 >> pure () else pure ()
                      pure updated
              finishRaw callback raw

-- | 0x711f58.
attributeW :: S Word32
attributeW = do
  callback <- begin
  selector <- byte 0x38
  e <- ask
  if not (envContext e)
    then finishRaw callback 2
    else
      if envMetadataCancel e == 1
        then complete callback 11 >> pure 0
        else do
          (splitRaw, wild) <- split True
          if splitRaw /= 0
            then finishRaw callback splitRaw
            else do
              n <- number
              found <- usable n
              raw <- case found of
                Nothing -> if selector /= 0 then pure 0 else reload
                Just r
                  | selector == 0 -> do
                      value <- word 0x34
                      updated <- update entryUpdate r 1 value wild
                      if updated /= 0 then reload >> pure updated else reload
                  | selector == 1 -> word 0x34 >>= \value -> update entryUpdateOne r 1 value wild
                  | otherwise -> pure 0
              finishRaw callback raw
  where
    reload = ask >>= plain catalogReload [] . envReload

-- | 0x711bc8; the callback word stays clear and the worker returns 0.
imageRemoveW :: S Word32
imageRemoveW = do
  callback <- begin
  e <- ask
  raw <- if envContext e then removeChecks else pure 2
  status <- translate raw
  complete callback status
  pure 0

-- | The loop at 0x711c04..0x711c3c: uppercases a-z through the terminator or index 47 and
-- returns r4, one past the last index processed.
uppercase :: Int -> S Word32
uppercase index = do
  b <- byte (4 + index)
  if (b - 97) .&. 0xff <= 25 then setByte (4 + index) (b - 32) else pure ()
  let next = index + 1
  if b /= 0 && next <= 47 then uppercase next else pure (fromIntegral next)

removeChecks :: S Word32
removeChecks = do
  e <- ask
  end <- uppercase 0
  drive <- byte 4
  colon <- byte 5
  if drive <= 64 || colon /= 58 || drive > 90
    then pure 4
    else do
      made (Service deviceState [("r0", drive - 65)] [("r0", 0)] [] [AtRegister "r1" 0 (B.singleton (envState e))])
      let state = envState e
      if state .&. 1 /= 0
        then pure 1
        else
          if state .&. 0x1a /= 0
            then pure 2
            else
              if state .&. 4 /= 0
                then pure 18
                else do
                  (splitRaw, _) <- split False
                  if splitRaw /= 0 then pure splitRaw else afterSplit end

afterSplit :: Word32 -> S Word32
afterSplit end = do
  e <- ask
  n <- number
  made (Service fileNumber [] [("r0", envFile e)] [AtRegister "r0" 0 splitName] [])
  if envFile e == 0 || n == 0
    then pure 4
    else do
      wild <- wildcard
      if wild == 1
        then pure 4
        else do
          let (first, second, third) = envEntryFlags e
              entry =
                B.replicate 0x6c 0 <> B.take 2 (word32le (envEntryDirectory e)) <> B.take 2 (word32le (envEntryFile e))
                  <> B.replicate 0xc 0 <> B.pack [first, second, third]
          made (Service catalogFind [("r0", 0), ("r1", 0), ("r2", 0), ("r3", 0xffffffff)] [("r0", envFind e)] [AtRegister "sp" 0 (word32le 0)] [AtIndirect "sp" 4 entry])
          if envFind e /= 0
            then pure (envFind e)
            else
              if n /= envEntryDirectory e .&. 0xffff || envFile e /= envEntryFile e .&. 0xffff
                then pure 4
                else if end <= 5 then pure 4 else dotSearch (end - 5) end

-- | 0x711d20..0x711d7c: a dot at r5 from end - 5 below end.
dotSearch :: Word32 -> Word32 -> S Word32
dotSearch at end
  | at >= end = miss
  | otherwise = do
      b <- byte (4 + fromIntegral at)
      if b == 46 then found (at + 1) else dotSearch (at + 1) end
  where
    miss = ask >>= plain extensionId [("r0", pathAddress)] . envExtension >> pure 4
    found next = do
      e <- ask
      ext <- plain extensionId [("r0", pathAddress + next)] (envExtension e)
      if ext == 0
        then pure 4
        else do
          kind <- plain extensionKind [("r0", ext)] (envKind e)
          let (first, second, third) = envEntryFlags e
          if kind == 0 || first .&. 0x80 /= 0 || second .&. 0xf == 0
            then pure 4
            else
              if third .&. 1 /= 0
                then pure 17
                else do
                  deleted <- plain fsDelete [("r0", pathAddress), ("r1", 0)] (envDelete e)
                  if deleted /= 0 then pure deleted else rebuild 1

-- Cases ------------------------------------------------------------------------------

data Worker = Worker {workerEntry :: Word32, workerCode :: Word32, workerModel :: S Word32}

contextRegion :: Env -> B.ByteString
contextRegion e = foldl put (B.replicate contextBytes 0) fields
  where
    fields =
      [(0x114, word32le (envDevice e)), (0x1f6, B.pack [envFilenameCancel e, envMetadataCancel e])]
        ++ [(0x2604 + 4 * fromIntegral n, word32le r) | (n, r) <- envRecords e]
    put bytes (offset, value) = B.take offset bytes <> value <> B.drop (offset + B.length value) bytes

requestRegion :: Word32 -> B.ByteString -> B.ByteString
requestRegion requestCode payload = words32le [requestCode .|. 0x65 `shiftL` 16 .|. 0x1c `shiftL` 24, fromIntegral (B.length payload), payloadBase, 0] <> payload

workerCase :: B.ByteString -> String -> Worker -> Env -> [(Int, Word32)] -> [(Int, B.ByteString)] -> Scenario
workerCase bytes name worker e words' strings =
  Scenario
    { scenarioName = "paths-" ++ name
    , scenarioEntry = workerEntry worker
    , scenarioData =
        [ DataRegion "context-slot" 0x910204 (word32le (if envContext e then contextBase else 0)) (word32le (if envContext e then contextBase else 0))
        , DataRegion "request" requestBase (requestRegion (workerCode worker) payload) (requestRegion (workerCode worker) (mPayload final))
        ]
          ++ [DataRegion "context" contextBase (contextRegion e) (contextRegion e) | envContext e]
    , scenarioCalls = [Call (workerEntry worker) [requestBase, workerTable, workerCode worker, workerEntry worker] [] True]
    , scenarioServices = mServices final
    , scenarioResults = [result]
    , scenarioStop = Nothing
    }
  where
    payload = foldl (\b (offset, value) -> B.take offset b <> value <> B.drop (offset + B.length value) b) (B.replicate payloadBytes 0)
      ([(offset, word32le value) | (offset, value) <- words'] ++ strings)
    (result, final) = runS (workerModel worker) e bytes (M payload [])

fileDeleteW, filenameWorkerW, directoryDeleteWorkerW, imageRemoveWorkerW, readOnlyW, attributeWorkerW :: Worker
fileDeleteW = Worker fileDeleteWorker 0x2c0a (requestW deleteFileRequest envFilenameCancel)
filenameWorkerW = Worker filenameWorker 0x2c03 filenameW
directoryDeleteWorkerW = Worker directoryDeleteWorker 0x2c0b directoryDeleteW
imageRemoveWorkerW = Worker imageRemoveWorker 0x2c25 imageRemoveW
readOnlyW = Worker readOnlyWorker 0x2c0c (requestW readOnlyRequest envMetadataCancel)
attributeWorkerW = Worker attributeWorker 0x2c04 attributeW

imagePath, lowerPath :: B.ByteString
imagePath = C.pack "B:/DCIM/100_FUJI/DSCF3097.JPG"
lowerPath = C.pack "b:/dcim/100_fuji/dscf3097.jpg"

scenarios :: B.ByteString -> [Scenario]
scenarios bytes =
  concat
    [ requestCases "file-delete" fileDeleteW (\e v -> e {envFilenameCancel = v}) (\e v -> e {envMetadataCancel = v})
    , requestCases "read-only" readOnlyW (\e v -> e {envMetadataCancel = v}) (\e v -> e {envFilenameCancel = v})
    , [ directoryDelete "completed" env {envRebuild = 9} request
      , directoryDelete "failed" env {envRequest = 3} request
      , directoryDelete "six" env {envRequest = 6} request
      , directoryDelete "no-context" env {envContext = False} request
      , directoryDelete "silent" env []
      ]
    , [ filename "updated-rebuilt" env {envWildcard = 1, envRebuild = 9} 0
      , filename "updated-kept" env 1
      , filename "kept-alias-rebuilt" env {envUpdate = 22} 0
      , filename "kept-alias-delivered" env {envUpdate = 22} 1
      , filename "failed-reset" env {envUpdate = 9} 0
      , filename "failed-seven" env {envUpdate = 7} 0
      , filename "number-zero" env {envNumber = 0} 0
      , filename "record-zero" env {envRecords = []} 0
      , filename "record-minus-one" env {envRecords = [(0x67, 0xffffffff)]} 0
      , filename "number-below-100" env {envNumber = 99, envRecords = [(99, 0xb00040)]} 0
      , filename "unnumbered-kept" env {envNumber = 0} 1
      , filename "split-refused" env {envSplit = 4} 0
      , filename "no-context" env {envContext = False} 0
      , filename "cancelled" env {envFilenameCancel = 1} 0
      , filename "metadata-byte-ignored" env {envMetadataCancel = 1} 1
      ]
    , [ attribute "update-reloaded" env {envWildcard = 1, envReload = 13} 0
      , attribute "update-failed-reloaded" env {envUpdate = 9, envReload = 13} 0
      , attribute "update-one" env {envUpdate = 9} 1
      , attribute "other-selector" env 2
      , attribute "unnumbered-reloaded" env {envRecords = [], envReload = 13} 0
      , attribute "unnumbered-selected" env {envNumber = 99, envRecords = [(99, 0xb00040)]} 1
      , attribute "split-refused" env {envSplit = 5} 0
      , attribute "no-context" env {envContext = False} 0
      , attribute "cancelled" env {envMetadataCancel = 1} 0
      , attribute "filename-byte-ignored" env {envFilenameCancel = 1} 1
      ]
    , [ remove "removed" env {envRebuild = 9} lowerPath
      , remove "removed-upper" env imagePath
      , remove "unterminated-tail" env (C.pack "b:/a.jpg\0xyz")
      , remove "overlong" env (B.replicate 0x30 0x71 <> C.pack "q")
      , remove "drive-at" env (C.pack "@:/A.JPG")
      , remove "drive-bracket" env (C.pack "[:/A.JPG")
      , remove "no-colon" env (C.pack "B/DCIM/A.JPG")
      , remove "device-locked" env {envState = 0x1f} imagePath
      , remove "device-absent" env {envState = 0x08} imagePath
      , remove "device-full" env {envState = 0x04} imagePath
      , remove "split-refused" env {envSplit = 5} imagePath
      , remove "number-zero" env {envNumber = 0} imagePath
      , remove "file-zero" env {envFile = 0} imagePath
      , remove "wildcard" env {envWildcard = 1} imagePath
      , remove "find-failed" env {envFind = 8} imagePath
      , remove "other-directory" env {envEntryDirectory = 0x68} imagePath
      , remove "other-file" env {envEntryFile = 3098} imagePath
      , remove "short" env (C.pack "B:/A")
      , remove "dot-outside" env (C.pack "b:/dcim/100_fuji/dscf3097.jpeg")
      , remove "extension-unknown" env {envExtension = 0} imagePath
      , remove "kind-unknown" env {envKind = 0} imagePath
      , remove "entry-refused" env {envEntryFlags = (0x80, 1, 0)} imagePath
      , remove "entry-unkinded" env {envEntryFlags = (0, 0x10, 0)} imagePath
      , remove "entry-read-only" env {envEntryFlags = (0, 1, 1)} imagePath
      , remove "delete-failed" env {envDelete = 9} imagePath
      , remove "no-context" env {envContext = False} lowerPath
      , removeSilent "silent" env imagePath
      ]
    ]
  where
    request = [(0, callbackStub)]
    requestCases name worker setOwn setOther =
      [ workerCase bytes (name ++ "-completed") worker env {envRebuild = 9} request strings
      , workerCase bytes (name ++ "-failed") worker env {envRequest = 3} request strings
      , workerCase bytes (name ++ "-six") worker env {envRequest = 6} request strings
      , workerCase bytes (name ++ "-no-context") worker env {envContext = False} request strings
      , workerCase bytes (name ++ "-cancelled") worker (setOwn env 1) request strings
      , workerCase bytes (name ++ "-cancelled-silent") worker (setOwn env 1) [] strings
      , workerCase bytes (name ++ "-other-cancel-ignored") worker (setOther env 1) request strings
      , workerCase bytes (name ++ "-silent") worker env [] strings
      ]
      where
        strings = [(4, imagePath)]
    directoryDelete name e words' = workerCase bytes ("directory-delete-" ++ name) directoryDeleteWorkerW e words' [(4, imagePath)]
    filename name e keep = workerCase bytes ("filename-" ++ name) filenameWorkerW e request [(4, imagePath), (0x34, B.singleton keep)]
    attribute name e selector = workerCase bytes ("attribute-" ++ name) attributeWorkerW e (request ++ [(0x34, 0x21)]) [(4, imagePath), (0x38, B.singleton selector)]
    remove name e path = workerCase bytes ("image-remove-" ++ name) imageRemoveWorkerW e request [(4, path)]
    removeSilent name e path = workerCase bytes ("image-remove-" ++ name) imageRemoveWorkerW e [] [(4, path)]
