{-# LANGUAGE OverloadedStrings #-}

-- | The linker script and the census watch ranges, from the one declared extent
-- list in "Replacement.Saclac.Artifact".
module Replacement.Saclac.Placement
  ( Units (..)
  , renderLinkerScript
  , renderLinkerScriptDebug
  , renderAllowlist
  , watchRanges
  , insertWatchRanges
  , bindOwnedPosts
  , bindOwnedEntries
  , hexAddress
  ) where

import Control.Monad (forM, unless)
import Data.Bits (shiftR)
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.List (isInfixOf, isPrefixOf, isSuffixOf, nub)
import Data.Word (Word32)
import qualified FujiAF.FirstStage as F
import Numeric (readHex, showHex)
import qualified Data.Text as T
import qualified Replacement.RtosPlatform.Aggregate as Agg
import Replacement.Saclac.Artifact (Admitted (..), Entry (..), Extent (..), Retained (..), entries, extentBytes, extents, loadingEntries, retainedData, retainedExternals, retainedRegions, unitImports, wordAt)
import qualified Data.Map.Strict as M
import qualified Replacement.DisplayScanout.Artifact as Scanout
import qualified Replacement.FixedImage.Layout as Fixed

hexAddress :: Word32 -> String
hexAddress value = "0x" ++ replicate (8 - length digits) '0' ++ digits
  where
    digits = showHex value ""

regionName :: Int -> String
regionName 0 = "DISPATCH"
regionName 1 = "ROUTER"
regionName index = "EXTENT" ++ show index

sectionOf :: Int -> String
sectionOf 0 = "dispatch"
sectionOf 1 = "router"
sectionOf _ = "driver"

-- | Whether the link carries the file service and the display scanout beside Root and the
-- flag/mailbox family.
data Units = BankUnits | RootFamilyOnly
  deriving (Eq, Show)

-- | The bank script. The carrier union names the fixed-bank family bodies the link keeps and
-- the loader entries they call, as @<entry symbol>_<domain>@ at the stock entry address.
renderLinkerScript :: Units -> Agg.CarrierUnion -> String
renderLinkerScript = renderLinkerScriptFor False

renderLinkerScriptDebug :: Units -> Agg.CarrierUnion -> String
renderLinkerScriptDebug = renderLinkerScriptFor True

renderLinkerScriptFor :: Bool -> Units -> Agg.CarrierUnion -> String
renderLinkerScriptFor debug units union = case (entries, extents) of
  (firstEntry : _, _ : _) -> unlines $
    [ "ENTRY(" ++ entryVeneer firstEntry ++ ")"
    , ""
    , "/* Generated from Replacement.Saclac.Artifact.extents. */"
    , "EXTERN(XM1_KERNEL_BOUNDARY XM1_SACLAC_TASK_STACK XM1_SACLAC_CONSTRUCTOR_STACK xm1_saclac_create_task XM1_DECODER_STATE xm1_decoder_init xm1_decoder_feed xm1_decoder_progress xm1_decoder_complete xm1_decoder_finish)"
    , "/* Present only when the bank links the task60 loading owner. */"
    , "EXTERN(" ++ unwords (map snd loadingEntries) ++ ")"
    ]
      ++ withUnits
         ( ("EXTERN(" ++ unwords [name | Scanout.Entry name _ <- Scanout.entries] ++ ")")
         : [retainedName external ++ " = " ++ hexAddress (retainedAddress external) ++ ";" | external <- unitImports] )
      ++ [ retainedName external ++ " = " ++ hexAddress (retainedAddress external) ++ ";"
         | external <- retainedExternals ++ retainedData,
           any (`isPrefixOf` retainedName external) ["xm1_kernel_", "xm1_decoder_", "__saclac_runtime_"]
         ]
      ++ [ "PROVIDE(" ++ retainedName external ++ " = " ++ hexAddress (retainedAddress external) ++ ");"
         | external <- retainedExternals ++ retainedData,
           any (`isPrefixOf` retainedName external) ["xm1_loading_", "xm1_lower_read_"]
         ]
      ++ [ "EXTERN(" ++ Agg.carrierEntrySymbol entry ++ ")"
         | (_, domainEntries) <- domainUnion, entry <- domainEntries, Agg.carrierEntryBody entry == Agg.BankBody
         ]
      ++ [ "PROVIDE(" ++ Agg.carrierEntrySymbol entry ++ "_" ++ label ++ " = " ++ hexAddress (Agg.carrierEntryAddress entry) ++ ");"
         | (label, domainEntries) <- domainUnion, entry <- domainEntries, Agg.carrierEntryBody entry == Agg.LoaderBody
         ]
      ++ [ "MEMORY {"
    ]
      ++ [ "  " ++ Fixed.linkerMemory ] ++ withUnits [ "  INCLUDE retired-memory.ld" ]
      ++ [ "  " ++ regionName index ++ " (rx) : ORIGIN = " ++ hexAddress (extentLow extent)
             ++ ", LENGTH = " ++ hexAddress (fromIntegral (extentBytes extent))
         | (index, extent) <- zip [0 :: Int ..] extents
         ]
      ++ [ "}"
         , ""
         , "SECTIONS"
         , "{"
         ]
      ++ withUnits [ "  INCLUDE retired.ld" ]
      ++ concat
        [ [ "  ." ++ sectionOf index ++ " : {"
          , "    KEEP(*(.text." ++ entryVeneer entry ++ "))"
          ]
            ++ [ "  } > " ++ regionName index ]
        | (index, entry) <- zip [0 :: Int ..] entries
        ]
      ++ [ "  .text : { __fixed_body_start = .; KEEP(*(.decoder.*)) *(.text*) } > OWNED_FIXED"
         , "  .rodata : { KEEP(*(.rodata.XM1_KERNEL_BOUNDARY)) KEEP(*(.rodata.XM1_SACLAC_TASK_STACK)) KEEP(*(.rodata.XM1_SACLAC_CONSTRUCTOR_STACK)) KEEP(*(.rodata.XM1_REQUIRES_LENS_SAFETY_CONTRACT)) *(.rodata*) . = ALIGN(4); } > OWNED_FIXED"
         , "  .data : { *(.data*) *(.sdata*) *(.got*) . = ALIGN(4); __fixed_body_end = .; } > OWNED_FIXED"
         , "  .bss (NOLOAD) : { __fixed_bss_start = .; *(.bss*) *(.sbss*) *(COMMON) . = ALIGN(4); __fixed_bss_end = .; } > OWNED_FIXED"
         , "  __power_cpu_start = __fixed_body_start;"
         , "  __power_cpu_end = __fixed_bss_end;"
         , "  /DISCARD/ : {"
         , "    *(.ARM.exidx*)"
         , "    *(.ARM.extab*)"
         , "    *(.eh_frame*)"
         , "    *(.ARM.attributes)"
         , "    *(.note*)"
         , "    *(.comment)"
         , (if debug then "" else "    *(.debug*)")
         , "    *(.llvm_addrsig)"
         , "  }"
         , "}"
         , ""
         ]
      ++ [ "ASSERT(" ++ entryVeneer entry ++ " == " ++ hexAddress (entryAddress entry)
             ++ ", \"" ++ entryVeneer entry ++ " moved\")"
         | entry <- entries
         ]
      ++ [ "ASSERT(SIZEOF(." ++ sectionOf index ++ ") <= " ++ hexAddress (fromIntegral (extentBytes extent))
             ++ ", \"" ++ sectionOf index ++ " exceeds its stock extent\")"
         | (index, extent) <- zip [0 :: Int ..] extents
         ]
      ++ [ "ASSERT(__fixed_bss_end <= " ++ hexAddress Fixed.imageLimit ++ ", \"fixed owned image exceeds reserved RAM\")"
         , ""
         ]
  _ -> ""
  where
    domainUnion = [("a", Agg.carrierEntriesA union), ("b", Agg.carrierEntriesB union)]
    withUnits lines' = if units == BankUnits then lines' else []

-- | The bank's check-fixed-image allowlist: every retained entry and data region its joint
-- admission accepts, so both admissions judge one declared set.
renderAllowlist :: Agg.CarrierUnion -> J.Value
renderAllowlist union = J.object
  [ "schema" J..= (2 :: Int)
  , "stock_calls" J..=
      [ J.object ["address" J..= hexAddress address, "name" J..= unwords (nub names), "source" J..= unwords (nub sources)]
      | (address, (names, sources)) <- M.toAscList calls ]
  , "stock_regions" J..=
      [ J.object ["address" J..= hexAddress address, "bytes" J..= bytes, "name" J..= name, "source" J..= artifactSource "retainedRegions"]
      | (name, address, bytes) <- retainedRegions ]
  ]
  where
    artifactSource name = "replacement/saclac/Replacement/Saclac/Artifact.hs " ++ name :: String
    calls = M.fromListWith (\(n1, s1) (n2, s2) -> (n2 ++ n1, s2 ++ s1)) $
      [ (retainedAddress external, ([retainedName external], [artifactSource "retainedExternals"])) | external <- retainedExternals ]
      ++ [ (retainedAddress external, ([retainedName external], [artifactSource "unitImports"])) | external <- unitImports ]
      ++ [ (Agg.carrierEntryAddress entry, ([Agg.carrierEntrySymbol entry ++ "_" ++ label], ["replacement/rtos_platform/carriers.json"]))
         | (label, domainEntries) <- [("a", Agg.carrierEntriesA union), ("b", Agg.carrierEntriesB union)]
         , entry <- domainEntries, Agg.carrierEntryBody entry == Agg.LoaderBody ]

-- | The census watch ranges: the same extents, in the shape the census tool
-- reads. The label is the entry's name without its prefix and suffix.
watchRanges :: [J.Value]
watchRanges =
  J.object ["label" J..= ("fixed-body" :: String), "start" J..= hexAddress (Fixed.imageBase + Fixed.headerBytes), "end" J..= hexAddress Fixed.imageLimit] :
  [ J.object
      [ "label" J..= (dropSuffix "_entry" (dropPrefix "xm1_saclac_" (entryVeneer entry)) ++ "-extent")
      , "start" J..= hexAddress (extentLow extent)
      , "end" J..= hexAddress (extentHigh extent)
      ]
  | (entry, extent) <- zip entries extents
  ]
  where
    dropPrefix prefix text = case splitAt (length prefix) text of
      (start, rest) | start == prefix -> rest
      _ -> text
    dropSuffix suffix text =
      let count = length text - length suffix
       in if count >= 0 && drop count text == suffix then take count text else text

-- | A census sites file with its watch ranges replaced by the declared extents.
insertWatchRanges :: J.Value -> Either String J.Value
insertWatchRanges value = case value of
  J.Object fields -> Right (J.Object (KM.insert (K.fromText "watch") (J.toJSON watchRanges) fields))
  _ -> Left "the census sites file is not a JSON object"

-- | The census sites file with one owned post entry per linked Root image variant: each variant's
-- slot-table backend @message_post@ address and the first instruction word loaded there. The
-- observer enables an entry's ABI only while the loaded word matches, so one sites file serves
-- every compared lane. Binding refuses unless, in each variant, exactly one listed entry holds its
-- declared word and that entry is the variant's own; another variant's entry can never fire there.
bindOwnedPosts :: [Admitted] -> J.Value -> Either String J.Value
bindOwnedPosts images value = do
  posts <- mapM postOf images
  let distinct = nub posts
      holds image (address, word) = wordAt image address == word
  mapM_ (\(image, own) -> unless (filter (holds image) distinct == [own])
    (Left ("owned post " ++ hexAddress (fst own) ++ " is not the only declared post its image holds"))) (zip images posts)
  let owned = [J.object ["address" J..= hexAddress address, "instruction" J..= hexAddress word] | (address, word) <- distinct]
  case value of
    J.Object fields -> case KM.lookup (K.fromText "interface") fields of
      Just (J.Object interface) ->
        Right (J.Object (KM.insert (K.fromText "interface") (J.Object (KM.insert (K.fromText "owned_posts") (J.toJSON owned) interface)) fields))
      _ -> Left "the census sites file has no interface object"
    _ -> Left "the census sites file is not a JSON object"
  where
    postOf admitted = case filter isPost (admittedSymbols admitted) of
      [symbol] -> let address = F.symbolAddress symbol in Right (address, wordAt admitted address)
      [] -> Left "the linked image has no slot-table message_post"
      _ -> Left "the linked image has more than one slot-table message_post"
    isPost symbol = let name = F.symbolName symbol
                     in all (`isInfixOf` name) ["10xm1_kernel", "10slot_table", "9SlotTable"] && "7Backend12message_post" `isSuffixOf` name

-- | The census sites file with Root's owned callback entries and client call veneer, bound from
-- each linked image variant. An owned callback is the replacement symbol of a declared identity
-- whose stock value is a declared stock callback entry: entering it is observed as entering that
-- stock callback. The client veneer is @xm1_saclac_client_call@, observed as the client call
-- stock's reply helpers make. Each row carries the first 16 code bytes, and binding refuses
-- unless, in each variant, exactly its own rows hold their bytes among the rows of the same role.
bindOwnedEntries :: [(Word32, String)] -> [Admitted] -> J.Value -> Either String J.Value
bindOwnedEntries identities images value = do
  fields <- case value of
    J.Object fields -> Right fields
    _ -> Left "the census sites file is not a JSON object"
  interface <- case KM.lookup (K.fromText "interface") fields of
    Just (J.Object interface) -> Right interface
    _ -> Left "the census sites file has no interface object"
  callbacks <- stockCallbacks interface
  let replaced = [(stock, symbol) | (stock, symbol) <- identities, stock `elem` callbacks]
  perImage <- forM images $ \image -> do
    owned <- forM replaced $ \(stock, symbol) -> do
      address <- symbolAt image symbol
      pure (stock, (address, codeAt image address))
    client <- symbolAt image "xm1_saclac_client_call"
    pure (owned, (client, codeAt image client))
  let callbackRows = nub (concatMap fst perImage)
      clientRows = nub (map snd perImage)
      holds image (address, code) = codeAt image address == code
  mapM_ (\(image, (owned, client)) -> do
      mapM_ (\(stock, own) -> unless (filter (holds image . snd) (filter ((== stock) . fst) callbackRows) == [(stock, own)])
        (Left ("owned callback " ++ hexAddress (fst own) ++ " is not the only declared replacement its image holds"))) owned
      unless (filter (holds image) clientRows == [client])
        (Left ("owned client call " ++ hexAddress (fst client) ++ " is not the only declared veneer its image holds")))
    (zip images perImage)
  let callbackJson = [J.object ["address" J..= hexAddress address, "code" J..= code, "stock_entry" J..= hexAddress stock] | (stock, (address, code)) <- callbackRows]
      clientJson = [J.object ["address" J..= hexAddress address, "code" J..= code] | (address, code) <- clientRows]
      updated = KM.insert (K.fromText "owned_clients") (J.toJSON clientJson) (KM.insert (K.fromText "owned_callbacks") (J.toJSON callbackJson) interface)
  Right (J.Object (KM.insert (K.fromText "interface") (J.Object updated) fields))
  where
    symbolAt admitted name = case filter ((== name) . F.symbolName) (admittedSymbols admitted) of
      [symbol] -> Right (F.symbolAddress symbol)
      [] -> Left ("the linked image has no " ++ name)
      _ -> Left ("the linked image repeats " ++ name)
    codeAt admitted address = "0x" ++ concat
      [ byteHex (fromIntegral (wordAt admitted (address + 4 * index) `shiftR` (8 * shift)))
      | index <- [0 .. 3], shift <- [0 .. 3 :: Int] ]
    byteHex :: Int -> String
    byteHex byte = let digits = showHex (byte `mod` 256) "" in replicate (2 - length digits) '0' ++ digits
    stockCallbacks interface = case KM.lookup (K.fromText "entries") interface of
      Just (J.Array rows) -> Right [address | J.Object row <- foldr (:) [] rows
                                    , Just (J.String "callback") <- [KM.lookup (K.fromText "kind") row]
                                    , Just (J.String text) <- [KM.lookup (K.fromText "address") row]
                                    , Just address <- [readHex32 (T.unpack text)]]
      _ -> Left "the census sites file has no interface entries"
    readHex32 text = case readHex (dropPrefix text) of
      [(address, "")] -> Just address
      _ -> Nothing
    dropPrefix text = if "0x" `isPrefixOf` text then drop 2 text else text
