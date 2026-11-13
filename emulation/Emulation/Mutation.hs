{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Wrong variants of an owned binary: one sampled site, one changed word.
--
-- The binary a comparison suite loads for the owned role is a linked ELF.
-- This module takes the extents of the functions under comparison, chosen from
-- that ELF's symbols, and produces variants that each differ from it in one
-- word: a conditional branch whose condition is inverted, or a store replaced
-- by a no-op that keeps the instruction's condition. Sites are drawn with the
-- seed handling of "Emulation.ScenarioGeneration", so a recorded seed and
-- generator version regenerate the same variants.
--
-- A site must be an instruction the decoder classifies with certainty and
-- must not be data. Data is excluded three ways: an @$d@ mapping symbol makes
-- the region data until the next mapping symbol; a word that a PC-relative
-- load or address computation targets is data; and only words reachable by
-- following the extent's control flow from its entry are instructions. Thumb
-- extents are refused rather than decoded as A32.
module Emulation.Mutation
  ( MutantKind (..)
  , mutantKindLabel
  , Extent (..)
  , extentOfSymbol
  , extentsFor
  , Site (..)
  , mutationVersion
  , mutationCountLimit
  , MutationError (..)
  , renderMutationError
  , eligibleSites
  , successors
  , planMutations
  , mutatedWord
  , Variant (..)
  , variantControlName
  , patchElf
  , VariantOutcome (..)
  , variantControl
  ) where

import qualified A32
import qualified ArmElf as E
import Data.Bits (shiftR, xor, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (isInfixOf, sortOn)
import Data.Maybe (mapMaybe)
import qualified Data.Set as S
import Data.Word (Word32)
import Emulation.Judge (ReportControl (..))
import Numeric (showHex)
import Emulation.ScenarioGeneration
  ( GeneratorVersion (..)
  , ScenarioSeed (..)
  , drawWord64
  , rngFor
  , scenarioCountLimit
  )

-- | What one variant changes.
data MutantKind = BranchInverted | StoreDropped
  deriving (Eq, Ord, Show, Enum, Bounded)

mutantKindLabel :: MutantKind -> String
mutantKindLabel BranchInverted = "branch-inverted"
mutantKindLabel StoreDropped = "store-dropped"

-- | One function extent under comparison, from its symbol.
data Extent = Extent
  { extentName :: String
  , extentStart :: Word32
  , extentSize :: Word32
  } deriving (Eq, Ord, Show)

extentOfSymbol :: E.Symbol -> Maybe Extent
extentOfSymbol symbol
  | E.symbolKind symbol /= E.FunctionSymbol = Nothing
  | E.symbolSize symbol == 0 = Nothing
  | odd (E.symbolValue symbol) = Nothing
  | otherwise = Just (Extent (E.symbolName symbol) (E.symbolValue symbol) (E.symbolSize symbol))

-- | The extents of the named functions and of every function whose mangled
-- name contains one of the crate names, in address order.
extentsFor :: E.Elf -> [String] -> [String] -> Either String [Extent]
extentsFor elf functions crates
  | null functions && null crates = Left "no function name and no crate name was given"
  | otherwise = case dedupeExtents (sortOn extentStart (mapMaybe extentOfSymbol selected)) of
      [] -> Left "no function symbol matched the selection"
      extents -> Right extents
 where
  selected =
    [ symbol
    | symbol <- E.elfSymbols elf
    , E.symbolSection symbol /= E.Undefined
    , E.symbolKind symbol == E.FunctionSymbol
    , E.symbolSize symbol > 0
    , E.symbolName symbol `elem` functions || any (`isInfixOf` E.symbolName symbol) crates
    ]

dedupeExtents :: [Extent] -> [Extent]
dedupeExtents [] = []
dedupeExtents (extent : rest) =
  extent : dedupeExtents (dropWhile (\other -> extentStart other == extentStart extent) rest)

-- | One instruction a variant may change.
data Site = Site
  { siteAddress :: Word32
  , siteWord :: Word32
  , siteKind :: MutantKind
  } deriving (Eq, Ord, Show)

-- | Bumped when the site rule or the mutated words change, so a recorded seed
-- is never read against a different generator.
mutationVersion :: GeneratorVersion
mutationVersion = GeneratorVersion "mutants-2"

-- | The corpus bound shared with the scenario generator.
mutationCountLimit :: Int
mutationCountLimit = scenarioCountLimit

data MutationError
  = InvalidVariantCount Int Int
  | TooFewSites Int Int
  deriving (Eq, Show)

renderMutationError :: MutationError -> String
renderMutationError (InvalidVariantCount requested limit) =
  "variant count " ++ show requested ++ " is outside 1.." ++ show limit
renderMutationError (TooFewSites requested available) =
  show requested ++ " variants requested but only " ++ show available ++ " eligible sites exist"

-- | Every instruction inside the extents that may be changed, sorted by
-- address. A Thumb extent refuses the whole selection: its words are not A32.
eligibleSites :: E.Elf -> [Extent] -> Either String [Site]
eligibleSites elf extents
  | null extents = Left "no function extent was selected"
  | otherwise = do
      thumbCheck
      let mappings = mappingSymbols elf
          targets = dataTargets elf extents
      pure (dedupe (sortOn siteAddress (concatMap (sitesIn elf mappings targets) extents)))
 where
  thumbCheck =
    case [ (extentName extent, extentStart extent)
         | extent <- extents
         , extentStart extent .&. 1 == 1
         ] ++
         [ (extentName extent, E.symbolValue symbol)
         | extent <- extents
         , symbol <- E.elfSymbols elf
         , take 2 (E.symbolName symbol) == "$t"
         , E.symbolValue symbol >= extentStart extent
         , E.symbolValue symbol < extentStart extent + extentSize extent
         ] ++
         [ ("image entry", E.elfEntry elf) | E.elfEntry elf .&. 1 == 1 ] of
      [] -> Right ()
      ((name, address) : _) ->
        Left ("Thumb code in the compared extents at " ++ name ++ " 0x" ++ hex address ++ "; only A32 is mutated")

dedupe :: [Site] -> [Site]
dedupe = go S.empty
  where
    go _ [] = []
    go seen (site : rest)
      | S.member (siteAddress site) seen = go seen rest
      | otherwise = site : go (S.insert (siteAddress site) seen) rest

-- Mapping symbols ---------------------------------------------------------

data Mapping = Mapping
  { mappingAddress :: Word32
  , mappingData :: Bool
  } deriving (Eq, Show)

-- | @$a@ and @$d@ mapping symbols, sorted. @$t@ never reaches here: a Thumb
-- extent is refused before sites are chosen.
mappingSymbols :: E.Elf -> [Mapping]
mappingSymbols elf = sortOn mappingAddress
  [ Mapping (E.symbolValue symbol) (take 2 (E.symbolName symbol) == "$d")
  | symbol <- E.elfSymbols elf
  , take 2 (E.symbolName symbol) `elem` ["$a", "$d"]
  ]

inDataRegion :: [Mapping] -> Word32 -> Bool
inDataRegion mappings address =
  case [mapping | mapping <- mappings, mappingAddress mapping <= address] of
    [] -> False
    found -> mappingData (last found)

-- | Words a PC-relative load or address computation targets are data, whatever
-- they decode to.
dataTargets :: E.Elf -> [Extent] -> S.Set Word32
dataTargets elf extents = S.fromList (concatMap scan extents)
  where
    scan extent =
      [ target
      | address <- [extentStart extent, extentStart extent + 4 .. extentStart extent + extentSize extent - 4]
      , Just word <- [E.wordAt elf address]
      , target <- case A32.decode address word of
          A32.Load _ width _ _ base (A32.OffsetImmediate offset) _
            | base == A32.pc ->
                let start = fromIntegral address + 8 + fromIntegral offset :: Int
                 in [fromIntegral start .. fromIntegral (start + widthBytes width - 4)]
          A32.DataProcessing _ operation _ _ base (A32.Immediate immediate)
            | base == A32.pc
            , operation `elem` [A32.ADD, A32.SUB] ->
                let value = fromIntegral immediate :: Int
                    delta = if operation == A32.ADD then value else negate value
                 in [fromIntegral (fromIntegral address + 8 + delta :: Int)]
          _ -> []
      ]
    widthBytes A32.Dual = 8
    widthBytes _ = 4

-- Reachability ------------------------------------------------------------

sitesIn :: E.Elf -> [Mapping] -> S.Set Word32 -> Extent -> [Site]
sitesIn elf mappings targets extent = walk [extentStart extent] S.empty []
  where
    end = extentStart extent + extentSize extent
    inside address = address >= extentStart extent && address < end
    walk [] _ found = reverse found
    walk (address : rest) visited found
      | not (inside address) || address .&. 3 /= 0 = walk rest visited found
      | S.member address visited = walk rest visited found
      | inDataRegion mappings address || S.member address targets = walk rest visited found
      | otherwise = case E.wordAt elf address of
          Nothing -> walk rest visited found
          Just word ->
            let instruction = A32.decode address word
                found' = case eligible instruction of
                  Nothing -> found
                  Just kind -> Site address word kind : found
             in walk (successors address instruction ++ rest) (S.insert address visited) found'

-- | The successor addresses of an instruction the walk may continue into.
-- A call returns, so a linked branch or a linked register call continues at the
-- next instruction; every other PC write ends the walk.
successors :: Word32 -> A32.Instruction -> [Word32]
successors address instruction = case instruction of
  A32.Branch (A32.Cond 14) False target -> [target]
  A32.Branch _ False target -> [address + 4, target]
  A32.Branch _ True target -> [address + 4, target]
  A32.BranchExchange _ True _ -> [address + 4]
  A32.BranchExchange _ False _ -> []
  A32.LoadMultiple _ _ _ _ registers | A32.pc `elem` registers -> []
  A32.Load _ _ _ register _ _ _ | register == A32.pc -> []
  A32.DataProcessing _ _ _ register _ _ | register == A32.pc -> []
  A32.Compute _ registers | A32.pc `elem` registers -> []
  A32.Multiply _ register _ _ _ | register == A32.pc -> []
  A32.Rejected _ -> []
  _ -> [address + 4]

-- | The forms this tool mutates, and only those it can classify: a
-- conditional plain branch, a word or byte or halfword store, and a block
-- store. The decoder already refuses the unpredictable pre/post-indexed
-- combinations and the banked block store.
eligible :: A32.Instruction -> Maybe MutantKind
eligible instruction = case instruction of
  A32.Branch (A32.Cond condition) False _ | condition /= 14 -> Just BranchInverted
  A32.Store _ width register _ _ _ | width /= A32.Dual, register /= A32.pc -> Just StoreDropped
  A32.StoreMultiple _ base _ writeback registers
    | not (null registers)
    , A32.pc `notElem` registers
    , base /= A32.pc
    , not (writeback && base `elem` registers) -> Just StoreDropped
  _ -> Nothing

-- Variants ----------------------------------------------------------------

-- | One wrong variant: the seed and index that regenerate it, the site, and
-- both words.
data Variant = Variant
  { variantSeed :: ScenarioSeed
  , variantIndex :: Int
  , variantExtent :: String
  , variantAddress :: Word32
  , variantOriginal :: Word32
  , variantMutated :: Word32
  , variantKind :: MutantKind
  } deriving (Eq, Show)

-- | Draw @count@ distinct sites from one seed. Index @i@ draws from its own
-- stream and removes what it drew, so the same seed and site list always give
-- the same variants.
planMutations :: ScenarioSeed -> Int -> [(Extent, Site)] -> Either MutationError [Variant]
planMutations seed count sites
  | count < 1 || count > mutationCountLimit = Left (InvalidVariantCount count mutationCountLimit)
  | count > length sites = Left (TooFewSites count (length sites))
  | otherwise = Right (go 0 sites [])
 where
  go index pool found
    | index >= count = reverse found
    | otherwise =
        let draw = fst (drawWord64 (rngFor seed index))
            at = fromIntegral (draw `mod` fromIntegral (length pool))
            ((extent, site), rest) = removeAt at pool
         in go (index + 1) rest
              ( Variant
                  { variantSeed = seed
                  , variantIndex = index
                  , variantExtent = extentName extent
                  , variantAddress = siteAddress site
                  , variantOriginal = siteWord site
                  , variantMutated = mutatedWord (siteKind site) (siteWord site)
                  , variantKind = siteKind site
                  }
              : found
              )

removeAt :: Int -> [a] -> (a, [a])
removeAt index items = (items !! index, take index items ++ drop (index + 1) items)

-- | The one word a variant changes. A store becomes @mov r0, r0@ under the
-- store's own condition: no memory write, no flags and no register change.
mutatedWord :: MutantKind -> Word32 -> Word32
mutatedWord BranchInverted word = word `xor` 0x10000000
mutatedWord StoreDropped word = (word .&. 0xf0000000) .|. 0x01a00000

-- | The owned binary with the variant's one word replaced.
patchElf :: E.Elf -> Variant -> Either String B.ByteString
patchElf elf variant = do
  section <- case
    [ section
    | section <- E.elfSections elf
    , E.sectionAllocated section
    , E.sectionType section == 1
    , E.sectionExecutable section
    , E.sectionAddress section <= address
    , address < E.sectionAddress section + E.sectionSize section
    ] of
    [found] -> Right found
    [] -> Left ("no executable section holds the variant site 0x" ++ hex address)
    _ -> Left ("more than one executable section holds the variant site 0x" ++ hex address)
  original <- maybe (Left "the variant site is outside the ELF's section contents") Right (E.wordAt elf address)
  if original /= variantOriginal variant
    then Left ("the ELF's word at 0x" ++ hex address ++ " is 0x" ++ hex original ++ ", not the recorded 0x" ++ hex (variantOriginal variant))
    else
      let offset = fromIntegral (E.sectionOffset section + address - E.sectionAddress section)
       in Right (B.take offset bytes <> littleEndian (variantMutated variant) <> B.drop (offset + 4) bytes)
 where
  bytes = E.elfBytes elf
  address = variantAddress variant

littleEndian :: Word32 -> B.ByteString
littleEndian word = B.pack [fromIntegral (word `shiftR` (8 * index)) | index <- [0 :: Int .. 3]]

-- Judge shape -------------------------------------------------------------

-- | What the suite saw when it ran its cases against one variant.
data VariantOutcome = VariantOutcome
  { outcomeVariant :: Variant
  , outcomeDetected :: Bool
  , outcomeCases :: [String]
  , outcomeDifferences :: [String]
  } deriving (Eq, Show)

-- | A variant is a control row of the comparison report: the judge lists an
-- undetected one as a survivor under its name, which carries the address and
-- kind.
variantControl :: VariantOutcome -> ReportControl
variantControl outcome = ReportControl
  { reportControlName = variantControlName (outcomeVariant outcome)
  , reportControlDetected = outcomeDetected outcome
  , reportControlExpected = "a case reports different"
  , reportControlDifferences = outcomeDifferences outcome
  , reportControlDiagnostics = []
  }

variantControlName :: Variant -> String
variantControlName variant =
  "mutant-" ++ show (variantIndex variant)
    ++ "-" ++ mutantKindLabel (variantKind variant)
    ++ "-0x" ++ hex (variantAddress variant)

hex :: Word32 -> String
hex value = replicate (8 - length text) '0' ++ text
  where
    text = showHex value ""
