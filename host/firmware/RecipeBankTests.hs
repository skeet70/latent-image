{-# LANGUAGE OverloadedStrings #-}
-- Host-only consumer contracts with synthetic literal-wire fixtures.
module Main where

import Control.Monad (forM_)
import qualified Data.ByteString as B
import Data.Char (ord)
import Data.Word (Word32, Word8)
import System.FilePath ((</>))

import FujiAF.RecipeBank
import qualified FujiAF.Host as H

main :: IO ()
main = do
  -- a spec row the pinned table does not carry, and a table that never
  -- terminates, are the two ways a compile could silently name a wrong look
  H.withTemporary "fuji-af-recipe-bank-" $ \root -> do
    let imagePath = root </> "id4.bin"
        base = 0x0061c400 :: Word32
        tableAddress = 0x00775428 :: Word32
        tableImage =
          B.concat
            [ B.replicate (fromIntegral (tableAddress - base)) 0
            , row 0 0
            , row 1 1
            , row 2 2
            , terminator
            , B.replicate 0x100 0
            ]
        row :: Word32 -> Word32 -> B.ByteString
        row code effective = word32 code <> word32 0x7ffffffe <> word32 effective
        terminator = word32 0xffff
    B.writeFile imagePath tableImage
    image <- B.readFile imagePath
    table <- either H.failH pure (readConversionTable base image tableAddress)
    H.ensure (rowToCode table 2 == Right (StoredCode 2)) "row 2 did not resolve to code 2"
    H.ensure (refused (rowToCode table 3)) "a row past the terminator was admitted"
    H.ensure (refused (rowToCode table (-1))) "a negative row was admitted"
    H.ensure (codeToRow table (StoredCode 1) == Right 1) "code 1 did not resolve to row 1"
    H.ensure (refused (codeToRow table (StoredCode 9))) "a code the table does not carry was admitted"
    H.ensure
      (refused (readConversionTable base (B.take (fromIntegral (tableAddress - base) + 12) tableImage) tableAddress))
      "a table with no terminator inside the image was admitted"

  -- payload and name boundaries the storage lane's buffers depend on
  let recipe = recipeOf 1 3 4 4 3 2 3 0xb 5
      pastMask = recipeOf 0 0 0 0 0 0 0 0 0x1f
  encodedPayload <- either H.failH pure (encodeSlotPayload recipe)
  H.ensure
    ( encodedPayload
        == B.pack
          [ 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x80,
            0x60, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x60,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x0b, 0x03, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00
          ]
    )
    "the nine stored codes are not at their payload offsets"
  H.ensure (refused (encodeSlotPayload pastMask)) "a code past its payload mask was admitted"
  H.ensure (refused (encodeName "RECIPE 01 XYZ")) "a 13-character name was admitted"
  H.ensure (refused (encodeName "WARM\233")) "a non-ASCII name was admitted"
  H.ensure
    (encodeName "RECIPE 01 XY" == Right (B.pack [82, 0, 69, 0, 67, 0, 73, 0, 80, 0, 69, 0, 32, 0, 48, 0, 49, 0, 32, 0, 88, 0, 89, 0, 0, 0]))
    "a 12-character name has the wrong UTF-16LE representation"
  let fullName = B.pack [65, 0, 66, 0, 67, 0, 68, 0, 69, 0, 70, 0, 71, 0, 72, 0, 73, 0, 74, 0, 75, 0, 76, 0, 0, 0]
  H.ensure (decodeName fullName == Right "ABCDEFGHIJKL") "a full-width terminated name was refused"
  H.ensure (refused (decodeName (putByte fullName 24 1))) "a full-width name without its zero terminator was admitted"

  -- appends must refuse a source array that is not the stock shape
  let values = encodeValuesArray [(0, 0), (1, 0)]
  H.ensure (refused (appendValuesPairs (B.take 8 values) [(1, 0)])) "a stock values array without its terminator was admitted"
  labels <- either H.failH pure (encodeLabelsArray [[1, 2, 3, 4]])
  H.ensure (refused (appendLabelQuadruples (B.take 16 labels) [[5, 6, 7, 8]])) "a stock labels array without its terminator was admitted"

  -- a branch the linker could not reach is a placement error, not a byte
  H.ensure (not (refused (armBranchWord 0x0071fa9c 0x011dcca0))) "an in-range A32 branch was refused"
  H.ensure (refused (armBranchWord 0 33554440)) "a forward delta past the A32 limit was admitted"
  H.ensure (refused (armBranchWord 33554428 0)) "a backward delta past the A32 limit was admitted"
  H.ensure (not (refused (armBranchWord 33554424 0))) "the backward A32 limit was refused"
  H.ensure (refused (armBranchWord 2 7)) "an unaligned A32 delta was admitted"
  H.ensure (refused (armBranchWord 0xd50a48 0x20000000)) "an unreachable A32 target was admitted"
  H.ensure (armTailBranchWord 0x0071fa9c 0x011dcca0 == Right 0xea2af47f) "the detach tail branch changed the link register"
  H.ensure (armBranchWord 0x0071fa9c 0x011dcca0 == Right 0xeb2af47f) "the linked branch lost its link bit"
  H.ensure (refused (thumbBranchWord 0x0011c100 0x08000000)) "an unreachable Thumb target was admitted"

  -- the plan the container worker applies: capacity, overlap and region checks
  let region = Region "gap" 0x0011ac5a0 0x100 "synthetic"
      good = WritePlan [region] [PlanWrite "gap" 0x0011ac5a0 (B.replicate 8 0) (B.replicate 8 1) "blob"] []
      outside = WritePlan [region] [PlanWrite "gap" 0x0011ac6a0 (B.replicate 8 0) (B.replicate 8 1) "blob"] []
      overlapping =
        WritePlan
          [region]
          [ PlanWrite "gap" 0x0011ac5a0 (B.replicate 8 0) (B.replicate 8 1) "a"
          , PlanWrite "gap" 0x0011ac5a4 (B.replicate 8 0) (B.replicate 8 1) "b"
          ]
          []
      unknown = WritePlan [region] [PlanWrite "elsewhere" 0x0011ac5a0 (B.replicate 8 0) (B.replicate 8 1) "blob"] []
  H.ensure (validatePlan good == Right ()) "an in-region plan was refused"
  H.ensure (refused (validatePlan outside)) "a write leaving its declared region was admitted"
  H.ensure (refused (validatePlan overlapping)) "overlapping writes were admitted"
  H.ensure (refused (validatePlan unknown)) "a write to an unknown region was admitted"
  let otherRegion = Region "other" 0x0011ac5a0 0x100 "synthetic"
      interleaved = WritePlan [region, otherRegion]
        [ PlanWrite "gap" 0x0011ac5a0 (B.replicate 16 0) (B.replicate 16 1) "a"
        , PlanWrite "other" 0x0011ac5a4 (B.replicate 1 0) (B.replicate 1 1) "b"
        , PlanWrite "gap" 0x0011ac5a8 (B.replicate 1 0) (B.replicate 1 1) "c"
        ] []
      changedLength = good {planWrites = [PlanWrite "gap" 0x0011ac5a0 (B.replicate 4 0) (B.replicate 8 1) "blob"]}
  H.ensure (refused (validatePlan interleaved)) "writes overlapping across region names were admitted"
  H.ensure (refused (validatePlan changedLength)) "a current/new span length mismatch was admitted"
  let plainLoad = PlainLoadAddress 0x01100000
      plainWindow = (PlainFlashOffset 0x005a0000, PlainFlashOffset 0x00900000)
      menuRegion = Region "menu" 0x011acd70 0x400 "synthetic"
      menuWrite = PlanWrite "menu" 0x011acd70 (B.replicate 44 0) (B.replicate 44 1) "menu node"
      validMapping = [(menuRegion, PlainFlashOffset 0x0064cd70)]
      shiftedMapping = [(menuRegion, PlainFlashOffset 0x0066cd70)]
      badEnd = PlanWrite "menu" 0x0145ffff (B.replicate 2 0) (B.replicate 2 1) "past window"
      aliasRegion = Region "alias" 0x011acd74 0x100 "synthetic"
      aliasWrite = PlanWrite "alias" 0x011acd74 (B.replicate 4 0) (B.replicate 4 1) "alias node"
  H.ensure (validatePlainCarrier plainLoad plainWindow validMapping [menuWrite] == Right ()) "the loader-consistent carrier mapping was refused"
  H.ensure (refused (validatePlainCarrier plainLoad plainWindow shiftedMapping [menuWrite])) "a shifted RAM/flash mapping was admitted"
  H.ensure (refused (validatePlainCarrier plainLoad plainWindow validMapping [badEnd])) "a span crossing the loader window end was admitted"
  H.ensure
    (refused (validatePlainCarrier plainLoad plainWindow (validMapping ++ [(aliasRegion, PlainFlashOffset 0x0064cd74)]) [menuWrite, aliasWrite]))
    "cross-region flash aliases were admitted"

  bankFileTests
  putStrLn "Recipe-bank v1 migration, v2 reload, safety, winner and plan checks passed"

bankFileTests :: IO ()
bankFileTests = do
  let generation = BankGeneration 7
      oldWire = literalV1 generation
      newWire = literalV2 generation
      migrated = v1State generation
      resident = v2State generation
      valid = classifyBankFile syntheticDomain
  H.ensure (bankFileBytes == 1024 && B.length newWire == 1024) "v2 does not occupy 1024 bytes"
  oldDecoded <- either H.failH pure (decodeBankFile oldWire)
  case oldDecoded of
    DecodedV1 old -> do
      H.ensure (v1Generation old == generation) "v1 generation changed"
      H.ensure
        (v1Slots old == [V1Populated (named "A") (payload savedA), V1Populated (named "PORTRA 400") (payload savedB), V1Unpopulated, V1Unpopulated, V1Unpopulated, V1Unpopulated, V1Populated (named "RECIPE 01 XY") (payload savedG)])
        "v1 populated names, payloads or empty slots changed"
      H.ensure (migrateV1 old == migrated) "v1 migration changed saved settings or initialization policy"
    DecodedV2 _ -> H.failH "a literal v1 file decoded as v2"
  H.ensure (decodedState oldDecoded == migrated) "v1 reload did not initialize working=saved, NONE, DISABLE and invalid base"
  H.ensure (decodeBankFile newWire == Right (DecodedV2 resident)) "literal v2 state did not reload"
  H.ensure (encodeBankFile resident == Right newWire) "v2 emission differs from the independent literal wire"
  H.ensure (valid oldWire == ValidFile oldDecoded) "safe synthetic v1 was refused"
  H.ensure (valid newWire == ValidFile (DecodedV2 resident)) "safe synthetic v2 was refused"
  reloaded <- either H.failH (pure . decodedState) (decodeBankFile newWire)
  case bankRecordAt C2 (bankRecords reloaded) of
    Populated name saved working -> do
      H.ensure (slotNameText name == "PORTRA 400") "a reloaded record lost its name"
      H.ensure (bankPayloadContents saved == savedB) "reload replaced saved with working"
      H.ensure (bankPayloadContents working == workingB && working /= saved) "reload discarded distinct working settings"
    Unpopulated -> H.failH "a populated v2 record disappeared"

  migration <- either (H.failH . show) pure (nextWrite (BankWinner FileA migrated))
  let migratedWrite = migrated {bankGeneration = nextWriteGeneration migration}
  migrationWire <- either H.failH pure (encodeBankFile migratedWrite)
  H.ensure
    (B.length migrationWire == 1024 && B.take 8 migrationWire == B.pack [0x46, 0x4a, 0x52, 0x42, 2, 0, 0x40, 0])
    "migration emitted a v1 file"
  H.ensure (decodeBankFile migrationWire == Right (DecodedV2 migratedWrite)) "migration did not durably preserve names, settings and initialization policy"
  H.ensure
    (chooseBank (Just (valid oldWire)) (Just (valid migrationWire)) == BankWinner FileB migratedWrite)
    "a completed v2 migration did not supersede the retained v1 winner"

  forM_ [0 .. 1023] $ \len -> do
    let torn = B.take len newWire
    H.ensure (refused (decodeBankFile torn)) ("a truncated v2 file was admitted at length " ++ show len)
    H.ensure
      (chooseBank (Just (valid oldWire)) (Just (valid torn)) == BankWinner FileA migrated)
      ("a truncated v2 replacement displaced v1 at length " ++ show len)
  forM_ [0 .. 511] $ \len ->
    H.ensure (refused (decodeBankFile (B.take len oldWire))) ("a truncated v1 file was admitted at length " ++ show len)
  forM_ [oldWire, newWire] $ \wire -> do
    H.ensure (refused (decodeBankFile (wire <> B.singleton 0))) "an overlong file was admitted"
    H.ensure (refused (decodeBankFile (putByte wire 0x18 (B.index wire 0x18 + 1)))) "a corrupt checksum was admitted"
    H.ensure (refused (decodeBankFile (putByte wire 0x20 (B.index wire 0x20 + 1)))) "a corrupt body was admitted"

  let replaceWord bytes offset value = replaceBytes bytes offset (word32 value)
      structuralFaults =
        [ ("magic", putByte newWire 0 0)
        , ("version", putByte newWire 4 1)
        , ("header size", putByte newWire 6 0x20)
        , ("firmware", putByte newWire 8 151)
        , ("capacity", putByte newWire 10 8)
        , ("name units", putByte newWire 12 13)
        , ("payload size", putByte newWire 14 41)
        , ("body size", replaceWord newWire 20 781)
        , ("populated mask", putByte newWire 29 0x80)
        , ("reserved halfword", putByte newWire 30 1)
        , ("selection domain", putByte newWire 0x20 8)
        , ("auto-update domain", putByte newWire 0x21 2)
        , ("base validity domain", putByte newWire 0x22 2)
        , ("valid base without mode", putByte newWire 0x23 0)
        , ("excluded base mode", putByte newWire 0x23 5)
        , ("custom selection without base", replaceBytes (replaceBytes newWire 0x22 (B.pack [0, 0])) 0x40 zeroPayload)
        , ("selection of empty slot", putByte newWire 0x20 3)
        , ("ENABLE with distinct working", putByte newWire 0x21 1)
        , ("reserved header byte", putByte newWire 0x24 1)
        , ("nonzero invalid base", replaceBytes newWire 0x20 (B.pack [0, 0, 0, 0]))
        , ("invalid base with mode", replaceBytes (replaceBytes newWire 0x20 (B.pack [0, 0, 0, 1])) 0x40 zeroPayload)
        , ("base unbound bits", putByte newWire (0x40 + 0x27) 4)
        , ("saved unbound bits", putByte newWire (0x68 + 26 + 0x27) 4)
        , ("working unbound bits", putByte newWire (0x68 + 66 + 0x27) 4)
        , ("nonzero empty record", putByte newWire (0x68 + 2 * 106) 1)
        , ("non-ASCII name", putByte newWire (0x68 + 1) 1)
        , ("full-width name terminator", putByte newWire (0x68 + 6 * 106 + 24) 1)
        , ("trailing padding", putByte newWire 0x34e 1)
        ]
  forM_ structuralFaults $ \(label, wire) ->
    H.ensure (refused (decodeBankFile (repairChecksum wire))) ("a checksum-valid v2 structural fault was admitted: " ++ label)
  forM_ [("payload unbound bits", 0x20 + 0x27), ("name terminator", 0x138 + 6 * 26 + 24), ("padding", 511)] $ \(label, offset) ->
    H.ensure
      (refused (decodeBankFile (repairChecksum (putByte oldWire offset 4))))
      ("a checksum-valid v1 structural fault was admitted: " ++ label)

  H.ensure (refused (encodeBankFile resident {bankSelection = Custom C3})) "encoding selected an empty slot"
  H.ensure (refused (encodeBankFile resident {bankBase = BaseInvalid})) "encoding allowed custom selection without a base"
  H.ensure (refused (encodeBankFile resident {bankAutoUpdate = Enabled})) "encoding ENABLE discarded working differences"
  H.ensure (refused (bankRecordsFromList [])) "an empty record collection was admitted"
  H.ensure (refused (bankRecordsFromList (replicate 8 Unpopulated))) "an eighth record was admitted"
  H.ensure (refused (slotName "RECIPE 01 XYZ") && refused (slotName "WARM\233")) "an invalid typed slot name was admitted"
  H.ensure (refused (bankPayload (B.take 39 savedA))) "a short typed payload was admitted"
  H.ensure (refused (bankPayload (putByte savedA 0x27 4))) "unbound bits entered a typed payload"
  let boundE9 = putByte savedA 0x0a 0xe0
  H.ensure
    (canonicalBankPayload (putByte boundE9 0x27 0xff) == Right (payload (putByte boundE9 0x27 3)))
    "payload canonicalization lost bound bits or kept unbound bits"
  H.ensure (bankPayloadContents (payload boundE9) == boundE9) "the bound +0x0a mode bits were dropped"
  forM_ [(1, P), (2, S), (3, A), (4, M)] $ \(modeByte, mode) ->
    H.ensure
      (decodeBankFile (repairChecksum (putByte newWire 0x23 modeByte)) == Right (DecodedV2 resident {bankBase = BaseValid mode (payload basePayload)}))
      ("eligible base mode did not reload: " ++ show mode)
  let noSelection = resident {bankSelection = NothingSelected}
      noBase = noSelection {bankBase = BaseInvalid}
      cleanRecords = records [Populated (named "A") (payload savedA) (payload savedA), Unpopulated, Unpopulated, Unpopulated, Unpopulated, Unpopulated, Unpopulated]
      enabled = resident {bankSelection = Custom C1, bankAutoUpdate = Enabled, bankRecords = cleanRecords}
  forM_ [noSelection, noBase, enabled] $ \state -> do
    wire <- either H.failH pure (encodeBankFile state)
    H.ensure (decodeBankFile wire == Right (DecodedV2 state)) "an admitted selection/base/ENABLE state failed reload"

  let unsafePayloads =
        [ ("walked code", putByte savedA 0x24 0x0f)
        , ("aspect zero", putByte savedA 0x1e 0)
        , ("unoffered size pair", putByte (putByte savedA 0x1a 0x40) 0x1e 0x60)
        , ("quality/RAW mismatch", putByte savedA 0x06 0x40)
        ]
  forM_ unsafePayloads $ \(fault, badPayload) ->
    forM_ [("base", 0x40), ("saved", 0x68 + 26), ("working", 0x68 + 66)] $ \(place, offset) -> do
      let wire = repairChecksum (replaceBytes newWire offset badPayload)
          classified = valid wire
      H.ensure (not (refused (decodeBankFile wire))) ("a semantic fault was misclassified as structural: " ++ place ++ " " ++ fault)
      H.ensure (isUnsafe classified) ("unsafe " ++ place ++ " payload was admitted: " ++ fault)
      H.ensure (chooseBank (Just classified) Nothing == CannotMigrate FileA) "an unsafe generation became a bank winner"
  let unsafeV1 = repairChecksum (putByte (literalV1 (BankGeneration 8)) (0x20 + 0x1e) 0)
      unsafeClass = valid unsafeV1
  H.ensure (not (refused (decodeBankFile unsafeV1))) "the unsafe v1 aspect fixture is not structurally valid"
  H.ensure (isUnsafe unsafeClass) "v1 aspect zero was repaired or admitted"
  H.ensure
    (chooseBank (Just (valid newWire)) (Just unsafeClass) == CannotMigrate FileB)
    "an older safe bank hid a newer unsafe v1 generation"
  H.ensure
    (chooseBank (Just unsafeClass) (Just (valid newWire)) == CannotMigrate FileA)
    "a newer unsafe v1 generation did not block migration from A"
  H.ensure
    (valid (repairChecksum (putByte oldWire (0x20 + 2 * 40 + 0x1e) 0)) == ValidFile oldDecoded)
    "an unpopulated v1 payload affected migration"

  let versions = [(literalV1, v1State), (literalV2, v2State)]
  forM_ versions $ \(wireA, stateA) ->
    forM_ versions $ \(wireB, stateB) -> do
      H.ensure
        (chooseBank (Just (valid (wireA (BankGeneration 8)))) (Just (valid (wireB generation))) == BankWinner FileA (stateA (BankGeneration 8)))
        "the newer A generation did not win across versions"
      H.ensure
        (chooseBank (Just (valid (wireA generation))) (Just (valid (wireB (BankGeneration 8)))) == BankWinner FileB (stateB (BankGeneration 8)))
        "the newer B generation did not win across versions"
      H.ensure
        (chooseBank (Just (valid (wireA generation))) (Just (valid (wireB generation))) == BankWinner FileA (stateA generation))
        "an equal-generation tie did not choose A across versions"
  let unsafeTie = valid (repairChecksum (putByte oldWire (0x20 + 0x1e) 0))
      safeNewer = v2State (BankGeneration 8)
  H.ensure (chooseBank (Just unsafeTie) (Just (valid newWire)) == CannotMigrate FileA) "an unsafe A tie fell back to B"
  H.ensure (chooseBank (Just (valid newWire)) (Just unsafeTie) == BankWinner FileA resident) "an unsafe B tie displaced safe A"
  H.ensure
    (chooseBank (Just unsafeTie) (Just (valid (literalV2 (BankGeneration 8)))) == BankWinner FileB safeNewer)
    "an older unsafe v1 displaced a newer safe v2"
  let corrupt = valid B.empty
  H.ensure (chooseBank Nothing Nothing == NoBank) "missing files did not produce NoBank"
  H.ensure (chooseBank (Just corrupt) Nothing == NoCandidate) "a present corrupt file was treated as missing"
  H.ensure (chooseBank (Just corrupt) (Just corrupt) == NoCandidate) "corrupt files became candidates"
  H.ensure (chooseBank (Just corrupt) (Just (valid newWire)) == BankWinner FileB resident) "corrupt A displaced valid B"
  H.ensure (chooseBank (Just (valid oldWire)) Nothing == BankWinner FileA migrated) "a lone safe v1 file did not load"
  H.ensure (chooseBank Nothing (Just (valid oldWire)) == BankWinner FileB migrated) "a lone safe v1 B file did not load"

  H.ensure (nextWrite NoBank == Right (NextWrite FileA (BankGeneration 1))) "a new bank did not start at A generation 1"
  H.ensure (nextWrite NoCandidate == Right (NextWrite FileA (BankGeneration 1))) "a bank without candidates did not start at A generation 1"
  H.ensure (nextWrite (BankWinner FileA migrated) == Right (NextWrite FileB (BankGeneration 8))) "v1 migration would overwrite its winner"
  H.ensure (nextWrite (BankWinner FileB resident) == Right (NextWrite FileA (BankGeneration 8))) "a B winner did not target A at winner+1"
  H.ensure (nextWrite (CannotMigrate FileA) == Left BankCannotMigrate) "an unsafe bank obtained a write plan"
  forM_ versions $ \(wire, state) -> do
    let finalGeneration = BankGeneration 0xffffffff
        finalState = state finalGeneration
        penultimate = state (BankGeneration 0xfffffffe)
    forM_ [(FileA, Just (valid (wire finalGeneration)), Nothing), (FileB, Nothing, Just (valid (wire finalGeneration)))] $ \(fileId, fileA, fileB) -> do
      let outcome = chooseBank fileA fileB
      H.ensure (outcome == BankWinner fileId finalState) "the final representable generation could not load"
      H.ensure (nextWrite outcome == Left GenerationExhausted) "generation exhaustion wrapped or authorized a write"
    H.ensure
      (nextWrite (BankWinner FileA penultimate) == Right (NextWrite FileB finalGeneration))
      "the last nonwrapping increment was refused"
    H.ensure (not (refused (encodeBankFile finalState))) "the final representable v2 generation could not be emitted"

-- Synthetic payload/domain provenance: authored for this host contract, using
-- wire field locations from custom-settings.fields.json, not firmware values.
-- Only the listed table domains and composite pairs are constrained here.
syntheticDomain :: PayloadDomain
syntheticDomain =
  PayloadDomain
    { domainTables =
        [ (0xfe, map FieldCode [0, 1, 3, 0xb])
        , (0xf6, map FieldCode [0, 1, 2])
        , (0xf9, map FieldCode [0, 1, 2, 3])
        , (0xfa, map FieldCode [0, 1, 2, 3, 4])
        , (0x1d, map FieldCode [0, 1, 2])
        ]
    , domainSizePairs =
        [ SizePair (FieldCode 0) (FieldCode 1)
        , SizePair (FieldCode 1) (FieldCode 2)
        , SizePair (FieldCode 2) (FieldCode 1)
        ]
    , domainQualityRaw =
        [ QualityRaw (FieldCode 0) (FieldCode 0)
        , QualityRaw (FieldCode 1) (FieldCode 0)
        , QualityRaw (FieldCode 2) (FieldCode 2)
        , QualityRaw (FieldCode 3) (FieldCode 2)
        , QualityRaw (FieldCode 4) (FieldCode 1)
        ]
    }

zeroPayload, savedA, savedB, savedG, workingB, basePayload :: B.ByteString
zeroPayload = B.replicate 40 0
savedA =
  B.pack
    [ 0x34, 0x12, 0x78, 0x56, 0, 0, 0, 0,
      0, 0, 0, 0, 0, 0, 0, 0,
      0, 0, 0, 0, 0, 0, 0, 0,
      0, 0, 0, 0, 0, 0, 0x20, 0,
      0, 0, 0, 0, 0, 0, 0, 0
    ]
savedB = putByte savedA 0x24 3
savedG = putByte savedA 0x24 0xb
workingB = putByte (putByte savedB 0x17 0x60) 0x24 1
basePayload = putByte savedA 0x20 0xb

v1State :: BankGeneration -> BankState
v1State generation =
  BankState generation NothingSelected Disabled BaseInvalid $
    records
      [ Populated (named "A") (payload savedA) (payload savedA)
      , Populated (named "PORTRA 400") (payload savedB) (payload savedB)
      , Unpopulated, Unpopulated, Unpopulated, Unpopulated
      , Populated (named "RECIPE 01 XY") (payload savedG) (payload savedG)
      ]

v2State :: BankGeneration -> BankState
v2State generation =
  BankState generation (Custom C2) Disabled (BaseValid P (payload basePayload)) $
    records
      [ Populated (named "A") (payload savedA) (payload savedA)
      , Populated (named "PORTRA 400") (payload savedB) (payload workingB)
      , Unpopulated, Unpopulated, Unpopulated, Unpopulated
      , Populated (named "RECIPE 01 XY") (payload savedG) (payload savedG)
      ]

named :: String -> SlotName
named = fixture . slotName

payload :: B.ByteString -> BankPayload
payload = fixture . bankPayload

records :: [BankRecord] -> BankRecords
records = fixture . bankRecordsFromList

fixture :: Either String a -> a
fixture = either (error . ("invalid test fixture: " ++)) id

-- Literal v1 wire keeps nonempty bytes in slots whose populated bits are clear.
literalV1 :: BankGeneration -> B.ByteString
literalV1 (BankGeneration generation) =
  repairChecksum $
    B.concat
      [ B.pack [0x46, 0x4a, 0x52, 0x42, 1, 0, 0x20, 0, 150, 0, 7, 0, 12, 0, 40, 0]
      , word32 generation
      , B.pack [0xce, 1, 0, 0, 0, 0, 0, 0, 0x43, 0, 0, 0]
      , B.concat [savedA, savedB, savedA, savedB, savedG, savedA, savedG]
      , B.concat (map literalName ["A", "PORTRA 400", "GHOST", "COOL", "WARM BATH", "NIGHT", "RECIPE 01 XY"])
      , B.replicate 18 0
      ]

literalV2 :: BankGeneration -> B.ByteString
literalV2 (BankGeneration generation) =
  repairChecksum $
    B.concat
      [ B.pack [0x46, 0x4a, 0x52, 0x42, 2, 0, 0x40, 0, 150, 0, 7, 0, 12, 0, 40, 0]
      , word32 generation
      , B.pack [0x0e, 3, 0, 0, 0, 0, 0, 0, 0x43, 0, 0, 0]
      , B.pack [2, 0, 1, 1]
      , B.replicate 28 0
      , basePayload
      , literalName "A", savedA, savedA
      , literalName "PORTRA 400", savedB, workingB
      , B.replicate (4 * 106) 0
      , literalName "RECIPE 01 XY", savedG, savedG
      , B.replicate 178 0
      ]

literalName :: String -> B.ByteString
literalName text = B.pack (concatMap (\c -> [fromIntegral (ord c), 0]) text) <> B.replicate (26 - 2 * length text) 0

repairChecksum :: B.ByteString -> B.ByteString
repairChecksum bytes = replaceBytes bytes 0x18 (word32 (specChecksum bytes))

-- The wire checksum is the complement of the sum of complemented bytes with
-- the checksum word zeroed.
specChecksum :: B.ByteString -> Word32
specChecksum bytes = 0xffffffff - B.foldl' (\acc byte -> acc + 255 - fromIntegral byte) 0 zeroed
 where
  zeroed = replaceBytes bytes 0x18 (B.replicate 4 0)

putByte :: B.ByteString -> Int -> Word8 -> B.ByteString
putByte bytes offset byte = replaceBytes bytes offset (B.singleton byte)

replaceBytes :: B.ByteString -> Int -> B.ByteString -> B.ByteString
replaceBytes bytes offset replacement = B.take offset bytes <> replacement <> B.drop (offset + B.length replacement) bytes

isUnsafe :: BankFileClass -> Bool
isUnsafe UnsafeFile {} = True
isUnsafe _ = False

recipeOf :: Word8 -> Word8 -> Word8 -> Word8 -> Word8 -> Word8 -> Word8 -> Word8 -> Word8 -> Recipe
recipeOf dr hi sh col sharp nr wb iso film =
  either (const (error "fixture: unrepresentable recipe")) id $
    recipeFromPairs
      [ (0x104, StoredCode dr)
      , (0x110, StoredCode hi)
      , (0x111, StoredCode sh)
      , (0x10e, StoredCode col)
      , (0x10f, StoredCode sharp)
      , (0xef, StoredCode nr)
      , (0xed, StoredCode wb)
      , (0x141, StoredCode iso)
      , (0xfe, StoredCode film)
      ]

refused :: Either String a -> Bool
refused (Left _) = True
refused (Right _) = False

word32 :: Word32 -> B.ByteString
word32 w = B.pack [fromIntegral ((w `div` (256 ^ i)) `mod` 256) | i <- [0 .. 3 :: Int]]
