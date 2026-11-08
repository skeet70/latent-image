{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module PaneDrawerCases (main) where

import Control.Monad (forM, forM_, unless)
import Data.Aeson (object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.ByteString as B
import Data.Int (Int8)
import Data.List (nub)
import Data.Word (Word32)
import Emulation.NativeCase (runCase)
import Emulation.RenderCases
import qualified Emulation.Sha256 as Sha
import FujiAF.Host (ensure, get, str)
import FujiAF.JSON (encodeCanonical)
import Numeric (readHex)
import System.Directory (createDirectoryIfMissing, makeAbsolute)
import System.Environment (getArgs)
import System.FilePath ((</>), takeDirectory)
import Text.Read (readMaybe)

newtype Mode = Mode Word32 deriving (Eq, Show)
newtype Capacity = Capacity Word32 deriving (Eq, Show)
newtype Page = Page Word32 deriving (Eq, Show)
newtype Cursor = Cursor Int deriving (Eq, Show)
newtype Scroll = Scroll Word32 deriving (Eq, Show)
data RowState = Normal | Enabled | Disabled | Hidden deriving (Eq, Show)
data Hook = Default | Handled deriving (Eq, Show)
newtype ChildId = ChildId Word32 deriving (Eq, Show)
newtype Variant = Variant Word32 deriving (Eq, Show)
newtype ResourceId = ResourceId Word32 deriving (Eq, Show)
data Row = Row { childId :: ChildId, rowVariant :: Variant, state :: RowState, labelId :: ResourceId, valueId :: ResourceId, hook :: Hook, special :: Bool, suppress :: Bool } deriving Show
child rowValue = let ChildId n=childId rowValue in n
variant rowValue = let Variant n=rowVariant rowValue in n
label rowValue = let ResourceId n=labelId rowValue in n
value rowValue = let ResourceId n=valueId rowValue in n
data Scenario = Scenario { name :: String, mode :: Mode, depth :: Word32, kind :: Word32, capacity :: Capacity, page :: Page, cursor :: Cursor, scroll :: Scroll, roots :: [Row], nested :: [Row], shortTabs :: Bool, forcedTabs :: Bool, titleHidden :: Bool, pixels :: Bool, wide :: Bool } deriving Show
data Candidate = Candidate B.ByteString [(Word32,Int)] Int

digest = Sha.sha256Hex . Sha.sha256
put offset bytes old = B.take offset old <> bytes <> B.drop (offset+B.length bytes) old
word offset value = put offset (wordsLE [value])
byte offset value = put offset (B.singleton (fromIntegral value))
s8 :: Int -> Word32
s8 = fromIntegral . (fromIntegral :: Int -> Int8)
stateWord Normal=0
stateWord Enabled=1
stateWord Disabled=2
stateWord Hidden=3
hookWord Default=0
hookWord Handled=1
row i = Row (ChildId (0x200+fromIntegral i)) (Variant (fromIntegral (i `mod` 3))) (if i `mod` 3==1 then Disabled else Enabled) (ResourceId (0x400+fromIntegral i)) (ResourceId (if i `mod` 4==3 then 0 else 0x500+fromIntegral i)) (if odd i then Handled else Default) (i `mod` 3==0) (i `mod` 5==4)
base = Scenario "base" (Mode 0) 0 0 (Capacity 3) (Page 0) (Cursor 0) (Scroll 0) (map row [0..2]) [] False False False False False
scenarios =
  [ base { name="root-n"++show n++"-p"++show p++"-c"++show c++"-d"++show d++"-k"++show k++"-m"++show m, roots=map row [0..n-1], page=Page p, cursor=Cursor c, depth=d, kind=k, mode=Mode m }
  | n<-[0,1,3,7], p<-nub [0,fromIntegral (max 0 ((n-1) `div` 3))], c<-nub [0,n `div` 2,max 0 (n-1)], (d,k,m)<-[(0,0,0),(1,1,1),(2,2,0),(2,9,0),(1,3,0),(2,2,3),(2,2,6),(2,2,9),(2,2,12),(0,0,14),(1,1,255)] ] ++
  [ base { name="nested-n"++show n++"-s"++show top++"-c"++show c++"-m"++show m, depth=2, kind=2, mode=Mode m, nested=map row [0..n-1], cursor=Cursor c, scroll=Scroll top }
  | n<-[0,1,3,7], top<-[0,2], c<-nub [0,n `div` 2,max 0 (n-1)], m<-[0,1,4,8,10,12] ] ++
  [ base {name="class-d"++show d++"-k"++show k++"-m"++show m,depth=d,kind=k,mode=Mode m,nested=if d==2 then map row [0..2] else []}
  | d<-[0,1,2],k<-[0,1,2,3,9],m<-[0,1,3,4,6,8,9,10,12,14,255] ] ++
  [ base {name="matrix-property-"++show property++"-d"++show d,depth=d,kind=d,roots=[(row 0){childId=ChildId property}],nested=if d==2 then map row [0..3] else []}
  | property<-[15,26,36,211,239,260,271],d<-[0,2] ] ++
  [ base {name="hidden",roots=[row 0,(row 1){state=Hidden},row 2,(row 3){state=Hidden},row 4]}
  , base {name="all-hidden",roots=map (\i -> (row i){state=Hidden}) [0..3]}
  , base {name="normal-state",roots=[(row 0){state=Normal}]}
  , base {name="hooks-default",roots=[(row i){hook=Default} | i<-[0..2]]}
  , base {name="hooks-handled",roots=[(row i){hook=Handled} | i<-[0..2]]}
  , base {name="nested-hidden",depth=2,kind=2,nested=[(row 0){state=Hidden},row 1,row 2,(row 3){state=Hidden},row 4],scroll=Scroll 1}
  , base {name="zero-capacity",capacity=Capacity 0}
  , base {name="page-truncation",capacity=Capacity 128,page=Page 2}
  , base {name="short-tabs",shortTabs=True}
  , base {name="forced-tabs",forcedTabs=True}
  , base {name="title-suppressed",titleHidden=True}
  , base {name="pixel-clear",pixels=True}
  , base {name="wide-pixel-clear",pixels=True,wide=True}
  , base {name="preview-selected",roots=[(row 0){childId=ChildId 0x281,valueId=ResourceId 0},(row 1){childId=ChildId 0x285,valueId=ResourceId 0}]}
  , base {name="preview-unselected",cursor=Cursor 1,roots=[(row 0){childId=ChildId 0x281,valueId=ResourceId 0},(row 1){childId=ChildId 0x285,valueId=ResourceId 0}]}
  ]

side s = let Mode m=mode s; (a,b,c,d)=case m of 3->(2,1,1,1); 6->(2,1,1,2);12->(2,1,1,2);9->(2,2,1,0);_->case depth s of 0->(1,0,1,2);1->(0,0,0,2);_->if kind s==9 then (3,3,1,0) else (2,2,1,0)
             t=kind s `elem` [3,9]
         in [if t then 3 else a,b,c,d,if t then 1 else 0,m,0x11223344]
visible = filter ((/=Hidden) . state)
summary s = let Capacity cap=capacity s; n=length (visible (roots s)) in wordsLE [fromIntegral (length (roots s)),if cap==0 then 0 else (fromIntegral n+cap-1) `div` cap,cap]
selected s rs = let Cursor c=cursor s in if c<length rs then child (rs!!c) else 0xffffffff
report s = let Page p=page s; Scroll top=scroll s; Cursor c=cursor s; rs=visible (nested s); ordinal=if c<length (nested s) then lookup (child (nested s!!c)) (zip (map child (drop (fromIntegral top) rs)) [0..]) else Nothing
           in foldr (uncurry word) (byte 0x1c (fromIntegral c) $ byte 0x1f p $ byte 0x3c (maybe 255 id ordinal) $ byte 0x3d (if null rs then 255 else 0) $ byte 0x3e (if length rs<3 then fromIntegral ((3-length rs) `div` 2) else 0) $ B.replicate 68 0)
             [(0,depth s),(4,1),(12,selected s (roots s)),(0x20,0x224),(0x24,fromIntegral (length (nested s))),(0x28,fromIntegral (length rs)),(0x2c,3),(0x30,top),(0x34,selected s (nested s)),(0x38,case rs of []->0xffffffff;v:_->child v)]
getWord bytes offset = sum [fromIntegral (B.index bytes (offset+i))*256^i | i<-[0..3]]

plain target = call (ServiceAddress target)
font n = (plain 0x1112b2c [n] 0){serviceWrites=[AbsoluteMemory 0x15e46c0 0 (wordsLE [n])]}
grid x y resource = plain 0x111236c [x,y,resource] 0
palette args = (plain 0x1137cf8 args 0){serviceWrites=[AbsoluteMemory 0x15e6680 0 (wordsLE args)]}
gridPalette n = (plain 0x1137d2c [n] 0){serviceWrites=[AbsoluteMemory 0x15e6680 0 (wordsLE [n,0,0,0])]}
box x y right bottom stroke colour alpha = (plain 0x1113514 [x,y,right,bottom] 0){serviceReads=[MemoryArgument (Argument 13) 0 (wordsLE [stroke,colour,alpha])]}
aligned x y alignment resource = (plain 0x111278c [x,0,y,s8 (fromIntegral y+1)] 0){serviceReads=[MemoryArgument (Argument 13) 0 (wordsLE [alignment,2,resource])]}
hookCall target s r x y chosen resource valueHook = (plain target [] (hookWord (hook r))) {serviceReads=reads}
  where
    fields=wordsLE [1,if target==0x116f948 then 0x224 else child r]
    tailFields=wordsLE (take 5 (side s)++[x,y])
    reads=[MemoryArgument (Argument 0) 0 fields,MemoryArgument (Argument 0) 12 tailFields,MemoryArgument (Argument 0) 44 (wordsLE [chosen,resource])] ++ [MemoryArgument (Argument 0) 8 (wordsLE [variant r]) | valueHook && target/=0x116f948] ++ [MemoryArgument (Argument 0) 8 (wordsLE [child r]) | target==0x116f948] ++ [MemoryArgument (Argument 0) 40 (wordsLE [if x==9 then 1 else 0]) | target/=0x116f948]

targets = map ServiceAddress [0x1165aa0,0x1112b2c,0xbf8d20,0xbf9360,0x1163e04,0x116cf50,0x1112da4,0x1113514,0x116dce0,0x1112ce4,0x111278c,0x11121d4,0x1112d84,0xbf93e0,0x1137cf8,0x1137d2c,0x1112e38,0x116e264,0x1112e58,0xbf90f0,0x1163c1c,0x116f948,0x111236c,0x1177e10,0x116d4e4,0x1159168,0x1112d64,0x11130e0,0x113f1e8,0x1130e68,0x1138a00,0x112eb38]

expectedCalls image s = prepare : rootCalls ++ nestedCalls ++ chrome
  where
    d=side s; a=d!!0;b=d!!1;c=d!!2;pane=d!!3; Mode m=mode s
    r=report s; rw=getWord r; rb offset=fromIntegral (B.index r offset)
    Capacity cap=capacity s; Page p=page s; Scroll top=scroll s
    prepare=(plain 0x1165aa0 [1,m] 0){serviceWrites=[AbsoluteMemory 0x15e64d8 0 (wordsLE d<>summary s<>B.replicate 12 0<>r)]}
    rootCalls=if b==3 then [] else [font (if depth s==1 then 13 else 15) | depth s `elem` [0,1,2]] ++ loopRoot 0 (fromIntegral ((cap*p) `mod` 256)) 0 (roots s)
    loopRoot _ _ _ []=[]
    loopRoot index skip drawn (v:rest)
      | fromIntegral drawn>=cap=[]
      | otherwise=describe : if state v==Hidden then next skip drawn else if skip>0 then next (skip-1) drawn else drawRoot index drawn v ++ next 0 (drawn+1)
      where
        describe=outCall (ServiceAddress 0xbf8d20) [(Argument 0,1),(Argument 1,fromIntegral index)] (Argument 2) (wordsLE [0x600+fromIntegral index,child v,0x300,variant v,stateWord (state v),if suppress v then 1 else 0])
        next skip' drawn'=loopRoot (index+1) skip' drawn' rest
    drawRoot index drawn v =
      [plain 0xbf9360 [1,fromIntegral index,if alternate then 1 else 0] (label v),plain 0x1163e04 [chosen,stateWord (state v)] 0,plain 0x116cf50 [1,child v] (if special v then 1 else 0)] ++
      labelCalls ++ valueCalls
      where
        y=s8 (4+2*drawn);chosen=if child v==rw 12 then 1 else 0;alternate=b==2 && chosen==0
        labelCalls=(if m==12 && chosen==1 then [plain 0x1112da4 [] 0,box 5 y 32 (s8 (fromIntegral y+1)) 17 56 153] else []) ++
          [hookCall 0x116dce0 s v 6 y chosen (label v) False] ++
          (if hook v==Handled then [] else if alternate then [plain 0x1112ce4 [8] 0 | special v]++[aligned 6 y 0 (label v)] else [plain 0x11121d4 [6,y,label v,1] 0]) ++
          [plain 0x1112d84 [] 0 | m==12 && chosen==1]
        x=if alternate then 9 else fromIntegral (-20::Int);alignment=if alternate then 1 else 0
        brackets=special v && (a==0 || (a==3 && b==0))
        valueCalls=if b==1 || (chosen==1 && a==2) then [] else
          [plain 0xbf93e0 [1,child v,variant v,0] (value v)] ++
          (if c==0 then if chosen==1 then [palette [120,255,0,0]] else if state v==Disabled then [palette [107,255,0,0]] else [gridPalette 104]
           else if a `elem` [2,3] then [palette (if chosen==1 then [121,255,0,0] else if state v==Disabled then [107,76,0,0] else [118,255,0,0])]
           else [palette (if state v==Disabled then [122,255,0,0] else [118,255,0,0])]) ++
          (if brackets then [plain 0x1112e38 [] 0,gridPalette 113] else [plain 0x1112ce4 [8] 0 | special v]) ++
          [hookCall 0x116e264 s v x y chosen (value v) True] ++
          [aligned x y alignment (value v) | not (suppress v) && value v/=0 && hook v==Default] ++ [plain 0x1112e58 [] 0 | brackets]
    nestedCalls=if pane==2 || m==3 then [] else font 11 : loopNested 0 (fromIntegral (top `mod` 256)) 0 (nested s)
    loopNested _ _ _ []=[]
    loopNested index skip drawn (v:rest)
      | drawn>=3=[]
      | otherwise=describe : if state v==Hidden then next skip drawn else if skip>0 then next (skip-1) drawn else drawNested drawn v ++ next 0 (drawn+1)
      where
        describe=outCall (ServiceAddress 0xbf90f0) [(Argument 0,1),(Argument 1,0x224),(Argument 2,fromIntegral index)] (Argument 3) (wordsLE [child v,stateWord (state v),0x700+fromIntegral index])
        next skip' drawn'=loopNested (index+1) skip' drawn' rest
    drawNested drawn v = [plain 0xbf93e0 [1,0x224,child v,1] (value v),plain 0x1163c1c [chosen,stateWord (state v)] 0,hookCall 0x116f948 s v 13 y chosen (value v) True] ++
      [grid 14 y (value v) | hook v==Default] ++ [plain target [1,0x224,rw 0x34] 0 | chosen==1,target<-[0x116d4e4,0x1159168]]
      where chosen=if child v==rw 0x34 then 1 else 0;y=s8 (2*(rb 0x3e+drawn)+3)
    rowsCalls=rootCalls++nestedCalls
    fontBefore=foldl fontAfter 9 rowsCalls
    tabBank=if a==0 then 1 else if a==2 then 2 else 0
    tab address stride index choice=getWord image (fromIntegral (address-0x1100000)+4*(stride*tabBank+index*3+choice))
    tabCount=if shortTabs s then 1 else 5
    tabCalls=[font 15 | a/=3] ++ [grid 1 (fromIntegral (3+2*i)) (tab (if shortTabs s then 0x116cd34 else 0x116ce9c) (if shortTabs s then 6 else 15) i (if fromIntegral i==p then 0 else 1)) | a/=3,i<-[0..tabCount-1]] ++
      [grid 1 (fromIntegral (3+2*(tabCount+i))) (tab 0x116cde8 15 i (if forcedTabs s then 2 else 1)) | a/=3,i<-[0..2]]
    chrome=clear ++ title ++ tabCalls ++ arrows ++ marker ++ nestedArrows ++ [plain 0x112eb38 [0x78c,1,19] 0 | a `elem` [0,1] || (a==3 && b==0)]
    clear=if a/=1 then [] else if pixels s then
      [plain 0x1112d64 [x,y,960,540] 0,(plain 0x11130e0 [x,y,right,bottom] 0){serviceReads=[MemoryArgument (Argument 13) 0 (wordsLE [18,0,255])]},plain 0x1112d64 [0,0,0,0] 0]
      else [box 1 1 32 20 18 0 255]
      where (x,y,right,bottom)=if wide s then (fromIntegral (-160::Int),fromIntegral (-30::Int),800,510) else (0,0,960,540)
    title=if titleHidden s then [] else
      (if d!!4==0 then [font 18,box 1 1 32 20 18 0 255] else []) ++
      [font 15,palette [109,255,0,0],grid 9 1 0x27a,font 15,gridPalette 6,plain 0x113f1e8 [] 0x888,grid 5 1 0x888,font fontBefore]
    arrows=rootArrows ++ (if pane==2 then [] else
      [plain 0x1112d84 [] 0 | m==3] ++ [font 14,grid 12 2 0xff0000e8] ++ [plain 0x1112da4 [] 0 | m==3] ++
      (if m `elem` [1,3] || b==3 then [] else [font 13,gridPalette 116,grid 12 9 0xff00035f]))
    rootArrows=if b==3 then [] else [font 17,grid 4 3 (if c==0 then 0xff0000e3 else if a `elem` [2,3] then 0xff0000e7 else 0xff0000e2)] ++
      (if getWord (summary s) 4>1 && c==0 then [font 16,palette [116,255,0,0],grid 18 18 0xff000617,grid 18 3 0xff000660] else [])
    savedMarker=foldl fontAfter fontBefore (tabCalls++arrows)
    marker=if a==1 then [] else rootMarker ++ nestedMarker ++ [font savedMarker]
    rootY=s8 (2*(rb 0x1c+2))
    style nestedMarkerFlag = plain 0x1130e68 [0] 0 : [grid (if nestedMarkerFlag then 13 else 5) (if nestedMarkerFlag then currentY else rootY) (getWord image (0xf528+if nestedMarkerFlag then 4 else 0))]
    rootMarker=if pane==2 && b==0 then [font 16]++style False++[font 13,palette [117,255,0,0]]++[grid 5 rootY 0xff00062f | a/=3]++[grid 32 rootY 0xff00062e]
      else if b `elem` [1,2] || (pane/=2 && b/=3) then [font 16,grid 5 rootY 0xff0000f3] else []
    currentY=s8 (2*(rb 0x3c+rb 0x3e)+3)
    rememberedY=s8 (2*(rb 0x3d+rb 0x3e)+3)
    nestedMarker=if pane==2 || m==3 then [] else [font 13] ++ (if rb 0x3c==255 then [] else style True) ++
      [plain 0x1138a00 [8] 0x55] ++
      [box 13 rememberedY 13 (s8 (fromIntegral rememberedY+1)) 11 (if currentY==rememberedY then 7 else 0x55) 255 | rb 0x3d/=255]
    nestedArrows=if pane/=2 && rw 0x28>rw 0x2c then [font 13,palette [116,255,0,0],grid 23 2 0xff000660,grid 23 17 0xff000617] else []

fontAfter old service = case (serviceAddress service,serviceInputs service) of
  (ServiceAddress 0x1112b2c,(_,n):_) -> n
  _ -> old

loadCandidate binary symbols frameText = do
  bytes<-B.readFile binary
  frame<-case readMaybe frameText of Just n | n>0 && n<=4096 -> pure n; _ -> fail "frame bytes outside 1..4096"
  listing<-readFile symbols
  spans<-fmap (nub . concat) $ forM (filter (not . null) (lines listing)) $ \line -> case words line of
    [address,size,tag,_] | tag `elem` ["t","T","w","W"] -> do
      start<-hex address; count<-hex size
      ensure (start>=0x1800000 && count>0 && start+count<=0x1800000+toInteger (B.length bytes) && start `mod` 4==0 && count `mod` 4==0) "invalid A32 candidate span"
      pure [(fromInteger start,fromInteger count)]
    [address,size,[tag],_] | tag `elem` ("aAbBcCdDgGiInNpPrRsSuUvV"::String) -> do
      _<-hex address
      _<-hex size
      pure []
    _ -> fail ("unsupported nm -S row: "++line)
  ensure (any (\line -> case words line of [address,_,tag,"owned_pane_drawer"] -> tag `elem` ["t","T","w","W"] && readHex address==[(0x1800000::Integer,"")];_->False) (lines listing)) "owned_pane_drawer entry absent at 0x01800000"
  pure (Candidate bytes spans frame)
  where hex text=case readHex text of [(n,"")]->pure n;_->fail "invalid nm hexadecimal field"

emit output image image5 candidate s = do
  let slice address count=B.take count (B.drop (fromIntegral (address-0x1100000)) image)
      leaf5=B.take 156 (B.drop (0xbf9614-0xbf1a00) image5)
      leaves=[(0x1135db0,76),(0xbf9614,40),(0xbf963c,36),(0xbf9660,40),(0xbf9688,40)]
      (entry,functions,frame)=case candidate of
        Nothing -> (0x11662a0,[(0x11662a0,4940)]++leaves,172)
        Just (Candidate _ spans bytes) -> (0x1800000,spans++leaves,bytes)
      settings=word 0xbb6c 4 $ byte 0xcf00 (if forcedTabs s then 255 else 0) $ word 0xcf44 0 $ word 0x10080 (if titleHidden s then 1 else 0) $ B.replicate 0x10084 0
      menu=word 0x18 (if pixels s then 2 else 0) $ word 0x3f3c (if wide s then 2 else 0) $ B.replicate 0x3f40 0
      regions=[("pane",0x11662a0,"rx",slice 0x11662a0 4940),("title-leaf",0x1135db0,"rx",slice 0x1135db0 76),
        ("getter-leaves",0xbf9614,"rx",leaf5),("tabs",0x116cd34,"r",slice 0x116cd34 540),("markers",0x110f528,"r",slice 0x110f528 16),
        ("return",0x1158184,"rx",slice 0x1158184 4),("stack",0x2000000,"rw",B.replicate 0x2000 0),
        ("pane-state",0x15e64d8,"rw",B.replicate 120 0xa5),("font",0x15e46c0,"rw",word 0 9 $ byte 0x47f 1 $ B.replicate 0x480 0),
        ("menu",0x956754,"rw",menu),("palette",0x15e6680,"rw",B.replicate 64 0),
        ("settings-pointer",0x940de4,"r",wordsLE [0x2200000]),("tab-pointer",0x93ade4,"r",wordsLE [0x2211000]),
        ("settings",0x2200000,"rw",settings),("tab-kind",0x2211000,"rw",wordsLE [if shortTabs s then 0x2c else if forcedTabs s then 0x1f else 0])] ++
        case candidate of Nothing->[];Just (Candidate bytes _ _)->[("candidate",0x1800000,"rx",bytes)]
      calls=expectedCalls image s
      finalMemory address initial=foldl (\old c -> foldl (\bytes mem -> case mem of AbsoluteMemory at offset contents | at==address -> put offset contents bytes;_ -> bytes) old (serviceWrites c)) initial calls
      memory name bytes=object ["region".=(name::String),"offset".=(0::Int),"hex".=hexBytes bytes]
      regs=[("r0",1),("r1",let Mode m=mode s in m),("r2",0x22222222),("r3",0x33333333),("r4",0x44444444),("r5",0x55555555),("r6",0x66666666),("r7",0x77777777),("r8",0x88888888),("r9",0x99999999),("r10",0xaaaaaaaa),("r11",0xbbbbbbbb),("r12",0xcccccccc),("sp",0x2001000),("lr",0x1158184),("cpsr",0x10)]
      registers values=object [K.fromString key .= addressText value | (key,value)<-values]
  artifacts<-forM regions $ \(ident,_,_,bytes)->do
    let filename=name s++"-"++ident++".bin"
    B.writeFile (output </> filename) bytes
    pure (object ["id".=ident,"path".=filename,"size".=B.length bytes,"sha256".=digest bytes])
  let regionJSON (ident,address,permissions,bytes)=object ["id".=ident,"address".=addressText address,"size".=B.length bytes,"permissions".=(permissions::String),"initial".=object ["artifact".=ident,"offset".=(0::Int),"length".=B.length bytes]]
      document=object ["schema_version".=(1::Int),"id".=name s,"origin".=("stock"::String),
        "provenance".=("Oracle derived from root-pane.md and inspected instruction transfers before stock execution. Preparation publishes synthetic D/N/R; five pinned call-free getters execute. Bank preview rows script zero from both current-value paths."::String),
        "cpu".=object ["architecture".=("arm"::String),"model".=("cortex-r5f"::String),"mode".=("arm"::String),"endian".=("little"::String)],
        "artifacts".=artifacts,"entry".=addressText entry,"exit".=addressText 0x1158184,"registers".=registers regs,"regions".=map regionJSON regions,
        "functions".=[object ["address".=addressText address,"size".=(size::Int)] | (address,size)<-functions],
        "stubs".=callsJSON targets calls,"limits".=object ["instructions".=(40000::Int),"wall_ms".=(5000::Int)],
        "expected".=object ["status".=("returned"::String),"registers".=registers (("pc",0x1158184):filter (\(key,_)->key `elem` ["r4","r5","r6","r7","r8","r9","r10","r11","sp"]) regs),
          "ignored_memory".=[object ["region".=("stack"::String),"offset".=(0x1000-frame::Int),"length".=frame]],
          "memory".=[memory ident (finalMemory address bytes) | (ident,address,_,bytes)<-regions,ident `elem` ["pane-state","font","palette"]],
          "events".=eventsJSON calls],
        "exclusions".=(["Preparation and 31 other callees are scripted. Five stock leaves execute; image slices are pinned inputs.","Private frame and unspecified hook words are excluded; all remaining mapped memory and preserved ABI registers are exact.","Host A32 behavior only. No guest scheduling, device or timing evidence."]::[String])]
      path=output </> name s++".json"
  B.writeFile path (encodeCanonical document)
  pure path

main :: IO ()
main=do
  args<-getArgs
  let (options,runner)=case reverse args of binary:"--runner":rest -> (reverse rest,Just binary);_->(args,Nothing)
  (image,destination,candidate)<-case options of
    ["--image",image,"--output",destination]->pure (image,destination,Nothing)
    ["--image",image,"--output",destination,"--candidate",binary,"--symbols",symbols,"--frame-bytes",frame]->do
      code<-loadCandidate binary symbols frame
      pure (image,destination,Just code)
    _->fail "usage: pane-drawer-cases --image IMAGE6 --output DIRECTORY [--candidate IMAGE --symbols NM-SYMBOLS --frame-bytes N] [--runner NATIVE-RUNNER]"
  bytes<-B.readFile image
  ensure (digest bytes=="ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d") "image6 SHA256 differs"
  ensure (digest (B.take 4940 (B.drop (0x11662a0-0x1100000) bytes))=="f2358629ce63cc2162be94cfcce4f5e2eb5ac1e683a51ed433b2c13b2a48ad75") "pane SHA256 differs"
  image5<-B.readFile (takeDirectory (takeDirectory image) </> "lz" </> "id5-00bf1a00.bin")
  ensure (digest image5=="3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a") "image5 SHA256 differs"
  output<-makeAbsolute destination
  createDirectoryIfMissing True output
  paths<-mapM (emit output bytes image5 candidate) scenarios
  forM_ runner $ \binary->do
    results<-forM paths $ \path->do
      (_,result)<-runCase binary path
      B.writeFile (path++".report") (encodeCanonical result)
      let passed=str (get result "status")=="passed"
      unless passed (putStrLn (path++": "++str (get result "status")))
      pure passed
    ensure (and results) "pane-drawer qualification failed"
  putStrLn (show (length paths)++" pane-drawer cases")
