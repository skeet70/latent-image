module Main where

import Control.Monad (forM_, unless)
import Data.Bits (complement, shiftR, xor, (.&.))
import qualified Data.ByteString as B
import Data.List (stripPrefix)
import Data.Word (Word32)
import FujiAF.Host (withTemporary)
import System.Environment (getArgs)
import System.Exit (ExitCode (..))
import System.FilePath ((</>))
import System.Directory (doesFileExist, removeFile)
import System.Process (readProcessWithExitCode)

newtype RamAddress = RamAddress Word32
newtype Sector = Sector Word32
newtype ByteCount = ByteCount Word32

data Region = Copy RamAddress ByteCount Sector Word32 | Zero RamAddress ByteCount
data Route = Fat | Raw deriving (Show)
data Fault = NoFault | ReadError | ShortRead | BadStack | BadReturn | CacheEnabled | CacheAfterRead

data Outcome = Handoff | Refusal | Budget | BridgeError deriving (Eq, Show)
data Machine = Machine
  { outcome :: Outcome
  , registers :: [Word32]
  , stackPointer :: Word32
  , linkRegister :: Word32
  , programCounter :: Word32
  , processorStatus :: Word32
  , stackWitness :: Word32
  , readCount :: Word32
  }

ensure :: Bool -> String -> IO ()
ensure ok message = unless ok (ioError (userError message))

word :: Word32 -> B.ByteString
word value = B.pack [fromIntegral (value `shiftR` bits) | bits <- [0,8,16,24]]

replace :: Int -> B.ByteString -> B.ByteString -> B.ByteString
replace offset value bytes = B.take offset bytes <> value <> B.drop (offset + B.length value) bytes

crc :: B.ByteString -> Word32
crc = complement . B.foldl' byte (complement 0)
  where
    byte value input = bits 8 (value `xor` fromIntegral input)
    bits :: Int -> Word32 -> Word32
    bits 0 value = value
    bits count value = bits (count-1) ((value `shiftR` 1) `xor` if value .&. 1 == 0 then 0 else 0xedb88320)

metadata :: Sector -> ByteCount -> [Region] -> B.ByteString
metadata (Sector first) (ByteCount bytes) regions =
  let header = B.concat (map word [0x31524d58,1,fromIntegral (length regions),0,first,bytes,0,0])
      descriptor (Copy (RamAddress destination) (ByteCount count) (Sector source) checksum) =
        B.concat (map word [1,destination,count,source,checksum])
      descriptor (Zero (RamAddress destination) (ByteCount count)) =
        B.concat (map word [2,destination,count,0,0])
      sector = header <> B.concat (map descriptor regions)
      padded = sector <> B.replicate (512 - B.length sector) 0
  in replace 12 (word (crc padded)) padded

resign :: B.ByteString -> B.ByteString
resign bytes = let unsigned = replace 12 (word 0) bytes in replace 12 (word (crc unsigned)) unsigned

routeName :: Route -> String
routeName Fat = "fat"
routeName Raw = "raw"

faultName :: Fault -> String
faultName NoFault = "none"
faultName ReadError = "read-error"
faultName ShortRead = "short-read"
faultName BadStack = "bad-stack"
faultName BadReturn = "bad-return"
faultName CacheEnabled = "cache-enabled"
faultName CacheAfterRead = "cache-after-read"

parseMachine :: String -> IO Machine
parseMachine text = do
  let rows = lines text
      field name = case [value | row <- rows, Just value <- [stripPrefix (name ++ "=") row]] of
        [value] -> pure value
        _ -> ioError (userError ("Missing or duplicate machine fact " ++ name))
      number name = do
        value <- field name
        case reads value of
          [(result,"")] -> pure result
          _ -> ioError (userError ("Invalid machine fact " ++ name))
  terminal <- field "status" >>= \value -> case value of
    "handoff" -> pure Handoff
    "refusal" -> pure Refusal
    "budget" -> pure Budget
    "bridge-error" -> pure BridgeError
    _ -> ioError (userError "Unknown terminal status")
  Machine terminal <$> mapM (number . ("r" ++) . show) [0 :: Int ..12]
    <*> number "sp" <*> number "lr" <*> number "pc" <*> number "cpsr"
    <*> number "stack_witness" <*> number "reads"

main :: IO ()
main = do
  args <- getArgs
  (engine,bootstrap) <- case args of
    [engine,bootstrap] -> pure (engine,bootstrap)
    _ -> ioError (userError "Expected ENGINE BOOTSTRAP")
  asset <- B.readFile bootstrap
  ensure (B.length asset == 4096) "Bootstrap is not 4096 bytes"
  ensure (crc (B.pack (map (fromIntegral . fromEnum) "123456789")) == 0xcbf43926) "Fixture CRC known-answer mismatch"
  -- Hand-authored transport bytes are not firmware or native continuation evidence.
  let firstBytes = B.pack (concat (replicate 2 [0..255]))
      secondBytes = word 0xe12fff1e <> B.replicate 1020 0x6d
      body = firstBytes <> secondBytes
      rawPrefix = B.replicate (3107*512) 0xce
      firstLba = Sector 3107
      regions =
        [ Copy (RamAddress 0x40000000) (ByteCount 512) firstLba (crc firstBytes)
        , Zero (RamAddress 0x40002000) (ByteCount 64)
        , Copy (RamAddress 0x40001800) (ByteCount 1024) (Sector 3108) (crc secondBytes)
        , Zero (RamAddress 0x40000300) (ByteCount 16)
        ]
      parameters = metadata firstLba (ByteCount 1536) regions
      entry sector = replace 0x800 sector asset
      initial sector = replace 0 (entry sector) (B.replicate 0x2000000 0xa5)
      unreserved = replace 0xc0000 (B.replicate 0x2000 0)
      expected = replace 0x300 (B.replicate 16 0) $
        replace 0x2000 (B.replicate 64 0) $
        replace 0x1800 secondBytes $ replace 0 firstBytes $ initial parameters
      witness Fat = 0x8000015c
      witness Raw = 0x80000174
  withTemporary "xm1-ram-rescue-protocol" $ \root -> do
    let run label route sector source fault = do
          let entryPath = root </> "entry.bin"
              bodyPath = root </> "body.bin"
              ramPath = root </> "ram.bin"
          B.writeFile entryPath (entry sector)
          B.writeFile bodyPath (case route of Fat -> source; Raw -> rawPrefix <> source)
          forM_ [ramPath,ramPath ++ ".state.json"] $ \path -> do
            existing <- doesFileExist path
            if existing then removeFile path else pure ()
          (code,out,err) <- readProcessWithExitCode engine
            [entryPath,bodyPath,routeName route,ramPath,faultName fault] ""
          machine <- parseMachine out
          ram <- B.readFile ramPath
          ensure (B.length ram == 0x2000000) (label ++ ": incomplete RAM")
          ensure (code == if outcome machine == Handoff then ExitSuccess else ExitFailure 1)
            (label ++ ": bridge failed: " ++ err ++ out)
          pure (machine,ram)
        refused label route sector source fault = do
          (machine,ram) <- run label route sector source fault
          ensure (outcome machine == Refusal) (label ++ ": target did not refuse: " ++ show (outcome machine))
          pure (machine,ram)
    forM_ [Fat,Raw] $ \route -> do
      (machine,ram) <- run "success" route parameters body NoFault
      ensure (outcome machine == Handoff && unreserved ram == unreserved expected) (show route ++ ": independent RAM/handoff mismatch")
      ensure (registers machine == 0 : [0x11000001..0x1100000c]) "Caller registers were not restored"
      ensure (stackPointer machine == 0xfff00f00 && linkRegister machine == witness route &&
        stackWitness machine == witness route && programCounter machine == 0x1800 &&
        processorStatus machine .&. 0xf8000000 == 0 && readCount machine == 3) "Caller frame/flags or transfer count mismatch"
      forM_ [ReadError,ShortRead,CacheAfterRead] $ \fault -> do
        (failure,failedRam) <- refused (faultName fault) route parameters body fault
        ensure (readCount failure == 1 && B.take 64 (B.drop 0x2000 failedRam) == B.replicate 64 0xa5)
          "I/O refusal continued into a later region"
      forM_ [BadStack,BadReturn,CacheEnabled] $ \fault -> do
        (failure,failedRam) <- refused (faultName fault) route parameters body fault
        ensure (readCount failure == 0 && failedRam == initial parameters) "Entry refusal touched RAM or invoked ROM read"
      (corrupt,corruptRam) <- refused "source CRC" route parameters (replace 3 (B.singleton 0xa4) body) NoFault
      ensure (readCount corrupt == 1 && B.take 64 (B.drop 0x2000 corruptRam) == B.replicate 64 0xa5)
        "Source CRC refusal continued into a later region"
      let badFields =
            [ ("magic",0,0), ("version",4,2), ("empty regions",8,0), ("too many regions",8,17)
            , ("reserved",24,1), ("empty body",20,0), ("partial body",20,1535)
            , ("source wrap",16,0xffffffff), ("kind",32,3), ("zero copy",40,0)
            , ("partial copy",40,511), ("low alias DMA",36,0), ("destination wrap",36,0xfffffe00)
            , ("word-aligned partial copy",40,508)
            , ("unaligned DMA destination",36,0x40000004)
            , ("RAM end",36,0x41ffff00), ("protected start",36,0x400c0000)
            , ("protected interior",36,0x400bff00), ("source before extent",44,3106)
            , ("source past extent",44,3110), ("zero source",64,1), ("zero CRC",68,1)
            , ("partial zero",60,63), ("overlapping destinations",56,0x40000000)
            ]
      forM_ badFields $ \(label,offset,value) -> do
        let sector = resign (replace offset (word value) parameters)
        (failure,failedRam) <- refused label route sector body NoFault
        ensure (readCount failure == 0 && unreserved failedRam == unreserved (initial sector)) (label ++ ": preflight performed data effects")
      let badMetadata = replace 12 (word 0) parameters
      (failure,failedRam) <- refused "metadata CRC" route badMetadata body NoFault
      ensure (readCount failure == 0 && unreserved failedRam == unreserved (initial badMetadata)) "Metadata CRC refusal performed data effects"
      (truncated,_) <- refused "truncated body" route parameters (B.take 511 body) NoFault
      ensure (readCount truncated == 1) "Truncated read was retried"
    putStrLn "Independent synthetic Copy/Zero, full RAM, caller-frame and refusal fixtures passed for FAT and raw routes"
