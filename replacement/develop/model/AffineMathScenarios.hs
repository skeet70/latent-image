module AffineMathScenarios (affineMathPublications, affineMathScenarios) where

import Data.Bits ((.|.), shiftL)
import qualified Data.ByteString as B
import Data.Word (Word32)
import PipelineModel
import Scenarios (Address (..), Patch (..), wordsAt)

estimator, validator :: StagePublication
estimator = StagePublication (Address 0xd9bddc) (StageSymbol "xm1_pipeline_image_estimate_transform")
validator = StagePublication (Address 0xd9bb9c) (StageSymbol "xm1_pipeline_image_affine_grid_valid")
affineMathPublications :: [StagePublication]
affineMathPublications = [estimator, validator]

source, target, matrix, workspace, options :: Word32
source = 0x03100000
target = 0x03101000
matrix = 0x03102000
workspace = 0x03110000
options = 0x03103000

identity :: [Word32]
identity = [65536,0,0,65536,0,0]

pack :: (Word32, Word32) -> Word32
pack (x,y) = (x `shiftL` 16) .|. y

points :: [(Word32,Word32)]
points = [(32+16*x,32+16*y) | y <- [0..3], x <- [0..3]]

configuration :: [(Int, Word32)] -> Patch
configuration overrides = Patch (Address options) (B.pack [fromIntegral (value index) | index <- [0..47]])
  where
    defaults = [(0,4),(1,4),(2,3),(4,4),(5,4),(6,1),(7,90),(12,1),(14,16),(16,70),(18,3),(20,50),(28,2),(29,3),(30,50),(31,1),(32,2),(33,3),(34,50),(35,1),(40,0),(41,2),(42,4),(43,90),(44,90)]
    value index = case lookup index overrides of
      Just result -> result
      Nothing -> maybe 0 id (lookup index defaults)

scenario :: String -> StagePublication -> [Word32] -> [Patch] -> [Patch] -> Word32 -> StageScenario
scenario name publication arguments initial final result = StageScenario
  { stageCaseName = StageCaseName ("affine-math-" ++ name)
  , stagePublication = publication
  , stageArguments = arguments
  , stageInitial = initial
  , stageFinal = final
  , stageReturn = Just result
  , stageCodePointers = []
  , stageCalls = []
  }

affineMathScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
affineMathScenarios _ _ =
  [ fit "translation-refinement" [] points translated [65536,0,0,65536,196608,131072] 1
  , fit "translation-consensus" [(24,1)] points translated [65536,0,0,65536,196608,131072] 1
  , fit "translation-neighbor-rejection" [(8,1)] points translated [65536,0,0,65536,196608,131072] 1
  , fit "translation-negative" [] points (map (\(x,y) -> (x-3,y-2)) points) [65536,0,0,65536,4294770688,4294836224] 1
  , fit "shear-consensus" [(24,1)] points (map (\(x,y) -> (x+y `div` 4,y)) points) [65536,16384,0,65536,0,0] 1
  , fit "anisotropic-scale" [] points (map (\(x,y) -> (x*5 `div` 4,y*3 `div` 4)) points) [81920,0,0,49152,0,0] 1
  , fit "grid-failure-preserves-fit" [(36,1),(41,0),(43,100)] points translated [65536,0,0,65536,196608,131072] 0
  , fit "disabled-fit-preserves-matrix" [(31,0)] points translated [7,8,9,10,11,12] 1
  , fit "stationary" [] points points identity 1
  , fit "collinear-degenerate" [] line (map (\(x,y) -> (x+3,y+2)) line) identity 0
  , fit "invalid-samples" [] invalid invalid identity 0
  , fit "excessive-scale" [] points (map (\(x,y) -> (x*2,y*2)) points) identity 0
  , fit "oversized-grid" [(0,32),(1,32)] [] [] identity 0
  , scenario "grid-accept" validator [source,target,matrix,workspace,options]
      (gridInitial translated) [] 1
  , scenario "grid-reject" validator [source,target,matrix,workspace,options]
      (gridInitial (map (\(x,y) -> (x+20,y)) points)) [] 0
  , scenario "empty-grid" validator [source,target,matrix,workspace,options]
      [configuration [(0,0),(1,0)]] [] 1
  ]
  where
    translated = map (\(x,y) -> (x+3,y+2)) points
    line = [(32+16*x,32) | x <- [0..15]]
    invalid = replicate 16 (0xffff,0xffff)
    plane address values = wordsAt address (map (\(x,y) -> if x == 0xffff then 0xffff0000 .|. y else pack (x*8,y*8)) values)
    fit name changes from to expected result = scenario name estimator [source,target,0,0,matrix,workspace,options]
      [plane source from,plane target to,configuration changes,wordsAt matrix [7,8,9,10,11,12],Patch (Address workspace) (B.replicate 0x6c00 0)]
      [wordsAt matrix expected] result
    gridInitial values = [plane source points,plane target values,configuration [],wordsAt matrix [65536,0,0,65536,196608,131072],Patch (Address workspace) (B.replicate 0x6c00 0),Patch (Address (workspace+0x6200)) (B.replicate 16 1)]
