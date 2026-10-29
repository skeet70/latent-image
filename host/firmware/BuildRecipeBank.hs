{-# LANGUAGE OverloadedStrings #-}
-- Host-only recipe data and executable-package emitter.
-- The three-argument invocation emits a separate pre-bootstrap diagnostic.
-- --complete consumes linked kernel/boot artifacts and pinned D3 decodes.
-- BuildRecipePackage composes the resulting DAT.
module Main where

import Control.Monad (foldM, forM, forM_, unless, when)
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Aeson (Value (..))
import Data.Char (isHexDigit)
import Data.List (elemIndex, nub, sort, sortOn)
import Data.Word (Word16, Word32)
import Numeric (showHex)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitSuccess)
import System.Posix.Files (setFileMode)

import FujiAF.JSON (encodeCanonical)
import FujiAF.RecipeBank
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import qualified FujiAF.ClassicChrome as CC
import qualified FujiAF.ClassicChromeRam as CC
import RamPushLayout
import RamPushToolchain

-- | The four label texts the submenu pages carry, in the order the actions
-- contract lists them.
uiTexts :: [String]
uiTexts =
  [ "EDIT/SAVE CUSTOM SETTING"
  , "SELECT CUSTOM SETTING"
  , "SAVE CURRENT"
  , "RENAME"
  ]

data BuildMode
  = PreBootstrap
  | FlashPackage FilePath FilePath FilePath FilePath
  | RamPush Word32 FilePath FilePath FilePath FilePath

runtimeMode :: BuildMode -> Bool
runtimeMode PreBootstrap = False
runtimeMode _ = True

modeKernel :: BuildMode -> Maybe FilePath
modeKernel PreBootstrap = Nothing
modeKernel (FlashPackage kernel _ _ _) = Just kernel
modeKernel (RamPush _ linkedDir _ _ _) = Just linkedDir

-- | Refuses unless the relink script ran with exactly the compiled tool paths.
requirePinnedToolchain :: IO ()
requirePinnedToolchain = forM_ pinned $ \(variable, expected) -> do
  actual <- lookupEnv variable
  unless (actual == Just expected) $
    H.failH ("ram-push: " ++ variable ++ " is not the compiled toolchain path " ++ expected)
  where
    t = ramPushToolchain
    pinned =
      [ ("RAM_PUSH_CLANG", toolClang t)
      , ("RAM_PUSH_LD_LLD", toolLdLld t)
      , ("RAM_PUSH_OBJCOPY", toolObjcopy t)
      , ("RAM_PUSH_NM", toolNm t)
      , ("RAM_PUSH_OBJDUMP", toolObjdump t)
      , ("PATH", toolCoreutilsBin t)
      ]


main :: IO ()
main = do
  arguments <- getArgs
  when (arguments == ["--ram-push-allocation-size"]) $ do
    putStrLn (hex32 runtimeAllocationSize)
    exitSuccess
  (specPath, outPath, outDir, buildMode) <- case arguments of
    [s, o, d] -> pure (s, o, d, PreBootstrap)
    ["--complete", s, o, d, kernel, boot, d3, stock] ->
      pure (s, o, d, FlashPackage kernel boot d3 stock)
    ["--ram-push", base, s, o, d, linkedDir, rc0Id4, rc0Id5, rc0Plain] -> do
      requirePinnedToolchain
      pure
        ( s
        , o
        , d
        , RamPush (hexValue base) linkedDir rc0Id4 rc0Id5 rc0Plain
        )
    _ ->
      H.failH
        "usage: build-recipe-bank SPEC.json OUT.json OUT_DIR | --complete SPEC.json OUT.json OUT_DIR KERNEL_DIR BOOT_DIR D3_DIR STOCK_DAT | --ram-push-allocation-size | --ram-push BASE SPEC.json OUT.json OUT_DIR LINKED_DIR RC0_ID4 RC0_ID5 RC0_PLAIN"
  original <- H.jsonRead =<< B.readFile specPath
  spec <- case buildMode of
    PreBootstrap -> pure original
    FlashPackage kernel _ d3 _ -> carveSpec original kernel d3
    RamPush base linkedDir _ _ _ ->
      carveSpecAt (RamCarve (RuntimeBase base)) original linkedDir ramNameImagePath
  H.fields spec "schema image4 image4_base image5 image5_base image2 image2_base image2_compiled_name boot slots placement payload_defaults" "recipe bank spec"
  unless (H.str (H.get spec "schema") == "recipe-bank-spec-1") $
    H.failH "spec: recipe-bank-spec-1 required"
  placement <- pure (H.get spec "placement")
  placementMode <- stringField placement "mode"
  unless
    ( placementMode == "pre-bootstrap"
        || (placementMode == "complete" && runtimeMode buildMode)
    ) $
    H.failH "spec: complete mode requires linked package inputs"
  menu <- pure (H.get placement "menu")
  host <- pure (H.get placement "host")
  image4Base <- hexField spec "image4_base"
  stockImage4 <- B.readFile =<< stringField spec "image4"
  image5Base <- hexField spec "image5_base"
  image5 <- B.readFile =<< stringField spec "image5"
  unless (image4Base == 0x0061c400 && image5Base == 0x00bf1a00) $
    H.failH "spec: pinned X-M1 1.50 decode bases required"
  checkImageHash "image4" stockImage4 "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
  image4 <- case buildMode of
    RamPush {} -> overlay stockImage4
      [(0x0077ac48 - image4Base, word32 13), (0x0077ac50 - image4Base, word32 13)]
    _ -> pure stockImage4
  checkImageHash "image5" image5 "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"
  sources <- parseRegions placement
  forM_ sources $ \source -> case regionName (sourceRegion source) of
    "id4_code" -> unless (sourceBase source == image4Base && sourceBytes source == stockImage4) $
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
  domain <- either H.failH pure (payloadDomainFromImage4 image4Base image4)
  slots <- parseSlots domain payloadDefaults tableFor spec
  initialWrite <- either (H.failH . show) pure (nextWrite NoBank)
  bankSlots' <- either H.failH pure
    (bankRecordsFromList
      (map (\slot -> Populated (inputSlotName slot) (inputSlotPayload slot) (inputSlotPayload slot)) slots
        ++ replicate (bankCapacity - length slots) Unpopulated))
  let ids = bankNameIds
      count = length slots
      mask = foldl' (\acc k -> acc + (1 `shiftL` (k :: Int))) 0 [0 .. count - 1] :: Word16
      generation = nextWriteGeneration initialWrite
      state = BankState generation NothingSelected Disabled BaseInvalid bankSlots'
      slotNames = map (slotNameText . inputSlotName) slots ++ replicate (bankCapacity - count) ""
      fullNames = if runtimeMode buildMode
        then ["CUSTOM " ++ show k | k <- [1 .. bankCapacity]]
        else slotNames
  paddingPayload <- either H.failH pure (canonicalBankPayload payloadDefaults)
  let payloads = B.concat (map (bankPayloadContents . inputSlotPayload) slots
        ++ replicate (bankCapacity - count) (bankPayloadContents paddingPayload))
  names <- B.concat <$> mapM (either H.failH pure . encodeName) slotNames
  let blob = payloads <> names
  records <- mapM (either H.failH pure . encodeNameRecord) fullNames
  originalRecords <- stockNameRecords spec
  stockRecords <- case buildMode of
    RamPush {} -> do
      chromeRecord <- either H.failH pure (encodeLabelRecord "CLASSIC CHROME")
      pure (B.take (2 * nameRecordBytes) originalRecords <> chromeRecord)
    _ -> pure originalRecords
  uiRecords <- mapM (either H.failH pure . encodeLabelRecord) uiTexts
  let nameTable = B.append stockRecords (B.concat records <> B.concat uiRecords)
  let filmEncoding = case buildMode of
        RamPush {} -> ClassicChromeFilmEncoding
        _ -> D3FilmEncoding
  cardFile <- either H.failH pure (encodeBankFileFor filmEncoding state)
  case classifyBankFileFor filmEncoding domain cardFile of
    ValidFile (DecodedV2 _) | filmEncoding == D3FilmEncoding -> pure ()
    ValidFile (DecodedV3 _) | filmEncoding == ClassicChromeFilmEncoding -> pure ()
    result -> H.failH ("spec: bank is not safe to publish: " ++ show result)
  -- the nine pages: one slot-list node, one sub-list node per slot, one
  -- selection node. Every page is a list whose rows carry the tokens the
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
      slotLabelsAddress = hexFieldIn menu "slot_labels"
      verbLabelsAddress = hexFieldIn menu "verb_labels"
      parentValuesAddress = hexFieldIn menu "parent_values_address"
      parentLabelsAddress = hexFieldIn menu "parent_labels_address"
      parentNode = hexFieldIn menu "parent_node"
      nameRecordAddress = hexFieldIn menu "name_record_address"
  unless (length subNodes == count) $ H.failH "spec: one sub-list node per slot"
  unless (recallNodeAddress == slotNodeAddress + 0x30 + 0x2c * fromIntegral count + 0xc) $
    H.failH "spec: the selection node must follow the sub-list nodes"
  editorNodeAddress <- case H.get menu "editor_node" of
    String _ -> pure (hexFieldIn menu "editor_node")
    _ -> H.failH "spec: the submenu rows open the editor node"
  subId <- hexField menu "sub_id"
  registrySubAddress <- hexField menu "registry_sub_address"
  let dialogId = 0x29
      registryDialogAddress = 0x0081babc + 4 * dialogId
  unless (registrySubAddress == 0x0081babc + 4 * subId) $
    H.failH "spec: the slot root registry word must match its node id"
  registryEditorAddress <- hexField menu "registry_editor_address"
  registryEditAddress <- hexField menu "registry_edit_address"
  forM_
    [ (registrySubAddress, "slot page")
    , (registryDialogAddress, "confirmation dialog")
    , (registryEditorAddress, "editor node")
    , (registryEditAddress, "edit screen")
    ]
    $ \(address, what) ->
      when (readWord image4 image4Base address /= 0) $
        H.failH ("spec: the registry entry for the " ++ what ++ " must be zero before the plan fills it")
  -- the edit pages: one screen node whose rows push the nine field lists, the
  -- verb pages' dialog, and the field lists whose rows are the fields' own stock
  -- rows with child 0
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
  let chromeBank = case buildMode of RamPush {} -> True; _ -> False
      filmTokens = [0, 3, 1, 7, 0xc, 5, 6] ++ [2 | chromeBank]
  forM_ fields $ \(index, property, _, table, labels, tokens, _) -> do
    let field = recipeFields !! index
    unless (property == fieldProperty field && table == fieldTable field) $
      H.failH "spec: edit fields must follow the pinned property and conversion-table order"
    when (property == 0xfe && (tokens /= filmTokens || labels /= filmLabelsAddress)) $
      H.failH "spec: film edit rows and labels differ from the package's supported films"
  stockFilmLabels <- readArray image4 image4Base 0x0078d4f8 16 0xffffffff
  unless (B.length stockFilmLabels == 96) $
    H.failH "spec: the pinned stock film label array must have five rows"
  filmLabelsBytes <- either H.failH pure $
    appendLabelQuadruples stockFilmLabels
      ([[0x49b, 0x49a, 0x499, 0x49c], [0x497, 0x496, 0x495, 0x498]]
        ++ [[0, 0, 0, 0x83e] | chromeBank])
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
  shootingValuesSource <- hexField menu "parent_values_source"
  shootingRows <- readArray image4 image4Base shootingValuesSource 8 0x7ffffffe
  quickRows <- readArray image4 image4Base (readWord image4 image4Base (quickMenuNode + 0x20)) 8 0x7ffffffe
  screenOrder <-
    either H.failH pure $
      editScreenOrder
        (rowChildClasses image4 image4Base shootingRows)
        (rowChildClasses image4 image4Base quickRows)
        [property | (_, property, _, _, _, _, _) <- fields]
  screenLabelsBytes <-
    either H.failH pure (encodeLabelsArray [[labelId, labelId, 0, 0] | k <- screenOrder, let (_, _, _, _, _, _, labelId) = fields !! k])
  dialogLabelsBytes <-
    either H.failH pure (encodeLabelsArray [[dialogOkLabel, dialogOkLabel, 0, 0], [dialogCancelLabel, dialogCancelLabel, 0, 0]])
  let fieldValuesBases =
        init (scanl (+) fieldValuesBase [fromIntegral (8 * (length tokens + 1)) | (_, _, _, _, _, tokens, _) <- fields])
      fieldNodes =
        [ (k, property, hostProp, table, labels, fieldNodesBase + 0x2c * fromIntegral k, valuesAddress, tokens)
        | ((k, property, hostProp, table, labels, tokens, _), valuesAddress) <- zip fields fieldValuesBases
        ]
      screenRows =
        [ (screenTokenBase + fromIntegral row, nodeAddress)
        | (row, k) <- zip [0 :: Int ..] screenOrder
        , let (_, _, _, _, _, nodeAddress, _, _) = fieldNodes !! k
        ]
      editWrites =
        [ sourceWrite
            sources
            "edit_half"
            screenNodeAddress
            (encodeCustomNode screenId 0 (fromIntegral screenClass) NavigationList screenParent screenValuesAddress screenLabelsAddress w28)
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
            (encodeCustomNode dialogId 0 (fromIntegral dialogClass) NavigationList screenNodeAddress dialogValuesAddress dialogLabelsAddress w28)
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
                 (encodeValuesArray [(token, 0) | token <- tokens])
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
  -- SELECT CUSTOM SETTING's eight rows and labels are the kernel's own
  -- rb_select_values and rb_select_labels
  (selectValuesAddress, selectLabelsAddress) <- case modeKernel buildMode of
    Nothing -> H.failH "spec: SELECT CUSTOM SETTING's arrays are linked kernel data, which pre-bootstrap placement does not carry"
    Just kernel -> do
      symbols <- readSymbols (kernel ++ "/kernel.syms")
      (,) <$> linked symbols "rb_select_values" <*> linked symbols "rb_select_labels"
  slotLabels <- either H.failH pure (encodeLabelsArray [[labelId, labelId, 0, 0] | labelId <- ids])
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
      slotNode = encodeCustomNode subId 0 slotClass NavigationList parentNode slotValuesAddress slotLabelsAddress w28
      selectNode = encodeCustomNode 1 0 recallClass NavigationList parentNode selectValuesAddress selectLabelsAddress w28
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
  -- the two appended parent rows open the slot list and the selection list, in the
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
        , sourceWrite sources "menu_values" recallNodeAddress selectNode "select list node header"
        , sourceWrite sources "menu_parent" parentValuesAddress parentValues' "parent values array with the two items"
        , sourceWrite sources "menu_parent" parentLabelsAddress parentLabels' "parent labels array with the two items"
        , sourceWrite sources "name_records" nameRecordAddress nameTable "stock 0x83c-0x83e, bank records 0x83f-0x845, label texts 0x846-0x849"
        , wordWrite sources "id4_code" (parentNode + 0x20) parentValuesAddress "parent node +0x20 values pointer"
        , wordWrite sources "id4_code" (parentNode + 0x24) parentLabelsAddress "parent node +0x24 labels pointer"
        , wordWrite sources "image5" actionTableWord actionTableValue "modal action table word, the interposed entry"
        , wordWrite sources "image5" probeWireAddress probeWireWord "render probe entry branch, PC-relative, replacing the stock B"
        , wordWrite sources "id4_code" registrySubAddress slotNodeAddress "registry entry for the slot page, the resolver root of the verb pages"
        , wordWrite sources "id4_code" registryDialogAddress dialogNodeAddress "registry entry for the confirmation dialog"
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
               (encodeCustomNode subId verbPageNodeType (fromIntegral (subClassBase + fromIntegral k)) NavigationList slotNodeAddress valuesAddress verbLabelsAddress w28)
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
                  TextEntry
                  (hexFieldIn menu "editor_parent")
                  (hexFieldIn menu "editor_values")
                  (hexFieldIn menu "editor_labels")
                  (hexFieldIn menu "editor_w28")
              )
              "bank name editor node header, rows over the stock charset arrays"
          ]
        _ -> []
  consumerWrites <- case modeKernel buildMode of
    Nothing -> pure []
    Just kernel -> do
      symbols <- readSymbols (kernel ++ "/kernel.syms")
      presenter <- linked symbols "rb_bank_present"
      drawRoot <- linked symbols "rb_bank_draw_root"
      drawSelected <- linked symbols "rb_bank_draw_selected"
      redrawDispatch <- linked symbols "rb_bank_redraw_dispatch"
      currentValueLabel <- linked symbols "rb_bank_current_value_label"
      keyRight <- linked symbols "rb_bank_key_right"
      keyLeft <- linked symbols "rb_bank_key_left"
      workerWrites <- fmap concat $ forM
        [ (0x0071c4d4, 0xeb000e42, "rb_media_request_post", armBranchWord)
        , (0x0071c108, 0xeb000f35, "rb_media_request_post", armBranchWord)
        , (0x0071c28c, 0xeb000ed4, "rb_media_request_post", armBranchWord)
        , (0x007115a4, 0xe92d49f0, "rb_media_open_guard_entry", armTailBranchWord)
        , (0x007116e4, 0xe92d48f0, "rb_media_write_guard_entry", armTailBranchWord)
        , (0x00711768, 0xe92d4878, "rb_media_close_guard_entry", armTailBranchWord)
        ] $ \(site, stockWord, name, encodeBranch) -> do
          unless (readWord image4 image4Base site == stockWord) $
            H.failH ("package: stock media worker word differs at " ++ hex32 site)
          target <- linked symbols name
          branch <- either H.failH pure (encodeBranch site target)
          pure [wordWrite sources "id4_code" site branch ("media request identity guard: " ++ name)]
      let plain = sourceFor sources "plain_sites"
      forM_ [(0x00d3d894, 0xe92d4800), (0x00d04188, 0x00c70528),
             (0x00d04114, 0x00c6f14c), (0x00cd24b4, 0x00010098),
             (0x00bfb1f0, 0xeb04f709)] $ \(site, stockWord) ->
        unless (readWord image5 image5Base site == stockWord) $
          H.failH ("package: stock consumer word differs at " ++ hex32 site)
      unless (readWord (sourceBytes plain) (sourceBase plain) 0x0115815c == 0xe92d4830) $
        H.failH "package: stock root drawer displaced push {r4,r5,r11,lr} differs"
      unless (readWord (sourceBytes plain) (sourceBase plain) 0x011089d8 == 0xe12fff33) $
        H.failH "package: stock selected-drawer dispatch blx r3 differs"
      presentBranch <- either H.failH pure (armTailBranchWord 0x00d3d894 presenter)
      drawBranch <- either H.failH pure (armTailBranchWord 0x0115815c drawRoot)
      drawSelectedBranch <- either H.failH pure (armBranchWord 0x011089d8 drawSelected)
      redrawDispatchBranch <- either H.failH pure (armBranchWord 0x00bfb1f0 redrawDispatch)
      previewWrites <- retargetSites (sourceBytes plain) (sourceBase plain) "plain_sites"
        "bank current-value preview call" currentValueLabel armBranchWord
        [0x0116657c, 0x011675c4] [0xebea4b97, 0xebea4785]
      -- the stock mode-apply calls a dial move runs: bl FUN_00d1a7d8 in the UI
      -- event 0x52 handler, and bl FUN_006439ac in UI state 53's events 2 and 0x11
      dialApply <- linked symbols "rb_mode_dial_apply"
      pairApply <- linked symbols "rb_mode_pair_apply"
      dialWrites <- retargetSites image5 image5Base "image5" "mode dial apply call"
        dialApply armBranchWord [0x00c2dfbc] [0xeb03b205]
      pairWrites <- retargetSites image5 image5Base "image5" "mode and class apply call"
        pairApply armBranchWord [0x00c2cf88, 0x00c2d278] [0xebe85a87, 0xebe859cb]
      pure $ workerWrites ++ previewWrites ++ dialWrites ++ pairWrites ++
        [ wordWrite sources "image5" 0x00d3d894 presentBranch "bank presenter entry branch"
        , wordWrite sources "plain_sites" 0x0115815c drawBranch "bank list graphics entry branch"
        , wordWrite sources "plain_sites" 0x011089d8 drawSelectedBranch "bank failure after selected drawer"
        , wordWrite sources "image5" 0x00bfb1f0 redrawDispatchBranch "bank redraw handoff after MainUI drain"
        , wordWrite sources "image5" 0x00d04188 keyRight "modal75 RIGHT action66 bank navigation"
        , wordWrite sources "image5" 0x00d04114 keyLeft "modal75 LEFT action37 bank navigation"
        , wordWrite sources "image5" 0x00cd24b4 0x00010001 "text editor BACK action1 bank cancel"
        ]
  let basePlan = WritePlan (map regionOf sources) (writes ++ consumerWrites) (buildPlanNotes parentNode parentValuesAddress parentLabelsAddress)
      plan = basePlan {planNotes = if runtimeMode buildMode
        then ["Runtime chunks are load destinations, not static container writes.",
          "Stock-word writes are emitted only into the publication table.",
          "recipe-bank.bin is a host diagnostic template, not preloaded runtime storage."]
        else planNotes basePlan ++ carrierSequence sources basePlan}
  unless (runtimeMode buildMode) $ validateBootSeparation (H.get spec "boot") writes
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
  case buildMode of
    PreBootstrap -> pure ()
    FlashPackage kernel boot d3 stock ->
      emitPackage spec sources plan kernel boot d3 stock outDir
    RamPush base linkedDir rc0Id4 rc0Id5 rc0Plain ->
      emitRamPush base plan specPath linkedDir rc0Id4 rc0Id5 rc0Plain outDir
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
        ++ show (length (planWrites plan))
    )

data LinkSymbol = LinkSymbol {symbolName :: String, symbolAddress :: Word32}

readSymbols :: FilePath -> IO [LinkSymbol]
readSymbols path = do
  text <- C.unpack <$> B.readFile path
  forM (lines text) $ \line -> case words line of
    address : rest | not (null rest) && all isHexDigit address ->
      pure (LinkSymbol (last rest) (hexValue address))
    _ -> H.failH ("link symbols: malformed row in " ++ path)

linked :: [LinkSymbol] -> String -> IO Word32
linked symbols name = case [symbolAddress s | s <- symbols, symbolName s == name] of
  [address] -> pure address
  _ -> H.failH ("link symbols: unique definition required for " ++ name)

replaceFields :: Value -> [(String, Value)] -> Value
replaceFields value changes = H.obj
  [(key, maybe (H.get value key) id (lookup key changes)) | key <- H.keys value]

carveBase, executableBase, dataBase, nameBase, sourceBaseAddress :: Word32
carveBase = 0x00be1a00
executableBase = runtimeExecutableBase carveBase
dataBase = 0x00bea200
nameBase = 0x00beb784
sourceBaseAddress = 0x011ad800

ramNameImagePath :: FilePath
ramNameImagePath =
  "private-data/analysis/x-m1-1.50/recipes/rc-complete-20261013/package-field-marker/d3/id2-patched.bin"

ramD3Image4Path, ramRc0Image2Path :: FilePath
ramD3Image4Path =
  "private-data/analysis/x-m1-1.50/recipes/rc-complete-20261013/package-field-marker/d3/id4-patched.bin"
ramRc0Image2Path = "private-data/analysis/x-m1-1.50/filmsim/id2-00130000.bin"

newtype RuntimeBase = RuntimeBase Word32

data CarvePlacement = FlashCarve | RamCarve RuntimeBase

carveSpec :: Value -> FilePath -> FilePath -> IO Value
carveSpec original kernel d3 =
  carveSpecAt FlashCarve original kernel (d3 ++ "/id2-patched.bin")

carveSpecAt :: CarvePlacement -> Value -> FilePath -> FilePath -> IO Value
carveSpecAt placementKind original kernel image2Path = do
  unless (length (H.list (H.get original "slots")) == bankCapacity) $
    H.failH "complete package requires seven independent slots"
  symbols <- readSymbols (kernel ++ "/kernel.syms")
  let (menuData, names) = case placementKind of
        FlashCarve -> (dataBase, nameBase)
        RamCarve (RuntimeBase base) -> (runtimeDataBase base, runtimeNameBase base)
  let relocate value = case value of
        Object _ -> H.obj [(key, relocate (H.get value key)) | key <- H.keys value]
        Array _ -> H.arr (map relocate (H.list value))
        String _ | take 2 (H.str value) == "0x" ->
          let address = hexValue (H.str value)
           in if address == 0x011ac900 then H.s (hex32 names)
              else if 0x011ac5a0 <= address && address < 0x011ad800
                then H.s (hex32 (menuData + address - 0x011ac5a0))
                else value
        _ -> value
      moved = relocate original
      placement = H.get moved "placement"
      menu = H.get placement "menu"
      host = H.get placement "host"
  hostSymbols <- forM
    [("action_table_word_value", "rb_bank_interpose"), ("probe_entry_wire_target", "rb_probe_entry"),
     ("page_entry_shim_address", "rb_bank_page_entry"), ("media_detach_shim_address", "rb_media_epoch_entry_a"),
     ("media_attach_shim_address", "rb_media_epoch_entry_b")] $ \(field, name) -> do
       address <- linked symbols name
       pure (field, H.s (hex32 address))
  getAddress <- linked symbols "rb_stage_edit_get_arm"
  setAddress <- linked symbols "rb_stage_edit_set_arm"
  let carveRegions = ["menu_values", "menu_labels", "name_records", "menu_parent", "edit_half"]
      regions = [if H.str (H.get r "name") `elem` carveRegions
          then replaceFields r [("source", H.s "<carve-zero-template>"),
            ("source_base", H.get r "start"), ("flash_offset", Null)]
          else r | r <- H.list (H.get placement "regions"), H.str (H.get r "name") /= "image4"]
      originalEdit = H.get menu "edit"
      chromeEdit = case placementKind of
        FlashCarve -> originalEdit
        RamCarve {} ->
          let labels = hexFieldIn originalEdit "film_labels" + 8
              fields = [if hexFieldIn field "property" == 0xfe
                  then replaceFields field [("tokens", H.arr (H.list (H.get field "tokens") ++ [H.s "0x2"])),
                    ("labels", H.s (hex32 labels))]
                  else field | field <- H.list (H.get originalEdit "fields")]
           in replaceFields originalEdit [("film_labels", H.s (hex32 labels)), ("fields", H.arr fields)]
      edit = replaceFields chromeEdit
        [("get_address", H.s (hex32 getAddress)), ("set_address", H.s (hex32 setAddress))]
      sizedRegions = [case placementKind of
          RamCarve {} | H.str (H.get region "name") == "edit_half" ->
            replaceFields region [("bytes", H.n 1692)]
          _ -> region | region <- regions]
      newPlacement = replaceFields placement [("mode", H.s "complete"), ("regions", H.arr sizedRegions),
        ("host", replaceFields host hostSymbols), ("menu", replaceFields menu [("edit", edit), ("w28", H.s "0x1")])]
      bootContract = case placementKind of
        FlashCarve -> replaceFields (H.get moved "boot")
          [("format", H.n 2), ("store_off", H.s "0x480"), ("store_bytes", H.n 60)]
        RamCarve {} -> H.get moved "boot"
  pure (replaceFields moved [("placement", newPlacement), ("image2", H.s image2Path), ("boot", bootContract)])

emitPackage :: Value -> [Source] -> WritePlan -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
emitPackage spec sources plan kernel boot d3 stockPath out = do
  stockBytes <- B.readFile stockPath
  checkImageHash "stock DAT" stockBytes "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"
  container <- either H.failH pure (U.parseContainer stockBytes)
  let table = U.partTableTail (U.parsedParts container)
      layout = U.partTableLayout (U.parsedParts container)
      tableEnd = U.tableEndOffset layout
      descriptorOffset = U.plainExtentTableOffset layout + 48 - tableEnd
      descriptor = [U.mediaWordAt table (descriptorOffset + 4 * k) | k <- [0 .. 6]]
  unless (descriptor == [3,0x011e0000,0x0026a5e0,0,0,0x47,0xffffffff]) $
    H.failH "package: final plain extent descriptor/resource differs from the pinned load gate"
  symbols <- readSymbols (kernel ++ "/kernel.syms")
  code <- B.readFile (kernel ++ "/kernel.bin")
  begin <- linked symbols "__rb_image_start"
  end <- linked symbols "__rb_image_end"
  textEnd <- linked symbols "__rb_text_end"
  unless (begin == executableBase && end > begin && end <= dataBase &&
          fromIntegral (end - begin) == B.length code && textEnd <= end) $
    H.failH "package: executable plus BSS does not match the linked carve extent"
  bssStart <- linked symbols "__rb_bss_start"
  bssEnd <- linked symbols "__rb_bss_end"
  unless (textEnd <= bssStart && bssStart <= bssEnd && bssEnd <= end &&
          B.all (== 0) (B.take (fromIntegral (bssEnd - bssStart)) (B.drop (fromIntegral (bssStart - begin)) code))) $
    H.failH "package: linked BSS must be included and zero initialized"
  presenterAddress <- linked symbols "rb_bank_present"
  presenterResume <- linked symbols "rb_stock_present_continue"
  unless (presenterResume == 0x00d3d898) $
    H.failH "package: presenter trampoline must resume after the displaced stock instruction"
  drawAddress <- linked symbols "rb_bank_draw_root"
  drawResume <- linked symbols "rb_stock_draw_root_continue"
  drawSelectedAddress <- linked symbols "rb_bank_draw_selected"
  drawNameAddress <- linked symbols "rb_bank_draw_name"
  redrawDispatchAddress <- linked symbols "rb_bank_redraw_dispatch"
  currentValueLabelAddress <- linked symbols "rb_bank_current_value_label"
  unless (drawResume == 0x01158160) $
    H.failH "package: root drawer trampoline must resume after the displaced stock instruction"
  keyRightAddress <- linked symbols "rb_bank_key_right"
  keyLeftAddress <- linked symbols "rb_bank_key_left"
  forM_ ["rb_bank_interpose", "rb_probe_entry", "rb_bank_page_entry", "rb_bank_present", "rb_call_stock_present",
    "rb_bank_draw_root", "rb_call_stock_draw_root", "rb_bank_key_right", "rb_bank_key_left",
    "rb_call_draw_root_prepare", "rb_call_draw_pane", "rb_call_draw_action_list", "rb_bank_draw_selected", "rb_bank_emit_failure_from_stock",
    "rb_bank_draw_name", "rb_call_name_style", "rb_call_draw_mode", "rb_call_name_background",
    "rb_call_name_title", "rb_call_name_rect", "rb_call_name_shell", "rb_call_name_label",
    "rb_call_name_material", "rb_call_name_utf16", "rb_call_name_stretch",
    "rb_call_name_ascii", "rb_call_name_footer", "rb_call_name_label_position", "rb_call_backdrop_suppressed",
    "rb_bank_redraw_dispatch", "rb_call_stock_redraw_dispatch",
    "rb_bank_current_value_label",
    "rb_media_request_post", "rb_media_open_guard_entry",
    "rb_media_write_guard_entry", "rb_media_close_guard_entry",
    "rb_media_epoch_entry_a", "rb_media_epoch_entry_b", "rb_stage_edit_get_arm", "rb_stage_edit_set_arm"] $ \name -> do
      address <- linked symbols name
      unless (begin <= address && address < textEnd && address .&. 3 == 0) $
        H.failH ("package: executable symbol outside A32 text: " ++ name)
  let imageBytes = nameBase - carveBase + fromIntegral (14 * nameRecordBytes)
      runtimeWrites = [w | w <- planWrites plan, carveBase <= writeAddress w && writeAddress w < 0x00befffc]
      publications = [w | w <- planWrites plan, w `notElem` runtimeWrites, writeCurrent w /= writeNew w]
  unless (B.length code <= 0x8300 && length publications < 128) $
    H.failH "package: code or publication table capacity exceeded"
  unless (hexFieldIn (H.get (H.get spec "placement") "menu") "name_record_address" == nameBase) $
    H.failH "package: fourteen name records require the fixed name base"
  unless (any (\w -> writeAddress w == nameBase && B.length (writeNew w) == 14 * nameRecordBytes) runtimeWrites) $
    H.failH "package: complete fourteen-record name table required"
  publicationPairs <- forM publications $ \w -> do
    source <- maybe (H.failH "package: publication source missing") pure (lookupSource sources w)
    let address = writeAddress w
        bytes = writeNew w
    unless (address .&. 3 == 0 && B.length bytes `elem` [2,4]) $
      H.failH "package: publication must be an aligned word or low halfword"
    old <- either H.failH pure (currentBytes source address 4)
    let fresh = if B.length bytes == 2 then bytes <> B.drop 2 old else bytes
    pure (address, readWord fresh address address)
  let expectedSites =
        [0x0078eab8,0x0078eabc,0x0081bc40,0x0081bc64,0x0081bc84,0x0081bb60,0x0071fa9c,0x0071f7dc,
         0x0071c4d4,0x0071c108,0x0071c28c,0x007115a4,0x007116e4,0x00711768] <>
        [base + 4 * host | base <- [0x0061e128,0x006246c8,0x0081b09c], host <- [0x267 .. 0x26f]] <>
        [0x00d04084,0x00d526fc,0x00bfa3f0,0x00bfa7ec,0x00bfacf0,0x00cd20a4,0x00cd211c,0x00cd243c,0x00d3d894,
         0x0115815c,0x011089d8,0x0116657c,0x011675c4,0x00bfb1f0,0x00d04188,0x00d04114,0x00cd24b4,
         0x00c2dfbc,0x00c2cf88,0x00c2d278] <>
        [0x011122c0,0x01112448,0x01112664,0x011127e8,0x01112a74,0x0111373c,0x01113994,
         0x011640f0,0x0116bbbc,0x0116bcb4,0x01178564,0x01178694,0x01178cf4,0x01178fb8,
         0x01179a58,0x01179abc,0x01179b14,0x01179efc,0x0117a734,0x0117deb8,0x0117def4,0x0117df7c,
         0x0117dc14,0x0117dc80,0x0117dcec,0x0117dd54,0x0117ddbc,0x0117de24,0x0117de70,0x0117dfe8,
         0x0117dc34,0x0117dca0,0x0117dd0c,0x0117dd74,0x0117dddc,0x0117e014]
  unless (length publicationPairs == 97 && sortOn id (map fst publicationPairs) == sortOn id expectedSites) $
    H.failH "package: publication destinations differ from the pinned bank contract"
  let allow = [(0x0061c400, 0x00282800), (0x00bf1a00, 0x004d0020), (0x01100000, 0x00080000)]
  forM_ publicationPairs $ \(address, _) ->
    unless (any (\(lo, spanBytes) -> lo <= address && address + 4 <= lo + spanBytes) allow) $
      H.failH "package: publication leaves stock image allowlist"
  let activationSites = [0x0071fa9c,0x0071f7dc,0x0071c4d4,0x0071c108,0x0071c28c,0x007115a4,0x007116e4,0x00711768,
        0x00d526fc,0x00bfa3f0,0x00bfa7ec,0x00bfacf0,
        0x00d04084,0x00cd20a4,0x00cd211c,0x00cd243c,0x00d3d894,0x0115815c,0x011089d8,0x0116657c,0x011675c4,
        0x00bfb1f0,0x00d04188,0x00d04114,0x00cd24b4,0x00c2dfbc,0x00c2cf88,0x00c2d278]
      priority address
        | address `elem` activationSites = 2 :: Int
        | address >= 0x01100000 = 0
        | otherwise = 1
      ordered = sortOn (\(address, _) -> (priority address, address)) publicationPairs
      tableBytes = B.concat [word32 address <> word32 value | (address, value) <- ordered] <>
        B.replicate (0x400 - 8 * length ordered) 0
  bootSymbols <- readSymbols (boot ++ "/boot.syms")
  forM_ [("rb_boot_entry", 0x0078319c), ("rb_boot_sum", 0x00782348),
    ("rb_boot_publish", 0x00789e2c), ("rb_boot_check", 0x0078d4f8),
    ("rb_validate_entry", sourceBaseAddress + 32),
    ("rb_copy_entry", sourceBaseAddress + 32 + 0x4bc),
    ("rb_store_entry", sourceBaseAddress + 32 + 0x480),
    ("RB_SRC_BASE", sourceBaseAddress), ("RB_SRC_LIMIT", sourceBaseAddress + 32 + imageBytes - 0x01100000),
    ("RB_IMAGE_BYTES", imageBytes),
    ("rb_plain_load_wait", 0x00724b68), ("rb_plain_load_release", 0x00724c00)] $
      \(name, expected) -> linked bootSymbols name >>= \actual ->
        unless (actual == expected) (H.failH ("package: boot symbol mismatch: " ++ name))
  validator <- B.readFile (boot ++ "/validate.bin")
  unless (B.length validator <= 0x80 && not (B.null validator)) $
    H.failH "package: bounded source validator required before publication table"
  copier <- B.readFile (boot ++ "/copy.bin")
  unless (B.length copier == 32) $ H.failH "package: thirty-two-byte checked copy pass required"
  store <- B.readFile (boot ++ "/store.bin")
  unless (B.length store == 60) $ H.failH "package: sixty-byte store/cache pass required"
  let chunks = [(0, validator), (0x80, tableBytes), (0x480, store), (0x4bc, copier), (executableBase - carveBase, code)] <>
        [(writeAddress w - carveBase, writeNew w) | w <- runtimeWrites]
  image <- assemble imageBytes chunks
  let BootCrc32 crc = bootCrc32 image
      header = B.concat (map word32 [0x42524a46, 2, imageBytes, crc, 0, 0, 0, 0])
      package = header <> image
      plainSource = sourceFor sources "plain_sites"
  current <- either H.failH pure (currentBytes plainSource sourceBaseAddress (B.length package))
  unless (B.all (== 0) current) $ H.failH "package: provisional gap1 carrier is not all-zero stock padding"
  unless (sourceBaseAddress >= 0x00befffc && sourceBaseAddress + fromIntegral (B.length package) <= 0x011c0000) $
    H.failH "package: source and runtime extents overlap or carrier exceeds gap1"
  id4 <- B.readFile (d3 ++ "/id4-patched.bin")
  id5 <- B.readFile (d3 ++ "/id5-patched.bin")
  checkImageHash "D3 image4" id4 "6e5df0b7a8a8083679d5cd78e314598e192f7aa04969ca296f5ea381ea5661e0"
  checkImageHash "D3 image5" id5 "9050aae0d456aae40b90630a3aea16cab57ecafc66dce28e9cfa77eb1cea7d76"
  fragments <- forM [("sum", 0x00782348, 48), ("entry", 0x0078319c, 48),
    ("publish", 0x00789e2c, 96), ("check", 0x0078d4f8, 96)] $ \(name, address, capacity) -> do
      bytes <- B.readFile (boot ++ "/" ++ name ++ ".bin")
      unless (B.length bytes == capacity) $ H.failH ("package: boot fragment size mismatch: " ++ name)
      pure (address - 0x0061c1fc, bytes)
  let stock4 = sourceBytes (sourceFor sources "id4_code")
  forM_ [(0x0073bc90,0xe5950014), (0x0073be70,0xe5843010),
    (0x0073be74,0xe5940014), (0x0073be78,0xebffa360),
    (0x0073c0a0,0xe5940014), (0x0073c0a8,0xebffa2ae),
    (0x0073c0c4,0xe5940014), (0x0073c0c8,0xebffa2cc)] $ \(address, word) ->
      unless (readWord stock4 0x0061c400 address == word) $
        H.failH "package: stock load-resource wait/release instructions moved"
  unless (readWord stock4 0x0061c400 0x0069d2e0 == 0xe12fff33) $
    H.failH "package: stock image5 launch instruction moved"
  branch <- either H.failH pure (armTailBranchWord 0x0069d2e0 0x0078319c)
  final4 <- overlay id4 (fragments <> [(0x0069d2e0 - 0x0061c1fc, word32 branch)])
  unless (readWord stock4 0x0061c400 0x0072b12c == 0xe3013a00 &&
          readWord stock4 0x0061c400 0x0072b130 == 0xe34030bf) $
    H.failH "package: stock heap upper-bound instruction pair moved"
  carved4 <- overlay final4 [(0x0072b130 - 0x0061c1fc, word32 0xe34030be)]
  B.writeFile (out ++ "/runtime-image.bin") image
  B.writeFile (out ++ "/source-package.bin") package
  B.writeFile (out ++ "/id4-final.bin") carved4
  B.writeFile (out ++ "/id5-final.bin") id5
  B.writeFile (out ++ "/publication.json") (encodeCanonical (H.arr
    [H.obj [("address", H.s (hex32 address)), ("value", H.s (hex32 value))] | (address, value) <- ordered]))
  packageHash <- H.sha256 package
  codeHash <- H.sha256 code
  B.writeFile (out ++ "/package-layout.json") (encodeCanonical (H.obj
    [("schema", H.s "recipe-executable-package-1"), ("installable", Bool False),
     ("source_start", H.s (hex32 sourceBaseAddress)), ("source_end", H.s (hex32 (sourceBaseAddress + fromIntegral (B.length package)))),
     ("runtime_start", H.s (hex32 carveBase)), ("runtime_end", H.s (hex32 (carveBase + imageBytes))),
     ("executable_start", H.s (hex32 begin)), ("executable_end", H.s (hex32 end)),
     ("name_records", H.n 14), ("name_base", H.s (hex32 nameBase)),
     ("format", H.n 2), ("covered_image_bytes", H.n (fromIntegral imageBytes)),
     ("checksum", H.s "CRC-32/ISO-HDLC"), ("image_crc32", H.s (hex32 crc)),
     ("validator_off", H.s "0x00000000"), ("validator_entry", H.s (hex32 (sourceBaseAddress + 32))),
     ("validator_bytes", H.n (fromIntegral (B.length validator))),
     ("store_off", H.s "0x00000480"), ("store_entry", H.s (hex32 (sourceBaseAddress + 32 + 0x480))),
     ("store_bytes", H.n (fromIntegral (B.length store))),
     ("copy_off", H.s "0x000004bc"), ("copy_entry", H.s (hex32 (sourceBaseAddress + 32 + 0x4bc))),
     ("copy_bytes", H.n (fromIntegral (B.length copier))),
     ("status_cell", H.s "0x00befffc"), ("publication_count", H.n (fromIntegral (length ordered))),
     ("presenter_entry", H.s "0x00d3d894"), ("presenter_target", H.s (hex32 presenterAddress)),
     ("presenter_displaced_word", H.s "0xe92d4800"), ("presenter_stock_resume", H.s "0x00d3d898"),
     ("root_draw_entry", H.s "0x0115815c"), ("root_draw_target", H.s (hex32 drawAddress)),
     ("root_draw_displaced_word", H.s "0xe92d4830"), ("root_draw_stock_resume", H.s "0x01158160"),
     ("failure_graphics_site", H.s "0x011089d8"), ("failure_graphics_target", H.s (hex32 drawSelectedAddress)),
     ("failure_graphics_stock_word", H.s "0xe12fff33"), ("failure_graphics_stock_dispatch", H.s "r3"),
     ("failure_graphics_packet_register", H.s "r6"),
     ("failure_graphics_view_register", H.s "r1"),
     ("name_editor_graphics_target", H.s (hex32 drawNameAddress)),
     ("name_editor_graphics_packet", H.s "0x47"),
     ("name_editor_node_id", H.s "0x6a"), ("name_editor_class", H.s "0x131"),
     ("name_editor_buffer", H.s "0x00954385"),
     ("redraw_poll_site", H.s "0x00bfb1f0"), ("redraw_poll_target", H.s (hex32 redrawDispatchAddress)),
     ("redraw_poll_stock_word", H.s "0xeb04f709"), ("redraw_poll_stock_target", H.s "0x00d38e1c"),
     ("current_value_preview_sites", H.arr (map (H.s . hex32) [0x0116657c,0x011675c4])),
     ("current_value_preview_target", H.s (hex32 currentValueLabelAddress)),
     ("current_value_preview_stock_words", H.arr (map (H.s . hex32) [0xebea4b97,0xebea4785])),
     ("current_value_preview_stock_target", H.s "0x00bf93e0"),
     ("key_right_site", H.s "0x00d04188"), ("key_right_target", H.s (hex32 keyRightAddress)),
     ("key_left_site", H.s "0x00d04114"), ("key_left_target", H.s (hex32 keyLeftAddress)),
     ("editor_cancel_cell_site", H.s "0x00cd24b4"), ("editor_cancel_cell_word", H.s "0x00010001"),
     ("source_package_sha256", H.s packageHash), ("executable_sha256", H.s codeHash),
     ("heap_upper_bound", H.s "0x00be1a00"), ("heap_capacity_loss_bytes", H.n 65536),
     ("heap_bound_site", H.s "0x0072b130"), ("heap_bound_current_word", H.s "0xe34030bf"),
     ("heap_bound_new_word", H.s "0xe34030be"),
     ("source_readiness", H.s "single bounded wait/release on final plain extent resource0x47, then high-water gate before header read"),
     ("loader_wait_ticks", H.n 1000),
     ("loader_wait_resource", H.s "0x47"), ("loader_descriptor_address", H.s "0x0061c1e0"),
     ("loader_descriptor_words", H.arr (map (H.s . hex32) descriptor)),
     ("refusal_fallback", H.s "D3/unlock/Q with reduced heap; no bank stock-word publication"),
     ("unaccepted", H.arr (map H.s ["gap1 lifetime/ownership", "nonallocator carve writer exclusion",
       "heap exhaustion with 64KiB reduction", "source fetch coherence", "publication core ownership",
       "independent final-byte review", "camera behavior"]))]))

data RamPublication = RamPublication
  { ramAddress :: Word32
  , ramOld :: Word32
  , ramNew :: Word32
  }

-- | D3's rows and tables move out of Classic Chrome's prefix window.
relocatedD3
  :: Word32 -> B.ByteString -> B.ByteString -> B.ByteString
  -> IO ([(Word32, B.ByteString)], [RamPublication])
relocatedD3 base rc0Id4 rc0Id5 d3Id4 = do
  let start = runtimeBankEnd base
      values = start
      labels = start + 88
      qLabels = start + 264
      rows = start + 440
      props = start + 456
      hook = start + 472
      table = start + 548
      oldBase = 0x0061c1fc :: Word32
      slice address bytes = B.take bytes (B.drop (fromIntegral (address - oldBase)) d3Id4)
      rowA = slice 0x0077ac48 12
      tableRows' = slice 0x0077ac30 156
      oldHook = slice 0x0061c3ac 76
      mov rd address upper =
        let immediate = if upper then address `shiftR` 16 else address .&. 0xffff
         in word32 ((if upper then 0xe3400000 else 0xe3000000)
              .|. (rd `shiftL` 12) .|. ((immediate .&. 0xf000) `shiftL` 4) .|. (immediate .&. 0xfff))
  call <- either H.failH pure (armBranchWord (hook + 52) 0x00d4fc74)
  hookBytes' <- overlay oldHook
    [(8, mov 4 rows False), (12, mov 4 rows True), (24, word32 0xe355000d),
     (32, mov 6 props False), (36, mov 6 props True), (52, word32 call)]
  chromeLabels <- either H.failH pure (encodeLabelsArray
    ([[readWord (slice 0x0061c24c 144) 0 (fromIntegral (16 * k + 4 * j)) | j <- [0 :: Int .. 3]] | k <- [0 :: Int .. 8]]
      ++ [[0, 0, 0, 0x83e]]))
  chromeQ <- either H.failH pure (encodeLabelsArray
    ([[readWord (slice 0x0061c2ec 144) 0 (fromIntegral (16 * k + 4 * j)) | j <- [0 :: Int .. 3]] | k <- [0 :: Int .. 8]]
      ++ [[0x83e, 0, 0, 0]]))
  filmTable <- overlay (tableRows' <> word32 15 <> B.drop 4 rowA <> B.concat (replicate 3 (word32 0xffff)))
    [(24, word32 13), (32, word32 13)]
  let pointers =
        [(0x007823c8, labels), (0x007823c4, values), (0x00785d98, labels),
         (0x00785d94, values), (0x0077d1a8, labels), (0x0077d1a4, values),
         (0x0078b024, qLabels), (0x0078b020, values), (0x0081b494, table)]
      pub4 (address, value) = RamPublication address (readWord rc0Id4 0x0061c340 address) value
  branch <- either H.failH pure (armBranchWord 0x00d50a48 hook)
  pure
    ([(values, encodeValuesArray [(row, 0) | row <- [0,3,1,7,12,5,6,13,4,2]]),
      (labels, chromeLabels), (qLabels, chromeQ), (rows, slice 0x0061c38c 16),
      (props, slice 0x0061c39c 16), (hook, hookBytes'), (table, filmTable)],
     map pub4 pointers ++
       [RamPublication 0x00d50a48 (readWord rc0Id5 0x00bf1a00 0x00d50a48) branch,
        RamPublication 0x00d5a088 (readWord rc0Id5 0x00bf1a00 0x00d5a088) 0xe320f000])

emitRamPush :: Word32 -> WritePlan -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
emitRamPush base plan specPath linkedDir rc0Id4Path rc0Id5Path rc0PlainPath out = do
  setFileMode out 0o700
  unless (base .&. 7 == 0 && base <= maxBound - runtimeAllocationSize) $
    H.failH "ram-push: base must be 8-byte aligned with room for the fixed allocation"
  rc0Id4 <- B.readFile rc0Id4Path
  rc0Id5 <- B.readFile rc0Id5Path
  rc0Plain <- B.readFile rc0PlainPath
  checkImageHash "RC0 image4" rc0Id4 "a4d169bd961ec7df34e3b9b7108b624b79680c3cc50297e434e0b3607fb65551"
  checkImageHash "RC0 image5" rc0Id5 "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"
  checkImageHash "RC0 plain image" rc0Plain "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d"
  symbols <- readSymbols (linkedDir ++ "/kernel.syms")
  installerDisassembly <- C.unpack <$> B.readFile (linkedDir ++ "/kernel.disasm")
  unless (installerHasCallClosure installerDisassembly) $
    H.failH "ram-push: installer lacks the two-core rendezvous publication closure"
  linkedStart <- linked symbols "__rb_image_start"
  linkedEnd <- linked symbols "__ram_push_end"
  rbEnd <- linked symbols "__rb_image_end"
  installerStart <- linked symbols "__ram_push_installer_start"
  unless
    ( linkedStart == runtimeExecutableBase base
        && rbEnd <= runtimeDataBase base
        && installerStart == runtimeInstallerBase base
        && installerStart < linkedEnd
        && linkedEnd - installerStart == runtimeInstallerExtent
        && linkedEnd - base <= runtimeAllocationSize
    ) $
    H.failH "ram-push: linked runtime does not match the fixed allocation contract"
  let size = runtimeAllocationSize
      runtimeRegions = ["menu_values", "menu_labels", "name_records", "menu_parent", "edit_half"]
      runtimeWrites =
        [w | w <- planWrites plan, writeRegion w `elem` runtimeRegions]
      publications =
        [ w
        | w <- planWrites plan
        , writeRegion w `notElem` runtimeRegions
        , writeCurrent w /= writeNew w
        ]
      installedImages =
        [ (0x0061c340, rc0Id4)
        , (0x00bf1a00, rc0Id5)
        , (0x01100000, rc0Plain)
        ]
      readInstalled address = case
        [ readWord bytes imageBase address
        | (imageBase, bytes) <- installedImages
        , imageBase <= address
        , toInteger address + 4 <= toInteger imageBase + toInteger (B.length bytes)
        ] of
          [word] -> pure word
          _ -> H.failH ("ram-push: publication is outside the RC0-installed images: " ++ hex32 address)
  publicationPairs <- forM publications $ \w -> do
    let address = writeAddress w
        freshBytes = writeNew w
    unless (address .&. 3 == 0 && B.length freshBytes `elem` [2, 4]) $
      H.failH "ram-push: publication must be an aligned word or low halfword"
    old <- readInstalled address
    let oldBytes = word32 old
    unless (writeCurrent w == B.take (B.length freshBytes) oldBytes) $
      H.failH ("ram-push: RC0 old bytes differ at " ++ hex32 address)
    let newBytes = freshBytes <> B.drop (B.length freshBytes) oldBytes
    pure (RamPublication address old (readWord newBytes address address))
  let expectedSites =
        [0x0078eab8,0x0078eabc,0x0081bc40,0x0081bc64,0x0081bc84,0x0081bb60,0x0071fa9c,0x0071f7dc,
         0x0071c4d4,0x0071c108,0x0071c28c,0x007115a4,0x007116e4,0x00711768] <>
        [siteBase + 4 * host | siteBase <- [0x0061e128,0x006246c8,0x0081b09c], host <- [0x267 .. 0x26f]] <>
        [0x00d04084,0x00d526fc,0x00bfa3f0,0x00bfa7ec,0x00bfacf0,0x00cd20a4,0x00cd211c,0x00cd243c,0x00d3d894,
         0x0115815c,0x011089d8,0x0116657c,0x011675c4,0x00bfb1f0,0x00d04188,0x00d04114,0x00cd24b4,
         0x00c2dfbc,0x00c2cf88,0x00c2d278] <>
        [0x011122c0,0x01112448,0x01112664,0x011127e8,0x01112a74,0x0111373c,0x01113994,
         0x011640f0,0x0116bbbc,0x0116bcb4,0x01178564,0x01178694,0x01178cf4,0x01178fb8,
         0x01179a58,0x01179abc,0x01179b14,0x01179efc,0x0117a734,0x0117deb8,0x0117def4,0x0117df7c,
         0x0117dc14,0x0117dc80,0x0117dcec,0x0117dd54,0x0117ddbc,0x0117de24,0x0117de70,0x0117dfe8,
         0x0117dc34,0x0117dca0,0x0117dd0c,0x0117dd74,0x0117dddc,0x0117e014]
      legacyActivationSites =
        [0x0071fa9c,0x0071f7dc,0x0071c4d4,0x0071c108,0x0071c28c,0x007115a4,0x007116e4,0x00711768,
         0x00d526fc,0x00bfa3f0,0x00bfa7ec,0x00bfacf0,
         0x00d04084,0x00cd20a4,0x00cd211c,0x00cd243c,0x00d3d894,0x0115815c,0x011089d8,0x0116657c,0x011675c4,
         0x00bfb1f0,0x00d04188,0x00d04114,0x00cd24b4,0x00c2dfbc,0x00c2cf88,0x00c2d278]
      legacyPriority publication
        | ramAddress publication `elem` legacyActivationSites = 2 :: Int
        | ramAddress publication >= 0x01100000 = 0
        | otherwise = 1
      legacyOrdered = sortOn (\publication -> (legacyPriority publication, ramAddress publication)) publicationPairs
      safeOrder = safePublicationOrder
      publicationCore index
        | index >= 97 = "unknown"
        | index <= 35 || index >= 54 && index <= 71 || index >= 75 = "cpu0"
        | otherwise = "unknown"
  unless
    ( length legacyOrdered == 97
        && sortOn id (map ramAddress legacyOrdered) == sortOn id expectedSites
    ) $
    H.failH "ram-push: publication destinations differ from the bank contract"
  rc0Id2 <- B.readFile ramRc0Image2Path
  d3Id4 <- B.readFile ramD3Image4Path
  checkImageHash "RC0 image2" rc0Id2 "236d12a11c8b386f71192675c12398358840327f21e392e6eaa91ddfd6e020a5"
  checkImageHash "D3 image4" d3Id4 "6e5df0b7a8a8083679d5cd78e314598e192f7aa04969ca296f5ea381ea5661e0"
  (d3Chunks, d3Activation) <- relocatedD3 base rc0Id4 rc0Id5 d3Id4
  let ccInputs =
        [ ("CC_ID4", "private-data/analysis/classic-chrome-patch/candidate-build-1/patched-id4.bin",
            "00493fab3f7c67fddf3899e8f38395def2b593b93e4dcb73206bfb6cc2ddff6a")
        , ("CC_ID5", "private-data/analysis/classic-chrome-patch/candidate-build-1/patched-id5.bin",
            "e740aeb28ce070a659583bd0bb5cfd9ecc5846ac302564741fcdf37b9b2089bb")
        , ("CC_PLAIN", "private-data/analysis/classic-chrome-patch/candidate-build-1/patched-plain.bin",
            "7e60e04e2dde4cecea4d575e02ae64b8f6053af3f459c42b5222f980e3c301fa")
        ]
  ccInputsRead <- forM ccInputs $ \(variable, fallback, digest) -> do
    path <- maybe fallback id <$> lookupEnv variable
    bytes <- B.readFile path
    checkImageHash variable bytes digest
    pure (path, bytes)
  ccFlash <- case map snd ccInputsRead of
    [a, b, c] -> pure (CC.FlashImages a b c)
    _ -> H.failH "ram-push: three Classic Chrome images required"
  ccSymbols <- either H.failH pure (CC.ramSymbols [(symbolName s, CC.Address (symbolAddress s)) | s <- symbols])
  ccPublications <- either H.failH pure
    (CC.ccRamPublications (CC.Rc0Images rc0Id4 rc0Id5 rc0Plain rc0Id2) ccFlash ccSymbols)
  extra <- fmap concat $ forM
    ["host/firmware/classic-chrome-liveview-image5.json", "host/firmware/classic-chrome-frames-image5.json"] $ \path -> do
      fragment <- H.jsonRead =<< B.readFile path
      forM (H.list (H.get fragment "words")) $ \w -> do
        address <- hexField w "address"
        old <- hexField w "current"
        new <- hexField w "new"
        unless (readWord rc0Id5 0x00bf1a00 address == old) $
          H.failH ("ram-push: Classic Chrome fragment old word differs at " ++ hex32 address)
        pure (RamPublication address old new)
  parameterPath <- maybe "private-data/analysis/classic-chrome/candidate.json" id <$> lookupEnv "CC_PARAMETERS"
  parameters <- either H.failH pure . CC.parseParameters =<< B.readFile parameterPath
  ccData <- linked symbols "cc_ram_data"
  let reserved = map ramAddress (legacyOrdered ++ d3Activation)
      ccKept = [p | p <- ccPublications, let CC.Address address = CC.publicationAddress p,
        address `notElem` reserved]
      convert p = let CC.Address address = CC.publicationAddress p
                   in RamPublication address (CC.publicationOld p) (CC.publicationNew p)
      ccSetup = map convert [p | p <- ccKept, CC.publicationPhase p == CC.Setup]
      ccActivation = map convert [p | p <- ccKept, CC.publicationPhase p == CC.Activation]
      logical = legacyOrdered <> ccSetup <> ccActivation <> d3Activation <> extra
      ordered = [logical !! index | index <- safeOrder]
      allocationEnd = toInteger base + toInteger runtimeAllocationSize
  unless
    ( length logical == publicationCount
        && length safeOrder == publicationCount
        && sortOn id safeOrder == [0 .. publicationCount - 1]
        && length ccSetup == publicationSetupCount - 69
        && length (nub (map ramAddress logical)) == publicationCount
        && all (\p -> toInteger (ramAddress p) + 4 <= toInteger base || toInteger (ramAddress p) >= allocationEnd) logical
    ) $
    H.failH "ram-push: publication order or destinations differ from the combined bank/Classic Chrome contract"
  let allow = [(0x0061c400, 0x00282800), (0x00bf1a00, 0x004d0020), (0x01100000, 0x00080000)]
      metadata =
        B.concat [word32 lo <> word32 spanBytes | (lo, spanBytes) <- allow]
          <> B.replicate (0x80 - 8 * length allow) 0
          <> B.concat [word32 (ramAddress p) <> word32 (ramNew p) | p <- legacyOrdered]
          <> B.replicate (0x400 - 8 * length legacyOrdered) 0
      installerPublications = B.concat
        [ encodePublicationRecord (ramAddress p) (ramOld p) (ramNew p)
            (if index < publicationSetupCount then 0 else 1)
        | (index, p) <- zip [0 :: Int ..] ordered
        ]
  store <- B.readFile (linkedDir ++ "/store.bin")
  unless (B.length store == 60) $
    H.failH "ram-push: exact sixty-byte flash store image required"
  linkedBytes <- B.readFile (linkedDir ++ "/linked.bin")
  unless (B.length linkedBytes == fromIntegral (linkedEnd - linkedStart)) $
    H.failH "ram-push: linked binary extent differs from its symbols"
  publicationsStart <- linked symbols "__ram_push_publications_start"
  publicationsEnd <- linked symbols "__ram_push_publications_end"
  unless
    (publicationsEnd - publicationsStart == fromIntegral (B.length installerPublications)) $
    H.failH "ram-push: installer publication section differs from the ABI size"
  runtime0 <- overlay (B.replicate (fromIntegral size) 0)
    [(linkedStart - base, linkedBytes)]
  runtime1 <- overlay runtime0
    ( [(0, metadata), (0x480, store)]
        <> [(writeAddress w - base, writeNew w) | w <- runtimeWrites]
        <> [(address - base, bytes) | (address, bytes) <- d3Chunks]
        <> [(ccData - base, CC.encodeDataBlock parameters)]
    )
  runtime <- overlay runtime1
    [(publicationsStart - base, installerPublications)]
  when (base /= carveBase && not (null (staleFixedReferences carveBase 0x00bebb20 runtime))) $
    H.failH "ram-push: relinked runtime retains an absolute reference into the fixed bank range"
  runtimeHash <- H.sha256 runtime
  let sourcePaths =
        [ "menu/recipe_bank_arm.S"
        , "menu/recipe_bank_probe.c"
        , "menu/recipe_bank_hook.c"
        , "menu/recipe_bank_storage.c"
        , "menu/recipe_bank_actions.c"
        , "menu/recipe_bank_edit.c"
        , "menu/recipe_bank_diagnostic.c"
        , "menu/recipe_bank_diagnostic.h"
        , "menu/recipe_bank_ui.h"
        , "menu/recipe_bank_edit.h"
        , "menu/recipe_bank_fields.inc"
        , "replacement/bank/lib.rs"
        , "replacement/service/lib.rs"
        , "replacement/service/rb_service_stock.S"
        , "replacement/service/rb_service.h"
        , "replacement/recipe_ui/src/lib.rs"
        , "replacement/owned_menu/src/lib.rs"
        , "replacement/capture/src/lib.rs"
        , "replacement/capture/rb_capture_stock.S"
        , "menu/ram_push_installer.c"
        , "menu/ram_push_abi.h"
        , "menu/recipe_bank_ram.ld"
        , "menu/recipe_bank_allowlist.json"
        , "menu/recipe_bank_boot.S"
        , "menu/recipe_bank_boot.ld"
        , "host/firmware/BuildRecipeBank.hs"
        , "host/firmware/build-recipe-bank-ram.sh"
        , "host/firmware/RamPushLayout.hs"
        , ramNameImagePath
        , ramD3Image4Path
        , ramRc0Image2Path
        , "menu/classic_chrome_ram.S"
        , "menu/classic_chrome_ram_stock.ld"
        , "host/FujiAF/ClassicChrome.hs"
        , "host/FujiAF/ClassicChromeRam.hs"
        , "host/firmware/classic-chrome-liveview-image5.json"
        , "host/firmware/classic-chrome-frames-image5.json"
        , parameterPath
        ] ++ map fst ccInputsRead
  sourceIdentity <- forM sourcePaths $ \path -> do
    digest <- H.sha256 =<< B.readFile path
    pure (path, H.s digest)
  rc0Identity <- forM
    [("recipe_bank_spec", specPath), ("rc0_image4", rc0Id4Path), ("rc0_image5", rc0Id5Path), ("rc0_plain", rc0PlainPath)] $
    \(label, path) -> do
      digest <- H.sha256 =<< B.readFile path
      pure (label, H.s digest)
  rustObjectPath <- maybe (H.failH "ram-push: RB_RUST_ARCHIVE is unset") pure =<< lookupEnv "RB_RUST_ARCHIVE"
  rustObjectDigest <- H.sha256 =<< B.readFile rustObjectPath
  rustFactsPath <- maybe (H.failH "ram-push: RB_RUST_BUILD_FACTS is unset") pure =<< lookupEnv "RB_RUST_BUILD_FACTS"
  rustFactsDigest <- H.sha256 =<< B.readFile rustFactsPath
  let rustIdentity =
        [ ("rust_object_replacement_bank", H.s rustObjectDigest)
        , ("rust_toolchain_replacement_bank", H.s rustFactsDigest)
        ]
  let toolIdentity =
        [ ("build-recipe-bank", H.s recipeBankBuilderDigest)
        , ("ram-push-toolchain", H.s ramPushToolchainDigest)
        ]
  installEntry <- linked symbols "ram_push_install"
  rollbackEntry <- linked symbols "ram_push_rollback"
  statusEntry <- linked symbols "ram_push_status"
  B.writeFile (out ++ "/runtime.bin") runtime
  setFileMode (out ++ "/runtime.bin") 0o600
  B.writeFile (out ++ "/ram-push-manifest.json") (encodeCanonical (H.obj
    [ ("schema", H.n 1)
    , ("source_identity", H.obj (sourceIdentity <> rc0Identity <> rustIdentity <> toolIdentity))
    , ("base", H.s (hex32 base))
    , ("size", H.n (fromIntegral size))
    , ("runtime_sha256", H.s runtimeHash)
    , ("entry", H.obj
        [ ("install", H.s (hex32 installEntry))
        , ("rollback", H.s (hex32 rollbackEntry))
        , ("status", H.s (hex32 statusEntry))
        ])
    , ("publications", H.arr
        [ H.obj
            [ ("index", H.n (fromIntegral logicalIndex))
            , ("phase", H.s (if position < publicationSetupCount then "setup" else "activation"))
            , ("address", H.s (hex32 (ramAddress publication)))
            , ("old", H.s (hex32 (ramOld publication)))
            , ("new", H.s (hex32 (ramNew publication)))
            , ("core", H.s (publicationCore logicalIndex))
            ]
        | (position, logicalIndex, publication) <- zip3 [0 :: Int ..] safeOrder ordered
        ])
    ]))
  setFileMode (out ++ "/ram-push-manifest.json") 0o600

assemble :: Word32 -> [(Word32, B.ByteString)] -> IO B.ByteString
assemble size chunks = do
  let sorted = sortOn fst chunks
      spans = [(offset, toInteger offset + toInteger (B.length bytes)) | (offset, bytes) <- sorted]
  unless (all (\(_, end) -> end <= toInteger size) spans &&
          and [end <= toInteger next | ((_, end), (next, _)) <- zip spans (drop 1 spans)]) $
    H.failH "package: chunk overlap or capacity overflow"
  overlay (B.replicate (fromIntegral size) 0) sorted

overlay :: B.ByteString -> [(Word32, B.ByteString)] -> IO B.ByteString
overlay bytes chunks = foldMBytes bytes chunks
 where
  foldMBytes current [] = pure current
  foldMBytes current ((offset, fresh) : rest) = do
    let start = fromIntegral offset
    unless (toInteger offset + toInteger (B.length fresh) <= toInteger (B.length current)) $
      H.failH "package: overlay leaves image"
    foldMBytes (B.take start current <> fresh <> B.drop (start + B.length fresh) current) rest

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
  image <- if path == "<carve-zero-template>"
    then pure (B.replicate (fromIntegral bytes) 0)
    else B.readFile path
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

-- | The verb pages' node type. For a depth-2 frame of type 9, FUN_01165aa0
-- sets 0x015e64dc = 3 and FUN_011662a0 then draws no left pane, so the slot
-- list and its selection block stay off the verb page; rb_bank_draw_root
-- draws the backdrop FUN_011662a0 also skips for that type.
verbPageNodeType :: Word32
verbPageNodeType = 9

-- | The stock Q menu node (id 0x62, X 0x1b9).
quickMenuNode :: Word32
quickMenuNode = 0x007910a4

-- | The X word (+0x08) of each row's child in a terminated values array, in
-- row order; rows without a child are skipped.
rowChildClasses :: B.ByteString -> Word32 -> B.ByteString -> [Word16]
rowChildClasses image base rows =
  [ fromIntegral (readWord image base (child + 8))
  | k <- [0 .. B.length rows `div` 8 - 2]
  , let child = readWord rows 0 (fromIntegral (8 * k + 4))
  , child /= 0
  ]

-- | The edit screen's rows as indices into the field list: the fields the
-- stock shooting menu's rows open, in its row order, which puts its page-1
-- fields before its page-2 fields; a field the shooting menu does not open
-- follows the nearest field before it in the stock Q menu's row order.
editScreenOrder :: [Word16] -> [Word16] -> [Word16] -> Either String [Int]
editScreenOrder shooting quick properties = do
  order <-
    foldM
      place
      (nub [p | p <- shooting, p `elem` properties])
      (nub [p | p <- quick, p `elem` properties, p `notElem` shooting])
  unless (sort order == sort properties) $
    Left "spec: an edit field is opened by neither the shooting menu nor the Q menu"
  pure [k | p <- order, Just k <- [elemIndex p properties]]
 where
  place order p = case [q | q <- reverse (takeWhile (/= p) quick), q `elem` order] of
    q : _ -> let (before, after) = break (== q) order in Right (before ++ q : p : drop 1 after)
    [] -> Left ("spec: edit field " ++ showHex p "" ++ " has no placed Q-menu predecessor")

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

data MenuNodeKind = NavigationList | TextEntry

encodeCustomNode :: Word32 -> Word32 -> Word16 -> MenuNodeKind -> Word32 -> Word32 -> Word32 -> Word32 -> B.ByteString
encodeCustomNode nodeId nodeType property kind parent values labels w28 =
  B.concat
    [ word32 nodeId
    , word32 nodeType
    , word32 (fromIntegral property)
    , word32 (case kind of NavigationList -> 7; TextEntry -> 0x2a)
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

data InputSlot = InputSlot
  { inputSlotName :: SlotName
  , inputSlotPayload :: BankPayload
  }

-- | Row-based slots overlay the pinned payload defaults: rows name the applied
-- properties and 0xf6, whose value is an offered IMAGE SIZE menu row.
parseSlots :: PayloadDomain -> B.ByteString -> (Word16 -> ConversionTable) -> Value -> IO [InputSlot]
parseSlots domain base tableFor spec = do
  let slots = H.list (H.get spec "slots")
  unless (not (null slots) && length slots <= bankCapacity) $
    H.failH ("spec: 1 to " ++ show bankCapacity ++ " slots required")
  forM slots $ \s -> do
    case H.get s "payload" of
      String _ -> H.fields s "name payload" "bank slot"
      Null -> H.fields s "name rows" "bank slot"
      _ -> H.failH "spec: a slot payload must be a hex string"
    name <- stringField s "name"
    typedName <- either H.failH pure (slotName name)
    payload <- case H.get s "payload" of
      String hexText -> either H.failH pure (decodePayloadHex (H.str (String hexText)))
      _ -> do
        let rows = H.get s "rows"
            keys = H.keys rows
            appliedKeys = [k | k <- keys, hexValue k /= 0xf6]
        sizeKey <- case [k | k <- keys, hexValue k == 0xf6] of
          [k] | sortOn id (map hexValue appliedKeys) == sortOn id (map (fromIntegral :: Word16 -> Word32) appliedProperties) -> pure k
          _ -> H.failH ("slot " ++ name ++ ": rows must name the applied properties " ++ show appliedProperties ++ " and 0xf6, or payload must carry all 0x28 bytes")
        rowPairs <- forM appliedKeys $ \key -> do
          let property = hexValue key
              table = tableFor (fromIntegral property)
          row <- H.integer (H.get rows key) 0 (toInteger (length (tableRows table)) - 1) ("slot " ++ name ++ " row " ++ key)
          pure (property, fromInteger row)
        sizeRow <- H.integer (H.get rows sizeKey) 0 0xff ("slot " ++ name ++ " IMAGE SIZE row")
        recipe <- either H.failH pure (recipeFromRows tableFor rowPairs)
        either H.failH pure (withImageSizeRow domain (fromInteger sizeRow) =<< encodeSlotPayloadOver base recipe)
    typedPayload <- either H.failH pure (canonicalBankPayload payload)
    case payloadUnsafeReason domain typedPayload of
      Just reason -> H.failH ("slot " ++ name ++ ": " ++ reason)
      Nothing -> pure (InputSlot typedName typedPayload)

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

planJson :: BankGeneration -> Word16 -> String -> String -> [Source] -> WritePlan -> B.ByteString -> [B.ByteString] -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Value
planJson (BankGeneration generation) mask blobDigest payloadDigest sources plan blob records values labels parentValues parentLabels = H.obj
  [ ("schema", H.s "recipe-bank-plan-1")
  , ("installable", Bool False)
  , ("emission", H.s (if any ((< 0x00bf1a00) . regionStart . sourceRegion) [s | s <- sources, regionName (sourceRegion s) == "menu_values"]
      then "boot-copied-executable-package" else "pre-bootstrap-diagnostic"))
  , ("generation", H.n (fromIntegral generation))
  , ("slot_valid_mask", H.n (fromIntegral mask))
  , ("bank_file_bytes", H.n (fromIntegral bankFileBytes))
  , ("bank_file_version", H.n 2)
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

hex32 :: Word32 -> String
hex32 value = "0x" ++ replicate (8 - length digits) '0' ++ digits
 where
  digits = showHex value ""
