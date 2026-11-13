{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main where

import Data.Aeson (Value, object, (.=))
import Data.Aeson.Types (Pair)
import qualified Data.ByteString as B
import Data.List (isPrefixOf)
import Data.Word (Word32)
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
data RecordedEntry = RecordedEntry
  { caseName :: CaseName, function :: DrawFunction, caller :: ReturnAddress
  , mode :: DrawMode, node :: NodeId, liveKind :: SnapshotKind
  , liveParent :: ParentId, liveSelected :: SelectedId, origin :: NodeOrigin
  }
data InstallationAdmission = Admitted | Refused deriving (Eq, Show)
data InstallationCase = InstallationCase RecordedEntry InstallationAdmission

wordAt :: B.ByteString -> Int -> Word32
wordAt bytes offset = sum [fromIntegral (B.index bytes (offset+i))*256^i | i<-[0..3]]

modelEntries :: [RecordedEntry]
modelEntries = map tree Tree.scenarios ++ map picker Picker.pickerCases
  where
    tree c = let Tree.NodeId n=Tree.node c; Tree.DrawMode m=Tree.mode c; snapshot=Tree.snapshot c
             in RecordedEntry (CaseName (Tree.scenarioName c)) Tree (ReturnAddress (Tree.returnAddress c))
                  (DrawMode m) (NodeId n) (SnapshotKind (snapshot!!0)) (ParentId (snapshot!!3))
                  (SelectedId (snapshot!!4)) Stock
    picker c = let Picker.NodeId n=Picker.pickerNode c; snapshot=Picker.pickerSnapshot c
               in RecordedEntry (CaseName ("picker-"++Picker.pickerName c)) Picker (ReturnAddress (Picker.pickerReturnAddress c))
                    (DrawMode (Picker.pickerMode c)) (NodeId n) (SnapshotKind (wordAt snapshot 0))
                    (ParentId (wordAt snapshot 12)) (SelectedId (wordAt snapshot 16)) Stock

boundaryEntries :: [InstallationCase]
boundaryEntries = map (`InstallationCase` Refused)
  (concatMap refused [tree,picker,fnPicker] ++
   [ tree {caseName=CaseName ("tree-saved-mode-"++show m),mode=DrawMode m} | m<-[5,6,7,9,11,12,13] ] ++
   [ entry {caseName=CaseName (prefix entry++"-saved-or-skipped-mode-"++show m),mode=DrawMode m}
   | entry<-[picker,fnPicker],m<-[2,3,7,9,13] ] ++
   [ tree {caseName=CaseName ("tree-kind-"++show kind),liveKind=SnapshotKind kind} | kind<-[0,2,0xffffffff] ] ++
   [ tree {caseName=CaseName "tree-node25-parent-near-miss",node=NodeId 0x25,liveKind=SnapshotKind 2,liveParent=ParentId 27,liveSelected=SelectedId 0}
   , tree {caseName=CaseName "tree-node38-parent-near-miss",node=NodeId 0x38,liveKind=SnapshotKind 2,liveParent=ParentId 228,liveSelected=SelectedId 11}
   , tree {caseName=CaseName "tree-node25-selection-near-miss",node=NodeId 0x25,liveKind=SnapshotKind 2,liveParent=ParentId 0x27,liveSelected=SelectedId 1}
   , tree {caseName=CaseName "tree-node38-selection-near-miss",node=NodeId 0x38,liveKind=SnapshotKind 2,liveParent=ParentId 0x228,liveSelected=SelectedId 12}
   ] ++
   [ picker {caseName=CaseName ("picker-uninstalled-caller-"++show address),caller=ReturnAddress address}
   | address<-[0x01169a18,0x0116a1bc,0x0116a8b4,0x0116b530,0x0116b56c] ]) ++
  map (`InstallationCase` Admitted)
  ([ tree {caseName=CaseName ("tree-live-mode-"++show m),mode=DrawMode m}
   | m<-[0,1,2,3,4,8,10,14,31,32,0xffffffff] ] ++
   [ entry {caseName=CaseName (prefix entry++"-live-mode-"++show m),mode=DrawMode m}
   | entry<-[picker,fnPicker],m<-[0,1,4,5,6,8,10,11,12,14,31,32,0xffffffff] ] ++
   [ tree {caseName=CaseName "tree-node25-override",node=NodeId 0x25,liveKind=SnapshotKind 2,liveParent=ParentId 0x27,liveSelected=SelectedId 0}
   , tree {caseName=CaseName "tree-node38-override",node=NodeId 0x38,liveKind=SnapshotKind 2,liveParent=ParentId 0x228,liveSelected=SelectedId 11}
   ])
  where
    tree=RecordedEntry (CaseName "tree-boundary") Tree (ReturnAddress 0x01167a60) (DrawMode 0) (NodeId 0x40)
      (SnapshotKind 1) (ParentId 0x224) (SelectedId 9) Stock
    picker=RecordedEntry (CaseName "picker-boundary") Picker (ReturnAddress 0x01168ce4) (DrawMode 0) (NodeId 0x30)
      (SnapshotKind 0x17) (ParentId 0x39) (SelectedId 1) Stock
    fnPicker=picker {caseName=CaseName "fn-picker-boundary",caller=ReturnAddress 0x0116c588,node=NodeId 0x63}
    prefix entry=case (function entry,caller entry) of (Tree,_)->"tree"; (Picker,ReturnAddress 0x0116c588)->"fn-picker"; _->"picker"
    refused entry=
      [ entry {caseName=CaseName (prefix entry++"-package-node"),node=NodeId 0x400,origin=Package}
      , entry {caseName=CaseName (prefix entry++"-unknown-origin"),origin=Unknown}
      , entry {caseName=CaseName (prefix entry++"-other-caller"),caller=ReturnAddress (case function entry of Tree->0x0116c058;Picker->0x01169308)}
      , entry {caseName=CaseName (prefix entry++"-cross-function-caller"),caller=ReturnAddress (case function entry of Tree->0x01168ce4;Picker->0x01167a60)}
      ]

entryFields :: RecordedEntry -> [Pair]
entryFields entry =
  [ "id" .= name, "function" .= show (function entry), "caller_return" .= callerWord
  , "mode" .= modeWord, "node" .= nodeWord, "live_kind" .= kindWord
  , "live_parent" .= parentWord, "live_selected" .= selectedWord
  , "origin" .= show (origin entry)
  ]
  where
    CaseName name=caseName entry; ReturnAddress callerWord=caller entry; DrawMode modeWord=mode entry
    NodeId nodeWord=node entry; SnapshotKind kindWord=liveKind entry
    ParentId parentWord=liveParent entry; SelectedId selectedWord=liveSelected entry

installationJSON :: InstallationCase -> Value
installationJSON (InstallationCase entry admission) =
  object (entryFields entry ++ ["expected_owned" .= (admission==Admitted)])

rustTest :: Int -> InstallationCase -> String
rustTest index (InstallationCase entry admission) = unlines
  [ "#[test]", "fn entry_case_"++show index++"() {"
  , "    let entry = EntryClass { function: DrawFunction::"++show (function entry)++", caller: ReturnAddress("++show callerWord++"),"
  , "        mode: "++show modeWord++", node: "++show nodeWord++", live_kind: "++show kindWord++","
  , "        live_parent: "++show parentWord++", live_selected: "++show selectedWord++", origin: NodeOrigin::"++show (origin entry)++" };"
  , "    assert_eq!(entry.owned(), "++(if admission==Admitted then "true" else "false")++", "++show name++");"
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

adapterInterface :: String -> Either String String
adapterInterface source = extract (lines source)
  where
    extract (derive:header:remaining)
      | header == "pub enum NodeOrigin {", "#[derive(" `isPrefixOf` derive =
          case break (== "}") remaining of
            (variants, "}":_) -> Right (unlines (derive:header:variants ++ ["}"]))
            _ -> Left "unterminated adapter NodeOrigin"
    extract (_:remaining) = extract remaining
    extract [] = Left "adapter NodeOrigin declaration absent"

main :: IO ()
main = do
  args<-getArgs
  case args of
    ["--adapter-interface",source,output] -> do
      interface <- either fail pure . adapterInterface =<< readFile source
      writeFile output interface
    ["--json",json,"--rust",rust] -> do
      B.writeFile json (encodeCanonical (object
        [ "schema_version" .= (2::Int), "native_model_cases" .= length modelEntries
        , "native_model_inputs" .= map (object . entryFields) modelEntries
        , "installation_cases" .= length boundaryEntries
        , "installation_entries" .= map installationJSON boundaryEntries
        , "pane_cases" .= length paneEntries, "pane_entries" .= map paneJSON paneEntries
        ]))
      writeFile rust ("use render_adapter_class::{DrawFunction, NodeOrigin, ReturnAddress, EntryClass, PaneEntryClass, PaneNode, PaneMode, SnapshotDepth, SnapshotKind};\n"++concat (zipWith rustTest [0..] boundaryEntries)++concat (zipWith paneRustTest [0..] paneEntries))
      putStrLn (show (length modelEntries)++" native model inputs, "++show (length boundaryEntries)++" installation boundary entries, "++show (length paneEntries)++" pane entries")
    _ -> fail "usage: render-adapter-class-cases --json OUTPUT.json --rust OUTPUT.rs | --adapter-interface SOURCE.rs OUTPUT.rs"
