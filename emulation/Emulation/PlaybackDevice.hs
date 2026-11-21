{-# LANGUAGE OverloadedStrings #-}
module Emulation.PlaybackDevice
  ( Batch, Cursor, initialCursor, parseBegin, parseBatch, acceptBatch
  , Programming, programming, programmedBytes, programmingValue
  , Status, statusCompletion, statusValue, startedStatus, followingStatus
  , Completion, completion, endpointValue, outputEventValue, completionValue
  , released, emptyBatch
  ) where

import Control.Monad (unless)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Aeson.Key as K
import Data.Bits ((.&.), (.|.), shiftR)
import qualified Data.ByteString as B
import Data.Foldable (toList)
import qualified Data.Text as T
import Data.Word (Word32, Word64)
import qualified PlaybackContract as C

data Register = Control | StatusRegister | Mode | InputEvent | OutputEvent | Scale | OutputBase | OutputCapacity | Endpoint | InputBase | InputBytes | Command | Flags | Crop | Origin | End deriving (Eq, Show)
data Operation = Read | Write deriving (Eq, Show)
newtype Pc = Pc Word32 deriving (Eq, Show)
newtype Tick = Tick Word64 deriving (Eq, Ord, Show)
data Access = Access C.JpegDeviceService Pc Register Operation Word32 Tick deriving (Eq, Show)
newtype Batch = Batch [Access] deriving (Eq, Show)
newtype Cursor = Cursor (Maybe Tick) deriving (Eq, Show)
initialCursor :: Cursor
initialCursor = Cursor Nothing

ensure :: Bool -> String -> Either String ()
ensure ok message = unless ok (Left message)
field :: T.Text -> Value -> Either String Value
field name (Object fields) = maybe (Left ("missing " ++ T.unpack name)) Right (KM.lookup (K.fromText name) fields)
field _ _ = Left "JPEG observation object required"
number :: A.FromJSON a => Value -> Either String a
number value = case A.fromJSON value of A.Success result -> Right result; A.Error problem -> Left problem
register :: Word32 -> Either String Register
register address = maybe (Left "foreign JPEG register") Right (lookup address
  [(0xfff63000,Control),(0xfff63004,StatusRegister),(0xfff63008,Mode),(0xfff63010,InputEvent),(0xfff63014,OutputEvent)
  ,(0xfff630a0,Scale),(0xfff63100,OutputBase),(0xfff63104,OutputCapacity),(0xfff63108,Endpoint),(0xfff6310c,InputBase)
  ,(0xfff63110,InputBytes),(0xfff63118,Command),(0xfff6311c,Flags),(0xfff63120,Crop),(0xfff63124,Origin),(0xfff63128,End)])
parseBegin :: Value -> Either String ()
parseBegin value = do
  ensure (field "armed" value == Right (Bool True)) "JPEG observer was not armed"
  ensure (field "range" value == Right (A.toJSON ([4294324224,4294324524] :: [Word64]))
    && field "limit" value == Right (A.toJSON (65536 :: Int))) "JPEG observation scope differs"
parseBatch :: [(C.JpegDeviceService, Word32, Word32)] -> Bool -> Value -> Either String Batch
parseBatch spans active value = do
  ensure (field "active" value == Right (Bool active)) "JPEG observation activity differs"
  omitted <- field "omitted" value >>= (number :: Value -> Either String Word64)
  ensure (omitted == 0) "JPEG bus transactions omitted"
  rows <- field "transactions" value >>= \v -> case v of Array xs -> Right (toList xs); _ -> Left "JPEG transactions array required"
  ensure (length rows <= 65536) "JPEG transaction batch exceeds bound"
  accesses <- mapM parse rows
  ensure (ordered accesses) "JPEG transaction ticks reordered"
  pure (Batch accesses)
  where
    parse row = do
      cpu <- field "cpu" row >>= (number :: Value -> Either String Word64)
      size <- field "size" row >>= (number :: Value -> Either String Word64)
      ensure (cpu == 0 && size == 4 && field "accepted" row == Right (Bool True)) "JPEG access rejected or wrong core/width"
      pc <- field "pc" row >>= (number :: Value -> Either String Word32)
      service <- case [s | (s,lo,hi) <- spans, lo <= pc, pc < hi] of [s] -> Right s; _ -> Left "JPEG access PC outside selected immutable device service"
      reg <- field "address" row >>= number >>= register
      operation <- field "operation" row >>= \v -> case v of String "read" -> Right Read; String "write" -> Right Write; _ -> Left "unknown JPEG bus operation"
      word <- field "value" row >>= number
      tick <- Tick <$> (field "tick" row >>= number)
      pure (Access service (Pc pc) reg operation word tick)
    ordered rows = and [a <= b | (Access _ _ _ _ _ a,Access _ _ _ _ _ b) <- zip rows (drop 1 rows)]
acceptBatch :: Cursor -> Batch -> Either String Cursor
acceptBatch cursor@(Cursor previous) (Batch rows) = case rows of
  [] -> Right cursor
  Access _ _ _ _ _ first : _ -> do
    ensure (maybe True (<= first) previous) "JPEG transaction stream moved backwards"
    let Access _ _ _ _ _ final = last rows
    pure (Cursor (Just final))
emptyBatch :: Batch -> Either String ()
emptyBatch (Batch rows) = ensure (null rows) "unconsumed JPEG device effects"

serviceRows :: C.JpegDeviceService -> [Access] -> Bool
serviceRows service = all (\(Access actual _ _ _ _ _) -> actual == service)
facts :: [Access] -> [(Register,Operation,Word32)]
facts = map (\(Access _ _ reg op value _) -> (reg,op,value))
reset :: [Access] -> Either String ()
reset rows = do
  ensure (serviceRows C.JpegReset rows) "reset service attribution differs"
  case facts rows of
    (Command,Write,1) : rest -> do
      let (polls,afterPoll) = span (\(reg,op,_) -> reg == StatusRegister && op == Read) rest
      ensure (not (null polls) && all (\(_,_,v) -> v .&. 0x10000 == 0) (init polls)
        && let (_,_,v) = last polls in v .&. 0x10000 /= 0) "reset status polling incomplete"
      let (_,_,status) = last polls
      case afterPoll of
        (StatusRegister,Write,ack) : (Control,Write,0x100) : controlPolls -> do
          ensure (ack == status && not (null controlPolls)) "reset acknowledgement differs"
          ensure (all (\(r,o,v) -> r == Control && o == Read && v .&. 0x100 /= 0) (init controlPolls)
            && let (r,o,v) = last controlPolls in r == Control && o == Read && v .&. 0x100 == 0) "reset control polling incomplete"
        _ -> Left "reset sequence incomplete or reordered"
    _ -> Left "reset command missing"

newtype Programming = Programming [Access] deriving (Eq, Show)
programming :: Batch -> Either String Programming
programming (Batch rows) = do
  let (resets,program) = span (\(Access service _ _ _ _ _) -> service == C.JpegReset) rows
  ensure (not (null resets)) "programming lacks reset"
  mapM_ reset (splitResets resets)
  ensure (serviceRows C.JpegProgram program) "programming service attribution differs"
  let writes = [(r,v) | (r,Write,v) <- facts program]
  ensure (map fst writes == [Mode,Scale,OutputBase,OutputCapacity,InputBase,InputBytes,Flags,Crop,Origin,End]) "programming writes missing or reordered"
  ensure (all (\(_,o,_) -> o == Write) (facts program)) "unexpected programming read"
  pure (Programming program)
  where
    splitResets [] = []
    splitResets (first:rest) = let (body,next) = break (\(Access _ _ reg op _ _) -> reg == Command && op == Write) rest in (first:body) : splitResets next
programmedBytes :: Programming -> Word32 -> Either String B.ByteString
programmedBytes (Programming rows) scale = do
  let writes = [(r,v) | (r,Write,v) <- facts rows]
  ensure (lookup Mode writes == Just 0xfd0020 && lookup Scale writes == Just scale
    && all (\r -> lookup r writes == Just 0) [Flags,Crop,Origin,End]) "unsupported or mismatched decoder configuration"
  words' <- mapM (\r -> maybe (Left "programmed extent missing") Right (lookup r writes)) [OutputBase,OutputCapacity,InputBase,InputBytes]
  pure (B.pack [fromIntegral (value `shiftR` shift) | value <- words', shift <- [0,8,16,24]])
programmingValue :: Programming -> Value
programmingValue (Programming rows) = selectors rows

newtype Status = Status Word32 deriving (Eq, Show)
statusCompletion :: Status -> Bool
statusCompletion (Status value) = value .&. 0x10 /= 0
statusValue :: Status -> Word32
statusValue (Status value) = value
startedStatus :: Batch -> Either String Status
startedStatus (Batch rows) = case rows of
  a@(Access _ _ Control Read control _) : b@(Access _ _ Control Write start _) : rest -> do
    ensure (serviceRows C.JpegStart [a,b] && start == control .|. 1) "start read/write sequence differs"
    subsequentStatus (Batch rest)
  _ -> Left "start effects missing or reordered"
subsequentStatus :: Batch -> Either String Status
subsequentStatus (Batch [Access service _ StatusRegister Read value _]) = do
  ensure (service == C.JpegStatusRead) "status read attribution differs"
  pure (Status value)
subsequentStatus _ = Left "status read missing or ambiguous"
followingStatus :: Status -> Batch -> Either String Status
followingStatus (Status status) (Batch rows) = case rows of
  Access service _ StatusRegister Write ack _ : rest -> do
    ensure (service == C.JpegStatusAck && ack == status) "noncompletion status acknowledgement differs"
    if status .&. 8 == 0 then subsequentStatus (Batch rest) else case rest of
      Access inputRead _ InputEvent Read value _ : Access inputAck _ InputEvent Write ackValue _ : next -> do
        ensure (inputRead == C.JpegInputRead && inputAck == C.JpegInputAck && value == ackValue) "input event acknowledgement differs"
        subsequentStatus (Batch next)
      _ -> Left "input event effects missing or reordered"
  _ -> Left "noncompletion acknowledgement missing"

data Completion = Completion Word32 Word32 [Access] deriving (Eq, Show)
completion :: Status -> Batch -> Either String Completion
completion (Status status) (Batch rows) = do
  ensure (status .&. 0x10 /= 0) "completion lacks decoded status"
  let expected = [(C.JpegStatusAck,StatusRegister,Write,status)]
      actual = [(s,r,o,v) | Access s _ r o v _ <- rows]
  case actual of
    ack : (outRead,OutputEvent,Read,event) : (outAck,OutputEvent,Write,eventAck) : (endpointRead,Endpoint,Read,endpoint) : [] -> do
      ensure ([ack] == expected && outRead == C.JpegOutputRead && outAck == C.JpegOutputAck
        && event == eventAck && endpointRead == C.JpegEndpointRead) "completion effects missing, reordered or mismatched"
      pure (Completion endpoint event rows)
    _ -> Left "completion device sequence differs"
endpointValue :: Completion -> Word32
endpointValue (Completion endpoint _ _) = endpoint
outputEventValue :: Completion -> Word32
outputEventValue (Completion _ event _) = event
completionValue :: Completion -> Value
completionValue (Completion _ _ rows) = selectors rows
released :: Batch -> Either String ()
released (Batch rows) = reset rows
selectors :: [Access] -> Value
selectors rows = object ["count" .= length rows, "accesses" .= map selector rows]
  where
    selector (Access service (Pc pc) reg op value (Tick tick)) = object
      ["service" .= show service,"pc" .= pc,"register" .= show reg,"operation" .= show op,"value" .= value,"tick" .= tick]
