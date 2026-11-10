{-# LANGUAGE GHC2021 #-}

module Replacement.PropertyService.RecipeEditOracle
  ( RecipeHost (..)
  , recipeHosts
  , hostProperty
  , payloadProperty
  , recipeHost
  , StoredCode (..)
  , StoredCodes (..)
  , RecipeSession (..)
  , BankArgument (..)
  , DefaultArgument (..)
  , SlotArgumentRaw (..)
  , SetterValue (..)
  , RecipeOperation (..)
  , RecipeCase (..)
  , recipeCases
  , recipeArguments
  , recipeExpectation
  , adjacentProperties
  , adjacentExpectation
  , noEffectDifferences
  ) where

import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.NativeObservation (Register (..))
import PropertyFields (PropertyId (..))
import Replacement.PropertyService.Classify (Owned (OwnedUnassigned), Property)
import Replacement.PropertyService.Oracle

-- The public property id is one greater than the callback argument.
data RecipeHost = Host267 | Host268 | Host269 | Host26a | Host26b | Host26c | Host26d | Host26e | Host26f
  deriving (Eq, Ord, Show, Enum, Bounded)

recipeHosts :: [RecipeHost]
recipeHosts = [minBound .. maxBound]

hostProperty :: RecipeHost -> PropertyId
hostProperty host = PropertyId (0x267 + fromIntegral (fromEnum host))

payloadProperty :: RecipeHost -> PropertyId
payloadProperty host = PropertyId $ case host of
  Host267 -> 0x104
  Host268 -> 0x110
  Host269 -> 0x111
  Host26a -> 0x10e
  Host26b -> 0x10f
  Host26c -> 0xef
  Host26d -> 0xed
  Host26e -> 0x141
  Host26f -> 0xfe

recipeHost :: PropertyId -> Maybe RecipeHost
recipeHost identifier = lookup identifier [(hostProperty host, host) | host <- recipeHosts]

newtype StoredCode = StoredCode Word32 deriving (Eq, Show)

data StoredCodes = StoredCodes
  { code267 :: StoredCode
  , code268 :: StoredCode
  , code269 :: StoredCode
  , code26a :: StoredCode
  , code26b :: StoredCode
  , code26c :: StoredCode
  , code26d :: StoredCode
  , code26e :: StoredCode
  , code26f :: StoredCode
  } deriving (Eq, Show)

data RecipeSession = SessionInactive | SessionActive StoredCodes deriving (Eq, Show)

newtype BankArgument = BankArgument Word32 deriving (Eq, Show)
newtype DefaultArgument = DefaultArgument Word32 deriving (Eq, Show)
newtype SlotArgumentRaw = SlotArgumentRaw Word32 deriving (Eq, Show)
newtype SetterValue = SetterValue Word32 deriving (Eq, Show)

data RecipeOperation
  = RecipeGet BankArgument DefaultArgument SlotArgumentRaw
  | RecipeSet SetterValue BankArgument SlotArgumentRaw
  deriving (Eq, Show)

data RecipeCase = RecipeCase RecipeHost RecipeOperation deriving (Eq, Show)

recipeCases :: [RecipeCase]
recipeCases =
  [RecipeCase host operation | host <- recipeHosts, operation <- gets ++ sets]
  where
    banks = map BankArgument [0, 1, 2, 0x80000000, 0xffffffff]
    slots = map SlotArgumentRaw [0, 1, 2, 3, 0xffffffff]
    modes = map DefaultArgument [0, 1, 2, 0xffffffff]
    values = map SetterValue [0, 1, 0xffffffff, defaultRequest - 1, defaultRequest, defaultRequest + 1]
    gets = [RecipeGet bank mode slot | bank <- banks, mode <- modes, slot <- slots]
    sets = [RecipeSet value bank slot | value <- values, bank <- banks, slot <- slots]

recipeArguments :: RecipeCase -> [(Register, Word32)]
recipeArguments (RecipeCase host operation) = zip [R0, R1, R2, R3] $ identifier : case operation of
  RecipeGet (BankArgument bank) (DefaultArgument mode) (SlotArgumentRaw slot) -> [bank, mode, slot]
  RecipeSet (SetterValue value) (BankArgument bank) (SlotArgumentRaw slot) -> [value, bank, slot]
  where
    PropertyId identifier = hostProperty host

recipeExpectation :: Fixture -> RecipeSession -> RecipeCase -> Maybe Expectation
recipeExpectation fx session (RecipeCase host operation) = do
  baseline <- modelExpectation fx OwnedUnassigned baselineOperation
  let result = case operation of
        RecipeSet {} -> 0
        RecipeGet {} -> case session of
          SessionInactive -> 0
          SessionActive codes -> let StoredCode value = stored codes in value
  pure baseline {expectedRegisters = (R0, result) : filter ((/= R0) . fst) (expectedRegisters baseline)}
  where
    baselineOperation = case operation of
      RecipeGet _ _ (SlotArgumentRaw slot) -> Get Bank0 LiveRead (SlotArgument slot)
      RecipeSet _ _ (SlotArgumentRaw slot) -> Set Bank0 (SlotArgument slot) SetZero
    stored codes = case host of
      Host267 -> code267 codes
      Host268 -> code268 codes
      Host269 -> code269 codes
      Host26a -> code26a codes
      Host26b -> code26b codes
      Host26c -> code26c codes
      Host26d -> code26d codes
      Host26e -> code26e codes
      Host26f -> code26f codes

adjacentProperties :: [PropertyId]
adjacentProperties = [PropertyId 0x266, PropertyId 0x270]

adjacentExpectation :: M.Map PropertyId Property -> Fixture -> OracleCase -> Maybe Expectation
adjacentExpectation properties fx testCase
  | oracleProperty testCase `elem` adjacentProperties = do
      owned <- modelled (standingOf properties (oracleProperty testCase))
      modelExpectation fx owned (oracleOperation testCase)
  | otherwise = Nothing

noEffectDifferences :: Expectation -> Run -> [Difference]
noEffectDifferences expected observed = expectationDifferences expected observed ++ case runWrites observed of
  Nothing -> [SubjectTruncated]
  Just writes -> [WritesDiffer | any outsideScratch writes]
  where
    outsideScratch event = eventWidth event <= 0 || eventAddress event < 0x959000 || toInteger (eventAddress event) + toInteger (eventWidth event) > 0x9597f0
