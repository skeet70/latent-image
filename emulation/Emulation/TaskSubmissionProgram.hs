{-# LANGUAGE OverloadedStrings #-}
module Emulation.TaskSubmissionProgram
  ( CanonicalImages(..), NativeInputs(..), Program, ProgramError(..)
  , stockProgram, ownedProgram, programImage4, programUpdateDigest, programEvidence
  , programSites, programFrameProof, programAllowsPc, programHasStore, GuestStore(..), programGuestStore, programClasses, checkProgramHeader, checkProgramSample
  ) where

import qualified A32 as A
import Control.Monad (foldM, unless)
import qualified Data.Aeson as J
import Data.Bits ((.&.), (.|.), rotateR, shiftL, shiftR, testBit)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.Int (Int32)
import Data.List (find, nub)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import qualified Emulation.Sha256 as SHA
import Emulation.StartupTaskWire
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.KernelInstall.Wire as W
import qualified Replacement.TaskSubmission.Admission as N
import qualified Replacement.TaskSubmission.Artifact as Artifact
import Data.Word (Word32)

data CanonicalImages = CanonicalImages
  { canonicalDat :: B.ByteString, canonicalA :: B.ByteString, canonicalB :: B.ByteString, canonicalImage4 :: B.ByteString }
data NativeInputs = NativeInputs
  { nativeFlat :: B.ByteString, nativeSymbols :: B.ByteString, nativeCandidate :: B.ByteString, nativeContract :: B.ByteString }
data Program = Stock | Owned N.FrameProof B.ByteString W.Digest ProgramEvidence (M.Map W.Address Boundary) (S.Set N.CodeAddress) (S.Set N.CodeAddress)
data GuestStore = GuestStore {guestStoreAddress :: W.Address, guestStoreValue :: W.WordValue} deriving (Eq, Show)
data ProgramError = ArtifactRejected Artifact.ArtifactError | CandidateAssociationRejected | SiteCollision W.Address
  | HeaderRejected | CodeRejected | CallDispositionRejected | CallTargetRejected | CallArgumentsRejected
  | CallProvenanceRejected | CallReturnRejected | InvocationRejected | ViewRejected | StackRejected N.AdmissionError
  | StackContextRejected | UnsupportedActivation | UnsupportedCallback | GuestStoreRejected
  deriving (Eq, Show)

require :: Bool -> ProgramError -> Either ProgramError ()
require premise failure = unless premise (Left failure)
needed :: ProgramError -> Maybe a -> Either ProgramError a
needed failure = maybe (Left failure) Right
one :: ProgramError -> [a] -> Either ProgramError a
one _ [item] = Right item
one failure _ = Left failure
address :: W.Address -> Word32
address (W.Address number) = number
value :: W.WordValue -> Word32
value (W.WordValue number) = number
bytes :: W.Bytes -> B.ByteString
bytes (W.Bytes raw) = raw
hash :: B.ByteString -> W.Digest
hash = W.Digest . SHA.sha256Hex . SHA.sha256
physical :: Word32 -> Word32
physical number = number .&. 0x3fffffff
state :: Sample -> W.Context
state = contextState . sampleContext
core :: Sample -> KernelCore
core = contextCore . sampleContext
pc :: Sample -> Word32
pc = address . W.contextPc . state
sp :: Sample -> Word32
sp = address . W.contextSp . state
reg :: Sample -> Int -> Either ProgramError Word32
reg sample index = case index of
  13 -> Right (sp sample)
  14 -> Right (address (W.contextLr (state sample)))
  15 -> Right (pc sample+8)
  _ -> value <$> needed CallArgumentsRejected (if index >= 0 && index < 13 then case drop index (W.contextRegisters (state sample)) of x:_ -> Just x; _ -> Nothing else Nothing)

stockProgram :: Program
stockProgram = Stock
ownedProgram :: CanonicalImages -> NativeInputs -> Artifact.StagedCandidate -> Either ProgramError Program
ownedProgram images native candidate = do
  source <- either (Left . ArtifactRejected) Right (Artifact.canonicalSource (canonicalDat images) (canonicalImage4 images) (canonicalA images) (canonicalB images))
  artifact <- either (Left . ArtifactRejected) Right (Artifact.deriveArtifact (nativeFlat native) (nativeSymbols native))
  let staged = Artifact.stageImage4 source artifact
      supplied = Artifact.candidateStagedImage4 candidate
      proof = Artifact.frameProof artifact
  require (Artifact.stagedBytes staged == Artifact.stagedBytes supplied
    && Artifact.contractValue staged == Artifact.contractValue supplied
    && nativeCandidate native == Artifact.candidateBytes candidate
    && nativeContract native == L.toStrict (J.encode (Artifact.candidateContractValue candidate))) CandidateAssociationRejected
  sites <- ownedSites proof
  pure (Owned proof (Artifact.stagedBytes staged) (hash (nativeCandidate native))
    (OwnedProgram (hash (nativeContract native)) (hash (nativeFlat native)) (hash (nativeSymbols native))) sites (S.fromList (N.executionPcs proof)) (S.fromList (N.globalWrites proof)))
programImage4 :: Program -> B.ByteString -> B.ByteString
programImage4 Stock fallback = fallback
programImage4 (Owned _ image _ _ _ _ _) _ = image
programUpdateDigest :: Program -> W.Digest -> W.Digest
programUpdateDigest Stock fallback = fallback
programUpdateDigest (Owned _ _ digest _ _ _ _) _ = digest
programEvidence :: Program -> ProgramEvidence
programEvidence Stock = StockProgram
programEvidence (Owned _ _ _ evidence _ _ _) = evidence
programFrameProof :: Program -> Maybe N.FrameProof
programFrameProof Stock = Nothing
programFrameProof (Owned proof _ _ _ _ _ _) = Just proof
programAllowsPc :: Program -> W.Address -> Bool
programAllowsPc Stock _ = True
programAllowsPc (Owned _ _ _ _ _ execution _) (W.Address location) =
  location < 0x725dd8 || location >= 0x72612c || S.member (N.CodeAddress location) execution
programHasStore :: Program -> W.Address -> Bool
programHasStore Stock _ = False
programHasStore (Owned _ _ _ _ _ _ stores) (W.Address location) = S.member (N.CodeAddress location) stores
programGuestStore :: Program -> Sample -> Either ProgramError (Maybe GuestStore)
programGuestStore Stock _ = pure Nothing
programGuestStore program sample
  | not (programHasStore program (W.contextPc (state sample))) = pure Nothing
  | otherwise = case A.decode (pc sample) (value (sampleCodeWord sample)) of
      A.Store (A.Cond predicate) A.Word source base displacement indexing -> do
        require (source /= A.pc) GuestStoreRejected
        taken <- condition (predicate `shiftL` 28) (value (W.contextCpsr (state sample)))
        if not taken then pure Nothing else do
          baseValue <- register base
          delta <- case displacement of
            A.OffsetImmediate offset -> pure (fromIntegral offset)
            A.OffsetRegister positive index kind amount -> do
              original <- register index
              shifted <- case kind of
                A.LSL -> pure (original `shiftL` amount)
                A.LSR -> pure (original `shiftR` (if amount == 0 then 32 else amount))
                A.ASR -> pure (fromIntegral ((fromIntegral original :: Int32) `shiftR` (if amount == 0 then 32 else amount)))
                A.ROR | amount /= 0 -> pure (rotateR original amount)
                A.ROR -> pure ((original `shiftR` 1) .|. if testBit (value (W.contextCpsr (state sample))) 29 then 0x80000000 else 0)
              pure (if positive then shifted else negate shifted)
          stored <- register source
          pure (Just (GuestStore (W.Address (if indexing == A.PostIndexed then baseValue else baseValue+delta)) (W.WordValue stored)))
      _ -> Left GuestStoreRejected
  where register (A.Reg index) = reg sample index

legacySites :: M.Map W.Address Boundary
legacySites = M.fromList [(W.Address location,boundary) | (boundary,locations) <-
  [(SubmissionEntry,[0x725dd8]),(AuxiliaryReturn,[0x725f0c]),(CreationCall,[0x725fa8]),(CreationReturn,[0x725fac])
  ,(ActivationCall,[0x726024]),(ActivationReturn,[0x726028]),(WrapperReturn,[0x69ce8c,0x69cee8,0x69cf38,0x69cf70])
  ,(AutoReadyEntry,[0x43f9c,0xd3f58]),(AutoReadyReturn,[0x40a74,0xd0a74]),(Prepare,[0x43f70,0xd3f2c])
  ,(Prepared,[0x43f98,0xd3f54]),(Scheduler,[0x43688,0xd3688]),(Selected,[0x436bc,0xd36bc])
  ,(StackTransfer,[0x436e8,0xd36e8]),(Trampoline,[0x43728,0xd3728]),(EntryTransfer,[0x43758,0xd3758])
  ,(TaskEntry,[0x69d160,0x69cbf0,0x69cd58]),(EnteredInstruction,[0x69d164,0x69cbf4,0x69cd5c])],location <- locations]
stockCalls :: [(Word32,N.CallTarget)]
stockCalls = [(0x725dfc,f N.CriticalEnter),(0x725e84,f N.CriticalExit),(0x725fb4,f N.CriticalExit),(0x726064,f N.CriticalExit)
  ,(0x725e74,f N.Reservation),(0x725f68,f N.Fill),(0x725f78,f N.Cache),(0x725f08,N.AuxiliaryService)
  ,(0x725fa8,N.CreationService),(0x726024,N.ActivationService),(0x726000,N.CurrentCoreService)
  ,(0x725e94,f N.Log),(0x725fcc,f N.Log),(0x726040,f N.Log),(0x726074,f N.Log)
  ,(0x725ea8,f N.ErrorCallback),(0x725fe0,f N.ErrorCallback),(0x726054,f N.ErrorCallback),(0x726088,f N.ErrorCallback)
  ,(0x7260b8,N.CurrentCoreService),(0x7260c8,N.SaveMaskService),(0x7260e4,f N.TryLock),(0x726104,f N.Unlock),(0x72611c,N.RestoreMaskService)]
  where f = N.FixedService
callLabels :: N.CallTarget -> (Boundary,Boundary)
callLabels target = case target of
  N.FixedService fixed -> case fixed of
    N.CriticalEnter -> (CriticalEnterCall,CriticalEnterReturn); N.CriticalExit -> (CriticalExitCall,CriticalExitReturn)
    N.Reservation -> (ReservationCall,ReservationReturn); N.Fill -> (FillCall,FillReturn); N.Cache -> (CacheCall,CacheReturn)
    N.Log -> (LogCall,LogReturn); N.ErrorCallback -> (ErrorCallbackCall,ErrorCallbackReturn)
    N.TryLock -> (TryLockCall,TryLockReturn); N.Unlock -> (UnlockCall,UnlockReturn)
  N.AuxiliaryService -> (AuxiliaryCall,AuxiliaryReturn); N.CreationService -> (CreationCall,CreationReturn)
  N.ActivationService -> (ActivationCall,ActivationReturn); N.CurrentCoreService -> (IdentityCall,IdentityReturn)
  N.SaveMaskService -> (SaveMaskCall,SaveMaskReturn); N.RestoreMaskService -> (RestoreMaskCall,RestoreMaskReturn)
allTargets :: [N.CallTarget]
allTargets = map N.FixedService [minBound..maxBound] ++ [N.AuxiliaryService,N.CreationService,N.ActivationService,N.CurrentCoreService,N.SaveMaskService,N.RestoreMaskService]
callRole :: Boundary -> Maybe N.CallTarget
callRole boundary = find ((==boundary) . fst . callLabels) allTargets
returnRole :: Boundary -> Maybe N.CallTarget
returnRole boundary = find ((==boundary) . snd . callLabels) allTargets
insertSite :: M.Map W.Address Boundary -> (W.Address,Boundary) -> Either ProgramError (M.Map W.Address Boundary)
insertSite sites (location,boundary) = do
  require (maybe True (==boundary) (M.lookup location sites)) (SiteCollision location)
  pure (M.insert location boundary sites)
fixedSites :: [(W.Address,Boundary)]
fixedSites = [(W.Address 0x725dd8,SubmissionEntry),(W.Address 0x726098,ReleaseEntry),(W.Address 0x72612c,DeletionEntry),(W.Address 0x726240,ReleaseReturn)]
callSites :: Word32 -> N.CallTarget -> [(W.Address,Boundary)]
callSites location target = let (called,returned) = callLabels target in [(W.Address location,called),(W.Address (location+4),returned)]
ownedSites :: N.FrameProof -> Either ProgramError (M.Map W.Address Boundary)
ownedSites proof = do
  semantic <- foldM insertSite common (fixedSites ++ concatMap sites (N.callBoundaries proof))
  pure (foldr (\(N.CodeAddress location) -> M.insertWith (\_ previous -> previous) (W.Address location) NativeWrite) semantic (N.globalWrites proof))
  where common = M.filterWithKey (\(W.Address location) _ -> location < 0x725dd8 || location >= 0x72612c) legacySites
        sites boundary = [(W.Address instruction,fst labels),(W.Address continuation,snd labels)]
          where N.CodeAddress instruction = N.callInstruction boundary
                N.CodeAddress continuation = N.callContinuation boundary
                labels = callLabels (N.callTarget boundary)
currentStockSites :: Either ProgramError (M.Map W.Address Boundary)
currentStockSites = foldM insertSite legacySites (fixedSites ++ concatMap (uncurry callSites) stockCalls)
programSites :: Program -> ReceiptProtocol -> M.Map W.Address Boundary
programSites Stock LegacyStockProtocol = legacySites
programSites Stock TaskSubmissionProtocol = either (const M.empty) id currentStockSites
programSites (Owned _ _ _ _ sites _ _) TaskSubmissionProtocol = sites
programSites (Owned _ _ _ _ _ _ _) LegacyStockProtocol = M.empty
programClasses :: Program -> ReceiptProtocol -> [(SourceKind,Word32,B.ByteString)] -> [W.WatchClass]
programClasses program protocol sources = [W.WatchClass range writes | range <- oldRanges ++ extra]
  ++ [W.WatchClass [fromIntegral base,fromIntegral base+fromIntegral (B.length actual)] [W.CpuWrite,W.CpuFetch,W.HostWrite,W.ModelBusWrite]
     | (kind,base,contents) <- sources, let actual = if kind == Image4 then programImage4 program contents else contents]
  where writes = [W.CpuWrite,W.HostWrite,W.ModelBusWrite]
        oldRanges = [[0x4dae0,0x4f10c],[0xddaa0,0xdf0cc],[0x545a4,0x63460],[0xe4564,0xf3410],[0xc0800,0xc0900]
          ,[0x96bd20,0x96bd28],[0xa0fc44,0xa101e4],[0xa5b204,0xa5b5c4],[0x89ea94,0x89ea9c],[0x9c280c,0x9c2810],[0xa10200,0xa5b200]]
        extra = case protocol of LegacyStockProtocol -> []; TaskSubmissionProtocol -> [[0xa0f82c,0xa0f830],[0x4f15c,0x4f160],[0x4f25c,0x4f260],[0x4f5e4,0x4f60a]]
checkProgramHeader :: Program -> Receipt -> Either ProgramError ()
checkProgramHeader program receipt = do
  case (program,receiptSchema receipt) of
    (Stock,LegacyStockProtocol) -> require (receiptProgram receipt == LegacyStockProgram && receiptScope receipt == StockStartupScope) HeaderRejected
    (Stock,TaskSubmissionProtocol) -> do
      _ <- currentStockSites
      require (receiptProgram receipt == StockProgram && receiptScope receipt `elem` [StockStartupScope,StockProbeScope]) HeaderRejected
    (Owned _ _ digest evidence _ _ _,TaskSubmissionProtocol) -> do
      let Identity _ update _ _ _ = receiptIdentity receipt
      require (receiptProgram receipt == evidence && update == digest && receiptScope receipt `elem` [OwnedStartupScope,OwnedProbeScope]) HeaderRejected
    _ -> Left HeaderRejected
  require (receiptSites receipt == programSites program (receiptSchema receipt)) HeaderRejected

views :: Sample -> [View]
views sample = sampleLocal sample ++ concatMap kernelViews (sampleKernels sample)
checkViews :: Sample -> Either ProgramError ()
checkViews sample = mapM_ check (views sample)
  where check view = do
          let start = address (viewPhysical view); raw = bytes (viewBytes view)
          require (not (B.null raw) && toInteger start+toInteger (B.length raw) <= 0x20000000
            && viewAliases view == replicate 2 (replicate 2 (hash raw))) ViewRejected
wordAt :: Sample -> Word32 -> Either ProgramError Word32
wordAt sample location = do
  let start = physical location
      candidates = [U.word32At raw (fromIntegral (start-base)) | view <- views sample
        , let base = address (viewPhysical view), let raw = bytes (viewBytes view)
        , start >= base, toInteger start+4 <= toInteger base+toInteger (B.length raw)]
  case candidates of
    first:rest -> do
      require (location .&. 3 == 0 && all (==first) rest) ViewRejected
      pure first
    [] -> Left ViewRejected
objectWord :: Sample -> ObjectKind -> Word32 -> Either ProgramError Word32
objectWord sample kind location = do
  require (any (\view -> viewObject view == kind && physical location >= address (viewPhysical view)
    && toInteger (physical location)+4 <= toInteger (address (viewPhysical view))+toInteger (B.length (bytes (viewBytes view)))) (views sample)) ViewRejected
  wordAt sample location
condition :: Word32 -> Word32 -> Either ProgramError Bool
condition instruction cpsr = case instruction `shiftR` 28 of
  0 -> pure z; 1 -> pure (not z); 2 -> pure c; 3 -> pure (not c); 4 -> pure n; 5 -> pure (not n); 6 -> pure v; 7 -> pure (not v)
  8 -> pure (c && not z); 9 -> pure (not c || z); 10 -> pure (n == v); 11 -> pure (n /= v)
  12 -> pure (not z && n == v); 13 -> pure (z || n /= v); 14 -> pure True; _ -> Left CodeRejected
  where n = testBit cpsr 31; z = testBit cpsr 30; c = testBit cpsr 29; v = testBit cpsr 28
actualTarget :: Sample -> Either ProgramError (Word32,Maybe A.Reg)
actualTarget sample
  | instruction .&. 0x0ffffff0 == 0x012fff30 = do
      let register = fromIntegral (instruction .&. 15)
      target <- reg sample register
      pure (target,Just (A.Reg register))
  | instruction .&. 0x0f000000 == 0x0b000000 =
      let displacement = fromIntegral ((fromIntegral ((instruction .&. 0xffffff) `shiftL` 8) :: Int32) `shiftR` 6)
      in pure (pc sample+8+displacement,Nothing)
  | otherwise = Left CodeRejected
  where instruction = value (sampleCodeWord sample)

prior :: Receipt -> Sample -> [Sample]
prior receipt sample = filter (\other -> core other == core sample && sampleHit other < sampleHit sample) (receiptSamples receipt)
data Invocation = Invocation N.EntryPoint Sample
invocation :: Receipt -> Sample -> Either ProgramError Invocation
invocation receipt sample = do
  entry <- needed InvocationRejected (find (\other -> sampleBoundary other `elem` [SubmissionEntry,ReleaseEntry]) (reverse (prior receipt sample ++ [sample])))
  let kind = if sampleBoundary entry == SubmissionEntry then N.Submission else N.Release
  require (not (any (\other -> sampleHit other > sampleHit entry && sampleBoundary other `elem` [WrapperReturn,ReleaseReturn,SubmissionEntry,ReleaseEntry]) (prior receipt sample))) InvocationRejected
  pure (Invocation kind entry)
latest :: Receipt -> Sample -> Sample -> Boundary -> Either ProgramError Sample
latest receipt sample entry boundary = needed CallArgumentsRejected (find (\other -> sampleHit other >= sampleHit entry && sampleBoundary other == boundary) (reverse (prior receipt sample)))
classOf :: Sample -> Word32 -> Either ProgramError Word32
classOf sample ident = do
  require (ident >= 1 && ident <= 104) CallArgumentsRejected
  actual <- wordAt sample (0x7cad9c+4*(ident-1))
  let expected = if ident `elem` [45,46,62,65,66,88,89,90,93,103] then 2 else 1
  require (actual == expected) CallArgumentsRejected
  pure actual
data TaskDescriptor = TaskDescriptor
  { descriptorIdentifier :: TaskId
  , descriptorPriority :: W.WordValue
  , descriptorEntry :: W.Address
  , descriptorArgument :: W.WordValue
  , descriptorTail :: W.WordValue
  }
descriptorNumber :: TaskDescriptor -> Word32
descriptorNumber snapshot = case descriptorIdentifier snapshot of TaskId ident -> fromIntegral ident
descriptor :: Sample -> Sample -> Either ProgramError TaskDescriptor
descriptor entry sample = do
  pointer <- reg entry 0
  require (pointer .&. 3 == 0) CallArgumentsRejected
  ident <- wordAt sample pointer
  require (ident >= 1 && ident <= 104) CallArgumentsRejected
  _ <- wordAt sample (pointer+4)
  priority <- W.WordValue <$> wordAt sample (pointer+8)
  _ <- wordAt sample (pointer+12)
  start <- W.Address <$> wordAt sample (pointer+16)
  argument <- W.WordValue <$> wordAt sample (pointer+20)
  tailWord <- W.WordValue <$> wordAt sample (pointer+24)
  pure (TaskDescriptor (TaskId (fromIntegral ident)) priority start argument tailWord)
expectedTarget :: Sample -> Sample -> N.CallTarget -> Either ProgramError Word32
expectedTarget entry sample target = case target of
  N.FixedService fixed -> pure (case fixed of
    N.CriticalEnter -> 0x7235d8; N.CriticalExit -> 0x7236b0; N.Reservation -> 0x725c6c; N.Fill -> 0x7221c8
    N.Cache -> 0x42634; N.Log -> 0x41a34; N.ErrorCallback -> 0x723398; N.TryLock -> 0x427d4; N.Unlock -> 0x427fc)
  _ -> do
    (rootIndex,slot,pinned) <- case target of
      N.AuxiliaryService -> selected 0x80 [0x4d39c,0xdd360]
      N.CreationService -> selected 0 [0x4d094,0xdd054]
      N.ActivationService -> pure (coreIndex (core sample),12,if core sample == PrimaryKernel then 0x44f2c else 0xd4eec)
      N.CurrentCoreService -> pure (0,0x1618,0x412bc)
      N.SaveMaskService -> pure (coreIndex (core sample),0x164,if core sample == PrimaryKernel then 0x41140 else 0xd1140)
      N.RestoreMaskService -> pure (coreIndex (core sample),0x168,if core sample == PrimaryKernel then 0x41164 else 0xd1164)
    root <- objectWord sample InterfaceRoots (0x96bd20+4*fromIntegral rootIndex)
    require (root == if rootIndex == 0 then 0x4004dae0 else 0x400ddaa0) CallTargetRejected
    actual <- wordAt sample (root+slot)
    require (actual == pinned) CallTargetRejected
    pure actual
  where selected slot targets = do
          initial <- descriptor entry entry
          selectedClass <- classOf entry (descriptorNumber initial)
          selectedTarget <- case (selectedClass,targets) of
            (1,primary:_) -> pure primary
            (2,_:secondary:_) -> pure secondary
            _ -> Left CallTargetRejected
          pure (fromIntegral selectedClass-1,slot,selectedTarget)

checkProvenance :: Sample -> Sample -> N.ArgumentProvenance -> Word32 -> Either ProgramError ()
checkProvenance entry sample provenance actual = case provenance of
  N.ConstantArgument expected -> require (actual == expected) CallProvenanceRejected
  N.StackArgument (N.StackOffset offset) -> require (toInteger actual == toInteger (sp entry)+offset) CallProvenanceRejected
  N.SavedRegisterArgument (A.Reg register) -> reg entry register >>= \expected -> require (actual == expected) CallProvenanceRejected
  N.ReturnCapabilityArgument -> require (actual == address (W.contextLr (state entry))) CallProvenanceRejected
  N.ServiceArgument target -> expectedTarget entry sample target >>= \expected -> require (actual == expected) CallProvenanceRejected
  N.NonstackArgument -> require (actual < sp sample || actual >= sp entry) CallProvenanceRejected
  N.ScalarArgument -> pure ()
checkAlternative :: Sample -> Sample -> N.CallTarget -> Maybe A.Reg -> N.CallBoundary -> Either ProgramError ()
checkAlternative entry sample target targetRegister boundary = do
  require (N.callTarget boundary == target && N.callTargetRegister boundary == targetRegister) CallProvenanceRejected
  reg sample 0 >>= checkProvenance entry sample (N.callR0 boundary)
  reg sample 1 >>= checkProvenance entry sample (N.callR1 boundary)
  mapM_ (\parameter -> do
    let N.StackOffset offset = N.parameterOffset parameter
        location = toInteger (sp entry)+offset
    require (location >= 0 && location+4 <= 0x100000000) CallProvenanceRejected
    wordAt sample (fromInteger location) >>= checkProvenance entry sample (N.parameterProvenance parameter)) (N.callParameters boundary)

parameters :: Sample -> Sample -> Int -> Either ProgramError [Word32]
parameters entry sample count = do
  pointer <- reg sample 1
  require (pointer .&. 3 == 0 && pointer >= sp sample && toInteger pointer+4*toInteger count <= toInteger (sp entry)) CallArgumentsRejected
  require (any (\view -> viewObject view == Parameters && address (viewPhysical view) == physical pointer && B.length (bytes (viewBytes view)) == 4*count) (sampleLocal sample)) ViewRejected
  mapM (wordAt sample . (pointer+) . fromIntegral . (*4)) [0..count-1]
arguments :: Sample -> [Word32] -> Either ProgramError ()
arguments sample expected = mapM (reg sample) [0..length expected-1] >>= \actual -> require (actual == expected) CallArgumentsRejected
checkArguments :: Receipt -> Sample -> Sample -> N.EntryPoint -> N.CallTarget -> Either ProgramError ()
checkArguments receipt entry sample kind target = case target of
  N.CurrentCoreService -> pure ()
  N.SaveMaskService -> pure ()
  N.RestoreMaskService -> do saved <- latest receipt sample entry SaveMaskReturn; reg saved 0 >>= \mask -> arguments sample [mask]
  N.FixedService N.TryLock -> arguments sample [0x409c280c,if core sample == PrimaryKernel then 1 else 2]
  N.FixedService N.Unlock -> arguments sample [0x409c280c]
  N.FixedService N.CriticalEnter -> pure ()
  N.FixedService N.CriticalExit -> do saved <- latest receipt sample entry CriticalEnterReturn; reg saved 0 >>= \token -> arguments sample [token]
  _ -> do
    require (kind == N.Submission) CallArgumentsRejected
    d <- descriptor entry sample
    let ident = descriptorNumber d; index = ident-1; wrapper = 0x40a5b204+8*index
    taskClass <- classOf sample ident
    case target of
      N.AuxiliaryService -> do
        r0 <- reg sample 0; require (r0 == taskClass `shiftL` 16 .|. ident) CallArgumentsRejected
        actual <- parameters entry sample 2
        require (actual == [if descriptorTail d == W.WordValue 0xa5f0a5f0 then 0 else 4,0]) CallArgumentsRejected
      N.CreationService -> do
        r0 <- reg sample 0; require (r0 == taskClass `shiftL` 16 .|. ident) CallArgumentsRejected
        base <- wordAt sample wrapper; size <- wordAt sample (wrapper+4)
        actual <- parameters entry sample 6
        require (actual == [2,value (descriptorArgument d),address (descriptorEntry d),value (descriptorPriority d),size,base]) CallArgumentsRejected
      N.ActivationService -> arguments sample [taskClass `shiftL` 16 .|. ident]
      N.FixedService N.Reservation -> do size <- wordAt sample (wrapper+4); arguments sample [index,size,wrapper]
      N.FixedService N.Fill -> do base <- wordAt sample wrapper; size <- wordAt sample (wrapper+4); arguments sample [base,ident .&. 0xff,size]
      N.FixedService N.Cache -> do base <- wordAt sample wrapper; size <- wordAt sample (wrapper+4); arguments sample [base .&. 0xbfffffff,size]
      N.FixedService N.Log -> do
        event <- reg sample 0; r1 <- reg sample 1
        require (event `elem` [3,37] && r1 == ident) CallArgumentsRejected
        failed <- needed CallArgumentsRejected (find (\other -> sampleHit other >= sampleHit entry && sampleBoundary other `elem` [ReservationReturn,AuxiliaryReturn,CreationReturn,ActivationReturn]) (reverse (prior receipt sample)))
        status <- reg failed 0; r2 <- reg sample 2
        require (status /= 0 && r2 == status && event == if sampleBoundary failed == ReservationReturn then 37 else 3) CallArgumentsRejected
      N.FixedService N.ErrorCallback -> do
        status <- objectWord sample ErrorState 0x40a0f82c
        logged <- latest receipt sample entry LogCall
        loggedStatus <- reg logged 2
        require (status == loggedStatus) CallArgumentsRejected
        arguments sample [status]

maximumDemand :: N.CallTarget -> Integer
maximumDemand target = case target of
  N.FixedService fixed -> case fixed of
    N.CriticalEnter -> 40; N.CriticalExit -> 32; N.Reservation -> 40; N.Fill -> 12; N.Cache -> 36
    N.Log -> 160; N.ErrorCallback -> 12; N.TryLock -> 0; N.Unlock -> 0
  N.AuxiliaryService -> 16; N.CreationService -> 16; N.ActivationService -> 40
  N.CurrentCoreService -> 8; N.SaveMaskService -> 4; N.RestoreMaskService -> 4
actualDemand :: Sample -> Sample -> N.CallTarget -> Either ProgramError Integer
actualDemand entry sample target = case target of
  N.FixedService N.Cache -> do base <- reg sample 0; size <- reg sample 1; pure (if base /= 0 && size /= 0 then 0 else 36)
  N.FixedService N.ErrorCallback -> do
    callback <- objectWord sample Callback 0x4089ea8c
    require (callback `elem` [0x723330,0x69cbf0]) UnsupportedCallback
    pure 12
  N.FixedService N.Log -> do
    event <- reg sample 0
    require (event `elem` [3,37]) CallArgumentsRejected
    require (any (\view -> viewObject view == LogEnable && address (viewPhysical view) == 0x4f5e4 && B.length (bytes (viewBytes view)) == 38) (sampleLocal sample)) ViewRejected
    byte <- byteAt sample (0x4004f5e4+event)
    if byte == 0 then pure 32 else do
      mode <- objectWord sample LogMode 0x4004f25c
      if mode /= 1 then pure 36 else do
        callback <- objectWord sample LogCallback 0x4004f15c
        require (callback `elem` [0,0x6470fc]) UnsupportedCallback
        pure (if callback == 0 then 40 else 160)
  N.ActivationService -> if testBit (value (W.contextCpsr (state sample))) 7 then pure 24 else do
    d <- descriptor entry sample
    kernel <- one UnsupportedActivation (filter ((==core sample) . kernelCore) (sampleKernels sample))
    current <- one UnsupportedActivation (filter ((==kernelCurrent kernel) . taskNode) (kernelTasks kernel))
    selected <- one UnsupportedActivation (filter ((==kernelSelected kernel) . taskNode) (kernelTasks kernel))
    fresh <- one UnsupportedActivation (filter ((==TaskId 1) . taskId) (kernelTasks kernel))
    contextWord <- wordAt sample (if core sample == PrimaryKernel then 0x400545a4 else 0x400e4564)
    require (core sample == PrimaryKernel && descriptorIdentifier d == TaskId 1 && descriptorPriority d == W.WordValue 15 && contextWord == 0
      && taskId current == TaskId 104 && taskId selected == TaskId 104
      && taskEffectivePriority current == 0 && taskEffectivePriority selected == 0
      && taskCreated fresh == W.WordValue 1 && taskState fresh == 0 && taskPending fresh .&. 1 == 0
      && taskEffectivePriority fresh == 14) UnsupportedActivation
    pure 40
  _ -> pure (maximumDemand target)
byteAt :: Sample -> Word32 -> Either ProgramError Word32
byteAt sample location = do
  let at = physical location
      candidates = [fromIntegral (B.index raw (fromIntegral (at-base))) | view <- views sample
        , let base = address (viewPhysical view), let raw = bytes (viewBytes view)
        , at >= base, toInteger at < toInteger base+toInteger (B.length raw)]
  case candidates of
    first:rest -> require (all (==first) rest) ViewRejected >> pure first
    [] -> Left ViewRejected

stackContext :: Sample -> Sample -> Either ProgramError N.StackContext
stackContext entry sample = do
  let incoming = physical (sp entry)
      boot = if core entry == PrimaryKernel then 0x40063420 else 0x400f33e0
  (low,high) <- if sp entry == boot then do
    let low = if core entry == PrimaryKernel then 0x62c54 else 0xf2c14
    pure (low,physical (boot+32))
    else do
      kernel <- one StackContextRejected (filter ((==core entry) . kernelCore) (sampleKernels sample))
      current <- one StackContextRejected (filter ((==kernelCurrent kernel) . taskNode) (kernelTasks kernel))
      require (taskId current == TaskId 104 && taskStackBytes current == W.WordValue 1024) StackContextRejected
      let base = physical (address (taskStackBase current)); top = toInteger base+1024
      require (top <= 0x20000000 && (toInteger incoming == top-16 || sampleBoundary entry == ReleaseEntry && toInteger incoming == top-40)) StackContextRejected
      pure (base,fromInteger top)
  require (low < incoming && incoming < high && sp entry .&. 7 == 0) StackContextRejected
  require (any (\view -> viewObject view == Stack && address (viewPhysical view) == low && toInteger (B.length (bytes (viewBytes view))) == toInteger high-toInteger low) (sampleLocal sample)) StackContextRejected
  let interval begin end = N.StackInterval (N.StackOffset (toInteger begin-toInteger incoming)) (N.StackOffset (toInteger end-toInteger incoming))
      objects = [interval begin end | view <- views sample
        , let begin = address (viewPhysical view), let end = begin+fromIntegral (B.length (bytes (viewBytes view)))
        , begin < high && low < end
        , not (viewObject view == Stack && begin == low && end == high)
        , viewObject view /= Parameters]
  pure (N.StackContext incoming (interval low high) (interval incoming high:objects))
checkStack :: Program -> Receipt -> Sample -> Sample -> N.EntryPoint -> Maybe (N.CallTarget,Integer) -> Either ProgramError ()
checkStack program receipt entry sample kind reached = do
  context <- stackContext entry sample
  case program of
    Owned proof _ _ _ _ _ _ -> do
      let demands = [N.CalleeDemand target (N.StackOffset (negate (case reached of Just (actual,demand) | actual == target -> demand; _ -> maximumDemand target)))
            | target <- nub (map N.callTarget (N.callBoundaries proof))]
      _ <- either (Left . StackRejected) Right (N.joinStack proof kind context demands)
      pure ()
    Stock -> do
      let own = if kind == N.Submission then 56 else 32
          nested = case reached of Just (_,demand) -> toInteger (sp sample)-toInteger (sp entry)-demand; Nothing -> negate own
      checkInterval context (min (negate own) nested) 0
  if kind == N.Release then do
    outer <- needed StackContextRejected (find ((==DeletionEntry) . sampleBoundary) (reverse (prior receipt entry)))
    require (sp outer == sp entry+24 && core sample == PrimaryKernel && W.contextLr (state entry) == W.Address 0x726240) StackContextRejected
    arguments outer [102]
    arguments entry [101]
    outerContext <- stackContext outer sample
    checkInterval outerContext (-80) 0
    kernel <- one StackContextRejected (filter ((==core sample) . kernelCore) (sampleKernels sample))
    current <- one StackContextRejected (filter ((==kernelCurrent kernel) . taskNode) (kernelTasks kernel))
    require (taskId current == TaskId 104 && kernelSelected kernel == kernelCurrent kernel) StackContextRejected
  else pure ()
checkInterval :: N.StackContext -> Integer -> Integer -> Either ProgramError ()
checkInterval context low high = do
  let N.StackInterval (N.StackOffset writableLow) (N.StackOffset writableHigh) = N.writableStack context
      absolute = toInteger (N.incomingStackAddress context)
      overlaps (N.StackInterval (N.StackOffset start) (N.StackOffset end)) = low < end && start < high
  require (absolute+low >= 0 && absolute+high <= 0x100000000 && low >= writableLow && high <= writableHigh
    && not (any overlaps (N.liveObjects context))) StackContextRejected

checkCall :: Program -> Receipt -> Sample -> N.CallTarget -> Either ProgramError ()
checkCall _ receipt sample target | receiptSchema receipt == LegacyStockProtocol = checkLegacyCall receipt sample target
checkCall program receipt sample target = do
  Invocation kind entry <- invocation receipt sample
  (actual,targetRegister) <- actualTarget sample
  taken <- condition (value (sampleCodeWord sample)) (value (W.contextCpsr (state sample)))
  require (sampleCallTaken sample == Just taken) CallDispositionRejected
  case program of
    Owned proof _ _ _ _ _ _ -> do
      let alternatives = [boundary | boundary <- N.callBoundaries proof,N.callEntry boundary == kind
            , N.callInstruction boundary == N.CodeAddress (pc sample)
            , let N.StackOffset offset = N.callStack boundary, toInteger (sp sample) == toInteger (sp entry)+offset]
      require (not (null alternatives)) CallProvenanceRejected
      if taken then require (any (\boundary -> checkAlternative entry sample target targetRegister boundary == Right ()) alternatives) CallProvenanceRejected
        else require (any (\boundary -> N.callTarget boundary == target && N.callTargetRegister boundary == targetRegister) alternatives) CallProvenanceRejected
    Stock -> require (lookup (pc sample) stockCalls == Just target && sp sample == sp entry-(if kind == N.Submission then 56 else 32)) CallProvenanceRejected
  if taken then do
    expected <- expectedTarget entry sample target
    require (actual == expected && actual .&. 3 == 0) CallTargetRejected
    checkArguments receipt entry sample kind target
    demand <- actualDemand entry sample target
    checkStack program receipt entry sample kind (Just (target,demand))
  else checkStack program receipt entry sample kind Nothing
  checkContinuation receipt sample target taken

checkContinuation :: Receipt -> Sample -> N.CallTarget -> Bool -> Either ProgramError ()
checkContinuation receipt sample target taken = do
  let following = filter (\other -> core other == core sample && sampleHit other > sampleHit sample) (receiptSamples receipt)
  next <- if taken && target == N.ActivationService && not (testBit (value (W.contextCpsr (state sample))) 7) then
    case following of
      preparing:prepared:returned:_ -> do
        require (core sample == PrimaryKernel && sampleBoundary preparing == Prepare && pc preparing == 0x43f70
          && sampleBoundary prepared == Prepared && pc prepared == 0x43f98
          && sampleCallTaken preparing == Nothing && sampleCallTaken prepared == Nothing
          && toInteger (sp preparing) == toInteger (sp sample)-24 && toInteger (sp prepared) == toInteger (sp sample)-32
          && W.contextLr (state preparing) == W.Address 0x45060
          && W.contextLr (state prepared) == W.Address 0x43f98
          && all (\point -> value (W.contextCpsr (state point)) .&. 0x80 == 0x80) [preparing,prepared]) CallReturnRejected
        arguments preparing [0x400545cc,0x4005ff0c]
        arguments prepared [0,0x4005ff0c]
        reg preparing 11 >>= \saved -> wordAt prepared (sp prepared) >>= \actual -> require (actual == saved) CallReturnRejected
        wordAt prepared (sp prepared+4) >>= \saved -> require (saved == 0x45060) CallReturnRejected
        kernel <- one CallReturnRejected (filter ((==PrimaryKernel) . kernelCore) (sampleKernels prepared))
        fresh <- one CallReturnRejected (filter ((==TaskId 1) . taskId) (kernelTasks kernel))
        resume <- objectWord prepared Domains 0x40054648
        require (taskState fresh == 1
          && toInteger (address (taskSavedSp fresh)) == toInteger (address (taskStackBase fresh))+toInteger (value (taskStackBytes fresh))
          && taskResumePc fresh == W.Address resume) CallReturnRejected
        pure returned
      _ -> Left CallReturnRejected
    else needed CallReturnRejected (case following of first:_ -> Just first; _ -> Nothing)
  if pc next == pc sample+4 && sampleBoundary next == snd (callLabels target) then
    require (sampleCallTaken next == Just taken) CallDispositionRejected
  else do
      require (taken && target == N.FixedService N.ErrorCallback) CallReturnRejected
      callback <- objectWord sample Callback 0x4089ea8c
      require (callback == 0x69cbf0 && sampleBoundary next == TaskEntry && pc next == callback
        && toInteger (sp next) == toInteger (sp sample)-8) CallReturnRejected
      entered <- one CallReturnRejected (drop 1 following)
      require (sampleBoundary entered == EnteredInstruction && pc entered == callback+4
        && toInteger (sp entered) == toInteger (sp next)-4
        && W.contextRegisters (state entered) == W.contextRegisters (state next)
        && W.contextLr (state entered) == W.contextLr (state next)
        && W.contextCpsr (state entered) == W.contextCpsr (state next)) CallReturnRejected
      reg next 11 >>= \saved -> wordAt entered (sp entered) >>= \actual -> require (actual == saved) CallReturnRejected

checkLegacyCall :: Receipt -> Sample -> N.CallTarget -> Either ProgramError ()
checkLegacyCall receipt sample target = do
  Invocation kind entry <- invocation receipt sample
  require (kind == N.Submission && sampleCallTaken sample == Nothing && sp sample == sp entry-56) CallDispositionRejected
  taken <- condition (value (sampleCodeWord sample)) (value (W.contextCpsr (state sample)))
  require taken CallDispositionRejected
  (actual,_) <- actualTarget sample
  d <- descriptor entry sample
  let ident = descriptorNumber d
      taskClass = if ident `elem` [45,46,62,65,66,88,89,90,93,103] then 2 else 1
      encoded = taskClass `shiftL` 16 .|. ident
  case target of
    N.CreationService -> do
      require (actual == if taskClass == 1 then 0x4d094 else 0xdd054) CallTargetRejected
      pointer <- reg sample 1
      arguments sample [encoded,pointer]
      require (pointer == sp entry-48) CallArgumentsRejected
      base <- wordAt sample (0x40a5b204+8*(ident-1))
      size <- wordAt sample (0x40a5b208+8*(ident-1))
      actualParameters <- mapM (wordAt sample . (pointer+)) [0,4..20]
      require (actualParameters == [2,value (descriptorArgument d),address (descriptorEntry d),value (descriptorPriority d),size,base]) CallArgumentsRejected
    N.ActivationService -> do
      require (actual == if core sample == PrimaryKernel then 0x44f2c else 0xd4eec) CallTargetRejected
      arguments sample [encoded]
    _ -> Left CallTargetRejected
checkReturn :: Program -> Receipt -> Sample -> N.CallTarget -> Either ProgramError ()
checkReturn program receipt sample target = do
  let previous = reverse (prior receipt sample)
      returnedAfter called other = sampleHit other > sampleHit called
        && returnRole (sampleBoundary other) == Just target && pc other == pc sample
  case find (\other -> pc other+4 == pc sample && callRole (sampleBoundary other) == Just target) previous of
    Just called | not (any (returnedAfter called) previous) -> paired called
    _ -> case program of
      -- Owned continuation PCs may also be branch targets.
      Owned _ _ _ _ _ _ _ -> require (sampleCallTaken sample == Just False) CallReturnRejected
      Stock -> Left CallReturnRejected
  where
    paired called = do
      taken <- condition (value (sampleCodeWord called)) (value (W.contextCpsr (state called)))
      require (sampleCallTaken sample == Just taken) CallDispositionRejected
      checkCall program receipt called target
      require (W.contextSp (state called) == W.contextSp (state sample)
        && take 8 (drop 4 (W.contextRegisters (state called))) == take 8 (drop 4 (W.contextRegisters (state sample)))
        && value (W.contextCpsr (state called)) .&. 0x1f == value (W.contextCpsr (state sample)) .&. 0x1f) CallReturnRejected
      if taken then pure ()
        else require ((state sample) {W.contextPc = W.contextPc (state called),W.contextCounts = W.contextCounts (state called)} == state called) CallReturnRejected
      case target of
        N.CurrentCoreService | taken -> reg sample 0 >>= \identity -> require (identity == fromIntegral (coreIndex (core sample))) CallReturnRejected
        N.ActivationService | taken && testBit (value (W.contextCpsr (state called))) 7 -> reg sample 0 >>= \status -> require (status == 0xffffffe7) CallReturnRejected
        _ -> pure ()

checkProgramSample :: Program -> Receipt -> Sample -> Either ProgramError ()
checkProgramSample program receipt sample = do
  require (M.lookup (W.contextPc (state sample)) (programSites program (receiptSchema receipt)) == Just (sampleBoundary sample)
    && pc sample .&. 3 == 0 && length (W.contextRegisters (state sample)) == 13) CodeRejected
  case program of
    Owned _ image _ _ _ _ _ | pc sample >= 0x725dd8 && pc sample < 0x72612c ->
      require (sampleCodeWord sample == W.WordValue (U.word32At image (fromIntegral (pc sample-0x61c400)))) CodeRejected
    _ -> pure ()
  if receiptSchema receipt == LegacyStockProtocol then pure () else checkViews sample
  case callRole (sampleBoundary sample) of
    Just target -> checkCall program receipt sample target
    Nothing -> case returnRole (sampleBoundary sample) of
      Just target | receiptSchema receipt == TaskSubmissionProtocol -> checkReturn program receipt sample target
      _ -> do
        require (sampleCallTaken sample == Nothing) CallDispositionRejected
        if sampleBoundary sample `elem` [SubmissionEntry,ReleaseEntry] && receiptSchema receipt == TaskSubmissionProtocol then do
          let kind = if sampleBoundary sample == SubmissionEntry then N.Submission else N.Release
          checkStack program receipt sample sample kind Nothing
        else pure ()
