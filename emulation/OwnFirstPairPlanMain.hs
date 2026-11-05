{-# LANGUAGE OverloadedStrings #-}
-- | own-first-pair-plan: writes one role's observer plan, and for a guest control its DAT, staged
-- from the candidate through the pre-table loader-word path.
module Main (main) where

import Control.Monad (forM, unless, when)
import qualified Data.Aeson as A
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.OwnFirstPairPlan
import Emulation.Sha256 (sha256, sha256Hex)
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Replacement.RtosOwnFirst.Artifact
import Replacement.RtosOwnFirst.Pins (stockPins)
import Replacement.RtosOwnFirst.Regions (Domain (..), parseRegionTable)
import System.Directory (createDirectory, doesPathExist)
import System.Environment (getArgs)
import System.Exit (die)
import System.FilePath ((</>))

usage :: String
usage = "usage: own-first-pair-plan REGIONS BIN_A SYMBOLS_A BIN_B SYMBOLS_B --role stock|owned|control-restored|control-wrong-i \
  \--source-dat DAT --loader-a LOADER_A --loader-b LOADER_B --output NEW_DIRECTORY"

-- | Owned +0x164's `and r0, r0, #0x80`, and the control's `and r0, r1, #0x80`, which returns the set I bit.
wrongIAddress, ownedPriorI, wrongPriorI :: Word32
wrongIAddress = 0x41154
ownedPriorI = 0xe2000080
wrongPriorI = 0xe2010080

restoredExtent :: (Word32, Word32)
restoredExtent = (0x41140, 0x41164)

loaderA :: Word32
loaderA = 0x40000

wordAt :: B.ByteString -> Word32 -> Word32
wordAt loader address = U.word32At loader (fromIntegral (address - loaderA))

putWord :: Word32 -> Word32 -> B.ByteString -> B.ByteString
putWord address value loader =
  let offset = fromIntegral (address - loaderA) in B.take offset loader <> U.putWord32 value <> B.drop (offset + 4) loader

-- | Loader A word patches for the control, from the staged loader.
controlPatches :: Role -> B.ByteString -> B.ByteString -> Either String [F.LoaderWordPatch]
controlPatches role stock staged = case role of
  ControlRestoredRole ->
    let (start, end) = restoredExtent
     in sequence [ F.loaderWordPatch (F.LoaderId 0) address (wordAt staged address) (wordAt stock address)
                 | address <- [start, start + 4 .. end - 4], wordAt staged address /= wordAt stock address ]
  ControlWrongIRole -> do
    unless (wordAt staged wrongIAddress == ownedPriorI) (Left "owned +0x164 does not hold the reviewed prior-I instruction")
    sequence [F.loaderWordPatch (F.LoaderId 0) wrongIAddress ownedPriorI wrongPriorI]
  _ -> Right []

controlDat :: CanonicalSource -> Admitted -> B.ByteString -> [F.LoaderWordPatch] -> Either String B.ByteString
controlDat source admitted staged patches = do
  parsed <- U.parseContainer staged
  parts <- F.applyLoaderWordPatches patches (U.parsedParts parsed)
  bytes <- U.rebuildContainer parsed parts
  consumed <- U.parseContainer bytes
  F.verifyPreTablePatches [] patches parsed consumed
  either (Left . show) Right (confineChanges source admitted bytes)
  pure bytes

options :: [String] -> Either String (M.Map String String)
options (name : value : rest) = M.insert name value <$> options rest
options [] = Right M.empty
options _ = Left usage

main :: IO ()
main = do
  arguments <- getArgs
  (regionsPath, binA, symbolsA, binB, symbolsB, rest) <- case arguments of
    regions : a : sa : b : sb : rest -> pure (regions, a, sa, b, sb, rest)
    _ -> die usage
  named <- either die pure (options rest)
  unless (M.keys named == ["--loader-a", "--loader-b", "--output", "--role", "--source-dat"]) (die usage)
  role <- either die pure (parseRole (named M.! "--role"))
  let output = named M.! "--output"
      refuse what = either (die . (("own-first-pair-plan: " ++ what ++ ": ") ++)) pure
  exists <- doesPathExist output
  when exists (die "own-first-pair-plan: --output must be new")
  [regions, linkedA, linkedSymbolsA, linkedB, linkedSymbolsB, dat, stockA, stockB] <-
    forM [regionsPath, binA, symbolsA, binB, symbolsB, named M.! "--source-dat", named M.! "--loader-a", named M.! "--loader-b"] B.readFile
  source <- refuse "source" (either (Left . show) Right (canonicalSource dat stockA stockB >>= \parsed -> parsed <$ requirePinned parsed))
  admitted <- refuse "admit" (either (Left . show) Right (admit stockPins source regions (LinkedDomain linkedA linkedSymbolsA) (LinkedDomain linkedB linkedSymbolsB)))
  staged <- refuse "stage" (either (Left . show) Right (stage source admitted))
  table <- refuse "regions" (parseRegionTable regions)
  let stagedA = stagedLoader staged DomainA
      stagedB = stagedLoader staged DomainB
  patches <- refuse "control" (controlPatches role stockA stagedA)
  (roleDat, loaders) <- case role of
    StockRole -> pure (dat, M.fromList [(DomainA, stockA), (DomainB, stockB)])
    OwnedRole -> pure (stagedDat staged, M.fromList [(DomainA, stagedA), (DomainB, stagedB)])
    ControlRestoredRole -> do
      bytes <- refuse "control DAT" (controlDat source admitted (stagedDat staged) patches)
      pure (bytes, M.fromList [(DomainA, stagedA), (DomainB, stagedB)])
    ControlWrongIRole -> do
      bytes <- refuse "control DAT" (controlDat source admitted (stagedDat staged) patches)
      pure (bytes, M.fromList [(DomainA, putWord wrongIAddress wrongPriorI stagedA), (DomainB, stagedB)])
  plan <- refuse "plan" (derivePlan role (sha256Hex (sha256 roleDat)) table loaders)
  createDirectory output
  BL.writeFile (output </> "plan.json") (A.encode (planValue plan) <> "\n")
  unless (null patches) (B.writeFile (output </> "control.DAT") roleDat)
  putStrLn ("own-first-pair-plan: " ++ roleWire role ++ " plan for DAT " ++ planDatSha256 plan)
