{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- Generates the owned controller-75 table (`modal75.rs` Table75 expression)
-- from owner-supplied image5 bytes and the reviewed inventory. Every generic
-- action is derived and then checked by concrete execution of its stock body.
module Main where

import qualified A32 as A
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (FromJSON (..), (.:))
import qualified Data.Aeson as J
import Data.Aeson.Types (Parser)
import Data.Bits (complement, shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Int (Int16, Int32)
import Data.List (intercalate, nub, sort)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word16, Word32)
import Emulation.Sha256 (sha256, sha256Hex)
import Numeric (readHex, showHex)
import System.Directory (doesPathExist)
import System.Environment (getArgs)

newtype Callback = Callback Word32 deriving (Eq, Ord)

instance Show Callback where
  show (Callback entry) = "callback " ++ hex entry
newtype Event = Event Word16 deriving (Eq, Ord, Show)
newtype State = State Word16 deriving (Eq, Ord, Show)

data Generic = Route [(Word32, State)] | Constant Word32 deriving (Eq, Show)
data CellAction = Retained | GenericAt Int deriving (Eq, Show)
data After = Keep | SetState State deriving (Eq, Show)
data OwnedCell = OwnedCell
  { cellEvent :: Event
  , cellState :: State
  , cellWord :: Word32
  , cellTarget :: Word32
  , cellAction :: CellAction
  , cellAfter :: After
  } deriving (Eq, Show)

data Control = NoControl | WrongNextState | WrongActionResult | WrongRouteEvent deriving (Eq, Show)

data InventoryCallback = InventoryCallback Callback Int String [(State, Event)]
data DirectCell = DirectCell State Event
data Inventory = Inventory [InventoryCallback] [DirectCell]

parseHex :: String -> Parser Word32
parseHex text = case text of
  '0' : 'x' : digits | length digits == 8, [(value, "")] <- readHex digits -> pure value
  _ -> fail ("eight-digit hex required: " ++ text)

instance FromJSON Inventory where
  parseJSON = J.withObject "inventory" $ \fields -> do
    callbacks <- mapM (J.withObject "callback" (\c -> do
      entry <- parseHex =<< c .: "callback"
      size <- c .: "code_bytes"
      layer <- c .: "layer"
      cells <- c .: "cells"
      pure (InventoryCallback (Callback entry) size layer [(State s, Event e) | [s, e] <- cells]))) =<< fields .: "callbacks"
    controller <- fields .: "controller"
    direct <- J.withObject "controller" (\c -> mapM (J.withObject "direct state cell" (\d -> DirectCell <$> (State <$> d .: "state") <*> (Event <$> d .: "event"))) =<< c .: "direct_state_cells") controller
    pure (Inventory callbacks direct)

imageBase, transitionRouter, stateCell75 :: Word32
imageBase = 0xbf1a00
transitionRouter = 0xc12aac
stateCell75 = 0x1467588 + 150

hex :: Word32 -> String
hex value = "0x" ++ replicate (8 - length digits) '0' ++ digits where digits = showHex value ""

wordAt :: B.ByteString -> Word32 -> Either String Word32
wordAt bytes address
  | address < imageBase || toInteger address + 4 > toInteger imageBase + toInteger (B.length bytes) = Left ("image5 read outside input at " ++ hex address)
  | otherwise = Right (foldr (\index value -> (value `shiftL` 8) .|. fromIntegral (B.index bytes (fromIntegral (address - imageBase) + index))) 0 [0 .. 3])

halfAt :: B.ByteString -> Word32 -> Either String Word16
halfAt bytes address = (\word -> fromIntegral (word `shiftR` fromIntegral (8 * (address .&. 2)))) <$> wordAt bytes (address .&. 0xfffffffc)

-- * Concrete execution of a stock callback body with the transition router stubbed

data Flags = Flags Bool Bool Bool Bool
data Machine = Machine
  { machinePc :: Word32
  , machineRegisters :: M.Map Int Word32
  , machineFlags :: Maybe Flags
  , machineStack :: M.Map Word32 Word32
  , machineStores :: [(Word32, A.Width, Word32)]
  , machineRouterCalls :: Int
  , machineConditions :: S.Set Word32
  , machineRouterIntact :: Bool
  }

data Outcome = Outcome {outcomeResult :: Word32, outcomeRouterCalls :: Int, outcomeStates :: [Word16], outcomeConditions :: S.Set Word32}

stackTop, returnSentinel :: Word32
stackTop = 0x80000000
returnSentinel = 0xdead0000

initialRegister :: Int -> Word32
initialRegister n = 0xa0000000 + fromIntegral n

holds :: Flags -> Word32 -> Bool
holds (Flags n z c v) condition = case condition of
  0 -> z
  1 -> not z
  2 -> c
  3 -> not c
  4 -> n
  5 -> not n
  6 -> v
  7 -> not v
  8 -> c && not z
  9 -> not c || z
  10 -> n == v
  11 -> n /= v
  12 -> not z && n == v
  13 -> z || n /= v
  _ -> True

compareFlags :: Word32 -> Word32 -> Flags
compareFlags a b =
  let result = a - b
      signed x = fromIntegral x :: Int32
   in Flags (signed result < 0) (result == 0) (a >= b) (toInteger (signed a) - toInteger (signed b) /= toInteger (signed result))

addFlags :: Word32 -> Word32 -> Flags
addFlags a b =
  let result = a + b
      signed x = fromIntegral x :: Int32
   in Flags (signed result < 0) (result == 0) (toInteger a + toInteger b >= 2 ^ (32 :: Int)) (toInteger (signed a) + toInteger (signed b) /= toInteger (signed result))

execute :: B.ByteString -> Callback -> Int -> Word32 -> Either String Outcome
execute bytes (Callback entry) size routerValue = go (1000 :: Int) start
 where
  start = Machine entry (M.fromList ([(n, initialRegister n) | n <- [0 .. 12]] ++ [(13, stackTop), (14, returnSentinel)])) Nothing M.empty [] 0 S.empty False
  limit = entry + fromIntegral size
  refuse machine reason = Left (hex entry ++ " at " ++ hex (machinePc machine) ++ ": " ++ reason)
  reg machine (A.Reg n) = if n == 15 then machinePc machine + 8 else M.findWithDefault 0 n (machineRegisters machine)
  setReg (A.Reg n) value machine = machine {machineRegisters = M.insert n value (machineRegisters machine), machineRouterIntact = machineRouterIntact machine && n /= 0}
  next machine = machine {machinePc = machinePc machine + 4}
  finish machine target
    | target /= returnSentinel = refuse machine "return to a value other than the incoming LR"
    | reg machine A.sp /= stackTop = refuse machine "stack pointer not restored"
    | any (\n -> reg machine (A.Reg n) /= initialRegister n) [4 .. 11] = refuse machine "callee-saved register not restored"
    | otherwise = Right (Outcome (reg machine (A.Reg 0)) (machineRouterCalls machine) [fromIntegral value | (address, A.Half, value) <- reverse (machineStores machine), address == stateCell75] (machineConditions machine))
  go 0 machine = refuse machine "step limit"
  go fuel machine
    | machinePc machine < entry || machinePc machine >= limit = refuse machine "left the callback extent"
    | otherwise = do
        word <- wordAt bytes (machinePc machine)
        let instruction = A.decode (machinePc machine) word
            A.Cond condition = conditionOf instruction
        taken <- if condition == 14 then Right True else case machineFlags machine of
          Nothing -> refuse machine "condition on unknown flags"
          Just flags -> Right (holds flags condition)
        let noted = if condition /= 14 && machineRouterCalls machine > 0 then machine {machineConditions = S.insert condition (machineConditions machine)} else machine
        routed <- if machineRouterCalls machine == 0 || A.Reg 0 `notElem` registersRead instruction then Right noted else case instruction of
          A.DataProcessing _ op _ _ source (A.Immediate _) | op `elem` [A.CMP, A.CMN], source == A.Reg 0, machineRouterIntact machine -> Right noted
          _ -> refuse machine "router result read other than by cmp/cmn r0, #imm"
        if not taken then go (fuel - 1) (next routed) else step fuel routed instruction
  step fuel machine instruction = case instruction of
    A.DataProcessing _ op setsFlags destination source operand -> do
      value <- case operand of
        A.Immediate immediate -> Right immediate
        A.ShiftedImmediate register A.LSL 0 -> Right (reg machine register)
        _ -> refuse machine "unsupported operand"
      let left = reg machine source
      case op of
        A.CMP -> go (fuel - 1) (next machine {machineFlags = Just (compareFlags left value)})
        A.CMN -> go (fuel - 1) (next machine {machineFlags = Just (addFlags left value)})
        _ -> do
          when setsFlags (refuse machine "flag-setting data processing")
          result <- case op of
            A.MOV -> Right value
            A.MVN -> Right (complement value)
            A.ADD -> Right (left + value)
            A.SUB -> Right (left - value)
            _ -> refuse machine "unsupported operation"
          if destination == A.pc then finish machine result else go (fuel - 1) (next (setReg destination result machine))
    A.MoveWide _ destination value -> go (fuel - 1) (next (setReg destination value machine))
    A.MoveTop _ destination value -> go (fuel - 1) (next (setReg destination ((value `shiftL` 16) .|. (reg machine destination .&. 0xffff)) machine))
    A.Store _ width source base (A.OffsetImmediate delta) indexing -> do
      let baseValue = reg machine base
          address = if indexing == A.PostIndexed then baseValue else baseValue + fromIntegral delta
          written = if indexing == A.OffsetOnly then machine else setReg base (baseValue + fromIntegral delta) machine
          value = reg machine source
      if base == A.sp
        then do
          unless (width == A.Word && address < stackTop) (refuse machine "stack store other than a word below the entry SP")
          go (fuel - 1) (next written {machineStack = M.insert address value (machineStack written)})
        else do
          unless (width == A.Half && address == stateCell75) (refuse machine "store other than a halfword to the state cell")
          go (fuel - 1) (next written {machineStores = (address, width, value .&. 0xffff) : machineStores written})
    A.Load _ A.Word False destination base (A.OffsetImmediate delta) A.OffsetOnly
      | base == A.pc && destination /= A.pc -> do
          value <- wordAt bytes (machinePc machine + 8 + fromIntegral delta)
          go (fuel - 1) (next (setReg destination value machine))
    A.StoreMultiple _ base A.DecrementBefore True registers | base == A.sp -> do
      let low = reg machine A.sp - 4 * fromIntegral (length registers)
          stack = foldr (\(slot, register) acc -> M.insert (low + 4 * slot) (reg machine register) acc) (machineStack machine) (zip [0 ..] (sort registers))
      go (fuel - 1) (next (setReg A.sp low machine {machineStack = stack}))
    A.LoadMultiple _ base A.IncrementAfter True registers | base == A.sp -> do
      let low = reg machine A.sp
      values <- forM (zip [0 ..] (sort registers)) $ \(slot, register) -> case M.lookup (low + 4 * slot) (machineStack machine) of
        Just value -> Right (register, value)
        Nothing -> refuse machine "load from unwritten stack"
      let restored = foldr (\(register, value) acc -> setReg register value acc) (setReg A.sp (low + 4 * fromIntegral (length registers)) machine) [(r, v) | (r, v) <- values, r /= A.pc]
      case lookup A.pc values of
        Just target -> finish restored target
        Nothing -> go (fuel - 1) (next restored)
    A.Branch _ True target
      | target == transitionRouter ->
          let clobbered = foldr (\n acc -> setReg (A.Reg n) (0xbad00000 + fromIntegral n) acc) machine [1, 2, 3, 12, 14]
           in go (fuel - 1) (next (setReg (A.Reg 0) routerValue clobbered {machineFlags = Nothing, machineRouterCalls = machineRouterCalls machine + 1}) {machineRouterIntact = True})
      | otherwise -> refuse machine ("call to " ++ hex target)
    A.Branch _ False target -> go (fuel - 1) machine {machinePc = target}
    A.BranchExchange _ False register | register == A.lr -> finish machine (reg machine A.lr)
    _ -> refuse machine ("unsupported instruction " ++ show instruction)

registersRead :: A.Instruction -> [A.Reg]
registersRead instruction = case instruction of
  A.DataProcessing _ op _ _ source operand -> [source | op `notElem` [A.MOV, A.MVN]] ++ operandRegisters operand
  A.MoveTop _ destination _ -> [destination]
  A.Store _ _ source base offset _ -> source : base : offsetRegisters offset
  A.Load _ _ _ _ base offset _ -> base : offsetRegisters offset
  A.StoreMultiple _ base _ _ registers -> base : registers
  A.LoadMultiple _ base _ _ _ -> [base]
  A.BranchExchange _ _ register -> [register]
  _ -> []
 where
  operandRegisters operand = case operand of
    A.Immediate _ -> []
    A.ShiftedImmediate register _ _ -> [register]
    A.ShiftedRegister register _ amount -> [register, amount]
  offsetRegisters offset = case offset of
    A.OffsetImmediate _ -> []
    A.OffsetRegister _ register _ _ -> [register]

conditionOf :: A.Instruction -> A.Cond
conditionOf instruction = case instruction of
  A.DataProcessing c _ _ _ _ _ -> c
  A.MoveWide c _ _ -> c
  A.MoveTop c _ _ -> c
  A.ReadStatus c _ -> c
  A.WriteApplicationFlags c _ -> c
  A.ZeroExtend c _ _ _ _ -> c
  A.Multiply c _ _ _ _ -> c
  A.Compute c _ -> c
  A.Load c _ _ _ _ _ _ -> c
  A.Store c _ _ _ _ _ -> c
  A.LoadMultiple c _ _ _ _ -> c
  A.StoreMultiple c _ _ _ _ -> c
  A.Branch c _ _ -> c
  A.BranchExchange c _ _ -> c
  A.Hint c -> c
  A.Rejected _ -> A.Cond 14

-- Router results that separate every path of a body that, after the router
-- call, reads the unmodified result only through `cmp/cmn r0, #imm` tested
-- by EQ or NE: each compared constant and at least one value compared by none.
candidates :: B.ByteString -> Callback -> Int -> Either String [Word32]
candidates bytes callback@(Callback entry) size = do
  words' <- mapM (wordAt bytes) [entry, entry + 4 .. entry + fromIntegral size - 4]
  let compared = concat [ case instruction of
                            A.DataProcessing _ A.CMP _ _ _ (A.Immediate value) -> [value]
                            A.DataProcessing _ A.CMN _ _ _ (A.Immediate value) -> [negate value]
                            _ -> []
                        | (address, word) <- zip [entry, entry + 4 ..] words', let instruction = A.decode address word ]
      fillers = [value | value <- [0, 1, 0xffffffff, 0x7fffffff, 0x12345678], value `notElem` compared]
  when (null fillers) (Left (show callback ++ ": no uncompared router result"))
  pure (nub (compared ++ fillers))

outcomes :: B.ByteString -> Callback -> Int -> Either String [(Word32, Outcome)]
outcomes bytes callback size = do
  values <- candidates bytes callback size
  results <- mapM (\value -> (,) value <$> execute bytes callback size value) values
  forM_ results $ \(_, outcome) -> unless (outcomeConditions outcome `S.isSubsetOf` S.fromList [0, 1]) (Left (show callback ++ ": router result tested by a condition other than EQ/NE"))
  pure results

derive :: B.ByteString -> Callback -> Int -> Either String Generic
derive bytes callback size = do
  results <- outcomes bytes callback size
  let calls = nub [outcomeRouterCalls outcome | (_, outcome) <- results]
  case calls of
    [0] -> case nub [(outcomeResult outcome, outcomeStates outcome) | (_, outcome) <- results] of
      [(result, [])] -> Right (Constant result)
      _ -> Left (show callback ++ ": router-free body is not a constant result")
    [1] -> do
      unless (all (\(value, outcome) -> outcomeResult outcome == value) results) (Left (show callback ++ ": result is not the router result"))
      pairs <- fmap concat . forM results $ \(value, outcome) -> case outcomeStates outcome of
        [] -> Right []
        [s] | s < 46 -> Right [(value, State s)]
        _ -> Left (show callback ++ ": more than one or an out-of-range state store")
      unless (length pairs <= 4) (Left (show callback ++ ": more than four routed states"))
      Right (Route pairs)
    _ -> Left (show callback ++ ": router called other than once on every path")

-- The model `modal75::run_generic` implements.
model :: Generic -> Word32 -> (Word32, [Word16], Int)
model generic value = case generic of
  Constant result -> (result, [], 0)
  Route pairs -> (value, [s | Just (State s) <- [lookup value pairs]], 1)

verifyAction :: B.ByteString -> Callback -> Int -> Generic -> Either String ()
verifyAction bytes callback size generic = do
  results <- outcomes bytes callback size
  forM_ results $ \(value, outcome) ->
    unless (model generic value == (outcomeResult outcome, outcomeStates outcome, outcomeRouterCalls outcome))
      (Left (show callback ++ ": owned action differs from stock for router result " ++ hex value))

-- * Table

data StockTables = StockTables {stockCells :: Word32, stockActions :: Word32, stockTransitions :: Word32}

stockCell :: B.ByteString -> StockTables -> State -> Event -> Either String (Word32, Word32, Int16)
stockCell bytes tables (State s) (Event e) = do
  unless (s < 46 && e < 51) (Left "cell outside 46 x 51")
  word <- wordAt bytes (stockCells tables + 4 * (fromIntegral e * 46 + fromIntegral s))
  target <- wordAt bytes (stockActions tables + 4 * (word .&. 0xffff))
  pure (word, target, fromIntegral (word `shiftR` 16))

verifyCell :: B.ByteString -> StockTables -> [(Callback, Generic)] -> OwnedCell -> Either String ()
verifyCell bytes tables actions cell = do
  (word, target, high) <- stockCell bytes tables (cellState cell) (cellEvent cell)
  let at = show (cellState cell, cellEvent cell)
  unless (word == cellWord cell && target == cellTarget cell) (Left ("cell " ++ at ++ ": table words differ from stock"))
  case cellAction cell of
    GenericAt index -> case drop index actions of
      (Callback entry, _) : _ | entry == target -> Right ()
      _ -> Left ("cell " ++ at ++ ": generic action is not the stock target's")
    Retained -> Right ()
  case cellAfter cell of
    SetState (State s) -> unless (high <= 0 && fromIntegral (negate high) == s) (Left ("cell " ++ at ++ ": direct state differs from stock"))
    Keep -> do
      transition <- wordAt bytes (stockTransitions tables + 4 * fromIntegral (max 0 high))
      unless (high > 0 && transition == 0) (Left ("cell " ++ at ++ ": kept state needs a null positive transition"))

applyControl :: Control -> [(Callback, Generic)] -> [OwnedCell] -> Either String ([(Callback, Generic)], [OwnedCell])
applyControl control actions cells = case control of
  NoControl -> Right (actions, cells)
  WrongNextState -> case break (\(_, g) -> case g of Route (_ : _) -> True; _ -> False) actions of
    (before, (callback, Route ((value, State s) : rest)) : after) -> Right (before ++ (callback, Route ((value, State ((s + 1) `mod` 46)) : rest)) : after, cells)
    _ -> Left "control wrong-next-state: no routed state"
  WrongActionResult -> case break (\(_, g) -> case g of Constant _ -> True; _ -> False) actions of
    (before, (callback, Constant result) : after) -> Right (before ++ (callback, Constant (result + 1)) : after, cells)
    _ -> Left "control wrong-action-result: no constant action"
  WrongRouteEvent -> case break (\cell -> case cellAction cell of GenericAt _ -> True; _ -> False) cells of
    (before, cell : after) | Event e <- cellEvent cell, e + 1 < 51 -> Right (actions, before ++ cell {cellEvent = Event (e + 1)} : after)
    _ -> Left "control wrong-route-event: no shiftable generic cell"

rustTable :: String -> [(Callback, Generic)] -> [OwnedCell] -> String
rustTable digest actions cells = unlines
  ( [ "// Generated by //replacement/modal_runtime:controller75-table from owner-supplied image5"
    , "// (sha256 " ++ digest ++ "); not for redistribution."
    , "Table75 {"
    , "    actions: &[" ]
    ++ [ "        " ++ rustAction generic ++ ", // " ++ hex entry | (Callback entry, generic) <- actions ]
    ++ [ "    ],", "    cells: &[" ]
    ++ [ "        OwnedCell { event: Event75(" ++ show e ++ "), state: State75(" ++ show s ++ "), cell_word: " ++ hex (cellWord cell) ++ ", stock_target: " ++ hex (cellTarget cell)
           ++ ", action: " ++ (case cellAction cell of Retained -> "CellAction::Retained"; GenericAt i -> "CellAction::Generic(" ++ show i ++ ")")
           ++ ", after: " ++ (case cellAfter cell of Keep -> "AfterAction::KeepActionState"; SetState (State n) -> "AfterAction::SetState(State75(" ++ show n ++ "))") ++ " },"
       | cell <- cells, let Event e = cellEvent cell; State s = cellState cell ]
    ++ [ "    ],", "}" ] )
 where
  rustAction generic = case generic of
    Constant result -> "GenericAction::ConstantResult(ActionResult(" ++ hex result ++ "))"
    Route pairs ->
      let padded = take 4 ([(value, s) | (value, State s) <- pairs] ++ repeat (0, 0))
       in "GenericAction::RouteToTransition(Route::new([" ++ intercalate ", " ["(" ++ hex value ++ ", State75(" ++ show s ++ "))" | (value, s) <- padded] ++ "], " ++ show (length pairs) ++ "))"

data StockVector = StockVector Event State Word32 Word32 (Maybe Word32) Word32 (Maybe Word32) [Word16]

-- Stock outcomes per owned cell and router result, for `modal75::check_vectors`.
rustVectors :: String -> [StockVector] -> String
rustVectors digest vectors = unlines
  ( [ "// Generated by //replacement/modal_runtime:controller75-table from owner-supplied image5"
    , "// (sha256 " ++ digest ++ "); not for redistribution."
    , "&[" ]
    ++ [ "    StockVector { event: " ++ show e ++ ", state: " ++ show s ++ ", live: LiveCell { word: " ++ hex word ++ ", target: " ++ hex target
           ++ ", transition: " ++ maybe "None" (\t -> "Some(" ++ hex t ++ ")") transition ++ " }, router: " ++ hex router
           ++ ", result: " ++ maybe "None" (\r -> "Some(" ++ hex r ++ ")") result ++ ", states: &[" ++ intercalate ", " (map show states) ++ "] },"
       | StockVector (Event e) (State s) word target transition router result states <- vectors ]
    ++ [ "]" ] )

main :: IO ()
main = do
  arguments <- getArgs
  let required name = case dropWhile (/= name) arguments of _ : value : _ -> pure value; _ -> fail ("missing " ++ name)
      optional name = case dropWhile (/= name) arguments of _ : value : _ -> Just value; _ -> Nothing
  outputPath <- required "--out"
  exists <- doesPathExist outputPath
  when exists (fail "output already exists")
  vectorsPath <- required "--vectors-out"
  vectorsExist <- doesPathExist vectorsPath
  when vectorsExist (fail "vectors output already exists")
  inventoryPath <- required "--inventory"
  imagePath <- required "--id5"
  control <- case optional "--control" of
    Nothing -> pure NoControl
    Just "wrong-next-state" -> pure WrongNextState
    Just "wrong-action-result" -> pure WrongActionResult
    Just "wrong-route-event" -> pure WrongRouteEvent
    Just other -> fail ("unknown control " ++ other)
  inventoryBytes <- B.readFile inventoryPath
  unless (sha256Hex (sha256 inventoryBytes) == "153bfd3c9f960e9b888cfc6991edf157ccc85a09bce8de1f36ff59ec4265d0e8") (fail "inventory identity differs")
  bytes <- B.readFile imagePath
  let digest = sha256Hex (sha256 bytes)
  unless (B.length bytes == 5185120 && digest == "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a") (fail "image5 identity differs")
  Inventory callbacks direct <- either fail pure (J.eitherDecodeStrict inventoryBytes)
  let generic = [(callback, size, cells) | InventoryCallback callback size layer cells <- callbacks, layer == "a-generic-instance"]
  outcome <- either fail pure $ do
    width <- halfAt bytes (0xcd5e78 + 150)
    unless (width == 46) (Left "controller 75 width differs from 46")
    tables <- StockTables <$> wordAt bytes (0xcf58b8 + 300) <*> wordAt bytes (0xcfe2a8 + 300) <*> wordAt bytes (0xd0013c + 300)
    actions <- mapM (\(callback, size, _) -> (,) callback <$> derive bytes callback size) generic
    let indexOf = M.fromList (zip [callback | (callback, _) <- actions] [0 ..])
        coordinates = S.toList (S.fromList ([(s, e) | (_, _, cells) <- generic, (s, e) <- cells] ++ [(s, e) | DirectCell s e <- direct]))
    cells <- forM coordinates $ \(s, e) -> do
      (word, target, high) <- stockCell bytes tables s e
      let action = maybe Retained GenericAt (M.lookup (Callback target) indexOf)
          after = if high <= 0 then SetState (State (fromIntegral (negate high))) else Keep
      pure (OwnedCell e s word target action after)
    (actions', cells') <- applyControl control actions cells
    forM_ actions' $ \(callback, g) -> case lookup callback [(c, size) | (c, size, _) <- generic] of
      Just size -> verifyAction bytes callback size g
      Nothing -> Left "action without inventory callback"
    mapM_ (verifyCell bytes tables actions') cells'
    vectors <- fmap concat . forM coordinates $ \(s, e) -> do
      (word, target, high) <- stockCell bytes tables s e
      transition <- if high > 0 then Just <$> wordAt bytes (stockTransitions tables + 4 * fromIntegral high) else Right Nothing
      let dispatcherStore = [fromIntegral (negate high) | high <= 0]
          vector router result states = StockVector e s word target transition router result (states ++ dispatcherStore)
      case lookup (Callback target) [(c, size) | (c, size, _) <- generic] of
        Just size -> map (\(value, outcome) -> vector value (Just (outcomeResult outcome)) (outcomeStates outcome)) <$> outcomes bytes (Callback target) size
        Nothing -> Right [vector 0 Nothing []]
    pure (actions', cells', vectors)
  let (actions, cells, vectors) = outcome
  writeFile outputPath (rustTable digest actions cells)
  writeFile vectorsPath (rustVectors digest vectors)
  putStrLn ("wrote " ++ outputPath ++ " and " ++ vectorsPath ++ "; actions=" ++ show (length actions) ++ "; cells=" ++ show (length cells)
    ++ "; direct-state=" ++ show (length [() | cell <- cells, SetState _ <- [cellAfter cell]]) ++ "; vectors=" ++ show (length vectors))
