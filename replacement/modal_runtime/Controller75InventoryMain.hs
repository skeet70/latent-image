{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- Inventory of modal controller 75 for the pilot design: its tables, action
-- callbacks, state/event writers, routes in and out, retained services and a
-- mechanical three-layer disposition of every callback.
module Main where

import qualified A32 as A
import Control.Monad (forM_, unless, when)
import Data.Aeson (FromJSON (..), Value, object, (.:), (.:?), (.=))
import qualified Data.Aeson as J
import Data.Aeson.Types (Parser)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.Bits (complement, shiftL, shiftR, (.&.), (.|.))
import Data.Int (Int16)
import Data.List (sortOn)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe)
import qualified Data.Set as S
import Data.Word (Word16, Word32)
import Emulation.Sha256 (sha256, sha256Hex)
import Numeric (readHex, showHex)
import Replacement.ModalRuntime.InstructionForm (DecoderStatus, FormName (..), Occurrence (..), formDefinition, statusName)
import qualified Replacement.ModalRuntime.InstructionForm as Form
import Replacement.ModalRuntime.ServiceCategories (Category (..), Seed (..), SourceFunction (..), categoryName, seeds, sourceFunctions)
import qualified Replacement.ModalRuntime.ServiceCategories as Service
import StockInventory
import System.Directory (doesPathExist)
import System.Environment (getArgs)

-- * Typed identities

newtype ControllerId = ControllerId Word32 deriving (Eq, Ord, Show)
newtype Event = Event Word16 deriving (Eq, Ord, Show)
newtype State = State Word16 deriving (Eq, Ord, Show)
newtype ActionSlot = ActionSlot Word16 deriving (Eq, Ord, Show)
newtype CallbackId = CallbackId Word32 deriving (Eq, Ord, Show)
newtype Site = Site Word32 deriving (Eq, Ord, Show)
newtype FunctionHead = FunctionHead Word32 deriving (Eq, Ord, Show)
data CellField = StateCell | EventCell deriving (Eq, Ord, Show)
data Direction = CellLoad | CellStore deriving (Eq, Ord, Show)

pilot :: ControllerId
pilot = ControllerId 75

-- * Pinned layout

imageBase, bssStart, dispatcher, stateCells, eventCells, controllerCount :: Word32
imageBase = 0xbf1a00
bssStart = 0x1460000
dispatcher = 0xbfb028
stateCells = 0x1467588
eventCells = 0x14676e4
controllerCount = 174

-- Owned Rust bodies for stock controller-75 actions.
ownedBodies :: M.Map CallbackId (String, String)
ownedBodies = M.fromList
  [ (CallbackId 0xc72e10, ("rb_ui_set", "replacement/owned_menu/src/lifecycle.rs; report section SET, confirm and leave source increment"))
  , (CallbackId 0xc75ed4, ("rb_ui_editor_confirm", "replacement/owned_menu/src/lifecycle.rs; report section SET, confirm and leave source increment"))
  , (CallbackId 0xc76650, ("rb_ui_leave", "replacement/owned_menu/src/lifecycle.rs; report section SET, confirm and leave source increment"))
  , (CallbackId 0xc77794, ("rb_ui_editor_cancel", "replacement/owned_menu/src/lib.rs; report section Owned editor-cancel increment"))
  ]

-- (state, event) cells named by the report's retained modal contract table.
contractCells :: [(State, Event)]
contractCells = [(State 0, Event 8), (State 0x1e, Event 8), (State 0, Event 13), (State 0x1e, Event 13)]

-- * Inputs

data Table = Table String Word32 [Word32]
data Controller = Controller ControllerId Word16 [Table]
data Trajectory = Trajectory String String [(State, Event)]
data Manifest = Manifest [Controller] [Trajectory]

data Proof = Proof String [String] [String]
data Function = Function
  { functionEntry :: Word32
  , functionWindowEnd :: Word32
  , functionTails :: [Word32]
  , functionCodeBytes :: Int
  , functionLiteralWords :: Int
  , functionProof :: Proof
  , functionDirectCalls :: [(Word32, Word32, String)]
  , functionPrivateInputs :: [(Int, String)]
  , functionWitnesses :: [(String, Word32, [Word32])]
  , functionCategories :: [String]
  , functionFrontier :: [Word32]
  }
newtype Derivation = Derivation [Function]

data RecordedAccess = RecordedAccess Site Direction (Maybe (ControllerId, CellField)) (Maybe Word32) Word32
newtype ScaleCells = ScaleCells (M.Map CallbackId [RecordedAccess])

data RouterCaller = RouterCaller Site Word32
data RouterTarget = RouterTarget Word32 ControllerId [Event]
data Routes = Routes [RouterCaller] [RouterTarget]

newtype Separation = Separation (M.Map CallbackId [Site])

data ClosureCapture = ClosureCapture [Event] [State] Int [(ActionSlot, Word32)]

parseHex :: String -> Parser Word32
parseHex text = case text of
  '0' : 'x' : digits | length digits == 8, [(value, "")] <- readHex digits -> pure value
  _ -> fail ("eight-digit guest hex address required: " ++ text)

hexField :: J.Object -> J.Key -> Parser Word32
hexField fields key = parseHex =<< fields .: key

instance FromJSON ControllerId where
  parseJSON value = do
    index <- parseJSON value
    unless (index < controllerCount) (fail "controller id outside 0..173")
    pure (ControllerId index)

instance FromJSON Table where
  parseJSON = J.withObject "table" $ \fields -> Table <$> fields .: "kind" <*> hexField fields "start" <*> (mapM parseHex =<< fields .: "words")

instance FromJSON Controller where
  parseJSON = J.withObject "controller" $ \fields -> Controller <$> fields .: "id" <*> fields .: "width" <*> fields .: "tables"

instance FromJSON Trajectory where
  parseJSON = J.withObject "trajectory" $ \fields -> do
    name <- fields .: "name"
    evidence <- fields .: "evidence_id"
    domains <- fromMaybe [] <$> fields .:? "domains"
    pairs <- concat <$> mapM (J.withObject "domain" (\domain -> do
      ident <- domain .: "id"
      coordinates <- domain .: "pairs"
      pure [(State state, Event event) | ident == pilot, (state, event) <- coordinates])) domains
    pure (Trajectory name evidence pairs)

instance FromJSON Manifest where
  parseJSON = J.withObject "manifest" $ \fields -> do
    reach <- fields .: "reachability"
    Manifest <$> fields .: "controllers" <*> J.withObject "reachability" (.: "trajectories") reach

instance FromJSON Function where
  parseJSON = J.withObject "function" $ \fields -> do
    entry <- hexField fields "address"
    windowEnd <- hexField fields "window_end_exclusive"
    tails <- mapM (J.withObject "branch" (`hexField` "target")) =<< fields .: "external_branch_candidates"
    extent <- fields .: "extent"
    (size, literals, proof) <- J.withObject "extent" (\e -> do
      size <- e .: "code_bytes"
      literals <- e .: "literal_words" :: Parser [String]
      proof <- J.withObject "return proof" (\p -> Proof <$> p .: "status" <*> (fromMaybe [] <$> p .:? "premises") <*> (fromMaybe [] <$> p .:? "premise_uses")) =<< e .: "return_proof"
      pure (size, length literals, proof)) extent
    calls <- mapM (J.withObject "call" (\c -> (,,) <$> hexField c "site" <*> hexField c "target" <*> c .: "status")) =<< fields .: "direct_calls"
    privates <- mapM (J.withObject "private input" (\p -> (,) <$> p .: "line" <*> p .: "token")) =<< fields .: "private_abi_source_inputs"
    witnesses <- mapM (J.withObject "witness" (\w -> (,,) <$> w .: "category" <*> hexField w "service" <*> (mapM parseHex =<< w .: "path"))) =<< fields .: "service_witnesses"
    categories <- fields .: "service_categories"
    frontier <- mapM parseHex =<< fields .: "unclassified_frontier"
    pure (Function entry windowEnd tails size literals proof calls privates witnesses categories frontier)

instance FromJSON Derivation where
  parseJSON = J.withObject "derivation" $ \fields -> Derivation <$> fields .: "functions"

instance FromJSON ScaleCells where
  parseJSON = J.withObject "scale cells" $ \fields -> do
    index <- fields .: "callback_index"
    entries <- mapM (J.withObject "callback" (\c -> do
      entry <- hexField c "address"
      accesses <- mapM access =<< c .: "controller_cell_accesses"
      pure (CallbackId entry, accesses))) index
    pure (ScaleCells (M.fromList entries))
   where
    access = J.withObject "cell access" $ \a -> do
      site <- hexField a "site"
      direction <- a .: "direction" >>= \text -> case text :: String of
        "CellStore" -> pure CellStore
        "CellLoad" -> pure CellLoad
        _ -> fail "unknown cell access direction"
      controller <- a .:? "controller"
      fieldName <- a .:? "field"
      cell <- case (controller, fieldName :: Maybe String) of
        (Just ident, Just "StateCell") -> pure (Just (ident, StateCell))
        (Just ident, Just "EventCell") -> pure (Just (ident, EventCell))
        (Nothing, Nothing) -> pure Nothing
        _ -> fail "unknown cell field"
      value <- traverse parseHex =<< a .:? "value"
      condition <- a .: "condition"
      pure (RecordedAccess (Site site) direction cell value condition)

instance FromJSON Routes where
  parseJSON = J.withObject "routes" $ \fields -> do
    callers <- mapM (J.withObject "caller" (\c -> RouterCaller <$> (Site <$> hexField c "site") <*> hexField c "router")) =<< fields .: "callers"
    targets <- mapM (J.withObject "router target" (\t -> RouterTarget <$> hexField t "router" <*> t .: "target" <*> (map Event <$> t .: "events"))) =<< fields .: "router_targets"
    pure (Routes callers targets)

instance FromJSON Separation where
  parseJSON = J.withObject "separation" $ \fields -> do
    sites <- fields .: "separation_sites"
    undischarged <- J.withObject "separation sites" (.: "undischarged_sites") sites
    pairs <- mapM (J.withObject "undischarged site" (\u -> (,) <$> (CallbackId <$> hexField u "callback") <*> (Site <$> hexField u "site"))) undischarged
    pure (Separation (M.fromListWith (++) [(callback, [site]) | (callback, site) <- pairs]))

newtype ClosureDiagnostic = ClosureDiagnostic ClosureCapture

instance FromJSON ClosureDiagnostic where
  parseJSON = J.withObject "closure diagnostic" $ \fields -> do
    captured <- fields .: "controllers"
    found <- mapM (J.withObject "controller" (\c -> do
      ident <- c .: "controller"
      if ident /= pilot then pure Nothing else do
        events <- map Event <$> c .: "events"
        states <- map State <$> c .: "states"
        cells <- c .: "cells"
        slots <- mapM (J.withObject "slot" (\s -> (,) <$> (ActionSlot <$> s .: "slot") <*> hexField s "target")) =<< c .: "action_slots"
        pure (Just (ClosureCapture events states cells slots)))) captured
    case [capture | Just capture <- found] of
      [capture] -> pure (ClosureDiagnostic capture)
      _ -> fail "closure diagnostic must capture controller 75 once"

-- * Image helpers

hex :: Word32 -> String
hex value = "0x" ++ replicate (8 - length digits) '0' ++ digits where digits = showHex value ""

wordAt :: B.ByteString -> Word32 -> Maybe Word32
wordAt bytes address
  | address < imageBase || toInteger address + 4 > toInteger imageBase + toInteger (B.length bytes) = Nothing
  | otherwise = Just (foldr (\index value -> (value `shiftL` 8) .|. fromIntegral (B.index bytes (fromIntegral (address - imageBase) + index))) 0 [0 .. 3])

cellOf :: Word32 -> Maybe (ControllerId, CellField)
cellOf address
  | address >= stateCells && address < stateCells + 2 * controllerCount && even (address - stateCells) = Just (ControllerId ((address - stateCells) `div` 2), StateCell)
  | address >= eventCells && address < eventCells + 2 * controllerCount && even (address - eventCells) = Just (ControllerId ((address - eventCells) `div` 2), EventCell)
  | otherwise = Nothing

-- * Straight-line value-set propagation over the whole image

-- A register's possible values: unconditional, split by a condition into the
-- values when it holds and when it fails, or known only while it holds.
data Held = Unconditional (S.Set Word32) | Split A.Cond (S.Set Word32) (S.Set Word32) | Only A.Cond (S.Set Word32)
type Registers = M.Map A.Reg Held

always :: A.Cond
always = A.Cond 14

valueLimit :: Int
valueLimit = 8

bounded :: S.Set Word32 -> Maybe (S.Set Word32)
bounded values = if S.size values > valueLimit then Nothing else Just values

-- Values under an instruction's condition. A condition other than the split's
-- own or its negation may select either side.
usable :: A.Cond -> Held -> Maybe (S.Set Word32)
usable condition held = case held of
  Unconditional values -> Just values
  Split split taken failed
    | condition == split -> Just taken
    | condition == A.negateCond split -> Just failed
    | otherwise -> bounded (S.union taken failed)
  Only only values -> if condition == only then Just values else Nothing

define :: A.Cond -> A.Reg -> Maybe (S.Set Word32) -> Registers -> Registers
define condition register written registers
  | register == A.pc = registers
  | condition == always = maybe (M.delete register registers) (\values -> M.insert register (Unconditional values) registers) (written >>= bounded)
  | otherwise = case (written >>= bounded, M.lookup register registers >>= usable (A.negateCond condition)) of
      (Just values, Just old) -> M.insert register (Split condition values old) registers
      (Just values, Nothing) -> M.insert register (Only condition values) registers
      (Nothing, Just old) -> M.insert register (Only (A.negateCond condition) old) registers
      (Nothing, Nothing) -> M.delete register registers

-- After the flags change, a split register holds one of its sides.
dropConditional :: Registers -> Registers
dropConditional = M.mapMaybe settle
 where
  settle held = case held of
    Unconditional values -> Just (Unconditional values)
    Split _ taken failed -> Unconditional <$> bounded (S.union taken failed)
    Only _ _ -> Nothing

data Effect
  = Plain
  | FrameAccess
  | LiteralLoad
  | CellAccess Direction ControllerId CellField (Maybe (S.Set Word32))
  | TableLoad Word32
  | GlobalLoad Word32
  | GlobalStore Word32
  | ComputedLoad
  | ComputedStore
  | Transfer Bool Word32
  | Return
  | IndirectTransfer
  | RejectedWord
  deriving (Eq, Show)

data EventWrite = EventWrite Site A.Cond (Maybe (S.Set Word32))
data DispatchFact = DispatchFact Site Bool (Maybe (S.Set Word32)) (M.Map ControllerId EventWrite)
data CellFact = CellFact Site Direction CellField A.Cond A.Width (Maybe (S.Set Word32))

data Sweep = Sweep
  { sweepDispatches :: [DispatchFact]
  , sweepCells :: [CellFact]
  , sweepEffects :: M.Map Word32 Effect
  }

-- Linear sweep of every aligned image word. Values reset at branch targets,
-- function heads and callback entries, after unconditional transfers and at
-- rejected words; calls clobber r0-r3, r12, LR, conditional values and the
-- segment's event writes.
sweep :: B.ByteString -> S.Set Word32 -> S.Set Word32 -> Sweep
sweep bytes joins recorded = go imageBase M.empty M.empty (Sweep [] [] M.empty)
 where
  limit = imageBase + fromIntegral (B.length bytes)
  go address registers0 events0 acc
    | address + 4 > limit = acc {sweepDispatches = reverse (sweepDispatches acc), sweepCells = reverse (sweepCells acc)}
    | otherwise =
        let (registers, events) = if S.member address joins then (M.empty, M.empty) else (registers0, events0)
            word = fromMaybe 0 (wordAt bytes address)
            instruction = A.decode address word
            (effect, registers', events', dispatch, cell) = step address instruction registers events
            acc' = acc
              { sweepDispatches = maybe id (:) dispatch (sweepDispatches acc)
              , sweepCells = maybe id (:) cell (sweepCells acc)
              , sweepEffects = if S.member address recorded then M.insert address effect (sweepEffects acc) else sweepEffects acc
              }
         in go (address + 4) registers' events' acc'
  readLiteral = wordAt bytes
  clobber = dropConditional . flip (foldr M.delete) [A.Reg 0, A.Reg 1, A.Reg 2, A.Reg 3, A.Reg 12, A.lr]
  step address instruction registers events = case instruction of
    A.MoveWide condition register value -> (Plain, define condition register (Just (S.singleton value)) registers, events, Nothing, Nothing)
    A.MoveTop condition register value ->
      let low = lookupAt condition register
       in (Plain, define condition register (S.map (\half -> (value `shiftL` 16) .|. (half .&. 0xffff)) <$> low) registers, events, Nothing, Nothing)
    A.DataProcessing condition op flags destination source operand ->
      let tests = op `elem` [A.TST, A.TEQ, A.CMP, A.CMN]
          sourceValue = if source == A.pc then Just (S.singleton (address + 8)) else lookupAt condition source
          operandValue = case operand of
            A.Immediate immediate -> Just (S.singleton immediate)
            A.ShiftedImmediate register A.LSL 0 -> if register == A.pc then Just (S.singleton (address + 8)) else lookupAt condition register
            _ -> Nothing
          result = case op of
            A.MOV -> operandValue
            A.MVN -> S.map complement <$> operandValue
            A.ADD -> combine (+) sourceValue operandValue
            A.SUB -> combine (-) sourceValue operandValue
            A.ORR -> combine (.|.) sourceValue operandValue
            A.AND -> combine (.&.) sourceValue operandValue
            _ -> Nothing
          written = if tests then registers else define condition destination result registers
          settled = if flags then dropConditional written else written
          transfer = not tests && destination == A.pc
          transferEffect = if op == A.MOV && operand == A.ShiftedImmediate A.lr A.LSL 0 then Return else IndirectTransfer
       in if transfer && condition == always then (transferEffect, M.empty, M.empty, Nothing, Nothing)
            else (if transfer then transferEffect else Plain, settled, events, Nothing, Nothing)
    A.Load condition width _ destination base offset indexing ->
      let literal = case (width, offset, indexing) of
            (A.Word, A.OffsetImmediate delta, A.OffsetOnly) | base == A.pc -> S.singleton <$> readLiteral (address + 8 + fromIntegral delta)
            _ -> Nothing
          target = effective address condition base offset indexing registers
          destinations = if width == A.Dual then [destination, nextRegister destination] else [destination]
          based = writeback base indexing registers
          loaded = foldr (\register held -> if register == destination && width == A.Word then define condition register literal held else M.delete register held) based destinations
          effect
            | A.pc `elem` destinations = if base == A.sp then Return else IndirectTransfer
            | base == A.pc = LiteralLoad
            | base == A.sp = FrameAccess
            | otherwise = case target of
                Just at | Just (ident, cellField) <- cellOf at -> CellAccess CellLoad ident cellField Nothing
                        | at >= imageBase && at < min bssStart limit -> TableLoad at
                        | otherwise -> GlobalLoad at
                Nothing -> ComputedLoad
          cell = do
            at <- target
            (ident, cellField) <- cellOf at
            unless (ident == pilot) Nothing
            pure (CellFact (Site address) CellLoad cellField condition width Nothing)
       in if A.pc `elem` destinations && condition == always then (effect, M.empty, M.empty, Nothing, cell) else (effect, loaded, events, Nothing, cell)
    A.Store condition width source base offset indexing ->
      let target = effective address condition base offset indexing registers
          value = if width == A.Dual then Nothing else S.map (mask width) <$> lookupAt condition source
          effect
            | base == A.sp = FrameAccess
            | otherwise = case target of
                Just at | Just (ident, cellField) <- cellOf at -> CellAccess CellStore ident cellField value
                        | otherwise -> GlobalStore at
                Nothing -> ComputedStore
          events' = case target >>= cellOf of
            Just (ident, EventCell) -> M.insert ident (EventWrite (Site address) condition value) events
            _ -> events
          cell = do
            at <- target
            (ident, cellField) <- cellOf at
            unless (ident == pilot) Nothing
            pure (CellFact (Site address) CellStore cellField condition width value)
       in (effect, writeback base indexing registers, events', Nothing, cell)
    A.LoadMultiple condition base _ back registerList ->
      let cleared = foldr M.delete (if back then M.delete base registers else registers) registerList
          returns = A.pc `elem` registerList
          effect
            | returns = if base == A.sp then Return else IndirectTransfer
            | base == A.sp = FrameAccess
            | otherwise = ComputedLoad
       in if returns && condition == always then (effect, M.empty, M.empty, Nothing, Nothing) else (effect, cleared, events, Nothing, Nothing)
    A.StoreMultiple _ base _ back _ -> (if base == A.sp then FrameAccess else ComputedStore, if back then M.delete base registers else registers, events, Nothing, Nothing)
    A.Branch condition link target ->
      let dispatch = if target == dispatcher then Just (DispatchFact (Site address) link (lookupAt condition (A.Reg 0)) events) else Nothing
       in if link then (Transfer True target, clobber registers, M.empty, dispatch, Nothing)
          else if condition == always then (Transfer False target, M.empty, M.empty, dispatch, Nothing)
          else (Transfer False target, registers, events, dispatch, Nothing)
    A.BranchExchange condition link register
      | link -> (IndirectTransfer, clobber registers, M.empty, Nothing, Nothing)
      | condition == always -> (if register == A.lr then Return else IndirectTransfer, M.empty, M.empty, Nothing, Nothing)
      | otherwise -> (if register == A.lr then Return else IndirectTransfer, registers, events, Nothing, Nothing)
    A.Compute _ written -> (Plain, foldr M.delete registers written, events, Nothing, Nothing)
    A.Multiply _ destination _ _ _ -> (Plain, M.delete destination registers, events, Nothing, Nothing)
    A.ZeroExtend _ destination _ _ _ -> (Plain, M.delete destination registers, events, Nothing, Nothing)
    A.ReadStatus _ destination -> (Plain, M.delete destination registers, events, Nothing, Nothing)
    A.WriteApplicationFlags _ _ -> (Plain, dropConditional registers, events, Nothing, Nothing)
    A.Hint _ -> (Plain, registers, events, Nothing, Nothing)
    A.Rejected _ -> (RejectedWord, M.empty, M.empty, Nothing, Nothing)
   where
    lookupAt condition register = M.lookup register registers >>= usable condition
  combine f left right = do
    xs <- left
    ys <- right
    let values = S.fromList [f x y | x <- S.toList xs, y <- S.toList ys]
    if S.size values > valueLimit then Nothing else Just values
  nextRegister (A.Reg number) = A.Reg (number + 1)
  writeback base indexing registers = if indexing == A.OffsetOnly then registers else M.delete base registers
  mask width value = case width of A.Byte -> value .&. 0xff; A.Half -> value .&. 0xffff; _ -> value
  single values = case S.toList values of [value] -> Just value; _ -> Nothing
  effective address condition base offset indexing registers = do
    baseValue <- if base == A.pc then Just (address + 8) else M.lookup base registers >>= usable condition >>= single
    case (offset, indexing) of
      (_, A.PostIndexed) -> Just baseValue
      (A.OffsetImmediate delta, _) -> Just (baseValue + fromIntegral delta)
      (A.OffsetRegister added register A.LSL amount, _) -> do
        index <- M.lookup register registers >>= usable condition >>= single
        let scaled = index `shiftL` amount
        Just (if added then baseValue + scaled else baseValue - scaled)
      _ -> Nothing

-- * Layers

data GenericKind = ConstantResult | StateTransition | RouteForward | TableLookup deriving (Eq, Ord, Show)
data StockReason = DeviceCaptureService | PersistenceService | PrivateAbiConsumer | UnresolvedComputedReference | UnclassifiedFrontier deriving (Eq, Ord, Show, Enum, Bounded)
data Layer = GenericInstance GenericKind | ResidualTranslation | RetainedStock StockReason deriving (Eq, Ord, Show)

layerName :: Layer -> String
layerName layer = case layer of
  GenericInstance _ -> "a-generic-instance"
  ResidualTranslation -> "b-residual-translation"
  RetainedStock _ -> "c-retained-stock"

genericName :: GenericKind -> String
genericName kind = case kind of
  ConstantResult -> "constant-or-argument-result"
  StateTransition -> "own-state-transition"
  RouteForward -> "constant-route"
  TableLookup -> "image-table-lookup"

reasonName :: StockReason -> String
reasonName reason = case reason of
  DeviceCaptureService -> "device-capture-playback-service"
  PersistenceService -> "persistence-service"
  PrivateAbiConsumer -> "private-abi-consumer-lead"
  UnresolvedComputedReference -> "unresolved-computed-reference"
  UnclassifiedFrontier -> "unclassified-frontier"

layerRules :: [String]
layerRules =
  [ "Generic primitives: dispatcher 0x00bfb028, the routes.json routers, and event routers: decompiled functions other than callbacks whose every dispatcher site dispatches the same single constant controller with an unconditional event-cell write of known values in the same straight-line segment."
  , "Direct callee closure: the controller-manifest service closure (ServiceCategories.classify) started at the callee; a call-free decompiled callee that is neither a seed nor a callback target is its own frontier."
  , "(c) is decided first. Reasons, all recorded, primary in this order: device-capture-playback-service (a non-generic callee's closure reaches a capture-playback-device seed); persistence-service (reaches a persistence seed); private-abi-consumer-lead (manifest private_abi_source_inputs nonempty); unresolved-computed-reference (an undischarged stack-separation site, an indirect transfer or a rejected word in the callback); unclassified-frontier (a non-generic callee's closure has a nonempty frontier)."
  , "(a) without a (c) reason: every call and tail call targets a generic primitive; every store is a stack-frame store or a controller-75 state-cell store with a known value set; every load is a literal, stack-frame, controller-75 cell or image-data (below bss) load. Kind: constant-route if it calls a generic primitive, else own-state-transition if it stores the state cell, else image-table-lookup if it loads image data, else constant-or-argument-result."
  , "(b) otherwise."
  , "Generic primitives' own bindings do not enter callback reasons: the design owns them once. Their closures are in generic_primitives and their frontier per callback in primitive_binding_frontier."
  , "The callback's own seed status and decompilation presence are not used: a callback that is itself a seed (0xc70528) is classified by its callees, and a callback absent from the decompilation has no private-ABI source tokens to check (field decompiled)."
  , "Existing owned Rust bodies are classified by the same rules as stock bodies and reported again under owned_bodies."
  ]

-- * Main

data Analysis = Analysis
  { analysisFunction :: Function
  , analysisCode :: [Word32]
  , analysisOccurrences :: [Occurrence]
  , analysisEffects :: [(Word32, Effect)]
  , analysisCallees :: S.Set Word32
  , analysisReasons :: [StockReason]
  , analysisLayer :: Layer
  }

callbackOf :: Analysis -> CallbackId
callbackOf = CallbackId . functionEntry . analysisFunction

data Closure = Closure [(Category, Word32, [Word32])] (S.Set Word32)

pinned :: String -> String -> B.ByteString -> IO ()
pinned label digest bytes = unless (sha256Hex (sha256 bytes) == digest) (fail (label ++ " identity differs"))

decodeOrFail :: FromJSON a => String -> B.ByteString -> IO a
decodeOrFail label bytes = either (\problem -> fail (label ++ ": " ++ problem)) pure (J.eitherDecodeStrict bytes)

valuesJson :: Maybe (S.Set Word32) -> Value
valuesJson = maybe J.Null (J.toJSON . S.toList)

main :: IO ()
main = do
  arguments <- getArgs
  let required name = case dropWhile (/= name) arguments of _ : value : _ -> pure value; _ -> fail ("missing " ++ name)
  outputPath <- required "--out"
  exists <- doesPathExist outputPath
  when exists (fail "output already exists")
  manifestPath <- required "--manifest"
  derivationPath <- required "--derivation"
  cellsPath <- required "--cells"
  routesPath <- required "--routes"
  separationPath <- required "--separation"
  closurePath <- required "--closure-diagnostic"
  imagePath <- required "--id5"
  sourcePath <- required "--decompilation"
  manifestBytes <- B.readFile manifestPath
  pinned "manifest" "f27b35628371e150cdc0cfea84b11424b52c8022f7604f832a04bb890273cd67" manifestBytes
  derivationBytes <- B.readFile derivationPath
  pinned "derivation" "2190bc890153640b101de810bfb8c57fb3f3ee6824a6f34297b270e78d091d90" derivationBytes
  cellsBytes <- B.readFile cellsPath
  pinned "scale cells" "f69a020dede2d506ce847a411a22af13bd9eaf970b7c924d6dc074e8fdf5967a" cellsBytes
  routesBytes <- B.readFile routesPath
  pinned "routes" "2fb80d955a8acc3757b52e9b9d4ea401cb5cd6ef68555f177195af9f1bede7b6" routesBytes
  separationBytes <- B.readFile separationPath
  pinned "separation" "b5e0f0e8ef67c1ebb54bb79727b850aff15eb71adb9d01606f915f5edb26d0a1" separationBytes
  closureBytes <- B.readFile closurePath
  pinned "closure diagnostic" "b7c71e0c67d08f9a739411acf8e099dfff277c6bf31f61af5c859756650586bd" closureBytes
  bytes <- B.readFile imagePath
  unless (B.length bytes == 5185120) (fail "image5 size differs")
  pinned "image5" "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a" bytes
  sourceBytes <- B.readFile sourcePath
  pinned "decompilation" "0e979effa216c9a096afcac109674cdcc42dc830a0669cdc9fdf7cbbdf03053f" sourceBytes
  sourceText <- readFile sourcePath
  Manifest controllers trajectories <- decodeOrFail "manifest" manifestBytes
  Derivation functions <- decodeOrFail "derivation" derivationBytes
  ScaleCells scale <- decodeOrFail "scale cells" cellsBytes
  Routes routerCallers routerTargets <- decodeOrFail "routes" routesBytes
  Separation separation <- decodeOrFail "separation" separationBytes
  ClosureDiagnostic (ClosureCapture closureEvents closureStates closureCells closureSlots) <- decodeOrFail "closure diagnostic" closureBytes
  let decompiled = sourceFunctions sourceText
      functionMap = M.fromList [(functionEntry function, function) | function <- functions]
      callbackTargets = S.fromList [GuestAddress entry | entry <- M.keys functionMap]
      actionTargets (Controller _ _ tables) = [target | Table "Actions" _ targets <- tables, target <- targets, target /= 0]
      owner = M.fromListWith S.union [(CallbackId target, S.singleton ident) | controller@(Controller ident _ _) <- controllers, target <- actionTargets controller]
  (width, cellWords, actionWords, transitionWords) <- case [(w, tables) | Controller ident w tables <- controllers, ident == pilot] of
    [(w, tables)] -> case ([ws | Table "Cells" _ ws <- tables], [ws | Table "Actions" _ ws <- tables], [ws | Table "Transitions" _ ws <- tables]) of
      ([cs], [as], [ts]) -> pure (w, cs, as, ts)
      _ -> fail "controller 75 tables differ from three kinds"
    _ -> fail "controller 75 missing"
  unless (width > 0 && length cellWords `mod` fromIntegral width == 0) (fail "cell table is not whole event rows")
  unless (length functions == M.size functionMap && M.size functionMap == 5795) (fail "derivation census differs")
  -- Code words of every callback, for attribution and per-callback effects.
  inspections <- either fail pure (mapM (\function -> do
    inspection <- inspectFunction xm1TaskRecords (GuestAddress imageBase) bytes (FunctionWindow (GuestAddress (functionEntry function)) (GuestAddress (functionWindowEnd function))) (map GuestAddress (functionTails function))
    pure (functionEntry function, [address | GuestAddress address <- inspectedCodeWords inspection])) functions)
  let codeOf = M.fromList inspections
      wordOwner = M.fromList [(address, CallbackId entry) | (entry, code) <- inspections, address <- code]
      events = fromIntegral (length cellWords `div` fromIntegral width) :: Word16
      decodedCells =
        [ (State (fromIntegral (index `mod` fromIntegral width)), Event (fromIntegral (index `div` fromIntegral width)), ActionSlot (fromIntegral word), fromIntegral (word `shiftR` 16) :: Int16)
        | (index, word) <- zip [0 :: Int ..] cellWords ]
      actionAt (ActionSlot slot) = if fromIntegral slot < length actionWords then Just (actionWords !! fromIntegral slot) else Nothing
      transitionAt high = if high > 0 && fromIntegral high < length transitionWords then Just (transitionWords !! fromIntegral high) else Nothing
      cellAt (State state) (Event event) =
        let index = fromIntegral event * fromIntegral width + fromIntegral state :: Int
         in if state < width && index < length decodedCells then Just (decodedCells !! index) else Nothing
      callbackSlots = M.fromListWith (++) [(CallbackId target, [ActionSlot slot]) | (slot, target) <- zip [0 ..] actionWords, target /= 0]
      callbacks = M.keys callbackSlots
      callbackCells = M.fromListWith (++) [(CallbackId target, [(state, event)]) | (state, event, slot, _) <- decodedCells, Just target <- [actionAt slot], target /= 0]
      recordedWords = S.fromList (concat [M.findWithDefault [] entry codeOf | CallbackId entry <- callbacks])
      joins = S.unions
        [ S.fromList [target | address <- [imageBase, imageBase + 4 .. imageBase + fromIntegral (B.length bytes) - 4], Just word <- [wordAt bytes address], A.Branch _ False target <- [A.decode address word]]
        , S.fromList [entry | GuestAddress entry <- M.keys decompiled]
        , S.fromList (M.keys functionMap) ]
      swept = sweep bytes joins recordedWords
      headOf site = fmap (\(GuestAddress entry, _) -> FunctionHead entry) (M.lookupLE (GuestAddress site) decompiled)
      attribution (Site site) = case M.lookup site wordOwner of
        Just callback@(CallbackId entry) -> object ["callback" .= hex entry, "controllers" .= [ident | ControllerId ident <- S.toList (M.findWithDefault S.empty callback owner)]]
        Nothing -> object ["source_function_head" .= fmap (\(FunctionHead entry) -> hex entry) (headOf site), "callback" .= J.Null]
      -- Dispatcher sites grouped by the nearest preceding decompiled head.
      dispatchByHead = M.fromListWith (++) [(headOf site, [fact]) | fact@(DispatchFact (Site site) _ _ _) <- sweepDispatches swept]
      constantTargets facts = S.unions [values | DispatchFact _ _ (Just values) _ <- facts]
      eventRouter (Just (FunctionHead entry)) facts =
        let calls = maybe S.empty sourceCalls (M.lookup (GuestAddress entry) decompiled)
            constantDispatch (DispatchFact _ _ targets writes) = case fmap S.toList targets of
              Just [ident] | Just (EventWrite _ condition (Just _)) <- M.lookup (ControllerId ident) writes -> condition == always
              _ -> False
         in S.member (GuestAddress dispatcher) calls && not (M.member entry functionMap) && S.size (constantTargets facts) == 1 && all constantDispatch facts
      eventRouter Nothing _ = False
      routesRouters = S.fromList ([router | RouterTarget router _ _ <- routerTargets])
      eventRouters = M.fromList [(entry, facts) | (Just (FunctionHead entry), facts) <- M.toList dispatchByHead, eventRouter (Just (FunctionHead entry)) facts]
      generic = S.unions [S.singleton dispatcher, routesRouters, M.keysSet eventRouters]
      routers75 = M.filter (\facts -> S.member 75 (constantTargets facts)) eventRouters
      dispatchers75 = [fact | fact@(DispatchFact _ _ (Just values) _) <- sweepDispatches swept, S.member 75 values]
      unresolvedDispatches = [fact | fact@(DispatchFact _ _ Nothing _) <- sweepDispatches swept]
      branchSites target = [Site address | address <- [imageBase, imageBase + 4 .. imageBase + fromIntegral (B.length bytes) - 4], Just word <- [wordAt bytes address], A.Branch _ _ t <- [A.decode address word], t == target]
      closureOf callee = let (witnesses, unresolved) = Service.classify decompiled callbackTargets (GuestAddress callee)
                             callFree = case M.lookup (GuestAddress callee) decompiled of
                               Just function -> S.null (sourceCalls function) && not (S.member (GuestAddress callee) callbackTargets) && callee `notElem` [address | Seed (GuestAddress address) _ _ _ <- seeds]
                               Nothing -> False
                          in Closure [(category, address, [p | GuestAddress p <- path]) | (category, GuestAddress address, path) <- witnesses]
                               (S.fromList ([address | GuestAddress address <- S.toList unresolved] ++ [callee | callFree]))
      -- A primitive's bindings are its source calls other than the dispatcher.
      primitiveClosure primitive =
        let calls = [c | GuestAddress c <- maybe [] (S.toList . sourceCalls) (M.lookup (GuestAddress primitive) decompiled), c /= dispatcher]
            parts = map closureOf calls
         in Closure (concat [w | Closure w _ <- parts]) (S.unions [f | Closure _ f <- parts])
      primitiveFrontier analysis = S.unions [f | g <- S.toList (analysisCallees analysis), S.member g generic, let Closure _ f = primitiveClosure g]
      effectsOf entry = [(address, M.findWithDefault RejectedWord address (sweepEffects swept)) | address <- M.findWithDefault [] entry codeOf]
      calleesOf entry = let code = S.fromList (M.findWithDefault [] entry codeOf)
                         in S.fromList [target | (_, Transfer link target) <- effectsOf entry, link || not (S.member target code)]
      allCallees = S.unions [calleesOf entry | CallbackId entry <- callbacks]
      closures = M.fromList [(callee, closureOf callee) | callee <- S.toList allCallees, not (S.member callee generic)]
      reaches category callee = case M.lookup callee closures of
        Just (Closure witnesses _) -> any (\(c, _, _) -> c == category) witnesses
        Nothing -> False
      frontierOf callee = maybe S.empty (\(Closure _ frontier) -> frontier) (M.lookup callee closures)
      analyse callback@(CallbackId entry) = do
        function <- maybe (Left ("callback missing from derivation: " ++ hex entry)) Right (M.lookup entry functionMap)
        let code = M.findWithDefault [] entry codeOf
            effects = effectsOf entry
            callees = calleesOf entry
            external = S.filter (not . flip S.member generic) callees
            structural = S.fromList [target | (_, target, status) <- functionDirectCalls function, status == "structurally-reachable-instruction"]
            codeSet = S.fromList code
            internalTransfer target = S.member target codeSet
            reasons =
              [DeviceCaptureService | any (reaches CapturePlaybackDevice) (S.toList external)]
              ++ [PersistenceService | any (reaches Persistence) (S.toList external)]
              ++ [PrivateAbiConsumer | not (null (functionPrivateInputs function))]
              ++ [UnresolvedComputedReference | M.member callback separation || any (\(_, e) -> e == IndirectTransfer || e == RejectedWord) effects]
              ++ [UnclassifiedFrontier | any (not . S.null . frontierOf) (S.toList external)]
            admitted (_, e) = case e of
              Plain -> True
              FrameAccess -> True
              LiteralLoad -> True
              Return -> True
              TableLoad _ -> True
              CellAccess CellLoad ident _ _ -> ident == pilot
              CellAccess CellStore ident StateCell (Just _) -> ident == pilot
              Transfer link target -> (not link && internalTransfer target) || S.member target generic
              _ -> False
            kind
              | any (\(_, e) -> case e of Transfer _ target -> S.member target generic; _ -> False) effects = RouteForward
              | any (\(_, e) -> case e of CellAccess CellStore _ StateCell _ -> True; _ -> False) effects = StateTransition
              | any (\(_, e) -> case e of TableLoad _ -> True; _ -> False) effects = TableLookup
              | otherwise = ConstantResult
            layer = case reasons of
              reason : _ -> RetainedStock reason
              [] | all admitted effects -> GenericInstance kind
                 | otherwise -> ResidualTranslation
        unless (S.filter (/= dispatcher) structural == S.filter (/= dispatcher) (S.fromList [target | (_, Transfer True target) <- effects])) (Left ("direct calls differ from decoded BL targets at " ++ hex entry))
        pure (Analysis function code [Form.classify address word | address <- code, Just word <- [wordAt bytes address]] effects callees reasons layer)
  analyses <- either fail pure (mapM analyse callbacks)
  -- Every scale record for controller 75 matches a sweep site with a compatible value.
  let sweepCellSites = M.fromList [(site, fact) | fact@(CellFact site _ _ _ _ _) <- sweepCells swept]
  forM_ callbacks $ \callback -> forM_ (M.findWithDefault [] callback scale) $ \(RecordedAccess site direction cell value _) ->
    case cell of
      Just (ident, field) | ident == pilot -> case M.lookup site sweepCellSites of
        Just (CellFact _ direction' field' _ _ values) | direction' == direction && field' == field && maybe True (\v -> maybe False (S.member v) values) value -> pure ()
        _ -> fail ("scale cell record not reproduced at " ++ let Site s = site in hex s)
      Just _ -> fail "controller 75 callback reads or writes another controller's cell"
      Nothing -> pure ()
  let analysisMap = M.fromList [(callbackOf analysis, analysis) | analysis <- analyses]
      bytesOf = functionCodeBytes . analysisFunction
      layerOf callback = fmap analysisLayer (M.lookup callback analysisMap)
      tally selected = object ["callbacks" .= length selected, "code_bytes" .= sum (map bytesOf selected)]
      byLayer name = [analysis | analysis <- analyses, layerName (analysisLayer analysis) == name]
      residual = byLayer "b-residual-translation"
      seedAddresses = S.fromList [address | Seed (GuestAddress address) _ _ _ <- seeds]
      seedSequence analysis =
        all (\callee -> S.member callee generic || S.member callee seedAddresses) (S.toList (analysisCallees analysis))
          && not (any (\(_, e) -> case e of GlobalLoad _ -> True; GlobalStore _ -> True; ComputedLoad -> True; ComputedStore -> True; _ -> False) (analysisEffects analysis))
      formTable selected = M.fromListWith (\(s, a, b, c) (_, x, y, z) -> (s, a + x, b + y, S.union c z))
        [ (name, (status, 1 :: Int, if conditional then 1 else 0 :: Int, S.singleton (callbackOf analysis)))
        | analysis <- selected, Occurrence name status conditional <- analysisOccurrences analysis ]
      formJson selected = [ object ["form" .= name, "decoder_status" .= statusName (status :: DecoderStatus), "instructions" .= count, "conditional" .= conditional, "callbacks" .= S.size users]
                          | (FormName name, (status, count, conditional, users)) <- sortOn (\(FormName name, (_, count, _, _)) -> (negate count, name)) (M.toList (formTable selected)) ]
      seedMeaning = M.fromList [(address, (categoryName category, meaning)) | Seed (GuestAddress address) category meaning _ <- seeds]
      closureFor callee = if S.member callee generic then primitiveClosure callee else fromMaybe (Closure [] S.empty) (M.lookup callee closures)
      calleeJson users callee = let Closure witnesses frontier = closureFor callee in object
        [ "callee" .= hex callee
        , "seed" .= fmap (\(category, meaning) -> object ["category" .= category, "meaning" .= meaning]) (M.lookup callee seedMeaning)
        , "generic_primitive" .= S.member callee generic
        , "closure_basis" .= (if S.member callee generic then "primitive source calls other than the dispatcher" else "callee closure" :: String)
        , "closure_categories" .= S.toList (S.fromList [categoryName c | (c, _, _) <- witnesses])
        , "closure_witnesses" .= [object ["category" .= categoryName c, "service" .= hex s, "path" .= map hex p] | (c, s, p) <- witnesses]
        , "frontier" .= map hex (S.toList frontier)
        , "callbacks" .= users ]
      primitiveJson g = calleeJson (length [() | analysis <- analyses, S.member g (analysisCallees analysis)]) g
      bindingsFor selected = let users = M.fromListWith (+) [(callee, 1 :: Int) | analysis <- selected, callee <- S.toList (analysisCallees analysis)]
                              in [calleeJson count callee | (callee, count) <- sortOn (\(callee, count) -> (negate count, callee)) (M.toList users)]
      routerOut entry = [ (router, target, evs) | RouterCaller (Site site) router <- routerCallers, S.member site (S.fromList (M.findWithDefault [] entry codeOf)), RouterTarget r target evs <- routerTargets, r == router ]
      dispatchFunctionOut analysis = [ (callee, S.toList (constantTargets facts)) | callee <- S.toList (analysisCallees analysis), not (S.member callee routesRouters), Just facts <- [M.lookup callee eventRouters] ]
      stateWrites = [ (site, values, condition) | analysis <- analyses, (address, CellAccess CellStore ident StateCell values) <- analysisEffects analysis, ident == pilot, let site = Site address, let condition = fromMaybe always (fmap (\(CellFact _ _ _ c _ _) -> c) (M.lookup site sweepCellSites)) ]
      storedStates = S.unions [values | (_, Just values, _) <- stateWrites]
      directStates = S.fromList [fromIntegral (negate high) :: Word32 | (_, _, _, high) <- decodedCells, high <= 0]
      frontierBlocks = M.fromListWith (++) [ (leaf, [callbackOf analysis]) | analysis <- analyses, RetainedStock _ <- [analysisLayer analysis], leaf <- S.toList (S.unions [frontierOf callee | callee <- S.toList (analysisCallees analysis)]) ]
      soleFrontier analysis = case (analysisReasons analysis, S.toList (S.unions [frontierOf callee | callee <- S.toList (analysisCallees analysis)])) of
        ([UnclassifiedFrontier], [leaf]) -> Just leaf
        _ -> Nothing
      soleBlocks = M.fromListWith (+) [(leaf, 1 :: Int) | analysis <- analyses, Just leaf <- [soleFrontier analysis]]
      frontierOfCallback analysis = S.unions [frontierOf callee | callee <- S.toList (analysisCallees analysis)]
      rankedLeaves = sortOn (\(leaf, users) -> (negate (length users), leaf)) (M.toList frontierBlocks)
      cellCallback (State s, Event e) = cellAt (State s) (Event e) >>= \(_, _, slot, _) -> actionAt slot
      selectedJson pairs = [ object ["state" .= s, "event" .= e, "callback" .= fmap hex target, "layer" .= fmap layerName (layerOf . CallbackId =<< target)]
                           | (State s, Event e) <- pairs, let target = cellCallback (State s, Event e) ]
      closureStock = S.fromList [ slot | (state, event, slot, _) <- decodedCells, event `elem` closureEvents, state `elem` closureStates ]
      result = object
        [ "schema_version" .= (1 :: Int)
        , "firmware" .= ("Fujifilm X-M1 1.50" :: String)
        , "evidence_class" .= ("static-inference over pinned image5 bytes, the pinned decompilation and reviewed derived inputs; no execution, equivalence or reachability claim" :: String)
        , "inputs" .= object
            [ "manifest" .= manifestPath, "manifest_sha256" .= sha256Hex (sha256 manifestBytes)
            , "derivation" .= derivationPath, "derivation_sha256" .= sha256Hex (sha256 derivationBytes)
            , "cells" .= cellsPath, "cells_sha256" .= sha256Hex (sha256 cellsBytes)
            , "routes" .= routesPath, "routes_sha256" .= sha256Hex (sha256 routesBytes)
            , "separation" .= separationPath, "separation_sha256" .= sha256Hex (sha256 separationBytes)
            , "closure_diagnostic" .= closurePath, "closure_diagnostic_sha256" .= sha256Hex (sha256 closureBytes)
            , "image5" .= imagePath, "image5_sha256" .= sha256Hex (sha256 bytes)
            , "decompilation" .= sourcePath, "decompilation_sha256" .= sha256Hex (sha256 sourceBytes) ]
        , "controller" .= object
            [ "id" .= (75 :: Int), "width_states" .= width, "event_rows" .= events
            , "state_cell" .= hex (stateCells + 150), "event_cell" .= hex (eventCells + 150)
            , "cell_words" .= length cellWords
            , "cells_with_nonnull_action" .= length [() | (_, _, slot, _) <- decodedCells, Just target <- [actionAt slot], target /= 0]
            , "cells_with_null_action" .= length [() | (_, _, slot, _) <- decodedCells, Just 0 <- [actionAt slot]]
            , "cells_with_slot_outside_table" .= length [() | (_, _, slot, _) <- decodedCells, Nothing <- [actionAt slot]]
            , "cells_with_positive_high" .= length [() | (_, _, _, high) <- decodedCells, high > 0]
            , "positive_high_values" .= S.toList (S.fromList [high | (_, _, _, high) <- decodedCells, high > 0])
            , "nonnull_transition_entries_selected" .= length [() | (_, _, _, high) <- decodedCells, Just target <- [transitionAt high], target /= 0]
            , "transition_words" .= map hex transitionWords
            , "noop_cells" .= length [() | (_, _, slot, high) <- decodedCells, actionAt slot == Just 0, transitionAt high == Just 0]
            , "direct_state_cells" .= [object ["state" .= s, "event" .= e, "next_state" .= negate (fromIntegral high :: Int), "action_slot" .= slot, "callback" .= fmap hex (actionAt (ActionSlot slot))] | (State s, Event e, ActionSlot slot, high) <- decodedCells, high <= 0]
            , "per_event" .= [ object ["event" .= e, "nonnull_cells" .= length [() | (_, Event e', slot, _) <- decodedCells, e' == e, Just target <- [actionAt slot], target /= 0], "callbacks" .= S.size (S.fromList [target | (_, Event e', slot, _) <- decodedCells, e' == e, Just target <- [actionAt slot], target /= 0])] | e <- [0 .. events - 1] ]
            , "per_state" .= [ object ["state" .= s, "nonnull_cells" .= length [() | (State s', _, slot, _) <- decodedCells, s' == s, Just target <- [actionAt slot], target /= 0], "callbacks" .= S.size (S.fromList [target | (State s', _, slot, _) <- decodedCells, s' == s, Just target <- [actionAt slot], target /= 0])] | s <- [0 .. width - 1] ]
            ]
        , "action_slots" .= object
            [ "table_words" .= length actionWords
            , "null_slots" .= [slot | (slot, target) <- zip [0 :: Int ..] actionWords, target == 0]
            , "nonnull_slots" .= length (filter (/= 0) actionWords)
            , "unique_callbacks" .= length callbacks
            , "slots_referenced_by_cells" .= S.size (S.fromList [slot | (_, _, slot, _) <- decodedCells])
            , "unreferenced_slots" .= [slot | slot <- [0 .. fromIntegral (length actionWords) - 1 :: Word16], not (S.member (ActionSlot slot) (S.fromList [s | (_, _, s, _) <- decodedCells]))]
            , "callbacks_with_several_slots" .= [object ["callback" .= hex entry, "slots" .= [s | ActionSlot s <- slots]] | (CallbackId entry, slots) <- M.toList callbackSlots, length slots > 1]
            , "closure_capture_reconciliation" .= object
                [ "closure_events" .= [e | Event e <- closureEvents], "closure_states" .= length closureStates, "closure_cells" .= closureCells
                , "closure_action_slots" .= length closureSlots
                , "stock_slots_over_closure_pairs" .= S.size closureStock
                , "slots_whose_closure_target_differs_from_stock" .= [object ["slot" .= s, "closure" .= hex t, "stock" .= fmap hex (actionAt (ActionSlot s))] | (ActionSlot s, t) <- closureSlots, actionAt (ActionSlot s) /= Just t]
                , "stock_slots_over_closure_pairs_absent_from_closure" .= [s | ActionSlot s <- S.toList (closureStock `S.difference` S.fromList (map fst closureSlots))] ] ]
        , "state_writes" .= object
            [ "callback_state_store_sites" .= length stateWrites
            , "conditional_sites" .= length [() | (_, _, condition) <- stateWrites, condition /= always]
            , "sites_with_unknown_value" .= length [() | (_, Nothing, _) <- stateWrites]
            , "callbacks_storing_state" .= S.size (S.fromList [callbackOf analysis | analysis <- analyses, any (\(_, e) -> case e of CellAccess CellStore ident StateCell _ -> ident == pilot; _ -> False) (analysisEffects analysis)])
            , "stored_state_values" .= S.toList storedStates
            , "stored_values_outside_width" .= [v | v <- S.toList storedStates, v >= fromIntegral width]
            , "direct_state_cell_values" .= S.toList directStates
            , "all_sweep_cell_accesses" .= [ object ["site" .= hex s, "direction" .= show direction, "field" .= show field, "condition" .= let A.Cond c = condition in c, "width" .= show accessWidth, "values" .= valuesJson values, "attribution" .= attribution site]
                                          | CellFact site@(Site s) direction field condition accessWidth values <- sweepCells swept ]
            ]
        , "routes_in" .= object
            [ "dispatching_functions" .= [ object
                  [ "entry" .= hex entry, "controllers" .= S.toList (constantTargets facts)
                  , "event_router" .= M.member entry eventRouters
                  , "source_calls" .= maybe [] (\function -> [hex a | GuestAddress a <- S.toList (sourceCalls function)]) (M.lookup (GuestAddress entry) decompiled)
                  , "dispatch_sites" .= [ object ["site" .= hex s, "link" .= link, "controllers" .= valuesJson targets, "event_write" .= fmap (\(EventWrite (Site w) c v) -> object ["site" .= hex w, "condition" .= let A.Cond n = c in n, "values" .= valuesJson v]) (M.lookup pilot writes)] | DispatchFact (Site s) link targets writes <- facts ]
                  , "events" .= S.toList (S.unions [fromMaybe S.empty v | DispatchFact _ _ _ writes <- facts, Just (EventWrite _ _ v) <- [M.lookup pilot writes]])
                  , "callers" .= [object ["site" .= hex s, "attribution" .= attribution site] | site@(Site s) <- branchSites entry] ]
                | (Just (FunctionHead entry), facts) <- M.toList dispatchByHead, S.member 75 (constantTargets facts) ]
            , "dispatch_sites_for_75" .= length dispatchers75
            , "dispatch_sites_for_75_outside_event_routers" .= [object ["site" .= hex s, "attribution" .= attribution site] | DispatchFact site@(Site s) _ _ _ <- dispatchers75, maybe True (\(FunctionHead h) -> not (M.member h routers75)) (headOf s)]
            , "event_values_covered" .= S.toList (S.unions [fromMaybe S.empty v | DispatchFact _ _ _ writes <- dispatchers75, Just (EventWrite _ _ v) <- [M.lookup pilot writes]])
            , "dispatch_sites_total" .= length (sweepDispatches swept)
            , "event_routers_global" .= [object ["entry" .= hex entry, "controller" .= S.toList (constantTargets facts), "dispatch_sites" .= length facts] | (entry, facts) <- M.toList eventRouters]
            , "dispatch_sites_with_unresolved_controller" .= [object ["site" .= hex s, "attribution" .= attribution site] | DispatchFact site@(Site s) _ _ _ <- unresolvedDispatches]
            , "routes_json_router_targets" .= [object ["router" .= hex r, "target" .= t, "events" .= length evs] | RouterTarget r (ControllerId t) evs <- routerTargets]
            ]
        , "routes_out" .= object
            [ "router_edges" .= [ object ["router" .= hex router, "target" .= t, "events" .= [e | Event e <- evs], "callbacks" .= n]
                                | ((router, ControllerId t, evs), n) <- M.toList (M.fromListWith (+) [((router, target, evs), 1 :: Int) | analysis <- analyses, (router, target, evs) <- S.toList (S.fromList (routerOut (functionEntry (analysisFunction analysis))))]) ]
            , "event_router_edges" .= [ object ["router" .= hex router, "targets" .= targets, "callbacks" .= n]
                                      | ((router, targets), n) <- M.toList (M.fromListWith (+) [(edge, 1 :: Int) | analysis <- analyses, edge <- dispatchFunctionOut analysis]) ]
            , "direct_dispatcher_callers" .= [hex (functionEntry (analysisFunction analysis)) | analysis <- analyses, S.member dispatcher (analysisCallees analysis)]
            ]
        , "generic_primitives" .= [ primitiveJson g | g <- S.toList generic, g == dispatcher || S.member g routesRouters || M.member g routers75 || any (S.member g . analysisCallees) analyses ]
        , "form_definition" .= formDefinition
        , "layer_rules" .= layerRules
        , "layers" .= object
            [ "a-generic-instance" .= tally (byLayer "a-generic-instance")
            , "a-by-kind" .= [object ["kind" .= genericName k, "callbacks" .= length selected, "code_bytes" .= sum (map bytesOf selected)] | k <- [ConstantResult, StateTransition, RouteForward, TableLookup], let selected = [a | a <- analyses, analysisLayer a == GenericInstance k]]
            , "b-residual-translation" .= tally residual
            , "c-retained-stock" .= tally (byLayer "c-retained-stock")
            , "c-by-primary-reason" .= [object ["reason" .= reasonName r, "callbacks" .= length selected, "code_bytes" .= sum (map bytesOf selected)] | r <- [minBound .. maxBound], let selected = [a | a <- analyses, analysisLayer a == RetainedStock r]]
            , "callbacks_with_reason" .= [object ["reason" .= reasonName r, "callbacks" .= length selected, "code_bytes" .= sum (map bytesOf selected)] | r <- [minBound .. maxBound], let selected = [a | a <- analyses, r `elem` analysisReasons a]]
            , "c-only-reason-frontier" .= tally [a | a <- analyses, analysisReasons a == [UnclassifiedFrontier]]
            , "b-seed-sequence" .= tally [a | a <- residual, seedSequence a]
            , "a-with-primitive-binding-frontier" .= tally [a | a <- byLayer "a-generic-instance", not (S.null (primitiveFrontier a))]
            , "b-with-primitive-binding-frontier" .= tally [a | a <- residual, not (S.null (primitiveFrontier a))]
            , "total" .= tally analyses ]
        , "frontier_blockers" .= [object ["leaf" .= hex leaf, "retained_callbacks_reaching" .= length users, "callbacks_whose_only_frontier_leaf_is_this" .= M.findWithDefault 0 leaf soleBlocks] | (leaf, users) <- take 40 rankedLeaves]
        , "frontier_leaves_total" .= M.size frontierBlocks
        , "frontier_only_callbacks_covered_by_top_leaves" .= [ object ["leaves" .= k, "callbacks" .= length covered, "code_bytes" .= sum (map bytesOf covered)]
                                                             | k <- [1, 5, 10, 20, 40, M.size frontierBlocks]
                                                             , let chosen = S.fromList (map fst (take k rankedLeaves))
                                                                   covered = [a | a <- analyses, analysisReasons a == [UnclassifiedFrontier], frontierOfCallback a `S.isSubsetOf` chosen] ]
        , "frontier_direct_callees" .= [ object ["callee" .= hex callee, "retained_callbacks" .= n, "frontier_leaves" .= S.size (frontierOf callee)]
                                       | (callee, n) <- sortOn (\(callee, n) -> (negate n, callee)) (M.toList (M.fromListWith (+) [(callee, 1 :: Int) | a <- analyses, RetainedStock _ <- [analysisLayer a], callee <- S.toList (analysisCallees a), not (S.null (frontierOf callee))])) ]
        , "residual_translation_forms" .= formJson residual
        , "all_callback_forms" .= formJson analyses
        , "residual_service_bindings" .= bindingsFor residual
        , "trajectories" .= [ object ["name" .= name, "evidence_id" .= evidence, "selected" .= selectedJson pairs] | Trajectory name evidence pairs <- trajectories, not (null pairs) ]
        , "contract_cells" .= selectedJson contractCells
        , "owned_bodies" .= [ object ["callback" .= hex entry, "owned_symbol" .= symbol, "provenance" .= provenance, "layer" .= fmap layerName (layerOf callback), "reasons" .= maybe [] (map reasonName . analysisReasons) (M.lookup callback analysisMap)] | (callback@(CallbackId entry), (symbol, provenance)) <- M.toList ownedBodies ]
        , "callbacks" .= [ callbackJson analysis | analysis <- analyses ]
        , "limits" .=
            ([ "Value sets come from a straight-line linear sweep; data words decoded as code can add sites, and joins only reset known values."
             , "Calls are assumed to preserve r4-r11 (callee-preserves-r4-r11), so values in r4-r11 survive a call."
             , "Joins are non-link branch targets, decompiled heads and callback entries. Computed-transfer targets and BL targets that are not decompiled heads are not joins; no controller-75 callback has an rd=pc or rt=pc form, so this affects only whole-image router detection and dispatcher r0 sets."
             , "Attribution outside callbacks names the nearest preceding decompiled head, which is the containing function only when the site lies inside that function."
             , "Routes in are one call level deep: callers of the event routers, not their transitive callers."
             , "Return proofs are the derivation's conditional proofs with their recorded premises."
             , "Layer rules are syntactic and closure-based; they do not establish behavior equivalence or reachability."
             ] :: [String])
        ]
      callbackJson analysis =
        let function = analysisFunction analysis
            entry = functionEntry function
            Proof proofStatus premises uses = functionProof function
            callback = CallbackId entry
         in object
              [ "callback" .= hex entry
              , "slots" .= [s | ActionSlot s <- M.findWithDefault [] callback callbackSlots]
              , "cells" .= [[s, e] | (State s, Event e) <- M.findWithDefault [] callback callbackCells]
              , "code_bytes" .= functionCodeBytes function, "code_words" .= length (analysisCode analysis), "literal_words" .= functionLiteralWords function
              , "return_proof" .= object ["status" .= proofStatus, "premises" .= premises, "premise_uses" .= length uses, "undischarged_separation_sites" .= [hex s | Site s <- M.findWithDefault [] callback separation]]
              , "layer" .= layerName (analysisLayer analysis)
              , "generic_kind" .= case analysisLayer analysis of GenericInstance k -> Just (genericName k); _ -> Nothing
              , "reasons" .= map reasonName (analysisReasons analysis)
              , "decompiled" .= M.member (GuestAddress entry) decompiled
              , "primitive_binding_frontier" .= map hex (S.toList (primitiveFrontier analysis))
              , "owned_body" .= fmap fst (M.lookup callback ownedBodies)
              , "callees" .= [ object ["callee" .= hex c, "generic" .= S.member c generic, "seed_category" .= fmap fst (M.lookup c seedMeaning), "frontier" .= S.size (frontierOf c)] | c <- S.toList (analysisCallees analysis) ]
              , "state_stores" .= [ object ["site" .= hex a, "values" .= valuesJson v] | (a, CellAccess CellStore ident StateCell v) <- analysisEffects analysis, ident == pilot ]
              , "router_out" .= [ object ["router" .= hex r, "target" .= t, "events" .= length evs] | (r, ControllerId t, evs) <- S.toList (S.fromList (routerOut entry)) ]
              , "event_router_out" .= [ object ["router" .= hex r, "targets" .= ts] | (r, ts) <- dispatchFunctionOut analysis ]
              , "effects" .= object
                  [ "global_loads" .= S.size (S.fromList [a | (_, GlobalLoad a) <- analysisEffects analysis])
                  , "table_loads" .= S.size (S.fromList [a | (_, TableLoad a) <- analysisEffects analysis])
                  , "global_stores" .= map hex (S.toList (S.fromList [a | (_, GlobalStore a) <- analysisEffects analysis]))
                  , "computed_loads" .= length [() | (_, ComputedLoad) <- analysisEffects analysis]
                  , "computed_stores" .= length [() | (_, ComputedStore) <- analysisEffects analysis] ]
              , "service_categories" .= functionCategories function
              , "service_witnesses" .= [object ["category" .= c, "service" .= hex s, "path" .= map hex p] | (c, s, p) <- functionWitnesses function]
              , "unclassified_frontier" .= map hex (functionFrontier function)
              , "private_abi_source_inputs" .= [object ["line" .= l, "token" .= t] | (l, t) <- functionPrivateInputs function]
              , "conditional_instructions" .= length [() | Occurrence _ _ True <- analysisOccurrences analysis]
              , "forms" .= M.toList (M.fromListWith (+) [(name, 1 :: Int) | Occurrence (FormName name) _ _ <- analysisOccurrences analysis])
              ]
  L.writeFile outputPath (J.encode result)
  putStrLn ("wrote " ++ outputPath ++ "; callbacks=" ++ show (length analyses)
    ++ "; a=" ++ show (length (byLayer "a-generic-instance")) ++ "; b=" ++ show (length residual) ++ "; c=" ++ show (length (byLayer "c-retained-stock")))
