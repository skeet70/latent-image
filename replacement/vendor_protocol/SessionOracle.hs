{-# LANGUAGE GHC2021 #-}
module Main where

import Control.Monad (forM_,unless)
import Data.Bits
import Data.List (intercalate)
import Data.Word
import Numeric (showHex)
import System.Environment (getArgs)
import System.Exit (ExitCode(..),die)
import System.Process (readProcessWithExitCode)

data Control = Control Word8 Word32 Word32 Word16 deriving Show
data Model = Model { active::Bool, sid::Word16, requested::Word32, armed::Bool, ready::Bool, reply::[Word8], available::Word32 } deriving Show
initial :: Model
initial = Model False 0 0 False False (replicate 256 0) 0
hex :: (Integral a,Show a)=>a->String
hex value=showHex value ""
byte :: Word8->String
byte value=let h=hex value in replicate (2-length h) '0'++h
be32 :: Word32->[Word8]
be32 value=[fromIntegral(value `shiftR` n)|n<-[24,16,8,0]]
le16 :: Word16->[Word8]
le16 value=[fromIntegral value,fromIntegral(value `shiftR` 8)]
frame :: Control->String
frame (Control op offset count token)=concatMap byte ([op,0]++be32 offset++be32 count++reverse(le16 token))
flag :: Bool->Word32
flag False=0
flag True=1

oracle :: Word32->Model->Control->(Model,String)
oracle tick before (Control opcode offset count token)
 | opcode==0xe1 && validSession && count/=0 && (toInteger offset+toInteger count>16) = (before,"error InvalidPublishedSpan")
 | otherwise = (after,intercalate " " ("ok":map hex fields)++" "++concatMap byte sent++".")
 where
  validSession=active before && token==sid before
  reject code=(before,1,code,0,0,0)
  prepare state code bytes=(state{requested=count,armed=True,ready=True,reply=bytes++drop(length bytes)(reply state),available=fromIntegral(length bytes)},2,code,3,0,0)
  next0=(if sid before==0 then fromIntegral tick else sid before)+1
  next=let value=next0 .&. 0x1fff in if value==0 then 1 else value
  (changed,delivery,status,action,payloadOffset,payloadLength)=case opcode of
   0xc0 -> let code=if active before then 0x80 else if token/=0 then 0x24 else if count/=8 then 0x21 else 0
               state=if code==0 then before{active=True,sid=next} else before
           in prepare state code (le16(if code==0 then next else 0)++[0,0,0,0,1,0])
   0xc2 -> let code=if not(active before) then 0x24 else if count/=2 then 0x21 else if token/=sid before then 0x24 else 0
           in prepare before code (le16(if code==0 then sid before else 0))
   0xc4 -> let code=if token/=sid before then 0x24 else if count/=2 then 0x21 else 0
               state=if code==0 then before{active=False} else before
           in prepare state code (le16(if code==0 then sid before else 0))
   0x03 -> let length' = (offset `shiftR` 8) .&. 0xff
               changedByte index old = case index of
                0 -> 0x70
                2 -> 0
                7 -> 10
                12 -> 0
                13 -> 0
                15 -> 0
                _ | length'/=0 && index==fromIntegral length' -> 0
                  | otherwise -> old
               bytes=zipWith changedByte [0::Int ..] (reply before)
           in (before{requested=length',armed=True,ready=True,reply=bytes,available=length'},2,0,3,0,0)
   0x1d -> (before{active=False,armed=False,ready=False},1,0x29,0,0,0)
   0xd0 | not validSession -> reject 0x24
        | count>512 -> reject 0x21
        | otherwise -> (before{armed=False,ready=False},2,0,1,0,0)
   0xe0 | not validSession -> reject 0x24
        | otherwise -> (before{armed=False,ready=False},2,0,2,0,0)
   0xd1 | not validSession -> reject 0x24
        | count==0 -> reject 0x21
        | otherwise -> (before{requested=count,armed=True},0,0,3,0,0)
   0xe8 | not validSession -> reject 0x24
        | count==0 -> reject 0x21
        | otherwise -> prepare before 0 [0,0,0,0]
   0xe1 | not validSession -> reject 0x24
        | count==0 -> reject 0x21
        | otherwise -> (before{requested=count},2,0,4,offset,count)
   _ -> reject 0x20
  sending=action==3 && armed changed && ready changed
  sent=if sending then take (fromIntegral(min (requested changed) (available changed))) (reply changed) else []
  after=if sending then changed{armed=False,ready=False} else changed
  fields=[delivery,status,action,flag(active changed),fromIntegral(sid changed),requested changed,flag(armed changed),flag(ready changed),payloadOffset,payloadLength]::[Word32]

scenarios :: [(Word32,[Control])]
scenarios =
 [(tick,[Control 0xc0 0 count token])|tick<-[0,1,0x1ffe,0x1fff,0xffff,0xffffffff],count<-[0,7,8,9],token<-[0,1,0xffff]] ++
 [(0,[Control 0xc0 0 8 0,Control op offset count token])|op<-[0x03,0x1d,0xc0,0xc2,0xc4,0xd0,0xd1,0xe0,0xe1,0xe8,0xff],(offset,count)<-[(0,0),(0,2),(0,8),(0,16),(0,32),(0,128),(0,512),(0,513),(15,1),(16,1),(0xffffffff,2),(1,0xffffffff)],token<-[0,1,2,0xffff]] ++
 [(0,[Control 0xc0 0 8 0,Control 0xc4 0 2 1,Control op 0 count token])|op<-[0xc0,0xc2,0xc4,0xd0,0xd1,0xe0,0xe1,0xe8],count<-[0,2,8],token<-[0,1,2]] ++
 [(0,[Control 0xc0 0 8 0,Control 0x03 (length' `shiftL` 8) 0 0,Control 0xe8 0 4 1])|length'<-[0,1,2,7,8,12,15,16,255]] ++
 [(0x1ffe,[Control 0xc0 0 8 0,Control 0xc4 0 2 0x1fff,Control 0xc0 0 8 0,Control 0xc2 0 2 1])]

main :: IO ()
main=do
 args<-getArgs
 runner<-case args of [path]->pure path;_->die "usage: session-oracle MODEL-RUNNER"
 forM_ (zip [0::Int ..] scenarios) $ \(index,(tick,steps))->do
  let expected=snd(foldl (\(state,rows) step->let (next,row)=oracle tick state step in(next,rows++[row])) (initial,[]) steps)
  (exit,stdout,stderr)<-readProcessWithExitCode runner ([hex tick,"41000000","10"]++map frame steps) ""
  unless(exit==ExitSuccess && lines stdout==expected)(die("session oracle mismatch "++show index++" "++show steps++"\nexpected "++show expected++"\nactual "++show(lines stdout)++"\n"++stderr))
 putStrLn "Source-derived X-M1 session laws match host Rust observations; invalid E1 spans are intended-repair refusals, not stock-equality claims."
