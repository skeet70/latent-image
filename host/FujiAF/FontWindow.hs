-- The id-3 font window (FONT_ICON.FDL) in a parsed update container: entry
-- 3's flash span, carried in the post-tail span after the id-2 extent. The
-- loader reads the whole window raw to entry 3's load address in 0x20000-byte
-- blocks, so the window and not the segment size bounds what is loaded.
module FujiAF.FontWindow
  ( fontWindow
  , extractFont
  , replaceFont
  ) where

import Data.Bits (complement)
import qualified Data.ByteString as B
import FujiAF.UpdateContainer

-- The window's media [start, stop), refused unless it is a nonempty
-- 0x20000-aligned span inside the post-tail span.
fontWindow :: Parts -> Either String (Int, Int)
fontWindow parts = do
  entry <- case [e | e <- partSegments parts, segId e == 3] of
    [e] -> Right e
    _ -> Left "the segment table does not hold exactly one entry 3"
  let start = toInteger (segFlash entry)
      stop = start + toInteger (segFlashLen entry)
      postStart = toInteger (postTailStart parts)
      postStop = postStart + toInteger (B.length (partPostTail parts))
  if segFlashLen entry == 0 || start `mod` 0x20000 /= 0 || toInteger (segFlashLen entry) `mod` 0x20000 /= 0
    then Left ("the font window is not a nonempty 0x20000-aligned span: flash " ++ showHexWord (segFlash entry) ++ " length " ++ showHexWord (segFlashLen entry))
    else pure ()
  if segSize entry > segFlashLen entry
    then Left ("the font size " ++ showHexWord (segSize entry) ++ " exceeds its window " ++ showHexWord (segFlashLen entry))
    else pure ()
  if start < postStart || stop > postStop
    then Left ("the font window " ++ show start ++ ".." ++ show stop ++ " lies outside the span carried after the image-2 extent, " ++ show postStart ++ ".." ++ show postStop)
    else pure ()
  pure (fromInteger start, fromInteger stop)

-- The window in media form, the loader's raw copy.
extractFont :: Parts -> Either String MediaImage
extractFont parts = do
  (start, stop) <- fontWindow parts
  pure (MediaImage (B.map complement (B.take (stop - start) (B.drop (start - postTailStart parts) (partPostTail parts)))))

-- The whole window replaced in place; no segment, descriptor or trailer word
-- changes. Emit through rebuildContainer.
replaceFont :: Parsed -> MediaImage -> Either String Parts
replaceFont source media = do
  let parts = parsedParts source
      bytes = mediaImageBytes media
      postTail = partPostTail parts
  (start, stop) <- fontWindow parts
  let offset = start - postTailStart parts
  if B.length bytes /= stop - start
    then Left ("font window is " ++ show (B.length bytes) ++ " bytes against the stored flash length " ++ show (stop - start))
    else Right parts {partPostTail = B.take offset postTail <> B.map complement bytes <> B.drop (offset + B.length bytes) postTail}

-- The post-tail span's media start: the id-2 extent's end.
postTailStart :: Parts -> Int
postTailStart parts =
  let (image2Start, _, _) = image2Extent parts
   in image2Start + B.length (partImage2 parts)
