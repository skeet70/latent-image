-- | The stock role of a file-service full-system pair: the same bank, record and loaders with
-- the file service's retired image-4 extents left stock.
module Replacement.FileService.StockRole
  ( Publication (..)
  , retiredRegion
  , retiredExtents
  , stockRolePublications
  ) where

import qualified ArmElf as E
import Data.Word (Word32)
import Replacement.FileService.Native (Extent (..), extentContains, hexWord)

-- | One stage-fixed-image publication row.
data Publication = Publication {publicationAddress :: Word32, publicationOld :: Word32, publicationNew :: Word32}
  deriving (Eq, Show)

-- | retired-memory.ld's RETIRED region.
retiredRegion :: Extent
retiredRegion = Extent 0x71bc2c 0x721834

-- | The linked sections retired.ld places in 'retiredRegion'.
retiredExtents :: E.Elf -> [Extent]
retiredExtents elf =
  [ Extent (E.sectionAddress section) (E.sectionAddress section + E.sectionSize section)
  | section <- E.elfSections elf
  , E.sectionAllocated section
  , E.sectionType section == 1
  , E.sectionSize section > 0
  , extentContains retiredRegion (E.sectionAddress section)
  ]

-- | The rows outside 'retiredRegion', in order. Refused when no row lies in a retired extent or
-- a row in the region lies outside every retired extent.
stockRolePublications :: [Extent] -> [Publication] -> Either String [Publication]
stockRolePublications extents rows
  | null dropped = Left "no publication lies in a retired extent"
  | (row : _) <- filter (\row -> not (any (`extentContains` publicationAddress row) extents)) dropped =
      Left ("publication " ++ hexWord (publicationAddress row) ++ " lies in the retired region outside every retired extent")
  | otherwise = Right kept
  where
    inRegion = extentContains retiredRegion . publicationAddress
    dropped = filter inRegion rows
    kept = filter (not . inRegion) rows
