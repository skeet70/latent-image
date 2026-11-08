module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (Value (..), eitherDecodeStrict', encode, object, (.=))
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.List (mapAccumL)
import qualified Data.IntMap.Strict as IM
import Data.Word (Word32)
import Numeric (showHex)
import RuntimeRelocationCompare
import qualified RamPushLayout as L
import System.Exit (exitFailure)

data Placement = Placement { base :: Word32, names :: Word32, installer :: Word32 }
data Damage = Clean | ChangedStockTarget | ChangedRegister | CaptureBL | CaptureWrongTarget | MissingCodeMap | CodeGap | NonzeroGap | ChangedData | ManifestDrift | RuntimeCodeDrift | ChangedLiteralLoad | MissingCompanion | AddressLikeScalar | StalePointer | StaleGeneratedPointer | MissingRelocations | MissingPointerRecord | NameConsumerToOtherLiteral | NameConsumerNotLoad | StaleNameLiteral | ShiftedNameLiteral | MissingNameFunction | StaleD3Publication | StalePlanPublication | WrongFixedPublication | AddressLikePublicationScalar | StalePublicationBranch | StalePublicationWide | ModalWrongController | ModalWrongFlags | ModalStockResume | ModalOwnedTarget | ModalBL | ModalWrongPush | ModalWrongSize | MissingModalHook | ReorderedPublications | WrongGroupDestination deriving (Eq, Show)

leftPlacement, rightPlacement :: Placement
leftPlacement = Placement 0x00c40000 0x00c5b784 0x00c5bfa0
rightPlacement = Placement 0x00c4fc88 0x00c6b784 0x00c6bfa0

main :: IO ()
main = do
  let left = fixture leftPlacement Clean
      right = fixture rightPlacement Clean
      accepted = either (const False) comparisonAccepted
      refused result = not (accepted result)
  report <- either (\problem -> putStrLn problem >> exitFailure) pure (comparePackages left right)
  check "nonuniform generated placement is accepted" (comparisonAccepted report)
  check "raw physical movement is retained" (not (null (comparisonRawDifferences report)))
  check "relocation evidence is retained" (not (null (comparisonDifferences report)))
  check "both actual capture words are reported" (length (comparisonCapture report) == 2)
  check "unchanged stock MOVW companions are explicit evidence" (length (comparisonSourceCompanions report) == 2)
  let nameMapping = object ["component" .= ("plan:0:NameRecords:names" :: String), "left" .= ("0x00c5b784" :: String), "right" .= ("0x00c6b784" :: String), "bytes" .= (924 :: Int), "left_offset" .= (0x1b784 :: Int), "right_offset" .= (0x1bafc :: Int), "address_delta" .= (0x10000 :: Int), "component_offset_delta" .= (0x378 :: Int), "mode" .= ("Data" :: String)]
  check "component map exposes extra 0x378 movement" (nameMapping `elem` comparisonMappings report)
  mapM_ (\damage -> check (show damage ++ " refuses") (refused (comparePackages left (fixture rightPlacement damage))))
    [ChangedStockTarget,ChangedRegister,CaptureBL,CaptureWrongTarget,MissingCodeMap,CodeGap,NonzeroGap,ChangedData,ManifestDrift,RuntimeCodeDrift,ChangedLiteralLoad,MissingCompanion,StalePointer,StaleGeneratedPointer,MissingRelocations,MissingPointerRecord,NameConsumerToOtherLiteral,NameConsumerNotLoad,StaleNameLiteral,ShiftedNameLiteral,MissingNameFunction,StaleD3Publication,StalePlanPublication,WrongFixedPublication,StalePublicationBranch,StalePublicationWide]
  check "address-like Classic Chrome scalars do not acquire pointer provenance" (refused (comparePackages (fixture leftPlacement AddressLikeScalar) (fixture rightPlacement AddressLikeScalar)))
  check "name literal requires exact names +0xc6 at both bases" (refused (comparePackages (fixture leftPlacement ShiftedNameLiteral) (fixture rightPlacement ShiftedNameLiteral)))
  check "matching malformed name consumers still refuse" (refused (comparePackages (fixture leftPlacement NameConsumerToOtherLiteral) (fixture rightPlacement NameConsumerToOtherLiteral)))
  check "known pointer provenance is reported" (not (null (comparisonPointerFields report)))
  check "all fixture publication pointer schemas are reported" (length (comparisonPublicationPointerFields report) == 10)
  check "matching wrong Classic Chrome publication pointers refuse" (refused (comparePackages (fixture leftPlacement WrongFixedPublication) (fixture rightPlacement WrongFixedPublication)))
  check "address-like publication scalars do not acquire pointer provenance" (refused (comparePackages (fixture leftPlacement AddressLikePublicationScalar) (fixture rightPlacement AddressLikePublicationScalar)))
  let PackageBytes elf runtime manifest plan = right
  check "truncated runtime refuses" (refused (comparePackages left (PackageBytes elf (B.init runtime) manifest plan)))
  check "truncated ELF refuses" (refused (comparePackages left (PackageBytes (B.take 20 elf) runtime manifest plan)))
  let scoped placement damage = fixtureFor L.modalScopedBankVariant placement damage
  scopedReport <- either (\problem -> putStrLn problem >> exitFailure) pure
    (comparePackages (scoped leftPlacement Clean) (scoped rightPlacement Clean))
  check "scoped relocation accepts the qualified selector and both branch targets" (comparisonAccepted scopedReport)
  check "both scoped activation words are retained" (length (comparisonModal scopedReport) == 2)
  check "the standard image reports no modal hook" (null (comparisonModal report))
  check "cross-variant comparison refuses" (refused (comparePackages left (scoped rightPlacement Clean)))
  let tagged tag (PackageBytes subjectElf subjectRuntime document subjectPlan) =
        case eitherDecodeStrict' document of
          Right (Object fields) -> PackageBytes subjectElf subjectRuntime (BL.toStrict (encode (Object (KM.insert "bank_variant" (String tag) fields)))) subjectPlan
          _ -> error "fixture manifest object required"
  check "explicit standard identity parses without changing comparison behavior"
    (accepted (comparePackages (tagged "standard" left) (tagged "standard" right)))
  check "manifest identity refuses publication count disagreement"
    (refused (comparePackages (tagged "modal75" left) (tagged "modal75" right)))
  check "unknown manifest identity refuses"
    (refused (comparePackages (tagged "unknown" left) (tagged "unknown" right)))
  mapM_ (\damage -> check (show damage ++ " refuses even when both relocated selectors match")
      (refused (comparePackages (scoped leftPlacement damage) (scoped rightPlacement damage))))
    [ModalWrongController,ModalWrongFlags,ModalStockResume,ModalOwnedTarget,ModalBL,ModalWrongPush,ModalWrongSize,MissingModalHook]
  forM_ [L.standardBankVariant, L.modalScopedBankVariant, L.lifecycleBankVariant, L.modalLifecycleBankVariant] $ \variant -> do
    valid <- either (\problem -> putStrLn problem >> exitFailure) pure
      (comparePackages (fixtureFor variant leftPlacement Clean) (fixtureFor variant rightPlacement Clean))
    check ("canonical selected group order accepts: " ++ show variant) (comparisonAccepted valid)
    check ("matching noncanonical physical order refuses: " ++ show variant)
      (refused (comparePackages (fixtureFor variant leftPlacement ReorderedPublications) (fixtureFor variant rightPlacement ReorderedPublications)))
    unless (null (L.publicationGroups variant)) $
      check ("matching wrong group destination refuses: " ++ show variant)
        (refused (comparePackages (fixtureFor variant leftPlacement WrongGroupDestination) (fixtureFor variant rightPlacement WrongGroupDestination)))
  putStrLn "runtime relocation comparison: behavioral checks passed"
 where
  check label passed = unless passed (putStrLn ("FAIL: " ++ label) >> exitFailure)

fixture :: Placement -> Damage -> PackageBytes
fixture = fixtureFor L.standardBankVariant

fixtureFor :: L.BankVariant -> Placement -> Damage -> PackageBytes
fixtureFor variant placement damage = PackageBytes elf runtime (json manifest) (json plan)
 where
  start = base placement
  name = names placement
  install = installer placement
  text = start + 0x500
  dataStart = start + if L.hasPublicationGroup L.Modal75Publication variant then 0x800 else 0x600
  publicationStart = install + 0x20
  rendezvousStart = (publicationStart + fromIntegral (publicationCount * 16) + 31) .&. 0xffffffe0
  mailboxStart = rendezvousStart + 0x80
  branch site target link = (if link then 0xeb000000 else 0xea000000) .|. (((target - site - 8) `shiftR` 2) .&. 0x00ffffff)
  wide top register value = (if top then 0xe3400000 else 0xe3000000) .|. (register `shiftL` 12) .|. ((immediate .&. 0xf000) `shiftL` 4) .|. (immediate .&. 0xfff)
   where immediate = if top then value `shiftR` 16 else value .&. 0xffff
  stock = if damage == ChangedStockTarget then 0x00710004 else 0x00710000
  nameFunction = text + 0x40
  nameConsumer = case damage of NameConsumerToOtherLiteral -> 0xe59f3084; NameConsumerNotLoad -> 0xe2833080; _ -> 0xe59f3080
  nameLiteral = case damage of StaleNameLiteral -> 0x00c5b84a; ShiftedNameLiteral -> name + 0xca; _ -> name + 0xc6
  nameBody = overlay 0x9c [(0,B.concat (replicate 37 (w32 0xe320f000))),(0x0c,w32 nameConsumer),(0x94,w32 nameLiteral)]
  scoped = L.hasPublicationGroup L.Modal75Publication variant
  lifecycle = L.hasPublicationGroup L.LifecyclePublications variant
  publicationCount = L.publicationCountFor variant
  allocationSize = fromIntegral (L.runtimeAllocationSizeFor variant)
  seam = text + 0x100
  owned = text + 0x140
  seamBody = B.concat (map w32
    [0xe52dc004,0xe10fc000,0xe52dc004
    ,if damage == ModalWrongController then 0xe350004a else 0xe350004b
    ,0x0a000004,0xe49dc004,if damage == ModalWrongFlags then 0xe1a00000 else 0xe128f00c
    ,0xe49dc004,0xe92d4ff0
    ,branch (seam+36) (if damage == ModalStockResume then 0x00bfb030 else 0x00bfb02c) False
    ,0xe49dc004,0xe128f00c,0xe49dc004
    ,branch (seam+52) (owned + if damage == ModalOwnedTarget then 4 else 0) False])
  code = overlay (if scoped then 0x144 else 0xdc)
    ([(0,B.concat (map w32 [branch text stock False,wide False (if damage == ChangedRegister then 1 else 0) name,wide True 0 name,if damage == ChangedLiteralLoad then 0xe59f0004 else 0xe59f0000,0xe12fff1e,name]))
    ,(0x40,nameBody)] ++ if scoped then [(0x100,seamBody),(0x140,w32 0xe12fff1e)] else [])
  storedPointer = if damage `elem` [StalePointer,MissingPointerRecord] then 0x00c5b784 else name
  payload = w32 (if damage == AddressLikeScalar then text else 0) <> B.replicate 96 0 <> w32 (if damage == ChangedData then 0x12345678 else storedPointer)
  nodeAddress = start + 0xa800
  node = B.concat (map w32 [1,0,0,7,0,0,1,0,if damage == StaleGeneratedPointer then 0x00c5b784 else name,0,1])
  captureTarget = text + if damage == CaptureWrongTarget then 4 else 0
  captureWord = branch 0x00e9e038 captureTarget (damage == CaptureBL)
  publication index =
    let site
          | scoped && index == publicationCount - 1 && damage /= MissingModalHook = 0x00bfb028
          | lifecycle && index == 223 = 0x00e99e8c
          | lifecycle && index == 224 = 0x00e6a754
          | lifecycle && index == 225 = 0x00e6bb90
          | otherwise = case index of 222 -> 0x00e9e038; 0 -> 0x0078eab8; 2 -> 0x01110004; 3 -> 0x007823c8; 4 -> 0x0078bd80; 5 -> 0x0078bd7c; 6 -> 0x0061c384; _ -> 0x01100000 + 4 * fromIntegral index
        old
          | scoped && index == publicationCount - 1 = if damage == ModalWrongPush then 0xe92d4000 else 0xe92d4ff0
          | otherwise = case index of 222 -> 0xe92d4870; 2 -> 0xe3403037; 6 -> 1; _ -> 0
        new = case index of
          0 -> if damage == StalePlanPublication then 0x00c5b784 else name
          1 -> branch site (if damage == StalePublicationBranch then 0x00c40500 else text) False
          2 -> wide True 3 (if damage == StalePublicationWide then 0x00c5b784 else name)
          3 -> if damage == StaleD3Publication then 0x00c5bbdc else name + 0x458
          4 -> if damage == WrongFixedPublication then 0x0078427c else 0x0061c388
          5 -> 0x0061c264
          6 -> 0
          7 -> if damage == AddressLikePublicationScalar then text else 1
          222 -> captureWord
          _ | scoped && index == publicationCount - 1 -> branch 0x00bfb028 seam (damage == ModalBL)
            | lifecycle && 223 <= index && index <= 225 -> branch site text False
            | otherwise -> 1
     in (if damage == WrongGroupDestination && index == 223 then site + 4 else site, old, new)
  logicalPublications = IM.fromDistinctAscList [(index, publication index) | index <- [0 :: Int .. publicationCount - 1]]
  canonicalOrder = [0 .. 35] ++ [55 .. 68] ++ [88] ++ [36 .. 53] ++ [97 .. 158] ++ [222] ++ [159 .. 221]
    ++ [75,76,72,73,74,69,70,71,77,78,79,80,81,82,83,89,90,91,92,93,94,95,96,84,85,86,87,54]
    ++ [223 .. publicationCount - 1]
  physicalOrder = case (damage, canonicalOrder) of
    (ReorderedPublications, first : second : rest) -> second : first : rest
    _ -> canonicalOrder
  publications =
    [(index, (site, old, new, if position < 131 then 0 else 1))
    | (position, index) <- zip [0 :: Int ..] physicalOrder
    , let (site, old, new) = logicalPublications IM.! index]
  table = B.concat [B.concat (map w32 [site,old,new,phase]) | (_, (site,old,new,phase)) <- publications]
  relocationSymbolIndex = fromIntegral (1 + length (takeWhile (\(symbolName,_,_,_,_) -> symbolName /= "__rb_name_records") symbols))
  abs32 site = w32 site <> w32 ((relocationSymbolIndex `shiftL` 8) .|. 2)
  sections = [(".text",text,6,code),(".data",dataStart,3,payload),(".ram_push.text",install,6,w32 (branch install text False) <> w32 0xe12fff1e),(".ram_push.publications",publicationStart,3,B.replicate (publicationCount * 16) 0),(".ram_push.rendezvous",rendezvousStart,3,B.replicate 128 0),(".ram_push.mailbox",mailboxStart,3,B.replicate 128 0)]
    ++ if damage == MissingRelocations then [] else [(".rel.text",0,0,abs32 (text+20)),(".rel.data",0,0,if damage == MissingPointerRecord then B.empty else abs32 (dataStart+100))]
  symbols = [("$a",text,0,0,1) | damage /= MissingCodeMap] ++ [("$d",text+10,0,0,1) | damage == CodeGap]
    ++ [("rb_name_show",nameFunction,0x9c,2,1) | damage /= MissingNameFunction]
    ++ (if scoped then [("$a",seam,0,0,1),("rb_modal_entry",seam,if damage == ModalWrongSize then 52 else 56,2,1),("rb_modal_dispatch",owned,4,2,1)] else [])
    ++ [("$a",nameFunction,0,0,1),("$d",nameFunction+0x94,0,0,1)] ++
    [("$d",text+20,0,0,1),("$a",install,0,0,3),("cc_ram_data",dataStart,0,0,2),("cc_ram_data_end",dataStart+100,0,0,2),("__rb_name_records",name,0,0,0xfff1),("RAM_PUSH_BANK_END",name+0x400,0,0,0xfff1),("__ram_push_publications_start",publicationStart,0,0,4),("__ram_push_publications_end",publicationStart+fromIntegral publicationCount*16,0,0,4),("rb_capture_develop_wait",text,4,2,1),("ram_push_install",install,4,2,3),("ram_push_rollback",install+4,4,2,3),("ram_push_status",install+4,4,2,3)]
  elf = makeElf text sections symbols
  header = B.concat (map w32 [0x0061c400,0x00282800,0x00bf1a00,0x004d0020,0x01100000,0x00080000]) <> B.replicate 104 0
    <> B.concat [w32 site <> w32 new | (site,_,new) <- take 97 (IM.elems logicalPublications)] <> B.replicate (0x400 - 97*8) 0
  chunks = [(0,header)] ++ [(fromIntegral (address-start),bytes) | (_,address,flags,bytes) <- sections, flags .&. 2 /= 0]
    ++ [(fromIntegral (publicationStart-start),table),(fromIntegral (name-start),B.replicate 924 65),(fromIntegral (nodeAddress-start),node),(fromIntegral (name+0x400+472-start),B.concat (replicate 19 (w32 0xe12fff1e)))]
    ++ [(0x4c0,B.singleton 1) | damage == NonzeroGap] ++ [(0x500,B.singleton 0) | damage == RuntimeCodeDrift]
  runtime = overlay allocationSize chunks
  manifest = object ["schema" .= (1 :: Int),"base" .= hex start,"size" .= allocationSize,"entry" .= object ["install" .= hex install,"rollback" .= hex (install+4),"status" .= hex (install+4)],"publications" .=
    [object ["index" .= index,"address" .= hex site,"old" .= hex old,"new" .= hex (if damage == ManifestDrift && index == 3 then 2 else new),"phase" .= (if phase == 0 then "setup" else "activation" :: String)] | (index,(site,old,new,phase)) <- publications]]
  plan = object ["schema" .= ("recipe-bank-plan-1" :: String),"writes" .=
    ([object ["region" .= ("name_records" :: String),"address" .= show name,"new" .= concat (replicate 924 "41"),"current" .= concat (replicate 924 "00"),"role" .= ("names" :: String)]
     ,object ["region" .= ("menu_values" :: String),"address" .= show nodeAddress,"new" .= byteHex node,"current" .= concat (replicate 44 "00"),"role" .= ("slot list node header" :: String)]
     ,object ["region" .= ("id4_code" :: String),"address" .= ("0x0078eab8" :: String),"new" .= byteHex (w32 name),"current" .= ("00000000" :: String),"role" .= ("parent node +0x20 values pointer" :: String)]]
    ++ [object ["region" .= ("plain_sites" :: String),"address" .= ("0x01110000" :: String),"new" .= ("84370be3" :: String),"current" .= ("84370be3" :: String),"role" .= ("label record base movw" :: String)] | damage /= MissingCompanion])]

json :: Value -> B.ByteString
json = BL.toStrict . encode
hex :: Word32 -> String
hex value = "0x" ++ replicate (8-length digits) '0' ++ digits where digits = showHex value ""
byteHex :: B.ByteString -> String
byteHex = concatMap (\value -> let digits = showHex value "" in replicate (2-length digits) '0' ++ digits) . B.unpack
w32 :: Word32 -> B.ByteString
w32 value = B.pack [fromIntegral (value `shiftR` (8*index)) | index <- [0 :: Int .. 3]]
w16 :: Word32 -> B.ByteString
w16 = B.take 2 . w32
overlay :: Int -> [(Int,B.ByteString)] -> B.ByteString
overlay size = foldl apply (B.replicate size 0)
 where apply bytes (offset,fresh) = B.take offset bytes <> fresh <> B.drop (offset+B.length fresh) bytes

makeElf :: Word32 -> [(String,Word32,Word32,B.ByteString)] -> [(String,Word32,Word32,Word32,Word32)] -> B.ByteString
makeElf entry sections symbols = overlay totalSize ([(0,header),(fromIntegral shoff,B.concat headers)] ++ payloads)
 where
  namesTable = B.singleton 0 <> C.pack (concatMap ((++ "\0") . first) sections ++ ".shstrtab\0.strtab\0.symtab\0")
  strings = B.singleton 0 <> C.pack (concatMap (\(name,_,_,_,_) -> name ++ "\0") symbols)
  nameOffsets = snd (mapAccumL (\offset name -> (offset + length name + 1,offset)) 1 (map first sections ++ [".shstrtab",".strtab",".symtab"]))
  symbolOffsets = snd (mapAccumL (\offset (name,_,_,_,_) -> (offset + length name + 1,offset)) 1 symbols)
  symtab = B.replicate 16 0 <> B.concat [w32 (fromIntegral offset) <> w32 address <> w32 size <> B.pack [fromIntegral kind,0] <> w16 section | (offset,(_,address,size,kind,section)) <- zip symbolOffsets symbols]
  allPayloads = map (\(_,_,_,bytes) -> bytes) sections ++ [namesTable,strings,symtab]
  (_,positions) = mapAccumL (\offset bytes -> let aligned = (offset+3) `div` 4 * 4 in (aligned+B.length bytes,(aligned,bytes))) 0x100 allPayloads
  payloads = positions
  end = maximum [offset+B.length bytes | (offset,bytes) <- positions]
  shoff = fromIntegral ((end+3) `div` 4 * 4)
  totalSize = fromIntegral shoff + 40 * (length sections + 4)
  header = B.pack [0x7f,69,76,70,1,1,1] <> B.replicate 9 0 <> w16 2 <> w16 40 <> w32 1 <> w32 entry <> w32 0 <> w32 shoff <> w32 0 <> w16 52 <> w16 32 <> w16 0 <> w16 40 <> w16 (fromIntegral (length sections+4)) <> w16 (fromIntegral (length sections+1))
  sectionHeader name kind flags address offset size link info entsize = B.concat (map w32 [fromIntegral name,kind,flags,address,fromIntegral offset,fromIntegral size,link,info,4,entsize])
  headers = B.replicate 40 0 :
    [sectionHeader name kind flags address offset (B.length bytes) link info entsize
    | (name,((sectionName,address,flags,bytes),(offset,_))) <- zip nameOffsets (zip sections positions)
    , let (kind,link,info,entsize) = case sectionName of
            ".rel.text" -> (9,fromIntegral (length sections+3),1,8)
            ".rel.data" -> (9,fromIntegral (length sections+3),2,8)
            _ -> (1,0,0,0)
    ] ++
    [sectionHeader (nameOffsets !! (length sections+index)) kind 0 0 offset (B.length bytes) link 0 entsize | (index,((offset,bytes),(kind,link,entsize))) <- zip [0 :: Int ..] (zip (drop (length sections) positions) [(3,0,0),(3,0,0),(2,fromIntegral (length sections+2),16)])]
  first (name,_,_,_) = name
