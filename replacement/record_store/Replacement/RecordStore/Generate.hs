{-# LANGUAGE GHC2021 #-}
module Replacement.RecordStore.Generate
  ( GenerationSeed (..)
  , GenerationConfig
  , mkGenerationConfig
  , generateCases
  , generationVersion
  , generationVersionText
  , ValidCase
  , validCase
  , oracleCase
  , DomainError (..)
  ) where

import Control.Monad (forM_)
import Data.Bits ((.&.), shiftR)
import qualified Data.ByteString as B
import Data.Char (isAsciiLower, isDigit)
import Data.List (nub, sort)
import qualified Data.Map.Strict as M
import Data.Word (Word8, Word16, Word32, Word64)
import Emulation.ScenarioGeneration
  ( GeneratedScenarios (..)
  , Generator (..)
  , GeneratorVersion (..)
  , Rng
  , ScenarioSeed (..)
  , SequenceBounds (..)
  , drawElement
  , drawWord64
  , generateScenarios
  , mkSequenceBounds
  )
import Numeric (showHex)
import Replacement.RecordStore.Cases
  ( CaseName (..)
  , Expected (..)
  , Operation (..)
  , OracleCase (..)
  , Registration (..)
  , Source (..)
  , callbackSize
  , callerSize
  , encodeCallback
  , encodeRequest
  , eventStride
  , model
  , requestSize
  , resultSize
  )
import Replacement.RecordStore.Hardware (hardware)
import Replacement.RecordStore.Model

data DomainError
  = InvalidCaseCount Int
  | InvalidOperationBound Int
  | InvalidCaseName String
  | MissingPrefix
  | UnsupportedOperation String
  | UnsupportedRegistration String
  | UnsupportedCallback String
  | UnsupportedFault String
  | LifecycleViolation String
  | InvalidSequence Word16
  | InvalidBankGeometry String
  | InvalidBankContents String
  | InvalidReadBounds String
  | EventCapacityExceeded Int
  | CallerCapacityExceeded Int
  | RequestJournalOverflow Int
  | CallbackJournalOverflow Int
  | MalformedCaseJson String
  | UnsupportedCaseVersion Int
  deriving (Eq, Show)

newtype GenerationSeed = GenerationSeed Word64
  deriving (Eq, Show)

data GenerationConfig = GenerationConfig
  { generationSeed :: GenerationSeed
  , generationCount :: Int
  , generationMaxOperations :: Int
  } deriving (Eq, Show)

mkGenerationConfig :: GenerationSeed -> Int -> Int -> Either DomainError GenerationConfig
mkGenerationConfig seed count maxOperations
  | count < 1 || count > 64 = Left (InvalidCaseCount count)
  | maxOperations < 3 || maxOperations > 12 = Left (InvalidOperationBound maxOperations)
  | otherwise = Right (GenerationConfig seed count maxOperations)

newtype ValidCase = ValidCase OracleCase
  deriving (Eq, Show)

oracleCase :: ValidCase -> OracleCase
oracleCase (ValidCase c) = c

-- | Bump when a seed starts producing different scenarios.
generationVersion :: GeneratorVersion
generationVersion = GeneratorVersion generationVersionText

generationVersionText :: String
generationVersionText = "record-store-cases/1"

minimumGeneratedOperations :: Int
minimumGeneratedOperations = 3

generateCases :: GenerationConfig -> [ValidCase]
generateCases config =
  case generateScenarios (recordStoreGenerator bounds) (ScenarioSeed seed) (generationCount config) of
    Right generated -> generatedScenarios generated
    Left problem -> error ("record-store generation: " ++ show problem)
  where
    seed = case generationSeed config of GenerationSeed value -> value
    bounds = either (error . show) id (mkSequenceBounds minimumGeneratedOperations (generationMaxOperations config))

recordStoreGenerator :: SequenceBounds -> Generator ValidCase Operation
recordStoreGenerator bounds =
  Generator
    { generatorVersion = generationVersion
    , generatorBounds = bounds
    , generatorEvents = operations . oracleCase
    , generatorScenario = generateScenario
    }

generateScenario :: ScenarioSeed -> Int -> SequenceBounds -> Rng -> ValidCase
generateScenario (ScenarioSeed seed) index bounds rng0 = selectValid [candidate { callbacks = chosen } | chosen <- nub (drawn : callbackChoices)]
  where
    kind' = toEnum (index `mod` 5) :: Kind
    family = families !! (index `mod` length families)
    (bank0, rng1) = buildBank kind' (planBank0 (familyBanks family)) rng0
    (bank1, rng2) = buildBank kind' (planBank1 (familyBanks family)) rng1
    (drawn, rng3) = case familyCallback family of
      Just chosen -> (chosen, rng2)
      Nothing -> drawElement callbackChoices rng2
    plan = take (maximumEvents bounds - 2) (familyBody family)
    (body, _) = materializeBody kind' plan rng3
    g = geometry kind'
    candidate = OracleCase
      { name = CaseName (generatedName seed index family)
      , kind = kind'
      , initialMedium = Medium (M.fromList [(poolBlock g Bank0, bank0), (poolBlock g Bank1, bank1)])
      , operations = Open : Register Normal : body
      , callbacks = drawn
      , injection = NoFault
      }

callbackChoices :: [Callback]
callbackChoices = [NoCallback, MarkerCallback, CorrectingCallback]

-- | First candidate the smart constructor accepts. Every planned body is valid
-- with NoCallback, which is the final candidate.
selectValid :: [OracleCase] -> ValidCase
selectValid candidates = case [accepted | candidate <- candidates, Right accepted <- [validCase candidate]] of
  first : _ -> first
  [] -> error "generation found no valid callback choice"

validCase :: OracleCase -> Either DomainError ValidCase
validCase candidate = do
  validateName candidate
  validateFault candidate
  validateCallback candidate
  let g = geometry (kind candidate)
  validateMedium g (initialMedium candidate)
  validateOperations g (operations candidate)
  validateCapacity g candidate
  validateJournals candidate
  Right (ValidCase candidate)

validateName :: OracleCase -> Either DomainError ()
validateName candidate = case caseName (name candidate) of
  first : rest
    | isAsciiLower first
    , length rest <= 63
    , all (\ch -> isAsciiLower ch || isDigit ch || ch == '-') rest -> Right ()
  _ -> Left (InvalidCaseName (caseName (name candidate)))

validateFault :: OracleCase -> Either DomainError ()
validateFault candidate = case injection candidate of
  NoFault -> Right ()
  other -> Left (UnsupportedFault (show other))

validateCallback :: OracleCase -> Either DomainError ()
validateCallback candidate = case callbacks candidate of
  NoCallback -> Right ()
  MarkerCallback -> Right ()
  CorrectingCallback -> Right ()
  other -> Left (UnsupportedCallback (show other))

validateMedium :: Geometry -> Medium -> Either DomainError ()
validateMedium g (Medium blocks) = do
  let expected = [poolBlock g Bank0, poolBlock g Bank1]
  if sort (M.keys blocks) == expected
    then Right ()
    else Left (InvalidBankGeometry ("medium blocks " ++ show (M.keys blocks)))
  forM_ expected $ \key -> case M.lookup key blocks of
    Nothing -> Left (InvalidBankGeometry ("missing block " ++ show key))
    Just bytes -> validateBank g key bytes

validateBank :: Geometry -> PoolBlock -> B.ByteString -> Either DomainError ()
validateBank g key bytes
  | B.length bytes /= blockBytes = Left (InvalidBankGeometry ("block " ++ show key ++ " size " ++ show (B.length bytes)))
  | B.all (== 0xff) suffix = Right ()
  | otherwise = Left (InvalidBankContents ("block " ++ show key ++ " after " ++ show occupied ++ " records"))
  where
    size = recordBytes g
    maxRecords = recordCount g
    storedSequenceAt p = sequenceOf (B.take 2 (B.drop (p * size) bytes))
    occupied = length (takeWhile (\p -> storedSequenceAt p /= Sequence 0xffff) [0 .. maxRecords - 1])
    suffix = B.drop (occupied * size) bytes

validateOperations :: Geometry -> [Operation] -> Either DomainError ()
validateOperations g operations' = case operations' of
  Open : Register Normal : body -> go True body
  _ -> Left MissingPrefix
  where
    go _ [] = Right ()
    go active (op : rest) = case op of
      On _ _ -> Left (UnsupportedOperation "on")
      CacheBoundary _ _ _ -> Left (UnsupportedOperation "cache-boundary")
      Open -> Left (UnsupportedOperation "open")
      Register Normal
        | active -> Left (LifecycleViolation "register while active")
        | otherwise -> go True rest
      Register other -> Left (UnsupportedRegistration (show other))
      Close
        | active -> go False rest
        | otherwise -> Left (LifecycleViolation "close while inactive")
      Latest -> requireActive active "latest" >> go active rest
      Append (Seed 0xffff) _ -> Left (InvalidSequence 0xffff)
      Append _ _ -> requireActive active "append" >> go active rest
      EraseAll -> requireActive active "erase-all" >> go active rest
      Collect -> requireActive active "collect" >> go active rest
      Zero -> go active rest
      ReadAt (BytePosition position) lengthBytes -> validateRead g position lengthBytes >> go active rest
    requireActive active label
      | active = Right ()
      | otherwise = Left (LifecycleViolation label)

-- | Positive length up to one record and a position inside the two banks,
-- including the bank-past boundary at @2 * blockBytes@. Reads that cross a bank
-- boundary or start at the boundary are supported negative cases: the stock and
-- model refuse them with an invalid status.
validateRead :: Geometry -> Word32 -> Int -> Either DomainError ()
validateRead g position lengthBytes
  | lengthBytes <= 0 = Left (InvalidReadBounds "nonpositive length")
  | lengthBytes > recordBytes g = Left (InvalidReadBounds "length exceeds record bytes")
  | toInteger position > bankSpace = Left (InvalidReadBounds "position past both banks")
  | otherwise = Right ()
  where
    bankSpace = 2 * toInteger blockBytes

validateCapacity :: Geometry -> OracleCase -> Either DomainError ()
validateCapacity g candidate
  | total > slots = Left (EventCapacityExceeded total)
  | 2 * recordCount g * recordBytes g > callerSize = Left (CallerCapacityExceeded (2 * recordCount g * recordBytes g))
  | otherwise = Right ()
  where
    total = length (operations candidate)
    slots = (resultSize - eventSlotBase) `div` eventStride
    eventSlotBase = 0x100

validateJournals :: OracleCase -> Either DomainError ()
validateJournals candidate
  | requestBytes > requestSize = Left (RequestJournalOverflow requestBytes)
  | callbackBytes > callbackSize = Left (CallbackJournalOverflow callbackBytes)
  | otherwise = Right ()
  where
    expected = model hardware candidate
    state = expectedStore expected
    p = persistence state
    requestBytes = sum (map (B.length . encodeRequest) (requests p))
    callbackBytes = sum (map (B.length . encodeCallback) (callbackEvents state))

data Family = Family
  { familyTag :: String
  , familyBanks :: BankPlan
  , familyBody :: [PlanOp]
  , familyCallback :: Maybe Callback
  }

data BankPlan = BankPlan
  { planBank0 :: Shape
  , planBank1 :: Shape
  }

data Shape
  = ShapeEmpty
  | ShapePrefix Int
  | ShapeFull
  | ShapeWrap
  | ShapeRejected

data PlanOp
  = PlanLatest
  | PlanAppend
  | PlanAppendStaging
  | PlanEraseAll
  | PlanCollect
  | PlanRead
  | PlanReadBankOne
  | PlanReadCrossing
  | PlanReadLast
  | PlanReadPast
  | PlanZero
  | PlanClose
  | PlanRegister

families :: [Family]
families =
  [ Family "wrap-predecessor" (BankPlan ShapeWrap ShapeEmpty) [PlanAppend, PlanLatest, PlanCollect] Nothing
  , Family "empty-latest" (BankPlan ShapeEmpty ShapeEmpty) [PlanLatest] Nothing
  , Family "partial-latest" (BankPlan (ShapePrefix 3) ShapeEmpty) [PlanLatest] Nothing
  , Family "full-latest" (BankPlan ShapeFull ShapeEmpty) [PlanLatest] Nothing
  , Family "both-full-latest" (BankPlan ShapeFull ShapeFull) [PlanLatest] Nothing
  , Family "append-room" (BankPlan (ShapePrefix 3) ShapeEmpty) [PlanAppend, PlanLatest] Nothing
  , Family "append-rotate" (BankPlan ShapeFull ShapeEmpty) [PlanAppend, PlanLatest] Nothing
  , Family "append-both-full" (BankPlan ShapeFull ShapeFull) [PlanAppend, PlanLatest] Nothing
  , Family "append-staging" (BankPlan (ShapePrefix 2) ShapeEmpty) [PlanAppendStaging, PlanLatest] Nothing
  , Family "rejected-latest" (BankPlan ShapeRejected ShapeEmpty) [PlanLatest] Nothing
  , Family "wrap-latest" (BankPlan ShapeWrap (ShapePrefix 1)) [PlanLatest] Nothing
  , Family "wrap-collect" (BankPlan ShapeWrap (ShapePrefix 1)) [PlanCollect] Nothing
  , Family "collect-partial" (BankPlan (ShapePrefix 4) (ShapePrefix 2)) [PlanCollect] Nothing
  , Family "erase-all" (BankPlan (ShapePrefix 3) (ShapePrefix 2)) [PlanEraseAll, PlanLatest] Nothing
  , Family "read-bank1" (BankPlan ShapeEmpty (ShapePrefix 2)) [PlanReadBankOne] Nothing
  , Family "read-edges" (BankPlan ShapeEmpty ShapeEmpty) [PlanReadLast, PlanReadPast] Nothing
  , Family "read-crossing" (BankPlan ShapeEmpty ShapeEmpty) [PlanReadCrossing] Nothing
  , Family "zero-read" (BankPlan (ShapePrefix 1) ShapeEmpty) [PlanZero, PlanRead] Nothing
  , Family "close-reopen" (BankPlan (ShapePrefix 2) ShapeEmpty) [PlanClose, PlanRegister, PlanLatest] Nothing
  , Family "close-read" (BankPlan (ShapePrefix 1) ShapeEmpty) [PlanClose, PlanRead, PlanRegister, PlanAppend] Nothing
  , Family "correcting" (BankPlan (ShapePrefix 3) ShapeEmpty) [PlanLatest] (Just CorrectingCallback)
  , Family "marker" (BankPlan ShapeRejected ShapeEmpty) [PlanLatest, PlanCollect] (Just MarkerCallback)
  , Family "no-callback" (BankPlan (ShapePrefix 1) ShapeEmpty) [PlanAppend, PlanRead, PlanCollect] (Just NoCallback)
  , Family "long-lifecycle" (BankPlan ShapeFull (ShapePrefix 2))
      [PlanLatest, PlanAppend, PlanCollect, PlanClose, PlanRegister, PlanLatest, PlanAppendStaging, PlanRead, PlanZero, PlanClose]
      Nothing
  ]

generatedName :: Word64 -> Int -> Family -> String
generatedName seed index family =
  "gen-" ++ hex16 seed ++ "-" ++ show index ++ "-" ++ familyTag family
  where
    hex16 value = let digits = showHex value "" in replicate (16 - length digits) '0' ++ digits

buildBank :: Kind -> Shape -> Rng -> (B.ByteString, Rng)
buildBank kind' shape rng = (bytes, rngAfter)
  where
    g = geometry kind'
    count = case shape of
      ShapeEmpty -> 0
      ShapePrefix n -> min n (recordCount g)
      ShapeFull -> recordCount g
      ShapeWrap -> 1
      ShapeRejected -> 2
    (filler, rngFiller) = drawElement [0x33, 0x44, 0x55, 0x77] rng
    (start, rngStart) = drawStart count rngFiller
    (rejectDraw, rngAfter) = drawWord64 rngStart
    records = case shape of
      ShapeWrap -> [(0x7fff, False)]
      ShapeRejected -> [(0x10, False), (0x11, True)]
      _ -> [(sequenceAt start p, rejectedAt count rejectDraw p) | p <- [0 .. count - 1]]
    encoded = B.concat [encodeRecord g filler seqNo rejected | (seqNo, rejected) <- records]
    bytes = encoded <> B.replicate (blockBytes - B.length encoded) 0xff

sequenceAt :: Word16 -> Int -> Word16
sequenceAt start p = fromIntegral ((fromIntegral start + p) .&. 0x7fff)

rejectedAt :: Int -> Word64 -> Int -> Bool
rejectedAt count rejectDraw p = p > 0 && p == count - 1 && (rejectDraw .&. 1) == 1

drawStart :: Int -> Rng -> (Word16, Rng)
drawStart count rng = (start, rng')
  where
    (draw, rng') = drawWord64 rng
    start
      | count > 0 && (draw .&. 1) == 1 = fromIntegral ((0x7fff - (count - 1)) .&. 0x7fff)
      | otherwise = fromIntegral ((draw `shiftR` 8) .&. 0x7fff)

encodeRecord :: Geometry -> Word8 -> Word16 -> Bool -> B.ByteString
encodeRecord g filler seqNo rejected =
  half seqNo <> B.singleton (if rejected then 0xee else 0x22) <> B.replicate (recordBytes g - 3) filler

materializeBody :: Kind -> [PlanOp] -> Rng -> ([Operation], Rng)
materializeBody kind' plan rng0 = go True plan rng0 []
  where
    go _ [] rng acc = (reverse acc, rng)
    go active (step : rest) rng acc =
      let (op, rng') = materializeOp kind' step rng
      in if legalOperation active op
           then go (nextActive active op) rest rng' (op : acc)
           else let fallback = if active then Latest else Register Normal
                in go (nextActive active fallback) rest rng' (fallback : acc)

materializeOp :: Kind -> PlanOp -> Rng -> (Operation, Rng)
materializeOp kind' step rng = case step of
  PlanLatest -> (Latest, rng)
  PlanAppend -> appendOperation CallerSource rng
  PlanAppendStaging -> appendOperation StagingSource rng
  PlanEraseAll -> (EraseAll, rng)
  PlanCollect -> (Collect, rng)
  PlanRead -> readOperation kind' rng
  PlanReadBankOne -> (ReadAt (BytePosition (fromIntegral blockBytes)) 16, rng)
  PlanReadCrossing -> (ReadAt (BytePosition (fromIntegral blockBytes - 1)) 2, rng)
  PlanReadLast -> (ReadAt (BytePosition (fromIntegral (2 * blockBytes) - 1)) 1, rng)
  PlanReadPast -> (ReadAt (BytePosition (fromIntegral (2 * blockBytes))) 1, rng)
  PlanZero -> (Zero, rng)
  PlanClose -> (Close, rng)
  PlanRegister -> (Register Normal, rng)

appendOperation :: Source -> Rng -> (Operation, Rng)
appendOperation source rng = (Append (Seed seed) source, rng')
  where
    (draw, rng') = drawWord64 rng
    seed = fromIntegral (draw .&. 0x7fff)

readOperation :: Kind -> Rng -> (Operation, Rng)
readOperation kind' rng = (ReadAt (BytePosition (fromIntegral position)) lengthBytes, rng'')
  where
    g = geometry kind'
    (positionDraw, rng') = drawWord64 rng
    choices = [0, 1, recordBytes g, blockBytes - 1, blockBytes, blockBytes + recordBytes g, 2 * blockBytes - 1, 2 * blockBytes]
    position = choices !! fromIntegral (positionDraw `mod` fromIntegral (length choices))
    maxLength = max 1 (min (recordBytes g) (2 * blockBytes - position))
    (lengthDraw, rng'') = drawWord64 rng'
    lengthBytes = 1 + fromIntegral (lengthDraw `mod` fromIntegral maxLength)

legalOperation :: Bool -> Operation -> Bool
legalOperation active op = case op of
  Latest -> active
  Append _ _ -> active
  EraseAll -> active
  Collect -> active
  Close -> active
  Register Normal -> not active
  ReadAt _ _ -> True
  Zero -> True
  _ -> False

nextActive :: Bool -> Operation -> Bool
nextActive _ (Register Normal) = True
nextActive _ Close = False
nextActive active _ = active
