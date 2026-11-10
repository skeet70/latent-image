{-# LANGUAGE GHC2021 #-}
module Emulation.ValuePickerCases
  ( PickerCase (..), PickerKind (..), RowState (..), Row (..), Layout (..)
  , LabelId (..), NodeId (..), HookResult (..), Setting44 (..), LabelGate (..)
  , pickerCases, pickerCalls, pickerReport, pickerSnapshot, targets, sideState
  , ReportAddress (..), PickerSite (..), pickerStackArguments, pickerReturnAddress, pickerX, pickerY
  , pickerWidth, pickerReportOffset
  ) where

import qualified Data.ByteString as B
import Data.Bits ((.&.), shiftR)
import Data.Word (Word32)
import Emulation.RenderCases
import Data.List (nubBy)

newtype LabelId = LabelId Word32 deriving (Eq, Show)
newtype NodeId = NodeId Word32 deriving (Eq, Show)
newtype ReportAddress = ReportAddress Word32 deriving (Eq, Show)
data PickerKind = ValueKind | GenericKind | BracketKind | VariantKind deriving (Eq, Show)
data RowState = Offered | Disabled | Hidden | Variant deriving (Eq, Show)
data Layout = Scroll Word32 | Page Word32 deriving (Eq, Show)
data HookResult = DrawLeaf | HookHandled deriving (Eq, Show)
data Setting44 = SettingZero | SettingNonzero deriving (Eq, Show)
data LabelGate = StockGate | BankGate deriving (Eq, Show)
data Row = Row { rowId :: Word32, rowState :: RowState, rowLabel :: LabelId } deriving (Eq, Show)
data PickerSite = MenuPicker | FnPicker | Picker9304 | Picker9a14 | Pickera1b8 | Pickera8b0 | Pickerb52c | Pickerb568 deriving (Eq, Show)
data PickerCase = PickerCase
  { pickerName :: String, pickerKind :: PickerKind, pickerRows :: [Row]
  , pickerPageSize :: Word32, pickerVisibleRows :: Word32, pickerLayout :: Layout
  , pickerCursor :: Word32, pickerCurrent :: Word32, pickerMode :: Word32
  , pickerHook :: HookResult, pickerSetting :: Setting44, pickerGate :: LabelGate
  , pickerNode :: NodeId, pickerDetail :: Bool, pickerSite :: PickerSite
  } deriving (Eq, Show)

kindWord :: PickerKind -> Word32
kindWord ValueKind = 1
kindWord GenericKind = 2
kindWord BracketKind = 4
kindWord VariantKind = 9

stateWord :: RowState -> Word32
stateWord Offered = 0
stateWord Disabled = 2
stateWord Hidden = 3
stateWord Variant = 1

sideState :: B.ByteString
sideState = wordsLE [0x10203040,0x50607080,0x90a0b0c0,0xd0e0f001,0x12345678]

targets :: [ServiceAddress]
targets = map ServiceAddress [0x43550,0x4d978,0x61c93c,0x61cd14,0x61ce1c,0x7221c8,0xbf8f4c,0xbf90f0,0xbf93e0,0xbf96b0,0x111236c,0x111258c,0x111299c,0x1112a6c,0x1112b2c,0x1112e38,0x1112e58,0x1112e78,0x11130e0,0x1113514,0x1130e68,0x1137cf8,0x1137d2c,0x1138060,0x1138a00,0x11391a0,0x1139a68,0x1159168,0x116f948,0x1177e10]

pickerSnapshot :: PickerCase -> B.ByteString
pickerSnapshot c = wordsLE [0x17,0x30,kindWord (pickerKind c),0x39,pickerCursor c,pickerCurrent c,0,0,pickerVisibleRows c,page,top]
  where (page,top) = case pickerLayout c of Scroll n -> (1,n); Page n -> (n,0)

pickerStackArguments :: PickerCase -> ReportAddress -> B.ByteString
pickerStackArguments c (ReportAddress report) = wordsLE [columnStep c,rowStep c,style c,scrollFlag,1,pickerMode c,report]
  where scrollFlag = case pickerLayout c of Scroll _ -> 1; Page _ -> 0

pickerReturnAddress :: PickerCase -> Word32
pickerReturnAddress c = case pickerSite c of
  MenuPicker -> 0x01168ce4
  FnPicker -> 0x0116c588
  Picker9304 -> 0x01169308
  Picker9a14 -> 0x01169a18
  Pickera1b8 -> 0x0116a1bc
  Pickera8b0 -> 0x0116a8b4
  Pickerb52c -> 0x0116b530
  Pickerb568 -> 0x0116b56c

pickerReportOffset :: PickerCase -> Int
pickerReportOffset c = case pickerSite c of
  MenuPicker -> 0x109c
  FnPicker -> 0x104c
  Picker9304 -> 0x10ac
  Picker9a14 -> 0x1054
  Pickera1b8 -> 0x1084
  Pickera8b0 -> 0x105c
  Pickerb52c -> 0x106c
  Pickerb568 -> 0x106c

pickerX :: PickerCase -> Word32
pickerX c = case pickerSite c of
  MenuPicker -> 4
  FnPicker -> 14
  Picker9304 -> 4
  Picker9a14 -> 6
  Pickera1b8 -> 3
  Pickera8b0 -> 14
  Pickerb52c -> 1
  Pickerb568 -> 1

pickerY :: PickerCase -> Word32
pickerY c = case pickerSite c of
  MenuPicker -> 4
  FnPicker -> 3
  Picker9304 -> 3
  Picker9a14 -> 4
  Pickera1b8 -> 5
  Pickera8b0 -> 4
  Pickerb52c -> 4
  Pickerb568 -> 4

pickerWidth :: PickerCase -> Word32
pickerWidth c = if pickerSite c == Pickera1b8 then 6 else 0

horizontal :: PickerCase -> Bool
horizontal c = pickerSite c `elem` [Pickerb52c,Pickerb568]

columnStep :: PickerCase -> Word32
columnStep c = if horizontal c then 255 else 0

rowStep :: PickerCase -> Word32
rowStep c = if horizontal c then 0 else 2

style :: PickerCase -> Word32
style c = if pickerSite c == Pickera1b8 then 3 else 0

capacity :: PickerCase -> Word32
capacity c = max 1 (pickerPageSize c)

service :: Word32 -> [Word32] -> Word32 -> ServiceCall
service a = call (ServiceAddress a)

output :: Word32 -> [(Int,Word32)] -> Int -> B.ByteString -> ServiceCall
output a args dest = outCall (ServiceAddress a) [(Argument r,v) | (r,v) <- args] (Argument dest)

font :: Word32 -> ServiceCall
font n = (service 0x1112b2c [n] 0) {serviceWrites=[AbsoluteMemory 0x015e46c0 0 (wordsLE [n])]}

palette :: Word32 -> ServiceCall
palette n = service 0x1137cf8 [n,255,0,0] 0

nodeWord :: PickerCase -> Word32
nodeWord c = let NodeId n = pickerNode c in n

childCall :: PickerCase -> Word32 -> Row -> ServiceCall
childCall c i r = output 0xbf90f0 [(0,nodeWord c),(1,0x39),(2,i)] 3 (wordsLE [rowId r,stateWord (rowState r),0x700+rowId r])

labelCall :: PickerCase -> Row -> Word32 -> Word32 -> ServiceCall
labelCall c r which result = service 0xbf93e0 [nodeWord c,0x39,rowId r,which] result

labelWord :: Row -> Word32
labelWord r = let LabelId n = rowLabel r in n

width :: Word32 -> Word32
width label = if label <= 0x83d then 3 else 5

widthCall :: Word32 -> Word32 -> ServiceCall
widthCall gate label = service (if label <= gate then 0x1112a6c else 0x11391a0) [label] (if label <= gate then 3 else 5)

prepass :: PickerCase -> [ServiceCall]
prepass c | pickerKind c /= BracketKind = []
prepass c = concat [childCall c i r : if rowState r == Hidden then [] else [labelCall c r 1 (labelWord r),widthCall gate (labelWord r)] | (i,r) <- zip [0..] (pickerRows c)]
  where gate = case pickerGate c of StockGate -> 0x83d; BankGate -> 0x849

xPosition :: PickerCase -> Word32
xPosition c | pickerKind c /= BracketKind = pickerX c
xPosition c = (17-((total+2) `shiftR` 1)) .&. 0xffff
  where
    gate = case pickerGate c of StockGate -> 0x83d; BankGate -> 0x849
    total = sum [if labelWord r <= gate then 3 else 5 | r <- pickerRows c,rowState r /= Hidden]

data Visit = Visit Word32 Word32 Row Bool Word32 deriving Show

visits :: PickerCase -> [Visit]
visits c = go 0 0 0 0 initialTop (pickerRows c)
  where
    initialTop = case pickerLayout c of Scroll n -> n; Page _ -> 0
    go _ _ _ _ _ [] = []
    go raw hidden pageCount drawn top (r:rs)
      | rowState r == Hidden = Visit raw hidden r False drawn : go (raw+1) (hidden+1) pageCount drawn top rs
      | drawn >= capacity c = [Visit raw hidden r False drawn]
      | top /= 0 = Visit raw hidden r False drawn : go (raw+1) hidden pageCount drawn (top-1) rs
      | otherwise = case pickerLayout c of
          Page page | quotient < page -> Visit raw hidden r False drawn : go (raw+1) hidden (pageCount+1) drawn top rs
                    | quotient > page -> [Visit raw hidden r False drawn]
          _ -> Visit raw hidden r True drawn : go (raw+1) hidden (pageCount+1) (drawn+1) top rs
      where quotient = (pageCount+capacity c) `div` capacity c

resource :: PickerCase -> Word32 -> Word32 -> Word32 -> B.ByteString
resource c label x y = wordsLE [label] <> halfLE (map fromIntegral [(x-1)*20,(y-1)*24,(pickerWidth c-1)*20,(rowStep c-1)*24]) <> wordsLE [0,0,style c,0,0]

hookRecord :: PickerCase -> Row -> Word32 -> Word32 -> B.ByteString
hookRecord c r x y = wordsLE [nodeWord c,0x39,rowId r] <> sideState <> wordsLE [signed16 x,signed16 y,0,if rowId r == pickerCursor c then 1 else 0,labelWord r,0,0,0]

signed16 :: Word32 -> Word32
signed16 x = if x .&. 0x8000 /= 0 then (x .&. 0xffff)+0xffff0000 else x .&. 0xffff

hook :: PickerCase -> Row -> Word32 -> Word32 -> ServiceCall
hook c r x y = (recordCall (ServiceAddress 0x116f948) (hookRecord c r x y)) {serviceOutputs=[(Argument 0,if pickerHook c == DrawLeaf then 0 else 1)]}

leaf :: B.ByteString -> ServiceCall
leaf = recordCall (ServiceAddress 0x1112e78)

rowCalls :: PickerCase -> Visit -> [ServiceCall]
rowCalls c (Visit _ _ r drawn ordinal)
  | not drawn = []
  | otherwise = [labelCall c r 1 label,widthCall 0x83d label,font 12] ++ base ++ highlight ++ details
  where
    label = labelWord r
    x = xPosition c + if columnStep c == 255 then sum [width (labelWord previous) + if pickerKind c == BracketKind && rowId previous == pickerCursor c then 2 else 0 | Visit _ _ previous True _ <- takeDrawn ordinal] else ordinal*columnStep c
    takeDrawn count = take (fromIntegral count) [v | v@(Visit _ _ _ True _) <- visits c]
    y = pickerY c+rowStep c*(centering+ordinal)
    centering = if nodeWord c /= 0x4c && capacity c > pickerVisibleRows c then ((capacity c-pickerVisibleRows c) `shiftR` 1) .&. 255 else 0
    selected = rowId r == pickerCursor c
    bytes = resource c label x y
    base | pickerKind c /= BracketKind = [palette (if selected then 0x69 else if rowState r == Disabled then 0x6b else 0x6a),hook c r x y] ++ [leaf bytes | pickerHook c == DrawLeaf]
         | not selected = [font 12,palette (if rowState r == Disabled then 0x6b else 0),leaf bytes]
         | otherwise = bracketCalls c r x y
    highlight | selected && pickerMode c `elem` [7,13] =
      [service 0x1177e10 [0x1113bf4,1,0] 0,font 13,palette 0x6a,hook c r x y]
      ++ (if pickerHook c == DrawLeaf then [font 13,palette 0x6a,leaf bytes] else [])
      ++ [ (service 0x11130e0 [signed16 ((x-1)*20-20),signed16 ((y-1)*24),if nodeWord c `elem` [0x21,0x22,0x23,0x28,0x4c,0x83] then 600 else 640,signed16 ((y-1)*24+48)] 0) {serviceReads=[MemoryArgument (Argument 13) 0 (wordsLE [15,124,255])]},service 0x1177e10 [0x1113bf4,0,0] 0]
              | otherwise = []
    details | not selected = []
            | pickerKind c == BracketKind = [hook c r x y] ++ if pickerHook c == DrawLeaf then bracketDetails c r y else []
            | otherwise = valueDetails c r y

bracketCalls :: PickerCase -> Row -> Word32 -> Word32 -> [ServiceCall]
bracketCalls c r x y =
  [font 12,service 0x1112e38 [] 0,service 0x1138a00 [2] 0x23,service 0x1137cf8 [7,255,0x23,255] 0
  ,leaf (resource c (labelWord r) (x+1) y)
  ,service 0x111236c [signed8 x,signed8 y,0xff00035f] 0
  ,service 0x111236c [signed8 (x+width (labelWord r)+1),signed8 y,0xff000360] 0
  ,service 0x1112e58 [] 0,service 0x1130e68 [0] 0,font 15
  ,service 0x111236c [signed8 x,signed8 y,0xff000060] 0
  ,(service 0x1138060 [signed8 (x+1),signed8 (x+width (labelWord r)),signed8 y,1] 0) {serviceReads=[MemoryArgument (Argument 13) 0 (wordsLE [2,0xff00005c])]}
  ,service 0x111236c [signed8 (x+width (labelWord r)+1),signed8 y,0xff000064] 0]

signed8 :: Word32 -> Word32
signed8 x = if x .&. 128 /= 0 then (x .&. 255)+0xffffff00 else x .&. 255

valueDetails :: PickerCase -> Row -> Word32 -> [ServiceCall]
valueDetails c r y
  | pickerKind c == ValueKind = [labelCall c r 2 detail] ++ (if detail /= 0 then drawValue detail False else []) ++ [labelCall c r 3 detail] ++ settingAndDraw True
  | otherwise = [labelCall c r 3 detail] ++ (if detail /= 0 then [font 15,service 0x1137d2c [19] 0,service 0x1112e38 [] 0,leaf (resourceSpecial detail 0xffec ((y-1)*24)),service 0x1112e58 [] 0] else [])
      ++ [labelCall c r 2 detail] ++ settingAndDraw False
      ++ [service 0x1159168 [nodeWord c,0x39,rowId r] 0 | pickerKind c == VariantKind]
  where
    detail = if pickerDetail c then 0x85 else 0
    drawValue d nodeOverride = [font (if pickerKind c == ValueKind then 13 else 15),service 0x1137d2c [10] 0,leaf (resourceSpecial d (if nodeOverride && nodeWord c == 6 then 140 else 0xffec) 0xffec)]
    settingAndDraw nodeOverride
      | detail == 0 = []
      | otherwise = [service 0x61c93c [0x44,0] (if pickerSetting c == SettingZero then 0 else 7)]
        ++ if pickerSetting c /= SettingZero then [] else drawValue detail nodeOverride ++ if pickerKind c == ValueKind then [] else
          [output 0x111299c [(0,detail),(1,0)] 2 (B.singleton 4),(service 0x1139a68 [5,2,3,25] 0) {serviceReads=[MemoryArgument (Argument 13) 0 (wordsLE [20,18,1])]}]

resourceSpecial :: Word32 -> Word32 -> Word32 -> B.ByteString
resourceSpecial label x y = wordsLE [label] <> halfLE (map fromIntegral [x,y,0,0]) <> wordsLE [0,0,0,0,0]

bracketDetails :: PickerCase -> Row -> Word32 -> [ServiceCall]
bracketDetails c r y = [labelCall c r 0 first,labelCall c r 2 second,labelCall c r 3 third] ++ labels ++
  [service 0xbf96b0 [] 5,service 0x1137cf8 [99,204,0,0] 0
  ,(service 0x1113514 [left,signed8 initial,right,signed8 (bottom-1)] 0) {serviceReads=[MemoryArgument (Argument 13) 0 (wordsLE [17,99,204])]}
  ,font 17
  ,(service 0x1138060 [left+1,right-1,signed8 bottom,1] 0) {serviceReads=[MemoryArgument (Argument 13) 0 (wordsLE [1,0xff000361])]}
  ,service 0x111236c [left,signed8 bottom,0xff000362] 0,service 0x111236c [right,signed8 bottom,0xff000363] 0]
  where
    (left,right) = if nodeWord c `elem` [0x10,0x30] then (5,28) else (6,29)
    (first,second,third) = if pickerDetail c then (0x85,0x86,0x87) else (0,0,0)
    initial = (y+2) .&. 0xffff
    bottom = if pickerDetail c then y+2+6 else initial
    labels = if not (pickerDetail c) then [] else
      [font 15,service 0x1137d2c [10] 0,leaf (resourceSpecial first 120 ((y+1)*24))
      ,output 0x111258c [(0,first)] 1 (B.pack [4,6])
      ,font 15,service 0x1137d2c [10] 0,leaf (resourceSpecial second 0xffec 0xffec)]

pickerCalls :: PickerCase -> [ServiceCall]
pickerCalls c = [output 0x43550 [(1,0),(2,64)] 0 (B.replicate 64 0)] ++ if pickerMode c `elem` [2,3] then [] else
  [if pickerMode c <= 13 && (2^pickerMode c :: Word32) .&. 0x2280 /= 0 then output 0x61ce1c [(0,nodeWord c),(1,2)] 2 (pickerSnapshot c) else output 0x61cd14 [] 0 (pickerSnapshot c)
  ,output 0x7221c8 [(1,0),(2,0x44)] 0 (B.replicate 0x44 0)
  ,output 0xbf8f4c [(0,nodeWord c),(1,0x39)] 2 (wordsLE [fromIntegral (length (pickerRows c)),0x55,pickerPageSize c,0,0,0])]
  ++ prepass c ++ loop 0 (visits c) ++ [font 9]
  where
    loop _ [] = []
    loop pageCount (v@(Visit raw _ r drawn ordinal):rest) = childCall c raw r : division ++ rowCalls c v ++ loop next rest
      where
        eligible = rowState r /= Hidden && ordinal < capacity c && case pickerLayout c of Scroll top -> raw >= top; Page _ -> True
        division = case pickerLayout c of Page _ | eligible -> [service 0x4d978 [pageCount+capacity c,capacity c] ((pageCount+capacity c) `div` capacity c)]; _ -> []
        next = pageCount + if drawn || not (null division) then 1 else 0

pickerReport :: PickerCase -> B.ByteString
pickerReport c | pickerMode c `elem` [2,3] = B.replicate 0x44 0xa5
pickerReport c = foldl patch (B.replicate 0x44 0) (base ++ rows)
  where
    patch bytes (offset,value) = B.take offset bytes <> value <> B.drop (offset+B.length value) bytes
    word offset value = (offset,wordsLE [value])
    byte :: Int -> Word32 -> (Int,B.ByteString)
    byte offset value = (offset,B.singleton (fromIntegral value))
    (page,top) = case pickerLayout c of Scroll n -> (1,n); Page n -> (n,0)
    center = if nodeWord c /= 0x4c && capacity c > pickerVisibleRows c then ((capacity c-pickerVisibleRows c) `shiftR` 1) .&. 255 else 0
    base = [word 0 0x17,word 4 (nodeWord c),word 0x20 0x39,word 0x28 (pickerVisibleRows c),word 0x2c (if nodeWord c == 0x4c then 3 else capacity c),word 0x30 top,word 0x38 (pickerCurrent c),byte 0x3d 255,byte 0x3e center]
    rows = concat [ [byte 0x3d index | rowId r == pickerCurrent c] ++ (if rowId r == pickerCursor c then [byte 0x3c index,word 0x34 (rowId r),word 0x40 (0x700+rowId r)] else []) | Visit raw hidden r True _ <- visits c,let index = raw-hidden-top-capacity c*(page-1)]

pickerCases :: [PickerCase]
pickerCases = nubBy (\a b -> pickerName a == pickerName b) (original ++ fnCases ++ callerCases)
  where
    original = basics ++ states ++ modes ++ details ++ bounds
    fnCases = [c {pickerName="fn-"++pickerName c,pickerNode=NodeId 0x63,pickerSite=FnPicker} | c <- original, Scroll _ <- [pickerLayout c]]
    callerCases =
      [ c {pickerName=show site++"-"++pickerName c,pickerSite=site,pickerNode=NodeId node,pickerLayout=if site `elem` [Pickerb52c,Pickerb568] then case pickerLayout c of Scroll _ -> Page 1; layout -> layout else pickerLayout c}
      | (site,node) <- [(Picker9304,0x30),(Picker9a14,0x21),(Pickera1b8,0x62),(Pickera8b0,0x1a),(Pickerb52c,10),(Pickerb568,11)]
      , c <- original
      , pickerGate c == StockGate
      , if site `elem` [Pickera1b8,Pickerb52c,Pickerb568] then pickerMode c == 0 else True
      , if site `elem` [Picker9304,Pickera1b8,Pickera8b0] then case pickerLayout c of Scroll _ -> True; _ -> False else True
      ]
    rows n = [Row i Offered (LabelId 0x83d) | i <- [1..n]]
    base name n size layout cursor = PickerCase name GenericKind (rows n) size n layout cursor n 0 DrawLeaf SettingZero StockGate (NodeId 0x30) False MenuPicker
    basics = [base ("rows-"++show n++"-cursor-"++show cursor) n 3 (Scroll 0) cursor | n <- [1,3,5],cursor <- [1,(n+1) `div` 2,n]]
      ++ [base ("page-"++show p++"-cursor-"++show cursor) 5 2 (Page p) cursor | p <- [1,2,3],cursor <- [1,3,5]]
      ++ [base ("scroll-"++show top) 5 2 (Scroll top) (top+1) | top <- [1,2,4]]
      ++ [(base "normalized-page-size" 1 0 (Scroll 0) 1), (base "centered-short-list" 1 5 (Scroll 0) 1)]
      ++ [base "zero-children" 0 3 (Scroll 0) 99]
    states = [(base ("state-"++show s++"-hook-"++show h) 3 3 (Scroll 0) 2) {pickerRows=[Row 1 s (LabelId 1),Row 2 Offered (LabelId 0x83e),Row 3 Disabled (LabelId 0x849)],pickerHook=h} | s <- [Offered,Disabled,Hidden,Variant],h <- [DrawLeaf,HookHandled]]
    modes = [(base ("mode-"++show m) 1 1 (Scroll 0) 1) {pickerMode=m} | m <- [2,3,7,9,13,5,6,11,12,14,255,0xffffffff]]
      ++ [(base "node-4c-highlight" 1 5 (Scroll 0) 1) {pickerMode=7,pickerNode=NodeId 0x4c}]
    details = [(base ("kind-"++show k++"-setting-"++show s) 1 1 (Scroll 0) 1) {pickerKind=k,pickerSetting=s,pickerDetail=True,pickerNode=NodeId 6} | k <- [ValueKind,GenericKind,VariantKind],s <- [SettingZero,SettingNonzero]]
    bounds = [(base ("bracket-"++show gate++"-label-"++show label) 1 1 (Scroll 0) 99) {pickerKind=BracketKind,pickerRows=[Row 1 Offered (LabelId label)],pickerGate=gate} | gate <- [StockGate,BankGate],label <- [0x83d,0x83e,0x849,0x84a]]
      ++ [(base "bracket-selected-hook-handled" 1 1 (Scroll 0) 1) {pickerKind=BracketKind,pickerHook=HookHandled}]
      ++ [(base "bracket-selected-hook-zero" 1 1 (Scroll 0) 1) {pickerKind=BracketKind,pickerHook=DrawLeaf}]
      ++ [(base "bracket-selected-details" 1 1 (Scroll 0) 1) {pickerKind=BracketKind,pickerHook=DrawLeaf,pickerDetail=True}]
