{-# LANGUAGE GHC2021 #-}

module Replacement.PropertyService.RecipeEditNative
  ( RecipeFixtureError (..)
  , fixtureErrorDetail
  , writeRecipeCase
  , recipeCaseSpec
  , adjacentCaseSpec
  , noEffectDifferences
  ) where

import qualified ArmElf as ELF
import Control.Monad (unless)
import qualified Data.ByteString as B
import Data.List (nub, sort)
import qualified Data.Map.Strict as M
import qualified Emulation.NativeCase as Native
import qualified Emulation.NativeObservation as N
import Emulation.OwnedCase (CaseRegion (..), CaseSpec (..), Permission (..), mkRegion, writeCase)
import Replacement.PropertyService.Admission (FunctionExtent (..))
import Replacement.PropertyService.Artifact
import qualified Replacement.PropertyService.Classify as C
import Replacement.PropertyService.Oracle
import qualified Replacement.PropertyService.Placement as P
import Replacement.PropertyService.RecipeArtifact
import Replacement.PropertyService.RecipeEditOracle
import qualified Replacement.RtosTimer.BankPackage as Package

-- Source-bound package initialization is not an installed-lifetime witness.
data RecipeFixtureError
  = ActiveSessionContextRequired
  | ArtifactStagingRefused ArtifactError
  | PackageLayoutRefused String
  | ExpectationUnavailable
  deriving (Eq, Show)

fixtureErrorDetail :: RecipeFixtureError -> String
fixtureErrorDetail failure = case failure of
  ActiveSessionContextRequired -> "active session requires execution or an admitted snapshot of actual rb_stage_edit_open(slot, RbMediaIdentity), including rb_stage_load and its real media/task/UI services; the package interface exposes no isolated active-session initializer"
  ArtifactStagingRefused failure_ -> show failure_
  PackageLayoutRefused detail -> detail
  ExpectationUnavailable -> "independent expectation is unavailable for this case"

recipeCaseSpec :: CanonicalSource -> RecipeArtifact -> RecipeSession -> RecipeCase -> Either RecipeFixtureError CaseSpec
recipeCaseSpec _ _ (SessionActive _) _ = Left ActiveSessionContextRequired
recipeCaseSpec source artifact SessionInactive testCase@(RecipeCase host operation) = do
  image <- either (Left . ArtifactStagingRefused) Right (stagedImage4Bytes source (recipeLinkedArtifact artifact))
  let fx = fixture image
      baseOperation = case operation of
        RecipeGet _ _ (SlotArgumentRaw slot) -> Get Bank0 LiveRead (SlotArgument slot)
        RecipeSet _ _ (SlotArgumentRaw slot) -> Set Bank0 (SlotArgument slot) SetZero
      oracleCase = OracleCase (hostProperty host) baseOperation
  expected <- maybe (Left ExpectationUnavailable) Right (recipeExpectation fx SessionInactive testCase)
  spec <- withPackage artifact (caseSpec image (artifactSpans artifact) fx (CaseLabel (show host ++ "-" ++ show operation)) (Classified (C.Owned C.OwnedUnassigned)) oracleCase expected)
  pure spec
    { caseInitial = recipeArguments testCase ++ filter (\(register, _) -> register `notElem` [N.R0, N.R1, N.R2, N.R3]) (caseInitial spec)
    , caseProvenance = "optional recipe-edit property module over the actual source-bound new-bank ELF, initially inactive session"
    }

adjacentCaseSpec :: CanonicalSource -> RecipeArtifact -> OracleCase -> Either RecipeFixtureError CaseSpec
adjacentCaseSpec source artifact oracleCase = do
  image <- either (Left . ArtifactStagingRefused) Right (stagedImage4Bytes source (recipeLinkedArtifact artifact))
  let fx = fixture image
      properties = M.fromList [(C.propertyId property, property) | property <- sourceProperties source]
  expected <- maybe (Left ExpectationUnavailable) Right (adjacentExpectation properties fx oracleCase)
  withPackage artifact (caseSpec image (artifactSpans artifact) fx (caseLabel oracleCase) (standingOf properties (oracleProperty oracleCase)) oracleCase expected)

artifactSpans :: RecipeArtifact -> [FunctionSpan]
artifactSpans artifact = splitFunctions (low, high)
  [FunctionSpan start end | FunctionExtent _ (P.GuestAddress start) (P.GuestAddress end) <- artifactFunctions linked]
  where
    linked = recipeLinkedArtifact artifact
    P.Carrier (P.GuestAddress low) (P.GuestAddress high) = artifactCarrier linked

withPackage :: RecipeArtifact -> CaseSpec -> Either RecipeFixtureError CaseSpec
withPackage artifact spec = do
  let package = recipePackage artifact
  elf <- either (Left . PackageLayoutRefused) Right (ELF.parseElf (Package.packageElfBytes package))
  let runtime = Package.packageRuntimeBytes package
      allocation = Package.packageAllocation package
      Package.BankAddress low = Package.extentStart allocation
      Package.BankAddress high = Package.extentEnd allocation
      sections = [section | section <- ELF.elfSections elf, ELF.sectionAllocated section, ELF.sectionSize section > 0]
      endOf section = toInteger (ELF.sectionAddress section) + toInteger (ELF.sectionSize section)
      within section = low <= ELF.sectionAddress section && endOf section <= toInteger high
      boundaries = sort (nub (low : high : concat [[ELF.sectionAddress section, fromInteger (endOf section)] | section <- sections]))
      intervals = zip boundaries (drop 1 boundaries)
      owners start end = [section | section <- sections, ELF.sectionAddress section <= start, toInteger end <= endOf section]
      covered function = any (\section -> ELF.sectionExecutable section && ELF.sectionAddress section <= ELF.symbolValue function && toInteger (ELF.symbolValue function) + toInteger (ELF.symbolSize function) <= endOf section) sections
      functions = nub [(N.GuestAddress (fromIntegral (ELF.symbolValue symbol)), ELF.symbolSize symbol) | symbol <- ELF.elfSymbols elf, ELF.symbolKind symbol == ELF.FunctionSymbol, ELF.symbolSize symbol > 0, covered symbol]
      binding = Package.getterBinding package
      Package.BankAddress getter = Package.getterEntry binding
      Package.BankAddress getterEnd = Package.extentEnd (Package.getterExtent binding)
      existing = map caseRegion (caseRegions spec)
      overlaps region = toInteger low < toInteger (Native.baseAddress region) + toInteger (Native.regionSize region) && toInteger (Native.baseAddress region) < toInteger high
  require (low < high && toInteger (B.length runtime) == toInteger high - toInteger low) "package runtime does not cover its allocation"
  require (all within sections && all (\(start, end) -> length (owners start end) <= 1) intervals) "package sections overlap or leave the bound allocation"
  require (not (any overlaps existing)) "package allocation overlaps the canonical fixture"
  require ((N.GuestAddress (fromIntegral getter), getterEnd - getter) `elem` functions) "bound callback is absent from actual package functions"
  require (all (\(address, size) -> N.guestAddress address `mod` 4 == 0 && size `mod` 4 == 0) functions) "native recipe fixture requires complete A32 package functions"
  mapped <- mapM (mapInterval low runtime owners) intervals
  pure spec
    { caseRegions = caseRegions spec ++ [region | (region, _) <- mapped]
    , caseFunctions = caseFunctions spec ++ functions
    , caseExpectedMemory = caseExpectedMemory spec ++ [(identifier, bytes) | (_, Just (identifier, bytes)) <- mapped]
    , caseExclusion = "one property call against the actual initially inactive package; no legacy recipe arms, active-session initialization, publication, or installed code lifetime is inferred"
    }
  where
    require premise detail = unless premise (Left (PackageLayoutRefused detail))
    mapInterval low runtime owners (start, end) = do
      permission <- case owners start end of
        [] -> Right ReadOnly
        [section]
          | ELF.sectionExecutable section && ELF.sectionWritable section -> Left (PackageLayoutRefused "package contains writable executable code")
          | ELF.sectionExecutable section -> Right ReadExecute
          | ELF.sectionWritable section -> Right ReadWrite
          | otherwise -> Right ReadOnly
        _ -> Left (PackageLayoutRefused "package sections overlap")
      let identifier = N.RegionId ("recipe-package-" ++ show start)
          bytes = B.take (fromIntegral (end - start)) (B.drop (fromIntegral (start - low)) runtime)
          region = mkRegion identifier (N.GuestAddress (fromIntegral start)) (fromIntegral (end - start)) permission bytes
      pure (region, case permission of ReadWrite -> Just (identifier, bytes); _ -> Nothing)

writeRecipeCase :: FilePath -> CanonicalSource -> RecipeArtifact -> RecipeSession -> RecipeCase -> IO (Either RecipeFixtureError FilePath)
writeRecipeCase directory source artifact session testCase =
  traverse (writeCase directory) (recipeCaseSpec source artifact session testCase)

