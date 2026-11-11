{-# LANGUAGE GHC2021 #-}
-- Every stage model under replacement/develop/model, with one uniform view of the stock
-- entries and scripted call addresses each case needs.
module StageCatalogue
  ( Stage (..)
  , StageCaseView (..)
  , StageImage (..)
  , stages
  , stageModules
  , supportModules
  , catalogueModules
  ) where

import qualified Data.ByteString as B
import Data.List.NonEmpty (toList)
import Data.Word (Word32)
import PipelineModel
import Scenarios (Address (..), Scenario (..), callAddress, stock)
import qualified AffineMathScenarios
import qualified BuffersScenarios
import qualified CropEngineScenarios
import qualified CropProcessScenarios
import qualified DateOverlayScenarios
import qualified DebugStorageScenarios
import qualified DiagnosticLoggingScenarios
import qualified DispatcherScenarios
import qualified DistortionScenarios
import qualified EncoderTaskScenarios
import qualified EntryScenarios
import qualified FontBuiltinScenarios
import qualified FontFdlScenarios
import qualified FontOutlineVmScenarios
import qualified FontRasterHandlersScenarios
import qualified FontRasterVmScenarios
import qualified FontScanlinesScenarios
import qualified FontScriptsScenarios
import qualified ImageGeometryScenarios
import qualified ImageOperationsScenarios
import qualified ImagePixelsScenarios
import qualified ImagePlanesScenarios
import qualified ImageReferencesScenarios
import qualified ImageResizeScenarios
import qualified MeshHistoryScenarios
import qualified MotionSearchScenarios
import qualified ObjectAllocatorScenarios
import qualified OperationObjectsScenarios
import qualified OperationPredicatesScenarios
import qualified OutputCoefficientsScenarios
import qualified OutputExpansionScenarios
import qualified OutputJpegScenarios
import qualified OutputMetadataScenarios
import qualified OutputTripletsScenarios
import qualified OutputsScenarios
import qualified PackedSoftwareScenarios
import qualified PermitScenarios
import qualified PreparationScenarios
import qualified PreviewHistoryScenarios
import qualified ProcessingScenarios
import qualified ProfileCalibrationScenarios
import qualified RedEyeBackendScenarios
import qualified RedEyeDetectorScenarios
import qualified RedEyeMaskScenarios
import qualified RedEyeScenarios
import qualified SaveScenarios
import qualified ScanNumericScenarios
import qualified ScanProcessingScenarios
import qualified SensorCalibrationScenarios
import qualified SensorRegistersScenarios
import qualified SensorRequestsScenarios
import qualified SensorSettingsScenarios
import qualified StripPlannerScenarios
import qualified TransactionScenarios
import qualified TransformsScenarios
import qualified WarpEngineScenarios

-- Which image a stage model reads. Bundle is image4 followed by image5.
data StageImage = Image5 | Bundle deriving (Eq, Show)

data StageCaseView = StageCaseView
  { caseName :: String
  , caseEntry :: Word32
  , caseCalls :: [Word32]
  } deriving (Eq, Show)

data Stage = Stage
  { stageName :: String
  , stageModule :: String
  , stageImage :: StageImage
  , stageViews :: B.ByteString -> (StagePublication -> Address) -> [StageCaseView]
  -- Present for stages whose cases are StageScenario records, which the qualifier can
  -- execute.
  , stageExecutable :: Maybe (B.ByteString -> (StagePublication -> Address) -> [StageScenario])
  }

caseNameText :: StageCaseName -> String
caseNameText (StageCaseName text) = text

scenarioView :: StageScenario -> StageCaseView
scenarioView scenario = StageCaseView (caseNameText (stageCaseName scenario)) (addressWord (stageStockEntry (stagePublication scenario))) (map (addressWord . callAddress) (stageCalls scenario))

transactionView :: Scenario -> StageCaseView
transactionView scenario = StageCaseView (scenarioName scenario) (stock (scenarioFunction scenario)) (map (addressWord . callAddress) (scenarioCalls scenario))

meshHistoryView :: MeshHistoryScenarios.MeshHistoryPhase -> StageCaseView
meshHistoryView phase = StageCaseView (caseNameText (MeshHistoryScenarios.meshPhaseName phase)) (addressWord (stageStockEntry (MeshHistoryScenarios.meshPhasePublication phase))) (map (addressWord . callAddress) (MeshHistoryScenarios.meshPhaseCalls phase))

outlineView :: FontOutlineVmScenarios.OutlineScenario -> StageCaseView
outlineView scenario = StageCaseView (FontOutlineVmScenarios.outlineName scenario) (FontOutlineVmScenarios.outlineOriginal (FontOutlineVmScenarios.outlineRoot scenario)) []

predicateView :: OperationPredicatesScenarios.PredicateScenario -> StageCaseView
predicateView scenario = StageCaseView (OperationPredicatesScenarios.predicateScenarioName scenario) (OperationPredicatesScenarios.predicateSource scenario) (map (addressWord . callAddress) (OperationPredicatesScenarios.predicateExpectedCalls scenario))

previewHistoryView :: PreviewHistoryScenarios.PreviewHistoryPhase -> StageCaseView
previewHistoryView phase = StageCaseView (caseNameText (PreviewHistoryScenarios.previewPhaseName phase)) (addressWord (stageStockEntry (PreviewHistoryScenarios.previewPhasePublication phase))) (map (addressWord . callAddress) (PreviewHistoryScenarios.previewPhaseCalls phase))

failureView :: StripPlannerScenarios.GeometryFailureScenario -> StageCaseView
failureView failure = StageCaseView (caseNameText (StripPlannerScenarios.failureCase failure)) (addressWord (stageStockEntry (StripPlannerScenarios.failureEntry failure))) []

scenarioStage :: String -> String -> StageImage -> (B.ByteString -> (StagePublication -> Address) -> [StageScenario]) -> Stage
scenarioStage name moduleName image build = Stage name moduleName image (\bytes resolve -> map scenarioView (build bytes resolve)) (Just build)

viewStage :: String -> String -> StageImage -> (B.ByteString -> (StagePublication -> Address) -> [StageCaseView]) -> Stage
viewStage name moduleName image views = Stage name moduleName image views Nothing

stages :: [Stage]
stages =
  [ scenarioStage "affine-math" "AffineMathScenarios" Image5 AffineMathScenarios.affineMathScenarios
  , scenarioStage "buffers" "BuffersScenarios" Image5 BuffersScenarios.buffersScenarios
  , scenarioStage "crop-engine" "CropEngineScenarios" Image5 CropEngineScenarios.cropEngineScenarios
  , scenarioStage "crop-process" "CropProcessScenarios" Image5 CropProcessScenarios.cropProcessScenarios
  , scenarioStage "date-overlay" "DateOverlayScenarios" Image5 DateOverlayScenarios.dateOverlayScenarios
  , scenarioStage "debug-storage" "DebugStorageScenarios" Image5 DebugStorageScenarios.debugStorageScenarios
  , scenarioStage "diagnostic-logging" "DiagnosticLoggingScenarios" Image5 DiagnosticLoggingScenarios.diagnosticLoggingScenarios
  , viewStage "dispatcher" "DispatcherScenarios" Image5 (\bytes _ -> map transactionView (DispatcherScenarios.dispatcherScenarios bytes))
  , scenarioStage "distortion" "DistortionScenarios" Image5 DistortionScenarios.distortionScenarios
  , scenarioStage "encoder-task" "EncoderTaskScenarios" Image5 EncoderTaskScenarios.encoderTaskScenarios
  , viewStage "entry" "EntryScenarios" Image5 (\bytes _ -> map transactionView (EntryScenarios.entryScenarios bytes))
  , scenarioStage "font-builtin" "FontBuiltinScenarios" Image5 (\_ _ -> FontBuiltinScenarios.fontBuiltinScenarios)
  , scenarioStage "font-fdl" "FontFdlScenarios" Image5 FontFdlScenarios.fontFdlScenarios
  , viewStage "font-outline-vm" "FontOutlineVmScenarios" Image5 (\bytes _ -> map outlineView (FontOutlineVmScenarios.outlineScenarios ++ FontOutlineVmScenarios.genuineGlyphScenarios bytes ++ map FontOutlineVmScenarios.fatalInput FontOutlineVmScenarios.outlineFatalScenarios))
  , scenarioStage "font-raster-handlers" "FontRasterHandlersScenarios" Image5 FontRasterHandlersScenarios.fontRasterHandlersScenarios
  , scenarioStage "font-raster-vm" "FontRasterVmScenarios" Image5 FontRasterVmScenarios.fontRasterVmScenarios
  , scenarioStage "font-scanlines" "FontScanlinesScenarios" Image5 FontScanlinesScenarios.fontScanlinesScenarios
  , scenarioStage "font-scripts" "FontScriptsScenarios" Image5 FontScriptsScenarios.fontScriptsScenarios
  , scenarioStage "image-geometry" "ImageGeometryScenarios" Image5 ImageGeometryScenarios.imageGeometryScenarios
  , scenarioStage "image-operations" "ImageOperationsScenarios" Image5 ImageOperationsScenarios.imageOperationsScenarios
  , scenarioStage "image-pixels" "ImagePixelsScenarios" Image5 (\bytes resolve -> ImagePixelsScenarios.imagePixelsScenarios bytes resolve ++ ImagePixelsScenarios.imagePixelsOwnedFailureScenarios bytes resolve)
  , scenarioStage "image-planes" "ImagePlanesScenarios" Image5 ImagePlanesScenarios.imagePlanesScenarios
  , scenarioStage "image-references" "ImageReferencesScenarios" Image5 ImageReferencesScenarios.imageReferencesScenarios
  , scenarioStage "image-resize" "ImageResizeScenarios" Image5 ImageResizeScenarios.imageResizeScenarios
  , viewStage "mesh-history" "MeshHistoryScenarios" Image5 (\bytes resolve -> map meshHistoryView (concatMap toList (map MeshHistoryScenarios.meshHistoryPhases (MeshHistoryScenarios.meshHistoryScenarios bytes resolve))))
  , scenarioStage "motion-search" "MotionSearchScenarios" Image5 MotionSearchScenarios.motionSearchScenarios
  , scenarioStage "object-allocator" "ObjectAllocatorScenarios" Image5 ObjectAllocatorScenarios.objectAllocatorScenarios
  , scenarioStage "operation-objects" "OperationObjectsScenarios" Image5 (\_ _ -> OperationObjectsScenarios.operationObjectsScenarios)
  , viewStage "operation-predicates" "OperationPredicatesScenarios" Image5 (\_ _ -> map predicateView OperationPredicatesScenarios.operationPredicatesScenarios)
  , scenarioStage "output-coefficients" "OutputCoefficientsScenarios" Bundle OutputCoefficientsScenarios.outputCoefficientScenarios
  , scenarioStage "output-expansion" "OutputExpansionScenarios" Bundle OutputExpansionScenarios.outputExpansionScenarios
  , scenarioStage "output-jpeg" "OutputJpegScenarios" Image5 OutputJpegScenarios.outputJpegScenarios
  , scenarioStage "output-metadata" "OutputMetadataScenarios" Bundle OutputMetadataScenarios.outputMetadataScenarios
  , scenarioStage "output-triplets" "OutputTripletsScenarios" Image5 OutputTripletsScenarios.scenarios
  , scenarioStage "outputs" "OutputsScenarios" Image5 OutputsScenarios.outputsScenarios
  , scenarioStage "packed-software" "PackedSoftwareScenarios" Image5 PackedSoftwareScenarios.packedSoftwareScenarios
  , viewStage "permit" "PermitScenarios" Image5 (\bytes _ -> map transactionView (PermitScenarios.permitScenarios bytes))
  , scenarioStage "preparation" "PreparationScenarios" Image5 PreparationScenarios.preparationScenarios
  , viewStage "preview-history" "PreviewHistoryScenarios" Image5 (\bytes resolve -> map previewHistoryView (concatMap toList (map PreviewHistoryScenarios.previewHistoryPhases (PreviewHistoryScenarios.previewHistoryScenarios bytes resolve))))
  , scenarioStage "processing" "ProcessingScenarios" Image5 ProcessingScenarios.processingScenarios
  , scenarioStage "profile-calibration" "ProfileCalibrationScenarios" Image5 (\bytes resolve -> ProfileCalibrationScenarios.profileCalibrationScenarios bytes resolve ++ map ProfileCalibrationScenarios.streamFaultScenario ProfileCalibrationScenarios.profileCalibrationFaultScenarios)
  , scenarioStage "red-eye-backend" "RedEyeBackendScenarios" Image5 RedEyeBackendScenarios.redEyeBackendScenarios
  , scenarioStage "red-eye-detector" "RedEyeDetectorScenarios" Image5 (\bytes _ -> RedEyeDetectorScenarios.redEyeDetectorScenarios bytes)
  , scenarioStage "red-eye-mask" "RedEyeMaskScenarios" Image5 RedEyeMaskScenarios.redeyeMaskScenarios
  , scenarioStage "red-eye" "RedEyeScenarios" Image5 RedEyeScenarios.redEyeScenarios
  , viewStage "save" "SaveScenarios" Image5 (\bytes _ -> map transactionView (SaveScenarios.saveScenarios bytes))
  , scenarioStage "scan-numeric" "ScanNumericScenarios" Image5 ScanNumericScenarios.scanNumericScenarios
  , scenarioStage "scan-processing" "ScanProcessingScenarios" Image5 ScanProcessingScenarios.scanProcessingScenarios
  , scenarioStage "sensor-calibration" "SensorCalibrationScenarios" Image5 SensorCalibrationScenarios.sensorCalibrationScenarios
  , scenarioStage "sensor-registers" "SensorRegistersScenarios" Image5 SensorRegistersScenarios.sensorRegisterScenarios
  , scenarioStage "sensor-requests" "SensorRequestsScenarios" Bundle SensorRequestsScenarios.sensorRequestScenarios
  , scenarioStage "sensor-settings" "SensorSettingsScenarios" Image5 (\bytes resolve -> SensorSettingsScenarios.sensorSettingsScenarios bytes resolve ++ SensorSettingsScenarios.sensorTemplateScenarios bytes)
  , viewStage "strip-planner" "StripPlannerScenarios" Image5 (\_ _ -> map failureView StripPlannerScenarios.failureScenarios)
  , viewStage "transaction" "TransactionScenarios" Image5 (\bytes _ -> map transactionView (TransactionScenarios.transactionScenarios bytes))
  , scenarioStage "transforms" "TransformsScenarios" Image5 TransformsScenarios.transformsScenarios
  , scenarioStage "warp-engine" "WarpEngineScenarios" Image5 WarpEngineScenarios.warpEngineScenarios
  ]

stageModules :: [String]
stageModules = map stageModule stages

supportModules :: [String]
supportModules = ["OutputExpansionPairLaw","PipelineModel","Publications","Scenarios","SourceAdmission","StageCatalogue","StockExtents"]

catalogueModules :: [String]
catalogueModules = stageModules ++ supportModules
