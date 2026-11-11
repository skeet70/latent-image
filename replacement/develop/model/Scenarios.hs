{-# LANGUAGE GHC2021 #-}
module Scenarios where

import Data.Aeson (FromJSON)
import Data.Bits (shiftR, (.|.), shiftL)
import qualified Data.ByteString as B
import Data.Word (Word32)
import qualified Emulation.NativeObservation as N

newtype Address = Address { addressWord :: Word32 } deriving (Eq, Ord, Show, FromJSON)
data Function = Load | Post | Dispatch | Entry | Save | Begin | Complete | Cancel | CancelAll | Current | Store | Transition | Wait | PermitRequest | PermitDecode | PermitPrimary | PermitSecondary deriving (Eq, Show)
data Patch = Patch Address B.ByteString deriving (Eq, Show)
data MemoryContract = Absolute Patch | Relative N.Register Int B.ByteString | Indirect N.Register Int B.ByteString deriving (Eq, Show)
data Call = Call Address [(N.Register, Word32)] [(N.Register, Word32)] [Patch]
          | ScriptCall Address [(N.Register, Word32)] [(N.Register, Word32)] [MemoryContract] [MemoryContract] deriving (Eq, Show)
callAddress :: Call -> Address
callAddress (Call address _ _ _) = address
callAddress (ScriptCall address _ _ _ _) = address
callInputs :: Call -> [(N.Register,Word32)]
callInputs (Call _ inputs _ _) = inputs
callInputs (ScriptCall _ inputs _ _ _) = inputs
callOutputs :: Call -> [(N.Register,Word32)]
callOutputs (Call _ _ outputs _) = outputs
callOutputs (ScriptCall _ _ outputs _ _) = outputs
data Scenario = Scenario
  { scenarioName :: String
  , scenarioFunction :: Function
  , scenarioArguments :: [Word32]
  , scenarioInitial :: [Patch]
  , scenarioFinal :: [Patch]
  , scenarioReturn :: Maybe Word32
  , scenarioCalls :: [Call]
  } deriving (Eq, Show)

context, state, params, callback, stackTop, exit :: Word32
context = 0x009a0000
state = 0x009b0000
params = context + 0x1000
callback = 0x00800000
stackTop = 0x009abff0
exit = 0x009dfff0

wordBytes :: Word32 -> B.ByteString
wordBytes value = B.pack [fromIntegral (value `shiftR` shift) | shift <- [0,8,16,24]]
word :: Word32 -> Word32 -> Patch
word address value = Patch (Address address) (wordBytes value)
wordsAt :: Word32 -> [Word32] -> Patch
wordsAt address values = Patch (Address address) (B.concat (map wordBytes values))
readWord :: B.ByteString -> Word32 -> Word32
readWord image address = foldr (.|.) 0 [fromIntegral (B.index image (fromIntegral (address - 0x00bf1a00) + index)) `shiftL` (8*index) | index <- [0..3]]

stock :: Function -> Word32
stock Load = 0x00e92488
stock Post = 0x00e92368
stock Dispatch = 0x00ea0934
stock Entry = 0x00ea17d0
stock Save = 0x00e99d4c
stock Begin = 0x00e93c34
stock Complete = 0x00e93d0c
stock Cancel = 0x00e937f8
stock CancelAll = 0x00e93770
stock Current = 0x00e9371c
stock Store = 0x00e9390c
stock Transition = 0x00e93bdc
stock PermitRequest = 0x00e67750
stock PermitDecode = 0x00e7ee84
stock PermitPrimary = 0x00e6a74c
stock PermitSecondary = 0x00e6bb88
stock Wait = 0x00e9e038
symbol :: Function -> String
symbol function = "xm1_develop_" ++ case function of
  Load -> "load"; Post -> "post"; Dispatch -> "dispatch"; Entry -> "entry"; Save -> "save_step"
  Begin -> "begin"; Complete -> "complete"; Cancel -> "cancel"; CancelAll -> "cancel_all"
  Current -> "current"; Store -> "store"; Transition -> "transition"
  PermitRequest -> "permit_request"; PermitDecode -> "decode"; PermitPrimary -> "permit_primary"; PermitSecondary -> "permit_secondary"
  Wait -> "wait_step"

base :: [Patch]
base = [word context state, word (context+4) 7, word (context+8) callback, word (context+0x34) 11, word (context+0x44) 3, word (context+0x48) 2]
call :: Word32 -> [Word32] -> Word32 -> Call
call address arguments result = Call (Address address) (zip [N.R0,N.R1,N.R2,N.R3] arguments) [(N.R0,result)] []
callbackCall :: Word32 -> Word32 -> Call
callbackCall operation argument = call callback [context,operation,argument,callback] 0
objectCall :: Word32 -> Word32 -> Word32 -> Call
objectCall operation phase argument = call (if phase == 0 || phase == 2 then 0x00e93140 else 0x00e92e30) [context,operation,phase,argument] 0
planValues :: [Word32]
planValues = [0x20,0x40,0x41,0x42,0x43,0x11223344,0x55667788,13,0xffffffff,12,0x00ea1fa0,0x12345678,0x87654321]
beginCalls :: [Word32] -> [Call]
beginCalls [operation, entry, _, _, _, argument, trace, _, _, _, text, start, _] =
  [call 0x723cb8 [] 0x123, call 0x726658 [0x23,text] 0, call 0x42188 [start] 0, objectCall operation 0 argument, objectCall operation 1 argument, callbackCall entry 0, call 0xeb0cbc [trace] 0]
beginCalls _ = error "step plan must contain thirteen words"
completeCalls :: [Word32] -> Word32 -> [Call]
completeCalls [operation, _, completed, _, _, argument, trace, _, _, _, _, _, end] status =
  [call 0xeb0ccc [trace] 0, objectCall operation 3 argument] ++ (if status == 1 then [objectCall operation 2 argument,callbackCall completed 0] else []) ++ [call 0x724638 [] 0,call 0xe4eca8 [state] 0,call 0x42188 [end] 0]
completeCalls _ _ = error "step plan must contain thirteen words"

scenarios :: B.ByteString -> [Scenario]
scenarios image = loadCases ++ frameworkCases ++ saveCases ++ postCases ++ dispatchCases
 where
  make name function arguments initial final result calls = Scenario name function arguments (base++initial) final result calls
  loadCases = [make ("load-"++show seed) Load [context,params] [wordsAt params values] [wordsAt (context+4) (take 6 values),wordsAt (context+0x34) (take 3 (drop 6 values)),wordsAt (context+0x1c) (drop 9 values)] Nothing [] | seed <- [0,1,0x80000000,0xffffffff], let values = take 13 (iterate (+0x01020304) seed)]
  frameworkCases =
    [make "begin" Begin [context,params] [wordsAt params planValues] [word (state+0x38) 0x42] (Just 0) (beginCalls planValues)] ++
    [make ("complete-"++show status) Complete [context,params,status] [wordsAt params planValues,word (state+0x38) 0x42] (if status == 1 then [word (state+0x38) 0x43] else []) (Just 0) (completeCalls planValues status) | status <- [0,1,2,0xffffffff]] ++
    [make ("current-"++show kind++"-"++show mode) Current [context] [word (context+4) kind,word (state+0x4b4) mode,word (state+slot) 0x12345678] [] (Just 0x12345678) [] | (kind,mode,slot) <- [(7,0,0x38),(1,0x1d,0),(1,0,0)]] ++
    [make "current-task-45" Current [context] [word (context+4) 1,word (context+0x34) 0x45,word (state+0xe8) 0x12345678,word state 0x87654321] [] (Just 0x12345678) []] ++
    [make "store-task-45" Store [context,0x100] [word (context+4) 1,word (context+0x34) 0x45,word state 0x87654321] [word (state+0xe8) 0x100] Nothing []] ++
    [make ("store-"++show value) Store [context,value] [] [word (state+0x38) value] Nothing [] | value <- [0,0xff,0x100,0xffffffff]] ++
    [make "transition-inactive" Transition [context,0x20] [] [] (Just 0) []] ++
    [make ("cancel-"++show bit++"-"++show current) Cancel [context,bit] [word (state+0x38) current] [word (context+0x4c) (1 `shiftL` fromIntegral bit)] Nothing (notify current mask ++ [Call (Address 0xeb0ea0) [(N.R0,context+0x4c),(N.R1,bit)] [(N.R0,0)] [word (context+0x4c) (1 `shiftL` fromIntegral bit)]]) | (bit,mask) <- zip [0..11] [2,4,8,0x80,0x100,0x400,0x8000,0x20000,0x20000,0x40000,0x80000000,0x2000], current <- [0,0xe2,0xe3,0xf8,0xfa]]
  notify current mask = if current >= 0xe2 && current <= 0xf8 && even current then [call 0x724558 [11,mask] 0] else []
  saveCases = concat [saveCase kind | kind <- [0,1,2,4]]
  saveCase kind =
    let operation = case kind of 2 -> 0x32; 4 -> 0x33; 1 -> 0x34; _ -> 0x35
        plan = lookupPlan operation 0
        slot = 0x009a1800
        active = [word (state+0x4b4) (if kind == 4 then 0x1e else 0x19),word (state+0x578) 2,word (state+0x322c) 2,word (state+0x27cc) 1,word slot 0x12345678]
     in [make ("save-disabled-"++show kind) Save [context,kind,slot] [word slot 0x12345678] [word (state+0x38) (plan!!4)] (Just 0) [] | kind /= 1] ++
        [make ("save-bypass-"++show kind) Save [context,kind,slot] active [word slot 1,word (state+0x38) (plan!!4)] (Just 0) (beginCalls plan ++ completeCalls plan 1)]
  lookupPlan operation index
    | index > 126 = error "stock plan not found"
    | readWord image (0xea6738 + index*52) == operation && readWord image (0xea6738 + index*52+28) == 13 = [readWord image (0xea6738+index*52+field*4) | field <- [0..12]]
    | otherwise = lookupPlan operation (index+1)
  postCases = [make "post-copy" Post [context,0] [word 0x015b52e8 1,word 0x015b5300 (context+0x1800),word 0x015b52f0 callback,wordsAt (state+0x188) [11,22,33,44,55]] [wordsAt (context+0x1800) [11,22,33,44,55]] Nothing [call callback [0] 0]]
  dispatchCases = [make "dispatch-unmapped-kind" Dispatch [context] [word (context+4) 0xffffffff] [word (context+0x40) 0] Nothing [call 0x724638 [] 0]]
