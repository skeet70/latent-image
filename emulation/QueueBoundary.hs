{-# LANGUAGE OverloadedStrings #-}
module Main where

import Data.Aeson (FromJSON(..), eitherDecodeStrict', withObject, withScientific, withText, (.:))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Word (Word32)
import FujiAF.Host (ensure, runBounded)
import System.Directory (createDirectory, doesFileExist, doesPathExist, makeAbsolute, setCurrentDirectory)
import System.Environment (getArgs, getExecutablePath, lookupEnv)
import System.Exit (ExitCode(..))
import System.FilePath ((</>))

newtype GuestAddress = GuestAddress Word32 deriving (Eq, Show)
instance FromJSON GuestAddress where
  parseJSON = withScientific "guest address" $ \value ->
    let integer = floor value :: Integer in
    if fromInteger integer == value && 0 <= integer && integer <= 0xffffffff
      then pure (GuestAddress (fromInteger integer)) else fail "guest address outside u32"

data KernelResult = Success | Unavailable | ContextRefused deriving (Eq, Show)
instance FromJSON KernelResult where
  parseJSON = withScientific "kernel result" $ \value -> case value of
    0 -> pure Success
    0xffffffce -> pure Unavailable
    0xffffffe7 -> pure ContextRefused
    _ -> fail "unknown kernel result"
data Context = UnmaskedSvcSelectorClear | MaskedSvcSelectorClear | MaskedSvcSelectorSet deriving (Eq, Show)
instance FromJSON Context where
  parseJSON = withText "context" $ \value -> case value of
    "unmasked-svc-selector-clear" -> pure UnmaskedSvcSelectorClear
    "masked-svc-selector-clear" -> pure MaskedSvcSelectorClear
    "masked-svc-selector-set" -> pure MaskedSvcSelectorSet
    _ -> fail "unknown context"
data Occupancy = Full | OneFree deriving (Eq, Show)
instance FromJSON Occupancy where
  parseJSON = withText "occupancy" $ \value -> case value of
    "full" -> pure Full
    "one-free" -> pure OneFree
    _ -> fail "unknown occupancy"
data Report = Report
  { context :: Context, occupancy :: Occupancy, callerReturn :: KernelResult
  , drained :: [GuestAddress], nextReturn :: KernelResult, nextAllocation :: GuestAddress
  , drainEmptyReturn :: KernelResult, postAllocationReceiveReturn :: KernelResult
  } deriving Show
instance FromJSON Report where
  parseJSON = withObject "queue report" $ \value -> Report
    <$> value .: "context" <*> value .: "occupancy" <*> value .: "caller_return"
    <*> value .: "drained" <*> value .: "next_allocation_return" <*> value .: "next_allocation"
    <*> value .: "drain_empty_return" <*> value .: "post_allocation_receive_return"

accept :: Report -> IO ()
accept result = do
  let old = GuestAddress 0x01010100
      oldSecond = GuestAddress 0x01010180
      block = GuestAddress 0x01010200
      (returned, deliveries, allocation) = case (context result, occupancy result) of
        (UnmaskedSvcSelectorClear, Full) -> (Unavailable, [old, oldSecond], Success)
        (UnmaskedSvcSelectorClear, OneFree) -> (Success, [old, block], Unavailable)
        (MaskedSvcSelectorClear, Full) -> (Unavailable, [old, oldSecond], Unavailable)
        (MaskedSvcSelectorSet, Full) -> (Unavailable, [old, oldSecond], Unavailable)
        _ -> error "unexpected queue case"
  ensure (callerReturn result == returned) (show (context result) ++ ": caller return differs")
  ensure (drained result == deliveries) (show (context result) ++ ": delivered queue differs")
  ensure (nextReturn result == allocation) (show (context result) ++ ": allocation ownership differs")
  ensure (allocation /= Success || nextAllocation result == block) "released block is not reusable"
  ensure (drainEmptyReturn result == Unavailable && postAllocationReceiveReturn result == Unavailable) "a nonblocking receive found a later queued block"

main :: IO ()
main = do
  lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= mapM_ setCurrentDirectory
  args <- getArgs
  (relativeRunner, capture, image, output) <- case args of
    [r, c, i, o] -> pure (r, c, i, o)
    _ -> ioError (userError "usage: queue-boundary CAPTURE IMAGE NEW-OUTPUT-DIRECTORY")
  executable <- getExecutablePath
  let runner = (executable ++ ".runfiles") </> relativeRunner
  present <- doesFileExist runner
  ensure present "queue boundary worker is absent from runfiles"
  exists <- doesPathExist output
  ensure (not exists) "output directory already exists"
  createDirectory output
  absolute <- makeAbsolute output
  let receipt = absolute </> "queue-boundary.json"
  (safetyTimed, safetyExit, safetyOut, safetyErr) <- runBounded 20000000 (1024*1024) runner ["--safety", absolute </> "safety-boundary.json"] B.empty
  B.writeFile (absolute </> "safety-stdout") safetyOut
  B.writeFile (absolute </> "safety-stderr") safetyErr
  ensure (not safetyTimed && safetyExit == ExitSuccess) ("safety boundary refused: " ++ BC.unpack safetyErr)
  (timed, exit, stdout, stderr) <- runBounded 20000000 (1024*1024) runner [capture, image, receipt] B.empty
  B.writeFile (absolute </> "stdout") stdout
  B.writeFile (absolute </> "stderr") stderr
  ensure (not timed && exit == ExitSuccess) ("queue worker refused: " ++ BC.unpack stderr)
  bytes <- B.readFile receipt
  reports <- either (ioError . userError) pure (eitherDecodeStrict' bytes)
  ensure (map (\r -> (context r, occupancy r)) reports == [(UnmaskedSvcSelectorClear,Full),(UnmaskedSvcSelectorClear,OneFree),(MaskedSvcSelectorClear,Full),(MaskedSvcSelectorSet,Full)]) "queue case matrix differs"
  mapM_ accept reports
  putStrLn "queue-boundary: declared delivery and ownership outcomes matched"
