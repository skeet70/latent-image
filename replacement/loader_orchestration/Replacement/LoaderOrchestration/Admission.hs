module Replacement.LoaderOrchestration.Admission
  ( AdmissionError(..), FrameProof, PathProof(..), Effect(..), Terminal(..), CompletionState(..)
  , Scalar(..), StackOffset(..), NopCount(..), CallTarget(..)
  , DispatcherBoundary, boundarySide, boundaryScenario, boundaryPhase, boundaryInstruction, boundaryContinuation
  , boundaryStack, boundaryFramePointer, boundaryLiveWords, dispatcherBoundaries
  , EffectSite, effectSites, effectScenario, effectKind, effectInstruction
  , ServiceEntryState, serviceEntryStates, entryScenario, entrySite, entryService, entryRegister
  , deriveFrameProof, proofPaths, executionPcs, callBoundaries, frameLow
  ) where

import qualified A32 as A
import Control.Monad (unless)
import Data.Bits ((.&.), (.|.), complement, shiftL)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import Replacement.LoaderOrchestration.Interface

newtype StackOffset = StackOffset Int deriving (Eq, Ord, Show)
newtype NopCount = NopCount Word32 deriving (Eq, Show)
newtype CallOrdinal = CallOrdinal Int deriving (Eq, Show)
newtype StepCount = StepCount Int
newtype PollCount = PollCount Int

data Scalar = Word Word32 | NonzeroWord | Stack StackOffset | Incoming A.Reg | IncomingLink
  | DispatchCapability | CallbackCapability | Returned CallOrdinal A.Reg deriving (Eq, Show)
data CompletionState = Pending | Published deriving (Eq, Show)
data Effect = ServiceCall Service [(A.Reg,Scalar)] | DispatchCall Phase | CompletionRead CompletionState
  | DelayNops NopCount | CompletionPublication | StoreBarrier | ReadyPublication deriving (Eq, Show)
data Terminal = WaitingForCompletion | SetupDidNotReturn | EnteredScheduler deriving (Eq, Show)
data CallTarget = Retained Service | Dispatcher deriving (Eq, Ord, Show)
data PathProof = PathProof Scenario [Effect] Terminal deriving (Eq, Show)
data DispatcherBoundary = DispatcherBoundary LoaderSide Scenario Phase CodeAddress CodeAddress
  StackOffset StackOffset (M.Map StackOffset Scalar) deriving (Eq, Show)
data EffectSite = EffectSite Scenario Effect CodeAddress deriving (Eq, Show)
-- | The registers as a retained service is entered: its call site, the service and the state with lr the continuation.
data ServiceEntryState = ServiceEntryState Scenario CodeAddress Service (M.Map A.Reg Scalar) deriving (Eq, Show)
data FrameProof = FrameProof [PathProof] (S.Set CodeAddress) (M.Map CodeAddress CallTarget) [DispatcherBoundary] [EffectSite] [ServiceEntryState]
data PathResult = PathResult
  { resultPath :: PathProof, resultPcs :: S.Set CodeAddress
  , resultCalls :: M.Map CodeAddress CallTarget, resultDispatcherCalls :: [DispatcherBoundary]
  , resultLocations :: [EffectSite], resultServiceEntries :: [ServiceEntryState]
  }
data AdmissionError = ExtentRejected | CompilerPrologueRejected | UnsupportedInstruction CodeAddress
  | FrameRejected | OperandRejected | ConditionRejected | MemoryRejected | CallRejected CodeAddress
  | EffectRejected | StepLimit | ControlFlowRejected CodeAddress | PollingInvariantRejected deriving (Eq, Show)

data PollCheckpoint = PollCheckpoint CodeAddress (M.Map A.Reg Scalar) (M.Map StackOffset Scalar) (Maybe Bool) CallOrdinal deriving (Eq)

data Machine = Machine
  { registers :: M.Map A.Reg Scalar, stack :: M.Map StackOffset Scalar, zero :: Maybe Bool
  , effects :: [Effect], visited :: S.Set CodeAddress, boundaries :: M.Map CodeAddress CallTarget, dispatcherEdges :: [DispatcherBoundary]
  , polls :: PollCount, checkpoint :: Maybe PollCheckpoint, calls :: CallOrdinal, steps :: StepCount, terminal :: Maybe Terminal
  , locations :: [(Effect,CodeAddress)], serviceEntries :: [(CodeAddress,Service,M.Map A.Reg Scalar)]
  }

proofPaths :: FrameProof -> [PathProof]
proofPaths (FrameProof paths _ _ _ _ _) = paths
executionPcs :: FrameProof -> [CodeAddress]
executionPcs (FrameProof _ points _ _ _ _) = S.toAscList points
callBoundaries :: FrameProof -> [(CodeAddress,CallTarget)]
callBoundaries (FrameProof _ _ points _ _ _) = M.toAscList points
frameLow :: FrameProof -> StackOffset
frameLow _ = StackOffset (-16)
dispatcherBoundaries :: FrameProof -> [DispatcherBoundary]
dispatcherBoundaries (FrameProof _ _ _ entries _ _) = entries
effectSites :: FrameProof -> [EffectSite]
effectSites (FrameProof _ _ _ _ entries _) = entries
serviceEntryStates :: FrameProof -> [ServiceEntryState]
serviceEntryStates (FrameProof _ _ _ _ _ entries) = entries
entryScenario :: ServiceEntryState -> Scenario
entryScenario (ServiceEntryState scenario _ _ _) = scenario
entrySite :: ServiceEntryState -> CodeAddress
entrySite (ServiceEntryState _ site _ _) = site
entryService :: ServiceEntryState -> Service
entryService (ServiceEntryState _ _ service _) = service
-- | A register's symbolic value as the service is entered.
entryRegister :: ServiceEntryState -> A.Reg -> Scalar
entryRegister (ServiceEntryState _ _ _ values) register = M.findWithDefault (Incoming register) register values
effectScenario :: EffectSite -> Scenario
effectScenario (EffectSite scenario _ _) = scenario
effectKind :: EffectSite -> Effect
effectKind (EffectSite _ effect _) = effect
effectInstruction :: EffectSite -> CodeAddress
effectInstruction (EffectSite _ _ instruction) = instruction
boundarySide :: DispatcherBoundary -> LoaderSide
boundarySide (DispatcherBoundary value _ _ _ _ _ _ _) = value
boundaryScenario :: DispatcherBoundary -> Scenario
boundaryScenario (DispatcherBoundary _ value _ _ _ _ _ _) = value
boundaryPhase :: DispatcherBoundary -> Phase
boundaryPhase (DispatcherBoundary _ _ value _ _ _ _ _) = value
boundaryInstruction :: DispatcherBoundary -> CodeAddress
boundaryInstruction (DispatcherBoundary _ _ _ value _ _ _ _) = value
boundaryContinuation :: DispatcherBoundary -> CodeAddress
boundaryContinuation (DispatcherBoundary _ _ _ _ value _ _ _) = value
boundaryStack :: DispatcherBoundary -> StackOffset
boundaryStack (DispatcherBoundary _ _ _ _ _ value _ _) = value
boundaryFramePointer :: DispatcherBoundary -> StackOffset
boundaryFramePointer (DispatcherBoundary _ _ _ _ _ _ value _) = value
boundaryLiveWords :: DispatcherBoundary -> M.Map StackOffset Scalar
boundaryLiveWords (DispatcherBoundary _ _ _ _ _ _ _ value) = value

stackOffset :: Scalar -> Either AdmissionError StackOffset
stackOffset (Stack offset) = Right offset
stackOffset _ = Left FrameRejected

require :: Bool -> AdmissionError -> Either AdmissionError ()
require premise failure = unless premise (Left failure)
get :: Machine -> A.Reg -> Scalar
get machine register = M.findWithDefault (Incoming register) register (registers machine)
put :: A.Reg -> Scalar -> Machine -> Machine
put register value machine = machine {registers=M.insert register value (registers machine)}
known :: Scalar -> Either AdmissionError Word32
known (Word value) = Right value
known _ = Left OperandRejected

emit :: CodeAddress -> Effect -> Machine -> Machine
emit site effect machine =
  let observed = machine {locations=(effect,site):locations machine}
  in case (effect,effects machine) of
    (DelayNops (NopCount next),DelayNops (NopCount previous):rest) -> observed {effects=DelayNops (NopCount (previous+next)):rest}
    _ -> observed {effects=effect:effects machine}

condition :: Machine -> A.Cond -> Either AdmissionError Bool
condition _ (A.Cond 14) = Right True
condition machine (A.Cond 0) = maybe (Left ConditionRejected) Right (zero machine)
condition machine (A.Cond 1) = maybe (Left ConditionRejected) (Right . not) (zero machine)
condition _ _ = Left ConditionRejected

operand :: Machine -> A.Operand -> Either AdmissionError Scalar
operand _ (A.Immediate value) = Right (Word value)
operand machine (A.ShiftedImmediate register A.LSL 0) = Right (get machine register)
operand _ _ = Left OperandRejected

arithmetic :: A.DataOp -> Scalar -> Scalar -> Either AdmissionError Scalar
arithmetic A.MOV _ right = Right right
arithmetic A.MVN _ right = Word . complement <$> known right
arithmetic A.ADD (Stack (StackOffset offset)) (Word value) | value<=4096 = Right (Stack (StackOffset (offset+fromIntegral value)))
arithmetic A.SUB (Stack (StackOffset offset)) (Word value) | value<=4096 = Right (Stack (StackOffset (offset-fromIntegral value)))
arithmetic operation (Word left) (Word right) = Word <$> case operation of
  A.ADD -> Right (left+right); A.SUB -> Right (left-right)
  A.AND -> Right (left .&. right); A.ORR -> Right (left .|. right)
  A.BIC -> Right (left .&. complement right)
  _ -> Left OperandRejected
arithmetic _ _ _ = Left OperandRejected

compareZero :: Scalar -> Scalar -> Either AdmissionError Bool
compareZero (Word left) (Word right) = Right (left==right)
compareZero NonzeroWord (Word 0) = Right False
compareZero (Word 0) NonzeroWord = Right False
compareZero _ _ = Left ConditionRejected

savedFrame :: M.Map StackOffset Scalar
savedFrame = M.fromList [(StackOffset (-16),Incoming (A.Reg 4)),(StackOffset (-12),Incoming (A.Reg 5)),(StackOffset (-8),Incoming (A.Reg 11)),(StackOffset (-4),IncomingLink)]

requireFrame :: Machine -> Either AdmissionError ()
requireFrame machine = require (get machine A.sp==Stack (StackOffset (-16))&&get machine (A.Reg 11)==Stack (StackOffset (-4))&&stack machine==savedFrame) FrameRejected

instructionCondition :: A.Instruction -> Maybe A.Cond
instructionCondition instruction = case instruction of
  A.DataProcessing cond _ _ _ _ _ -> Just cond
  A.MoveWide cond _ _ -> Just cond; A.MoveTop cond _ _ -> Just cond
  A.Load cond _ _ _ _ _ _ -> Just cond; A.Store cond _ _ _ _ _ -> Just cond
  A.StoreMultiple cond _ _ _ _ -> Just cond
  A.Branch cond _ _ -> Just cond; A.BranchExchange cond _ _ -> Just cond
  A.Hint cond -> Just cond
  _ -> Nothing

programCounterDestination :: A.Instruction -> Bool
programCounterDestination instruction = case instruction of
  A.DataProcessing _ operation _ destination _ _ -> operation/=A.CMP && destination==A.pc
  A.MoveWide _ destination _ -> destination==A.pc
  A.MoveTop _ destination _ -> destination==A.pc
  A.Load _ _ _ destination _ _ _ -> destination==A.pc
  _ -> False

bodyUsesFrame :: A.Instruction -> Bool
bodyUsesFrame instruction = case instruction of
  A.DataProcessing _ operation _ destination left right -> frame destination || (operation `notElem` [A.MOV,A.MVN] && frame left) || operandFrame right
  A.MoveWide _ destination _ -> frame destination
  A.MoveTop _ destination _ -> frame destination
  A.Load _ _ _ destination base offset _ -> frame destination || frame base || offsetFrame offset
  A.Store _ _ source base offset _ -> frame source || frame base || offsetFrame offset
  A.StoreMultiple {} -> True
  A.BranchExchange _ _ register -> frame register
  _ -> False
  where
    frame register = register==A.sp || register==A.Reg 11
    operandFrame (A.Immediate _) = False
    operandFrame (A.ShiftedImmediate register _ _) = frame register
    operandFrame (A.ShiftedRegister left _ right) = frame left || frame right
    offsetFrame (A.OffsetImmediate _) = False
    offsetFrame (A.OffsetRegister _ register _ _) = frame register

callReturning :: CodeAddress -> Scalar -> Machine -> Machine
callReturning continuation result machine =
  let CallOrdinal ordinal = calls machine
      next = CallOrdinal (ordinal+1)
      values = foldr (\register -> M.insert register (Returned next register)) (registers machine) (map A.Reg [0,1,2,3,12])
  in machine {registers=M.insert A.lr (case continuation of CodeAddress pc -> Word pc) (M.insert (A.Reg 0) result values),calls=next,zero=Nothing}

callService :: Scenario -> CodeAddress -> CodeAddress -> Service -> Machine -> Either AdmissionError Machine
callService (Scenario role _ _) site continuation service machine = do
  requireFrame machine
  require (service/=Schedule || get machine (A.Reg 0)==Word 0xfffffff6) EffectRejected
  let arguments = [(register,get machine register) | register <- liveRegisters service]
      captured = (emit site (ServiceCall service arguments) machine {boundaries=M.insert site (Retained service) (boundaries machine)})
        {serviceEntries=(site,service,M.insert A.lr (case continuation of CodeAddress pc -> Word pc) (registers machine)):serviceEntries machine}
      CallOrdinal ordinal = calls machine
      result = case service of
        Identity -> case role of Primary -> Word 0; Peer -> NonzeroWord
        PublishCallback -> get machine (A.Reg 0)
        _ -> Returned (CallOrdinal (ordinal+1)) (A.Reg 0)
  pure $ if service==Schedule then captured {terminal=Just EnteredScheduler} else callReturning continuation result captured

phase :: Scalar -> Either AdmissionError Phase
phase (Word value) = case value of 0 -> Right Setup; 1 -> Right Root; 2 -> Right Diagnostic; 3 -> Right Idle0; 4 -> Right PeerTasks; _ -> Left EffectRejected
phase _ = Left EffectRejected

callDispatcher :: LoaderSide -> Scenario -> CodeAddress -> CodeAddress -> Machine -> Either AdmissionError Machine
callDispatcher side scenario@(Scenario _ _ outcome) site continuation machine = do
  requireFrame machine
  selected <- phase (get machine (A.Reg 0))
  incomingStack <- stackOffset (get machine A.sp)
  incomingFramePointer <- stackOffset (get machine (A.Reg 11))
  let boundary = DispatcherBoundary side scenario selected site continuation incomingStack incomingFramePointer (stack machine)
      captured = emit site (DispatchCall selected) machine {boundaries=M.insert site Dispatcher (boundaries machine),dispatcherEdges=boundary:dispatcherEdges machine}
      CallOrdinal ordinal = calls machine
      result = if selected==Diagnostic then CallbackCapability else Returned (CallOrdinal (ordinal+1)) (A.Reg 0)
  pure $ if selected==Setup && outcome==SetupNonreturn then captured {terminal=Just SetupDidNotReturn} else callReturning continuation result captured

expected :: Scenario -> ([Effect],Terminal)
expected (Scenario role completion setup) =
  let call service = ServiceCall service []
      logMode value = ServiceCall Log [(A.Reg 0,Word value)]
      identity = [call Identity]
      waiting = case completion of
        InitiallyPublished -> [CompletionRead Published]
        PublishedAfterDelay -> [CompletionRead Pending,DelayNops (NopCount 100),CompletionRead Published]
        NeverPublished -> [CompletionRead Pending,DelayNops (NopCount 100),CompletionRead Pending,DelayNops (NopCount 100),CompletionRead Pending]
      before = identity ++ case role of
        Primary -> [ServiceCall ClearShared [(A.Reg 0,Word 0x400a0800),(A.Reg 1,Word 0x20100)]]
        Peer -> waiting
      prepare = map call [PrivateInitialize] ++ [logMode 0] ++ map call [Prepare1,Prepare2,Prepare3,Prepare4]
      selected = case role of
        Primary -> [DispatchCall Setup,CompletionPublication,StoreBarrier,call PrimaryContinue]
        Peer -> [call PeerContinue]
      continue = map call [Continue1,Continue2,Continue3,Continue4,Continue5,Continue6,Continue7]
      tasks = case role of Primary -> [DispatchCall Root,DispatchCall Idle0]; Peer -> [DispatchCall PeerTasks]
      after = [DispatchCall Diagnostic,ServiceCall PublishCallback [(A.Reg 0,CallbackCapability)],call AutoReady,call ActivateTimers,logMode 1,ReadyPublication,call Schedule]
  in if role==Peer && completion==NeverPublished then (before,WaitingForCompletion)
    else if role==Primary && setup==SetupNonreturn then (before++prepare++[DispatchCall Setup],SetupDidNotReturn)
    else (before++prepare++selected++continue++tasks++after,EnteredScheduler)

deriveFrameProof :: LoaderSide -> B.ByteString -> CodeAddress -> Either AdmissionError FrameProof
deriveFrameProof side bytes (CodeAddress compilerPc) = do
  let start@(CodeAddress first) = carrierStart side
      end = first+fromIntegral (B.length bytes)
      word at = U.word32At bytes (fromIntegral (at-first))
  require (not (B.null bytes)&&B.length bytes<=340&&B.length bytes `mod` 4==0) ExtentRejected
  require (compilerPc==first+12&&compilerPc+8<=end&&word compilerPc==0xe92d4800&&word (compilerPc+4)==0xe1a0b00d) CompilerPrologueRejected
  results <- traverse (run start end) scenarios
  pure (FrameProof (map resultPath results) (S.unions (map resultPcs results)) (M.unions (map resultCalls results))
    (concatMap resultDispatcherCalls results) (concatMap resultLocations results) (concatMap resultServiceEntries results))
  where
    targets = serviceTable side
    run start end scenario = do
      let initial = Machine (M.fromList [(A.Reg 0,DispatchCapability),(A.sp,Stack (StackOffset 0)),(A.lr,IncomingLink)]) M.empty Nothing [] S.empty M.empty [] (PollCount 0) Nothing (CallOrdinal 0) (StepCount 0) Nothing [] []
      final <- execute start initial
      requireFrame final
      stopped <- maybe (Left EffectRejected) Right (terminal final)
      require (case scenario of Scenario Peer NeverPublished _ -> stopped==WaitingForCompletion; _ -> True) PollingInvariantRejected
      let observed = reverse (effects final)
      require ((observed,stopped)==expected scenario) EffectRejected
      pure (PathResult (PathProof scenario observed stopped) (visited final) (boundaries final) (reverse (dispatcherEdges final))
        [EffectSite scenario effect site | (effect,site) <- reverse (locations final)]
        [ServiceEntryState scenario site service values | (site,service,values) <- reverse (serviceEntries final)])
      where
        CodeAddress first = start
        readLiteral at = do
          require (at>=first&&toInteger at+4<=toInteger end&&at .&. 3==0) MemoryRejected
          pure (Word (U.word32At bytes (fromIntegral (at-first))))
        registerValue pc machine register = if register==A.pc then Word (pc+8) else get machine register
        address pc machine base (A.OffsetImmediate offset) = do
          pointer <- known (registerValue pc machine base)
          pure (pointer+fromIntegral offset)
        address _ _ _ _ = Left MemoryRejected
        completionRead site destination machine = do
          let Scenario _ source _ = scenario
              PollCount count = polls machine
              value = case source of InitiallyPublished -> NonzeroWord; PublishedAfterDelay | count>0 -> NonzeroWord; _ -> Word 0
              state = if value==Word 0 then Pending else Published
              next = put destination value (emit site (CompletionRead state) machine {polls=PollCount (count+1)})
              recurrent = PollCheckpoint site (registers next) (stack next) (zero next) (calls next)
          if source/=NeverPublished || count==0 then pure next
            else if count==1 then pure next {checkpoint=Just recurrent}
            else do
              require (checkpoint next==Just recurrent) PollingInvariantRejected
              pure next {terminal=Just WaitingForCompletion}
        execute site@(CodeAddress pc) machine = case terminal machine of
          Just _ -> Right machine
          Nothing -> do
            let StepCount count = steps machine
            require (count<4000) StepLimit
            require (pc>=first&&toInteger pc+4<=toInteger end&&pc .&. 3==0&&not (compilerPc<=pc&&pc<compilerPc+8)) (UnsupportedInstruction site)
            let raw = U.word32At bytes (fromIntegral (pc-first))
                decoded = A.decode pc raw
                next = CodeAddress (pc+4)
                marked = machine {steps=StepCount (count+1),visited=S.insert site (visited machine)}
            require (pc<compilerPc+8 || not (bodyUsesFrame decoded)) FrameRejected
            require (not (programCounterDestination decoded)) (ControlFlowRejected site)
            if raw==0xf57ff04f then execute next (emit site StoreBarrier marked) else do
              cond <- maybe (Left (UnsupportedInstruction site)) Right (instructionCondition decoded)
              active <- condition marked cond
              if not active then execute next marked else case decoded of
                A.DataProcessing _ A.CMP True _ left right -> do
                  rightValue <- operand marked right
                  equal <- compareZero (get marked left) rightValue
                  execute next marked {zero=Just equal}
                A.DataProcessing _ operation False destination left right -> do
                  rightValue <- operand marked right
                  value <- arithmetic operation (get marked left) rightValue
                  execute next (put destination value marked)
                A.MoveWide _ destination value -> execute next (put destination (Word value) marked)
                A.MoveTop _ destination value -> do
                  previous <- known (get marked destination)
                  execute next (put destination (Word ((previous .&. 0xffff) .|. (value `shiftL` 16))) marked)
                A.StoreMultiple _ base A.DecrementBefore True savedRegisters -> do
                  require (site==start&&base==A.sp&&savedRegisters==map A.Reg [4,5,11,14]&&get marked A.sp==Stack (StackOffset 0)) FrameRejected
                  let saved = M.fromList [(StackOffset offset,get marked register) | (offset,register) <- zip [-16,-12,-8,-4] savedRegisters]
                  execute next (put A.sp (Stack (StackOffset (-16))) marked {stack=saved})
                A.Load _ A.Word False destination base offset A.OffsetOnly -> do
                  at <- address pc marked base offset
                  loaded <- if at==0x400a00c4 then completionRead site destination marked else do literal <- readLiteral at;pure (put destination literal marked)
                  execute next loaded
                A.Store _ A.Word source base offset A.OffsetOnly -> do
                  at <- address pc marked base offset
                  require (get marked source==Word 1) MemoryRejected
                  effect <- if at==0x400a00c4 then Right CompletionPublication else if at==readyAddress side then Right ReadyPublication else Left MemoryRejected
                  execute next (emit site effect marked)
                A.Branch _ False target -> execute (CodeAddress target) marked
                A.Branch _ True target -> do
                  service <- maybe (Left (CallRejected (CodeAddress target))) Right (M.lookup (CodeAddress target) targets)
                  called <- callService scenario site next service marked
                  execute next called
                A.BranchExchange _ True register -> do
                  require (get marked register==DispatchCapability) (CallRejected site)
                  called <- callDispatcher side scenario site next marked
                  execute next called
                A.Hint _ | raw==0xe320f000 -> execute next (emit site (DelayNops (NopCount 1)) marked)
                _ -> Left (UnsupportedInstruction site)
