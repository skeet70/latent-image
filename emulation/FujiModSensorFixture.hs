{-# LANGUAGE OverloadedStrings #-}
-- | Derives a separately identified X-M1 FujiMod run document whose
-- image_input carries a declared synthetic RAW16 frame source and, from
-- fixture schema 2, an enable_readback law. verify fails unless the document
-- is byte-identical to the derivation.
module Main (main) where

import Control.Monad (unless, when)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftR, (.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C8
import Data.List (find, sort)
import Data.Maybe (fromMaybe, isJust)
import Data.Scientific (toBoundedInteger)
import qualified Data.Text as T
import Data.Traversable (for)
import qualified Data.Vector as V
import Data.Word (Word16, Word64, Word8)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.JSON (decodeStrict, encodeCanonical)
import System.Directory (doesPathExist, setCurrentDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (die)

usage :: String
usage =
  unlines
    [ "usage: fujimod-xm1-sensor-fixture write --run BASE.json --fixture FIXTURE.json --output NEW.json"
    , "       fujimod-xm1-sensor-fixture verify --run BASE.json --fixture FIXTURE.json --document DERIVED.json"
    ]

newtype FixtureId = FixtureId T.Text

newtype NodeName = NodeName T.Text

-- | A key of the run document's scene `sensors` map.
newtype FramesName = FramesName T.Text

newtype AssetId = AssetId T.Text

newtype Ticks = Ticks Word64
  deriving (Eq)

-- | Its RAW16 asset is at most 'maxAssetBytes'.
data Raster = Raster {rasterWidth :: Int, rasterHeight :: Int}

data Scene
  = -- | Sample @i@ of the row-major raster holds @i mod 4096@.
    IndexRamp12

-- | The FujiMod sensor part's black level, full scale and per-channel
-- reference white; RAW16 input bypasses them.
data Response = Response Word16 Word16 (Word16, Word16, Word16)

-- | The image input's @0x38c@ guest-read law, FujiMod's @EnableReadback@.
data EnableReadback = IdentityReadback | ExchangedSourceBits
  deriving (Bounded, Enum)

data Fixture = Fixture
  { fixtureId :: FixtureId
  , fixtureNode :: NodeName
  , fixtureFrames :: FramesName
  , fixtureAsset :: AssetId
  , fixtureRaster :: Raster
  , fixtureScene :: Scene
  , fixtureSensorPeriod :: Ticks
  , fixtureResponse :: Response
  , -- | Declared by schema 2; schema 1 leaves the options without the key.
    fixtureEnableReadback :: Maybe EnableReadback
  }

type Parse = Either String

-- | FujiMod's generated-asset bound.
maxAssetBytes :: Int
maxAssetBytes = 64 * 1024 * 1024

sceneName :: Scene -> T.Text
sceneName IndexRamp12 = "raw16-index-ramp12"

scenePattern :: Scene -> [Word8]
scenePattern IndexRamp12 = concat [[fromIntegral (s .&. 0xff), fromIntegral (s `shiftR` 8)] | s <- [0 .. 4095 :: Int]]

enableReadbackName :: EnableReadback -> T.Text
enableReadbackName IdentityReadback = "identity"
enableReadbackName ExchangedSourceBits = "exchanged_source_bits"

assetBytes :: Raster -> Int
assetBytes raster = rasterWidth raster * rasterHeight raster * 2

asObject :: String -> Value -> Parse (KM.KeyMap Value)
asObject _ (Object o) = Right o
asObject what _ = Left (what ++ " is not an object")

asArray :: String -> Value -> Parse (V.Vector Value)
asArray _ (Array a) = Right a
asArray what _ = Left (what ++ " is not an array")

asText :: String -> Value -> Parse T.Text
asText _ (String s) = Right s
asText what _ = Left (what ++ " is not a string")

asBounded :: (Integral a, Bounded a) => String -> Value -> Parse a
asBounded what (Number n) = maybe (Left (what ++ " is not an integer in range")) Right (toBoundedInteger n)
asBounded what _ = Left (what ++ " is not a number")

field :: String -> K.Key -> KM.KeyMap Value -> Parse Value
field what key o = maybe (Left (what ++ " has no " ++ K.toString key)) Right (KM.lookup key o)

exactKeys :: String -> [K.Key] -> KM.KeyMap Value -> Parse ()
exactKeys what keys o =
  unless (sort (KM.keys o) == sort keys) $
    Left (what ++ " takes exactly " ++ unwords (map K.toString keys))

parseFixture :: Value -> Parse Fixture
parseFixture value = do
  o <- asObject "fixture" value
  schema <- field "fixture" "schema" o >>= asBounded "fixture schema"
  enableReadback <- case schema :: Int of
    1 -> Right Nothing
    2 -> Just <$> (field "fixture" "enable_readback" o >>= asText "fixture enable_readback" >>= parseEnableReadback)
    _ -> Left "fixture schema must be 1 or 2"
  exactKeys "fixture" (["schema", "id", "image_input", "frames", "asset", "raster", "scene", "sensor_period_ticks", "response"] ++ ["enable_readback" | isJust enableReadback]) o
  identifier <- FixtureId <$> (field "fixture" "id" o >>= asText "fixture id")
  node <- NodeName <$> (field "fixture" "image_input" o >>= asText "fixture image_input")
  frames <- FramesName <$> (field "fixture" "frames" o >>= asText "fixture frames")
  asset <- AssetId <$> (field "fixture" "asset" o >>= asText "fixture asset")
  raster <- field "fixture" "raster" o >>= parseRaster
  scene <-
    field "fixture" "scene" o >>= asText "fixture scene" >>= \name ->
      if name == sceneName IndexRamp12 then Right IndexRamp12 else Left ("unknown fixture scene " ++ T.unpack name)
  let bytes = assetBytes raster
      patternBytes = length (scenePattern scene)
  when (bytes `mod` patternBytes /= 0) $
    Left ("the raster's RAW16 asset of " ++ show bytes ++ " bytes is not a whole number of " ++ show patternBytes ++ "-byte patterns")
  period <- Ticks <$> (field "fixture" "sensor_period_ticks" o >>= asBounded "fixture sensor_period_ticks")
  when (period == Ticks 0) (Left "fixture sensor_period_ticks must be positive")
  response <- field "fixture" "response" o >>= parseResponse
  pure (Fixture identifier node frames asset raster scene period response enableReadback)

parseEnableReadback :: T.Text -> Parse EnableReadback
parseEnableReadback name =
  maybe (Left ("unknown fixture enable_readback " ++ T.unpack name)) Right $
    find ((== name) . enableReadbackName) [minBound .. maxBound]

parseRaster :: Value -> Parse Raster
parseRaster value = do
  o <- asObject "raster" value
  exactKeys "raster" ["width", "height"] o
  width <- field "raster" "width" o >>= asBounded "raster width"
  height <- field "raster" "height" o >>= asBounded "raster height"
  when (width <= 0 || height <= 0 || odd width) (Left "raster needs a positive even width and a positive height")
  when (width > (maxAssetBytes `div` 2) `div` height) (Left "the raster's RAW16 asset exceeds 64 MiB")
  pure (Raster width height)

parseResponse :: Value -> Parse Response
parseResponse value = do
  o <- asObject "response" value
  exactKeys "response" ["black", "full_scale", "reference_white"] o
  black <- field "response" "black" o >>= asBounded "response black"
  fullScale <- field "response" "full_scale" o >>= asBounded "response full_scale"
  white <- field "response" "reference_white" o >>= asArray "response reference_white" >>= mapM (asBounded "reference white") . V.toList
  case white of
    [r, g, b] -> pure (Response black fullScale (r, g, b))
    _ -> Left "response reference_white takes three channels"

-- | The X-M1 RAW channel 2 completion line; channel 3 raises the next line.
rawIrq :: Value
rawIrq = object ["core" .= (0 :: Int), "line" .= (36 :: Int)]

derive :: Fixture -> Value -> Parse Value
derive fixture base = do
  let NodeName node = fixtureNode fixture
      FramesName frames = fixtureFrames fixture
      AssetId asset = fixtureAsset fixture
      Ticks period = fixtureSensorPeriod fixture
      Raster width height = fixtureRaster fixture
      Response black fullScale (whiteR, whiteG, whiteB) = fixtureResponse fixture
      named (Object o) = KM.lookup "name" o == Just (String node)
      named _ = False
  document <- asObject "run document" base
  devices <- field "run document" "devices" document >>= asObject "devices"
  nodes <- field "devices" "nodes" devices >>= asArray "device nodes"
  index <- case V.toList (V.findIndices named nodes) of
    [i] -> Right i
    _ -> Left ("the base plan does not have exactly one node " ++ T.unpack node)
  nodeObject <- asObject "image input node" (nodes V.! index)
  model <- field "image input node" "model" nodeObject >>= asObject "image input model"
  kind <- field "image input model" "kind" model >>= asText "image input kind"
  unless (kind == "image-input") (Left (T.unpack node ++ " is not an image-input model"))
  sensor <- field "image input model" "sensor" model
  framePeriod <- field "image input model" "frame_period" model
  unless (sensor == Null && framePeriod == Null) (Left (T.unpack node ++ " already has a sensor or frame-period attachment"))
  irq <- field "image input model" "raw_irq" model
  unless (irq == rawIrq) (Left (T.unpack node ++ " RAW interrupt is not core 0 line 36"))
  options <- field "image input model" "options" model >>= asObject "image input options"
  sensorPeriod <- field "image input options" "sensor_period_ticks" options
  unless (sensorPeriod == Null) (Left (T.unpack node ++ " already has a sensor period"))
  clock <- field "image input options" "clock" options >>= asObject "input clock"
  clockSource <- field "input clock" "source" clock >>= asObject "input clock source"
  sourceKind <- field "input clock source" "kind" clockSource >>= asText "input clock source kind"
  phasePeriod <- field "input clock source" "period_ticks" clockSource >>= asBounded "input clock period"
  unless (sourceKind == "periodic" && phasePeriod == period) $
    Left ("the fixture sensor period " ++ show period ++ " is not the base periodic input clock period")
  insertReadback <- for (fixtureEnableReadback fixture) $ \law -> do
    mode <- field "input clock" "mode" clock >>= asText "input clock mode"
    unless (mode == "input_enable") (Left (T.unpack node ++ " input clock mode is not input_enable"))
    when (KM.member "enable_readback" options) (Left (T.unpack node ++ " already has an enable_readback option"))
    pure (KM.insert "enable_readback" (String (enableReadbackName law)))
  cfa <- field "image input options" "cfa" options
  let sensorAttachment =
        object
          [ "frames" .= frames
          , "options"
              .= object
                [ "width" .= width
                , "height" .= height
                , "image_area" .= [0, 0, width, height]
                , "cfa" .= cfa
                , "black" .= black
                , "full_scale" .= fullScale
                , "reference_white" .= [whiteR, whiteG, whiteB]
                ]
          , "controls" .= Null
          ]
      model' =
        KM.insert "sensor" sensorAttachment $
          KM.insert "options" (Object (fromMaybe id insertReadback (KM.insert "sensor_period_ticks" (Number (fromIntegral period)) options))) model
      nodes' = nodes V.// [(index, Object (KM.insert "model" (Object model') nodeObject))]
  scene <- field "run document" "scene" document >>= asObject "scene"
  sensors <- maybe (Right KM.empty) (asObject "scene sensors") (KM.lookup "sensors" scene)
  when (KM.member (K.fromText frames) sensors) (Left ("the base scene already has sensor frames " ++ T.unpack frames))
  let sceneSensor =
        object ["width" .= width, "height" .= height, "pixels" .= ("raw16" :: T.Text), "asset" .= asset]
      scene' = KM.insert "sensors" (Object (KM.insert (K.fromText frames) sceneSensor sensors)) scene
  assets <- field "run document" "generated_assets" document >>= asArray "generated assets"
  let assetId (Object o) = KM.lookup "id" o
      assetId _ = Nothing
  when (V.any ((== Just (String asset)) . assetId) assets) (Left ("the base run document already generates " ++ T.unpack asset))
  let generated =
        object
          [ "id" .= asset
          , "pattern" .= scenePattern (fixtureScene fixture)
          , "size" .= assetBytes (fixtureRaster fixture)
          ]
  pure . Object $
    KM.insert "generated_assets" (Array (V.snoc assets generated)) $
      KM.insert "scene" (Object scene') $
        KM.insert "devices" (Object (KM.insert "nodes" (Array nodes') devices)) document

identity :: Fixture -> B.ByteString -> B.ByteString -> B.ByteString -> Value
identity fixture fixtureBytes baseBytes derivedBytes =
  object $
    [ "fixture" .= object ["id" .= fixtureText, "sha256" .= hex fixtureBytes]
    , "base_run_document_sha256" .= hex baseBytes
    , "run_document_sha256" .= hex derivedBytes
    , "image_input" .= node
    , "sensor"
        .= object
          [ "frames" .= frames
          , "raster" .= [rasterWidth raster, rasterHeight raster]
          , "pixels" .= ("raw16" :: T.Text)
          , "scene" .= sceneName (fixtureScene fixture)
          , "asset" .= asset
          , "asset_bytes" .= assetBytes raster
          , "sensor_period_ticks" .= period
          ]
    ]
      ++ foldMap (\law -> ["enable_readback" .= enableReadbackName law]) (fixtureEnableReadback fixture)
 where
  FixtureId fixtureText = fixtureId fixture
  NodeName node = fixtureNode fixture
  FramesName frames = fixtureFrames fixture
  AssetId asset = fixtureAsset fixture
  Ticks period = fixtureSensorPeriod fixture
  raster = fixtureRaster fixture
  hex = sha256Hex . sha256

decodeFile :: FilePath -> IO (B.ByteString, Value)
decodeFile path = do
  bytes <- B.readFile path
  either (\e -> die (path ++ ": " ++ e)) (pure . (,) bytes) (decodeStrict bytes)

derivation :: FilePath -> FilePath -> IO (Fixture, B.ByteString, B.ByteString, B.ByteString)
derivation run fixturePath = do
  (fixtureBytes, fixtureValue) <- decodeFile fixturePath
  fixture <- either (\e -> die (fixturePath ++ ": " ++ e)) pure (parseFixture fixtureValue)
  (baseBytes, base) <- decodeFile run
  derived <- either (\e -> die (run ++ ": " ++ e)) pure (derive fixture base)
  pure (fixture, fixtureBytes, baseBytes, encodeCanonical derived <> "\n")

main :: IO ()
main = do
  lookupEnv "BUILD_WORKING_DIRECTORY" >>= mapM_ setCurrentDirectory
  arguments <- getArgs
  case arguments of
    ["write", "--run", run, "--fixture", fixturePath, "--output", output] -> do
      exists <- doesPathExist output
      when exists (die (output ++ " already exists"))
      (fixture, fixtureBytes, baseBytes, derivedBytes) <- derivation run fixturePath
      B.writeFile output derivedBytes
      C8.putStrLn (encodeCanonical (identity fixture fixtureBytes baseBytes derivedBytes))
    ["verify", "--run", run, "--fixture", fixturePath, "--document", document] -> do
      (fixture, fixtureBytes, baseBytes, derivedBytes) <- derivation run fixturePath
      actual <- B.readFile document
      unless (actual == derivedBytes) (die (document ++ " differs from the derivation of " ++ run ++ " and " ++ fixturePath))
      C8.putStrLn (encodeCanonical (identity fixture fixtureBytes baseBytes derivedBytes))
    _ -> die usage
