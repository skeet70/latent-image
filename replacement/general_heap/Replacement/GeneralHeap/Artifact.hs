{-# LANGUAGE OverloadedStrings #-}
module Replacement.GeneralHeap.Artifact
  ( CanonicalSource, LinkedArtifact, StagedImages, StagedCandidate, ArtifactError (..)
  , canonicalSource, deriveArtifact, stageImages, image4Bytes, image5Bytes, stageCandidate
  , candidateBytes, contractValue
  , Record5Window (..), validateRecord5Confinement
  ) where

import qualified A32 as A
import Control.Monad (forM_, unless)
import Data.Aeson (Value, object, (.=))
import Data.Bits (complement, (.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import qualified Emulation.Image4Staging as Image4
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Replacement.GeneralHeap.Admission
import qualified StockInventory as Stock
import System.Directory (canonicalizePath)
import System.Environment (getEnvironment)
import System.Exit (ExitCode (..))
import System.FilePath ((</>))
import System.Process (CreateProcess (..), proc)

data ArtifactError = SourceMismatch | StockDerivationRejected String | ExtentRejected | MetadataRejected AdmissionError | SymbolsRejected deriving (Eq, Show)
data CanonicalSource = CanonicalSource B.ByteString B.ByteString B.ByteString [Stock.FunctionDeclaration]
data LinkedArtifact = LinkedArtifact B.ByteString B.ByteString [F.Symbol] [FrameProof] BackendGeometry ArenaLayout
data StagedImages = StagedImages CanonicalSource LinkedArtifact B.ByteString B.ByteString
newtype StagedCandidate = StagedCandidate B.ByteString
data Record5Window = Record5Window Int Int deriving (Eq, Show)

validateRecord5Confinement :: Record5Window -> B.ByteString -> B.ByteString -> Either ArtifactError ()
validateRecord5Confinement (Record5Window low high) original candidate = do
  let checksumEnd = U.mainChecksumOffset + 4
      slice offset count = B.take count . B.drop offset
  require (B.length original == B.length candidate && low >= checksumEnd && low < high && high <= B.length original) ExtentRejected
  require (B.take U.mainChecksumOffset original == B.take U.mainChecksumOffset candidate
    && slice checksumEnd (low-checksumEnd) original == slice checksumEnd (low-checksumEnd) candidate
    && B.drop high original == B.drop high candidate) ExtentRejected

hash :: B.ByteString -> String
hash = SHA.sha256Hex . SHA.sha256

require :: Bool -> ArtifactError -> Either ArtifactError ()
require condition failure = unless condition (Left failure)

canonicalSource :: B.ByteString -> B.ByteString -> B.ByteString -> Either ArtifactError CanonicalSource
canonicalSource dat four five = do
  require (map hash [dat,four,five] == canonicalHashes) SourceMismatch
  parsed <- either (const (Left SourceMismatch)) Right (U.parseContainer dat)
  let parts = U.parsedParts parsed
      segment4 = filter ((== 4) . U.segId) (U.partSegments parts)
  require (case segment4 of
    [segment] -> U.segLoad segment == 0x61c400 && U.segBss segment == 0x89ec00 && U.segBssLen segment == 0x352e00
    _ -> False) SourceMismatch
  proofs <- mapM (\(image,low,high,tails) -> either (Left . StockDerivationRejected) Right
    (Stock.deriveExtent (Stock.GuestAddress (if image == Image4 then 0x61c400 else 0xbf1a00))
      (if image == Image4 then four else five) (Stock.FunctionWindow (Stock.GuestAddress low) (Stock.GuestAddress high)) (map Stock.GuestAddress tails))) stockWindows
  pure (CanonicalSource dat four five proofs)

canonicalHashes :: [String]
canonicalHashes = ["43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490","f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16","3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"]

stockWindows :: [(Image,Word32,Word32,[Word32])]
stockWindows = [(Image4,0x89cda4,0x89cdc4,[]),(Image4,0x89cdc4,0x89cde4,[]),(Image4,0x89cde4,0x89d4f8,[0x89e560]),(Image4,0x89da3c,0x89da64,[]),(Image4,0x89da64,0x89dfc0,[0x89cde4,0x89e560]),(Image4,0x89e464,0x89e560,[]),(Image4,0x89e560,0x89e7c4,[]),(Image5,0xbf1c1c,0xbf1c44,[]),(Image5,0xbf1c44,0xbf1cd4,[])]

functionNames :: [F.Symbol] -> B.ByteString -> [String]
functionNames symbols bytes = [F.symbolName symbol | symbol <- symbols, F.symbolSize symbol > 0, F.symbolName symbol `elem` names]
  where names = [C.unpack name | line <- C.lines bytes, [_,_,kind,name] <- [C.words line], kind `elem` ["T","t"]]

deriveArtifact :: B.ByteString -> B.ByteString -> Either ArtifactError LinkedArtifact
deriveArtifact bytes symbolBytes = do
  symbols <- either (const (Left SymbolsRejected)) Right (F.parseSymbols symbolBytes)
  let at name = either (const (Left SymbolsRejected)) (Right . F.symbolAddress) (F.lookupSymbol symbols name)
      inside4 address = any (\(Carrier image low high) -> image == Image4 && low <= address && address < high && address /= 0x89d4f8) carriers
      textNames = functionNames symbols symbolBytes
  forM_ entrySymbols $ \(name,address) -> do
    actual <- at name
    require (actual == address) SymbolsRejected
  forM_ ["malloc","free","realloc","calloc","trim"] $ \operation -> do
    target <- at ("xm1_heap_" ++ operation ++ "_r")
    require (inside4 target && target .&. 3 == 0) SymbolsRejected
  sentinel <- at "__heap_sentinel"
  end <- at "__heap_link_end"
  require (sentinel == 0x89d4f8 && end == 0xbf1c48 && B.length bytes == fromIntegral (end-0x89cda4)
    && U.word32At bytes (0x89d4f8-0x89cda4) == 0) ExtentRejected
  let emittedWord address = U.word32At bytes (fromIntegral (address-0x89cda4))
      tailTo address target = require (A.decode address (emittedWord address) == A.Branch (A.Cond 14) False target) SymbolsRejected
  forM_ [(0x89cda4,0x89cde4,False),(0x89cdc4,0x89e560,False),(0x89da3c,0x89da64,True),(0xbf1c1c,0xbf1c44,True)] $ \(address,core,twoArguments) -> do
    let prefix = [0xe1a02001 | twoArguments] ++ [0xe1a01000,0xe30c0970,0xe3400089,0xe5900000]
    require ([emittedWord (address+fromIntegral offset) | offset <- [0,4..4*length prefix-4]] == prefix) SymbolsRejected
    tailTo (address+fromIntegral (4*length prefix)) core
  forM_ [("malloc",0x89cde4),("free",0x89e560),("realloc",0x89da64),("calloc",0xbf1c44),("trim",0x89e464)] $ \(operation,address) -> at ("xm1_heap_" ++ operation ++ "_r") >>= tailTo address
  metadata <- at "XM1_HEAP_METADATA"
  require (inside4 metadata && inside4 (metadata+55) && metadata .&. 3 == 0) ExtentRejected
  (geometry,arena) <- either (Left . MetadataRejected) Right (decodeGeometry [U.word32At bytes (fromIntegral (metadata-0x89cda4)+offset) | offset <- [0,4..52]])
  entries <- mapM at textNames
  require (not (null entries)) SymbolsRejected
  frames <- either (Left . MetadataRejected) Right (deriveFrames bytes entries)
  let code = S.fromList (instructionPCs frames)
      literals = S.fromList (map fst (literalWords frames))
  require (S.null (S.intersection code literals) && not (S.member sentinel code)
    && all (`S.notMember` code) [metadata,metadata+4..metadata+52]) (MetadataRejected CarrierRejected)
  pure (LinkedArtifact bytes symbolBytes symbols frames geometry arena)

stageImages :: CanonicalSource -> LinkedArtifact -> Either ArtifactError StagedImages
stageImages source@(CanonicalSource _ four five _) artifact@(LinkedArtifact linked _ _ _ _ _) = do
  let patch image base original = foldl (\current (Carrier kind low high) -> if kind /= image then current else
        let offset = fromIntegral (low-base); count = fromIntegral (high-low)
            owned = B.take count (B.drop (fromIntegral (low-0x89cda4)) linked)
            padded = owned <> B.replicate (count-B.length owned) 0
        in B.take offset current <> padded <> B.drop (offset+count) current) original carriers
      patched4 = patch Image4 0x61c400 four
      sentinelOffset = 0x89d4f8-0x61c400
      staged4 = B.take sentinelOffset patched4 <> U.putWord32 0 <> B.drop (sentinelOffset+4) patched4
      staged5 = patch Image5 0xbf1a00 five
  require (confined Image4 0x61c400 four staged4 && confined Image5 0xbf1a00 five staged5) ExtentRejected
  pure (StagedImages source artifact staged4 staged5)

confined :: Image -> Word32 -> B.ByteString -> B.ByteString -> Bool
confined image base original staged = B.length original == B.length staged && restore staged == original
  where
    restore = foldl (.) id [\bytes -> let offset = fromIntegral (low-base); count = fromIntegral (high-low)
      in B.take offset bytes <> B.take count (B.drop offset original) <> B.drop (offset+count) bytes | Carrier kind low high <- Carrier Image4 0x89d4f8 0x89d4fc : carriers, kind == image]

image4Bytes :: StagedImages -> B.ByteString
image4Bytes (StagedImages _ _ bytes _) = bytes
image5Bytes :: StagedImages -> B.ByteString
image5Bytes (StagedImages _ _ _ bytes) = bytes
candidateBytes :: StagedCandidate -> B.ByteString
candidateBytes (StagedCandidate bytes) = bytes

runTool :: FilePath -> FilePath -> [String] -> IO ()
runTool root executable arguments = do
  absolute <- canonicalizePath executable
  inherited <- getEnvironment
  let process = (proc absolute arguments) {cwd=Just root,env=Just (("TMPDIR",root):filter ((/= "TMPDIR") . fst) inherited)}
  (stopped,status,_,errors) <- H.runBoundedProcess (H.ProcessTimeoutMicros 600000000) (H.OutputCapBytes (1024*1024)) process B.empty
  H.ensure (not stopped && status == ExitSuccess) ("heap staging tool failed: " ++ C.unpack errors)

stageCandidate :: Image4.Tools -> FilePath -> StagedImages -> IO StagedCandidate
stageCandidate tools configured (StagedImages (CanonicalSource dat _ original5 _) _ four five) = do
  scratch <- Image4.validateScratchDirectory configured
  first <- Image4.runImage4Staging tools scratch dat dat four
  rootPath <- canonicalizePath configured
  H.withTemporaryAt rootPath "heap-image5-" $ \root -> do
    before <- either H.failH pure (U.parseContainer (Image4.candidateDat first))
    let beforeParts = U.parsedParts before
        record = U.partRecord5 beforeParts
        sourceSize = U.recDecodedSize (U.recHeader record)
        basePath = root </> "image4.DAT"
        imagePath = root </> "image5.bin"
        recordPath = root </> "record5.bin"
        outputPath = root </> "candidate.DAT"
        readbackPath = root </> "carried5.bin"
        sourceImage = root </> "source-image5.bin"
        readbackImage = root </> "carried-image5.bin"
    B.writeFile readbackPath (B.map complement (U.serializeRecord record))
    runTool root (Image4.decoderPath tools) [readbackPath,"0",sourceImage,show sourceSize]
    readbackOriginal <- H.readRegular (16*1024*1024) sourceImage
    H.ensure (readbackOriginal == original5) "heap image5 canonical record differs from admitted source"
    B.writeFile basePath (Image4.candidateDat first)
    B.writeFile imagePath five
    runTool root (Image4.encoderPath tools) ["0x2e0000",imagePath,recordPath]
    decoder <- canonicalizePath (Image4.decoderPath tools)
    runTool root (Image4.rebuildPath tools) ["replace-record5","--decoder",decoder,basePath,recordPath,imagePath,outputPath]
    result <- H.readRegular (64*1024*1024) outputPath
    final <- either H.failH pure (U.parseContainer result)
    let finalParts = U.parsedParts final
        (mediaLow,mediaHigh) = U.record5Window beforeParts
        mainStart = U.stagingLength + fromIntegral (U.hMainOffset (U.parsedHeader before))
    either (H.failH . show) pure (validateRecord5Confinement (Record5Window (mainStart+mediaLow) (mainStart+mediaHigh)) (Image4.candidateDat first) result)
    H.ensure (U.hMainChecksum (U.parsedHeader final) == U.hMainChecksum (U.derivedHeader finalParts)) "heap staging carries an invalid main checksum"
    B.writeFile readbackPath (B.map complement (U.serializeRecord (U.partRecord5 finalParts)))
    runTool root decoder [readbackPath,"0",readbackImage,show sourceSize]
    carried <- H.readRegular (16*1024*1024) readbackImage
    H.ensure (carried == five) "heap carried image5 differs from confined staged image"
    pure (StagedCandidate result)

contractValue :: FilePath -> FilePath -> StagedImages -> StagedCandidate -> Value
contractValue path4 path5 (StagedImages (CanonicalSource dat source4 source5 stockProofs) (LinkedArtifact linked symbolBytes symbols frames geometry arena) four five) (StagedCandidate candidate) = object
  ["schema" .= (1::Int),"qualification" .= ("conditional-source-only"::String)
  ,"canonical_source" .= object ["dat_sha256" .= hash dat,"image4_sha256" .= hash source4,"image5_sha256" .= hash source5]
  ,"staged_image4" .= object ["path" .= path4,"sha256" .= hash four],"staged_image5" .= object ["path" .= path5,"sha256" .= hash five]
  ,"candidate_dat_sha256" .= hash candidate,"linked_sha256" .= hash linked,"symbols_sha256" .= hash symbolBytes
  ,"carrier_ranges" .= [range image low high | Carrier image low high <- carriers]
  ,"immutable_intervals" .= [range image low high | Carrier image low high <- carriers]
  ,"sentinel" .= object ["address" .= (0x89d4f8::Word32),"initial" .= (0::Word32)]
  ,"symbols" .= M.fromList [(F.symbolName symbol,F.symbolAddress symbol) | symbol <- symbols]
  ,"function_symbols" .= M.fromList [(F.symbolName symbol,F.symbolAddress symbol) | symbol <- symbols, F.symbolName symbol `elem` textNames]
  ,"native_allocator_api" .= object ["image_shims" .= False,"decision" .= ("maintainer-approved"::String)
    ,"front_end" .= ("Rust GlobalAlloc and Allocator ship in the rlib over the fixed arena-prefix state, sentinel and retained lock"::String)
    ,"reason" .= ("resident GlobalAlloc shims exceed the retired carrier union"::String)]
  ,"owned_instruction_pcs" .= instructionPCs frames,"literal_words" .= [object ["address" .= address,"value" .= value] | (address,value) <- literalWords frames]
  ,"frame_metadata" .= map frameValue frames
  ,"backend" .= ("rlsf-0.2.3/u32/u16/17/16"::String),"stock_header" .= ("boundary-tag-prev-inuse"::String)
  ,"backend_geometry" .= geometryValue geometry,"arena" .= arenaValue arena
  ,"stock_derivations" .= [object ["entry" .= address,"code_extent_bytes" .= bytes] | Stock.FunctionDeclaration (Stock.GuestAddress address) (Stock.FunctionSize bytes) <- stockProofs]
  ,"stock_windows" .= [object ["image" .= (if image == Image4 then 4 else 5::Int),"low" .= low,"high" .= high,"external_tails" .= tails] | (image,low,high,tails) <- stockWindows]
  ,"retained_dependencies" .= ([0x72b110,0x72b164,0x72b1cc,0x89d908,0x890ef8,0x890ee8,0x890ed8,0x6dbf38,0x72a7ac,0x72a7fc]::[Word32])
  ,"graphics_callback_table" .= [object ["address" .= address,"target" .= target] | (address,target) <- ([(0x890f08,0x890ed8),(0x890f0c,0x890ef8),(0x890f10,0x890ecc),(0x890f14,0x890ee8)]::[(Word32,Word32)])]
  ,"conditional_premises" .= premises
  ,"redesign_items" .= map redesignValue [minBound..maxBound]
  ]
  where
    textNames = functionNames symbols symbolBytes
    range image low high = object ["low" .= low,"high" .= high,"image" .= (if image == Image4 then 4 else 5::Int)]
    premises :: [String]
    premises = ["Unknown interior entries into retired content are excluded; continuous runtime fetch guards check this premise through aliases and later installation."
      ,"No unknown metadata reader consumes beyond payload-4; stock boundary tag decodes to an extent within the allocation."
      ,"No retained reader or writer consumes retired bin state repurposed as owned code after the sentinel word."
      ,"The image4 producer initializes the sentinel before first allocation and never resets it with live allocations."
      ,"The charged arena state prefix and pool have no external writer or competing backing consumer; retained backing is not called by owned core."
      ,"Retained recursive lock/unlock serialize every mutation and initialization across all admitted task/core contexts."
      ,"Compiler and retained callees preserve AAPCS callee-saved registers and declared stack boundaries; emitted metadata is not a full compiler ABI proof."
      ,"Stack backing, aliases and retained/interrupt writes exclude live saved words and executing owned carriers."
      ,"Image5 stubs are published by record5 installation; no earlier image4 owned path depends on image5 code."
      ,"Unchanged graphics/adapters and callback table words forward to the patched stock entries."
      ,"Cold boot only; no warm-checkpoint allocator ownership migration is admitted."
      ,"Finite host checks do not qualify silicon contention/deadlines or capture, development and storage workloads."]
