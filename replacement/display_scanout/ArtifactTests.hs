module Main where

import Control.Monad (forM_, unless)
import Data.Bits (shiftL, shiftR, testBit, (.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Int (Int32)
import Data.Word (Word32)
import Numeric (showHex)
import Replacement.DisplayScanout.Artifact

check :: String -> Bool -> IO ()
check label condition = unless condition (fail label)

row :: Word32 -> Char -> String -> String
row address kind name = pad (showHex address "") ++ " 00000010 " ++ [kind] ++ " " ++ name
  where pad text = replicate (8 - length text) '0' ++ text

exports :: [(String, Word32)]
exports = zip [name | Entry name _ <- entries] [0xba0100, 0xba0200 ..]

symbols :: [String] -> B.ByteString
symbols extra = C.pack (unlines ([row address 'T' name | (name, address) <- exports] ++ [row 0xba0020 'T' "__fixed_body_start", row 0xba3000 'T' "__fixed_body_end"] ++ extra))

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
    (publicationsFor image4 (replacing "xm1_scanout_isr_one" [row 0xba0100 'D' "xm1_scanout_isr_one"])))
  check "a body that does not start at the fixed origin is refused" (refusal BodyBoundsRejected
    (bodyBounds (C.pack (unlines [row 0xba0040 'T' "__fixed_body_start", row 0xba3000 'T' "__fixed_body_end"]))))
  check "a non-canonical image 4 is refused" (refusal SourceMismatch (canonicalPublications image4 (symbols [])))
  check "a malformed symbol row is refused" (either (const True) (const False) (publicationsFor image4 (symbols ["garbage"])))
