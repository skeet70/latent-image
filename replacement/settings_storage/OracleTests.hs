{-# LANGUAGE GHC2021 #-}
module Main (main) where

import Admission (admitArtifact)
import BankApplyFixture (bankApplyFixture, bankApplyAncestorFixtures)
import qualified CustomNative as Custom
import qualified CustomAdmission as Boundary
import Data.Bits (shiftR, (.&.), (.|.))
import Data.Either (isLeft)
import qualified Data.Map.Strict as M
import qualified Emulation.NativeCase as NativeCase
import qualified Emulation.NativeObservation as Observation
import Emulation.OwnedCase (CaseId(..), CaseRegion(..), Permission(..), Symbol(..), mkRegion, parseSymbols, word32le)
import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word32)
import Numeric (showHex)
import qualified FujiAF.FirstStage as F
import Oracle
import Replacement.PropertyService.Artifact (StorageWidth(..), storageOutputName)
import Replacement.PropertyService.Placement (Carrier(..), GuestAddress(..))
import qualified SettingsEeNative as Ee
import System.Environment (getArgs)

assert :: String -> Bool -> IO ()
assert label holds = unless holds (fail label)

bankApplyElfTests :: IO ()
bankApplyElfTests = do
  let valid = bankApplyFixture
      entry@(GuestAddress start) = Boundary.bankApplyImageBase valid
      elfBytes offset content input = input {Boundary.bankApplyElfBytes = replace offset content (Boundary.bankApplyElfBytes input)}
      elfWord offset value = elfBytes offset (word32le value)
      nm address size kind = valid {Boundary.bankApplySymbolBytes = C.pack
        (showHex address "" ++ " " ++ showHex size "" ++ " " ++ kind ++ " xm1_storage_apply\n")}
      linkedWord offset value = valid {Boundary.bankApplyLinkedBytes = replace offset (word32le value) (Boundary.bankApplyLinkedBytes valid)}
      changedCall word = elfWord 0x104 word (linkedWord 4 word)
      callTo target = 0xeb000000 .|. (((target - start - 12) `shiftR` 2) .&. 0xffffff)
      alias = B.concat (map word32le [1,0,start + 4,start + 4,0,16,6,4])
      duplicateSegment = B.take 32 (B.drop 52 (Boundary.bankApplyElfBytes valid))
      duplicateFunction = B.take 16 (B.drop 0x1b0 (Boundary.bankApplyElfBytes valid))
      missingMappings = elfBytes 0x190 (B.replicate 32 0) valid
      missingFirstMapping = elfBytes 0x190 (B.replicate 16 0) valid
      cases =
        [ ("missing ELF", valid {Boundary.bankApplyElfBytes = B.empty})
        , ("missing function", elfBytes 0x1b0 (B.replicate 16 0) valid)
        , ("duplicate function", elfBytes 0x1a0 duplicateFunction valid)
        , ("object instead of STT_FUNC", elfWord 0x1bc 0x00010011 valid)
        , ("undefined function", elfWord 0x1bc 0x00000012 valid)
        , ("absolute function", elfWord 0x1bc 0xfff10012 valid)
        , ("empty function", elfWord 0x1b8 0 valid)
        , ("ELF size differs from nm", elfWord 0x1b8 8 valid)
        , ("ELF address differs from nm", elfWord 0x1b4 (start + 4) valid)
        , ("nm size differs", nm start (8 :: Word32) "T")
        , ("nm address differs", nm (start + 4) (12 :: Word32) "T")
        , ("local nm text", nm start (12 :: Word32) "t")
        , ("nm readonly data", nm start (12 :: Word32) "R")
        , ("missing nm function", valid {Boundary.bankApplySymbolBytes = B.empty})
        , ("duplicate nm function", valid {Boundary.bankApplySymbolBytes = Boundary.bankApplySymbolBytes valid <> Boundary.bankApplySymbolBytes valid})
        , ("wrong direct callee", changedCall (callTo 0x624698))
        , ("conditional property call", changedCall (0x1b000000 .|. (callTo 0x624694 .&. 0xffffff)))
        , ("indirect property call", changedCall 0xe12fff30)
        , ("published call bytes differ", linkedWord 4 0xe1a00000)
        , ("published function tail differs", linkedWord 8 0xe1a00000)
        , ("section bytes differ", elfWord 0x2b8 0x140 valid)
        , ("segment bytes differ", elfWord 56 0x140 valid)
        , ("missing carrier", valid {Boundary.bankApplyCarriers = []})
        , ("carrier begins inside function", valid {Boundary.bankApplyCarriers = [Carrier (GuestAddress (start + 4)) (GuestAddress (start + 0x100))]})
        , ("carrier ends inside function", valid {Boundary.bankApplyCarriers = [Carrier entry (GuestAddress (start + 8))]})
        , ("publication begins inside function", valid {Boundary.bankApplyImageBase = GuestAddress (start + 4)})
        , ("publication truncates function", valid {Boundary.bankApplyLinkedBytes = B.take 8 (Boundary.bankApplyLinkedBytes valid)})
        , ("writable section", elfWord 0x2b0 7 valid)
        , ("nonallocated section", elfWord 0x2b0 4 valid)
        , ("nonexecutable section", elfWord 0x2b0 2 valid)
        , ("NOBITS section", elfWord 0x2ac 8 valid)
        , ("section begins inside function", elfWord 0x2b4 (start + 4) valid)
        , ("section truncates function", elfWord 0x2bc 8 valid)
        , ("writable segment", elfWord 76 7 valid)
        , ("nonexecutable segment", elfWord 76 4 valid)
        , ("unreadable segment", elfWord 76 1 valid)
        , ("segment truncates function", elfWord 68 8 valid)
        , ("segment memory shorter than file", elfWord 72 8 valid)
        , ("missing load segment", elfWord 44 0x00280000 valid)
        , ("ambiguous load segments", elfBytes 84 duplicateSegment (elfWord 44 0x00280002 valid))
        , ("writable memory-only alias", elfBytes 84 alias (elfWord 44 0x00280002 valid))
        , ("missing mappings", missingMappings)
        , ("mapping starts after entry", missingFirstMapping)
        , ("Thumb entry", elfWord 0x190 25 valid)
        , ("Thumb inside function", elfWord 0x1a0 25 (elfWord 0x1a4 (start + 4) valid))
        , ("property call is mapped data", elfWord 0x1a0 22 (elfWord 0x1a4 (start + 4) valid))
        , ("conflicting mapping states", elfWord 0x1a0 22 (elfWord 0x1a4 start valid))
        , ("unaligned ARM mapping", elfWord 0x1a4 (start + 2) valid)
        , ("unaligned internal data boundary", elfWord 0x1a0 22 (elfWord 0x1a4 (start + 5) valid))
        , ("mapping belongs to another section", elfWord 0x19c 0x00020000 (elfBytes 0x1a0 (B.replicate 16 0) valid))
        , ("mapping precedes its ELF section", elfWord 0x194 (start - 4) valid)
        , ("sized mapping symbol", elfWord 0x198 4 valid)
        , ("object mapping symbol", elfWord 0x19c 0x00010001 valid)
        ]
  boundary <- either fail pure (Boundary.admitBankApplyElf valid)
  assert "ELF boundary exposes actual function entry" (Boundary.bankApplyEntry boundary == entry)
  assert "ELF boundary exposes complete function end" (Boundary.bankApplyEnd boundary == GuestAddress (start + 12))
  forM_ [0,4 .. 20] $ \offset ->
    assert ("ELF boundary classifies actual call return " ++ show offset)
      (Boundary.bankApplyCallReturn boundary (GuestAddress (start + offset)) == (offset == 8))
  forM_ cases $ \(label, input) ->
    assert ("bank apply ELF refuses " ++ label) (isLeft (Boundary.admitBankApplyElf input))

bankApplyAncestorElfTests :: IO ()
bankApplyAncestorElfTests = forM_ bankApplyAncestorFixtures $ \valid -> do
  let thumbName=fromIntegral (B.length (fst (C.breakSubstring (C.pack "$t\0") (B.drop 0x380 (Boundary.bankApplyElfBytes valid))))) :: Word32
  let GuestAddress start=Boundary.bankApplyImageBase valid
      expected=map GuestAddress [start,start+0x20,start+0x30,0x630a34]
      elfWord offset value input=input {Boundary.bankApplyElfBytes=replace offset (word32le value) (Boundary.bankApplyElfBytes input)}
      linkedWord offset value input=input {Boundary.bankApplyLinkedBytes=replace offset (word32le value) (Boundary.bankApplyLinkedBytes input)}
      codeWord offset value=elfWord (0x100+offset) value (linkedWord offset value valid)
      nm name address size kind input=input {Boundary.bankApplySymbolBytes=C.unlines
        [case C.words row of
          [_,_,_,symbol] | symbol == C.pack name -> C.pack (showHex address "" ++ " " ++ showHex (size::Word32) "" ++ " " ++ kind ++ " " ++ name)
          _ -> row
        | row <- C.lines (Boundary.bankApplySymbolBytes input)]}
      linkedCall=0xeb000000 .|. (((start-(start+0x24)-8) `shiftR` 2) .&. 0xffffff)
      conditionalTail=0x1a000000 .|. (((start-(start+0x24)-8) `shiftR` 2) .&. 0xffffff)
      fixedConditional=0x1a000000 .|. ((((start+0x30)-0x630a34-8) `shiftR` 2) .&. 0xffffff)
      fixedWord value=elfWord 0x180 value (linkedWord (fromIntegral (0x630a34-start)) value valid)
      alias=nm "branch_data" start 12 "T" (elfWord 0x2a4 start (elfWord 0x2a8 12 (elfWord 0x2ac 0x00010012 valid)))
      controls=
        [ ("ancestor nm size",nm "tail_leaf" (start+0x20) 12 "T" valid)
        , ("ancestor ELF size",elfWord 0x268 12 valid)
        , ("ancestor published bytes",linkedWord 0x24 0xe1a00000 valid)
        , ("ancestor ELF bytes",elfWord 0x124 0xe1a00000 valid)
        , ("ancestor mapped as data",elfWord 0x224 (start+0x20) valid)
        , ("ancestor mapped as Thumb",elfWord 0x220 thumbName (elfWord 0x224 (start+0x20) valid))
        , ("ancestor outside carriers",valid {Boundary.bankApplyCarriers=[Carrier (GuestAddress start) (GuestAddress (start+0x20))]})
        , ("nonalias sized overlap",nm "tail_leaf" (start+0x20) 0x20 "T" (elfWord 0x268 0x20 valid))
        , ("fixed ancestor writable section",elfWord 0x558 7 valid)
        , ("fixed ancestor truncated load",elfWord 0x64 0 valid)
        , ("fixed ancestor conditional veneer",fixedWord fixedConditional)
        , ("fixed ancestor indirect veneer",fixedWord 0xe12fff10)
        , ("data word described as function",nm "branch_data" (start+0x40) 4 "T" (elfWord 0x2ac 0x00010012 valid))
        ]
  boundary <- either fail pure (Boundary.admitBankApplyElf valid)
  assert "ELF root discovery follows sized tail ancestors across carriers and the fixed veneer"
    (Boundary.bankApplyRoots boundary == expected)
  aliased <- either fail pure (Boundary.admitBankApplyElf alias)
  assert "exact ELF function aliases do not create extra roots" (Boundary.bankApplyRoots aliased == expected)
  conditional <- either fail pure (Boundary.admitBankApplyElf (codeWord 0x24 conditionalTail))
  assert "conditional direct tail edges preserve potential incoming LR roots" (Boundary.bankApplyRoots conditional == expected)
  called <- either fail pure (Boundary.admitBankApplyElf (codeWord 0x24 linkedCall))
  assert "a link-setting call does not make its caller an inherited-LR ancestor"
    (Boundary.bankApplyRoots called == [GuestAddress start])
  forM_ controls $ \(label,input) ->
    assert ("ELF ancestor admission refuses " ++ label) (isLeft (Boundary.admitBankApplyElf input))

eeTraceTests :: IO ()
eeTraceTests = do
  let wire rows=word32le (fromIntegral (length rows)) <> B.concat (map (B.concat . map word32le) rows) <> B.replicate (216-length rows*24) 0
      equal=wire [[0xd4d604,77,0xee,0,25,7]]
      differentRows=
        [ [0x634650,0,0,0,24,0],[0x634650,0,8,0,24,0],[0x634650,0,9,0,24,0]
        , [0x634650,0,0,1,24,0],[0x634650,0,8,1,24,0],[0x634650,0,9,1,24,0]
        , [0x644d84,0,0,0,24,0],[0x644e84,0,0,0,24,4],[0xd4d604,77,0xee,0,24,12]
        ]
      different=wire differentRows
      changed offset value=replace offset (word32le value) different
      controls=
        [ ("stored code instead of inverse",changed 20 6)
        , ("missing final setter effects",changed (4+8*24+20) 4)
        , ("effects before cache publication",changed (4+4*24+16) 0x6d6d6d6d)
        , ("bank mutation before resets",changed (4+20) 1)
        , ("first retained helper has no declared writes",changed (4+7*24+20) 0)
        , ("notification precedes second helper writes",changed (4+8*24+20) 3)
        , ("wrong retained helper argument",changed (4+6*24+8) 1)
        , ("omitted reset",wire (drop 1 differentRows))
        , ("reordered resets",wire ([[0x634650,0,8,0,24,0],[0x634650,0,0,0,24,0]] ++ drop 2 differentRows))
        , ("wrong effect count",changed 0 8)
        ]
  assert "EE equal path retains inverse cache and completed final bank effects" (Ee.checkEeTrace Ee.EeEqual equal == Right ())
  assert "EE changed path retains all eight ordered calls and declared helper writes" (Ee.checkEeTrace Ee.EeDifferent different == Right ())
  assert "EE equal and changed traces are not interchangeable" (isLeft (Ee.checkEeTrace Ee.EeDifferent equal) && isLeft (Ee.checkEeTrace Ee.EeEqual different))
  forM_ controls $ \(label,bytes) ->
    assert ("EE observed trace rejects " ++ label) (isLeft (Ee.checkEeTrace Ee.EeDifferent bytes))
  forM_ [0..219] $ \size ->
    assert ("EE trace refuses truncation " ++ show size) (isLeft (Ee.checkEeTrace Ee.EeDifferent (B.take size different)))
  assert "EE equal trace preserves zero unused observation slots"
    (isLeft (Ee.checkEeTrace Ee.EeEqual (replace 219 (B.singleton 1) equal)))
  let controlsDeclared=[minBound..maxBound] :: [Ee.EeControl]
  assert "EE controls include cache, ordering, final setter, void result, and stale backing"
    (length controlsDeclared == 7 && M.size (M.fromList [(Ee.eeControlName control,()) | control <- controlsDeclared]) == 7)

main :: IO ()
main = do
  bankApplyElfTests
  bankApplyAncestorElfTests
  eeTraceTests
  (linkedPath,symbolPath,customPath,customSymbolPath) <- getArgs >>= \arguments -> case arguments of
    [linked,symbols,custom,customSymbols] -> pure (linked,symbols,custom,customSymbols)
    _ -> fail "usage: storage-oracle-tests COMPONENT_BIN COMPONENT_SYMBOLS CUSTOM_BIN CUSTOM_SYMBOLS"
  linked <- B.readFile linkedPath
  rows <- map words . lines <$> readFile symbolPath
  symbols <- parseSymbols [name | [_,_,kind,name] <- rows,kind `elem` ["T","t","A","a","R","r"]] symbolPath
  custom <- B.readFile customPath
  customNames <- B.readFile customSymbolPath >>= either fail pure . F.parseSymbols
  let customSymbols = M.fromList [(F.symbolName symbol,Symbol (F.symbolAddress symbol) (F.symbolSize symbol)) | symbol <- customNames]
      customRequired name = maybe (fail ("missing custom symbol " ++ name)) pure (M.lookup name customSymbols)
      customPatched address instruction = replace (fromIntegral (address-0x5d0000)) (word32le instruction) custom
  assert "actual custom artifact admits all bounded store widths" (Boundary.admitCustomBoundaries 0x5d0000 custom customSymbols == Right ())
  forM_ [minBound..maxBound] $ \width -> do
    let name = Boundary.customStoreName width
    Symbol address _ <- customRequired name
    assert ("custom admission refuses a missing " ++ name)
      (isLeft (Boundary.admitCustomBoundaries 0x5d0000 custom (M.delete name customSymbols)))
    assert ("custom admission refuses an unguarded store replacing " ++ name)
      (isLeft (Boundary.admitCustomBoundaries 0x5d0000 (customPatched address 0xe5801000) customSymbols))
  Symbol customHalf _ <- customRequired (Boundary.customStoreName Boundary.CustomStoreHalf)
  Symbol customWord _ <- customRequired (Boundary.customStoreName Boundary.CustomStoreWord)
  forM_
    [ ("custom halfword alignment",customHalf+4,0xe320f000)
    , ("custom halfword previous whole-write bound",customHalf+20,0xe3520034)
    , ("custom halfword working whole-write bound",customHalf+40,0xe3520034)
    , ("custom halfword live whole-write bound",customHalf+60,0xe3520028)
    , ("custom halfword width",customHalf+72,0xe5c01000)
    , ("custom word alignment",customWord+4,0xe320f000)
    , ("custom word whole-write bound",customWord+20,0xe352003c)
    , ("custom word working-window substitution",customWord+8,0xe30f2874)
    , ("custom word live-window substitution",customWord+8,0xe302295c)
    , ("custom word width",customWord+32,0xe5c01000)
    ] $ \(label,address,instruction) ->
      assert ("custom admission refuses changed " ++ label)
        (isLeft (Boundary.admitCustomBoundaries 0x5d0000 (customPatched address instruction) customSymbols))
  let required name = maybe (fail ("missing linked symbol " ++ name)) pure (M.lookup name symbols)
  Symbol tailAddress tailSize <- required "TAIL_BYTES"
  Symbol slotAddress slotSize <- required "SLOT_BYTES"
  Symbol outputAddress _ <- required (storageOutputName StorageByte)
  Symbol outputHalfAddress _ <- required (storageOutputName StorageHalf)
  Symbol statusAddress _ <- required "xm1_storage_status32"
  let keys = map BackupKey [15..30]
      zero = B.replicate 16 0
      stable = MirrorState zero zero
      changed index = replace index (B.singleton 1) zero
  assert "checksum excludes incoming checksum byte" (checksum (replace 15 (B.singleton 7) zero) == zero)
  assert "checksum wraps at 256" (checksum (B.pack [255,1] <> B.replicate 14 0) == B.pack [255,1] <> B.replicate 14 0)
  assert "checksum wraps at 511" (B.last (checksum (B.pack [255,255,1] <> B.replicate 13 0)) == 1)
  assert "unchanged mirror performs no writes" (closeMirror keys zero stable == (stable,[]))
  forM_ [0..14] $ \index -> do
    let (final,effects) = closeMirror keys (changed index) stable
    assert ("changed byte and checksum writes " ++ show index) (effects == [DriverEffect (BackupKey (fromIntegral (15+index))) 1 0,DriverEffect (BackupKey 30) 255 0])
    assert ("shadow follows synchronized live " ++ show index) (shadowBytes final == liveBytes final && B.last (liveBytes final) == 255)
    assert ("input checksum untouched " ++ show index) (B.last (changed index) == 0)
    assert ("second close unchanged " ++ show index) (snd (closeMirror keys (changed index) final) == [])
  let (_,lastOnly) = closeMirror keys zero (MirrorState zero (replace 15 (B.singleton 1) zero))
  assert "last shadow byte is compared" (lastOnly == [DriverEffect (BackupKey 30) 0 1])
  assert "task locks each request independently" (requestEdges Task [] ++ requestEdges Task [] == [Event 0x7c 0x25 0xffffffff 0,Event 0x6c 0x25 0 0,Event 0x7c 0x25 0xffffffff 0,Event 0x6c 0x25 0 0])
  assert "interrupt request emits diagnostic" (requestEdges Interrupt [] == [Event 0x726c74 2 59 0])
  let image = B.replicate 0x200000 0
      offset address = fromIntegral (address-0x5c0000)
      binary = replace (offset tailAddress) (B.replicate (fromIntegral tailSize) 0) (replace (offset slotAddress) (B.replicate (fromIntegral slotSize) 0) linked)
      patchedWord address value = replace (offset address) (word32le value) binary
      end = 0x5c0000 + fromIntegral (B.length binary)
  assert "admission accepts built boundaries with synthetic default bytes" (admitArtifact image binary symbols == Right ())
  assert "admission rejects an empty artifact" (isLeft (admitArtifact image B.empty symbols))
  assert "admission rejects an unaligned artifact" (isLeft (admitArtifact image (binary <> B.singleton 0) symbols))
  assert "admission rejects an entry below its load address" (isLeft (admitArtifact image binary (M.insert "xm1_getter_owned" (Symbol 0x5bfffc 4) symbols)))
  assert "admission rejects an entry beyond the linked bytes" (isLeft (admitArtifact image binary (M.insert "xm1_getter_owned" (Symbol end 4) symbols)))
  assert "admission rejects a missing entry" (isLeft (admitArtifact image binary (M.delete "xm1_storage_restore" symbols)))
  assert "admission rejects an unknown external" (isLeft (admitArtifact image binary (M.insert "undeclared_device" (Symbol 0x700000 0) symbols)))
  assert "admission rejects a malformed default extent" (isLeft (admitArtifact image binary (M.insert "SLOT_BYTES" (Symbol slotAddress (slotSize+1)) symbols)))
  assert "admission rejects bytes differing from supplied defaults" (isLeft (admitArtifact image (replace (offset tailAddress) (B.singleton 1) binary) symbols))
  assert "admission rejects a removed caller-span length check" (isLeft (admitArtifact image (patchedWord (outputAddress+8) 0xe320f000) symbols))
  assert "admission rejects a removed caller-span overflow check" (isLeft (admitArtifact image (patchedWord (outputAddress+16) 0xe320f000) symbols))
  assert "admission rejects a removed caller-span index check" (isLeft (admitArtifact image (patchedWord (outputAddress+24) 0xe320f000) symbols))
  forM_
    [ ("removed halfword caller-span length check",8,0xe320f000)
    , ("removed halfword caller-span overflow check",16,0xe320f000)
    , ("removed halfword index overflow check",24,0xe320f000)
    , ("removed halfword whole-write check",32,0xe320f000)
    , ("removed halfword destination alignment check",44,0xe320f000)
    , ("narrowed halfword caller output",48,0xe5cc3000)
    ] $ \(label,index,instruction) ->
      assert ("admission rejects a " ++ label)
        (isLeft (admitArtifact image (patchedWord (outputHalfAddress+index) instruction) symbols))
  assert "admission rejects a narrowed status store" (isLeft (admitArtifact image (patchedWord (statusAddress+8) 0xe5cc0000) symbols))
  assert "admission rejects a moved backup driver target" (isLeft (admitArtifact image binary (M.insert "xm1_backup_write6a59b8" (Symbol 0x6a59bc 0) symbols)))
  assert "admission rejects a moved custom refresh target" (isLeft (admitArtifact image binary (M.insert "xm1_bank_custom_refresh" (Symbol 0x6a86c8 0) symbols)))
  assert "admission rejects an undeclared name at the custom refresh address" (isLeft (admitArtifact image binary (M.insert "undeclared_custom_refresh" (Symbol 0x6a86c4 0) symbols)))
  let baseline = Observation.NativeObservation
        { Observation.nativeStatus = Observation.Returned
        , Observation.nativeRegisters = M.fromList [(register,0) | register <- [minBound..maxBound]]
        , Observation.nativeMemory = [(Observation.RegionId name,B.replicate 0x80 0) | name <- ["results","caller","medium","requests","custom-images","custom-live","custom-probes"]]
        , Observation.nativeProvenance = Observation.Complete (Observation.ProvenanceTotal 0) []
        , Observation.nativeInstructions = Observation.InstructionCount 1
        , Observation.nativeEngineError = Observation.EngineError 0
        , Observation.nativeKernel = Nothing
        }
      different = baseline {Observation.nativeMemory = [(identifier,if identifier == Observation.RegionId "results" then B.singleton 1 <> B.drop 1 bytes else bytes) | (identifier,bytes) <- Observation.nativeMemory baseline]}
      witness reference subject = Custom.CustomControlWitness (CaseId "control-case")
        (Custom.projectCustomObservation reference) (Custom.projectCustomObservation subject)
      returnedDifference = witness baseline different
      blockedStatuses =
        [ Observation.InstructionBudget,Observation.WallBudget,Observation.MemoryGuard,Observation.FetchGuard
        , Observation.UnknownCall,Observation.UnsupportedInstruction,Observation.StubContract,Observation.StopAddress
        , Observation.RunnerEngineError,Observation.InvalidCase,Observation.HostTimeout,Observation.RunnerFailure
        , Observation.InvalidOutput,Observation.runnerStatusFromLabel "unknown-control-exit"
        ]
  assert "custom controls require witnesses" (isLeft (Custom.customControlKilled []))
  assert "returned unchanged custom controls survive" (Custom.customControlKilled [witness baseline baseline] == Right False)
  assert "returned observable custom differences kill controls" (Custom.customControlKilled [returnedDifference] == Right True)
  forM_ blockedStatuses $ \status -> do
    let blocked = different {Observation.nativeStatus = status}
    assert ("custom controls refuse a blocked subject: " ++ show status) (isLeft (Custom.customControlKilled [witness baseline blocked]))
    assert ("custom controls refuse a blocked reference: " ++ show status) (isLeft (Custom.customControlKilled [witness blocked different]))
    assert ("an earlier difference does not excuse a later blocked witness: " ++ show status)
      (isLeft (Custom.customControlKilled [returnedDifference,witness baseline blocked]))
  let projected=Custom.projectCustomObservation baseline
      changedMemory name position value observation=observation
        {Observation.nativeMemory=[(identifier,if identifier == Observation.RegionId name then replace position (B.singleton value) bytes else bytes)
          | (identifier,bytes) <- Observation.nativeMemory observation]}
      differences subject=Custom.comparePair projected (Custom.projectCustomObservation subject)
  forM_ [("results",0),("caller",0),("medium",0),("requests",0),("custom-images",4),("custom-images",119),("custom-live",0),("custom-probes",0)] $ \(name,position) ->
    assert ("custom projection retains semantic byte " ++ name ++ ":" ++ show position)
      (not (null (differences (changedMemory name position 1 baseline))))
  forM_ [0,60,120] $ \position ->
    assert ("custom projection excludes the original nonobservable image byte " ++ show position)
      (null (differences (changedMemory "custom-images" position 1 baseline)))
  forM_ [Observation.R4,Observation.R5,Observation.R6,Observation.R7,Observation.R8,Observation.R9,Observation.R10,Observation.R11,Observation.Sp] $ \register ->
    assert ("custom projection retains ABI register " ++ show register)
      (not (null (differences baseline {Observation.nativeRegisters=M.insert register 1 (Observation.nativeRegisters baseline)})))
  assert "custom projection excludes incidental r0 and unrelated raw memory"
    (null (differences baseline
      {Observation.nativeRegisters=M.insert Observation.R0 0xdeadbeef (Observation.nativeRegisters baseline)
      ,Observation.nativeMemory=(Observation.RegionId "unused-code",B.replicate 4096 0xff):Observation.nativeMemory baseline}))
  assert "custom projection retains every required region rather than equating missing images"
    (not (null (differences baseline {Observation.nativeMemory=filter ((/=Observation.RegionId "medium") . fst) (Observation.nativeMemory baseline)})))
  let extended=baseline {Observation.nativeMemory=[(identifier,if identifier == Observation.RegionId "results" then B.replicate 0x10004 0 else bytes) | (identifier,bytes) <- Observation.nativeMemory baseline]}
  assert "custom projection preserves the existing result-prefix comparison"
    (null (Custom.comparePair (Custom.projectCustomObservation extended) (Custom.projectCustomObservation (changedMemory "results" 0x10000 1 extended))))
  case filter Custom.noPersistence Custom.customCases of
    [] -> fail "custom persistence invariant fixture is absent"
    input:_ -> do
      let valid=baseline {Observation.nativeMemory=[(identifier,if identifier == Observation.RegionId "caller" then B.replicate 0x800 0xff else bytes) | (identifier,bytes) <- Observation.nativeMemory baseline]}
      assert "projected custom invariants retain the staging tail and persistence result"
        (null (Custom.customInvariants input (Custom.projectCustomObservation valid)))
      assert "projected custom invariants refuse persistence"
        (not (null (Custom.customInvariants input (Custom.projectCustomObservation (changedMemory "results" 0 1 valid)))))
      assert "projected custom invariants refuse a changed staging tail"
        (not (null (Custom.customInvariants input (Custom.projectCustomObservation (changedMemory "caller" 0x134 0 valid)))))
  forM_ [0,0x40000000] $ \alias -> do
    let mapped = Custom.ownedRecordRegions
          [mkRegion (Observation.RegionId "record-memory") (Observation.GuestAddress (fromInteger (0x89f8b0+alias))) (0x917fe4-0x89f8b0) ReadWrite (B.replicate (0x917fe4-0x89f8b0) 0)]
        accessible address permission = NativeCase.regionAt (map caseRegion mapped) (address+alias) 1 permission
    forM_ [0x89fa60,0x89fa67,0x8a0080,0x8c007f,0x8c0080,0x8e007f,0x8e0080,0x8e0087,0x8e0088,0x8e008b] $ \address ->
      assert ("both cache buffers and exact metadata are accessible: " ++ show (address+alias)) (accessible address 'r' && accessible address 'w')
    forM_ ([0x89f8f0..0x89f8f7] ++ [0x917fa8..0x917fe3]) $ \address ->
      assert ("obsolete cache gap and saved destinations are inaccessible: " ++ show (address+alias)) (not (accessible address 'r') && not (accessible address 'w'))
  putStrLn "settings storage model tests passed"
