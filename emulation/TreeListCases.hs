{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module TreeListCases (main, scenarios, scenarioName, expectedReport, expectedCalls, NodeId (..), DrawMode (..), node, mode, snapshot) where

import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.ByteString as B
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
data Scenario = Scenario
  { scenarioName :: String, node :: NodeId, capacity :: PageCapacity, page :: Page
  , cursor :: Cursor, scroll :: ScrollTop, mode :: DrawMode, grid :: Grid, rows :: [Row]
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
      (Grid 2 3 0 2) (map row [0..count-1])
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
  ]
  where
    base = Scenario "flat" (NodeId 0x40) (PageCapacity 3) (Page 1) (Cursor 1) (ScrollTop 0) (DrawMode 0) (Grid 2 3 0 2) (map row [0..2])
    hidden (Row a b c _ d e f g h) = Row a b c Hidden d e f g h
    hooks h (Row a b c d e f _ _ i) = Row a b c d e f h h i

reportAddress, stackAddress, entrySP, fontAddress, sideAddress :: Word32
reportAddress=0x02100000
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
center s = let NodeId n=node s; v=visibleCount s; p=pageSize s
           in if n `elem` [0x53,0x57] || v>=p then 0 else fromIntegral (((p-v) `div` 2) `mod` 256)
wrap16 :: Int -> Int
wrap16 n = n `mod` 65536
signed16 :: Int -> Word32
signed16 = fromIntegral . (fromIntegral :: Int -> Int16)
signed8 :: Int -> Word32
signed8 = fromIntegral . (fromIntegral :: Int -> Int8)

snapshot :: Scenario -> [Word32]
snapshot s = let Cursor a=cursor s; Page q=page s; ScrollTop top=scroll s
             in [1,0x11223344,0,0x224,9,0x302,0,a,visibleCount s,q,top]

-- Earlier pages and hidden children leave drawn coordinates unchanged.
drawnRows :: Scenario -> [(Int,Word32,Int,Row)]
drawnRows s = go 0 0 0 (rows s)
  where
    Page q=page s; p=pageSize s
    go _ _ _ []=[]
    go raw k ordinal (r@(Row _ _ _ state _ _ _ _ _):rest)
      | state==Hidden = go (raw+1) k ordinal rest
      | q>rowPage = go (raw+1) ((k+1) `mod` 256) ordinal rest
      | q<rowPage = []
      | otherwise = (raw,k,ordinal,r):go (raw+1) ((k+1) `mod` 256) (ordinal+1) rest
      where rowPage=(k+p) `div` p

expectedReport :: Scenario -> B.ByteString
expectedReport s = foldl record initial (drawnRows s)
  where
    snap=snapshot s; Page q=page s; Cursor selected=cursor s
    initial=foldr (uncurry word) (byte 0x1d 255 $ byte 0x3d 255 $ byte 0x1e (fromIntegral (center s)) $ byte 0x1f q $ B.replicate 68 0)
      [(0,1),(4,snap!!1),(8,snap!!3),(0x10,snap!!4),(0x14,visibleCount s `mod` 256),(0x18,pageSize s),(0x20,snap!!3),(0x30,snap!!10),(0x38,snap!!5)]
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
expectedCalls s = prefix ++ loop 0 0 0 (rows s) ++ [font 9]
  where
    NodeId n=node s; DrawMode m=mode s; Page q=page s; Cursor selected=cursor s
    Grid first second firstStep secondStep=grid s
    p=pageSize s
    prefix=
      [ (outCall (ServiceAddress 0x43550) [(Argument 1,0),(Argument 2,64)] (Argument 0) (B.replicate 64 0))
      , outCall (ServiceAddress 0x61cd14) [] (Argument 0) (wordsLE (snapshot s))
      , outCall (ServiceAddress 0x7221c8) [(Argument 0,reportAddress),(Argument 1,0),(Argument 2,68)] (Argument 0) (B.replicate 68 0)
      , outCall (ServiceAddress 0xbf8e64) [(Argument 0,n)] (Argument 1) (wordsLE [fromIntegral (length (rows s)),0,let PageCapacity cap=capacity s in cap])
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
        hookRecord resource hookMode=wordsLE [n,child,variant,0,0,0,0,0,signed16 x,signed16 y,0,chosen,resource,1,hookMode,fromIntegral raw]
        descriptor resource valueDescriptor=wordsLE [resource] <>
          halfLE [fromIntegral (if valueDescriptor then -20 else 20*(x-1)),fromIntegral pixelY,0,fromIntegral (if valueDescriptor then pixelY+48 else 0)] <>
          wordsLE [0,0,0,if valueDescriptor then 2 else 0,0]
        previousMode=if ordinal==0 then 0 else m
        draw=[plain 0xbf9360 [n,fromIntegral raw,0] label,font 13,plain 0x1163e04 [chosen,stateWord state] 0,
          record 0x116dce0 (hookRecord label previousMode) (hookWord labelHook)] ++
          [record 0x1112e78 (descriptor label False) 0 | labelHook==DefaultLeaf] ++
          [plain 0x116cf50 [n,child] (if palette then 1 else 0)] ++
          [plain 0x1112ce4 [8] 0 | palette] ++
          [plain 0xbf93e0 [n,child,variant,0] value,font 13,record 0x116e264 (hookRecord value m) (hookWord valueHook)] ++
          [record 0x1112e78 (descriptor value True) 0 | valueHook==DefaultLeaf] ++
          (if valueHook==DefaultLeaf && value/=0 then
            [font 16,plain 0x1137cf8 [28,204,107,204] 0,
             (plain 0x1138060 [24,30,signed8 y,1] 0) {serviceReads=[MemoryArgument (Argument 13) 0 (wordsLE [2,0xff000101])]}]
           else [])

callerFrame :: Scenario -> B.ByteString
callerFrame s = let Grid _ _ _ secondStep=grid s; DrawMode m=mode s
                in wordsLE [fromIntegral secondStep,4,4,0,2,m,reportAddress]

registers :: Scenario -> Value
registers s = object [K.fromString name .= addressText value | (name,value)<-values]
  where NodeId n=node s; Grid x y dx _=grid s
        values=[("r0",n),("r1",fromIntegral x),("r2",fromIntegral y),("r3",fromIntegral dx),
          ("r4",0x44444444),("r5",0x55555555),("r6",0x66666666),("r7",0x77777777),
          ("r8",0x88888888),("r9",0x99999999),("r10",0xaaaaaaaa),("r11",0xbbbbbbbb),
          ("r12",0xcccccccc),("sp",entrySP),("lr",0x01167a60),("cpsr",0x10)]

emit :: FilePath -> B.ByteString -> TableImage -> Maybe CandidateCode -> Scenario -> IO FilePath
emit output image tables candidate s = do
  let stack=put 0x1000 (callerFrame s) (B.replicate 0x2000 0)
      regions=[("tree-body",0x1164e1c,"rx",B.take 3204 (B.drop (0x1164e1c-0x1100000) image)),
        ("caller-return",0x1167a60,"rx",B.take 4 (B.drop (0x1167a60-0x1100000) image)),("stack",stackAddress,"rw",stack),
        ("report",reportAddress,"rw",B.replicate 68 0xa5),("side",sideAddress,"rw",wordsLE [0x12345678,0x89abcdef,0x55555555,0x77777777,0x99999999,0]),
        ("font",fontAddress,"rw",wordsLE [9]),("menu",0x9571d0,"rw",wordsLE (snapshot s) <> B.replicate (0x430-44) 0),
        ("palette",0x15e6680,"rw",B.replicate 64 0)] ++ tableRegions tables s ++
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
      side=wordsLE [if null (drawnRows s) then 0x12345678 else 3,0x89abcdef,if null (drawnRows s) then 0x55555555 else 0,0x77777777,0x99999999,0]
      value=object ["schema_version" .= (1::Int),"id" .= scenarioName s,"origin" .= ("stock"::String),
        "provenance" .= ("Expected calls and memory derived before execution from reports/display-render-contracts/tree-list.md. Owned flat/live MenuList class. No nested scroll loop or page tabs. The 0x430-byte menu block contains the synthetic snapshot followed by zero bytes. Registry 0x81babc, 40-byte node record and 256-byte child-table prefix are pinned image-4 slices; all menu accessors remain stubbed."::String),
        "cpu" .= object ["architecture" .= ("arm"::String),"model" .= ("cortex-r5f"::String),"mode" .= ("arm"::String),"endian" .= ("little"::String)],
        "artifacts" .= artifacts,"entry" .= addressText entry,"exit" .= addressText 0x01167a60,
        "registers" .= registers s,"regions" .= map regionJSON regions,
        "functions" .= [object ["address" .= addressText address,"size" .= size] | (address,size)<-functions],
        "stubs" .= callsJSON allTargets (expectedCalls s),
        "limits" .= object ["instructions" .= (20000::Int),"wall_ms" .= (5000::Int)],
        "expected" .= object ["status" .= ("returned"::String),
          "ignored_memory" .= [object ["region" .= ("stack"::String),"offset" .= (0x1000-frameBytes::Int),"length" .= frameBytes]],
          "registers" .= object ["sp" .= addressText entrySP,"r4" .= addressText 0x44444444,"r5" .= addressText 0x55555555,"r6" .= addressText 0x66666666,"r7" .= addressText 0x77777777,"r8" .= addressText 0x88888888,"r9" .= addressText 0x99999999,"r10" .= addressText 0xaaaaaaaa,"r11" .= addressText 0xbbbbbbbb,"pc" .= addressText 0x01167a60],
          "memory" .= [memory "report" 0 (expectedReport s),memory "side" 0 side,memory "font" 0 (wordsLE [9]),memory "stack" 0x1000 (callerFrame s)],
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
