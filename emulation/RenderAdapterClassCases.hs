{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main where

import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import Data.Word (Word32)
import Emulation.RenderCases (ServiceAddress (..), serviceAddress)
import qualified Emulation.ValuePickerCases as Picker
import FujiAF.JSON (encodeCanonical)
import System.Environment (getArgs)
import qualified TreeListCases as Tree

newtype CaseName = CaseName String
newtype ReturnAddress = ReturnAddress Word32
newtype DrawMode = DrawMode Word32
newtype NodeId = NodeId Word32
newtype SnapshotKind = SnapshotKind Word32
newtype ParentId = ParentId Word32
newtype SelectedId = SelectedId Word32
data DrawFunction = Tree | Picker deriving (Eq, Show)
data NodeOrigin = Stock | Package | Unknown deriving (Eq, Show)
data ModelClass = OwnedTree | OwnedPicker | Retained deriving (Eq, Show)
data RecordedEntry = RecordedEntry
  { caseName :: CaseName, function :: DrawFunction, caller :: ReturnAddress
  , mode :: DrawMode, node :: NodeId, liveKind :: SnapshotKind
  , liveParent :: ParentId, liveSelected :: SelectedId, origin :: NodeOrigin
  , modelClass :: ModelClass
  }

wordAt :: B.ByteString -> Int -> Word32
wordAt bytes offset = sum [fromIntegral (B.index bytes (offset+i))*256^i | i<-[0..3]]

modelEntries :: [RecordedEntry]
modelEntries = map tree Tree.scenarios ++ map picker Picker.pickerCases
  where
    tree c = let Tree.NodeId n=Tree.node c; Tree.DrawMode m=Tree.mode c; snapshot=Tree.snapshot c
             in RecordedEntry (CaseName (Tree.scenarioName c)) Tree (ReturnAddress 0x01167a60)
                  (DrawMode m) (NodeId n) (SnapshotKind (snapshot!!0)) (ParentId (snapshot!!3))
                  (SelectedId (snapshot!!4)) Stock OwnedTree
    picker c = let Picker.NodeId n=Picker.pickerNode c; snapshot=Picker.pickerSnapshot c
                   usesLive=any ((==ServiceAddress 0x61cd14) . serviceAddress) (Picker.pickerCalls c)
               in RecordedEntry (CaseName ("picker-"++Picker.pickerName c)) Picker (ReturnAddress (Picker.pickerReturnAddress c))
                    (DrawMode (Picker.pickerMode c)) (NodeId n) (SnapshotKind (wordAt snapshot 0))
                    (ParentId (wordAt snapshot 12)) (SelectedId (wordAt snapshot 16)) Stock
                    (if usesLive then OwnedPicker else Retained)

boundaryEntries :: [RecordedEntry]
boundaryEntries = concatMap refused [tree,picker,fnPicker] ++
  [ tree {caseName=CaseName ("tree-saved-mode-"++show m),mode=DrawMode m,modelClass=Retained}
  | m<-[5,6,7,9,11,12,13] ] ++
  [ picker {caseName=CaseName ("picker-saved-mode-"++show m),mode=DrawMode m,modelClass=Retained}
  | m<-[7,9,13] ] ++
  [ picker {caseName=CaseName ("picker-skipped-mode-"++show m),mode=DrawMode m,modelClass=Retained}
  | m<-[2,3] ] ++
  [ fnPicker {caseName=CaseName ("fn-picker-saved-or-skipped-mode-"++show m),mode=DrawMode m,modelClass=Retained}
  | m<-[2,3,7,9,13] ] ++
  [ tree {caseName=CaseName ("tree-kind-"++show kind),liveKind=SnapshotKind kind,modelClass=Retained}
  | kind<-[0,2,0xffffffff] ] ++
  [ tree {caseName=CaseName "tree-node25-override",node=NodeId 0x25,liveKind=SnapshotKind 2,liveParent=ParentId 0x27,liveSelected=SelectedId 0}
  , tree {caseName=CaseName "tree-node38-override",node=NodeId 0x38,liveKind=SnapshotKind 2,liveParent=ParentId 0x228,liveSelected=SelectedId 11}
  , tree {caseName=CaseName "tree-node25-parent-near-miss",node=NodeId 0x25,liveKind=SnapshotKind 2,liveParent=ParentId 27,liveSelected=SelectedId 0,modelClass=Retained}
  , tree {caseName=CaseName "tree-node38-parent-near-miss",node=NodeId 0x38,liveKind=SnapshotKind 2,liveParent=ParentId 228,liveSelected=SelectedId 11,modelClass=Retained}
  , tree {caseName=CaseName "tree-node25-selection-near-miss",node=NodeId 0x25,liveKind=SnapshotKind 2,liveParent=ParentId 0x27,liveSelected=SelectedId 1,modelClass=Retained}
  , tree {caseName=CaseName "tree-node38-selection-near-miss",node=NodeId 0x38,liveKind=SnapshotKind 2,liveParent=ParentId 0x228,liveSelected=SelectedId 12,modelClass=Retained}
  ] ++
  [ entry {caseName=CaseName (prefix entry++"-high-mode-"++show m),mode=DrawMode m}
  | entry<-[tree,picker],m<-[14,31,32,0xffffffff] ] ++
  [ picker {caseName=CaseName ("picker-live-mode-"++show m),mode=DrawMode m} | m<-[5,6,11,12] ]
  where
    tree=RecordedEntry (CaseName "tree-boundary") Tree (ReturnAddress 0x01167a60) (DrawMode 0) (NodeId 0x40)
      (SnapshotKind 1) (ParentId 0x224) (SelectedId 9) Stock OwnedTree
    picker=RecordedEntry (CaseName "picker-boundary") Picker (ReturnAddress 0x01168ce4) (DrawMode 0) (NodeId 0x30)
      (SnapshotKind 0x17) (ParentId 0x39) (SelectedId 1) Stock OwnedPicker
    fnPicker=picker {caseName=CaseName "fn-picker-boundary",caller=ReturnAddress 0x0116c588,node=NodeId 0x63}
    prefix entry=case (function entry,caller entry) of (Tree,_)->"tree"; (Picker,ReturnAddress 0x0116c588)->"fn-picker"; _->"picker"
    refused entry=
      [ entry {caseName=CaseName (prefix entry++"-package-node"),node=NodeId 0x400,origin=Package,modelClass=Retained}
      , entry {caseName=CaseName (prefix entry++"-unknown-origin"),origin=Unknown,modelClass=Retained}
      , entry {caseName=CaseName (prefix entry++"-other-caller"),caller=ReturnAddress (case function entry of Tree->0x0116c058;Picker->0x01169308),modelClass=Retained}
      , entry {caseName=CaseName (prefix entry++"-cross-function-caller"),caller=ReturnAddress (case function entry of Tree->0x01168ce4;Picker->0x01167a60),modelClass=Retained}
      ]

entryJSON :: RecordedEntry -> Value
entryJSON entry = object
  [ "id" .= name, "function" .= show (function entry), "caller_return" .= callerWord
  , "mode" .= modeWord, "node" .= nodeWord, "live_kind" .= kindWord
  , "live_parent" .= parentWord, "live_selected" .= selectedWord
  , "origin" .= show (origin entry), "model_class" .= show (modelClass entry)
  , "expected_owned" .= (modelClass entry/=Retained)
  ]
  where
    CaseName name=caseName entry; ReturnAddress callerWord=caller entry; DrawMode modeWord=mode entry
    NodeId nodeWord=node entry; SnapshotKind kindWord=liveKind entry
    ParentId parentWord=liveParent entry; SelectedId selectedWord=liveSelected entry

rustTest :: Int -> RecordedEntry -> String
rustTest index entry = unlines
  [ "#[test]", "fn entry_case_"++show index++"() {"
  , "    let entry = EntryClass { function: DrawFunction::"++show (function entry)++", caller: ReturnAddress("++show callerWord++"),"
  , "        mode: "++show modeWord++", node: "++show nodeWord++", live_kind: "++show kindWord++","
  , "        live_parent: "++show parentWord++", live_selected: "++show selectedWord++", origin: NodeOrigin::"++show (origin entry)++" };"
  , "    assert_eq!(entry.owned(), "++(if modelClass entry==Retained then "false" else "true")++", "++show name++");"
  , "}"
  ]
  where
    CaseName name=caseName entry; ReturnAddress callerWord=caller entry; DrawMode modeWord=mode entry
    NodeId nodeWord=node entry; SnapshotKind kindWord=liveKind entry
    ParentId parentWord=liveParent entry; SelectedId selectedWord=liveSelected entry


newtype SnapshotDepth = SnapshotDepth Word32
data PaneEntry = PaneEntry
  { paneName :: CaseName, paneCaller :: ReturnAddress, paneNode :: NodeId, paneMode :: DrawMode
  , paneDepth :: SnapshotDepth, paneKind :: SnapshotKind, paneOrigin :: NodeOrigin, paneOwned :: Bool
  }

paneEntries :: [PaneEntry]
paneEntries =
  [ base {paneName=CaseName ("pane-live-"++show (m,d,k)),paneMode=DrawMode m,paneDepth=SnapshotDepth d,paneKind=SnapshotKind k}
  | m<-[0,1,3,4,6,8,9,10,12,14,255], d<-[0,1,2], k<-[0,1,2,3,9] ] ++
  [ base {paneName=CaseName ("pane-refused-mode-"++show m),paneMode=DrawMode m,paneOwned=False}
  | m<-[2,5,7,11,13,15,31,256,0xffffffff] ] ++
  [ base {paneName=CaseName ("pane-refused-kind-"++show k),paneKind=SnapshotKind k,paneOwned=False}
  | k<-[4,8,10,0xffffffff] ] ++
  [ base {paneName=CaseName ("pane-refused-depth-"++show d),paneDepth=SnapshotDepth d,paneOwned=False}
  | d<-[3,9,0xffffffff] ] ++
  [ base {paneName=CaseName ("pane-origin-"++show origin_),paneOrigin=origin_,paneOwned=False} | origin_<-[Package,Unknown] ] ++
  [ base {paneName=CaseName "pane-other-caller",paneCaller=ReturnAddress 0x11586e8,paneOwned=False}
  , base {paneName=CaseName "pane-other-node",paneNode=NodeId 0x60,paneOwned=False} ]
 where
  base=PaneEntry (CaseName "pane") (ReturnAddress 0x1158184) (NodeId 1) (DrawMode 0) (SnapshotDepth 1) (SnapshotKind 0) Stock True

paneRustTest :: Int -> PaneEntry -> String
paneRustTest index entry = unlines
  [ "#[test]", "fn pane_case_"++show index++"() {"
  , "    let entry = PaneEntryClass { caller: ReturnAddress("++show caller_++"), node: PaneNode("++show node_++"), mode: PaneMode("++show mode_++"),"
  , "        depth: SnapshotDepth("++show depth_++"), kind: SnapshotKind("++show kind_++"), origin: NodeOrigin::"++show (paneOrigin entry)++" };"
  , "    assert_eq!(entry.owned(), "++(if paneOwned entry then "true" else "false")++", "++show name++");", "}" ]
 where
  CaseName name=paneName entry; ReturnAddress caller_=paneCaller entry; NodeId node_=paneNode entry
  DrawMode mode_=paneMode entry; SnapshotDepth depth_=paneDepth entry; SnapshotKind kind_=paneKind entry

paneJSON :: PaneEntry -> Value
paneJSON entry = object ["id" .= name,"function" .= ("Pane"::String),"caller_return" .= caller_,"node" .= node_,"mode" .= mode_,"snapshot_depth" .= depth_,"snapshot_kind" .= kind_,"origin" .= show (paneOrigin entry),"expected_owned" .= paneOwned entry]
 where
  CaseName name=paneName entry; ReturnAddress caller_=paneCaller entry; NodeId node_=paneNode entry
  DrawMode mode_=paneMode entry; SnapshotDepth depth_=paneDepth entry; SnapshotKind kind_=paneKind entry
main :: IO ()
main = do
  args<-getArgs
  case args of
    ["--json",json,"--rust",rust] -> do
      let entries=modelEntries++boundaryEntries
      B.writeFile json (encodeCanonical (object
        [ "schema_version" .= (1::Int), "model_cases" .= length modelEntries
        , "boundary_cases" .= length boundaryEntries, "entries" .= (map entryJSON entries ++ map paneJSON paneEntries)
        , "pane_cases" .= length paneEntries
        ]))
      writeFile rust ("use render_adapter_class::{DrawFunction, NodeOrigin, ReturnAddress, EntryClass, PaneEntryClass, PaneNode, PaneMode, SnapshotDepth, SnapshotKind};\n"++concat (zipWith rustTest [0..] entries)++concat (zipWith paneRustTest [0..] paneEntries))
      putStrLn (show (length modelEntries)++" native model entries, "++show (length boundaryEntries)++" class boundary entries, "++show (length paneEntries)++" pane entries")
    _ -> fail "usage: render-adapter-class-cases --json OUTPUT.json --rust OUTPUT.rs"
