{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

module Main where

import qualified A32 as A
import Control.Monad (unless, when)
import Data.Aeson (Value, object, (.=))
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import Data.Int (Int16, Int32)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word16, Word32)
import Emulation.Sha256 (sha256, sha256Hex)
import Numeric (showHex)
import Replacement.ModalRuntime.ServiceCategories (Category (..), Seed (..), SourceFunction (..), SourceLine (..), categoryName, classify, seeds, sourceFunctions)
import StockInventory
import System.Directory (doesPathExist)
import System.Environment (getArgs)

newtype ControllerId = ControllerId Word32 deriving (Eq, Ord, Show)
newtype CellWord = CellWord Word32 deriving (Eq, Ord, Show)
data TableKind = Cells | Actions | Transitions deriving (Eq, Ord, Show)
data Controller = Controller
  { controllerId :: ControllerId
  , controllerWidth :: Word16
  , controllerTables :: [(TableKind, GuestAddress, GuestAddress, [Word32])]
  }

hex :: Word32 -> String
hex word = "0x" ++ replicate (8 - length digits) '0' ++ digits where digits = showHex word ""
addressJson :: GuestAddress -> String
addressJson (GuestAddress address) = hex address

wordAt :: B.ByteString -> GuestAddress -> Word32
wordAt bytes (GuestAddress address)
  | address < imageBase || toInteger address + 4 > toInteger imageBase + toInteger (B.length bytes) = error ("image5 read outside input: " ++ hex address)
  | otherwise = foldr (\index value -> (value `shiftL` 8) .|. fromIntegral (B.index bytes (fromIntegral (address-imageBase) + index))) 0 [0..3]

imageBase :: Word32
imageBase = 0xbf1a00

halfAt :: B.ByteString -> Word32 -> Word16
halfAt bytes address = fromIntegral (wordAt bytes (GuestAddress (address .&. 0xfffffffc)) `shiftR` fromIntegral (8 * (address .&. 2)))

root :: TableKind -> Word32
root Cells = 0xcf58b8
root Actions = 0xcfe2a8
root Transitions = 0xd0013c

controllers :: B.ByteString -> [Controller]
controllers bytes = map capture [0..173]
 where
  pointers = [(kind, index, GuestAddress (wordAt bytes (GuestAddress (root kind + 4*index)))) | kind <- [Cells,Actions,Transitions], index <- [0..173]]
  fences = S.fromList ([address | (_,_,GuestAddress address) <- pointers, address /= 0] ++ [0xcd5e78,0xcd5fd4,0xcf58b8,0xcf5b70,0xcfe2a8,0xcfe560,0xd0013c,0xd003f4,0xd09100])
  capture index = Controller (ControllerId index) (halfAt bytes (0xcd5e78 + 2*index))
    [(kind, GuestAddress start, GuestAddress end, if start == 0 then [] else [wordAt bytes (GuestAddress address) | address <- [start,start+4..end-4]])
    | kind <- [Cells,Actions,Transitions]
    , let start = wordAt bytes (GuestAddress (root kind + 4*index))
          end = if start == 0 then 0 else maybe (error ("no table fence after " ++ hex start)) id (S.lookupGT start fences)]

cellJson :: Word32 -> Int -> CellWord -> Value
cellJson width index (CellWord word) = object
  [ "index" .= index, "state_coordinate" .= (fromIntegral index `mod` width), "event_coordinate" .= (fromIntegral index `div` width)
  , "word" .= hex word, "action_slot" .= (fromIntegral word :: Word16)
  , "transition" .= if high > 0 then object ["kind" .= ("callback-slot" :: String), "slot" .= high] else object ["kind" .= ("direct-state" :: String), "state" .= (negate (fromIntegral high :: Int32))]
  ] where high = fromIntegral (word `shiftR` 16) :: Int16

controllerJson :: Controller -> Value
controllerJson controller = object
  [ "id" .= index, "width" .= width, "state_address" .= hex (0x1467588+2*index), "event_address" .= hex (0x14676e4+2*index)
  , "tables" .= [object
      [ "kind" .= show kind, "root_word" .= hex (root kind+4*index), "start" .= addressJson start, "end_exclusive" .= addressJson end
      , "extent_basis" .= ("next distinct dispatcher-table/root/width anchor; terminal candidate fence 0xd09100 from id5.dis:285630-285636; static candidate, not dynamic index closure" :: String)
      , "words" .= map hex contents
      , "decoded_cells" .= [cellJson (fromIntegral width) cellIndex (CellWord word) | kind == Cells, width /= 0, (cellIndex,word) <- zip [0..] contents]
      ] | (kind,start,end,contents) <- controllerTables controller]
  ] where ControllerId index = controllerId controller; width = controllerWidth controller

functionJson :: B.ByteString -> M.Map GuestAddress SourceFunction -> S.Set GuestAddress -> GuestAddress -> Value
functionJson bytes functions callbackTargets entry@(GuestAddress low) = object
  [ "address" .= hex low
  , "source" .= fmap (\function -> let SourceLine first = sourceLine function; SourceLine lastLine = sourceEndLine function in object ["first_line" .= first,"last_line" .= lastLine]) found
  , "window_end_exclusive" .= fmap addressJson fence
  , "extent" .= case inspected of
      Left problem -> object ["status" .= ("unresolved" :: String), "reason" .= problem]
      Right inspection -> let FunctionDeclaration _ (FunctionSize size) = inspectedDeclaration inspection in object
        [ "status" .= either (const ("structural-with-unresolved-return" :: String)) (const "derived-with-premises") (inspectedReturn inspection)
        , "code_end_exclusive" .= hex (low+fromIntegral size), "code_bytes" .= size
        , "storage_end_exclusive" .= fmap addressJson fence
        , "instruction_words" .= length (inspectedCodeWords inspection), "literal_words" .= map addressJson (inspectedLiteralWords inspection)
        , "return_proof" .= either (\problem -> object ["status" .= ("unresolved" :: String),"reason" .= problem])
            (\uses -> object ["status" .= ("conditional" :: String),"premises" .= S.toList (S.fromList [returnPremiseLabel premise | PremiseUse _ premise <- uses]),"premise_uses" .= map show uses])
            (inspectedReturn inspection)]
  , "stock_inventory_opaque_callee_assumption" .= ("ReturnProvenance.hs `callee` assumes opaque calls preserve SP and r4-r11; this is not a recovered all-callee ABI proof" :: String)
  , "direct_calls" .= [object ["site" .= hex site,"target" .= addressJson target,"source_function_reference" .= maybe False (S.member target . sourceCalls) found,"status" .= either (const ("source-window-encoded-candidate" :: String)) (const "structurally-reachable-instruction") inspected] | (site,target) <- calls]
  , "indirect_call_candidates" .= [hex site | (site, A.BranchExchange _ True _) <- instructions]
  , "external_branch_candidates" .= [object ["site" .= hex site,"target" .= hex target] | (site,A.Branch _ False target) <- instructions, target < low || maybe True (\(GuestAddress high) -> target >= high) fence]
  , "service_categories" .= map categoryName (S.toList categories)
  , "direct_service_categories" .= S.toList (S.fromList [categoryName category | function <- maybe [] pure found, Seed address category _ _ <- seeds, S.member address (sourceCalls function)])
  , "service_witnesses" .= [object ["category" .= categoryName category,"service" .= addressJson target,"path" .= map addressJson path] | (category,target,path) <- witnesses]
  , "unclassified_frontier" .= map addressJson (S.toList unresolved)
  , "category_completeness" .= ("partial: decompiled direct-call expressions and encoded source-window call candidates; indirect, memory and device effects remain obligations; seed services are retained boundaries, not closed transitive implementations" :: String)
  , "private_abi_source_inputs" .= [object ["line" .= line,"token" .= token] | function <- maybe [] pure found, (SourceLine line,token) <- sourcePrivateInputs function]
  , "private_abi_status" .= ("incoming r11/LR saved-frame exposure and dispatcher scratch/register-role compatibility remain open; decompiler tokens are leads, not a proof of absence" :: String)
  ]
 where
  found = M.lookup entry functions
  fence = S.lookupGT entry (S.insert (GuestAddress 0xcd0c78) (S.union (M.keysSet functions) callbackTargets))
  windowInstructions = case fence of
    Just (GuestAddress high) | low >= imageBase && high <= imageBase+fromIntegral (B.length bytes) -> [(address,A.decode address (wordAt bytes (GuestAddress address))) | address <- [low,low+4..high-4]]
    _ -> []
  instructions = case inspected of
    Right inspection -> [(address,A.decode address (wordAt bytes (GuestAddress address))) | GuestAddress address <- inspectedCodeWords inspection]
    Left _ -> windowInstructions
  calls = [(site,GuestAddress target) | (site,A.Branch _ True target) <- instructions]
  tails = S.toList (S.fromList [GuestAddress target | (_,A.Branch _ False target) <- windowInstructions, S.member (GuestAddress target) (S.union (M.keysSet functions) callbackTargets), target < low || maybe True (\(GuestAddress high) -> target >= high) fence])
  inspected = case fence of
    Nothing -> Left "no source/callback function fence"
    Just high | low < imageBase -> Left "target is outside supplied image5"
              | otherwise -> inspectFunction xm1TaskRecords (GuestAddress imageBase) bytes (FunctionWindow entry high) tails
  (witnesses, unresolved) = classify functions callbackTargets entry
  categories = S.fromList ([category | (category,_,_) <- witnesses] ++ [Unclassified | null witnesses || not (S.null unresolved)])

main :: IO ()
main = do
  arguments <- getArgs
  let required name = case dropWhile (/= name) arguments of _:value:_ -> pure value; _ -> fail ("missing " ++ name)
  outputPath <- required "--out"
  exists <- doesPathExist outputPath
  when exists (fail "output already exists")
  imagePath <- required "--id5"
  sourcePath <- required "--decompilation"
  reachPath <- required "--reachability"
  bytes <- B.readFile imagePath
  unless (B.length bytes == 5185120 && sha256Hex (sha256 bytes) == "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a") (fail "image5 identity differs")
  sourceBytes <- B.readFile sourcePath
  unless (sha256Hex (sha256 sourceBytes) == "0e979effa216c9a096afcac109674cdcc42dc830a0669cdc9fdf7cbbdf03053f") (fail "decompilation identity differs")
  sourceText <- readFile sourcePath
  reachBytes <- B.readFile reachPath
  reachability <- either fail pure (J.eitherDecodeStrict reachBytes :: Either String Value)
  let functions = sourceFunctions sourceText
      captured = controllers bytes
      actionTargets = S.fromList [GuestAddress target | controller <- captured, (Actions,_,_,contents) <- controllerTables controller, target <- contents, target /= 0]
      transitionTargets = S.fromList [GuestAddress target | controller <- captured, (Transitions,_,_,contents) <- controllerTables controller, target <- contents, target /= 0]
      targets = S.union actionTargets transitionTargets
      result = object
        [ "schema_version" .= (1 :: Int), "firmware" .= ("Fujifilm X-M1 1.50" :: String)
        , "evidence_class" .= ("static-inference; receipt projection only, not runtime qualification" :: String)
        , "inputs" .= object ["image5" .= imagePath,"image5_sha256" .= sha256Hex (sha256 bytes),"decompilation" .= sourcePath,"decompilation_sha256" .= sha256Hex (sha256 sourceBytes),"reachability" .= reachPath,"reachability_sha256" .= sha256Hex (sha256 reachBytes)]
        , "scope" .= ("All 174 root registrations and physical candidate table contents, distinct from the existing 49,738-cell reachable static closure. Adjacent anchors bound candidate storage, not admissible event/state indices." :: String)
        , "function_fence_evidence" .= object ["0x00cd0c78" .= ("id5.dis:228442-228473, final callback returns at0xcd0c74 before padding/table data; structural inspection still required" :: String)]
        , "counts" .= object
          [ "controllers" .= length captured, "cell_words" .= sum [length contents | controller <- captured,(Cells,_,_,contents) <- controllerTables controller]
          , "action_slots" .= sum [length contents | controller <- captured,(Actions,_,_,contents) <- controllerTables controller]
          , "transition_slots" .= sum [length contents | controller <- captured,(Transitions,_,_,contents) <- controllerTables controller]
          , "unique_action_targets" .= S.size actionTargets, "unique_transition_targets" .= S.size transitionTargets, "unique_targets" .= S.size targets]
        , "controllers" .= map controllerJson captured
        , "functions" .= map (functionJson bytes functions targets) (S.toList targets)
        , "service_seeds" .= [object ["address" .= addressJson address,"category" .= categoryName category,"meaning" .= meaning,"citation" .= citation] | Seed address category meaning citation <- seeds]
        , "reachability" .= reachability
        , "limits" .= (["Table adjacency is a static candidate extent, not a lifetime immutability or index-domain proof.","No copied table contents are intended for public redistribution.","A successful StockInventory derivation is conditional on its recorded return premises and the helper's opaque-callee SP/r4-r11-preservation assumption. Refused targets remain unresolved; source windows are not code extents.","Call categories are overlapping, incomplete static source-reference witnesses. Unknown frontiers are retained, not assigned semantics from address ranges.","No guest execution, global dispatcher publication or retired-body exclusion is established."] :: [String])
        ]
  L.writeFile outputPath (J.encode result)
  putStrLn ("wrote " ++ outputPath ++ "; controllers=" ++ show (length captured) ++ "; targets=" ++ show (S.size targets))
