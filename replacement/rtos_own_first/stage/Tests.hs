{-# LANGUAGE OverloadedStrings #-}

-- | Staging over the synthetic container of //replacement/startup_artifact:Fixtures.hs, whose loaders
-- sit at the X-M1 load addresses with pattern bytes, and a two-slot region set per loader.
module Main (main) where

import Control.Monad (unless)
import Data.Bits (complement)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.IORef (modifyIORef', newIORef, readIORef)
import Data.Word (Word32)
import Fixtures (fixtureDat)
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)
import Replacement.RtosOwnFirst.Artifact
import Replacement.RtosOwnFirst.Frame (Frame (..))
import Replacement.RtosOwnFirst.Regions
import System.Exit (exitFailure)

fill :: Word32
fill = 0xe7f000f0

-- | Loader A addresses; loader B is the same layout 0x90000 higher.
shift :: Domain -> Word32 -> Word32
shift domain address = case domain of
  DomainA -> address
  DomainB -> address + 0x90000

hex :: Word32 -> String
hex value = "\"0x" ++ showHex value "\""

data Variation = Variation
  { slot164Frame :: Int
  , slot164Entry :: Word32
  }

stockVariation :: Variation
stockVariation = Variation 16 0x41140

regionsJson :: Variation -> B.ByteString
regionsJson variation = C.pack ("{\"fill_word\":\"0xe7f000f0\",\"domains\":{\"a\":" ++ domainJson DomainA ++ ",\"b\":" ++ domainJson DomainB ++ "}}")
  where
    domainJson domain =
      let at = hex . shift domain
          row slot entry start end symbol frame =
            "{\"slot\":" ++ hex slot ++ ",\"entry\":" ++ at entry ++ ",\"start\":" ++ at start ++ ",\"end\":" ++ at end
              ++ ",\"symbol\":\"" ++ symbol ++ "\",\"frame_bytes\":" ++ show (frame :: Int) ++ "}"
       in "{\"loader_base\":" ++ at 0x40000 ++ ",\"regions\":["
            ++ row 0x164 (slot164Entry variation) 0x41140 0x41160 "own_164" (slot164Frame variation)
            ++ "," ++ row 0x168 0x41160 0x41160 0x41180 "own_168" 8
            ++ "],\"tail\":{\"slot\":\"0x1618\",\"address\":" ++ at 0x412bc ++ ",\"end\":" ++ at 0x412cc
            ++ ",\"symbol\":\"own_1618\",\"frame_bytes\":0,\"body\":{\"address\":" ++ at 0x41170 ++ ",\"end\":" ++ at 0x41178
            ++ ",\"symbol\":\"own_1618_body\",\"host_slot\":\"0x168\",\"frame_bytes\":0}}}"

words32 :: [Word32] -> B.ByteString
words32 = B.concat . map U.putWord32

slot164Code, slot164Installed, slot168Code, tailCode :: [Word32]
-- push {r4, lr}; bl slot 0x168; pop {r4, pc}
slot164Code = [0xe92d4010, 0xeb000005, 0xe8bd8010]
slot164Installed = slot164Code ++ replicate 5 fill
-- sub sp, sp, #8; add sp, sp, #8; bx lr; fill; then the tail body: mrc p14, 0, r0, c2, c0, 0; bx lr; fill
slot168Code = [0xe24dd008, 0xe28dd008, 0xe12fff1e, fill, 0xee120e10, 0xe12fff1e, fill, fill]
-- b body; fill
tailCode = [0xeaffffab, fill, fill, fill]

-- | The flat image from 0x41140 (or 0xd1140) to the tail end, gaps zero.
ownedBin :: B.ByteString
ownedBin =
  words32 slot164Code <> B.replicate 20 0 <> words32 slot168Code
    <> B.replicate (0x412bc - 0x41180) 0 <> words32 tailCode

symbolsFile :: Domain -> Word32 -> B.ByteString
symbolsFile domain entry164 =
  C.pack . unlines $
    [ row (shift domain entry164) 0xc "T" "own_164"
    , row (shift domain 0x41160) 0xc "T" "own_168"
    , row (shift domain 0x41170) 0x8 "T" "own_1618_body"
    , row (shift domain 0x412bc) 0x4 "T" "own_1618"
    , row 0x4005ff0c 0 "A" "context_word"
    ]
  where
    row :: Word32 -> Word32 -> String -> String -> String
    row address size kind name = pad (showHex address "") ++ " " ++ pad (showHex size "") ++ " " ++ kind ++ " " ++ name
    pad text = replicate (8 - length text) '0' ++ text

containerLoader :: B.ByteString -> Domain -> B.ByteString
containerLoader dat domain = case U.parseContainer dat of
  Right parsed ->
    let flash = case domain of
          DomainA -> 0x60000
          DomainB -> 0x70000
     in B.map complement (B.take (domainLoaderSize domain) (B.drop flash (U.partPreTable (U.parsedParts parsed))))
  Left failure -> error failure

linked :: Domain -> LinkedDomain
linked domain = LinkedDomain ownedBin (symbolsFile domain 0x41140)

positive :: Either StageError (CanonicalSource, Admitted, Staged)
positive = do
  source <- canonicalSource fixtureDat stockA stockB
  admitted <- admit source (regionsJson stockVariation) (linked DomainA) (linked DomainB)
  staged <- stage source admitted
  pure (source, admitted, staged)

admitWith :: Variation -> (Domain -> LinkedDomain) -> B.ByteString -> B.ByteString -> Either StageError Admitted
admitWith variation linking loaderA loaderB = do
  source <- canonicalSource fixtureDat loaderA loaderB
  admit source (regionsJson variation) (linking DomainA) (linking DomainB)

stockA, stockB :: B.ByteString
stockA = containerLoader fixtureDat DomainA
stockB = containerLoader fixtureDat DomainB

poke :: Int -> B.ByteString -> B.ByteString
poke offset bytes = B.take offset bytes <> B.singleton (complement (B.index bytes offset)) <> B.drop (offset + 1) bytes

-- | Byte offsets, from the loader base, where two loader images differ.
differing :: B.ByteString -> B.ByteString -> [Int]
differing left right = [index | (index, True) <- zip [0 ..] (B.zipWith (/=) left right)]

main :: IO ()
main = do
  failures <- newIORef (0 :: Int)
  let check label premise = unless premise $ do
        putStrLn ("FAIL " ++ label)
        modifyIORef' failures (+ 1)
      refusedAs label expected actual = check (label ++ ": " ++ either show (const "admitted") actual) (either (== expected) (const False) actual)

  -- Positive staging.
  case positive of
    Left failure -> check ("positive staging: " ++ show failure) False
    Right (source, admitted, staged) -> do
      let restored = restore source admitted (stagedDat staged)
      let loaderA = stagedLoader staged DomainA
          loaderB = stagedLoader staged DomainB
          window :: Word32 -> Word32 -> Int -> B.ByteString -> B.ByteString
          window base start count loader = B.take count (B.drop (fromIntegral (start - base)) loader)
      check "slot 0x164 slack is filled" (window 0x40000 0x41140 0x20 loaderA == words32 slot164Installed)
      check "slot 0x168 and its tail body are installed" (window 0x40000 0x41160 0x20 loaderA == words32 slot168Code)
      check "the +0x1618 entry branches to the body" (window 0x40000 0x412bc 0x10 loaderA == words32 tailCode)
      check "loader B carries the same words at its own addresses" (window 0xd0000 0xd1140 0x40 loaderB == words32 (slot164Installed ++ slot168Code))
      let inExtents offset = (offset >= 0x1140 && offset < 0x1180) || (offset >= 0x12bc && offset < 0x12cc)
      check "loader A changes only inside the extents" (all inExtents (differing stockA loaderA))
      check "loader B changes only inside the extents" (all inExtents (differing stockB loaderB))
      check "the candidate DAT holds the staged loader A" (containerLoader (stagedDat staged) DomainA == loaderA)
      check "the candidate DAT holds the staged loader B" (containerLoader (stagedDat staged) DomainB == loaderB)
      check "the candidate differs from the source" (stagedDat staged /= fixtureDat)
      check "restore yields the source DAT" (restored == Right fixtureDat)
      check "entry table maps each stock entry to its owned symbol"
        ( entryTable admitted DomainB
            == [ EntryRow (SlotOffset 0x164) (GuestAddress 0xd1140) (SymbolName "own_164") (GuestAddress 0xd1140)
               , EntryRow (SlotOffset 0x168) (GuestAddress 0xd1160) (SymbolName "own_168") (GuestAddress 0xd1160)
               , EntryRow (SlotOffset 0x1618) (GuestAddress 0xd12bc) (SymbolName "own_1618") (GuestAddress 0xd12bc)
               ]
        )
      check "slot 0x164's frame composes its call" (fmap frameComposed (admittedFrame admitted DomainA (GuestAddress 0x41140)) == Just (FrameBytes 16))

  -- An owned byte between the extents.
  refusedAs "owned byte outside the regions" (OwnedByteOutsideRegions DomainA (GuestAddress 0x41200))
    (admitWith stockVariation (\domain -> if domain == DomainA then LinkedDomain (poke (0x41200 - 0x41140) ownedBin) (symbolsFile domain 0x41140) else linked domain) stockA stockB)

  -- An entry symbol off its region start, and a table entry off its region start.
  refusedAs "symbol entry not at region start" (EntryNotAtStart DomainB (SlotOffset 0x164))
    (admitWith stockVariation (\domain -> if domain == DomainB then LinkedDomain ownedBin (symbolsFile domain 0x41144) else linked domain) stockA stockB)
  refusedAs "table entry not at region start" (EntryNotAtStart DomainA (SlotOffset 0x164))
    (admitWith stockVariation {slot164Entry = 0x41144} linked stockA stockB)

  -- A loader file whose region bytes differ from the DAT's loader.
  refusedAs "stock-byte mismatch" (StockMismatch DomainA (SlotOffset 0x168)) (admitWith stockVariation linked (poke 0x1164 stockA) stockB)
  refusedAs "loader mismatch outside the regions" (LoaderMismatch DomainA) (admitWith stockVariation linked (poke 0x2000 stockA) stockB)

  -- A declared frame the walk does not reproduce.
  refusedAs "frame mismatch" (FrameMismatch DomainA (SlotFrame (SlotOffset 0x164)) (FrameBytes 8) (FrameBytes 16))
    (admitWith stockVariation {slot164Frame = 8} linked stockA stockB)

  -- A staged DAT with a word changed outside the regions does not restore to the source.
  case positive of
    Left failure -> check ("restore setup: " ++ show failure) False
    Right (source, admitted, stagedCandidate) -> do
      let staged = stagedDat stagedCandidate
      check "the staged DAT restores" (verifyRestore source admitted staged == Right ())
      let tampered = do
            parsed <- U.parseContainer staged
            patch <- F.loaderWordPatch (F.LoaderId 0) 0x41200 (U.word32At stockA 0x1200) 0
            parts <- F.applyLoaderWordPatches [patch] (U.parsedParts parsed)
            U.rebuildContainer parsed parts
      case tampered of
        Left failure -> check ("tampering: " ++ failure) False
        Right bytes -> refusedAs "non-roundtripping restore" RestoreMismatch (verifyRestore source admitted bytes)

  count <- readIORef failures
  unless (count == 0) exitFailure
  putStrLn "rtos-own-first-stage-tests: all checks passed"
