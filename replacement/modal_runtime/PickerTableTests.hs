{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (unless)
import Data.Aeson ((.=))
import qualified Data.Aeson as J
import Data.Bits (shiftL, shiftR, (.&.))
import qualified Data.ByteString as B
import Data.Either (isLeft)
import Data.List (isInfixOf)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Numeric (showHex)
import Replacement.ModalRuntime.PickerTable

main :: IO ()
main = generation

check :: String -> Bool -> IO ()
check label passed = unless passed (fail label)

-- * Synthetic bytes

image :: Word32 -> Int -> M.Map Word32 Word32 -> Image
image base size words' = Image base (B.pack [byteAt (base + fromIntegral offset) | offset <- [0 .. size - 1]])
 where
  byteAt address = case M.lookup (address .&. 0xfffffffc) words' of
    Just word -> fromIntegral (word `shiftR` (8 * fromIntegral (address .&. 3)))
    Nothing -> 0

-- * Generation from synthetic JSON

hexText :: Word32 -> String
hexText value = "0x" ++ showHex value ""

nodeA, nodeB :: Word32
nodeA = 0x784014
nodeB = 0x784040

record :: Word32 -> [Word32]
record property = [1, 0, property, 7, 0, 0, 1, 0x78ea98, 0x78f828, 0x7884cc, 0x2b]

recordHex :: Word32 -> String
recordHex property = concat [pad (showHex byte "") | word <- record property, shift <- [0, 8, 16, 24], let byte = (word `shiftR` shift) .&. 0xff]
 where
  pad digits = replicate (2 - length digits) '0' ++ digits

image4For :: Word32 -> Image
image4For changed = image 0x784000 0x100 . M.fromList $
  zip [nodeA, nodeA + 4 ..] (record 214) ++ zip [nodeB, nodeB + 4 ..] (record 15) ++ [(nodeA + 40, changed)]

data Fixture = Fixture
  { cellTargetInImage :: Word32
  , cellTargetInManifest :: Word32
  , cellsAt :: Word32
  , actionsAt :: Word32
  , node4Word10 :: Word32
  , reverseCensus :: Bool
  }

baseline :: Fixture
baseline = Fixture 0xc72e10 0xc72e10 0xce0000 0xcf0000 0x2b False

okWord :: Word32
okWord = 0x0001005a

decode :: J.FromJSON b => J.Value -> b
decode value = case J.fromJSON value of
  J.Success parsed -> parsed
  J.Error reason -> error reason

inputsFor :: Fixture -> Inputs
inputsFor fixture = Inputs image5 (image4For (node4Word10 fixture)) tables census manifest
 where
  image5 = image 0xcd0000 0x30000 . M.fromList $
    [ (0xcd5e78 + 148, 46 `shiftL` 16)
    , (0xcf58b8 + 300, cellsAt fixture)
    , (0xcfe2a8 + 300, actionsAt fixture)
    , (cellsAt fixture + 4 * (8 * 46), okWord)
    , (actionsAt fixture + 4 * 0x5a, cellTargetInImage fixture)
    ]
  tables = decode $ J.object
    [ "nodes" .=
        [ J.object ["address" .= hexText nodeA, "raw_hex" .= recordHex 214, "property_or_class" .= (214 :: Int)]
        , J.object ["address" .= hexText nodeB, "raw_hex" .= recordHex 15, "property_or_class" .= (15 :: Int)]
        , J.object ["address" .= hexText 0x7823a4, "property_or_class" .= (254 :: Int)]
        ]
    ]
  dispatch perPress action = J.object ["action" .= hexText action, "per_press" .= (perPress :: Bool)]
  keyActions = [0xc6e378, 0xc6eb44, 0xc6f14c, 0xc70528, 0xc72e10, 0xc76650, 0xc7a514]
  picker = map (dispatch False) (keyActions ++ [0xca4488, 0xc59ea4, 0xc2f354]) ++ [dispatch True 0xc2d8ec]
  key menu setting node dispatches = J.object
    [ "key" .= J.object ["menu_id" .= (menu :: Int), "setting_id" .= (setting :: Int)]
    , "stock_nodes" .= [J.object ["address" .= hexText node]]
    , "dispatches" .= dispatches ]
  keys =
    [ key 1 214 nodeA picker
    , key 1 15 nodeB picker
    , key 1 254 0x7823a4 picker
    , key 0 214 nodeA picker
    , key 1 321 0x783de0 (dispatch False 0xc7a6ac : picker)
    , key 1 322 0x783e0c (map (dispatch False) keyActions)
    ]
  census = decode $ J.object ["menus" .= J.object ["keys" .= (if reverseCensus fixture then reverse keys else keys)]]
  manifest = decode $ J.object
    [ "controllers" .=
        [ J.object
            [ "id" .= (75 :: Int), "width" .= (46 :: Int)
            , "tables" .=
                [ J.object ["kind" .= ("Cells" :: String), "start" .= hexText (cellsAt fixture), "decoded_cells" .= [J.object ["index" .= (368 :: Int), "state_coordinate" .= (0 :: Int), "event_coordinate" .= (8 :: Int), "action_slot" .= (0x5a :: Int), "word" .= hexText okWord]]]
                , J.object ["kind" .= ("Actions" :: String), "start" .= hexText (actionsAt fixture), "words" .= (replicate 0x5a "0x00000000" ++ [hexText (cellTargetInManifest fixture)])]
                , J.object ["kind" .= ("Transitions" :: String)]
                ]
            ]
        ]
    ]

okSpec :: CellSpec
okSpec = CellSpec (Controller 75) (State 0) (Event 8) (ActionAddress 0xc72e10) Ok

run :: Fixture -> Either String Generated
run = generate [okSpec] . inputsFor

generation :: IO ()
generation = do
  check "contract cells are the seven controller-75 key cells" (map specController contractCells == replicate 7 (Controller 75) && map specAction contractCells == [minBound .. maxBound])
  generated <- either fail pure (run baseline)
  check "selection keeps menu-1 keys with the picker dispatch set, minus 254" (generatedNodes generated == [PickerNode (NodeAddress nodeB) (PropertyId 15), PickerNode (NodeAddress nodeA) (PropertyId 214)])
  check "cell from image5" (map (\c -> (cellWord c, cellTarget c)) (generatedCells generated) == [(CellWord okWord, ActionAddress 0xc72e10)])
  check "cell outside controller 75 refuses" (isLeft (generate [okSpec {specController = Controller 30}] (inputsFor baseline)))
  check "cell table outside image5 refuses" (isLeft (run baseline {cellsAt = 0xe00000}))
  check "action table outside image5 refuses" (isLeft (run baseline {actionsAt = 0xe00000}))
  check "stock target differing from the contract refuses" (isLeft (run baseline {cellTargetInImage = 0xc72e14, cellTargetInManifest = 0xc72e14}))
  check "image5 target differing from the manifest refuses" (isLeft (run baseline {cellTargetInManifest = 0xc72e14}))
  check "node record differing from image4 refuses" (isLeft (run baseline {node4Word10 = 0x2c}))
  let table = rustTable ["header"] generated
  reversed <- either fail pure (run baseline {reverseCensus = True})
  check "output independent of census order" (rustTable ["header"] reversed == table)
  check "node expression" ("PickerNode { node: NodeAddress(0x00784014), property: PropertyId(214) }," `isInfixOf` table)
  check "cell expression" ("PickerCell { controller: 75, state: 0, event: 8, word: 0x0001005a, target: 0x00c72e10, action: PickerAction::Ok }," `isInfixOf` table)
