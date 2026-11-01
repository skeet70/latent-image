module Main where

import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import Data.Either (isLeft, isRight)
import Data.Word (Word32)
import Emulation.PrimaryInitializerConsumerSource (descriptorId)
import Emulation.PrimaryInitializerSource (parseSource)

main :: IO ()
main = do
  forM_ [1..120] $ \ident ->
    unless (isRight (descriptorId ident)) (fail "source consumer refused a bounded descriptor identifier")
  forM_ [0,121,maxBound :: Word32] $ \ident ->
    unless (isLeft (descriptorId ident)) (fail "source consumer admitted an out-of-range descriptor identifier")
  forM_ [B.empty,B.replicate 0x25c 0] $ \bytes ->
    unless (isLeft (parseSource bytes)) (fail "source boundary admitted noncanonical container bytes")
