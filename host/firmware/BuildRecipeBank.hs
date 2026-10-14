{-# LANGUAGE OverloadedStrings #-}
-- build-recipe-bank: compiles one editable recipe-bank spec into the bank
-- blob, the nine submenu pages' arrays and node headers, the fourteen label
-- records, the registration writes and the plan a runtime worker applies.
-- Host-only; no camera, no container.
--
-- The `recipe-bank-blob.bin` output is the card file's data region (the seven
-- 0x28 payloads then the seven 26-byte names, 462 bytes for seven slots), not
-- the code blob; the code blob is the linked span from RB_BASE.
--
--   build-recipe-bank SPEC.json OUT.json OUT_DIR
--
-- A region that declares `flash_offset` is one the plain-extent carrier writes:
-- the plan then carries each write's flash offset, emits one span file per
-- write (`span-NN.bin`, the exact bytes) and records the chained command
-- sequence in its notes, where call N's output is call N+1's input DAT.
--
-- SPEC.json (schema recipe-bank-spec-1):
--   image4 / image4_base       the image-4 decode the nine conversion tables and
--                              the parent list's arrays are read from
--   image5 / image5_base       the image-5 decode the registration and probe words are read from
--   slots                      [ { name, payload: "<0x28 bytes hex>" }
--                                | { name, rows: {"0x104": row, ...} } ], at most 7;
--                              payload carries the full-custom 0x28 record;
--                              encoding zeroes only +0x27 bits 2-7; rows
--                              encode the nine-code subset into a zero base payload
--   placement
--     regions                  [ { name, start, bytes, source, source_base, note } ];
--                              every write's current bytes are read from its region's source
--     menu
--       parent_node            the list node the two items are appended to
--       parent_values_address  parent_labels_address
--       slot_node  recall_node
--       slot_values  sub_values  recall_values
--       slot_labels  verb_labels
--       name_record_address  w28
--     host
--       route                  the class-0x281 page route
--       class_property         the pages' X word
--       action_table_word_address / action_table_word_value
--                              the modal action table word and the interposer's link address
--       probe_entry_wire_address / probe_entry_wire_target
--                              the render probe's entry word and the blob address written there
--
-- The image-4, image-5 and plain-image sources are pinned X-M1 1.50 decodes.
-- The action table word must be the stock unused zero entry.
module Main where

import Control.Monad (forM, forM_, unless, when)
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Aeson (Value (..))
import Data.Char (isHexDigit)
import Data.List (sortOn)
import Data.Word (Word16, Word32)
import Numeric (showHex)
import System.Environment (getArgs)

import FujiAF.JSON (encodeCanonical)
import FujiAF.RecipeBank
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U

-- | The four label texts the submenu pages carry, in the order the actions
-- contract lists them.
uiTexts :: [String]
uiTexts =
  [ "EDIT/SAVE CUSTOM SETTING"
  , "SELECT CUSTOM SETTING"
  , "SAVE CURRENT SETTINGS"
  , "EDIT CUSTOM NAME"
  ]

main :: IO ()
main = do
  arguments <- getArgs
  (specPath, outPath, outDir) <- case arguments of
    [s, o, d] -> pure (s, o, d)
    _ -> H.failH "usage: build-recipe-bank SPEC.json OUT.json OUT_DIR"
  spec <- H.jsonRead =<< B.readFile specPath
  H.fields spec "schema image4 image4_base image5 image5_base image2 image2_base image2_compiled_name boot slots placement payload_defaults" "recipe bank spec"
  unless (H.str (H.get spec "schema") == "recipe-bank-spec-1") $
    H.failH "spec: recipe-bank-spec-1 required"
  placement <- pure (H.get spec "placement")
  mode <- stringField placement "mode"
  unless (mode == "pre-bootstrap") $
    H.failH ("spec: placement mode " ++ show mode ++ " has no emitter; the carve placement is under review")
  menu <- pure (H.get placement "menu")
  host <- pure (H.get placement "host")
  image4Base <- hexField spec "image4_base"
  image4 <- B.readFile =<< stringField spec "image4"
  image5Base <- hexField spec "image5_base"
  image5 <- B.readFile =<< stringField spec "image5"
  unless (image4Base == 0x0061c400 && image5Base == 0x00bf1a00) $
    H.failH "spec: pinned X-M1 1.50 decode bases required"
  checkImageHash "image4" image4 "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
  checkImageHash "image5" image5 "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"
  sources <- parseRegions placement
  forM_ sources $ \source -> case regionName (sourceRegion source) of
    "id4_code" -> unless (sourceBase source == image4Base && sourceBytes source == image4) $
      H.failH "spec: id4_code source must be the pinned image4"
    "image5" -> unless (sourceBase source == image5Base && sourceBytes source == image5) $
      H.failH "spec: image5 region source must be the pinned image5"
    _ -> case sourceFlash source of
      Just _ -> unless (sourceBase source == snd U.stockPlainLoad) $
        H.failH "spec: a plain carrier source must use the pinned loader destination"
      Nothing ->
        let start = toInteger (regionStart (sourceRegion source))
            load = toInteger (snd U.stockPlainLoad)
         in when (load <= start && start < load + toInteger (plainWindowEnd - plainWindowStart)) $
              H.failH "spec: a region in the loaded plain window requires a flash_offset"
  let plainSources = [source | source <- sources, Just _ <- [sourceFlash source]]
  forM_ (take 1 plainSources) $ \canonical -> do
    checkImageHash "plain image" (sourceBytes canonical) "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d"
    forM_ plainSources $ \source ->
      unless (sourceBytes source == sourceBytes canonical) $
        H.failH "spec: plain carrier regions must use the same pinned plain image"
  tables <- forM recipeFields $ \field -> do
    table <- either H.failH pure (readConversionTable image4Base image4 (fieldTable field))
    pure (fieldProperty field, table)
  let tableFor p = case lookup p tables of
        Just t -> t
        Nothing -> error ("unreachable: no table for property " ++ show p)
  defaultsSpec <- pure (H.get spec "payload_defaults")
  defaultsAddress <- hexField defaultsSpec "address"
  defaultsBytes <- hexField defaultsSpec "bytes"
  when (fromIntegral defaultsBytes /= payloadBytes) $
    H.failH "spec: the payload defaults record must be one payload wide"
  let defaultsOffset = fromIntegral (defaultsAddress - image4Base) :: Int
  when (defaultsAddress < image4Base || defaultsOffset + fromIntegral defaultsBytes > B.length image4) $
    H.failH "spec: the payload defaults record is not inside the image4 decode"
  let payloadDefaults = B.take (fromIntegral defaultsBytes) (B.drop defaultsOffset image4)
  defaultsDigest <- H.sha256 payloadDefaults
  expectedDigest <- stringField defaultsSpec "sha256"
  unless (defaultsDigest == expectedDigest) $
    H.failH ("spec: the payload defaults record hashes " ++ defaultsDigest ++ ", not the pinned defaults block")
  slots <- parseSlots payloadDefaults tableFor spec
  let ids = bankNameIds
      count = length slots
      mask = foldl' (\acc k -> acc + (1 `shiftL` (k :: Int))) 0 [0 .. count - 1] :: Word16
  generation <- either H.failH pure (nextGeneration Nothing Nothing)
  blob <- either H.failH pure (encodeFileBlob (slots ++ replicate (bankCapacity - length slots) (FileSlot "" payloadDefaults)))
  let payloads = B.take (bankCapacity * payloadBytes) blob
      names = B.drop (bankCapacity * payloadBytes) blob
      fullNames = map fileSlotName slots ++ replicate (bankCapacity - count) ""
  records <- mapM (either H.failH pure . encodeNameRecord) fullNames
  stockRecords <- stockNameRecords spec
  uiRecords <- mapM (either H.failH pure . encodeLabelRecord) uiTexts
  let nameTable = B.append stockRecords (B.concat records <> B.concat uiRecords)
  cardFile <- either H.failH pure (encodeBankFile (BankHeader generation mask) blob)
  -- the nine pages: one slot-list node, one sub-list node per slot, one recall
  -- node. Every page is a class-0x281 list whose rows carry the tokens the
  -- modal interposer decodes (bank-integration-contract).
  let slotNodeAddress = hexFieldIn menu "slot_node"
      recallNodeAddress = hexFieldIn menu "recall_node"
      slotClass = fromIntegral (hexFieldIn menu "class_slot") :: Word16
      subClassBase = hexFieldIn menu "class_sub_base"
      recallClass = fromIntegral (hexFieldIn menu "class_recall") :: Word16
      w28 = hexFieldIn menu "w28"
      subNodes = [slotNodeAddress + 0x30 + 0x2c * k | k <- [0 .. fromIntegral count - 1]]
      slotValuesAddress = hexFieldIn menu "slot_values"
      subValuesAddress = hexFieldIn menu "sub_values"
      recallValuesAddress = hexFieldIn menu "recall_values"
      slotLabelsAddress = hexFieldIn menu "slot_labels"
      verbLabelsAddress = hexFieldIn menu "verb_labels"
      parentValuesAddress = hexFieldIn menu "parent_values_address"
      parentLabelsAddress = hexFieldIn menu "parent_labels_address"
      parentNode = hexFieldIn menu "parent_node"
      nameRecordAddress = hexFieldIn menu "name_record_address"
  unless (length subNodes == count) $ H.failH "spec: one sub-list node per slot"
  unless (recallNodeAddress == slotNodeAddress + 0x30 + 0x2c * fromIntegral count + 0xc) $
    H.failH "spec: the recall node must follow the sub-list nodes"
  editorNodeAddress <- case H.get menu "editor_node" of
    String _ -> pure (hexFieldIn menu "editor_node")
    _ -> H.failH "spec: the submenu rows open the editor node"
  subId <- hexField menu "sub_id"
  registrySubAddress <- hexField menu "registry_sub_address"
  registryEditorAddress <- hexField menu "registry_editor_address"
  registryEditAddress <- hexField menu "registry_edit_address"
  forM_
    [ (registrySubAddress, "slot page")
    , (registryEditorAddress, "editor node")
    , (registryEditAddress, "edit screen")
    ]
    $ \(address, what) ->
      when (readWord image4 image4Base address /= 0) $
        H.failH ("spec: the registry entry for the " ++ what ++ " must be zero before the plan fills it")
  -- the staged-edit pages: one screen node, its dialog child and the nine field
  -- lists whose rows are the fields' own stock rows with child 0
  edit <- pure (H.get menu "edit")
  screenNodeAddress <- hexField edit "screen_node"
  screenValuesAddress <- hexField edit "screen_values"
  screenLabelsAddress <- hexField edit "screen_labels"
  screenClass <- hexField edit "screen_class"
  screenId <- hexField edit "screen_id"
  screenParent <- hexField edit "screen_parent"
  screenTokenBase <- hexField edit "screen_row_tokens_base"
  dialogRowToken <- hexField edit "dialog_row_token"
  dialogNodeAddress <- hexField edit "dialog_node"
  dialogValuesAddress <- hexField edit "dialog_values"
  dialogLabelsAddress <- hexField edit "dialog_labels"
  dialogClass <- hexField edit "dialog_class"
  dialogOkToken <- hexField edit "dialog_ok_token"
  dialogCancelToken <- hexField edit "dialog_cancel_token"
  dialogOkLabel <- hexField edit "dialog_ok_label"
  dialogCancelLabel <- hexField edit "dialog_cancel_label"
  fieldNodesBase <- hexField edit "field_nodes"
  fieldValuesBase <- hexField edit "field_values"
  getAddress <- hexField edit "get_address"
  setAddress <- hexField edit "set_address"
  getSitesBase <- hexField edit "get_sites_base"
  setSitesBase <- hexField edit "set_sites_base"
  conversionSitesBase <- hexField edit "conversion_sites_base"
  fieldSpecs <- case H.get edit "fields" of
    Array items -> pure (foldr (:) [] items)
    _ -> H.failH "spec: edit.fields must be an array"
  unless (length fieldSpecs == 9) $ H.failH "spec: the edit screen needs nine field lists"
  fields <- forM (zip [0 :: Int ..] fieldSpecs) $ \(k, fieldSpec) -> do
    tokens <- hexWords fieldSpec "tokens"
    labelIds <- hexWords fieldSpec "label_ids"
    labelId <- case labelIds of
      [one] -> pure one
      _ -> H.failH "spec: every field list needs one label id"
    pure
      ( k
      , fromIntegral (hexFieldIn fieldSpec "property") :: Word16
      , hexFieldIn fieldSpec "host"
      , hexFieldIn fieldSpec "table"
      , hexFieldIn fieldSpec "labels"
      , tokens
      , labelId
      )
  filmLabelsAddress <- hexField edit "film_labels"
  let filmTokens = [0, 3, 1, 7, 0xc, 5, 6]
  forM_ fields $ \(index, property, _, table, labels, tokens, _) -> do
    let field = recipeFields !! index
    unless (property == fieldProperty field && table == fieldTable field) $
      H.failH "spec: edit fields must follow the pinned property and conversion-table order"
    when (property == 0xfe && (tokens /= filmTokens || labels /= filmLabelsAddress)) $
      H.failH "spec: film edit rows and labels must use the seven pinned stock modes"
  stockFilmLabels <- readArray image4 image4Base 0x0078d4f8 16 0xffffffff
  unless (B.length stockFilmLabels == 96) $
    H.failH "spec: the pinned stock film label array must have five rows"
  filmLabelsBytes <- either H.failH pure $
    appendLabelQuadruples stockFilmLabels [[0x49b, 0x49a, 0x499, 0x49c], [0x497, 0x496, 0x495, 0x498]]
  -- the edit screen's rows are claimed by class only, so the pairing of a row
  -- token with the field it opens rides the row's child pointer: the field order
  -- in the spec must match the host stride or a token would open another field
  forM_ (zip [0 :: Int ..] fields) $ \(k, (_, _, hostProp, _, _, _, _)) ->
    unless (hostProp == 0x267 + fromIntegral k) $
      H.failH
        ( "spec: the field at index "
            ++ show k
            ++ " has host "
            ++ show hostProp
            ++ ", not "
            ++ show (0x267 + k)
        )
  screenLabelsBytes <-
    either H.failH pure (encodeLabelsArray [[0, labelId, 0, 0] | (_, _, _, _, _, _, labelId) <- fields])
  dialogLabelsBytes <-
    either H.failH pure (encodeLabelsArray [[0, dialogOkLabel, 0, 0], [0, dialogCancelLabel, 0, 0]])
  let fieldValuesBases =
        init (scanl (+) fieldValuesBase [fromIntegral (8 * (length tokens + 2)) | (_, _, _, _, _, tokens, _) <- fields])
      fieldNodes =
        [ (k, property, hostProp, table, labels, fieldNodesBase + 0x2c * fromIntegral k, valuesAddress, tokens)
        | ((k, property, hostProp, table, labels, tokens, _), valuesAddress) <- zip fields fieldValuesBases
        ]
      screenRows =
        [(screenTokenBase + fromIntegral k, nodeAddress) | (k, _, _, _, _, nodeAddress, _, _) <- fieldNodes]
          ++ [(dialogRowToken, dialogNodeAddress)]
      editWrites =
        [ sourceWrite
            sources
            "edit_half"
            screenNodeAddress
            (encodeListNode screenId (fromIntegral screenClass) screenParent screenValuesAddress screenLabelsAddress w28)
            "edit screen node header"
        , sourceWrite sources "edit_half" screenValuesAddress (encodeValuesArray screenRows) "edit screen rows"
        , sourceWrite
            sources
            "edit_half"
            screenLabelsAddress
            screenLabelsBytes
            "edit screen row labels"
        , sourceWrite
            sources
            "edit_half"
            dialogNodeAddress
            (encodeListNode screenId (fromIntegral dialogClass) screenNodeAddress dialogValuesAddress dialogLabelsAddress w28)
            "dialog node header"
        , sourceWrite
            sources
            "edit_half"
            dialogValuesAddress
            (encodeValuesArray [(dialogOkToken, 0), (dialogCancelToken, 0)])
            "dialog rows"
        , sourceWrite
            sources
            "edit_half"
            dialogLabelsAddress
            dialogLabelsBytes
            "dialog row labels"
        , sourceWrite sources "edit_half" filmLabelsAddress filmLabelsBytes "seven stock-mode film edit label quadruples"
        ]
          ++ [ sourceWrite
                 sources
                 "edit_half"
                 nodeAddress
                 (encodeListNode screenId (fromIntegral hostProp) screenNodeAddress valuesAddress labels w28)
                 "field-list node header"
             | (_, _, hostProp, _, labels, nodeAddress, valuesAddress, _) <- fieldNodes
             ]
          ++ [ sourceWrite
                 sources
                 "edit_half"
                 valuesAddress
                 (encodeValuesArray ([(token, 0) | token <- tokens] ++ [(dialogRowToken, dialogNodeAddress)]))
                 "field-list values"
             | (_, _, _, _, _, _, valuesAddress, tokens) <- fieldNodes
             ]
          ++ [ wordWrite sources "id4_code" (getSitesBase + 4 * hostProp) getAddress "getter dispatch arm"
             | (_, _, hostProp, _, _, _, _, _) <- fieldNodes
             ]
          ++ [ wordWrite sources "id4_code" (setSitesBase + 4 * hostProp) setAddress "setter dispatch arm"
             | (_, _, hostProp, _, _, _, _, _) <- fieldNodes
             ]
          ++ [ wordWrite sources "id4_code" (conversionSitesBase + 4 * hostProp) table "conversion pointer"
             | (_, _, hostProp, table, _, _, _, _) <- fieldNodes
             ]
          ++ [wordWrite sources "id4_code" registryEditAddress screenNodeAddress "registry entry for the edit screen"]
  actionTableWord <- hexField host "action_table_word_address"
  actionTableValue <- hexField host "action_table_word_value"
  probeWireAddress <- hexField host "probe_entry_wire_address"
  probeEntry <- hexField host "probe_entry_wire_target"
  probeWireStockWord <- hexField host "probe_entry_wire_stock_word"
  forM_ [("action table", actionTableWord), ("probe wire", probeWireAddress)] $
    \(what, address) ->
      when (address < image5Base || fromIntegral (address - image5Base) + 4 > B.length image5) $
        H.failH ("spec: the " ++ what ++ " target runs past image 5")
  unless (readWord image5 image5Base actionTableWord == 0) $
    H.failH "spec: the action table word must hold the pinned unused zero entry"
  -- the modal machine's state cells are halfwords inside the table entry, each
  -- validated as its declared current value before it is moved
  actionCells <- case H.get host "action_cells" of
    Array items -> pure (foldr (:) [] items)
    _ -> H.failH "spec: host.action_cells must be an array"
  actionCellWrites <-
    forM actionCells $ \cell -> do
      let cellAddress = hexFieldIn cell "address"
          cellCurrent = hexFieldIn cell "current"
          cellNew = hexFieldIn cell "new"
      when (cellAddress < image5Base || fromIntegral (cellAddress - image5Base) + 4 > B.length image5) $
        H.failH "spec: an action cell runs past image 5"
      let current = readWord image5 image5Base cellAddress .&. 0xffff
      unless (current == cellCurrent .&. 0xffff) $
        H.failH
          ( "spec: the action cell "
              ++ show cellAddress
              ++ " holds "
              ++ show current
              ++ ", not the declared value"
          )
      pure
        ( sourceWrite
            sources
            "image5"
            cellAddress
            (B.pack [fromIntegral (cellNew .&. 0xff), fromIntegral ((cellNew `shiftR` 8) .&. 0xff)])
            "modal action cell state index"
        )
  -- the probe wire site is an A32 B instruction, not a pointer slot: the stock
  -- word names the branch the patch retargets, and the plan rewrites the same
  -- instruction rather than dropping an address into it
  let probeWireCurrent = readWord image5 image5Base probeWireAddress
  when (probeWireCurrent /= probeWireStockWord) $
    H.failH
      ( "spec: the probe wire site holds "
          ++ show probeWireCurrent
          ++ ", not the declared stock branch word"
      )
  probeWireWord <- either H.failH pure (armTailBranchWord probeWireAddress probeEntry)
  -- every retargeted control-transfer site is handled the same way: the plan
  -- reads the word it is about to replace and refuses a site whose bytes moved
  let retargetSites siteImage siteBase regionName what shim encodeBranch sites stockWords = do
        unless (length sites == length stockWords) $
          H.failH ("spec: every " ++ what ++ " site needs its declared stock word")
        words' <-
          forM (zip sites stockWords) $ \(site, stockWord) -> do
            when (site < siteBase || fromIntegral (site - siteBase) + 4 > B.length siteImage) $
              H.failH ("spec: a " ++ what ++ " site runs past its image")
            let current = readWord siteImage siteBase site
            unless (current == stockWord) $
              H.failH
                ( "spec: the "
                    ++ what
                    ++ " site "
                    ++ show site
                    ++ " holds "
                    ++ show current
                    ++ ", not the declared stock word"
                )
            either H.failH pure (encodeBranch site shim)
        pure [wordWrite sources regionName site word (what ++ ", PC-relative") | (site, word) <- zip sites words']
  pageEntryShim <- hexField host "page_entry_shim_address"
  pageEntrySites <- hexWords host "page_entry_sites"
  pageEntryStockWords <- hexWords host "page_entry_stock_words"
  pageEntryWrites <-
    retargetSites image5 image5Base "image5" "page-entry cache shim call" pageEntryShim armBranchWord pageEntrySites pageEntryStockWords
  mediaDetachShim <- hexField host "media_detach_shim_address"
  mediaDetachSites <- hexWords host "media_detach_sites"
  mediaDetachStockWords <- hexWords host "media_detach_stock_words"
  mediaDetachWrites <-
    retargetSites image4 image4Base "id4_code" "detach-entry epoch bump" mediaDetachShim armTailBranchWord mediaDetachSites mediaDetachStockWords
  mediaAttachShim <- hexField host "media_attach_shim_address"
  mediaAttachSites <- hexWords host "media_attach_sites"
  mediaAttachStockWords <- hexWords host "media_attach_stock_words"
  mediaAttachWrites <-
    retargetSites image4 image4Base "id4_code" "attach-entry epoch bump" mediaAttachShim armTailBranchWord mediaAttachSites mediaAttachStockWords
  -- a kind-7 setting list row is labelled from the quadruple's second word
  -- (zero-based selector 1, consumer 0x01163f2c), while the root list's rows
  -- are labelled from their first word, so the two emissions differ
  slotLabels <- either H.failH pure (encodeLabelsArray [[0, labelId, 0, 0] | labelId <- ids])
  verbLabels <-
    either H.failH pure
      (encodeLabelsArray [[0, 0x848, 0, 0], [0, 0x849, 0, 0], [0, 0x6aa, 0, 0], [0, 0x13f, 0, 0]])
  let slotValues = encodeValuesArray [(0x70 + k, node) | (k, node) <- zip [0 .. fromIntegral count - 1] subNodes]
      subValuesBlocks =
        [ encodeValuesArray
            [ (0x40 + 4 * k, 0)
            , (0x41 + 4 * k, editorNodeAddress)
            , (0x42 + 4 * k, screenNodeAddress)
            , (0x43 + 4 * k, 0)
            , (dialogRowToken, dialogNodeAddress)
            ]
        | k <- [0 .. fromIntegral count - 1]
        ]
      subValuesBases = init (scanl (+) subValuesAddress (map (fromIntegral . B.length) subValuesBlocks))
      recallValues = encodeValuesArray [(0x60 + k, 0) | k <- [0 .. fromIntegral count - 1]]
      slotNode = encodeListNode 1 slotClass parentNode slotValuesAddress slotLabelsAddress w28
      recallNode = encodeListNode 1 recallClass parentNode recallValuesAddress slotLabelsAddress w28
  -- parent list: read its stock arrays, append two rows and two label quads
  case H.get placement "label" of
    Object _ ->
      when (hexFieldIn menu "name_record_address" /= hexFieldIn (H.get placement "label") "new_base") $
        H.failH "spec: the name record table and the moved label base must be the same address"
    _ -> pure ()
  -- the stock arrays are the parent node's own, read from the id4 decode at the
  -- addresses its header names; the extended copies land in gap 1
  parentValuesSource <- hexField menu "parent_values_source"
  parentLabelsSource <- hexField menu "parent_labels_source"
  stockValues <- readArray image4 image4Base parentValuesSource 8 0x7ffffffe
  stockLabels <- readArray image4 image4Base parentLabelsSource 16 0xffffffff
  -- the two appended parent rows open the slot list and the recall list, in the
  -- contract's own row tokens and label ids rather than numbered after the
  -- stock list
  (parentValues', parentLabels') <-
    either H.failH pure
      ( encodeParentAppend
          stockValues
          [(0x7d, slotNodeAddress), (0x7e, recallNodeAddress)]
          stockLabels
          [[0x846, 0, 0, 0], [0x847, 0, 0, 0]]
      )
  let writes =
        [ sourceWrite sources "menu_values" slotValuesAddress slotValues "slot list values"
        , sourceWrite sources "menu_labels" slotLabelsAddress slotLabels "slot label quadruples"
        , sourceWrite sources "menu_values" slotNodeAddress slotNode "slot list node header"
        , sourceWrite sources "menu_values" subValuesAddress (B.concat subValuesBlocks) "sub-list values, one row per field"
        , sourceWrite sources "menu_labels" verbLabelsAddress verbLabels "verb row labels"
        , sourceWrite sources "menu_values" recallValuesAddress recallValues "recall list values"
        , sourceWrite sources "menu_values" recallNodeAddress recallNode "recall list node header"
        , sourceWrite sources "menu_parent" parentValuesAddress parentValues' "parent values array with the two items"
        , sourceWrite sources "menu_parent" parentLabelsAddress parentLabels' "parent labels array with the two items"
        , sourceWrite sources "name_records" nameRecordAddress nameTable "stock 0x83c-0x83e, bank records 0x83f-0x845, label texts 0x846-0x849"
        , wordWrite sources "id4_code" (parentNode + 0x20) parentValuesAddress "parent node +0x20 values pointer"
        , wordWrite sources "id4_code" (parentNode + 0x24) parentLabelsAddress "parent node +0x24 labels pointer"
        , wordWrite sources "image5" actionTableWord actionTableValue "modal action table word, the interposed entry"
        , wordWrite sources "image5" probeWireAddress probeWireWord "render probe entry branch, PC-relative, replacing the stock B"
        , wordWrite sources "id4_code" registrySubAddress slotNodeAddress "registry entry for the slot page, the resolver root of the verb pages"
        , wordWrite sources "id4_code" registryEditorAddress editorNodeAddress "registry entry for the editor node"
        ]
        ++ actionCellWrites
        ++ pageEntryWrites
        ++ mediaDetachWrites
        ++ mediaAttachWrites
        ++ editWrites
        ++ [ sourceWrite
               sources
               "menu_values"
               address
               (encodeListNode subId (fromIntegral (subClassBase + fromIntegral k)) slotNodeAddress valuesAddress verbLabelsAddress w28)
               "sub-list node header"
           | (k, address, valuesAddress) <- zip3 [0 :: Int ..] subNodes subValuesBases
           ]
        ++ editorWrites
        ++ labelWrites sources placement
      editorWrites = case H.get menu "editor_node" of
        String _ ->
          [ sourceWrite
              sources
              "menu_values"
              (hexFieldIn menu "editor_node")
              ( encodeCustomNode
                  (hexFieldIn menu "editor_id")
                  6
                  (fromIntegral (hexFieldIn menu "editor_property") :: Word16)
                  0x2a
                  (hexFieldIn menu "editor_parent")
                  (hexFieldIn menu "editor_values")
                  (hexFieldIn menu "editor_labels")
                  (hexFieldIn menu "editor_w28")
              )
              "bank name editor node header, rows over the stock charset arrays"
          ]
        _ -> []
  let basePlan = WritePlan (map regionOf sources) writes (buildPlanNotes parentNode parentValuesAddress parentLabelsAddress)
      plan = basePlan {planNotes = buildPlanNotes parentNode parentValuesAddress parentLabelsAddress ++ carrierSequence sources basePlan}
  validateBootSeparation (H.get spec "boot") writes
  either H.failH pure (validatePlan plan)
  either H.failH pure (validateCarrierSpans sources plan)
  forM_ (zip [0 :: Int ..] (planWrites plan)) $ \(k, w) -> B.writeFile (outDir ++ "/" ++ spanFileName k) (writeNew w)
  B.writeFile (outDir ++ "/" ++ "slot-payloads.bin") payloads
  B.writeFile (outDir ++ "/" ++ "slot-names.bin") names
  B.writeFile (outDir ++ "/" ++ "recipe-bank.bin") cardFile
  B.writeFile (outDir ++ "/" ++ "recipe-bank-blob.bin") blob
  B.writeFile (outDir ++ "/" ++ "menu-values.bin") slotValues
  B.writeFile (outDir ++ "/" ++ "menu-labels.bin") slotLabels
  B.writeFile (outDir ++ "/" ++ "parent-values.bin") parentValues'
  B.writeFile (outDir ++ "/" ++ "parent-labels.bin") parentLabels'
  B.writeFile (outDir ++ "/" ++ "name-records.bin") nameTable
  blobDigest <- H.sha256 blob
  payloadDigest <- H.sha256 payloads
  let planDocument = planJson generation mask blobDigest payloadDigest sources plan blob records slotValues slotLabels parentValues' parentLabels'
  B.writeFile outPath (encodeCanonical planDocument)
  putStrLn
    ( "recipe-bank: "
        ++ show count
        ++ " slots, data bank "
        ++ show (B.length blob)
        ++ " bytes, name records "
        ++ show (length records)
        ++ " x "
        ++ show nameRecordBytes
        ++ ", plan writes "
        ++ show (length writes)
    )

-- spec ------------------------------------------------------------------
checkImageHash :: String -> B.ByteString -> String -> IO ()
checkImageHash label image expected = do
  actual <- H.sha256 image
  unless (actual == expected) $
    H.failH ("spec: " ++ label ++ " does not match the pinned X-M1 1.50 decode")

validateBootSeparation :: Value -> [PlanWrite] -> IO ()
validateBootSeparation boot writes = do
  base <- hexField boot "carve_base"
  bytes <- H.integer (H.get boot "carve_bytes") 1 0xffffffff "boot carve_bytes"
  stub <- hexField boot "stub_entry"
  let start = toInteger base
      end = start + bytes
      overlaps lo hi w = toInteger (writeAddress w) < hi && lo < toInteger (writeAddress w) + toInteger (B.length (writeNew w))
  unless (end <= 0x100000000) $ H.failH "spec: the boot carve overflows the address space"
  unless (toInteger stub + 4 <= 0x100000000) $ H.failH "spec: the boot stub entry overflows the address space"
  forM_ ["name_base", "status_cell"] $ \key -> do
    address <- hexField boot key
    unless (start <= toInteger address && toInteger address + 4 <= end) $
      H.failH ("spec: boot " ++ key ++ " leaves the carve")
  when (any (overlaps start end) writes || any (overlaps (toInteger stub) (toInteger stub + 4)) writes) $
    H.failH "spec: a pre-bootstrap write overlaps the boot carve or stub entry"


stringField :: Value -> String -> IO String
stringField v key = case H.get v key of
  s@(String _) -> pure (H.str s)
  _ -> H.failH ("spec: string field " ++ key ++ " required")

hexField :: Value -> String -> IO Word32
hexField v key = hexValue <$> stringField v key

hexFieldIn :: Value -> String -> Word32
hexFieldIn v key = case H.get v key of
  String s -> hexValue (H.str (String s))
  _ -> error ("spec: hex field " ++ key ++ " required")

-- | An array of hex-word spec fields, e.g. the retarget sites and their stock
-- words; each element parses the same way a single hex field does.
hexWords :: Value -> String -> IO [Word32]
hexWords v key = case H.get v key of
  Array items -> pure (map (hexValue . H.str) (foldr (:) [] items))
  _ -> H.failH ("spec: " ++ key ++ " must be an array of hex words")

hexValue :: String -> Word32
hexValue text
  | null digits || length digits > 8 = error "spec: one to eight hex digits required"
  | otherwise = foldl' step (0 :: Word32) digits
 where
  digits = case text of
    ('0' : x : rest) | x == 'x' || x == 'X' -> rest
    other -> other
  step acc c
    | isHexDigit c = acc * 16 + fromIntegral (digit c)
    | otherwise = error ("spec: not a hex digit: " ++ [c])
  digit c
    | c >= '0' && c <= '9' = fromEnum c - fromEnum '0'
    | c <= 'F' = 10 + fromEnum c - fromEnum 'A'
    | otherwise = 10 + fromEnum c - fromEnum 'a'

-- | A placement region plus the source its current bytes come from: a decode
-- image with its load address, or the container's media form; `sourceFlash` is
-- the region's flash offset inside the plain-image window when the region is
-- one the carrier's replace-plain addresses by flash offset.
data Source = Source
  { sourceRegion :: Region
  , sourceBytes :: B.ByteString
  , sourceBase :: Word32
  , sourceFlash :: Maybe PlainFlashOffset
  }

parseRegions :: Value -> IO [Source]
parseRegions placement = forM (H.list (H.get placement "regions")) $ \r -> do
  name <- stringField r "name"
  start <- hexField r "start"
  bytes <- H.integer (H.get r "bytes") 1 0xffffffff ("region " ++ name ++ " bytes")
  path <- stringField r "source"
  base <- hexField r "source_base"
  image <- B.readFile path
  flash <- case H.get r "flash_offset" of
    String s -> pure (Just (PlainFlashOffset (hexValue (H.str (String s)))))
    Null -> pure Nothing
    _ -> H.failH ("spec: region " ++ name ++ " flash_offset must be a hex string")
  pure (Source (Region name start (fromIntegral bytes) (H.str (H.get r "note"))) image base flash)

regionOf :: Source -> Region
regionOf = sourceRegion

sourceFor :: [Source] -> String -> Source
sourceFor sources name = case filter ((== name) . regionName . sourceRegion) sources of
  [s] -> s
  [] -> error ("spec: no region named " ++ name)
  _ -> error ("spec: region " ++ name ++ " declared twice")

-- | A blob write into `regionName`, with the current bytes read out of that
-- region's declared source.
sourceWrite :: [Source] -> String -> Word32 -> B.ByteString -> String -> PlanWrite
sourceWrite sources regionName address bytes role =
  let source = sourceFor sources regionName
   in case currentBytes source address (B.length bytes) of
        Left message -> error message
        Right current -> PlanWrite regionName address current bytes role

-- | A 4-byte word write, with the current word read from its source.
wordWrite :: [Source] -> String -> Word32 -> Word32 -> String -> PlanWrite
wordWrite sources regionName address value role =
  sourceWrite sources regionName address (B.pack [fromIntegral ((value `div` (256 ^ i)) `mod` 256) | i <- [0 .. 3 :: Int]]) role

currentBytes :: Source -> Word32 -> Int -> Either String B.ByteString
currentBytes source address len
  | address < sourceBase source = Left ("plan: target " ++ show address ++ " below the source base")
  | fromIntegral (address - sourceBase source) + len > B.length (sourceBytes source) =
      Left ("plan: target " ++ show address ++ " runs past its source")
  | otherwise =
      pure (B.take len (B.drop (fromIntegral (address - sourceBase source)) (sourceBytes source)))

-- | Reads a stock terminator-shaped array out of a decode image: 8-byte pairs
-- ending in {0x7ffffffe, 0}, or 16-byte records ending in four 0xffffffff
-- words.
readArray :: B.ByteString -> Word32 -> Word32 -> Int -> Word32 -> IO B.ByteString
readArray image base address stride terminator
  | address < base = H.failH "placement: array address below the image base"
  | otherwise = go 0
 where
  start = fromIntegral (address - base) :: Int
  go k
    | start + stride * (k + 1) > B.length image =
        H.failH "placement: array terminator not inside the image"
    | otherwise =
        let at i = fromIntegral (B.index image (start + stride * k + i)) :: Word32
            word i = at i .|. (at (i + 1) `shiftL` 8) .|. (at (i + 2) `shiftL` 16) .|. (at (i + 3) `shiftL` 24)
            isTerm
              | stride == 8 = word 0 == terminator && word 4 == 0
              | otherwise = all (\i -> word (4 * i) == terminator) [0 .. 3]
         in if isTerm then pure (B.take (stride * (k + 1)) (B.drop start image)) else go (k + 1)

-- | One 32-bit word read out of a decode image at its runtime address.
readWord :: B.ByteString -> Word32 -> Word32 -> Word32
readWord image base address
  | address < base = error ("spec: address " ++ show address ++ " below the image base")
  | fromIntegral (address - base) + 4 > B.length image = error ("spec: address " ++ show address ++ " past the image")
  | otherwise = foldl' step 0 [0 .. 3 :: Int]
 where
  offset = fromIntegral (address - base) :: Int
  step acc i = acc + fromIntegral (B.index image (offset + i)) * (256 ^ i)

-- | The label-class edit set: direct gates compare the highest admitted id;
-- normalized gates compare that id minus one.
--
-- Spec block under `placement.label`:
--   region          the region the sites live in (a plain-image region)
--   bound_sites     [addresses of gates comparing the id]
--   bound_new       the highest admitted id, e.g. "0x849"
--   normalized_sites [addresses of gates comparing id - 1]
--   bound_normalized the highest admitted `id - 1`, e.g. "0x848"
--   base_sites      [ { "movw": hex, "movt": hex } , ... ]
--   new_base        the bank record table's address
labelWrites :: [Source] -> Value -> [PlanWrite]
labelWrites sources placement = case H.get placement "label" of
  Object _ -> directWrites ++ normalizedWrites ++ concatMap baseWrite baseSites
  _ -> []
 where
  block = H.get placement "label"
  regionName = H.str (H.get block "region")
  directSites = map valueHex (H.list (H.get block "bound_sites"))
  normalizedSites = case H.get block "normalized_sites" of
    Array _ -> map valueHex (H.list (H.get block "normalized_sites"))
    _ -> []
  baseSites = H.list (H.get block "base_sites")
  boundNew = hexFieldIn block "bound_new"
  boundNormalized = case H.get block "bound_normalized" of
    String s -> hexValue (H.str (String s))
    _ -> boundNew - 1
  newBase = hexFieldIn block "new_base"
  source = sourceFor sources regionName
  gateWrite label address bound =
    let old = readWord (sourceBytes source) (sourceBase source) address
        fresh = (old .&. 0xfff0f000) .|. (bound .&. 0xfff) .|. (((bound `shiftR` 12) .&. 0xf) `shiftL` 16)
     in PlanWrite regionName address (word32 old) (word32 fresh) label
  directWrites = map (gateWrite "label gate bound" `flip` boundNew) directSites
  normalizedWrites = map (gateWrite "label gate bound (id-1 form)" `flip` boundNormalized) normalizedSites
  baseWrite entry =
    let movwAddress = hexFieldIn entry "movw"
        movtAddress = hexFieldIn entry "movt"
        oldMovw = readWord (sourceBytes source) (sourceBase source) movwAddress
        oldMovt = readWord (sourceBytes source) (sourceBase source) movtAddress
        freshMovw = (oldMovw .&. 0xfff0f000) .|. (newBase .&. 0xfff) .|. (((newBase `shiftR` 12) .&. 0xf) `shiftL` 16)
        freshMovt = (oldMovt .&. 0xfff0f000) .|. ((newBase `shiftR` 16) .&. 0xfff)
     in [ PlanWrite regionName movwAddress (word32 oldMovw) (word32 freshMovw) "label record base movw"
        , PlanWrite regionName movtAddress (word32 oldMovt) (word32 freshMovt) "label record base movt"
        ]

-- | The three records the stock base already carries, copied verbatim from the
-- image-2 decode the spec names. That image must be the current one, i.e. the
-- build that carries the compiled D3 name write in the id-0x83d record at
-- decode 0x0037b7c6 (recipe A, WARM STANDARD); a stock image-2 silently drops
-- that name. Id 0x83e stays the stock empty record and no accessor gate moves
-- for it.
stockNameRecords :: Value -> IO B.ByteString
stockNameRecords spec = do
  path <- stringField spec "image2"
  base <- hexField spec "image2_base"
  image <- B.readFile path
  let offset = fromIntegral (stockRecordBase - base) :: Int
  unless (offset >= 0 && offset + 3 * nameRecordBytes <= B.length image) $
    H.failH "spec: image2 does not carry the three stock name records at 0x37b784"
  let records' = B.take (3 * nameRecordBytes) (B.drop offset image)
  expected <- stringField spec "image2_compiled_name"
  -- the special-class record's inline buffer is 56 bytes (28 units), wider than
  -- the 26-byte bank-name codec, so the guard compares buffer bytes rather than
  -- decoding at the bank-name width
  expectedBuffer <- either H.failH pure (B.take nameRecordBufferBytes <$> encodeLabelRecord expected)
  let buffer = B.take nameRecordBufferBytes (B.drop nameRecordBytes records')
  unless (buffer == expectedBuffer) $
    H.failH
      ( "spec: the image-2 id-0x83d record buffer does not hold "
          ++ show expected
          ++ "; point image2 at build-compiled-recipes' id2-patched.bin"
      )
  pure records'

stockRecordBase :: Word32
stockRecordBase = 0x0037b784

valueHex :: Value -> Word32
valueHex (String s) = hexValue (H.str (String s))
valueHex _ = error "spec: hex string required"

-- | A node header with the fields the bank's editor needs (a distinct type,
-- property and kind), in the 0x2c-byte shape the pinned node map carries.
encodeCustomNode :: Word32 -> Word32 -> Word16 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> B.ByteString
encodeCustomNode nodeId nodeType property kind parent values labels w28 =
  B.concat
    [ word32 nodeId
    , word32 nodeType
    , word32 (fromIntegral property)
    , word32 kind
    , word32 0
    , word32 0
    , word32 1
    , word32 parent
    , word32 values
    , word32 labels
    , word32 w28
    ]

word32 :: Word32 -> B.ByteString
word32 w = B.pack [fromIntegral ((w `div` (256 ^ i)) `mod` 256) | i <- [0 .. 3 :: Int]]

-- | A slot is a name plus its 0x28 payload. The spec carries the payload
-- verbatim (the full-custom rule) or, when it names the nine fields, the
-- builder encodes them into a zero base payload.
parseSlots :: B.ByteString -> (Word16 -> ConversionTable) -> Value -> IO [FileSlot]
parseSlots base tableFor spec = do
  let slots = H.list (H.get spec "slots")
  unless (not (null slots) && length slots <= bankCapacity) $
    H.failH ("spec: 1 to " ++ show bankCapacity ++ " slots required")
  forM slots $ \s -> do
    case H.get s "payload" of
      String _ -> H.fields s "name payload" "bank slot"
      Null -> H.fields s "name rows" "bank slot"
      _ -> H.failH "spec: a slot payload must be a hex string"
    name <- stringField s "name"
    _ <- either H.failH pure (encodeName name)
    payload <- case H.get s "payload" of
      String hexText -> either H.failH pure (decodePayloadHex (H.str (String hexText)))
      _ -> do
        let rows = H.get s "rows"
            keys = H.keys rows
        unless (sortOn id (map hexValue keys) == sortOn id (map (fromIntegral :: Word16 -> Word32) appliedProperties)) $
          H.failH ("slot " ++ name ++ ": rows must name the applied properties " ++ show appliedProperties ++ " or payload must carry all 0x28 bytes")
        rowPairs <- forM keys $ \key -> do
          let property = hexValue key
              table = tableFor (fromIntegral property)
          row <- H.integer (H.get rows key) 0 (toInteger (length (tableRows table)) - 1) ("slot " ++ name ++ " row " ++ key)
          pure (property, fromInteger row)
        recipe <- either H.failH pure (recipeFromRows tableFor rowPairs)
        either H.failH pure (encodeSlotPayloadOver base recipe)
    pure (FileSlot name payload)

decodePayloadHex :: String -> Either String B.ByteString
decodePayloadHex text
  | length text /= 2 * payloadBytes =
      Left ("payload: " ++ show (length text) ++ " hex characters, expected " ++ show (2 * payloadBytes))
  | not (all isHexDigit text) = Left "payload: not hexadecimal"
  | otherwise = pure (B.pack (pairwise text))
 where
  pairwise [] = []
  pairwise (a : b : rest) = fromIntegral (16 * digit a + digit b) : pairwise rest
  pairwise _ = []
  digit c
    | c >= '0' && c <= '9' = fromEnum c - fromEnum '0'
    | c <= 'F' = 10 + fromEnum c - fromEnum 'A'
    | otherwise = 10 + fromEnum c - fromEnum 'a'

recipeFromRows :: (Word16 -> ConversionTable) -> [(Word32, Int)] -> Either String Recipe
recipeFromRows tableFor rows = recipeFromPairs =<< mapM one appliedProperties
 where
  one p = case [r | (k, r) <- rows, k == fromIntegral p] of
    [row] -> do
      _ <- fieldForProperty p
      code <- rowToCode (tableFor p) row
      pure (p, code)
    [] -> Left ("spec: slot is missing property " ++ show p)
    _ -> Left ("spec: slot names property " ++ show p ++ " twice")

bankNameIds :: [Word32]
bankNameIds = [0x83f .. 0x83f + fromIntegral bankCapacity - 1]

buildPlanNotes :: Word32 -> Word32 -> Word32 -> [String]
buildPlanNotes parentNode valuesAddress labelsAddress =
  [ "parent node 0x" ++ showHex parentNode "" ++ " +0x20/+0x24 retargeted to 0x" ++ showHex valuesAddress "" ++ " / 0x" ++ showHex labelsAddress ""
  , "bank name ids and UI text ids use the special-class accessor bound and base writes in this plan"
  , "the SD bank file is separate from this provisional pre-bootstrap RAM placement"
  ]

planJson :: Word32 -> Word16 -> String -> String -> [Source] -> WritePlan -> B.ByteString -> [B.ByteString] -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Value
planJson generation mask blobDigest payloadDigest sources plan blob records values labels parentValues parentLabels = H.obj
  [ ("schema", H.s "recipe-bank-plan-1")
  , ("generation", H.n (fromIntegral generation))
  , ("slot_valid_mask", H.n (fromIntegral mask))
  , ("bank_file_bytes", H.n (fromIntegral bankFileBytes))
  , ("bank_blob_bytes", H.n (fromIntegral (B.length blob)))
  , ("bank_blob_sha256", H.s blobDigest)
  , ("slot_payloads_sha256", H.s payloadDigest)
  , ("name_records", H.n (fromIntegral (length records)))
  , ("name_record_bytes", H.n (fromIntegral nameRecordBytes))
  , ("list_values_bytes", H.n (fromIntegral (B.length values)))
  , ("list_labels_bytes", H.n (fromIntegral (B.length labels)))
  , ("parent_values_bytes", H.n (fromIntegral (B.length parentValues)))
  , ("parent_labels_bytes", H.n (fromIntegral (B.length parentLabels)))
  , ("writes", H.arr (map (writeJson sources) (planWrites plan)))
  , ("notes", H.arr (map H.s (planNotes plan)))
  ]

-- | The flash offset of a write inside the plain-image window, when its region
-- declares one: the region's base plus the write's offset inside the region.
flashOfWrite :: [Source] -> PlanWrite -> Maybe Word32
flashOfWrite sources write = do
  source <- lookupSource sources write
  PlainFlashOffset flash <- sourceFlash source
  pure (flash + (writeAddress write - regionStart (sourceRegion source)))

lookupSource :: [Source] -> PlanWrite -> Maybe Source
lookupSource sources write = case filter ((== writeRegion write) . regionName . sourceRegion) sources of
  (s : _) -> Just s
  [] -> Nothing

-- | The plain image's loaded window, as the flash offsets the carrier addresses.
plainWindowStart, plainWindowEnd :: Word32
plainWindowStart = 0x5a0000
plainWindowEnd = 0x900000

validateCarrierSpans :: [Source] -> WritePlan -> Either String ()
validateCarrierSpans sources plan =
  validatePlainCarrier
    (PlainLoadAddress (snd U.stockPlainLoad))
    (PlainFlashOffset plainWindowStart, PlainFlashOffset plainWindowEnd)
    [(sourceRegion source, flash) | source <- sources, Just flash <- [sourceFlash source]]
    (planWrites plan)

spanFileName :: Int -> String
spanFileName k = "span-" ++ pad k ++ ".bin"
 where
  pad n = let s = show n in replicate (2 - length s) '0' ++ s

-- | The chained carrier sequence: each call takes the previous call's output as
-- its stock DAT, and the first takes the pinned stock DAT.
carrierSequence :: [Source] -> WritePlan -> [String]
carrierSequence sources plan =
  [ "call "
      ++ show k
      ++ ": rebuild-update replace-plain --flash-offset 0x"
      ++ showHex flash ""
      ++ " "
      ++ input k
      ++ " "
      ++ spanFileName planIndex
      ++ " "
      ++ output k
  | (k, (planIndex, flash)) <- zip [0 :: Int ..] carrierWrites
  ]
 where
  carrierWrites =
    [(planIndex, flash) | (planIndex, write) <- zip [0 :: Int ..] (planWrites plan), Just flash <- [flashOfWrite sources write]]
  input 0 = "<STOCK_DAT>"
  input k = "<out-" ++ show (k - 1) ++ ">"
  output k = "<out-" ++ show k ++ ">"

writeJson :: [Source] -> PlanWrite -> Value
writeJson sources w = H.obj
  [ ("region", H.s (writeRegion w))
  , ("address", H.s (show (writeAddress w)))
  , ("current", H.s (hexBytes (writeCurrent w)))
  , ("new", H.s (hexBytes (writeNew w)))
  , ("role", H.s (writeRole w))
  , ("flash_offset", maybe Null (H.s . show) (flashOfWrite sources w))
  ]

hexBytes :: B.ByteString -> String
hexBytes = concatMap byte . B.unpack
 where
  byte b = [nib (fromIntegral b `div` 16), nib (fromIntegral b `mod` 16)]
  nib n = "0123456789abcdef" !! n
