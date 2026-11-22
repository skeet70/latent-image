-- | The stock role of a file-service full-system pair: the same bank, record and loaders with
-- the file service's retired image-4 extents left stock.
module Replacement.FileService.StockRole
  ( Publication (..)
  , retiredRegions
  , retiredExtents
  , stockRolePublications
  ) where

import qualified ArmElf as E
import Data.Word (Word32)
import Replacement.FileService.Native (Extent (..), extentContains, hexWord)

-- | One stage-fixed-image publication row.
data Publication = Publication {publicationAddress :: Word32, publicationOld :: Word32, publicationNew :: Word32}
  deriving (Eq, Show)

-- | retired-memory.ld's regions.
retiredRegions :: [Extent]
retiredRegions =
  [Extent 0x7115a4 0x712890, Extent 0x715a58 0x715f0c, Extent 0x718a8c 0x71b9e8, Extent 0x71bc2c 0x721834]

-- | The linked sections retired.ld places in 'retiredRegions'.
retiredExtents :: E.Elf -> [Extent]
retiredExtents elf =
  [ Extent (E.sectionAddress section) (E.sectionAddress section + E.sectionSize section)
  | section <- E.elfSections elf
  , E.sectionAllocated section
  , E.sectionType section == 1
  , E.sectionSize section > 0
  , any (`extentContains` E.sectionAddress section) retiredRegions
  ]

-- | The rows outside 'retiredRegions', in order. Refused when no row lies in a retired extent or
-- a row in the region lies outside every retired extent.
stockRolePublications :: [Extent] -> [Publication] -> Either String [Publication]
stockRolePublications extents rows
  | null dropped = Left "no publication lies in a retired extent"
  | (row : _) <- filter (\row -> not (any (`extentContains` publicationAddress row) extents)) dropped =
      Left ("publication " ++ hexWord (publicationAddress row) ++ " lies in the retired region outside every retired extent")
  | otherwise = Right kept
  where
    inRegion row = any (`extentContains` publicationAddress row) retiredRegions
    dropped = filter inRegion rows
    kept = filter (not . inRegion) rows
