{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (unless)
import Data.Aeson (eitherDecodeStrict', encode, object, (.=))
import Data.Bits (shiftR)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as L
import Data.Either (isRight)
import Data.List (mapAccumL)
import Data.Word (Word32, Word8)
import qualified FujiAF.Host as H
import ShimCatalog
import System.Directory (createDirectoryIfMissing, removeDirectoryRecursive)
import System.FilePath ((</>))
import System.IO.Error (tryIOError)

main :: IO ()
main = H.withTemporary "shim-check-behavior" $ \root -> do
  let bytes = fixture [("xm1_fixture",1)]
      digest = digestBytes bytes
      image = Image (ImageId "//fixture:image") (ContractPath "fixture.elf") [UnitId "fixture"] Production [] [SymbolName "xm1_fixture"] [] (Just digest) []
      evidence = Evidence (Just (imageId image)) Export [SymbolName "xm1_fixture"] [] [] Nothing "retained caller enters the linked fixture export"
      pending = Predicates Unestablished Unestablished Unestablished
      row = Row (ShimId "fixture-entry") "fixture export" StockExport "retained caller" Owned "all callers use the direct owned entry" Required [evidence] pending Nothing []
      unit = Unit (UnitId "fixture") "Synthetic behavioral fixture, not firmware evidence." LinkedUnit [row]
      manifest = Manifest [image] []
      run owners images = checkAll root owners images
      expect code action = action >>= \result -> unless (refusalCode result == Just code) (H.failH ("expected " ++ show code ++ ", got " ++ show result))
      withRow replacement = [unit {unitRows = [replacement]}]
      fixtureDirectory = root </> "replacement/fixture"
  createDirectoryIfMissing True fixtureDirectory
  writeFile (fixtureDirectory </> "BUILD.bazel") "xm1_rust_elf(name = \"image\", asm_srcs = [\"entry.S\"])\n"
  writeFile (fixtureDirectory </> "entry.S") ""
  B.writeFile (root </> "fixture.elf") bytes
  accepted <- run [unit] manifest
  unless (accepted == Right [ImageOutcome (imageId image) digest [RowOutcome (rowId row) Unestablished Required]]) (H.failH "owned other side must remain pending without comparison and caller closure")
  expect MissingDeclaration (run [] manifest)
  expect MissingDeclaration (run [unit {unitRows = []}] manifest)
  expect MissingExport (run (withRow row {rowEvidence = [evidence {evidenceSymbols = [SymbolName "xm1_absent"]}]}) manifest)
  expect StaleArtifact (run [unit] (Manifest [image {imageExpected = Just (Digest (replicate 64 '0'))}] []))
  expect StaleArtifact (run [unit] (Manifest [image {imageExpected = Nothing}] []))
  expect FalseRemoval (run (withRow row {rowState = Removed}) manifest)
  expect FalseRemoval (run (withRow row {rowState = ConditionMet}) manifest)
  expect FalseRemoval (run (withRow row {rowState = Removed,rowPredicates = Predicates Established Established Established}) manifest)
  expect ContradictorySeam (run (withRow row {rowEvidence = [evidence {evidenceImage = Just (ImageId "//fixture:other")}]}) (Manifest [image,image {imageId = ImageId "//fixture:other",imageElf = ContractPath "other.elf",imageUnits = []}] []))
  expect UnboundSeam (run (withRow row {rowEvidence = [evidence {evidenceKind = Layout,evidenceSymbols = [],evidenceContract = Just (ContractPath "fixture.ld")}]}) manifest)
  let importedBytes = fixture [("xm1_fixture",1),("retained_service",0)]
      imported = image {imageExpected = Just (digestBytes importedBytes)}
  B.writeFile (root </> "fixture.elf") importedBytes
  expect UnexpectedImport (run [unit] (Manifest [imported] []))
  let absoluteBytes = fixture [("xm1_fixture",1),("retained_service",0xfff1)]
      absolute = image {imageExpected = Just (digestBytes absoluteBytes)}
  B.writeFile (root </> "fixture.elf") absoluteBytes
  expect UnexpectedImport (run [unit] (Manifest [absolute] []))
  let localBytes = fixtureWithBindings [("xm1_fixture",1,0x12),("rb_private",1,0x02)]
  B.writeFile (root </> "fixture.elf") localBytes
  localResult <- run [unit] (Manifest [image {imageExpected = Just (digestBytes localBytes)}] [])
  unless (isRight localResult) (H.failH "a private local function is not an exported shim")
  let unprefixedBytes = fixture [("xm1_fixture",1),("stock_facade",1)]
  B.writeFile (root </> "fixture.elf") unprefixedBytes
  expect MissingDeclaration (run [unit] (Manifest [image {imageExpected = Just (digestBytes unprefixedBytes)}] []))
  let privateImportBytes = fixture [("xm1_fixture",1),("__retained_service",0xfff1)]
  B.writeFile (root </> "fixture.elf") privateImportBytes
  expect UnexpectedImport (run [unit] (Manifest [image {imageExpected = Just (digestBytes privateImportBytes)}] []))
  B.writeFile (root </> "fixture.elf") bytes
  writeFile (root </> "fixture.ld") "Synthetic fixture layout contract.\n"
  let layout = evidence {evidenceKind = Layout,evidenceSymbols = [],evidenceAddresses = [GuestAddress 0x1000],evidenceContract = Just (ContractPath "fixture.ld")}
  layoutResult <- run (withRow row {rowEvidence = [evidence,layout]}) manifest
  unless (isRight layoutResult) (H.failH "linked layout anchor should be admitted")
  expect UnboundSeam (run (withRow row {rowEvidence = [evidence,layout {evidenceAddresses = [GuestAddress 0x2000]}]}) manifest)
  expect UnboundSeam (run (withRow row {rowEvidence = [evidence,layout {evidenceAddresses = [],evidenceSymbols = [SymbolName "xm1_fixture"],evidenceTargets = [GuestAddress 0x65722c]}]}) manifest)
  let directBytes = fixtureWithInstructions [0xeb0007fe] [("xm1_fixture",1,0x12)]
  B.writeFile (root </> "fixture.elf") directBytes
  expect UnexpectedImport (run [unit] (Manifest [image {imageExpected = Just (digestBytes directBytes)}] []))
  let computedBytes = fixtureWithInstructions [0xe59f5010,0xe12fff35,0xe2857f69,0xe12fff37,0xe12fff1e,0xe1a00000,0x2000] [("xm1_fixture",1,0x12)]
      computedImage = image {imageExpected = Just (digestBytes computedBytes)}
      retained = evidence {evidenceKind = RetainedEntry,evidenceTargets = [GuestAddress 0x2000,GuestAddress 0x21a4],evidenceContract = Just (ContractPath "fixture.ld")}
  B.writeFile (root </> "fixture.elf") computedBytes
  computedResult <- run (withRow row {rowEvidence = [evidence,retained]}) (Manifest [computedImage] [])
  unless (isRight computedResult) (H.failH "constant retained calls preserve callee-saved address provenance")
  expect UnexpectedImport (run (withRow row {rowEvidence = [evidence,retained {evidenceTargets = [GuestAddress 0x2000]}]}) (Manifest [computedImage] []))
  let scalarBytes = fixtureWithInstructions [0xe12fff1e,0x3000] [("xm1_fixture",1,0x12)]
  B.writeFile (root </> "fixture.elf") scalarBytes
  expect UnboundSeam (run (withRow row {rowEvidence = [evidence,retained {evidenceTargets = [GuestAddress 0x3000]}]}) (Manifest [image {imageExpected = Just (digestBytes scalarBytes)}] []))
  B.writeFile (root </> "fixture.elf") bytes
  expect StaleArtifact (run [unit] (Manifest [image {imageQualified = [PinnedArtifact (ContractPath "fixture.ld") (Digest (replicate 64 '0'))]}] []))
  let falseRow = row {rowPredicates = Predicates Refuted Unestablished Established}
  falseResult <- run (withRow falseRow) manifest
  unless (falseResult == Right [ImageOutcome (imageId image) digest [RowOutcome (rowId row) Refuted Required]]) (H.failH "false differs from unestablished")
  let Digest imageHash = digest
      proofBytes = L.toStrict (encode (object ["schema" .= (1 :: Int),"image" .= ("//fixture:image" :: String),"image_sha256" .= imageHash,"compared_units" .= (["fixture","fixture-caller"] :: [String]),"predicates" .= object ["caller_closure" .= ("true" :: String),"same_image_comparison" .= ("true" :: String),"direct_owned_api" .= ("true" :: String)],"accepted" .= True]))
      comparison = ComparisonReceipt (ContractPath "comparison.json") (digestBytes proofBytes) digest
      removedRow = row {rowId = ShimId "retired-entry",rowState = Removed,rowPredicates = Predicates Established Established Established,rowComparison = Just comparison,rowEvidence = [evidence {evidenceSymbols = [SymbolName "xm1_retired"]}]}
      caller = Unit (UnitId "fixture-caller") "Synthetic same-image caller." ModuleUnit []
      joined = image {imageUnits = [unitId unit,unitId caller]}
  B.writeFile (root </> "comparison.json") proofBytes
  removedResult <- run [unit {unitRows = [row,removedRow]},caller] (Manifest [joined] [])
  unless (removedResult == Right [ImageOutcome (imageId image) digest [RowOutcome (rowId row) Unestablished Required,RowOutcome (rowId removedRow) Established Removed]]) (H.failH "qualified same-image removal with absent seam should pass")
  let missingConsumer = Unit (UnitId "fixture-third-consumer") "Declared consumer outside this image." ModuleUnit []
  expect FalseRemoval (run [unit {unitRows = [row,removedRow {rowConsumers = [unitId missingConsumer]}]},caller,missingConsumer] (Manifest [joined] []))
  expect FalseRemoval (run [unit {unitRows = [row,removedRow {rowEvidence = [evidence]}]},caller] (Manifest [joined] []))
  B.writeFile (root </> "comparison.json") "{}"
  expect StaleArtifact (run [unit {unitRows = [row,removedRow]},caller] (Manifest [joined] []))
  let nonlinked = row {rowEvidence = [Evidence Nothing Reference [] [] [] (Just (ContractPath "fixture.ld")) "no linked production artifact exists"]}
      referenceUnit = unit {unitClass = ReferenceUnit,unitRows = [nonlinked]}
  referenceResult <- run [referenceUnit] (Manifest [] [ReferenceArtifact (ImageId "//fixture:reference") [UnitId "fixture"] (ContractPath "fixture.ld") "frozen reference-only fixture"])
  unless (referenceResult == Right []) (H.failH "nonlinked reference cannot masquerade as a checked image")
  unless (either (const True) (const False) (eitherDecodeStrict' "\"maybe\"" :: Either String Truth)) (H.failH "unknown predicate must be rejected at boundary")
  let regression :: String -> ErrorCode -> B.ByteString -> [Unit] -> Image -> IO (String,ErrorCode,Maybe ErrorCode)
      regression label expected fixtureBytes owners candidate = do
        B.writeFile (root </> "fixture.elf") fixtureBytes
        observed <- run owners (Manifest [candidate {imageExpected = Just (digestBytes fixtureBytes)}] [])
        pure (label,expected,refusalCode observed)
      conditionalBytes = fixtureWithInstructions [0x059ff000,0xe12fff1e,0x2000] [("xm1_fixture",1,0x12)]
      untypedBytes = fixtureWithBindings [("xm1_fixture",1,0x10)]
      privateExportBytes = fixtureWithBindings [("xm1_fixture",1,0x02)]
      absoluteExportBytes = fixtureWithBindings [("xm1_fixture",0xfff1,0x12)]
      foreignProof = L.toStrict (encode (object ["schema" .= (1 :: Int),"image" .= ("//fixture:image" :: String),"image_sha256" .= imageHash,"compared_units" .= (["fixture","not-linked"] :: [String]),"predicates" .= object ["caller_closure" .= ("true" :: String),"same_image_comparison" .= ("true" :: String),"direct_owned_api" .= ("true" :: String)],"accepted" .= True]))
      foreignComparison = ComparisonReceipt (ContractPath "foreign-comparison.json") (digestBytes foreignProof) digest
  B.writeFile (root </> "foreign-comparison.json") foreignProof
  regressions <- sequence
    [ regression "conditional constant PC transfer" UnexpectedImport conditionalBytes [unit] image
    , regression "allowlisted import without family" MissingDeclaration absoluteBytes [unit] image {imageImports = [SymbolName "retained_service"]}
    , regression "comparison participant outside image" FalseRemoval bytes [unit {unitRows = [row,removedRow {rowComparison = Just foreignComparison}]}] image
    , regression "untyped global without family" MissingDeclaration untypedBytes [unit {unitRows = []}] image
    , regression "required export is local" MissingExport privateExportBytes [unit] image
    , regression "family export is local" MissingExport privateExportBytes [unit] image {imageExports = []}
    , regression "required export is absolute" MissingExport absoluteExportBytes [unit] image {imageImports = [SymbolName "xm1_fixture"]}
    , regression "family export is absolute" MissingExport absoluteExportBytes [unit] image {imageExports = [],imageImports = [SymbolName "xm1_fixture"]}
    , regression "linked symbol claimed reference-only" ContradictorySeam bytes [unit {unitRows = [row,nonlinked {rowId = ShimId "false-reference",rowEvidence = [Evidence Nothing Reference [SymbolName "xm1_fixture"] [] [] (Just (ContractPath "fixture.ld")) "this entry is claimed absent"]}]}] image
    ]
  B.writeFile (root </> "fixture.elf") untypedBytes
  declaredUntyped <- run [unit] (Manifest [image {imageExpected = Just (digestBytes untypedBytes)}] [])
  unless (isRight declaredUntyped) (H.failH "declared global untyped entry should remain admitted")
  B.writeFile (root </> "fixture.elf") absoluteBytes
  declaredImport <- run (withRow row {rowEvidence = [evidence,evidence {evidenceKind = Import,evidenceSymbols = [SymbolName "retained_service"]}]}) (Manifest [image {imageImports = [SymbolName "retained_service"],imageExpected = Just (digestBytes absoluteBytes)}] [])
  unless (isRight declaredImport) (H.failH "allowlisted import with its family should remain admitted")
  -- A constant branch to a global label one past .text (0x100c) is not allocated; only this image's own publication
  -- row may declare it.
  let endpointBytes = fixtureWithValues [0xe59f1000,0xe12fff11,0x100c] [("xm1_fixture",1,0x12,0x1000),("__fixture_end",1,0x10,0x100c)]
      endpointImage = image {imageExpected = Just (digestBytes endpointBytes)}
      endpointLabel = evidence {evidenceKind = Publication,evidenceSymbols = [SymbolName "__fixture_end"],evidenceContract = Just (ContractPath "fixture.ld")}
  B.writeFile (root </> "fixture.elf") endpointBytes
  expect UnexpectedImport (run (withRow row {rowEvidence = [evidence,endpointLabel {evidenceKind = Layout}]}) (Manifest [endpointImage] []))
  endpointResult <- run (withRow row {rowEvidence = [evidence,endpointLabel]}) (Manifest [endpointImage] [])
  unless (isRight endpointResult) (H.failH "a published endpoint label of this image should bound its own reference")
  let undeclaredDirectory = root </> "replacement/undeclared"
      rootDirectory = root </> "replacement/rtos_timer"
  createDirectoryIfMissing True undeclaredDirectory
  writeFile (undeclaredDirectory </> "BUILD.bazel") "xm1_rust_elf(name = \"new-image\", asm_srcs = [\"entry.S\"])\n"
  writeFile (undeclaredDirectory </> "entry.S") ""
  undeclared <- regression "new firmware unit without declaration" MissingDeclaration bytes [unit] image
  removeDirectoryRecursive undeclaredDirectory
  createDirectoryIfMissing True rootDirectory
  writeFile (rootDirectory </> "BUILD.bazel") "_ROOT_LINKED_INPUTS = [\"//fixture:image.bin\", \"//fixture:image.symbols\"]\nhaskell_binary(name = \"stage-root-platform\", data = _ROOT_LINKED_INPUTS)\n"
  baselineComposition <- run [unit] manifest
  unless (isRight baselineComposition) (H.failH "declared Root constituent should remain admitted")
  writeFile (rootDirectory </> "BUILD.bazel") "_ROOT_LINKED_INPUTS = [\"//fixture:image.bin\", \"//fixture:image.symbols\", \"//fixture:new.bin\", \"//fixture:new.symbols\"]\nhaskell_binary(name = \"stage-root-platform\", data = _ROOT_LINKED_INPUTS)\n"
  missingConstituent <- regression "new Root constituent without image binding" MissingDeclaration bytes [unit] image
  writeFile (rootDirectory </> "BUILD.bazel") "_STARTUP_INPUTS = [\"//fixture:image.bin\"]\n_PLATFORM_INPUTS = [\"//fixture:new.bin\"]\n_ROOT_LINKED_INPUTS = _STARTUP_INPUTS + _PLATFORM_INPUTS\nhaskell_binary(name = \"stage-root-platform\", data = _ROOT_LINKED_INPUTS)\n"
  aliasedConstituent <- regression "Root alias concatenation hides platform constituent" MissingDeclaration bytes [unit] image
  writeFile (rootDirectory </> "BUILD.bazel") "_STARTUP_INPUTS = [\"//fixture:image.bin\", \"//fixture:new.bin\"]\n_PLATFORM_INPUTS = []\n_ROOT_LINKED_INPUTS = _STARTUP_INPUTS + _PLATFORM_INPUTS\nhaskell_binary(name = \"stage-root-platform\", data = _ROOT_LINKED_INPUTS)\n"
  aliasedStartup <- regression "Root alias concatenation hides startup constituent" MissingDeclaration bytes [unit] image
  writeFile (rootDirectory </> "BUILD.bazel") "haskell_binary(name = \"stage-root-platform\", data = [\"//fixture:image.bin\"])\n"
  createDirectoryIfMissing True undeclaredDirectory
  writeFile (undeclaredDirectory </> "entry.rs") ""
  writeFile (undeclaredDirectory </> "BUILD.bazel") "genrule(name = \"firmware\", srcs = [\"entry.rs\"], outs = [\"firmware.o\"], cmd = \"output=$$(nix-build --no-out-link firmware.nix); $(location @lab_lld//:bin/ld.lld) -m armelf -r $$output/lib/firmware.a -o $@\", tools = [\"@lab_lld//:bin/ld.lld\"])\n"
  nativeNixUnit <- regression "Nix ARM-link producer without declaration" MissingDeclaration bytes [unit] image
  removeDirectoryRecursive undeclaredDirectory
  let failures = [(label,expected,observed) | (label,expected,observed) <- regressions ++ [undeclared,missingConstituent,aliasedConstituent,aliasedStartup,nativeNixUnit], observed /= Just expected]
  mapM_ print failures
  unless (null failures) (H.failH "linked admission regressions failed")
  writeFile (rootDirectory </> "BUILD.bazel") "_STARTUP_INPUTS = [\"//fixture:image.bin\"]\n_PLATFORM_INPUTS = []\n_ALIAS = (_STARTUP_INPUTS + _PLATFORM_INPUTS)\n_ROOT_LINKED_INPUTS = _ALIAS\nhaskell_binary(name = \"stage-root-platform\", data = _ROOT_LINKED_INPUTS)\n"
  aliasedBaseline <- run [unit] manifest
  unless (aliasedBaseline == accepted) (H.failH "alias and concatenation refactoring changes admitted composition")
  mapM_ (\declarations -> do
    writeFile (rootDirectory </> "BUILD.bazel") (declarations ++ "\nhaskell_binary(name = \"stage-root-platform\", data = _ROOT_LINKED_INPUTS)\n")
    attempt <- tryIOError (run [unit] manifest)
    unless (either (const True) (either (const True) (const False)) attempt) (H.failH "unresolved or unsupported composition metadata was admitted"))
    [ "_ROOT_LINKED_INPUTS = _MISSING"
    , "_ROOT_LINKED_INPUTS = _ALIAS\n_ALIAS = _ROOT_LINKED_INPUTS"
    , "_STARTUP_INPUTS = [\"//fixture:image.bin\"]\n_ROOT_LINKED_INPUTS = _STARTUP_INPUTS[0:]"
    , "_ROOT_LINKED_INPUTS = []"
    , "_ROOT_LINKED_INPUTS = [\"//fixture:image.bin\"]\n_ROOT_LINKED_INPUTS += [\"//fixture:new.bin\"]"
    , "_ROOT_LINKED_INPUTS = [\"//fixture:image.bin\"]\n_ROOT_LINKED_INPUTS.append(\"//fixture:new.bin\")"
    , "_ROOT_LINKED_INPUTS = [\"//fixture:image.bin\"]\n_ROOT_LINKED_INPUTS = [\"//fixture:new.bin\"]"
    ]
  mutationOutcomes <- mapM (\(label,declarations) -> do
    writeFile (rootDirectory </> "BUILD.bazel") (declarations ++ "\nhaskell_binary(name = \"stage-root-platform\", data = _ROOT_LINKED_INPUTS)\n")
    attempt <- tryIOError (run [unit] manifest)
    pure (label,either (const True) (either (const True) (const False)) attempt))
    [ ("mutation through reverse alias" :: String,"_ROOT_LINKED_INPUTS = [\"//fixture:image.bin\"]\n_ALIAS = _ROOT_LINKED_INPUTS\n_ALIAS.append(\"//fixture:new.bin\")")
    , ("indexed constituent replacement","_ROOT_LINKED_INPUTS = [\"//fixture:image.bin\"]\n_ROOT_LINKED_INPUTS[0] = \"//fixture:new.bin\"")
    , ("mutation assigned to unrelated binding","_ROOT_LINKED_INPUTS = [\"//fixture:image.bin\"]\n_ALIAS = _ROOT_LINKED_INPUTS\n_UNUSED = _ALIAS.append(\"//fixture:new.bin\")")
    , ("augmentation through reverse alias","_ROOT_LINKED_INPUTS = [\"//fixture:image.bin\"]\n_ALIAS = _ROOT_LINKED_INPUTS\n_ALIAS += [\"//fixture:new.bin\"]")
    ]
  let admittedMutations = filter (not . snd) mutationOutcomes
  mapM_ print admittedMutations
  unless (null admittedMutations) (H.failH "mutable Root composition metadata was admitted")
  putStrLn "shim checker behavioral admission and refusal cases passed"

refusalCode :: Either Refusal a -> Maybe ErrorCode
refusalCode (Left (Refusal code _)) = Just code
refusalCode (Right _) = Nothing

fixture :: [(String,Word32)] -> B.ByteString
fixture functions = fixtureWithBindings [(name,sectionIndex,0x12) | (name,sectionIndex) <- functions]

fixtureWithBindings :: [(String,Word32,Word8)] -> B.ByteString
fixtureWithBindings = fixtureWithInstructions [0xe12fff1e]

fixtureWithInstructions :: [Word32] -> [(String,Word32,Word8)] -> B.ByteString
fixtureWithInstructions instructions functions = fixtureWithValues instructions [(name,sectionIndex,binding,0x1000) | (name,sectionIndex,binding) <- functions]

fixtureWithValues :: [Word32] -> [(String,Word32,Word8,Word32)] -> B.ByteString
fixtureWithValues instructions functions = B.concat [header,B.replicate (0x100 - B.length header) 0,text,names,strings,symbols,sections]
  where
    text = B.concat (map w32 instructions)
    names = C.pack "\0.text\0.shstrtab\0.strtab\0.symtab\0"
    strings = B.singleton 0 <> C.pack (concatMap (\(name,_,_,_) -> name ++ "\0") functions)
    offsets = snd (mapAccumL (\offset (name,_,_,_) -> (offset + length name + 1,offset)) 1 functions)
    symbols = B.replicate 16 0 <> B.concat [w32 (fromIntegral offset) <> w32 value <> w32 (fromIntegral (B.length text)) <> B.pack [binding,0] <> w16 sectionIndex | (offset,(_,sectionIndex,binding,value)) <- zip offsets functions]
    namesAt = 0x100 + B.length text
    stringsAt = namesAt + B.length names
    symbolsAt = stringsAt + B.length strings
    sectionsAt = symbolsAt + B.length symbols
    section :: Word32 -> Word32 -> Word32 -> Word32 -> Int -> Int -> Word32 -> Word32 -> B.ByteString
    section name kind flags address offset size link entsize = B.concat (map w32 [name,kind,flags,address,fromIntegral offset,fromIntegral size,link,0,4,entsize])
    sections = B.concat [B.replicate 40 0,section 1 1 6 0x1000 0x100 (B.length text) 0 0,section 7 3 0 0 namesAt (B.length names) 0 0,section 17 3 0 0 stringsAt (B.length strings) 0 0,section 25 2 0 0 symbolsAt (B.length symbols) 3 16]
    header = B.pack [0x7f,69,76,70,1,1,1] <> B.replicate 9 0 <> w16 2 <> w16 40 <> w32 1 <> w32 0x1000 <> w32 0 <> w32 (fromIntegral sectionsAt) <> w32 0 <> w16 52 <> w16 32 <> w16 0 <> w16 40 <> w16 5 <> w16 2

w32 :: Word32 -> B.ByteString
w32 word = B.pack [fromIntegral (word `shiftR` shift) | shift <- [0,8,16,24]]
w16 :: Word32 -> B.ByteString
w16 = B.take 2 . w32
