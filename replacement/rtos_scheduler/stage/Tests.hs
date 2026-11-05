module Main (main) where

import Control.Monad (forM_, unless)
import Data.Bits (complement)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Fixtures (fixtureDat)
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Replacement.RtosOwnFirst.Regions
import Replacement.RtosScheduler.Artifact
import System.Environment (getArgs)

right :: Show e => Either e a -> IO a
right = either (fail . show) pure
expect :: String -> (StageError -> Bool) -> Either StageError a -> IO ()
expect label predicate result = case result of
  Left err | predicate err -> pure ()
  Left err -> fail (label ++ ": wrong refusal " ++ show err)
  Right _ -> fail (label ++ ": accepted")
replace :: Int -> B.ByteString -> B.ByteString -> B.ByteString
replace at replacement bytes = B.take at bytes <> replacement <> B.drop (at + B.length replacement) bytes
replaceText :: String -> String -> B.ByteString -> B.ByteString
replaceText before after bytes =
  let (prefix, suffix) = C.breakSubstring (C.pack before) bytes
   in if B.null suffix then error "fixture field absent" else prefix <> C.pack after <> B.drop (length before) suffix
swapSymbol :: String -> String -> Linked -> Linked
swapSymbol old new linked = linked {linkedSymbols = C.unlines [if old `elem` words (C.unpack line) then C.pack new else line | line <- C.lines (linkedSymbols linked)]}

main :: IO ()
main = do
  args <- getArgs
  case args of
    [regionsPath,aPath,asPath,bPath,bsPath] -> do
      regions <- B.readFile regionsPath
      table <- right (parseTable regions)
      a <- Linked <$> B.readFile aPath <*> B.readFile asPath
      b <- Linked <$> B.readFile bPath <*> B.readFile bsPath
      parsed <- right (U.parseContainer fixtureDat)
      patches <- right (sequence [F.loaderWordPatch (F.LoaderId loader) (base+offset) (old base loader offset parsed) word | (loader,base,code) <- [(0,0x433e4,[0xe24dd004,0xe28dd004,0xe12fff1e]),(1,0xd33e4,[0xe24dd004,0xe28dd004,0xe12fff1e]),(0,0x43ca4,[0xe12fff1e])], (offset,word) <- zip [0,4..] code])
      parts <- right (F.applyLoaderWordPatches patches (U.parsedParts parsed))
      dat <- right (U.rebuildContainer parsed parts)
      rebuilt <- right (U.parseContainer dat)
      let loader d = let seg = [s | s <- U.partSegments (U.parsedParts rebuilt), U.segId s == domainLoaderId d] !! 0 in B.map complement (B.take (domainLoaderSize d) (B.drop (fromIntegral (U.segFlash seg)) (U.partPreTable (U.parsedParts rebuilt))))
      source <- right (canonicalSource dat (loader DomainA) (loader DomainB))
      admitted <- right (admit source table Candidate a b)
      staged <- right (stage admitted)
      restored <- right (restore admitted (stagedDat staged))
      unless (restored == dat && stagedDat staged /= dat) (fail "staging and restore")
      forM_ [StockInsert,SilentNotify,WrongPredicate] $ \variant -> do
        ca <- right (controlLinked variant DomainA a)
        cb <- right (controlLinked variant DomainB b)
        control <- right (admit source table variant ca cb >>= stage)
        unless (stagedDat control /= stagedDat staged) (fail "control did not change DAT")
      expect "pin source" (== SourceUnpinned) (requirePinned source)
      expect "loader identity" (\e -> case e of LoaderMismatch DomainA -> True; _ -> False) (canonicalSource dat (B.cons 0 (B.drop 1 (loader DomainA))) (loader DomainB))
      expect "container" (\e -> case e of ContainerRejected _ -> True; _ -> False) (canonicalSource B.empty (loader DomainA) (loader DomainB))
      expect "malformed table" (\e -> case e of TableRejected _ -> True; _ -> False) (parseTable (C.pack "{}"))
      forM_ [("fill_word\": \"0xe7f000f0","fill_word\": \"0x0"),("loader_base\": \"0x40000","loader_base\": \"0x40004"),("entry\": \"0x439d8","entry\": \"0x439dc"),("address\": \"0x43c74","address\": \"0x43c78"),("stock_raise_irq\": \"0x433e4","stock_raise_irq\": \"0x433e8")] $ \(before,after) ->
        expect before (\e -> case e of TableRejected _ -> True; _ -> False) (parseTable (replaceText before after regions))
      let first = rowsA table !! 0
          changed row = table {rowsA = row : drop 1 (rowsA table)}
          admitA t x = admit source t Candidate x b
          entryErr e = case e of EntryMismatch DomainA Acquire -> True; _ -> False
          markerErr e = case e of MarkerMismatch DomainA _ -> True; _ -> False
          symbolErr e = case e of SymbolOutside DomainA _ -> True; _ -> False
          slackErr e = case e of SlackNotFill DomainA _ -> True; _ -> False
          byteErr e = case e of ByteOutside DomainA _ -> True; _ -> False
          frameErr e = case e of FrameRejected DomainA _ -> True; _ -> False
      expect "malformed symbols" (== SymbolsRejected DomainA) (admitA table a {linkedSymbols = C.pack "invalid\n"})
      expect "missing entry" markerErr (admitA table (swapSymbol "xm1_rtos_scheduler_acquire" "000439d8 00000030 T renamed_entry" a))
      expect "extent pin" (== PinMismatch DomainA) (admitA (changed first {window = Extent (GuestAddress 0x439d8) (GuestAddress 0x43a10)}) a)
      expect "entry" entryErr (admitA table (swapSymbol "xm1_rtos_scheduler_acquire" "000439dc 00000028 T xm1_rtos_scheduler_acquire" a))
      expect "end marker" markerErr (admitA table (swapSymbol "__acquire_end" "00043a10 00000000 A __acquire_end" a))
      expect "shim marker" markerErr (admitA table (swapSymbol "__prepare_body_start" "00043c78 00000000 T __prepare_body_start" a))
      expect "symbol containment" symbolErr (admitA table a {linkedSymbols = linkedSymbols a <> C.pack "00042000 00000004 T outside\n"})
      expect "absolute symbol containment" symbolErr (admitA table a {linkedSymbols = linkedSymbols a <> C.pack "40000000 00000000 A outside_absolute\n"})
      expect "symbol span" symbolErr (admitA table (swapSymbol "xm1_rtos_scheduler_acquire" "000439d8 00000040 T xm1_rtos_scheduler_acquire" a))
      expect "gap byte" byteErr (admitA table a {linkedBin = replace (0x43a3c-0x439d8) (B.singleton 1) (linkedBin a)})
      expect "trailing byte" byteErr (admitA table a {linkedBin = linkedBin a <> B.singleton 0})
      expect "slack fill" slackErr (admitA table a {linkedBin = replace (0x44040-0x439d8) (U.putWord32 0) (linkedBin a)})
      expect "walk refusal" frameErr (admitA table a {linkedBin = replace 0 (U.putWord32 0xe7f000f0) (linkedBin a)})
      expect "frame demand" (\e -> case e of FrameMismatch DomainA Acquire _ _ -> True; _ -> False) (admitA (changed first {demand = FrameBytes 0}) a)
      expect "DAT confinement" (== ChangeOutside 0) (confineChanges admitted (replace 0 (B.singleton 0) (stagedDat staged)))
      expect "DAT length" (\e -> case e of ChangeOutside _ -> True; _ -> False) (confineChanges admitted (stagedDat staged <> B.singleton 0))
      putStrLn "scheduler staging and refusal checks passed"
    _ -> fail "expected region table and linked binaries/symbols"
  where
    old base loader offset parsed = let seg = [s | s <- U.partSegments (U.parsedParts parsed), U.segId s == loader] !! 0 in complement (U.word32At (U.partPreTable (U.parsedParts parsed)) (fromIntegral (U.segFlash seg + base + offset - U.segLoad seg)))
