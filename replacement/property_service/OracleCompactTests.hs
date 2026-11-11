{-# LANGUAGE GHC2021 #-}
module OracleCompactTests (main) where

import Control.Exception (IOException, try)
import Control.Monad (forM_, unless, void)
import Data.Either (isLeft, isRight)
import qualified OracleMain as Oracle
import Replacement.PropertyService.Compact (AllocationGeometry (Btrfs4096), ByteCount (..))
import Replacement.PropertyService.Qualification (StorageLimitBytes (..), StorageBytes (..), StorageSnapshot (..))

check :: String -> Bool -> IO ()
check label condition = unless condition (fail label)

main :: IO ()
main = do
  check "ordinary execution is unchanged" . (== Oracle.OrdinaryResearch) =<< Oracle.researchMode []
  let arguments = ["--compact-measurement", "--native-only"]
  check "measured allocation geometry is admitted" (Oracle.parseCompactAllocationGeometry "btrfs 4096 4096\n" == Right Btrfs4096)
  forM_ ["", "ext4 4096 4096\n", "btrfs 512 4096\n", "btrfs 4096 65536\n", "btrfs 4096 4096\nbtrfs 4096 4096\n"] $ \measured ->
    check "unknown allocation geometry cannot create an emergency allowance" (isLeft (Oracle.parseCompactAllocationGeometry measured))
  check "compact measurement is explicit" . (== Oracle.CompactMeasurement) =<< Oracle.researchMode arguments
  refused "measurement requires native-only input" ["--compact-measurement"]
  refused "measurement cannot be duplicated" (arguments ++ ["--compact-measurement"])
  forM_ ["--smoke", "--ids", "--cache", "--rejudge-run-root", "--rejudge-results-sha256", "--batch-checkpoint", "--candidate-image4", "--contract"] $ \option ->
    refused "measurement cannot narrow coverage or reuse observations" (arguments ++ [option, "unused"])
  let limit = StorageLimitBytes 75161927680
      pending = 67108864
      used = 72736874496
      floorBytes = 135115554816
      snapshot bytes free = StorageSnapshot (StorageBytes bytes) (StorageBytes free) (StorageBytes 0)
      judge rootBytes free outputBytes reserve = Oracle.checkCompactBudget limit (snapshot rootBytes free) (snapshot outputBytes free) (ByteCount reserve)
  check "bounded full-counterexample reservation is admitted" (isRight (judge used 162094735360 0 pending))
  check "exact remaining physical floor is admitted" (isRight (judge used (floorBytes + pending) 0 pending))
  check "reservation cannot spend the fixed physical floor" (isLeft (judge used (floorBytes + pending - 1) 0 pending))
  check "root checkpoint margin is not spendable" (isLeft (judge (75161927680 - 2147483648 - pending + 1) 162094735360 0 pending))
  check "exact root margin remains usable" (isRight (judge (75161927680 - 2147483648 - pending) 162094735360 0 pending))
  check "retention cannot exceed admitted bytes" (isLeft (judge used 162094735360 (201326592 - pending + 1) pending))
  check "exact retention capacity is admitted" (isRight (judge used 162094735360 (201326592 - pending) pending))
  check "negative reservations cannot create space" (isLeft (judge used 162094735360 0 (-1)))
  where
    refused label arguments = do
      result <- try (void (Oracle.researchMode arguments)) :: IO (Either IOException ())
      check label (isLeft result)
