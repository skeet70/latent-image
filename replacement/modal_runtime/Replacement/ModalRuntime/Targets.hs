{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | The finite modal-75 target admission inventory for the owned dispatcher.
--
-- The dispatcher reads the stock cell/action/transition tables; this module
-- declares the only controller, cell slots, action slots and targets admitted
-- for the natural recipe-bank route. The stock action targets are the complete
-- set reachable from the 1024 modal-75 cell slots. Slots 1 and 152 are the bank
-- interposition slots and must be published to the bank-owned
-- @rb_bank_interpose@ symbol instead of their stock words.
module Replacement.ModalRuntime.Targets
  ( TargetReport (..)
  , BankInterposition (..)
  , validateModal75
  , stockActionTargets
  , bankInterpositionSlots
  , modal75CellSlots
  , modal75ActionWords
  , id5BaseValue
  , id5SizeValue
  , id5Sha256
  ) where

import Data.Bits ((.&.))
import qualified Data.ByteString as B
import Data.Word (Word16, Word32)
import Numeric (showHex)

controllerId, cellBase, actionBase, transitionBase, prehook :: Word32
controllerId = 75
cellBase = 0x00cd1ae4
actionBase = 0x00d04080
transitionBase = 0x00d01370
prehook = 0x00d0cc68

id5Base :: Word32
id5Base = id5BaseValue

id5BaseValue :: Word32
id5BaseValue = 0x00bf1a00

id5SizeValue :: Int
id5SizeValue = 5185120

id5Sha256 :: String
id5Sha256 = "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"

modal75CellSlots, modal75ActionWords, modal75TransitionWords :: Int
modal75CellSlots = 1024
modal75ActionWords = 299
modal75TransitionWords = 26

data BankInterposition = BankInterposition
  { interpositionActionSlot :: Word16
  , interpositionTarget :: Word32
  }
  deriving (Eq, Show)

data TargetReport = TargetReport
  { reportController :: Word32
  , reportCellSlots :: Int
  , reportActionSlots :: [Word16]
  , reportStockTargets :: [Word32]
  , reportBankInterpositions :: [BankInterposition]
  , reportTransitionTargets :: [Word32]
  , reportPrehook :: Word32
  }
  deriving (Eq, Show)

bankInterpositionSlots :: [Word16]
bankInterpositionSlots = [1, 152]

stockActionTargets :: [Word32]
stockActionTargets =
  [ 0x00c6e378, 0x00c6e424, 0x00c6e458, 0x00c6e4bc
  , 0x00c6e520, 0x00c6e570, 0x00c6e5a8, 0x00c6e5e4
  , 0x00c6e61c, 0x00c6e658, 0x00c6e68c, 0x00c6e6f0
  , 0x00c6e740, 0x00c6e798, 0x00c6e924, 0x00c6e960
  , 0x00c6e998, 0x00c6e9fc, 0x00c6eab0, 0x00c6eafc
  , 0x00c6eb44, 0x00c6ebf8, 0x00c6ec2c, 0x00c6ec7c
  , 0x00c6ecb4, 0x00c6ecf0, 0x00c6ed28, 0x00c6ed5c
  , 0x00c6edac, 0x00c6ee04, 0x00c6ef90, 0x00c6efcc
  , 0x00c6f004, 0x00c6f0b8, 0x00c6f104, 0x00c6f14c
  , 0x00c6f410, 0x00c6f4c4, 0x00c6f560, 0x00c6f5ac
  , 0x00c6f6d0, 0x00c6f754, 0x00c6f814, 0x00c6f84c
  , 0x00c6f8e4, 0x00c6f95c, 0x00c6f994, 0x00c6f9b4
  , 0x00c6fac8, 0x00c6fb3c, 0x00c6fbfc, 0x00c6fc9c
  , 0x00c6fd48, 0x00c6fe98, 0x00c6fee0, 0x00c70014
  , 0x00c7006c, 0x00c7014c, 0x00c701ac, 0x00c70214
  , 0x00c70354, 0x00c7039c, 0x00c70410, 0x00c704f4
  , 0x00c70528, 0x00c716e0, 0x00c7176c, 0x00c717e8
  , 0x00c71814, 0x00c71a90, 0x00c71c54, 0x00c71ce4
  , 0x00c71d1c, 0x00c71f18, 0x00c71f84, 0x00c71fbc
  , 0x00c72378, 0x00c72468, 0x00c72510, 0x00c72598
  , 0x00c726bc, 0x00c72844, 0x00c72a00, 0x00c72a58
  , 0x00c72aa4, 0x00c72b0c, 0x00c72d38, 0x00c72ddc
  , 0x00c72e10, 0x00c742a8, 0x00c74364, 0x00c74424
  , 0x00c746fc, 0x00c74770, 0x00c74814, 0x00c74a2c
  , 0x00c74b34, 0x00c74c0c, 0x00c74e10, 0x00c74e5c
  , 0x00c74f5c, 0x00c7501c, 0x00c7504c, 0x00c75444
  , 0x00c75588, 0x00c75670, 0x00c757b0, 0x00c75914
  , 0x00c75adc, 0x00c75b50, 0x00c75b84, 0x00c75c70
  , 0x00c75cdc, 0x00c75ed4, 0x00c75f38, 0x00c75fa0
  , 0x00c760c0, 0x00c76138, 0x00c7617c, 0x00c7626c
  , 0x00c76344, 0x00c763bc, 0x00c763f0, 0x00c76484
  , 0x00c76524, 0x00c76574, 0x00c765e0, 0x00c76650
  , 0x00c76a64, 0x00c76b00, 0x00c76b6c, 0x00c76d3c
  , 0x00c76da4, 0x00c76e38, 0x00c76ee4, 0x00c76f8c
  , 0x00c76ff8, 0x00c77044, 0x00c770a8, 0x00c77150
  , 0x00c77180, 0x00c77234, 0x00c77274, 0x00c77328
  , 0x00c773b8, 0x00c77450, 0x00c77584, 0x00c775b0
  , 0x00c77624, 0x00c77670, 0x00c77794, 0x00c777fc
  , 0x00c77828, 0x00c77898, 0x00c778e4, 0x00c77930
  , 0x00c779a0, 0x00c77a6c, 0x00c77b24, 0x00c77b90
  , 0x00c77bfc, 0x00c77c84, 0x00c77c98, 0x00c77d10
  , 0x00c77d7c, 0x00c77e3c, 0x00c77eac, 0x00c77f24
  , 0x00c77f98, 0x00c78020, 0x00c78038, 0x00c780b8
  , 0x00c7811c, 0x00c78180, 0x00c78200, 0x00c78258
  , 0x00c782a4, 0x00c78314, 0x00c78328, 0x00c7838c
  , 0x00c78444, 0x00c784ac, 0x00c7851c, 0x00c78530
  , 0x00c7859c, 0x00c7861c, 0x00c78644, 0x00c7867c
  , 0x00c786e0, 0x00c78744
  ]

validateModal75 :: B.ByteString -> Word32 -> Either String TargetReport
validateModal75 id5 bankInterpose
  | bankInterpose == 0 = Left "bank interposition target is zero"
  | bankInterpose .&. 3 /= 0 = Left "bank interposition target is not aligned"
  | otherwise = do
      banks <- mapM checkBankSlot bankInterpositionSlots
      let cells = [(slot, readWord (cellBase + 4 * fromIntegral slot)) | slot <- [0 .. modal75CellSlots - 1]]
          actions = [fromIntegral (word .&. 0xffff) :: Word16 | (_, word) <- cells]
          highs = [(word `div` 0x10000) :: Word32 | (_, word) <- cells]
      mapM_ checkActionIndex actions
      mapM_ checkHigh highs
      mapM_ checkBankStockWord bankInterpositionSlots
      let stockTargets = [readWord (actionBase + 4 * fromIntegral slot) | slot <- actions, slot `notElem` bankInterpositionSlots]
      mapM_ checkStockTarget stockTargets
      let transitionTargets = [readWord (transitionBase + 4 * high) | high <- highs, high > 0]
      mapM_ checkTransitionTarget transitionTargets
      pure
        TargetReport
          { reportController = controllerId
          , reportCellSlots = modal75CellSlots
          , reportActionSlots = unique actions
          , reportStockTargets = unique stockTargets
          , reportBankInterpositions = banks
          , reportTransitionTargets = unique transitionTargets
          , reportPrehook = prehook
          }
 where
  checkBankSlot slot =
    if slot `elem` bankInterpositionSlots
      then Right (BankInterposition slot bankInterpose)
      else Left "bank slot outside the declared set"
  checkActionIndex slot
    | fromIntegral slot < modal75ActionWords = Right ()
    | otherwise = Left ("action index outside the 299-word table: " ++ hex (fromIntegral slot))
  checkHigh high
    | high <= 1 && fromIntegral high < modal75TransitionWords = Right ()
    | otherwise = Left ("cell high outside the declared {0,1} set: " ++ show high)
  checkStockTarget target
    | target `elem` stockActionTargets = Right ()
    | otherwise = Left ("stock action target outside the inventory: " ++ hex target)
  checkBankStockWord slot =
    case slot of
      1 | readWord (actionBase + 4) == 0 -> Right ()
      152 | readWord (actionBase + 4 * 152) == 0x00c77794 -> Right ()
      _ -> Left ("bank slot stock word differs for slot " ++ show slot)
  checkTransitionTarget target
    | target == 0 = Right ()
    | otherwise = Left ("transition target outside the zero inventory: " ++ hex target)
  readWord address =
    let offset = fromIntegral (toInteger address - toInteger id5Base)
     in if offset < 0 || offset + 4 > B.length id5
          then error ("id5 read outside image at " ++ hex address)
          else fromIntegral (B.index id5 offset)
            + fromIntegral (B.index id5 (offset + 1)) * 0x100
            + fromIntegral (B.index id5 (offset + 2)) * 0x10000
            + fromIntegral (B.index id5 (offset + 3)) * 0x1000000
  unique = foldr (\value acc -> if value `elem` acc then acc else value : acc) []
  hex value = let digits = showHex value "" in "0x" ++ replicate (8 - length digits) '0' ++ digits
