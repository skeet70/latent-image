{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}

-- | Original-versus-owned differential qualification for FUN_00bfb028.
module Replacement.ModalRuntime.Qualify
  ( Options (..)
  , runQualification
  ) where

import Control.Exception (SomeException, try)
import Control.Monad (forM_, unless, when)
import Data.Aeson (Value (..), object, (.=))
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Int (Int16)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe)
import qualified Data.Set as S
import Data.String (fromString)
import Data.Word (Word16, Word32)
import Emulation.NativeCase (Region (..), loadCase)
import Emulation.NativeObservation
  ( GuestAddress (..)
  , NativeObservation (..)
  , Provenance (..)
  , ProvenanceTotal (..)
  , Register (..)
  , RegionId (..)
  , RunnerStatus (..)
  , Write (..)
  , WriteBytes (..)
  , lookupRegister
  , observeNative
  , parseWord32
  , regionIdOf
  , registerLabel
  , renderObservationError
  , runnerStatusLabel
  )
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host (ensure, get, jsonRead, num, runBounded)
import FujiAF.JSON (encodeCanonical)
import Numeric (showHex)
import System.Directory (createDirectoryIfMissing, doesPathExist)
import System.Exit (ExitCode (..))
import System.FilePath (takeDirectory, (</>))

data Options = Options
  { optionRunner :: FilePath
  , optionBuild :: FilePath
  , optionOutput :: FilePath
  , optionId5 :: FilePath
  , optionId5Base :: Word32
  , optionKernelTextSha256 :: String
  , optionKernelSymsSha256 :: String
  }

controllerId :: Word32
controllerId = 75

stockEntry, bodyPlusFour, prehookAddr, noopAddr :: Word32
stockEntry = 0x00bfb028
bodyPlusFour = 0x00bfb02c
prehookAddr = 0x00d0cc68
noopAddr = 0x00d3cde8

stockSize, prehookSize, noopSize :: Int
stockSize = 0xf8
prehookSize = 0x34
noopSize = 0x14

cellPointers, actionPointers, transitionPointers :: Word32
cellPointers = 0x00cf58b8
actionPointers = 0x00cfe2a8
transitionPointers = 0x00d0013c

eventCells, widthCells, stateCells, shadowBase :: Word32
eventCells = 0x014676e4
widthCells = 0x00cd5e78
stateCells = 0x01467588
shadowBase = 0x0095037e

fixtureRamBase, actionTableBase, transitionTableBase :: Word32
fixtureRamBase = 0x00970000
actionTableBase = fixtureRamBase + 0x1000
transitionTableBase = fixtureRamBase + 0x2000

bogusTransitionBase, bogusCellBase, logBase, prehookLogBase :: Word32
bogusTransitionBase = fixtureRamBase + 0x2400
bogusCellBase = fixtureRamBase + 0x0800
logBase = fixtureRamBase + 0x3000
prehookLogBase = fixtureRamBase + 0x3040

fixtureCodeBase, actionFunctionAddr, transitionFunctionAddr :: Word32
fixtureCodeBase = 0x00978000
actionFunctionAddr = fixtureCodeBase
transitionFunctionAddr = fixtureCodeBase + 0x200

stackBase, stackSize :: Word32
stackBase = 0x00979000
stackSize = 0x1000

stackTop, exitAddr :: Word32
stackTop = stackBase + stackSize - 0x10
exitAddr = 0x0097a000

shadowSentinel :: Word16
shadowSentinel = 0xbbbb

logSentinel :: Word32
logSentinel = 0xdddddddd

id5BaseValue, id5Size :: Word32
id5BaseValue = 0x00bf1a00
id5Size = 5185120

kernelTextLimit, kernelSymsLimit :: Int
kernelTextLimit = 0xa300
kernelSymsLimit = 1024 * 1024

id5Digest :: String
id5Digest = "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"

bxLr :: Word32
bxLr = 0xe12fff1e

hex :: Word32 -> String
hex value = let digits = showHex value "" in "0x" ++ replicate (8 - length digits) '0' ++ digits

word32le :: Word32 -> B.ByteString
word32le word = B.pack
  [ fromIntegral word
  , fromIntegral (word `shiftR` 8)
  , fromIntegral (word `shiftR` 16)
  , fromIntegral (word `shiftR` 24)
  ]

assemble :: [Word32] -> B.ByteString
assemble = B.concat . map word32le

patchWord :: B.ByteString -> Int -> Word32 -> B.ByteString
patchWord bytes offset word =
  B.take offset bytes <> word32le word <> B.drop (offset + 4) bytes

patchHalf :: B.ByteString -> Int -> Word16 -> B.ByteString
patchHalf bytes offset word =
  B.take offset bytes <> B.pack [fromIntegral word, fromIntegral (word `shiftR` 8)] <> B.drop (offset + 2) bytes

sliceAt :: B.ByteString -> Word32 -> Word32 -> Int -> B.ByteString
sliceAt bytes base address size =
  let offset = fromIntegral (address - base)
   in if offset < 0 || offset + size > B.length bytes
        then error ("stock slice outside id5: " ++ hex address ++ " size " ++ show size)
        else B.take size (B.drop offset bytes)

readWordAt :: B.ByteString -> Int -> Word32
readWordAt bytes offset
  | offset < 0 || offset + 4 > B.length bytes = error ("word read outside region at " ++ show offset)
  | otherwise =
      fromIntegral (B.index bytes offset)
        + fromIntegral (B.index bytes (offset + 1)) * 0x100
        + fromIntegral (B.index bytes (offset + 2)) * 0x10000
        + fromIntegral (B.index bytes (offset + 3)) * 0x1000000

readHalfwordAt :: B.ByteString -> Int -> Word16
readHalfwordAt bytes offset
  | offset < 0 || offset + 2 > B.length bytes = error ("halfword read outside region at " ++ show offset)
  | otherwise =
      fromIntegral (B.index bytes offset) + fromIntegral (B.index bytes (offset + 1)) * 0x100

movw :: Int -> Word32 -> Word32
movw register immediate =
  0xe3000000
    .|. (fromIntegral register `shiftL` 12)
    .|. ((immediate .&. 0xf000) `shiftL` 4)
    .|. (immediate .&. 0x0fff)

movt :: Int -> Word32 -> Word32
movt register immediate =
  0xe3400000
    .|. (fromIntegral register `shiftL` 12)
    .|. ((immediate .&. 0xf000) `shiftL` 4)
    .|. (immediate .&. 0x0fff)

movImm :: Int -> Word32 -> [Word32]
movImm register value = [movw register (value .&. 0xffff), movt register (value `shiftR` 16)]

strImmediate :: Int -> Int -> Word32
strImmediate register offset = 0xe58c0000 .|. (fromIntegral register `shiftL` 12) .|. fromIntegral offset

strhImmediate :: Int -> Int -> Word32
strhImmediate register offset = 0xe1cc00b0 .|. (fromIntegral register `shiftL` 12) .|. fromIntegral offset

branchWord :: Word32 -> Word32 -> Word32
branchWord site target =
  let delta = (target - site - 8) .&. 0xffffffff
   in 0xea000000 .|. ((delta `shiftR` 2) .&. 0xffffff)

data Mutation = Mutation
  { mutationHigh :: Maybe Int16
  , mutationCellBase :: Maybe Word32
  , mutationActionBase :: Maybe Word32
  , mutationTransitionBase :: Maybe Word32
  }

data ActionPlan
  = NoAction
  | ActionCall Word32 Word32 (Maybe Mutation)

data Fault
  = NoFault
  | FaultReturn Word32
  | FaultHigh Int16

data Scenario = Scenario
  { scenarioId :: String
  , scenarioEvent :: Word16
  , scenarioWidth :: Word16
  , scenarioState :: Word16
  , scenarioCellBase :: Maybe Word32
  , scenarioCellIndex :: Word16
  , scenarioActionSlot :: Word16
  , scenarioInitialHigh :: Int16
  , scenarioAction :: ActionPlan
  , scenarioTransition :: Bool
  , scenarioTransitionReturn :: Word32
  , scenarioDecoyTransition :: Bool
  , scenarioProbePrehook :: Bool
  , scenarioFault :: Fault
  }

cellAddress :: Scenario -> Word32
cellAddress scenario =
  fromMaybe 0 (scenarioCellBase scenario) + 4 * fromIntegral (scenarioCellIndex scenario)

baseHigh :: Scenario -> Int16
baseHigh scenario =
  case scenarioAction scenario of
    ActionCall _ _ (Just mutation) -> fromMaybe (scenarioInitialHigh scenario) (mutationHigh mutation)
    _ -> scenarioInitialHigh scenario

highAfterAction :: Scenario -> Int16
highAfterAction scenario =
  case scenarioFault scenario of
    FaultHigh high -> high
    _ -> baseHigh scenario

mutationHighOf :: Scenario -> Maybe Int16
mutationHighOf scenario =
  case scenarioFault scenario of
    FaultHigh high -> Just high
    _ ->
      case scenarioAction scenario of
        ActionCall _ _ (Just mutation) -> mutationHigh mutation
        _ -> Nothing

actionPresent :: Scenario -> Bool
actionPresent scenario =
  case scenarioAction scenario of
    NoAction -> False
    ActionCall _ _ _ -> True

transitionCalled :: Scenario -> Bool
transitionCalled scenario = highAfterAction scenario > 0 && scenarioTransition scenario

initialPointers :: Scenario -> (Word32, Word32, Word32)
initialPointers scenario = (fromMaybe 0 (scenarioCellBase scenario), actionTableBase, transitionTableBase)

mutatedPointers :: Scenario -> (Word32, Word32, Word32)
mutatedPointers scenario =
  case scenarioAction scenario of
    ActionCall _ _ (Just mutation) ->
      let (cell, action, transition) = initialPointers scenario
       in ( fromMaybe cell (mutationCellBase mutation)
          , fromMaybe action (mutationActionBase mutation)
          , fromMaybe transition (mutationTransitionBase mutation)
          )
    _ -> initialPointers scenario

data Expected = Expected
  { expectedReturn :: Word32
  , expectedState :: Word16
  , expectedShadow :: [Word16]
  , expectedActionLog :: [Word32]
  , expectedTransitionLog :: [Word32]
  , expectedPrehookLog :: Maybe (Word32, Word32)
  , expectedCellHigh :: Word16
  , expectedPointers :: (Word32, Word32, Word32)
  }

expectedOf :: Scenario -> Expected
expectedOf scenario =
  case scenarioCellBase scenario of
    Nothing ->
      Expected
        { expectedReturn = 0
        , expectedState = scenarioState scenario
        , expectedShadow = replicate 3 shadowSentinel
        , expectedActionLog = replicate 4 logSentinel
        , expectedTransitionLog = replicate 4 logSentinel
        , expectedPrehookLog = Nothing
        , expectedCellHigh = fromIntegral (scenarioInitialHigh scenario)
        , expectedPointers = initialPointers scenario
        }
    Just _ ->
      let high = highAfterAction scenario
          actionLog =
            case scenarioAction scenario of
              NoAction -> replicate 4 logSentinel
              ActionCall _ _ _ ->
                [controllerId, 1, fromIntegral (scenarioEvent scenario), actionFunctionAddr]
          transitionLog
            | transitionCalled scenario =
                case scenarioAction scenario of
                  ActionCall result leftover _ -> [result, leftover, transitionTableBase, transitionFunctionAddr]
                  NoAction -> [controllerId, 1, transitionTableBase, transitionFunctionAddr]
            | otherwise = replicate 4 logSentinel
          prehookLog
            | scenarioProbePrehook scenario =
                Just (fromIntegral (scenarioState scenario), fromIntegral (scenarioEvent scenario))
            | otherwise = Nothing
       in Expected
            { expectedReturn =
                case scenarioAction scenario of
                  NoAction -> 0
                  ActionCall result _ _ -> result
            , expectedState =
                if high <= 0
                  then fromIntegral ((negate (fromIntegral high :: Integer)) `mod` 65536)
                  else scenarioState scenario
            , expectedShadow = [fromIntegral controllerId, scenarioState scenario, scenarioEvent scenario]
            , expectedActionLog = actionLog
            , expectedTransitionLog = transitionLog
            , expectedPrehookLog = prehookLog
            , expectedCellHigh = fromIntegral high
            , expectedPointers = mutatedPointers scenario
            }

modeledPrehook :: [Word32]
modeledPrehook =
  concat
    [ movImm 12 prehookLogBase
    , [strImmediate 1 0, strImmediate 2 4]
    , movImm 12 shadowBase
    , [strhImmediate 0 0, strhImmediate 1 2, strhImmediate 2 4]
    , [0xe3a01001]
    , movImm 3 0x00944580
    , [bxLr]
    ]

actionFunction :: Scenario -> [Word32]
actionFunction scenario =
  case scenarioAction scenario of
    NoAction -> [bxLr]
    ActionCall result leftover mutation ->
      let returned =
            case scenarioFault scenario of
              FaultReturn wrong -> wrong
              _ -> result
          mutated =
            case scenarioFault scenario of
              FaultHigh high -> Just (withHigh mutation high)
              _ -> mutation
       in concat
            [ movImm 12 logBase
            , [strImmediate 0 0, strImmediate 1 4, strImmediate 2 8, strImmediate 3 12]
            , mutationWords scenario mutated
            , movImm 0 returned
            , movImm 1 leftover
            , [bxLr]
            ]
 where
  withHigh Nothing high = Mutation (Just high) Nothing Nothing Nothing
  withHigh (Just mutation) high = mutation { mutationHigh = Just high }

mutationWords :: Scenario -> Maybe Mutation -> [Word32]
mutationWords _ Nothing = []
mutationWords scenario (Just mutation) =
  highWrite ++ cellWrite ++ actionWrite ++ transitionWrite
 where
  highWrite =
    case mutationHigh mutation of
      Nothing -> []
      Just high ->
        movImm 12 (cellAddress scenario + 2)
          ++ movImm 2 (fromIntegral high .&. 0xffff)
          ++ [strhImmediate 2 0]
  cellWrite =
    case mutationCellBase mutation of
      Nothing -> []
      Just value -> movImm 12 (cellPointers + 4 * controllerId) ++ movImm 2 value ++ [strImmediate 2 0]
  actionWrite =
    case mutationActionBase mutation of
      Nothing -> []
      Just value -> movImm 12 (actionPointers + 4 * controllerId) ++ movImm 2 value ++ [strImmediate 2 0]
  transitionWrite =
    case mutationTransitionBase mutation of
      Nothing -> []
      Just value -> movImm 12 (transitionPointers + 4 * controllerId) ++ movImm 2 value ++ [strImmediate 2 0]

transitionFunction :: Scenario -> [Word32]
transitionFunction scenario =
  concat
    [ movImm 12 (logBase + 16)
    , [strImmediate 0 0, strImmediate 1 4, strImmediate 2 8, strImmediate 3 12]
    , movImm 0 (scenarioTransitionReturn scenario)
    , [bxLr]
    ]

fixtureBytes :: Scenario -> B.ByteString
fixtureBytes scenario =
  let empty = B.replicate 0x4000 0
      index = fromIntegral (scenarioCellIndex scenario)
      cell =
        case scenarioCellBase scenario of
          Nothing -> empty
          Just _ -> patchHalf (patchHalf empty (4 * index) (scenarioActionSlot scenario)) (4 * index + 2) (fromIntegral (scenarioInitialHigh scenario))
      actionEntry =
        case scenarioAction scenario of
          NoAction -> cell
          ActionCall _ _ _ -> patchWord cell (0x1000 + 4 * fromIntegral (scenarioActionSlot scenario)) actionFunctionAddr
      high = highAfterAction scenario
      transitionEntry bytes
        | scenarioTransition scenario && high > 0 =
            patchWord bytes (0x2000 + 4 * fromIntegral high) transitionFunctionAddr
        | otherwise = bytes
      decoy bytes
        | scenarioDecoyTransition scenario
        , let initial = scenarioInitialHigh scenario
        , initial > 0
        , initial /= high =
            patchWord bytes (0x2000 + 4 * fromIntegral initial) transitionFunctionAddr
        | otherwise = bytes
      bogusTransition bytes =
        case scenarioAction scenario of
          ActionCall _ _ (Just mutation)
            | Just _ <- mutationTransitionBase mutation
            , high > 0 ->
                patchWord bytes (0x2400 + 4 * fromIntegral high) transitionFunctionAddr
          _ -> bytes
      bogusCell bytes =
        case scenarioAction scenario of
          ActionCall _ _ (Just mutation)
            | Just _ <- mutationCellBase mutation ->
                patchHalf bytes (0x0800 + 4 * index + 2) 0xffff
          _ -> bytes
      logged = foldl (\bytes (offset, word) -> patchWord bytes (0x3000 + offset) word) (bogusCell (bogusTransition (decoy (transitionEntry actionEntry)))) (zip [0,4 ..] (replicate 8 logSentinel))
      probed =
        if scenarioProbePrehook scenario
          then patchWord (patchWord logged 0x3040 logSentinel) 0x3044 logSentinel
          else logged
   in probed

fixtureCodeBytes :: Scenario -> B.ByteString
fixtureCodeBytes scenario =
  let action = assemble (actionFunction scenario)
      transition = assemble (transitionFunction scenario)
   in if B.length action > 0x200
        then error "action fixture exceeds its slot"
        else B.concat [action, B.replicate (0x200 - B.length action) 0, transition, B.replicate (0x1000 - 0x200 - B.length transition) 0]

pointerTableBytes :: Word32 -> B.ByteString
pointerTableBytes value = patchWord (B.replicate 0x200 0) (4 * fromIntegral controllerId) value

stateCellBytes :: Scenario -> B.ByteString
stateCellBytes scenario = patchHalf (B.replicate 0x100 0) (2 * fromIntegral controllerId) (scenarioState scenario)

eventCellBytes :: Scenario -> B.ByteString
eventCellBytes scenario = patchHalf (B.replicate 0x100 0) (2 * fromIntegral controllerId) (scenarioEvent scenario)

widthCellBytes :: Scenario -> B.ByteString
widthCellBytes scenario = patchHalf (B.replicate 0x100 0) (2 * fromIntegral controllerId) (scenarioWidth scenario)

shadowBytes :: B.ByteString
shadowBytes = B.concat (replicate 4 (B.pack [0xbb, 0xbb]))

data Permission = ReadOnly | ReadWrite | ReadExecute

permissionLabel :: Permission -> String
permissionLabel ReadOnly = "r"
permissionLabel ReadWrite = "rw"
permissionLabel ReadExecute = "rx"

data CaseRegion = CaseRegion Region Permission

mkRegion :: RegionId -> Word32 -> Int -> Permission -> B.ByteString -> CaseRegion
mkRegion ident base size permission bytes =
  CaseRegion
    ( Region
        { description = object
            [ "id" .= regionName ident
            , "address" .= hex base
            , "size" .= size
            , "permissions" .= permissionLabel permission
            ]
        , baseAddress = fromIntegral base
        , regionSize = fromIntegral size
        , regionBytes = bytes
        }
    )
    permission

data Variant = StockVariant | OwnedVariant

data CaseSpec = CaseSpec
  { caseSpecId :: String
  , caseSpecEntry :: Word32
  , caseSpecRegions :: [CaseRegion]
  , caseSpecFunctions :: [(Word32, Word32)]
  , caseSpecStatus :: RunnerStatus
  }

caseValue :: CaseSpec -> Value
caseValue spec =
  object
    [ "schema_version" .= (1 :: Int)
    , "id" .= caseSpecId spec
    , "origin" .= ("synthetic" :: String)
    , "provenance" .= ("owned modal dispatcher differential: X-M1 1.50 FUN_00bfb028 stock body against rb_modal_dispatch with the same retained prehook, tables and generated callbacks" :: String)
    , "cpu" .= object ["architecture" .= ("arm" :: String), "model" .= ("cortex-r5f" :: String), "mode" .= ("arm" :: String), "endian" .= ("little" :: String)]
    , "artifacts" .= [artifactJson region | CaseRegion region _ <- caseSpecRegions spec, not (B.all (== 0) (regionBytes region))]
    , "entry" .= hex (caseSpecEntry spec)
    , "exit" .= hex exitAddr
    , "registers" .= object [fromString (registerLabel register) .= hex value | (register, value) <- initialRegisters]
    , "regions" .= map regionJson (caseSpecRegions spec)
    , "functions" .= [object ["address" .= hex address, "size" .= size] | (address, size) <- caseSpecFunctions spec]
    , "stubs" .= ([] :: [Value])
    , "limits" .= object ["instructions" .= (4000000 :: Int), "wall_ms" .= (4000 :: Int)]
    , "expected" .=
        object
          [ "status" .= runnerStatusLabel (caseSpecStatus spec)
          , "registers" .= object [fromString (registerLabel Sp) .= hex stackTop]
          , "memory" .= ([] :: [Value])
          , "events" .= ([] :: [Value])
          ]
    , "exclusions" .= ["single dispatcher call; retained prehook, tables and generated action/transition callbacks; no scheduler, interrupt, frame or device path"]
    ]

regionJson :: CaseRegion -> Value
regionJson (CaseRegion region permission) =
  object
    [ "id" .= regionName (regionIdOf region)
    , "address" .= hex (fromIntegral (baseAddress region))
    , "size" .= regionSize region
    , "permissions" .= permissionLabel permission
    , "initial" .=
        ( if B.all (== 0) (regionBytes region)
            then object ["zero" .= True]
            else object
              [ "artifact" .= regionName (regionIdOf region)
              , "offset" .= (0 :: Int)
              , "length" .= B.length (regionBytes region)
              ]
        )
    ]

artifactJson :: CaseRegion -> Value
artifactJson (CaseRegion region _) =
  object
    [ "id" .= regionName (regionIdOf region)
    , "path" .= (regionName (regionIdOf region) ++ ".bin")
    , "size" .= B.length (regionBytes region)
    , "sha256" .= sha256Hex (sha256 (regionBytes region))
    ]

writeCase :: FilePath -> CaseSpec -> IO FilePath
writeCase directory spec = do
  createDirectoryIfMissing True directory
  forM_ (caseSpecRegions spec) $ \(CaseRegion region _) ->
    when (not (B.all (== 0) (regionBytes region))) $
      B.writeFile (directory </> (regionName (regionIdOf region) ++ ".bin")) (regionBytes region)
  let path = directory </> "case.json"
  B.writeFile path (encodeCanonical (caseValue spec))
  pure path

initialRegisters :: [(Register, Word32)]
initialRegisters =
  [ (R0, controllerId)
  , (R1, 0x11110001)
  , (R2, 0x11110002)
  , (R3, 0x11110003)
  , (R4, 0xa5a50004)
  , (R5, 0xa5a50005)
  , (R6, 0xa5a50006)
  , (R7, 0xa5a50007)
  , (R8, 0xa5a50008)
  , (R9, 0xa5a50009)
  , (R10, 0xa5a5000a)
  , (R11, 0xa5a5000b)
  , (R12, 0x1111000c)
  , (Sp, stackTop)
  , (Lr, exitAddr)
  , (Cpsr, 0x10)
  ]

preservedRegisters :: [Register]
preservedRegisters = [R0, R4, R5, R6, R7, R8, R9, R10, R11, Sp, Pc]

sharedRegions :: [RegionId]
sharedRegions =
  [ RegionId "fixtures"
  , RegionId "cell-pointers"
  , RegionId "action-pointers"
  , RegionId "transition-pointers"
  , RegionId "state-cells"
  , RegionId "event-cells"
  , RegionId "width-cells"
  , RegionId "shadow"
  ]

data Context = Context
  { contextRunner :: FilePath
  , contextOutput :: FilePath
  , contextSymbols :: M.Map String Symbol
  , contextKernelText :: B.ByteString
  , contextStock :: B.ByteString
  , contextPrehook :: B.ByteString
  , contextNoop :: B.ByteString
  }

data Symbol = Symbol Word32 Word32

requiredSymbols :: [String]
requiredSymbols =
  [ "__rb_image_start"
  , "rb_modal_dispatch"
  , "rb_modal_call_prehook"
  , "rb_modal_call_action"
  , "rb_modal_call_transition"
  ]

parseSymbols :: [String] -> String -> M.Map String Symbol
parseSymbols required contents =
  foldl add M.empty (zip [1 :: Int ..] (lines contents))
 where
  add symbols (lineNumber, line) = case words line of
    [address, size, _kind, name] -> insert symbols lineNumber name address size
    [address, _kind, name] -> insert symbols lineNumber name address "0"
    _ -> symbols
  insert symbols lineNumber name address size
    | name `notElem` required = symbols
    | otherwise =
        case (parseWord32 address, parseWord32 size) of
          (Just value, Just extent)
            | M.member name symbols -> error ("duplicate symbol " ++ name ++ " at line " ++ show lineNumber)
            | otherwise -> M.insert name (Symbol value extent) symbols
          _ -> error ("invalid symbol line " ++ show lineNumber)

symbolAddress :: Context -> String -> Word32
symbolAddress context name = case M.lookup name (contextSymbols context) of
  Just (Symbol address _) -> address
  Nothing -> error ("kernel symbol missing: " ++ name)

symbolFunction :: Context -> String -> (Word32, Word32)
symbolFunction context name = case M.lookup name (contextSymbols context) of
  Just (Symbol address size)
    | size == 0 -> error ("zero-size function symbol " ++ name)
    | otherwise -> (address, size)
  Nothing -> error ("kernel symbol missing: " ++ name)

commonRegions :: Context -> Scenario -> [CaseRegion]
commonRegions context scenario =
  [ mkRegion (RegionId "prehook") prehookAddr prehookSize ReadExecute (prehookBytes context scenario)
  , mkRegion (RegionId "noop") noopAddr noopSize ReadExecute (contextNoop context)
  , mkRegion (RegionId "cell-pointers") cellPointers 0x200 ReadWrite (pointerTableBytes (fromMaybe 0 (scenarioCellBase scenario)))
  , mkRegion (RegionId "action-pointers") actionPointers 0x200 ReadWrite (pointerTableBytes actionTableBase)
  , mkRegion (RegionId "transition-pointers") transitionPointers 0x200 ReadWrite (pointerTableBytes transitionTableBase)
  , mkRegion (RegionId "fixtures") fixtureRamBase 0x4000 ReadWrite (fixtureBytes scenario)
  , mkRegion (RegionId "fixture-code") fixtureCodeBase 0x1000 ReadExecute (fixtureCodeBytes scenario)
  , mkRegion (RegionId "stack") stackBase (fromIntegral stackSize) ReadWrite (B.replicate (fromIntegral stackSize) 0)
  , mkRegion (RegionId "exit") exitAddr 4 ReadExecute (word32le bxLr)
  , mkRegion (RegionId "shadow") shadowBase 8 ReadWrite shadowBytes
  , mkRegion (RegionId "state-cells") stateCells 0x100 ReadWrite (stateCellBytes scenario)
  , mkRegion (RegionId "event-cells") eventCells 0x100 ReadOnly (eventCellBytes scenario)
  , mkRegion (RegionId "width-cells") widthCells 0x100 ReadOnly (widthCellBytes scenario)
  ]

prehookBytes :: Context -> Scenario -> B.ByteString
prehookBytes context scenario =
  if scenarioProbePrehook scenario then assemble modeledPrehook else contextPrehook context

fixtureFunctionSizes :: Scenario -> [(Word32, Word32)]
fixtureFunctionSizes scenario =
  [ (actionFunctionAddr, fromIntegral (length (actionFunction scenario)) * 4)
  , (transitionFunctionAddr, fromIntegral (length (transitionFunction scenario)) * 4)
  ]

buildCase :: Context -> Variant -> Scenario -> CaseSpec
buildCase context variant scenario =
  case variant of
    StockVariant ->
      CaseSpec
        { caseSpecId = scenarioId scenario ++ "-stock"
        , caseSpecEntry = stockEntry
        , caseSpecRegions = commonRegions context scenario ++ [mkRegion (RegionId "stock") stockEntry stockSize ReadExecute (contextStock context)]
        , caseSpecFunctions = (stockEntry, fromIntegral stockSize) : (prehookAddr, fromIntegral prehookSize) : (noopAddr, fromIntegral noopSize) : fixtureFunctionSizes scenario
        , caseSpecStatus = Returned
        }
    OwnedVariant ->
      let imageStart = symbolAddress context "__rb_image_start"
          entry = fst (symbolFunction context "rb_modal_dispatch")
       in CaseSpec
            { caseSpecId = scenarioId scenario ++ "-owned"
            , caseSpecEntry = entry
            , caseSpecRegions = commonRegions context scenario ++ [mkRegion (RegionId "ktext") imageStart (B.length (contextKernelText context)) ReadExecute (contextKernelText context)]
            , caseSpecFunctions =
                [ symbolFunction context "rb_modal_dispatch"
                , symbolFunction context "rb_modal_call_prehook"
                , symbolFunction context "rb_modal_call_action"
                , symbolFunction context "rb_modal_call_transition"
                , (prehookAddr, fromIntegral prehookSize)
                , (noopAddr, fromIntegral noopSize)
                ]
                  ++ fixtureFunctionSizes scenario
            , caseSpecStatus = Returned
            }

loadObserved :: FilePath -> FilePath -> IO NativeObservation
loadObserved runner path = do
  (caseDescription, regions, wire, _) <- loadCase path
  let wall = num (get (get caseDescription "limits") "wall_ms")
      directory = takeDirectory path
  (timed, exit, out, err) <- runBounded (fromInteger (wall * 1000 + 2000000)) (8 * 1024 * 1024) runner [] wire
  B.writeFile (directory </> "runner-stdout.bin") out
  B.writeFile (directory </> "runner-stderr.bin") err
  B.writeFile (directory </> "runner-exit.txt") (BC.pack (show exit ++ "\n"))
  when timed $ error "native runner host timeout"
  value <- jsonRead out
  ensure (get value "wire_version" == Number 1) "invalid runner output"
  observation <- either (error . renderObservationError) pure (observeNative exit regions value)
  when (nativeStatus observation /= Returned && exit == ExitSuccess) $
    error "contradictory runner status and process exit"
  B.writeFile (directory </> "runner-observation.json") (encodeCanonical value)
  pure observation

registerValue :: NativeObservation -> Register -> Word32
registerValue observation register =
  fromMaybe (error ("missing register " ++ registerLabel register)) (lookupRegister register observation)

regionBytesOf :: NativeObservation -> RegionId -> B.ByteString
regionBytesOf observation ident =
  fromMaybe (error ("missing memory region " ++ regionName ident)) (lookup ident (nativeMemory observation))

checkExpected :: Scenario -> Expected -> NativeObservation -> [String]
checkExpected scenario expected observation
  | nativeStatus observation /= Returned =
      ["status " ++ runnerStatusLabel (nativeStatus observation) ++ ", not returned"]
  | otherwise =
      returnProblems
        ++ stateProblem
        ++ shadowProblems
        ++ prehookLogProblems
        ++ logProblems
        ++ cellProblem
        ++ pointerProblems
        ++ cellInputProblems
        ++ writeProblems
 where
  expectedRegisters =
    [ (R0, expectedReturn expected)
    , (R4, 0xa5a50004)
    , (R5, 0xa5a50005)
    , (R6, 0xa5a50006)
    , (R7, 0xa5a50007)
    , (R8, 0xa5a50008)
    , (R9, 0xa5a50009)
    , (R10, 0xa5a5000a)
    , (R11, 0xa5a5000b)
    , (Sp, stackTop)
    , (Pc, exitAddr)
    ]
  returnProblems =
    [ "register " ++ registerLabel register ++ " is " ++ hex (registerValue observation register)
        ++ ", not " ++ hex value
    | (register, value) <- expectedRegisters
    , registerValue observation register /= value
    ]
      ++ [ "control state " ++ hex (registerValue observation Cpsr .&. 0x3ff) ++ ", not " ++ hex 0x10
         | registerValue observation Cpsr .&. 0x3ff /= 0x10
         ]
  stateWord = readHalfwordAt (regionBytesOf observation (RegionId "state-cells")) (2 * fromIntegral controllerId)
  stateProblem =
    [ "state " ++ hex (fromIntegral stateWord) ++ ", not " ++ hex (fromIntegral (expectedState expected))
    | stateWord /= expectedState expected
    ]
  shadowProblems =
    [ "shadow[" ++ show offset ++ "] " ++ hex (fromIntegral actual) ++ ", not " ++ hex (fromIntegral wanted)
    | (offset, wanted) <- zip [0, 2, 4] (expectedShadow expected)
    , let actual = readHalfwordAt (regionBytesOf observation (RegionId "shadow")) offset
    , actual /= wanted
    ]
  prehookActualState = readWordAt (regionBytesOf observation (RegionId "fixtures")) 0x3040
  prehookActualEvent = readWordAt (regionBytesOf observation (RegionId "fixtures")) 0x3044
  prehookLogProblems =
    case expectedPrehookLog expected of
      Nothing -> []
      Just (stateValue, eventValue) ->
        [ "prehook log r1 " ++ hex prehookActualState ++ ", not " ++ hex stateValue
        | prehookActualState /= stateValue
        ]
          ++ [ "prehook log r2 " ++ hex prehookActualEvent ++ ", not " ++ hex eventValue
             | prehookActualEvent /= eventValue
             ]
  logProblems =
    [ "log[" ++ show offset ++ "] " ++ hex actual ++ ", not " ++ hex wanted
    | (offset, wanted) <- zip [0, 4 ..] (expectedActionLog expected ++ expectedTransitionLog expected)
    , let actual = readWordAt (regionBytesOf observation (RegionId "fixtures")) (0x3000 + offset)
    , actual /= wanted
    ]
  cellProblem =
    case scenarioCellBase scenario of
      Nothing -> []
      Just _ ->
        [ "cell high " ++ hex (fromIntegral actualCellHigh) ++ ", not " ++ hex (fromIntegral (expectedCellHigh expected))
        | actualCellHigh /= expectedCellHigh expected
        ]
  actualCellHigh = readHalfwordAt (regionBytesOf observation (RegionId "fixtures")) (4 * fromIntegral (scenarioCellIndex scenario) + 2)
  (cellValue, actionValue, transitionValue) = expectedPointers expected
  pointerOffset = 4 * fromIntegral controllerId
  pointerProblems =
    [ "pointer " ++ name ++ " " ++ hex actual ++ ", not " ++ hex wanted
    | (name, wanted, actual) <-
        [ ("cell", cellValue, readWordAt (regionBytesOf observation (RegionId "cell-pointers")) pointerOffset)
        , ("action", actionValue, readWordAt (regionBytesOf observation (RegionId "action-pointers")) pointerOffset)
        , ("transition", transitionValue, readWordAt (regionBytesOf observation (RegionId "transition-pointers")) pointerOffset)
        ]
    , actual /= wanted
    ]
  actualEvent = readHalfwordAt (regionBytesOf observation (RegionId "event-cells")) (2 * fromIntegral controllerId)
  actualWidth = readHalfwordAt (regionBytesOf observation (RegionId "width-cells")) (2 * fromIntegral controllerId)
  cellInputProblems =
    [ "event cell " ++ hex (fromIntegral actualEvent) ++ ", not " ++ hex (fromIntegral (scenarioEvent scenario))
    | actualEvent /= scenarioEvent scenario
    ]
      ++ [ "width cell " ++ hex (fromIntegral actualWidth) ++ ", not " ++ hex (fromIntegral (scenarioWidth scenario))
         | actualWidth /= scenarioWidth scenario
         ]
  allowed = allowedWriteIntervals scenario
  writeProblems = case nativeProvenance observation of
    Truncated total -> ["write provenance truncated at " ++ show (provenanceTotal total)]
    Complete _ writes ->
      [ "write at " ++ hex (fromIntegral address) ++ " size " ++ show size ++ " leaves the admitted effect scope"
      | Write (GuestAddress address) (WriteBytes size) <- writes
      , not (any (\(lo, extent) -> lo <= fromIntegral address && fromIntegral address + fromIntegral size <= lo + extent) allowed)
      ]

allowedWriteIntervals :: Scenario -> [(Word32, Word32)]
allowedWriteIntervals scenario =
  shadowInterval
    ++ stateInterval
    ++ cellHighInterval
    ++ actionLogIntervals
    ++ transitionLogIntervals
    ++ pointerIntervals
    ++ prehookLogInterval
    ++ [(stackBase, stackSize)]
 where
  hasCell = case scenarioCellBase scenario of
    Nothing -> False
    Just _ -> True
  high = highAfterAction scenario
  shadowInterval = [(shadowBase, 6) | hasCell]
  stateInterval = [(stateCells + 2 * controllerId, 2) | hasCell, high <= 0]
  cellHighInterval =
    case mutationHighOf scenario of
      Nothing -> []
      Just _ -> [(cellAddress scenario + 2, 2)]
  actionLogIntervals =
    [ (logBase + 4 * fromIntegral offset, 4)
    | actionPresent scenario
    , offset <- [0 .. 3 :: Int]
    ]
  transitionLogIntervals =
    [ (logBase + 16 + 4 * fromIntegral offset, 4)
    | transitionCalled scenario
    , offset <- [0 .. 3 :: Int]
    ]
  pointerIntervals =
    case scenarioAction scenario of
      ActionCall _ _ (Just mutation) ->
        [ (cellPointers + 4 * controllerId, 4) | Just _ <- [mutationCellBase mutation] ]
          ++ [ (actionPointers + 4 * controllerId, 4) | Just _ <- [mutationActionBase mutation] ]
          ++ [ (transitionPointers + 4 * controllerId, 4) | Just _ <- [mutationTransitionBase mutation] ]
      _ -> []
  prehookLogInterval = [(prehookLogBase, 8) | scenarioProbePrehook scenario]

data Diff
  = StatusDiff
  | RegisterDiff Register
  | CpsrDiff
  | MemoryDiff RegionId
  | ProvenanceDiff

renderDiff :: Diff -> String
renderDiff StatusDiff = "status"
renderDiff (RegisterDiff register) = "register " ++ registerLabel register
renderDiff CpsrDiff = "cpsr control"
renderDiff (MemoryDiff ident) = "memory " ++ regionName ident
renderDiff ProvenanceDiff = "non-stack write set"

sameNonStackWrites :: NativeObservation -> NativeObservation -> Bool
sameNonStackWrites oracle candidate =
  case (nativeProvenance oracle, nativeProvenance candidate) of
    (Complete _ left, Complete _ right) -> nonStack left == nonStack right
    _ -> False
 where
  nonStack writes =
    S.fromList
      [ (fromIntegral address :: Word32, fromIntegral size :: Word32)
      | Write (GuestAddress address) (WriteBytes size) <- writes
      , not (stackBase <= fromIntegral address && fromIntegral address + fromIntegral size <= stackBase + stackSize)
      ]

compareObservations :: NativeObservation -> NativeObservation -> [Diff]
compareObservations oracle candidate
  | nativeStatus oracle /= Returned || nativeStatus candidate /= Returned = [StatusDiff]
  | otherwise =
      [ RegisterDiff register
      | register <- preservedRegisters
      , registerValue oracle register /= registerValue candidate register
      ]
        ++ [CpsrDiff | registerValue oracle Cpsr .&. 0x3ff /= registerValue candidate Cpsr .&. 0x3ff]
        ++ [ MemoryDiff ident
           | ident <- sharedRegions
           , regionBytesOf oracle ident /= regionBytesOf candidate ident
           ]
        ++ [ProvenanceDiff | not (sameNonStackWrites oracle candidate)]

data Outcome = Outcome
  { outcomeId :: String
  , outcomeOk :: Bool
  , outcomeMismatches :: [String]
  , outcomeStock :: Maybe NativeObservation
  , outcomeOwned :: Maybe NativeObservation
  }

runScenario :: Context -> Scenario -> IO Outcome
runScenario context scenario = do
  result <- try (runScenario' context scenario)
  pure $ case result of
    Left (exception :: SomeException) ->
      Outcome (scenarioId scenario) False ["exception: " ++ show exception] Nothing Nothing
    Right (stock, owned, problems) ->
      Outcome (scenarioId scenario) (null problems) problems (Just stock) (Just owned)

runScenario' :: Context -> Scenario -> IO (NativeObservation, NativeObservation, [String])
runScenario' context scenario = do
  let expected = expectedOf scenario
      stockSpec = buildCase context StockVariant scenario
      ownedSpec = buildCase context OwnedVariant scenario
      stockPath = contextOutput context </> (scenarioId scenario ++ "-stock")
      ownedPath = contextOutput context </> (scenarioId scenario ++ "-owned")
  stockPathWritten <- writeCase stockPath stockSpec
  stock <- loadObserved (contextRunner context) stockPathWritten
  ownedPathWritten <- writeCase ownedPath ownedSpec
  owned <- loadObserved (contextRunner context) ownedPathWritten
  let problems =
        map ("stock: " ++) (checkExpected scenario expected stock)
          ++ map ("owned: " ++) (checkExpected scenario expected owned)
          ++ map ("differential: " ++) (map renderDiff (compareObservations stock owned))
  pure (stock, owned, problems)

runOwnedControl :: Context -> Scenario -> NativeObservation -> [Diff] -> IO (Bool, String)
runOwnedControl context scenario oracle expectedDiffs = do
  let spec = buildCase context OwnedVariant scenario
      directory = contextOutput context </> scenarioId scenario
  path <- writeCase directory spec
  candidate <- loadObserved (contextRunner context) path
  let diffs = compareObservations oracle candidate
  pure (diffs == expectedDiffs, "diffs: " ++ show (map renderDiff diffs))

controlReturn :: Context -> NativeObservation -> IO (Bool, String)
controlReturn context oracle = do
  let base = findScenario "mr-action-nonpositive"
      expected = expectedOf base
      wrong = differentFrom (expectedReturn expected) 1
      faulty = base { scenarioId = "control-return", scenarioFault = FaultReturn wrong }
  runOwnedControl context faulty oracle [RegisterDiff R0]

controlState :: Context -> NativeObservation -> IO (Bool, String)
controlState context oracle = do
  let base = findScenario "mr-mutate-to-negative"
      expected = expectedOf base
      wrongHigh = fromIntegral (expectedCellHigh expected) - 1 :: Int16
      faulty = base { scenarioId = "control-state", scenarioFault = FaultHigh wrongHigh }
  runOwnedControl context faulty oracle [MemoryDiff (RegionId "fixtures"), MemoryDiff (RegionId "state-cells")]

differentFrom :: Word32 -> Word32 -> Word32
differentFrom value fallback = if value == fallback then value + 1 else fallback

runControl :: IO (Bool, String) -> IO (Bool, String)
runControl action = do
  result <- try action
  pure $ case result of
    Left (exception :: SomeException) -> (False, "exception: " ++ show exception)
    Right value -> value

guardCase :: CaseSpec
guardCase =
  CaseSpec
    { caseSpecId = "control-body-plus-four"
    , caseSpecEntry = stockEntry
    , caseSpecRegions =
        [ mkRegion (RegionId "guard") stockEntry 4 ReadExecute (word32le (branchWord stockEntry bodyPlusFour))
        , mkRegion (RegionId "exit") exitAddr 4 ReadExecute (word32le bxLr)
        , mkRegion (RegionId "stack") stackBase (fromIntegral stackSize) ReadWrite (B.replicate (fromIntegral stackSize) 0)
        ]
    , caseSpecFunctions = [(stockEntry, 4)]
    , caseSpecStatus = FetchGuard
    }

renderedReport :: [Outcome] -> [(String, Bool, String)] -> Value
renderedReport outcomes controls =
  object
    [ "qualified" .= (all outcomeOk outcomes && all (\(_, ok, _) -> ok) controls)
    , "cases" .=
        [ object
            [ "id" .= outcomeId outcome
            , "ok" .= outcomeOk outcome
            , "mismatches" .= outcomeMismatches outcome
            ]
        | outcome <- outcomes
        ]
    , "controls" .=
        [ object ["id" .= ident, "ok" .= ok, "detail" .= detail]
        | (ident, ok, detail) <- controls
        ]
    ]

retainFailures :: FilePath -> [Outcome] -> [(String, Bool, String)] -> Value -> IO ()
retainFailures output outcomes controls report = do
  let failures =
        [ object ["id" .= outcomeId outcome, "mismatches" .= outcomeMismatches outcome]
        | outcome <- outcomes
        , not (outcomeOk outcome)
        ]
          ++ [ object ["id" .= ident, "detail" .= detail]
             | (ident, ok, detail) <- controls
             , not ok
             ]
  when (not (null failures)) $ do
    let directory = output </> "retained-failures"
    createDirectoryIfMissing True directory
    B.writeFile (directory </> "manifest.json") (encodeCanonical (object ["failures" .= failures, "report" .= report]))

runQualification :: Options -> IO Bool
runQualification options = do
  exists <- doesPathExist (optionOutput options)
  when exists $ error "output path already exists"
  unless (optionId5Base options == id5BaseValue) $ error "id5 base differs from the pinned image base"
  id5 <- readExact (optionId5 options) (fromIntegral id5Size)
  unless (sha256Hex (sha256 id5) == id5Digest) $ error "id5 digest differs from the pinned image"
  kernelText <- readBounded (optionBuild options </> "kernel-text.bin") kernelTextLimit
  unless (sha256Hex (sha256 kernelText) == optionKernelTextSha256 options) $ error "kernel-text digest mismatch"
  symsBytes <- readBounded (optionBuild options </> "kernel.syms") kernelSymsLimit
  unless (sha256Hex (sha256 symsBytes) == optionKernelSymsSha256 options) $ error "kernel.syms digest mismatch"
  let symbols = parseSymbols requiredSymbols (BC.unpack symsBytes)
      stock = sliceAt id5 (optionId5Base options) stockEntry stockSize
      prehook = sliceAt id5 (optionId5Base options) prehookAddr prehookSize
      noop = sliceAt id5 (optionId5Base options) noopAddr noopSize
      context = Context (optionRunner options) (optionOutput options) symbols kernelText stock prehook noop
      imageStart = symbolAddress context "__rb_image_start"
  forM_ ["rb_modal_dispatch", "rb_modal_call_prehook", "rb_modal_call_action", "rb_modal_call_transition"] $ \name -> do
    let (address, size) = symbolFunction context name
    unless (imageStart <= address && address + size <= imageStart + fromIntegral (B.length kernelText)) $
      error ("owned symbol outside kernel-text.bin: " ++ name)
  createDirectoryIfMissing True (optionOutput options)
  outcomes <- runCases context scenarios []
  case outcomes of
    Left failed -> do
      writeFailed context [failed] []
      pure False
    Right completed -> do
      let baseReturn = findScenario "mr-action-nonpositive"
          baseState = findScenario "mr-mutate-to-negative"
          oracleOf ident =
            case [stockObservation | outcome <- completed, outcomeId outcome == scenarioId ident, Just stockObservation <- [outcomeStock outcome]] of
              (observation : _) -> observation
              [] -> error ("no stock observation for " ++ scenarioId ident)
      returnControl <- runControl (controlReturn context (oracleOf baseReturn))
      if not (fst returnControl)
        then writeFailed context completed [("control-return", fst returnControl, snd returnControl)] >> pure False
        else do
          stateControl <- runControl (controlState context (oracleOf baseState))
          if not (fst stateControl)
            then writeFailed context completed [("control-state", fst stateControl, snd stateControl)] >> pure False
            else do
              guardControl <- runControl $ do
                path <- writeCase (contextOutput context </> "control-body-plus-four") guardCase
                observation <- loadObserved (contextRunner context) path
                let pc = registerValue observation Pc
                pure
                  ( nativeStatus observation == FetchGuard && pc == bodyPlusFour
                  , "status " ++ runnerStatusLabel (nativeStatus observation) ++ "; pc " ++ hex pc
                  )
              let controls =
                    [ ("control-return", fst returnControl, snd returnControl)
                    , ("control-state", fst stateControl, snd stateControl)
                    , ("control-body-plus-four", fst guardControl, snd guardControl)
                    ]
                  qualified = all (\(_, ok, _) -> ok) controls
                  report = renderedReport completed controls
              writeReport context completed controls
              unless qualified $ retainFailures (optionOutput context) completed controls report
              pure qualified
 where
  writeFailed context completed controls = do
    let outcomes = completed
        report = renderedReport outcomes controls
    writeReport context outcomes controls
    retainFailures (optionOutput context) outcomes controls report
  writeReport context outcomes controls =
    B.writeFile (optionOutput context </> "modal-runtime-results.json") (encodeCanonical (renderedReport outcomes controls))

runCases :: Context -> [Scenario] -> [Outcome] -> IO (Either Outcome [Outcome])
runCases _ [] completed = pure (Right (reverse completed))
runCases context (scenario : rest) completed = do
  outcome <- runScenario context scenario
  if outcomeOk outcome
    then runCases context rest (outcome : completed)
    else pure (Left outcome)

readExact :: FilePath -> Int -> IO B.ByteString
readExact path expected = do
  bytes <- B.readFile path
  if B.length bytes == expected then pure bytes else error (path ++ ": expected " ++ show expected ++ " bytes")

readBounded :: FilePath -> Int -> IO B.ByteString
readBounded path limit = do
  bytes <- B.readFile path
  if not (B.null bytes) && B.length bytes <= limit then pure bytes else error (path ++ ": size outside (0," ++ show limit ++ "]")

findScenario :: String -> Scenario
findScenario ident = case [scenario | scenario <- scenarios, scenarioId scenario == ident] of
  (scenario : _) -> scenario
  [] -> error ("scenario missing: " ++ ident)

scenarios :: [Scenario]
scenarios =
  [ Scenario "mr-null-cell" 0x1234 0x0002 0x0005 Nothing 0 5 1 NoAction False 0 False False NoFault
  , Scenario "mr-null-action-zero" 3 4 5 (Just fixtureRamBase) 17 5 0 NoAction False 0 False False NoFault
  , Scenario "mr-null-action-negative" 1 1 0 (Just fixtureRamBase) 1 5 (-1) NoAction False 0 False False NoFault
  , Scenario "mr-null-action-minimum" 1 1 0 (Just fixtureRamBase) 1 5 (-32768) NoAction False 0 False False NoFault
  , Scenario "mr-null-action-positive-null" 1 1 0 (Just fixtureRamBase) 1 5 1 NoAction False 0 False False NoFault
  , Scenario "mr-null-action-positive-transition" 1 1 0 (Just fixtureRamBase) 1 5 2 NoAction True 0xdeadbeef False False NoFault
  , Scenario "mr-action-nonpositive" 3 4 5 (Just fixtureRamBase) 17 5 (-2) (ActionCall 0x11223344 0x55667788 Nothing) False 0 False False NoFault
  , Scenario "mr-action-zero" 3 4 5 (Just fixtureRamBase) 17 5 0 (ActionCall 0x01020304 0x05060708 Nothing) False 0 False False NoFault
  , Scenario "mr-action-positive-null" 3 4 5 (Just fixtureRamBase) 17 5 1 (ActionCall 0x0a0b0c0d 0x0e0f1011 Nothing) False 0 False False NoFault
  , Scenario "mr-action-positive-transition" 3 4 5 (Just fixtureRamBase) 17 5 1 (ActionCall 0x12131415 0x16171819 Nothing) True 0xffffffff False False NoFault
  , Scenario "mr-index-wrap" 0x0100 0x0100 7 (Just fixtureRamBase) 7 5 (-1) (ActionCall 0x20212223 0x24252627 Nothing) False 0 False False NoFault
  , Scenario "mr-mutate-to-negative" 3 4 5 (Just fixtureRamBase) 17 5 1 (ActionCall 0x30313233 0x34353637 (Just (Mutation (Just (-2)) Nothing Nothing Nothing))) False 0 True False NoFault
  , Scenario "mr-mutate-to-positive" 3 4 5 (Just fixtureRamBase) 17 5 0 (ActionCall 0x40414243 0x44454647 (Just (Mutation (Just 2) Nothing Nothing Nothing))) True 0x55565758 False False NoFault
  , Scenario "mr-capture-pointers" 3 4 5 (Just fixtureRamBase) 17 5 1 (ActionCall 0x50515253 0x54555657 (Just (Mutation (Just 2) (Just bogusCellBase) (Just 0) (Just bogusTransitionBase)))) False 0 True False NoFault
  , Scenario "mr-negative-event-abi" 0xffff 0x0000 0x0000 (Just fixtureRamBase) 0 5 (-1) (ActionCall 0x60616263 0x64656667 Nothing) False 0 False True NoFault
  , Scenario "mr-negative-state-abi" 0x0000 0x0000 0x8000 (Just fixtureRamBase) 0 5 (-1) (ActionCall 0x70717273 0x74757677 Nothing) False 0 False True NoFault
  ]
