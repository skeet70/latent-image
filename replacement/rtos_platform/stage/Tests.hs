{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Bits (xor)
import Data.IORef (modifyIORef', newIORef, readIORef)
import qualified Data.Map.Strict as M
import Data.Word (Word8, Word32)
import Numeric (showHex)
import Fixtures (fixtureDat)
import qualified FujiAF.UpdateContainer as U
import Replacement.RtosOwnFirst.Regions (Domain (..), Extent (..), GuestAddress (..), extentEnd, extentStart)
import Replacement.RtosPlatform.Artifact
import Replacement.RtosPlatform.Place
import Replacement.RtosPlatform.Union
import qualified Replacement.RtosPlatform.Aggregate as Aggregate

synthetic :: Union
synthetic = Union 0xe7f000f0 domainA domainB
  where
    domainA = DomainMap DomainA (GuestAddress 0x40000) 0xdae0 [entry 0x40430 "body_a" "veneer_a"] [Carrier FixedPool (Extent (GuestAddress 0x40430) (GuestAddress 0x40530))] [] [Demand FixedPool "body_a" 0x20]
    domainB = DomainMap DomainB (GuestAddress 0xd0000) 0xdaa0 [entry 0xd0430 "body_b" "veneer_b"] [Carrier FixedPool (Extent (GuestAddress 0xd0430) (GuestAddress 0xd0530))] [] [Demand FixedPool "body_b" 0x20]
    entry at symbol veneer = Entry FixedPool "test" (GuestAddress at) symbol veneer

linkedFor :: Union -> Linked
linkedFor _ = Linked (bin (GuestAddress 0x40430)) (symbols (GuestAddress 0x40430) "veneer_a" "body_a") "" [] (bin (GuestAddress 0xd0430)) (symbols (GuestAddress 0xd0430) "veneer_b" "body_b") "" []

bin :: GuestAddress -> B.ByteString
bin (GuestAddress _base) =
  B.concat
    [ U.putWord32 0xea000002
    , B.replicate 0x0c 0
    , U.putWord32 0xe12fff1e
    , B.concat (replicate 7 (U.putWord32 0xe1a00000))
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
  failures <- newIORef (0 :: Int)
  let check name ok = if ok then pure () else modifyIORef' failures (+ 1) >> putStrLn ("FAIL " ++ name)
      expectLeft name predicate result = case result of
        Left err -> check name (predicate err)
        Right _ -> check name False
  check "zero-extent labels do not become native functions"
    (Aggregate.parseAggregateSymbols "0004282c 00000000 T label\n00042830 00000004 T function\n"
      == Right [Aggregate.AggregateSymbol "function" 0x42830 4])
  case withLink synthetic (linkedFor synthetic) of
    Left err -> check ("positive admission: " ++ show err) False
    Right (source, admitted, staged) -> do
      check "candidate DAT differs" (stagedDat staged /= fixtureDat)
      check "restore yields the fixture" (restore admitted (stagedDat staged) == Right fixtureDat)
      check "confined candidate changes only the carrier" (confineChanges admitted (stagedDat staged) == Right ())
      check "staged loader differs from stock" (stagedLoader staged DomainA /= sourceLoaders source M.! DomainA)
      check "staged loader B differs from stock" (stagedLoader staged DomainB /= sourceLoaders source M.! DomainB)
  let refusedDemand = synthetic { unionA = (unionA synthetic) { mapDemand = [Demand FixedPool "body_a" 0x100] } }
  expectLeft "demand larger than the largest run refuses" (\e -> case e of FitRejected (CarrierDeficit DomainA FixedPool "body_a" 0x100 0xfc) -> True; _ -> False) (withLink refusedDemand (linkedFor synthetic))
  let movedVeneer = synthetic { unionA = (unionA synthetic) { mapEntries = [Entry FixedPool "test" (GuestAddress 0x40434) "body_a" "veneer_a"] } }
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
  count <- readIORef failures
  if count == 0 then putStrLn "platform-tests: passed" else putStrLn ("platform-tests: " ++ show count ++ " failures") >> fail "platform-tests failed"

xorByte :: Word8 -> Word8 -> Word8
xorByte = xor

parseUnionError :: Union -> Maybe String
parseUnionError union = case parseUnion (encodeUnion union) of
  Left err -> Just (show err)
  Right _ -> Nothing

encodeUnion :: Union -> B.ByteString
encodeUnion union = C.pack ("{\"schema\":1,\"fill_word\":\"0xe7f000f0\",\"domains\":{\"a\":" ++ domainJson (unionA union) ++ ",\"b\":" ++ domainJson (unionB union) ++ "}}")
  where
    domainJson dm = "{\"loader_base\":\"" ++ addressText (mapBase dm) ++ "\",\"loader_size\":" ++ show (mapSize dm) ++ ",\"entries\":" ++ list (map entryJson (mapEntries dm)) ++ ",\"carriers\":" ++ list (map carrierJson (mapCarriers dm)) ++ ",\"externals\":[],\"demand\":" ++ list (map demandJson (mapDemand dm)) ++ "}"
    list xs = "[" ++ comma xs ++ "]"
    comma [] = ""
    comma [x] = x
    comma (x : xs) = x ++ "," ++ comma xs
    entryJson e = "{\"group\":\"" ++ groupLabel (entryGroup e) ++ "\",\"name\":\"" ++ entryName e ++ "\",\"address\":\"" ++ addressText (entryAddress e) ++ "\",\"symbol\":\"" ++ entrySymbol e ++ "\",\"veneer\":\"" ++ entryVeneer e ++ "\"}"
    carrierJson c = "{\"group\":\"" ++ groupLabel (carrierGroup c) ++ "\",\"start\":\"" ++ addressText (extentStart (carrierExtent c)) ++ "\",\"end\":\"" ++ addressText (extentEnd (carrierExtent c)) ++ "\"}"
    demandJson d = "{\"group\":\"" ++ groupLabel (demandGroup d) ++ "\",\"symbol\":\"" ++ demandSymbol d ++ "\",\"bytes\":" ++ show (demandBytes d) ++ "}"
    addressText (GuestAddress value) = "0x" ++ hex value
