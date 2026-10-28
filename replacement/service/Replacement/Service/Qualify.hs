{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
-- | Native qualification for the retained row-availability service.
--
-- The direct cases enter the built ARM @rb_service_field_row_state@, the
-- assembly trampoline and the original stock @FUN_00d52684@. The consumer
-- separates the public result from the stack workspace: @casebuf[0x1e00..0x2000)@
-- is stack scratch, the stock table mirror is transient scratch, and every
-- other byte is immutable. Write provenance must be complete and confined to
-- those spans.
module Replacement.Service.Qualify
  ( RowState (..)
  , rowStateWord
  , ServiceRow (..)
  , serviceRows
  , serviceContractSha256
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
  , serviceInitialRegisters
  , serviceExpectedRegisters
  , isServiceCaseId
  , Observation (..)
  , observe
  , Expectation (..)
  , expectationFromCase
  , Check (..)
  , classifyStatus
  , checkServiceCase
  , checkStatus
  , checkProblems
  , hexWord32
  , mutateRegister
  , mutateWrite
  , runServiceCase
  , serviceQualificationReport
  , reportQualified
  ) where

import Control.Exception (IOException, try)
import Control.Monad (forM, unless, when)
import Data.Aeson (Value (..))
import Data.Bits (xor)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Char (isHexDigit)
import Data.List (find, nubBy)
import Data.Maybe (fromMaybe, isNothing)
import Data.Word (Word32, Word64, Word8)
import Emulation.NativeCase (Region (..), loadCase)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host hiding (sha256)
import Numeric (readHex, showHex)
import System.Directory (canonicalizePath)

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

-- | The packaged lane asks property 0x110 for rows 0..4; the ISO edit list
-- asks 0x141 for the stock list's 21 rows.
serviceRows :: [ServiceRow]
serviceRows = map toneRow [0 .. 4] ++ map isoRow isoRows
  where
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

serviceContractSha256 :: String
serviceContractSha256 = sha256Hex (sha256 (BC.pack (unlines contractLines)))
  where
    contractLines =
      [ serviceRowId row ++ " " ++ showHex (serviceRowProperty row) "" ++ " "
          ++ showHex (serviceRowToken row) "" ++ " " ++ show (rowStateWord (serviceRowExpected row))
      | row <- serviceRows
      ]

-- The stack is the top of the casebuf region; the checked public output and
-- the immutable body sit below it.
serviceStackRegion :: String
serviceStackRegion = "casebuf"

serviceStackBase, serviceStackOffset, serviceStackSize :: Int
serviceStackBase = 0x009a0000
serviceStackOffset = 0x1e00
serviceStackSize = 0x200

serviceStackTop :: Word32
serviceStackTop = 0x009a1ff0

serviceTransientRegion :: String
serviceTransientRegion = "mirror"

serviceTransientOffset, serviceTransientSize :: Int
serviceTransientOffset = 0
serviceTransientSize = 0x200

serviceCasebufSize :: Int
serviceCasebufSize = 0x2000

serviceCasebufFill :: Word8
serviceCasebufFill = 0xa5

serviceCasebufInitial :: B.ByteString
serviceCasebufInitial = B.replicate serviceCasebufSize serviceCasebufFill

serviceSeeds :: [(String, Word32)]
serviceSeeds =
  [ ("r4", 0x4a4a4a4a)
  , ("r5", 0x5b5b5b5b)
  , ("r6", 0x6c6c6c6c)
  , ("r7", 0x7d7d7d7d)
  , ("r8", 0x8e8e8e8e)
  , ("r9", 0x9f9f9f9f)
  , ("r10", 0xa0a0a0a0)
  , ("r11", 0xb1b1b1b1)
  ]

serviceInitialRegisters :: Word32 -> Word32 -> Word32 -> Word32 -> [(String, Word32)]
serviceInitialRegisters property row stackTop exit =
  [ ("r0", property)
  , ("r1", row)
  , ("r2", 0)
  , ("r3", 0)
  ]
    ++ serviceSeeds
    ++ [ ("r12", 0)
       , ("sp", stackTop)
       , ("lr", exit)
       , ("cpsr", 0x10)
       ]

serviceExpectedRegisters :: Word32 -> Word32 -> Word32 -> [(String, Word32)]
serviceExpectedRegisters expected stackTop exit =
  ("r0", expected) : serviceSeeds ++ [("sp", stackTop), ("pc", exit)]

isServiceCaseId :: String -> Bool
isServiceCaseId = ("svc-" `prefixOf`)
  where
    prefixOf prefix text = take (length prefix) text == prefix

hexWord32 :: Word32 -> String
hexWord32 value = "0x" ++ replicate (8 - length text) '0' ++ text
  where
    text = showHex value ""

parseWord32 :: String -> Maybe Word32
parseWord32 ('0' : 'x' : digits) = parseDigits digits
parseWord32 digits = parseDigits digits

parseDigits :: String -> Maybe Word32
parseDigits digits
  | null digits || length digits > 8 || not (all isHexDigit digits) = Nothing
  | otherwise =
      case readHex digits :: [(Word32, String)] of
        [(value, "")] -> Just value
        _ -> Nothing

unhex :: String -> Maybe B.ByteString
unhex text
  | odd (length text) = Nothing
  | otherwise = B.pack <$> go text
  where
    go [] = Just []
    go (highChar : lowChar : rest) = do
      high <- digit highChar
      low <- digit lowChar
      ((high * 16 + low) :) <$> go rest
    go _ = Nothing
    digit char
      | char >= '0' && char <= '9' = Just (fromIntegral (fromEnum char - fromEnum '0'))
      | char >= 'a' && char <= 'f' = Just (fromIntegral (fromEnum char - fromEnum 'a' + 10))
      | otherwise = Nothing

data Observation = Observation
  { observationStatus :: String
  , observationRegisters :: [(String, Word32)]
  , observationMemory :: [(String, B.ByteString)]
  , observationWrites :: Maybe [(Word64, Int)]
  , observationProvenanceTotal :: Word64
  , observationInstructions :: Word64
  , observationEngineError :: Word32
  }
  deriving (Eq, Show)

requiredRegisters :: [String]
requiredRegisters =
  ["r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9", "r10",
   "r11", "r12", "sp", "lr", "cpsr", "pc"]

observe :: [(String, Word64, Int)] -> Value -> Either String Observation
observe regions value = do
  status <- case get value "status" of
    String _ -> Right (str (get value "status"))
    _ -> Left "missing runner status"
  when (null status) (Left "empty runner status")
  registers <- parseRegisters (get value "registers")
  memory <- parseMemory regions (get value "memory")
  total <- requireNumber value "memory_provenance_total"
  provenance <- case get value "memory_provenance" of
    Array _ -> Right (list (get value "memory_provenance"))
    _ -> Left "missing memory provenance"
  writes <- parseProvenance provenance
  instructions <- requireNumber value "instructions"
  engineError <- parseEngineError (get value "unicorn_error")
  let truncated = total > 4096
  if truncated
    then when (length provenance /= 4096) (Left "provenance retention count mismatch")
    else when (fromIntegral (length provenance) /= total) (Left "provenance count mismatch")
  pure
    Observation
      { observationStatus = status
      , observationRegisters = registers
      , observationMemory = memory
      , observationWrites = if truncated then Nothing else Just writes
      , observationProvenanceTotal = total
      , observationInstructions = instructions
      , observationEngineError = engineError
      }

requireNumber :: Value -> String -> Either String Word64
requireNumber value key =
  case get value key of
    String _ -> maybe (Left ("malformed " ++ key)) Right (readDecimal (str (get value key)))
    Number _ ->
      let number = num (get value key)
       in if number >= 0 && number <= fromIntegral (maxBound :: Word64)
            then Right (fromIntegral number)
            else Left ("malformed " ++ key)
    _ -> Left ("missing " ++ key)

readDecimal :: String -> Maybe Word64
readDecimal text = case reads text of
  [(value, "")] -> Just value
  _ -> Nothing

parseEngineError :: Value -> Either String Word32
parseEngineError value = case value of
  Number _ ->
    let number = num value
     in if number >= 0 && number <= 4294967295
          then Right (fromIntegral number)
          else Left "malformed unicorn_error"
  _ -> Left "missing unicorn_error"

parseRegisters :: Value -> Either String [(String, Word32)]
parseRegisters value = case value of
  Object _ -> do
    pairs <- forM (keys value) $ \name ->
      case parseWord32 (str (get value name)) of
        Just word -> Right (name, word)
        Nothing -> Left ("malformed register " ++ name)
    let missing = [name | name <- requiredRegisters, isNothing (lookup name pairs)]
    if null missing then Right pairs else Left ("missing registers " ++ unwords missing)
  _ -> Left "missing registers"

parseMemory :: [(String, Word64, Int)] -> Value -> Either String [(String, B.ByteString)]
parseMemory regions value = case value of
  Array _ -> do
    let entries = list value
    when (length entries /= length regions) (Left "memory region count mismatch")
    forM (zip regions entries) $ \((ident, base, size), entry) -> do
      address <- case parseWord32 (str (get entry "address")) of
        Just word -> Right (fromIntegral word :: Word64)
        Nothing -> Left ("malformed address for region " ++ ident)
      when (address /= base) (Left ("memory region " ++ ident ++ " address mismatch"))
      bytes <- maybe (Left ("malformed bytes for region " ++ ident)) Right (unhex (str (get entry "bytes")))
      when (B.length bytes /= size) (Left ("memory region " ++ ident ++ " size mismatch"))
      pure (ident, bytes)
  _ -> Left "missing memory"

parseProvenance :: [Value] -> Either String [(Word64, Int)]
parseProvenance entries = do
  parsed <- forM entries $ \entry -> do
    kind <- case str (get entry "kind") of
      "write" -> Right True
      "read" -> Right False
      _ -> Left "malformed provenance kind"
    address <- case parseWord32 (str (get entry "address")) of
      Just word -> Right (fromIntegral word :: Word64)
      Nothing -> Left "malformed provenance address"
    size <- case get entry "size" of
      Number _ -> case num (get entry "size") of
        value | value >= 1 && value <= 8 -> Right (fromIntegral value)
        _ -> Left "malformed provenance size"
      _ -> Left "missing provenance size"
    pure (kind, address, size)
  pure [(address, size) | (True, address, size) <- parsed]

data Expectation = Expectation
  { expectationExpectedR0 :: Word32
  , expectationPreserved :: [(String, Word32)]
  , expectationInitialMemory :: [(String, B.ByteString)]
  , expectationBounds :: [(String, Word64, Int)]
  , expectationStackBase :: Word64
  , expectationStackOffset :: Int
  , expectationStackSize :: Int
  , expectationStackTop :: Word64
  , expectationTransientRegion :: String
  , expectationTransientOffset :: Int
  , expectationTransientSize :: Int
  }
  deriving (Eq, Show)

data Check
  = CheckPass
  | CheckMismatch [String]
  | CheckBlocked String
  | CheckUnsupported String
  | CheckUnknown String
  | CheckTruncated String
  deriving (Eq, Show)

classifyStatus :: String -> Check
classifyStatus "returned" = CheckPass
classifyStatus "unknown_call" = CheckUnsupported "unknown_call"
classifyStatus "unsupported_instruction" = CheckUnsupported "unsupported_instruction"
classifyStatus "engine_error" = CheckBlocked "engine_error"
classifyStatus "memory_guard" = CheckBlocked "memory_guard"
classifyStatus "fetch_guard" = CheckBlocked "fetch_guard"
classifyStatus "stub_contract" = CheckBlocked "stub_contract"
classifyStatus "invalid_case" = CheckBlocked "invalid_case"
classifyStatus "host_timeout" = CheckBlocked "host_timeout"
classifyStatus "runner_failure" = CheckBlocked "runner_failure"
classifyStatus "invalid_output" = CheckBlocked "invalid_output"
classifyStatus "wall_budget" = CheckBlocked "wall_budget"
classifyStatus "instruction_budget" = CheckBlocked "instruction_budget"
classifyStatus other = CheckUnknown other

checkStatus :: Check -> String
checkStatus CheckPass = "passed"
checkStatus (CheckMismatch _) = "failed"
checkStatus (CheckBlocked _) = "blocked"
checkStatus (CheckUnsupported _) = "unsupported"
checkStatus (CheckUnknown _) = "unknown"
checkStatus (CheckTruncated _) = "truncated"

checkProblems :: Check -> [String]
checkProblems (CheckMismatch problems) = problems
checkProblems (CheckBlocked reason) = [reason]
checkProblems (CheckUnsupported reason) = [reason]
checkProblems (CheckUnknown reason) = [reason]
checkProblems (CheckTruncated reason) = [reason]
checkProblems CheckPass = []

checkServiceCase :: Expectation -> Observation -> Check
checkServiceCase expectation observation =
  case classifyStatus (observationStatus observation) of
    CheckPass ->
      let problems = registers ++ memory ++ effects
       in if null problems then CheckPass else CheckMismatch problems
    other -> other
  where
    registersOf = observationRegisters observation
    missingRegisters =
      [ "register " ++ name ++ ": missing from runner output"
      | name <- "r0" : "pc" : map fst (expectationPreserved expectation)
      , isNothing (lookup name registersOf)
      ]
    registers
      | not (null missingRegisters) = missingRegisters
      | otherwise =
          [ "r0: expected " ++ hexWord32 (expectationExpectedR0 expectation)
              ++ ", observed " ++ hexWord32 result
          | Just result <- [lookup "r0" registersOf]
          , result /= expectationExpectedR0 expectation
          ]
            ++ [ "register " ++ name ++ ": expected " ++ hexWord32 expected
                   ++ ", observed " ++ hexWord32 result
               | (name, expected) <- expectationPreserved expectation
               , Just result <- [lookup name registersOf]
               , result /= expected
               ]
    memory =
      concat
        [ case (lookup ident (observationMemory observation), lookup ident (expectationInitialMemory expectation)) of
            (Nothing, _) -> ["region " ++ ident ++ ": missing from runner output"]
            (_, Nothing) -> ["region " ++ ident ++ ": missing initial bytes"]
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
                  [ "transient region " ++ ident ++ " did not return to its initial bytes"
                  | actual /= initial
                  ]
              | otherwise ->
                  [ "immutable region " ++ ident ++ " changed"
                  | actual /= initial
                  ]
        | (ident, _, _) <- expectationBounds expectation
        ]
    effects = case observationWrites observation of
      Nothing -> ["effect evidence truncated at " ++ show (observationProvenanceTotal observation) ++ " writes"]
      Just writes -> concatMap classifyWrite writes
    stackSpan =
      ( expectationStackBase expectation + fromIntegral (expectationStackOffset expectation)
      , fromIntegral (expectationStackSize expectation)
      )
    classifyWrite (address, size) =
      case find (\(ident, base, extent) -> contains base extent address size) (expectationBounds expectation) of
        Nothing -> ["forbidden write outside the declared regions at " ++ show address]
        Just (ident, base, extent)
          | ident == serviceStackRegion ->
              let (low, spanSize) = stackSpan
                  high = low + fromIntegral spanSize
               in [ "stack write outside the scratch span at " ++ show address
                  | address < low || address + fromIntegral size > high
                      || address + fromIntegral size > expectationStackTop expectation
                  ]
          | ident == expectationTransientRegion expectation ->
              let low = base + fromIntegral (expectationTransientOffset expectation)
                  high = low + fromIntegral (expectationTransientSize expectation)
               in [ "transient write outside " ++ ident ++ " at " ++ show address
                  | address < low || address + fromIntegral size > high
                  ]
          | otherwise -> ["forbidden write to immutable region " ++ ident ++ " at " ++ show address]
    contains base extent address size =
      address >= base && address + fromIntegral size <= base + fromIntegral extent

expectationFromCase :: Value -> [Region] -> Either String Expectation
expectationFromCase caseValue regions = do
  let expected = get caseValue "expected"
      expectedRegisters = get expected "registers"
      initialRegisters = get caseValue "registers"
  expectedR0 <- readRegister "r0" expectedRegisters
  preserved <- forM ["r4", "r5", "r6", "r7", "r8", "r9", "r10", "r11", "sp", "pc"] $ \name ->
    (,) name <$> readRegister name expectedRegisters
  stackTop <- readRegister "sp" expectedRegisters
  stackRegion <- maybe (Left "service stack region casebuf missing") Right (find ((== serviceStackRegion) . regionIdent) regions)
  let stackBase = fromIntegral (baseAddress stackRegion) :: Word64
  unless (stackBase == fromIntegral serviceStackBase) (Left "service stack region has an unexpected base")
  unless (fromIntegral (regionSize stackRegion) >= serviceStackOffset + serviceStackSize) (Left "service stack region is too small")
  unless (maybe False (\region -> fromIntegral (regionSize region) >= serviceTransientSize) (find ((== serviceTransientRegion) . regionIdent) regions)) (Left "service transient region mirror missing")
  pure
    Expectation
      { expectationExpectedR0 = expectedR0
      , expectationPreserved = preserved
      , expectationInitialMemory = [(regionIdent region, regionBytes region) | region <- regions]
      , expectationBounds =
          [ (regionIdent region, fromIntegral (baseAddress region), fromIntegral (regionSize region))
          | region <- regions
          ]
      , expectationStackBase = stackBase
      , expectationStackOffset = serviceStackOffset
      , expectationStackSize = serviceStackSize
      , expectationStackTop = fromIntegral stackTop
      , expectationTransientRegion = serviceTransientRegion
      , expectationTransientOffset = serviceTransientOffset
      , expectationTransientSize = serviceTransientSize
      }
  where
    readRegister name object =
      case parseWord32 (str (get object name)) of
        Just value -> Right value
        Nothing -> Left ("expected register missing or malformed: " ++ name)

regionIdent :: Region -> String
regionIdent region = str (get (description region) "id")

regionInfo :: [Region] -> [(String, Word64, Int)]
regionInfo regions =
  [ (regionIdent region, fromIntegral (baseAddress region), fromIntegral (regionSize region))
  | region <- regions
  ]

mutateRegister :: Observation -> String -> Word32 -> Observation
mutateRegister observation name value =
  observation
    { observationRegisters =
        [ if key == name then (key, value) else (key, old)
        | (key, old) <- observationRegisters observation
        ]
    }

mutateWrite :: Observation -> Word64 -> Int -> Observation
mutateWrite observation address size =
  observation
    { observationWrites = fmap (++ [(address, size)]) (observationWrites observation)
    , observationProvenanceTotal = observationProvenanceTotal observation + 1
    }

runServiceCase :: FilePath -> FilePath -> IO (Value, Value)
runServiceCase runner path = do
  loaded <- try (loadCase path)
  case loaded of
    Left (message :: IOException) -> pure (invalidCase (show message), invalidResult (show message))
    Right (caseValue, regions, wire, identity) ->
      case expectationFromCase caseValue regions of
        Left message -> pure (invalidCase message, invalidResult message)
        Right expectation -> do
          executable <- canonicalizePath runner
          digest <- sha256Hex . sha256 <$> B.readFile executable
          let limits = get caseValue "limits"
              wall = num (get limits "wall_ms")
              timeout = fromIntegral (wall * 1000000 + 2000000) :: Int
          (timed, _exit, out, err) <- runBounded timeout (8 * 1024 * 1024) executable [] wire
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
              , merge (invalidResult "host timeout") [("id", get caseValue "id"), ("status", s "blocked")]
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
                    , merge (invalidResult (show message)) [("id", get caseValue "id"), ("status", s "blocked")]
                    )
                Right value
                  | get value "wire_version" /= n 1 ->
                      pure
                        ( merge baseReport
                            [ ("status", s "invalid_output")
                            , ("reason", s "wire_version is not 1")
                            , ("runner", value)
                            ]
                        , merge (invalidResult "wire_version is not 1") [("id", get caseValue "id"), ("status", s "blocked")]
                        )
                  | otherwise ->
                      case observe (regionInfo regions) value of
                        Left message ->
                          pure
                            ( merge baseReport
                                [ ("status", s "invalid_output")
                                , ("reason", s message)
                                , ("runner", value)
                                ]
                            , merge (invalidResult message) [("id", get caseValue "id"), ("status", s "blocked")]
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
                                    , ("observed_r0", maybe Null (n . fromIntegral) (lookup "r0" (observationRegisters observation)))
                                    , ("runner_status", s (observationStatus observation))
                                    , ("provenance_total", n (fromIntegral (observationProvenanceTotal observation)))
                                    , ("provenance_complete", b (isJustWrites (observationWrites observation)))
                                    , ("instructions", n (fromIntegral (observationInstructions observation)))
                                    , ("mutants", arr mutants)
                                    ])
                                  identity
                              report =
                                merge
                                  baseReport
                                  [ ("status", s status)
                                  , ("runner_status", s (observationStatus observation))
                                  , ("check", s status)
                                  , ("mismatches", arr (map s problems))
                                  , ("provenance_total", n (fromIntegral (observationProvenanceTotal observation)))
                                  , ("runner", value)
                                  ]
                          pure (report, result)
  where
    isJustWrites (Just _) = True
    isJustWrites Nothing = False
    invalidCase message =
      obj [("schema_version", n 1), ("status", s "invalid_case"), ("reason", s message)]
    invalidResult message =
      obj [("status", s "blocked"), ("mismatches", arr [s message])]

mutantChecks :: Expectation -> Observation -> [Value]
mutantChecks expectation observation =
  [ mutant "abi-r7-clobber" (mutateRegister observation "r7" (r7 `xor` 0x01010101))
  , mutant "effect-immutable-write" (mutateWrite observation (fromIntegral serviceStackBase + 0x100) 4)
  ]
  where
    r7 = fromMaybe 0 (lookup "r7" (observationRegisters observation))
    mutant name changed =
      let check = checkServiceCase expectation changed
       in obj
            [ ("name", s name)
            , ("status", s (if refused check then "refused" else "accepted"))
            , ("mismatches", arr (map s (checkProblems check)))
            ]
    refused (CheckMismatch _) = True
    refused _ = False

serviceQualificationReport :: [Value] -> Value
serviceQualificationReport results =
  obj
    [ ("schema_version", n 1)
    , ("profile", s "service-native")
    , ("contract_sha256", s serviceContractSha256)
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
    found = [result | row <- serviceRows, Just result <- [find ((== serviceRowId row) . str . flip get "id") results]]
    statusOf result = str (get result "status")
    summary row =
      case find ((== serviceRowId row) . str . flip get "id") results of
        Nothing ->
          obj
            [ ("id", s (serviceRowId row))
            , ("property", n (fromIntegral (serviceRowProperty row)))
            , ("row", n (fromIntegral (serviceRowToken row)))
            , ("expected", n (fromIntegral (rowStateWord (serviceRowExpected row))))
            , ("status", s "missing")
            ]
        Just result ->
          merge
            result
            [ ("property", n (fromIntegral (serviceRowProperty row)))
            , ("row", n (fromIntegral (serviceRowToken row)))
            , ("expected", n (fromIntegral (rowStateWord (serviceRowExpected row))))
            ]
    qualifiedRows = length [() | result <- found, statusOf result == "passed"]
    failed = [result | result <- found, statusOf result == "failed"]
    blocked = [result | result <- found, statusOf result `elem` ["blocked", "unsupported", "truncated"]]
    unknown = [result | result <- found, statusOf result == "unknown"]
    missing = [row | row <- serviceRows, isNothing (find ((== serviceRowId row) . str . flip get "id") results)]
    failures =
      [ caseId ++ ": " ++ problem
      | result <- failed
      , let caseId = str (get result "id")
      , problem <- map str (list (get result "mismatches"))
      ]
        ++ [ caseId ++ ": mutant " ++ name ++ " was not refused"
           | result <- found
           , let caseId = str (get result "id")
           , mutant <- list (get result "mutants")
           , str (get mutant "status") /= "refused"
           , let name = str (get mutant "name")
           ]
    blockers =
      [ str (get result "id") ++ ": " ++ statusOf result
      | result <- blocked
      ]
    unknowns = [str (get result "id") ++ ": " ++ statusOf result | result <- unknown]
    coverageGaps =
      [ "capability requires all " ++ show (length serviceRows) ++ " direct service rows"
      | not (null missing)
      ]
    status
      | not (null failures) = "failed"
      | not (null blockers) = "blocked"
      | not (null unknowns) = "unknown"
      | not (null coverageGaps) = "partial"
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
      [ "The native runner executes the built kernel's rb_service_field_row_state -> rb_call_stock_probe -> FUN_00d52684 path; it does not execute RAM-push publication or camera I/O."
      , "The fixture fixes shooting mode 0x24 (M); the other mode columns of the stock probe are not covered."
      , "casebuf[0x1e00,0x2000) is stack scratch and the stock table mirror is transient scratch; every other byte is immutable. Complete provenance is required."
      , "The ABI and effect mutations are injected into the observed state and refused by the same checked consumer; they are not separate candidate binaries."
      , "r11 is saved by the Rust prologue, so the public entry's preservation is asserted; a stock r11 clobber is not isolated from that save."
      ]

reportQualified :: Value -> Bool
reportQualified report = get report "capability" == b True
