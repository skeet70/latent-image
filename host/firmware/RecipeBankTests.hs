{-# LANGUAGE OverloadedStrings #-}
-- The recipe-bank codec's consumer-visible boundaries: the file a consumer
-- decodes and the file the builder emits, checked against literal bytes, the
-- slot and name offsets those consumers allocate against, and the refusals the
-- storage lane, the container worker and the arm build depend on. The literal
-- fixture is the interop anchor the C kernel's cases read; its checksum is
-- computed here from the written formula rather than from the module's own
-- encoder, so no assertion is a same-host round trip. No pinned artefact and
-- no camera.
module Main where

import Data.Bits (complement, shiftL, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (nub)
import Data.Word (Word16, Word32, Word8)
import System.FilePath ((</>))

import FujiAF.RecipeBank
import qualified FujiAF.RecipeBankFields as F
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
  payload <- either H.failH pure (encodeSlotPayload recipe)
  H.ensure
    ( payload
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

  -- the bank file, decoded from literal bytes and re-emitted byte for byte;
  -- these are also the bytes the C kernel's cases read
  let names = ["A", "PORTRA 400", "RECIPE 01 XY", "COOL", "WARM BATH", "NIGHT", "X"]
      filmCodes = [0x0, 0x3, 0x1, 0xb, 0xc, 0x0, 0x3] :: [Word8]
      drCodes = [0, 1, 2, 3, 0, 1, 2] :: [Word8]
      hiCodes = [0, 3, 0, 3, 0, 3, 0] :: [Word8]
      shCodes = [2, 4, 2, 4, 2, 4, 2] :: [Word8]
      colCodes = [0, 4, 0, 4, 0, 4, 0] :: [Word8]
      sharpCodes = [2, 3, 2, 3, 2, 3, 2] :: [Word8]
      putField p fldMask shift code bytes =
        putByte bytes p ((bytes `B.index` p .&. complement fldMask) .|. (code `shiftL` shift))
      slotPayload k =
        foldr
          (\(p, fldMask, shift, codes) acc -> putField p fldMask shift (codes !! k) acc)
          defaultsPayload
          [ (0x24, 0x0f, 0 :: Int, filmCodes)
          , (0x05, 0xc0, 6, drCodes)
          , (0x17, 0xe0, 5, hiCodes)
          , (0x09, 0xe0, 5, shCodes)
          , (0x07, 0xe0, 5, colCodes)
          , (0x08, 0xe0, 5, sharpCodes)
          ]
      slots = [FileSlot name (slotPayload k) | (k, name) <- zip [0 ..] names]
      mask = 0x43 :: Word16
  blob <- either H.failH pure (encodeFileBlob slots)
  H.ensure (B.length blob == 462) "the blob is not the 462 bytes consumers allocate"
  literalFile <- either H.failH pure (literalBank 7 mask blob)
  (header, decoded) <- either H.failH pure (decodeBankFile literalFile)
  H.ensure (bankGeneration header == 7) "the literal generation did not decode"
  H.ensure (bankSlotValid header == mask) "the literal slot mask did not decode"
  H.ensure (map fileSlotName decoded == names) "a literal name did not decode to its string"
  H.ensure (map fileSlotPayload decoded == map fileSlotPayload slots) "a literal payload did not survive decoding"
  emitted <- either H.failH pure (encodeBankFile (BankHeader 7 mask) blob)
  H.ensure (emitted == literalFile) "the emitted file differs from the literal file"
  H.ensure (B.take 4 literalFile == "FJRB") "the literal file has the wrong magic"

  -- slot and name offsets: a one-slot misalignment must change attribution
  let nameBlockStart = 0x20 + 7 * 0x28
  H.ensure (B.take 2 (B.drop nameBlockStart literalFile) == B.pack [65, 0]) "the name block does not start at 0x138"
  H.ensure
    ((fileSlotName <$> decodeSlotAt literalFile 0) == Right "A")
    "slot 0 did not decode from its own offset"
  H.ensure
    ((fileSlotName <$> decodeSlotAt literalFile 6) == Right "X")
    "slot 6 did not decode from its own offset"
  let flipAt i = B.take i literalFile `B.append` B.pack [B.index literalFile i + 1] `B.append` B.drop (i + 1) literalFile
      setAt i w = B.take i literalFile `B.append` B.pack [w] `B.append` B.drop (i + 1) literalFile
  H.ensure (refused (decodeBankFile (flipAt (nameBlockStart - 1)))) "a flipped last payload byte was admitted"
  H.ensure (refused (decodeBankFile (flipAt nameBlockStart))) "a flipped first name byte was admitted"

  -- refusals a consumer depends on
  H.ensure (refused (decodeBankFile (B.take 511 literalFile))) "a short file was admitted"
  H.ensure (refused (decodeBankFile (literalFile `B.append` B.pack [0]))) "a long file was admitted"
  H.ensure (refused (decodeBankFile (flipAt 0x18))) "a flipped checksum byte was admitted"
  let repairChecksum bytes =
        B.take 0x18 bytes <> word32 (specChecksum bytes) <> B.drop 0x1c bytes
      fullWidthSlot = 2
      fullWidthTerminator = nameBlockStart + fullWidthSlot * nameBytes + 24
  H.ensure
    (refused (decodeBankFile (repairChecksum (setAt (0x20 + 0x27) (B.index literalFile (0x20 + 0x27) .|. 0x04)))))
    "payload unbound bits set were admitted"
  H.ensure (refused (decodeBankFile (repairChecksum (flipAt fullWidthTerminator)))) "a checksum-valid nonzero full-width name terminator was admitted"
  H.ensure (refused (decodeBankFile (repairChecksum (flipAt 511)))) "nonzero trailing padding was admitted"
  H.ensure
    (not (refused (encodeBankFile (BankHeader 0xffffffff mask) blob)))
    "the final representable generation was refused"
  H.ensure
    (refused (nextGeneration (Just 0xffffffff) Nothing))
    "a generation past the final representable one was admitted"
  H.ensure (refused (encodeBankFile (BankHeader 1 0x8000) blob)) "a slot mask past capacity was admitted"

  -- the authoritative field map: the same generated table the storage kernel
  -- carries, and the unbound mask the codec and the kernel both apply
  let walk = F.payloadFields
  H.ensure (length walk == F.payloadFieldCount) "the walk table does not match its recorded count"
  H.ensure (length F.editFields == F.editFieldCount) "the edit table does not match its recorded count"
  H.ensure
    (map F.fieldProperty (take 2 walk) == [0x1cb, 0x1cc])
    "the walk table does not carry the two halfword properties first"
  H.ensure
    ( all
        (\f -> F.fieldBytes f == 2 && F.fieldMask f == 0xffff && F.fieldShift f == 0 && F.fieldTable f == 0)
        (take 2 walk)
    )
    "a halfword row is not full width, unshifted and table-less"
  let rowFor p = [f | f <- walk, F.fieldProperty f == p]
  H.ensure (map F.fieldProperty walk == nub (map F.fieldProperty walk)) "a walk row binds a property twice"
  H.ensure
    (map (\f -> (F.fieldOffset f, F.fieldMask f, F.fieldShift f)) (rowFor 0x1a) == [(0x27, 0x02, 1)])
    "property 0x1a is not the one-bit +0x27 bit 1 row"
  H.ensure
    (map (\f -> (F.fieldOffset f, F.fieldMask f, F.fieldShift f)) (rowFor 0x1e8) == [(0x27, 0x01, 0)])
    "property 0x1e8 is not the one-bit +0x27 bit 0 row"
  H.ensure
    (all (\f -> fromIntegral (F.fieldOffset f) + fromIntegral (F.fieldBytes f) <= payloadBytes) walk)
    "a walk row leaves the payload"
  H.ensure (all (\f -> F.fieldMask f <= 0xff) F.editFields) "an edit row is not a byte field"
  H.ensure
    ( map (\f -> (fieldProperty f, fieldOffset f, fieldMask f, fieldShift f, fieldTable f)) recipeFields
        == [ (0x104, 0x05, 0xc0, 6, 0x00775428)
           , (0x110, 0x17, 0xe0, 5, 0x00775e24)
           , (0x111, 0x09, 0xe0, 5, 0x0077643c)
           , (0x10e, 0x07, 0xe0, 5, 0x0077a810)
           , (0x10f, 0x08, 0xe0, 5, 0x00778620)
           , (0xef, 0x10, 0xe0, 5, 0x00779f10)
           , (0xed, 0x21, 0x0f, 0, 0x0077a60c)
           , (0x141, 0x20, 0x1f, 0, 0x0077af48)
           , (0xfe, 0x24, 0x0f, 0, 0x0077ac30)
           ]
    )
    "the oracle's nine field placements are not the pinned map rows"
  let boundE9Payload = putByte defaultsPayload 0x0a 0xe0
      e9Blob = either (const B.empty) id (encodeFileBlob [FileSlot "A" boundE9Payload])
      -- 0xfc is the unbound mask at +0x27 (bits 0-1 carry properties 0x1e8 and
      -- 0x1a), so this fixture sets exactly the bits the encode path must drop
      unboundBlob = either (const B.empty) id (encodeFileBlob [FileSlot "A" (putByte defaultsPayload 0x27 0xfc)])
  H.ensure (B.index e9Blob 0x0a == 0xe0) "the encode path dropped the bound +0x0a bits"
  H.ensure (B.index unboundBlob 0x27 == 0x00) "the encode path kept unbound +0x27 bits"
  H.ensure (not (refused (decodeFileBlob e9Blob))) "a payload carrying the bound +0x0a bits was refused"
  H.ensure (refused (decodeFileBlob (putByte e9Blob 0x27 0x04))) "an unbound +0x27 bit was admitted"
  H.ensure (not (refused (decodeFileBlob (putByte e9Blob 0x27 0x03)))) "a payload carrying both bound +0x27 bits was refused"
  putStrLn "Recipe-bank file fixture, offsets, refusals and plan checks passed"

-- | The pinned per-slot defaults row (custom-settings.fields.json): the
-- literal payload every fixture slot is built from, with the film and dynamic
-- range codes varied per slot from their stock table rows.
defaultsPayload :: B.ByteString
defaultsPayload =
  B.pack
    [ 0x00, 0x00, 0x00, 0x00, 0x7f, 0x22, 0x22, 0x09,
      0x49, 0x49, 0x69, 0x49, 0x49, 0x09, 0x09, 0x09,
      0x49, 0x29, 0x09, 0x69, 0x09, 0x09, 0xa9, 0x40,
      0x43, 0x22, 0x03, 0x02, 0x82, 0x82, 0x8f, 0x8f,
      0x88, 0x30, 0x63, 0x60, 0x30, 0x07, 0x40, 0x00
    ]

putByte :: B.ByteString -> Int -> Word8 -> B.ByteString
putByte bytes i w = B.take i bytes `B.append` B.pack [w] `B.append` B.drop (i + 1) bytes

-- | The file assembled from literal parts with the checksum computed from the
-- written formula: the complement of the sum of the eight-bit complemented
-- bytes over the 512 bytes with the checksum field zeroed. The kernel computes
-- the same value for these bytes (its cases read this fixture).
literalBank :: Word32 -> Word16 -> B.ByteString -> Either String B.ByteString
literalBank gen mask blob
  | B.length blob /= 462 = Left "fixture: blob is not 462 bytes"
  | otherwise =
      let header =
            B.concat
              [ "FJRB"
              , le16 1
              , le16 0x20
              , le16 150
              , le16 7
              , le16 12
              , le16 0x28
              , word32 gen
              , word32 462
              , word32 0
              , le16 mask
              , le16 0
              ]
          padded = header `B.append` blob `B.append` B.replicate (512 - 0x20 - 462) 0
          checksum = specChecksum padded
       in Right (B.take 0x18 padded `B.append` word32 checksum `B.append` B.drop 0x1c padded)

specChecksum :: B.ByteString -> Word32
specChecksum bytes = complement32 (B.foldl' step 0 (zeroed bytes))
 where
  zeroed b = B.take 0x18 b `B.append` B.replicate 4 0 `B.append` B.drop 0x1c b
  step acc w = acc + (0xff - fromIntegral w)
  complement32 v = 0xffffffff - v

decodeSlotAt :: B.ByteString -> Int -> Either String FileSlot
decodeSlotAt file k = do
  (header, slots) <- decodeBankFile file
  if bankSlotValid header /= 0 && k < length slots
    then Right (slots !! k)
    else Left "fixture: slot out of range"

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

le16 :: Word16 -> B.ByteString
le16 w = B.pack [fromIntegral (w .&. 0xff), fromIntegral (w `div` 256)]
