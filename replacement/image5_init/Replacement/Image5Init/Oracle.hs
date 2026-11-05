{-# LANGUAGE OverloadedStrings #-}
module Replacement.Image5Init.Oracle (runOracle, caseCount) where

import Prelude hiding (logBase)
import Control.Monad (forM, unless)
import Data.Aeson (Value, object, (.=))
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import qualified Data.ByteString as B
import Data.List (nub, sortOn)
import Data.Word (Word32)
import qualified Emulation.NativeObservation as N
import Emulation.OwnedCase
import FujiAF.JSON (encodeCanonical)
import Numeric (readHex)
import System.Directory (createDirectoryIfMissing)
import System.FilePath ((</>), takeDirectory)

newtype CallNumber = CallNumber Int deriving (Eq, Show)
newtype Target = Target Word32 deriving (Eq, Show)
data Injection = AllZero | DirectNonzero CallNumber | HookNonzero Int deriving (Eq, Show)
data Table = FullTable | OneHook deriving (Eq, Show)
data Row = Row String Injection Table deriving (Eq, Show)
data Body = Body B.ByteString Word32

targets :: [Target]
targets = map Target [0x72872c,0x6c62a8,0x73aca4,0xed4acc,0xed4afc,0x69cf84,0xd9856c,0xd986b0,0xd80d44,0x1056c24,0xdb01fc,0xec5350,0xdca604,0xdb63e0,0xed10b4,0xef8c90,0xed2b00,0x1055d10,0x6b7df8,0x69f000,0x72872c]

hooks :: [Target]
hooks = map Target [0x107277c,0x1066304,0x1062114]

checked :: [Int]
checked = [4,9,10,12,13,14,15,17,18,19,20]

baseline :: Row
baseline = Row "all-zero" AllZero FullTable

rows :: [Row]
rows = [baseline]
  ++ [Row ("checked-" ++ show n) (DirectNonzero (CallNumber n)) FullTable | n <- checked]
  ++ [Row ("ignored-" ++ show n) (DirectNonzero (CallNumber n)) FullTable | n <- [1,2,3,5,6,8,11,16]]
  ++ [Row "one-hook" AllZero OneHook, Row "hook-nonzero" (HookNonzero 1) FullTable, Row "ignored-7" (HookNonzero 3) FullTable]

caseCount :: Int
caseCount = length rows

base, entry, stack, logBase, exitAddress, stubBase :: Word32
base = 0xd9856c
entry = 0xd985a4
stack = 0x30001000
logBase = 0x31000000
exitAddress = 0x32000000
stubBase = 0x02000000

sentinel :: N.Register -> Word32
sentinel r = 0xa5a50000 + fromIntegral (fromEnum r)

address :: Word32 -> N.GuestAddress
address = N.GuestAddress . fromIntegral

region :: String -> Word32 -> Permission -> B.ByteString -> CaseRegion
region name at permission bytes = mkRegion (N.RegionId name) (address at) (B.length bytes) permission bytes

loadImmediate :: Int -> Word32 -> [Word32]
loadImmediate r value = [half 0xe3000000 value, half 0xe3400000 (value `shiftR` 16)]
  where half opcode v = opcode .|. ((v .&. 0xf000) `shiftL` 4) .|. (fromIntegral r `shiftL` 12) .|. (v .&. 0xfff)

branchTo :: Word32 -> Word32 -> Word32
branchTo at to = 0xea000000 .|. (fromIntegral ((toInteger to - toInteger at - 8) `div` 4) .&. 0xffffff)

-- Each record holds r0-r11, SP, and the target. Only r0 and r12 are scratch.
stub :: Row -> Target -> B.ByteString
stub (Row _ injection _) target@(Target destination) = assemble $
  loadImmediate 12 logBase ++ [0xe59cc000,0xe88c2fff]
  ++ loadImmediate 0 destination ++ [0xe58c0034,0xe28cc038]
  ++ loadImmediate 0 logBase ++ [0xe580c000,0xe24cc038]
  ++ result ++ [0xe12fff1e]
  where
    result
      | target == Target 0x72872c = [0xe59c0000] ++ if injection == DirectNonzero (CallNumber 1)
          then loadImmediate 12 0x01010100 ++ [0xe150000c,0x03a00001] else []
      | otherwise = loadImmediate 0 (if injected then 1 else 0)
    injected = case injection of
      DirectNonzero (CallNumber n) -> targets !! (n-1) == target
      HookNonzero n -> hooks !! (n-1) == target
      AllZero -> False

expectedTargets :: Row -> [Target]
expectedTargets (Row _ injection table) = concat
  [ if n == 7 then take (if table == OneHook then 1 else 3) hooks else [target]
  | (n,target) <- zip [1..] targets, n <= lastCall ]
  where lastCall = case injection of
          DirectNonzero (CallNumber n) | n `elem` checked -> n
          _ -> 21

expectedStatus :: Row -> N.RunnerStatus
expectedStatus (Row _ (DirectNonzero (CallNumber n)) _) | n `elem` checked = N.InstructionBudget
expectedStatus _ = N.Returned

codeRegions :: [(Word32,B.ByteString)] -> [CaseRegion]
codeRegions segments = [region ("code-" ++ show n) at ReadExecute bytes | (n,(at,bytes)) <- zip [0 :: Int ..] (merge (sortOn fst segments))]
  where
    merge ((a,x):(b,y):rest)
      | b >= a + fromIntegral (B.length x) && b-a-fromIntegral (B.length x) <= 0x20000 =
          merge ((a,x <> B.replicate (fromIntegral (b-a)-B.length x) 0 <> y):rest)
      | otherwise = (a,x):merge ((b,y):rest)
    merge remaining = remaining

caseSpec :: String -> Body -> Row -> Maybe Word32 -> CaseSpec
caseSpec role (Body bytes walk) row@(Row name _ table) stop = CaseSpec
  { caseId = role ++ "-" ++ name
  , caseProvenance = "X-M1 1.50 image5 initializer and reverse hook walk, spec sections 2.2, 3, 5"
  , caseExclusion = "Retained external callees are exit-recording stubs. Timing, residue arguments, frame pointer and hook cursor allocation are not compared."
  , caseEntry = address entry
  , caseExit = address exitAddress
  , caseRegions = codeRegions ((base,bytes) : concat
      [ [(target,assemble [branchTo target code]),(code,stub row t)]
      | (i,t@(Target target)) <- zip [0 :: Int ..] external, let code = stubBase + fromIntegral i*256])
      ++ [ region "table" 0xbf1a20 ReadOnly (assemble [0xffffffff,0x1062114,if table == OneHook then 0xffffffff else 0x1066304,0x107277c])
         , region "stack" (stack-4096) ReadWrite (B.replicate 4096 0xa5)
         , region "log" logBase ReadWrite (word32le (logBase+4) <> B.replicate 4092 0)
         , region "exit" exitAddress ReadExecute (assemble [0xe12fff1e]) ]
  , caseFunctions = [(address entry, if walk > entry then walk-entry else fromIntegral (B.length bytes) - (entry-base)),(address walk,if walk < entry then entry-walk else base+fromIntegral (B.length bytes)-walk)]
      ++ [(address (stubBase+fromIntegral i*256),fromIntegral (B.length (stub row t))) | (i,t) <- zip [0 :: Int ..] external]
      ++ [(address target,4) | Target target <- external]
  , caseInitial = [(r,sentinel r) | r <- [N.R0 .. N.R12]] ++ [(N.Sp,stack),(N.Lr,exitAddress),(N.Cpsr,0x13)]
  , caseExpectedStatus = maybe (expectedStatus row) (const N.StopAddress) stop
  , caseExpectedRegisters = []
  , caseExpectedMemory = []
  , caseCp14Identity = Nothing
  , caseKernel = Just (KernelCase (InstructionLimit 12000) (address <$> stop) Nothing Nothing Nothing [])
  }
  where external = nub (filter (/= Target 0xd9856c) targets ++ hooks)

data Event = Event Target [Word32] deriving (Eq, Show)
data Run = Run N.RunnerStatus [Event] [(N.Register,Word32)] deriving (Eq, Show)

parseRun :: N.NativeObservation -> Either String Run
parseRun observation = do
  bytes <- maybe (Left "missing log") Right (lookup (N.RegionId "log") (N.nativeMemory observation))
  let end = readWordAt bytes 0
  unless (end >= logBase+4 && end <= logBase+4096 && (end-logBase-4) `mod` 56 == 0) (Left "invalid log cursor")
  let count = fromIntegral ((end-logBase-4) `div` 56)
      event i = let at = 4+i*56 in Event (Target (readWordAt bytes (at+52))) [readWordAt bytes (at+j*4) | j <- [0..12]]
  registers <- forM ([N.R0 .. N.R11] ++ [N.Sp,N.Pc]) $ \r ->
    maybe (Left ("missing register " ++ show r)) (Right . (,) r) (N.lookupRegister r observation)
  pure (Run (N.nativeStatus observation) (map event [0..count-1]) registers)

runCases :: FilePath -> FilePath -> String -> Body -> [Row] -> Maybe Word32 -> IO [Run]
runCases runner directory role body cases stop = do
  batch <- writeBatch directory [caseSpec role body row stop | row <- cases]
  observed <- loadObservedBatch runner batch
  unless (map (\(CaseId label,_) -> label) observed == [caseId (caseSpec role body row stop) | row <- cases]) (fail "native runner case identity/order differs")
  mapM (either fail pure . parseRun . snd) observed

setArguments :: [(Int,Word32)]
setArguments = [(1,0x01010100),(3,7),(6,0xed4a8c),(10,0),(11,1),(16,0),(21,0x11010100)]

specErrors :: Row -> Run -> [String]
specErrors row run@(Run status events registers) =
  ["callee sequence" | map (\(Event target _) -> target) events /= expectedTargets row]
  ++ ["outcome" | status /= expectedStatus row]
  ++ concat [eventErrors event | event <- events]
  ++ ["set r0 call " ++ show n | (n,value) <- setArguments, Just (Event _ values) <- [directEvent n], take 1 values /= [value]]
  ++ if status == N.Returned then
       ["return r0" | lookup N.R0 registers /= Just 0x11010100]
       ++ ["restored " ++ show r | r <- [N.R4 .. N.R11], lookup r registers /= Just (sentinel r)]
       ++ ["restored SP" | lookup N.Sp registers /= Just stack]
     else []
  where
    eventErrors (Event target values) =
      ["call SP " ++ show target | values !! 12 /= stack - if target `elem` hooks then 24 else 8]
      ++ ["preserved " ++ show r ++ " at " ++ show target | r <- [if target `elem` hooks then N.R5 else N.R4 .. N.R10], values !! fromEnum r /= sentinel r]
    directEvent n = lookup n (numbered run)

numbered :: Run -> [(Int,Event)]
numbered (Run _ events _) = zip (filter (/=7) [1..21]) [event | event@(Event target _) <- events, target `notElem` hooks]

compareErrors :: Run -> Run -> [String]
compareErrors stock@(Run s se _) owned@(Run o oe _) =
  ["role callee sequence" | map target se /= map target oe]
  ++ ["role outcome" | s /= o]
  ++ ["role set r0 call " ++ show n | (n,_) <- setArguments, Just (Event _ a) <- [lookup n (numbered stock)], Just (Event _ b) <- [lookup n (numbered owned)], take 1 a /= take 1 b]
  where target (Event t _) = t

mutate :: Body -> Either String Body
mutate (Body bytes walk) = case [(at,word) | at <- [0,4..B.length bytes-4], let word = readWordAt bytes at, word .&. 0xff000000 == 0xeb000000] of
  (a,wa):(b,wb):_ -> Right (Body (B.concat [word32le (replacement at) | at <- [0,4..B.length bytes-4]]) walk)
    where
      destination at word = fromIntegral (toInteger base + toInteger at + 8 + toInteger (signed24 word)*4)
      signed24 word = let imm = fromIntegral (word .&. 0xffffff) :: Int in if imm >= 0x800000 then imm-0x1000000 else imm
      transfer at to = 0xeb000000 .|. (fromIntegral ((toInteger to-toInteger base-toInteger at-8) `div` 4) .&. 0xffffff)
      replacement at | at == a = transfer at (destination b wb :: Word32)
                     | at == b = transfer at (destination a wa :: Word32)
                     | otherwise = readWordAt bytes at
  _ -> Left "owned image has fewer than two direct calls"

runValue :: Run -> Value
runValue (Run status events registers) = object
  [ "status" .= N.runnerStatusLabel status
  , "registers" .= [object ["register" .= N.registerLabel r,"value" .= value] | (r,value) <- registers]
  , "calls" .= [object ["target" .= t,"sp_relative" .= (toInteger (values !! 12)-toInteger stack),"r0_r11" .= take 12 values] | Event (Target t) values <- events] ]

-- The fixture is the exact 324-byte slice at file offset 0x1a6b6c of id5-00bf1a00.bin.
runOracle :: FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO Bool
runOracle runner stockPath ownedPath symbolsPath work reportPath = do
  stockWords <- words <$> readFile stockPath
  stockBytes <- assemble <$> mapM (\text -> case readHex text of [(value,"")] -> pure value; _ -> fail "invalid stock hex fixture") stockWords
  unless (B.length stockBytes == 324) (fail "stock fixture extent differs")
  ownedBytes <- B.readFile ownedPath
  symbols <- parseSymbols ["xm1_image5_hooks"] symbolsPath
  let walk = guestWord (symbolAddress symbols "xm1_image5_hooks")
      stock = Body stockBytes base
      owned = Body ownedBytes walk
  wrong <- either fail pure (mutate owned)
  stockRuns <- runCases runner (work </> "stock") "stock" stock rows Nothing
  ownedRuns <- runCases runner (work </> "owned") "owned" owned rows Nothing
  wrongRuns <- runCases runner (work </> "control") "control" wrong [baseline] Nothing
  let reachable = [row | row@(Row _ injection _) <- rows, injection /= DirectNonzero (CallNumber 4)]
  stockStops <- runCases runner (work </> "stock-walk-entry") "stock-entry" stock reachable (Just base)
  ownedStops <- runCases runner (work </> "owned-walk-entry") "owned-entry" owned reachable (Just walk)
  let stopErrors :: [Run] -> [String]
      stopErrors bodyStops = ["internal call7 entry SP/status/registers" | Run status _ regs <- bodyStops, status /= N.StopAddress || lookup N.Sp regs /= Just (stack-8) || any (\r -> lookup r regs /= Just (sentinel r)) [N.R4 .. N.R10]]
      cases = zip3 rows stockRuns ownedRuns
      errors = [(name,specErrors row s ++ specErrors row o ++ compareErrors s o) | (row@(Row name _ _),s,o) <- cases]
      controlErrors = case (stockRuns,wrongRuns) of
        (stockBaseline:_,[wrongRun]) -> specErrors baseline wrongRun ++ compareErrors stockBaseline wrongRun
        _ -> ["control result count"]
      allErrors = concatMap snd errors ++ stopErrors stockStops ++ stopErrors ownedStops
      passed = null allErrors && not (null controlErrors)
      report = object
        [ "schema" .= (1 :: Int), "case_count" .= caseCount, "verdict" .= (if passed then "pass" else "fail" :: String)
        , "hook_r4_policy" .= ("reported, not compared: stock uses r4 as cursor" :: String)
        , "internal_call7_entry_errors" .= (stopErrors stockStops ++ stopErrors ownedStops)
        , "internal_call7_entries" .= [object ["case" .= name,"stock" .= runValue s,"owned" .= runValue o] | (Row name _ _,s,o) <- zip3 reachable stockStops ownedStops]
        , "control" .= object ["mutation" .= ("swap first two linked direct call targets" :: String),"refused" .= not (null controlErrors),"errors" .= controlErrors]
        , "cases" .= [object ["name" .= name,"errors" .= problems,"stock" .= runValue s,"owned" .= runValue o] | ((name,problems),(_,s,o)) <- zip errors cases] ]
  createDirectoryIfMissing True (takeDirectory reportPath)
  B.writeFile reportPath (encodeCanonical report)
  pure passed
