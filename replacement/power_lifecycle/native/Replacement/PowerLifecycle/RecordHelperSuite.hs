{-# LANGUAGE OverloadedStrings #-}
module Replacement.PowerLifecycle.RecordHelperSuite (runSuite, sourceIdentity) where

import qualified ArmElf as E
import Control.Monad (forM, forM_, unless)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.CachedNativeBatch (cachedObservedBatch)
import Emulation.NativeObservation
import Emulation.OwnedCase
import Emulation.Sha256 (sha256, sha256Hex)
import System.FilePath ((</>))

sourceIdentity :: String
sourceIdentity = "x70view55436fc85c0560dd252b0410af6f2427fe198cd3; image4 base0061c400 SHA256 f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16; image5 base00bf1a00; stock extents 0x00ef9bc0-0x00f21a3c"

data Role = Stock | Owned deriving (Eq, Show)

-- | One stock body: its address, its image bytes extent, the argument registers
-- and the memory it reads or writes.
data Helper = Helper
  { helperId :: String
  , helperSymbol :: String
  , helperEntry :: Word32
  , helperSize :: Word32
  , helperImage :: Image
  , helperArguments :: [(Register, Word32)]
  , helperMemory :: [(RegionId, Word32, Int, B.ByteString)]
  , helperCompareMemory :: [RegionId]
  , helperReturns :: Bool
  }

data Image = Image4 | Image5 deriving (Eq, Show)

entry :: String -> Word32 -> Word32 -> Image -> [(Register, Word32)] -> [RegionId] -> Bool -> Helper
entry name address size image arguments compared returns = Helper ("record-helper-" ++ name) ("xm1_power_record_helper_" ++ name) address size image arguments [] compared returns

-- | The shared block at 0x0091_904c holds the two getters' words.
sharedBlock :: B.ByteString
sharedBlock = B.pack [fromIntegral (index * 37 + 11) | index <- [0 .. 0xfff :: Int]]

listsBlock :: B.ByteString
listsBlock = B.replicate 0x2000 0

displayBlock :: B.ByteString
displayBlock = B.replicate 0x40 0xa5

jobBlock :: B.ByteString
jobBlock = B.pack (0xa5 : 0xa5 : 0xa5 : 0xa5 : [0xef, 0xbe, 0xad, 0xde] ++ replicate 8 0xa5)

withBlock :: Helper -> B.ByteString -> RegionId -> Word32 -> Helper
withBlock helper bytes ident base = helper { helperMemory = [(ident, base, B.length bytes, bytes)], helperCompareMemory = [ident] }

helpers :: [Helper]
helpers =
  [ entry "ef9bc0" 0x00ef9bc0 28 Image5 [] [] True
  , entry "ef9c00" 0x00ef9c00 28 Image5 [] [] True
  , entry "ef9d18" 0x00ef9d18 28 Image5 [] [] True
  , entry "ef9dc0" 0x00ef9dc0 28 Image5 [] [] True
  , entry "ef9ddc" 0x00ef9ddc 28 Image5 [] [] True
  , entry "ef9e14" 0x00ef9e14 28 Image5 [] [] True
  , entry "ef9e4c" 0x00ef9e4c 28 Image5 [] [] True
  , entry "ef9e84" 0x00ef9e84 28 Image5 [] [] True
  , entry "ef9ea0" 0x00ef9ea0 28 Image5 [] [] True
  , entry "ef9ebc" 0x00ef9ebc 28 Image5 [] [] True
  , entry "ef9ed8" 0x00ef9ed8 28 Image5 [] [] True
  , entry "ef9f10" 0x00ef9f10 28 Image5 [] [] True
  , entry "ef9f64" 0x00ef9f64 28 Image5 [] [] True
  , withBlock (entry "75a440" 0x0075a440 36 Image4 [] [] True) sharedBlock (RegionId "shared") 0x0091_904c
  , withBlock (entry "75a61c" 0x0075a61c 32 Image4 [] [] True) sharedBlock (RegionId "shared") 0x0091_904c
  , withBlock (entry "f160ac" 0x00f160ac 36 Image5 [(R0, 0x9abc_def0)] [] False) listsBlock (RegionId "lists") 0x0169_e580
  , withBlock (entry "f1fd84" 0x00f1fd84 36 Image5 [] [] False) displayBlock (RegionId "display") 0x016b_5284
  , withBlock (entry "f21a3c" 0x00f21a3c 32 Image5 [] [] True) jobBlock (RegionId "job") 0x016b_5428
  ]

imageBase :: Image -> Word32
imageBase Image4 = 0x0061c400
imageBase Image5 = 0x00bf1a00

stop :: Word32
stop = 0x0020_0000

makeCase :: B.ByteString -> B.ByteString -> E.Elf -> Role -> Helper -> CaseSpec
makeCase image4 image5 elf role helper = CaseSpec
  { caseId = helperId helper
  , caseProvenance = sourceIdentity
  , caseExclusion = "Leaf stock body over declared ordinary memory only: no MMIO, no undefined callee, no interrupt, timing or physical device. The owned side runs the Rust body at its linked address."
  , caseEntry = guest entryAddress
  , caseExit = guest stop
  , caseRegions = code ++ memory ++ [stack, returnRegion]
  , caseFunctions = if role == Stock then [(guest (helperEntry helper), helperSize helper)] else M.toList (M.fromListWith max [(guest (E.symbolValue s), E.symbolSize s) | s <- E.elfSymbols elf, E.symbolKind s == E.FunctionSymbol, E.symbolSize s > 0])
  , caseInitial = [(r, maybe (seed r) id (lookup r (helperArguments helper))) | r <- [R0 .. R12]] ++ [(Sp, 0x0030_8000), (Lr, stop), (Cpsr, 0x10)]
  , caseExpectedStatus = Returned
  , caseExpectedRegisters = [(Sp, 0x0030_8000), (Lr, stop)]
  , caseExpectedMemory = []
  , caseIgnoredMemory = []
  , caseCp14Identity = Nothing
  , caseKernel = Nothing
  }
  where
    guest = GuestAddress . fromIntegral
    entryAddress = if role == Stock then helperEntry helper else case [E.symbolValue s | s <- E.elfSymbols elf, E.symbolName s == helperSymbol helper] of
      [value] -> value
      _ -> error ("admitted symbol disappeared: " ++ helperSymbol helper)
    bytes = if helperImage helper == Image4 then image4 else image5
    base = imageBase (helperImage helper)
    slice address size = B.take (fromIntegral size) (B.drop (fromIntegral (address - base)) bytes)
    allocated = filter (\s -> E.sectionAllocated s && E.sectionSize s > 0) (E.elfSections elf)
    code = if role == Stock
      then [mkRegion (RegionId ("stock-" ++ helperId helper)) (guest (helperEntry helper)) (fromIntegral (helperSize helper)) ReadExecute (slice (helperEntry helper) (helperSize helper))]
      else [mkRegion (RegionId ("owned-" ++ show (E.sectionAddress s))) (guest (E.sectionAddress s)) (fromIntegral (E.sectionSize s)) ReadExecute (B.pack [maybe (error "admitted ELF hole") id (E.byteAt elf a) | a <- [E.sectionAddress s .. E.sectionAddress s + E.sectionSize s - 1]]) | s <- allocated]
    memory = [mkRegion ident (guest address) size ReadWrite content | (ident, address, size, content) <- helperMemory helper]
    stack = mkRegion (RegionId "stack") (guest 0x0030_0000) 0x10000 ReadWrite (B.replicate 0x10000 0xa5)
    returnRegion = mkRegion (RegionId "return") (guest stop) 4 ReadExecute (word32le 0xe7f000f0)

seed :: Register -> Word32
seed r = 0xa5a5_0000 + fromIntegral (fromEnum r)

compareResult :: Helper -> NativeObservation -> NativeObservation -> [String]
compareResult helper stock owned =
  [label ++ " status" | (label, o) <- [("stock", stock), ("owned", owned)], nativeStatus o /= Returned] ++
  ["return register" | helperReturns helper, registerValue stock R0 /= registerValue owned R0] ++
  ["memory" | ident <- helperCompareMemory helper, regionBytesOf stock ident /= regionBytesOf owned ident] ++
  ["caller frame" | B.drop 0x8000 (regionBytesOf stock (RegionId "stack")) /= B.replicate 0x8000 0xa5 || B.drop 0x8000 (regionBytesOf owned (RegionId "stack")) /= B.replicate 0x8000 0xa5]

runSuite :: FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO [(String, [String])]
runSuite runner cache directory image4Path image5Path elfPath = do
  image4 <- B.readFile image4Path
  image5 <- B.readFile image5Path
  unless (sha256Hex (sha256 image4) == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") (fail "image4 identity differs")
  elf <- either fail pure . E.parseElf =<< B.readFile elfPath
  forM_ helpers $ \helper -> case [s | s <- E.elfSymbols elf, E.symbolName s == helperSymbol helper, E.symbolKind s == E.FunctionSymbol, E.symbolSize s > 0] of
    [_] -> pure ()
    _ -> fail ("missing owned function: " ++ helperSymbol helper)
  forM_ (filter (\s -> E.sectionAllocated s && E.sectionSize s > 0) (E.elfSections elf)) $ \s ->
    unless (E.sectionType s == 1 && not (E.sectionWritable s) && E.sectionAddress s >= 0x100000 && toInteger (E.sectionAddress s) + toInteger (E.sectionSize s) <= 0x180000) (fail "owned allocated section escapes admitted bank")
  fmap concat $ forM (zip [0 :: Int ..] (chunks helpers)) $ \(index, selected) -> do
    observations <- forM [Stock, Owned] $ \role -> do
      batch <- writeBatch (directory </> show index </> show role) (map (makeCase image4 image5 elf role) selected)
      (_, results) <- cachedObservedBatch cache runner batch
      pure (M.fromList [(caseIdText key, value) | (key, value) <- results])
    case observations of
      [stock, owned] -> pure [(helperId helper, case (M.lookup (helperId helper) stock, M.lookup (helperId helper) owned) of
          (Just a, Just b) -> compareResult helper a b
          _ -> ["missing observation"]) | helper <- selected]
      _ -> fail "role count"
  where
    chunks [] = []
    chunks rest = let (batch, remaining) = splitAt 8 rest in batch : chunks remaining
