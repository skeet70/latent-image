module Main (main) where

import Control.Monad (forM_, unless)
import Data.Bits (shiftR)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word32)
import Replacement.RecordStore.Carrier
import Replacement.RecordStore.References
import Replacement.PropertyService.Placement
import ServiceInventory (Image (..))
import System.Environment (getArgs)
import Numeric (showHex)

word :: Word32 -> B.ByteString
word value = B.pack [fromIntegral (value `shiftR` bit) | bit <- [0,8,16,24]]

check :: String -> Bool -> IO ()
check name ok = unless ok (fail name)

main :: IO ()
main = do
  ranges <- either fail pure (parseRanges (C.pack "range\tmain\ttest\t0x00bf1a00\t12524032\t8\tdigest\tfunction\n"))
  let resolved bytes = resolvedReferences ranges [(ScanImage5, Image "image5" "" 0xbf1a00 bytes)]
  check "literal-loaded retired data refuses" (not (null (resolved (word 0xe59f0000 <> word 0xe12fff1e <> word 0x8a0080))))
  check "literal-loaded entry is retained" (null (resolved (word 0xe59f0000 <> word 0xe12fff1e <> word 0x6ac2b4)))
  check "MOVW MOVT retired code refuses" (not (null (resolved (word 0xe30c02b8 <> word 0xe340006a))))
  check "arithmetic encoding is an unresolved raw candidate" (null (resolved (word 0x008a0080)))
  check "data encoding is not a resolved branch" (null (resolved (word 0xe12fff1e <> word 0xe12fff1e <> word 0xebeb6a2c)))
  let scan base bytes = scanRetired [(ScanImage5, Image "image5" "" base bytes)]
  check "entry pointer is retained" (null (scan 0xbf1a00 (word 0x6ac2b4)))
  check "internal code pointer refuses" (not (null (scan 0xbf1a00 (word 0x6ac2b8))))
  check "slot pointer refuses" (not (null (scan 0xbf1a00 (word 0x89f8f8))))
  check "cache pointer refuses" (not (null (scan 0xbf1a00 (word 0x8c0080))))
  check "tag pointer refuses" (not (null (scan 0xbf1a00 (word 0x8e0088))))
  check "saved destination refuses" (not (null (scan 0xbf1a00 (word 0x917fb4))))
  check "staging pointer retained" (null (scan 0xbf1a00 (word 0x89fa80)))
  let gapScan label base bytes = scanObsoleteGap [(label, Image "fixture" "" base bytes)]
  check "retained obsolete gap pointer refuses" (not (null (gapScan ScanImage5 0xbf1a00 (word 0x89f8f0))))
  check "retained obsolete gap interior pointer refuses" (not (null (gapScan ScanPlain 0x1100000 (word 0x89f8f7))))
  check "retired obsolete gap pointer is ignored" (null (gapScan ScanImage4 0x6ac2b4 (word 0x89f8f0)))
  let addresses = [address | GuestAddress address <- entryAddresses]
      names = ["open","register","close","read_latest","append","erase_all","collect","read_at","zero"]
      entrySymbols = concat [showHex address " 00000004 T record_store_entry_" ++ name ++ "\n" ++ showHex (address+8) " 00000004 T record_store_" ++ name ++ "\n" | (address,name) <- zip addresses names]
      layout = [(C.unpack name,address) | (name,address) <- cacheSymbols]
      symbolRows rows = C.pack (entrySymbols ++ concat [showHex address " 00000000 A " ++ name ++ "\n" | (name,address) <- rows])
      symbols = symbolRows layout
      linked = foldr (\address bytes -> let at = fromIntegral (address - 0x6ac2b4) in B.take at bytes <> word 0xea000000 <> B.drop (at+4) bytes) (B.replicate 0x1e60 0) addresses
      source = B.replicate 0x100000 0x77
      offset = 0x6ac2b4 - 0x61c400
  case stageImage source linked symbols of
    Left err -> fail err
    Right result -> do
      check "staged span equals linked image" (B.take 0x1e60 (B.drop offset result) == linked)
      check "prefix preserved" (B.take offset result == B.take offset source)
      check "suffix preserved" (B.drop (offset+0x1e60) result == B.drop (offset+0x1e60) source)
  check "missing entry symbols refuse" (case stageImage source linked B.empty of Left _ -> True; Right _ -> False)
  check "wrong linked extent refuses" (case stageImage source (B.drop 4 linked) symbols of Left _ -> True; Right _ -> False)
  forM_ layout $ \(name,address) -> do
    check ("missing layout symbol refuses: " ++ name) (case stageImage source linked (symbolRows (filter ((/= name) . fst) layout)) of Left _ -> True; Right _ -> False)
    check ("moved layout symbol refuses: " ++ name) (case stageImage source linked (symbolRows ((name,address+4):filter ((/= name) . fst) layout)) of Left _ -> True; Right _ -> False)
  check "obsolete tag binding refuses" (case stageImage source linked (symbolRows (("record_store_cache_tag",0x89f8f0):layout)) of Left _ -> True; Right _ -> False)
  check "obsolete gap alias refuses" (case stageImage source linked (symbolRows (("unexpected_alias",0x89f8f4):layout)) of Left _ -> True; Right _ -> False)
  check "owned cache interior alias refuses" (case stageImage source linked (symbolRows (("unexpected_alias",0x8c0084):layout)) of Left _ -> True; Right _ -> False)
  check "owned ages alias refuses" (case stageImage source linked (symbolRows (("unexpected_alias",0x89fa64):layout)) of Left _ -> True; Right _ -> False)
  forM_ layout $ \binding@(name,_) ->
    check ("duplicated layout symbol refuses: " ++ name) (case stageImage source linked (symbolRows (binding:layout)) of Left _ -> True; Right _ -> False)
  args <- getArgs
  let linkedPairs [] = pure ()
      linkedPairs (binPath:symbolsPath:rest) = do
        actual <- B.readFile binPath
        actualSymbols <- B.readFile symbolsPath
        either fail (const (pure ())) (stageImage source actual actualSymbols)
        linkedPairs rest
      linkedPairs _ = fail "expected linked bin/symbol pairs"
  linkedPairs args
