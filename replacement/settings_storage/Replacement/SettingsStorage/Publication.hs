{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Replacement.SettingsStorage.Publication
  ( PublicationPlan, Region(..), Stage4Contract(..), parseStage4Contract
  , planPublication, publicationLinker, stagePublication, publicationReferences, planValue
  , publicationValue, publicationStackEntries, publicationRegions, publishedRegions
  ) where

import Control.Monad (forM_, unless, foldM)
import Data.Aeson (FromJSON(..), eitherDecodeStrict', object, withObject, (.:), (.=), Value)
import Data.Char (isAlphaNum, isAscii)
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (sortOn)
import Data.Maybe (catMaybes)
import Data.Ord (Down(..))
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import Numeric (readHex, showHex)
import Text.Read (readMaybe)
import Replacement.PropertyService.Placement hiding (stage4Carrier)
import qualified Replacement.RecordStore.References as R
import qualified Replacement.RecordStore.Carrier as RecordCarrier
import ServiceInventory (Image(..))
import qualified CustomAdmission as Admission

newtype Symbols = Symbols (M.Map String Word32)
data Region = Region { regionStart :: Word32, regionEnd :: Word32 } deriving (Eq, Show)
newtype SectionName = SectionName B.ByteString deriving (Eq, Ord, Show)
newtype SectionBytes = SectionBytes Word32 deriving (Eq, Ord, Show)
newtype InputAlignment = InputAlignment Word32 deriving (Eq, Ord, Show)
newtype SegmentIndex = SegmentIndex Int
data SegmentEndpoint = SegmentStart | SegmentEnd
data InputSelection = TextSection SectionName | ReadOnlySections deriving (Eq, Ord, Show)
data MeasuredInput = MeasuredInput GuestAddress SectionName SectionBytes InputAlignment deriving (Eq, Show)
data InputAtom = InputAtom InputSelection SectionBytes InputAlignment deriving (Eq, Show)
data PublicationSegment = PublicationSegment Region GuestAddress SectionBytes [InputAtom] deriving (Eq, Show)
data PublicationPlan = PublicationPlan [PublicationSegment] SectionBytes deriving (Eq, Show)
data Stage4Contract = Stage4Contract
  { stage4SourceHash :: String, stage4ImageHash :: String, stage4Carrier :: Region
  , stage4Hull :: Region, stage4Retained :: [Region]
  , stage4Entries :: [(String,Word32)]
  } deriving (Eq, Show)
instance FromJSON Region where
  parseJSON = withObject "region" $ \o -> Region <$> o .: "start" <*> o .: "end"
instance FromJSON Stage4Contract where
  parseJSON = withObject "stage4 contract" $ \o -> do
    source <- o .: "source_sha256" >>= withObject "source identities" (.: "dat")
    stack <- o .: "stack"
    entries <- stack .: "entries" >>= mapM (withObject "property entry" $ \entry -> (,) <$> entry .: "name" <*> entry .: "entry")
    Stage4Contract source <$> o .: "staged_image4_sha256" <*> o .: "carrier" <*> o .: "hull" <*> o .: "retained_regions" <*> pure entries
parseStage4Contract :: B.ByteString -> Either String Stage4Contract
parseStage4Contract = eitherDecodeStrict'

approved :: [Region]
approved = [Region 0x627578 0x629eac, Region 0x61e114 0x61ffe0, Region 0x629eb4 0x62b900]
parseSymbols :: B.ByteString -> Either String Symbols
parseSymbols bytes = Symbols . M.fromList <$> mapM parse [fields | line <- C.lines bytes, let fields = C.words line, not (null fields)]
  where
    parse [address,_,_,name] = case readHex (C.unpack address) of
      [(number,"")] -> Right (C.unpack name,number)
      _ -> Left "invalid symbol address"
    parse _ = Left "expected llvm-nm --print-size symbols"
symbol :: Symbols -> String -> Either String Word32
symbol (Symbols values) name = maybe (Left ("missing symbol " ++ name)) Right (M.lookup name values)

readWord :: B.ByteString -> Either String Word32
readWord encoded = case readHex (C.unpack encoded) :: [(Integer,String)] of
  [(value,"")] | value >= 0 && value <= 0xffffffff -> Right (fromInteger value)
  _ -> Left "invalid linker-map hexadecimal word"

aligned :: GuestAddress -> InputAlignment -> GuestAddress
aligned (GuestAddress address) (InputAlignment alignment) = GuestAddress ((address + alignment - 1) .&. complementMask)
  where complementMask = 0xffffffff - (alignment - 1)

parseMeasuredInputs :: GuestAddress -> B.ByteString -> Either String [MeasuredInput]
parseMeasuredInputs (GuestAddress end) bytes = do
  case C.lines bytes of
    header:_ | C.words header == ["VMA","LMA","Size","Align","Out","In","Symbol"] -> pure ()
    _ -> Left "expected an LLD input-section map"
  measured <- catMaybes <$> mapM parse (drop 1 (C.lines bytes))
  let ordered = sortOn (\(MeasuredInput address _ _ _) -> address) measured
  finish <- foldM contiguous (GuestAddress 0x5d0000) ordered
  unless (aligned finish (InputAlignment 4) == GuestAddress end) (Left "linker-map sections do not cover the emitted host text")
  pure ordered
 where
  parse line = case C.words line of
    [addressText,lmaText,sizeText,alignmentText,source] | ":(" `B.isInfixOf` source && C.isSuffixOf ")" source -> do
      address <- readWord addressText
      lma <- readWord lmaText
      size <- readWord sizeText
      if size == 0 || address < 0x5d0000 || address >= end then pure Nothing else do
        unless (address == lma && toInteger address + toInteger size <= toInteger end) (Left "linker-map input crosses the host text extent")
        alignment <- case readMaybe (C.unpack alignmentText) :: Maybe Word32 of
          Just value | value > 0 && value <= 16 && value .&. (value-1) == 0 -> Right (InputAlignment value)
          _ -> Left "unsupported linker-map input alignment"
        let (_,suffix) = C.breakSubstring ":(" source
            name = B.take (B.length suffix-3) (B.drop 2 suffix)
            allowed character = isAscii character && (isAlphaNum character || character `elem` ("._$-"::String))
        unless (not (B.null name) && C.all allowed name && (name == ".text" || ".text." `B.isPrefixOf` name || name == ".rodata" || ".rodata." `B.isPrefixOf` name))
          (Left "unsupported linker-map input-section name")
        pure (Just (MeasuredInput (GuestAddress address) (SectionName name) (SectionBytes size) alignment))
    _ -> pure Nothing
  contiguous previous (MeasuredInput address _ (SectionBytes size) alignment) = do
    unless (address == aligned previous alignment) (Left "linker-map text has an unmeasured gap or overlapping input sections")
    let GuestAddress start = address
    pure (GuestAddress (start+size))

inputAtoms :: [MeasuredInput] -> [InputAtom]
inputAtoms measured = textAtoms ++ [atom ReadOnlySections readOnly | not (null readOnly)]
 where
  isText (MeasuredInput _ (SectionName name) _ _) = name == ".text" || ".text." `B.isPrefixOf` name
  readOnly = filter (not . isText) measured
  groups = M.fromListWith (++) [(name,[input]) | input@(MeasuredInput _ name _ _) <- measured,isText input]
  textAtoms = [atom (TextSection name) inputs | (name,inputs) <- M.toList groups]
  atom selection inputs =
    let sizes = [(size,alignment) | MeasuredInput _ _ (SectionBytes size) (InputAlignment alignment) <- inputs]
        bound = case sizes of [(size,_)] -> size; _ -> sum [size+alignment-1 | (size,alignment) <- sizes]
    in InputAtom selection (SectionBytes bound) (InputAlignment (maximum (map snd sizes)))

planPublication :: B.ByteString -> B.ByteString -> Either String PublicationPlan
planPublication symbolBytes mapBytes = do
  symbols <- parseSymbols symbolBytes
  end <- symbol symbols "__custom_text_end"
  unless (end > 0x5d0000 && end <= 0x5e0000 && end .&. 3 == 0) (Left "invalid emitted host carrier extent")
  measured <- parseMeasuredInputs (GuestAddress end) mapBytes
  let atoms = sortOn (\(InputAtom selection size _) -> (Down size,selection)) (inputAtoms measured)
      maximumAlignment = maximum [alignment | InputAtom _ _ alignment <- atoms]
      empty region = PublicationSegment region (aligned (GuestAddress (regionStart region)) maximumAlignment) (SectionBytes 0) []
  allocated <- foldM place (map empty approved) atoms
  let segments = sortOn (\(PublicationSegment _ address _ _) -> address) [segment | segment@(PublicationSegment _ _ _ selections) <- allocated,not (null selections)]
  unless (not (null segments)) (Left "empty measured publication")
  pure (PublicationPlan segments (SectionBytes (end-0x5d0000)))
 where
  place [] _ = Left "whole measured input section does not fit an approved retired interval"
  place (segment@(PublicationSegment region start (SectionBytes used) inputs):rest) atom@(InputAtom _ (SectionBytes size) alignment) =
    let GuestAddress base = start
        GuestAddress next = aligned (GuestAddress (base+used)) alignment
        GuestAddress finish = aligned (GuestAddress (next+size)) (InputAlignment 4)
    in if finish <= regionEnd region
      then Right (PublicationSegment region start (SectionBytes (finish-base)) (atom:inputs):rest)
      else (segment:) <$> place rest atom

publicationRegions :: PublicationPlan -> [Region]
publicationRegions (PublicationPlan segments _) = [region | PublicationSegment region _ _ _ <- segments]

segmentSymbol :: SegmentIndex -> SegmentEndpoint -> String
segmentSymbol (SegmentIndex index) endpoint = "__custom_segment_" ++ show index ++ "_" ++ case endpoint of SegmentStart -> "start"; SegmentEnd -> "end"

publishedRegions :: PublicationPlan -> B.ByteString -> Either String [Region]
publishedRegions plan bytes = parseSymbols bytes >>= segmentRegions plan

segmentRegions :: PublicationPlan -> Symbols -> Either String [Region]
segmentRegions (PublicationPlan segments _) symbols = do
  used <- mapM readSegment (zip (map SegmentIndex [0..]) segments)
  start <- symbol symbols "__custom_text_start"
  end <- symbol symbols "__custom_text_end"
  unless (start == minimum (map regionStart used) && end == maximum (map regionEnd used)) (Left "published text markers differ from its measured segments")
  pure used
 where
  readSegment (index,PublicationSegment capacity (GuestAddress expected) (SectionBytes budget) _) = do
    start <- symbol symbols (segmentSymbol index SegmentStart)
    end <- symbol symbols (segmentSymbol index SegmentEnd)
    let used = Region start end
    unless (start == expected && end > start && end .&. 3 == 0 && end-start <= budget && inside capacity used) (Left "published segment differs from its whole-section placement")
    pure used

publicationLinker :: PublicationPlan -> B.ByteString -> Either String B.ByteString
publicationLinker (PublicationPlan segments _) template = do
  let marker = "SECTIONS {\n"
      (preamble,after) = C.breakSubstring marker template
      (body,closing) = C.breakSubstring "\n}\n" (B.drop (B.length marker) after)
  unless (not (B.null after) && not (B.null closing) && not (marker `B.isInfixOf` B.drop (B.length marker) after)) (Left "host linker section block is not unique")
  dataSection <- special body ".data "
  bss <- special body ".bss "
  discard <- special body "/DISCARD/ "
  fixed <- mapM (fixedSection body) publishedEntries
  let code = [(start,renderSegment index segment) | (index,segment@(PublicationSegment _ (GuestAddress start) _ _)) <- zip (map SegmentIndex [0..]) segments]
      outputs = concatMap snd (sortOn fst (code++fixed))
      startName = segmentSymbol (SegmentIndex 0) SegmentStart
      endName = segmentSymbol (SegmentIndex (length segments-1)) SegmentEnd
      markers = ["  __custom_text_start = " ++ startName ++ ";","  __custom_text_end = " ++ endName ++ ";"]
  pure ("__published_image_start = 0x61d8dc;\n" <> preamble <> marker <> C.unlines (outputs++map C.pack markers++[dataSection,bss,discard]) <> "}\n" <> B.drop 3 closing)
 where
  special body prefix = case [line | line <- C.lines body,prefix `B.isPrefixOf` C.strip line] of
    [line] -> Right line
    _ -> Left "host linker support section is missing or ambiguous"
  fixedSection body (address,entryName,_) =
    let expected = C.pack ("KEEP(*(.text." ++ entryName ++ "))")
        matches = [line | line <- C.lines body,case C.words line of [_,_,":","{",selection,"}"] -> selection == expected; _ -> False]
    in case matches of
      [line] -> case C.words line of
        [_,encoded,":","{",_,"}"] -> do
          actual <- maybe (Left "fixed veneer lacks an explicit address") readWord (C.stripPrefix "0x" encoded)
          unless (actual == address) (Left "fixed veneer address moved in host linker")
          pure (address,[line])
        _ -> Left "invalid fixed veneer output"
      _ -> Left "fixed veneer output is missing or ambiguous"
  renderSegment index@(SegmentIndex ordinal) (PublicationSegment capacity (GuestAddress start) (SectionBytes budget) atoms) =
    let name = if ordinal == 0 then ".text" else ".text." ++ show ordinal
        begin = segmentSymbol index SegmentStart
        end = segmentSymbol index SegmentEnd
        selector (InputAtom (TextSection (SectionName section)) _ _) = "    *(" <> section <> ")"
        selector (InputAtom ReadOnlySections _ _) = "    *(.rodata .rodata.*)"
    in map C.pack ["  " ++ name ++ " 0x" ++ showHex start "" ++ " : {","    " ++ begin ++ " = .;"]
      ++ map selector (reverse atoms)
      ++ map C.pack ["    . = ALIGN(4);","    " ++ end ++ " = .;","  }"
        ,"  ASSERT(SIZEOF(" ++ name ++ ") <= " ++ show budget ++ ", \"published segment exceeds measured section budget\")"
        ,"  ASSERT(" ++ end ++ " <= 0x" ++ showHex (regionEnd capacity) "" ++ ", \"published segment exceeds approved carrier\")"]

customEntries :: [(Word32,String)]
customEntries = Admission.customEntries
recordEntries :: [(Word32,String)]
recordEntries = Admission.recordEntries
retiredSettings :: [Region]
retiredSettings = map (uncurry Region) Admission.retiredSettingsRegions
publishedEntries :: [(Word32,String,String)]
publishedEntries = [(address,"settings_custom_"++name++"_entry","settings_custom_"++name) | (address,name) <- customEntries]
  ++ [(address,"record_store_entry_"++name,"record_store_"++name) | (address,name) <- recordEntries]
  ++ [(address,"settings_bank_"++name++"_entry","xm1_storage_"++name) | (address,name) <- Admission.bankEntries]
contains :: Region -> Word32 -> Bool
contains r value = value >= regionStart r && value < regionEnd r
inside :: Region -> Region -> Bool
inside outer inner = regionStart inner >= regionStart outer && regionEnd inner <= regionEnd outer
intersects :: Region -> Region -> Bool
intersects a b = regionStart a < regionEnd b && regionStart b < regionEnd a
asCarrier :: Region -> Carrier
asCarrier r = Carrier (GuestAddress (regionStart r)) (GuestAddress (regionEnd r))

publicationReferences :: R.FunctionRanges -> [(ScanImage,Image)] -> [ScanHit]
publicationReferences ranges images = R.resolvedReferences ranges images ++ filter refused customHits
  where
    customHits = concatMap (\region -> scanReferences (asCarrier region) images S.empty) (retiredSettings ++ [Region 0x89f8f0 0x89f8f8])
    entries = map (\(address,_,_) -> address) publishedEntries
    replaced site = any (`contains` site) retiredSettings
    refused hit
      | hitImage hit == ScanImage4, let GuestAddress site = hitSite hit, replaced site = False
      | otherwise = case hitReference hit of
          BranchInto (GuestAddress target) -> target `notElem` entries
          MovPairInto (GuestAddress target) -> target `notElem` entries
          PcRelativeInto (GuestAddress target) -> target `notElem` entries
          WordInto -> hitWord hit `notElem` entries

word :: B.ByteString -> Int -> Word32
word bytes offset = sum [fromIntegral (B.index bytes (offset+j)) `shiftL` (8*j) | j <- [0..3]]
wordBytes :: Word32 -> B.ByteString
wordBytes value = B.pack [fromIntegral (value `shiftR` shift) | shift <- [0,8,16,24]]
patch :: B.ByteString -> Word32 -> B.ByteString -> Either String B.ByteString
patch image address replacement = do
  unless (address >= 0x61c400 && toInteger address + toInteger (B.length replacement) <= 0x61c400 + toInteger (B.length image)) (Left "publication patch outside image4")
  let offset = fromIntegral (address - 0x61c400)
  pure (B.take offset image <> replacement <> B.drop (offset+B.length replacement) image)

stagePublication :: Stage4Contract -> PublicationPlan -> B.ByteString -> B.ByteString -> B.ByteString -> Either String B.ByteString
stagePublication contract plan staged linked symbolBytes = do
  symbols <- parseSymbols symbolBytes
  used <- segmentRegions plan symbols
  imageBase <- symbol symbols "__published_image_start"
  unless (imageBase == 0x61d8dc) (Left "published sparse binary base moved")
  let propertyNames = map fst (stage4Entries contract)
  unless (all (`elem` propertyNames) ["xm1_storage_store8","xm1_storage_store16"]
      && all (`notElem` propertyNames) ["xm1_bank_store8","xm1_bank_store16","xm1_save_store8","xm1_save_store16"])
    (Left "publication requires the owned settings property boundary, not raw shims")
  let carriers = publicationRegions plan
      occupied = stage4Carrier contract : stage4Retained contract
  forM_ carriers $ \carrier -> do
    unless (carrier `elem` approved && inside (stage4Hull contract) carrier && not (any (intersects carrier) occupied)) (Left "publication overlaps retained stage4 storage")
    forM_ [regionStart carrier,regionStart carrier+4..regionEnd carrier-4] $ \address -> do
      unless (address >= 0x61c400 && address+4 <= 0x61c400+fromIntegral (B.length staged)) (Left "retired carrier outside image4")
      unless (word staged (fromIntegral (address-0x61c400)) == 0xe7f000f0) (Left "publication carrier is not stage4 retirement fill")
  unless (B.length linked == fromIntegral (0x6ae100 - imageBase)) (Left "published sparse binary extent mismatch")
  forM_ RecordCarrier.cacheSymbols $ \(name,expected) ->
    symbol symbols (C.unpack name) >>= \actual -> unless (actual == expected) (Left ("shared record-store state moved: " ++ C.unpack name))
  let Symbols values = symbols
  unless (M.notMember "record_store_cache_tag" values) (Left "obsolete single cache tag remains")
  let entries = publishedEntries
  forM_ entries $ \(address,entryName,targetName) -> do
    actual <- symbol symbols entryName
    target <- symbol symbols targetName
    unless (actual == address && any (`contains` target) used && target .&. 3 == 0) (Left ("published entry target outside carriers: " ++ entryName))
    let expected = 0xea000000 .|. (((target-address-8) `shiftR` 2) .&. 0xffffff)
    unless (word linked (fromIntegral (address-imageBase)) == expected) (Left ("published entry encoding mismatch: " ++ entryName))
  retired <- foldM (\image region -> patch image (regionStart region) (B.concat (replicate (fromIntegral ((regionEnd region-regionStart region) `div` 4)) (wordBytes 0xe7f000f0)))) staged retiredSettings
  placed <- foldM (\image region -> patch image (regionStart region) (B.take (fromIntegral (regionEnd region-regionStart region)) (B.drop (fromIntegral (regionStart region-imageBase)) linked))) retired used
  foldM (\image (address,_,_) -> patch image address (B.take 4 (B.drop (fromIntegral (address-imageBase)) linked))) placed entries

planValue :: PublicationPlan -> Value
planValue (PublicationPlan segments (SectionBytes measured)) = object ["schema" .= (2::Int),"segments" .= map segmentValue segments,"emitted_bytes" .= measured]
 where
  segmentValue (PublicationSegment carrier (GuestAddress start) (SectionBytes budget) atoms) = object
    ["carrier" .= object ["start" .= regionStart carrier,"end" .= regionEnd carrier],"start" .= start,"reserved_bytes" .= budget,"sections" .= map atomValue (reverse atoms)]
  atomValue (InputAtom selection (SectionBytes size) (InputAlignment alignment)) = object
    (["budget_bytes" .= size,"alignment" .= alignment] ++ case selection of
      TextSection (SectionName name) -> ["kind" .= ("text"::String),"name" .= C.unpack name]
      ReadOnlySections -> ["kind" .= ("read-only-data"::String)])

publicationValue :: PublicationPlan -> B.ByteString -> B.ByteString -> Either String Value
publicationValue plan staged symbolBytes = do
  used <- publishedRegions plan symbolBytes
  let region value = object ["start" .= regionStart value,"end" .= regionEnd value]
  pure (object ["carriers" .= map region used
    , "retired_regions" .= map region retiredSettings
    , "adapters" .= [object ["address" .= address,"word" .= word staged (fromIntegral (address-0x61c400))] | (address,_,_) <- publishedEntries]])

publicationStackEntries :: Stage4Contract -> B.ByteString -> Either String Value
publicationStackEntries contract bytes = do
  Symbols symbols <- parseSymbols bytes
  pure (object ["entries" .= [object ["name" .= name,"entry" .= address] | (name,address) <- stage4Entries contract ++ M.toList symbols]])
