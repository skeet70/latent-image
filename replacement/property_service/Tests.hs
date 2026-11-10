module Main (main) where

import Control.Monad (forM_, unless)
import qualified ArmElf as ELF
import Data.Bits (shiftR, testBit, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (isSuffixOf)
import qualified Data.Map.Strict as M
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
carrierStartAddress = 0x62c120
carrierEndAddress = 0x62fbfc

testCarrier :: Carrier
testCarrier = Carrier (GuestAddress carrierStartAddress) (GuestAddress carrierEndAddress)

-- | A classification whose encoded tables are the linked ones: owned setter arms start the carrier,
-- unassigned properties use both stock fallbacks, read-only fields the setter's and write-only
-- pairs the getter's.
syntheticClassification :: B.ByteString -> [Symbol] -> [Property]
syntheticClassification bytes symbols =
  [ property (PropertyId (fromIntegral index + 1)) (wordAt table index) | index <- [0 .. 644] ]
  where
    Symbol table _ _ = symbolNamed symbols ("10PROPERTIES" `isSuffixOf`)
    Symbol slots _ _ = symbolNamed symbols ("5SLOTS" `isSuffixOf`)
    Symbol specials _ _ = symbolNamed symbols ("8SPECIALS" `isSuffixOf`)
    wordAt base index = U.word32At bytes (fromIntegral (base - carrierStartAddress) + 4 * index)
    owned identifier = Property identifier (GuestAddress 0x61e200) (GuestAddress carrierStartAddress) . Owned
    property identifier word = case (word .&. 3, (word `shiftR` 2) .&. 0x3f) of
      (1, _) -> Property identifier (GuestAddress 0x6243e4) (GuestAddress 0x6301fc) (Owned OwnedUnassigned)
      (2, _) -> owned identifier (OwnedField (field word (FieldDefaultOnly (fieldDefault word)) NoHook))
      (3, _) -> owned identifier (OwnedSlotField (slotArm word (wordAt slots (fromIntegral ((word `shiftR` 22) .&. 0x7f)))))
      (_, 0) -> error ("kind-0 word with index 0 for property " ++ show identifier)
      (_, index) -> special identifier (wordAt specials (2 * fromIntegral (index - 1))) (wordAt specials (2 * fromIntegral (index - 1) + 1))
    special identifier layout behaviour = case layout .&. 3 of
      1 -> Property identifier (GuestAddress 0x61e200) (GuestAddress 0x6301fc) (Owned (OwnedReadOnlyField (field layout (FieldDefaultOnly (DefaultConstant value)) NoHook)))
      2 -> Property identifier (GuestAddress 0x6243e4) (GuestAddress carrierStartAddress)
        (Owned (OwnedWriteOnlyPair (PairArm (BankOffset (fromIntegral ((layout `shiftR` 13) .&. 0x1ff))) (BankOffset (fromIntegral ((behaviour `shiftR` 17) .&. 0x1ff))) value (saveAt layout 3))))
      _ -> owned identifier (OwnedField (field layout source hook))
      where
        value = (layout `shiftR` 22) .&. 0x1ff
        config at = ConfigOffset (fromIntegral ((behaviour `shiftR` at) .&. 0x1ff))
        source = case behaviour .&. 3 of
          1 -> FieldStock (stockOf ((behaviour `shiftR` 2) .&. 0xf)) value
          2 -> FieldConfig (config 6) value
          _ -> FieldDefaultOnly (DefaultConstant value)
        hook = case (behaviour `shiftR` 15) .&. 3 of
          1 -> Bank0Bracket
          2 -> ConfigGatedResave (config 17)
          _ -> NoHook
    stockOf code = case code of
      1 -> Stock61df44
      2 -> Stock61d8a4
      3 -> Stock61d59c
      4 -> Stock61d618
      5 -> Stock61d5f0
      6 -> Stock61d65c Profile2
      7 -> Stock61d65c Profile3
      _ -> Stock61d65c Profile4
    widthAt word bit = if testBit word bit then Half else Byte
    rangeAt word lsb = BitRange (fromIntegral ((word `shiftR` lsb) .&. 0xf)) (fromIntegral ((word `shiftR` (lsb + 4)) .&. 0x1f))
    saveAt word bit = if testBit word bit then SaveBank0Previous else NoSave
    fieldDefault word
      | testBit word 31 = DefaultTail (TailOffset (fromIntegral ((word `shiftR` 22) .&. 0xff))) (widthAt word 2)
      | otherwise = DefaultConstant ((word `shiftR` 22) .&. 0x1ff)
    field word source hook = FieldArm (BankOffset (fromIntegral ((word `shiftR` 13) .&. 0x1ff))) (widthAt word 2) (rangeAt word 4) source (saveAt word 3) hook
    slotArm word slot = SlotArm
      (BankBranch (BankOffset (fromIntegral ((word `shiftR` 13) .&. 0x1ff))) (widthAt word 2) (rangeAt word 4)
        (if testBit slot 17 then BankTailByte (TailOffset (fromIntegral ((slot `shiftR` 18) .&. 0x3ff))) else BankConstant ((slot `shiftR` 18) .&. 0x3ff)))
      (RecordBranch (RecordOffset (fromIntegral (slot .&. 0x3f))) (widthAt slot 6) (rangeAt slot 7))
      (saveAt slot 16)
      (if slot `shiftR` 28 == 0 then SlotDefaultsRecord else SlotStock (stockOf (slot `shiftR` 28)))
      (if testBit word 29 then MirrorRecord0 else NoMirror)

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
  let image = Image "image4" "" 0x61c400 B.empty
      hit = ScanHit ScanPlain (GuestAddress 0x1100000) 0x62c124 WordInto
      dead = hit {hitImage = ScanImage4, hitSite = GuestAddress 0x620000}
      slotArm = Owned (OwnedSlotField (SlotArm (BankBranch (BankOffset 0x95) Byte (BitRange 4 4) (BankConstant 3)) (RecordBranch (RecordOffset 0x24) Byte (BitRange 4 4)) SaveBank0Previous SlotDefaultsRecord NoMirror))
  expect "fixed carrier without arm alignment" (Just stage4Carrier) (chooseCarrier image [] [])
  expect "external carrier word blocks placement" Nothing (chooseCarrier image [] [hit])
  expect "reservation crossing carrier start blocks placement" Nothing (chooseCarrier image [] [hit {hitWord = 0x62c11c}])
  expect "retired internal reference does not reserve carrier" (Just stage4Carrier) (chooseCarrier image [] [dead])
  expect "128 slot fields fill table" (Right 128) (length . encodedSlots <$> encodeTables (replicate 128 slotArm))
  expect "129 slot fields refused" (Left SlotTableFull) (encodeTables (replicate 129 slotArm))

syntheticImage4 :: [Property] -> B.ByteString
syntheticImage4 _ = B.replicate 0x16000 0

placementTests :: [Property] -> LinkedArtifact -> IO ()
placementTests properties artifact = do
  let stock = syntheticImage4 properties
      image bytes = Image "image4" "" 0x61c400 bytes
      stage bytes = do
        scan <- either (Left . PlacementRejected) Right (scanHull (image bytes) [(ScanImage4, image bytes)] properties)
        stagePlacement properties (image bytes) scan artifact
      EntryAddresses (GuestAddress getter) (GuestAddress setter) = artifactEntries artifact
      changed address value = putWordAt 0x61c400 address value
  placement <- either (fail . show) pure (stage stock)
  let staged = placementBytes placement
      word :: Word32 -> Word32
      word address = U.word32At staged (fromIntegral (address - 0x61c400))
  expect "getter tail adapter" (branchWord False 0x61e110 getter) (word 0x61e110)
  expect "setter tail adapter" (branchWord False 0x624694 setter) (word 0x624694)
  expect "retired getter dispatch" 0xe7f000f0 (word 0x61e128)
  expect "retired setter tail" 0xe7f000f0 (word 0x630200)
  expect "unaltered wrapper" 0 (word 0x6245f8)
  expect "valid staged image" (Right ()) (validateStagedImage4 (placementScan placement) artifact stock staged)
  let refusal label address value = expectRefusal label StagingRejected (validateStagedImage4 (placementScan placement) artifact stock (changed address value staged))
  refusal "getter adapter redirected" 0x61e110 (branchWord False 0x61e110 setter)
  refusal "setter adapter redirected" 0x624694 (branchWord False 0x624694 getter)
  refusal "getter adapter BL" 0x61e110 (branchWord True 0x61e110 getter)
  refusal "setter adapter BL" 0x624694 (branchWord True 0x624694 setter)
  refusal "retained wrapper overwritten" 0x6245f8 1
  refusal "getter tail restored" 0x624400 0xe12fff1e
  refusal "setter tail restored" 0x630200 0xe12fff1e
  refusal "stock table restored" 0x61e12c 0x61eb40
  let reservedStock = changed 0x61c400 0x61eb40 stock
  reserved <- either (fail . show) pure (stage reservedStock)
  expectRefusal "reserved literal overwritten" StagingRejected
    (validateStagedImage4 (placementScan reserved) artifact reservedStock (changed 0x61eb44 1 (placementBytes reserved)))
  let target = GuestAddress 0x61eb40
      site = 0x61c400
      call = branchWord True site 0x61eb40
  expectRefusal "external branch into retired body"
    (PlacementRejected (ExecutableReference (ScanHit ScanImage4 (GuestAddress site) call (BranchInto target))))
    (stage (changed site call stock))
  case stage (changed 0x61c400 0x62c124 stock) of
    Left (PlacementRejected (CarrierNotCandidate _)) -> pure ()
    other -> fail ("external carrier reference admitted: " ++ either show (const "success") other)

-- | Admit one callable function with retained leaves and bracket sites.
walkWith :: [(GuestAddress, StackOffset)] -> [(GuestAddress, GuestAddress, StackOffset)] -> [Word32] -> Either AdmissionError CodeProof
walkWith = walkWithStores []

walkWithStores :: [GuestAddress] -> [(GuestAddress, StackOffset)] -> [(GuestAddress, GuestAddress, StackOffset)] -> [Word32] -> Either AdmissionError CodeProof
walkWithStores stores leaves sites code = admitCode layout (B.concat (map U.putWord32 code))
  where
    layout = CodeLayout
      { layoutBase = GuestAddress 0x1000
      , layoutFunctions = [FunctionExtent (FunctionName "f") (GuestAddress 0x1000) (GuestAddress (0x1000 + 4 * fromIntegral (length code)))]
      , layoutMappings = [ArmSpan (AddressSpan (GuestAddress 0x1000) (GuestAddress (0x1000 + 4 * fromIntegral (length code))))]
      , layoutLiteralPointers = []
      , layoutData = []
      , layoutStoreSites = stores
      , layoutStockLeaves = leaves
      , layoutStockSites = sites
      }

walk :: [Word32] -> Either AdmissionError CodeProof
walk = walkWith [] []

walkerTests :: IO ()
walkerTests = do
  let refusedBy label site violation outcome = case outcome of
        Left (AdmissionError (GuestAddress actual) found) | actual == site && found == violation -> putStrLn ("refused " ++ label ++ ": " ++ show violation)
        Left other -> fail (label ++ ": expected " ++ show violation ++ " at 0x" ++ showHex site "" ++ ", got " ++ show other)
        Right _ -> fail (label ++ ": admitted")
      refused label site violation code = refusedBy label site violation (walk code)
      admitted label code = case walk code of
        Right _ -> putStrLn ("admitted " ++ label)
        Left failure -> fail (label ++ ": " ++ show failure)
      leaf = GuestAddress 0x61d59c
  let callback = FunctionExtent (FunctionName "recipe-getter") (GuestAddress 0x2000) (GuestAddress 0x2008)
      callbackLayout = CodeLayout
        { layoutBase = GuestAddress 0x1000
        , layoutFunctions = [FunctionExtent (FunctionName "caller") (GuestAddress 0x1000) (GuestAddress 0x100c)]
        , layoutMappings = [ArmSpan (AddressSpan (GuestAddress 0x1000) (GuestAddress 0x100c))]
        , layoutLiteralPointers = []
        , layoutData = []
        , layoutStoreSites = []
        , layoutStockLeaves = []
        , layoutStockSites = []
        }
      callbackCode = B.concat (map U.putWord32 [0xe92d4800, 0xeb0003fd, 0xe8bd8800])
  case admitCodeWithCallbacks [callback] callbackLayout callbackCode of
    Left failure -> fail ("bound callback refused: " ++ show failure)
    Right _ -> pure ()
  refusedBy "callback requires an admitted dependency" 0x1004 (CallOutsideOwned (GuestAddress 0x2000))
    (admitCode callbackLayout callbackCode)
  refusedBy "callback interior is not callable" 0x1004 (CallOutsideOwned (GuestAddress 0x2004))
    (admitCodeWithCallbacks [callback] callbackLayout (putWordAt 0x1000 0x1004 0xeb0003fe callbackCode))
  case walkWithStores [GuestAddress 0x1000] [] [] [0xe5801000, 0xe12fff1e] of
    Right _ -> pure ()
    Left failure -> fail ("admitted boundary store refused: " ++ show failure)
  refusedBy "unlisted store beside admitted site" 0x1004 NonStackStore
    (walkWithStores [GuestAddress 0x1000] [] [] [0xe5801000, 0xe5801000, 0xe12fff1e])
  refusedBy "multiple store at single-store boundary" 0x1000 NonStackStore
    (walkWithStores [GuestAddress 0x1000] [] [] [0xe8800006, 0xe12fff1e])
  case walk [0xe92d4800, 0xe1a0b00d, 0xe8bd8800] of
    Right _ -> putStrLn "admitted push {r11, lr}; mov r11, sp; pop {r11, pc}"
    Left failure -> fail ("framed return refused: " ++ show failure)
  refused "bx r0" 0x1000 PcWrite [0xe12fff10]
  refused "unbalanced pop" 0x1004 StackUnbalanced [0xe92d4010, 0xe8bd8030]
  refused "recursion" 0x1000 Recursion [0xe92d4800, 0xebfffffd, 0xe8bd8800]
  refused "literal also executed" 0x1000 LiteralNotData [0xe51f0004, 0xe12fff1e]
  refused "store through copied sp without an allocated frame" 0x1004 NonStackStore [0xe1a0100d, 0xe5810000, 0xe12fff1e]
  forM_ [0x40,0x44] $ \offset ->
    admitted ("frame-local aggregate store at offset " ++ show offset)
      [0xe92d4ff0,0xe28db01c,0xe24dd064,0xe28d3000 .|. offset,0xe3570000,0xe8835005,0xe28dd064,0xe8bd8ff0]
  admitted "chained local stack aliases"
    [0xe24dd020,0xe1a0300d,0xe283200c,0xe2421004,0xe8810005,0xe28dd020,0xe12fff1e]
  admitted "local alias derived from r11"
    [0xe92d4800,0xe1a0b00d,0xe24dd010,0xe24b3008,0xe5830004,0xe28dd010,0xe8bd8800]
  forM_ [0xe8835005,0xe9835005,0xe8035005,0xe9035005] $ \store ->
    admitted ("bounded local multiple-store mode " ++ showHex store "")
      [0xe24dd030,0xe28d3010,store,0xe28dd030,0xe12fff1e]
  admitted "local byte store at the frame's final byte"
    [0xe24dd010,0xe28d300f,0xe5c30000,0xe28dd010,0xe12fff1e]
  admitted "bounded stack-alias loop reaches a finite proof"
    [0xe24dd010,0xe1a0300d,0xe2833001,branchWord False 0x100c 0x1008]
  refused "incoming caller pointer has no local stack proof" 0x1004 NonStackStore
    [0xe24dd020,0xe5803000,0xe28dd020,0xe12fff1e]
  refused "unknown multiple-store base" 0x1004 NonStackStore
    [0xe24dd020,0xe8835005,0xe28dd020,0xe12fff1e]
  refused "immediate write destroys a local alias" 0x100c NonStackStore
    [0xe24dd020,0xe28d3004,0xe3a03000,0xe8835005,0xe28dd020,0xe12fff1e]
  refused "load destroys a local alias" 0x100c NonStackStore
    [0xe24dd020,0xe28d3004,0xe59d3000,0xe8835005,0xe28dd020,0xe12fff1e]
  refused "multiple load destroys a local alias" 0x100c NonStackStore
    [0xe24dd020,0xe28d3004,0xe89d0008,0xe8835005,0xe28dd020,0xe12fff1e]
  refused "loading the return address destroys an lr stack alias" 0x1018 NonStackStore
    [0xe92d4000,0xe24dd010,0xe28de014,0xe28dd010,0xe49de004,0xe24dd010,0xe50e0004,0xe28dd010,0xe12fff1e]
  refused "one proved branch cannot credit an unknown branch" 0x100c NonStackStore
    [0xe24dd020,0xe3500000,0x028d3004,0xe8835005,0xe28dd020,0xe12fff1e]
  admitted "different proved aliases at a branch join"
    [0xe24dd020,0xe1a0300d,0xe3500000,0x128d3004,0xe8835005,0xe28dd020,0xe12fff1e]
  refused "deallocated alias does not revive after reallocation" 0x1010 NonStackStore
    [0xe24dd020,0xe28d3004,0xe28dd020,0xe24dd020,0xe8835005,0xe28dd020,0xe12fff1e]
  refused "multiple-store span exceeds the live frame" 0x1008 NonStackStore
    [0xe24dd010,0xe28d3004,0xe8835005,0xe28dd010,0xe12fff1e]
  refused "decrementing multiple store precedes the live frame" 0x1008 NonStackStore
    [0xe24dd010,0xe1a0300d,0xe9030005,0xe28dd010,0xe12fff1e]
  refused "halfword span exceeds the live frame" 0x1008 NonStackStore
    [0xe24dd010,0xe28d300f,0xe1c300b0,0xe28dd010,0xe12fff1e]
  refused "register-indexed alias store has no bounded offset" 0x1008 NonStackStore
    [0xe24dd010,0xe1a0300d,0xe7830004,0xe28dd010,0xe12fff1e]
  refused "multiple-store writeback is not an admitted alias transfer" 0x1008 NonStackStore
    [0xe24dd020,0xe28d3004,0xe8a30005,0xe28dd020,0xe12fff1e]
  forM_ [0x10,0x14] $ \offset ->
    refused ("local alias overlaps saved r11/lr at offset " ++ show offset) 0x100c NonStackStore
      [0xe92d4800,0xe24dd010,0xe28d3000 .|. offset,0xe8830001,0xe28dd010,0xe8bd8800]
  forM_ [(0xe28d3004,0xe5830000),(0xe28d4004,0xe5840000)] $ \(derive,store) ->
    refusedBy ("call destroys local alias used by " ++ showHex store "") 0x1014 NonStackStore
      (walkWith [(leaf,StackOffset (-4))] []
        [0xe92d4800,0xe1a0b00d,0xe24dd010,derive,branchWord True 0x1010 0x61d59c,store,0xe28dd010,0xe8bd8800])
  refused "loaded return address" 0x1004 ReturnAddressLost [0xe590e000, 0xe12fff1e]
  refused "store over the saved lr" 0x1004 NonStackStore [0xe92d4800, 0xe58d0004, 0xe8bd8800]
  refused "r11 changed at return" 0x1004 FramePointerLost [0xe1a0b000, 0xe12fff1e]
  refused "moved return address" 0x1004 ReturnAddressLost [0xe1a0e000, 0xe12fff1e]
  case walk [0xe92d4800, 0xe3a0e000, 0xe8bd8800] of
    Right _ -> putStrLn "admitted lr as scratch while the return address is saved"
    Left failure -> fail ("lr scratch refused: " ++ show failure)
  refused "stock-frame pop without owned prologue" 0x1004 StackUnbalanced [0xe1a0000c, 0xe8bd8830]
  case walkWith [(leaf, StackOffset (-4))] [] [0xe92d4800, branchWord True 0x1004 0x61d59c, 0xe8bd8800] of
    Right proof -> expect "a stock leaf call adds the leaf's stack" (Just (StackOffset (-12))) (M.lookup (GuestAddress 0x1000) (proofComposed proof))
    Left failure -> fail ("stock leaf call refused: " ++ show failure)
  case walkWith [(leaf, StackOffset (-4))] [] [0xe92d4800, 0xe8bd4800, branchWord False 0x1008 0x61d59c] of
    Right proof -> expect "a tail branch to a stock leaf after the frame pop" (Just (StackOffset (-8))) (M.lookup (GuestAddress 0x1000) (proofComposed proof))
    Left failure -> fail ("stock leaf tail branch refused: " ++ show failure)
  refusedBy "a tail branch to a stock leaf inside the frame" 0x1004 StackUnbalanced
    (walkWith [(leaf, StackOffset (-4))] [] [0xe92d4800, branchWord False 0x1004 0x61d59c])
  case walk [0xe92d4800, 0xe1a0b00d, 0xe24dd008, 0xe50b0004, 0xe28dd008, 0xe8bd8800] of
    Right _ -> putStrLn "admitted a store through r11 into the frame"
    Left failure -> fail ("frame store through r11 refused: " ++ show failure)
  refused "a store through r11 over the saved lr" 0x100c NonStackStore [0xe92d4800, 0xe1a0b00d, 0xe24dd008, 0xe58b0004, 0xe28dd008, 0xe8bd8800]
  refused "a store through r11 below sp" 0x100c NonStackStore [0xe92d4800, 0xe1a0b00d, 0xe24dd008, 0xe50b000c, 0xe28dd008, 0xe8bd8800]
  let bracket = [(GuestAddress 0x1004, GuestAddress 0x61da3c, StackOffset (-32))]
  refusedBy "bracket at minus 40" 0x1008 StockCallBelowEntry
    (walkWith [] [(GuestAddress 0x1008, GuestAddress 0x61da3c, StackOffset (-32))]
      [0xe92d49f8, 0xe24dd008, branchWord True 0x1008 0x61da3c, 0xe28dd008, 0xe8bd89f8])
  case walkWith [] bracket [0xe92d49f8, branchWord True 0x1004 0x61da3c, 0xe8bd89f8] of
    Right _ -> pure ()
    Left failure -> fail ("owned bracket call refused: " ++ show failure)
  refusedBy "bracket from unlisted site" 0x1000 (CallOutsideOwned (GuestAddress 0x61da3c))
    (walkWith [] bracket [branchWord True 0x1000 0x61da3c, 0xe12fff1e])

literalPoolTests :: IO ()
literalPoolTests = do
  let spanAt low high = AddressSpan (GuestAddress low) (GuestAddress high)
      function name low high = FunctionExtent (FunctionName name) (GuestAddress low) (GuestAddress high)
      layout = CodeLayout
        { layoutBase = GuestAddress 0x1000
        , layoutFunctions = [function "caller" 0x1000 0x1018, function "expect" 0x1018 0x1024, function "panic" 0x1024 0x1030, function "trap" 0x1030 0x103c]
        , layoutMappings = [ArmSpan (spanAt 0x1000 0x1010), DataSpan (spanAt 0x1010 0x1018), ArmSpan (spanAt 0x1018 0x103c), DataSpan (spanAt 0x103c 0x1054)]
        , layoutLiteralPointers = [OwnedReadonly (spanAt 0x103c 0x1054)]
        , layoutData = [(GuestAddress 0x103c, GuestAddress 0x1054)]
        , layoutStoreSites = []
        , layoutStockLeaves = []
        , layoutStockSites = []
        }
      code = B.concat (map U.putWord32
        [ 0xe59f0008, 0xe3a0100b, 0xe59f2004, branchWord True 0x100c 0x1018
        , 0x103c, 0x1048
        , 0xe92d4800, 0xe1a0b00d, branchWord True 0x1020 0x1024
        , 0xe92d4800, 0xe1a0b00d, branchWord True 0x102c 0x1030
        , 0xe92d4800, 0xe1a0b00d, branchWord False 0x1038 0x1038
        ]) <> C.pack "tail byte\0\0\0" <> B.replicate 12 0
      admit = admitCode layout
      changed address value = putWordAt 0x1000 address value code
      rejected label outcome = case outcome of
        Left _ -> putStrLn ("refused " ++ label)
        Right _ -> fail (label ++ ": admitted")
  proof <- either (fail . show) pure (admit code)
  expect "emitted LDR pair before a nonreturning call admits data pointers"
    (S.fromList [GuestAddress 0x1010, GuestAddress 0x1014]) (proofLiterals proof)
  expect "nonreturning calls retain composed frame depth" (Just (StackOffset (-24)))
    (M.lookup (GuestAddress 0x1000) (proofComposed proof))
  rejected "direct branch into a mapped literal pool" (admit (changed 0x1004 (branchWord False 0x1004 0x1010)))
  rejected "fallthrough into a mapped literal pool" (admit (changed 0x100c 0xe1a00000))
  rejected "conditional nonreturning call still falls through" (admit (changed 0x100c (0x1b000000 .|. (branchWord True 0x100c 0x1018 .&. 0xffffff))))
  rejected "returning callee restores the call continuation" (admit (changed 0x1038 0xe8bd8800))
  rejected "unowned literal pointer" (admit (changed 0x1010 0x2000))
  rejected "literal referent crosses its owned extent" (admit (changed 0x1010 0x1051))
  let headerLayout=layout {layoutLiteralPointers=RetainedStorage (spanAt 0x409c29b8 0x409c29c8) : layoutLiteralPointers layout}
      headerLiteral pointer=admitCode headerLayout (changed 0x1010 pointer)
  either (fail . ("canonical header literal refused: " ++) . show) (const (pure ())) (headerLiteral 0x409c29b8)
  expect "prebiased header literal" (Left (AdmissionError (GuestAddress 0x1000) LiteralPointerOutsideOwned))
    (() <$ headerLiteral 0x409c29b7)
  rejected "literal load crosses its function extent" (admit (changed 0x1000 0xe59f0034))
  rejected "unresolved PC-relative register offset" (admit (changed 0x1000 0xe79f0001))
  rejected "literal referent is executable code" (admit (changed 0x1010 0x1018))
  rejected "overlapping code and data mappings" (admitCode layout
    { layoutMappings = ArmSpan (spanAt 0x1010 0x1018) : layoutMappings layout } code)
  rejected "unclassified literal pool" (admitCode layout
    { layoutMappings = [ArmSpan (spanAt 0x1000 0x1010), ArmSpan (spanAt 0x1018 0x103c), DataSpan (spanAt 0x103c 0x1054)] } code)
  rejected "data pool relabeled as instructions" (admitCode layout
    { layoutMappings = [ArmSpan (spanAt 0x1000 0x103c), DataSpan (spanAt 0x103c 0x1054)] } code)
  rejected "unsupported trap prevents a nonreturn summary" (admit (changed 0x1038 0xe1200070))
  rejected "non-stack trap store prevents a nonreturn summary" (admit (changed 0x1034 0xe5801000))

main :: IO ()
main = do
  arguments <- getArgs
  if arguments == ["--literal-pools"] then walkerTests >> literalPoolTests else artifactTests arguments

artifactTests :: [String] -> IO ()
artifactTests arguments = do
  (bytes, symbolText, elfBytes) <- case arguments of
    [binPath, symbolsPath, elfPath] -> (,,) <$> B.readFile binPath <*> B.readFile symbolsPath <*> B.readFile elfPath
    _ -> fail "usage: property-service-artifact-tests LINKED_BIN SYMBOLS LINKED_ELF"
  elf <- either fail pure (ELF.parseElf elfBytes)
  section <- case [item | item <- ELF.elfSections elf, ELF.sectionAllocated item] of
    [item] -> pure item
    _ -> fail "expected one allocated section"
  let changedElf linked =
        let offset = fromIntegral (ELF.sectionOffset section)
        in B.take offset elfBytes <> linked <> B.drop (offset + B.length linked) elfBytes
  let symbols = symbolRows symbolText
      properties = syntheticClassification bytes symbols
      admit linked names = deriveArtifact ProductionArtifact properties testCarrier (ArtifactInput linked names (changedElf linked))
      Symbol setStart setSize _ = symbolNamed symbols (== "xm1_property_set")
      Symbol tableAddress _ _ = symbolNamed symbols ("10PROPERTIES" `isSuffixOf`)
      Symbol slotAddress _ _ = symbolNamed symbols ("5SLOTS" `isSuffixOf`)
      Symbol specialAddress _ _ = symbolNamed symbols ("8SPECIALS" `isSuffixOf`)
      Symbol slotDefaults _ _ = symbolNamed symbols (== "SLOT_BYTES")
      Symbol tailDefaults _ _ = symbolNamed symbols (== "TAIL_BYTES")
      Symbol getterStubAddress getterStubSize _ = symbolNamed symbols (== "xm1_property_get_entry")
      Symbol setterStubAddress setterStubSize _ = symbolNamed symbols (== "xm1_property_set_entry")
      Symbol store8 _ _ = symbolNamed symbols (== storageStoreName StorageByte)
      Symbol store16 _ _ = symbolNamed symbols (== storageStoreName StorageHalf)
      Symbol mirrorWrite _ _ = symbolNamed symbols (== "xm1_storage_write")
      Symbol output8 _ _ = symbolNamed symbols (== "xm1_storage_output8")
      Symbol output16 _ _ = symbolNamed symbols (== storageOutputName StorageHalf)
      Symbol status32 _ _ = symbolNamed symbols (== "xm1_storage_status32")
      calls = [site | site <- [setStart, setStart + 4 .. setStart + setSize - 4], U.word32At bytes (fromIntegral (site - carrierStartAddress)) .&. 0xff000000 == 0xeb000000]
      getterPop = getterStubAddress + getterStubSize - 4
      setterPop = setterStubAddress + setterStubSize - 4
  firstCall <- case calls of
    site : _ -> pure site
    [] -> fail "xm1_property_set has no BL"
  artifact <- either (fail . ("real artifact refused: " ++) . show) pure (admit bytes symbolText)
  unless (artifactCarrier artifact == testCarrier) (fail "carrier differs")
  putStrLn "admitted the linked artifact against the synthetic classification"
  let proof = artifactProof artifact
      emittedPools =
        [ (literal, entry)
        | GuestAddress literal <- S.toList (proofLiterals proof)
        , S.member (GuestAddress (literal - 4)) (proofInstructions proof)
        , FunctionExtent _ (GuestAddress entry) (GuestAddress finish) <- artifactFunctions artifact
        , entry < literal, literal < finish
        ]
      refusedPool label changed = case admit changed symbolText of
        Left (CodeRejected _) -> putStrLn ("refused " ++ label)
        Left other -> fail (label ++ ": expected a code-policy refusal, got " ++ show other)
        Right _ -> fail (label ++ ": admitted")
  case emittedPools of
    [] -> pure ()
    (literal, entry) : _ -> do
      refusedPool "direct branch into the emitted pool" (replaceWord entry (branchWord False entry literal) bytes)
      refusedPool "fallthrough into the emitted pool" (replaceWord (literal - 4) 0xe1a00000 bytes)
      refusedPool "unowned pointer in the emitted pool" (replaceWord literal 0x2000 bytes)
  expectRefusal "missing mandatory ELF evidence" ElfRejected
    (deriveArtifact ProductionArtifact properties testCarrier (ArtifactInput bytes symbolText B.empty))
  expectRefusal "ELF bytes differ from the linked artifact" ElfRejected
    (deriveArtifact ProductionArtifact properties testCarrier (ArtifactInput (replaceWord firstCall 0xe1a00000 bytes) symbolText elfBytes))
  symtab <- case [item | item <- ELF.elfSections elf, ELF.sectionName item == ".symtab"] of
    [item] -> pure item
    _ -> fail "expected one symbol table"
  armMarker <- case [index | (index, item) <- zip [0 :: Word32 ..] (ELF.elfSymbols elf), take 2 (ELF.symbolName item) == "$a"] of
    index : _ -> pure index
    [] -> fail "missing ARM mapping evidence"
  dataMarker <- case [index | (index, item) <- zip [0 :: Word32 ..] (ELF.elfSymbols elf), take 2 (ELF.symbolName item) == "$d"] of
    index : _ -> pure index
    [] -> fail "missing data mapping evidence"
  let markerField index field = ELF.sectionOffset symtab + index * 16 + field
      withElf changed = deriveArtifact ProductionArtifact properties testCarrier (ArtifactInput bytes symbolText changed)
      unaligned = putWordAt 0 (markerField armMarker 4) (carrierStartAddress + 2) elfBytes
      conflict = putWordAt 0 (markerField dataMarker 4) carrierStartAddress elfBytes
      armName = U.word32At elfBytes (fromIntegral (markerField armMarker 0))
      relabeled = putWordAt 0 (markerField dataMarker 0) armName elfBytes
  expectRefusal "unaligned ELF mapping" MappingRejected (withElf unaligned)
  expectRefusal "conflicting ELF code and data mappings" MappingRejected (withElf conflict)
  case withElf relabeled of
    Left _ -> putStrLn "refused data relabeled as instructions in ELF"
    Right _ -> fail "ELF data relabeled as instructions admitted"
  forM_ [("slot",slotDefaults),("tail",tailDefaults)] $ \(name,address) -> do
    boundary <- case [ELF.symbolValue item | item <- ELF.elfSymbols elf
      ,take 2 (ELF.symbolName item) `elem` ["$a","$d","$t"],ELF.symbolValue item <= address] of
      [] -> fail ("missing mapping for embedded " ++ name ++ " defaults")
      points -> pure (maximum points)
    let dataMarkers=[index | (index,item) <- zip [0::Word32 ..] (ELF.elfSymbols elf)
          ,ELF.symbolValue item == boundary,take 2 (ELF.symbolName item) == "$d"]
        asInstructions=foldl (\current index -> putWordAt 0 (markerField index 0) armName current) elfBytes dataMarkers
    unless (not (null dataMarkers)) (fail ("embedded " ++ name ++ " defaults are not mapped data"))
    expectRefusal ("embedded " ++ name ++ " defaults relabeled as instructions")
      (CodeRejected (AdmissionError (GuestAddress carrierStartAddress) LayoutInvalid)) (withElf asInstructions)
  let flippedAt address = let at = fromIntegral (address - carrierStartAddress) + 1 in B.take at bytes <> B.singleton (B.index bytes at + 1) <> B.drop (at + 1) bytes
  expectRefusal "mutated PROPERTIES byte" TableRejected (admit (flippedAt tableAddress) symbolText)
  expectRefusal "mutated SLOTS byte" SlotTableRejected (admit (flippedAt slotAddress) symbolText)
  let unsaved = [case propertyClass p of Owned (OwnedSlotField arm) -> p {propertyClass = Owned (OwnedSlotField arm {slotSave = NoSave})}; _ -> p | p <- properties]
  expectRefusal "slot fields the SLOTS table does not hold" SlotTableRejected (deriveArtifact ProductionArtifact unsaved testCarrier (ArtifactInput bytes symbolText elfBytes))
  expectRefusal "mutated SPECIALS byte" SpecialTableRejected (admit (flippedAt specialAddress) symbolText)
  let swapped = [case propertyClass p of Owned (OwnedField arm@(FieldArm _ _ _ (FieldStock _ provisional) _ _)) -> p {propertyClass = Owned (OwnedField arm {fieldSource = FieldStock Stock61d5f0 provisional})}; _ -> p | p <- properties]
  expectRefusal "stock defaults the SPECIALS table does not hold" SpecialTableRejected (deriveArtifact ProductionArtifact swapped testCarrier (ArtifactInput bytes symbolText elfBytes))
  expectRefusal "mutated embedded slot defaults" (DefaultTableRejected (FunctionName "SLOT_BYTES")) (admit (flippedAt slotDefaults) symbolText)
  expectRefusal "mutated embedded tail defaults" (DefaultTableRejected (FunctionName "TAIL_BYTES")) (admit (flippedAt tailDefaults) symbolText)
  expectRefusal "stock leaf symbol moved" SymbolRejected (admit bytes (moveSymbol "xm1_stock_61d59c" 0x61d5a0 symbolText))
  expectRefusal "BL from Rust to a bracket callee" (CodeRejected (AdmissionError (GuestAddress firstCall) (CallOutsideOwned (GuestAddress 0x61da3c))))
    (admit (replaceWord firstCall (branchWord True firstCall 0x61da3c) bytes) symbolText)
  expectRefusal "artifact linked away from the hull scan's carrier" (PlacementRejected (CarrierNotCandidate (GuestAddress carrierStartAddress)))
    (deriveArtifact ProductionArtifact properties (Carrier (GuestAddress 0x62c4b0) (GuestAddress carrierEndAddress)) (ArtifactInput bytes symbolText elfBytes))
  expectRefusal "setter entry calls rather than tail branches" (EncodingRejected (EntryEncoding SetterEntry))
    (admit (replaceWord setterStubAddress (branchWord True setterStubAddress setStart) bytes) symbolText)
  expectRefusal "setter ending b 0x630200" (EncodingRejected (EntryEncoding SetterEntry))
    (admit (replaceWord setterPop (branchWord False setterPop 0x630200) bytes) symbolText)
  let delegating = case break (\p -> case propertyClass p of Owned (OwnedField _) -> True; _ -> False) properties of
        (before, p : after) -> before ++ p {propertyClass = Delegated ExitAdjusts} : after
        (before, []) -> before
      unbracketed = [case propertyClass p of Owned (OwnedField arm@(FieldArm _ _ _ _ _ Bank0Bracket)) -> p {propertyClass = Owned (OwnedField arm {fieldHook = NoHook})}; _ -> p | p <- properties]
  expectRefusal "a classification delegating a property" TableRejected (deriveArtifact ProductionArtifact delegating testCarrier (ArtifactInput bytes symbolText elfBytes))
  expectRefusal "bracket hooks the tables do not hold" TableRejected (deriveArtifact ProductionArtifact unbracketed testCarrier (ArtifactInput bytes symbolText elfBytes))
  expectRefusal "getter uses implicit r12" (EncodingRejected (EntryEncoding GetterEntry))
    (admit (replaceWord getterPop 0xe1a0000c bytes) symbolText)
  expectRefusal "store boundary lower bound changed" (EncodingRejected (StorageEncoding StorageByte))
    (admit (replaceWord store8 0xe300c000 bytes) symbolText)
  expectRefusal "store boundary range check removed" (EncodingRejected (StorageEncoding StorageByte))
    (admit (replaceWord (store8 + 12) 0xe320f000 bytes) symbolText)
  expectRefusal "halfword boundary widened by a byte" (EncodingRejected (StorageEncoding StorageHalf))
    (admit (replaceWord (store16 + 16) (U.word32At bytes (fromIntegral (store8 + 16 - carrierStartAddress))) bytes) symbolText)
  expectRefusal "mirror byte driver target changed" (EncodingRejected (MirrorEncoding (FunctionName "xm1_storage_write")))
    (admit (replaceWord (mirrorWrite + 8) (branchWord True (mirrorWrite + 8) 0x6a59bc) bytes) symbolText)
  expectRefusal "mirror boundary called without saving lr" (EncodingRejected (MirrorEncoding (FunctionName "xm1_storage_write")))
    (admit (replaceWord mirrorWrite 0xe320f000 bytes) symbolText)
  expectRefusal "mirror absolute target moved" SymbolRejected
    (admit bytes (moveSymbol "xm1_backup_write6a59b8" 0x6a59bc symbolText))
  expectRefusal "mirror retained target called outside its boundary" (CodeRejected (AdmissionError (GuestAddress firstCall) (CallOutsideOwned (GuestAddress 0x6a59b8))))
    (admit (replaceWord firstCall (branchWord True firstCall 0x6a59b8) bytes) symbolText)
  expectRefusal "area-3 writer called by the header path" (CodeRejected (AdmissionError (GuestAddress firstCall) (CallOutsideOwned (GuestAddress 0x61d92c))))
    (admit (replaceWord firstCall (branchWord True firstCall 0x61d92c) bytes) symbolText)
  expectRefusal "moved entry" SymbolRejected (admit bytes (moveSymbol "xm1_property_set_entry" (setterStubAddress + 4) symbolText))
  expectRefusal "store outside a boundary" (CodeRejected (AdmissionError (GuestAddress firstCall) NonStackStore))
    (admit (replaceWord firstCall 0xe5810000 bytes) symbolText)
  expectRefusal "BL to an outside address" (CodeRejected (AdmissionError (GuestAddress firstCall) (CallOutsideOwned (GuestAddress 0x700000))))
    (admit (replaceWord firstCall (branchWord True firstCall 0x700000) bytes) symbolText)
  expectRefusal "caller-output length bound removed" (EncodingRejected (OutputEncoding StorageByte))
    (admit (replaceWord (output8 + 8) 0xe320f000 bytes) symbolText)
  expectRefusal "caller-output overflow bound removed" (EncodingRejected (OutputEncoding StorageByte))
    (admit (replaceWord (output8 + 16) 0xe320f000 bytes) symbolText)
  expectRefusal "caller-output index bound removed" (EncodingRejected (OutputEncoding StorageByte))
    (admit (replaceWord (output8 + 24) 0xe320f000 bytes) symbolText)
  forM_
    [ ("halfword output length bound removed",8,0xe320f000)
    , ("halfword output span overflow bound removed",16,0xe320f000)
    , ("halfword output index overflow bound removed",24,0xe320f000)
    , ("halfword output whole-write bound removed",32,0xe320f000)
    , ("halfword output alignment bound removed",44,0xe320f000)
    , ("halfword output narrowed to a byte",48,0xe5cc3000)
    ] $ \(label,offset,instruction) ->
      expectRefusal label (EncodingRejected (OutputEncoding StorageHalf))
        (admit (replaceWord (output16+offset) instruction bytes) symbolText)
  expectRefusal "mapped status store narrowed to a byte" (EncodingRejected StatusEncoding)
    (admit (replaceWord (status32 + 8) 0xe5cc0000 bytes) symbolText)
  expectRefusal "mapped status word moved" (EncodingRejected StatusEncoding)
    (admit (replaceWord status32 0xe30ecdc8 bytes) symbolText)
  expectRefusal "store boundary width changed" (EncodingRejected (StorageEncoding StorageByte))
    (admit (replaceWord (store8 + 132) 0xe5801000 bytes) symbolText)
  expectRefusal "store boundary writes through another pointer" (EncodingRejected (StorageEncoding StorageHalf))
    (admit (replaceWord (store16 + 132) 0xe1c210b0 bytes) symbolText)
  expectRefusal "B into stock" (CodeRejected (AdmissionError (GuestAddress firstCall) (BranchOutsideOwned (GuestAddress 0x61e110))))
    (admit (replaceWord firstCall (branchWord False firstCall 0x61e110) bytes) symbolText)
  expectRefusal "a stock-frame pop inside Rust" (CodeRejected (AdmissionError (GuestAddress firstCall) PcWrite))
    (admit (replaceWord firstCall 0xe8bd89f8 bytes) symbolText)
  expectRefusal "overlong artifact" ExtentRejected (admit (bytes <> B.replicate 4 0) symbolText)
  let pastCarrier = carrierEndAddress + 4
  expectRefusal "artifact past the carrier" ExtentRejected
    (admit (bytes <> B.replicate (fromIntegral (pastCarrier - carrierStartAddress) - B.length bytes) 0) (moveSymbol "__property_service_end" pastCarrier symbolText))
  dataSiteTests
  carrierTests
  placementTests properties artifact
  walkerTests
  literalPoolTests
