module ReferenceScan
  ( Address (..), Image (..), Reference (..), ReferenceKind (..)
  , scanReferences, engineTargets, renderReference
  ) where

import qualified Data.ByteString as B
import Data.Bits
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import Numeric (showHex)

newtype Address = Address Word32 deriving (Eq, Ord, Show)
data Image = Image { imageName :: String, imageBase :: Address, imageBytes :: B.ByteString }
data ReferenceKind = PointerWord | PointerHalfword | ArmB | ArmBL | ArmBLX
  | ThumbBL | ThumbBLX | ThumbBW | ThumbConditionalBW
  | ArmMovPair | ThumbMovPair | ArmADR | ThumbADR
  deriving (Eq, Ord, Show)
data Reference = Reference
  { referenceTarget :: Address, referenceKind :: ReferenceKind
  , referenceImage :: String, referenceSite :: Address
  } deriving (Eq, Ord, Show)
newtype Register = Register Word32 deriving (Eq, Ord)
data MoveLow = MoveLow Word32 Word32

engineTargets :: [Address]
engineTargets = map Address
  [0xbfa234,0xbf976c,0xbf985c,0xbf9a2c,0xbfa800,0xbfad04,0xbf9a5c,0xbf9f44,0xbfab98
  ,0xbf9794,0xbf9990,0xbf9b1c,0xbf988c,0xbf9c1c,0xbf95b8,0xbf95a8,0xbf90f0,0xbfaf84]

-- Pairs are candidates, not data-flow proofs: the same register within 64 bytes.
scanReferences :: [Address] -> Image -> [Reference]
scanReferences targets (Image name (Address base) bytes) = go 0 M.empty M.empty
  where
    wanted = S.fromList targets
    half off = fromIntegral (B.index bytes off) .|. shiftL (fromIntegral (B.index bytes (off + 1))) 8 :: Word32
    hit kind pc target = [Reference (Address target) kind name (Address pc) | S.member (Address target) wanted]
    pointer kind pc target = hit kind pc (target .&. complement 1)
    paired kind pc reg hi lows = case M.lookup reg lows of
      Just (MoveLow lo at) | pc - at <= 64 -> pointer kind pc (shiftL hi 16 .|. lo)
      _ -> []
    go off armLows thumbLows
      | off + 2 > B.length bytes = []
      | otherwise = short ++ if off + 4 > B.length bytes then [] else refs ++ go (off + 2) armNext thumbNext
      where
        pc = base + fromIntegral off
        a = half off
        b = half (off + 2)
        w = a .|. shiftL b 16
        aligned = pc .&. 3 == 0
        cond = shiftR w 28
        armValid = cond /= 15
        armMove op = aligned && armValid && w .&. 0x0ff00000 == op && armReg /= Register 15
        armReg = Register (shiftR w 12 .&. 15)
        armImm = shiftL (shiftR w 16 .&. 15) 12 .|. (w .&. 0xfff)
        thumbMove op = a .&. 0xfbf0 == op && b .&. 0x8000 == 0 && thumbReg /= Register 15
        thumbReg = Register (shiftR b 8 .&. 15)
        thumbImm = shiftL (a .&. 15) 12 .|. shiftL (shiftR a 10 .&. 1) 11 .|. shiftL (shiftR b 12 .&. 7) 8 .|. (b .&. 255)
        armNext = if armMove 0x03000000 then M.insert armReg (MoveLow armImm pc) armLows else armLows
        thumbNext = if thumbMove 0xf240 then M.insert thumbReg (MoveLow thumbImm pc) thumbLows else thumbLows
        short = if a .&. 0xf800 == 0xa000 then pointer ThumbADR pc (((pc + 4) .&. complement 3) + shiftL (a .&. 255) 2) else []
        s = shiftR a 10 .&. 1
        j1 = shiftR b 13 .&. 1
        j2 = shiftR b 11 .&. 1
        displacement = shiftL s 24 .|. shiftL (1 `xor` j1 `xor` s) 23 .|. shiftL (1 `xor` j2 `xor` s) 22 .|. shiftL (a .&. 0x3ff) 12 .|. shiftL (b .&. 0x7ff) 1
        branchTarget = pc + 4 + signExtend 25 displacement
        wideBase = a .&. 0xf800 == 0xf000
        armOffset = shiftL (signExtend 24 (w .&. 0xffffff)) 2
        adrImmediate = rotateR (w .&. 255) (fromIntegral (shiftR w 8 .&. 15) * 2)
        adrBase = (pc + 4) .&. complement 3
        wideImm = shiftL (shiftR a 10 .&. 1) 11 .|. shiftL (shiftR b 12 .&. 7) 8 .|. (b .&. 255)
        refs = concat
          [ pointer (if aligned then PointerWord else PointerHalfword) pc w
          , if aligned && w .&. 0x0e000000 == 0x0a000000
              then hit (if cond == 15 then ArmBLX else if testBit w 24 then ArmBL else ArmB) pc
                (pc + 8 + armOffset + if cond == 15 && testBit w 24 then 2 else 0) else []
          , if wideBase && b .&. 0xd000 == 0xd000 then hit ThumbBL pc branchTarget else []
          , if wideBase && b .&. 0xd001 == 0xc000 then hit ThumbBLX pc (adrBase + signExtend 25 displacement) else []
          , if wideBase && b .&. 0xd000 == 0x9000 then hit ThumbBW pc branchTarget else []
          , if wideBase && b .&. 0xd000 == 0x8000 && shiftR a 6 .&. 15 < 14
              then hit ThumbConditionalBW pc (pc + 4 + signExtend 21 (shiftL s 20 .|. shiftL j2 19 .|. shiftL j1 18 .|. shiftL (a .&. 63) 12 .|. shiftL (b .&. 0x7ff) 1)) else []
          , if armMove 0x03400000 then paired ArmMovPair pc armReg armImm armLows else []
          , if thumbMove 0xf2c0 then paired ThumbMovPair pc thumbReg thumbImm thumbLows else []
          , if aligned && armValid && w .&. 0x0fff0000 `elem` [0x028f0000,0x024f0000] && armReg /= Register 15
              then pointer ArmADR pc (if testBit w 23 then pc + 8 + adrImmediate else pc + 8 - adrImmediate) else []
          , if a .&. 0xfbff `elem` [0xf20f,0xf2af] && b .&. 0x8000 == 0 && thumbReg /= Register 15
              then pointer ThumbADR pc (if testBit a 7 then adrBase - wideImm else adrBase + wideImm) else []
          ]

signExtend :: Int -> Word32 -> Word32
signExtend width value = if testBit value (width - 1) then value .|. complement (shiftL 1 width - 1) else value

renderReference :: Reference -> String
renderReference (Reference _ kind name (Address site)) = label kind ++ "@" ++ name ++ ":" ++ showHex site ""
  where
    label PointerWord = "word"
    label PointerHalfword = "word2"
    label ArmB = "b"
    label ArmBL = "bl"
    label ArmBLX = "blx"
    label ThumbBL = "thumb-bl"
    label ThumbBLX = "thumb-blx"
    label ThumbBW = "thumb-b.w"
    label ThumbConditionalBW = "thumb-bcond.w"
    label ArmMovPair = "movw-movt"
    label ThumbMovPair = "thumb-movw-movt"
    label ArmADR = "adr"
    label ThumbADR = "thumb-adr"
