module RecipeBankRamLayout
  ( runtimeExecutableBase
  , runtimeDataBase
  , runtimeNameBase
  , runtimeBankEnd
  , runtimeInstallerExtent
  , runtimeAllocationSize
  , safePublicationOrder
  , publicationSetupCount
  , publicationCount
  , encodePublicationRecord
  , installerHasCallClosure
  , staleFixedReferences
  ) where

import Data.List (isInfixOf, isSuffixOf)

import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Word (Word32)

runtimeExecutableBase :: Word32 -> Word32
runtimeExecutableBase base = base + 0x500

runtimeDataBase :: Word32 -> Word32
runtimeDataBase base = base + 0x8000

runtimeNameBase :: Word32 -> Word32
runtimeNameBase base =
  let candidate = (base .&. 0xffff0000) .|. 0xb784
   in if candidate < base + 0x9800 then candidate + 0x10000 else candidate

runtimeBankEnd :: Word32 -> Word32
runtimeBankEnd base = runtimeNameBase base + 14 * 66

runtimeInstallerExtent :: Word32
runtimeInstallerExtent = 0x1b40

runtimeAllocationSize :: Word32
runtimeAllocationSize = 0x1b6d8

-- | Logical indexes in table order: the bank's 69 setup entries, D3's 98
-- setup entries (logical 94..191), D3's 58 activation entries (logical
-- 192..249), then the bank's 25 activation entries ending with root 54.
safePublicationOrder :: [Int]
safePublicationOrder =
  [0 .. 35] <> [55 .. 68] <> [85] <> [36 .. 53]
    <> [94 .. 249]
    <> [75,76,72,73,74,69,70,71,77,78,79,80,86,87,88,89,90,91,92,93,81,82,83,84,54]

publicationSetupCount :: Int
publicationSetupCount = 167

publicationCount :: Int
publicationCount = 250

installerHasCallClosure :: String -> Bool
installerHasCallClosure disassembly =
  and
    [ requires "ram_push_install" ["execute_operation", "publish_mailbox"]
    , requires "ram_push_rollback" ["execute_operation", "publish_mailbox"]
    , requires "execute_operation" ["run_rendezvous"]
    , requires "run_rendezvous"
        ["post_peer_park", "wait_for_signal", "ram_push_rendezvous_locked", "publish_words_locked"]
    , ordered "run_rendezvous" ["ram_push_rendezvous_locked", "publish_words_locked"]
    , requires "ram_push_rendezvous_locked" ["validate_old_words", "validate_rollback_words"]
    , requires "publish_words_locked" ["store_new_words", "restore_old_words", "synchronize_current_core"]
    , requires "peer_park_action" ["synchronize_current_core", "ram_push_cpu1_window_exit"]
    , functionPresent "peer_park_complete"
    , contains "post_peer_park" "blx"
    , masked "run_rendezvous"
    , masked "peer_park_action"
    , symbolPresent "ram_push_cpu0_window_exit"
    , symbolPresent "ram_push_cpu1_window_exit"
    ]
 where
  requires name dependencies =
    maybe False
      (\body -> all (\dependency -> ("<" ++ dependency ++ ">") `isInfixOf` body) dependencies)
      (functionBody name)
  ordered name dependencies =
    maybe False (orderedReferences dependencies . lines) (functionBody name)
  orderedReferences [] _ = True
  orderedReferences (dependency : rest) body =
    case dropWhile (not . isInfixOf ("<" ++ dependency ++ ">")) body of
      [] -> False
      _ : after -> orderedReferences rest after
  contains name instruction =
    maybe False (isInfixOf instruction) (functionBody name)
  masked name =
    maybe False
      (\body -> "cpsid" `isInfixOf` body && "cpsie" `isInfixOf` body)
      (functionBody name)
  functionPresent name = maybe False (const True) (functionBody name)
  symbolPresent name = ("<" ++ name ++ ">:") `isInfixOf` disassembly
  functionBody name =
    case dropWhile (not . isSuffixOf ("<" ++ name ++ ">:")) (lines disassembly) of
      [] -> Nothing
      _ : body -> Just (unlines (takeWhile (not . isSuffixOf ">:") body))

encodePublicationRecord :: Word32 -> Word32 -> Word32 -> Word32 -> B.ByteString
encodePublicationRecord address oldWord newWord phase =
  word32 address <> word32 oldWord <> word32 newWord <> word32 phase


staleFixedReferences :: Word32 -> Word32 -> B.ByteString -> [(Int, Word32)]
staleFixedReferences low high bytes = direct <> movPairs
 where
  direct =
    [ (offset, value)
    | offset <- [0 .. B.length bytes - 4]
    , let value = wordAt offset
    , low <= value
    , value < high
    ]
  movPairs =
    [ (offset, value)
    | offset <- [0, 4 .. B.length bytes - 8]
    , let lower = wordAt offset
    , isMovw lower
    , upperOffset <- [offset + 4, offset + 8 .. min (offset + 32) (B.length bytes - 4)]
    , let upper = wordAt upperOffset
    , isMovt upper
    , register lower == register upper
    , let value = (immediate upper `shiftL` 16) .|. immediate lower
    , low <= value
    , value < high
    ]
  wordAt offset =
    sum
      [fromIntegral (B.index bytes (offset + index)) `shiftL` (8 * index) | index <- [0 .. 3]]
  isMovw word = word .&. 0x0ff00000 == 0x03000000
  isMovt word = word .&. 0x0ff00000 == 0x03400000
  register word = (word `shiftR` 12) .&. 15
  immediate word = ((word `shiftR` 16) .&. 15) `shiftL` 12 .|. (word .&. 0xfff)

word32 :: Word32 -> B.ByteString
word32 word = B.pack [fromIntegral ((word `div` (256 ^ index)) `mod` 256) | index <- [0 .. 3 :: Int]]
