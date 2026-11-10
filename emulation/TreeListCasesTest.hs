{-# LANGUAGE GHC2021 #-}
module Main where

import Control.Monad (unless)
import qualified Data.ByteString as B
import Data.List (find, isPrefixOf, nub)
import Data.Word (Word32)
import Emulation.RenderCases
import TreeListCases (scenarios, scenarioName, expectedReport, expectedCalls, callerFrame, reportAddress, returnAddress)

main :: IO ()
main = do
  check "unique case names" (length names==length (nub names))
  let report name=expectedReport (named name)
      calls name=expectedCalls (named name)
      wordAt bytes offset=sum [fromIntegral (B.index bytes (offset+i))*256^i | i<-[0..3]] :: Word32
      targets name=[a | ServiceCall (ServiceAddress a) _ _ _ _<-calls name]
  check "short page centered" (B.index (report "flat-n1-p1-c0-s0-m0") 0x1e==1)
  check "full page not centered" (B.index (report "flat-n3-p1-c1-s0-m0") 0x1e==0)
  check "first cursor child" (wordAt (report "flat-n3-p1-c0-s0-m0") 12==0x200)
  check "middle cursor child" (wordAt (report "flat-n3-p1-c1-s0-m0") 12==0x201)
  check "last-page cursor child" (wordAt (report "flat-n8-p3-c7-s0-m0") 12==0x207)
  check "later-page cursor remainder" (B.index (report "flat-n8-p3-c7-s0-m0") 0x1c==1)
  check "off-page cursor absent" (wordAt (report "flat-n8-p1-c7-s0-m0") 12==0)
  check "hidden rows do not consume cursor" (wordAt (report "flat-hidden") 12==0x204)
  check "hidden selection absent" (B.index (report "flat-all-hidden") 0x1d==255)
  check "capacity zero normalized" (wordAt (report "flat-zero-capacity") 0x18==1)
  check "node53 bypasses centering" (B.index (report "flat-no-centering-53") 0x1e==0)
  check "node57 bypasses centering" (B.index (report "flat-no-centering-57") 0x1e==0)
  check "flat scroll top reported" (wordAt (report "flat-n8-p2-c4-s5-m0") 0x30==5)
  check "flat scroll top does not alter service sequence" (calls "flat-n8-p2-c4-s5-m0"==replaceSnapshot (calls "flat-n8-p2-c4-s0-m0") (calls "flat-n8-p2-c4-s5-m0"))
  check "flat nested offset remains zero" (all ((==0) . (`B.index` 0x3e) . expectedReport) flatScenarios)
  check "empty tree only initializes and restores" (targets "flat-empty"==[0x43550,0x61cd14,0x7221c8,0xbf8e64,0x1112b2c])
  check "single default row service order" (targets "flat-n1-p1-c0-s0-m0"==
    [0x43550,0x61cd14,0x7221c8,0xbf8e64,0xbf8d20,0x4d978,0xbf9360,0x1112b2c,0x1163e04,
     0x116dce0,0x1112e78,0x116cf50,0xbf93e0,0x1112b2c,0x116e264,0x1112e78,0x1112b2c,0x1137cf8,0x1138060,0x1112b2c])
  check "selected disabled row retains its state"
    ([(chosen,state) | ServiceCall (ServiceAddress 0x1163e04) [(Argument 0,chosen),(Argument 1,state)] _ _ _<-calls "flat-n3-p1-c1-s0-m0"]==[(0,1),(1,2),(0,1)])
  check "handled hooks omit resource leaves" (0x1112e78 `notElem` targets "flat-hooks-handled")
  check "default hooks issue both resource leaves" (length (filter (==0x1112e78) (targets "flat-hooks-default"))==6)
  check "settings palette branch exercised" (0x1112ce4 `elem` targets "flat-hooks-default")
  check "flat never draws nested rows or tabs" (all (all (`notElem` [0x11730dc,0xbf8f4c,0xbf90f0,0x116f948,0x116d4e4,0x1159168]) . map (\c->let ServiceAddress a=serviceAddress c in a) . expectedCalls) flatScenarios)
  check "slot1 uses node slot and third-register output" (case calls "source-secondarytree-tree-m5"!!1 of
    ServiceCall (ServiceAddress 0x61ce1c) [(Argument 0,0x40),(Argument 1,1)] _ _ [MemoryArgument (Argument 2) 0 bytes]->B.length bytes==44
    _->False)
  check "slot2 source replaces live snapshot" (take 2 (targets "source-menulist-tree-m7")==[0x43550,0x61ce1c] && 0x61cd14 `notElem` targets "source-menulist-tree-m7")
  check "tree parent report comes from child descriptor" (wordAt (report "source-menulist-tree-m0") 8==0x601 && wordAt (report "source-menulist-tree-m0") 0x10==0x301)
  check "tree report does not invent flat capacity" (wordAt (report "source-menulist-tree-m0") 0x18==0 && wordAt (report "source-menulist-tree-m0") 0x2c==3)
  check "hidden scrolled nested selection is visible zero" (B.index (report "tree-hidden-scroll") 0x3c==0 && wordAt (report "tree-hidden-scroll") 0x40==0x902)
  check "nested count centers second column" (B.index (report "tree-nested-centering") 0x3e==2)
  check "unknown kind retains no selected parent" (wordAt (report "unknown-kind") 0==9 && wordAt (report "unknown-kind") 8==0 && 0x116dce0 `notElem` targets "unknown-kind")
  check "forced node25 stays flat" (wordAt (report "forced-flat-25") 0==1 && 0xbf8f4c `notElem` targets "forced-flat-25")
  check "forced node38 applies highlight" (wordAt (report "forced-flat-38") 0==1 && 0x1163c1c `elem` targets "forced-flat-38")
  check "resolved parent report overrides child match" (wordAt (report "tree-resolver-overrides") 8==0x9876 && wordAt (report "tree-resolver-overrides") 0x10==0x7654)
  check "modes2 and3 omit nested summary" (all (\m->0xbf8f4c `notElem` targets ("source-menulist-tree-m"++show m)) [2::Int,3])
  check "handled selection redraw retains footer" (length (filter (==0x116f948) (targets "tree-mode13-handled"))==2 && last (init (targets "tree-mode13-handled"))==0x1159168)
  check "side mode2 suppresses detail but preserves footer" (0x116d4e4 `notElem` targets "tree-side-suppresses-detail" && 0x1159168 `elem` targets "tree-side-suppresses-detail")
  let secondary=named "source-secondarytree-tree-m0"
  check "secondary caller report is stack resident" (reportAddress secondary==0x02001024 && returnAddress secondary==0x0116c058)
  check "secondary outgoing arguments retain caller values" (callerFrame secondary==wordsLE [2,14,3,0,2,0,0x02001024])
  putStrLn "tree-list generator tests passed"
  where
    names=map scenarioName scenarios
    flatScenarios=filter (isPrefixOf "flat-" . scenarioName) scenarios
    named name=case find ((==name) . scenarioName) scenarios of Just s->s; Nothing->error ("missing case "++name)
    check label condition=unless condition (fail label)
    replaceSnapshot zero nonzero=take 1 zero ++ take 1 (drop 1 nonzero) ++ drop 2 zero
