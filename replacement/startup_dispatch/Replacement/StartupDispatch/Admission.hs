module Replacement.StartupDispatch.Admission
  ( AdmissionError (..), FrameProof, Phase (..), PathProof (..), Effect (..)
  , deriveFrameProof, proofPaths, scratchLow
  , Descriptor (..), TaskId (..), NameAddress (..), Priority (..), StackBytes (..), EntryAddress (..), Argument (..)
  ) where

import qualified A32 as A
import Control.Monad (foldM, unless)
import Data.Bits (complement, shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U

data AdmissionError = CarrierRejected | AdmissionError String deriving (Eq, Show)
data Phase = Setup | Root | Diagnostic | Idle0 | Idle1AndStandby | Other deriving (Eq, Show)
newtype TaskId = TaskId Word32 deriving (Eq, Show)
newtype NameAddress = NameAddress Word32 deriving (Eq, Show)
newtype Priority = Priority Word32 deriving (Eq, Show)
newtype StackBytes = StackBytes Word32 deriving (Eq, Show)
newtype EntryAddress = EntryAddress Word32 deriving (Eq, Show)
newtype Argument = Argument Word32 deriving (Eq, Show)
data Descriptor = Descriptor TaskId NameAddress Priority StackBytes EntryAddress Argument deriving (Eq, Show)
data Effect = ModeRead | ClearWords | Install | Submit Descriptor deriving (Eq, Show)
data PathProof = PathProof Phase [Effect] Bool deriving (Eq, Show)
data FrameProof = FrameProof Int [PathProof]

data Scalar = Constant Word32 | Interval Word32 Word32 | Stack Int | Incoming A.Reg | ReturnAddress | Unknown deriving (Eq, Show)
data Flags = Flags (Maybe Bool) (Maybe Bool) (Maybe Bool) (Maybe Bool)
data Machine = Machine
  { registers :: M.Map A.Reg Scalar, memory :: M.Map Int Scalar
  , flags :: Flags, effects :: [Effect], pending :: [(Int, Word32)]
  , low :: Int, steps :: Int
  }

proofPaths :: FrameProof -> [PathProof]
proofPaths (FrameProof _ paths) = paths

scratchLow :: FrameProof -> Int
scratchLow (FrameProof bottom _) = bottom

descriptor :: [Word32] -> Either AdmissionError Descriptor
descriptor [task,name,priority,stack,code,argument] =
  Right (Descriptor (TaskId task) (NameAddress name) (Priority priority) (StackBytes stack) (EntryAddress code) (Argument argument))
descriptor _ = Left (AdmissionError "A32 descriptor does not contain six assigned words")

require :: Bool -> String -> Either AdmissionError ()
require premise message = unless premise (Left (AdmissionError message))

known :: Scalar -> Either AdmissionError Word32
known (Constant word) = Right word
known _ = Left (AdmissionError "A32 operand is not a proven constant")

get :: Machine -> A.Reg -> Scalar
get machine register = M.findWithDefault Unknown register (registers machine)

put :: A.Reg -> Scalar -> Machine -> Machine
put register value machine = machine
  { registers = M.insert register value (registers machine)
  , low = case (register,value) of
      (A.Reg 13,Stack offset) -> min (low machine) offset
      _ -> low machine
  }

condition :: Flags -> A.Cond -> Either AdmissionError Bool
condition (Flags n z c v) (A.Cond code) = case code of
  14 -> Right True
  0 -> need z
  1 -> not <$> need z
  2 -> need c
  3 -> not <$> need c
  8 -> (&&) <$> need c <*> (not <$> need z)
  9 -> (||) <$> (not <$> need c) <*> need z
  10 -> (==) <$> need n <*> need v
  11 -> (/=) <$> need n <*> need v
  12 -> (&&) <$> (not <$> need z) <*> ((==) <$> need n <*> need v)
  13 -> (||) <$> need z <*> ((/=) <$> need n <*> need v)
  _ -> Left (AdmissionError "A32 condition is outside admission")
  where need = maybe (Left (AdmissionError "A32 condition depends on an unproved flag")) Right

bounds :: Scalar -> Maybe (Word32, Word32)
bounds (Constant word) = Just (word, word)
bounds (Interval first lastWord) = Just (first, lastWord)
bounds _ = Nothing

compareFlags :: Scalar -> Scalar -> Either AdmissionError Flags
compareFlags left right = case (bounds left, bounds right) of
  (Just (a,b), Just (c,d)) | a == b && c == d ->
    let result = a - c
        negative word = word .&. 0x80000000 /= 0
    in Right (Flags (Just (negative result)) (Just (a == c)) (Just (a >= c))
      (Just (negative a /= negative c && negative result /= negative a)))
  (Just (a,b), Just (c,d)) | b < c || a > d ->
    Right (Flags Nothing (Just False) (Just (a > d)) Nothing)
  _ -> Left (AdmissionError "A32 comparison does not separate the entire input class")

operand :: Machine -> A.Operand -> Either AdmissionError Scalar
operand _ (A.Immediate word) = Right (Constant word)
operand machine (A.ShiftedImmediate register _ 0) = Right (get machine register)
operand machine (A.ShiftedImmediate register shift amount) | amount > 0 && amount < 32 = do
  word <- known (get machine register)
  case shift of
    A.LSL -> Right (Constant (word `shiftL` amount))
    A.LSR -> Right (Constant (word `shiftR` amount))
    _ -> Left (AdmissionError "A32 shift is outside admission")
operand _ _ = Left (AdmissionError "A32 register shift is outside admission")

arithmetic :: A.DataOp -> Scalar -> Scalar -> Either AdmissionError Scalar
arithmetic A.MOV _ right = Right right
arithmetic A.ADD (Stack offset) (Constant word) = Right (Stack (offset + fromIntegral word))
arithmetic A.SUB (Stack offset) (Constant word) = Right (Stack (offset - fromIntegral word))
arithmetic operation (Constant left) (Constant right) = Constant <$> case operation of
  A.ADD -> Right (left + right)
  A.SUB -> Right (left - right)
  A.RSB -> Right (right - left)
  A.AND -> Right (left .&. right)
  A.ORR -> Right (left .|. right)
  A.BIC -> Right (left .&. complement right)
  _ -> Left (AdmissionError "A32 arithmetic is outside admission")
arithmetic A.SUB (Interval first lastWord) (Constant word) | first >= word = Right (Interval (first-word) (lastWord-word))
arithmetic _ _ _ = Left (AdmissionError "A32 arithmetic loses a symbolic premise")

writeStack :: Int -> Scalar -> Machine -> Either AdmissionError Machine
writeStack address value machine = do
  require (address .&. 3 == 0 && address < 0 && address /= -20 && address >= -4096)
    "A32 store touches the unknown word or leaves the admitted stack"
  require (address <= -48 || address `elem` [-44,-40,-36,-32,-28,-24,-16,-12,-8,-4])
    "A32 store touches an unowned frame word"
  writes <- if address `elem` [-44,-40,-36,-32,-28,-24] then do
    word <- known value
    pure (pending machine ++ [(address,word)])
    else pure (pending machine)
  pure machine {memory = M.insert address value (memory machine), pending = writes, low = min (low machine) address}

readStack :: Int -> Machine -> Either AdmissionError Scalar
readStack address machine = maybe (Left (AdmissionError "A32 reads an uninitialized or alias-visible frame word")) Right (M.lookup address (memory machine))

entry :: Word32
entry = 0x69cdf8

deriveFrameProof :: B.ByteString -> Either AdmissionError FrameProof
deriveFrameProof bytes = do
  unless (not (B.null bytes) && B.length bytes <= 396 && B.length bytes `mod` 4 == 0) (Left CarrierRejected)
  paths <- sequence [run phase mode status | phase <- [Setup,Root,Diagnostic,Idle0,Idle1AndStandby,Other]
    , mode <- if phase == Setup then [Constant 0,Constant 1,Interval 2 maxBound] else [Unknown]
    , status <- if phase == Setup then [Constant 0,Interval 1 maxBound] else [Unknown]]
  pure (FrameProof (minimum (map fst paths)) (map snd paths))
  where
    instruction pc = do
      require (pc >= entry && toInteger pc + 4 <= toInteger entry + toInteger (B.length bytes) && pc .&. 3 == 0)
        "A32 execution leaves its linked carrier"
      pure (A.decode pc (U.word32At bytes (fromIntegral (pc-entry))))
    run phase mode status = do
      let input = case phase of
            Setup -> Constant 0; Root -> Constant 1; Diagnostic -> Constant 2
            Idle0 -> Constant 3; Idle1AndStandby -> Constant 4; Other -> Interval 5 maxBound
          initial = Machine (M.fromList ([(A.Reg n,Incoming (A.Reg n)) | n <- [0..12]] ++ [(A.sp,Stack 0),(A.lr,ReturnAddress)]))
            M.empty (Flags Nothing Nothing Nothing Nothing) [] [] 0 0
          expected = case phase of
            Setup -> [ModeRead] ++ [ClearWords | mode == Constant 1] ++ [Install]
            Root -> [Submit (Descriptor (TaskId 104) (NameAddress 0x781960) (Priority 1) (StackBytes 0x400) (EntryAddress 0x69d160) (Argument 0))]
            Diagnostic -> []
            Idle0 -> [Submit (Descriptor (TaskId 102) (NameAddress 0x781968) (Priority 15) (StackBytes 0x400) (EntryAddress 0x69cbf0) (Argument 0))]
            Idle1AndStandby ->
              [ Submit (Descriptor (TaskId 103) (NameAddress 0x781970) (Priority 15) (StackBytes 0x400) (EntryAddress 0x69cbf0) (Argument 0))
              , Submit (Descriptor (TaskId 93) (NameAddress 0x781978) (Priority 13) (StackBytes 0x400) (EntryAddress 0x69cd58) (Argument 0))
              ]
            Other -> []
          finish spins machine = do
            require (effects machine == expected && null (pending machine)) "A32 ordered effects differ from startup dispatch"
            require (spins == (phase == Setup && status /= Constant 0)) "A32 setup return/spin outcome differs"
            unless spins $ do
              require (get machine A.sp == Stack 0) "A32 return changes incoming SP"
              require (all (\n -> get machine (A.Reg n) == Incoming (A.Reg n)) [4..11]) "A32 return changes a caller-live register"
              require (get machine (A.Reg 0) == Constant (if phase == Diagnostic then 0x69cc6c else 0)) "A32 dispatch return differs"
            pure (low machine,PathProof phase (effects machine) spins)
          retained target machine = do
            require (case get machine A.sp of Stack offset -> offset <= -48; _ -> False) "A32 retained call overlaps the guest frame"
            require (target == 0x725dd8 || null (pending machine)) "A32 descriptor writes precede an unrelated service"
            (effect, result) <- case target of
              0x657184 -> pure (ModeRead,mode)
              0x7221a0 -> do
                require (map (get machine . A.Reg) [0,1,2] == map Constant [0x89ec00,0,0x352e00 `div` 4]) "A32 clear interface differs"
                pure (ClearWords,Unknown)
              0x725a04 -> pure (Install,status)
              0x725dd8 -> do
                require (get machine (A.Reg 0) == Stack (-44)) "A32 descriptor moved from incoming SP - 44"
                require (map fst (pending machine) == [-44,-40,-36,-32,-28,-24]) "A32 descriptor stores are missing, repeated or reordered"
                submitted <- descriptor (map snd (pending machine))
                pure (Submit submitted,Unknown)
              _ -> Left (AdmissionError "A32 external call is not a retained startup service")
            let clobbered = foldr (\register -> put register Unknown) machine (map A.Reg [0,1,2,3,12] ++ [A.lr])
            pure ((put (A.Reg 0) result clobbered)
              { effects = effects machine ++ [effect], pending = [], flags = Flags Nothing Nothing Nothing Nothing
              , memory = if target == 0x725dd8 then foldr M.delete (memory machine) [-44,-40,-36,-32,-28,-24] else memory machine
              })
          walk pc machine = do
            require (steps machine < 1024) "A32 path exceeds the finite admission bound"
            decoded <- instruction pc
            let advanced = machine {steps = steps machine + 1}
                next = walk (pc+4)
                assign register value = if register == A.pc then transfer value advanced else next (put register value advanced)
                execute cond action = condition (flags machine) cond >>= \taken -> if taken then action else next advanced
                transfer ReturnAddress returned = finish False returned
                transfer (Constant target) changed = walk target changed
                transfer _ _ = Left (AdmissionError "A32 indirect transfer has unknown provenance")
                offset base displacement = case (get machine base,displacement) of
                  (Stack root,A.OffsetImmediate delta) -> Right (root + fromIntegral delta)
                  _ -> Left (AdmissionError "A32 stack addressing is not proved")
            case decoded of
              A.MoveWide cond destination word -> execute cond (assign destination (Constant word))
              A.MoveTop cond destination word -> execute cond $ do
                previous <- known (get machine destination)
                assign destination (Constant ((previous .&. 0xffff) .|. (word `shiftL` 16)))
              A.DataProcessing cond operation updates destination source value -> execute cond $ do
                right <- operand machine value
                if operation == A.CMP && updates then do
                  compared <- compareFlags (get machine source) right
                  next advanced {flags = compared}
                else do
                  require (not updates) "A32 arithmetic flag update is not admitted"
                  result <- arithmetic operation (get machine source) right
                  assign destination result
              A.Store cond A.Word source base displacement indexing -> execute cond $ do
                address <- offset base displacement
                let actual = if indexing == A.PostIndexed then case get machine base of Stack root -> root; _ -> address else address
                changed <- writeStack actual (get machine source) advanced
                next (if indexing == A.OffsetOnly then changed else put base (Stack address) changed)
              A.Load cond A.Word False destination base displacement indexing -> execute cond $ do
                if base == A.pc && indexing == A.OffsetOnly then case displacement of
                  A.OffsetImmediate delta -> do
                    let address = toInteger pc + 8 + toInteger delta
                    require (address >= toInteger entry && address + 4 <= toInteger entry + toInteger (B.length bytes) && address `mod` 4 == 0) "A32 literal leaves carrier"
                    assign destination (Constant (U.word32At bytes (fromInteger (address-toInteger entry))))
                  _ -> Left (AdmissionError "A32 literal address is not immediate")
                else do
                  address <- offset base displacement
                  let actual = if indexing == A.PostIndexed then case get machine base of Stack root -> root; _ -> address else address
                  value <- readStack actual machine
                  let changed = if indexing == A.OffsetOnly then advanced else put base (Stack address) advanced
                  if destination == A.pc then transfer value changed else next (put destination value changed)
              A.StoreMultiple cond base A.DecrementBefore True saved -> execute cond $ do
                root <- case get machine base of Stack address -> Right address; _ -> Left (AdmissionError "A32 push has unknown base")
                require (base == A.sp && not (null saved)) "A32 block store is not a stack save"
                let bottom = root - 4 * length saved
                changed <- foldM (\state (address,register) -> writeStack address (get machine register) state) advanced (zip [bottom,bottom+4..] saved)
                next (put A.sp (Stack bottom) changed)
              A.LoadMultiple cond base A.IncrementAfter True restored -> execute cond $ do
                root <- case get machine base of Stack address -> Right address; _ -> Left (AdmissionError "A32 pop has unknown base")
                require (base == A.sp && not (null restored)) "A32 block load is not a stack restore"
                values <- mapM (`readStack` machine) (take (length restored) [root,root+4..])
                let changed = foldr (uncurry put) (put A.sp (Stack (root+4*length restored)) advanced) (zip restored values)
                if A.pc `elem` restored then transfer (get changed A.pc) changed else next changed
              A.Branch cond link target -> execute cond $ do
                if link then do
                  let called = put A.lr (Constant (pc+4)) advanced
                  if target >= entry && target < entry + fromIntegral (B.length bytes) then walk target called
                  else retained target called >>= next
                else if target == pc then finish True advanced
                else if target >= entry && target < entry + fromIntegral (B.length bytes) then walk target advanced
                else retained target advanced >>= transfer (get advanced A.lr)
              A.BranchExchange cond link register -> execute cond $ do
                require (not link) "A32 computed call is not admitted"
                transfer (get machine register) advanced
              A.Hint cond -> execute cond (next advanced)
              _ -> Left (AdmissionError ("A32 instruction is outside startup admission: " ++ show decoded))
      walk entry (put (A.Reg 0) input initial) >>= \(bottom,path) -> do
        require (bottom <= -48) "A32 adapter does not establish the original guest frame"
        pure (bottom,path)
