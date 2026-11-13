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
  , AddressRelocation (..)
  , AddressReferenceKind (..)
  , addressRelocations
  , directBranchTarget
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
import Data.Bits (shiftL, shiftR, testBit, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Int (Int32)
import Data.List (sortOn)
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

data AddressReferenceKind = DataReference | DirectReference deriving (Eq, Show)

data AddressRelocation = AddressRelocation
  { addressSite :: Word32
  , addressTarget :: Word32
  , addressKind :: AddressReferenceKind
  } deriving (Eq, Show)

-- | Resolved references retained by --emit-relocs, including relocated MOVW/MOVT pairs.
addressRelocations :: Elf -> Either String [AddressRelocation]
addressRelocations elf = concat <$> mapM readTable tables
 where
  bytes = elfBytes elf
  tables = [(index, section) | (index, section) <- zip [0 :: Int ..] (elfSections elf), sectionType section `elem` [4, 9]]
  fileWord :: Int -> Either String Word32
  fileWord offset = do
    unless (offset >= 0 && offset + 4 <= B.length bytes) (Left "ELF relocation field outside file")
    pure (sum [fromIntegral (B.index bytes (offset + index)) `shiftL` (8 * index) | index <- [0 .. 3]])
  indexed label values index =
    if toInteger index < toInteger (length values) then pure (values !! fromIntegral index) else Left (label ++ " index outside ELF table")
  readTable (index, section) = do
    shoff <- fileWord 0x20
    let header = fromIntegral shoff + index * 40
    link <- fileWord (header + 24)
    info <- fileWord (header + 28)
    entrySize <- fileWord (header + 36)
    symbols <- indexed "relocation symbol table" (elfSections elf) link
    target <- indexed "relocation target section" (elfSections elf) info
    if not (sectionAllocated target) then pure [] else do
      unless (sectionType section == 9 && sectionType symbols == 2 && entrySize == 8 && sectionSize section `mod` 8 == 0
        && sectionType target == 1) (Left "allocated ARM ELF32 REL layout required")
      records <- forM [0 .. fromIntegral (sectionSize section) `div` 8 - 1] $ \record -> do
        let offset = fromIntegral (sectionOffset section) + record * 8
        site <- fileWord offset
        encoding <- fileWord (offset + 4)
        unless (site .&. 3 == 0 && sectionAddress target <= site && toInteger site + 4 <= toInteger (sectionAddress target) + toInteger (sectionSize target))
          (Left "ARM relocation site outside its section or unaligned")
        symbol <- indexed "relocation symbol" (elfSymbols elf) (encoding `shiftR` 8)
        unless (symbolSection symbol /= Undefined) (Left "ARM relocation references undefined symbol")
        content <- maybe (Left "ARM relocation word outside allocated bytes") pure (wordAt elf site)
        pure (site, encoding .&. 0xff, encoding `shiftR` 8, content)
      forM [(site, kind, symbol, content) | (site, kind, symbol, content) <- records, kind /= 0] $ \(site, kind, symbol, content) -> do
        direct <- case kind of
          1 -> branchTarget site content
          28 -> branchTarget site content
          29 -> branchTarget site content
          _ -> pure 0
        let immediate word = ((word `shiftR` 4) .&. 0xf000) .|. (word .&. 0xfff)
            register word = (word `shiftR` 12) .&. 15
            pair other = sortOn fst [(at, instruction) | (at, code, identity, instruction) <- records
              , code == other, identity == symbol, register instruction == register content]
            paired other = case if other == 44 then filter ((> site) . fst) (pair other) else reverse (filter ((< site) . fst) (pair other)) of
              (_, instruction) : _ -> Right instruction
              [] -> Left "unpaired ARM address relocation"
        case kind of
          2 -> pure (AddressRelocation site content DataReference)
          3 -> pure (AddressRelocation site (site + content) DataReference)
          1 -> pure (AddressRelocation site direct DirectReference)
          28 -> pure (AddressRelocation site direct DirectReference)
          29 -> pure (AddressRelocation site direct DirectReference)
          43 -> do
            upper <- paired 44
            pure (AddressRelocation site ((immediate upper `shiftL` 16) .|. immediate content) DataReference)
          44 -> do
            lower <- paired 43
            pure (AddressRelocation site ((immediate content `shiftL` 16) .|. immediate lower) DataReference)
          _ -> Left ("unsupported ARM address relocation: " ++ show kind)
  branchTarget site content = maybe (Left "ARM branch relocation does not contain a direct branch") Right (directBranchTarget site content)

directBranchTarget :: Word32 -> Word32 -> Maybe Word32
directBranchTarget address word
  | word .&. 0x0e000000 /= 0x0a000000 = Nothing
  | otherwise = Just (if thumb then (target + halfword) .|. 1 else target)
 where
  displacement = (fromIntegral ((word .&. 0x00ffffff) `shiftL` 8) :: Int32) `shiftR` 6
  target = address + 8 + fromIntegral displacement
  thumb = word `shiftR` 28 == 15
  halfword = if testBit word 24 then 2 else 0
