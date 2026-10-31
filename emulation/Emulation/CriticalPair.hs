{-# LANGUAGE OverloadedStrings #-}
module Emulation.CriticalPair
  ( Original, LinkedArtifact, Staging, deriveOriginal, deriveLinkedArtifact
  , stageStartup, startupDat, patchedImage4, stagingValue, verifyFinalDat
  ) where

import qualified A32 as A
import Control.Monad (forM, forM_, unless)
import Data.Aeson (Value, object, (.=))
import Data.Bits (complement, shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.Set as Set
import Data.Word (Word32)
import Emulation.FirstStageWitness (leWordAt, sha256Hex)
import qualified Emulation.InterfaceInitializer as I
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import StageBuild (StageDeclarations (..))
import qualified StartupStageContract as S

newtype Guest = Guest Word32 deriving (Eq, Ord, Show)
newtype FrameBytes = FrameBytes Int
newtype InterfaceOffset = InterfaceOffset Word32
newtype RetainedCode = RetainedCode Word32
newtype ImageWordPatch = ImageWordPatch (Guest, Word32, Word32)
data Core = Core0 | Core1

data Services = Services Core Guest F.Sha256 [(InterfaceOffset, RetainedCode)]
data Original = Original B.ByteString S.OriginalStartupDat B.ByteString [Services]
data LinkedArtifact = LinkedArtifact B.ByteString B.ByteString [F.Symbol] Guest Guest FrameBytes FrameBytes
data Staging = Staging B.ByteString B.ByteString B.ByteString [ImageWordPatch] [Services] LinkedArtifact

require :: Bool -> String -> Either String ()
require condition message = unless condition (Left message)

at :: [F.Symbol] -> String -> Either String Word32
at symbols name = F.symbolAddress <$> F.lookupSymbol symbols name

branch :: Bool -> Word32 -> Word32 -> Either String Word32
branch link site target = do
  let displacement = toInteger target - toInteger site - 8
  require (target .&. 3 == 0 && displacement >= -0x2000000 && displacement < 0x2000000)
    "critical-pair target is outside the A32 branch range"
  pure ((if link then 0xeb000000 else 0xea000000) .|. (fromInteger (displacement `div` 4) .&. 0xffffff))

wordsAt :: B.ByteString -> Int -> [Word32] -> Either String ()
wordsAt bytes offset expected = do
  observed <- mapM (leWordAt bytes . (offset +)) [0,4 .. length expected * 4 - 4]
  require (observed == expected) "critical-pair instruction contract differs"

loader :: U.Parts -> Core -> F.Sha256 -> Either String B.ByteString
loader parts core digest = do
  let (ident,base,extent) = case core of
        Core0 -> (0,0x40000,0xdae0)
        Core1 -> (1,0xd0000,0xdaa0)
  segment <- case filter ((== ident) . U.segId) (U.partSegments parts) of
    [found] -> Right found
    _ -> Left "critical-pair loader segment is not unique"
  let offset = fromIntegral (U.segFlash segment)
      count = fromIntegral (U.segSize segment)
      carrier = U.partPreTable parts
  require (U.segLoad segment == base && count == extent && offset + count <= B.length carrier)
    "critical-pair loader geometry differs"
  let bytes = B.map complement (B.take count (B.drop offset carrier))
  observed <- F.parseSha256 (sha256Hex bytes)
  require (observed == digest) "critical-pair loader identity differs"
  pure bytes

services :: Core -> Guest -> B.ByteString -> Either String Services
services core (Guest base) bytes = do
  let wide low high = do
        lo <- leWordAt bytes low
        hi <- leWordAt bytes high
        require (lo .&. 0xfff00000 == 0xe3000000 && hi .&. 0xfff00000 == 0xe3400000)
          "critical-pair publication is not MOVW/MOVT"
        let immediate word = ((word `shiftR` 4) .&. 0xf000) .|. (word .&. 0xfff)
        pure (immediate lo .|. (immediate hi `shiftL` 16))
  root <- wide 0x438 0x43c
  header <- leWordAt bytes 0x10
  require (root == header) "critical-pair loader header does not name its published interface"
  slots <- forM [(0x164,0x86c,0x870,0x874),(0x168,0x878,0x87c,0x880)] $ \(offset,low,high,store) -> do
    target <- wide low high
    wordsAt bytes store [0xe5832000 + offset]
    pure (InterfaceOffset offset, RetainedCode target)
  extensions <- forM [(0x1618,0x8a8,0x8b0,0x8b4),(0x161c,0x8b8,0x8c0,0x8c4)
                    ,(0x1620,0x8c8,0x8d0,0x8d4),(0x1624,0x8d8,0x8e0,0x8e4)] $ \(offset,low,high,store) -> do
    target <- wide low high
    wordsAt bytes store [0xe7831002]
    pure (InterfaceOffset offset, RetainedCode target)
  require ([target | (_,RetainedCode target) <- slots ++ extensions]
    == map (base +) [0x1140,0x1164,0x12bc,0x12cc,0x12e8,0x1304])
    "critical-pair installed service targets differ"
  wordsAt bytes 0x8ac [0xe3012618]
  forM_ [0x8bc,0x8cc,0x8dc] $ \offset -> wordsAt bytes offset [0xe2822004]
  wordsAt bytes 0x1140 [0xe52db004,0xe28db000,0xe10f0000,0xe3801080,0xe121f001
                     ,0xe2000080,0xe28bd000,0xe8bd0800,0xe12fff1e]
  wordsAt bytes 0x1164 [0xe52db004,0xe28db000,0xe2000080,0xe10f1000,0xe3c11080
                     ,0xe1800001,0xe121f000,0xe28bd000,0xe8bd0800,0xe12fff1e]
  wordsAt bytes 0x12bc [0xe92d4800,0xe28db004,0xeb00053b,0xe8bd8800]
  forM_ [(0x12cc,0xeb00053b),(0x12e8,0xeb00053e),(0x1304,0xeb000542)] $ \(offset,call) ->
    wordsAt bytes offset [0xe92d4800,0xe28db004,0xe3500000,0x03000804,0x0344000c,call,0xe8bd8800]
  wordsAt bytes 0x27b8 [0xee121e10,0xe3a02901,0xe2822003,0xe1510002,0x03a00000,0x13a00001,0xe12fff1e]
  wordsAt bytes 0x27d4 [0xe1902f9f,0xe3520000,0x13e00000,0x112fff1e,0x01802f91,0xe3520000
                     ,0x0e072fba,0x03a00000,0x012fff1e,0xeafffff5]
  wordsAt bytes 0x27fc [0xe3a01000,0xe1902f9f,0xe3520000,0x03e00000,0x012fff1e,0x11802f91
                     ,0xe3520000,0x0e072fba,0x03a00000,0x012fff1e,0xeafffff4,0xe5900000,0xe12fff1e]
  digest <- F.parseSha256 (sha256Hex bytes)
  pure (Services core (Guest root) digest (slots ++ extensions))

deriveOriginal :: B.ByteString -> B.ByteString -> Either String Original
deriveOriginal dat image4 = do
  original <- I.parseOriginalSource dat
  require (sha256Hex image4 == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")
    "critical-pair requires canonical decoded image4"
  let parts = U.parsedParts (S.originalStartupParsed original)
  primaryDigest <- F.parseSha256 "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"
  secondaryDigest <- F.parseSha256 "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"
  primary <- loader parts Core0 primaryDigest
  secondary <- loader parts Core1 secondaryDigest
  bindings <- sequence [services Core0 (Guest 0x40000) primary,services Core1 (Guest 0xd0000) secondary]
  wordsAt image4 (0x7235d8 - 0x61c400)
    [0xe92d4df0,0xe28db01c,0xe30b5d20,0xe3405096,0xe5952000,0xe3013618,0xe7923003
    ,0xe12fff33,0xe1a04000,0xe7953100,0xe5933164,0xe12fff33,0xe1a0a000]
  wordsAt image4 (0x725dd8 - 0x61c400) [0xe92d48f0,0xe28db014,0xe24dd020]
  wordsAt image4 (0x725dfc - 0x61c400) [0xebfff5f5,0xe1a07000]
  wordsAt image4 (0x725e80 - 0x61c400) [0xe1a00007,0xebfff609]
  wordsAt image4 (0x725fb0 - 0x61c400) [0xe1a00007,0xebfff5bd]
  wordsAt image4 (0x726060 - 0x61c400) [0xe1a00007,0xebfff591]
  wordsAt image4 (0x726090 - 0x61c400) [0xe24bd014,0xe8bd88f0]
  wordsAt image4 (0x725a0c - 0x61c400) [0xe30b0d20,0xe3400096,0xe3003000,0xe3403004,0xe5933010
    ,0xe5803000,0xe3003000,0xe340300d,0xe5933010,0xe5803004]
  pure (Original dat original image4 bindings)

adapterProof :: B.ByteString -> [F.Symbol] -> Either String ()
adapterProof image symbols = do
  origin <- at symbols "__secondary_stage_start"
  let check name expected = do
        symbol <- F.lookupSymbol symbols name
        require (F.symbolSize symbol == fromIntegral (length expected * 4)) "critical-pair adapter extent differs"
        wordsAt image (fromIntegral (F.symbolAddress symbol - origin)) expected
  enter <- at symbols "xm1_critical_enter_adapter"
  enterBody <- at symbols "xm1_critical_enter_continue"
  enterCall <- branch True (enter + 84) enterBody
  exit <- at symbols "xm1_critical_exit_adapter"
  exitBody <- at symbols "xm1_critical_exit"
  exitCall <- branch True (exit + 32) exitBody
  check "xm1_critical_enter_adapter" [0xe92d4df0,0xe28db01c,0xe30b5d20,0xe3405096,0xe5952000
    ,0xe3013618,0xe7923003,0xe12fff33,0xe1a04000,0xe7953100,0xe5933164,0xe12fff33,0xe1a0a000
    ,0xe24dd020,0xe88d100f,0xe58de014,0xe10f2000,0xe58d2018
    ,0xe1a0000d,0xe1a01004,0xe1a0200a,enterCall,0xe59d2018,0xe12cf002,0xe89d100f
    ,0xe59de014,0xe28dd020,0xe8bd8df0]
  check "xm1_critical_exit_adapter" [0xe92d4878,0xe28db014,0xe24dd020,0xe88d100f,0xe58de014
    ,0xe10f2000,0xe58d2018,0xe1a0000d,exitCall,0xe59d2018,0xe12cf002,0xe89d100f
    ,0xe59de014,0xe28dd020,0xe8bd8878]
  check "xm1_critical_retained_call" [0xe92d4070,0xe1a04000,0xe1a05001,0xe594e018,0xe12cf00e
    ,0xe894100f,0xe12fff35,0xe884100f,0xe10f0000,0xe5840018,0xe8bd8070]

-- Offsets are relative to each Rust function's incoming SP.
data Origin = Caller A.Reg | ReturnAddress deriving (Eq, Ord)
data Frame = Frame (M.Map A.Reg Int) (M.Map A.Reg Origin) (M.Map Int Origin) deriving (Eq, Ord)

frameExtent :: B.ByteString -> [F.Symbol] -> String -> Either String FrameBytes
frameExtent image symbols name = do
  origin <- at symbols "__secondary_stage_start"
  helper <- at symbols "xm1_critical_retained_call"
  symbol <- F.lookupSymbol symbols name
  let start = F.symbolAddress symbol
      end = start + F.symbolSize symbol
      preserved = map A.Reg [4..11]
      initial = Frame (M.singleton A.sp 0) (M.fromList ((A.lr,ReturnAddress) : [(r,Caller r) | r <- preserved])) M.empty
      finish (Frame aliases values _) = do
        require (M.lookup A.sp aliases == Just 0) "critical-pair Rust return changes SP"
        require (all (\r -> M.lookup r values == Just (Caller r)) preserved) "critical-pair Rust return changes a preserved register"
      walk [] _ deepest returns = do
        require (returns > (0 :: Int)) "critical-pair Rust has no admitted return"
        pure (FrameBytes deepest)
      walk ((pc,state@(Frame aliases values slots)):pending) seen deepest returns
        | Set.member (pc,state) seen = walk pending seen deepest returns
        | otherwise = do
          require (Set.size seen < 20000 && pc >= start && pc < end && pc .&. 3 == 0)
            "critical-pair Rust control flow leaves its frame proof"
          sp <- maybe (Left "critical-pair Rust SP is unknown") Right (M.lookup A.sp aliases)
          require (sp <= 0 && sp >= -0x400 && sp `mod` 4 == 0) "critical-pair Rust frame leaves the admitted scratch budget"
          word <- leWordAt image (fromIntegral (pc - origin))
          let decoded = A.decode pc word
              visited = Set.insert (pc,state) seen
              deepest' = max deepest (-sp)
              next updated = walk ((pc+4,updated):pending) visited deepest' returns
              conditional (A.Cond condition) updated =
                walk ((pc+4,updated) : [(pc+4,state) | condition /= 14] ++ pending) visited deepest' returns
              clear registers = Frame (foldr M.delete aliases registers) (foldr M.delete values registers) slots
              set destination alias value = do
                require (destination /= A.pc) "critical-pair Rust writes PC outside an admitted return"
                require (destination /= A.sp || alias /= Nothing) "critical-pair Rust loses its SP alias"
                let insertMaybe key = maybe (M.delete key) (M.insert key)
                pure (Frame (insertMaybe destination alias aliases) (insertMaybe destination value values) slots)
              address base offset indexing = case (M.lookup base aliases,offset) of
                (Just baseAlias,A.OffsetImmediate value) -> do
                  let delta = fromIntegral value
                      effective = baseAlias + (if indexing == A.PostIndexed then 0 else delta)
                      updated = if indexing == A.OffsetOnly then aliases else M.insert base (baseAlias+delta) aliases
                  pure (Just effective,updated)
                (Just _,_) -> Left "critical-pair Rust indexes a stack alias dynamically"
                (Nothing,_) -> pure (Nothing,aliases)
              call condition target tailCall = do
                require (target == helper && sp `mod` 8 == 0) "critical-pair Rust calls an unbound service or misaligns SP"
                let depth = max deepest' (24-sp)
                if tailCall then do
                  finish state
                  require (M.lookup A.lr values == Just ReturnAddress) "critical-pair Rust tail call loses its return"
                  walk pending visited depth (returns+1)
                else let Frame nextAliases nextValues nextSlots = clear [A.Reg 0,A.Reg 1,A.Reg 2,A.Reg 3,A.Reg 12,A.lr]
                      in walk ((pc+4,Frame (M.insert A.sp sp nextAliases) nextValues nextSlots)
                        : [(pc+4,state) | condition /= A.Cond 14] ++ pending) visited depth returns
          case decoded of
            A.Branch condition link target
              | link -> call condition target False
              | target < start || target >= end -> call condition target True
              | otherwise -> walk ((target,state):[(pc+4,state) | condition /= A.Cond 14] ++ pending) visited deepest' returns
            A.BranchExchange condition False register | register == A.lr -> do
              finish state
              require (M.lookup A.lr values == Just ReturnAddress) "critical-pair Rust loses LR"
              walk ([(pc+4,state) | condition /= A.Cond 14] ++ pending) visited deepest' (returns+1)
            A.StoreMultiple condition base A.DecrementBefore True registers | base == A.sp -> do
              require (condition == A.Cond 14) "critical-pair Rust conditionally changes its stack"
              let newSp = sp - 4 * length registers
                  addresses = zip registers [newSp,newSp+4..]
                  saved = foldr (\(r,offset) -> maybe (M.delete offset) (M.insert offset) (M.lookup r values)) slots addresses
              next (Frame (M.insert A.sp newSp aliases) values saved)
            A.LoadMultiple condition base A.IncrementAfter True registers | base == A.sp -> do
              let loaded = zip registers [sp,sp+4..]
                  updated = foldr (\(r,offset) -> maybe (M.delete r) (M.insert r) (M.lookup offset slots)) values loaded
                  aliases' = M.insert A.sp (sp+4*length registers) (foldr M.delete aliases registers)
                  result = Frame aliases' updated slots
              if A.pc `elem` registers then do
                finish result
                require (M.lookup A.pc updated == Just ReturnAddress) "critical-pair Rust pop loses its return"
                walk ([(pc+4,state) | condition /= A.Cond 14] ++ pending) visited deepest' (returns+1)
              else conditional condition result
            A.StoreMultiple condition base _ writeback registers | M.notMember base aliases -> do
              require (A.pc `notElem` registers && A.sp `notElem` registers) "critical-pair Rust exports a control register"
              conditional condition (if writeback then clear [base] else state)
            A.LoadMultiple condition base _ writeback registers | M.notMember base aliases -> do
              require (A.pc `notElem` registers && A.sp `notElem` registers) "critical-pair Rust loads unbound control registers"
              conditional condition (clear (registers ++ [base | writeback]))
            A.DataProcessing condition op _ destination base operand
              | op `elem` [A.CMP,A.CMN,A.TST,A.TEQ] -> next state
              | otherwise -> do
                  let (alias,value) = case (op,operand) of
                        (A.MOV,A.ShiftedImmediate source A.LSL 0) -> (M.lookup source aliases,M.lookup source values)
                        (A.ADD,A.Immediate amount) -> ((+fromIntegral amount) <$> M.lookup base aliases,Nothing)
                        (A.SUB,A.Immediate amount) -> ((subtract (fromIntegral amount)) <$> M.lookup base aliases,Nothing)
                        _ -> (Nothing,Nothing)
                  updated <- set destination alias value
                  conditional condition updated
            A.Load condition A.Word False destination base offset indexing -> do
              (effective,aliases') <- address base offset indexing
              let value = effective >>= (`M.lookup` slots)
                  Frame _ values' slots' = clear [destination]
                  result = Frame (M.delete destination aliases') (maybe values' (\v -> M.insert destination v values') value) slots'
              require (destination /= A.pc && destination /= A.sp) "critical-pair Rust loads an unbound control register"
              conditional condition result
            A.Store condition A.Word source base offset indexing -> do
              (effective,aliases') <- address base offset indexing
              let allocated = M.findWithDefault sp A.sp aliases'
              forM_ effective $ \offset' -> require (offset' >= allocated && offset'+4 <= 0) "critical-pair Rust store leaves its allocated frame"
              let saved = maybe slots (\offset' -> maybe (M.delete offset' slots) (\v -> M.insert offset' v slots) (M.lookup source values)) effective
              conditional condition (Frame aliases' values saved)
            A.MoveWide condition destination _ -> set destination Nothing Nothing >>= conditional condition
            A.MoveTop condition destination _ -> set destination Nothing Nothing >>= conditional condition
            A.ZeroExtend condition destination _ _ _ -> set destination Nothing Nothing >>= conditional condition
            A.Compute condition destinations -> do
              require (A.sp `notElem` destinations && A.pc `notElem` destinations) "critical-pair Rust computes a control register"
              conditional condition (clear destinations)
            A.Hint _ -> next state
            _ -> Left ("critical-pair Rust instruction is outside its frame proof: " ++ show decoded)
  walk [(start,initial)] Set.empty 0 0

deriveLinkedArtifact :: B.ByteString -> B.ByteString -> Either String LinkedArtifact
deriveLinkedArtifact image symbolBytes = do
  symbols <- F.parseSymbols symbolBytes
  start <- at symbols "__secondary_stage_start"
  end <- at symbols "__secondary_stage_end"
  require (start == 0x10400 && end <= 0x11800 && fromIntegral end - fromIntegral start == B.length image)
    "critical-pair linked image leaves established record-2 padding"
  forM_ ["xm1_critical_enter_adapter","xm1_critical_enter_continue","xm1_critical_exit_adapter"
        ,"xm1_critical_exit","xm1_critical_retained_call"] $ \name -> do
    symbol <- F.lookupSymbol symbols name
    require (F.symbolAddress symbol >= start && F.symbolSize symbol > 0
      && toInteger (F.symbolAddress symbol) + toInteger (F.symbolSize symbol) <= toInteger end)
      "critical-pair linked function leaves its artifact"
  adapterProof image symbols
  enter <- Guest <$> at symbols "xm1_critical_enter_adapter"
  exit <- Guest <$> at symbols "xm1_critical_exit_adapter"
  enterFrame <- frameExtent image symbols "xm1_critical_enter_continue"
  exitFrame <- frameExtent image symbols "xm1_critical_exit"
  pure (LinkedArtifact image symbolBytes symbols enter exit enterFrame exitFrame)

stageStartup :: Original -> LinkedArtifact -> Either String Staging
stageStartup (Original reference original image4 bindings) artifact@(LinkedArtifact image symbolBytes symbols (Guest enter) (Guest exit) _ _) = do
  declared <- I.declarations symbols
  let parsed = S.originalStartupParsed original
  start <- at symbols "__secondary_stage_start"
  entry <- at symbols "xm1_secondary_entry" >>= F.armEntry
  offset <- F.slotOffset (fromIntegral start - 0x10000)
  plan <- F.stagePlan F.SecondRecord entry offset (U.MediaImage image) (declaredRetained declared)
  parts <- F.replaceRecordSlot plan (U.parsedParts parsed) >>= F.applyLoaderWordPatches (declaredLoaderPatches declared)
  staged <- U.rebuildContainer parsed parts
  reparsed <- U.parseContainer staged
  F.verifyStagedRecord plan (declaredLoaderPatches declared) parsed reparsed
  _ <- I.deriveContract I.Owned reference staged image symbolBytes
  enterBranch <- branch False 0x7235d8 enter
  exitBranch <- branch False 0x7236b0 exit
  let patches = [ImageWordPatch (Guest 0x7235d8,0xe92d4df0,enterBranch),ImageWordPatch (Guest 0x7236b0,0xe92d4878,exitBranch)]
      apply bytes (ImageWordPatch (Guest address,before,after)) = do
        let index = fromIntegral (address - 0x61c400)
        observed <- leWordAt bytes index
        require (observed == before) "critical-pair image4 patch preimage differs"
        pure (B.take index bytes <> U.putWord32 after <> B.drop (index+4) bytes)
  patched <- foldl (\previous patch -> previous >>= (`apply` patch)) (Right image4) patches
  pure (Staging reference staged patched patches bindings artifact)

startupDat :: Staging -> B.ByteString
startupDat (Staging _ bytes _ _ _ _) = bytes

patchedImage4 :: Staging -> B.ByteString
patchedImage4 (Staging _ _ bytes _ _ _) = bytes

verifyFinalDat :: Staging -> B.ByteString -> Either String ()
verifyFinalDat staging final = do
  before <- U.parseContainer (startupDat staging)
  after <- U.parseContainer final
  let originalParts = U.parsedParts before
      finalParts = U.parsedParts after
      restored = finalParts {U.partRecord4 = U.partRecord4 originalParts,U.partGap45 = U.partGap45 originalParts}
  reconstructed <- U.rebuildContainer before restored
  require (reconstructed == startupDat staging) "critical-pair final DAT changes bytes outside its image4 record window"
  require (U.recDecodedSize (U.recHeader (U.partRecord4 finalParts)) == fromIntegral (B.length (patchedImage4 staging)))
    "critical-pair final DAT changes decoded image4 extent"

stagingValue :: Staging -> B.ByteString -> Value
stagingValue (Staging reference staged patched patches bindings (LinkedArtifact image symbolBytes _ (Guest enter) (Guest exit) (FrameBytes enterFrame) (FrameBytes exitFrame))) final = object
  [ "schema" .= (1 :: Int),"kind" .= ("critical-pair-source-staging" :: String)
  , "source_sha256" .= sha256Hex reference,"startup_dat_sha256" .= sha256Hex staged
  , "final_dat_sha256" .= sha256Hex final,"decoded_image4_sha256" .= sha256Hex patched
  , "linked_image_sha256" .= sha256Hex image,"symbols_sha256" .= sha256Hex symbolBytes
  , "entry_adapter" .= enter,"exit_adapter" .= exit
  , "entry_frame_bytes" .= (64+enterFrame),"exit_frame_bytes" .= (56+exitFrame)
  , "owned_spans" .= ([[0x7235d8,0x7236b0],[0x7236b0,0x723790]] :: [[Word32]])
  , "patches" .= [object ["site" .= site,"before" .= before,"after" .= after] | ImageWordPatch (Guest site,before,after) <- patches]
  , "retained_services" .= [object ["core" .= (case core of Core0 -> 0 :: Int; Core1 -> 1)
      ,"table" .= table,"loader_sha256" .= F.sha256Hex digest
      ,"slots" .= [object ["offset" .= offset,"target" .= target] | (InterfaceOffset offset,RetainedCode target) <- slots]]
      | Services core (Guest table) digest slots <- bindings]
  , "qualification" .= ("source-only; no natural acceptance, retirement or SDK protocol claim" :: String)
  , "premises" .= (["record-2 padding remains executable, coherent and unmodified through the natural task103 return"
      ,"task103 supplies an eight-byte-aligned writable stack, disjoint from roots, tables, counter and retained lock"
      ,"normalized CP14 identity and the installed A/B service publications remain stable across the paired boundary"
      ,"retained IRQ, exclusive-monitor and MCR p15,c7,c10,5 ordering semantics remain dependencies"
      ,"no scheduler ownership, acquisition inference, finite contention bound or general caller/alias closure is established"] :: [String])
  ]
