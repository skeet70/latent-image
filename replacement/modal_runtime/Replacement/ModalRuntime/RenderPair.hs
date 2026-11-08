{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Replacement.ModalRuntime.RenderPair
  ( RenderConfig(..), DrawCoverage(..), ResourceId(..), DrawClass(..), DrawFunction(..), Draw(..)
  , deriveDrawClass, renderDraws, compareDraws, checkRenderStackObservations, checkRetainedRenderStackObservations
  , validateRenderDeclarations, validateRetainedRenderDeclarations, DeclarationScope (..), checkRenderCoverage
  , checkRenderInvocationEvidence
  , osdInputs, discoveredBanks
  , foldRenderDraws, collectDrawsReader, compareDrawsReader, compareDrawsReaderRole, discoveredBanksReader, discoveredBanksReaderRole
  , checkRenderCoverageReader, checkRenderStackObservationsReader
  , checkRetainedRenderStackObservationsReader
  , checkRenderInvocationEvidenceReader, osdInputsReader
  ) where

import Control.Monad (foldM, forM, forM_, unless, when)
import qualified Data.Aeson as J
import qualified Data.Aeson.Types as J
import Data.Bits ((.&.), shiftL, shiftR)
import qualified Data.ByteString as B
import Data.List (nub, sort)
import qualified Data.Map.Strict as M
import Data.Maybe (listToMaybe)
import qualified Data.Set as S
import qualified Data.Text as T
import Data.Word (Word8, Word32)
import Numeric (readHex, showHex)
import qualified RenderStack as S
import Replacement.ModalRuntime.MenuEnginePair
import Replacement.ModalRuntime.MenuEngineTree
import Replacement.ModalRuntime.RenderObjectProvenance
import qualified Replacement.ModalRuntime.RenderStackEvidence as E

newtype ResourceId = ResourceId Word32 deriving (Eq, Show)
data DrawCoverage = DrawsRequired | RetainedObservation | DeclaredDrawFree deriving (Eq, Show)
data RenderConfig = RenderConfig { renderBanks :: [Int], renderClock :: Maybe ResourceId, renderStackBase :: Word32, renderCoverage :: DrawCoverage } deriving (Eq, Show)
data DrawFunction = TreeDraw | PickerDraw | PaneDraw deriving (Eq, Show)
data DrawClass = OwnedDraw DrawFunction | StockDraw DrawFunction deriving (Eq, Show)
data Draw = Draw { drawClass :: DrawClass, drawMemory :: [B.ByteString], drawHandles :: M.Map ObjectAddress HandleIdentity } deriving (Eq, Show)

word :: B.ByteString -> Int -> Either String Word32
word bytes offset
  | offset < 0 || offset + 4 > B.length bytes = Left "short render word"
  | otherwise = Right (sum [fromIntegral (B.index bytes (offset+i)) `shiftL` (8*i) | i <- [0..3]])

site :: Hit -> T.Text
site hit = case hitSite hit of OtherSite label -> label; _ -> ""

checkRenderCoverage :: RenderConfig -> [Observation] -> [Observation] -> Either String ()
checkRenderCoverage config stock owned = forM_ [(StockRole,stock),(OwnedRole,owned)] $ \(role,observations) ->
  let hasDraw = any (\hit -> site hit `elem` ["render-tree-entry","render-picker-entry","render-pane-entry"]) (concatMap observationHits observations)
  in case (renderCoverage config,hasDraw) of
    (DrawsRequired,False) -> Left (show role ++ ": no render draw entry hits in armed segments")
    (DeclaredDrawFree,True) -> Left (show role ++ ": draw entry contradicts explicit draw-free declaration")
    _ -> Right ()

-- I4:61ca2c..61cd0c, including the identity-zero override at 61cce8.
liveKind :: TablesDocument -> TableNode -> Word32 -> Either String Word32
liveKind document node cursor = do
  let setting = nodeClass node
      row = case nodeRows node of
        Just rows -> case drop (fromIntegral cursor) rows of
          x:_ -> Just x
          [] -> Nothing
        Nothing -> Nothing
      selected = maybe (Left "render cursor outside composed rows") (Right . rowToken) row
      child = maybe (Left "render cursor outside composed rows") (Right . rowChild) row
      hasChild = (/= LeafChild) <$> child
      parent = case nodeParent node of Nothing -> Right Nothing; Just at -> maybe (Left "render parent missing from composed records") (Right . Just) (lookup at [(nodeAddress n,n) | n <- documentNodes document])
  flag <- if setting `elem` [5,0x220,0x247,0x245,0x47,0xdf,0xe0,0x240,0x1bb,0x1b9,0xd0,0x185,0x246] then Right True
    else do
      special <- case setting of
        0xed -> (==3) <$> selected
        0xe5 -> (==0) <$> selected
        0x141 -> (==0x1b) <$> selected
        _ -> Right False
      if special then Right False else do
        one <- case setting of 7 -> (==1) <$> selected; 0x166 -> (==0) <$> selected; _ -> Right False
        if one then Right True
        else if setting `elem` [0x2d,0x43,0xad,0xe6,0x100,0x101,0x102,0xdc,0xfc] then Right False
        else do
          zero <- if setting == 0x27 then (==0) <$> selected else Right False
          if zero then Right False else do
            p <- parent
            case p of
              Nothing -> hasChild
              Just ancestor | nodeId ancestor == nodeId node -> Right False
                            | nodeClass ancestor /= 5 -> Right True
                            | otherwise -> hasChild
  pure (if nodeId node == 0 then 0 else if flag then 1 else 2)

deriveDrawClass :: TablesDocument -> TablesDocument -> DrawFunction -> Hit -> Either String DrawClass
deriveDrawClass stock composed function hit = do
  (args, block) <- case hitMemory hit of a:b:_ | B.length a == 28 && B.length b == 0x430 -> Right (a,b); _ -> Left "render entry lacks arguments/menu block"
  mode <- if function == PaneDraw then let (_,mode_,_,_) = hitRegisters hit in Right mode_ else word args 20
  record <- Address <$> word block 0
  cursor <- word block 8
  caller <- maybe (Left "render entry lacks lr") Right (hitLr hit)
  let (nodeId_,_,_,_) = hitRegisters hit
      current = lookup record [(nodeAddress n,n) | n <- documentNodes composed]
      stockOrigin = any (\n -> nodeAddress n == record && nodeId n == nodeId_) (documentNodes stock)
      saved = case function of
        PaneDraw -> mode `elem` [2,5,7,11,13]
        _ -> mode <= 13 && ((1 `shiftL` fromIntegral mode :: Word32) .&. (if function == TreeDraw then 0x3ae0 else 0x2280)) /= 0
      skipped = function == PickerDraw && mode `elem` [2,3]
      wantedCallers = case function of TreeDraw -> [0x1167a60]; PickerDraw -> [0x1168ce4,0x116c588]; PaneDraw -> [0x1158184]
  owned <- if not stockOrigin || saved || skipped || caller `notElem` wantedCallers then Right False else case current of
    Nothing -> Left "render record absent from composed tree"
    Just node | nodeId node /= nodeId_ -> Right False
              | function == PickerDraw -> Right True
              | function == PaneDraw -> do
                  depth <- liveKind composed node cursor
                  pure (nodeId_ == 1 && mode `elem` [0,1,3,4,6,8,9,10,12,14,255]
                    && depth `elem` [0,1,2] && nodeType node `elem` [0,1,2,3,9])
              | otherwise -> do
                  kind <- liveKind composed node cursor
                  selected <- case nodeRows node of
                    Just rows -> case drop (fromIntegral cursor) rows of
                      r:_ -> Right (rowToken r)
                      [] -> Left "render cursor outside rows"
                    Nothing -> Right 0
                  let flatOverride = (nodeId_ == 0x25 && nodeClass node == 0x27 && selected == 0) || (nodeId_ == 0x38 && nodeClass node == 0x228 && selected == 11)
                  pure (flatOverride || kind == 1)
  pure ((if owned then OwnedDraw else StockDraw) function)

functionOf :: T.Text -> Maybe DrawFunction
functionOf label | "render-tree-" `T.isPrefixOf` label = Just TreeDraw
                 | "render-picker-" `T.isPrefixOf` label = Just PickerDraw
                 | "render-pane-" `T.isPrefixOf` label = Just PaneDraw
                 | otherwise = Nothing

renderDraws :: RenderConfig -> TablesDocument -> TablesDocument -> Role -> [Observation] -> Either String [[Draw]]
renderDraws config stock composed role observations = do
  unless (sort (nub (renderBanks config)) == sort (renderBanks config) && all (\b -> b >= 0 && b < 19) (renderBanks config)) (Left "invalid render bank declaration")
  (_,draws) <- foldM segment (emptyProvenance,[]) observations
  pure (reverse draws)
 where
  segment (state,previous) observation = do
    batch <- maybe (Left "missing continuous render object provenance") Right (observationProvenance observation)
    cursor <- beginBatch state batch
    (active,finished,ending) <- foldM (step observation) (M.empty,[],cursor) (observationHits observation)
    unless (all (\(_,_,entry) -> unpairedCaller entry) (M.elems active)) (Left "unterminated render draw")
    (updated,pending) <- consumeThrough (batchEnd batch) ending
    unless (null pending) (Left "render object provenance has unconsumed events")
    pure (updated,reverse finished:previous)
  step observation (active,draws,cursor) hit = do
    current <- case hitProvenanceSequence hit of
      Nothing -> Left "render trace hit lacks continuous object provenance sequence"
      Just provenanceSequence -> consumeThrough provenanceSequence cursor
    case functionOf (site hit) of
      Nothing -> Right (active,draws,current)
      Just function -> do
        let label = site hit
            core = hitCore hit
        if "-entry" `T.isSuffixOf` label then do
          unless (maybe True (\(_,_,previous) -> unpairedCaller previous) (M.lookup core active)) (Left "nested render draw")
          klass <- deriveDrawClass stock composed function hit
          pure (M.insert core (klass,False,hit) active,draws,current)
        else case M.lookup core active of
          Nothing -> Left "render body/adapter/return without entry"
          Just (klass,adapter,entry) -> do
            unless (klass == OwnedDraw function || klass == StockDraw function) (Left "render function changed within draw")
            let residentOwned = role == OwnedRole && observationResidency observation == EngineResident && klass == OwnedDraw function
            if "-adapter" `T.isSuffixOf` label then do
              when adapter (Left "duplicate render adapter entry")
              pure (M.insert core (klass,True,entry) active,draws,current)
            else if "-stock" `T.isSuffixOf` label then do
              when residentOwned (Left "resident owned render class reached stock body")
              pure (active,draws,current)
            else if "-return" `T.isSuffixOf` label then do
              when (residentOwned && not adapter) (Left "resident owned render draw lacks adapter entry")
              validateReturn config entry hit
              handles <- drawHandleIdentities (fst current) (hitMemory hit)
              pure (M.delete core active,Draw klass (hitMemory hit) handles:draws,current)
            else Right (active,draws,current)
  drawHandleIdentities snapshot memory = M.fromList . concat <$> forM (zip [0..] (renderBanks config)) (\(position,bank) ->
    concat <$> forM [0..127] (\index -> do
      let offset = index*212+4
      handle <- HandleAddress <$> word (memory !! (6+position*7+offset `div` 4096)) (offset `mod` 4096)
      let object = ObjectAddress (0x15e7538 + fromIntegral bank*27136 + fromIntegral index*212)
      identity <- handleIdentityAt snapshot object handle
      pure [(object,value) | Just value <- [identity]]))
  unpairedCaller hit = hitLr hit `notElem` [Just 0x1167a60,Just 0x1168ce4,Just 0x116c588,Just 0x1158184]

validateReturn :: RenderConfig -> Hit -> Hit -> Either String ()
validateReturn config entry returned = do
  let memory = hitMemory returned
      sizes = [68,if functionOf (site entry) == Just PaneDraw then 40 else 24,64,152,4096,704] ++ concatMap (const [4096,4096,4096,4096,4096,4096,2560]) (renderBanks config)
  unless (map B.length memory == sizes) (Left "render return memory does not match declared bank set")
  before <- case drop 4 (hitMemory entry) of heads:_ | B.length heads == 152 -> Right heads; _ -> Left "render entry lacks list heads"
  let after = memory !! 3
  forM_ [b | b <- [0..18], b `notElem` renderBanks config] $ \bank ->
    unless (B.take 8 (B.drop (bank*8) before) == B.take 8 (B.drop (bank*8) after)) (Left "unread render bank changed head/tail")

normalized :: RenderConfig -> Draw -> [B.ByteString]
normalized config draw = case renderClock config of
  Nothing -> drawMemory draw
  Just (ResourceId resource) -> take 3 memory ++ [heads,events] ++ map maskBank bankBytes
   where
    memory = drawMemory draw
    banks [] = []
    banks chunks = B.concat (take 7 chunks) : banks (drop 7 chunks)
    bankBytes = banks (drop 6 memory)
    objects = M.fromList [(0x15e7538 + fromIntegral bank*0x6a00 + fromIntegral index*212, B.take 212 (B.drop (index*212) bytes))
      | (bank,bytes) <- zip (renderBanks config) bankBytes, index <- [0..127]]
    isClock object = word object 8 == Right resource
    skipClock :: Int -> Word32 -> Word32
    skipClock remaining address | remaining > 0, Just object <- M.lookup address objects, isClock object, Right next <- word object 0 = skipClock (remaining-1) next
                                | otherwise = address
    encoded n = B.pack [fromIntegral (n `shiftR` (8*i)) | i <- [0..3]]
    link object = case word object 0 of Right next -> encoded (skipClock 128 next) <> B.drop 4 object; Left _ -> object
    maskBank bytes = B.concat [let object = B.take 212 (B.drop (i*212) bytes) in
      if isClock object then B.replicate 212 0 else link object | i <- [0..127]]
    lastObject :: Int -> Word32 -> Word32
    lastObject remaining address | remaining > 0, address /= 0, Just object <- M.lookup address objects, Right next <- word object 0, skipClock 128 next /= 0 = lastObject (remaining-1) (skipClock 128 next)
                                 | otherwise = address
    originalHeads = case drop 3 memory of h:_ -> h; [] -> B.empty
    heads = B.concat [case (word originalHeads (bank*8),word originalHeads (bank*8+4)) of
      (Right first,Right last_) | bank `elem` renderBanks config ->
        let first_ = skipClock 128 first
            tailClock = maybe False isClock (M.lookup last_ objects)
        in encoded first_ <> encoded (if tailClock then (if first_ == 0 then 0 else lastObject 128 first_) else last_)
                               | otherwise -> encoded first <> encoded last_
      _ -> B.take 8 (B.drop (bank*8) originalHeads) | bank <- [0..18]]
    originalEvents = B.concat (take 2 (drop 4 memory))
    events = B.concat [maskPlane (B.take 960 (B.drop (p*960) originalEvents)) | p <- [0..4]]
    maskPlane plane =
      let kept = [slot | i <- [0..29], let slot = B.take 32 (B.drop (i*32) plane), word slot 0 /= Right resource]
      in B.concat kept <> B.replicate ((30-length kept)*32) 0

compareDraws :: RenderConfig -> [Draw] -> [Draw] -> Either String ()
compareDraws config stock owned =
  let left = collapseDraws config stock
      right = collapseDraws config owned
      mismatches = [(index, drawDifference config a b) | (index, a, b) <- zip3 [0 :: Int ..] left right, not (sameDrawValue config a b)]
  in case (mismatches, length left == length right) of
    ((index, detail) : _, _) -> Left ("render draw order/state differs at draw " ++ show index ++ ": " ++ detail)
    ([], False) -> Left ("render draw order/state differs: " ++ show (length left) ++ " stock draws for " ++ show (length right) ++ " owned draws")
    ([], True) -> Right ()

drawVisibleHandles :: RenderConfig -> Draw -> M.Map ObjectAddress HandleIdentity
drawVisibleHandles config draw = case renderClock config of
  Nothing -> drawHandles draw
  Just (ResourceId resource) -> M.filterWithKey (\(ObjectAddress address) _ ->
    let offset = address-0x15e7538
        bank = fromIntegral (offset `div` 27136)
        within = fromIntegral (offset `mod` 27136)+8
        position = lookup bank (zip (renderBanks config) [0..])
        resourceAt i = word (drawMemory draw !! (6+i*7+within `div` 4096)) (within `mod` 4096)
    in maybe True (\i -> resourceAt i /= Right resource) position) (drawHandles draw)

sameDrawValue :: RenderConfig -> Draw -> Draw -> Bool
sameDrawValue config left right = drawClass left == drawClass right && sameMemoryValue config left right

memorySegments :: RenderConfig -> [B.ByteString] -> [(Int,Int,B.ByteString)]
memorySegments config memory = case renderClock config of
  Nothing -> [(bank,chunk*4096,bytes) | ((bank,chunk),bytes) <- zip [(bank,chunk) | bank <- renderBanks config, chunk <- [0..6]] (drop 6 memory)]
  Just _ -> [(bank,0,bytes) | (bank,bytes) <- zip (renderBanks config) (drop 5 memory)]

bankWord :: RenderConfig -> [B.ByteString] -> Int -> Int -> Either String Word32
bankWord config memory position offset =
  let (index, within) = case renderClock config of
        Nothing -> (6 + position*7 + offset `div` 4096, offset `mod` 4096)
        Just _ -> (5 + position, offset)
  in word (memory !! index) within

-- A bank's list holds its draw state; off-list state-2 slots are cache entries.
liveObjects :: RenderConfig -> [B.ByteString] -> Int -> Either String [Int]
liveObjects config memory position = do
  let bank = renderBanks config !! position
      base = 0x15e7538 + fromIntegral bank*27136
  first <- word (memory !! 3) (bank*8)
  go base first [] (128 :: Int)
 where
  go base address seen remaining
    | address == 0 = Right (reverse seen)
    | remaining == 0 = Left "render bank list exceeds its 128 objects"
    | address < base = Left "render bank list leaves its pool"
    | otherwise = do
        let offset = fromIntegral (address - base) :: Int
        unless (offset `mod` 212 == 0 && offset `div` 212 < 128) (Left "render bank list leaves its pool")
        let index = offset `div` 212
        when (index `elem` seen) (Left "render bank list cycles")
        next <- bankWord config memory position offset
        go base next (index:seen) (remaining - 1)

liveObjectSets :: RenderConfig -> [B.ByteString] -> Either String [[Int]]
liveObjectSets config memory = mapM (liveObjects config memory) [0 .. length (renderBanks config) - 1]

bankLive :: RenderConfig -> [[Int]] -> Int -> [Int]
bankLive config sets bank = case lookup bank (zip (renderBanks config) [0..]) of
  Just position -> sets !! position
  Nothing -> []

liveAddresses :: RenderConfig -> [[Int]] -> S.Set ObjectAddress
liveAddresses config sets = S.fromList
  [ObjectAddress (0x15e7538 + fromIntegral bank*27136 + fromIntegral index*212) | (position, bank) <- zip [0..] (renderBanks config), index <- sets !! position]

liveHandles :: RenderConfig -> [[Int]] -> M.Map ObjectAddress HandleIdentity -> M.Map ObjectAddress HandleIdentity
liveHandles config sets handles = M.filterWithKey (\object _ -> S.member object (liveAddresses config sets)) handles

sameMemoryValue :: RenderConfig -> Draw -> Draw -> Bool
sameMemoryValue config left right =
  let a = normalized config left
      b = normalized config right
      prefix = if renderClock config == Nothing then 6 else 5
      expected = length (renderBanks config) * (if renderClock config == Nothing then 7 else 1)
      leftHandles = drawVisibleHandles config left
      rightHandles = drawVisibleHandles config right
  in take prefix a == take prefix b && length (drop prefix a) == expected && length (drop prefix b) == expected
    && case (liveObjectSets config a, liveObjectSets config b) of
        (Right liveLeft, Right liveRight) | liveLeft == liveRight ->
          let rightVisible = liveHandles config liveRight rightHandles
              segmentsA = memorySegments config a
              segmentsB = memorySegments config b
              lives = map (\(bank,_,_) -> bankLive config liveLeft bank) segmentsA
          in M.foldrWithKey (\object identity rest -> rest && maybe True (== identity) (M.lookup object rightVisible)) True (liveHandles config liveLeft leftHandles)
            && and (zipWith3 (sameSegmentValue config leftHandles rightHandles) lives segmentsA segmentsB)
        _ -> False

drawDifference :: RenderConfig -> Draw -> Draw -> String
drawDifference config left right
  | drawClass left /= drawClass right = "class stock=" ++ show (drawClass left) ++ " owned=" ++ show (drawClass right)
  | Just detail <- handleDifference = detail
  | Just detail <- prefixDifference = detail
  | length (drop prefix a) /= length (drop prefix b) = "state chunk count stock=" ++ show (length (drop prefix a)) ++ " owned=" ++ show (length (drop prefix b))
  | Just detail <- segmentDifference = detail
  | otherwise = "draw state differs"
 where
  a = normalized config left
  b = normalized config right
  prefix = if renderClock config == Nothing then 6 else 5
  leftHandles = drawVisibleHandles config left
  rightHandles = drawVisibleHandles config right
  handleDifference = case (liveObjectSets config a, liveObjectSets config b) of
    (Right liveLeft, Right liveRight) ->
      let rightVisible = liveHandles config liveRight rightHandles
      in case [(object, identity, M.lookup object rightVisible) | (object, identity) <- M.toList (liveHandles config liveLeft leftHandles), M.lookup object rightVisible /= Just identity] of
        (object, identity, other) : _ -> Just ("handle " ++ show object ++ " stock=" ++ show identity ++ " owned=" ++ show other)
        [] -> Nothing
    _ -> Nothing
  prefixDifference = case firstChunkDifference (take prefix a) (take prefix b) of
    Just (offset, x, y) -> Just ("prefix offset " ++ show offset ++ " stock=" ++ hexByte x ++ " owned=" ++ hexByte y)
    Nothing -> Nothing
  segmentDifference = case (liveObjectSets config a, liveObjectSets config b) of
    (Right liveLeft, Right liveRight) | liveLeft == liveRight ->
      let segmentsA = memorySegments config a
          segmentsB = memorySegments config b
          lives = map (\(bank,_,_) -> bankLive config liveLeft bank) segmentsA
      in listToMaybe
        ["bank " ++ show bank ++ " offset " ++ show (offset + i) ++ " stock=" ++ hexByte (B.index x i) ++ " owned=" ++ hexByte (B.index y i)
        | (live,(bank,offset,x),(_,_,y)) <- zip3 lives segmentsA segmentsB
        , i <- take 1 [i | i <- [0 .. min (B.length x) (B.length y) - 1], B.index x i /= B.index y i, not (segmentByteAllowed config leftHandles rightHandles live bank offset i)]]
    (Right _, Right _) -> Just "render bank lists differ"
    (Left problem, _) -> Just ("stock render bank list: " ++ problem)
    (_, Left problem) -> Just ("owned render bank list: " ++ problem)

hexByte :: Word8 -> String
hexByte value = let digits = showHex value "" in if length digits < 2 then '0' : digits else digits

firstChunkDifference :: [B.ByteString] -> [B.ByteString] -> Maybe (Int, Word8, Word8)
firstChunkDifference = go 0
 where
  go _ [] [] = Nothing
  go offset (x:xs) (y:ys) = case [i | i <- [0 .. min (B.length x) (B.length y) - 1], B.index x i /= B.index y i] of
    i : _ -> Just (offset + i, B.index x i, B.index y i)
    [] | B.length x /= B.length y -> Just (offset + min (B.length x) (B.length y), 0, 0)
       | otherwise -> go (offset + B.length x) xs ys
  go offset _ _ = Just (offset, 0, 0)

-- ov48b_find_object and ov48b_allocate_object read state, handle, identifier, text, x and y.
-- Off-list key and styling bytes are the only stale content.
controlByte :: Int -> Bool
controlByte within = within < 0x14 || (within >= 0x34 && within < 0x38)

segmentByteAllowed :: RenderConfig -> M.Map ObjectAddress HandleIdentity -> M.Map ObjectAddress HandleIdentity -> [Int] -> Int -> Int -> Int -> Bool
segmentByteAllowed config left right live bank offset index =
  let within = (offset + index) `mod` 212
      object = (offset + index) `div` 212
      handle = within >= 4 && within < 8 && sameHandleByte config left right bank (offset + index)
      content = object `notElem` live && not (controlByte within)
  in handle || content

sameSegmentValue :: RenderConfig -> M.Map ObjectAddress HandleIdentity -> M.Map ObjectAddress HandleIdentity -> [Int] -> (Int,Int,B.ByteString) -> (Int,Int,B.ByteString) -> Bool
sameSegmentValue config left right live (bank,offset,a) (_,_,b) = B.length a == B.length b && (a == b || and
  [ B.index a i == B.index b i || segmentByteAllowed config left right live bank offset i | i <- [0..B.length a-1] ])

sameHandleByte :: RenderConfig -> M.Map ObjectAddress HandleIdentity -> M.Map ObjectAddress HandleIdentity -> Int -> Int -> Bool
sameHandleByte _ left right bank offset =
  let within = offset `mod` 212
      object = ObjectAddress (0x15e7538+fromIntegral bank*27136+fromIntegral (offset `div` 212)*212)
  in within >= 4 && within < 8 && case M.lookup object left of
    Just identity -> M.lookup object right == Just identity
    Nothing -> False

collapseDraws :: RenderConfig -> [Draw] -> [Draw]
collapseDraws config (a:b:rest) | renderClock config /= Nothing && drawKeyValue config a == drawKeyValue config b && drawMemory a /= drawMemory b = collapseDraws config (b:rest)
collapseDraws config (a:rest) = a : collapseDraws config rest
collapseDraws _ [] = []

drawKeyValue :: RenderConfig -> Draw -> (DrawClass, [B.ByteString], M.Map ObjectAddress HandleIdentity)
drawKeyValue config draw = (drawClass draw, normalized config draw, drawVisibleHandles config draw)

foldRenderDraws :: RenderConfig -> TablesDocument -> TablesDocument -> Role -> [ObservationReader]
  -> (a -> Draw -> Either String a) -> a -> IO (Either String a)
foldRenderDraws config stock composed role readers onDraw initial
  | not (sort (nub (renderBanks config)) == sort (renderBanks config) && all (\b -> b >= 0 && b < 19) (renderBanks config)) =
      pure (Left "invalid render bank declaration")
  | otherwise = go initial emptyProvenance readers
 where
  go state _ [] = pure (Right state)
  go state provenance (reader : rest) = case readerProvenance reader of
    Nothing -> pure (Left "missing continuous render object provenance")
    Just batch -> case beginBatch provenance batch of
      Left problem -> pure (Left problem)
      Right cursor -> do
        result <- readerFoldHits reader (step reader) (state, M.empty, cursor)
        case result of
          Left problem -> pure (Left problem)
          Right (next, active, ending) ->
            if not (all (\(_,_,entry) -> unpairedCaller entry) (M.elems active))
              then pure (Left "unterminated render draw")
              else case consumeThrough (batchEnd batch) ending of
                Left problem -> pure (Left problem)
                Right (updated, pending) ->
                  if null pending then go next updated rest else pure (Left "render object provenance has unconsumed events")
  step reader (state, active, cursor) hit = do
    current <- case hitProvenanceSequence hit of
      Nothing -> Left "render trace hit lacks continuous object provenance sequence"
      Just provenanceSequence -> consumeThrough provenanceSequence cursor
    case functionOf (site hit) of
      Nothing -> Right (state, active, current)
      Just function -> do
        let label = site hit
            core = hitCore hit
        if "-entry" `T.isSuffixOf` label then do
          unless (maybe True (\(_,_,previous) -> unpairedCaller previous) (M.lookup core active)) (Left "nested render draw")
          klass <- deriveDrawClass stock composed function hit
          Right (state, M.insert core (klass,False,hit) active, current)
        else case M.lookup core active of
          Nothing -> Left "render body/adapter/return without entry"
          Just (klass,adapter,entry) -> do
            unless (klass == OwnedDraw function || klass == StockDraw function) (Left "render function changed within draw")
            let residentOwned = role == OwnedRole && readerResidency reader == EngineResident && klass == OwnedDraw function
            if "-adapter" `T.isSuffixOf` label then do
              when adapter (Left "duplicate render adapter entry")
              Right (state, M.insert core (klass,True,entry) active, current)
            else if "-stock" `T.isSuffixOf` label then do
              when residentOwned (Left "resident owned render class reached stock body")
              Right (state, active, current)
            else if "-return" `T.isSuffixOf` label then do
              when (residentOwned && not adapter) (Left "resident owned render draw lacks adapter entry")
              validateReturn config entry hit
              handles <- drawHandleIdentities (fst current) (hitMemory hit)
              next <- onDraw state (Draw klass (hitMemory hit) handles)
              Right (next, M.delete core active, current)
            else Right (state, active, current)
  drawHandleIdentities snapshot memory = M.fromList . concat <$> forM (zip [0..] (renderBanks config)) (\(position,bank) ->
    concat <$> forM [0..127] (\index -> do
      let offset = index*212+4
      handle <- HandleAddress <$> word (memory !! (6+position*7+offset `div` 4096)) (offset `mod` 4096)
      let object = ObjectAddress (0x15e7538 + fromIntegral bank*27136 + fromIntegral index*212)
      identity <- handleIdentityAt snapshot object handle
      pure [(object,value) | Just value <- [identity]]))
  unpairedCaller hit = hitLr hit `notElem` [Just 0x1167a60,Just 0x1168ce4,Just 0x116c588,Just 0x1158184]

collectDrawsReader :: RenderConfig -> TablesDocument -> TablesDocument -> Role -> [ObservationReader] -> IO (Either String [Draw])
collectDrawsReader config stock composed role readers =
  fmap (fmap reverse) (foldRenderDraws config stock composed role readers (\acc draw -> Right (draw:acc)) [])

compareDrawsReader :: RenderConfig -> TablesDocument -> TablesDocument -> [Draw] -> [ObservationReader] -> IO (Either String ())
compareDrawsReader = compareDrawsReaderRole OwnedRole

compareDrawsReaderRole :: Role -> RenderConfig -> TablesDocument -> TablesDocument -> [Draw] -> [ObservationReader] -> IO (Either String ())
compareDrawsReaderRole role config stock composed stockDraws ownedReaders = do
  let left = collapseDraws config stockDraws
  result <- foldRenderDraws config stock composed role ownedReaders step (left, Nothing, 0, 0, Nothing)
  pure $ case result of
    Left problem -> Left problem
    Right (remaining, previous, leftCount, rightCount, failure) ->
      let (remaining', leftCount', failure') = emit remaining leftCount failure previous
      in case failure' of
        Just problem -> Left problem
        Nothing | not (null remaining') -> Left "render draw order/state differs: extra stock draws"
                | leftCount' /= length left -> Left "render draw order/state differs"
                | rightCount /= length left -> Left "render draw order/state differs"
                | otherwise -> Right ()
 where
  step (remaining, previous, leftCount, rightCount, failure) draw =
    case previous of
      Just prior | renderClock config /= Nothing && drawKeyValue config prior == drawKeyValue config draw && drawMemory prior /= drawMemory draw ->
        Right (remaining, Just draw, leftCount, rightCount + 1, failure)
      _ -> let (remaining', leftCount', failure') = emit remaining leftCount failure previous
           in Right (remaining', Just draw, leftCount', rightCount + 1, failure')
  emit remaining leftCount failure Nothing = (remaining, leftCount, failure)
  emit remaining leftCount failure (Just draw) = case remaining of
    next : rest | sameDrawValue config next draw -> (rest, leftCount + 1, failure)
    _ -> (remaining, leftCount, Just "render draw order/state differs")

discoveredBanksReader :: RenderConfig -> TablesDocument -> TablesDocument -> [ObservationReader] -> IO (Either String [Int])
discoveredBanksReader = discoveredBanksReaderRole OwnedRole

discoveredBanksReaderRole :: Role -> RenderConfig -> TablesDocument -> TablesDocument -> [ObservationReader] -> IO (Either String [Int])
discoveredBanksReaderRole role config stock composed readers = do
  result <- foldRenderDraws config stock composed role readers step []
  pure result
 where
  step banks draw = fmap (\found -> sort (nub (banks ++ found))) (discoveredBanks config [draw])

checkRenderCoverageReader :: RenderConfig -> [ObservationReader] -> [ObservationReader] -> IO (Either String ())
checkRenderCoverageReader config stock owned = go [(StockRole,stock),(OwnedRole,owned)]
 where
  go [] = pure (Right ())
  go ((role,readers):rest) = do
    found <- anyDraw readers
    case found of
      Left problem -> pure (Left problem)
      Right hasDraw -> case (renderCoverage config,hasDraw) of
        (DrawsRequired,False) -> pure (Left (show role ++ ": no render draw entry hits in armed segments"))
        (DeclaredDrawFree,True) -> pure (Left (show role ++ ": draw entry contradicts explicit draw-free declaration"))
        _ -> go rest
  anyDraw [] = pure (Right False)
  anyDraw (reader:rest) = do
    result <- readerFoldHits reader (\found hit -> Right (found || site hit `elem` ["render-tree-entry","render-picker-entry","render-pane-entry"])) False
    case result of
      Left problem -> pure (Left problem)
      Right True -> pure (Right True)
      Right False -> anyDraw rest

checkRenderStackObservationsReader :: RenderConfig -> S.RenderStackReport -> TablesDocument -> TablesDocument -> [ObservationReader] -> IO (Either String StackReport)
checkRenderStackObservationsReader config bounds stock composed readers
  | case readers of first:_ -> readerPhase first /= "render-baseline"; [] -> True = pure (Left "missing pre-segment render stack baseline")
  | otherwise = do
      snapshots <- forM readers $ \reader -> do
        record <- pure (readNamedReader "render-task-record" 40 reader)
        stack <- pure (readNamedReader "render-task-stack" 4096 reader)
        pure ((,,) (readerPhase reader) <$> record <*> stack)
      case sequence snapshots of
        Left problem -> pure (Left problem)
        Right entries -> do
          let firstRecord = case entries of (_,record,_):_ -> Just record; [] -> Nothing
          if not (all (\(_,record,_) -> Just record == firstRecord) entries)
            then pure (Left "task 33 descriptor changed")
            else do
              result <- foldFloors [] readers
              case result of
                Left problem -> pure (Left problem)
                Right floors -> do
                  let lowest = if null floors then Nothing else Just (minimum floors)
                      changed = case entries of
                        (_,_,first):rest -> [renderStackBase config + fromIntegral offset | offset <- [0,4..4092], any (\(_,_,later) -> B.take 4 (B.drop offset first) /= B.take 4 (B.drop offset later)) rest]
                        [] -> []
                      highWater = case changed of a:_ -> Just a; [] -> Nothing
                  case highWater of
                    Just mark | mark < renderStackBase config + 64 -> pure (Left "task 33 changed inside existing bottom guard")
                    _ -> pure (Right (StackReport lowest highWater))
 where
  foldFloors floors [] = pure (Right floors)
  foldFloors floors (reader:rest) = do
    result <- readerFoldHits reader step floors
    case result of
      Left problem -> pure (Left problem)
      Right next -> foldFloors next rest
  step floors hit
    | site hit `elem` ["render-tree-entry","render-picker-entry","render-pane-entry"] = do
        let function = case functionOf (site hit) of Just f -> f; Nothing -> TreeDraw
        klass <- deriveDrawClass stock composed function hit
        case drop 5 (hitMemory hit) of
          current:node:record:_ -> do
            unless (word current 0 == Right 0x4006060c) (Left "render seam is not task 33 on primary")
            checkDescriptor config node record
          _ -> Left "render seam lacks task descriptor"
        if klass /= OwnedDraw function then Right floors else do
          sp <- maybe (Left "render entry lacks sp") Right (hitSp hit)
          let bound = case function of TreeDraw -> S.reportTree bounds; PickerDraw -> S.reportPicker bounds; PaneDraw -> S.reportPane bounds
              floor_ = toInteger sp - toInteger (S.entryDepth bound) - toInteger (S.entryStockMargin bound)
          unless (floor_ >= toInteger (renderStackBase config)) (Left "render static floor below task 33 stack")
          Right (floor_ : floors)
    | otherwise = Right floors

readNamedReader :: T.Text -> Int -> ObservationReader -> Either String B.ByteString
readNamedReader name size reader = case [b | (n,b) <- readerReads reader, n == name] of
  [bytes] | B.length bytes == size -> Right bytes
  _ -> Left ("missing/short " ++ T.unpack name)

checkRetainedRenderStackObservationsReader :: RenderConfig -> [ObservationReader] -> IO (Either String StackReport)
checkRetainedRenderStackObservationsReader config readers
  | case readers of first:_ -> readerPhase first /= "render-baseline"; [] -> True = pure (Left "missing pre-segment render stack baseline")
  | otherwise = do
      snapshots <- forM readers $ \reader -> do
        record <- pure (readNamedReader "render-task-record" 40 reader)
        stack <- pure (readNamedReader "render-task-stack" 4096 reader)
        pure ((,,) (readerPhase reader) <$> record <*> stack)
      case sequence snapshots of
        Left problem -> pure (Left problem)
        Right entries -> do
          let firstRecord = case entries of (_,record,_):_ -> Just record; [] -> Nothing
          if not (all (\(_,record,_) -> Just record == firstRecord) entries)
            then pure (Left "task 33 descriptor changed")
            else do
              result <- foldSeams readers
              case result of
                Left problem -> pure (Left problem)
                Right () -> do
                  let changed = case entries of
                        (_,_,first):rest -> [renderStackBase config + fromIntegral offset | offset <- [0,4..4092], any (\(_,_,later) -> B.take 4 (B.drop offset first) /= B.take 4 (B.drop offset later)) rest]
                        [] -> []
                      highWater = case changed of a:_ -> Just a; [] -> Nothing
                  pure (Right (StackReport Nothing highWater))
 where
  foldSeams [] = pure (Right ())
  foldSeams (reader:rest) = do
    result <- readerFoldHits reader step ()
    case result of
      Left problem -> pure (Left problem)
      Right () -> foldSeams rest
  step () hit
    | site hit `elem` ["render-tree-entry","render-picker-entry","render-pane-entry"] = do
        case drop 5 (hitMemory hit) of
          current:node:record:_ -> do
            unless (word current 0 == Right 0x4006060c) (Left "render seam is not task 33 on primary")
            checkDescriptor config node record
          _ -> Left "render seam lacks task descriptor"
        Right ()
    | otherwise = Right ()

checkRenderInvocationEvidenceReader :: S.RenderStackReport -> [ObservationReader] -> E.RenderStackEvidence -> IO (Either String E.AllocationVerdict)
checkRenderInvocationEvidenceReader bounds readers evidence = do
  result <- go [] readers
  case result of
    Left problem -> pure (Left problem)
    Right witnesses -> pure $ do
      let frames = [(E.TreeStack,E.FrameBytes (S.entryDepth (S.reportTree bounds)))
            ,(E.PickerStack,E.FrameBytes (S.entryDepth (S.reportPicker bounds)))
            ,(E.PaneStack,E.FrameBytes (S.entryDepth (S.reportPane bounds)))]
      E.checkEntryWitnesses frames witnesses evidence
      pure (E.allocationVerdict evidence)
 where
  go witnesses [] = pure (Right (reverse witnesses))
  go witnesses (reader:rest) = do
    result <- readerFoldHits reader step witnesses
    case result of
      Left problem -> pure (Left problem)
      Right next -> go next rest
  step witnesses hit
    | site hit `elem` ["render-tree-entry","render-picker-entry","render-pane-entry"] = do
        unless (hitCore hit == 0) (Left "render invocation entry is not on primary")
        counter <- maybe (Left "render invocation entry lacks instruction boundary") Right (hitInstructions hit)
        sp <- maybe (Left "render invocation entry lacks SP") Right (hitSp hit)
        lr <- maybe (Left "render invocation entry lacks LR") Right (hitLr hit)
        kind <- case functionOf (site hit) of
          Just TreeDraw -> Right E.TreeStack
          Just PickerDraw -> Right E.PickerStack
          Just PaneDraw -> Right E.PaneStack
          Nothing -> Left "unknown render invocation entry"
        Right (E.EntryWitness kind counter (E.StackAddress (sp .&. 0xbfffffff)) (E.CodeAddress lr) : witnesses)
    | otherwise = Right witnesses

osdInputsReader :: [ObservationReader] -> IO (Either String [(T.Text,J.Value)])
osdInputsReader readers = do
  -- Captures may precede the first observation of both stage-0 buffer addresses.
  complete <- collectBuffers M.empty readers
  case complete of
    Left problem -> pure (Left problem)
    Right buffers -> go buffers readers []
 where
  collectBuffers known [] = pure (Right known)
  collectBuffers known (reader:rest) = do
    result <- window known reader
    case result of
      Left problem -> pure (Left problem)
      Right (buffers,_,_) -> collectBuffers buffers rest
  go _ [] acc = pure (Right acc)
  go known (reader:rest) acc = do
    result <- window known reader
    case result of
      Left problem -> pure (Left problem)
      Right (buffers,initial,increments) -> do
        let added = case (M.lookup 0 buffers,M.lookup 1 buffers) of
              (Just a,Just b) | a /= b -> [(readerPhase reader,J.object ["front_byte" J..= initial,"buffers" J..= [a,b],"increments" J..= increments])]
              _ -> []
        if increments == 0 || not (null added)
          then go buffers rest (acc ++ added)
          else pure (Left "OSD has not observed both stage-0 buffers")
  window known reader = case readNamedReader "render-osd-front" 1 reader of
    Left problem -> pure (Left problem)
    Right bytes -> do
      let ending = fromIntegral (B.head bytes) :: Word32
      if ending > 1 then pure (Left "invalid OSD front byte") else do
        result <- readerFoldHits reader (hitStep known) (known,Nothing,Nothing,0 :: Integer)
        pure $ case result of
          Left problem -> Left problem
          Right (buffers,pending,indices,increments) -> case pending of
            Just _ -> Left "OSD buffer handoff has no increment"
            Nothing -> case indices of
              Nothing -> Right (buffers,ending,increments)
              Just (start,lastFront) | lastFront == ending -> Right (buffers,start,increments)
                                     | otherwise -> Left "OSD end byte disagrees with last increment"
  hitStep _known (buffers,pending,indices,count) trace
    | site trace == "render-osd-buffer" && hitLr trace `elem` map Just [0x6d7b3c,0x6d7c24,0x6d7d10] = do
        unless (pending == Nothing) (Left "OSD buffer handoff overwritten before increment")
        let (_,buffer,_,_) = hitRegisters trace
        Right (buffers,Just buffer,indices,count)
    | site trace == "render-osd-increment" = do
        let (_,_,newFront,_) = hitRegisters trace
        unless (newFront == 1 || newFront == 2) (Left "OSD increment index out of range")
        buffer <- maybe (Left "OSD increment has no buffer handoff") Right pending
        let index = newFront `mod` 2
        start <- case indices of
          Nothing -> Right (newFront - 1)
          Just (initial,previous) -> do
            unless (newFront == previous + 1) (Left "OSD increments do not alternate within window")
            Right initial
        case M.lookup index buffers of
          Just old -> unless (old == buffer) (Left "OSD stage-0 buffer changed")
          Nothing -> Right ()
        Right (M.insert index buffer buffers,Nothing,Just (start,index),count+1)
    | otherwise = Right (buffers,pending,indices,count)

discoveredBanks :: RenderConfig -> [Draw] -> Either String [Int]
discoveredBanks config draws = fmap (sort . nub . concat) $ forM draws $ \draw -> do
  heads <- case drop 3 (drawMemory draw) of h:_ -> Right h; _ -> Left "missing render heads"
  fmap concat $ forM [0..18] $ \bank -> do
    head_ <- word heads (bank*8)
    tail_ <- word heads (bank*8+4)
    let chunks = drop 6 (drawMemory draw)
        stored = case lookup bank (zip (renderBanks config) [B.concat (take 7 (drop (i*7) chunks)) | i <- [0..length (renderBanks config)-1]]) of
          Just bytes -> B.any (/=0) bytes
          Nothing -> False
    pure [bank | head_ /= 0 || tail_ /= 0 || stored]

readNamed :: T.Text -> Int -> Observation -> Either String B.ByteString
readNamed name size observation = case [b | (n,b) <- observationReads observation,n == name] of
  [bytes] | B.length bytes == size -> Right bytes
  _ -> Left ("missing/short " ++ T.unpack name)

checkDescriptor :: RenderConfig -> B.ByteString -> B.ByteString -> Either String ()
checkDescriptor config node record = do
  unless (word node 0 == Right 0x4005c3ac) (Left "task 33 creation record moved")
  unless (and [word record offset == Right value | (offset,value) <- [(8,0x1109130),(12,8),(16,4096),(20,renderStackBase config),(32,33)]]) (Left "task 33 descriptor identity/extent differs")

checkRenderStackObservations :: RenderConfig -> S.RenderStackReport -> TablesDocument -> TablesDocument -> [Observation] -> Either String StackReport
checkRenderStackObservations config bounds stock composed = renderStackObservations config (Just (bounds,stock,composed))

checkRetainedRenderStackObservations :: RenderConfig -> [Observation] -> Either String StackReport
checkRetainedRenderStackObservations config = renderStackObservations config Nothing

renderStackObservations :: RenderConfig -> Maybe (S.RenderStackReport,TablesDocument,TablesDocument) -> [Observation] -> Either String StackReport
renderStackObservations config admission observations = do
  unless (case observations of first:_ -> observationPhase first == "render-baseline"; [] -> False) (Left "missing pre-segment render stack baseline")
  snapshots <- forM observations $ \observation -> do
    node <- readNamed "render-task-node" 4 observation
    record <- readNamed "render-task-record" 40 observation
    checkDescriptor config node record
    stack <- readNamed "render-task-stack" 4096 observation
    pure (record,stack)
  unless (all (\(record,_) -> Just record == fmap fst (safeFirst snapshots)) snapshots) (Left "task 33 descriptor changed")
  floors <- case admission of
    Nothing -> pure []
    Just (bounds,stock,composed) -> fmap concat $ forM [hit | observation <- observations, hit <- observationHits observation, site hit `elem` ["render-tree-entry","render-picker-entry","render-pane-entry"]] $ \hit -> do
      let function = case functionOf (site hit) of Just f -> f; Nothing -> TreeDraw
      klass <- deriveDrawClass stock composed function hit
      if klass /= OwnedDraw function then pure [] else do
        sp <- maybe (Left "render entry lacks sp") Right (hitSp hit)
        let bound = case function of TreeDraw -> S.reportTree bounds; PickerDraw -> S.reportPicker bounds; PaneDraw -> S.reportPane bounds
            floor_ = toInteger sp - toInteger (S.entryDepth bound) - toInteger (S.entryStockMargin bound)
        unless (floor_ >= toInteger (renderStackBase config)) (Left "render static floor below task 33 stack")
        pure [floor_]
  forM_ [hit | observation <- observations, hit <- observationHits observation, site hit `elem` ["render-tree-entry","render-picker-entry","render-pane-entry"]] $ \hit -> case drop 5 (hitMemory hit) of
    current:node:record:_ -> do
      unless (word current 0 == Right 0x4006060c) (Left "render seam is not task 33 on primary")
      checkDescriptor config node record
    _ -> Left "render seam lacks task descriptor"
  let lowest = if null floors then Nothing else Just (minimum floors)
      changed = case snapshots of
        (_,first):rest -> [renderStackBase config + fromIntegral offset | offset <- [0,4..4092], any (\(_,later) -> B.take 4 (B.drop offset first) /= B.take 4 (B.drop offset later)) rest]
        [] -> []
      highWater = safeFirst changed
  case highWater of
    Just mark | mark < renderStackBase config + 64 -> Left "task 33 changed inside existing bottom guard"
    _ -> Right ()
  pure (StackReport lowest highWater)
 where
  safeFirst (a:_) = Just a
  safeFirst [] = Nothing

checkRenderInvocationEvidence :: S.RenderStackReport -> [Observation] -> E.RenderStackEvidence -> Either String E.AllocationVerdict
checkRenderInvocationEvidence bounds observations evidence = do
  witnesses <- forM [hit | observation <- observations, hit <- observationHits observation
    , site hit `elem` ["render-tree-entry","render-picker-entry","render-pane-entry"]] $ \hit -> do
      unless (hitCore hit == 0) (Left "render invocation entry is not on primary")
      counter <- maybe (Left "render invocation entry lacks instruction boundary") Right (hitInstructions hit)
      sp <- maybe (Left "render invocation entry lacks SP") Right (hitSp hit)
      lr <- maybe (Left "render invocation entry lacks LR") Right (hitLr hit)
      kind <- case functionOf (site hit) of
        Just TreeDraw -> Right E.TreeStack
        Just PickerDraw -> Right E.PickerStack
        Just PaneDraw -> Right E.PaneStack
        Nothing -> Left "unknown render invocation entry"
      pure (E.EntryWitness kind counter (E.StackAddress (sp .&. 0xbfffffff)) (E.CodeAddress lr))
  let frames = [(E.TreeStack,E.FrameBytes (S.entryDepth (S.reportTree bounds)))
        ,(E.PickerStack,E.FrameBytes (S.entryDepth (S.reportPicker bounds)))
        ,(E.PaneStack,E.FrameBytes (S.entryDepth (S.reportPane bounds)))]
  E.checkEntryWitnesses frames witnesses evidence
  pure (E.allocationVerdict evidence)

osdInputs :: [Observation] -> Either String [(T.Text,J.Value)]
osdInputs observations = do
  known <- foldM (\buffers observation -> do
    (updated,_,_) <- window buffers observation
    pure updated) M.empty observations
  concat <$> mapM (segment known) observations
 where
  window known observation = do
    bytes <- readNamed "render-osd-front" 1 observation
    let ending = fromIntegral (B.head bytes) :: Word32
    unless (ending <= 1) (Left "invalid OSD front byte")
    (buffers, pending, indices, increments) <- foldM hit (known,Nothing,Nothing,0 :: Integer) (observationHits observation)
    unless (pending == Nothing) (Left "OSD buffer handoff has no increment")
    initial <- case indices of
      Nothing -> Right ending
      Just (start,lastFront) -> do
        unless (lastFront == ending) (Left "OSD end byte disagrees with last increment")
        Right start
    pure (buffers,initial,increments)
  segment known observation = do
    (buffers,initial,increments) <- window known observation
    case (M.lookup 0 buffers,M.lookup 1 buffers) of
      (Just a,Just b) | a /= b -> Right [(observationPhase observation,J.object ["front_byte" J..= initial,"buffers" J..= [a,b],"increments" J..= increments])]
      _ -> if increments == 0 then Right [] else Left "OSD has not observed both stage-0 buffers"
  hit (buffers,pending,indices,count) trace
    | site trace == "render-osd-buffer" && hitLr trace `elem` map Just [0x6d7b3c,0x6d7c24,0x6d7d10] = do
        unless (pending == Nothing) (Left "OSD buffer handoff overwritten before increment")
        let (_,buffer,_,_) = hitRegisters trace
        Right (buffers,Just buffer,indices,count)
    | site trace == "render-osd-increment" = do
        let (_,_,newFront,_) = hitRegisters trace
        unless (newFront == 1 || newFront == 2) (Left "OSD increment index out of range")
        buffer <- maybe (Left "OSD increment has no buffer handoff") Right pending
        let index = newFront `mod` 2
        start <- case indices of
          Nothing -> Right (newFront - 1)
          Just (initial,previous) -> do
            unless (newFront == previous + 1) (Left "OSD increments do not alternate within window")
            Right initial
        case M.lookup index buffers of Just old -> unless (old == buffer) (Left "OSD stage-0 buffer changed"); Nothing -> Right ()
        pure (M.insert index buffer buffers,Nothing,Just (start,index),count+1)
    | otherwise = Right (buffers,pending,indices,count)

data ReadRange = AbsoluteRange Word32 Int | StackRange Int Int deriving (Eq, Show)

data DeclarationScope = AllDrawers | RetainedTreePicker deriving (Eq)

validateRenderDeclarations :: RenderConfig -> Role -> J.Value -> J.Parser ()
validateRenderDeclarations = validateDeclarations AllDrawers

validateRetainedRenderDeclarations :: RenderConfig -> J.Value -> J.Parser ()
validateRetainedRenderDeclarations config = validateDeclarations RetainedTreePicker config StockRole

validateDeclarations :: DeclarationScope -> RenderConfig -> Role -> J.Value -> J.Parser ()
validateDeclarations scope config role = J.withObject "render receipt" $ \fields -> do
  declarations <- fields J..: "declared_trace_sites" >>= mapM (J.withObject "trace declaration" $ \declaration -> do
    label <- declaration J..: "label"
    address <- declaration J..: "address" >>= hex
    ranges <- maybe (pure []) (mapM range) =<< declaration J..:? "reads"
    pure (label,(address,ranges)))
  let entryRanges = [StackRange 0 28,AbsoluteRange 0x9571d0 1072,AbsoluteRange 0x15e64d8 24,AbsoluteRange 0x15e46c0 4,AbsoluteRange 0x15e748c 152,AbsoluteRange 0x40054760 4,AbsoluteRange 0x40060614 4,AbsoluteRange 0x4005c3ac 40]
      chunks base size = [AbsoluteRange (base+fromIntegral offset) (min 4096 (size-offset)) | offset <- [0,4096..size-1]]
      returns offset = [StackRange offset 68,AbsoluteRange 0x15e64d8 24,AbsoluteRange 0x15e6680 64,AbsoluteRange 0x15e748c 152]
        ++ chunks 0x958d7c 4800 ++ concat [chunks (0x15e7538 + fromIntegral bank*0x6a00) 27136 | bank <- renderBanks config]
      paneEntryRanges = take 2 entryRanges ++ [AbsoluteRange 0x15e64d8 40] ++ drop 3 entryRanges
      paneReturns = [AbsoluteRange 0x15e650c 68,AbsoluteRange 0x15e64d8 40] ++ drop 2 (returns 0)
      expected =
        [ ("render-tree-entry",(0x1164e1c,entryRanges)),("render-picker-entry",(0x1163f2c,entryRanges))
        , ("render-pane-entry",(0x11662a0,paneEntryRanges)),("render-pane-stock",(0x11662a4,[]))
        , ("render-pane-return",(0x1158184,paneReturns))
        , ("render-tree-return",(0x1167a60,returns 196)),("render-picker-return",(0x1168ce4,returns 156))
        , ("render-picker-fn-return",(0x116c588,returns 76))
        , ("render-tree-stock",(0x1164e20,[])),("render-picker-stock",(0x1163f30,[]))
        , ("render-osd-buffer",(0x6d76ec,[])),("render-osd-increment",(0x6d7d44,[])) ]
  let selected = case scope of
        AllDrawers -> expected
        RetainedTreePicker -> filter (\(label,_) -> label `notElem` ["render-pane-entry","render-pane-stock","render-pane-return"]) expected
          ++ [ ("rtc-calendar-sampled",(0x6b11e0,[AbsoluteRange 0x409c34be 6]))
             , ("rtc-counter-byte-0",(0x6a5cfc,[])),("rtc-counter-byte-1",(0x6a5d08,[]))
             , ("rtc-counter-byte-2",(0x6a5d14,[])),("rtc-counter-byte-3",(0x6a5d20,[])) ]
  forM_ selected $ \(label,value) -> unless ([actual | (name,actual) <- declarations,name == label] == [value]) (fail ("render trace declaration differs: " ++ T.unpack label))
  when (scope == RetainedTreePicker && any (\(label,_) -> "-adapter" `T.isSuffixOf` label) declarations)
    (fail "retained render receipt declares an owned adapter")
  when (role == OwnedRole) $ forM_ ["render-tree-adapter","render-picker-adapter","render-pane-adapter"] $ \label ->
    unless (length [() | (name,_) <- declarations,name == label] == 1) (fail "missing/duplicate render adapter declaration")
 where
  hex :: String -> J.Parser Word32
  hex text = case readHex text of
    [(n,"")] | length text == 8 -> pure n
    _ -> fail "render declaration address requires eight hex digits"
  range = J.withObject "render trace range" $ \fields -> do
    stack <- fields J..:? "sp"
    case stack of
      Just offset -> StackRange offset <$> fields J..: "bytes"
      Nothing -> AbsoluteRange <$> (fields J..: "address" >>= hex) <*> fields J..: "length"
