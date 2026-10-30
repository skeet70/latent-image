{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value, object, (.=))
import Data.Bits ((.&.), shiftR, testBit)
import qualified Data.ByteString as B
import Data.List (sortOn)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import Emulation.NativeCase (loadCase)
import Emulation.NativeObservation
import Emulation.OwnedCase
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.JSON (encodeCanonical)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))

newtype FrameClass = FrameClass Word32
newtype RowToken = RowToken Word32
newtype NodeId = NodeId Word32
newtype StateWord = StateWord Word32
newtype Notification = Notification Word32
newtype CellAddress = CellAddress Word32

data Action = Set | Confirm | Leave deriving (Eq, Show)
data StageKind = Direct | Enter | Exit deriving (Eq, Show)
data StageId = Accept | Cancel deriving (Eq, Show)
data Effect
  = Open StageKind StageId
  | Notify Notification
  | Postlude
  | ConfirmEditor Word32
  | Reset
  | Prepare
  | Refresh
  | Enable
  | RefreshA
  | RefreshB
  | ApplyMode Word32
  | FlagA
  | FlagB Word32
  | FlagC
  | Select29
  | Select30
  | Prepare2a
  | EditPrepare
  | EditOpen
  | BufferCopy Word32
  | BufferClear
  | WordsClear
  | SetMode
  | WbEnable

data Scenario = Scenario
  { identifier :: String
  , action :: Action
  , frameClass :: FrameClass
  , rowToken :: RowToken
  , frameNode :: NodeId
  , queryAnswer :: Word32
  , session :: Word32
  , modeD0 :: Word32
  , modeE8 :: Word32
  , currentCode :: Word32
  , wbReady :: Word32
  , initialCells :: [(CellAddress, Word32)]
  , transitionedFrame :: (NodeId, FrameClass, RowToken)
  , transitionChoice :: Maybe Word32
  , transitionAnswer :: Word32
  , expectedReturn :: Word32
  , expectedState :: StateWord
  , expectedCells :: [(CellAddress, Word32)]
  , expectedEffects :: [Effect]
  }

base :: String -> Action -> FrameClass -> RowToken -> Scenario
base name act cls tok = Scenario name act cls tok (NodeId 0x72) 0 0 0 0 0 0 []
  (NodeId 0x61, FrameClass 5, RowToken 0x7d) Nothing 0xffffffff 0xffffffff (StateWord 0) [] []

blink :: [(CellAddress, Word32)]
blink = [(CellAddress 0x009504a8, 1)]

setState :: Word32 -> Scenario -> Scenario
setState next s = s { expectedState = StateWord next, expectedCells = blink,
  expectedEffects = [Open Enter Accept, Notify (Notification 0x92a)] }

scenarios :: [Scenario]
scenarios =
  [ (base "set-denied" Set (FrameClass 0x267) (RowToken 2)) { queryAnswer = 2, expectedReturn = 0 }
  , (base "set-root" Set (FrameClass 5) (RowToken 0x29)) { frameNode = NodeId 0, expectedEffects = [Open Enter Accept] }
  , (base "set-generic" Set (FrameClass 0x267) (RowToken 2)) { expectedCells = blink, expectedEffects = [Open Exit Accept, Notify (Notification 0x930)] }
  , (base "set-post-class5-token35" Set (FrameClass 0x267) (RowToken 2)) { transitionedFrame = (NodeId 1, FrameClass 5, RowToken 0x35), expectedEffects = [Open Exit Accept] }
  , (base "set-post-e8" Set (FrameClass 0x267) (RowToken 2)) { transitionedFrame = (NodeId 1, FrameClass 0xe8, RowToken 0), expectedEffects = [Open Exit Accept] }
  , (base "set-generic-nonexit" Set (FrameClass 0x270) (RowToken 0)) { transitionAnswer = 0x5d, expectedReturn = 0x5d, expectedEffects = [Open Exit Accept] }
  , (base "set-f5" Set (FrameClass 0xf5) (RowToken 0)) { expectedEffects = [Open Exit Accept] }
  , setState 3 (base "set-184" Set (FrameClass 0x184) (RowToken 0))
  , setState 1 (base "set-141" Set (FrameClass 0x141) (RowToken 0x1b))
  , (base "set-141-mode2" Set (FrameClass 0x141) (RowToken 0x1b)) { modeE8 = 1, initialCells = [(CellAddress 0x00956934, 2)], expectedCells = blink, expectedEffects = [Open Exit Accept, Notify (Notification 0x930)] }
  , setState 4 (base "set-26" Set (FrameClass 0x26) (RowToken 0))
  , (setState 0x29 (base "set-19-zero" Set (FrameClass 0x19) (RowToken 0))) { expectedCells = (CellAddress 0x0095047c, 0) : blink }
  , (setState 0x29 (base "set-19-one" Set (FrameClass 0x19) (RowToken 1))) { expectedCells = (CellAddress 0x0095047c, 1) : blink }
  , (base "set-167-zero" Set (FrameClass 0x167) (RowToken 0)) { expectedState = StateWord 0x20, expectedCells = blink, expectedEffects = [Open Enter Accept, BufferClear, WordsClear, Notify (Notification 0x92a)] }
  , (base "set-167-zero-result2" Set (FrameClass 0x167) (RowToken 0)) { transitionAnswer = 2, expectedReturn = 2, expectedEffects = [Open Enter Accept] }
  , (base "set-167-copy-zero" Set (FrameClass 0x167) (RowToken 0)) { transitionChoice = Just 1, expectedState = StateWord 0x20, expectedCells = (CellAddress 0x00954cf4, 1) : blink, expectedEffects = [Open Enter Accept, BufferCopy 0x00952036, WordsClear, Notify (Notification 0x92a)] }
  , (base "set-167-copy-nonzero" Set (FrameClass 0x167) (RowToken 0)) { transitionChoice = Just 1, currentCode = 7, expectedState = StateWord 0x20, expectedCells = (CellAddress 0x00954cf4, 1) : blink, expectedEffects = [Open Enter Accept, BufferCopy 0x0095203a, WordsClear, Notify (Notification 0x92a)] }
  , (base "set-167-one" Set (FrameClass 0x167) (RowToken 1)) { expectedState = StateWord 0x26, expectedCells = (CellAddress 0x00954cf4, 1) : blink, expectedEffects = [EditPrepare, EditOpen, Notify (Notification 0x92a)] }
  , (base "set-167-two" Set (FrameClass 0x167) (RowToken 2)) { expectedState = StateWord 0x26, expectedCells = (CellAddress 0x00954cf4, 2) : blink, expectedEffects = [EditPrepare, EditOpen, Notify (Notification 0x92a)] }
  , (base "set-ee-mode" Set (FrameClass 0xee) (RowToken 0)) { modeD0 = 1, expectedState = StateWord 9, expectedEffects = [Open Enter Accept] }
  , (base "set-ee-normal" Set (FrameClass 0xee) (RowToken 0)) { expectedCells = blink, expectedEffects = [Open Exit Accept, Notify (Notification 0x930)] }
  , (base "set-ed-three" Set (FrameClass 0xed) (RowToken 3)) { expectedEffects = [Open Enter Accept] }
  , (base "set-ed-mode" Set (FrameClass 0xed) (RowToken 0)) { modeD0 = 1, expectedState = StateWord 9, expectedEffects = [Open Enter Accept] }
  , (base "set-ed-ready" Set (FrameClass 0xed) (RowToken 1)) { wbReady = 1, expectedReturn = 1, expectedState = StateWord 0xe, expectedEffects = [Postlude, WbEnable, Prepare, Refresh, Notify (Notification 0x135)] }
  , (base "set-node5b-zero" Set (FrameClass 0xdf) (RowToken 0)) { frameNode = NodeId 0x5b, modeD0 = 1, expectedEffects = [Open Direct Accept] }
  , (base "set-node5b-one" Set (FrameClass 0xdf) (RowToken 1)) { frameNode = NodeId 0x5b, modeD0 = 1, expectedReturn = 1, expectedEffects = [Open Direct Accept, ApplyMode 2, FlagA, FlagB 0, FlagC] }
  , (base "set-node5b-two" Set (FrameClass 0xdf) (RowToken 2)) { frameNode = NodeId 0x5b, modeD0 = 1, expectedReturn = 1, expectedEffects = [Open Direct Accept, ApplyMode 2, FlagA, FlagB 1, FlagC] }
  , (base "set-node5b-other" Set (FrameClass 0xe0) (RowToken 0)) { frameNode = NodeId 0x5b, modeD0 = 1, expectedReturn = 0 }
  , (base "set-mode47" Set (FrameClass 0x47) (RowToken 0)) { initialCells = [(CellAddress 0x00954500, 1)], expectedReturn = 1, expectedCells = [(CellAddress 0x009537f8, 1), (CellAddress 0x00950108, 1)], expectedEffects = [SetMode, ApplyMode 2] }
  , (base "set-mode-c-other" Set (FrameClass 0x267) (RowToken 0)) { initialCells = [(CellAddress 0x00954500, 1)], expectedCells = blink, expectedEffects = [Open Enter Accept, Notify (Notification 0x92b)] }
  ]
  ++ [ (base name Set (FrameClass 0x267) (RowToken 0)) { initialCells = [(CellAddress address, 1)], expectedEffects = [Open Direct Accept] }
     | (name, address) <- [("set-activation", 0x009544f8), ("set-mode-a", 0x00950474), ("set-mode-b", 0x00950478)] ]
  ++ [ setState next (base ("set-class5-state-" ++ show tok) Set (FrameClass 5) (RowToken tok))
     | (tok, next) <- [(0x1e,9),(8,10),(0x17,2),(0x46,0x10),(0x68,0x10),(0xa1,0x10),(0x28,0x15),(0x2d,3),(0xd,0x14),(0x25,0x12),(0x7b,0x13)] ]
  ++ [ (base ("set-class5-reset-" ++ show tok) Set (FrameClass 5) (RowToken tok)) { expectedEffects = [Open Enter Accept, Reset] }
     | tok <- [0x32,0x33,0x36,0x37,0x38,0x7c] ]
  ++ [ (setState 0x2d (base "set-class5-7c-mode" Set (FrameClass 5) (RowToken 0x7c))) { modeE8 = 1, expectedEffects = [Open Enter Accept, Notify (Notification 0x92c)] }
     , (base "set-class5-wb" Set (FrameClass 5) (RowToken 0x1c)) { expectedCells = blink, expectedEffects = [Open Enter Accept, Notify (Notification 0x92c)] }
     , (base "set-class5-f5" Set (FrameClass 5) (RowToken 0x14)) { expectedEffects = [Open Enter Accept] }
     , (base "set-class5-dialog" Set (FrameClass 5) (RowToken 0x2c)) { expectedReturn = 1, expectedState = StateWord 0xd, expectedEffects = [Prepare, Refresh, Notify (Notification 0x13d), Postlude] }
     , (base "set-class5-enable" Set (FrameClass 5) (RowToken 0x99)) { modeE8 = 1, expectedReturn = 1, expectedCells = [(CellAddress 0x0095201c, 1), (CellAddress 0x00950104, 1)], expectedEffects = [Enable, RefreshA, RefreshB, ApplyMode 1] }
     , (setState 0x29 (base "set-class5-58" Set (FrameClass 5) (RowToken 0x58))) { modeD0 = 1 }
     , (setState 0x29 (base "set-class5-5a" Set (FrameClass 5) (RowToken 0x5a))) { modeE8 = 1, expectedCells = (CellAddress 0x0095047c, 1) : blink }
     , (setState 0x2a (base "set-class5-5b" Set (FrameClass 5) (RowToken 0x5b))) { modeE8 = 1 }
     , (base "set-class5-29" Set (FrameClass 5) (RowToken 0x29)) { expectedReturn = 2, expectedEffects = [Select29] }
     , (base "set-class5-30" Set (FrameClass 5) (RowToken 0x30)) { expectedReturn = 2, expectedEffects = [Select30] }
     , (base "set-class5-51" Set (FrameClass 5) (RowToken 0x51)) { currentCode = 1, expectedEffects = [Prepare2a, Open Direct Accept] }
     , (base "set-class5-generic" Set (FrameClass 5) (RowToken 0x7d)) { expectedCells = blink, expectedEffects = [Open Enter Accept, Notify (Notification 0x92b)] }
     ]
  ++ [ (base name Confirm (FrameClass 0) (RowToken 0)) { queryAnswer = q, session = se, transitionAnswer = result, expectedReturn = ret, expectedEffects = effects }
     | (name,q,se,result,ret,effects) <-
       [("confirm-denied",1,0,2,0,[]),("confirm-session2",2,2,2,0,[]),
        ("confirm-query-forward",2,1,2,2,[ConfirmEditor 2,Postlude]),
        ("confirm-zero",0,0,0xffffffff,0xffffffff,[ConfirmEditor 0,Postlude]),
        ("confirm-other",0,1,0x5d,0x5d,[ConfirmEditor 0])]
     ]
  ++ [ (base name Leave (FrameClass cls) (RowToken tok)) { frameNode = NodeId n, modeD0 = md,
         initialCells = cells, expectedEffects = effects, expectedCells = writes }
     | (name,cls,tok,n,md,cells,effects,writes) <-
       [("leave-root",0,0,0,0,[],[Open Direct Accept],[]),
        ("leave-class5",5,0x1c,1,0,[],[Open Direct Accept],[]),
        ("leave-node5b",0xed,0,0x5b,0,[],[Open Direct Accept],[]),
        ("leave-activation",0x267,0,0x72,0,[(CellAddress 0x009544f8,1)],[Open Direct Cancel],[]),
        ("leave-mode-a-ee",0xee,0,0x72,0,[(CellAddress 0x00950474,1)],[Open Exit Accept],[]),
        ("leave-mode-b-ed",0xed,0,0x72,0,[(CellAddress 0x00950478,1)],[Open Direct Cancel],[]),
        ("leave-activation-ed-mode",0xed,0,0x72,1,[(CellAddress 0x009544f8,1)],[Open Direct Accept],[]),
        ("leave-167",0x167,0,0x72,0,[],[Open Exit Accept,Notify (Notification 0x933)],blink),
        ("leave-f5",0xf5,0,0x72,0,[],[Open Exit Cancel],[]),
        ("leave-ed-mode",0xed,0,0x72,1,[],[Open Direct Accept],[]),
        ("leave-ee",0xee,0,0x72,0,[],[Open Exit Accept],[]),
        ("leave-ed",0xed,0,0x72,0,[],[Open Exit Cancel],[]),
        ("leave-fe",0xfe,0,0x72,0,[],[Open Exit Cancel],[]),
        ("leave-generic",0x267,0,0x72,0,[],[Open Exit Cancel,Notify (Notification 0x933)],blink)]
     ]
  ++ [ (base ("leave-post-node-" ++ show n) Leave (FrameClass 0x267) (RowToken 0)) { transitionedFrame = (NodeId n, FrameClass 5, RowToken 0), expectedEffects = [Open Exit Cancel] }
     | n <- [0x60,0x37,0x61] ]
  ++ [ (base "leave-denied" Leave (FrameClass 0x267) (RowToken 0)) { queryAnswer = 1, expectedReturn = 0 } ]

entry :: Action -> (Word32, Int, String)
entry Set = (0xc72e10, 0x1498, "rb_ui_set")
entry Confirm = (0xc75ed4, 0x64, "rb_ui_editor_confirm")
entry Leave = (0xc76650, 0x414, "rb_ui_leave")

cfg, effectsAddress, ramBase, exitAddress, stackAddress :: Word32
cfg = 0x00960800
effectsAddress = 0x00960100
ramBase = 0x00940000
exitAddress = 0x00962000
stackAddress = 0x00970000

patch :: B.ByteString -> [(Int, B.ByteString)] -> B.ByteString
patch = foldl (\bytes (offset, value) -> B.take offset bytes <> value <> B.drop (offset + B.length value) bytes)

cellImage :: Scenario -> Bool -> B.ByteString
cellImage s expected = patch (B.replicate 0x20000 0) $
  [(fromIntegral (address - ramBase), word32le value) | (CellAddress address,value) <- initialCells s ++ if expected then expectedCells s else []]
  ++ [(0x14d08, word32le (session s)), (0x12678, B.pack [3,0]), (0x12670, B.pack [5,0])]

kindWord :: StageKind -> Word32
kindWord Direct = 2
kindWord Enter = 14
kindWord Exit = 15
idWord :: StageId -> Word32
idWord Accept = 29
idWord Cancel = 26

effectWords :: Effect -> [Word32]
effectWords effect = case effect of
  Open k i -> [1,kindWord k,idWord i,0]
  Notify (Notification n) -> [2,n,0,0]
  Postlude -> [3,9,0,0]
  ConfirmEditor q -> [4,q,0,0]
  Reset -> [5,0,0,0]
  Prepare -> [6,0,0,0]
  Refresh -> [7,0,0,0]
  Enable -> [8,1,0,0]
  RefreshA -> [9,0,0,0]
  RefreshB -> [10,0,0,0]
  ApplyMode v -> [11,v,0,0]
  FlagA -> [12,1,0,0]
  FlagB v -> [13,v,0,0]
  FlagC -> [14,1,0,0]
  Select29 -> [15,1,0,0]
  Select30 -> [16,1,0,0]
  Prepare2a -> [17,0x2a,0,0]
  EditPrepare -> [18,0x28,0,0]
  EditOpen -> [19,0x28,0,0]
  BufferCopy source -> [20,0x009543ae,source,0x19]
  BufferClear -> [21,0x009543ae,0,0x19]
  WordsClear -> [22,0x00952690,0,0xc]
  SetMode -> [23,4,0,0]
  WbEnable -> [24,1,0,0]

-- Retained services expose their ABI effects; frame reads are unlogged.
logEntry :: Int -> Word32
logEntry arity = 0x00962400 + fromIntegral arity * 0x100

logFunction :: Word32 -> Int -> [Word32]
logFunction ident arity = [0xe59f3004,0xe59fc004,0xe12fff1c,ident,logEntry arity]

logBody :: Int -> [Word32]
logBody arity =
  [0xe92d0030,0xe59fc028,0xe59c4000,0xe2845010,0xe58c5000,0xe08c4004,
   0xe5843004,0xe3a05000]
  ++ [if arity >= n then 0xe5840008 + fromIntegral (n-1) * 0x1004 else 0xe5845008 + fromIntegral (n-1) * 4 | n <- [1..3]]
  ++ [0xe8bd0030,0xe12fff1e,effectsAddress]

loadFunction :: Word32 -> [Word32]
loadFunction address = [0xe59f0004,0xe5900000,0xe12fff1e,address]

services :: [(String, Word32, [Word32])]
services =
  [("query",0x0063a0f8,[0xe3500053,0x13a000ff,0x112fff1e] ++ loadFunction cfg), ("node",0x0061d154,loadFunction (cfg+16)),
   ("class",0x0061d1f4,loadFunction (cfg+20)), ("token",0x0061d244,loadFunction (cfg+24)),
   ("mode-d0",0x006394d0,loadFunction (cfg+28)), ("mode-e8",0x006394e8,loadFunction (cfg+32)),
   ("wb-ready",0x0064257c,loadFunction (cfg+36)), ("current",0x0061c93c,loadFunction (cfg+40)),
   ("transition",0x00c12aac,
     [0xe59fc02c,0xe59c002c,0xe58c0010,0xe59c0030,0xe58c0014,0xe59c0034,0xe58c0018,
      0xe59c003c,0xe3500000,0x159f1010,0x15810000,0xe59c0038,0xe12fff1e,cfg,0,0x00954cf4])]
  ++ [("log-" ++ show arity,logEntry arity,logBody arity) | arity <- [0..3]]
  ++ [(name,address,logFunction tag arity) | (name,address,tag,arity) <-
     [("stage",0x00d3d858,1,2),("redraw",0x00d38d98,2,1),("postlude",0x00d38f34,3,1),
      ("confirm",0x00d4ad38,4,1),("reset",0x00635b60,5,1),("prepare",0x00d19cfc,6,0),
      ("refresh",0x00d3c9d8,7,0),("enable",0x00637894,8,1),("refresh-a",0x00639438,9,0),
      ("refresh-b",0x00639468,10,0),("apply",0x00d350c4,11,1),("flag-a",0x006357dc,12,1),
      ("flag-b",0x00635800,13,1),("flag-c",0x00635824,14,1),("select29",0x00d425cc,15,1),
      ("select30",0x00d42450,16,1),("prepare2a",0x00d3bc34,17,1),("edit-prepare",0x00d45ce4,18,1),
      ("edit-open",0x00d45cf8,19,1),("copy",0x00722a50,20,3),("clear",0x00722178,21,3),
      ("word-clear",0x007221c8,22,3),("set-mode",0x006378dc,23,1),("wb-enable",0x00638010,24,1)]]

registers :: [(Register, Word32)]
registers = [(R0,0),(R1,0),(R2,0),(R3,0),(R4,0xa5a50004),(R5,0xa5a50005),(R6,0xa5a50006),
  (R7,0xa5a50007),(R8,0xa5a50008),(R9,0xa5a50009),(R10,0xa5a5000a),(R11,0xa5a5000b),
  (R12,0),(Sp,stackAddress+0x3ff0),(Lr,exitAddress),(Cpsr,0x10)]

data ModelArea = FrameModel | ModeModel | BufferModel | TransitionModel
  | PrepareModel | MenuModel | EditorModel | ControlModel
  deriving (Eq, Ord)

data ModelBody = ModelBody
  { modelId :: RegionId
  , modelAddress :: GuestAddress
  , modelCode :: [Word32]
  }

modelArea :: GuestAddress -> ModelArea
modelArea address = case [area | (low,high,area) <- ranges, low <= guestAddress address, guestAddress address < high] of
  [area] -> area
  _ -> error ("unassigned retained model address " ++ hexAddress address)
  where
    ranges = [(0x610000,0x620000,FrameModel),(0x630000,0x650000,ModeModel),
      (0x720000,0x730000,BufferModel),(0xc10000,0xc20000,TransitionModel),
      (0xd10000,0xd20000,PrepareModel),(0xd30000,0xd40000,MenuModel),
      (0xd40000,0xd50000,EditorModel),(0x960000,0x970000,ControlModel)]

areaId :: ModelArea -> RegionId
areaId area = RegionId $ case area of
  FrameModel -> "model-frame"
  ModeModel -> "model-mode"
  BufferModel -> "model-buffer"
  TransitionModel -> "model-transition"
  PrepareModel -> "model-prepare"
  MenuModel -> "model-menu"
  EditorModel -> "model-editor"
  ControlModel -> "model-control"

modelBodies :: [ModelBody]
modelBodies =
  [ModelBody (RegionId name) (GuestAddress (fromIntegral address)) code
  | (name,address,code) <- ("exit",exitAddress,[0xe12fff1e]) : services]

modelRegions :: [CaseRegion]
modelRegions = map packArea (M.toAscList grouped)
  where
    grouped = M.fromListWith (++) [(modelArea (modelAddress body),[body]) | body <- modelBodies]
    packArea (area,bodies) =
      let low = minimum (map (guestAddress . modelAddress) bodies)
          high = maximum [guestAddress (modelAddress body) + fromIntegral (length (modelCode body)*4) | body <- bodies]
          size = fromIntegral (high-low)
          bytes = patch (B.replicate size 0)
            [(fromIntegral (guestAddress (modelAddress body)-low),assemble (modelCode body)) | body <- bodies]
      in mkRegion (areaId area) (GuestAddress low) size ReadExecute bytes

ownedClosure :: M.Map String Symbol -> B.ByteString -> GuestAddress -> [(GuestAddress, Word32)]
ownedClosure symbols kernel root = M.toAscList (go M.empty [root])
  where
    imageBase = symbolAddress symbols "__rb_image_start"
    imageEnd = guestAddress imageBase + fromIntegral (B.length kernel)
    inventory = M.fromListWith max
      [(GuestAddress (fromIntegral address),size) | Symbol address size <- M.elems symbols,
        size /= 0, guestAddress imageBase <= fromIntegral address, fromIntegral address < imageEnd]
    retained = S.fromList [target | (_,address,_) <- services,
      let target = GuestAddress (fromIntegral address), modelArea target /= ControlModel]
    go visited [] = visited
    go visited (address:rest)
      | M.member address visited = go visited rest
      | otherwise = case M.lookup address inventory of
          Nothing -> error ("owned branch is not a function entry: " ++ hexAddress address)
          Just size ->
            let targets = concatMap (branches address size) [0,4..fromIntegral size-4]
                next = [target | target <- targets, not (inside address size target)]
            in go (M.insert address size visited) (concatMap resolve next ++ rest)
    inside :: GuestAddress -> Word32 -> GuestAddress -> Bool
    inside start size target =
      guestAddress start <= guestAddress target && guestAddress target < guestAddress start + fromIntegral size
    resolve target
      | inside imageBase (fromIntegral (B.length kernel)) target = case M.lookupLE target inventory of
          Just (start,size) | inside start size target -> [start]
          _ -> error ("unresolved internal branch: " ++ hexAddress target)
      | S.member target retained = []
      | otherwise = error ("undeclared retained branch: " ++ hexAddress target)
    wordAt address offset = readWordAt kernel (fromIntegral (guestAddress address-guestAddress imageBase)+offset)
    branches address size offset
      | size == 0 || size `mod` 4 /= 0 || guestAddress address + fromIntegral size > imageEnd =
          error ("invalid owned function extent: " ++ hexAddress address)
      | instruction .&. 0x0e000000 == 0x0a000000 =
          if instruction `shiftR` 28 == 15 then unresolved else
            let immediate = instruction .&. 0x00ffffff
                displacement = if testBit immediate 23 then (immediate-0x01000000)*4 else immediate*4
            in [GuestAddress (fromIntegral (guestWord pc+8+displacement))]
      | instruction .&. 0x0ffffff0 `elem` [0x012fff10,0x012fff30] =
          if instruction .&. 0x0fffffff == 0x012fff1e then [] else constantTarget
      | instruction .&. 0x0fffffff == 0x01a0f00e = []
      | instruction .&. 0x0e100000 == 0x08100000 && testBit instruction 15 =
          if instruction .&. 0x0fff0000 == 0x08bd0000 then [] else unresolved
      | instruction .&. 0x0c100000 == 0x04100000 && destination == 15 =
          if instruction .&. 0x0fffffff == 0x049df004 then [] else unresolved
      | instruction .&. 0x0c000000 == 0 && destination == 15
          && ((instruction `shiftR` 21) .&. 15) `notElem` [8,9,10,11] = unresolved
      | otherwise = []
      where
        instruction = wordAt address offset
        pc = GuestAddress (guestAddress address + fromIntegral offset)
        destination = (instruction `shiftR` 12) .&. 15
        unresolved = error ("unresolved owned indirect transfer at " ++ hexAddress pc)
        constantTarget
          | offset < 8 = unresolved
          | low .&. 0xfff00000 /= 0xe3000000 || high .&. 0xfff00000 /= 0xe3400000 = unresolved
          | (low `shiftR` 12) .&. 15 /= register || (high `shiftR` 12) .&. 15 /= register = unresolved
          | otherwise = [GuestAddress (fromIntegral (immediate16 low + immediate16 high*0x10000))]
          where
            low = wordAt address (offset-8)
            high = wordAt address (offset-4)
            register = instruction .&. 15
            immediate16 word = ((word `shiftR` 4) .&. 0xf000) + (word .&. 0xfff)

checkDisjoint :: String -> [(RegionId, GuestAddress, Int)] -> IO ()
checkDisjoint label intervals = forM_ (zip ordered (drop 1 ordered)) $ \((left,start,size),(right,next,_)) ->
  when (guestAddress start + fromIntegral size > guestAddress next)
    (error (label ++ " overlap: " ++ regionName left ++ " and " ++ regionName right))
  where ordered = sortOn (\(_,address,_) -> address) intervals

writeCheckedCase :: FilePath -> CaseSpec -> IO FilePath
writeCheckedCase directory spec = do
  let bodyBounds = [(modelId body,modelAddress body,length (modelCode body)*4) | body <- modelBodies]
      containers = regionBounds (map caseRegion modelRegions)
      executable = regionBounds [region | CaseRegion region ReadExecute <- caseRegions spec]
      contains :: (RegionId, GuestAddress, Int) -> (RegionId, GuestAddress, Int) -> Bool
      contains (_,start,size) (_,address,count) =
        guestAddress start <= guestAddress address && guestAddress address + fromIntegral count <= guestAddress start + fromIntegral size
  checkDisjoint "retained model bodies" bodyBounds
  checkDisjoint "retained model containers" containers
  checkDisjoint ("case " ++ caseId spec ++ " regions") (regionBounds (map caseRegion (caseRegions spec)))
  forM_ bodyBounds $ \body@(name,_,_) ->
    unless (any (`contains` body) containers) (error ("model body outside container: " ++ regionName name))
  forM_ (caseFunctions spec) $ \(address,size) ->
    unless (any (\container -> contains container (RegionId "function",address,fromIntegral size)) executable)
      (error ("function outside executable region: " ++ hexAddress address))
  path <- writeCase directory spec
  _ <- loadCase path
  pure path

caseSpec :: M.Map String Symbol -> B.ByteString -> B.ByteString -> Word32 -> Scenario -> Bool -> CaseSpec
caseSpec symbols kernel stock stockBase s owned = CaseSpec
  { caseId = identifier s ++ if owned then "-owned" else "-original"
  , caseProvenance = "X-M1 1.50 original image-5 action versus owned lifecycle; synthetic retained-service ABI effects and frame changes, literal return/state/effect expectations"
  , caseExclusion = "No device, dispatcher, scheduling, interrupt or retained-service implementation qualification; buffer effects stop at the retained ABI; original bytes supplied privately"
  , caseEntry = if owned then symbolAddress symbols symbol else GuestAddress (fromIntegral address)
  , caseExit = GuestAddress (fromIntegral exitAddress)
  , caseRegions = common ++ if owned
      then [mkRegion (RegionId "owned") (symbolAddress symbols "__rb_image_start") (B.length kernel) ReadExecute kernel]
      else [mkRegion (RegionId "original") (GuestAddress (fromIntegral address)) size ReadExecute original]
  , caseFunctions = serviceFunctions ++ if owned then ownedFunctions else [(GuestAddress (fromIntegral address),fromIntegral size)]
  , caseInitial = registers
  , caseExpectedStatus = Returned
  , caseExpectedRegisters = (R0,expectedReturn s) : [(r,v) | (r,v) <- registers, r `elem` [R4,R5,R6,R7,R8,R9,R10,R11,Sp]] ++ [(Pc,exitAddress)]
  , caseExpectedMemory = [(RegionId "ram",cellImage s True),(RegionId "state",stateBytes), (RegionId "effects",effectBytes)]
  }
  where
    (address,size,symbol) = entry (action s)
    original = B.take size (B.drop (fromIntegral (address-stockBase)) stock)
    FrameClass cls = frameClass s
    RowToken tok = rowToken s
    NodeId n = frameNode s
    (NodeId pn,FrameClass pc,RowToken pt) = transitionedFrame s
    StateWord st = expectedState s
    stateBytes = patch (B.replicate 0x40 0) [(0x1e,B.pack [fromIntegral st,fromIntegral (st `div` 256)])]
    effectData = assemble (concatMap effectWords (expectedEffects s))
    effectBytes = word32le (fromIntegral (B.length effectData)) <> effectData <> B.replicate (0x400-4-B.length effectData) 0
    config = assemble [queryAnswer s,0,0,0,n,cls,tok,modeD0 s,modeE8 s,wbReady s,currentCode s,pn,pc,pt,transitionAnswer s,maybe 0 id (transitionChoice s)]
    common =
      [mkRegion (RegionId "ram") (GuestAddress (fromIntegral ramBase)) 0x20000 ReadWrite (cellImage s False),
       mkRegion (RegionId "state") (GuestAddress 0x01467600) 0x40 ReadWrite (B.replicate 0x40 0),
       mkRegion (RegionId "effects") (GuestAddress (fromIntegral effectsAddress)) 0x400 ReadWrite (B.replicate 0x400 0),
       mkRegion (RegionId "config") (GuestAddress (fromIntegral cfg)) (B.length config) ReadWrite config,
       mkRegion (RegionId "stack") (GuestAddress (fromIntegral stackAddress)) 0x4000 ReadWrite (B.replicate 0x4000 0)]
      ++ modelRegions
    serviceFunctions = [(GuestAddress (fromIntegral a),fromIntegral (length code*4)) | (_,a,code) <- services]
    ownedFunctions = ownedClosure symbols kernel (symbolAddress symbols symbol)

data Mismatch
  = StatusMismatch
  | RegisterMismatch Register
  | RegionMismatch RegionId
  | CpsrControlMismatch
  deriving (Eq)

mismatchLabel :: Mismatch -> String
mismatchLabel StatusMismatch = "status"
mismatchLabel (RegisterMismatch register) = registerLabel register
mismatchLabel (RegionMismatch region) = regionName region
mismatchLabel CpsrControlMismatch = "cpsr-control"

problems :: CaseSpec -> NativeObservation -> [Mismatch]
problems spec observation =
  [StatusMismatch | nativeStatus observation /= Returned]
  ++ [RegisterMismatch r | (r,v) <- caseExpectedRegisters spec, registerValue observation r /= v]
  ++ [RegionMismatch region | (region,bytes) <- caseExpectedMemory spec, regionBytesOf observation region /= bytes]
  ++ [CpsrControlMismatch | cpsrControl (registerValue observation Cpsr) /= 0x10]

data GuestFault = ReturnFault | AbiFault | StateFault

guestFaultCode :: GuestAddress -> GuestFault -> [Word32]
guestFaultCode entryAddress fault = case fault of
  ReturnFault -> registerFault 0xe2200001
  AbiFault -> registerFault 0xe2244001
  StateFault ->
    [0xe92d5000,0xe59fc020,0xe12fff3c,0xe92d0003,0xe59fc018,0xe1dc10b0,
     0xe2211001,0xe1cc10b0,0xe8bd0003,0xe8bd5000,0xe12fff1e,guestWord entryAddress,0x0146761e]
  where
    registerFault instruction =
      [0xe92d5000,0xe59fc00c,0xe12fff3c,instruction,0xe8bd5000,0xe12fff1e,guestWord entryAddress]

guestFaultSpecs :: CaseSpec -> [(CaseSpec, Mismatch)]
guestFaultSpecs reference =
  [(fault name kind,expected) | (name,kind,expected) <-
    [("return",ReturnFault,RegisterMismatch R0),("abi",AbiFault,RegisterMismatch R4),
     ("state",StateFault,RegionMismatch (RegionId "state"))]]
  where
    fault name kind =
      let code = guestFaultCode (caseEntry reference) kind
      in reference
        { caseId = "control-" ++ name
        , caseProvenance = "AAPCS-aligned guest wrapper calls real owned SET, restores LR/SP and changes one observed output"
        , caseEntry = GuestAddress 0x00963000
        , caseRegions = caseRegions reference
            ++ [mkRegion (RegionId "fault") (GuestAddress 0x00963000) (length code*4) ReadExecute (assemble code)]
        , caseFunctions = (GuestAddress 0x00963000,fromIntegral (length code*4)) : caseFunctions reference
        }

fetchControlSpecs :: B.ByteString -> Word32 -> CaseSpec -> [(CaseSpec, GuestAddress)]
fetchControlSpecs stock stockBase reference =
  [(fault address,GuestAddress (fromIntegral address)) | address <- [0xc72e10,0xc75ed4,0xc76650]]
  where
    fault address = reference
      { caseId = "control-fetch-" ++ show address
      , caseProvenance = "negative control for forbidden original-action fetch observation"
      , caseEntry = GuestAddress 0x00963000
      , caseRegions = caseRegions reference
          ++ [mkRegion (RegionId "fault") (GuestAddress 0x00963000) 12 ReadExecute
                (assemble [0xe59fc000,0xe12fff1c,address]),
              mkRegion (RegionId "forbidden-original") (GuestAddress (fromIntegral address)) 4 ReadOnly
                (B.take 4 (B.drop (fromIntegral (address-stockBase)) stock))]
      , caseFunctions = (GuestAddress 0x00963000,12) : caseFunctions reference
      }

controlReport :: CaseSpec -> NativeObservation -> Bool -> Value
controlReport spec observation rejected = object
  ["id" .= caseId spec,
   "rejected" .= rejected,
   "producer_status" .= runnerStatusLabel (nativeStatus observation),
   "producer_engine_error" .= engineError (nativeEngineError observation),
   "producer_pc" .= hexAddress (GuestAddress (fromIntegral (registerValue observation Pc))),
   "mismatches" .= map mismatchLabel (problems spec observation)]

main :: IO ()
main = do
  args <- getArgs
  build <- required args "--build"
  out <- required args "--out"
  id5Path <- required args "--id5"
  id5Base <- parseAddress "--id5-base" <$> required args "--id5-base"
  symbolRows <- lines <$> readFile (build </> "kernel.symbols")
  let names = "__rb_image_start" : [name | [_,_,size,"FUNC",_,_,_,name] <- map words symbolRows, size /= "0"]
  symbols <- parseSymbols names (build </> "kernel.syms")
  kernel <- B.readFile (build </> "kernel-text.bin")
  stock <- B.readFile id5Path
  unless (id5Base <= 0xc72e10 && fromIntegral (0xc76a64-id5Base) <= B.length stock) (error "original image-5 actions are truncated")
  unless (id5Base == 0x00bf1a00 && sha256Hex (sha256 stock) == "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a")
    (error "original image-5 identity does not match X-M1 1.50")
  paths <- forM scenarios $ \s -> forM [False,True] $ \owned -> do
    let spec = caseSpec symbols kernel stock id5Base s owned
    path <- writeCheckedCase (out </> caseId spec) spec
    pure (spec,path)
  let referenceScenario = (base "controls" Set (FrameClass 0x267) (RowToken 2))
        { expectedCells = blink, expectedEffects = [Open Exit Accept, Notify (Notification 0x930)] }
      reference = caseSpec symbols kernel stock id5Base referenceScenario True
  referencePath <- writeCheckedCase (out </> caseId reference) reference
  faultPaths <- forM (guestFaultSpecs reference) $ \(spec,expected) -> do
    path <- writeCheckedCase (out </> caseId spec) spec
    pure (spec,path,expected)
  fetchPaths <- forM (fetchControlSpecs stock id5Base reference) $ \(spec,target) -> do
    path <- writeCheckedCase (out </> caseId spec) spec
    pure (spec,path,target)
  if "--execute" `elem` args then do
    runner <- required args "--runner"
    referenceObservation <- loadObserved runner referencePath
    let referenceFailures = problems reference referenceObservation
    faultControls <- forM faultPaths $ \(spec,path,expected) -> do
      observation <- loadObserved runner path
      let rejected = null referenceFailures && problems spec observation == [expected]
      pure (rejected,controlReport spec observation rejected)
    results <- forM paths $ \pair -> forM pair $ \(spec,path) -> do
      observation <- loadObserved runner path
      pure (caseId spec,problems spec observation)
    fetchControls <- forM fetchPaths $ \(spec,path,target) -> do
      observation <- loadObserved runner path
      let rejected = nativeStatus observation == FetchGuard && registerValue observation Pc == guestWord target
      pure (rejected,controlReport spec observation rejected)
    let flat = concat results
        controls = faultControls ++ fetchControls
    B.writeFile (out </> "controller-lifecycle-results.json") (encodeCanonical (object
      ["cases" .= [object ["id" .= name,"mismatches" .= map mismatchLabel failures] | (name,failures) <- flat],
       "reference_mismatches" .= map mismatchLabel referenceFailures,
       "controls" .= map snd controls]))
    when (not (null referenceFailures) || any (not . null . snd) flat || any (not . fst) controls) exitFailure
  else putStrLn ("emitted " ++ show (length scenarios) ++ " original/owned pairs, one owned reference and six guest controls; no execution")
