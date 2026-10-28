{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
-- | Native qualification for the retained row-availability service.
--
-- The direct cases enter the built ARM @rb_service_field_row_state@, the
-- assembly trampoline and the original stock @FUN_00d52684@. The shared
-- observation parser separates the public result from the stack workspace;
-- this module owns the service contract over it.
module Replacement.Service.Qualify
  ( RowState (..)
  , rowStateWord
  , ServiceRow (..)
  , ServiceLifetime (..)
  , ServiceEffectPolicy (..)
  , ServiceEndpoints (..)
  , ServiceMutant (..)
  , ServiceContract (..)
  , serviceContract
  , serviceContractValue
  , serviceContractBytes
  , serviceContractSha256
  , serviceRows
  , serviceStackRegion
  , serviceStackBase
  , serviceStackOffset
  , serviceStackSize
  , serviceStackTop
  , serviceTransientRegion
  , serviceTransientOffset
  , serviceTransientSize
  , serviceCasebufSize
  , serviceCasebufFill
  , serviceCasebufInitial
  , serviceSeeds
  , serviceReturnRegister
  , servicePreservedRegisters
  , serviceEntrySymbol
  , serviceTrampolineSymbol
  , serviceStockTarget
  , serviceStockSize
  , serviceStockSpan
  , serviceInitialRegisters
  , serviceExpectedRegisters
  , isServiceCaseId
  , serviceMutantName
  , serviceMutantNames
  , Expectation (..)
  , expectationFromCase
  , Check (..)
  , classifyStatus
  , checkServiceCase
  , checkStatus
  , checkProblems
  , mutateRegister
  , mutateWrite
  , runServiceCase
  , serviceQualificationReport
  , reportQualified
  ) where

import Control.Exception (IOException, try)
import Control.Monad (forM, unless)
import Data.Aeson (Value (..))
import Data.Bits (xor)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.List (find, nub, nubBy, sort)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, listToMaybe)
import Data.Word (Word32, Word8)
import Emulation.NativeCase (Region (..), loadCase)
import qualified Emulation.NativeObservation as N
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host hiding (sha256)
import FujiAF.JSON (encodeCanonical)
import Numeric (showHex)
import System.Directory (canonicalizePath)
import System.FilePath (takeDirectory, takeFileName)

-- | The states the stock probe defines.
data RowState = Offered | Disabled | Hidden
  deriving (Eq, Show)

rowStateWord :: RowState -> Word32
rowStateWord Offered = 0
rowStateWord Disabled = 2
rowStateWord Hidden = 3

-- | One direct row case. The expected state is derived from the stock probe's
-- own arms, not from executing the candidate.
data ServiceRow = ServiceRow
  { serviceRowId :: String
  , serviceRowProperty :: Word32
  , serviceRowToken :: Word32
  , serviceRowExpected :: RowState
  , serviceRowProvenance :: String
  }
  deriving (Eq, Show)

-- | The synchronous service lifetime: no host operation, no retained
-- completion and no post-return callback.
data ServiceLifetime = ServiceLifetime
  { lifetimeHostOperation :: Bool
  , lifetimeRetainedCompletion :: Bool
  , lifetimePostReturnCallback :: Bool
  }
  deriving (Eq, Show)

-- | Scratch spans and the final-byte proof the checker applies.
data ServiceEffectPolicy = ServiceEffectPolicy
  { effectAllowedScratch :: [N.RegionId]
  , effectTransientRestores :: Bool
  , effectImmutableExact :: Bool
  , effectCompleteProvenance :: Bool
  }
  deriving (Eq, Show)

data ServiceEndpoints = ServiceEndpoints
  { endpointEntry :: String
  , endpointTrampoline :: String
  , endpointStock :: Word32
  , endpointStockSize :: Int
  }
  deriving (Eq, Show)

-- | The two required consumer controls.
data ServiceMutant = AbiR7Clobber | EffectImmutableWrite
  deriving (Bounded, Enum, Eq, Ord, Show)

serviceMutantName :: ServiceMutant -> String
serviceMutantName AbiR7Clobber = "abi-r7-clobber"
serviceMutantName EffectImmutableWrite = "effect-immutable-write"

-- | The one service contract consumed by case generation, checking and the
-- qualification report. The machine-readable value below is derived from it.
data ServiceContract = ServiceContract
  { contractSchemaVersion :: Int
  , contractRows :: [ServiceRow]
  , contractStateDomain :: [RowState]
  , contractReturnRegister :: N.Register
  , contractArgumentRegisters :: [N.Register]
  , contractScratchRegisters :: [N.Register]
  , contractCpsr :: Word32
  , contractPreserved :: [N.Register]
  , contractSeeds :: [(N.Register, Word32)]
  , contractStackRegion :: N.RegionId
  , contractStackBase :: Int
  , contractStackOffset :: Int
  , contractStackSize :: Int
  , contractStackTop :: Word32
  , contractCasebufSize :: Int
  , contractCasebufFill :: Word8
  , contractTransientRegion :: N.RegionId
  , contractTransientOffset :: Int
  , contractTransientSize :: Int
  , contractEndpoints :: ServiceEndpoints
  , contractLifetime :: ServiceLifetime
  , contractEffects :: ServiceEffectPolicy
  , contractMutants :: [ServiceMutant]
  }
  deriving (Eq, Show)

serviceContract :: ServiceContract
serviceContract =
  ServiceContract
    { contractSchemaVersion = 1
    , contractRows = rows
    , contractStateDomain = [Offered, Disabled, Hidden]
    , contractReturnRegister = N.R0
    , contractArgumentRegisters = [N.R0, N.R1]
    , contractScratchRegisters = [N.R2, N.R3, N.R12]
    , contractCpsr = 0x10
    , contractPreserved = [N.R4 .. N.R11] ++ [N.Sp, N.Pc]
    , contractSeeds =
        [ (N.R4, 0x4a4a4a4a)
        , (N.R5, 0x5b5b5b5b)
        , (N.R6, 0x6c6c6c6c)
        , (N.R7, 0x7d7d7d7d)
        , (N.R8, 0x8e8e8e8e)
        , (N.R9, 0x9f9f9f9f)
        , (N.R10, 0xa0a0a0a0)
        , (N.R11, 0xb1b1b1b1)
        ]
    , contractStackRegion = N.RegionId "casebuf"
    , contractStackBase = 0x009a0000
    , contractStackOffset = 0x1e00
    , contractStackSize = 0x200
    , contractStackTop = 0x009a1ff0
    , contractCasebufSize = 0x2000
    , contractCasebufFill = 0xa5
    , contractTransientRegion = N.RegionId "mirror"
    , contractTransientOffset = 0
    , contractTransientSize = 0x200
    , contractEndpoints =
        ServiceEndpoints
          { endpointEntry = "rb_service_field_row_state"
          , endpointTrampoline = "rb_call_stock_probe"
          , endpointStock = 0x00d52684
          , endpointStockSize = 25140
          }
    , contractLifetime = ServiceLifetime False False False
    , contractEffects =
        ServiceEffectPolicy
          { effectAllowedScratch = [N.RegionId "casebuf", N.RegionId "mirror"]
          , effectTransientRestores = False
          , effectImmutableExact = True
          , effectCompleteProvenance = True
          }
    , contractMutants = [AbiR7Clobber, EffectImmutableWrite]
    }
  where
    rows = map toneRow [0 .. 4] ++ map isoRow isoRows
    isoRows = [0x1a, 0x19, 0x17, 0x16, 0x15, 0x14, 0x13, 0x12, 0x11, 0x10,
               0xf, 0xe, 0xd, 0xc, 0xb, 0xa, 9, 8, 7, 0, 0x1b] :: [Word32]
    toneRow row =
      ServiceRow
        ("svc-110-row-" ++ showHex row "")
        0x110
        row
        Offered
        ( "contract: direct rb_service_field_row_state(0x110, " ++ showHex row ""
            ++ ") returns 0 (offered); FUN_00d52684 case 0x110 answers 0 because its only check "
            ++ "FUN_00d52684(5,0x1a) takes property 5's default arm. RB_EDIT_FIELD maps host 0x268 "
            ++ "to the five-row property 0x110 (menu/recipe_bank_edit.c, reports/custom-settings.fields.json)."
        )
    isoRow row =
      ServiceRow
        ("svc-141-row-" ++ showHex row "")
        0x141
        row
        (if row == 0 then Hidden else Offered)
        ("contract: direct rb_service_field_row_state(0x141, " ++ showHex row "" ++ ") " ++ isoWhy row)
    isoWhy 0 =
      "returns 3 (hidden) in the M fixture: FUN_0063d060(0x24) = 1, *0x00d563b4 = 0xc and " ++
      "0x00d563b4[0x24] = 1 select FUN_00d52684 case 0x141 row 0's hidden arm."
    isoWhy 7 =
      "returns 0 (offered) in the M fixture: case 0x141 row 7's matrix arm unhides when " ++
      "FUN_0063d060(0x24) = 1, *0x00d563b4 = 0xc and 0x00d563b4[0x24] = 1."
    isoWhy row =
      "returns 0 (offered); FUN_00d52684 case 0x141 row " ++ showHex row "" ++
      " takes its default offered arm under the same M fixture."

serviceRows :: [ServiceRow]
serviceRows = contractRows serviceContract

serviceSeeds :: [(N.Register, Word32)]
serviceSeeds = contractSeeds serviceContract

serviceReturnRegister :: N.Register
serviceReturnRegister = contractReturnRegister serviceContract

servicePreservedRegisters :: [N.Register]
servicePreservedRegisters = contractPreserved serviceContract

serviceEntrySymbol :: String
serviceEntrySymbol = endpointEntry (contractEndpoints serviceContract)

serviceTrampolineSymbol :: String
serviceTrampolineSymbol = endpointTrampoline (contractEndpoints serviceContract)

serviceStockTarget :: Word32
serviceStockTarget = endpointStock (contractEndpoints serviceContract)

serviceStockSize :: Int
serviceStockSize = endpointStockSize (contractEndpoints serviceContract)

serviceStockSpan :: String
serviceStockSpan = N.hexWord32 serviceStockTarget ++ "/" ++ show serviceStockSize

serviceStackRegion :: N.RegionId
serviceStackRegion = contractStackRegion serviceContract

serviceStackBase, serviceStackOffset, serviceStackSize :: Int
serviceStackBase = contractStackBase serviceContract
serviceStackOffset = contractStackOffset serviceContract
serviceStackSize = contractStackSize serviceContract

serviceStackTop :: Word32
serviceStackTop = contractStackTop serviceContract

serviceTransientRegion :: N.RegionId
serviceTransientRegion = contractTransientRegion serviceContract

serviceTransientOffset, serviceTransientSize :: Int
serviceTransientOffset = contractTransientOffset serviceContract
serviceTransientSize = contractTransientSize serviceContract

serviceCasebufSize :: Int
serviceCasebufSize = contractCasebufSize serviceContract

serviceCasebufFill :: Word8
serviceCasebufFill = contractCasebufFill serviceContract

serviceMutantNames :: [String]
serviceMutantNames = map serviceMutantName (contractMutants serviceContract)

serviceCasebufInitial :: B.ByteString
serviceCasebufInitial =
  B.replicate serviceStackOffset 0
    <> B.replicate serviceStackSize serviceCasebufFill
    <> B.replicate (serviceCasebufSize - serviceStackOffset - serviceStackSize) 0

serviceContractValue :: Value
serviceContractValue =
  obj
    [ ("schema_version", n (fromIntegral (contractSchemaVersion serviceContract)))
    , ("rows", arr (map rowValue (contractRows serviceContract)))
    , ("state_domain", arr [n (fromIntegral (rowStateWord state)) | state <- contractStateDomain serviceContract])
    , ("return_register", s (N.registerLabel (contractReturnRegister serviceContract)))
    , ("argument_registers", arr [s (N.registerLabel register) | register <- contractArgumentRegisters serviceContract])
    , ("scratch_registers", arr [s (N.registerLabel register) | register <- contractScratchRegisters serviceContract])
    , ("cpsr", s (N.hexWord32 (contractCpsr serviceContract)))
    , ("preserved_registers", arr [s (N.registerLabel register) | register <- contractPreserved serviceContract])
    , ("seeds", arr [obj [("register", s (N.registerLabel register)), ("value", n (fromIntegral value))] | (register, value) <- contractSeeds serviceContract])
    , ( "stack"
      , obj
          [ ("region", s (N.regionName (contractStackRegion serviceContract)))
          , ("base", n (fromIntegral (contractStackBase serviceContract)))
          , ("offset", n (fromIntegral (contractStackOffset serviceContract)))
          , ("size", n (fromIntegral (contractStackSize serviceContract)))
          , ("top", n (fromIntegral (contractStackTop serviceContract)))
          , ("casebuf_size", n (fromIntegral (contractCasebufSize serviceContract)))
          , ("fill", n (fromIntegral (contractCasebufFill serviceContract)))
          ]
      )
    , ( "transient"
      , obj
          [ ("region", s (N.regionName (contractTransientRegion serviceContract)))
          , ("offset", n (fromIntegral (contractTransientOffset serviceContract)))
          , ("size", n (fromIntegral (contractTransientSize serviceContract)))
          ]
      )
    , ( "endpoints"
      , obj
          [ ("entry", s (endpointEntry (contractEndpoints serviceContract)))
          , ("trampoline", s (endpointTrampoline (contractEndpoints serviceContract)))
          , ("stock", s (N.hexWord32 (endpointStock (contractEndpoints serviceContract))))
          , ("stock_size", n (fromIntegral (endpointStockSize (contractEndpoints serviceContract))))
          ]
      )
    , ( "lifetime"
      , obj
          [ ("host_operation", b (lifetimeHostOperation (contractLifetime serviceContract)))
          , ("retained_completion", b (lifetimeRetainedCompletion (contractLifetime serviceContract)))
          , ("post_return_callback", b (lifetimePostReturnCallback (contractLifetime serviceContract)))
          ]
      )
    , ( "effects"
      , obj
          [ ("allowed_scratch", arr [s (N.regionName region) | region <- effectAllowedScratch (contractEffects serviceContract)])
          , ("transient_restores", b (effectTransientRestores (contractEffects serviceContract)))
          , ("immutable_exact", b (effectImmutableExact (contractEffects serviceContract)))
          , ("complete_provenance", b (effectCompleteProvenance (contractEffects serviceContract)))
          ]
      )
    , ("mutants", arr [s (serviceMutantName mutant) | mutant <- contractMutants serviceContract])
    ]
  where
    rowValue row =
      obj
        [ ("id", s (serviceRowId row))
        , ("property", n (fromIntegral (serviceRowProperty row)))
        , ("row", n (fromIntegral (serviceRowToken row)))
        , ("expected", n (fromIntegral (rowStateWord (serviceRowExpected row))))
        ]

serviceContractBytes :: B.ByteString
serviceContractBytes = encodeCanonical serviceContractValue

serviceContractSha256 :: String
serviceContractSha256 = sha256Hex (sha256 serviceContractBytes)

serviceInitialRegisters :: Word32 -> Word32 -> Word32 -> Word32 -> [(N.Register, Word32)]
serviceInitialRegisters property row stackTop exit =
  [ (register, case register of
        N.R0 -> property
        N.R1 -> row
        _ -> 0)
  | register <- contractArgumentRegisters serviceContract ++ contractScratchRegisters serviceContract
  ]
    ++ serviceSeeds
    ++ [ (N.Sp, stackTop)
       , (N.Lr, exit)
       , (N.Cpsr, contractCpsr serviceContract)
       ]

serviceExpectedRegisters :: Word32 -> Word32 -> Word32 -> [(N.Register, Word32)]
serviceExpectedRegisters expected stackTop exit =
  [ (register, case register of
        N.Sp -> stackTop
        N.Pc -> exit
        _ -> fromMaybe expected (lookup register serviceSeeds))
  | register <- serviceReturnRegister : servicePreservedRegisters
  ]

isServiceCaseId :: String -> Bool
isServiceCaseId = ("svc-" `prefixOf`)
  where
    prefixOf prefix text = take (length prefix) text == prefix

data Expectation = Expectation
  { expectationExpectedR0 :: Word32
  , expectationPreserved :: [(N.Register, Word32)]
  , expectationInitialMemory :: [(N.RegionId, B.ByteString)]
  , expectationBounds :: [(N.RegionId, N.GuestAddress, Int)]
  , expectationStackBase :: N.GuestAddress
  , expectationStackOffset :: Int
  , expectationStackSize :: Int
  , expectationStackTop :: N.GuestAddress
  , expectationTransientRegion :: N.RegionId
  , expectationTransientOffset :: Int
  , expectationTransientSize :: Int
  }
  deriving (Eq, Show)

data Check
  = CheckPass
  | CheckMismatch [String]
  | CheckBlocked N.RunnerStatus
  | CheckUnsupported N.RunnerStatus
  | CheckUnknown N.RunnerStatus
  | CheckTruncated N.ProvenanceTotal
  deriving (Eq, Show)

classifyStatus :: N.RunnerStatus -> Check
classifyStatus N.Returned = CheckPass
classifyStatus status@N.UnknownCall = CheckUnsupported status
classifyStatus status@N.UnsupportedInstruction = CheckUnsupported status
classifyStatus status@N.InstructionBudget = CheckBlocked status
classifyStatus status@N.WallBudget = CheckBlocked status
classifyStatus status@N.MemoryGuard = CheckBlocked status
classifyStatus status@N.FetchGuard = CheckBlocked status
classifyStatus status@N.StubContract = CheckBlocked status
classifyStatus status@N.RunnerEngineError = CheckBlocked status
classifyStatus status@N.InvalidCase = CheckBlocked status
classifyStatus status@N.HostTimeout = CheckBlocked status
classifyStatus status@N.RunnerFailure = CheckBlocked status
classifyStatus status@N.InvalidOutput = CheckBlocked status
classifyStatus status@(N.OtherStatus _) = CheckUnknown status

checkStatus :: Check -> String
checkStatus CheckPass = "passed"
checkStatus (CheckMismatch _) = "failed"
checkStatus (CheckBlocked _) = "blocked"
checkStatus (CheckUnsupported _) = "unsupported"
checkStatus (CheckUnknown _) = "unknown"
checkStatus (CheckTruncated _) = "truncated"

checkProblems :: Check -> [String]
checkProblems (CheckMismatch problems) = problems
checkProblems (CheckBlocked status) = [N.runnerStatusLabel status]
checkProblems (CheckUnsupported status) = [N.runnerStatusLabel status]
checkProblems (CheckUnknown status) = [N.runnerStatusLabel status]
checkProblems (CheckTruncated total) = ["effect evidence truncated at " ++ show (N.provenanceTotal total)]
checkProblems CheckPass = []

checkServiceCase :: Expectation -> N.NativeObservation -> Check
checkServiceCase expectation observation =
  case classifyStatus (N.nativeStatus observation) of
    CheckPass ->
      case N.nativeProvenance observation of
        N.Truncated total
          | effectCompleteProvenance policy -> CheckTruncated total
          | otherwise -> CheckMismatch ["contract requires complete provenance but the log is truncated"]
        N.Complete _ writes ->
          let problems = registers ++ memory ++ concatMap classifyWrite writes
           in if null problems then CheckPass else CheckMismatch problems
    other -> other
  where
    policy = contractEffects serviceContract
    registersOf = N.nativeRegisters observation
    missingRegisters =
      [ "register " ++ N.registerLabel register ++ ": missing from runner output"
      | register <- serviceReturnRegister : servicePreservedRegisters
      , M.notMember register registersOf
      ]
    registers
      | not (null missingRegisters) = missingRegisters
      | otherwise =
          [ N.registerLabel serviceReturnRegister ++ ": expected " ++ N.hexWord32 (expectationExpectedR0 expectation)
              ++ ", observed " ++ N.hexWord32 result
          | Just result <- [M.lookup serviceReturnRegister registersOf]
          , result /= expectationExpectedR0 expectation
          ]
            ++ [ "register " ++ N.registerLabel register ++ ": expected " ++ N.hexWord32 expected
                   ++ ", observed " ++ N.hexWord32 result
               | (register, expected) <- expectationPreserved expectation
               , Just result <- [M.lookup register registersOf]
               , result /= expected
               ]
    memory =
      concat
        [ case (lookup ident (N.nativeMemory observation), lookup ident (expectationInitialMemory expectation)) of
            (Nothing, _) -> ["region " ++ N.regionName ident ++ ": missing from runner output"]
            (_, Nothing) -> ["region " ++ N.regionName ident ++ ": missing initial bytes"]
            (Just actual, Just initial)
              | ident == serviceStackRegion ->
                  let low = expectationStackOffset expectation
                      high = low + expectationStackSize expectation
                      outside = B.take low actual <> B.drop high actual
                      outsideInitial = B.take low initial <> B.drop high initial
                   in [ "stack region changed outside the scratch span"
                      | outside /= outsideInitial
                      ]
              | ident == expectationTransientRegion expectation ->
                  [ "transient region " ++ N.regionName ident ++ " did not return to its initial bytes"
                  | effectTransientRestores policy
                  , actual /= initial
                  ]
              | otherwise ->
                  [ "immutable region " ++ N.regionName ident ++ " changed"
                  | effectImmutableExact policy
                  , actual /= initial
                  ]
        | (ident, _, _) <- expectationBounds expectation
        ]
    classifyWrite (N.Write (N.GuestAddress address) (N.WriteBytes size)) =
      case find (\(_, base, extent) -> contains base extent address size) (expectationBounds expectation) of
        Nothing -> ["forbidden write outside the declared regions at " ++ show address]
        Just (ident, base, _)
          | ident `notElem` effectAllowedScratch policy ->
              ["forbidden write to region " ++ N.regionName ident ++ " at " ++ show address]
          | ident == serviceStackRegion ->
              let low = N.guestAddress (expectationStackBase expectation) + fromIntegral (expectationStackOffset expectation)
                  high = low + fromIntegral (expectationStackSize expectation)
               in [ "stack write outside the scratch span at " ++ show address
                  | address < low || address + fromIntegral size > high
                      || N.GuestAddress (address + fromIntegral size) > expectationStackTop expectation
                  ]
          | ident == expectationTransientRegion expectation ->
              let low = N.guestAddress base + fromIntegral (expectationTransientOffset expectation)
                  high = low + fromIntegral (expectationTransientSize expectation)
               in [ "transient write outside " ++ N.regionName ident ++ " at " ++ show address
                  | address < low || address + fromIntegral size > high
                  ]
          | otherwise -> ["forbidden write to immutable region " ++ N.regionName ident ++ " at " ++ show address]
    contains (N.GuestAddress base) extent address size =
      address >= base && address + fromIntegral size <= base + fromIntegral extent

expectationFromCase :: Value -> [Region] -> Either String Expectation
expectationFromCase caseValue regions = do
  let expected = get caseValue "expected"
      expectedRegisters = get expected "registers"
  expectedR0 <- readRegister serviceReturnRegister expectedRegisters
  preserved <- forM servicePreservedRegisters $ \register ->
    (,) register <$> readRegister register expectedRegisters
  stackTop <- readRegister N.Sp expectedRegisters
  stackRegion <- maybe (Left "service stack region casebuf missing") Right (find ((== serviceStackRegion) . N.regionIdOf) regions)
  let stackBase = N.GuestAddress (fromIntegral (baseAddress stackRegion))
  unless (stackBase == N.GuestAddress (fromIntegral serviceStackBase)) (Left "service stack region has an unexpected base")
  unless (fromIntegral (regionSize stackRegion) >= serviceStackOffset + serviceStackSize) (Left "service stack region is too small")
  unless (maybe False (\region -> fromIntegral (regionSize region) >= serviceTransientSize) (find ((== serviceTransientRegion) . N.regionIdOf) regions)) (Left "service transient region mirror missing")
  pure
    Expectation
      { expectationExpectedR0 = expectedR0
      , expectationPreserved = preserved
      , expectationInitialMemory = N.regionInitialBytes regions
      , expectationBounds = N.regionBounds regions
      , expectationStackBase = stackBase
      , expectationStackOffset = serviceStackOffset
      , expectationStackSize = serviceStackSize
      , expectationStackTop = N.GuestAddress (fromIntegral stackTop)
      , expectationTransientRegion = serviceTransientRegion
      , expectationTransientOffset = serviceTransientOffset
      , expectationTransientSize = serviceTransientSize
      }
  where
    readRegister register object =
      case N.parseWord32 (str (get object (N.registerLabel register))) of
        Just value -> Right value
        Nothing -> Left ("expected register missing or malformed: " ++ N.registerLabel register)

mutateRegister :: N.NativeObservation -> N.Register -> Word32 -> N.NativeObservation
mutateRegister observation register value =
  observation {N.nativeRegisters = M.insert register value (N.nativeRegisters observation)}

mutateWrite :: N.NativeObservation -> N.GuestAddress -> Word8 -> N.NativeObservation
mutateWrite observation address size =
  case N.nativeProvenance observation of
    N.Complete (N.ProvenanceTotal total) writes ->
      observation
        { N.nativeProvenance =
            N.Complete
              (N.ProvenanceTotal (total + 1))
              (writes ++ [N.Write address (N.WriteBytes size)])
        }
    N.Truncated _ -> observation

runServiceCase :: FilePath -> FilePath -> IO (Value, Value)
runServiceCase runner path = do
  let caseId = takeFileName (takeDirectory path)
  loaded <- try (loadCase path)
  case loaded of
    Left (message :: IOException) -> pure (invalidCase caseId (show message), invalidResult caseId (show message))
    Right (caseValue, regions, wire, identity) ->
      case expectationFromCase caseValue regions of
        Left message -> pure (invalidCase caseId message, invalidResult caseId message)
        Right expectation -> do
          executable <- canonicalizePath runner
          digest <- sha256Hex . sha256 <$> B.readFile executable
          let limits = get caseValue "limits"
              wall = num (get limits "wall_ms")
              timeout = fromIntegral (wall * 1000 + 2000000) :: Int
          (timed, exit, out, err) <- runBounded timeout (8 * 1024 * 1024) executable [] wire
          let baseReport =
                merge
                  (obj
                    [ ("schema_version", n 1)
                    , ("case_id", get caseValue "id")
                    , ("runner_sha256", s digest)
                    , ("stderr", s (BC.unpack err))
                    ])
                  identity
          if timed
            then pure
              ( merge baseReport
                  [ ("status", s "host_timeout")
                  , ("runner_stdout", s (BC.unpack out))
                  ]
              , invalidResult caseId "host timeout"
              )
            else do
              parsed <- try (jsonRead out)
              case parsed of
                Left (message :: IOException) ->
                  pure
                    ( merge baseReport
                        [ ("status", s "invalid_output")
                        , ("reason", s (show message))
                        , ("runner_stdout", s (BC.unpack out))
                        ]
                    , invalidResult caseId (show message)
                    )
                Right value
                  | get value "wire_version" /= n 1 ->
                      pure
                        ( merge baseReport
                            [ ("status", s "invalid_output")
                            , ("reason", s "wire_version is not 1")
                            , ("runner", value)
                            ]
                        , invalidResult caseId "wire_version is not 1"
                        )
                  | otherwise ->
                      case N.observeNative exit regions value of
                        Left message ->
                          pure
                            ( merge baseReport
                                [ ("status", s "invalid_output")
                                , ("reason", s (N.renderObservationError message))
                                , ("runner", value)
                                ]
                            , invalidResult caseId (N.renderObservationError message)
                            )
                        Right observation -> do
                          let check = checkServiceCase expectation observation
                              status = checkStatus check
                              problems = checkProblems check
                              mutants = if status == "passed" then mutantChecks expectation observation else []
                              result =
                                merge
                                  (obj
                                    [ ("id", get caseValue "id")
                                    , ("status", s status)
                                    , ("mismatches", arr (map s problems))
                                    , ("expected_r0", n (fromIntegral (expectationExpectedR0 expectation)))
                                    , ("observed_r0", maybe Null (n . fromIntegral) (M.lookup serviceReturnRegister (N.nativeRegisters observation)))
                                    , ("runner_status", s (N.runnerStatusLabel (N.nativeStatus observation)))
                                    , ("runner_sha256", s digest)
                                    , ("provenance_total", n (fromIntegral (N.provenanceTotal (N.provenanceCount (N.nativeProvenance observation)))))
                                    , ("provenance_complete", b (not (isTruncated (N.nativeProvenance observation))))
                                    , ("instructions", n (fromIntegral (N.instructionCount (N.nativeInstructions observation))))
                                    , ("mutants", arr mutants)
                                    ])
                                  identity
                              report =
                                merge
                                  baseReport
                                  [ ("status", s status)
                                  , ("runner_status", s (N.runnerStatusLabel (N.nativeStatus observation)))
                                  , ("check", s status)
                                  , ("mismatches", arr (map s problems))
                                  , ("provenance_total", n (fromIntegral (N.provenanceTotal (N.provenanceCount (N.nativeProvenance observation)))))
                                  , ("runner", value)
                                  ]
                          pure (report, result)
  where
    isTruncated (N.Truncated _) = True
    isTruncated (N.Complete _ _) = False
    invalidCase caseId message =
      obj [("schema_version", n 1), ("case_id", s caseId), ("status", s "invalid_case"), ("reason", s message)]
    invalidResult caseId message =
      obj [("id", s caseId), ("status", s "blocked"), ("mismatches", arr [s message])]

mutantChecks :: Expectation -> N.NativeObservation -> [Value]
mutantChecks expectation observation =
  [ mutant (serviceMutantName required) (mutate required)
  | required <- contractMutants serviceContract
  ]
  where
    r7 = fromMaybe 0 (M.lookup N.R7 (N.nativeRegisters observation))
    mutate AbiR7Clobber = mutateRegister observation N.R7 (r7 `xor` 0x01010101)
    mutate EffectImmutableWrite = mutateWrite observation (N.GuestAddress (fromIntegral serviceStackBase + 0x100)) 4
    mutant name changed =
      let check = checkServiceCase expectation changed
       in obj
            [ ("name", s name)
            , ("status", s (if refused check then "refused" else "accepted"))
            , ("mismatches", arr (map s (checkProblems check)))
            ]
    refused (CheckMismatch _) = True
    refused _ = False

data CaseOutcome
  = CasePassed
  | CaseFailed
  | CaseBlocked
  | CaseUnsupported
  | CaseTruncated
  | CaseUnknown
  | CaseMalformed String
  deriving (Eq, Show)

caseOutcome :: Value -> CaseOutcome
caseOutcome result =
  case get result "status" of
    String _ -> case str (get result "status") of
      "passed" -> CasePassed
      "failed" -> CaseFailed
      "blocked" -> CaseBlocked
      "unsupported" -> CaseUnsupported
      "truncated" -> CaseTruncated
      "unknown" -> CaseUnknown
      "" -> CaseMalformed "empty status"
      other -> CaseMalformed ("unknown status " ++ other)
    _ -> CaseMalformed "missing status"

caseOutcomeLabel :: CaseOutcome -> String
caseOutcomeLabel CasePassed = "passed"
caseOutcomeLabel CaseFailed = "failed"
caseOutcomeLabel CaseBlocked = "blocked"
caseOutcomeLabel CaseUnsupported = "unsupported"
caseOutcomeLabel CaseTruncated = "truncated"
caseOutcomeLabel CaseUnknown = "unknown"
caseOutcomeLabel (CaseMalformed _) = "malformed"

serviceQualificationReport :: [Value] -> Value
serviceQualificationReport results =
  obj
    [ ("schema_version", n 1)
    , ("profile", s "service-native")
    , ("contract_sha256", s serviceContractSha256)
    , ("contract", serviceContractValue)
    , ("runner_sha256", maybe (s "") (\value -> get value "runner_sha256") (listToMaybe found))
    , ("status", s status)
    , ("capability", b (status == "qualified"))
    , ("required_rows", n (fromIntegral (length serviceRows)))
    , ("qualified_rows", n (fromIntegral qualifiedRows))
    , ("cases", arr (map summary serviceRows))
    , ("mutants", arr mutantSummaries)
    , ("artifacts", arr (nubBy sameArtifact (concatMap resultArtifacts found)))
    , ("failures", arr (map s (take 20 failures)))
    , ("blockers", arr (map s (take 20 blockers)))
    , ("unknowns", arr (map s (take 20 unknowns)))
    , ("coverage_gaps", arr (map s coverageGaps))
    , ("evidence_limits", arr (map s evidenceLimits))
    ]
  where
    matches row = [result | result <- results, str (get result "id") == serviceRowId row]
    unique row = case matches row of
      [result] -> Just result
      _ -> Nothing
    found = [result | row <- serviceRows, Just result <- [unique row]]
    duplicateRows = [serviceRowId row | row <- serviceRows, length (matches row) > 1]
    missingRows = [serviceRowId row | row <- serviceRows, null (matches row)]
    unknownRows =
      [ caseId
      | result <- results
      , let caseId = str (get result "id")
      , isServiceCaseId caseId
      , caseId `notElem` map serviceRowId serviceRows
      ]
    outcomeOf result = caseOutcome result
    summary row =
      case matches row of
        [] ->
          obj
            [ ("id", s (serviceRowId row))
            , ("property", n (fromIntegral (serviceRowProperty row)))
            , ("row", n (fromIntegral (serviceRowToken row)))
            , ("expected", n (fromIntegral (rowStateWord (serviceRowExpected row))))
            , ("status", s "missing")
            ]
        [result] ->
          merge
            result
            [ ("property", n (fromIntegral (serviceRowProperty row)))
            , ("row", n (fromIntegral (serviceRowToken row)))
            , ("expected", n (fromIntegral (rowStateWord (serviceRowExpected row))))
            , ("status", s (caseOutcomeLabel (outcomeOf result)))
            ]
        duplicates ->
          obj
            [ ("id", s (serviceRowId row))
            , ("property", n (fromIntegral (serviceRowProperty row)))
            , ("row", n (fromIntegral (serviceRowToken row)))
            , ("expected", n (fromIntegral (rowStateWord (serviceRowExpected row))))
            , ("status", s "duplicate")
            , ("duplicates", n (fromIntegral (length duplicates)))
            ]
    qualifiedRows = length [() | result <- found, outcomeOf result == CasePassed]
    failed = [result | result <- found, outcomeOf result == CaseFailed]
    blocked = [result | result <- found, outcomeOf result `elem` [CaseBlocked, CaseUnsupported, CaseTruncated]]
    unknown = [result | result <- found, outcomeOf result == CaseUnknown]
    malformed = [(result, reason) | result <- found, CaseMalformed reason <- [outcomeOf result]]
    mutantProblems result =
      let entries = list (get result "mutants")
          names = map (\entry -> str (get entry "name")) entries
          statuses = map (\entry -> str (get entry "status")) entries
          required = serviceMutantNames
          duplicated = nub [name | name <- names, length (filter (== name) names) > 1]
       in [ "mutant domain differs: expected " ++ show (sort required) ++ ", observed " ++ show (sort names)
          | sort names /= sort required
          ]
            ++ [ "duplicate mutant control: " ++ name | name <- duplicated ]
            ++ [ "mutant " ++ name ++ ": " ++ resultStatus
               | (name, resultStatus) <- zip names statuses
               , resultStatus /= "refused"
               ]
    failures =
      [ str (get result "id") ++ ": failed"
      | result <- failed
      ]
        ++ [ str (get result "id") ++ ": " ++ problem
           | result <- failed
           , problem <- map str (list (get result "mismatches"))
           ]
        ++ [ str (get result "id") ++ ": " ++ reason
           | (result, reason) <- malformed
           ]
        ++ [ "duplicate required case id: " ++ rowId | rowId <- duplicateRows ]
        ++ [ "unknown service case id: " ++ rowId | rowId <- unknownRows ]
        ++ [ str (get result "id") ++ ": " ++ problem
           | result <- found
           , outcomeOf result == CasePassed
           , problem <- mutantProblems result
           ]
    blockers =
      [ str (get result "id") ++ ": " ++ caseOutcomeLabel (outcomeOf result)
      | result <- blocked
      ]
    unknowns = [str (get result "id") ++ ": " ++ caseOutcomeLabel (outcomeOf result) | result <- unknown]
    coverageGaps =
      [ "capability requires all " ++ show (length serviceRows) ++ " direct service rows"
      | not (null missingRows)
      ]
    status
      | not (null failures) = "failed"
      | not (null blockers) = "blocked"
      | not (null unknowns) = "unknown"
      | not (null coverageGaps) = "partial"
      | qualifiedRows /= length serviceRows = "failed"
      | otherwise = "qualified"
    mutantSummaries =
      [ obj
          [ ("case_id", get result "id")
          , ("name", get mutant "name")
          , ("status", get mutant "status")
          , ("mismatches", get mutant "mismatches")
          ]
      | result <- found
      , mutant <- list (get result "mutants")
      ]
    resultArtifacts result = list (get result "artifacts")
    sameArtifact left right =
      str (get left "id") == str (get right "id")
        && str (get left "sha256") == str (get right "sha256")
    evidenceLimits =
      [ "The contract digest binds rows, state domain, return and preserved registers, seeds, stack and transient spans, endpoints, lifetime, effect policy and mutant domain."
      , "The per-case mutant entries are checks injected into an observed case state; they are not separate candidate binaries."
      , "A test-only ARM write/restore mutant is executed by //replacement/service:qualification-tests through runServiceCase and the native runner; that test reports the executed refusal."
      , "The direct-row results are this report's case data; packaged publication and teardown evidence is bound by its own receipt and is not claimed here."
      , "r11 is saved by the Rust prologue, so the public entry's preservation is asserted; a stock r11 clobber is not isolated from that save."
      ]

reportQualified :: Value -> Bool
reportQualified report = get report "capability" == b True
