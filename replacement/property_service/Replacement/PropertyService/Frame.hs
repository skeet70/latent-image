-- | The private stack frame of one native case: the bytes below its entry SP that its own execution
-- may leave changed, derived from the executable regions the case maps.
module Replacement.PropertyService.Frame
  ( FrameDepth (..)
  , caseFrameDepth
  , withPrivateFrame
  ) where

import Control.Monad (unless, when)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.NativeCase (Region (..))
import Emulation.NativeObservation (GuestAddress (..), Register (Sp), RegionId (RegionId), regionBounds)
import Emulation.OwnedCase (CaseRegion (..), CaseSpec (..), IgnoredMemory (..), IgnoredReason (PrivateFrame), Permission (ReadExecute, ReadWrite))
import qualified Replacement.PropertyService.HeaderBoundary as H
import qualified StockStack as S

-- | Bytes below an entry SP that a case's own execution reaches.
newtype FrameDepth = FrameDepth Int deriving (Eq, Ord, Show)

-- | The depth of the case's entry path: a static bound over the code its executable regions hold,
-- with owned entries taken at their supplied depths and owned ranges entered only there.
caseFrameDepth :: [(Word32, Word32)] -> S.Supplied -> CaseSpec -> Either S.StackRefusal FrameDepth
caseFrameDepth ownedRanges supplied spec =
  case S.stockDepth (S.stockCode executableRegions ownedRanges) supplied indirect S.noSwitches entry of
    Left refusal -> Left refusal
    Right report -> Right (FrameDepth (fromIntegral (depthBytes (S.reportDepth report))))
  where
    executableRegions = [(fromIntegral (baseAddress region), regionBytes region) | CaseRegion region ReadExecute <- caseRegions spec]
    entry = fromIntegral (guestAddress (caseEntry spec))
    depthBytes (S.StackDepth depth) = depth

-- | The stock interface root table and the slot stubs the property cases enter.
indirect :: S.IndirectTargets
indirect = S.IndirectTargets H.kernelRootBase (M.fromList [(S.SlotOffset offset, [address]) | (offset, address) <- H.slotStubs]) M.empty []

-- | Declares the top @depth@ bytes of the case's writable stack region as its private frame. That
-- region ends at the entry SP and holds no expected byte.
withPrivateFrame :: FrameDepth -> CaseSpec -> Either String CaseSpec
withPrivateFrame (FrameDepth depth) spec = do
  pointer <- maybe (Left "case declares no entry SP") Right (lookup Sp (caseInitial spec))
  (low, size) <- case [(low, size) | (RegionId "stack", GuestAddress low, size) <- regionBounds writable] of
    [bounds] -> Right bounds
    _ -> Left "case declares no single writable stack region"
  unless (toInteger low + toInteger size == toInteger pointer) (Left "stack region does not end at the entry SP")
  unless (depth > 0) (Left "private frame depth is not positive")
  unless (depth <= size) (Left "private frame is deeper than the stack region")
  when (any ((== RegionId "stack") . fst) (caseExpectedMemory spec)) (Left "the case expects stack bytes inside its private frame")
  Right spec {caseIgnoredMemory = [IgnoredMemory (RegionId "stack") (size - depth) depth PrivateFrame]}
  where
    writable = [region | CaseRegion region ReadWrite <- caseRegions spec]
