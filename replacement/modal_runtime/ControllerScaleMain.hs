{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

module Main where

import qualified A32 as A
import Control.Monad (unless, when)
import Data.Aeson (FromJSON (..), Value, object, (.:), (.:?), (.=))
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as L
import Data.Bits (complement, shiftL, shiftR, (.&.), (.|.))
import Data.List (sort, sortOn)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word16, Word32)
import Emulation.Sha256 (sha256, sha256Hex)
import Numeric (readHex, showHex)
import StockInventory
import System.Directory (doesPathExist)
import System.Environment (getArgs)

newtype CallbackId = CallbackId GuestAddress deriving (Eq, Ord, Show)
newtype ControllerId = ControllerId Word32 deriving (Eq, Ord, Show)
newtype ByteCount = ByteCount Int deriving (Eq, Ord, Show)
newtype ShapeHash = ShapeHash String deriving (Eq, Ord, Show)
data TableKind = Cells | Actions | Transitions deriving (Eq, Show)
data Table = Table TableKind GuestAddress [Word32]
data Controller = Controller ControllerId Word16 [Table]
data Callback = Callback CallbackId GuestAddress (S.Set GuestAddress)
data TrajectoryKind = RetainedFilm | PackagedFilm | BankEditor | NativeEditor | ColdFirstStart deriving (Eq, Show)
data Domain = Domain ControllerId [(Word16,Word16)]
data Trajectory = Trajectory TrajectoryKind (Maybe [Domain])
data CellField = StateCell | EventCell deriving (Eq, Ord, Show)
data CellRegistration = CellRegistration ControllerId GuestAddress GuestAddress
type CellMap = M.Map Word32 (ControllerId, CellField)
data AccessDirection = CellLoad | CellStore deriving (Eq, Show)
data CellAccess = CellAccess
  { cellSite :: GuestAddress
  , cellDirection :: AccessDirection
  , cellAddress :: Word32
  , cellTarget :: Maybe (ControllerId, CellField)
  , cellWidth :: A.Width
  , cellValue :: Maybe Word32
  , cellCondition :: A.Cond
  }
data Manifest = Manifest [Controller] [CellRegistration] [Callback] [Trajectory]
data ShapeMode = HelperBound | HelperErased deriving (Eq, Show)
data BranchBinding = InternalOffset Word32 | ExternalIdentity Word32 | ExternalParameter deriving (Eq, Show)
data ShapeWord = LiteralParameter | InternalCodeLiteral Word32 | Operation A.Instruction | BranchShape A.Cond Bool BranchBinding | ExactOpaque Word32 deriving (Eq, Show)
data Analysis = Analysis
  { analysisCallback :: CallbackId
  , analysisCodeBytes :: ByteCount
  , analysisStorageBytes :: ByteCount
  , analysisInstructionBytes :: ByteCount
  , analysisRawHash :: ShapeHash
  , analysisBoundHash :: ShapeHash
  , analysisErasedHash :: ShapeHash
  , analysisCalls :: [(GuestAddress, GuestAddress)]
  , analysisBoundParameterSites :: [GuestAddress]
  , analysisErasedParameterSites :: [GuestAddress]
  , analysisReturnClosed :: Bool
  , analysisCells :: [CellAccess]
  }

instance FromJSON ControllerId where
  parseJSON value = do
    index <- parseJSON value
    unless (index < 174) (fail "controller id outside0..173")
    pure (ControllerId index)

parseAddress :: String -> Either String GuestAddress
parseAddress ('0':'x':digits)
  | length digits == 8, [(address,"")] <- readHex digits = Right (GuestAddress address)
parseAddress _ = Left "eight-digit guest hex address required"

instance FromJSON Table where
  parseJSON = J.withObject "table" $ \fields -> do
    name <- fields .: "kind"
    kind <- case name :: String of "Cells" -> pure Cells; "Actions" -> pure Actions; "Transitions" -> pure Transitions; _ -> fail "unknown table kind"
    start <- either fail pure . parseAddress =<< fields .: "start"
    words' <- fields .: "words"
    values <- mapM (fmap (\(GuestAddress address) -> address) . either fail pure . parseAddress) words'
    pure (Table kind start values)

instance FromJSON Controller where
  parseJSON = J.withObject "controller" $ \fields -> Controller <$> fields .: "id" <*> fields .: "width" <*> fields .: "tables"

instance FromJSON CellRegistration where
  parseJSON = J.withObject "controller cells" $ \fields -> do
    state <- either fail pure . parseAddress =<< fields .: "state_address"
    event <- either fail pure . parseAddress =<< fields .: "event_address"
    ident <- fields .: "id"
    pure (CellRegistration ident state event)

instance FromJSON Callback where
  parseJSON = J.withObject "callback" $ \fields -> do
    entry <- either fail pure . parseAddress =<< fields .: "address"
    end <- either fail pure . parseAddress =<< fields .: "window_end_exclusive"
    branches <- fields .: "external_branch_candidates"
    tails <- mapM (J.withObject "external branch" (\branch -> either fail pure . parseAddress =<< branch .: "target")) branches
    pure (Callback (CallbackId entry) end (S.fromList tails))

instance FromJSON Domain where
  parseJSON = J.withObject "domain" $ \fields -> Domain <$> fields .: "id" <*> fields .: "pairs"

instance FromJSON Trajectory where
  parseJSON = J.withObject "trajectory" $ \fields -> do
    name <- fields .: "name"
    kind <- case name :: String of
      "retained-stock-FILMSIM-MENU-four-DOWN-OK-BACK" -> pure RetainedFilm
      "packaged-modal224-FILMSIM-repeat-BACK-live-view-handback" -> pure PackagedFilm
      "packaged-recipe-bank-SAVE-CURRENT-rename-A-cancel-B-confirm-leave-reentry" -> pure BankEditor
      "native-direct-editor-SET-confirm-or-cancel-leave" -> pure NativeEditor
      "cold-first-start-language-date-live-view-shooting-menu" -> pure ColdFirstStart
      _ -> fail "unknown trajectory kind"
    Trajectory kind <$> fields .:? "domains"

instance FromJSON Manifest where
  parseJSON = J.withObject "manifest" $ \fields -> do
    reachability <- fields .: "reachability"
    trajectories <- J.withObject "reachability" (.: "trajectories") reachability
    Manifest <$> fields .: "controllers" <*> fields .: "controllers" <*> fields .: "functions" <*> pure trajectories

imageBase :: Word32
imageBase = 0xbf1a00

hex :: Word32 -> String
hex value = "0x" ++ replicate (8-length digits) '0' ++ digits where digits = showHex value ""

addressText :: GuestAddress -> String
addressText (GuestAddress address) = hex address

callbackText :: CallbackId -> String
callbackText (CallbackId address) = addressText address

hashText :: ShapeHash -> String
hashText (ShapeHash digest) = digest

wordAt :: B.ByteString -> Word32 -> Word32
wordAt bytes address
  | address < imageBase || toInteger address+4 > toInteger imageBase+toInteger (B.length bytes) = error "image read outside input"
  | otherwise = foldr (\index value -> (value `shiftL` 8) .|. fromIntegral (B.index bytes (fromIntegral (address-imageBase)+index))) 0 [0..3]

hashWords :: [Word32] -> ShapeHash
hashWords = ShapeHash . sha256Hex . sha256 . B.pack . concatMap (\word -> [fromIntegral (word `shiftR` shift) | shift <- [0,8,16,24]])

hashShape :: [(Word32,ShapeWord)] -> ShapeHash
hashShape = ShapeHash . sha256Hex . sha256 . C.pack . show

normalize :: ShapeMode -> Word32 -> Word32 -> A.Instruction -> ShapeWord
normalize mode low high instruction = case instruction of
  A.DataProcessing condition op flags destination source operand -> Operation (A.DataProcessing condition op flags destination source (operandShape operand))
  A.MoveWide condition register _ -> Operation (A.MoveWide condition register 0)
  A.MoveTop condition register _ -> Operation (A.MoveTop condition register 0)
  A.Load condition width signed destination base offset indexing -> Operation (A.Load condition width signed destination base (offsetShape offset) indexing)
  A.Store condition width source base offset indexing -> Operation (A.Store condition width source base (offsetShape offset) indexing)
  A.Branch condition link target -> BranchShape condition link
    (if low <= target && target < high then InternalOffset (target-low) else case mode of HelperBound -> ExternalIdentity target; HelperErased -> ExternalParameter)
  other -> Operation other
 where
  operandShape (A.Immediate _) = A.Immediate 0
  operandShape other = other
  offsetShape (A.OffsetImmediate offset) = A.OffsetImmediate (signum offset)
  offsetShape other = other

analyze :: B.ByteString -> CellMap -> Callback -> Either String Analysis
analyze bytes cells (Callback ident@(CallbackId entry@(GuestAddress low)) end@(GuestAddress high) tails) = do
  let window = FunctionWindow entry end
      encoded = [(address,wordAt bytes address) | address <- [low,low+4..high-4]]
  inspection <- inspectFunction (GuestAddress imageBase) bytes window (S.toList tails)
  let code = S.fromList [address | GuestAddress address <- inspectedCodeWords inspection]
      literals = S.fromList [address | GuestAddress address <- inspectedLiteralWords inspection]
      decodeAt address word = A.decode address word
      shapeAt mode address word
        | S.member address literals && S.member word code = InternalCodeLiteral (word-low)
        | S.member address literals = LiteralParameter
        | A.Compute {} <- decodeAt address word = ExactOpaque word
        | otherwise = normalize mode low high (decodeAt address word)
      shaped mode = [(address-low,shapeAt mode address word) | (address,word) <- encoded]
      calls = [(GuestAddress address,GuestAddress target) | (address,word) <- encoded, S.member address code, A.Branch _ True target <- [decodeAt address word], target < low || target >= high]
      parameters mode = [GuestAddress address | (address,word) <- encoded, parameter (shapeAt mode address word)]
      FunctionDeclaration _ codeSize = inspectedDeclaration inspection
      FunctionSize size = codeSize
      joins = S.fromList ([target | (address,word) <- encoded, S.member address code, A.Branch _ False target <- [decodeAt address word], low <= target, target < high]
        ++ [word | (address,word) <- encoded, S.member address literals, S.member word code])
  pure (Analysis ident (ByteCount size) (ByteCount (fromIntegral (high-low))) (ByteCount (4*S.size code))
    (hashWords (map snd encoded)) (hashShape (shaped HelperBound)) (hashShape (shaped HelperErased)) calls (parameters HelperBound) (parameters HelperErased) (either (const False) (const True) (inspectedReturn inspection))
    (cellAccesses bytes cells joins [(address,word) | (address,word) <- encoded, S.member address code]))
 where
  parameter shape = case shape of
    LiteralParameter -> True
    Operation (A.DataProcessing _ _ _ _ _ (A.Immediate _)) -> True
    Operation (A.MoveWide _ _ _) -> True
    Operation (A.MoveTop _ _ _) -> True
    Operation (A.Load _ _ _ _ _ (A.OffsetImmediate offset) _) -> offset /= 0
    Operation (A.Store _ _ _ _ (A.OffsetImmediate offset) _) -> offset /= 0
    BranchShape _ _ ExternalParameter -> True
    _ -> False

always :: A.Cond
always = A.Cond 14

-- A register value holds whenever its condition holds; conditional values are dropped when flags change.
type Known = M.Map A.Reg (A.Cond, Word32)

-- Straight-line constant propagation over code words in address order. Known values reset at internal
-- branch and code-literal targets and after unconditional transfers. Calls are modeled as clobbering r0-r3, r12,
-- LR and flags and preserving r4-r11 and SP. Only accesses whose effective address is a known constant are recorded.
cellAccesses :: B.ByteString -> CellMap -> S.Set Word32 -> [(Word32,Word32)] -> [CellAccess]
cellAccesses bytes cells joins = go M.empty
 where
  spanLow = maybe 0 fst (M.lookupMin cells)
  spanHigh = maybe 0 ((+2) . fst) (M.lookupMax cells)
  go _ [] = []
  go known0 ((address,word):rest) =
    let known = if S.member address joins then M.empty else known0
        (accesses,known') = step address (A.decode address word) known
    in accesses ++ go known' rest
  readImage address
    | address >= imageBase && toInteger address+4 <= toInteger imageBase+toInteger (B.length bytes) = Just (wordAt bytes address)
    | otherwise = Nothing
  usable condition (held,value) = if held == always || held == condition then Just value else Nothing
  dropConditional = M.filter ((== always) . fst)
  transfer condition isTransfer known = if isTransfer && condition == always then M.empty else known
  clobberCall = dropConditional . flip (foldr M.delete) [A.Reg 0,A.Reg 1,A.Reg 2,A.Reg 3,A.Reg 12,A.lr]
  step address instruction known = case instruction of
    A.MoveWide condition register value -> ([],define condition register (Just value) known)
    A.MoveTop condition register value -> ([],define condition register ((\lowHalf -> (value `shiftL` 16) .|. (lowHalf .&. 0xffff)) <$> lookupAt condition register) known)
    A.DataProcessing condition op flags destination source operand ->
      let tests = op `elem` [A.TST,A.TEQ,A.CMP,A.CMN]
          sourceValue = if source == A.pc then Just (address+8) else lookupAt condition source
          value = case (op,operand) of
            (A.MOV,A.Immediate immediate) -> Just immediate
            (A.MVN,A.Immediate immediate) -> Just (complement immediate)
            (A.ADD,A.Immediate immediate) -> (+ immediate) <$> sourceValue
            (A.SUB,A.Immediate immediate) -> subtract immediate <$> sourceValue
            _ -> Nothing
          written = if tests then known else define condition destination value known
      in ([],transfer condition (not tests && destination == A.pc) (if flags then dropConditional written else written))
    A.Load condition width _ destination base offset indexing ->
      let literal = case (width,offset,indexing) of
            (A.Word,A.OffsetImmediate delta,A.OffsetOnly) | base == A.pc -> readImage (address+8+fromIntegral delta)
            _ -> Nothing
          destinations = if width == A.Dual then [destination,nextRegister destination] else [destination]
          based = writeback condition base indexing known
          loaded = foldr (\register held -> if register == destination then define condition register literal held else M.delete register held) based destinations
      in (record address CellLoad condition width (effective address condition base offset indexing known) Nothing,transfer condition (A.pc `elem` destinations) loaded)
    A.Store condition width source base offset indexing ->
      let value = if width == A.Dual then Nothing else mask width <$> lookupAt condition source
      in (record address CellStore condition width (effective address condition base offset indexing known) value,writeback condition base indexing known)
    A.LoadMultiple condition base _ back registers ->
      ([],transfer condition (A.pc `elem` registers) (foldr M.delete (if back then M.delete base known else known) registers))
    A.StoreMultiple _ base _ back _ -> ([],if back then M.delete base known else known)
    A.Branch condition link _ -> ([],if link then clobberCall known else transfer condition True known)
    A.BranchExchange condition link _ -> ([],if link then clobberCall known else transfer condition True known)
    A.Compute _ registers -> ([],foldr M.delete known registers)
    A.Multiply _ destination _ _ _ -> ([],M.delete destination known)
    A.ZeroExtend _ destination _ _ _ -> ([],M.delete destination known)
    A.ReadStatus _ destination -> ([],M.delete destination known)
    A.WriteApplicationFlags _ _ -> ([],dropConditional known)
    A.Hint _ -> ([],known)
    A.Rejected _ -> ([],M.empty)
   where
    lookupAt condition register = M.lookup register known >>= usable condition
    define condition register value held = case value of
      Just constant | register /= A.pc -> M.insert register (condition,constant) held
      _ -> M.delete register held
  nextRegister (A.Reg number) = A.Reg (number+1)
  writeback _ base indexing known = if indexing == A.OffsetOnly then known else M.delete base known
  mask width value = case width of A.Byte -> value .&. 0xff; A.Half -> value .&. 0xffff; _ -> value
  effective address condition base offset indexing known = do
    baseValue <- if base == A.pc then Just (address+8) else M.lookup base known >>= usable condition
    case (offset,indexing) of
      (_,A.PostIndexed) -> Just baseValue
      (A.OffsetImmediate delta,_) -> Just (baseValue+fromIntegral delta)
      (A.OffsetRegister added register A.LSL amount,_) -> do
        index <- M.lookup register known >>= usable condition
        let scaled = index `shiftL` amount
        Just (if added then baseValue+scaled else baseValue-scaled)
      _ -> Nothing
  record address direction condition width target value = case target of
    Just cell | spanLow <= cell && cell < spanHigh -> [CellAccess (GuestAddress address) direction cell (M.lookup cell cells) width value condition]
    _ -> []

clusters :: (Analysis -> ShapeHash) -> [Analysis] -> M.Map ShapeHash [Analysis]
clusters key = M.fromListWith (++) . map (\analysis -> (key analysis,[analysis]))

projectTrajectory :: [Controller] -> M.Map CallbackId Analysis -> Trajectory -> Either String Value
projectTrajectory controllers index (Trajectory kind domains) = do
  selected <- fmap (S.fromList . concat) (mapM selectDomain (maybe [] id domains))
  let selectedAnalyses = [analysis | target <- S.toList selected,Just analysis <- [M.lookup target index]]
  unless (length selectedAnalyses == S.size selected) (Left "selected target absent from callback census")
  pure (object
    [ "trajectory" .= show kind,"domain_recorded" .= maybe False (const True) domains
    , "controllers" .= [controller | Domain (ControllerId controller) _ <- maybe [] id domains]
    , "paired_coordinates" .= sum [length pairs | Domain _ pairs <- maybe [] id domains]
    , "selected_stock_targets" .= map callbackText (S.toList selected),"selected_count" .= S.size selected
    , "helper_bound_shapes" .= M.size (clusters analysisBoundHash selectedAnalyses)
    , "helper_erased_shapes" .= M.size (clusters analysisErasedHash selectedAnalyses)
    , "scope" .= ("static projection of recorded paired entry domains through pinned stock tables, not observed callback entries or published-package target identities"::String)])
 where
  selectDomain (Domain ident pairs) = case [controller | controller@(Controller ident' _ _) <- controllers,ident == ident'] of
    [Controller _ width tables] -> mapM (selectPair width tables) pairs >>= pure . filter (/= CallbackId (GuestAddress 0))
    _ -> Left "trajectory controller missing or duplicated"
  selectPair width tables (state,event) = do
    cell <- tableWord Cells tables (fromIntegral (event*width+state))
    target <- tableWord Actions tables (fromIntegral (fromIntegral cell::Word16))
    pure (CallbackId (GuestAddress target))
  tableWord wanted tables offset = case [words' | Table kind' _ words' <- tables,wanted == kind'] of
    [words'] -> case drop offset words' of value:_ -> Right value; [] -> Left "trajectory index outside captured table"
    _ -> Left "missing or duplicate trajectory table"

controllerActions :: Controller -> S.Set CallbackId
controllerActions (Controller _ _ tables) = S.fromList [CallbackId (GuestAddress target) | Table Actions _ targets <- tables,target <- targets,target /= 0]

owners :: [Controller] -> M.Map CallbackId (S.Set ControllerId)
owners controllers = M.fromListWith S.union [(target,S.singleton ident) | controller@(Controller ident _ _) <- controllers,target <- S.toList (controllerActions controller)]

clusterJson :: M.Map CallbackId (S.Set ControllerId) -> (ShapeHash,[Analysis]) -> Value
clusterJson ownership (key,members) = object
  [ "hash" .= hashText key, "members" .= map (callbackText . analysisCallback) members, "count" .= length members
  , "controllers" .= [index | ControllerId index <- S.toList (S.unions [M.findWithDefault S.empty (analysisCallback member) ownership | member <- members])]
  , "code_span_bytes" .= sum [size | member <- members,let ByteCount size = analysisCodeBytes member]
  , "external_helpers" .= map addressText (S.toList (S.fromList [target | member <- members,(_,target) <- analysisCalls member]))
  ]

summaryJson :: M.Map ShapeHash [Analysis] -> Value
summaryJson grouped = object
  [ "clusters" .= M.size grouped
  , "singleton_clusters" .= length [() | members <- M.elems grouped,length members == 1]
  , "callbacks_in_repeated_clusters" .= sum [length members | members <- M.elems grouped,length members > 1]
  , "largest_cluster" .= maximum (0:map length (M.elems grouped))
  , "cluster_size_histogram" .= [object ["size" .= size,"clusters" .= count] | (size,count) <- M.toList (M.fromListWith (+) [(length members,1::Int) | members <- M.elems grouped])]
  , "largest_prefixes" .= [object ["clusters" .= count,"callbacks" .= sum (take count (reverse (sort (map length (M.elems grouped)))))] | count <- [1,5,10,20,50,100::Int]]
  ]

sizeSummary :: [Analysis] -> Value
sizeSummary analyses = object
  [ "min" .= pick 0,"p25" .= pick 25,"median" .= pick 50,"p75" .= pick 75,"p90" .= pick 90,"p95" .= pick 95,"p99" .= pick 99,"max" .= pick 100
  , "quantile_method" .= ("sorted code spans at floor(percentile*(n-1)/100)"::String)
  , "sum_code_spans" .= sum sizes
  , "sum_instruction_bytes" .= sum [size | analysis <- analyses,let ByteCount size = analysisInstructionBytes analysis]
  , "sum_storage_bytes" .= sum [size | analysis <- analyses,let ByteCount size = analysisStorageBytes analysis]
  , "bins" .= [object ["max_bytes" .= high,"callbacks" .= length [() | size <- sizes,low < size,size <= high]] | (low,high) <- zip [0,32,64,128,256,512,1024,4096] [32,64,128,256,512,1024,4096,maxBound::Int]]
  ]
 where
  sizes = sort [size | analysis <- analyses,let ByteCount size = analysisCodeBytes analysis]
  pick percentile = sizes !! ((percentile*(length sizes-1)) `div` 100)

registeredUnder :: M.Map CallbackId (S.Set ControllerId) -> Analysis -> ControllerId -> Bool
registeredUnder ownership analysis ident = S.member ident (M.findWithDefault S.empty (analysisCallback analysis) ownership)

cellAccessJson :: M.Map CallbackId (S.Set ControllerId) -> Analysis -> CellAccess -> Value
cellAccessJson ownership analysis access = object
  [ "site" .= addressText (cellSite access),"direction" .= show (cellDirection access),"address" .= hex (cellAddress access)
  , "controller" .= fmap (\(ControllerId index,_) -> index) (cellTarget access),"field" .= fmap (show . snd) (cellTarget access)
  , "registered_controller" .= fmap (registeredUnder ownership analysis . fst) (cellTarget access)
  , "width" .= show (cellWidth access),"value" .= fmap hex (cellValue access),"condition" .= (\(A.Cond code) -> code) (cellCondition access)
  ]

cellSummary :: M.Map CallbackId (S.Set ControllerId) -> M.Map ShapeHash [Analysis] -> [Analysis] -> Value
cellSummary ownership byBound analyses = object
  [ "method" .= ("straight-line constant propagation of MOVW/MOVT, MOV/MVN/ADD/SUB immediates and PC-relative literal words to load/store effective addresses (immediate offsets and LSL register offsets with a known register) inside the manifest state/event cell span; resets at internal branch and code-literal targets and after unconditional transfers; calls clobber r0-r3, r12, LR and flags and are assumed to preserve r4-r11. A record is a static instruction site under its own condition, not an executed path. Absence of a record is not absence of a write: unknown-register, non-LSL register-offset, callee-computed and join-crossing addresses are not resolved."::String)
  , "callbacks_with_cell_store" .= count (not . null . stores)
  , "callbacks_with_state_store" .= count (any (field StateCell) . stores)
  , "callbacks_with_event_store" .= count (any (field EventCell) . stores)
  , "callbacks_with_cell_load" .= count (any ((== CellLoad) . cellDirection) . analysisCells)
  , "store_sites" .= length allStores
  , "store_sites_with_constant_value" .= length [() | (_,access) <- allStores,Just _ <- [cellValue access]]
  , "store_sites_not_at_cell_start" .= length [() | (_,access) <- allStores,Nothing <- [cellTarget access]]
  , "state_store_sites_registered_controller" .= relation StateCell True
  , "state_store_sites_other_controller" .= relation StateCell False
  , "event_store_sites_registered_controller" .= relation EventCell True
  , "event_store_sites_other_controller" .= relation EventCell False
  , "distinct_written_cells" .= S.size (S.fromList [cell | (_,access) <- allStores,Just cell <- [cellTarget access]])
  , "repeated_helper_bound_clusters_with_cell_stores" .= length storingClusters
  , "callbacks_in_repeated_helper_bound_clusters_with_cell_stores" .= sum (map length storingClusters)
  , "repeated_helper_bound_clusters_with_differing_store_signatures" .= length [() | members <- storingClusters,S.size (S.fromList (map signature members)) > 1]
  , "signature" .= ("per member, ordered stores as (field, registered-controller relation, constant value); differing signatures within one helper-bound shape mean the members differ in written field, registered-controller relation, stored constant or recorded store count"::String)
  ]
 where
  stores analysis = [access | access <- analysisCells analysis,cellDirection access == CellStore]
  field wanted access = maybe False ((== wanted) . snd) (cellTarget access)
  count predicate = length (filter predicate analyses)
  allStores = [(analysis,access) | analysis <- analyses,access <- stores analysis]
  relation wanted registered = length [() | (analysis,access) <- allStores,Just (ident,kind) <- [cellTarget access],kind == wanted,registeredUnder ownership analysis ident == registered]
  storingClusters = [members | members <- M.elems byBound,length members > 1,any (not . null . stores) members]
  signature analysis = [(fmap (\(ident,kind) -> (show kind,registeredUnder ownership analysis ident)) (cellTarget access),cellValue access) | access <- stores analysis]

main :: IO ()
main = do
  arguments <- getArgs
  let required name = case dropWhile (/= name) arguments of _:value:_ -> pure value; _ -> fail ("missing " ++ name)
  manifestPath <- required "--manifest"
  imagePath <- required "--id5"
  outputPath <- required "--out"
  exists <- doesPathExist outputPath
  when exists (fail "output already exists")
  manifestBytes <- B.readFile manifestPath
  unless (sha256Hex (sha256 manifestBytes) == "f27b35628371e150cdc0cfea84b11424b52c8022f7604f832a04bb890273cd67") (fail "manifest identity differs")
  Manifest controllers registrations callbacks trajectories <- either fail pure (J.eitherDecodeStrict manifestBytes)
  unless (length controllers == 174 && length callbacks == 5795) (fail "manifest census differs")
  bytes <- B.readFile imagePath
  unless (B.length bytes == 5185120 && sha256Hex (sha256 bytes) == "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a") (fail "image5 identity differs")
  let cells = M.fromList (concat [[(state,(ident,StateCell)),(event,(ident,EventCell))] | CellRegistration ident (GuestAddress state) (GuestAddress event) <- registrations])
  unless (length registrations == 174 && M.size cells == 348) (fail "controller cell registrations differ")
  analyses <- either fail pure (mapM (analyze bytes cells) callbacks)
  let callbackIndex = M.fromList [(analysisCallback analysis,analysis) | analysis <- analyses]
  projected <- either fail pure (mapM (projectTrajectory controllers callbackIndex) trajectories)
  let ownership = owners controllers
      byRaw = clusters analysisRawHash analyses
      byBound = clusters analysisBoundHash analyses
      byErased = clusters analysisErasedHash analyses
      rank = sortOn (negate . length . snd) . M.toList
      helperUsers = M.fromListWith S.union [(target,S.singleton (analysisCallback analysis)) | analysis <- analyses,(_,target) <- analysisCalls analysis]
      callPatterns = M.fromListWith (++) [(map snd (analysisCalls analysis),[analysisCallback analysis]) | analysis <- analyses]
      result = object
        [ "schema_version" .= (2::Int),"evidence_class" .= ("static structural clustering and constant-address cell access sites; no semantic equivalence or new execution"::String)
        , "inputs" .= object ["manifest" .= manifestPath,"manifest_sha256" .= sha256Hex (sha256 manifestBytes),"image5" .= imagePath,"image5_sha256" .= sha256Hex (sha256 bytes)]
        , "normalization" .= object
          [ "raw" .= ("exact complete inspected storage words"::String)
          , "helper_bound" .= ("preserve instruction offsets, registers, conditions, operation/width/addressing modes, register lists, shift amounts and immediate-offset sign/zero; parameterize scalar immediates, MOVW/MOVT halves, non-control literal values and offset magnitudes; retain exact external helper identities, internal branch offsets and internal-code literal offsets; Compute words remain bit-exact"::String)
          , "helper_erased" .= ("same parameter shape, additionally replace external branch/call identities; this is only a coarse upper bound on template opportunities"::String)
          , "limits" .= ("Shape equality is not behavior equivalence. Parameters can encode IDs, permissions, sizes, flags, device addresses, settings, state and errors. Every binding and retained ABI needs independent recovery. No register renaming or reordered-operation matching is performed."::String)]
        , "callbacks" .= length analyses,"sizes" .= sizeSummary analyses
        , "cluster_summary" .= object ["raw" .= summaryJson byRaw,"helper_bound" .= summaryJson byBound,"helper_erased" .= summaryJson byErased]
        , "controller_cell_summary" .= cellSummary ownership byBound analyses
        , "trajectory_projection" .= projected
        , "helper_bound_clusters" .= map (clusterJson ownership) (rank byBound)
        , "helper_erased_clusters" .= map (clusterJson ownership) (rank byErased)
        , "callback_index" .= [object ["address" .= callbackText (analysisCallback analysis),"raw_hash" .= hashText (analysisRawHash analysis),"helper_bound_hash" .= hashText (analysisBoundHash analysis),"helper_erased_hash" .= hashText (analysisErasedHash analysis),"helper_bound_parameter_sites" .= map addressText (analysisBoundParameterSites analysis),"helper_erased_parameter_sites" .= map addressText (analysisErasedParameterSites analysis),"return_proof_conditional" .= analysisReturnClosed analysis,"controller_cell_accesses" .= map (cellAccessJson ownership analysis) (analysisCells analysis)] | analysis <- analyses]
        , "shared_helpers" .= [object ["address" .= addressText target,"callbacks" .= S.size users,"members" .= map callbackText (S.toList users)] | (target,users) <- sortOn (negate . S.size . snd) (M.toList helperUsers)]
        , "call_patterns" .= [object ["helpers_in_instruction_order" .= map addressText pattern',"callbacks" .= length members,"members" .= map callbackText members] | (pattern',members) <- sortOn (negate . length . snd) (M.toList callPatterns)]
        , "controllers" .= [let actions = controllerActions controller
                                members = [analysis | target <- S.toList actions,Just analysis <- [M.lookup target callbackIndex]]
                            in object ["id" .= index,"actions" .= S.size actions,"helper_bound_shapes" .= M.size (clusters analysisBoundHash members),"helper_erased_shapes" .= M.size (clusters analysisErasedHash members),"actions_in_global_repeated_helper_bound_shapes" .= length [() | analysis <- members,length (M.findWithDefault [] (analysisBoundHash analysis) byBound)>1]]
                           | controller@(Controller (ControllerId index) _ _) <- controllers]
        ]
  L.writeFile outputPath (J.encode result)
  putStrLn ("wrote " ++ outputPath ++ "; callbacks=" ++ show (length analyses) ++ "; helper-bound clusters=" ++ show (M.size byBound))
