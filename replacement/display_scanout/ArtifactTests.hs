module Main where

import Control.Monad (forM_, unless)
import Data.Bits (shiftL, shiftR, testBit, (.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Int (Int32)
import Data.Word (Word32)
import Numeric (showHex)
import Replacement.DisplayScanout.Artifact
import Replacement.FixedImage.Layout (headerBytes, imageBase)

check :: String -> Bool -> IO ()
check label condition = unless condition (fail label)

row :: Word32 -> Char -> String -> String
row address kind name = pad (showHex address "") ++ " 00000010 " ++ [kind] ++ " " ++ name
  where pad text = replicate (8 - length text) '0' ++ text

origin, bodyEnd :: Word32
origin = imageBase + headerBytes
bodyEnd = origin + 0x2fe0

exports :: [(String, Word32)]
exports = zip [name | Entry name _ <- entries] [origin + 0xe0, origin + 0x1e0 ..]

symbols :: [String] -> B.ByteString
symbols extra = C.pack (unlines ([row address 'T' name | (name, address) <- exports] ++ [row origin 'T' "__fixed_body_start", row bodyEnd 'T' "__fixed_body_end"] ++ extra))

image4 :: B.ByteString
image4 = B.pack [fromIntegral (offset `shiftR` 2) | offset <- [0 .. 0x282800 - 1 :: Int]]

-- | The target of an A32 `b` at `address`.
branchTarget :: Word32 -> Word32 -> Maybe Word32
branchTarget address instruction
  | instruction .&. 0xff000000 /= 0xea000000 = Nothing
  | otherwise =
      let imm = instruction .&. 0xffffff
          signed = fromIntegral (if testBit imm 23 then imm - 0x1000000 else imm) :: Int32
      in Just (fromIntegral (toInteger address + 8 + toInteger signed `shiftL` 2))

littleEndian :: Word32 -> Word32
littleEndian address = let offset = fromIntegral (address - 0x61c400)
  in sum [fromIntegral (B.index image4 (offset + index)) * 256 ^ index | index <- [0 .. 3]]

refusal :: ArtifactError -> Either ArtifactError a -> Bool
refusal expected = either (== expected) (const False)

-- | The fixture's symbol rows with the row naming `name` replaced.
replacing :: String -> [String] -> B.ByteString
replacing name replacement = C.pack (unlines (concatMap (\line -> if names line then replacement else [line]) (lines (C.unpack (symbols [])))))
  where names line = case words line of [_, _, _, named] -> named == name; _ -> False

main :: IO ()
main = do
  case publicationsFor image4 (symbols []) of
    Left failure -> fail (show failure)
    Right rows -> do
      check "one publication per stock entry" (length rows == length entries)
      forM_ (zip entries rows) $ \(Entry name address, Publication published old new) -> do
        check (name ++ " is published at its stock entry") (published == address)
        check (name ++ " keeps the canonical word as its preimage") (old == littleEndian address)
        check (name ++ " branches to its fixed export") (branchTarget address new == lookup name exports)
  check "a missing export is refused" (refusal (MissingExport "xm1_scanout_blank") (publicationsFor image4 (replacing "xm1_scanout_blank" [])))
  check "an export outside the fixed range is refused" (refusal (ExportOutsideFixed "xm1_scanout_commit")
    (publicationsFor image4 (replacing "xm1_scanout_commit" [row 0x02000000 'T' "xm1_scanout_commit"])))
  check "a data symbol named as an export is refused" (refusal (MissingExport "xm1_scanout_isr_one")
    (publicationsFor image4 (replacing "xm1_scanout_isr_one" [row (origin + 0xe0) 'D' "xm1_scanout_isr_one"])))
  check "the body spans the unit's exports" (bodyBounds (symbols []) == Right (origin + 0xe0, origin + 0xbf0))
  check "the body extends to the unit's own code" (bodyBounds (symbols [row (origin + 0x2000) 't' "_RNvCs1_19xm1_display_scanout4tail"]) == Right (origin + 0xe0, origin + 0x2010))
  check "a core instantiation inside the unit's code is admitted" (bodyBounds (symbols [row (origin + 0x1f0) 't' "_RNvNtCs2_4core3ops5index"]) == Right (origin + 0xe0, origin + 0xbf0))
  check "another crate's code inside the unit's range is refused" (refusal BodyBoundsRejected
    (bodyBounds (symbols [row (origin + 0x1f0) 'T' "_RNvCs3_10xm1_saclac4step"])))
  check "a body without every export is refused" (refusal BodyBoundsRejected (bodyBounds (replacing "xm1_scanout_blank" [])))
  check "a non-canonical image 4 is refused" (refusal SourceMismatch (canonicalPublications image4 (symbols [])))
  check "a malformed symbol row is refused" (either (const True) (const False) (publicationsFor image4 (symbols ["garbage"])))
