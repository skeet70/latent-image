-- | Path models of stock machine actions and the dispatcher-entered cases they expand to. An
-- action model reads guest words, calls scripted retained services and stores; every read and
-- every service result branches over the values the stock bytes compare it with plus one they
-- do not, and each resulting path is one case.
module Replacement.PlaybackActor.Model
  ( Width (..)
  , Action
  , call
  , callWith
  , load
  , store
  , packet
  , finish
  , Region (..)
  , Machine (..)
  , machineScenarios
  ) where

import Data.Bits (shiftL, shiftR, xor, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (nub)
import qualified Data.Map.Strict as M
import Data.Word (Word32, Word8)
import Replacement.FileService.Native hiding (Call)
import qualified Replacement.FileService.Native as N

data Width = Byte | Half | Word
  deriving (Eq, Show)

widthBytes :: Width -> Int
widthBytes w = case w of
  Byte -> 1
  Half -> 2
  Word -> 4

data Program
  = Return Word32
  | Invoke Word32 (Maybe Word32) [Word32] (Word32 -> Program)
  | Load Word32 Width [Word32] (Word32 -> Program)
  | Store Word32 Width Word32 Program
  | Packet Word32 Word32 Program

-- | An action model in continuation style; 'finish' ends it with the dispatcher result.
newtype Action a = Action {runAction :: (a -> Program) -> Program}

instance Functor Action where
  fmap f (Action m) = Action (\k -> m (k . f))

instance Applicative Action where
  pure a = Action ($ a)
  Action f <*> Action a = Action (\k -> f (\g -> a (k . g)))

instance Monad Action where
  Action m >>= f = Action (\k -> m (\a -> runAction (f a) k))

-- | A service without an argument, branching over the given results and one other.
call :: Word32 -> [Word32] -> Action Word32
call address results = Action (Invoke address Nothing results)

callWith :: Word32 -> Word32 -> [Word32] -> Action Word32
callWith address argument results = Action (Invoke address (Just argument) results)

-- | A guest read, branching over the given values and the region's own bytes when first read.
load :: Word32 -> Width -> [Word32] -> Action Word32
load address width values = Action (Load address width values)

store :: Word32 -> Width -> Word32 -> Action ()
store address width value = Action (\k -> Store address width value (k ()))

-- | A device notification through 0x72b680 whose frame starts with the two words.
packet :: Word32 -> Word32 -> Action ()
packet code value = Action (\k -> Packet code value (k ()))

finish :: Word32 -> Action a
finish result = Action (const (Return result))

-- | Guest data a machine's cases map. Bytes no case sets hold the fill: zero, or the low
-- address byte xor 0x5c when patterned.
data Region = Region
  { regionName :: String
  , regionBase :: Word32
  , regionSize :: Int
  , regionPatterned :: Bool
  }

data Machine = Machine
  { machineName :: String
  , machineIndex :: Word32
  , machineTransitions :: Word32
  , machineStates :: Int
  , machineEvents :: Int
  , machineRegions :: [Region]
  , machineActions :: [Action Word32]
  }

dispatcher, deviceNotification, machinesBase, modeValueAddress, modeValue :: Word32
dispatcher = 0x011e8bf8
deviceNotification = 0x0072b680
machinesBase = 0x014ea4ec
modeValueAddress = 0x014ea694
modeValue = 0x5a3c0081

-- | The r0 a service returns when no compared value is chosen.
marker :: Word32 -> Word32
marker address = 0x5e000000 .|. (address .&. 0x00ffffff)

data Path = Path
  { pathInitial :: M.Map Word32 Word8
  , pathMemory :: M.Map Word32 Word8
  , pathServices :: [Service]
  }

bytesOf :: Width -> Word32 -> [Word8]
bytesOf width value = [fromIntegral (value `shiftR` (8 * i)) | i <- [0 .. widthBytes width - 1]]

valueOf :: [Word8] -> Word32
valueOf bytes = foldr (\b acc -> acc `shiftL` 8 .|. fromIntegral b) 0 bytes

poke :: Word32 -> Width -> Word32 -> M.Map Word32 Word8 -> M.Map Word32 Word8
poke address width value memory = foldr (uncurry M.insert) memory (zip [address ..] (bytesOf width value))

fill :: [Region] -> Word32 -> Word8
fill regions address = case [r | r <- regions, regionBase r <= address, address < regionBase r + fromIntegral (regionSize r)] of
  r : _ | regionPatterned r -> fromIntegral (address .&. 0xff) `xor` 0x5c
  _ : _ -> 0
  [] -> error ("model reads outside its regions at " ++ show address)

explore :: [Region] -> Path -> Program -> [(Path, Word32)]
explore regions path program = case program of
  Return result -> [(path, result)]
  Invoke address argument results k ->
    [ (final, result)
    | value <- nub (results ++ [marker address])
    , let service = Service address (maybe [] (\a -> [("r0", a)]) argument) [("r0", value)] [] []
    , (final, result) <- explore regions path {pathServices = service : pathServices path} (k value)
    ]
  Load address width values k ->
    let span' = [address .. address + fromIntegral (widthBytes width) - 1]
        known = [M.lookup a (pathMemory path) | a <- span']
     in case sequence known of
          Just bytes -> explore regions path (k (valueOf bytes))
          Nothing ->
            [ (final, result)
            | value <- nub (values ++ [valueOf (map (fill regions) span')])
            , let path' = path {pathInitial = poke address width value (pathInitial path), pathMemory = poke address width value (pathMemory path)}
            , (final, result) <- explore regions path' (k value)
            ]
  Store address width value k -> explore regions path {pathMemory = poke address width value (pathMemory path)} k
  Packet code value k ->
    let service = Service deviceNotification [] [("r0", marker deviceNotification)] [AtRegister "r0" 0 (words32le [code, value])] []
     in explore regions path {pathServices = service : pathServices path} k

-- | The event and state that make the dispatcher select table index i: the first transition
-- cell naming it.
selector :: B.ByteString -> Machine -> Int -> (Word32, Word32)
selector image machine index =
  case [ (fromIntegral (cell `div` states), fromIntegral (cell `mod` states))
       | cell <- [0 .. states * machineEvents machine - 1]
       , fromIntegral (B.index image (offset + 2 * cell)) == index
       ] of
    first : _ -> first
    [] -> error (machineName machine ++ ": no transition selects action " ++ show index)
  where
    states = machineStates machine
    offset = fromIntegral (machineTransitions machine - 0x01100000)

-- | Every path of every action (table indices 2 onwards), entered through the dispatcher.
machineScenarios :: B.ByteString -> Machine -> [Scenario]
machineScenarios image machine = concat (zipWith cases [2 ..] (machineActions machine))
  where
    regions = machineRegions machine
    eventAddress = machinesBase + 2 * machineIndex machine
    stateAddress = machinesBase + 0xc0 + 2 * machineIndex machine
    cases index action =
      let (event, state) = selector image machine index
          seeded = poke modeValueAddress Word modeValue (poke stateAddress Half state (poke eventAddress Half event M.empty))
          paths = explore regions (Path seeded seeded []) (runAction action Return)
       in [ scenario (machineName machine ++ "-action" ++ show index ++ "-path" ++ show n) path result
          | (n, (path, result)) <- zip [1 :: Int ..] paths
          ]
    bytes memory r = B.pack [M.findWithDefault (fill regions a) a memory | a <- [regionBase r .. regionBase r + fromIntegral (regionSize r) - 1]]
    scenario name path result =
      Scenario
        { scenarioName = name
        , scenarioEntry = dispatcher
        , scenarioData = [DataRegion (regionName r) (regionBase r) (bytes (pathInitial path) r) (bytes (pathMemory path) r) | r <- regions]
        , scenarioCalls = [N.Call dispatcher [machineIndex machine] [] True]
        , scenarioServices = reverse (pathServices path)
        , scenarioResults = [result]
        , scenarioStop = Nothing
        }
