{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Emulation.RenderCases
  ( ServiceAddress (..), Argument (..), MemoryArgument (..), ServiceCall (..)
  , call, outCall, recordCall, wordsLE, halfLE, hexBytes, addressText
  , callsJSON, eventsJSON
  ) where

import Data.Aeson (Value, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.ByteString as B
import Data.Bits (shiftR)
import Data.Word (Word16, Word32)
import Numeric (showHex)

newtype ServiceAddress = ServiceAddress Word32 deriving (Eq, Show, Ord)
newtype Argument = Argument Int deriving (Eq, Show)
data MemoryArgument = MemoryArgument Argument Int B.ByteString | AbsoluteMemory Word32 Int B.ByteString deriving (Eq, Show)
data ServiceCall = ServiceCall
  { serviceAddress :: ServiceAddress
  , serviceInputs :: [(Argument, Word32)]
  , serviceOutputs :: [(Argument, Word32)]
  , serviceReads :: [MemoryArgument]
  , serviceWrites :: [MemoryArgument]
  } deriving (Eq, Show)

call :: ServiceAddress -> [Word32] -> Word32 -> ServiceCall
call target args result = ServiceCall target (zip (map Argument [0..]) args) [(Argument 0,result)] [] []

outCall :: ServiceAddress -> [(Argument, Word32)] -> Argument -> B.ByteString -> ServiceCall
outCall target args dest bytes = ServiceCall target args [(Argument 0,0)] [] [MemoryArgument dest 0 bytes]

recordCall :: ServiceAddress -> B.ByteString -> ServiceCall
recordCall target bytes = ServiceCall target [] [(Argument 0,0)] [MemoryArgument (Argument 0) 0 bytes] []

wordsLE :: [Word32] -> B.ByteString
wordsLE = B.pack . concatMap (\w -> [fromIntegral (w `shiftR` s) | s <- [0,8,16,24]])

halfLE :: [Word16] -> B.ByteString
halfLE = B.pack . concatMap (\w -> [fromIntegral w,fromIntegral (w `shiftR` 8)])

hexBytes :: B.ByteString -> String
hexBytes = concatMap (\b -> let h = showHex b "" in replicate (2-length h) '0' ++ h) . B.unpack

addressText :: Word32 -> String
addressText w = let h = showHex w "" in "0x" ++ replicate (8-length h) '0' ++ h

bindingsJSON :: [(Argument, Word32)] -> Value
bindingsJSON bindings = object [K.fromString (if n == 13 then "sp" else "r" ++ show n) .= addressText value | (Argument n,value) <- bindings]

memoryJSON :: MemoryArgument -> Value
memoryJSON (MemoryArgument (Argument register) offset bytes) = object
  [ "register" .= (if register == 13 then "sp" else "r" ++ show register)
  , "offset" .= offset, "hex" .= hexBytes bytes ]
memoryJSON (AbsoluteMemory address offset bytes) = object
  [ "address" .= addressText address, "offset" .= offset, "hex" .= hexBytes bytes ]

callsJSON :: [ServiceAddress] -> [ServiceCall] -> [Value]
callsJSON targets calls = [object
  [ "address" .= addressText address
  , "calls" .= [object ["inputs" .= bindingsJSON (serviceInputs c), "outputs" .= bindingsJSON (serviceOutputs c), "reads" .= map memoryJSON (serviceReads c), "writes" .= map memoryJSON (serviceWrites c)] | c <- calls, serviceAddress c == target]
  ] | target@(ServiceAddress address) <- targets]

eventsJSON :: [ServiceCall] -> [Value]
eventsJSON = map (\c -> let ServiceAddress target = serviceAddress c in object
  ["address" .= addressText target,"inputs" .= bindingsJSON (serviceInputs c),"outputs" .= bindingsJSON (serviceOutputs c)])
