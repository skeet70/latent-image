module PipelineModel
  ( StageSymbol (..)
  , StageCaseName (..)
  , StagePublication (..)
  , StageScenario (..)
  , Plane (..)
  , planeWords
  , planePatch
  ) where

import Data.Word (Word32)
import Scenarios (Address, Call, Patch, wordsAt)

newtype StageSymbol = StageSymbol String deriving (Eq, Ord, Show)
newtype StageCaseName = StageCaseName String deriving (Eq, Ord, Show)

data Plane = Plane
  { planeFormat :: Word32
  , planeAddress :: Word32
  , planeLength :: Word32
  , planeStride :: Word32
  , planeWidth :: Word32
  , planeHeight :: Word32
  , planeFlags :: Word32
  } deriving (Eq, Show)

planeWords :: Plane -> [Word32]
planeWords plane = [planeFormat plane,planeAddress plane,planeLength plane,planeStride plane,planeWidth plane,planeHeight plane,planeFlags plane]

planePatch :: Word32 -> Plane -> Patch
planePatch address = wordsAt address . planeWords

data StagePublication = StagePublication
  { stageStockEntry :: Address
  , stageOwnedSymbol :: StageSymbol
  } deriving (Eq, Show)

data StageScenario = StageScenario
  { stageCaseName :: StageCaseName
  , stagePublication :: StagePublication
  , stageArguments :: [Word32]
  , stageInitial :: [Patch]
  , stageFinal :: [Patch]
  , stageReturn :: Maybe Word32
  , stageCodePointers :: [(Address, StagePublication)]
  , stageCalls :: [Call]
  }
