{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import qualified ArmElf as E
import Control.Monad (foldM, forM, forM_, unless)
import Data.Bits ((.&.), (.|.), complement, shiftL, shiftR)
import qualified Data.ByteString as B
import Data.List (isInfixOf, sort)
import qualified Data.Map.Strict as M
import Data.Word (Word8, Word32)
import Emulation.NativeCase (Region (..), loadCase, runCase)
import Emulation.NativeObservation
import Emulation.OwnedCase
import Emulation.Sha256 (sha256, sha256Hex)
import GHC.Float (castFloatToWord32)
import FujiAF.Host (get)
import FujiAF.JSON (encodeCanonical)
import System.Environment (getArgs)
import System.Directory (doesDirectoryExist, doesPathExist, getFileSize, listDirectory, pathIsSymbolicLink)
import System.FilePath ((</>), takeDirectory)

pin :: String
pin = "x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; image4 f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16; scanout CPU contract"

address :: Word32 -> GuestAddress
address = GuestAddress . fromIntegral

mov :: Word32 -> Word32 -> [Word32]
mov r v = [0xe3000000 .|. ((v .&. 0xf000) `shiftL` 4) .|. (r `shiftL` 12) .|. (v .&. 0xfff),0xe3400000 .|. (((v `shiftR` 16) .&. 0xf000) `shiftL` 4) .|. (r `shiftL` 12) .|. ((v `shiftR` 16) .&. 0xfff)]

newtype Memory = Memory (M.Map Word32 Word8)

empty :: Memory
empty = Memory M.empty

put :: Word32 -> B.ByteString -> Memory -> Memory
put a bytes (Memory m) = Memory (M.union (M.fromList (zip [a..] (B.unpack bytes))) m)

word :: Word32 -> Word32 -> Memory -> Memory
word a = put a . word32le

half :: Word32 -> Word32 -> Memory -> Memory
half a = put a . B.take 2 . word32le

byte :: Word32 -> Word32 -> Memory -> Memory
byte a = put a . B.singleton . fromIntegral

bytesAt :: Memory -> Word32 -> Int -> B.ByteString
bytesAt (Memory m) a n = B.concat (segments a selected)
  where
    end=a+fromIntegral n
    selected=M.toAscList (M.takeWhileAntitone (<end) (M.dropWhileAntitone (<a) m))
    segments cursor []=[B.replicate (fromIntegral (end-cursor)) 0]
    segments cursor ((offset,value):rest)=B.replicate (fromIntegral (offset-cursor)) 0:B.singleton value:segments (offset+1) rest

getWord :: Memory -> Word32 -> Word32
getWord m a = readWordAt (bytesAt m a 4) 0

getHalf :: Memory -> Word32 -> Word32
getHalf m a = getWord m a .&. 0xffff

getByte :: Memory -> Word32 -> Word32
getByte m a = getWord m a .&. 255

changes :: [(Word32,Word32)] -> Memory -> Memory
changes xs m = foldr (uncurry word) m xs

rbase, sbase, ibase, tbase, ubase, dbank, qbank, fbank :: Word32 -> Word32
rbase e = 0x9553cc+e*0x58
sbase e = 0x96b9bc+e*0x1a4
ibase e = 0x9554b8+e*0x23c
tbase e = 0x9429f4+e*0x58
ubase e = 0x917c10+e*0x128
dbank e = 0xfff6b000+e*0x3000
qbank e = 0xfff6a000+e*0x3000
fbank e = 0xfff69000+e*0x3000

regions :: [(String,Word32,Int)]
regions = [("globals",0x916600,0x20),("scheduler",0x917c00,0x400),("timing",0x9429f0,0xc0),("runtime",0x9553c0,0x700),("mode",0x955b00,0x60),("shadow",0x96b9b0,0x360),("cache-pointer",0x96ca50,4),("cache",0x4000000,0x1100),("packet",0x200000,0x100),("journal",0x210000,0x1000)] ++ [("registers-"++show e,0xfff69000+e*0x3000,0x3000) | e <- [0,1]]

seed :: Memory
seed = changes ([(0x96ca50,0x4000000),(0x210f04,0x7a11),(0x9553d8,0x51c002),(0x955430,0x51c102)]++[(0x210c00+n*16,0x200080) | n <- [1..6]]++[(0x955b00+n*4,0x4d000000+n*0x10101) | n <- [0..0x17]]) $ put 0x4001080 (B.pack [0,1,0x23,3]) $ foldr initializeInputs (foldr initialize empty regions) [0,1]
  where
    initialize (_,a,n) m = if a>=0xfff00000 || a==0x96b9b0 then put a (B.concat [word32le (0xa5310000+(a .&. 0xffff)*0x100+fromIntegral i*0x10203) | i <- [0..n `div` 4-1]]) m else m
    initializeInputs e=put (ibase e) (assemble [0xb6420000+e*0x10101+n*0x10203 | n <- [0..0x23c `div` 4-1]])

data Root = Item | Range | Enable | Options | Blank | Flush | Defaults | Stage | Immediate | Reset | Wait | Formatter | Strobe | Register | IsrZero | IsrOne | Commit | CommitZero | CommitOne | Capture
  | EngineInitialize | OperationRelease | ModeWindow | ModeBacking | ModeEnabled | ModeRange | ModeColour | ModeScale | ModeWindowSize | ModeColourPacket deriving (Eq,Show,Enum,Bounded)

-- The panel-feature build is a second artifact; the standalone firmware ELF does not export these.
data ElfRole = MainElf | PanelElf deriving (Eq,Show)

rootRole :: Root -> ElfRole
rootRole op = case op of
  EngineInitialize -> PanelElf; OperationRelease -> PanelElf
  ModeWindow -> PanelElf; ModeBacking -> PanelElf; ModeEnabled -> PanelElf; ModeRange -> PanelElf
  ModeColour -> PanelElf; ModeScale -> PanelElf; ModeWindowSize -> PanelElf; ModeColourPacket -> PanelElf
  _ -> MainElf

root :: Root -> (String,Word32)
root op = case op of
  Item -> ("item_configuration",0x6c9cf4)
  Range -> ("range_packet",0x6c9d44)
  Enable -> ("range_enabled",0x6c9e7c)
  Options -> ("group_options",0x6c9f70)
  Blank -> ("blank",0x6cb8b4)
  Flush -> ("timing_flush",0x6c7b9c)
  Defaults -> ("timing_defaults",0x6c7da8)
  Stage -> ("timing_stage",0x6c7e50)
  Immediate -> ("timing_immediate",0x6c8790)
  Reset -> ("reset",0x6c9160)
  Wait -> ("wait",0x6c8710)
  Formatter -> ("formatter_flush",0x6cc1f0)
  Strobe -> ("formatter_strobe",0x6ce2dc)
  Register -> ("scheduler_register",0x6c7318)
  IsrZero -> ("isr_zero",0x6c70e4)
  IsrOne -> ("isr_one",0x6c6eb0)
  Commit -> ("commit",0x6ca3e0)
  CommitZero -> ("commit_zero",0x6cb0c4)
  CommitOne -> ("commit_one",0x6cb0d8)
  Capture -> ("capture",0x6c926c)
  EngineInitialize -> ("engine_initialize",0x6cbb88)
  OperationRelease -> ("operation_release",0x6c8ee8)
  ModeWindow -> ("mode_window",0x6cfed0)
  ModeBacking -> ("mode_backing",0x6cffbc)
  ModeEnabled -> ("mode_enabled",0x6d01bc)
  ModeRange -> ("mode_range",0x6d0498)
  ModeColour -> ("mode_colour",0x6d0ee4)
  ModeScale -> ("mode_scale",0x6d0000)
  ModeWindowSize -> ("mode_window_size",0x6cfe14)
  ModeColourPacket -> ("mode_colour_packet",0x6d0244)

data Scenario = Scenario
  { label :: String, operation :: Root, arguments :: [Word32], initial :: Memory
  , expected :: Memory, result :: Maybe Word32, held :: Bool
  , expectedWrites :: Maybe [(Word32,Word32)], repetitions :: Int, paired :: Bool
  }

scenario :: String -> Root -> [Word32] -> Memory -> Memory -> Maybe Word32 -> Scenario
scenario name op args before after value = Scenario name op args before after value False (Just []) 1 True

shadowCases :: [Scenario]
shadowCases = concat
  [ [let a=s+0x64+g*0x4c+i*16; old=getWord seed a; value=0x71234567
     in scenario (key "item" [e,g,i]) Item [e,g,i,value] seed (word a ((old .&. 0xc0000000) .|. (value .&. 0x3fffffff)) seed) (Just 0) | i <- [0..3]]
    ++ [scenario (key "item-invalid" [e,g,i]) Item [e,g,i,9] seed seed (Just (negate 30)) | i <- [4,0xffffffff]]
    ++ [rangeCase e g i dim pad | i <- [0..3],(dim,pad) <- [(0,0),(7,2),(9,31)]]
    ++ [scenario (key "range-invalid" [e,g,i]) Range [e,g,i,0x200000] seed seed (Just (negate 30)) | i <- [4,0xffffffff]]
    ++ [enableCase e g i value alt initialized | i <- [0,3,255,4],value <- [0,1,2],alt <- [0,1],initialized <- [0,1]]
    ++ [optionsCase e g]
  | e <- [0,1],g <- [0,1],let s=sbase e]
  ++ [blankCase e on initialized | e <- [0,1],on <- [0,1,2],initialized <- [0,1]]
  ++ [let c=blankCase e 1 1; i=ibase e; after=expected c in c {label=key "blank-repeated" [e],repetitions=2,expected=byte i (getByte after i .&. 0xfe) $ half (i+4) 0 $ half (i+8) 0 $ half (i+0x40) 0 after} | e <- [0,1]]

key :: String -> [Word32] -> String
key name values = name++concatMap (("-"++).show) values

rangeCase :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Scenario
rangeCase e g i dim pad = scenario (key "range" [e,g,i,dim,pad]) Range [e,g,i,0x200000] before after (Just 0)
  where
    s=sbase e; p=ibase e+0x14+g*16+i*4; a=s+g*0x4c+i*16
    before=put 0x200000 (B.concat (map (B.take 2 . word32le) [0xdead,0xbeef,dim,pad,23])<>B.pack [if dim==0 then 17 else 241,0]) $ word p 0xc000c000 seed
    after=byte (s+0x58+g*0x4c+i) (if dim==0 then 17 else 128) $ half (a+0x68) ((getHalf before (a+0x68) .&. 0xf000) .|. ((if pad>dim then pad-dim else 0) .&. 0xfff)) $ half (a+0x60) ((getHalf before (a+0x60) .&. 0xe000) .|. 22) $ half (a+0x62) ((getHalf before (a+0x62) .&. 0xe000) .|. ((dim-1) .&. 0x1fff)) $ word p ((getWord before p .&. 0xc000c000) .|. 0x1ead3eef) before

enableCase :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Scenario
enableCase e g i value alt initialized = scenario (key "enable" [e,g,i,value,alt,initialized]) Enable [e,g,i,value] before after (Just rc)
  where
    before=half a 0xa5a5 $ byte (rbase e+0x54) initialized $ byte 0x916610 alt seed
    a=if alt==0 then sbase e+0x54+g*0x4c else ibase e+4+g*4
    mask=if i==255 then 0x1111 else 1 `shiftL` fromIntegral (i*4)
    rc=if initialized==0 then negate 10 else if i>3 && i/=255 then negate 30 else 0
    after=if rc/=0 then before else half a (if value==1 then getHalf before a .|. mask else getHalf before a .&. complement mask) before

optionsCase :: Word32 -> Word32 -> Scenario
optionsCase e g = scenario (key "options" [e,g]) Options [e,g,0x200000] before after (Just 0)
  where
    s=sbase e; i=ibase e
    before=put 0x200000 (assemble [5,9,2,1,3]) seed
    replace a mask v m=byte a ((getByte m a .&. complement mask) .|. (v .&. mask)) m
    common=replace (s+0x50+g*0x4c) 7 5 before
    after=if g==1 then replace (s+3) 15 9 $ replace (i+0x3c) 0x30 0x20 $ replace (s+9) 15 7 common else replace (s+2) 0xf0 0 $ replace (i+0x3c) 12 8 $ replace (s+8) 0xf0 0x70 common

blankCase :: Word32 -> Word32 -> Word32 -> Scenario
blankCase e on initialized = scenario (key "blank" [e,on,initialized]) Blank [e,0x200000] before after (if initialized==0 then Just (negate 10) else Nothing)
  where
    s=sbase e; i=ibase e
    before=byte (rbase e+0x54) initialized $ put 0x200000 (B.pack [fromIntegral on,0x21,0x43,0x65]) $ changes [(i,0x12345679),(i+4,0xabcd1357),(i+8,0x24681357),(i+0x40,0x98fe7654)] seed
    base=byte 0x916610 on $ put (s+0x180) (B.pack [0x65,0x43,0x21]) before
    (src,dst)=if on/=0 then (s,i) else (i,s)
    moved=foldr (\(a,b) m -> half a 0 $ half b (getHalf before a) m) base [(s0,d0) | (so,io) <- [(0xe8,0x40),(0x54,4),(0xa0,8)],let (s0,d0)=if on/=0 then (s+so,i+io) else (i+io,s+so)]
    after=if initialized==0 then before else byte src (getByte before src .&. 0xfe) $ byte dst ((getByte before dst .&. 0xfe) .|. (getByte before src .&. 1)) moved

timingCases :: B.ByteString -> [Scenario]
timingCases image = [flushCase e dirty force equal zero | e <- [0,1],dirty <- [0,1],force <- [0,1,2],equal <- [False,True],zero <- [False,True]] ++ [immediateCase e flags | e <- [0,1],flags <- [0..7]] ++ [formatterCase e dirty | e <- [0,1],dirty <- [0,1]] ++ concat [heldCases e | e <- [0,1]]
  ++ [defaultsCase e flags | e <- [0,1],flags <- [0..7]++[0xf8,0xff]]
  ++ [stageCase image e mode route rate flags | e <- [0,1],mode <- [0..9],route <- [0..3],rate <- [0..5],let flags=(mode+route+rate) .&. 7]
  ++ [stageCase image e 2 3 0 flags | e <- [0,1],flags <- [0..7]++[0xf8,0xff],flags/=5]

calibrationSeed :: Memory
calibrationSeed = put 0x4000000 (B.pack [fromIntegral (1+(n*73+n `div` 251*19) `mod` 255) | n <- [0::Int ..0x11ff]]) seed

defaultWord :: Word32 -> Word32 -> Word32
defaultWord flags old = (old .&. complement mask) .|. (value .&. mask)
  where
    mask=(if flags .&. 1==0 then 0x0fff0000 else 0) .|. (if flags .&. 2==0 then 7 else 0) .|. (if flags .&. 4==0 then 0x10000000 else 0)
    value=((getByte calibrationSeed 0x4001081*256+getByte calibrationSeed 0x4001082) .&. 0xfff) `shiftL` 16 .|. (getByte calibrationSeed 0x4001083 .&. 7) .|. ((getByte calibrationSeed 0x4001080 .&. 1) `shiftL` 28)

defaultsCase :: Word32 -> Word32 -> Scenario
defaultsCase e flags = scenario (key "defaults" [e,flags]) Defaults [e,0x200000] before (word 0x200000 (defaultWord flags old) before) Nothing
  where
    old=0xe5abcdfa
    before=byte (tbase e+0x1c) flags $ word 0x200000 old calibrationSeed

-- Selector words are read from authenticated image 4, not the candidate's table.
stageCase :: B.ByteString -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Scenario
stageCase image e mode route rate flags = scenario (key "stage" [e,mode,route,rate,flags]) Stage [e,0x200000] before after Nothing
  where
    rom a=readWordAt image (fromIntegral (a-0x61c400))
    c a=getByte calibrationSeed (0x4000000+a)
    p hi lo=((c hi*256+c lo) .&. 0xfff)
    b a=c (rom a)
    pairAt a=p (rom a) (rom (a+4))
    pulse a=(b a .&. 1) `shiftL` 28 .|. (b (a+4) .&. 1) `shiftL` 12 .|. pairAt (a+8) `shiftL` 16 .|. pairAt (a+16)
    output a=pairAt a .|. (b (a+8) .&. 1) `shiftL` 12
    interval a=pairAt a .|. pairAt (a+8) `shiftL` 16
    normalized=if rate==0 || rate==3 then 2 else rate
    row=rom (0x828bc0+route*4)*2+rom (0x828d10+normalized*4)
    direct=route==3
    dimensions=interval (if direct then 0x828978+mode*16 else 0x828cd0+row*16)
    control=(c 0x104e .&. 1) `shiftL` 16 .|. (if direct then b (0x828a98+mode*8) .|. (b (0x828a9c+mode*8) .&. 1) `shiftL` 8 else 0x100 .|. (b (0x828ca0+row*12) .&. 1) .|. (b (0x828ca4+row*12) .&. 1) `shiftL` 1 .|. (b (0x828ca8+row*12) .&. 1) `shiftL` 4)
    modeBase=0x7cd640+mode*24
    modePulse=(b modeBase .&. 1) `shiftL` 28 .|. pairAt (modeBase+4) `shiftL` 16 .|. (b (modeBase+12) .&. 1) `shiftL` 12 .|. pairAt (modeBase+16)
    modePair=0x7cd730+mode*16
    modeInterval=pairAt modePair `shiftL` 16 .|. pairAt (modePair+8)
    delay=if mode==1 then c 0x106d else if mode==0 || mode==2 then c 0x106c else 0
    adjustment=if direct then 0 else pairAt (0x828b60+row*24) .|. (b (0x828b68+row*24) .&. 1) `shiftL` 16 .|. ((c (rom (0x828b6c+row*24))*256+c (rom (0x828b70+row*24))) .&. 0x1ff) `shiftL` 17 .|. (b (0x828b74+row*24) .&. 1) `shiftL` 26
    adjusted=((dimensions .&. 0xfff) `div` 2-(if mode `elem` [1,4,6] then 7 else 9)) .&. 0x1fff
    t=tbase e
    before=byte (t+0x1c) flags $ changes ([(tbase other,0xabc00000+other) | other <- [0,1]]++[(t+a,0xfedcba98+a*0x10203) | a <- [4,8..0x54]]) $ put 0x200000 (assemble [mode,route,rate]) calibrationSeed
    registers=[control,dimensions,modePulse,modeInterval,if direct then 0 else pulse (0x828918+row*24),if direct then 0 else pairAt (0x828a18+row*16) `shiftL` 16 .|. pairAt (0x828a20+row*16),delay,output (if direct then 0x828ae8+mode*12 else 0x828c70+row*12),interval (if direct then 0x828bd0+mode*16 else 0x828a58+row*16),if direct then 0 else output (0x828d68+row*12),if direct then 0 else interval (0x828d28+row*16),(getWord before (t+0x4c) .&. complement 0x1fff) .|. (adjusted-adjusted `div` 30),defaultWord flags (getWord before (t+0x50)),adjustment]
    after=changes ([(tbase other,e) | other <- [0,1]]++[(t+4,mode),(t+8,route),(t+12,rate),(t+0x10,1),(0x200008,normalized)]++zip [t+0x20,t+0x24..t+0x54] registers) before

flushCase :: Word32 -> Word32 -> Word32 -> Bool -> Bool -> Scenario
flushCase e dirty force equal zero = (scenario (key "flush" [e,dirty,force,if equal then 1 else 0,if zero then 1 else 0]) Flush [e] before after (Just rc)) {expectedWrites=Just writes}
  where
    t=tbase e; q=qbank e; control=0x143; second=if zero then 0 else 0x357
    parameters=zip [0x28,0x2c..0x40] [8,12..0x20] ++ (if e==1 then [(0x44,0x24),(0x48,0x28)] else []) ++ zip [0x4c,0x50,0x54] [0x2c,0x34,0x38]
    before=changes ([(t+0x10,dirty),(t+0x14,force),(t+0x20,control),(t+0x24,second),(q,if equal then control .&. complement 0x100 else 0x991),(q+4,second)]++[(t+a,0x16370000+a*0x101) | a <- [0x28,0x2c..0x54]]) seed
    change=not equal || force==1
    rc=if dirty/=0 && change then 1 else 0
    writes=if dirty==0 then [] else [(q+b,getWord before (t+a)) | (a,b) <- parameters] ++ (if change then [(q,control)]++[(q+4,second) | second/=0] else [])
    after=if dirty==0 then before else changes writes $ word (t+0x10) 0 $ (if change then word (t+0x14) 0 else id) before

immediateCase :: Word32 -> Word32 -> Scenario
immediateCase e flags = (scenario (key "immediate" [e,flags]) Immediate [e,0x200000] before after (Just 0)) {expectedWrites=Just [(qbank e+0x34,value)]}
  where
    t=tbase e; old=0xe432a5fa
    before=word (t+0x50) old $ put 0x200000 (B.pack [0xbc,0x1a,5,1,fromIntegral flags]) seed
    replace mask v x = (x .&. complement mask) .|. (v .&. mask)
    low=if flags .&. 2/=0 then 5 else 3
    middle=if flags .&. 1/=0 then 0xabc else 0x123
    high=if flags .&. 4/=0 then 1 else 0
    value=replace 0x10000000 (high `shiftL` 28) $ replace 0x0fff0000 (middle `shiftL` 16) $ replace 7 low old
    after=word (qbank e+0x34) value $ word (t+0x50) value $ byte (t+0x1c) flags before

formatterOffsets :: [Word32]
formatterOffsets = [0,4,0x20,0xc8,0x34]++[0x38,0x3c..0x48]++[0x64,0x68..0x74]++[0x78]++[0x7c,0x80..0x90]++[0x94,0x98..0xa8]++[0xac,0xb0..0xc0]++[0xc4]

formatterCase :: Word32 -> Word32 -> Scenario
formatterCase e dirty = (scenario (key "formatter" [e,dirty]) Formatter [e] before after Nothing) {expectedWrites=Just writes}
  where
    u=ubase e
    before=changes ((u+0x34,dirty):[(u+0x38+a,0x72410000+a*0x103) | a <- formatterOffsets]) seed
    writes=if dirty==0 then [] else [(fbank e+a,getWord before (u+0x38+a)) | a <- formatterOffsets]
    after=if dirty==0 then before else changes writes $ word (u+0x34) 0 before

heldCases :: Word32 -> [Scenario]
heldCases e = [heldCase "reset-held" Reset (word (dbank e) 0x27182818 seed) (word (dbank e) 0 seed) [(dbank e,0x10000),(dbank e,0)],heldCase "wait-held" Wait (word (qbank e+0x30) 0 $ word (qbank e) 0x37192818 seed) (word (qbank e) 0x37182818 $ word (qbank e+0x30) 0 seed) [(qbank e,0x37182818)],heldCase "strobe-held" Strobe (word (fbank e) 0x07182818 seed) (word (fbank e) 0x17182818 seed) [(fbank e,0x17182818)]]
  where heldCase name op before after writes=(scenario (key name [e]) op [e] before after Nothing) {held=True,expectedWrites=Just writes}

schedulerCases :: [Scenario]
schedulerCases =
  [registerCase n order 1 0 0 1 0 0 False | n <- [0..4],order <- [0,20,25,0xffffffff]]
  ++ [registerCase 2 35 persistent phase selector live failure counter False | persistent <- [0,1,2],phase <- [0,1,2],(selector,live,failure,counter) <- [(0,0,0,0),(1,1,0,0xffffffff),(1,0,negate 7,9)]]
  ++ [registerCase 4 0 1 2 0 0 0 99 True]
  ++ [isrCase engine status count active live releaseFailure configureFailure | engine <- [0,1],status <- [0,2],count <- [0,2],(active,live,releaseFailure,configureFailure) <- [(1,1,0,0),(0,1,0,0),(1,0,0,0),(1,1,negate 9,0),(1,1,0,negate 7)]]

slot :: Word32 -> Word32 -> [Word32] -> Memory -> Memory
slot base index values=put (base+index*16) (assemble values)

registerCase :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Bool -> Scenario
registerCase occupied order persistent phase selector live failure counter nullCallback =
  scenario (key "register" [occupied,order,persistent,phase,selector,live,failure,counter,if nullCallback then 1 else 0]) Register [0x200000,0x200040] before after (Just rc)
  where
    list=if persistent==0 then 0x917f24 else 0x917ee4
    handleCell=if selector==0 then 0x917ed4 else 0x917ed0
    incoming=if nullCallback then 0 else callback 5
    records=(if occupied==0 then [] else [[101+n,40-n*10,callback (n+1),0] | n <- [0..occupied-1]])++replicate (4-fromIntegral occupied) [0,0,0,0]
    before=changes [(0x917ed8,counter),(handleCell,live),(0x942a4c,selector),(0x210f00,failure),(0x210f04,if failure==0 then 0x7a11 else 0)] $ put list (B.concat (map assemble records)) $ put 0x200000 (assemble [incoming,persistent,order,phase]) $ word 0x200040 0xdeadbeef seed
    position=length (takeWhile (\record -> record!!1>=order) records)
    accepted=not nullCallback && position<4
    normalized=if phase==1 then 1 else 0
    next=if counter==0 then 1 else counter
    rc=if nullCallback then negate 30 else if accepted then 0 else negate 20
    newRecord=[next,order,incoming,normalized]
    inserted=take 4 (take position records++[newRecord]++drop position records)
    normalizedState=if nullCallback then before else word (0x200000+12) normalized $ word 0x917ed8 (next+1) before
    insertedState=if accepted then put list (B.concat (map assemble inserted)) $ word 0x200040 next $ word 0x917ee0 normalized normalizedState else word 0x200040 0 normalizedState
    registered=accepted && live==0
    source=(if selector==0 then 11 else 9)+normalized
    irqState=if registered then irqReceipt source $ word handleCell (if failure==0 then 0x7a11 else 0) insertedState else insertedState
    after=if not nullCallback && persistent==1 && (not accepted || live/=0 || failure==0) then word 0x917edc 1 irqState else irqState

isrCase :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Scenario
isrCase engine status count active live releaseFailure configureFailure =
  scenario (key "isr" [engine,status,count,active,live,releaseFailure,configureFailure]) (if engine==0 then IsrZero else IsrOne) [] before after Nothing
  where
    p=if status .&. 2==0 then 1 else 0
    cell=if engine==0 then 0x917ed4 else 0x917ed0
    handle=if live==0 then 0 else 0x531
    one=0x917f24; persistent=0x917ee4
    before=changes [(cell,handle),(0x917edc,active),(0x917ee0,count),(qbank engine+0x30,status),(0x210f00,configureFailure),(0x210f04,if configureFailure==0 then 0x7a11 else 0),(0x210f08,releaseFailure)
      ,(0x210c10,one+8),(0x210c14,one+16+8),(0x210c18,callback 3)
      ,(0x210c30,one+16),(0x210c34,one+8),(0x210c38,callback 2)
      ,(0x210c40,persistent+8),(0x210c44,persistent+16),(0x210c48,77)
      ,(0x210c50,persistent+16)]
      $ slot one 0 [21,50,callback 1,p] $ slot one 1 [22,40,callback 2,p]
      $ slot persistent 0 [31,30,callback 4,p] $ slot persistent 1 [0,20,callback 5,p]
      $ slot persistent 2 [33,10,callback 6,p] seed
    switching=status .&. 2==0 || count/=0
    released=handle/=0 && (switching || active==0)
    remaining=if released && releaseFailure==0 then 0 else handle
    configured=switching && remaining==0 && active/=0
    finalHandle=if configured then if configureFailure==0 then 0x7a11 else 0 else remaining
    source=(if engine==0 then 11 else 9)+(if status .&. 2==0 then 0 else 1)
    irqState=(if configured then irqReceipt source else id) $ (if released then releaseReceipt handle else id) before
    after=receipt [1,3,4,5,6] $ changes [(cell,finalHandle),(0x917edc,1),(0x917ee0,if p==1 then count-2 else count)
      ,(0x210804,0),(0x21080c,0),(0x210810,callback 4),(0x210814,77),(0x210818,0)]
      $ word (one+8) (callback 2) $ slot one 1 [0,0,0,0] $ slot one 0 [0,0,0,0] $ word (persistent+16) 77 irqState

commitMap :: Word32 -> [(Word32,Word32)]
commitMap e =
  zip [0,4..0x14] [0,4..0x14] ++ [(0x18,0x18) | e==1]
  ++ zip [0x1c,0x20,0x24,0x28,0x2c,0x30] [0x1c,0x20,0x38,0x3c,0x88,0x8c]
  ++ zip [0x34,0x38,0x3c,0x40,0x44,0x48,0x4c] [0x24,0x28,0x2c,0x30,0xd4,0xd8,0xdc]
  ++ zip [0x50,0x54,0x58,0x9c,0xa0,0xa4] [0x34,0x40,0xe0,0x84,0x90,0xe4]
  ++ concat [zip [0x5c+n*16,0x60+n*16,0x64+n*16,0x68+n*16,0xa8+n*16,0xac+n*16,0xb0+n*16,0xb4+n*16] [0x44+n*4,0x54+n*4,0x64+n*4,0x74+n*4,0x94+n*4,0xa4+n*4,0xb4+n*4,0xc4+n*4] | n <- [0..3]]
  ++ zip [0xec,0xf0,0x174,0xf4,0xf8,0x178] [0x10c,0x110,0x194,0x114,0x118,0x198]
  ++ concat [zip [0xfc+n*8,0x100+n*8] [0x11c+n*8,0x120+n*8] | n <- [0..9]]
  ++ zip [0x14c,0x150,0x154] [0x16c,0x170,0x174]
  ++ zip [0x158,0x15c,0x160,0x164] [0x178,0x17c,0x180,0x184]
  ++ zip [0x168,0x16c,0x170,0xe8] [0x188,0x18c,0x190,0x108]
  ++ zip [0x17c,0x180,0x184,0x188,0x18c,0x190,0x194,0x198,0x19c,0x1a0] [0xe8,0xec,0xf0,0xf4,0xf8,0xfc,0x100,0x1bc,0x9d0,0x9d4]

commitCases :: [Scenario]
commitCases=[commitCase e pending 1 0 False False | e <- [0,1],pending <- [0,2]] ++
  [commitCase e 1 mode auxiliary reverseVertical additions | e <- [0,1],(mode,auxiliary) <- [(1,9),(2,0),(2,1)],reverseVertical <- [False,True],additions <- [False,True]]
  ++ [let c=commitCase e pending 1 0 False False in c {label=key "commit-wrapper" [e,pending],operation=if e==0 then CommitZero else CommitOne,arguments=[]} | e <- [0,1],pending <- [0,1,2]]

commitCase :: Word32 -> Word32 -> Word32 -> Word32 -> Bool -> Bool -> Scenario
commitCase e pending mode auxiliary reverseVertical additions =
  (scenario (key "commit" [e,pending,mode,auxiliary,if reverseVertical then 1 else 0,if additions then 1 else 0]) Commit [e] before after Nothing) {expectedWrites=Just writes}
  where
    r=rbase e; s=sbase e; i=ibase e
    first=r+0x24; second=r+0x3c
    callbackState=changes [(first,callback 1),(first+8,callback 2),(second,callback 4)
      ,(0x210c10,first),(0x210c14,first+20),(0x210c18,callback 3)
      ,(0x210c20,first+8),(0x210c24,first),(0x210c28,callback 6)
      ,(0x210c40,second),(0x210c44,second+20),(0x210c48,callback 5)] seed
    records=zip [0..] ([i+0xf4+n*32 | n <- [0..9]]++[i+0xb4+n*32 | n <- [0..1]])
    record (n,a)=put a (assemble (map (castFloatToWord32 . fromIntegral) [4*n+1,4*n+2,4*n+3,4*n+4]++[((9+n) `shiftL` 16) .|. ((negate (7+n)) .&. 0xffff),0x40400000,0x12345678,200 .|. (if even n then 0x100 else 0)]))
    before=foldr record (changes ([(r+0xc,pending),(r+4,mode),(r+8,auxiliary),(s+0xe8,0x000303ff),(s+8,0x00000120),(i+0x3c,0x3f),(i+0xc,0xc123d456),(i+0x34,if additions then 5 else 0),(i+0x38,if additions then 12 else 0)]++[(i+0x14+n*4,0xd321c654+n*0x10001) | n <- [0..7]]) $ half (s+0x12) 20 $ byte 0x916611 (if reverseVertical then 1 else 0) callbackState) records
    prepared=if pending==1 then prepare e mode auxiliary reverseVertical additions before else before
    writes=if pending/=1 then [] else [(dbank e+b,getWord prepared (s+a)) | (a,b) <- commitMap e]
    drained=changes [(first,callback 6),(first+8,0),(first+20,0),(second,0),(second+20,0)] prepared
    after=receipt [1,2,3,4,5] $ changes writes drained

prepare :: Word32 -> Word32 -> Word32 -> Bool -> Bool -> Memory -> Memory
prepare e mode auxiliary reverseVertical additions before = groups
  where
    s=sbase e; i=ibase e
    maskedHalf a mask value m=half a ((getHalf m a .&. complement mask) .|. (value .&. mask)) m
    defaults=foldr (\(a,value) -> maskedHalf (s+a) 0x7ff value) (changes [(s+0x158,0x12345678),(s+0x15c,0),(s+0x160,0),(s+0x164,0)] $ half (s+0x16c) 128 $ word (rbase e+0xc) 0 before) [(0x14c,3),(0x14e,0),(0x150,0),(0x152,0)]
    inputRects=[(i+0x58+n*8,s+0xfc+n*8) | n <- [0..9]]++[(i+0x44+n*8,s+0xec+n*8) | n <- [0..1]]
    coordinates=foldr (\(n,(a,_)) m -> foldr (\(offset,value) -> maskedHalf (a+offset) 0xfff value) m [(0,4*n+2),(2,4*n+1),(4,4*n+4),(6,4*n+3)]) defaults (zip [0..] inputRects)
    copied=foldr (\(a,b) m -> foldr (\offset -> maskedHalf (b+offset) 0xfff (getHalf coordinates (a+offset))) m [0,2,4,6]) coordinates inputRects
    flags=foldr (\(n,a) -> byte a (if even n then 1 else 0)) copied (zip [0::Word32 ..] ([i+0xa8..i+0xb1]++[i+0x54,i+0x55]))
    selectors=changes [(s+0x154,getWord flags (s+0x154) .&. 0xf0f00000),(s+0x168,getWord flags (s+0x168) .&. 0xf0f00000),(s+0x170,getWord flags (s+0x170) .&. 0xfffcfc00)] flags
    scaled=if mode==2 && auxiliary/=0 then 100 else 1
    signed=foldr (\(a,v) -> maskedHalf (s+a) 0x7ff v) selectors [(0x174,19*scaled),(0x176,negate (17*scaled)),(0x178,20*scaled),(0x17a,negate (18*scaled))]
    reversed=if reverseVertical then foldr (\(n,(_,b)) m -> if even n then maskedHalf (b+2) 0xfff (21-(4*n+3)) $ maskedHalf (b+6) 0xfff (21-(4*n+1)) m else m) signed (zip [0..] inputRects) else signed
    control=if reverseVertical then 0x3e else 0x3f
    scalar=word (s+4) control $ word (s+0x34) (getWord before (i+0xc)) reversed
    origins=(if additions && control .&. 1/=0 then maskedHalf (s+0x36) 0x3fff ((getHalf scalar (s+0x36) .&. 0x3fff)+5) else id)
      $ (if additions then maskedHalf (s+0x34) 0x3fff ((getHalf scalar (s+0x34) .&. 0x3fff)+12) else id) scalar
    bases=changes [(s+group*0x4c+0x5c+n*16,getWord before (i+0x14+group*16+n*4)) | group <- [0,1],n <- [0..3]] origins
    groups=if not additions then bases else foldr (\(group,n) m -> let a=s+group*0x4c+0x5c+n*16; horizontal=if group==0 then 3 else 6 in maskedHalf (a+2) 0x3fff ((getHalf bases (a+2) .&. 0x3fff)+5) $ maskedHalf a 0x3fff ((getHalf bases a .&. 0x3fff)+horizontal) m) bases [(g,n) | g <- [0,1],n <- [0..3]]

captureCases :: [Scenario]
captureCases=[captureCase e first changed failure | e <- [0,1],(first,changed) <- [(True,False),(False,False),(False,True)],failure <- [0,negate 7]]

captureCase :: Word32 -> Bool -> Bool -> Word32 -> Scenario
captureCase e first changed failure = scenario (key (if first then "capture-reset-receipt" else "capture-config") [e,if changed then 1 else 0,failure]) Capture [e,0x200000] before after (Just rc)
  where
    r=rbase e; s=sbase e
    packet=if first then [0,0x33,0x44] else if changed then [5,6,7] else [2,3,4]
    before=changes [(r,3),(r+4,2),(r+8,4),(0x210f00,failure),(0x210f04,if failure==0 then 0x7a11 else 0)] $ byte (r+0x54) (if first then 0 else 1) $ put 0x200000 (assemble packet) seed
    runtime=if first then changes [(r,0x33),(r+4,if e==0 then 3 else 1),(r+8,0x44)] $ put r (B.replicate 0x58 0) before else if changed then changes [(r,6),(r+4,5),(r+8,7)] before else before
    captured=if first then byte s (getByte before (dbank e) .|. 6) $ changes [(s+a,getWord before (dbank e+b)) | (a,b) <- commitMap e,a/=0xe8] runtime else runtime
    resetDone=if first then changes [(0x210300,1),(0x210304,e)] captured else captured
    registered=if not first then resetDone else if e==1 then word (r+0x14) (if failure==0 then 0x7a11 else 0) $ irqReceipt 14 resetDone else word (r+0x14) 1 $ word 0x917ed4 (if failure==0 then 0x7a11 else 0) $ word 0x917ed8 2 $ word 0x917edc (if failure==0 then 1 else 0) $ slot 0x917ee4 0 [1,1,0x6cb0c4,0] $ irqReceipt 11 resetDone
    rc=if first && e==1 then failure else 0
    after=if first || changed then byte (r+0x54) (if first then 1 else 2) registered else registered

services :: [(Word32,B.ByteString)]
services =
  [(0x7235d8,assemble (mov 0 0x13++[0xe12fff1e])),(0x7236b0,assemble [0xe12fff1e]),(0x723ce8,assemble (mov 12 0x211000++[0xe59c3000,0xe2833001,0xe58c3000,0xe58c0004,0xe12fff1e]))
  ,(0x69f8fc,assemble (mov 12 0x4000000++[0xe7dc0000,0xe12fff1e]))
  ,(0x64e918,assemble (mov 12 0x210100++[0xe59c3000,0xe2833001,0xe58c3000,0xe78c0103,0xe58c1304]++mov 12 0x210f00++[0xe59c3004,0xe5823000,0xe59c0000,0xe12fff1e]))
  ,(0x64eb20,assemble (mov 12 0x210200++[0xe59c3000,0xe2833001,0xe58c3000,0xe78c0103]++mov 12 0x210f08++[0xe59c0000,0xe12fff1e]))]
  ++ [(callback n,assemble (mov 12 0x210000++[0xe59c3000,0xe2833001,0xe58c3000]++mov 0 n++[0xe78c0103]++mov 12 (0x210c00+n*16)++[0xe59c0000,0xe5901000]++mov 0 (0x210800+n*4)++[0xe5801000,0xe59c0004,0xe59c1008,0xe3500000,0x15801000,0xe12fff1e])) | n <- [1..6]]

callback :: Word32 -> Word32
callback n=0x230000+n*0x100

receipt :: [Word32] -> Memory -> Memory
receipt ns m=changes ((0x210000,fromIntegral (length ns)):zip [0x210004,0x210008..] ns) m

irqReceipt :: Word32 -> Memory -> Memory
irqReceipt source=changes [(0x210100,1),(0x210104,source),(0x210404,if source==14 then 0x6cb0d8 else if source>=11 then 0x6c70e4 else 0x6c6eb0)]

releaseReceipt :: Word32 -> Memory -> Memory
releaseReceipt handle=changes [(0x210200,1),(0x210204,handle)]

ownedSource :: E.Elf -> [Root] -> Either String ([CaseRegion],[(GuestAddress,Word32)],M.Map String Word32)
ownedSource elf roots = do
  let allocated=[s | s <- E.elfSections elf,E.sectionAllocated s,E.sectionSize s>0]
  unless (not (null allocated) && all (\s -> not (E.sectionWritable s) && E.sectionType s==1 && E.sectionAddress s>=0x2000000 && toInteger (E.sectionAddress s)+toInteger (E.sectionSize s)<=0x2040000) allocated) (Left "owned ELF escapes read-only artifact")
  code <- forM (zip [0::Int ..] allocated) $ \(n,s) -> do
    bytes <- forM [E.sectionAddress s..E.sectionAddress s+E.sectionSize s-1] $ \a -> maybe (Left "missing allocated ELF byte") Right (E.byteAt elf a)
    pure (mkRegion (RegionId ("owned-"++show n)) (address (E.sectionAddress s)) (fromIntegral (E.sectionSize s)) (if E.sectionExecutable s then ReadExecute else ReadOnly) (B.pack bytes))
  let functions=M.toAscList (M.fromListWith max [(address (E.symbolValue s),E.symbolSize s) | s <- E.elfSymbols elf,E.symbolKind s==E.FunctionSymbol,E.symbolSize s>0,E.symbolValue s>=0x2000000,E.symbolValue s<0x2040000])
      names=M.fromList [(E.symbolName s,E.symbolValue s) | s <- E.elfSymbols elf,E.symbolKind s==E.FunctionSymbol,E.symbolSize s>0]
  forM_ (["xm1_scanout_"++fst (root op) | op <- roots]++["xm1_scanout_commit_zero","xm1_scanout_commit_one"]) $ \name ->
    unless (length [s | s <- E.elfSymbols elf,E.symbolName s==name,E.symbolKind s==E.FunctionSymbol,E.symbolSize s>0]==1) (Left ("owned ELF export missing or ambiguous: "++name))
  pure (code,functions,names)

makeSpec :: String -> Word32 -> [CaseRegion] -> [(GuestAddress,Word32)] -> Scenario -> CaseSpec
makeSpec role entry code functions c = CaseSpec
  { caseId=role++"-"++label c,caseProvenance=pin++"; synthetic distinct saved words, reserved-bit sentinels and memory-backed D/Q/F pages"
  , caseExclusion="CPU sequence only. No autonomous interrupt, reset/strobe progress, frame timing, latch, composition or physical display law is modeled. Held polls must exhaust the bounded instruction budget. Capture first-init cases replace only reset with a CPU receipt boundary and do not qualify reset completion."
  , caseEntry=address entry,caseExit=address 0x220000
  , caseRegions=code++[mkRegion (RegionId name) (address a) n ReadWrite (bytesAt (initial c) a n) | (name,a,n) <- regionLayout c]++[mkRegion (RegionId "stack") (address 0x300000) 0x4000 ReadWrite (B.replicate 0x4000 0),mkRegion (RegionId "exit") (address 0x220000) 4 ReadExecute (word32le 0xe12fff1e)]++[mkRegion (RegionId ("boundary-"++show a)) (address a) (B.length bytes) ReadExecute bytes | (a,bytes) <- boundaries]
  , caseFunctions=functions++[(address a,fromIntegral (B.length bytes)) | (a,bytes) <- boundaries]
  , caseInitial=zip [R0,R1,R2,R3] (take 4 (arguments c++repeat 0))++[(R12,0xa500000c),(Sp,0x303000),(Lr,0x220000),(Cpsr,0x13)]++abi
  , caseExpectedStatus=if held c then InstructionBudget else Returned
  , caseExpectedRegisters=if held c then [] else [(Sp,0x303000)]++abi++maybe [] (\v -> [(R0,v)]) (result c)
  , caseExpectedMemory=[(RegionId name,bytesAt (expected c) a n) | (name,a,n) <- regionLayout c,name/="yield-receipt"]
  , caseIgnoredMemory=[],caseCp14Identity=Nothing,caseKernel=Just (KernelCase (InstructionLimit (if held c then 4096 else 200000)) Nothing Nothing Nothing Nothing [] []) }
  where
    abi=[(r,0xa5000000+fromIntegral (fromEnum r)) | r <- [R4,R5,R6,R7,R8,R9,R10,R11]]
    boundaries=[service | service@(a,_) <- services,role/="owned" || a/=0x69f8fc]

regionLayout :: Scenario -> [(String,Word32,Int)]
regionLayout c=[(name,a,if name=="cache" then 0x420000 else n) | (name,a,n) <- regions,name/="cache" || usesCalibration c,a<0xfff00000 || paired c && operation c `notElem` [Defaults,Stage]]++[("yield-receipt",0x211000,8) | operation c==Wait]

usesCalibration :: Scenario -> Bool
usesCalibration c=paired c && operation c `elem` [Immediate,Defaults,Stage,EngineInitialize]

caller :: Scenario -> Word32 -> [CaseRegion] -> [(GuestAddress,Word32)] -> (Word32,[CaseRegion],[(GuestAddress,Word32)])
caller c entry code functions
  | repetitions c==1=(entry,code,functions)
  | otherwise=(0x240000,mkRegion (RegionId "caller") (address 0x240000) (B.length program) ReadExecute program:code,(address 0x240000,fromIntegral (B.length program)):functions)
  where
    program=assemble ([0xe92d4010]++concat (replicate (repetitions c) (concat [mov register value | (register,value) <- zip [0..3] (arguments c)]++mov 12 entry++[0xe12fff3c]))++[0xe8bd8010])

resetBoundary :: B.ByteString
resetBoundary=assemble (mov 12 0x210300++[0xe59c3000,0xe2833001,0xe58c3000,0xe58c0004,0xe12fff1e])

patchCode :: Word32 -> B.ByteString -> [CaseRegion] -> Either String [CaseRegion]
patchCode entry replacement code = do
  unless (length (filter contains code)==1) (Left "reset receipt patch is missing or ambiguous")
  pure [if contains item then item {caseRegion=region {regionBytes=B.take offset bytes<>replacement<>B.drop (offset+B.length replacement) bytes}} else item | item <- code,let region=caseRegion item,let offset=fromIntegral entry-fromIntegral (baseAddress region),let bytes=regionBytes region]
  where contains item=let region=caseRegion item in fromIntegral entry>=baseAddress region && toInteger entry+toInteger (B.length replacement)<=toInteger (baseAddress region)+toInteger (regionSize region)

ownedScenario :: M.Map String Word32 -> Scenario -> Either String Scenario
ownedScenario names c = do
  pointers <- forM [(0x6cb0c4,"xm1_scanout_commit_zero"),(0x6cb0d8,"xm1_scanout_commit_one"),(0x6c70e4,"xm1_scanout_isr_zero"),(0x6c6eb0,"xm1_scanout_isr_one")] $ \(stock,name) -> do
    owned <- maybe (Left ("missing owned callback export "++name)) Right (M.lookup name names)
    pure (stock,owned)
  let relocate a m=case lookup (getWord m a) pointers of Just owned -> word a owned m; Nothing -> m
  pure c {expected=foldr relocate (expected c) [0x210404,0x917eec]}

needsResetReceipt :: Scenario -> Bool
needsResetReceipt c=operation c==Capture && case arguments c of e:_ -> getByte (initial c) (rbase e+0x54)==0; _ -> False

mmioWrites :: NativeObservation -> Maybe [(Word32,Word32)]
mmioWrites observation = do
  ordered <- nativeKernel observation >>= observedOrder
  fmap concat $ forM ordered $ \event -> case event of
    OrderedWrite w (WriteValue value) | guestAddress (writeAddress w)>=0xfff69000 && guestAddress (writeAddress w)<0xfff6f000 ->
      if writeBytes (writeLength w)==4 then Just [(fromIntegral (guestAddress (writeAddress w)),fromIntegral value)] else Nothing
    _ -> Just []

check :: Scenario -> NativeObservation -> IO ()
check c observation = do
  unless (nativeStatus observation==if held c then InstructionBudget else Returned) (fail (label c++": unexpected stop"))
  forM_ (caseExpectedMemory (makeSpec "oracle" 0 [] [] c)) $ \(name,wanted) -> unless (lookup name (nativeMemory observation)==Just wanted) (fail (label c++": independent memory oracle differs: "++show name))
  forM_ (expectedWrites c) $ \writes -> unless (mmioWrites observation==Just writes) (fail (label c++": independent ordered register-write oracle differs"))
  forM_ (caseExpectedRegisters (makeSpec "oracle" 0 [] [] c)) $ \(register,wanted) ->
    unless (lookupRegister register observation==Just wanted) (fail (label c++": independent register oracle differs: "++show register))
  unless (fmap (.&. 0x1ff) (lookupRegister Cpsr observation)==Just 0x13) (fail (label c++": caller CPSR control differs"))
  if operation c==Wait then case lookup (RegionId "yield-receipt") (nativeMemory observation) of
    Just bytes -> unless (readWordAt bytes 0>0 && readWordAt bytes 4==0) (fail (label c++": yield receipt differs"))
    Nothing -> fail (label c++": yield receipt missing")
  else pure ()

boundaryCases :: [Scenario]
boundaryCases =
  [reject (key "boundary-engine" [e,fromIntegral (fromEnum op)]) op [e,0x200000,0,0] seed | e <- [2,0xffffffff],op <- [Item,Range,Enable,Options,Blank,Flush,Defaults,Stage,Immediate,Reset,Wait,Formatter,Strobe,Commit,Capture]]
  ++ [reject (key "boundary-group" [e,g,fromIntegral (fromEnum op)]) op (if op==Options then [e,g,0x200000] else [e,g,0,0x200000]) (byte (rbase e+0x54) 1 seed) | e <- [0,1],g <- [2,0xffffffff],op <- [Item,Range,Enable,Options]]
  ++ [let before=put 0x200000 (assemble packet) seed in reject (key "boundary-stage" (e:packet)) Stage [e,0x200000] before | e <- [0,1],packet <- [[10,0,0],[0xffffffff,0,0],[0,4,0],[0,0xffffffff,0],[0,0,6],[0,0,0xffffffff]]]
  where reject name op args before=(scenario name op args before before (Just (negate 30))) {paired=False}

-- Panel-cohort entries whose bodies touch only RAM: no device page and no retained callback.
selectedEngine :: Memory -> Word32
selectedEngine m = if getWord m 0x955b20==0 && getWord m 0x955b08==1 then 1 else 0

modeSeed :: Word32 -> Word32 -> Memory
modeSeed forced mode = changes [(0x955b20,forced),(0x955b08,mode)] seed

packetHalfwords :: [Word32] -> B.ByteString
packetHalfwords = B.concat . map (B.take 2 . word32le)

modeCases :: [Scenario]
modeCases = concat
  [ [windowCase f m sel packet | f <- [0,1],m <- [0,1,2],sel <- [0,1,2,3],packet <- [[0x111,0x222,5,3],[0xfff,0,1,0xffff],[0,0,0,0]]]
  , [backingCase f m v | f <- [0,1],m <- [0,1],v <- [0,0x3fffffff,0xffffffff,0x12345678]]
  , [enabledCase f m initialized alt v | f <- [0,1],m <- [0,1],initialized <- [0,1],alt <- [0,1],v <- [0,1,2]]
  , [modeRangeCase f m g i v | f <- [0,1],m <- [0,1],g <- [0,1],i <- [0,1,3,255,4],v <- [0,1,2]]
  , [colourCase f m v | f <- [0,1],m <- [0,1],v <- [0,0x123456,0xffffff,0xffffffff]]
  , [colourPacketCase f m first second third flag v | f <- [0,1],m <- [0,1],(first,second,third) <- [(0x11,0x22,0x33),(0xff,0,0x80)],flag <- [0,1],v <- [0,0x89abcdef]]
  , [windowSizeCase f m sel | f <- [0,1],m <- [0,1],sel <- [0,1,2,3]]
  ]

windowCase :: Word32 -> Word32 -> Word32 -> [Word32] -> Scenario
windowCase forced mode selector packet = scenario (key "mode-window" (forced:mode:selector:packet)) ModeWindow [selector,0x200000] before after (Just rc)
  where
    before=put 0x200000 (packetHalfwords packet) (modeSeed forced mode)
    e=selectedEngine before
    rc=if selector>2 then negate 30 else 0
    origin=sbase e+0x1c+selector*8
    size=sbase e+0x20+selector*8
    originValue=(getWord before origin .&. 0xf000f000) .|. ((packet!!0 .&. 0xfff) `shiftL` 16) .|. (packet!!1 .&. 0xfff)
    sizeValue=(getWord before size .&. 0xf000f000) .|. (((packet!!2-1) .&. 0xfff) `shiftL` 16) .|. ((packet!!3-1) .&. 0xfff)
    after=if rc/=0 then before else word size sizeValue (word origin originValue before)

backingCase :: Word32 -> Word32 -> Word32 -> Scenario
backingCase forced mode value = scenario (key "mode-backing" [forced,mode,value]) ModeBacking [value] before after (Just 0)
  where
    before=modeSeed forced mode
    at=sbase (selectedEngine before)+0x3c
    after=word at ((getWord before at .&. 0xc0000000) .|. (value .&. 0x3fffffff)) before

enabledCase :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Scenario
enabledCase forced mode initialized alternate value = scenario (key "mode-enabled" [forced,mode,initialized,alternate,value]) ModeEnabled [value] before after (Just rc)
  where
    base=modeSeed forced mode
    e=selectedEngine base
    before=byte (rbase e+0x54) initialized (byte 0x916610 alternate base)
    rc=if initialized==0 then negate 10 else 0
    saved=sbase e
    alternateWord=ibase e
    flag=if value==1 then 1 else 0
    after
      | initialized==0=before
      | alternate/=0=word alternateWord ((getWord before alternateWord .&. complement 1) .|. flag) (byte saved (getByte before saved .&. 0xfe) before)
      | otherwise=byte saved ((getByte before saved .&. 0xfe) .|. flag) before

modeRangeCase :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Scenario
modeRangeCase forced mode group item value = scenario (key "mode-range" [forced,mode,group,item,value]) ModeRange [group,item,value] before after (Just rc)
  where
    base=modeSeed forced mode
    e=selectedEngine base
    before=byte (rbase e+0x54) 1 base
    mask | item==255=0x1111
         | item<4=1 `shiftL` fromIntegral (item*4)
         | otherwise=0
    rc | item>3 && item/=255=negate 30
       | otherwise=0
    at | getByte before 0x916610/=0=ibase e+4+group*4
       | otherwise=sbase e+0x54+group*0x4c
    old=getHalf before at
    new=(old .&. (complement (mask .&. 0xffff) .&. 0xffff)) .|. (if value==1 then mask else 0)
    after=if rc/=0 then before else half at new before

colourCase :: Word32 -> Word32 -> Word32 -> Scenario
colourCase forced mode value = scenario (key "mode-colour" [forced,mode,value]) ModeColour [value] before after (Just 0)
  where
    before=modeSeed forced mode
    at=sbase (selectedEngine before)+0x180
    after=byte (at+2) (value .&. 0xff) (byte (at+1) ((value `shiftR` 8) .&. 0xff) (byte at ((value `shiftR` 16) .&. 0xff) before))

colourPacketCase :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Scenario
colourPacketCase forced mode first second third flag value = scenario (key "mode-colour-packet" [forced,mode,first,second,third,flag,value]) ModeColourPacket [0x200000] before after (Just 0)
  where
    base=modeSeed forced mode
    e=selectedEngine base
    bytes=B.pack [fromIntegral flag,0,0,0,fromIntegral first,fromIntegral second,fromIntegral third,0] <> word32le value <> B.pack [0,0,0,0]
    before=put 0x200000 bytes base
    at=sbase e
    after=word (at+0x1a0) value (byte (at+0x19f) ((getByte before (at+0x19f) .&. 0xfe) .|. flag) (byte (at+0x19e) third (byte (at+0x19c) (((second .&. 0xf) `shiftL` 4) .|. (first .&. 0xf)) before)))

windowSizeCase :: Word32 -> Word32 -> Word32 -> Scenario
windowSizeCase forced mode selector = scenario (key "mode-window-size" [forced,mode,selector]) ModeWindowSize [selector,0x200000] before before (Just 0)
  where before=put 0x200000 (B.pack [0,0,0,0,0x34,0x12,0,0]) (modeSeed forced mode)

leaseCases :: [Scenario]
leaseCases = [releaseCase e initialized count | e <- [0,1],initialized <- [0,1],count <- [0,1,2,0xff]]

releaseCase :: Word32 -> Word32 -> Word32 -> Scenario
releaseCase e initialized count = scenario (key "operation-release" [e,initialized,count]) OperationRelease [e] before after (Just rc)
  where
    before=byte (rbase e+0x55) count (byte (rbase e+0x54) initialized seed)
    rc | initialized==0=negate 10
       | count==1=0
       | otherwise=negate 12
    after=if initialized==0 then before else byte (rbase e+0x55) (count-1) before

-- 0x6cbb88..0x6cbf24: every input is a calibration byte from the initialized cache.
engineCases :: [Scenario]
engineCases = [engineCase e route mode | e <- [0,1],route <- [0..3],mode <- [0,1,2,3,5,9]]

engineCase :: Word32 -> Word32 -> Word32 -> Scenario
engineCase e route mode = scenario (key "engine-initialize" [e,route,mode]) EngineInitialize [e,route,mode] before after Nothing
  where
    before=calibrationSeed
    base=sbase e
    pipeline=ibase e
    c offset=getByte calibrationSeed (0x4000000+offset)
    pair high low=(c low .|. (c high `shiftL` 8)) .&. 0xffff
    direct=route==3
    fixed | not direct=Nothing
          | mode>=5=Just (1919,1079,4096,4096,1920,1080)
          | mode==3 || mode==4=Just (1279,719,4096,4096,1280,720)
          | mode==2=Just (719,479,0xe33,4096,640,480)
          | otherwise=Nothing
    widthKeys | direct=(0x1041,0x1040)
              | route==2=(0xe32,0xe33)
              | otherwise=(0xe2e,0xe2f)
    heightKeys | direct=(if mode==1 then (0x1048,0x1047) else (0x1046,0x1045))
               | route==2=(0xe34,0xe35)
               | otherwise=(0xe30,0xe31)
    (width,height,horizontal,vertical,outputWidth,outputHeight)=case fixed of
      Just values->values
      Nothing->(pair (fst widthKeys) (snd widthKeys),pair (fst heightKeys) (snd heightKeys)
               ,if direct then pair 0x1043 0x1042 else 4096
               ,if direct then (if mode==1 then pair 0x104c 0x104b else pair 0x104a 0x1049) else 4096
               ,if route<=1 then 720 else (pair (fst widthKeys) (snd widthKeys) .&. 0xfff)+1
               ,if route<=1 then 480 else (pair (fst heightKeys) (snd heightKeys) .&. 0xfff)+1)
    interlaced=case fixed of
      Just _->mode `elem` [2,3,4,7,8,9]
      Nothing->route/=3 && (c (if route==2 then 0xe2d else 0xe2c) .&. 1)/=0
    dimension old value=(old .&. complement 0xfff) .|. (value .&. 0xfff)
    afterDimensions=half (base+18) (dimension (getHalf before (base+18)) width) (half (base+16) (dimension (getHalf before (base+16)) height) before)
    afterScales=half (base+0x186) horizontal (half (base+0x184) vertical afterDimensions)
    afterOutput=word (pipeline+0x234) outputWidth (word (pipeline+0x238) outputHeight afterScales)
    afterOrigins=word (pipeline+0x34) 0 (word (pipeline+0x38) 0 afterOutput)
    afterCopy=half (base+22) ((getHalf afterOrigins (base+22) .&. complement 0xfff) .|. (getHalf afterOrigins (base+18) .&. 0xfff))
             (half (base+20) ((getHalf afterOrigins (base+20) .&. complement 0xfff) .|. (getHalf afterOrigins (base+16) .&. 0xfff)) afterOrigins)
    after=if interlaced then byte (base+1) ((getByte afterCopy (base+1) .&. 0xfe) .|. 1) afterCopy else afterCopy

caseGroups :: B.ByteString -> [(String,[Scenario])]
caseGroups image=[("shadow",shadowCases),("timing",timingCases image),("scheduler",schedulerCases),("commit",commitCases),("capture",captureCases),("boundary",boundaryCases),("mode",modeCases),("lease",leaseCases),("engine",engineCases)]

proofLimit :: Integer
proofLimit=256*1024*1024

existingBytes :: FilePath -> IO Integer
existingBytes path = do
  exists <- doesPathExist path
  if not exists then pure 0 else do
    symbolic <- pathIsSymbolicLink path
    unless (not symbolic) (fail "proof output contains a symbolic link")
    directory <- doesDirectoryExist path
    if directory then sum <$> (mapM (existingBytes . (path </>)) =<< listDirectory path) else getFileSize path

data Action = StageCases | ExecuteCases deriving (Eq)

prospectiveBytes :: Action -> CaseSpec -> Integer
prospectiveBytes action spec =
  sum [toInteger (B.length bytes) | item <- caseRegions spec,let bytes=regionBytes (caseRegion item),not (B.all (==0) bytes)]
  +sum [2*toInteger (B.length bytes) | (_,bytes) <- caseExpectedMemory spec]
  -- Metadata reserve is 128 KiB per case; execution reserves the serialized report-file bound.
  +128*1024
  +(if action==ExecuteCases then reportFileLimit spec else 0)

reportFileLimit :: CaseSpec -> Integer
reportFileLimit spec=if any (\(_,bytes) -> B.length bytes>=0x420000) (caseExpectedMemory spec) then 40*1024*1024 else 8*1024*1024

loadBoundedObserved :: FilePath -> FilePath -> Integer -> IO NativeObservation
loadBoundedObserved runner path limit=do
  (_,mappedRegions,_,_)<-loadCase path
  (_,report)<-runCase runner path
  let encoded=encodeCanonical report
  unless(toInteger(B.length encoded)<=limit)(fail("serialized native report exceeds admitted file bound: bytes="++show(B.length encoded)++" limit="++show limit++"; case inputs remain staged"))
  B.writeFile(takeDirectory path</>"native-result.json")encoded
  exit<-either(fail . renderObservationError)pure(reportRunnerExit report)
  either(fail . renderObservationError)pure(observeNative exit mappedRegions(get report "runner"))

stockExtents :: [(Word32,Word32,String)]
stockExtents =
  [(0x6c6eb0,0x6ce384,"d6bb1379ce0bca32e65d08ace6c291c0657d69f616c34122ad29f4eafaddbcd1")
  ,(0x6cfe14,0x6d0f28,"9aaeaa0c847ac165e6b14375d9c6a8e138bd4967b05db6ebd52e5f7bbb046f81")
  ,(0x722054,0x722218,"3830f1e4dff71c4b79ebce225a2dede0cfa9da3a203da54df030e3d10be78c51")
  ,(0x89bcd8,0x89be8c,"ddead5f2a3ec281f83a3a1aca971cb8ad41dd2331b012996cc9ca69b83521355")]

stockFunctions :: [Word32] -> [(GuestAddress,Word32)]
stockFunctions starts=concat [spans low high | (low,high,_) <- stockExtents]
  where
    spans low high=case filter (\s -> s>=low && s<high) starts of
      [] -> [(address low,high-low)]
      roots -> zip (map address roots) (zipWith (-) (drop 1 roots++[high]) roots)

panelRoots :: [Root]
panelRoots=[op | op <- [minBound..maxBound],rootRole op==PanelElf]

mainRoots :: [Root]
mainRoots=[op | op <- [minBound..maxBound],rootRole op==MainElf]

main :: IO ()
main = do
  args <- getArgs
  case args of
    ["list"] -> forM_ (caseGroups B.empty) $ \(category,cases) -> forM_ cases $ \c -> putStrLn (category++" "++label c)
    ["stage",runner,imagePath,elfPath,out] -> run StageCases runner imagePath elfPath Nothing out "all"
    ["execute",runner,imagePath,elfPath,out] -> run ExecuteCases runner imagePath elfPath Nothing out "all"
    ["stage",runner,imagePath,elfPath,out,selection] -> run StageCases runner imagePath elfPath Nothing out selection
    ["execute",runner,imagePath,elfPath,out,selection] -> run ExecuteCases runner imagePath elfPath Nothing out selection
    ["stage",runner,imagePath,elfPath,panelElf,out,selection] -> run StageCases runner imagePath elfPath (Just panelElf) out selection
    ["execute",runner,imagePath,elfPath,panelElf,out,selection] -> run ExecuteCases runner imagePath elfPath (Just panelElf) out selection
    _ -> fail "usage: display-scanout-native list | (stage|execute) RUNNER IMAGE4 OWNED_ELF PRIVATE_OUTPUT [all|CATEGORY|CASE_ID] | (stage|execute) RUNNER IMAGE4 OWNED_ELF PANEL_ELF PRIVATE_OUTPUT [all|CATEGORY|CASE_ID]"

run :: Action -> FilePath -> FilePath -> FilePath -> Maybe FilePath -> FilePath -> String -> IO ()
run action runner imagePath elfPath panelElfPath out selection = do
  image <- B.readFile imagePath
  unless (sha256Hex (sha256 image)=="f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") (fail "image4 identity differs")
  elf <- either fail pure . E.parseElf =<< B.readFile elfPath
  (mainCode,mainFunctions,mainNames) <- either fail pure (ownedSource elf mainRoots)
  panel <- traverse (\path -> do
    parsed <- either fail pure . E.parseElf =<< B.readFile path
    either fail pure (ownedSource parsed panelRoots)) panelElfPath
  let ownedFor role=case role of
        MainElf->pure (mainCode,mainFunctions,mainNames)
        PanelElf->maybe (fail ("panel cohort case needs a PANEL_ELF argument: "++selection)) pure panel
  let slice a b=B.take (fromIntegral (b-a)) (B.drop (fromIntegral (a-0x61c400)) image)
  forM_ stockExtents $ \(a,b,digest) ->
    unless (sha256Hex (sha256 (slice a b))==digest) (fail ("stock executable extent differs: "++show a))
  let stockCode=[mkRegion (RegionId ("stock-"++show a)) (address a) (fromIntegral (b-a)) ReadExecute (slice a b) | (a,b,_) <- stockExtents]
        ++[mkRegion (RegionId ("selectors-"++show a)) (address a) (fromIntegral (b-a)) ReadOnly (slice a b) | (a,b) <- [(0x7cd640,0x7cd7d0),(0x828918,0x828d98)]]
      starts=sort (map (snd . root) [minBound..maxBound])
      functions=stockFunctions starts
      groups=caseGroups image
      allCases=concatMap snd groups
      selected=if selection=="all" then allCases else maybe (filter ((==selection) . label) allCases) id (lookup selection groups)
  unless (not (null selected)) (fail "unknown or empty scanout scenario selection")
  present <- existingBytes out
  prospective <- foldM (\total c -> do
    (ownedCode,ownedFunctions,names) <- ownedFor (rootRole (operation c))
    ownedCase <- either fail pure (ownedScenario names c)
    let (stockEntry,stockCallerCode,stockCallerFunctions)=caller c (snd (root (operation c))) stockCode functions
        (ownedEntry,ownedCallerCode,ownedCallerFunctions)=caller ownedCase 0 ownedCode ownedFunctions
        stockSpec=makeSpec "stock" stockEntry stockCallerCode stockCallerFunctions c
        ownedSpec=makeSpec "owned" ownedEntry ownedCallerCode ownedCallerFunctions ownedCase
    pure (total+(if paired c then prospectiveBytes action stockSpec else 0)+prospectiveBytes action ownedSpec)) 0 selected
  let requiredBytes=present+prospective
  unless (requiredBytes<=proofLimit) (fail ("proof allowance exceeded: required_bound_bytes="++show requiredBytes++" existing_bytes="++show present++" prospective_bound_bytes="++show prospective++" limit_bytes="++show proofLimit++"; select a named category or case and retain the same output root"))
  resetEntry <- case [s | s <- E.elfSymbols elf,"display_scanout6timing5reset" `isInfixOf` E.symbolName s,E.symbolKind s==E.FunctionSymbol,E.symbolSize s>=fromIntegral (B.length resetBoundary)] of
    [s] -> pure (E.symbolValue s)
    _ -> fail "owned timing::reset function missing or ambiguous for isolated capture receipt"
  forM_ selected $ \c -> do
    (ownedCode,ownedFunctions,names) <- ownedFor (rootRole (operation c))
    let (name,stockEntry)=root (operation c)
    ownedEntry <- maybe (fail ("missing owned export xm1_scanout_"++name)) pure (M.lookup ("xm1_scanout_"++name) names)
    ownedCase <- either fail pure (ownedScenario names c)
    stockCaseCode <- either fail pure (if needsResetReceipt c then patchCode 0x6c9160 resetBoundary stockCode else Right stockCode)
    ownedCaseCode <- either fail pure (if needsResetReceipt c then patchCode resetEntry resetBoundary ownedCode else Right ownedCode)
    let profile=if usesCalibration c then Resource8 else DefaultMemory
        (stockCall,stockCallCode,stockCallFunctions)=caller c stockEntry stockCaseCode functions
        (ownedCall,ownedCallCode,ownedCallFunctions)=caller ownedCase ownedEntry ownedCaseCode ownedFunctions
        stockSpec=makeSpec "stock" stockCall stockCallCode stockCallFunctions c
        ownedSpec=makeSpec "owned" ownedCall ownedCallCode ownedCallFunctions ownedCase
    ownedPath <- writeCaseWithProfile profile (out </> label c </> "owned") ownedSpec
    stockPaths <- if paired c then do
      path <- writeCaseWithProfile profile (out </> label c </> "stock") stockSpec
      pure [path]
      else pure []
    if action==StageCases then putStrLn (unwords (stockPaths++[ownedPath])) else do
      owned <- loadBoundedObserved runner ownedPath (reportFileLimit ownedSpec)
      check ownedCase owned
      stocks <- forM stockPaths $ \path -> do
        stock <- loadBoundedObserved runner path (reportFileLimit stockSpec)
        check c stock
        pure stock
      let normalize ident bytes
            | ident==RegionId "journal" = replacePointer 0x404 bytes
            | ident==RegionId "scheduler" && needsResetReceipt c && take 1 (arguments c)==[0] = replacePointer 0x2ec bytes
            | otherwise=bytes
          replacePointer offset bytes=
            let a=if offset==0x404 then 0x210404 else 0x917eec
            in B.take offset bytes<>word32le (getWord (expected c) a)<>B.drop (offset+4) bytes
          public normalizeOwned observation=sort [(n,if normalizeOwned then normalize n b else b) | (n,b) <- nativeMemory observation,n `elem` [RegionId publicRegion | (publicRegion,_,_) <- regionLayout c,publicRegion/="yield-receipt"]]
      forM_ stocks $ \stock -> unless (public False stock==public True owned) (fail (label c++": stock/owned public state differs"))
  putStrLn (if action==StageCases then "scanout native cases staged; no execution" else "scanout CPU cases satisfy independent effects; supported stock pairs compared; boundary cases owned-only")
