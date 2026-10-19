-- | A reader for the little-endian ARM ELF32 executables the RAM-push
-- relinker links: section and program headers, the symbol table and section
-- bytes by address.
module ArmElf
  ( Elf (..)
  , Section (..)
  , Segment (..)
  , Symbol (..)
  , SymbolKind (..)
  , SymbolSection (..)
  , parseElf
  , sectionWritable
  , sectionExecutable
  , sectionAllocated
  , segmentWritable
  , segmentExecutable
  , loadSegment
  , wordAt
  , byteAt
  ) where

import Control.Monad (forM, unless, when)
import Data.Bits (shiftL, testBit, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word16, Word32, Word8)

data Elf = Elf
  { elfEntry :: Word32
  , elfSections :: [Section]
  , elfSegments :: [Segment]
  , elfSymbols :: [Symbol]
  , elfBytes :: B.ByteString
  }

data Section = Section
  { sectionName :: String
  , sectionType :: Word32
  , sectionFlags :: Word32
  , sectionAddress :: Word32
  , sectionOffset :: Word32
  , sectionSize :: Word32
  }
  deriving (Eq, Show)

data Segment = Segment
  { segmentType :: Word32
  , segmentOffset :: Word32
  , segmentAddress :: Word32
  , segmentFileSize :: Word32
  , segmentMemorySize :: Word32
  , segmentFlags :: Word32
  }
  deriving (Eq, Show)

data SymbolKind = FunctionSymbol | ObjectSymbol | OtherSymbol
  deriving (Eq, Show)

data SymbolSection = Undefined | Absolute | InSection Int
  deriving (Eq, Show)

data Symbol = Symbol
  { symbolName :: String
  , symbolValue :: Word32
  , symbolSize :: Word32
  , symbolKind :: SymbolKind
  , symbolSection :: SymbolSection
  }
  deriving (Eq, Show)

sectionAllocated, sectionWritable, sectionExecutable :: Section -> Bool
sectionAllocated section = testBit (sectionFlags section) 1
sectionWritable section = testBit (sectionFlags section) 0
sectionExecutable section = testBit (sectionFlags section) 2

loadSegment :: Segment -> Bool
loadSegment segment = segmentType segment == 1

segmentWritable, segmentExecutable :: Segment -> Bool
segmentWritable segment = testBit (segmentFlags segment) 1
segmentExecutable segment = testBit (segmentFlags segment) 0

parseElf :: B.ByteString -> Either String Elf
parseElf bytes = do
  unless (B.take 4 bytes == B.pack [0x7f, 0x45, 0x4c, 0x46]) (Left "ELF magic required")
  unless (B.length bytes >= 52) (Left "ELF header is truncated")
  unless (byte 4 == 1 && byte 5 == 1) (Left "little-endian ELF32 required")
  unless (half 0x10 == 2) (Left "ELF executable (ET_EXEC) required")
  unless (half 0x12 == 40) (Left "ARM ELF (EM_ARM) required")
  let phoff = word 0x1c
      shoff = word 0x20
      phentsize = half 0x2a
      phnum = half 0x2c
      shentsize = half 0x2e
      shnum = half 0x30
      shstrndx = half 0x32
  unless (shentsize == 40 && phentsize == 32) (Left "ELF32 header entry sizes required")
  sectionRaw <- forM [0 .. fromIntegral shnum - 1] $ \index ->
    readSection (fromIntegral shoff + index * 40)
  segments <- forM [0 .. fromIntegral phnum - 1] $ \index ->
    readSegment (fromIntegral phoff + index * 32)
  when (fromIntegral shstrndx >= length sectionRaw) (Left "section name table index is out of range")
  let (_, _, shstr) = sectionRaw !! fromIntegral shstrndx
  sections <- forM sectionRaw $ \(nameOffset, _, section) -> do
    name <- stringAt shstr nameOffset
    pure section {sectionName = name}
  symbols <- case [(link, section) | ((_, link, _), section) <- zip sectionRaw sections, sectionType section == 2] of
    [(link, table)] -> do
      when (fromIntegral link >= length sections) (Left "symbol string table index is out of range")
      readSymbols table (sections !! fromIntegral link)
    [] -> Left "symbol table required"
    _ -> Left "exactly one symbol table required"
  pure (Elf (word 0x18) sections segments symbols bytes)
 where
  byte :: Int -> Word8
  byte offset = B.index bytes offset
  half :: Int -> Word16
  half offset = fromIntegral (byte offset) .|. (fromIntegral (byte (offset + 1)) `shiftL` 8)
  word :: Int -> Word32
  word offset = sum [fromIntegral (byte (offset + index)) `shiftL` (8 * index) | index <- [0 .. 3]]
  inBounds offset size = offset >= 0 && size >= 0 && offset + size <= B.length bytes
  -- (name offset, sh_link, header)
  readSection offset = do
    unless (inBounds offset 40) (Left "section header is outside the file")
    let section = Section "" (word (offset + 4)) (word (offset + 8)) (word (offset + 12)) (word (offset + 16)) (word (offset + 20))
    when (sectionType section /= 8 && sectionType section /= 0 && not (inBounds (fromIntegral (sectionOffset section)) (fromIntegral (sectionSize section)))) $
      Left "section contents are outside the file"
    pure (word offset, word (offset + 24), section)
  readSegment offset = do
    unless (inBounds offset 32) (Left "program header is outside the file")
    pure (Segment (word offset) (word (offset + 4)) (word (offset + 8)) (word (offset + 16)) (word (offset + 20)) (word (offset + 24)))
  stringAt table offset = do
    let start = fromIntegral (sectionOffset table) + fromIntegral offset
        end = fromIntegral (sectionOffset table) + fromIntegral (sectionSize table)
    unless (start < end) (Left "string offset is outside its table")
    let name = B.takeWhile (/= 0) (B.take (end - start) (B.drop start bytes))
    unless (B.length name < end - start) (Left "unterminated ELF string")
    pure (C.unpack name)
  readSymbols table strings = do
    let count = fromIntegral (sectionSize table) `div` 16
    forM [0 .. count - 1] $ \index -> do
      let offset = fromIntegral (sectionOffset table) + index * 16
      name <- stringAt strings (word offset)
      let info = byte (offset + 12)
          shndx = half (offset + 14)
          kind = case info .&. 15 of
            2 -> FunctionSymbol
            1 -> ObjectSymbol
            _ -> OtherSymbol
          place
            | shndx == 0 = Undefined
            | shndx == 0xfff1 = Absolute
            | otherwise = InSection (fromIntegral shndx)
      pure (Symbol name (word (offset + 4)) (word (offset + 8)) kind place)

-- | The little-endian word at an address inside an allocated PROGBITS section.
wordAt :: Elf -> Word32 -> Maybe Word32
wordAt elf address = do
  bytes <- mapM (byteAt elf) [address, address + 1, address + 2, address + 3]
  pure (sum [fromIntegral value `shiftL` (8 * index) | (index, value) <- zip [0 :: Int ..] bytes])

byteAt :: Elf -> Word32 -> Maybe Word8
byteAt elf address =
  case [ section
       | section <- elfSections elf
       , sectionAllocated section
       , sectionType section == 1
       , toInteger (sectionAddress section) <= toInteger address
       , toInteger address < toInteger (sectionAddress section) + toInteger (sectionSize section)
       ] of
    [section] -> Just (B.index (elfBytes elf) (fromIntegral (sectionOffset section + address - sectionAddress section)))
    _ -> Nothing
