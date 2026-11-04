module Main (main) where

import Control.Monad (unless)
import Data.Bits (shiftR, testBit, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (isSuffixOf)
import qualified Data.Set as S
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.UpdateContainer as U
import Numeric (readHex, showHex)
import PropertyFields (PropertyId (..))
import Replacement.PropertyService.Admission
import Replacement.PropertyService.Artifact
import Replacement.PropertyService.Classify
import Replacement.PropertyService.Encoding (EncodedTables (..), EncodingError (..), encodeTables)
import Replacement.PropertyService.Placement
import ServiceInventory (Image (..))
import System.Environment (getArgs)

data Symbol = Symbol Word32 Word32 String

symbolRows :: B.ByteString -> [Symbol]
symbolRows text =
  [ Symbol address size (C.unpack name)
  | [addressText, sizeText, _, name] <- map C.words (C.lines text)
  , [(address, "")] <- [readHex (C.unpack addressText)]
  , [(size, "")] <- [readHex (C.unpack sizeText)]
  ]

symbolNamed :: [Symbol] -> (String -> Bool) -> Symbol
symbolNamed symbols chosen = case [symbol | symbol@(Symbol _ _ name) <- symbols, chosen name] of
  [symbol] -> symbol
  _ -> error "test symbol missing or repeated"

carrierStartAddress, carrierEndAddress :: Word32
carrierStartAddress = 0x62c4b0
carrierEndAddress = 0x62d674

testCarrier :: Carrier
testCarrier = Carrier (GuestAddress carrierStartAddress) (GuestAddress carrierEndAddress)

-- | A classification whose encoded tables are the linked ones: owned fields' and slot fields' setter
-- arms start the carrier, delegated setter arms end it, and unassigned properties use both stock fallbacks.
syntheticClassification :: B.ByteString -> [Symbol] -> [Property]
syntheticClassification bytes symbols =
  [ property (PropertyId (fromIntegral index + 1)) (wordAt table index) | index <- [0 .. 644] ]
  where
    Symbol table _ _ = symbolNamed symbols ("10PROPERTIES" `isSuffixOf`)
    Symbol slots _ _ = symbolNamed symbols ("5SLOTS" `isSuffixOf`)
    wordAt base index = U.word32At bytes (fromIntegral (base - carrierStartAddress) + 4 * index)
    owned identifier = Property identifier (GuestAddress 0x61e200) (GuestAddress carrierStartAddress) . Owned
    property identifier word = case word .&. 3 of
      1 -> Property identifier (GuestAddress 0x6243e4) (GuestAddress 0x6301fc) (Owned OwnedUnassigned)
      2 -> owned identifier (OwnedField (field word))
      3 -> owned identifier (OwnedSlotField (slotArm word (wordAt slots (fromIntegral ((word `shiftR` 22) .&. 0x7f)))))
      _ -> Property identifier (GuestAddress 0x61e300) (GuestAddress carrierEndAddress) (Delegated ExitAdjusts)
    widthAt word bit = if testBit word bit then Half else Byte
    rangeAt word lsb = BitRange (fromIntegral ((word `shiftR` lsb) .&. 0xf)) (fromIntegral ((word `shiftR` (lsb + 4)) .&. 0x1f))
    saveAt word bit = if testBit word bit then SaveBank0Previous else NoSave
    field word = FieldArm (BankOffset (fromIntegral ((word `shiftR` 13) .&. 0x1ff))) (widthAt word 2) (rangeAt word 4) (FieldDefault (word `shiftR` 22)) (saveAt word 3)
    slotArm word slot = SlotArm
      (BankBranch (BankOffset (fromIntegral ((word `shiftR` 13) .&. 0x1ff))) (widthAt word 2) (rangeAt word 4)
        (if testBit slot 17 then BankTailByte (TailOffset (fromIntegral ((slot `shiftR` 18) .&. 0x3ff))) else BankConstant (FieldDefault ((slot `shiftR` 18) .&. 0x3ff))))
      (RecordBranch (RecordOffset (fromIntegral (slot .&. 0x3f))) (widthAt slot 6) (rangeAt slot 7))
      (saveAt slot 16)

putWordAt :: Word32 -> Word32 -> Word32 -> B.ByteString -> B.ByteString
putWordAt base address value bytes =
  let at = fromIntegral (address - base) in B.take at bytes <> U.putWord32 value <> B.drop (at + 4) bytes

replaceWord :: Word32 -> Word32 -> B.ByteString -> B.ByteString
replaceWord = putWordAt carrierStartAddress

branchWord :: Bool -> Word32 -> Word32 -> Word32
branchWord link site target = (if link then 0xeb000000 else 0xea000000) .|. (((target - site - 8) `shiftR` 2) .&. 0xffffff)

moveSymbol :: String -> Word32 -> B.ByteString -> B.ByteString
moveSymbol name address text = C.unlines
  [ if C.words line !! 3 == C.pack name then C.unwords (C.pack (pad (showHex address "")) : drop 1 (C.words line)) else line
  | line <- C.lines text, not (C.null line)
  ]
  where
    pad digits = replicate (8 - length digits) '0' ++ digits

expectRefusal :: String -> ArtifactError -> Either ArtifactError a -> IO ()
expectRefusal label expected result = case result of
  Left actual | actual == expected -> putStrLn ("refused " ++ label ++ ": " ++ show actual)
  Left actual -> fail (label ++ ": expected " ++ show expected ++ ", got " ++ show actual)
  Right _ -> fail (label ++ ": admitted")

expect :: (Eq a, Show a) => String -> a -> a -> IO ()
expect label expected actual
  | expected == actual = putStrLn ("checked " ++ label)
  | otherwise = fail (label ++ ": expected " ++ show expected ++ ", got " ++ show actual)

-- | A reviewed data site accepts its own hit and refuses it once the word or the window changes.
dataSiteTests :: IO ()
dataSiteTests = do
  let base = 0x1000000
      address = GuestAddress (base + 64)
      word = 0x0062c500
      imageWith replacements = Image "plain" "" base (B.pack [maybe 0 id (lookup index replacements) | index <- [0 .. 127 :: Int]])
      placed value = [(64 + index, fromIntegral (value `shiftR` (8 * index))) | index <- [0 .. 3]]
      reviewed = imageWith (placed word)
      outcome image = let images = [(ScanPlain, image)] in partitionHits images sites (scanReferences testCarrier images S.empty)
      sites = case siteWindow reviewed address of
        Just window -> [DataSite ScanPlain address word (SHA.sha256 window) HighEntropyData]
        Nothing -> []
  case outcome reviewed of
    ([], [_]) -> putStrLn "accepted the reviewed data site"
    (refused, accepted) -> fail ("reviewed data site: " ++ show (length refused) ++ " refused, " ++ show (length accepted) ++ " accepted")
  case outcome (imageWith (placed (word + 4))) of
    ([_], []) -> putStrLn "refused a changed data-site word"
    _ -> fail "changed data-site word admitted"
  case outcome (imageWith ((40, 1) : placed word)) of
    ([_], []) -> putStrLn "refused a changed data-site window"
    _ -> fail "changed data-site window admitted"

carrierTests :: IO ()
carrierTests = do
  let fieldArm = Owned (OwnedField (FieldArm (BankOffset 0) Byte (BitRange 0 8) (FieldDefault 0) NoSave))
      slotArm = Owned (OwnedSlotField (SlotArm (BankBranch (BankOffset 0x95) Byte (BitRange 4 4) (BankConstant (FieldDefault 3))) (RecordBranch (RecordOffset 0x24) Byte (BitRange 4 4)) SaveBank0Previous))
      delegated = Delegated ExitAdjusts
      classified arms = [Property (PropertyId n) (GuestAddress 0x61e200) (GuestAddress arm) verdict | (n, (arm, verdict)) <- zip [1 ..] arms]
      carriers runs = [Carrier (GuestAddress low) (GuestAddress high) | (low, high) <- runs]
  expect "runs largest first" (carriers [(0x2000, 0x2400), (0x1000, 0x1200)])
    (carrierCandidates (classified [(0x1000, fieldArm), (0x1100, fieldArm), (0x1200, delegated), (0x2000, fieldArm), (0x2400, delegated)]))
  expect "lowest run first on a tie" (carriers [(0x1000, 0x1100), (0x2000, 0x2100)])
    (carrierCandidates (classified [(0x1000, fieldArm), (0x1100, delegated), (0x2000, fieldArm), (0x2100, delegated)]))
  expect "run ending at the setter fallback" (carriers [(0x630000, 0x6301fc)])
    (carrierCandidates (classified [(0x1000, delegated), (0x630000, fieldArm)]))
  expect "slot fields extend an owned run" (carriers [(0x1000, 0x1200), (0x2000, 0x2100)])
    (carrierCandidates (classified [(0x1000, fieldArm), (0x1100, slotArm), (0x1200, delegated), (0x2000, fieldArm), (0x2100, delegated)]))
  expect "128 slot fields fill the slot table" (Right 128) (length . encodedSlots <$> encodeTables (replicate 128 slotArm))
  expect "a 129th slot field is refused" (Left SlotTableFull) (encodeTables (replicate 129 slotArm))

-- | A stock image4 holding the two tables of `properties`, `b 0x630200` before the carrier and zeros.
syntheticImage4 :: [Property] -> B.ByteString
syntheticImage4 properties = foldl (\bytes (address, value) -> putWordAt base address value bytes) (B.replicate 0x14000 0) placed
  where
    base = 0x61c400
    GuestAddress getterTable = getterTableBase
    GuestAddress setterTable = setterTableBase
    placed =
      (carrierStartAddress - 4, branchWord False (carrierStartAddress - 4) 0x630200)
        : concat
          [ [(getterTable + 4 * (n - 1), getter), (setterTable + 4 * (n - 1), setter)]
          | Property (PropertyId n) (GuestAddress getter) (GuestAddress setter) _ <- properties
          ]

placementTests :: [Property] -> LinkedArtifact -> IO ()
placementTests properties artifact = do
  let stock = syntheticImage4 properties
      image4 bytes = Image "image4" "" 0x61c400 bytes
      stage props bytes = stagePlacement props (image4 bytes) [(ScanImage4, image4 bytes)] artifact
      with address value = putWordAt 0x61c400 address value stock
      GuestAddress getterTable = getterTableBase
      GuestAddress setterTable = setterTableBase
      wordIn bytes address = U.word32At bytes (fromIntegral (address - 0x61c400))
      StubAddresses (GuestAddress getterStubAddress) (GuestAddress setterStubAddress) = artifactStubs artifact
      linked = linkedBytes artifact
      carrierOffset = fromIntegral (carrierStartAddress - 0x61c400)
      firstOf predicate = case [p | p <- properties, predicate (propertyClass p)] of
        p : _ -> pure p
        [] -> fail "synthetic classification lacks a class"
  owned <- firstOf (\verdict -> case verdict of Owned (OwnedField _) -> True; _ -> False)
  delegatedProperty <- firstOf (\verdict -> case verdict of Delegated _ -> True; _ -> False)
  let PropertyId ownedId = propertyId owned
      PropertyId delegatedId = propertyId delegatedProperty
  placement <- either (fail . ("synthetic staging refused: " ++) . show) pure (stage properties stock)
  let staged = placementBytes placement
      fill = B.drop (carrierOffset + B.length linked) (B.take (carrierOffset + carrierBytes testCarrier) staged)
  expect "staged length" (B.length stock) (B.length staged)
  expect "carrier starts with the linked bytes" linked (B.take (B.length linked) (B.drop carrierOffset staged))
  expect "carrier fill" (B.concat (replicate (B.length fill `div` 4) (U.putWord32 0xe7f000f0))) fill
  expect "owned getter word" getterStubAddress (wordIn staged (getterTable + 4 * (ownedId - 1)))
  expect "owned setter word" setterStubAddress (wordIn staged (setterTable + 4 * (ownedId - 1)))
  expect "delegated words" (wordIn stock (getterTable + 4 * (delegatedId - 1)), wordIn stock (setterTable + 4 * (delegatedId - 1)))
    (wordIn staged (getterTable + 4 * (delegatedId - 1)), wordIn staged (setterTable + 4 * (delegatedId - 1)))
  expect "bytes outside the carrier and tables" (B.take 0x1d2c stock) (B.take 0x1d2c staged)
  expect "preceding word" (wordIn stock (carrierStartAddress - 4)) (placementPreceding placement)
  expectRefusal "changed delegated word" (PlacementRejected (TableWordDisagrees GetterTable (PropertyId delegatedId)))
    (stage properties (with (getterTable + 4 * (delegatedId - 1)) 0x61e400))
  let intoCarrier = [if propertyId p == propertyId delegatedProperty then p {propertySetterArm = GuestAddress carrierStartAddress} else p | p <- properties]
  expectRefusal "delegated setter word into the carrier" (PlacementRejected (CarrierWordNotOwned (PropertyId delegatedId)))
    (stage intoCarrier (syntheticImage4 intoCarrier))
  let hit site word reference = PlacementRejected (CarrierReferenced [ScanHit ScanImage4 (GuestAddress site) word reference])
      callWord = branchWord True 0x620000 0x62c600
  expectRefusal "word equal to a carrier address" (hit 0x620000 0x62c500 WordInto) (stage properties (with 0x620000 0x62c500))
  expectRefusal "stock BL into the carrier" (hit 0x620000 callWord (BranchInto (GuestAddress 0x62c600))) (stage properties (with 0x620000 callWord))
  expectRefusal "MOVW/MOVT building a carrier address" (hit 0x620000 0xe30c3500 (MovPairInto (GuestAddress 0x62c500)))
    (stage properties (putWordAt 0x61c400 0x620004 0xe3403062 (with 0x620000 0xe30c3500)))
  expectRefusal "PC-relative load into the carrier" (hit 0x62c490 0xe59f0020 (PcRelativeInto (GuestAddress 0x62c4b8)))
    (stage properties (with 0x62c490 0xe59f0020))
  expectRefusal "ADR into the carrier" (hit 0x62c494 0xe28f0020 (PcRelativeInto (GuestAddress 0x62c4bc)))
    (stage properties (with 0x62c494 0xe28f0020))
  expectRefusal "conditional preceding word" (PlacementRejected (PrecedingNotTransfer 0x0a000000))
    (stage properties (with (carrierStartAddress - 4) 0x0a000000))
  let larger = Carrier (GuestAddress 0x624500) (GuestAddress 0x62b000)
      moved = [ if propertyId p == propertyId owned then p {propertySetterArm = GuestAddress 0x624500}
                else if propertyId p == propertyId delegatedProperty then p {propertySetterArm = GuestAddress 0x62b000}
                else p
              | p <- properties ]
      precededImage = putWordAt 0x61c400 0x6244fc (branchWord False 0x6244fc 0x630200) (syntheticImage4 moved)
      literalLoad = 0xe51f0020
  expect "a larger run comes first" (take 2 (carrierCandidates moved)) [larger, testCarrier]
  expectRefusal "larger run that staging admits" (PlacementRejected (LargerCarrierAdmissible larger)) (stage moved precededImage)
  case stage moved (putWordAt 0x61c400 0x62b010 literalLoad precededImage) of
    Right nextRun -> expect "larger run passed over for its literal reference"
      [PassedOver larger (CarrierReferenced [ScanHit ScanImage4 (GuestAddress 0x62b010) literalLoad (PcRelativeInto (GuestAddress 0x62aff8))])]
      (placementPassedOver nextRun)
    Left failure -> fail ("next run refused: " ++ show failure)

-- | One function at 0x1000 holding `code`, admitted alone.
walk :: [Word32] -> Either AdmissionError CodeProof
walk code = admitCode layout (B.concat (map U.putWord32 code))
  where
    layout = CodeLayout
      { layoutBase = GuestAddress 0x1000
      , layoutFunctions = [FunctionExtent (FunctionName "f") (GuestAddress 0x1000) (GuestAddress (0x1000 + 4 * fromIntegral (length code)))]
      , layoutData = []
      , layoutStoreAccessors = []
      , layoutRoots = []
      , layoutTails = []
      }

walkerTests :: IO ()
walkerTests = do
  let refused label site violation code = case walk code of
        Left (AdmissionError (GuestAddress actual) found) | actual == site && found == violation -> putStrLn ("refused " ++ label ++ ": " ++ show violation)
        Left other -> fail (label ++ ": expected " ++ show violation ++ " at 0x" ++ showHex site "" ++ ", got " ++ show other)
        Right _ -> fail (label ++ ": admitted")
  case walk [0xe92d4800, 0xe1a0b00d, 0xe8bd8800] of
    Right _ -> putStrLn "admitted push {r11, lr}; mov r11, sp; pop {r11, pc}"
    Left failure -> fail ("framed return refused: " ++ show failure)
  refused "bx r0" 0x1000 PcWrite [0xe12fff10]
  refused "unbalanced pop" 0x1004 StackUnbalanced [0xe92d4010, 0xe8bd8030]
  refused "recursion" 0x1000 Recursion [0xe92d4800, 0xebfffffd, 0xe8bd8800]
  refused "literal also executed" 0x1004 UnwalkedWord [0xe51f0004, 0xe12fff1e]
  refused "store through a copy of sp" 0x1004 NonStackStore [0xe1a0100d, 0xe5810000, 0xe12fff1e]
  refused "loaded return address" 0x1004 ReturnAddressLost [0xe590e000, 0xe12fff1e]
  refused "store over the saved lr" 0x1004 NonStackStore [0xe92d4800, 0xe58d0004, 0xe8bd8800]
  refused "r11 changed at return" 0x1004 FramePointerLost [0xe1a0b000, 0xe12fff1e]
  refused "moved return address" 0x1004 ReturnAddressLost [0xe1a0e000, 0xe12fff1e]

main :: IO ()
main = do
  arguments <- getArgs
  (bytes, symbolText) <- case arguments of
    [binPath, symbolsPath] -> (,) <$> B.readFile binPath <*> B.readFile symbolsPath
    _ -> fail "usage: property-service-artifact-tests LINKED_BIN SYMBOLS"
  let symbols = symbolRows symbolText
      properties = syntheticClassification bytes symbols
      admit linked names = deriveArtifact properties linked names
      Symbol setStart setSize _ = symbolNamed symbols (== "xm1_property_set")
      Symbol tableAddress _ _ = symbolNamed symbols ("10PROPERTIES" `isSuffixOf`)
      Symbol slotAddress _ _ = symbolNamed symbols ("5SLOTS" `isSuffixOf`)
      Symbol setterStubAddress _ _ = symbolNamed symbols (== "xm1_setter_stub")
      Symbol bankStore8 _ _ = symbolNamed symbols (== "xm1_bank_store8")
      Symbol bankLoad16 _ _ = symbolNamed symbols (== "xm1_bank_load16")
      Symbol slotDefaultsLoad8 _ _ = symbolNamed symbols (== "xm1_slot_defaults_load8")
      Symbol tailDefaultsLoad8 _ _ = symbolNamed symbols (== "xm1_tail_defaults_load8")
      calls = [site | site <- [setStart, setStart + 4 .. setStart + setSize - 4], U.word32At bytes (fromIntegral (site - carrierStartAddress)) .&. 0xff000000 == 0xeb000000]
  firstCall <- case calls of
    site : _ -> pure site
    [] -> fail "xm1_property_set has no BL"
  artifact <- either (fail . ("real artifact refused: " ++) . show) pure (admit bytes symbolText)
  unless (artifactCarrier artifact == testCarrier) (fail "carrier differs")
  putStrLn "admitted the linked artifact against the synthetic classification"
  let flippedAt address = let at = fromIntegral (address - carrierStartAddress) + 1 in B.take at bytes <> B.singleton (B.index bytes at + 1) <> B.drop (at + 1) bytes
  expectRefusal "mutated PROPERTIES byte" TableRejected (admit (flippedAt tableAddress) symbolText)
  expectRefusal "mutated SLOTS byte" SlotTableRejected (admit (flippedAt slotAddress) symbolText)
  let unsaved = [case propertyClass p of Owned (OwnedSlotField arm) -> p {propertyClass = Owned (OwnedSlotField arm {slotSave = NoSave})}; _ -> p | p <- properties]
  expectRefusal "slot fields the SLOTS table does not hold" SlotTableRejected (deriveArtifact unsaved bytes symbolText)
  expectRefusal "setter stub passing r7 as the slot" (EncodingRejected (StubEncoding SetterStub))
    (admit (replaceWord (setterStubAddress + 12) 0xe1a03007 bytes) symbolText)
  expectRefusal "slot-defaults accessor base changed" (EncodingRejected (AccessorEncoding (Accessor SlotDefaultsImage Load8)))
    (admit (replaceWord (slotDefaultsLoad8 + 4) 0xe340c07d bytes) symbolText)
  expectRefusal "tail-defaults accessor storing" (EncodingRejected (AccessorEncoding (Accessor TailDefaultsImage Load8)))
    (admit (replaceWord (tailDefaultsLoad8 + 8) 0xe7cc1000 bytes) symbolText)
  expectRefusal "moved stub" SymbolRejected (admit bytes (moveSymbol "xm1_setter_stub" (setterStubAddress + 4) symbolText))
  expectRefusal "store outside an accessor" (CodeRejected (AdmissionError (GuestAddress firstCall) NonStackStore))
    (admit (replaceWord firstCall 0xe5810000 bytes) symbolText)
  expectRefusal "BL to an outside address" (CodeRejected (AdmissionError (GuestAddress firstCall) (CallOutsideOwned (GuestAddress 0x700000))))
    (admit (replaceWord firstCall (branchWord True firstCall 0x700000) bytes) symbolText)
  expectRefusal "accessor base changed" (EncodingRejected (AccessorEncoding (Accessor BankImage Store8)))
    (admit (replaceWord (bankStore8 + 4) 0xe340c097 bytes) symbolText)
  expectRefusal "accessor variant encoding" (EncodingRejected (AccessorEncoding (Accessor BankImage Load16)))
    (admit (replaceWord (bankLoad16 + 8) 0xe19c0fb0 bytes) symbolText)
  expectRefusal "B into stock" (CodeRejected (AdmissionError (GuestAddress firstCall) (BranchOutsideOwned (GuestAddress 0x61e110))))
    (admit (replaceWord firstCall (branchWord False firstCall 0x61e110) bytes) symbolText)
  expectRefusal "overlong artifact" ExtentRejected (admit (bytes <> B.replicate 4 0) symbolText)
  let pastCarrier = carrierEndAddress + 4
  expectRefusal "artifact past the carrier" ExtentRejected
    (admit (bytes <> B.replicate (fromIntegral (pastCarrier - carrierStartAddress) - B.length bytes) 0) (moveSymbol "__property_service_end" pastCarrier symbolText))
  dataSiteTests
  carrierTests
  placementTests properties artifact
  walkerTests
