-- | The Classic Chrome RAM-push publications over the RC0-installed X-M1 1.50
-- images (reports/classic-chrome-patch.md section 8.2): the flash
-- candidate's data words, and one branch word per rewritten code range into
-- menu/classic_chrome_ram.S linked at the allocation base with the generic
-- installer and no recipe-bank object. No I/O.
module FujiAF.ClassicChromeRam
  ( PublicationPhase (..)
  , CcPublication (..)
  , Rc0Images (..)
  , FlashImages (..)
  , RamSymbols (..)
  , ramSymbolNames
  , ramSymbols
  , ccRamPublications
  , encodeBranch
  , ccPublicationCount
  , ccSetupCount
  , ccActivationCount
  , ccAbiVersion
  , ccAllocationSize
  , ccImageExtent
  , ccInstallerExtent
  , ccInstallerStart
  , ccDataBlockSymbol
  , overlayDataBlock
  , bankSymbols
  , foreignInputObjects
  ) where

import Control.Monad (forM, forM_, unless, when)
import Data.Bits ((.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (isPrefixOf, isSuffixOf, nub, sort)
import Data.Word (Word32)
import FujiAF.ClassicChrome
import Numeric (showHex)

data PublicationPhase = Setup | Activation deriving (Eq, Show)

data CcPublication = CcPublication
  { publicationPhase :: PublicationPhase
  , publicationAddress :: Address
  , publicationOld :: Word32
  , publicationNew :: Word32
  , publicationRole :: String
  }
  deriving (Eq, Show)

-- | Decodes as the RC0 body holds them: image 4 at 0x0061c340, image 5 at
-- 0x00bf1a00, plain at 0x01100000, image 2 at 0x00130000.
data Rc0Images = Rc0Images
  { rc0Image4 :: B.ByteString
  , rc0Image5 :: B.ByteString
  , rc0Plain :: B.ByteString
  , rc0Image2 :: B.ByteString
  }

-- | The flash candidate's decodes: image 4 at 0x0061c200, image 5 and plain
-- at their stock bases.
data FlashImages = FlashImages
  { flashImage4 :: B.ByteString
  , flashImage5 :: B.ByteString
  , flashPlain :: B.ByteString
  }

-- | Link addresses of menu/classic_chrome_ram.S's entries.
data RamSymbols = RamSymbols
  { symbolConvert :: Address
  , symbolMarker :: Address
  , symbolFilmMode :: Address
  , symbolSaturation :: Address
  , symbolRawCommit :: Address
  , symbolRawHighlight :: Address
  , symbolRawRequest :: Address
  , symbolColour :: Address
  , symbolChroma :: Address
  , symbolTone :: Address
  }

ramSymbolNames :: [String]
ramSymbolNames =
  [ "cc_ram_convert", "cc_ram_marker", "cc_ram_filmmode", "cc_ram_saturation", "cc_ram_raw_commit"
  , "cc_ram_raw_highlight", "cc_ram_raw_request", "cc_ram_colour", "cc_ram_chroma", "cc_ram_tone"
  ]

-- | From a link's symbol table; each name must be defined once.
ramSymbols :: [(String, Address)] -> Either String RamSymbols
ramSymbols table = do
  found <- forM ramSymbolNames $ \name -> case [a | (n, a) <- table, n == name] of
    [a] -> Right a
    _ -> Left ("link symbols: unique definition required for " ++ name)
  case found of
    [c, m, f, s, rc, rh, rr, col, chr, t] -> Right (RamSymbols c m f s rc rh rr col chr t)
    _ -> Left "link symbols: symbol list mismatch"

ccDataBlockSymbol :: String
ccDataBlockSymbol = "cc_ram_data"

ccPublicationCount, ccSetupCount, ccActivationCount :: Int
ccPublicationCount = 131
ccSetupCount = 84
ccActivationCount = 47

-- | The mailbox ABI word of a runtime built for this payload.
ccAbiVersion :: Word32
ccAbiVersion = 3

-- | menu/classic_chrome_ram.S's extent from the base; the installer starts
-- at the next 32-byte boundary.
ccImageExtent :: Word32
ccImageExtent = 0x184

ccInstallerStart :: Word32 -> Word32
ccInstallerStart base = (base + ccImageExtent + 31) .&. 0xffffffe0

-- | From the installer start, with the 131-entry publication table.
ccInstallerExtent :: Word32
ccInstallerExtent = 0x13c0

-- | The runtime extent at a 32-byte-aligned base, the largest over bases.
ccAllocationSize :: Word32
ccAllocationSize = ccInstallerStart 0 + ccInstallerExtent

-- | Names of a link's symbol table that belong to the recipe bank.
bankSymbols :: [String] -> [String]
bankSymbols = filter (\name -> any (`isPrefixOf` name) ["rb_", "recipe_bank", "__rb_"])

-- | Input objects an lld link map places bytes from, other than the
-- Classic Chrome object and the installer.
foreignInputObjects :: String -> [String]
foreignInputObjects linkMap =
  nub
    [ object
    | line <- lines linkMap
    , token <- words line
    , (path, ':' : '(' : _) <- [break (== ':') token]
    , ".o" `isSuffixOf` path
    , let object = reverse (takeWhile (/= '/') (reverse path))
    , object `notElem` ["classic_chrome_ram.o", "ram_push_installer.o"]
    ]

rc0Image4Base, flashImage4Base, image4End, image2Base, windowEnd :: Word32
rc0Image4Base = 0x0061c340
flashImage4Base = 0x0061c200
image4End = 0x0089ec00
image2Base = 0x00130000
windowEnd = 0x0061c400

-- | A32 B, condition AL.
encodeBranch :: Address -> Address -> Either String Word32
encodeBranch site target = (\word -> (word .&. 0x00ffffff) .|. 0xea000000) <$> encodeBl site target

-- | Stock entry words that become a branch into the allocation, in
-- activation order, with the symbol each targets.
entrySites :: RamSymbols -> [(Word32, Word32, Address, String)]
entrySites s =
  [ (0x00738fe8, 0xe52db004, symbolConvert s, "canonical-to-capture converter FUN_00738fe8 entry")
  , (0x00736a74, 0xe52db004, symbolConvert s, "live-to-settings converter FUN_00736a74 entry")
  , (0x00d7851c, 0xe52db004, symbolMarker s, "+0x110 marker setter FUN_00d7851c entry")
  , (0x00eab2e0, 0xea00000b, symbolFilmMode s, "FilmMode FUN_00eab258 default arm")
  , (0x00eaac88, 0xea00004e, symbolSaturation s, "Saturation FUN_00eaac74 default arm")
  , (0x00d461ec, 0xe5832008, symbolRawCommit s, "RAW commit FUN_00d46010 colour store")
  , (0x00d5ada4, 0xe3530000, symbolRawHighlight s, "RAW highlight FUN_00d588b8 colour test")
  , (0x012548a4, 0xe2433003, symbolRawRequest s, "RAW request FUN_012546a8 film field")
  ]

hookTargets :: RamSymbols -> [(HookSite, Address)]
hookTargets s = zip hookSites [symbolColour s, symbolChroma s, symbolChroma s, symbolTone s, symbolTone s]

shootingValuesWords :: [(Word32, Word32, String)]
shootingValuesWords =
  [ (0x0061c380, 0x7ffffffe, "shooting values terminator moves past row 2")
  , (0x0061c384, 0, "shooting values terminator second word")
  , (0x0061c378, 2, "shooting values row 2 (CLASSIC CHROME) replaces the RC0 terminator")
  ]

-- | RC0's labels terminator words past the RAW labels; the flash window
-- zero-fills them and the RAM keeps RC0's.
retainedTail :: [Word32]
retainedTail = [0x0061c3f8, 0x0061c3fc]

converterExtents :: [(Word32, Word32)]
converterExtents = [(a, a + fromIntegral n) | (Address a, n) <- [captureConverter, settingsConverter]]

-- | The ordered publications, after the flash decodes' differences from the
-- RC0 decodes are refused unless they have the pinned shape.
ccRamPublications :: Rc0Images -> FlashImages -> RamSymbols -> Either String [CcPublication]
ccRamPublications rc0 flash symbols = do
  let image4Rc0 a = if a < rc0Image4Base then 0 else wordAt (rc0Image4 rc0) rc0Image4Base a
      image4Flash = wordAt (flashImage4 flash) flashImage4Base
      image5Rc0 = wordAt (rc0Image5 rc0) image5Base
      image5Flash = wordAt (flashImage5 flash) image5Base
      plainRc0 = wordAt (rc0Plain rc0) plainBase
      plainFlash = wordAt (flashPlain flash) plainBase
      Address image5Base = image5DecodeBase
      Address plainBase = plainDecodeBase
      differing old new lo hi = [a | a <- [lo, lo + 4 .. hi - 4], old a /= new a]
      within ranges a = any (\(lo, hi) -> lo <= a && a < hi) ranges
      addresses = map (\w -> let Address a = writeAddress w in a)
      changing = filter (\w -> writeCurrent w /= writeNew w)
      inRegion content a = let (Address lo, n) = (regionAddress content, regionSize content) in lo <= a && a < lo + fromIntegral n
  unless (rc0Image4Base + len (rc0Image4 rc0) == image4End && flashImage4Base + len (flashImage4 flash) == image4End) $
    Left "image 4 decodes do not end at the bss start 0x0089ec00"
  unless (B.length (rc0Image5 rc0) == B.length (flashImage5 flash) && B.length (rc0Plain rc0) == B.length (flashPlain flash)) $
    Left "image 5 or plain decodes differ in length"

  let image4Diff = differing image4Rc0 image4Flash flashImage4Base image4End
      windowDiff = filter (< windowEnd) image4Diff
      bodyDiff = filter (>= windowEnd) image4Diff
      bodyData = addresses (conversionRowWords ++ availabilityRowWords ++ rc0LabelPointers ++ [rawNodeLabelsWord, rawNodeValuesWord])
      published content = filter (inRegion content) windowDiff
      shootingValuesDiff = published ShootingValues
      unexplainedWindow =
        [ a | a <- windowDiff
        , not (any (`inRegion` a) [DataBlock, RawValues, ShootingLabels, ShootingValues, RawLabels])
        , a `notElem` retainedTail
        ]
  unless (null unexplainedWindow) $
    Left ("image 4 window differs outside its arrays at " ++ hexList unexplainedWindow)
  unless (sort shootingValuesDiff == sort [a | (a, _, _) <- shootingValuesWords] && and [image4Flash a == v | (a, v, _) <- shootingValuesWords]) $
    Left "shooting values differ from RC0's seven rows plus row 2 and the moved terminator"
  unless (and [image4Rc0 a == 0xffffffff && image4Flash a == 0 | a <- retainedTail]) $
    Left "the window tail past the RAW labels is not RC0's terminator bytes over the flash zero fill"
  unless (sort (filter (not . within converterExtents) bodyDiff) == sort bodyData) $
    Left "image 4 body differs from the pinned rows, availability cells and pointers"
  unless (all (`elem` bodyDiff) (map fst converterExtents)) $
    Left "the flash candidate leaves a converter entry stock"

  let image5Diff = differing image5Rc0 image5Flash image5Base (image5Base + len (rc0Image5 rc0))
      image5Expected = addresses (changing image5FixedWords) ++ [a | HookSite (Address a) _ _ _ _ <- hookSites]
  unless (sort image5Diff == sort image5Expected) $
    Left "image 5 differs from the pinned code ranges and hook sites"
  let plainDiff = differing plainRc0 plainFlash plainBase (plainBase + len (rc0Plain rc0))
  unless (sort plainDiff == sort (addresses (changing (plainGateWords ++ plainRawWords)))) $
    Left "plain differs from the pinned gates and RAW request block"

  let Address recordAt = image2RecordAddress
      recordEnd = recordAt + fromIntegral (B.length image2Record)
      recordWordsEnd = (recordEnd + 3) .&. complement3
      complement3 = 0xfffffffc
      image2Old = wordAt (rc0Image2 rc0) image2Base
      recordBytes = B.take (B.length image2Record) (B.drop (fromIntegral (recordAt - image2Base)) (rc0Image2 rc0))
      image2Patched = B.take (fromIntegral (recordAt - image2Base)) (rc0Image2 rc0) <> image2Record
        <> B.drop (fromIntegral (recordEnd - image2Base)) (rc0Image2 rc0)
      image2New = wordAt image2Patched image2Base
  unless (recordAt >= image2Base && recordWordsEnd <= image2Base + len (rc0Image2 rc0)) $
    Left "image 2 decode does not hold the 0x83e record"
  unless (recordBytes == B.replicate (B.length image2Record) 0) $
    Left "RC0 image-2 record 0x83e is not 66 zero bytes"

  let word phase old new role a = CcPublication phase (Address a) (old a) (new a) role
      image4Data phase role a = word phase image4Rc0 image4Flash role a
      setup =
        map (image4Data Setup "RAW conversion values") (published RawValues)
          ++ map (image4Data Setup "shooting labels") (published ShootingLabels)
          ++ map (word Setup image2Old image2New "image-2 record 0x83e CLASSIC CHROME")
               (differing image2Old image2New recordAt recordWordsEnd)
          ++ [CcPublication Setup a (plainRc0 x) (plainFlash x) (writeRole w) | w <- plainGateWords, let a@(Address x) = writeAddress w]
          ++ [CcPublication Setup a (image4Rc0 x) (image4Flash x) (writeRole w) | w <- conversionRowWords ++ availabilityRowWords, let a@(Address x) = writeAddress w]
  entries <- forM (entrySites symbols) $ \(site, stock, target, role) -> do
    let old = if site >= plainBase then plainRc0 site else if site >= image5Base then image5Rc0 site else image4Rc0 site
    when (old /= stock) $ Left ("RC0 entry word at " ++ hex site ++ " is not the stock " ++ hex stock)
    new <- either (Left . ((hex site ++ ": ") ++)) Right (encodeBranch (Address site) target)
    pure (CcPublication Activation (Address site) old new role)
  hooks <- forM (hookTargets symbols) $ \(HookSite site@(Address a) stock _ _ role, target) -> do
    when (image5Rc0 a /= stock) $ Left ("RC0 hook word at " ++ hex a ++ " is not the stock " ++ hex stock)
    new <- either (Left . ((hex a ++ ": ") ++)) Right (encodeBl site target)
    pure (CcPublication Activation site stock new ("bl replaces the " ++ role))
  let labelPointers =
        [ CcPublication Activation a (image4Rc0 x) (image4Flash x) (writeRole w)
        | w <- rc0LabelPointers, let a@(Address x) = writeAddress w ]
      valuesWords = [CcPublication Activation (Address a) (image4Rc0 a) v role | (a, v, role) <- shootingValuesWords]
      rawLabels = map (image4Data Activation "RAW conversion labels") (published RawLabels)
      rawNodes = [CcPublication Activation a (image4Rc0 x) (image4Flash x) (writeRole w) | w <- [rawNodeLabelsWord, rawNodeValuesWord], let a@(Address x) = writeAddress w]
      activation = entries ++ hooks ++ labelPointers ++ valuesWords ++ rawLabels ++ rawNodes
      ordered = setup ++ activation
  forM_ labelPointers $ \p -> unless (publicationOld p == rc0LabelsDestinationStock && publicationNew p == rc0LabelsDestination) $
    Left ("labels pointer at " ++ hexAddress (publicationAddress p) ++ " is not RC0's 0x0061c380 to 0x0061c2b0")
  forM_ (plainGateWords ++ conversionRowWords ++ availabilityRowWords ++ [rawNodeLabelsWord, rawNodeValuesWord]) $ \w ->
    case [p | p <- ordered, publicationAddress p == writeAddress w] of
      [p] -> unless (publicationOld p == writeCurrent w && publicationNew p == writeNew w) $
        Left ("word at " ++ hexAddress (writeAddress w) ++ " differs from the flash design's current and new words")
      _ -> Left ("word at " ++ hexAddress (writeAddress w) ++ " is not published once")
  unless (length setup == ccSetupCount && length activation == ccActivationCount) $
    Left ("publication counts " ++ show (length setup) ++ " setup and " ++ show (length activation) ++ " activation differ from the pinned "
      ++ show ccSetupCount ++ " and " ++ show ccActivationCount)
  unless (length (nub (map publicationAddress ordered)) == length ordered && all (\p -> publicationOld p /= publicationNew p) ordered) $
    Left "publications must be distinct changed words"
  pure ordered

-- | Writes the parameter set's block over the runtime at `blockOffset`,
-- whose linked bytes are the magic followed by zeros.
overlayDataBlock :: Int -> B.ByteString -> B.ByteString -> Either String B.ByteString
overlayDataBlock blockOffset block runtime = do
  let linkedBlock = B.take dataBlockSize (B.drop blockOffset runtime)
  unless (B.length block == dataBlockSize && B.take 4 block == B.take 4 linkedBlock) $
    Left "data block length or magic differs from the linked block"
  unless (blockOffset >= 0 && blockOffset + dataBlockSize <= B.length runtime && B.drop 4 linkedBlock == B.replicate (dataBlockSize - 4) 0) $
    Left "linked data block is not the magic followed by zeros inside the runtime"
  pure (B.take blockOffset runtime <> block <> B.drop (blockOffset + dataBlockSize) runtime)

wordAt :: B.ByteString -> Word32 -> Word32 -> Word32
wordAt bytes base address =
  let offset = fromIntegral (address - base)
   in if address < base || offset + 4 > B.length bytes
        then error ("address " ++ hex address ++ " lies outside the image at " ++ hex base)
        else sum [fromIntegral (B.index bytes (offset + i)) * 256 ^ i | i <- [0 .. 3]]

len :: B.ByteString -> Word32
len = fromIntegral . B.length

hex :: Word32 -> String
hex w = let digits = showHex w "" in "0x" ++ replicate (8 - length digits) '0' ++ digits

hexAddress :: Address -> String
hexAddress (Address a) = hex a

hexList :: [Word32] -> String
hexList = unwords . map hex . take 8
