{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module TreeListCases (main, scenarios, scenarioName, expectedReport, expectedCalls, NodeId (..), DrawMode (..), node, mode, snapshot, callerFrame, reportAddress, returnAddress) where

import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.ByteString as B
import Data.Bits (testBit)
import Data.Int (Int8, Int16)
import Data.List (nub)
import Data.Word (Word32)
import Emulation.NativeCase (runCase)
import Emulation.RenderCases
import qualified Emulation.Sha256 as Sha
import FujiAF.Host (ensure, get, str)
import FujiAF.JSON (encodeCanonical)
import System.Directory (createDirectoryIfMissing, makeAbsolute)
import System.Environment (getArgs)
import System.FilePath ((</>), takeDirectory)
import Numeric (readHex)
import Text.Read (readMaybe)

newtype FrameBytes = FrameBytes Int
data CandidateCode = CandidateCode B.ByteString [(Word32,Int)] FrameBytes
newtype TableImage = TableImage B.ByteString

digest :: B.ByteString -> String
digest = Sha.sha256Hex . Sha.sha256

tableRegions :: TableImage -> Scenario -> [(String,Word32,String,B.ByteString)]
tableRegions (TableImage image) s =
  [("node-registry",root,"r",slice root 512),("node-record",recordAddress,"r",slice recordAddress 40)] ++
  [("child-table",children,"r",slice children 256) | children/=0]
  where
    root=0x81babc
    NodeId n=node s
    wordAt address = sum [fromIntegral (B.index (slice address 4) i)*256^i | i<-[0..3]]
    recordAddress=wordAt (root+4*n)
    children=wordAt (recordAddress+0x20)
    slice address count
      | address<0x61c400 || toInteger address+toInteger count>0x61c400+toInteger (B.length image) = error "menu table slice outside pinned image 4"
      | otherwise = B.take count (B.drop (fromIntegral (address-0x61c400)) image)

loadCandidate :: FilePath -> FilePath -> String -> IO CandidateCode
loadCandidate binary symbols frameText = do
  bytes <- B.readFile binary
  FrameBytes frame <- case readMaybe frameText of
    Just n | n>0 && n<=4096 -> pure (FrameBytes n)
    _ -> fail "--frame-bytes must be in 1..4096"
  listing <- readFile symbols
  spans <- fmap nub $ forM (filter (not . null) (lines listing)) $ \line -> case words line of
    [address,size,kind,_] | kind `elem` ["t","T","w","W"] -> do
      start <- hexNumber address
      count <- hexNumber size
      ensure (start>=0x01800000 && count>0 && start+count<=0x01800000+toInteger (B.length bytes) && start `mod` 4==0 && count `mod` 4==0) "candidate function span outside A32 image"
      pure (fromInteger start,fromInteger count)
    _ -> fail ("unsupported nm -S function row: "++line)
  ensure (any ((==0x01800000) . fst) spans) "candidate entry absent from function symbols"
  pure (CandidateCode bytes spans (FrameBytes frame))
  where
    hexNumber text = case readHex text of [(n,"")] -> pure n; _ -> fail "invalid hexadecimal nm field"

newtype NodeId = NodeId Word32 deriving (Eq, Show)
newtype ChildId = ChildId Word32 deriving (Eq, Show)
newtype SelectionId = SelectionId Word32 deriving (Eq, Show)
newtype ResourceId = ResourceId Word32 deriving (Eq, Show)
newtype Variant = Variant Word32 deriving (Eq, Show)
newtype PageCapacity = PageCapacity Word32 deriving (Eq, Show)
newtype Page = Page Word32 deriving (Eq, Show)
newtype Cursor = Cursor Word32 deriving (Eq, Show)
newtype ScrollTop = ScrollTop Word32 deriving (Eq, Show)
newtype DrawMode = DrawMode Word32 deriving (Eq, Show)
data RowState = Offered | Disabled | Hidden deriving (Eq, Show)
data Hook = DefaultLeaf | Handled deriving (Eq, Show)
data Row = Row ChildId SelectionId Variant RowState ResourceId ResourceId Hook Hook Bool deriving (Eq, Show)
data Grid = Grid Int Int Int Int deriving (Eq, Show)
data Caller = MenuList | SecondaryTree deriving (Eq, Show)
data SnapshotKind = Flat | Tree | Unknown | Forced25 | Forced38 deriving (Eq, Show)
data NestedRow = NestedRow SelectionId RowState ResourceId ResourceId Hook deriving (Eq, Show)
data Resolver = ChildMatch | Resolved ChildId SelectionId Cursor deriving (Eq, Show)
data Scenario = Scenario
  { scenarioName :: String, node :: NodeId, capacity :: PageCapacity, page :: Page
  , cursor :: Cursor, scroll :: ScrollTop, mode :: DrawMode, grid :: Grid, rows :: [Row]
  , caller :: Caller, snapshotKind :: SnapshotKind, nestedGrid :: Grid, nestedRows :: [NestedRow]
  , nestedCapacity :: PageCapacity, resolver :: Resolver, sideMode :: DrawMode
  } deriving (Eq, Show)

stateWord :: RowState -> Word32
stateWord Offered = 1
stateWord Disabled = 2
stateWord Hidden = 3
hookWord :: Hook -> Word32
hookWord DefaultLeaf = 0
hookWord Handled = 1

row :: Int -> Row
row i = Row (ChildId (0x200 + fromIntegral i)) (SelectionId (0x300 + fromIntegral i))
  (Variant (if odd i then 7 else 0)) (if i `mod` 3 == 1 then Disabled else Offered)
  (ResourceId (0x400 + fromIntegral i)) (ResourceId (if i `mod` 3 == 2 then 0 else 0x500 + fromIntegral i))
  (if i `mod` 2 == 0 then DefaultLeaf else Handled)
  (if i `mod` 3 == 1 then Handled else DefaultLeaf) (odd i)

scenarios :: [Scenario]
scenarios =
  [ Scenario ("flat-n" ++ show count ++ "-p" ++ show p ++ "-c" ++ show selected ++ "-s" ++ show top ++ "-m" ++ show m)
      (NodeId 0x40) (PageCapacity 3) (Page p) (Cursor selected) (ScrollTop top) (DrawMode m)
      (Grid 2 3 0 2) (map row [0..count-1]) MenuList Flat (Grid 14 4 0 2) [] (PageCapacity 3) ChildMatch (DrawMode 0)
  | count <- [1,3,8], p <- [1..fromIntegral ((count+2) `div` 3)]
  , selected <- nub [0,fromIntegral (count `div` 2),fromIntegral (count-1)]
  , top <- [0,5], m <- [0,8]
  ] ++
  [ base {scenarioName="flat-hidden", rows=[row 0, hidden (row 1), row 2, hidden (row 3), row 4], cursor=Cursor 2}
  , base {scenarioName="flat-empty", rows=[]}
  , base {scenarioName="flat-zero-capacity", capacity=PageCapacity 0, rows=[row 0,row 1], page=Page 2, cursor=Cursor 1}
  , base {scenarioName="flat-no-centering-53", node=NodeId 0x53, rows=[row 0]}
  , base {scenarioName="flat-no-centering-57", node=NodeId 0x57, rows=[row 0]}
  , base {scenarioName="flat-grid-wrap", grid=Grid 32767 65535 2 3}
  , base {scenarioName="flat-all-hidden", rows=map (hidden . row) [0..3]}
  , base {scenarioName="flat-live-mode-14", mode=DrawMode 14}
  , base {scenarioName="flat-live-mode-255", mode=DrawMode 255}
  , base {scenarioName="flat-hooks-default", rows=[hooks DefaultLeaf (row i) | i<-[0..2]]}
  , base {scenarioName="flat-hooks-handled", rows=[hooks Handled (row i) | i<-[0..2]]}
  ] ++
  [ base { scenarioName="source-"++show who++"-"++show kind++"-m"++show m, caller=who, snapshotKind=kind, mode=DrawMode m
         , nestedRows=map nestedRow [0..4], scroll=ScrollTop 1
         , grid=if who==SecondaryTree then Grid 6 4 0 2 else grid base
         , nestedGrid=if who==SecondaryTree then Grid 14 3 0 2 else nestedGrid base }
  | who<-[MenuList,SecondaryTree], kind<-[Flat,Tree], m<-[0..14]++[255]
  ] ++
  [ tree {scenarioName="tree-hidden-scroll", nestedRows=[nestedRow 0,hiddenNested (nestedRow 1),nestedRow 2,nestedRow 3,nestedRow 4],scroll=ScrollTop 1}
  , tree {scenarioName="tree-empty",rows=[],nestedRows=[]}
  , tree {scenarioName="tree-zero-capacity",capacity=PageCapacity 0,nestedCapacity=PageCapacity 0}
  , tree {scenarioName="tree-all-hidden",rows=map (hidden . row) [0..3],nestedRows=map (hiddenNested . nestedRow) [0..3]}
  , tree {scenarioName="tree-resolver-overrides",resolver=Resolved (ChildId 0x9876) (SelectionId 0x7654) (Cursor 2)}
  , tree {scenarioName="tree-nested-centering",nestedCapacity=PageCapacity 8,nestedRows=[nestedRow 0,nestedRow 1,nestedRow 2]}
  , tree {scenarioName="tree-grid-wrap",nestedGrid=Grid 65535 32767 3 5}
  , tree {scenarioName="tree-side-format",sideMode=DrawMode 13}
  , tree {scenarioName="tree-side-suppresses-detail",sideMode=DrawMode 2}
  , tree {scenarioName="tree-mode7-default",mode=DrawMode 7,nestedRows=[NestedRow (SelectionId 0x702) Offered (ResourceId 0x802) (ResourceId 0x902) DefaultLeaf]}
  , tree {scenarioName="tree-mode13-handled",mode=DrawMode 13,nestedRows=[NestedRow (SelectionId 0x702) Offered (ResourceId 0x802) (ResourceId 0x902) Handled]}
  , base {scenarioName="unknown-kind",snapshotKind=Unknown}
  , base {scenarioName="forced-flat-25",node=NodeId 0x25,snapshotKind=Forced25}
  , base {scenarioName="forced-flat-38",node=NodeId 0x38,snapshotKind=Forced38,sideMode=DrawMode 13}
  ]
  where
    base = Scenario "flat" (NodeId 0x40) (PageCapacity 3) (Page 1) (Cursor 1) (ScrollTop 0) (DrawMode 0) (Grid 2 3 0 2) (map row [0..2]) MenuList Flat (Grid 14 4 0 2) [] (PageCapacity 3) ChildMatch (DrawMode 0)
    tree=base {snapshotKind=Tree,nestedRows=map nestedRow [0..4]}
    nestedRow i=NestedRow (SelectionId (0x700+fromIntegral i)) (if odd i then Disabled else Offered) (ResourceId (0x800+fromIntegral i)) (ResourceId (0x900+fromIntegral i)) (if odd i then Handled else DefaultLeaf)
    hiddenNested (NestedRow a _ b c d)=NestedRow a Hidden b c d
    hidden (Row a b c _ d e f g h) = Row a b c Hidden d e f g h
    hooks h (Row a b c d e f _ _ i) = Row a b c d e f h h i

reportAddress :: Scenario -> Word32
reportAddress s=case caller s of MenuList->0x02100000; SecondaryTree->entrySP+36
returnAddress :: Scenario -> Word32
returnAddress s=case caller s of MenuList->0x01167a60; SecondaryTree->0x0116c058
stackAddress, entrySP, fontAddress, sideAddress :: Word32
stackAddress=0x02000000
entrySP=stackAddress+0x1000
fontAddress=0x015e46c0
sideAddress=0x015e64d8

put :: Int -> B.ByteString -> B.ByteString -> B.ByteString
put offset bytes old = B.take offset old <> bytes <> B.drop (offset+B.length bytes) old
word :: Int -> Word32 -> B.ByteString -> B.ByteString
word offset value = put offset (wordsLE [value])
byte :: Int -> Word32 -> B.ByteString -> B.ByteString
byte offset value = put offset (B.singleton (fromIntegral value))

visibleCount :: Scenario -> Word32
visibleCount = fromIntegral . length . filter (\(Row _ _ _ state _ _ _ _ _) -> state /= Hidden) . rows
pageSize :: Scenario -> Word32
pageSize s = let PageCapacity p=capacity s in max 1 p
center :: Scenario -> Int
center s = let NodeId n=node s; v=if effectiveKind s==9 then 0 else visibleCount s; p=pageSize s
           in if n `elem` [0x53,0x57] || v>=p then 0 else fromIntegral (((p-v) `div` 2) `mod` 256)
wrap16 :: Int -> Int
wrap16 n = n `mod` 65536
signed16 :: Int -> Word32
signed16 = fromIntegral . (fromIntegral :: Int -> Int16)
signed8 :: Int -> Word32
signed8 = fromIntegral . (fromIntegral :: Int -> Int8)

snapshot :: Scenario -> [Word32]
snapshot s = let Cursor a=cursor s; Page q=page s; ScrollTop top=scroll s
                 nestedCount=fromIntegral (length [() | NestedRow _ state _ _ _<-nestedRows s,state/=Hidden])
             in case snapshotKind s of
               Flat->[1,0x11223344,0,0x224,9,0x302,0,a,visibleCount s,q,top]
               Tree->[2,0x11223344,0,0x201,0x702,0x701,0,a,nestedCount,q,top]
               Unknown->[9,0x11223344,0,0x224,9,0x302,0,a,visibleCount s,q,top]
               Forced25->[2,0x11223344,0,0x27,0,0x302,0,a,visibleCount s,q,top]
               Forced38->[2,0x11223344,0,0x228,11,0x302,0,a,visibleCount s,q,top]
effectiveKind :: Scenario -> Word32
effectiveKind s=case snapshotKind s of Tree->2; Unknown->9; _->1
selectedIndex :: Scenario -> Word32
selectedIndex s
  | effectiveKind s==1=let Cursor index=cursor s in index
  | effectiveKind s/=2=0
  | otherwise=case resolver s of
      Resolved _ _ (Cursor index) | visibleCount s/=0->index
      _->case [fromIntegral i | (i,Row (ChildId child) _ _ _ _ _ _ _ _)<-zip [0::Int ..] (filter offered (rows s)),child==snapshot s!!3] of
        []->0
        indices->last indices
  where offered (Row _ _ _ state _ _ _ _ _)=state/=Hidden
currentPage :: Scenario -> Word32
currentPage s=case effectiveKind s of
  1->let Page q=page s in q
  2->selectedIndex s `div` pageSize s+1
  _->0
nestedCenter :: Scenario -> Int
nestedCenter s=let PageCapacity cap=nestedCapacity s; count=snapshot s!!8
               in if count<cap then fromIntegral (((cap-count) `div` 2) `mod` 256) else 0
drawnNested :: Scenario -> [(Int,Word32,Int,NestedRow)]
drawnNested s
  | effectiveKind s/=2 || mode s `elem` [DrawMode 2,DrawMode 3]=[]
  | otherwise=take (fromIntegral cap) $ zipWith ordinal [0..] $ drop (fromIntegral top) visible
  where
    PageCapacity cap=nestedCapacity s; ScrollTop top=scroll s
    visible=[(raw,fromIntegral index,r) | (index,(raw,r))<-zip [0::Int ..] [(raw,r) | (raw,r@(NestedRow _ state _ _ _))<-zip [0..] (nestedRows s),state/=Hidden]]
    ordinal ord (raw,index,r)=(raw,index-top,ord,r)

-- Earlier pages and hidden children leave drawn coordinates unchanged.
drawnRows :: Scenario -> [(Int,Word32,Int,Row)]
drawnRows s = go 0 0 0 (rows s)
  where
    q=currentPage s; p=pageSize s
    go _ _ _ []=[]
    go raw k ordinal (r@(Row _ _ _ state _ _ _ _ _):rest)
      | state==Hidden = go (raw+1) k ordinal rest
      | q>rowPage = go (raw+1) ((k+1) `mod` 256) ordinal rest
      | q<rowPage = []
      | otherwise = (raw,k,ordinal,r):go (raw+1) ((k+1) `mod` 256) (ordinal+1) rest
      where rowPage=(k+p) `div` p

expectedReport :: Scenario -> B.ByteString
expectedReport s = foldl nestedRecord nestedInitial (drawnNested s)
  where
    snap=snapshot s; q=currentPage s; selected=selectedIndex s
    common=foldr (uncurry word) (byte 0x1d 255 $ byte 0x3d 255 $ byte 0x1e (fromIntegral (center s)) $ B.replicate 68 0)
      [(0,effectiveKind s),(4,snap!!1),(0x20,snap!!3),(0x30,snap!!10),(0x38,snap!!5)]
    initial=case effectiveKind s of
      1->foldr (uncurry word) (byte 0x1f q common) [(8,snap!!3),(0x10,snap!!4),(0x14,snap!!8 `mod` 256),(0x18,pageSize s)]
      2->foldr (uncurry word) (byte 0x1f q common) prepassReport
      _->common
    prepassReport=case resolver s of
      Resolved (ChildId parent) (SelectionId selection) _ | visibleCount s/=0->[(8,parent),(0x10,selection)]
      _->case [(8,0x600+fromIntegral raw,selection) | (raw,Row (ChildId child) (SelectionId selection) _ state _ _ _ _ _)<-zip [0::Int ..] (rows s),state/=Hidden,child==snap!!3] of
        []->[]
        matches->let (_,parent,selection)=last matches in [(8,parent),(0x10,selection)]
    topReport=foldl record initial (drawnRows s)
    nestedInitial
      | effectiveKind s==2 && mode s `notElem` [DrawMode 2,DrawMode 3]=
        let PageCapacity cap=nestedCapacity s in foldr (uncurry word) (byte 0x3e (fromIntegral (nestedCenter s)) topReport)
          [(0x20,snap!!3),(0x24,fromIntegral (length (nestedRows s))),(0x28,snap!!8),(0x2c,cap),(0x30,snap!!10),(0x34,0),(0x38,snap!!5)]
      | otherwise=topReport
    nestedRecord old (_,index,_,NestedRow (SelectionId selection) _ _ (ResourceId dat) _)=
      let previous=if selection==snap!!5 then byte 0x3d index old else old
      in if selection==snap!!4 then word 0x40 dat (byte 0x3c index previous) else previous
    record old (_,k,ordinal,Row (ChildId child) (SelectionId selection) _ _ _ _ _ _ _) =
      let chosen=if k==selected then word 12 child $ byte 0x1c (if q<=1 then fromIntegral ordinal else k `mod` pageSize s) old else old
      in if selection==snapshot s!!5 then byte 0x1d k chosen else chosen

allTargets :: [ServiceAddress]
allTargets=map ServiceAddress [0x43550,0x61ce1c,0x61cd14,0x7221c8,0xbf8e64,0xbf8d20,0x11730dc,0x4d978,0x4da6c,0xbf9360,0x1112b2c,0x1163e04,0x1112da4,0x1113514,0x116dce0,0x1112e78,0x1177e10,0x1163c1c,0x111236c,0x1112d84,0x116cf50,0x1112ce4,0xbf93e0,0x116e264,0x1137cf8,0x1138060,0xbf8f4c,0xbf90f0,0x116f948,0x116d4e4,0x1159168]

plain :: Word32 -> [Word32] -> Word32 -> ServiceCall
plain = call . ServiceAddress
font :: Word32 -> ServiceCall
font value = (plain 0x1112b2c [value] 0) {serviceWrites=[AbsoluteMemory fontAddress 0 (wordsLE [value])]}
record :: Word32 -> B.ByteString -> Word32 -> ServiceCall
record target bytes result=(recordCall (ServiceAddress target) bytes) {serviceOutputs=[(Argument 0,result)]}

expectedCalls :: Scenario -> [ServiceCall]
expectedCalls s = prefix ++ prepass ++ loop 0 0 0 (rows s) ++ nestedCalls s ++ [font 9]
  where
    NodeId n=node s; DrawMode m=mode s; q=currentPage s; selected=selectedIndex s
    Grid first second firstStep secondStep=grid s
    p=pageSize s
    kind=effectiveKind s
    sourceCall
      | m<=13 && testBit (0x2280::Word32) (fromIntegral m)=saved 2
      | m<=13 && testBit (0x1860::Word32) (fromIntegral m)=saved 1
      | otherwise=outCall (ServiceAddress 0x61cd14) [] (Argument 0) (wordsLE (snapshot s))
    saved slot=outCall (ServiceAddress 0x61ce1c) [(Argument 0,n),(Argument 1,slot)] (Argument 2) (wordsLE (snapshot s))
    describe raw (Row (ChildId child) (SelectionId selection) (Variant variant) state _ _ _ _ _)=
      outCall (ServiceAddress 0xbf8d20) [(Argument 0,n),(Argument 1,fromIntegral raw)] (Argument 2)
        (wordsLE [0x600+fromIntegral raw,child,selection,variant,stateWord state,0])
    prepass
      | kind/=2=[]
      | otherwise=concat [describe raw row : [resolve | state/=Hidden] | (raw,row@(Row _ _ _ state _ _ _ _ _))<-zip [0::Int ..] (rows s)]
          ++ [plain 0x4d978 [selected,p] (selected `div` p)]
    resolve=case resolver s of
      ChildMatch->plain 0x11730dc [n,2] 0
      Resolved (ChildId parent) (SelectionId selection) (Cursor index)->
        (plain 0x11730dc [n,2] 1) {serviceWrites=[MemoryArgument (Argument 2) 0 (wordsLE [parent]),MemoryArgument (Argument 3) 0 (wordsLE [selection]),IndirectMemoryArgument (Argument 13) 0 (wordsLE [index])]}
    prefix=
      [ (outCall (ServiceAddress 0x43550) [(Argument 1,0),(Argument 2,64)] (Argument 0) (B.replicate 64 0))
      , sourceCall
      , outCall (ServiceAddress 0x7221c8) [(Argument 0,reportAddress s),(Argument 1,0),(Argument 2,68)] (Argument 0) (B.replicate 68 0)
      , outCall (ServiceAddress 0xbf8e64) [(Argument 0,n)] (Argument 1) (wordsLE [fromIntegral (length (rows s)),0,let PageCapacity cap=capacity s in cap,0x1234,0x5678,0x9abc])
      ]
    loop _ _ _ []=[]
    loop raw k ordinal (Row (ChildId child) (SelectionId selection) (Variant variant) state (ResourceId label) (ResourceId value) labelHook valueHook palette:rest) =
      describe : if state==Hidden then next k ordinal else
      plain 0x4d978 [k+p,p] rowPage : if q>rowPage then next ((k+1) `mod` 256) ordinal else if q<rowPage then [] else
      division ++ draw ++ next ((k+1) `mod` 256) (ordinal+1)
      where
        rowPage=(k+p) `div` p
        describe=outCall (ServiceAddress 0xbf8d20) [(Argument 0,n),(Argument 1,fromIntegral raw)] (Argument 2)
          (wordsLE [0x600+fromIntegral raw,child,selection,variant,stateWord state,0])
        next = \k' ordinal' -> loop (raw+1) k' ordinal' rest
        division=if k==selected && rowPage>1 then [(plain 0x4da6c [k,p] (k `div` p)) {serviceOutputs=[(Argument 0,k `div` p),(Argument 1,k `mod` p)]}] else []
        x=wrap16 (first+ordinal*firstStep)
        y=wrap16 (second+center s*secondStep+ordinal*secondStep)
        pixelY=wrap16 (24*(y-1))
        chosen=if k==selected then 1 else 0
        hookRecord resource hookMode=wordsLE [n,child,variant,0,0,0,0,0,signed16 x,signed16 y,0,chosen,resource,kind,hookMode,fromIntegral raw]
        descriptor resource valueDescriptor=wordsLE [resource] <>
          halfLE [fromIntegral (if valueDescriptor then -20 else 20*(x-1)),fromIntegral pixelY,0,fromIntegral (if valueDescriptor then pixelY+48 else 0)] <>
          wordsLE [0,0,0,if valueDescriptor then 2 else 0,0]
        formatMode=m `elem` [11,12]
        previousMode=if ordinal==0 || formatMode then 0 else m
        alternate=kind==2 || m `elem` [5,6,11,12]
        draw=[plain 0xbf9360 [n,fromIntegral raw,0] label,font (if alternate then 15 else 13),plain 0x1163e04 [chosen,stateWord state] 0] ++
          (if formatMode && chosen/=0 then [plain 0x1112da4 [] 0,cellBox (x-1) y 30 17 56 153] else []) ++
          [record 0x116dce0 (hookRecord label previousMode) (hookWord labelHook)] ++
          [record 0x1112e78 (descriptor label False) 0 | labelHook==DefaultLeaf] ++
          (if n==0x38 && snapshot s!!4==11 && snapshot s!!3==0x228 && chosen/=0 && (m==13 || sideMode s==DrawMode 13) then
            [textFormat True,plain 0x1163c1c [0,0] 0,plain 0x111236c [13,signed8 y,label] 0,cellBox (x-1) y 31 15 124 255,textFormat False] else []) ++
          (if formatMode then [plain 0x1112d84 [] 0 | chosen/=0] else
            [plain 0x116cf50 [n,child] (if palette then 1 else 0)] ++
            [plain 0x1112ce4 [8] 0 | palette] ++
            [plain 0xbf93e0 [n,child,variant,0] value,font (if kind==2 then 15 else 13),record 0x116e264 (hookRecord value m) (hookWord valueHook)] ++
            [record 0x1112e78 (descriptor value True) 0 | valueHook==DefaultLeaf] ++
            (if kind==1 && valueHook==DefaultLeaf && value/=0 && m `notElem` [5,6] then
              [font 16,plain 0x1137cf8 [28,204,107,204] 0,
               (plain 0x1138060 [24,30,signed8 y,1] 0) {serviceReads=[MemoryArgument (Argument 13) 0 (wordsLE [2,0xff000101])]}]
             else []))

cellBox :: Int -> Int -> Word32 -> Word32 -> Word32 -> Word32 -> ServiceCall
cellBox x y right stroke colour alpha=
  (plain 0x1113514 [signed8 x,signed8 y,right,signed8 (y+1)] 0)
    {serviceReads=[MemoryArgument (Argument 13) 0 (wordsLE [stroke,colour,alpha])]}
textFormat :: Bool -> ServiceCall
textFormat enabled=plain 0x1177e10 [0x1113bf4,if enabled then 1 else 0,0] 0

nestedCalls :: Scenario -> [ServiceCall]
nestedCalls s
  | effectiveKind s/=2 || mode s `elem` [DrawMode 2,DrawMode 3]=[]
  | otherwise=outCall (ServiceAddress 0xbf8f4c) [(Argument 0,n),(Argument 1,parent)] (Argument 2)
      (wordsLE [fromIntegral (length (nestedRows s)),0,cap,0x1357,0x2468,0xace0]) : loop 0 0 (snapshot s!!10) (nestedRows s)
  where
    NodeId n=node s; DrawMode m=mode s; parent=snapshot s!!3; PageCapacity cap=nestedCapacity s
    Grid first second firstStep secondStep=nestedGrid s
    initialHook=case reverse (drawnRows s) of
      []->B.replicate 64 0
      (raw,k,ordinal,Row (ChildId child) _ (Variant variant) _ (ResourceId label) (ResourceId value) _ _ _):_->
        let Grid x y dx dy=grid s; formatMode=m `elem` [11,12]
        in wordsLE [n,child,variant,0,0,0,0,0,signed16 (wrap16 (x+ordinal*dx)),signed16 (wrap16 (y+center s*dy+ordinal*dy)),0,
          if k==selectedIndex s then 1 else 0,if formatMode then label else value,2,if formatMode then 0 else m,fromIntegral raw]
    loop _ _ _ []=[]
    loop raw drawn skip (r@(NestedRow (SelectionId selection) state (ResourceId label) _ hook):rest)
      | drawn>=cap=[]
      | otherwise=outCall (ServiceAddress 0xbf90f0) [(Argument 0,n),(Argument 1,parent),(Argument 2,fromIntegral raw)] (Argument 3)
          (let NestedRow _ _ _ (ResourceId dat) _=r in wordsLE [selection,stateWord state,dat]) :
          if state==Hidden then loop (raw+1) drawn skip rest
          else if skip/=0 then loop (raw+1) drawn (skip-1) rest
          else draw ++ loop (raw+1) ((drawn+1) `mod` 256) skip rest
      where
        x=wrap16 (first+fromIntegral drawn*firstStep)
        y=wrap16 (second+nestedCenter s*secondStep+fromIntegral drawn*secondStep)
        selected=selection==snapshot s!!4
        recordBytes=foldr (uncurry word) initialHook [(0,n),(4,parent),(8,selection),(32,signed16 x),(36,signed16 y),(44,if selected then 1 else 0),(48,label)]
        descriptor=wordsLE [label]<>halfLE [fromIntegral (20*(x-1)),fromIntegral (24*(y-1)),0,0]<>wordsLE [0,0,0,0,0]
        draw=[plain 0xbf93e0 [n,parent,selection,1] label,font 11,plain 0x1137cf8 [if selected then 105 else if state==Disabled then 107 else 106,255,0,0] 0,
          record 0x116f948 recordBytes (hookWord hook)] ++
          [record 0x1112e78 descriptor 0 | hook==DefaultLeaf] ++
          (if selected then
            (if m `elem` [7,13] || sideMode s==DrawMode 13 then
              [textFormat True,plain 0x1163c1c [0,stateWord state] 0] ++
              (if hook==Handled then [record 0x116f948 (word 44 0 recordBytes) (hookWord hook)]
               else [plain 0x111236c [if n==0x38 then 21 else 14,signed8 y,label] 0]) ++
              [cellBox (x-1) y 32 15 124 255,textFormat False]
             else []) ++
            [plain 0x116d4e4 [n,parent,snapshot s!!4] 0 | sideMode s `notElem` [DrawMode 2,DrawMode 3]] ++
            [plain 0x1159168 [n,parent,snapshot s!!4] 0]
           else [])

callerFrame :: Scenario -> B.ByteString
callerFrame s = let Grid _ _ _ secondStep=grid s; DrawMode m=mode s; Grid x y dx dy=nestedGrid s
                in wordsLE [fromIntegral secondStep,fromIntegral x,fromIntegral y,fromIntegral dx,fromIntegral dy,m,reportAddress s]

savedRegisters :: Scenario -> [(String,Word32)]
savedRegisters s=
  [("r4",if caller s==SecondaryTree then n else 0x44444444),
   ("r5",if caller s==SecondaryTree then reportAddress s else 0x55555555),
   ("r6",0x66666666),("r7",if caller s==SecondaryTree then m else 0x77777777),
   ("r8",0x88888888),("r9",0x99999999),("r10",0xaaaaaaaa),
   ("r11",if caller s==SecondaryTree then entrySP+140 else 0xbbbbbbbb)]
  where NodeId n=node s; DrawMode m=mode s

registers :: Scenario -> Value
registers s = object [K.fromString name .= addressText value | (name,value)<-values]
  where NodeId n=node s; Grid x y dx _=grid s
        values=[("r0",n),("r1",fromIntegral x),("r2",fromIntegral y),("r3",fromIntegral dx)] ++ savedRegisters s ++
          [("r12",0xcccccccc),("sp",entrySP),("lr",returnAddress s),("cpsr",0x10)]

emit :: FilePath -> B.ByteString -> TableImage -> Maybe CandidateCode -> Scenario -> IO FilePath
emit output image tables candidate s = do
  let initialStack=case caller s of MenuList->B.replicate 0x2000 0; SecondaryTree->put 0x1024 (B.replicate 68 0xa5) (B.replicate 0x2000 0)
      stack=put 0x1000 (callerFrame s) initialStack
      DrawMode special=sideMode s
      regions=[("tree-body",0x1164e1c,"rx",B.take 3204 (B.drop (0x1164e1c-0x1100000) image)),
        ("caller-return",returnAddress s,"rx",B.take 4 (B.drop (fromIntegral (returnAddress s)-0x1100000) image)),("stack",stackAddress,"rw",stack),
        ("side",sideAddress,"rw",wordsLE [0x12345678,0x89abcdef,0x55555555,0x77777777,0x99999999,special]),
        ("font",fontAddress,"rw",wordsLE [9]),("menu",0x9571d0,"rw",wordsLE (snapshot s) <> B.replicate (0x430-44) 0),
        ("palette",0x15e6680,"rw",B.replicate 64 0)] ++
        [("report",reportAddress s,"rw",B.replicate 68 0xa5) | caller s==MenuList] ++ tableRegions tables s ++
        case candidate of Nothing -> []; Just (CandidateCode bytes _ _) -> [("candidate-code",0x01800000,"rx",bytes)]
      (entry,functions,frameBytes)=case candidate of
        Nothing -> (0x1164e1c,[(0x1164e1c,3204)],352)
        Just (CandidateCode _ spans (FrameBytes frame)) -> (0x01800000,spans,frame)
  artifacts <- forM regions $ \(name,_,_,bytes) -> do
    let filename=scenarioName s++"-"++name++".bin"
    B.writeFile (output </> filename) bytes
    pure (object ["id" .= name,"path" .= filename,"size" .= B.length bytes,"sha256" .= digest bytes])
  let regionJSON (name,address,permission,bytes)=object
        ["id" .= name,"address" .= addressText address,"size" .= B.length bytes,"permissions" .= (permission::String),
         "initial" .= object ["artifact" .= name,"offset" .= (0::Int),"length" .= B.length bytes]]
      memory name offset bytes=object ["region" .= (name::String),"offset" .= (offset::Int),"hex" .= hexBytes bytes]
      DrawMode m=mode s
      side=wordsLE [if null (drawnRows s) then 0x12345678 else 3,0x89abcdef,if null (drawnRows s) then 0x55555555 else if effectiveKind s==2 || m `elem` [5,6,11,12] then 1 else 0,0x77777777,0x99999999,special]
      value=object ["schema_version" .= (1::Int),"id" .= scenarioName s,"origin" .= ("stock"::String),
        "provenance" .= ("Expected calls and memory derive from X-M1 image-6 bytes 0x1164e1c..0x1165aa0 and caller 0x116bff4..0x116c058, documented in reports/display-render-contracts/tree-list.md. Cases cover live and both saved slots, flat and nested tree flow, report preservation, and both caller return sites. SecondaryTree preserves its actual outgoing stack arguments and report at entry SP+36. All menu accessors and draw leaves remain scripted retained boundaries; pinned table slices are not executed."::String),
        "cpu" .= object ["architecture" .= ("arm"::String),"model" .= ("cortex-r5f"::String),"mode" .= ("arm"::String),"endian" .= ("little"::String)],
        "artifacts" .= artifacts,"entry" .= addressText entry,"exit" .= addressText (returnAddress s),
        "registers" .= registers s,"regions" .= map regionJSON regions,
        "functions" .= [object ["address" .= addressText address,"size" .= size] | (address,size)<-functions],
        "stubs" .= callsJSON allTargets (expectedCalls s),
        "limits" .= object ["instructions" .= (20000::Int),"wall_ms" .= (5000::Int)],
        "expected" .= object ["status" .= ("returned"::String),
          "ignored_memory" .= [object ["region" .= ("stack"::String),"offset" .= (0x1000-frameBytes::Int),"length" .= frameBytes]],
          "registers" .= object (("sp" .= addressText entrySP):("pc" .= addressText (returnAddress s)):[K.fromString name .= addressText address | (name,address)<-savedRegisters s]),
          "memory" .= [case caller s of MenuList->memory "report" 0 (expectedReport s); SecondaryTree->memory "stack" 0x1024 (expectedReport s),
            memory "side" 0 side,memory "font" 0 (wordsLE [9]),memory "stack" 0x1000 (callerFrame s)],
          "events" .= eventsJSON (expectedCalls s)],
        "exclusions" .= (["All 31 callees are scripted boundaries; no rendering leaf or menu table implementation is executed.","Private callee frame bytes are ignored. Caller arguments and all other mapped bytes remain exact.","Host user-mode A32 only; no camera, task scheduling, timing or device claims."]::[String])]
      path=output </> scenarioName s++".json"
  B.writeFile path (encodeCanonical value)
  pure path

main :: IO ()
main = do
  args<-getArgs
  let (options,runner)=case reverse args of
        binary:"--runner":rest -> (reverse rest,Just binary)
        _ -> (args,Nothing)
  case options of
    ["--image",image,"--output",destination] -> generate image destination Nothing runner
    ["--image",image,"--output",destination,"--candidate",binary,"--symbols",symbols,"--frame-bytes",frame] -> do
      candidate<-loadCandidate binary symbols frame
      generate image destination (Just candidate) runner
    _ -> fail "usage: tree-list-cases --image IMAGE6 --output DIRECTORY [--candidate IMAGE --symbols NM-SYMBOLS --frame-bytes N] [--runner NATIVE-RUNNER]"
  where
    generate image destination candidate runner=do
      bytes<-B.readFile image
      ensure (digest bytes=="ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d") "image 6 SHA-256 differs"
      let body=B.take 3204 (B.drop (0x1164e1c-0x1100000) bytes)
      ensure (digest body=="785cbbbfbd3a4542c8798970e5038eecb7c4b5e694e84331d6b75334379a84d9") "tree function SHA-256 differs from contract"
      tableBytes<-B.readFile (takeDirectory (takeDirectory image) </> "lz" </> "id4-0061c400.bin")
      ensure (digest tableBytes=="f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") "menu table image 4 SHA-256 differs"
      output<-makeAbsolute destination
      createDirectoryIfMissing True output
      paths<-mapM (emit output bytes (TableImage tableBytes) candidate) scenarios
      forM_ runner $ \binary -> do
        results<-forM paths $ \path -> do
          (_,result)<-runCase binary path
          B.writeFile (path++".report") (encodeCanonical result)
          let passed=str (get result "status")=="passed"
          unless passed (putStrLn (path++": "++str (get result "status")))
          pure passed
        ensure (and results) "tree-list qualification failed"
      when (null runner) (putStrLn "Cases emitted without execution")
      putStrLn (show (length paths)++" tree-list cases")
