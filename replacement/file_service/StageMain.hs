{-# LANGUAGE OverloadedStrings #-}

-- | file-service-stage publications IMAGE4 ELF OUTPUT_JSON
--   file-service-stage body ELF OUTPUT_BIN
--   file-service-stage stock-role ELF PUBLICATIONS_JSON OUTPUT_JSON
--
-- publications: every word of the linked image-4 sections (entry branches and traps over the
-- retired extents) that differs from stock image 4, as stage-fixed-image rows.
-- body: the OWNED_FIXED bytes [__fixed_body_start, __fixed_body_end) of the same link.
-- stock-role: a bank's publication rows without the file service's retired extents of ELF.
module Main (main) where

import qualified ArmElf as E
import Control.Monad (unless)
import Data.Aeson (FromJSON (..), encode, object, withObject, (.:), (.=))
import qualified Data.Aeson as A
import Data.Bits (shiftL)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Word (Word32)
import Replacement.FileService.StockRole (Publication (..), retiredExtents, stockRolePublications)
import System.Environment (getArgs)

newtype Row = Row Publication

instance FromJSON Row where
  parseJSON = withObject "publication" $ \o -> fmap Row (Publication <$> o .: "address" <*> o .: "old" <*> o .: "new")

image4Base :: Word32
image4Base = 0x61c400

stockWord :: B.ByteString -> Word32 -> Word32
stockWord image address =
  sum [fromIntegral (B.index image (offset + index)) `shiftL` (8 * index) | index <- [0 .. 3]]
  where
    offset = fromIntegral (address - image4Base)

loadElf :: FilePath -> IO E.Elf
loadElf path = B.readFile path >>= either fail pure . E.parseElf

symbol :: E.Elf -> String -> IO Word32
symbol elf name = case [E.symbolValue entry | entry <- E.elfSymbols elf, E.symbolName entry == name] of
  [value] -> pure value
  _ -> fail (name ++ " is missing or repeated")

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ["publications", imagePath, elfPath, output] -> do
      image <- B.readFile imagePath
      elf <- loadElf elfPath
      let limit = image4Base + fromIntegral (B.length image)
          placed =
            [ section
            | section <- E.elfSections elf
            , E.sectionAllocated section
            , E.sectionType section == 1
            , E.sectionSize section > 0
            , E.sectionAddress section >= image4Base
            , E.sectionAddress section < limit
            ]
      unless (all (\section -> E.sectionAddress section `mod` 4 == 0 && E.sectionSize section `mod` 4 == 0 && E.sectionAddress section + E.sectionSize section <= limit) placed) $
        fail "an image-4 section is unaligned or leaves image 4"
      rows <- fmap concat . mapM (\section -> mapM (row image elf) [E.sectionAddress section, E.sectionAddress section + 4 .. E.sectionAddress section + E.sectionSize section - 4]) $ placed
      let changed = [(address, old, new) | (address, old, new) <- rows, old /= new]
      BL.writeFile output (encode [object ["address" .= address, "old" .= old, "new" .= new] | (address, old, new) <- changed])
      putStrLn (show (length changed) ++ " publication words over " ++ show (length placed) ++ " sections")
    ["body", elfPath, output] -> do
      elf <- loadElf elfPath
      start <- symbol elf "__fixed_body_start"
      end <- symbol elf "__fixed_body_end"
      let bytes = [maybe 0 id (E.byteAt elf address) | address <- [start .. end - 1]]
      B.writeFile output (B.pack bytes)
      putStrLn ("body " ++ show (length bytes) ++ " bytes")
    ["stock-role", elfPath, publicationsPath, output] -> do
      elf <- loadElf elfPath
      rows <- B.readFile publicationsPath >>= either fail pure . A.eitherDecodeStrict'
      kept <- either fail pure (stockRolePublications (retiredExtents elf) [publication | Row publication <- rows])
      BL.writeFile output (encode [object ["address" .= publicationAddress p, "old" .= publicationOld p, "new" .= publicationNew p] | p <- kept])
      putStrLn (show (length rows - length kept) ++ " retired-extent publication words dropped, " ++ show (length kept) ++ " kept")
    _ -> fail "usage: file-service-stage publications IMAGE4 ELF OUTPUT_JSON | body ELF OUTPUT_BIN | stock-role ELF PUBLICATIONS_JSON OUTPUT_JSON"
  where
    row image elf address = case E.wordAt elf address of
      Just new -> pure (address, stockWord image address, new)
      Nothing -> fail ("no linked word at " ++ show address)
