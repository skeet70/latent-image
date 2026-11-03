{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- Routing maps of the two controller routers that write another controller's
-- event cell and reenter the dispatcher, recovered by bounded path-sensitive
-- symbolic execution over the pinned image5 bytes.
module Main where

import qualified A32 as A
import Control.Monad (unless, when)
import Data.Aeson (FromJSON (..), Value, object, (.:), (.=))
import Data.Aeson.Types (Parser)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.Bits (complement, shiftL, shiftR, (.&.), (.|.))
import Data.List (sortOn)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import Emulation.Sha256 (sha256, sha256Hex)
import Numeric (readHex, showHex)
import System.Directory (doesPathExist)
import System.Environment (getArgs)

newtype ControllerId = ControllerId Word32 deriving (Eq, Ord, Show)
newtype CodeAddress = CodeAddress Word32 deriving (Eq, Ord, Show)
newtype Router = Router CodeAddress deriving (Eq, Ord, Show)
newtype AccessBytes = AccessBytes Int deriving (Eq, Ord, Show)

accessBytes :: A.Width -> Int
accessBytes width = case width of
  A.Byte -> 1
  A.Half -> 2
  A.Word -> 4
  A.Dual -> 8
data CellField = StateCell | EventCell deriving (Eq, Ord, Show)

-- A value the router reads from outside its own registers. Memory words are
-- identified by address only: see `stableMemoryPremise`.
data Symbol
  = MemorySymbol AccessBytes Bool Word32
  | ResultSymbol CodeAddress CodeAddress
  | QuerySymbol CodeAddress [Word32]
  | ArgumentSymbol Int
  deriving (Eq, Ord, Show)

-- Offset: symbol plus a constant, modulo 2^32. TableEntry: the word or
-- halfword at base + ((symbol + offset) << scale).
data Val
  = Known Word32
  | Offset Symbol Word32
  | TableEntry Word32 AccessBytes Symbol Word32 Int
  | Unknown
  deriving (Eq, Ord, Show)

-- CompareFlags x c: flags of x - c. ResultFlags x: only N and Z of x are known.
-- OperandFlags x y: flags of CMP x, y with y not constant; only recorded.
data Flags = CompareFlags Val Word32 | ResultFlags Val | OperandFlags Val Val | UnknownFlags deriving (Eq, Show)

-- Inclusive unsigned 32-bit intervals, sorted and disjoint.
newtype Domain = Domain [(Integer, Integer)] deriving (Eq, Show)

data Atom = UnmodelledCondition CodeAddress A.Cond Bool (Maybe (Val, Val)) deriving (Eq, Show)

data Store = Store CodeAddress Word32 A.Width Val deriving (Eq, Show)
data Call = Call CodeAddress CodeAddress [Val] deriving (Eq, Show)

data PathState = PathState
  { pathPc :: Word32
  , pathRegisters :: M.Map A.Reg Val
  , pathFlags :: Flags
  -- Outcomes chosen for conditions on the current unmodelled flags.
  , pathDecided :: [(A.Cond, Bool)]
  , pathMemory :: M.Map Word32 (A.Width, Val)
  , pathDomains :: M.Map Symbol Domain
  , pathAtoms :: [Atom]
  , pathStores :: [Store]
  , pathCalls :: [Call]
  , pathVisits :: M.Map Word32 Int
  }

withFlags :: Flags -> PathState -> PathState
withFlags flags state = state {pathFlags = flags, pathDecided = []}

data Route = Route
  { routeRouter :: Router
  , routeSite :: CodeAddress
  , routeTarget :: Val
  , routeStores :: [Store]
  , routeDomains :: M.Map Symbol Domain
  , routeAtoms :: [Atom]
  , routeCalls :: [Call]
  }

data Ending = Returned CodeAddress Val | LoopBound CodeAddress | Unresolved CodeAddress String deriving (Eq, Show)

-- Paths reaching one dispatch site with one target and event value. The
-- domains keep the symbols every such path constrains, as the union over the
-- paths: the values for which some path reaches the site.
data Aggregate = Aggregate
  { aggPaths :: !Int
  , aggDomains :: !(M.Map Symbol Domain)
  , aggSymbols :: !(S.Set Symbol)
  , aggUnmodelled :: !Int
  , aggArguments :: !Bool
  , aggSample :: Route
  }

data ReturnAggregate = ReturnAggregate !Int !(M.Map Symbol Domain)

type RouteKey = (CodeAddress, Val, Maybe Val)

data Exploration = Exploration
  { explorationRoutes :: !(M.Map RouteKey Aggregate)
  , explorationReturns :: !Int
  , explorationUndispatched :: !(M.Map CodeAddress ReturnAggregate)
  , explorationLoopBounds :: ![CodeAddress]
  , explorationUnresolved :: ![(CodeAddress, String)]
  , explorationSteps :: !Int
  }

unionDomain :: Domain -> Domain -> Domain
unionDomain (Domain left) (Domain right) = normalize (left ++ right)

mergeAggregate :: Aggregate -> Aggregate -> Aggregate
mergeAggregate new old = Aggregate (aggPaths new + aggPaths old) (M.intersectionWith unionDomain (aggDomains new) (aggDomains old))
  (S.union (aggSymbols new) (aggSymbols old)) (aggUnmodelled new + aggUnmodelled old) (aggArguments new || aggArguments old) (aggSample old)

imageBase :: Word32
imageBase = 0xbf1a00

dispatcher :: CodeAddress
dispatcher = CodeAddress 0xbfb028

routers :: [Router]
routers = [Router (CodeAddress 0xc0dbf8), Router (CodeAddress 0xc12aac)]

stateBase, eventBase, controllerCount :: Word32
stateBase = 0x1467588
eventBase = 0x14676e4
controllerCount = 174

visitLimit :: Int
visitLimit = 4

stepLimit :: Int
stepLimit = 50000000

stableMemoryPremise :: String
stableMemoryPremise = "router-callees-preserve-loaded-words: a word loaded at the same address on one path has the same value at every load, so the router's callees on that path (listed per route) do not write the words the router compares"

queryPremise :: String
queryPremise = "router-queries-repeatable: on one path, calls to a callee in `query_arity` with the same constant leading arguments return the same r0, so each such call is one query-result symbol"

-- Argument counts from the pinned decompilation (ghidra/all.c lines 21741, 28420, 529635, 506793).
queryArity :: M.Map CodeAddress Int
queryArity = M.fromList [(CodeAddress 0x632a3c, 1), (CodeAddress 0x63a0e0, 0), (CodeAddress 0xd35fbc, 0), (CodeAddress 0xd1740c, 1)]

hex :: Word32 -> String
hex value = "0x" ++ replicate (8 - length digits) '0' ++ digits where digits = showHex value ""

codeText :: CodeAddress -> String
codeText (CodeAddress address) = hex address

wordAt :: B.ByteString -> Word32 -> Maybe Word32
wordAt bytes address
  | address < imageBase || toInteger address + 4 > toInteger imageBase + toInteger (B.length bytes) = Nothing
  | otherwise = Just (foldr (\index value -> (value `shiftL` 8) .|. fromIntegral (B.index bytes (fromIntegral (address - imageBase) + index))) 0 [0 .. 3])

cellOf :: Word32 -> Maybe (ControllerId, CellField)
cellOf address
  | address >= stateBase && address < stateBase + 2 * controllerCount && even (address - stateBase) = Just (ControllerId ((address - stateBase) `div` 2), StateCell)
  | address >= eventBase && address < eventBase + 2 * controllerCount && even (address - eventBase) = Just (ControllerId ((address - eventBase) `div` 2), EventCell)
  | otherwise = Nothing

-- Domains --------------------------------------------------------------------

modulus :: Integer
modulus = 2 ^ (32 :: Int)

fullDomain :: Domain
fullDomain = Domain [(0, modulus - 1)]

normalize :: [(Integer, Integer)] -> Domain
normalize = Domain . merge . sortOn fst . filter (uncurry (<=))
  where
    merge ((a, b) : (c, d) : rest) | c <= b + 1 = merge ((a, max b d) : rest)
    merge (x : rest) = x : merge rest
    merge [] = []

intersectDomain :: Domain -> Domain -> Domain
intersectDomain (Domain left) (Domain right) = normalize [(max a c, min b d) | (a, b) <- left, (c, d) <- right]

complementDomain :: Domain -> Domain
complementDomain (Domain spans) = normalize (go 0 spans)
  where
    go low [] = [(low, modulus - 1)]
    go low ((a, b) : rest) = (low, a - 1) : go (b + 1) rest

-- The domain of s given the domain of s + k.
shiftDomain :: Word32 -> Domain -> Domain
shiftDomain k (Domain spans) = normalize (concatMap move spans)
  where
    delta = negate (toInteger k)
    move (a, b) =
      let a' = (a + delta) `mod` modulus
          b' = a' + (b - a)
       in if b' < modulus then [(a', b')] else [(a', modulus - 1), (0, b' - modulus)]

signedRange :: Integer -> Integer -> Domain
signedRange low high = normalize [(toU a, toU b) | (a, b) <- [(max low (-half), min high (-1)), (max low 0, min high (half - 1))], a <= b]
  where
    half = 2 ^ (31 :: Int)
    toU value = if value < 0 then value + modulus else value

signed :: Word32 -> Integer
signed value = if value >= 0x80000000 then toInteger value - modulus else toInteger value

emptyDomain :: Domain -> Bool
emptyDomain (Domain spans) = null spans

domainSize :: Domain -> Integer
domainSize (Domain spans) = sum [b - a + 1 | (a, b) <- spans]

-- Values x with x compared against c satisfying the condition, or Nothing when
-- the condition uses flags the comparison does not determine.
compareDomain :: A.Cond -> Word32 -> Maybe Domain
compareDomain (A.Cond condition) c = case condition of
  0 -> Just (normalize [(cu, cu)])
  1 -> Just (complementDomain (normalize [(cu, cu)]))
  2 -> Just (normalize [(cu, modulus - 1)])
  3 -> Just (normalize [(0, cu - 1)])
  4 -> Just (shiftDomain (negate c) (normalize [(0x80000000, modulus - 1)]))
  5 -> Just (shiftDomain (negate c) (normalize [(0, 0x7fffffff)]))
  8 -> Just (normalize [(cu + 1, modulus - 1)])
  9 -> Just (normalize [(0, cu)])
  10 -> Just (signedRange (signed c) (2 ^ (31 :: Int) - 1))
  11 -> Just (signedRange (negate (2 ^ (31 :: Int))) (signed c - 1))
  12 -> Just (signedRange (signed c + 1) (2 ^ (31 :: Int) - 1))
  13 -> Just (signedRange (negate (2 ^ (31 :: Int))) (signed c))
  _ -> Nothing
  where
    cu = toInteger c

-- N and Z of the value itself.
resultDomain :: A.Cond -> Maybe Domain
resultDomain (A.Cond condition) = case condition of
  0 -> Just (normalize [(0, 0)])
  1 -> Just (normalize [(1, modulus - 1)])
  4 -> Just (normalize [(0x80000000, modulus - 1)])
  5 -> Just (normalize [(0, 0x7fffffff)])
  _ -> Nothing

-- Splits a state on a condition into the states where it holds and fails.
branchOn :: CodeAddress -> A.Cond -> PathState -> [(Bool, PathState)]
branchOn site condition state
  | condition == A.Cond 14 = [(True, state)]
  | otherwise = case pathFlags state of
      CompareFlags (Known x) c -> decided (member x <$> compareDomain condition c)
      ResultFlags (Known x) -> decided (resultDomain condition >>= \domain -> pure (member x domain))
      CompareFlags (Offset symbol k) c | Just holds <- compareDomain condition c -> constrained symbol k holds
      ResultFlags (Offset symbol k) | Just holds <- resultDomain condition -> constrained symbol k holds
      _ -> unmodelled
  where
    member x (Domain spans) = any (\(a, b) -> toInteger x >= a && toInteger x <= b) spans
    decided (Just outcome) = [(outcome, state)]
    decided Nothing = unmodelled
    operands = case pathFlags state of
      OperandFlags left right -> Just (left, right)
      _ -> Nothing
    -- Unmodelled flags tested again on one path keep their first outcome.
    unmodelled = case [outcome | (tested, outcome) <- pathDecided state, tested == condition] ++ [not outcome | (tested, outcome) <- pathDecided state, tested == A.negateCond condition] of
      outcome : _ -> [(outcome, state)]
      [] -> [ (outcome, state {pathAtoms = UnmodelledCondition site condition outcome operands : pathAtoms state, pathDecided = (condition, outcome) : pathDecided state})
            | outcome <- [True, False] ]
    constrained symbol k holds =
      [ (outcome, state {pathDomains = M.insert symbol narrowed (pathDomains state)})
      | (outcome, values) <- [(True, holds), (False, complementDomain holds)]
      , let narrowed = intersectDomain (M.findWithDefault fullDomain symbol (pathDomains state)) (shiftDomain k values)
      , not (emptyDomain narrowed)
      ]

-- Execution ------------------------------------------------------------------

addValue :: Val -> Word32 -> Val
addValue value k = case value of
  Known x -> Known (x + k)
  Offset symbol offset -> Offset symbol (offset + k)
  _ -> Unknown

operandValue :: PathState -> Word32 -> A.Operand -> Val
operandValue state pc operand = case operand of
  A.Immediate value -> Known value
  A.ShiftedImmediate register A.LSL 0 -> registerValue state pc register
  A.ShiftedImmediate register shift amount -> case registerValue state pc register of
    Known x -> maybe Unknown Known (shifted shift amount x)
    _ -> Unknown
  A.ShiftedRegister {} -> Unknown
  where
    shifted A.LSL amount x = Just (x `shiftL` amount)
    shifted A.LSR amount x = Just (if amount == 0 then 0 else x `shiftR` amount)
    shifted _ _ _ = Nothing

registerValue :: PathState -> Word32 -> A.Reg -> Val
registerValue state pc register
  | register == A.pc = Known (pc + 8)
  | otherwise = M.findWithDefault Unknown register (pathRegisters state)

setRegister :: A.Reg -> Val -> PathState -> PathState
setRegister register value state = state {pathRegisters = M.insert register value (pathRegisters state)}

loadValue :: B.ByteString -> PathState -> Word32 -> A.Reg -> A.Width -> Bool -> Val -> Maybe (A.Reg, A.Shift, Int, Bool) -> Val
loadValue bytes state pc base width signedLoad address index = case (address, index) of
  (Known at, Nothing)
    | base == A.pc, width == A.Word -> maybe Unknown Known (wordAt bytes at)
    | Just (storedWidth, value) <- M.lookup at (pathMemory state), storedWidth == width, not signedLoad -> value
    | any (overlapsStore at) (M.toList (pathMemory state)) -> Unknown
    | width `elem` [A.Word, A.Half, A.Byte] -> Offset (MemorySymbol (AccessBytes (accessBytes width)) signedLoad at) 0
  (Known at, Just (register, A.LSL, scale, True))
    | Offset symbol k <- registerValue state pc register -> TableEntry at (AccessBytes (accessBytes width)) symbol k scale
  _ -> Unknown
  where
    size = toInteger (accessBytes width)
    overlapsStore at (stored, (storedWidth, _)) = toInteger stored < toInteger at + size && toInteger at < toInteger stored + toInteger (accessBytes storedWidth)

-- One instruction. Returns successor states, or an ending.
step :: B.ByteString -> PathState -> Either Ending [PathState]
step bytes state = case wordAt bytes pc of
  Nothing -> Left (Unresolved site "left image5")
  Just word -> case A.decode pc word of
    instruction -> do
      let condition = conditionOf instruction
      concat <$> mapM (\(holds, branchState) -> if holds then execute instruction branchState else Right [branchState {pathPc = pc + 4}]) (branchOn site condition state)
  where
    pc = pathPc state
    site = CodeAddress pc
    next current = current {pathPc = pc + 4}
    clobberCall current = foldl' (\s register -> setRegister register Unknown s) current (map A.Reg [1, 2, 3, 12, 14])
    execute instruction current = case instruction of
      A.MoveWide _ destination value -> Right [next (setRegister destination (Known value) current)]
      A.MoveTop _ destination value -> Right [next (setRegister destination (case registerValue current pc destination of
        Known low -> Known ((low .&. 0xffff) .|. (value `shiftL` 16))
        _ -> Unknown) current)]
      A.DataProcessing _ operation setsFlags destination source operand
        | operation `elem` [A.CMP, A.CMN, A.TST, A.TEQ] -> Right [next (withFlags (testFlags operation (registerValue current pc source) (operandValue current pc operand)) current)]
        | destination == A.pc -> Left (Unresolved site "data-processing write to PC")
        | otherwise ->
            let left = registerValue current pc source
                right = operandValue current pc operand
                result = case (operation, left, right) of
                  (A.MOV, _, value) -> value
                  (A.MVN, _, Known value) -> Known (complement value)
                  (A.ADD, value, Known k) -> addValue value k
                  (A.ADD, Known k, value) -> addValue value k
                  (A.SUB, value, Known k) -> addValue value (negate k)
                  (A.RSB, Known x, Known y) -> Known (y - x)
                  (A.AND, Known x, Known y) -> Known (x .&. y)
                  (A.ORR, Known x, Known y) -> Known (x .|. y)
                  (A.EOR, Known x, Known y) -> Known (x `xorWord` y)
                  (A.BIC, Known x, Known y) -> Known (x .&. complement y)
                  _ -> Unknown
                flagged
                  | not setsFlags = id
                  | operation == A.SUB, Known k <- right = withFlags (CompareFlags left k)
                  | otherwise = withFlags (ResultFlags result)
             in Right [next (flagged (setRegister destination result current))]
      A.Load _ width signedLoad destination base offset indexing -> do
        when (destination == A.pc) (Left (Unresolved site "load to PC"))
        let baseValue = registerValue current pc base
            (address, index) = case offset of
              A.OffsetImmediate delta -> (if indexing == A.PostIndexed then baseValue else addValue baseValue (fromIntegral delta), Nothing)
              A.OffsetRegister added register shift amount -> case registerValue current pc register of
                Known k -> (if indexing == A.PostIndexed then baseValue else maybe Unknown (addValue baseValue) (scaled added shift amount k), Nothing)
                _ -> (baseValue, Just (register, shift, amount, added))
            loaded = if width == A.Dual then Unknown else loadValue bytes current pc base width signedLoad address index
            written = writeback base indexing offset baseValue current
        Right [next (setRegister destination loaded written)]
      A.Store _ width source base offset indexing ->
        let baseValue = registerValue current pc base
            address = case offset of
              A.OffsetImmediate delta -> if indexing == A.PostIndexed then baseValue else addValue baseValue (fromIntegral delta)
              A.OffsetRegister added register shift amount -> case registerValue current pc register of
                Known k -> if indexing == A.PostIndexed then baseValue else maybe Unknown (addValue baseValue) (scaled added shift amount k)
                _ -> Unknown
            value = registerValue current pc source
            stored = case address of
              Known at -> current {pathMemory = M.insert at (width, value) (M.filterWithKey (\other (otherWidth, _) -> not (toInteger other < toInteger at + toInteger (accessBytes width) && toInteger at < toInteger other + toInteger (accessBytes otherWidth))) (pathMemory current)), pathStores = Store site at width value : pathStores current}
              -- A store through an unknown address may change any recorded word.
              _ | base == A.sp || base == A.Reg 11 -> current
                | otherwise -> current {pathMemory = M.empty}
         in Right [next (writeback base indexing offset baseValue stored)]
      A.StoreMultiple {} -> Right [next current]
      A.LoadMultiple _ _ _ _ listed
        | A.pc `elem` listed -> Left (Returned site (registerValue current pc (A.Reg 0)))
        | otherwise -> Right [next (foldl' (\s register -> setRegister register Unknown s) current listed)]
      A.Branch _ True target
        | CodeAddress target == dispatcher -> Left (Unresolved site "dispatcher call reached outside the route recorder")
        | otherwise -> Right [called (CodeAddress target) current]
      A.Branch _ False target -> Right [current {pathPc = target}]
      A.BranchExchange _ False register | register == A.lr -> Left (Returned site (registerValue current pc (A.Reg 0)))
      A.BranchExchange {} -> Left (Unresolved site "register branch")
      A.Hint _ -> Right [next current]
      A.Compute _ destinations -> Right [next (foldl' (\s register -> setRegister register Unknown s) current destinations)]
      A.Multiply _ destination _ _ _ -> Right [next (setRegister destination Unknown current)]
      A.ZeroExtend _ destination _ _ _ -> Right [next (setRegister destination Unknown current)]
      A.ReadStatus _ destination -> Right [next (setRegister destination Unknown current)]
      A.WriteApplicationFlags _ _ -> Right [next (withFlags UnknownFlags current)]
      A.Rejected reason -> Left (Unresolved site (A.forbiddenReason reason))
    called target current =
      let arguments = map (registerValue current pc . A.Reg) [0 .. 3]
          result = case M.lookup target queryArity of
            Just arity | known <- [x | Known x <- take arity arguments], length known == arity -> QuerySymbol target known
            _ -> ResultSymbol site target
          returned = setRegister (A.Reg 0) (Offset result 0) (clobberCall current)
       in next (withFlags UnknownFlags returned {pathCalls = Call site target arguments : pathCalls current})
    -- CMN x, k gives the N and Z of x + k; its C and V are not modelled.
    testFlags operation left right = case (operation, right) of
      (A.CMP, Known k) -> CompareFlags left k
      (A.CMN, Known k) -> ResultFlags (addValue left k)
      (A.CMP, _) -> OperandFlags left right
      _ -> UnknownFlags
    scaled added shift amount k = case shift of
      A.LSL -> Just ((if added then id else negate) (k `shiftL` amount))
      _ -> Nothing
    writeback base indexing offset baseValue current
      | indexing == A.OffsetOnly = current
      | otherwise = setRegister base (case offset of
          A.OffsetImmediate delta -> addValue baseValue (fromIntegral delta)
          _ -> Unknown) current

xorWord :: Word32 -> Word32 -> Word32
xorWord x y = (x .|. y) .&. complement (x .&. y)

conditionOf :: A.Instruction -> A.Cond
conditionOf instruction = case instruction of
  A.DataProcessing condition _ _ _ _ _ -> condition
  A.MoveWide condition _ _ -> condition
  A.MoveTop condition _ _ -> condition
  A.ReadStatus condition _ -> condition
  A.WriteApplicationFlags condition _ -> condition
  A.ZeroExtend condition _ _ _ _ -> condition
  A.Multiply condition _ _ _ _ -> condition
  A.Compute condition _ -> condition
  A.Load condition _ _ _ _ _ _ -> condition
  A.Store condition _ _ _ _ _ -> condition
  A.LoadMultiple condition _ _ _ _ -> condition
  A.StoreMultiple condition _ _ _ _ -> condition
  A.Branch condition _ _ -> condition
  A.BranchExchange condition _ _ -> condition
  A.Hint condition -> condition
  A.Rejected _ -> A.Cond 14

-- Explores every path from the router entry. A dispatcher call records a route
-- and then continues as an opaque call.
explore :: B.ByteString -> Router -> Exploration
explore bytes router@(Router (CodeAddress entry)) = go [initial] (Exploration M.empty 0 M.empty [] [] 0)
  where
    initial = PathState entry (M.fromList [(A.Reg n, Offset (ArgumentSymbol n) 0) | n <- [0 .. 3]]) UnknownFlags [] M.empty M.empty [] [] [] M.empty
    go [] result = result
    go (state : pending) result
      | explorationSteps result > stepLimit = result {explorationUnresolved = (CodeAddress (pathPc state), "step limit") : explorationUnresolved result}
      | M.findWithDefault 0 (pathPc state) (pathVisits state) >= visitLimit =
          go pending (counted result {explorationLoopBounds = CodeAddress (pathPc state) : explorationLoopBounds result})
      | otherwise =
          let visited = state {pathVisits = M.insertWith (+) (pathPc state) 1 (pathVisits state)}
              pc = pathPc state
           in case wordAt bytes pc >>= \word -> pure (A.decode pc word) of
                Just (A.Branch condition True target) | CodeAddress target == dispatcher ->
                  let branches = branchOn (CodeAddress pc) condition visited
                      taken = [s | (True, s) <- branches]
                      skipped = [s {pathPc = pc + 4} | (False, s) <- branches]
                      recorded = foldl' record (explorationRoutes result) [routeOf (CodeAddress pc) s | s <- taken]
                   in go (map continueCall taken ++ skipped ++ pending) (counted result {explorationRoutes = recorded})
                _ -> case step bytes visited of
                  Left (Returned site _) -> go pending (counted (returned site visited result))
                  Left (LoopBound site) -> go pending (counted result {explorationLoopBounds = site : explorationLoopBounds result})
                  Left (Unresolved site reason) -> go pending (counted result {explorationUnresolved = (site, reason) : explorationUnresolved result})
                  Right successors -> go (successors ++ pending) (counted result)
    counted result = result {explorationSteps = explorationSteps result + 1}
    routeOf site s = Route router site (registerValue s (pathPc s) (A.Reg 0)) (reverse (pathStores s)) (pathDomains s) (reverse (pathAtoms s)) (reverse (pathCalls s))
    record routes route =
      let aggregate = Aggregate 1 (routeDomains route) (M.keysSet (routeDomains route)) (if null (routeAtoms route) then 0 else 1) (dependsOnArguments route) route
          key = (routeSite route, routeTarget route, routeEvent route >>= snd)
       in M.insertWith mergeAggregate key aggregate routes
    returned site s result
      | any (\(Call _ callee _) -> callee == dispatcher) (pathCalls s) = result {explorationReturns = explorationReturns result + 1}
      | otherwise = result
          { explorationReturns = explorationReturns result + 1
          , explorationUndispatched = M.insertWith (\(ReturnAggregate n d) (ReturnAggregate m e) -> ReturnAggregate (n + m) (M.intersectionWith unionDomain d e))
              site (ReturnAggregate 1 (pathDomains s)) (explorationUndispatched result) }
    continueCall s =
      let pc = pathPc s
          arguments = map (registerValue s pc . A.Reg) [0 .. 3]
          cleared = foldl' (\current register -> setRegister register Unknown current) s (map A.Reg [1, 2, 3, 12, 14])
       in (setRegister (A.Reg 0) (Offset (ResultSymbol (CodeAddress pc) dispatcher) 0) cleared)
            {pathPc = pc + 4, pathFlags = UnknownFlags, pathDecided = [], pathMemory = M.empty, pathCalls = Call (CodeAddress pc) dispatcher arguments : pathCalls s}

-- Manifest --------------------------------------------------------------------

data ManifestController = ManifestController ControllerId [Word32]
data ManifestFunction = ManifestFunction Word32 (Maybe Word32)
data Manifest = Manifest [ManifestController] [ManifestFunction]

parseAddress :: String -> Parser Word32
parseAddress ('0' : 'x' : digits) | [(value, "")] <- readHex digits = pure value
parseAddress text = fail ("address expected: " ++ text)

instance FromJSON ManifestController where
  parseJSON = J.withObject "controller" $ \fields -> do
    ident <- fields .: "id"
    tables <- fields .: "tables" :: Parser [Value]
    actions <- concat <$> mapM (J.withObject "table" (\table -> do
      kind <- table .: "kind"
      words' <- table .: "words"
      if (kind :: String) == "Actions" then mapM parseAddress words' else pure [])) tables
    pure (ManifestController (ControllerId ident) (filter (/= 0) actions))

instance FromJSON ManifestFunction where
  parseJSON = J.withObject "function" $ \fields -> do
    entry <- parseAddress =<< fields .: "address"
    extent <- fields .: "extent" :: Parser Value
    end <- J.withObject "extent" (\e -> do
      text <- e J..:? "code_end_exclusive"
      maybe (pure Nothing) (fmap Just . parseAddress) text) extent
    pure (ManifestFunction entry end)

instance FromJSON Manifest where
  parseJSON = J.withObject "manifest" $ \fields -> Manifest <$> fields .: "controllers" <*> fields .: "functions"

-- JSON -----------------------------------------------------------------------

symbolJson :: Symbol -> Value
symbolJson symbol = case symbol of
  MemorySymbol (AccessBytes size) signedLoad address -> object ["kind" .= ("memory" :: String), "bytes" .= size, "signed" .= signedLoad, "address" .= hex address]
  ResultSymbol site target -> object ["kind" .= ("call-result" :: String), "site" .= codeText site, "callee" .= codeText target]
  QuerySymbol target known -> object ["kind" .= ("query-result" :: String), "callee" .= codeText target, "arguments" .= known]
  ArgumentSymbol n -> object ["kind" .= ("incoming-argument" :: String), "register" .= ("r" ++ show n)]

valueJson :: B.ByteString -> M.Map Symbol Domain -> Val -> Value
valueJson bytes domains value = case value of
  Known x -> object ["kind" .= ("constant" :: String), "value" .= x]
  Offset symbol k -> object ["kind" .= ("symbolic" :: String), "symbol" .= symbolJson symbol, "plus" .= hex k]
  TableEntry base size symbol k scale -> object
    [ "kind" .= ("table" :: String), "base" .= hex base, "bytes" .= (\(AccessBytes n) -> n) size, "index_symbol" .= symbolJson symbol, "index_plus" .= hex k, "scale" .= scale
    , "entries" .= tableEntries bytes domains base size symbol k scale ]
  Unknown -> object ["kind" .= ("unknown" :: String)]

-- Enumerates a static image5 table when its index domain is small.
tableEntries :: B.ByteString -> M.Map Symbol Domain -> Word32 -> AccessBytes -> Symbol -> Word32 -> Int -> Maybe [Value]
tableEntries bytes domains base width symbol k scale = do
  domain@(Domain spans) <- M.lookup symbol domains
  unless (domainSize domain <= 1024) Nothing
  mapM (\index -> do
    let address = base + ((fromIntegral index + k) `shiftL` scale)
    word <- wordAt bytes (address .&. complement 3)
    let entry = case width of
          AccessBytes 2 -> (word `shiftR` (8 * fromIntegral (address .&. 2))) .&. 0xffff
          AccessBytes 1 -> (word `shiftR` (8 * fromIntegral (address .&. 3))) .&. 0xff
          _ -> word
    pure (object ["index" .= index, "address" .= hex address, "value" .= entry])) [i | (a, b) <- spans, i <- [a .. b]]

domainJson :: Domain -> Value
domainJson (Domain spans) = J.toJSON [object ["low" .= a, "high" .= b] | (a, b) <- spans]

guardJson :: M.Map Symbol Domain -> [Atom] -> Value
guardJson domains atoms = object
  [ "symbols" .= [object ["symbol" .= symbolJson symbol, "values" .= domainJson domain] | (symbol, domain) <- M.toList domains, domain /= fullDomain]
  , "unmodelled_conditions" .= [ object ["site" .= codeText site, "condition" .= c, "holds" .= holds, "compared" .= fmap (\(left, right) -> [valueJson B.empty domains left, valueJson B.empty domains right]) operands]
    | UnmodelledCondition site (A.Cond c) holds operands <- atoms ]
  ]

storeJson :: B.ByteString -> M.Map Symbol Domain -> Store -> Value
storeJson bytes domains (Store site address width value) = object
  [ "site" .= codeText site, "address" .= hex address, "width" .= show width, "value" .= valueJson bytes domains value
  , "cell" .= fmap (\(ControllerId ident, field) -> object ["controller" .= ident, "field" .= show field]) (cellOf address) ]

callJson :: B.ByteString -> M.Map Symbol Domain -> Call -> Value
callJson bytes domains (Call site target arguments) = object ["site" .= codeText site, "callee" .= codeText target, "arguments" .= map (valueJson bytes domains) arguments]

-- The event value the route writes into the dispatched controller's cell.
routeEvent :: Route -> Maybe (ControllerId, Maybe Val)
routeEvent route = case routeTarget route of
  Known ident -> Just (ControllerId ident, lastEvent ident)
  _ -> Nothing
  where
    lastEvent ident = case [value | Store _ address A.Half value <- reverse (routeStores route), address == eventBase + 2 * ident] of
      value : _ -> Just value
      [] -> Nothing

-- Whether an incoming argument selects the route: its guard, unmodelled
-- compares, target or event value.
dependsOnArguments :: Route -> Bool
dependsOnArguments route = any argument (M.keys (routeDomains route)) || any argumentValue (routeTarget route : eventValues ++ compared)
  where
    argument (ArgumentSymbol _) = True
    argument _ = False
    argumentValue value = case value of
      Offset symbol _ -> argument symbol
      TableEntry _ _ symbol _ _ -> argument symbol
      _ -> False
    eventValues = [value | Just (_, Just value) <- [routeEvent route]]
    compared = concat [[left, right] | UnmodelledCondition _ _ _ (Just (left, right)) <- routeAtoms route]

main :: IO ()
main = do
  arguments <- getArgs
  let required name = case dropWhile (/= name) arguments of _ : value : _ -> pure value; _ -> fail ("missing " ++ name)
  outputPath <- required "--out"
  exists <- doesPathExist outputPath
  when exists (fail "output already exists")
  imagePath <- required "--id5"
  manifestPath <- required "--manifest"
  bytes <- B.readFile imagePath
  unless (B.length bytes == 5185120 && sha256Hex (sha256 bytes) == "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a") (fail "image5 identity differs")
  manifestBytes <- B.readFile manifestPath
  Manifest controllers functions <- either fail pure (J.eitherDecodeStrict manifestBytes)
  let registrations = M.fromListWith S.union [(target, S.singleton ident) | ManifestController ident targets <- controllers, target <- targets]
      extents = [(entry, end) | ManifestFunction entry (Just end) <- functions]
      containing site = [(entry, M.findWithDefault S.empty entry registrations) | (entry, end) <- extents, entry <= site, site < end]
      routerAddresses = S.fromList [address | Router (CodeAddress address) <- routers]
      imageEnd = imageBase + fromIntegral (B.length bytes)
      callers =
        [ (Router (CodeAddress target), CodeAddress address, link, containing address)
        | address <- [imageBase, imageBase + 4 .. imageEnd - 4]
        , Just word <- [wordAt bytes address]
        , word `shiftR` 28 /= 15
        , A.Branch _ link target <- [A.decode address word]
        , S.member target routerAddresses ]
      explorations = [(router, explore bytes router) | router <- routers]
      aggregates = [(router, key, aggregate) | (router, exploration) <- explorations, (key, aggregate) <- M.toList (explorationRoutes exploration)]
      routerTargets = M.fromListWith (\(a, x) (b, y) -> (S.union a b, x || y))
        [ ((router, ControllerId ident), case event of
            Just (Known value) -> (S.singleton value, False)
            _ -> (S.empty, True))
        | (router, (_, Known ident, event), _) <- aggregates ]
      sourceControllers = S.fromList [(router, source) | (router, _, _, owners) <- callers, (_, sources) <- owners, source <- S.toList sources]
      edges = M.fromList
        [ ((source, router, target), summary)
        | (router, source) <- S.toList sourceControllers
        , ((router', target), summary) <- M.toList routerTargets, router' == router ]
      aggregateJson ((_, target, event), aggregate) = object
        [ "target_controller" .= valueJson bytes (aggDomains aggregate) target
        , "event_value" .= maybe (object ["kind" .= ("not-written-on-path" :: String)]) (valueJson bytes (aggDomains aggregate)) event
        , "paths" .= aggPaths aggregate
        , "guard_union" .= [object ["symbol" .= symbolJson symbol, "values" .= domainJson domain] | (symbol, domain) <- M.toList (aggDomains aggregate), domain /= fullDomain]
        , "symbols_constrained_on_some_path" .= map symbolJson (S.toList (aggSymbols aggregate))
        , "paths_with_unmodelled_conditions" .= aggUnmodelled aggregate
        , "depends_on_incoming_arguments" .= aggArguments aggregate
        , "query_contingent" .= [symbolJson symbol | symbol@(QuerySymbol {}) <- S.toList (aggSymbols aggregate)]
        , "sample_path" .= let route = aggSample aggregate in object
            [ "guard" .= guardJson (routeDomains route) (routeAtoms route)
            , "cell_writes" .= [storeJson bytes (routeDomains route) store | store@(Store _ address _ _) <- routeStores route, cellOf address /= Nothing]
            , "other_constant_stores" .= [storeJson bytes (routeDomains route) store | store@(Store _ address _ _) <- routeStores route, cellOf address == Nothing]
            , "calls_before" .= map (callJson bytes (routeDomains route)) (routeCalls route) ]
        ]
      routerJson (router@(Router entry), exploration) = object
        [ "entry" .= codeText entry
        , "paths_reaching_dispatch" .= sum (map aggPaths (M.elems (explorationRoutes exploration)))
        , "steps" .= explorationSteps exploration
        , "dispatch_sites" .= [ object ["site" .= codeText site, "routes" .= map aggregateJson members]
          | (site, members) <- M.toList (M.fromListWith (flip (++)) [(site, [(key, aggregate)]) | (key@(site, _, _), aggregate) <- M.toList (explorationRoutes exploration)]) ]
        , "endings" .= object
            [ "returns" .= explorationReturns exploration
            , "returns_without_dispatch" .= [ object ["site" .= codeText site, "paths" .= paths, "guard_union" .= [object ["symbol" .= symbolJson symbol, "values" .= domainJson domain] | (symbol, domain) <- M.toList domains, domain /= fullDomain]]
                | (site, ReturnAggregate paths domains) <- M.toList (explorationUndispatched exploration) ]
            , "loop_bounds" .= map codeText (explorationLoopBounds exploration)
            , "unresolved" .= [object ["site" .= codeText site, "reason" .= reason] | (site, reason) <- explorationUnresolved exploration] ]
        , "callers" .= length [() | (r, _, _, _) <- callers, r == router]
        ]
      result = object
        [ "schema_version" .= (1 :: Int)
        , "firmware" .= ("Fujifilm X-M1 1.50" :: String)
        , "evidence_class" .= ("static-inference: bounded path-sensitive symbolic execution of pinned image5 bytes; no guest execution" :: String)
        , "inputs" .= object ["image5" .= imagePath, "image5_sha256" .= sha256Hex (sha256 bytes), "manifest" .= manifestPath, "manifest_sha256" .= sha256Hex (sha256 manifestBytes)]
        , "method" .= object
            [ "dispatcher" .= codeText dispatcher
            , "state_cells" .= hex stateBase, "event_cells" .= hex eventBase, "controllers" .= controllerCount
            , "visit_limit_per_path" .= visitLimit, "step_limit" .= stepLimit
            , "premises" .= [stableMemoryPremise, queryPremise, "image5-literals-static: PC-relative literal words and enumerated table entries read from the pinned image5 bytes are their runtime values" :: String]
            , "query_arity" .= [object ["callee" .= codeText callee, "arguments" .= arity] | (callee, arity) <- M.toList queryArity]
            , "callee_model" .= ("calls clobber r0-r3, r12, LR and flags; r0 becomes the call's result symbol (a query-result symbol for callees in query_arity); the dispatcher call also forgets recorded memory, other calls keep it under the stable-memory premise; stores through unknown non-frame addresses forget recorded memory" :: String)
            , "callers" .= ("every aligned non-0xF-condition A32 B/BL word in image5 targeting a router; data words that decode as branches are candidates, attributed to a manifest callback only when inside its structural code extent" :: String) ]
        , "routers" .= map routerJson explorations
        , "callers" .= [ object
            [ "router" .= codeText entry, "site" .= codeText site, "link" .= link
            , "callbacks" .= [object ["callback" .= hex callback, "controllers" .= [ident | ControllerId ident <- S.toList owners]] | (callback, owners) <- owned] ]
          | (Router entry, site, link, owned) <- callers ]
        , "router_targets" .= [ object ["router" .= codeText entry, "target" .= target, "events" .= S.toList events, "has_nonconstant_event" .= nonconstant]
          | ((Router entry, ControllerId target), (events, nonconstant)) <- M.toList routerTargets ]
        , "graph" .= [ object ["source" .= source, "router" .= codeText entry, "target" .= target, "events" .= S.toList events, "has_nonconstant_event" .= nonconstant]
          | ((ControllerId source, Router entry, ControllerId target), (events, nonconstant)) <- M.toList edges ]
        , "counts" .= object
            [ "caller_sites" .= length callers
            , "caller_sites_in_callbacks" .= length [() | (_, _, _, owned) <- callers, not (null owned)]
            , "route_aggregates" .= length aggregates
            , "paths_reaching_dispatch" .= sum [aggPaths aggregate | (_, _, aggregate) <- aggregates]
            , "route_aggregates_with_constant_target" .= length [() | (_, (_, Known _, _), _) <- aggregates]
            , "route_aggregates_with_constant_event" .= length [() | (_, (_, _, Just (Known _)), _) <- aggregates]
            , "route_aggregates_depending_on_arguments" .= length [() | (_, _, aggregate) <- aggregates, aggArguments aggregate]
            , "dispatch_sites" .= S.size (S.fromList [site | (_, (site, _, _), _) <- aggregates])
            , "graph_edges" .= M.size edges
            , "source_controllers" .= S.size (S.fromList [s | (s, _, _) <- M.keys edges])
            , "target_controllers" .= S.size (S.fromList [t | (_, _, t) <- M.keys edges]) ]
        , "limits" .= ([ "Routing depends on global words the routers load, not on the caller; every caller controller is joined to every route of the router it calls. Guards state which loaded values select each route."
                       , "Guards over memory symbols assume the stable-memory premise; call-result guards name the callee and site but not its semantics."
                       , "Byte-pattern caller candidates outside manifest callback extents are reported unattributed, not as proven callers."
                       , "No dynamic reachability, mailbox producer or event-value consumer semantics are established." ] :: [String])
        ]
  mapM_ (\(Router entry, exploration) -> putStrLn (codeText entry ++ ": aggregates=" ++ show (M.size (explorationRoutes exploration)) ++ " steps=" ++ show (explorationSteps exploration) ++ " unresolved=" ++ show (length (explorationUnresolved exploration)) ++ " loops=" ++ show (length (explorationLoopBounds exploration)))) explorations
  L.writeFile outputPath (J.encode result)
  putStrLn ("wrote " ++ outputPath)
