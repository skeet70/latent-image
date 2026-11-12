{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import Control.Monad (forM_)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Bits (xor)
import Data.IORef (modifyIORef', newIORef, readIORef)
import Data.List (isPrefixOf, sort)
import qualified Data.Map.Strict as M
import Data.Word (Word8, Word32)
import Numeric (showHex)
import Fixtures (fixtureDat)
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Replacement.RtosOwnFirst.Regions (Domain (..), Extent (..), GuestAddress (..), extentEnd, extentStart)
import Replacement.RtosPlatform.Artifact
import Replacement.RtosPlatform.Place
import Replacement.RtosPlatform.Union
import qualified Replacement.RtosPlatform.Aggregate as Aggregate
import System.Environment (getArgs)

synthetic :: Union
synthetic = Union 0xe7f000f0 domainA domainB []
  where
    domainA = DomainMap DomainA (GuestAddress 0x40000) 0xdae0 [entry 0x40430 "body_a" "veneer_a"] [Carrier FixedPool (Extent (GuestAddress 0x40430) (GuestAddress 0x40530))] [] [Demand FixedPool "body_a" 0x20]
    domainB = DomainMap DomainB (GuestAddress 0xd0000) 0xdaa0 [entry 0xd0430 "body_b" "veneer_b"] [Carrier FixedPool (Extent (GuestAddress 0xd0430) (GuestAddress 0xd0530))] [] [Demand FixedPool "body_b" 0x20]
    entry at symbol veneer = Entry FixedPool "test" (GuestAddress at) symbol (".text." ++ symbol) veneer False

linkedFor :: Union -> Linked
linkedFor _ = Linked (bin (GuestAddress 0x40430)) (symbols (GuestAddress 0x40430) "veneer_a" "body_a") "" [(0x40460, 8)] (bin (GuestAddress 0xd0430)) (symbols (GuestAddress 0xd0430) "veneer_b" "body_b") "" [(0xd0460, 8)]

bin :: GuestAddress -> B.ByteString
bin (GuestAddress _base) =
  B.concat
    [ U.putWord32 0xea000002
    , B.replicate 0x0c 0
    , U.putWord32 0xe12fff1e
    , U.putWord32 0
    , B.concat (replicate 6 (U.putWord32 0xe1a00000))
    , B.replicate (0x100 - 0x30) 0
    ]

symbols :: GuestAddress -> String -> String -> B.ByteString
symbols (GuestAddress base) veneer body =
  C.unlines
    [ C.pack (hex base ++ " 00000004 T " ++ veneer)
    , C.pack (hex (base + 0x10) ++ " 00000020 T " ++ body)
    ]

hex :: Word32 -> String
hex value = let digits = showHex value "" in replicate (8 - length digits) '0' ++ digits

sourceFor :: Either StageError CanonicalSource
sourceFor = canonicalSource fixtureDat (B.replicate 0xdae0 0) (B.replicate 0xdaa0 0)

withLink :: Union -> Linked -> Either StageError (CanonicalSource, Admitted, Staged)
withLink union linked = do
  source <- sourceFor
  admitted <- admit union source linked
  staged <- stage admitted
  pure (source, admitted, staged)

branchingRuns :: [Extent]
branchingRuns =
  [ Extent (GuestAddress 0x1000) (GuestAddress 0x10c8)
  , Extent (GuestAddress 0x10c8) (GuestAddress 0x1190)
  , Extent (GuestAddress 0x1190) (GuestAddress 0x1258)
  ]
    ++ [Extent (GuestAddress start) (GuestAddress (start + 0x4c)) | start <- [0x1258, 0x12a4, 0x12f0, 0x133c, 0x1388]]
    ++ [Extent (GuestAddress 0x10000) (GuestAddress 0x10064)]

branchingSections :: [Section]
branchingSections =
  [ Section ".text.s2_save" 40 4
  , Section ".text.s2_resume" 32 4
  , Section ".rodata.s2_pool" 8 4
  , Section ".text.fill_a" 150 4
  , Section ".text.fill_b" 150 4
  , Section ".text.fill_c" 150 4
  ]

main :: IO ()
main = do
  arguments <- getArgs
  failures <- newIORef (0 :: Int)
  let check name ok = if ok then pure () else modifyIORef' failures (+ 1) >> putStrLn ("FAIL " ++ name)
      expectLeft name predicate result = case result of
        Left err -> check name (predicate err)
        Right _ -> check name False
  case arguments of
    [carriersPath, symbolsAPath, symbolsBPath] -> do
      carriersBytes <- B.readFile carriersPath
      case parseUnion carriersBytes of
        Left err -> check ("census carriers parse: " ++ show err) False
        Right union -> forM_ [DomainA, DomainB] $ \domain -> do
          symbolsBytes <- B.readFile (if domain == DomainA then symbolsAPath else symbolsBPath)
          case Aggregate.parseAggregateSymbols symbolsBytes of
            Left err -> check ("census symbols parse: " ++ err) False
            Right linkedSymbols -> do
              let dm = domainMap union domain
                  veneers = map entryVeneer (mapEntries dm)
                  linked = sort [(Aggregate.symbolName symbol, Aggregate.symbolSize symbol) | symbol <- linkedSymbols, Aggregate.symbolName symbol `notElem` veneers]
                  demand = sort [(demandSymbol row, fromIntegral (demandBytes row)) | row <- mapDemand dm]
              check ("census rows equal the linked symbols for " ++ show domain) (linked == demand)
    _ -> check "census test needs the carriers and both symbol files" False
  check "zero-extent labels do not become native functions"
    (Aggregate.parseAggregateSymbols "0004282c 00000000 T label\n00042830 00000004 T function\n"
      == Right [Aggregate.AggregateSymbol "function" 0x42830 4])
  let aliasEntry = Entry S3d "terminate" (GuestAddress 0x1000) "terminate" ".text.scheduler_timeout_entry" "veneer" False
      aliasImage = Aggregate.AggregateImage 0x1000 (U.putWord32 0xea000002 <> B.replicate 0x0c 0 <> U.putWord32 0xea000002 <> B.replicate 0x10 0)
      aliasSymbols = [Aggregate.AggregateSymbol "veneer" 0x1000 4, Aggregate.AggregateSymbol "terminate_body" 0x1020 4]
      aliasPlacement = Aggregate.SectionPlacement 0x1010 4 ".text.scheduler_timeout_entry"
      resolvedAlias = resolveEntryTarget aliasImage aliasSymbols [aliasPlacement] aliasEntry
  check "allocated public thunk resolves without a zero-size function"
    (resolvedAlias == Right (Aggregate.AggregateSymbol "terminate" 0x1010 4))
  expectLeft "entry thunk corruption is refused" (const True)
    (resolveEntryTarget (Aggregate.AggregateImage 0x1000 (B.replicate 0x24 0)) aliasSymbols [aliasPlacement] aliasEntry)
  expectLeft "fixed veneer corruption is refused" (const True)
    (resolveEntryTarget (Aggregate.AggregateImage 0x1000 (U.putWord32 0 <> B.drop 4 (Aggregate.imageBytes aliasImage))) aliasSymbols [aliasPlacement] aliasEntry)
  expectLeft "unallocated zero-size alias is refused" (const True)
    (resolveEntryTarget aliasImage aliasSymbols [] aliasEntry)
  let carrierImage = Aggregate.AggregateImage 0x1000 (B.pack [0 .. 15])
      carrierSymbols = [Aggregate.AggregateSymbol "body" 0x1000 4]
      carrierPlacement = Aggregate.SectionPlacement 0x1008 4 ".text.body"
      carrierFill = 0xe7f000f0
      fillWord = B.pack [0xf0, 0x00, 0xf0, 0xe7]
      composed = Aggregate.carrierBytes carrierFill carrierSymbols [carrierPlacement] carrierImage 0x1000 16
      stockBase = B.replicate 0x20 0xaa
      installed = Aggregate.installCarriers 0x1000 stockBase carrierFill carrierSymbols [carrierPlacement] carrierImage [(0x1004, 8)]
  check "carrier bytes take linked image bytes where a symbol or placement covers them"
    (composed == Right (B.pack [0, 1, 2, 3] <> fillWord <> B.pack [8, 9, 10, 11] <> fillWord))
  check "carrier installation leaves bytes outside the extent at their stock value"
    (installed == Right (B.take 4 stockBase <> fillWord <> B.pack [8, 9, 10, 11] <> B.drop 12 stockBase))
  expectLeft "a carrier outside the linked image is refused" (const True)
    (Aggregate.carrierBytes carrierFill carrierSymbols [carrierPlacement] carrierImage 0x1010 4)
  let zeroSlack = B.pack [0, 1, 2, 3] <> B.replicate 4 0 <> B.pack [8, 9, 10, 11] <> B.replicate 4 0
      artifactSymbols = either error id (F.parseSymbols "00001000 00000004 T body\n")
      artifactStaged = installCarrier DomainA carrierFill artifactSymbols [(0x1008, 4)] zeroSlack (Extent (GuestAddress 0x1000) (GuestAddress 0x1010))
      aggregateStaged = Aggregate.carrierBytes carrierFill carrierSymbols [carrierPlacement] (Aggregate.AggregateImage 0x1000 zeroSlack) 0x1000 16
  check "the artifact and aggregate staged-byte definitions agree"
    ((either (Left . show) Right artifactStaged :: Either String B.ByteString) == either Left Right aggregateStaged)
  expectLeft "the artifact definition refuses nonzero slack that the aggregate definition overwrites" (const True)
    (installCarrier DomainA carrierFill artifactSymbols [(0x1008, 4)] (B.pack [0 .. 15]) (Extent (GuestAddress 0x1000) (GuestAddress 0x1010)))
  check "carrier parser retains emitted entry sections"
    (case parseUnion (encodeUnion synthetic) of
      Right parsed -> map entrySection (mapEntries (unionA parsed)) == [".text.body_a"]
      Left _ -> False)
  expectLeft "entry thunk without a genuine positive-size body is refused" (const True)
    (resolveEntryTarget aliasImage [] [aliasPlacement] aliasEntry)
  case withLink synthetic (linkedFor synthetic) of
    Left err -> check ("positive admission: " ++ show err) False
    Right (source, admitted, staged) -> do
      check "candidate DAT differs" (stagedDat staged /= fixtureDat)
      check "restore yields the fixture" (restore admitted (stagedDat staged) == Right fixtureDat)
      check "confined candidate changes only the carrier" (confineChanges admitted (stagedDat staged) == Right ())
      check "staged loader differs from stock" (stagedLoader staged DomainA /= sourceLoaders source M.! DomainA)
      check "staged loader B differs from stock" (stagedLoader staged DomainB /= sourceLoaders source M.! DomainB)
      case canonicalSource (stagedDat staged) (stagedLoader staged DomainA) (stagedLoader staged DomainB) of
        Left err -> check ("extract staged DAT: " ++ show err) False
        Right extracted -> forM_ [DomainA, DomainB] $ \domain -> do
          let base = if domain == DomainA then 0x40430 else 0xd0430
              loaderBase = if domain == DomainA then 0x40000 else 0xd0000
              body = B.take 0x20 (B.drop 0x10 (bin (GuestAddress base)))
              loader = sourceLoaders extracted M.! domain
          check "allocated body bytes survive DAT staging" (B.take 0x20 (B.drop (fromIntegral (base + 0x10 - loaderBase)) loader) == body)
          check "extracted DAT matches staged loader" (loader == stagedLoader staged domain)
          check "uncovered carrier slack receives trap fill" (B.take 4 (B.drop (fromIntegral (base + 4 - loaderBase)) loader) == U.putWord32 0xe7f000f0)
          check "allocated readonly zero words survive DAT staging" (B.take 8 (B.drop (fromIntegral (base + 0x30 - loaderBase)) loader) == B.replicate 8 0)
  let refusedDemand = synthetic { unionA = (unionA synthetic) { mapDemand = [Demand FixedPool "body_a" 0x100] } }
  expectLeft "demand larger than the largest run refuses" (\e -> case e of FitRejected (CarrierDeficit DomainA FixedPool "body_a" 0x100 0xfc) -> True; _ -> False) (withLink refusedDemand (linkedFor synthetic))
  let movedVeneer = synthetic { unionA = (unionA synthetic) { mapEntries = [Entry FixedPool "test" (GuestAddress 0x40434) "body_a" ".text.body_a" "veneer_a" False] } }
  expectLeft "a moved entry refuses" (\e -> case e of EntryMoved DomainA _ (GuestAddress 0x40430) -> True; _ -> False) (withLink movedVeneer (linkedFor synthetic))
  let wrongSize = synthetic { unionA = (unionA synthetic) { mapDemand = [Demand FixedPool "body_a" 0x24] } }
  expectLeft "a demand-size mismatch refuses" (\e -> case e of DemandSize DomainA _ 0x20 -> True; _ -> False) (withLink wrongSize (linkedFor synthetic))
  let dataLink = (linkedFor synthetic) { linkedSymbolsA = linkedSymbolsA (linkedFor synthetic) <> C.pack "00040500 00000004 D data_a\n" }
  expectLeft "a data symbol refuses" (\e -> case e of DataSymbol DomainA "data_a" -> True; _ -> False) (withLink synthetic dataLink)
  let dirtyLink = (linkedFor synthetic) { linkedBinA = B.take 0xd0 (linkedBinA (linkedFor synthetic)) <> U.putWord32 0x12345678 <> B.drop 0xd4 (linkedBinA (linkedFor synthetic)) }
  expectLeft "a nonzero slack word refuses" (\e -> case e of SlackNotFillOrZero DomainA (GuestAddress 0x40500) -> True; _ -> False) (withLink synthetic dirtyLink)
  let overlapping = synthetic { unionA = (unionA synthetic) { mapCarriers = [Carrier FixedPool (Extent (GuestAddress 0x40430) (GuestAddress 0x40530)), Carrier S1 (Extent (GuestAddress 0x40500) (GuestAddress 0x40600))] } }
  case parseUnionError overlapping of
    Just _ -> pure ()
    Nothing -> check "overlapping carriers are refused by the parser" False
  case withLink synthetic (linkedFor synthetic) of
    Left err -> check ("change-confinement setup: " ++ show err) False
    Right (_, admitted, _) -> do
      let outside = 0x10
          mutated = B.take outside fixtureDat <> B.singleton (B.index fixtureDat outside `xorByte` 0x01) <> B.drop (outside + 1) fixtureDat
      expectLeft "a change outside the union refuses" (\e -> case e of ChangeOutside _ -> True; _ -> False) (confineChanges admitted mutated)
  let tinyRuns = [Extent (GuestAddress 0x1000) (GuestAddress 0x1064)]
      tinySections = [Section ".text.s2_save" 40 4, Section ".text.s2_resume" 32 4, Section ".text.big" 100 4]
  case solveSections 20 tinyRuns tinySections of
    Left (SearchExhausted _) -> pure ()
    Left err -> check ("bounded packing: " ++ show err) False
    Right _ -> check "bounded packing accepted an impossible assignment" False
  -- Three 200-byte runs hold the 150-byte sections; five 76-byte runs hold the
  -- save/resume bundle but not the pool. The first three bundle runs leave two
  -- 200-byte runs for three sections, the fourth leaves all three.
  case solveSections 20 branchingRuns branchingSections of
    Left (SearchExhausted _) -> pure ()
    Left err -> check ("branching budget: " ++ show err) False
    Right _ -> check "branching budget placed under an exhausted cumulative budget" False
  case solveSections 200 branchingRuns branchingSections of
    Left err -> check ("branching budget success: " ++ show err) False
    Right placements ->
      check "branching placement is not publicly admissible" (admitPlacement branchingRuns branchingSections placements == Right ())
  check "runs carry the group of their carrier"
    (groupedRuns (unionA synthetic) == [(FixedPool, Extent (GuestAddress 0x40434) (GuestAddress 0x40530))])
  let reservedRuns = [Extent (GuestAddress 0x2000) (GuestAddress 0x2064), Extent (GuestAddress 0x3000) (GuestAddress 0x30c8)]
      reservedSections = [Section ".text.group_a" 0x40 4, Section ".text.group_b" 0x60 4]
  case placeReserved reservedRuns [] [1] [Section ".text.group_a" 0xd0 4] of
    Left (NoEligibleRun ".text.group_a" 0xd0 _) -> pure ()
    Left err -> check ("reserved deficit: " ++ show err) False
    Right _ -> check "a reserved section that needs another group's run was placed" False
  case placeReserved reservedRuns [] [0, 1] reservedSections of
    Left err -> check ("reserved packing: " ++ show err) False
    Right placements -> do
      check "reserved packing uses only its own runs" (all ((`elem` [0, 1]) . placementRun) placements)
      check "reserved packing takes the tightest run first"
        ([(placementRun p, sectionName (placementSection p)) | p <- placements] == [(0, ".text.group_b"), (1, ".text.group_a")])
  case placeReserved reservedRuns [(".text.group_a", 1)] [0, 1] reservedSections of
    Left err -> check ("reserved pin: " ++ show err) False
    Right placements -> do
      let at name = [(placementRun p, placementOffset p) | p <- placements, sectionName (placementSection p) == name]
      check "a pinned section starts its run" (at ".text.group_a" == [(1, 0)])
      check "the remaining sections fill the leftover run" (at ".text.group_b" == [(0, 0)])
  case placeReserved reservedRuns [(".text.group_a", 2)] [0, 1] reservedSections of
    Left (PinOutsideGroup ".text.group_a" 2) -> pure ()
    Left err -> check ("reserved pin group: " ++ show err) False
    Right _ -> check "a pin outside the group's runs was accepted" False
  case placeReserved reservedRuns [(".text.group_a", 0), (".text.group_b", 0)] [0, 1] reservedSections of
    Left (PinRunRepeated ".text.group_a" ".text.group_b") -> pure ()
    Left err -> check ("repeated pin run: " ++ show err) False
    Right _ -> check "two pins on one run were accepted" False
  case placeReserved reservedRuns [] [0, 1] [Section ".text.group_a" 0xb0 4, Section ".text.group_b" 0xb0 4, Section ".text.group_c" 0xb0 4] of
    Left (SearchExhausted _) -> pure ()
    Left err -> check ("over-capacity reserved group: " ++ show err) False
    Right _ -> check "an over-capacity reserved group was placed" False
  case placeReserved [Extent (GuestAddress 0x2000) (GuestAddress 0x2064), Extent (GuestAddress 0x2100) (GuestAddress 0x2164)] []
        [0, 1] [Section ".text.pack_a" 60 4, Section ".text.pack_b" 60 4, Section ".text.pack_c" 80 4] of
    Left (SearchExhausted reasons) -> check "a group that fits by sum is refused by the packing search"
      (any ("reserved packing" `isPrefixOf`) reasons)
    Left err -> check ("unpackable reserved group: " ++ show err) False
    Right _ -> check "a group that fits by sum but cannot be packed was placed" False
  check "pinned entries parse"
    (case parseUnion (encodeUnion synthetic { unionA = (unionA synthetic) { mapEntries = [e { entryPinned = True } | e <- mapEntries (unionA synthetic)] } }) of
      Right parsed -> map entryPinned (mapEntries (unionA parsed)) == [True]
      Left _ -> False)
  check "reserved prefixes parse"
    (case parseUnion (encodeUnion synthetic { unionReserved = [(EventFlag, [".text.family_flag_"])] }) of
      Right parsed -> unionReserved parsed == [(EventFlag, [".text.family_flag_"])]
      Left _ -> False)
  let withExternals externals = synthetic { unionA = (unionA synthetic) { mapExternals = externals } }
  case parseUnionError (withExternals [External "held" (GuestAddress 0x40440) RetainedService]) of
    Just _ -> pure ()
    Nothing -> check "a retained external inside a carrier is refused" False
  case parseUnionError (withExternals [External "stray" (GuestAddress 0x40450) OwnedEntry]) of
    Just _ -> pure ()
    Nothing -> check "an owned external that is not an entry address is refused" False
  case parseUnionError (withExternals [External "entry" (GuestAddress 0x40430) OwnedEntry]) of
    Nothing -> pure ()
    Just err -> check ("an owned external on an entry address: " ++ err) False
  count <- readIORef failures
  if count == 0 then putStrLn "platform-tests: passed" else putStrLn ("platform-tests: " ++ show count ++ " failures") >> fail "platform-tests failed"

xorByte :: Word8 -> Word8 -> Word8
xorByte = xor

parseUnionError :: Union -> Maybe String
parseUnionError union = case parseUnion (encodeUnion union) of
  Left err -> Just (show err)
  Right _ -> Nothing

encodeUnion :: Union -> B.ByteString
encodeUnion union = C.pack ("{\"schema\":1,\"fill_word\":\"0xe7f000f0\"" ++ reservedJson ++ ",\"domains\":{\"a\":" ++ domainJson (unionA union) ++ ",\"b\":" ++ domainJson (unionB union) ++ "}}")
  where
    reservedJson = case unionReserved union of
      [] -> ""
      groups -> ",\"reserved_prefixes\":{" ++ comma ["\"" ++ groupLabel g ++ "\":" ++ list (map (\name -> "\"" ++ name ++ "\"") names) | (g, names) <- groups] ++ "}"
    domainJson dm = "{\"loader_base\":\"" ++ addressText (mapBase dm) ++ "\",\"loader_size\":" ++ show (mapSize dm) ++ ",\"entries\":" ++ list (map entryJson (mapEntries dm)) ++ ",\"carriers\":" ++ list (map carrierJson (mapCarriers dm)) ++ ",\"externals\":" ++ list (map externalJson (mapExternals dm)) ++ ",\"demand\":" ++ list (map demandJson (mapDemand dm)) ++ "}"
    list xs = "[" ++ comma xs ++ "]"
    comma [] = ""
    comma [x] = x
    comma (x : xs) = x ++ "," ++ comma xs
    entryJson e = "{\"group\":\"" ++ groupLabel (entryGroup e) ++ "\",\"name\":\"" ++ entryName e ++ "\",\"address\":\"" ++ addressText (entryAddress e) ++ "\",\"symbol\":\"" ++ entrySymbol e ++ "\",\"section\":\"" ++ entrySection e ++ "\",\"veneer\":\"" ++ entryVeneer e ++ "\"" ++ (if entryPinned e then ",\"pinned\":true" else "") ++ "}"
    carrierJson c = "{\"group\":\"" ++ groupLabel (carrierGroup c) ++ "\",\"start\":\"" ++ addressText (extentStart (carrierExtent c)) ++ "\",\"end\":\"" ++ addressText (extentEnd (carrierExtent c)) ++ "\"}"
    externalJson e = "{\"name\":\"" ++ externalName e ++ "\",\"address\":\"" ++ addressText (externalAddress e) ++ "\",\"kind\":\"" ++ kindText (externalKind e) ++ "\"}"
    kindText OwnedEntry = "owned"
    kindText RetainedService = "retained"
    kindText ScratchEntry = "scratch"
    demandJson d = "{\"group\":\"" ++ groupLabel (demandGroup d) ++ "\",\"symbol\":\"" ++ demandSymbol d ++ "\",\"bytes\":" ++ show (demandBytes d) ++ "}"
    addressText (GuestAddress value) = "0x" ++ hex value
