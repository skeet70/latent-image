-- SPDX-License-Identifier: AGPL-3.0-only
{-# LANGUAGE GeneralizedNewtypeDeriving #-}
module Main (main) where

import Control.Exception (SomeException, displayException, try)
import Control.Monad (forM_)
import qualified Data.ByteString as B
import Data.List (nub, sort, sortOn)
import qualified Data.Map.Strict as M
import Data.Word (Word16, Word8)
import FujiAF.ConsoleStore (word16le)
import qualified FujiAF.Host as H
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)
import Text.Printf (PrintfArg, printf)

newtype Sequence = Sequence Word16 deriving (Eq, Ord, Show, PrintfArg)
newtype RecordOffset = RecordOffset Int deriving (Eq, Ord)
data Bank = FirstBank | SecondBank deriving (Eq)
data Region = RSeq | RCounters | RBody | RPath | RStamp | RParity | ROther deriving (Eq)
data Record = Record { sequenceId :: Sequence, recordBank :: Bank, recordBytes :: B.ByteString }
data Move = Move RecordOffset Word8 Word8

regionOf :: RecordOffset -> Region
regionOf (RecordOffset o)
  | o <= 0x01 = RSeq
  | o >= 0x24 && o <= 0x27 = RCounters
  | o >= 0x100 && o <= 0x11c = RPath
  | o >= 0x28 && o <= 0x13f = RBody
  | o >= 0x1e8 && o <= 0x1e9 = RStamp
  | o >= 0x1ec && o <= 0x1ff = RParity
  | otherwise = ROther

offLabel :: RecordOffset -> String
offLabel (RecordOffset o)
  | o >= 0x100 && o <= 0x11c = printf "path+%02x" (o-0x100)
  | o >= 0x28 && o <= 0x13f = printf "p+%03x" (o-0x28)
  | otherwise = printf "r+%03x" o
moveLabel :: Move -> String
moveLabel (Move o x y) = printf "%s:%02x>%02x" (offLabel o) x y
movesOf :: B.ByteString -> B.ByteString -> [Move]
movesOf a b = [Move (RecordOffset o) (B.index a o) (B.index b o) | o <- [0..511], B.index a o /= B.index b o]
body :: Record -> B.ByteString
body = B.take 0x118 . B.drop 0x28 . recordBytes

readBank :: Bank -> FilePath -> IO [Record]
readBank bank path = do
  bytes <- H.readRegular 0x20000 path
  H.ensure (B.length bytes == 0x20000) "Settings bank must contain a complete 128-KiB capture"
  let marked = [Record (Sequence seqNo) bank raw | slot <- [0..30], let raw = B.take 512 (B.drop (slot*512) bytes), let seqNo = word16le raw 0, seqNo /= 0xffff]
  H.ensure (all (\r -> let Sequence value = sequenceId r in value < 0x8000) marked) "Settings sequence outside 15-bit range"
  pure marked

chainOrder :: [Record] -> Either String [Record]
chainOrder [] = Left "Settings banks contain no marked records"
chainOrder records
  | M.size bySeq /= length records = Left "Duplicate settings sequence"
  | otherwise = mapM (\value -> maybe (Left "Noncontiguous settings sequence chain") Right (M.lookup (Sequence (fromIntegral value)) bySeq)) order
  where
    bySeq = M.fromList [(sequenceId r,r) | r <- records]
    ring = sort [fromIntegral value :: Int | r <- records, let Sequence value = sequenceId r]
    count = length ring
    gap i = ((ring !! ((i+1) `mod` count) - ring !! i) `mod` 0x8000,i)
    (_,largest) = maximum [gap i | i <- [0..count-1]]
    start = ring !! ((largest+1) `mod` count)
    order = take count (iterate (\value -> (value+1) `mod` 0x8000) start)

runs :: [RecordOffset] -> [(RecordOffset,RecordOffset)]
runs [] = []
runs (x:xs) = go x x xs
  where
    go lo hi [] = [(lo,hi)]
    go lo (RecordOffset hi) (y@(RecordOffset value):ys)
      | value == hi+1 = go lo y ys
      | otherwise = (lo,RecordOffset hi):go y y ys
showRuns :: [RecordOffset] -> String
showRuns offsets = concat [if lo == hi then printf "[+%03x]" lo else printf "[+%03x..+%03x]" lo hi | (RecordOffset lo,RecordOffset hi) <- runs offsets]

report :: [Record] -> IO ()
report [] = fail "Settings banks contain no marked records"
report records@(first:_) = do
  let bodies = nub (map body records)
      bodyId value = 1+length (takeWhile (/= value) bodies)
      pairs = [(a,b,movesOf (recordBytes a) (recordBytes b)) | (a,b) <- zip records (drop 1 records)]
      marked0 = length (filter ((== FirstBank) . recordBank) records)
      marked1 = length records-marked0
      free = 62-length records
      final = foldl' (\_ record -> record) first records
  printf "chain: %d marked records, sequence 0x%04x..0x%04x, contiguous yes\n" (length records) (sequenceId first) (sequenceId final)
  forM_ (zip [(1 :: Int)..] bodies) $ \(i,value) -> do
    let members = [sequenceId r | r <- records, body r == value]
    printf "body group %d: %d record(s), sequences %s\n" i (length members) (unwords (map (printf "0x%04x") members))
  printf "fill: bank 0 %d/31, bank 1 %d/31, free slots before the first bank erase %d\n" marked0 marked1 free
  printf "capacity gate: conservative acts that fit after read 0 = max(0, free - 2) = %d; physical no-erase limit = %d (cut endings and no-change verification boots cost 0; margin 2)\n" (max 0 (free-2)) free
  putStrLn "\n== consecutive pair diffs =="
  forM_ pairs $ \(a,b,moves) -> do
    let by region = [m | m@(Move offset _ _) <- moves, regionOf offset == region]
        bodyMoves = by RBody
        pathMoves = by RPath
    printf "0x%04x -> 0x%04x  body-group %d | seq %s | counters %s | body %d%s | path %d%s | stamp %s | parity %d | other %d\n"
      (sequenceId a) (sequenceId b) (bodyId (body b)) (unwords (map moveLabel (by RSeq))) (unwords (map moveLabel (by RCounters)))
      (length bodyMoves) (if null bodyMoves then "" else " " ++ unwords (map moveLabel bodyMoves))
      (length pathMoves) (if null pathMoves then "" else " " ++ showRuns [o | Move o _ _ <- pathMoves] ++ " " ++ unwords (map moveLabel pathMoves))
      (unwords (map moveLabel (by RStamp))) (length (by RParity)) (length (by ROther))
  putStrLn "\n== per-offset census over all pairs =="
  let changes = M.fromListWith (++) [(offset,[(x,y)]) | (_,_,moves) <- pairs, Move offset x y <- moves]
  forM_ (sortOn fst (M.toList changes)) $ \(offset@(RecordOffset value),transitions) ->
    printf "%s (r+%03x) moved %2d times: %s\n" (offLabel offset) value (length transitions) (unwords (nub [printf "%02x>%02x" x y | (x,y) <- transitions]))

main :: IO ()
main = do
  result <- try $ do
    args <- getArgs
    case args of
      ["--help"] -> putStrLn "Usage: log-diff-check BANK0_FILE BANK1_FILE\nX-M1 1.50 area-2 append-chain report from complete 128-KiB bank captures; no camera I/O."
      [first,second] -> do
        a <- readBank FirstBank first
        b <- readBank SecondBank second
        records <- either fail pure (chainOrder (a++b))
        report records
      _ -> fail "Usage: log-diff-check BANK0_FILE BANK1_FILE"
  case result :: Either SomeException () of
    Left failure -> hPutStrLn stderr (displayException failure) >> exitFailure
    Right () -> pure ()
