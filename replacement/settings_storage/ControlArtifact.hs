{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module ControlArtifact (ControlArtifact, deriveCloseAppendControl, deriveCloseAppendArtifact, controlImage4, controlMetadata, controlEvidence) where

import qualified A32 as A
import qualified ArmElf as E
import Control.Monad (unless)
import Data.Aeson (Value, object, (.=))
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word32)
import Emulation.OwnedCase (readWordAt, word32le)
import Emulation.Sha256 (Sha256, sha256, sha256Hex)
import qualified FujiAF.FirstStage as F
import qualified Native as N
import Replacement.PropertyService.Admission (FunctionExtent(..))
import qualified Replacement.PropertyService.Artifact as P
import qualified Replacement.PropertyService.Placement as L

newtype ByteOffset = ByteOffset Int
newtype InstructionWord = InstructionWord Word32

data ControlEdge = ControlEdge L.GuestAddress InstructionWord InstructionWord

data ControlArtifact = ControlArtifact
  { normalizedArtifact :: P.LinkedArtifact
  , controlEdge :: ControlEdge
  , controlLinkedDigest :: Sha256
  , controlSymbolsDigest :: Sha256
  , controlElfDigest :: Sha256
  }

deriveCloseAppendControl :: P.CanonicalSource -> P.ArtifactInput -> Either String ControlArtifact
deriveCloseAppendControl = deriveCloseAppendArtifact P.CloseAppendArea3Control

deriveCloseAppendArtifact :: P.ArtifactPurpose -> P.CanonicalSource -> P.ArtifactInput -> Either String ControlArtifact
deriveCloseAppendArtifact purpose source input = do
  carrier@(L.Carrier (L.GuestAddress start) _) <- maybe (Left "control source hull has no carrier") Right (L.hullCarrier (P.sourceHull source))
  elf <- E.parseElf (P.inputElf input)
  symbols <- F.parseSymbols (P.inputSymbols input)
  alias <- F.lookupSymbol symbols "xm1_bank_record_append"
  unless (F.symbolAddress alias == 0x6ac7d4 && F.symbolSize alias == 0) (Left "close-append control record entry differs")
  let rows = C.lines (P.inputSymbols input)
      isAlias row = case C.words row of
        [_,_,kind,name] -> kind == "A" && name == "xm1_bank_record_append"
        _ -> False
  unless (length (filter isAlias rows) == 1) (Left "close-append control needs one absolute record entry")
  elfAlias <- one "close-append ELF record entry" [symbol | symbol <- E.elfSymbols elf,E.symbolName symbol == "xm1_bank_record_append"]
  unless (E.symbolValue elfAlias == 0x6ac7d4 && E.symbolSize elfAlias == 0 && E.symbolKind elfAlias == E.OtherSymbol && E.symbolSection elfAlias == E.Absolute) (Left "close-append ELF record entry differs")
  let binary = P.inputBytes input
      executable site = any (\section -> E.sectionAllocated section && E.sectionExecutable section && E.sectionType section == 1 && toInteger (E.sectionAddress section) <= toInteger site && toInteger site + 4 <= toInteger (E.sectionAddress section) + toInteger (E.sectionSize section)) (E.elfSections elf)
      edges = [(site,word,link) | offset <- [0,4..B.length binary-4],let site = start + fromIntegral offset,executable site,let word = readWordAt binary offset,A.Branch (A.Cond 14) link 0x6ac7d4 <- [A.decode site word]]
  (site,original,link) <- one "close-append executable edge" edges
  section <- one "close-append executable section" [candidate | candidate <- E.elfSections elf,E.sectionAllocated candidate,E.sectionExecutable candidate,E.sectionType candidate == 1,toInteger (E.sectionAddress candidate) <= toInteger site,toInteger site + 4 <= toInteger (E.sectionAddress candidate) + toInteger (E.sectionSize candidate)]
  unless (E.wordAt elf site == Just original) (Left "close-append binary and ELF edge differ")
  owner <- one "close-append ELF function owner" [symbol | symbol <- E.elfSymbols elf,E.symbolKind symbol == E.FunctionSymbol,E.InSection _ <- [E.symbolSection symbol],toInteger (E.symbolValue symbol) <= toInteger site,toInteger site + 4 <= toInteger (E.symbolValue symbol) + toInteger (E.symbolSize symbol)]
  let replacement = InstructionWord (if link then 0xe320f000 else 0xe12fff1e)
  normalizedBytes <- replaceWord binary (ByteOffset (fromIntegral (site-start))) replacement
  rewrittenElf <- replaceWord (P.inputElf input) (ByteOffset (fromIntegral (E.sectionOffset section + site - E.sectionAddress section))) replacement
  relocation <- one "close-append edge relocation"
    [ offset + 4
    | table <- E.elfSections elf, E.sectionType table == 9
    , offset <- [fromIntegral (E.sectionOffset table), fromIntegral (E.sectionOffset table) + 8 .. fromIntegral (E.sectionOffset table + E.sectionSize table) - 8]
    , readWordAt rewrittenElf offset == site
    , readWordAt rewrittenElf (offset + 4) .&. 0xff `elem` [28, 29]
    ]
  normalizedElf <- replaceWord rewrittenElf (ByteOffset relocation)
    (InstructionWord (readWordAt rewrittenElf relocation .&. 0xffffff00))
  artifact <- either (Left . show) Right (P.deriveArtifact purpose (P.sourceProperties source) carrier
    (P.ArtifactInput normalizedBytes (C.unlines (filter (not . isAlias) rows)) normalizedElf))
  admittedOwner <- one "close-append admitted function owner" [extent | extent@(FunctionExtent _ low high) <- P.artifactFunctions artifact,low <= L.GuestAddress site,L.GuestAddress site < high]
  let FunctionExtent _ (L.GuestAddress ownerStart) (L.GuestAddress ownerEnd) = admittedOwner
  unless (ownerStart == E.symbolValue owner && toInteger ownerEnd == toInteger ownerStart + toInteger (E.symbolSize owner)) (Left "close-append ELF and admitted function owner differ")
  pure (ControlArtifact artifact (ControlEdge (L.GuestAddress site) (InstructionWord original) replacement) (sha256 binary) (sha256 (P.inputSymbols input)) (sha256 (P.inputElf input)))

controlImage4 :: P.CanonicalSource -> ControlArtifact -> Either String B.ByteString
controlImage4 source control = do
  staged <- either (Left . show) Right (P.stagedImage4Bytes source (normalizedArtifact control))
  let ControlEdge (L.GuestAddress site) original (InstructionWord normalized) = controlEdge control
      offset = fromIntegral (site-0x61c400)
  unless (readWordAt staged offset == normalized) (Left "close-append normalized staging edge differs")
  replaceWord staged (ByteOffset offset) original

controlMetadata :: ControlArtifact -> N.PropertyMetadata
controlMetadata = N.propertyMetadata . normalizedArtifact

controlEvidence :: ControlArtifact -> Value
controlEvidence control = object
  [ "kind" .= ("close-appends-area3"::String)
  , "classification" .= ("behavioral-native-control-not-production-admission"::String)
  , "linked_sha256" .= sha256Hex (controlLinkedDigest control)
  , "symbols_sha256" .= sha256Hex (controlSymbolsDigest control)
  , "elf_sha256" .= sha256Hex (controlElfDigest control)
  , "extra_edge" .= object ["site" .= site,"target" .= (0x6ac7d4::Word32)]
  , "production_admission_or_stack_credit" .= False
  ]
  where ControlEdge (L.GuestAddress site) _ _ = controlEdge control

replaceWord :: B.ByteString -> ByteOffset -> InstructionWord -> Either String B.ByteString
replaceWord bytes (ByteOffset offset) (InstructionWord word) = do
  unless (offset >= 0 && offset + 4 <= B.length bytes) (Left "control word lies outside its artifact")
  pure (B.take offset bytes <> word32le word <> B.drop (offset+4) bytes)

one :: String -> [a] -> Either String a
one _ [value] = Right value
one label _ = Left (label ++ " is missing or ambiguous")
