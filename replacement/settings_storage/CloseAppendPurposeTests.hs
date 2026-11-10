{-# LANGUAGE GHC2021 #-}
module Main (main) where

import qualified A32 as A
import qualified ArmElf as E
import Control.Monad (forM_, unless)
import qualified ControlArtifact as C
import qualified Data.ByteString as B
import Data.Either (isLeft)
import Data.List (nub)
import Data.Word (Word32)
import Emulation.OwnedCase (readWordAt, word32le)
import qualified Replacement.PropertyService.Artifact as P
import qualified Replacement.PropertyService.Placement as L
import System.Environment (getArgs)

main :: IO ()
main = do
  [datPath,image4Path,loaderAPath,loaderBPath,image5Path,plainPath,image2Path,controlBinPath,controlSymbolsPath,controlElfPath] <- getArgs
  dat <- B.readFile datPath
  image4 <- B.readFile image4Path
  loaderA <- B.readFile loaderAPath
  loaderB <- B.readFile loaderBPath
  image5 <- B.readFile image5Path
  plain <- B.readFile plainPath
  image2 <- B.readFile image2Path
  source <- either (fail . show) pure (P.canonicalSource dat image4 loaderA loaderB image5 plain image2)
  controlBin <- B.readFile controlBinPath
  controlSymbols <- B.readFile controlSymbolsPath
  controlElf <- B.readFile controlElfPath
  elf <- either (fail . ("control ELF: " ++)) pure (E.parseElf controlElf)
  let input = P.ArtifactInput controlBin controlSymbols controlElf
  unless (isLeft (C.deriveCloseAppendArtifact P.ProductionArtifact source input))
    (fail "production purpose admitted the close-appends control's outside-owned handle")
  _ <- either fail pure (C.deriveCloseAppendArtifact P.CloseAppendArea3Control source input)
  carrier <- maybe (fail "canonical property hull has no carrier") pure (L.hullCarrier (P.sourceHull source))
  let L.Carrier (L.GuestAddress start) _ = carrier
      handle = 0x89f86c
      unrelated = 0x409c29b7
      targets = handleLiteralTargets elf handle
  unless (not (null targets)) (fail "control ELF has no declared area-3 handle literal")
  forM_ targets $ \address ->
    unless (toInteger address >= toInteger start
      && toInteger address + 4 <= toInteger start + toInteger (B.length controlBin))
      (fail "control literal target lies outside the original BIN carrier")
  let mutatedBin = foldl (\bytes address -> patchWord (fromIntegral (address - start)) unrelated bytes) controlBin targets
      mutatedElf = foldl (\bytes address -> patchElfWord elf address unrelated bytes) controlElf targets
      mutated = P.ArtifactInput mutatedBin controlSymbols mutatedElf
  mutatedParsedElf <- either (fail . ("mutated control ELF: " ++)) pure (E.parseElf mutatedElf)
  forM_ targets $ \address -> do
    unless (readWordAt mutatedBin (fromIntegral (address - start)) == unrelated)
      (fail "binary handle literal was not mutated")
    unless (E.wordAt mutatedParsedElf address == Just unrelated)
      (fail "ELF handle literal was not mutated")
  unless (isLeft (C.deriveCloseAppendArtifact P.CloseAppendArea3Control source mutated))
    (fail "close-append purpose admitted an unrelated pointer literal")
  putStrLn "close-append purpose regression passed"

handleLiteralTargets :: E.Elf -> Word32 -> [Word32]
handleLiteralTargets elf handle = nub
  [ target
  | section <- E.elfSections elf
  , E.sectionAllocated section
  , E.sectionExecutable section
  , E.sectionType section == 1
  , address <- wordAddresses (E.sectionAddress section) (E.sectionSize section)
  , Just word <- [E.wordAt elf address]
  , A.Load _ width _ _ base (A.OffsetImmediate displacement) _ <- [A.decode address word]
  , base == A.pc
  , width == A.Word || width == A.Dual
  , let literal = toInteger address + 8 + toInteger displacement
  , literal >= 0
  , literal <= 0xffffffff
  , target <- if width == A.Dual then [fromInteger literal, fromInteger literal + 4] else [fromInteger literal]
  , E.wordAt elf target == Just handle
  ]

wordAddresses :: Word32 -> Word32 -> [Word32]
wordAddresses start size
  | size < 4 = []
  | otherwise = [start, start + 4 .. start + size - 4]

patchWord :: Int -> Word32 -> B.ByteString -> B.ByteString
patchWord offset value bytes = B.take offset bytes <> word32le value <> B.drop (offset + 4) bytes

patchElfWord :: E.Elf -> Word32 -> Word32 -> B.ByteString -> B.ByteString
patchElfWord elf address value bytes = case
  [section | section <- E.elfSections elf,
    E.sectionAllocated section, E.sectionType section == 1,
    toInteger (E.sectionAddress section) <= toInteger address,
    toInteger address + 4 <= toInteger (E.sectionAddress section) + toInteger (E.sectionSize section)] of
  [section] -> patchWord (fromIntegral (E.sectionOffset section + address - E.sectionAddress section)) value bytes
  _ -> error "control ELF literal section is missing or ambiguous"
