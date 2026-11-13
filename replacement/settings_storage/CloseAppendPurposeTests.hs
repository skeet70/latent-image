{-# LANGUAGE GHC2021 #-}
module Main (main) where

import qualified A32 as A
import qualified ArmElf as E
import Control.Monad (unless)
import qualified ControlArtifact as C
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as Text
import Data.Either (isLeft)
import Data.Word (Word32)
import Emulation.OwnedCase (word32le)
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
  let input = P.ArtifactInput controlBin controlSymbols controlElf
  _ <- either fail pure (C.deriveCloseAppendArtifact P.ProductionArtifact source input)
  _ <- either fail pure (C.deriveCloseAppendArtifact P.CloseAppendArea3Control source input)
  putStrLn "normalized close-append control admitted under both purposes"
  elf <- either fail pure (E.parseElf controlElf)
  references <- either fail pure (E.addressRelocations elf)
  (site, link) <- case [(E.addressSite reference, link) | reference <- references
    , E.addressTarget reference == 0x6ac7d4
    , Just instruction <- [E.wordAt elf (E.addressSite reference)]
    , A.Branch _ link _ <- [A.decode (E.addressSite reference) instruction]] of
      [edge] -> pure edge
      _ -> fail "expected one close-append branch relocation"
  section <- case [item | item <- E.elfSections elf, E.sectionAllocated item
    , E.sectionAddress item <= site, toInteger site + 4 <= toInteger (E.sectionAddress item) + toInteger (E.sectionSize item)] of
      [item] -> pure item
      _ -> fail "expected close-append section"
  carrier@(L.Carrier (L.GuestAddress start) _) <- maybe (fail "missing canonical carrier") pure (L.hullCarrier (P.sourceHull source))
  let replacement = if link then 0xe320f000 else 0xe12fff1e
      patchedBin = patchWord (fromIntegral (site - start)) replacement controlBin
      staleElf = patchWord (fromIntegral (E.sectionOffset section + site - E.sectionAddress section)) replacement controlElf
      withoutAlias = Text.unlines [row | row <- Text.lines controlSymbols, case Text.words row of
        [_, _, kind, name] -> kind /= Text.pack "A" || name /= Text.pack "xm1_bank_record_append"
        _ -> True]
  unless (isLeft (P.deriveArtifact P.ProductionArtifact (P.sourceProperties source) carrier (P.ArtifactInput patchedBin withoutAlias staleElf)))
    (fail "rewritten close-append edge retained a stale branch relocation")
  putStrLn "refused rewritten close-append edge with stale relocation"

patchWord :: Int -> Word32 -> B.ByteString -> B.ByteString
patchWord offset value bytes = B.take offset bytes <> word32le value <> B.drop (offset + 4) bytes
