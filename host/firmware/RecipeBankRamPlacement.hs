-- | The recipe bank's RAM-push layout: 253 publications in an 0x1d9d8-byte
-- allocation. RecipeBankRamLayout keeps the earlier 250-entry layout, which
-- the Classic Chrome builders' pinned identities cover, and the installer and
-- publication helpers this module re-exports.
module RecipeBankRamPlacement
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

import Data.Bits ((.&.), (.|.))
import Data.Word (Word32)

import RecipeBankRamLayout
  ( encodePublicationRecord
  , installerHasCallClosure
  , publicationSetupCount
  , runtimeExecutableBase
  , runtimeInstallerExtent
  , staleFixedReferences
  )

runtimeDataBase :: Word32 -> Word32
runtimeDataBase base = base + 0xa800

runtimeNameBase :: Word32 -> Word32
runtimeNameBase base =
  let minimumName = base + 0xbb00
      candidate = (minimumName .&. 0xffff0000) .|. 0xb784
   in if candidate < minimumName then candidate + 0x10000 else candidate

runtimeBankEnd :: Word32 -> Word32
runtimeBankEnd base = runtimeNameBase base + 14 * 66

runtimeAllocationSize :: Word32
runtimeAllocationSize = 0x1d9d8

-- | Logical indexes in table order: the bank's 69 setup entries, D3's 98
-- setup entries (logical 97..194), D3's 58 activation entries (logical
-- 195..252), then the bank's 28 activation entries ending with root 54.
safePublicationOrder :: [Int]
safePublicationOrder =
  [0 .. 35] <> [55 .. 68] <> [88] <> [36 .. 53]
    <> [97 .. 252]
    <> [75,76,72,73,74,69,70,71,77,78,79,80,81,82,83,89,90,91,92,93,94,95,96,84,85,86,87,54]

publicationCount :: Int
publicationCount = 253
