{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- Discharges `global-object-stack-separation` at callback store sites whose
-- address is a constant, by interval disjointness from the recovered stack
-- regions, and reports what remains.
module Main where

import qualified A32 as A
import Control.Monad (unless, when)
import Data.Aeson (FromJSON (..), Value, object, (.:), (.=))
import qualified Data.Aeson as J
import Data.Aeson.Types (Parser, parseEither)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.Bits (complement, shiftL, (.&.), (.|.))
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import Emulation.Sha256 (sha256, sha256Hex)
import Numeric (readHex, showHex)
import StockInventory
import System.Directory (doesPathExist)
import System.Environment (getArgs)

-- Physical address: DRAM aliases at 0x40000000 fold onto their physical address.
newtype PhysicalAddress = PhysicalAddress Integer deriving (Eq, Ord, Show)
data Interval = Interval PhysicalAddress PhysicalAddress deriving (Eq, Ord, Show)
data StackKind = TaskStackPool | SeparateStoragePool | KernelBootStack | KernelIdleStackRuntime | ExceptionModeStacks | BootRomStack deriving (Eq, Ord, Show)
data StackRegion = StackRegion StackKind Interval String
-- AbsoluteAddress: constant address, byte count, and whether the constant was
-- held in r4-r11 across a call.
data SiteAddress = AbsoluteAddress Word32 Int Bool | IndexedFromBase Word32 | UnresolvedAddress deriving (Eq, Ord, Show)
data SiteClass = Discharged | DischargedUnderCalleePreservation | OverlapsStack | IndexedSite | UnresolvedSite deriving (Eq, Ord, Show)
newtype CallbackId = CallbackId Word32 deriving (Eq, Ord, Show)
data ReturnStatus = CitedConditional | CitedUnresolved deriving (Eq, Show)
data Callback = Callback CallbackId Word32 [GuestAddress] ReturnStatus

imageBase :: Word32
imageBase = 0xbf1a00

dramAliasBase, dramAliasEnd :: Integer
dramAliasBase = 0x40000000
dramAliasEnd = 0x60000000

physical :: Integer -> PhysicalAddress
physical address
  | address >= dramAliasBase && address < dramAliasEnd = PhysicalAddress (address - dramAliasBase)
  | otherwise = PhysicalAddress address

interval :: Integer -> Integer -> Interval
interval low high = Interval (physical low) (PhysicalAddress (let PhysicalAddress p = physical low in p + high - low))

overlaps :: Interval -> Interval -> Bool
overlaps (Interval (PhysicalAddress a) (PhysicalAddress b)) (Interval (PhysicalAddress c) (PhysicalAddress d)) = a < d && c < b

-- Recovered stack allocations; each extent is the conservative containing range.
stackRegions :: [StackRegion]
stackRegions =
  [ StackRegion TaskStackPool (interval 0x40a10200 0x40a5a200) "task reservation 0x725c6c: cursor 0x4089ea98 from 0x40a10200, growth bounded by 0x4a000; timer arenas own [0x40a5a200,0x40a5b200) (E-KERNEL-PHASE0-RESOURCE-ORDER-SOURCE-150; reports/replacement-inventory-baseline.md section 8)"
  , StackRegion SeparateStoragePool (interval 0x409c4460 0x40a0f460) "separate storage reservation 0x723470: cursor 0x4089ea90 from 0x409c4460, growth bounded by 0x4b000; included because phase submissions fill/cache stack storage (E-KERNEL-PHASE0-RESOURCE-ORDER-SOURCE-150)"
  , StackRegion KernelBootStack (interval 0x40062c54 0x40063460) "loader A boot stack 0x40062c54..0x40063460, initialized over 0x80c bytes before SP installation (reports/replacement-inventory-baseline.md section 8.13; E-STARTUP-TASK-RETAINED-STACK-SOURCE-150)"
  , StackRegion KernelBootStack (interval 0x400f2c14 0x400f3420) "loader B boot stack 0x400f2c14..0x400f3420, top 0x400f3420 (reports/replacement-inventory-baseline.md section 8.13; emulation/README.md opaque caller-frame premise; E-STARTUP-TASK-RETAINED-STACK-SOURCE-150)"
  , StackRegion KernelIdleStackRuntime (interval 0x60000 0xa0000) "loader A constant SP top 0x000a0000 at 0x436f4 with unrecovered extent; whole kernel A runtime partition below it (reports/classic-chrome-patch.md stacks; reports/ram-calibration.md regions)"
  , StackRegion KernelIdleStackRuntime (interval 0xf0000 0x130000) "loader B constant SP top 0x00130000 at 0xd36f4 with unrecovered extent; whole kernel B runtime partition below it (same sources)"
  , StackRegion ExceptionModeStacks (interval 0x70000000 0x70002000) "core TCM exception-mode SPs 0x70000800..0x70002000 set by record-2 code at 0x118b8 (E-PEER-ENTRY-FRAME-HANDOFF-SOURCE-150); loaders reload the IRQ top 0x70002000 at 0x40250/0xd0250 (reports/classic-chrome-patch.md stacks; emulation/README.md natural IRQ callback)"
  , StackRegion BootRomStack (interval 0xfff00000 0xfff01000) "boot ROM SRAM stack top 0xfff00f00 (reports/bootrom-window.md)"
  ]

stateCells, eventCells :: Interval
stateCells = interval 0x1467588 (0x1467588 + 2 * 174)
eventCells = interval 0x14676e4 (0x14676e4 + 2 * 174)

hex :: Word32 -> String
hex value = "0x" ++ replicate (8 - length digits) '0' ++ digits where digits = showHex value ""

intervalJson :: Interval -> Value
intervalJson (Interval (PhysicalAddress low) (PhysicalAddress high)) = object ["low" .= hex (fromIntegral low), "high_exclusive" .= hex (fromIntegral high)]

wordAt :: B.ByteString -> Word32 -> Maybe Word32
wordAt bytes address
  | address < imageBase || toInteger address + 4 > toInteger imageBase + toInteger (B.length bytes) = Nothing
  | otherwise = Just (foldr (\index value -> (value `shiftL` 8) .|. fromIntegral (B.index bytes (fromIntegral (address - imageBase) + index))) 0 [0 .. 3])

parseAddress :: String -> Parser Word32
parseAddress ('0' : 'x' : digits) | [(value, "")] <- readHex digits = pure value
parseAddress text = fail ("address expected: " ++ text)

instance FromJSON Callback where
  parseJSON = J.withObject "callback" $ \fields -> do
    entry <- parseAddress =<< fields .: "address"
    end <- parseAddress =<< fields .: "window_end_exclusive"
    branches <- fields .: "external_branch_candidates"
    tails <- mapM (J.withObject "external branch" (\branch -> GuestAddress <$> (parseAddress =<< branch .: "target"))) branches
    extent <- fields .: "extent"
    status <- J.withObject "extent" (\e -> do
      proof <- e .: "return_proof"
      J.withObject "return proof" (\p -> do
        text <- p .: "status"
        case text :: String of
          "conditional" -> pure CitedConditional
          "unresolved" -> pure CitedUnresolved
          _ -> fail "unknown return-proof status") proof) extent
    pure (Callback (CallbackId entry) end tails status)

newtype Manifest = Manifest [Callback]

instance FromJSON Manifest where
  parseJSON = J.withObject "manifest" $ \fields -> Manifest <$> fields .: "functions"

data Task = Task Word32 Integer

instance FromJSON Task where
  parseJSON = J.withObject "task" $ \fields -> Task <$> (parseAddress =<< fields .: "stack_base") <*> fields .: "stack_bytes"

always :: A.Cond
always = A.Cond 14

-- Constant store addresses by straight-line propagation of MOVW/MOVT, MOV/MVN/
-- ADD/SUB immediates and PC-relative literal words. A value defined under a
-- condition is used only by instructions with that condition and is dropped when
-- flags change. Known values reset at join targets, after a gap in the code
-- words and after unconditional transfers. Calls clobber r0-r3, r12 and LR; a
-- value kept in r4-r11 across a call is marked as relying on the callee
-- preserving it.
storeAddresses :: B.ByteString -> S.Set Word32 -> [Word32] -> M.Map Word32 SiteAddress
storeAddresses bytes joins code = go M.empty Nothing code
  where
    codeSet = S.fromList code
    go _ _ [] = M.empty
    go known0 previous (address : rest) =
      let known = if S.member address joins || previous /= Just (address - 4) || not (S.member (address - 4) codeSet) then M.empty else known0
          instruction = maybe (A.Rejected A.UndefinedEncoding) (A.decode address) (wordAt bytes address)
          (site, known') = step address instruction known
       in maybe id (M.insert address) site (go known' (Just address) rest)
    usable condition (held, value, crossed) = if held == always || held == condition then Just (value, crossed) else Nothing
    lookupAt known condition register = M.lookup register known >>= usable condition
    define condition register value known = case value of
      Just (constant, crossed) | register /= A.pc -> M.insert register (condition, constant, crossed) known
      _ -> M.delete register known
    dropConditional = M.filter (\(held, _, _) -> held == always)
    clobber = M.map (\(held, value, _) -> (held, value, True)) . dropConditional . flip (foldr M.delete) [A.Reg 0, A.Reg 1, A.Reg 2, A.Reg 3, A.Reg 12, A.lr]
    transfer condition known = if condition == always then M.empty else known
    writeback base indexing known = if indexing == A.OffsetOnly then known else M.delete base known
    effective address condition base offset indexing known = do
      (baseValue, baseCrossed) <- if base == A.pc then Just (address + 8, False) else lookupAt known condition base
      case (offset, indexing) of
        (_, A.PostIndexed) -> Just (Right (baseValue, baseCrossed))
        (A.OffsetImmediate delta, _) -> Just (Right (baseValue + fromIntegral delta, baseCrossed))
        (A.OffsetRegister added register A.LSL amount, _) -> Just (case lookupAt known condition register of
          Just (index, indexCrossed) -> Right (if added then baseValue + (index `shiftL` amount) else baseValue - (index `shiftL` amount), baseCrossed || indexCrossed)
          Nothing -> Left baseValue)
        (A.OffsetRegister {}, _) -> Just (Left baseValue)
    widthBytes width = case width of A.Byte -> 1; A.Half -> 2; A.Word -> 4; A.Dual -> 8
    classifyAddress size = maybe UnresolvedAddress (either IndexedFromBase (\(value, crossed) -> AbsoluteAddress value size crossed))
    step address instruction known = case instruction of
      A.MoveWide condition register value -> (Nothing, define condition register (Just (value, False)) known)
      A.MoveTop condition register value -> (Nothing, define condition register ((\(low, crossed) -> ((value `shiftL` 16) .|. (low .&. 0xffff), crossed)) <$> lookupAt known condition register) known)
      A.DataProcessing condition operation flags destination source operand ->
        let tests = operation `elem` [A.TST, A.TEQ, A.CMP, A.CMN]
            sourceValue = if source == A.pc then Just (address + 8, False) else lookupAt known condition source
            value = case (operation, operand) of
              (A.MOV, A.Immediate immediate) -> Just (immediate, False)
              (A.MVN, A.Immediate immediate) -> Just (complement immediate, False)
              (A.MOV, A.ShiftedImmediate register A.LSL 0) -> lookupAt known condition register
              (A.ADD, A.Immediate immediate) -> (\(x, crossed) -> (x + immediate, crossed)) <$> sourceValue
              (A.SUB, A.Immediate immediate) -> (\(x, crossed) -> (x - immediate, crossed)) <$> sourceValue
              _ -> Nothing
            written = if tests then known else define condition destination value known
            flagged = if flags then dropConditional written else written
         in (Nothing, if not tests && destination == A.pc then transfer condition flagged else flagged)
      A.Load condition width _ destination base offset indexing ->
        let literal = case (width, offset, indexing) of
              (A.Word, A.OffsetImmediate delta, A.OffsetOnly) | base == A.pc -> (\word -> (word, False)) <$> wordAt bytes (address + 8 + fromIntegral delta)
              _ -> Nothing
            destinations = if width == A.Dual then [destination, nextRegister destination] else [destination]
            loaded = foldr (\register held -> if register == destination then define condition register literal held else M.delete register held) (writeback base indexing known) destinations
         in (Nothing, if A.pc `elem` destinations then transfer condition loaded else loaded)
      A.Store condition width _ base offset indexing ->
        (Just (classifyAddress (widthBytes width) (effective address condition base offset indexing known)), writeback base indexing known)
      A.StoreMultiple condition base mode back registers ->
        let count = length registers
            start = (\(value, crossed) -> (case mode of
              A.IncrementAfter -> value
              A.IncrementBefore -> value + 4
              A.DecrementAfter -> value + 4 - 4 * fromIntegral count
              A.DecrementBefore -> value - 4 * fromIntegral count, crossed)) <$> lookupAt known condition base
         in (Just (maybe UnresolvedAddress (\(value, crossed) -> AbsoluteAddress value (4 * count) crossed) start), if back then M.delete base known else known)
      A.LoadMultiple condition base _ back registers ->
        let cleared = foldr M.delete (if back then M.delete base known else known) registers
         in (Nothing, if A.pc `elem` registers then transfer condition cleared else cleared)
      A.Branch condition link _ -> (Nothing, if link then clobber known else transfer condition known)
      A.BranchExchange condition link _ -> (Nothing, if link then clobber known else transfer condition known)
      A.Compute _ registers -> (Nothing, foldr M.delete known registers)
      A.Multiply _ destination _ _ _ -> (Nothing, M.delete destination known)
      A.ZeroExtend _ destination _ _ _ -> (Nothing, M.delete destination known)
      A.ReadStatus _ destination -> (Nothing, M.delete destination known)
      A.WriteApplicationFlags _ _ -> (Nothing, dropConditional known)
      A.Hint _ -> (Nothing, known)
      A.Rejected _ -> (Nothing, M.empty)
    nextRegister (A.Reg number) = A.Reg (number + 1)

data CallbackResult = CallbackResult
  { resultCallback :: CallbackId
  , resultCited :: ReturnStatus
  , resultDerived :: Bool
  , resultSites :: [(Word32, SiteAddress, SiteClass)]
  , resultOtherPremises :: S.Set String
  }

classifySite :: SiteAddress -> SiteClass
classifySite address = case address of
  AbsoluteAddress at size crossed
    | any (\(StackRegion _ region _) -> overlaps (interval (toInteger at) (toInteger at + toInteger size)) region) stackRegions -> OverlapsStack
    | crossed -> DischargedUnderCalleePreservation
    | otherwise -> Discharged
  IndexedFromBase _ -> IndexedSite
  UnresolvedAddress -> UnresolvedSite

analyze :: B.ByteString -> Callback -> Either String CallbackResult
analyze bytes (Callback ident@(CallbackId entry) end tails cited) = do
  inspection <- inspectFunction xm1TaskRecords (GuestAddress imageBase) bytes (FunctionWindow (GuestAddress entry) (GuestAddress end)) tails
  let code = [address | GuestAddress address <- inspectedCodeWords inspection]
      literals = [address | GuestAddress address <- inspectedLiteralWords inspection]
      codeSet = S.fromList code
      joins = S.fromList ([target | address <- code, Just word <- [wordAt bytes address], A.Branch _ False target <- [A.decode address word], S.member target codeSet]
        ++ [word | address <- literals, Just word <- [wordAt bytes address], S.member word codeSet])
      addresses = storeAddresses bytes joins code
  case inspectedReturn inspection of
    Left _ -> pure (CallbackResult ident cited False [] S.empty)
    Right uses -> do
      let sites = S.toList (S.fromList [at | PremiseUse (ProgramCounter at) GlobalObjectStackSeparation <- uses])
          resolved = [(site, address, classifySite address) | site <- sites, let address = M.findWithDefault UnresolvedAddress site addresses]
          others = S.fromList [returnPremiseLabel premise | PremiseUse _ premise <- uses, premise /= GlobalObjectStackSeparation]
      pure (CallbackResult ident cited True resolved others)

main :: IO ()
main = do
  arguments <- getArgs
  let required name = case dropWhile (/= name) arguments of _ : value : _ -> pure value; _ -> fail ("missing " ++ name)
  manifestPath <- required "--manifest"
  imagePath <- required "--id5"
  summaryPath <- required "--ram-summary"
  outputPath <- required "--out"
  exists <- doesPathExist outputPath
  when exists (fail "output already exists")
  manifestBytes <- B.readFile manifestPath
  unless (sha256Hex (sha256 manifestBytes) == "f27b35628371e150cdc0cfea84b11424b52c8022f7604f832a04bb890273cd67") (fail "manifest identity differs")
  Manifest callbacks <- either fail pure (J.eitherDecodeStrict manifestBytes)
  unless (length callbacks == 5795) (fail "manifest census differs")
  bytes <- B.readFile imagePath
  unless (B.length bytes == 5185120 && sha256Hex (sha256 bytes) == "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a") (fail "image5 identity differs")
  summaryBytes <- B.readFile summaryPath
  unless (sha256Hex (sha256 summaryBytes) == "2dc2bc42802a9f05016eff808489b4fc9d19b1b6cf7d8e2cc69cc8d5b118a607") (fail "RAM calibration summary identity differs")
  tasks <- either fail pure (J.eitherDecodeStrict summaryBytes >>= parseEither (J.withObject "summary" (\summary -> do
    engines <- summary .: "engines"
    case engines of
      engine : _ -> J.withObject "engine" (\fields -> fields .: "tasks" >>= J.withObject "tasks" (.: "body_tasks")) engine
      [] -> fail "no engine")))
  results <- either fail pure (mapM (analyze bytes) callbacks)
  let regionPairs = [(a, b) | (i, StackRegion _ a _) <- zip [0 :: Int ..] stackRegions, (j, StackRegion _ b _) <- zip [0 ..] stackRegions, i < j, overlaps a b]
      taskIntervals = [(base, size, interval (toInteger base) (toInteger base + size)) | Task base size <- tasks]
      taskPool = interval 0x40a10200 0x40a5a200
      inside (Interval (PhysicalAddress a) (PhysicalAddress b)) (Interval (PhysicalAddress c) (PhysicalAddress d)) = c <= a && b <= d
      derived = filter resultDerived results
      usesSeparation = filter (not . null . resultSites) derived
      dischargedAll result = all (\(_, _, siteClass) -> siteClass `elem` [Discharged, DischargedUnderCalleePreservation]) (resultSites result)
      dischargedWithoutCallees result = all (\(_, _, siteClass) -> siteClass == Discharged) (resultSites result)
      allSites = concatMap resultSites derived
      siteCount wanted = length [() | (_, _, siteClass) <- allSites, siteClass == wanted]
      absoluteObjects = M.fromListWith (+) [((at, size), 1 :: Int) | (_, AbsoluteAddress at size _, _) <- allSites]
      inCells (at, size) = any (\cells -> inside (interval (toInteger at) (toInteger at + toInteger size)) cells) [stateCells, eventCells]
      citedUnresolved = [result | result <- results, resultCited result == CitedUnresolved]
      citedNowDerived = filter resultDerived citedUnresolved
      summary label selected = object
        [ "label" .= (label :: String)
        , "callbacks" .= length selected
        , "derived" .= length (filter resultDerived selected)
        , "derived_using_separation" .= length [() | result <- selected, resultDerived result, not (null (resultSites result))]
        , "separation_discharged_at_every_site" .= length [() | result <- selected, resultDerived result, not (null (resultSites result)), dischargedAll result]
        , "derived_without_separation_after_discharge" .= length [() | result <- selected, resultDerived result, dischargedAll result]
        , "derived_without_separation_or_callee_preservation_after_discharge" .= length [() | result <- selected, resultDerived result, dischargedWithoutCallees result]
        , "derived_with_no_premise_after_discharge" .= length [() | result <- selected, resultDerived result, dischargedAll result, S.null (resultOtherPremises result)] ]
      document = object
        [ "schema_version" .= (1 :: Int)
        , "firmware" .= ("Fujifilm X-M1 1.50" :: String)
        , "evidence_class" .= ("static-inference: interval disjointness of constant callback store addresses from recovered stack allocations; derivations recomputed with the linked StockInventory" :: String)
        , "inputs" .= object
            [ "manifest" .= manifestPath, "manifest_sha256" .= sha256Hex (sha256 manifestBytes)
            , "image5" .= imagePath, "image5_sha256" .= sha256Hex (sha256 bytes)
            , "ram_summary" .= summaryPath, "ram_summary_sha256" .= sha256Hex (sha256 summaryBytes) ]
        , "stack_regions" .= [object ["kind" .= show kind, "physical" .= intervalJson region, "evidence" .= evidence] | StackRegion kind region evidence <- stackRegions]
        , "address_model" .= ("addresses in [0x40000000,0x60000000) fold onto physical DRAM by clearing 0x40000000; other addresses are their own physical address" :: String)
        , "controller_cells" .= object
            [ "state" .= intervalJson stateCells, "event" .= intervalJson eventCells
            , "state_disjoint_from_every_stack_region" .= not (any (\(StackRegion _ region _) -> overlaps stateCells region) stackRegions)
            , "event_disjoint_from_every_stack_region" .= not (any (\(StackRegion _ region _) -> overlaps eventCells region) stackRegions) ]
        , "overlapping_stack_regions" .= length regionPairs
        , "observed_task_stacks" .= object
            [ "source" .= ("E-RAM-CALIBRATION-150 body task table (hardware capture E-BULK-READ-RUN3-150)" :: String)
            , "tasks" .= length taskIntervals
            , "inside_task_stack_pool" .= length [() | (_, size, region) <- taskIntervals, size > 0, inside region taskPool]
            , "zero_length" .= length [() | (_, 0, _) <- taskIntervals]
            , "outside_task_stack_pool" .= [object ["stack_base" .= hex base, "stack_bytes" .= size] | (base, size, region) <- taskIntervals, size > 0, not (inside region taskPool)] ]
        , "separation_sites" .= object
            [ "sites" .= length allSites
            , "discharged" .= (siteCount Discharged + siteCount DischargedUnderCalleePreservation)
            , "discharged_relying_on_callee_preserved_registers" .= siteCount DischargedUnderCalleePreservation
            , "absolute_overlapping_a_stack_region" .= siteCount OverlapsStack
            , "indexed_from_constant_base" .= siteCount IndexedSite
            , "unresolved_address" .= siteCount UnresolvedSite
            , "absolute_sites_in_controller_cells" .= length [() | (_, AbsoluteAddress at size _, _) <- allSites, inCells (at, size)]
            , "distinct_absolute_objects" .= M.size absoluteObjects
            , "absolute_objects_outside_controller_cells" .= [object ["address" .= hex at, "bytes" .= size, "sites" .= count] | ((at, size), count) <- M.toList absoluteObjects, not (inCells (at, size))]
            , "indexed_bases" .= M.toList (M.fromListWith (+) [(hex base, 1 :: Int) | (_, IndexedFromBase base, _) <- allSites])
            , "undischarged_sites" .= [object ["callback" .= hex callback, "site" .= hex site, "class" .= show siteClass] | result <- derived, let CallbackId callback = resultCallback result, (site, _, siteClass) <- resultSites result, siteClass `notElem` [Discharged, DischargedUnderCalleePreservation]] ]
        , "callbacks" .= [summary "all manifest callbacks" results, summary "cited unresolved (E-MODAL-CONTROLLER-MANIFEST-150)" citedUnresolved, summary "cited conditional" [r | r <- results, resultCited r == CitedConditional]]
        , "cited_unresolved_still_refused" .= [hex callback | result <- citedUnresolved, not (resultDerived result), let CallbackId callback = resultCallback result]
        , "derived_using_separation" .= length usesSeparation
        , "cited_unresolved_now_derived" .= length citedNowDerived
        , "premises" .= ([ "callee-preserves-r4-r11: a constant held in r4-r11 across a call is unchanged after it, the AAPCS assumption StockInventory's derivation already makes for opaque calls; discharges that rely on it are counted separately."
                         , "recovered-stack-allocation-completeness: see named_assumptions." ] :: [String])
        , "named_assumptions" .= [ object
            [ "name" .= ("recovered-stack-allocation-completeness" :: String)
            , "statement" .= ("Every stack pointer installed while a controller callback or anything it interrupts runs lies in one of the recovered stack regions." :: String)
            , "recovered_sources" .= [object ["kind" .= show kind, "physical" .= intervalJson region, "evidence" .= evidence] | StackRegion kind region evidence <- stackRegions]
            , "supporting_observation" .= ("all 84 task stacks of the E-RAM-CALIBRATION-150 hardware task table lie in the task pool" :: String)
            , "not_closed" .= ([ "task-creation callers that pass stack memory not obtained from reservation 0x725c6c or 0x723470"
                               , "stacks installed by code not in the recovered loader, record-2, kernel scheduler and exception-entry sources"
                               , "image4 exception entries that copy SP from another mode reuse a listed region only if that mode's SP was set by a recovered source" ] :: [String]) ] ]
        , "premise_notes" .= ([ "Store sites whose address is not a constant, including indexed globals and loaded objects joined with globals, keep global-object-stack-separation."
                         , "The constant-address scan is straight-line and resets at joins; an unresolved scan leaves the premise in place." ] :: [String])
        ]
  L.writeFile outputPath (J.encode document)
  putStrLn ("wrote " ++ outputPath ++ "; sites=" ++ show (length allSites) ++ "; discharged=" ++ show (siteCount Discharged + siteCount DischargedUnderCalleePreservation))
