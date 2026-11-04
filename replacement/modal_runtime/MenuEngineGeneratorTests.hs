{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (unless)
import Data.Aeson ((.=))
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as K
import qualified Data.ByteString as B
import Data.Either (isLeft)
import Data.List (isInfixOf)
import Data.Word (Word32)
import Numeric (showHex)
import qualified Replacement.ModalRuntime.MenuEngineCases as C
import qualified Replacement.ModalRuntime.MenuEngineTree as T

main :: IO ()
main = treeGeneration >> caseExtraction

check :: String -> Bool -> IO ()
check label passed = unless passed (fail label)

decode :: J.FromJSON b => J.Value -> Either String b
decode value = case J.fromJSON value of
  J.Success parsed -> Right parsed
  J.Error reason -> Left reason

hexText :: Word32 -> String
hexText value = "0x" ++ showHex value ""

-- * Tree generator

stringRef :: Word32 -> J.Value
stringRef identity = J.object ["id" .= hexText identity, "text" .= J.Null]

data NodeSpec = NodeSpec {specAddress :: Word32, specWord4 :: Int, specRows :: Int, specChild :: Maybe Word32, specParent :: Maybe Word32}

nodeJson :: NodeSpec -> J.Value
nodeJson spec = J.object
  [ "address" .= hexText (specAddress spec), "id" .= (1 :: Int), "type" .= (0 :: Int), "property_or_class" .= (0x104 :: Int)
  , "kind" .= specRows spec, "word4" .= specWord4 spec, "word5" .= (0 :: Int), "flag" .= (1 :: Int)
  , "parent" .= fmap hexText (specParent spec), "word10" .= (42 :: Int)
  , "rows" .= [J.object ["row" .= (0 :: Int), "child" .= fmap hexText (specChild spec)], J.object ["row" .= (1 :: Int), "child" .= J.Null]]
  , "labels" .= [J.object ["preview_icon" .= stringRef 0, "list_icon" .= stringRef 0x780140, "help" .= stringRef 0x12, "name" .= stringRef 0x34]]
  , "registry_ids" .= ([] :: [Int]), "referenced_by" .= ([] :: [Int]), "raw_hex" .= ("" :: String) ]

tablesJson :: String -> [NodeSpec] -> [Maybe Word32] -> J.Value
tablesJson image nodes registry = J.object
  [ "schema" .= (1 :: Int)
  , "inputs" .= J.object ["image4_sha256" .= image, "strings_sha256" .= T.stringsPin]
  , "registry" .= [J.object ["id" .= slot, "address" .= fmap hexText entry] | (slot, entry) <- zip [0 :: Int ..] registry]
  , "nodes" .= map nodeJson nodes ]

page, picker :: NodeSpec
page = NodeSpec 0x78ea98 0 7 (Just 0x789ea4) Nothing
picker = NodeSpec 0x789ea4 0 7 Nothing (Just 0x78ea98)

registryOf :: [Maybe Word32]
registryOf = Nothing : Just 0x78ea98 : replicate (0x86 - 2) Nothing

generateFrom :: J.Value -> Either String T.Generated
generateFrom value = decode value >>= T.generate (const (replicate 64 'a')) "tables-sha"

treeGeneration :: IO ()
treeGeneration = do
  generated <- either fail pure (generateFrom (tablesJson T.image4Pin [page, picker] registryOf))
  let tree = T.generatedTree generated
      records = T.generatedRecords generated
  check "page node refers to the picker by position"
    ("Row { token: RowToken::new(0x0), child: RowChild::Node(NodeRef::new(1)) }" `isInfixOf` tree)
  check "picker node names its parent and flags"
    ("wrap: Wrap::Wraps, availability: Availability::Queried, remember: Remember::On, parent: Some(NodeRef::new(0))" `isInfixOf` tree)
  check "labels move to their own output" ("list_icon: StringId::new(0x780140)" `isInfixOf` T.generatedLabels generated && not ("StringId" `isInfixOf` tree))
  check "tree data sits in the engine section" ("#[link_section = \".menu_engine.rodata\"]\nstatic TREE: Tree" `isInfixOf` tree)
  check "registry slot 1 is the page" ("        Some(NodeRef::new(0)), // 0x1" `isInfixOf` tree)
  check "tree carries the identity" (("identity: TreeIdentity([" ++ concat (replicate 31 "0xaa, ") ++ "0xaa])") `isInfixOf` tree)
  check "records carry the same identity and addresses in node order"
    ("tree: TreeIdentity([0xaa" `isInfixOf` records && "RecordAddress::new(0x78ea98), // 0" `isInfixOf` records && "RecordAddress::new(0x789ea4), // 1" `isInfixOf` records)
  check "another image refuses" (isLeft (generateFrom (tablesJson "00" [page, picker] registryOf)))
  check "a flag outside 0 and 1 refuses" (isLeft (generateFrom (tablesJson T.image4Pin [page {specWord4 = 2}, picker] registryOf)))
  check "zero rows per page refuses" (isLeft (generateFrom (tablesJson T.image4Pin [page, picker {specRows = 0}] registryOf)))
  check "a child outside the nodes refuses" (isLeft (generateFrom (tablesJson T.image4Pin [page {specChild = Just 0x700000}, picker] registryOf)))
  check "a short registry refuses" (isLeft (generateFrom (tablesJson T.image4Pin [page, picker] (take 4 registryOf))))
  check "an unresolved node refuses"
    (isLeft (generateFrom (J.object ["schema" .= (1 :: Int), "inputs" .= J.object ["image4_sha256" .= T.image4Pin, "strings_sha256" .= T.stringsPin], "registry" .= ([] :: [Int]), "nodes" .= [J.object ["address" .= ("0x1" :: String), "unresolved" .= ("pointer outside image" :: String)]]])))
  let wordsHex = concatMap (\value -> concat [pad (showHex ((value `div` (256 ^ i)) `mod` 256) "") | i <- [0 :: Int .. 3]])
      pad digits = replicate (2 - length digits) '0' ++ digits
      planWrite role at values = J.object ["address" .= show at, "new" .= wordsHex values, "role" .= (role :: String), "region" .= ("menu_parent" :: String)]
      planJson values labels = J.object ["writes" .=
        [ planWrite "parent node +0x20 values pointer" (0x78ea98 + 0x20 :: Word32) [0x00c4a800]
        , planWrite "parent values array with the two items" (0x00c4a800 :: Word32) values
        , planWrite "parent labels array with the two items" (0x00c4a920 :: Word32) labels ]]
      stockValues = [0, 0x789ea4, 1, 0] :: [Word32]
      stockLabels = [0, 0x780140, 0x12, 0x34] :: [Word32]
      overlaid :: [Word32] -> [Word32] -> Either String T.Generated
      overlaid values labels = do
        document <- decode (tablesJson T.image4Pin [page, picker] registryOf)
        overlay <- T.parsePackagePlan (planJson values labels)
        composed <- T.composePackage overlay document
        T.generate (const (replicate 64 'a')) "tables-sha" composed
      ends = [0x7ffffffe, 0] :: [Word32]
      labelEnd = replicate 4 0xffffffff :: [Word32]
  composed <- either fail pure (overlaid (stockValues ++ [0x7d, 0x00c4b000] ++ ends) (stockLabels ++ [0x846, 0, 0, 0] ++ labelEnd))
  check "an appended row with a package child is a package row"
    ("Row { token: RowToken::new(0x7d), child: RowChild::Package }" `isInfixOf` T.generatedTree composed)
  check "the composed node keeps its decoded rows first"
    ("[Row; 3] = [Row { token: RowToken::new(0x0), child: RowChild::Node(NodeRef::new(1)) }" `isInfixOf` T.generatedTree composed)
  check "the composed tree has another identity" (T.generatedIdentity composed == replicate 64 'a')
  check "an appended row whose child is a decoded node keeps it"
    (either (const False) (("Row { token: RowToken::new(0x7e), child: RowChild::Node(NodeRef::new(1)) }" `isInfixOf`) . T.generatedTree)
      (overlaid (stockValues ++ [0x7e, 0x789ea4] ++ ends) (stockLabels ++ [0x847, 0, 0, 0] ++ labelEnd)))
  check "a package whose rows do not begin with the decoded rows refuses"
    (isLeft (overlaid ([5, 0x789ea4, 1, 0, 0x7d, 0x00c4b000] ++ ends) (stockLabels ++ [0x846, 0, 0, 0] ++ labelEnd)))
  check "a package values array without its terminator refuses"
    (isLeft (overlaid (stockValues ++ [0x7d, 0x00c4b000]) (stockLabels ++ labelEnd)))

-- * Case extractor

hit :: String -> Integer -> Word32 -> [Word32] -> J.Value
hit site instructions lr registers = J.object
  [ "site" .= site, "core" .= (0 :: Int), "instructions" .= instructions, "lr" .= hexText lr
  , "registers" .= J.object [K.fromString ("r" ++ show index) .= hexText value | (index, value) <- zip [0 :: Int ..] registers] ]

write :: Integer -> Word32 -> [Int] -> Word32 -> Int -> J.Value
write instructions address before value size = J.object
  [ "address" .= address, "before" .= before, "value" .= value, "size" .= size, "instructions" .= instructions, "core" .= (0 :: Int), "pc" .= (0 :: Int) ]

sites :: [(String, Word32)]
sites =
  [ ("stage-entry", 0xd4dd2c), ("engine-kind", 0xbfa234), ("frame-snapshot", 0x61ceac), ("row-availability", 0xd52684)
  , ("availability-return", 0xbfa914), ("initial-row", 0xd588b8), ("initial-row-return", 0xbfaa3c), ("engine-notify", 0xd5b350)
  , ("special-post", 0xd42a64) ]

configJson :: [(String, Word32)] -> [(Word32, Word32)] -> J.Value
configJson traced written = J.object
  [ "trace_sites" .= [J.object ["address" .= hexText address, "label" .= label] | (label, address) <- traced]
  , "write_ranges" .= [J.object ["start" .= hexText start, "end" .= hexText end] | (start, end) <- written]
  , "limit" .= (100 :: Int) ]

ranges :: [(Word32, Word32)]
ranges = [(0x93eb74, 0x944440), (0x9571d0, 0x957600), (0x1467580, 0x1467588)]

blockHex :: [(Int, Int)] -> String
blockHex changes = concat [let byte = maybe 0 id (lookup offset changes) in pad (showHex byte "") | offset <- [0 .. 0x42f]]
 where
  pad digits = replicate (2 - length digits) '0' ++ digits

data ReceiptSpec = ReceiptSpec {hits :: [J.Value], writes :: [J.Value], after :: [(Int, Int)], configSha :: String, datSha :: String}

baseline :: ReceiptSpec
baseline = ReceiptSpec
  { hits =
      [ hit "stage-entry" 100 0xca46c4 [0xe, 1, 0x1d, 0]
      , hit "engine-kind" 110 0xbf95d4 [0xe, 1, 0x1d, 0]
      , hit "engine-notify" 120 0xbface4 [0xe, 1, 0x1d, 5]
      , hit "row-availability" 130 0xbfa914 [0x104, 0, 0, 0]
      , hit "row-availability" 131 0xd53ab8 [0x104, 6, 0, 0]
      , hit "availability-return" 140 0x61ce00 [2, 0, 0, 0]
      , hit "initial-row" 150 0xbfaa3c [0xe, 1, 0x104, 0]
      , hit "initial-row-return" 160 0x61d520 [0x7ffffffe, 0, 0, 0]
      , hit "frame-snapshot" 170 0xbf95ec [1, 0, 0, 0]
      , hit "special-post" 200 0xca46fc [0xe, 1, 0x1d, 0]
      , hit "initial-row" 300 0xd430cc [0xf, 1, 0x104, 0]
      ]
  , writes =
      [ write 50 0x93eb74 [0, 0, 0, 0] 9 4
      , write 125 0x9571f0 [0, 0, 0, 0] 2 4
      , write 171 0x93eba0 [0, 0, 0, 0] 1 4
      , write 172 0x943190 [0, 0] 1 2
      , write 250 0x9571f0 [2, 0, 0, 0] 0 4
      ]
  , after = [(0x20, 2)]
  , configSha = "config-sha"
  , datSha = C.datSha256
  }

receiptJson :: ReceiptSpec -> J.Value
receiptJson spec = J.object
  [ "inputs" .= J.object ["config_sha256" .= configSha spec, "dat_sha256" .= datSha spec]
  , "menu_globals" .= J.object ["address" .= ("0x9571d0" :: String), "before" .= blockHex [], "after" .= blockHex (after spec)]
  , "trace_hits" .= hits spec
  , "watch" .= J.object ["enabled" .= True, "violation" .= J.Null, "ranges" .= [[start, end] | (start, end) <- ranges], "records" .= writes spec]
  ]

extractWith :: J.Value -> ReceiptSpec -> Either String C.Case
extractWith config spec = do
  parsedConfig <- decode config
  receipt <- decode (receiptJson spec)
  C.extract "config-sha" parsedConfig receipt

caseExtraction :: IO ()
caseExtraction = do
  let config = configJson sites ranges
      extractSpec = extractWith config
  extracted <- either fail pure (extractSpec baseline)
  check "event from the stage entry" (C.caseEvent extracted == (0xe, 1, 0x1d, 0))
  let zeros = B.replicate 0x430 0
      written = B.take 0x20 zeros <> B.singleton 2 <> B.drop 0x21 zeros
  check "service calls in instruction order with the block at each"
    ( C.caseCalls extracted
        == [ C.ServiceCall (C.CallNotify (0xe, 1, 0x1d, 5)) zeros
           , C.ServiceCall (C.CallAvailability 0x104 0 2) written
           , C.ServiceCall (C.CallInitialRow 0xe 1 0x104 Nothing) written
           , C.ServiceCall (C.CallStore 1) written ] )
  check "after block replays the engine writes" (B.index (C.caseAfter extracted) 0x20 == 2)
  check "snapshot writes in the areas, in order" (C.caseSnapshotWrites extracted == [(0x93eba0, 1)])
  check "no last-move writes" (null (C.caseLastMoveWrites extracted) && C.caseLastMoveBefore extracted == (Nothing, Nothing))
  let withoutPost = baseline {hits = filter (/= hit "special-post" 200 0xca46fc [0xe, 1, 0x1d, 0]) (hits baseline), after = []}
  check "a rejected receipt refuses" (isLeft (extractSpec withoutPost))
  check "another firmware refuses" (isLeft (extractSpec baseline {datSha = "00"}))
  moved <- either fail pure (extractSpec baseline {writes = writes baseline ++ [write 165 0x1467584 [1, 0, 0, 0] 0 4]})
  check "last-move writes and the word's prior value"
    (C.caseLastMoveWrites moved == [(C.LastMoveToken, 0)] && C.caseLastMoveBefore moved == (Nothing, Just 1))
  check "missing initial-row-return site refuses" (isLeft (extractWith (configJson (filter ((/= "initial-row-return") . fst) sites) ranges) baseline))
  check "uncovered last-move range refuses" (isLeft (extractWith (configJson sites (take 2 ranges)) baseline))
  check "another config refuses" (isLeft (extractSpec baseline {configSha = "other"}))
  check "a second stage entry refuses" (isLeft (extractSpec baseline {hits = hits baseline ++ [hit "stage-entry" 400 0xca46c4 [4, 1, 1, 0]]}))
  check "a stage entry from another consumer refuses" (isLeft (extractSpec baseline {hits = hit "stage-entry" 100 0xca0100 [0xe, 1, 0x1d, 0] : drop 1 (hits baseline)}))
  check "an after block differing from the replay refuses" (isLeft (extractSpec baseline {after = [(0x20, 3)]}))
  check "a block write over other bytes refuses" (isLeft (extractSpec baseline {writes = [write 125 0x9571f0 [7, 0, 0, 0] 2 4]}))
  check "a block write before the stage entry refuses" (isLeft (extractSpec baseline {writes = write 90 0x9571f4 [0, 0, 0, 0] 1 4 : writes baseline}))
  check "an availability state outside 0..3 refuses"
    (isLeft (extractSpec baseline {hits = [if h == hit "availability-return" 140 0x61ce00 [2, 0, 0, 0] then hit "availability-return" 140 0x61ce00 [4, 0, 0, 0] else h | h <- hits baseline]}))
  check "Rust case text" ("result: EngineResult::Accepted" `isInfixOf` C.rustCases [("push", "r", "c", extracted)])
