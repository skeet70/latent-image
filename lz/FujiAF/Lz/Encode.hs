-- Encoder for the X-M1 1.50 hardware-compressed record format
-- (FujiAF.Lz.Format). Re-encoding the two decoded stock images reproduces the
-- stock record streams byte for byte (lz-check, reports/firmware-extraction.md
-- "The encoder"), so the match and page-fill policies below are the ones the
-- vendor streams exhibit, not merely decodable choices:
--
--   * greedy longest match at every output position, the candidate's length
--     capped at its distance (stock matches never self-overlap) and at 15,
--     a match needs length 3, and equal lengths resolve to the nearest
--     distance;
--   * literal runs hold bytes without a match of length 3, at most 127;
--   * feed boundaries (record offsets that are multiples of the header chunk
--     word) end tokens: a run clamps to a page end or to 127 and continues
--     after it, a run that would strand one byte of page absorbs the next
--     output byte as a literal so it fills the page, a match with four bytes
--     of page left becomes a one-byte literal run plus the match tail (same
--     distance) ending at the page, and a match with three bytes left becomes
--     a two-byte literal run ending at the page plus the match tail at the
--     next one. Matches elsewhere are whole; the window runs across pages.
module FujiAF.Lz.Encode
  ( encodeRecord
  , EncodeError (..)
  ) where

import Control.Monad.ST (runST)
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import qualified Data.Vector.Unboxed.Mutable as MV
import Data.Word (Word32)
import FujiAF.Lz.Format
  ( Header (..)
  , Token (..)
  , chunkSize
  , encodeHeader
  , encodeToken
  , mkDistance
  , mkMatchLen
  , windowSize
  )

data EncodeError
  = EmptyImage
  | ImageTooLarge
  | WindowExceeded
  | FeedAlignedTotalLen
  | InvalidTokenization
  deriving (Show, Eq)

-- Stock matches never overlap themselves (len <= dist), so the minimum
-- distance is the minimum match length.
minDistance :: Int
minDistance = 3

maxMatch :: Int
maxMatch = 15

literalRunCap :: Int
literalRunCap = 0x7f

headerLen :: Int
headerLen = 0x14

-- Encode an image into a complete record: header plus token stream. The
-- record must fit the segment entry's flash window in bytes (WindowExceeded):
-- the stock records already use 89 and 96 percent of theirs, so the bound
-- belongs here rather than in the replacement path's memory. A stream whose
-- total_len lands on a feed boundary is refused (FeedAlignedTotalLen): the
-- final kick would be a full chunk, a geometry no stock record shows and
-- whose failure shape is a boot hang. The emitted tokens are re-walked
-- (InvalidTokenization) so no record can leave with a zero-length run, a
-- token starting at feed room one, or a token crossing a feed boundary.
encodeRecord :: Word32 -> B.ByteString -> Either EncodeError B.ByteString
encodeRecord window image
  | B.null image = Left EmptyImage
  | B.length image > 0xffffffff = Left ImageTooLarge
  | otherwise =
      let tokens = encodeTokens image
          stream = B.concat (map encodeToken tokens)
          totalLen = fromIntegral (headerLen + B.length stream)
          pages = (totalLen + fromIntegral chunkSize - 1) `div` fromIntegral chunkSize
          header =
            Header
              { headerOutSize = fromIntegral (B.length image)
              , headerTotalLen = totalLen
              , headerPages = pages
              , headerChunk = fromIntegral chunkSize
              , headerMode = 4
              }
       in do
            validateTokens tokens
            if totalLen `mod` fromIntegral chunkSize == 0
              then Left FeedAlignedTotalLen
              else Right ()
            if totalLen > window then Left WindowExceeded else Right ()
            Right (encodeHeader header <> stream)

-- The shipped-record invariants, in record offsets: runs 1..127 with matching
-- payloads, no token start at feed room one, no token crossing a feed
-- boundary.
validateTokens :: [Token] -> Either EncodeError ()
validateTokens = walk headerLen
  where
    walk _ [] = Right ()
    walk pos (token : rest) = do
      len <- case token of
        LiteralRun count bytes
          | count >= 1 && count <= 0x7f && B.length bytes == fromIntegral count -> Right (1 + fromIntegral count)
          | otherwise -> Left InvalidTokenization
        Match _ _ -> Right 2
      let roomLeft = chunkSize - (pos `mod` chunkSize)
      if roomLeft == 1 || roomLeft < len
        then Left InvalidTokenization
        else walk (pos + len) rest

-- The token stream: greedy matches with the feed-page fill rules applied while
-- walking the output. Tokens come back in stream order.
encodeTokens :: B.ByteString -> [Token]
encodeTokens image = runST $ do
  heads <- MV.replicate hashSlots (negate 1)
  next <- MV.replicate n (negate 1)
  let insert p
        | p + 3 > n = pure ()
        | otherwise = do
            let h = hashAt image p
            old <- MV.read heads h
            MV.write next p old
            MV.write heads h p
      insertRange a b = mapM_ insert [a .. b - 1]
      bestAt i
        | i + 3 > n = pure Nothing
        | otherwise = do
            let walk j best
                  | j < 0 || i - j > windowSize = pure best
                  | otherwise = do
                      let l = min (i - j) (matchLen image j i maxMatch)
                      best' <-
                        if l >= minDistance
                          then case best of
                            Just (_, bl) | bl >= l -> pure best
                            _ -> pure (Just (i - j, l))
                          else pure best
                      case best' of
                        Just (_, bl) | bl >= maxMatch -> pure best'
                        _ -> do
                          j' <- MV.read next j
                          walk j' best'
            j0 <- MV.read heads (hashAt image i)
            walk j0 Nothing
      runToken start len = LiteralRun (fromIntegral len) (B.take len (B.drop start image))
      matchToken d l =
        case (mkDistance (fromIntegral d), mkMatchLen (fromIntegral l)) of
          (Just distance, Just len) -> Match distance len
          _ -> error "FujiAF.Lz.Encode: match outside the token domain"
      room sp = chunkSize - ((sp + headerLen) `mod` chunkSize)
      flushRun i pending acc
        | pending == 0 = acc
        | otherwise = runToken (i - pending) pending : acc
      emitMatch i sp d l acc r
        | r == 4 = pure (i + l, sp + 4, matchToken d (l - 1) : runToken i 1 : acc)
        | r == 3 = pure (i + l, sp + 5, matchToken d (l - 2) : runToken i 2 : acc)
        | otherwise = pure (i + l, sp + 2, matchToken d l : acc)
      go i sp pending acc
        | i >= n = pure (reverse (flushRun i pending acc))
        | pending == room sp - 1 || pending == literalRunCap =
            -- At feed room 129 a full 127-run would leave one byte of page,
            -- where no token fits; flush one byte short and let the fill
            -- guard close the page with the remainder.
            let keep = if pending == literalRunCap && room sp == literalRunCap + 2 then 1 else 0
                emit = pending - keep
             in if emit <= 0
                  then error "FujiAF.Lz.Encode: zero-length run flush at feed room one"
                  else go i (sp + 1 + emit) keep (runToken (i - pending) emit : acc)
        | otherwise = do
            best <- bestAt i
            let r = room sp
            case best of
              Just (d, l)
                | pending > 0 && r - 1 - pending == 1 -> do
                    insert i
                    go (i + 1) sp (pending + 1) acc
                | pending > 0 -> do
                    insertRange i (i + l)
                    let sp' = sp + 1 + pending
                    (j, sp2, acc2) <- emitMatch i sp' d l (runToken (i - pending) pending : acc) (room sp')
                    go j sp2 0 acc2
                | r == 4 -> do
                    insertRange i (i + l)
                    go (i + l) (sp + 4) 0 (matchToken d (l - 1) : runToken i 1 : acc)
                | r == 3 -> do
                    insertRange i (i + l)
                    go (i + l) (sp + 5) 0 (matchToken d (l - 2) : runToken i 2 : acc)
                | otherwise -> do
                    insertRange i (i + l)
                    go (i + l) (sp + 2) 0 (matchToken d l : acc)
              Nothing -> do
                insert i
                go (i + 1) sp (pending + 1) acc
  go 0 0 0 []
  where
    n = B.length image
    hashSlots = 65536

hashAt :: B.ByteString -> Int -> Int
hashAt image i =
  ( (fromIntegral (B.index image i) * 31 + fromIntegral (B.index image (i + 1))) * 31
      + fromIntegral (B.index image (i + 2))
  )
    .&. 0xffff

-- The longest run of equal bytes at i and j, at most cap.
matchLen :: B.ByteString -> Int -> Int -> Int -> Int
matchLen image j i cap = go 0
  where
    go l
      | l >= cap = cap
      | i + l >= B.length image = l
      | B.index image (i + l) == B.index image (j + l) = go (l + 1)
      | otherwise = l
