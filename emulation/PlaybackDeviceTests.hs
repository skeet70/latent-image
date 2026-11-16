{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless, forM_)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import Data.Either (isLeft, isRight)
import qualified Data.Text as T
import Data.Word (Word32)
import qualified Emulation.PlaybackDevice as P
import qualified PlaybackContract as C

check :: String -> Bool -> IO ()
check name ok = unless ok (fail name)

spans :: [(C.JpegDeviceService,Word32,Word32)]
spans = [(C.JpegReset,0x1000,0x1100),(C.JpegProgram,0x1100,0x1200),(C.JpegStart,0x1200,0x1300)
  ,(C.JpegStatusRead,0x1300,0x1400),(C.JpegStatusAck,0x1400,0x1500),(C.JpegInputRead,0x1500,0x1600)
  ,(C.JpegInputAck,0x1600,0x1700),(C.JpegOutputRead,0x1700,0x1800),(C.JpegOutputAck,0x1800,0x1900)
  ,(C.JpegEndpointRead,0x1900,0x1a00)]
row :: Word32 -> Word32 -> T.Text -> Word32 -> Value
row pc address operation value = object ["cpu" .= (0 :: Int),"size" .= (4 :: Int),"accepted" .= True
  ,"pc" .= pc,"address" .= address,"operation" .= operation,"value" .= value,"tick" .= (42 :: Int)]
resetRows :: [Value]
resetRows = [row 0x1004 0xfff63118 "write" 1,row 0x1010 0xfff63004 "read" 0x10000
  ,row 0x1020 0xfff63004 "write" 0x10000,row 0x1030 0xfff63000 "write" 0x100,row 0x1040 0xfff63000 "read" 0]
programRows :: [Value]
programRows = [row 0x1104 0xfff63008 "write" 0xfd0020,row 0x1108 0xfff630a0 "write" 2
  ,row 0x1110 0xfff63100 "write" 0x40100000,row 0x1114 0xfff63104 "write" 0x40000
  ,row 0x1118 0xfff6310c "write" 0x40200000,row 0x111c 0xfff63110 "write" 0x18000
  ,row 0x1120 0xfff6311c "write" 0,row 0x1124 0xfff63120 "write" 0
  ,row 0x1128 0xfff63124 "write" 0,row 0x112c 0xfff63128 "write" 0]
startRows :: [Value]
startRows = [row 0x1204 0xfff63000 "read" 0x40,row 0x1208 0xfff63000 "write" 0x41,row 0x1304 0xfff63004 "read" 0x10]
completionRows :: [Value]
completionRows = [row 0x1404 0xfff63004 "write" 0x10,row 0x1704 0xfff63014 "read" 0x12345678
  ,row 0x1804 0xfff63014 "write" 0x12345678,row 0x1904 0xfff63108 "read" 0x40130000]
batchValue :: [Value] -> Value
batchValue rows = object ["active" .= True,"transactions" .= rows,"omitted" .= (0 :: Int)]
parse :: [Value] -> Either String P.Batch
parse = P.parseBatch spans True . batchValue
put :: T.Text -> Value -> Value -> Value
put name value (Object fields) = Object (KM.insert (K.fromText name) value fields)
put _ _ value = value

main :: IO ()
main = do
  programmed <- either fail pure (parse (resetRows ++ programRows) >>= P.programming)
  check "programmed extents come from accepted stores" (P.programmedBytes programmed 2 == Right (B.pack
    [0,0,16,64,0,0,4,0,0,0,32,64,0,128,1,0]))
  check "configuration mismatch refused" (isLeft (P.programmedBytes programmed 1))
  forM_ [0..14] $ \index -> check "every reset and programming access required"
    (isLeft (parse (take index (resetRows ++ programRows) ++ drop (index+1) (resetRows ++ programRows)) >>= P.programming))
  let swapped = take 2 programRows ++ [programRows !! 3,programRows !! 2] ++ drop 4 programRows
  check "reordered programming refused" (isLeft (parse (resetRows ++ swapped) >>= P.programming))
  check "programming read refused" (isLeft (parse (resetRows ++ row 0x1104 0xfff63008 "read" 0 : programRows) >>= P.programming))
  let delayedReset = take 1 resetRows ++ [row 0x1010 0xfff63004 "read" 0,row 0x1010 0xfff63004 "read" 0] ++ drop 1 resetRows
  check "poll count is measured rather than guessed" (isRight (parse (delayedReset ++ programRows) >>= P.programming))
  status <- either fail pure (parse startRows >>= P.startedStatus)
  check "completion status comes from bus read" (P.statusCompletion status && P.statusValue status == 0x10)
  pending <- either fail pure (parse (take 2 startRows ++ [row 0x1304 0xfff63004 "read" 0x1000000]) >>= P.startedStatus)
  check "noncompletion status stays noncomplete" (not (P.statusCompletion pending))
  next <- either fail pure (parse [row 0x1404 0xfff63004 "write" 0x1000000,row 0x1304 0xfff63004 "read" 0x10] >>= P.followingStatus pending)
  check "subsequent IRQ requires acknowledged prior status" (P.statusCompletion next)
  check "missing prior status ACK refused" (isLeft (parse [row 0x1304 0xfff63004 "read" 0x10] >>= P.followingStatus pending))
  forM_ [0..2] $ \index -> check "start and status accesses required"
    (isLeft (parse (take index startRows ++ drop (index+1) startRows) >>= P.startedStatus))
  done <- either fail pure (parse completionRows >>= P.completion status)
  check "endpoint and output event are actual reads" (P.endpointValue done == 0x40130000 && P.outputEventValue done == 0x12345678)
  forM_ [0..3] $ \index -> check "completion access omissions refused"
    (isLeft (parse (take index completionRows ++ drop (index+1) completionRows) >>= P.completion status))
  check "completion reorder refused" (isLeft (parse (take 1 completionRows ++ reverse (drop 1 completionRows)) >>= P.completion status))
  check "event ACK must match actual read" (isLeft (parse (take 2 completionRows ++ [row 0x1804 0xfff63014 "write" 0] ++ drop 3 completionRows) >>= P.completion status))
  check "release consumes actual reset" (isRight (parse resetRows >>= P.released))
  check "release missing device reset refused" (isLeft (parse [] >>= P.released))
  check "foreign PC refused" (isLeft (parse [row 0x2000 0xfff63004 "read" 0x10]))
  check "valid PC in wrong service refused" (isLeft (parse (put "pc" (Number 4356) (head startRows) : tail startRows) >>= P.startedStatus))
  let access = head startRows
  forM_ [("accepted",Bool False),("cpu",Number 1),("size",Number 2),("operation",String "fetch"),("address",Number 4294324524)] $ \(name,value) ->
    check "rejected or out of contract access refused" (isLeft (parse [put name value access]))
  check "omitted rows refused" (isLeft (P.parseBatch spans True (put "omitted" (Number 1) (batchValue []))))
  check "observation bound enforced" (isLeft (parse (replicate 65537 access)))
  let armed = object ["armed" .= True,"range" .= ([4294324224,4294324524] :: [Integer]),"limit" .= (65536 :: Int)]
  check "scoped begin admitted" (P.parseBegin armed == Right ())
  check "broader begin refused" (isLeft (P.parseBegin (put "range" (Number 0) armed)))
  check "inactive drain refused" (isLeft (P.parseBatch spans True (put "active" (Bool False) (batchValue []))))
  check "tick reorder refused" (isLeft (parse [access,put "tick" (Number 41) access]))
  first <- either fail pure (parse [access])
  cursor <- either fail pure (P.acceptBatch P.initialCursor first)
  earlier <- either fail pure (parse [put "tick" (Number 41) access])
  check "cross batch tick reorder refused" (isLeft (P.acceptBatch cursor earlier))
  check "end accepts inactive read-only tail" (isRight (P.parseBatch spans False (put "active" (Bool False) (batchValue []))))
