-- | The linked images and mutations exercise interface refusals at their reported sites.
module Main (main) where

import ArmElf
import BankAllowlist
import BankAllowlistComposition (composeBankAllowlist)
import Control.Monad (forM_, unless)
import Data.Bits (shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (find)
import Data.Either (isLeft)
import Data.Word (Word32)
import RamPushLayout (modalEngineBankVariant)
import System.Directory (getTemporaryDirectory, removeFile)
import System.Environment (getArgs)
import System.Exit (ExitCode (..))
import System.IO (hClose, openBinaryTempFile)
import System.Process (readProcessWithExitCode)

check :: String -> Bool -> IO ()
check label passed = unless passed (fail label)

main :: IO ()
main = do
  [allowlistPath, checker, elfLow, elfHigh, engineElf, modalFragmentPath, engineFragmentPath] <- getArgs
  bytes <- B.readFile allowlistPath
  policy <- either fail pure (parseAllowlist bytes)
  forM_ [elfLow, elfHigh] $ \path -> do
    image <- B.readFile path
    report <- either fail pure (checkBankFile bytes image)
    check (path ++ ": " ++ show (map renderFinding (violations report))) (null (violations report))
  clean <- B.readFile elfLow
  elf <- either fail pure (parseElf clean)
  let entry = maybe (error "missing boundary entry") symbolValue (find ((== "rb_probe_entry") . symbolName) (elfSymbols elf))
      image word = either error id (parseElf (patch elf clean entry word))
      report rules subject = either error id (checkBank rules subject)
      refuses label rules subject address reason = check label
        (any (\f -> findingAddress f == address && findingReason f == reason) (violations (report rules subject)))
      direct target = 0xeb000000 .|. (((target - entry - 8) `shiftR` 2) .&. 0x00ffffff)
  refuses "undeclared direct stock call" policy (image (direct 0x00700000)) entry (UndeclaredStockTarget 0x00700000)
  check "register calls are not a binary placement constraint"
    (not (any ((== entry) . findingAddress) (violations (report policy (image 0xe12fff33)))))
  let literalSite = entry + 8
      withLiteral target = either error id (parseElf (patch elf (patch elf clean entry 0xe59f0000) literalSite target))
  check "unrelocated literal scalars are not classified as addresses"
    (null (violations (report policy (withLiteral 0x00900004))))
  relocations <- either fail pure (addressRelocations elf)
  let referenceLayout = either error id (readLayout elf)
      directReferences = [r | r <- relocations, addressKind r == DirectReference
        , textStart referenceLayout <= addressSite r, addressSite r < textEnd referenceLayout]
  check "direct relocation fixtures exist" (not (null directReferences))
  forM_ directReferences $ \reference -> do
    let site = addressSite reference
        instruction = maybe (error "missing relocated branch") id (wordAt elf site)
        retarget target opcode = opcode .|. (((target - site - 8) `shiftR` 2) .&. 0x00ffffff)
        mutated = either error id (parseElf (patch elf clean site (retarget 0x00700000 (instruction .&. 0xff000000))))
    refuses "relocated call or jump refuses undeclared target" policy mutated site (UndeclaredStockTarget 0x00700000)
    let stock = case stockCalls policy of call : _ -> stockCallAddress call; [] -> error "missing stock call fixture"
        interworking = either error id (parseElf (patch elf clean site (retarget stock 0xfa000000)))
    refuses "relocated BLX cannot enter an ARM stock entry" policy interworking site (UndeclaredStockTarget (stock .|. 1))
  let relocationTable = maybe (error "missing allocated relocation table") id
        (find (\section -> sectionType section == 9 && sectionSize section > 0) (elfSections elf))
      encodingOffset = fromIntegral (sectionOffset relocationTable) + 4
      encoding = sum [fromIntegral (B.index clean (encodingOffset + index)) * (256 ^ index) | index <- [0 .. 3 :: Int]] :: Word32
      unsupported = either error id (parseElf (patchFile clean encodingOffset ((encoding .&. 0xffffff00) .|. 255)))
      rela = elf {elfSections = [if sectionName s == sectionName relocationTable then s {sectionType = 4} else s | s <- elfSections elf]}
  check "unsupported allocated relocation kind refuses" (isLeft (checkBank policy unsupported))
  check "allocated RELA table refuses" (isLeft (checkBank policy rela))
  let relocation = maybe (error "missing bank data relocation") id
        (find (\r -> addressKind r == DataReference && textStart layoutForReferences <= addressSite r && addressSite r + 4 <= imageEnd layoutForReferences) relocations)
      relocationAddress = addressSite relocation
      relocated = either error id (parseElf (patch elf clean relocationAddress 0x00900004))
      layoutForReferences = either error id (readLayout elf)
  refuses "undeclared definite ELF address reference" policy relocated relocationAddress (ExternalAddress 0x00900004)
  let admitted = policy {stockRegions = StockRegion 0x00900004 4 "fixture" "synthetic" : stockRegions policy}
  check "declared region admits the same relocated reference"
    (null (violations (report admitted relocated)))
  let wide target = 0xe3000000 .|. ((target .&. 0xf000) * 16) .|. (target .&. 0xfff)
      wideImage = either error id (parseElf (patch elf (patch elf clean entry (wide 4)) (entry + 4) 0xe3400090))
  check "unrelocated movw/movt scalars are not classified as addresses"
    (null (violations (report policy wideImage)))
  let segment = Segment 1 0 entry 4 4 7
  refuses "writable executable segment" policy (elf {elfSegments = segment : elfSegments elf}) entry (LayoutProblem WritableExecutableSegment)
  let extra = Section ".unowned" 1 2 0x04000000 0 4
  refuses "allocated section outside layout" policy (elf {elfSections = extra : elfSections elf}) 0x04000000 (LayoutProblem SectionOutsideLayout)
  let resized = elf {elfSections = [if sectionName s == ".text" then s {sectionSize = sectionSize s + 4} else s | s <- elfSections elf]}
  layout <- either fail pure (readLayout elf)
  refuses "section extent disagrees with image" policy resized (textStart layout) (LayoutProblem (SectionExtent ".text"))
  let changeSymbol name value subject = subject {elfSymbols = [if symbolName s == name then s {symbolValue = value} else s | s <- elfSymbols subject]}
      changeSection name update subject = subject {elfSections = [if sectionName s == name then update s else s | s <- elfSections subject]}
      layoutCases =
        [ ("allocated data must have bytes", changeSection ".data" (\s -> s {sectionType = 8}) elf, dataStart layout, SectionNotProgbits)
        , ("bank executable offset", changeSymbol "__rb_image_start" (textStart layout + 4) elf, textStart layout + 4, BankCodeStart)
        , ("bank image cannot overlap generated data", changeSymbol "__rb_image_end" (generatedStart layout + 4) elf, generatedStart layout + 4, BankImageOverlap)
        , ("generated end", changeSymbol "RAM_PUSH_BANK_END" (bankEnd layout + 4) elf, bankEnd layout + 4, GeneratedEnd)
        , ("generated bundle overlap", changeSymbol "__ram_push_installer_start" (bankEnd layout - 4) elf, bankEnd layout, GeneratedBundleOverlap)
        , ("installer placement", changeSymbol "__ram_push_installer_start" (installerStart layout + 4) elf, installerStart layout + 4, InstallerPlacement)
        , ("runtime allocation extent", changeSymbol "__ram_push_end" (allocationEnd layout + 4) elf, allocationEnd layout + 4, RuntimeExtent)
        , ("installer section extent", changeSection ".ram_push.mailbox" (\s -> s {sectionAddress = installerStart layout - 4}) elf, installerStart layout - 4, InstallerSectionOutside)
        , ("A32 mapping required", elf {elfSymbols = Symbol "$t" entry 0 OtherSymbol (InSection 1) : elfSymbols elf}, entry, ThumbCode)
        , ("function extent stays in owned code", elf {elfSymbols = [if symbolName s == "rb_probe_entry" then s {symbolSize = textEnd layout - entry + 4} else s | s <- elfSymbols elf]}, entry, SectionExtent "function")
        ]
  forM_ layoutCases $ \(label, subject, address, reason) -> refuses label policy subject address (LayoutProblem reason)
  let stores = [0xe5801000, 0xe78d1100, 0xe10f0000, 0xe128f000, 0xe7c00011, 0xe320f003]
  forM_ stores $ \word -> check "owned internal instruction shapes are not an interface refusal"
    (not (any ((== entry) . findingAddress) (violations (report policy (image word)))))
  let symbol = maybe (error "missing entry symbol") id (find ((== "rb_probe_entry") . symbolName) (elfSymbols elf))
      aliased = elf {elfSymbols = symbol {symbolName = "alias"} : elfSymbols elf}
  check "aliases preserve function and instruction counts"
    (reportFunctions (report policy elf) == reportFunctions (report policy aliased)
      && reportInstructions (report policy elf) == reportInstructions (report policy aliased))
  engineBytes <- B.readFile engineElf
  modal <- B.readFile modalFragmentPath
  engineFragment <- B.readFile engineFragmentPath
  composed <- either fail pure (composeBankAllowlist modalEngineBankVariant
    [("menu/recipe_bank_allowlist.json", bytes), ("menu/recipe_bank_modal_allowlist_fragment.json", modal), ("menu/recipe_bank_menu_engine_allowlist_fragment.json", engineFragment)])
  engineReport <- either fail pure (checkBankFile composed engineBytes)
  check ("engine interfaces: " ++ show (map renderFinding (violations engineReport))) (null (violations engineReport))
  enginePolicy <- either fail pure (parseAllowlist composed)
  engineImage <- either fail pure (parseElf engineBytes)
  engineReferences <- either fail pure (addressRelocations engineImage)
  let wideReference = maybe (error "missing relocated MOVW") id (find (\r -> addressKind r == DataReference
        && maybe False (\word -> word .&. 0x0ff00000 == 0x03000000) (wordAt engineImage (addressSite r))) engineReferences)
      wideSite = addressSite wideReference
      upperSite = maybe (error "missing relocated MOVT") addressSite (find (\r -> addressKind r == DataReference
        && addressSite r > wideSite && addressTarget r == addressTarget wideReference
        && maybe False (\word -> word .&. 0x0ff00000 == 0x03400000) (wordAt engineImage (addressSite r))) engineReferences)
      wideWord = maybe (error "MOVW bytes missing") id (wordAt engineImage wideSite)
      upperWord = maybe (error "MOVT bytes missing") id (wordAt engineImage upperSite)
      changedPair = either error id (parseElf (patch engineImage
        (patch engineImage engineBytes wideSite ((wideWord .&. 0xfff0f000) .|. 4)) upperSite ((upperWord .&. 0xfff0f000) .|. 0x90)))
  changedReferences <- either fail pure (addressRelocations changedPair)
  check "relocated MOVW/MOVT resolves the linked address"
    (AddressRelocation wideSite 0x00900004 DataReference `elem` changedReferences
      && AddressRelocation upperSite 0x00900004 DataReference `elem` changedReferences)
  refuses "undeclared relocated MOVW/MOVT address" enginePolicy changedPair wideSite (ExternalAddress 0x00900004)
  engineLayout <- either fail pure (readLayout engineImage)
  case engine engineLayout of
    Nothing -> fail "missing engine layout"
    Just extents -> do
      refuses "engine data cannot precede engine code" enginePolicy
        (changeSymbol "__rb_menu_engine_data_start" (engineCodeStart extents - 4) engineImage)
        (engineCodeStart extents) (LayoutProblem EngineRegion)
      refuses "engine data permissions" enginePolicy
        (changeSection ".menu_engine_data" (\s -> s {sectionFlags = sectionFlags s .|. 4}) engineImage)
        (engineDataStart extents) (LayoutProblem (SectionExtent ".menu_engine_data"))
  let engineEntry = maybe (error "missing engine entry") symbolValue (find ((== "rb_menu_engine_entry") . symbolName) (elfSymbols engineImage))
      deep = either error id (parseElf (patch engineImage engineBytes engineEntry 0xe24dda01))
      recursive = either error id (parseElf (patch engineImage engineBytes engineEntry (0xebfffffe)))
  refuses "task stack budget" enginePolicy deep (textStart engineLayout) StackBudgetViolation
  refuses "direct recursion" enginePolicy recursive (textStart engineLayout) StackBudgetViolation
  temporary <- getTemporaryDirectory
  (cleanCode, _, _) <- readProcessWithExitCode checker [allowlistPath, elfLow] ""
  check "clean CLI exits zero" (cleanCode == ExitSuccess)
  (path, handle) <- openBinaryTempFile temporary "bank-interface.elf"
  B.hPut handle (patch elf clean entry (direct 0x00700000))
  hClose handle
  (badCode, _, _) <- readProcessWithExitCode checker [allowlistPath, path] ""
  removeFile path
  check "violating CLI exits one" (badCode == ExitFailure 1)
  putStrLn "bank interface refusals and owned instruction freedom passed"

patch :: Elf -> B.ByteString -> Word32 -> Word32 -> B.ByteString
patch elf bytes address word = case find (\s -> sectionAllocated s && sectionAddress s <= address && address + 4 <= sectionAddress s + sectionSize s) (elfSections elf) of
  Nothing -> error "fixture address outside ELF"
  Just section -> let offset = fromIntegral (sectionOffset section + address - sectionAddress section)
    in B.take offset bytes <> B.pack [fromIntegral (word `shiftR` shift) | shift <- [0, 8, 16, 24 :: Int]] <> B.drop (offset + 4) bytes

patchFile :: B.ByteString -> Int -> Word32 -> B.ByteString
patchFile bytes offset word = B.take offset bytes <> B.pack [fromIntegral (word `shiftR` shift) | shift <- [0, 8, 16, 24 :: Int]] <> B.drop (offset + 4) bytes
