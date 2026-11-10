{-# LANGUAGE OverloadedStrings #-}

-- | Task-record create/delete cases over the stock loaders and the owned native entries.
module Replacement.RtosTaskRecords.Suite
  ( Image (..)
  , CaseRun (..)
  , ControlRun (..)
  , runDomain
  , parseOwnedSymbols
  ) where

import Control.Monad (forM, forM_, unless)
import Data.Bits ((.|.), shiftL, xor)
import qualified Data.ByteString as B
import Data.List (isPrefixOf, nub, sort, sortOn)
import qualified Data.Map.Strict as M
import Data.Word (Word32, Word8)
import Emulation.CachedNativeBatch (BatchReuse, cachedObservedBatch)
import Emulation.NativeObservation
import Emulation.OwnedCase
import Replacement.RtosOwnFirst.Regions (Domain (..), domainLabel)
import Replacement.RtosTaskRecords.Layout
import qualified Replacement.RtosPlatform.Aggregate as Agg
import System.FilePath ((</>))

data Image = Image { imageBase :: Word32, imageBytes :: B.ByteString, imagePlacements :: [Agg.SectionPlacement] }

data Role = Stock | Owned deriving (Eq, Show)

data Op = Create | DeleteReset | DeleteClear deriving (Eq, Show)

data Kind = Equal | Repair String deriving (Eq, Show)

data Check
  = WordIs String Word32 Word32
  | ByteIs String Word32 Word8
  | RegisterIs Register Word32
  | CpsrControlIs Word32
  | RegionsUnchanged [String]
  | DmbBeforeWrite Word32
  deriving (Eq, Show)

data Row = Row
  { rowSlot :: Word32
  , rowExists :: Word32
  , rowState :: Word32
  , rowFlags :: Word32
  , rowBound :: Bool
  , rowPriority :: Word32
  , rowStack :: Word32
  , rowAttributes :: Word32
  , rowArgument :: Word32
  , rowEntry :: Word32
  , rowStackBase :: Word32
  , rowClass :: Word32
  , rowMode :: Word32
  , rowCpsr :: Word32
  }
  deriving (Eq, Show)

data Case = Case
  { caseName :: String
  , caseOp :: Op
  , caseRow :: Row
  , caseKind :: Kind
  , caseEqualChecks :: [Check]
  , caseStockChecks :: [Check]
  , caseOwnedChecks :: [Check]
  , caseOwnedOnlyChecks :: [Check]
  }
  deriving (Eq, Show)

data CaseRun = CaseRun
  { caseDomain :: Domain
  , runCohort :: String
  , runName :: String
  , runKind :: Kind
  , runChecks :: [(String, Bool)]
  , runDifferences :: [String]
  , runReuse :: (BatchReuse, BatchReuse)
  }

data ControlRun = ControlRun
  { controlCohort :: String
  , controlName :: String
  , controlCase :: String
  , controlHolds :: Bool
  }

requestBase, stackBase, stackSize, stackPointer, exitAddress :: Word32
requestBase = 0x0100_0000
stackBase = 0x0030_0000
stackSize = 0x1000
stackPointer = stackBase + 0x800
exitAddress = 0x0100_0200

createAddress, deleteAddress :: Domain -> Word32
createAddress DomainA = 0x0004_d094
createAddress DomainB = 0x000d_d054
deleteAddress DomainA = 0x0004_d1a4
deleteAddress DomainB = 0x000d_d168

createExtent, deleteExtent :: Domain -> Word32
createExtent DomainA = 0x110
createExtent DomainB = 0x114
deleteExtent _ = 0x104

dataRegions :: [String]
dataRegions = ["definitions", "tasks", "map"]

seedRegister :: Register -> Word32
seedRegister register = 0xa5a5_0000 + fromIntegral (fromEnum register)

baseRow :: Word32 -> Op -> Row
baseRow slot op =
  Row
    { rowSlot = slot
    , rowExists = if op == Create then 0 else 1
    , rowState = 0
    , rowFlags = 0xff
    , rowBound = True
    , rowPriority = 5
    , rowStack = 0x101
    , rowAttributes = 0x1111_1111
    , rowArgument = 0x2222_2222
    , rowEntry = 0x4000_1000
    , rowStackBase = 0x0080_0000
    , rowClass = 0
    , rowMode = if op == DeleteReset then 1 else 0
    , rowCpsr = 0x13
    }

definitionAddress :: Facts -> Domain -> Word32 -> Word32
definitionAddress facts domain slot =
  fromIntegral
    ( fromIntegral (dfTaskDefinitions (domainFacts facts domain))
        + (fromIntegral slot - 1) * fromIntegral (definitionSize (factsDefinition facts))
        :: Int
    )

taskAddress :: Facts -> Domain -> Word32 -> Word32
taskAddress facts domain slot =
  fromIntegral
    ( fromIntegral (dfTaskTable (domainFacts facts domain))
        + (fromIntegral slot - 1) * fromIntegral (taskSize (factsTask facts))
        :: Int
    )

creationIdAddress :: Facts -> Domain -> Word32 -> Word32
creationIdAddress facts domain slot =
  fromIntegral (fromIntegral (dfTaskSlotIds (domainFacts facts domain)) + (fromIntegral slot - 1) * 4 :: Int)

definitionField :: Facts -> Domain -> Word32 -> Word32 -> Word32
definitionField facts domain slot offset = definitionAddress facts domain slot + offset

taskField :: Facts -> Domain -> Word32 -> Word32 -> Word32
taskField facts domain slot offset = taskAddress facts domain slot + offset

existsField :: Facts -> Domain -> Word32 -> Word32
existsField facts domain slot = definitionField facts domain slot (definitionExists (factsDefinition facts))

definitionBase, taskBase, creationIdBase :: Facts -> Domain -> Word32
definitionBase facts domain = definitionAddress facts domain 0
taskBase facts domain = taskAddress facts domain 0
creationIdBase facts domain = creationIdAddress facts domain 0

definitionRegionSize, taskRegionSize, creationIdRegionSize :: Facts -> Word32
definitionRegionSize facts = (factsTaskSlots facts + 2) * definitionSize (factsDefinition facts)
taskRegionSize facts = (factsTaskSlots facts + 2) * taskSize (factsTask facts)
creationIdRegionSize facts = (factsTaskSlots facts + 2) * 4

staleBinding :: Facts -> Word32 -> Word32
staleBinding facts slot
  | slot < factsTaskSlots facts = slot + 1
  | otherwise = slot - 1

initialWords :: Facts -> Domain -> Row -> [(Word32, Word32)]
initialWords facts domain row =
  concatMap definitionWords slots ++ concatMap taskWords slots ++ mapWords ++ requestWords
  where
    slots = [0 .. factsTaskSlots facts + 1]
    target = rowSlot row
    d = factsDefinition facts
    t = factsTask facts
    domainWords = domainFacts facts domain
    isTarget slot = slot == target
    definitionWords slot =
      let at = definitionAddress facts domain slot
          defaultWords =
            [ (at + definitionAttributes d, 0)
            , (at + definitionArgument d, if slot == 0 then 0xffff_ffff else slot - 1)
            , (at + definitionEntry d, 0)
            , (at + definitionPriority d, 15)
            , (at + definitionStackSize d, 4)
            , (at + definitionStack d, dfDefaultStackWord domainWords)
            , (at + definitionExceptionAttributes d, 0)
            , (at + definitionExceptionHandler d, 0)
            , (at + definitionSlot d, slot)
            , (at + definitionExists d, 0)
            ]
          targetWords =
            [ (at + definitionAttributes d, rowAttributes row)
            , (at + definitionArgument d, rowArgument row)
            , (at + definitionEntry d, rowEntry row)
            , (at + definitionPriority d, rowPriority row)
            , (at + definitionStackSize d, rowStack row)
            , (at + definitionStack d, rowStackBase row)
            , (at + definitionExceptionAttributes d, 0xeeee_eeee)
            , (at + definitionExceptionHandler d, 0xffff_ffff)
            , (at + definitionSlot d, rowSlot row)
            , (at + definitionExists d, rowExists row)
            ]
       in if isTarget slot then targetWords else defaultWords
    taskWords slot =
      let at = taskAddress facts domain slot
          binding =
            if isTarget slot && not (rowBound row)
              then definitionAddress facts domain (staleBinding facts slot)
              else definitionAddress facts domain slot
          defaultWords =
            [ (at + taskNext t, 0)
            , (at + taskPrev t, 0)
            , (at + taskDefinition t, binding)
            , (at + taskState t, 15 `shiftL` 8)
            , (at + taskExceptionPattern t, 0)
            , (at + taskWait t, 0)
            , (at + taskSavedSp t, 0)
            , (at + taskResumePc t, 0)
            , (at + taskDomain t, dfDomainId domainWords)
            , (at + taskTerminatePending t, 0)
            , (at + taskPriorityChangePending t, 0)
            , (at + taskCancelPending t, 0)
            , (at + taskContextDiscarded t, 0)
            , (at + taskPendingPriority t, 0)
            ]
          targetWords =
            [ (at + taskNext t, 0)
            , (at + taskPrev t, 0)
            , (at + taskDefinition t, binding)
            , (at + taskState t, rowState row .|. (0xff `shiftL` 8) .|. (rowFlags row `shiftL` 16))
            , (at + taskExceptionPattern t, 0xeeee_eeee)
            , (at + taskWait t, 0)
            , (at + taskSavedSp t, 0x1111_1111)
            , (at + taskResumePc t, 0x2222_2222)
            , (at + taskDomain t, 0)
            , (at + taskTerminatePending t, 0xaaaa_aaaa)
            , (at + taskPriorityChangePending t, 0xbbbb_bbbb)
            , (at + taskCancelPending t, 0xcccc_cccc)
            , (at + taskContextDiscarded t, 0xdddd_dddd)
            , (at + taskPendingPriority t, 0x9999_9999)
            ]
       in if isTarget slot then targetWords else defaultWords
    mapWords = [(creationIdAddress facts domain slot, 0) | slot <- slots]
    requestWords =
      [ (requestBase, rowAttributes row)
      , (requestBase + 0x04, rowArgument row)
      , (requestBase + 0x08, rowEntry row)
      , (requestBase + 0x0c, rowPriority row)
      , (requestBase + 0x10, rowStack row)
      , (requestBase + 0x14, rowStackBase row)
      ]

overlay :: Word32 -> Word32 -> [(Word32, Word32)] -> B.ByteString
overlay base size words' = B.concat (pieces 0 ordered)
  where
    sizeI = fromIntegral size :: Int
    ordered =
      M.toAscList
        ( M.fromList
            [ (fromIntegral (address - base) :: Int, value)
            | (address, value) <- words'
            , address >= base
            , address + 4 <= base + size
            ]
        )
    pieces cursor [] = [B.replicate (sizeI - cursor) 0]
    pieces cursor ((offset, value) : rest) =
      B.replicate (offset - cursor) 0 : word32le value : pieces (offset + 4) rest

slice :: Word32 -> B.ByteString -> Word32 -> Word32 -> B.ByteString
slice base bytes address size
  | address < base || address + size > base + fromIntegral (B.length bytes) = error "task-record code slice is outside its image"
  | otherwise = B.take (fromIntegral size) (B.drop (fromIntegral (address - base)) bytes)

expectedResult :: Facts -> Role -> Op -> Row -> Word32
expectedResult _facts Stock op row = case op of
  Create -> if rowExists row /= 0 then statusAlreadyExists else 0
  _ -> 0
expectedResult facts Owned op row = case op of
  Create
    | invalidSlot -> statusInvalid
    | rowExists row /= 0 -> statusAlreadyExists
    | rowState row /= 0 -> statusAlreadyExists
    | not (rowBound row) -> statusInvalid
    | rowPriority row < 1 || rowPriority row > 16 -> statusInvalid
    | rowStack row > 0xffff_fffc -> statusInvalid
    | otherwise -> 0
  DeleteClear
    | invalidSlot -> statusInvalid
    | rowExists row == 0 -> statusAlreadyExists
    | otherwise -> 0
  DeleteReset
    | invalidSlot -> statusInvalid
    | rowState row /= 0 -> statusAlreadyExists
    | not (rowBound row) -> statusInvalid
    | otherwise -> 0
  where
    invalidSlot = rowSlot row == 0 || rowSlot row > factsTaskSlots facts

statusAlreadyExists, statusInvalid :: Word32
statusAlreadyExists = 0xffff_ffd7
statusInvalid = 0xffff_ffee

standardChecks :: Row -> [Check]
standardChecks row =
  [ RegisterIs register (seedRegister register)
  | register <- [R4 .. R11]
  ]
    ++ [ RegisterIs Sp stackPointer
       , CpsrControlIs (cpsrControl (rowCpsr row))
       ]

checksFor :: Facts -> Domain -> Role -> Case -> [Check]
checksFor facts domain role one =
  standardChecks (caseRow one)
    ++ case role of
      Stock -> case caseKind one of
        Equal -> caseEqualChecks one
        Repair _ -> caseStockChecks one
      Owned -> case caseKind one of
        Equal -> caseEqualChecks one ++ caseOwnedOnlyChecks one
        Repair _ -> caseOwnedChecks one ++ caseOwnedOnlyChecks one
  where
    _unused = (facts, domain)

check :: Facts -> Domain -> Case -> NativeObservation -> Check -> Either String ()
check facts domain one observation check' = case check' of
  WordIs region address value -> expectWord region address value
  ByteIs region address value -> do
    bytes <- readRegion region
    let base = regionBase region
    if address < base || address >= base + fromIntegral (B.length bytes)
      then Left ("address " ++ showHexWord address ++ " is outside " ++ region)
      else
        let actual = B.index bytes (fromIntegral (address - base))
         in unless (actual == value) (Left (region ++ " " ++ showHexWord address ++ " is " ++ showHexWord (fromIntegral actual) ++ ", expected " ++ showHexWord (fromIntegral value)))
  RegisterIs register value ->
    case lookupRegister register observation of
      Just actual | actual == value -> Right ()
      Just actual -> Left (registerLabel register ++ " is " ++ showHexWord actual ++ ", expected " ++ showHexWord value)
      Nothing -> Left ("missing register " ++ registerLabel register)
  CpsrControlIs value ->
    case lookupRegister Cpsr observation of
      Just actual
        | cpsrControl actual == cpsrControl value -> Right ()
        | otherwise -> Left ("cpsr control " ++ showHexWord (cpsrControl actual) ++ ", expected " ++ showHexWord (cpsrControl value))
      Nothing -> Left "missing register cpsr"
  RegionsUnchanged regions -> forM_ regions $ \region -> do
    actual <- readRegion region
    unless (actual == initialRegion region) (Left ("region " ++ region ++ " changed"))
  DmbBeforeWrite address -> case nativeKernel observation >>= observedOrder of
    Just order ->
      let writes = [index | (index, OrderedWrite (Write (GuestAddress at) _) _) <- zip [0 ..] order, at == fromIntegral address]
       in case writes of
            [] -> Left ("no write to " ++ showHexWord address)
            (first : _) ->
              if any (== OrderedBarrier Dmb) (take first order)
                then Right ()
                else Left ("no DMB before " ++ showHexWord address)
    Nothing -> Left "no ordered log"
  where
    d = factsDefinition facts
    t = factsTask facts
    readRegion region = case lookup (RegionId region) (nativeMemory observation) of
      Just bytes -> Right bytes
      Nothing -> Left ("missing region " ++ region)
    initialRegion "definitions" =
      overlay (definitionBase facts domain) (definitionRegionSize facts) (initialWords facts domain (caseRow one))
    initialRegion "tasks" =
      overlay (taskBase facts domain) (taskRegionSize facts) (initialWords facts domain (caseRow one))
    initialRegion "map" =
      overlay (creationIdBase facts domain) (creationIdRegionSize facts) (initialWords facts domain (caseRow one))
    initialRegion _ = B.empty
    expectWord region address value = do
      bytes <- readRegion region
      let base = regionBase region
      if address < base || address + 4 > base + fromIntegral (B.length bytes)
        then Left ("address " ++ showHexWord address ++ " is outside " ++ region)
        else
          let actual = readWordAt bytes (fromIntegral (address - base))
           in unless (actual == value) (Left (region ++ " " ++ showHexWord address ++ " is " ++ showHexWord actual ++ ", expected " ++ showHexWord value))
    regionBase "definitions" = definitionBase facts domain
    regionBase "tasks" = taskBase facts domain
    regionBase "map" = creationIdBase facts domain
    regionBase _ = 0
    _unused = (d, t)

showHexWord :: Word32 -> String
showHexWord value = "0x" ++ showHex' value
  where
    showHex' 0 = "0"
    showHex' n = reverse (go n)
    go 0 = []
    go n = let (q, r) = n `divMod` 16 in "0123456789abcdef" !! fromIntegral r : go q

describe :: Check -> String
describe check' = case check' of
  WordIs region address value -> region ++ " " ++ showHexWord address ++ " = " ++ showHexWord value
  ByteIs region address value -> region ++ " " ++ showHexWord address ++ " byte = " ++ showHexWord (fromIntegral value)
  RegisterIs register value -> registerLabel register ++ " = " ++ showHexWord value
  CpsrControlIs value -> "cpsr control = " ++ showHexWord (cpsrControl value)
  RegionsUnchanged regions -> "unchanged " ++ unwords regions
  DmbBeforeWrite address -> "dmb before " ++ showHexWord address

regionDifferences :: [String] -> NativeObservation -> NativeObservation -> [String]
regionDifferences regions stock owned =
  [ "region " ++ region ++ " differs"
  | region <- regions
  , M.lookup (RegionId region) (M.fromList (nativeMemory stock)) /= M.lookup (RegionId region) (M.fromList (nativeMemory owned))
  ]

cohortOf :: Case -> String
cohortOf one = case caseName one of
  "smoke-create" -> "smoke"
  "smoke-delete" -> "smoke"
  _ -> "core"

cases :: Facts -> Domain -> [Case]
cases facts domain =
  [ Case "create-new" Create (baseRow 7 Create) Equal [RegisterIs R0 0] [] [] [dmb 7]
  , Case "create-priority-1" Create (baseRow 8 Create) {rowPriority = 1} Equal [RegisterIs R0 0] [] [] [dmb 8]
  , Case "create-priority-16" Create (baseRow 9 Create) {rowPriority = 16} Equal [RegisterIs R0 0] [] [] [dmb 9]
  , Case "create-class-2" Create (baseRow 10 Create) {rowClass = 2} Equal [RegisterIs R0 0] [] [] [dmb 10]
  , Case "create-flags" Create (baseRow 11 Create) {rowFlags = 0xff} Equal [RegisterIs R0 0] [] [] [dmb 11]
  , Case "create-masked" Create (baseRow 12 Create) {rowCpsr = 0xd3} Equal [RegisterIs R0 0] [] [] [dmb 12]
  , Case "create-existing" Create (baseRow 13 Create) {rowExists = 1} Equal [RegisterIs R0 statusAlreadyExists] [] [] []
  , Case
      "create-priority-0"
      Create
      (baseRow 14 Create) {rowPriority = 0}
      (Repair "priority 0 is refused without a write")
      []
      [ RegisterIs R0 0
      , WordIs "definitions" (cfield 14 (definitionPriority d)) 0xffff_ffff
      , WordIs "definitions" (exists 14) 1
      , ByteIs "tasks" (tfield 14 (taskPriority t)) 0xff
      ]
      [RegisterIs R0 statusInvalid, RegionsUnchanged dataRegions]
      []
  , Case
      "create-priority-17"
      Create
      (baseRow 15 Create) {rowPriority = 17}
      (Repair "priority 17 is refused without a write")
      []
      [ RegisterIs R0 0
      , WordIs "definitions" (cfield 15 (definitionPriority d)) 16
      , WordIs "definitions" (exists 15) 1
      , ByteIs "tasks" (tfield 15 (taskPriority t)) 16
      ]
      [RegisterIs R0 statusInvalid, RegionsUnchanged dataRegions]
      []
  , Case
      "create-live-ready"
      Create
      (baseRow 16 Create) {rowState = 1}
      (Repair "a ready task record is refused without a write")
      []
      [ RegisterIs R0 0
      , WordIs "definitions" (exists 16) 1
      , ByteIs "tasks" (tfield 16 (taskState t)) 0
      , ByteIs "tasks" (tfield 16 (taskFlags t)) 0xf8
      , ByteIs "tasks" (tfield 16 (taskPriority t)) 4
      ]
      [RegisterIs R0 statusAlreadyExists, RegionsUnchanged dataRegions]
      []
  , Case
      "create-live-wait"
      Create
      (baseRow 17 Create) {rowState = 0x12}
      (Repair "a blocked task record is refused without a write")
      []
      [RegisterIs R0 0, WordIs "definitions" (exists 17) 1, ByteIs "tasks" (tfield 17 (taskState t)) 0]
      [RegisterIs R0 statusAlreadyExists, RegionsUnchanged dataRegions]
      []
  , Case
      "create-live-queued"
      Create
      (baseRow 18 Create) {rowState = 0x32}
      (Repair "an owner-lock-queued task record is refused without a write")
      []
      [RegisterIs R0 0, WordIs "definitions" (exists 18) 1, ByteIs "tasks" (tfield 18 (taskState t)) 0]
      [RegisterIs R0 statusAlreadyExists, RegionsUnchanged dataRegions]
      []
  , Case
      "create-unbound"
      Create
      (baseRow 19 Create) {rowBound = False}
      (Repair "a stale definition pointer is refused without a write")
      []
      [ RegisterIs R0 0
      , WordIs "definitions" (exists 19) 1
      , ByteIs "tasks" (tfield 19 (taskPriority t)) 15
      ]
      [RegisterIs R0 statusInvalid, RegionsUnchanged dataRegions]
      []
  , Case
      "create-stack-wrap"
      Create
      (baseRow 20 Create) {rowStack = 0xffff_fffd}
      (Repair "stack bytes whose rounding wraps are refused without a write")
      []
      [ RegisterIs R0 0
      , WordIs "definitions" (cfield 20 (definitionStackSize d)) 0
      , WordIs "definitions" (exists 20) 1
      ]
      [RegisterIs R0 statusInvalid, RegionsUnchanged dataRegions]
      []
  , Case
      "create-slot-0"
      Create
      (baseRow 0 Create)
      (Repair "slot 0 is refused instead of writing C[-1]")
      []
      [RegisterIs R0 0, WordIs "definitions" (exists 0) 1, ByteIs "tasks" (tfield 0 (taskState t)) 0]
      [RegisterIs R0 statusInvalid, RegionsUnchanged dataRegions]
      []
  , Case
      "create-slot-121"
      Create
      (baseRow 121 Create)
      (Repair "slot 121 is refused instead of writing C[120]")
      []
      [RegisterIs R0 0, WordIs "definitions" (exists 121) 1]
      [RegisterIs R0 statusInvalid, RegionsUnchanged dataRegions]
      []
  , Case "delete-reset" DeleteReset (baseRow 7 DeleteReset) Equal [RegisterIs R0 0] [] [] [dmb 7]
  , Case "delete-reset-class-2" DeleteReset (baseRow 8 DeleteReset) {rowClass = 2} Equal [RegisterIs R0 0] [] [] [dmb 8]
  , Case "delete-reset-masked" DeleteReset (baseRow 9 DeleteReset) {rowCpsr = 0x93} Equal [RegisterIs R0 0] [] [] [dmb 9]
  , Case "delete-clear" DeleteClear (baseRow 10 DeleteClear) {rowState = 1} Equal [RegisterIs R0 0] [] [] []
  , Case
      "delete-clear-nonexistent"
      DeleteClear
      (baseRow 11 DeleteClear) {rowExists = 0}
      (Repair "clearing a missing definition is refused")
      []
      [RegisterIs R0 0]
      [RegisterIs R0 statusAlreadyExists, RegionsUnchanged dataRegions]
      []
  , Case
      "delete-reset-nonexistent"
      DeleteReset
      (baseRow 12 DeleteReset) {rowExists = 0}
      Equal
      [ RegisterIs R0 0
      , WordIs "definitions" (cfield 12 (definitionAttributes d)) 0
      , WordIs "definitions" (cfield 12 (definitionPriority d)) 15
      , WordIs "definitions" (cfield 12 (definitionStackSize d)) 4
      , WordIs "definitions" (exists 12) 0
      , ByteIs "tasks" (tfield 12 (taskState t)) 0
      ]
      []
      []
      [dmb 12]
  , Case
      "delete-reset-live-ready"
      DeleteReset
      (baseRow 13 DeleteReset) {rowState = 1}
      (Repair "an active task is not reset under the caller")
      []
      [RegisterIs R0 0, WordIs "definitions" (exists 13) 0, ByteIs "tasks" (tfield 13 (taskState t)) 0]
      [RegisterIs R0 statusAlreadyExists, RegionsUnchanged dataRegions]
      []
  , Case
      "delete-reset-live-wait"
      DeleteReset
      (baseRow 14 DeleteReset) {rowState = 0x12}
      (Repair "a blocked task is not reset under the caller")
      []
      [RegisterIs R0 0, WordIs "definitions" (exists 14) 0, ByteIs "tasks" (tfield 14 (taskState t)) 0]
      [RegisterIs R0 statusAlreadyExists, RegionsUnchanged dataRegions]
      []
  , Case
      "delete-reset-live-queued"
      DeleteReset
      (baseRow 15 DeleteReset) {rowState = 0x32}
      (Repair "an owner-lock-queued task is not reset under the caller")
      []
      [RegisterIs R0 0, WordIs "definitions" (exists 15) 0, ByteIs "tasks" (tfield 15 (taskState t)) 0]
      [RegisterIs R0 statusAlreadyExists, RegionsUnchanged dataRegions]
      []
  , Case
      "delete-reset-live-suspended"
      DeleteReset
      (baseRow 16 DeleteReset) {rowState = 4}
      (Repair "a suspended task is not reset under the caller")
      []
      [RegisterIs R0 0, WordIs "definitions" (exists 16) 0, ByteIs "tasks" (tfield 16 (taskState t)) 0]
      [RegisterIs R0 statusAlreadyExists, RegionsUnchanged dataRegions]
      []
  , Case
      "delete-reset-unbound"
      DeleteReset
      (baseRow 17 DeleteReset) {rowBound = False}
      (Repair "a stale definition pointer is refused without a write")
      []
      [RegisterIs R0 0, WordIs "definitions" (exists 17) 0, ByteIs "tasks" (tfield 17 (taskPriority t)) 15]
      [RegisterIs R0 statusInvalid, RegionsUnchanged dataRegions]
      []
  , Case
      "delete-reset-unpublished-live"
      DeleteReset
      (baseRow 18 DeleteReset) {rowExists = 0, rowState = 1}
      (Repair "an unpublished active task is not reset under the caller")
      []
      [RegisterIs R0 0, WordIs "definitions" (exists 18) 0, ByteIs "tasks" (tfield 18 (taskState t)) 0]
      [RegisterIs R0 statusAlreadyExists, RegionsUnchanged dataRegions]
      []
  , Case
      "delete-reset-unpublished-unbound"
      DeleteReset
      (baseRow 19 DeleteReset) {rowExists = 0, rowBound = False}
      (Repair "an unpublished stale definition pointer is refused without a write")
      []
      [RegisterIs R0 0, WordIs "definitions" (exists 19) 0, ByteIs "tasks" (tfield 19 (taskPriority t)) 15]
      [RegisterIs R0 statusInvalid, RegionsUnchanged dataRegions]
      []
  , Case
      "delete-reset-slot-0"
      DeleteReset
      (baseRow 0 DeleteReset) {rowExists = 1}
      (Repair "slot 0 is refused instead of resetting C[-1]")
      []
      [ RegisterIs R0 0
      , WordIs "definitions" (cfield 0 (definitionPriority d)) 15
      , WordIs "definitions" (exists 0) 0
      ]
      [RegisterIs R0 statusInvalid, RegionsUnchanged dataRegions]
      []
  , Case
      "delete-reset-slot-121"
      DeleteReset
      (baseRow 121 DeleteReset) {rowExists = 1}
      (Repair "slot 121 is refused instead of resetting C[120]")
      []
      [RegisterIs R0 0, WordIs "definitions" (cfield 121 (definitionPriority d)) 15, WordIs "definitions" (exists 121) 0]
      [RegisterIs R0 statusInvalid, RegionsUnchanged dataRegions]
      []
  , Case "smoke-create" Create (baseRow 1 Create) Equal [RegisterIs R0 0] [] [] [dmb 1]
  , Case "smoke-delete" DeleteReset (baseRow 1 DeleteReset) Equal [RegisterIs R0 0] [] [] [dmb 1]
  ]
  where
    d = factsDefinition facts
    t = factsTask facts
    cfield slot offset = definitionField facts domain slot offset
    tfield slot offset = taskField facts domain slot offset
    exists slot = existsField facts domain slot
    dmb slot = DmbBeforeWrite (exists slot)

caseFor :: Facts -> Domain -> Role -> Image -> Image -> [(String, Word32, Word32)] -> Case -> CaseSpec
caseFor facts domain role stockImage ownedImage symbols one =
  CaseSpec
    { caseId = caseName one ++ "-" ++ domainLabel domain
    , caseProvenance = "X-M1 1.50 task-record stock ABI, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; synthetic C/T/map state"
    , caseExclusion = "Synthetic task tables and request record; no scheduler, object queue or stack-check consumer."
    , caseEntry = GuestAddress (fromIntegral entry)
    , caseExit = GuestAddress (fromIntegral exitAddress)
    , caseRegions =
        [ mkRegion (RegionId "code") (GuestAddress (fromIntegral codeBase)) (B.length codeBytes) ReadExecute codeBytes
        ] ++ maskRegions ++
        [ mkRegion (RegionId "definitions") (GuestAddress (fromIntegral definitionsBase)) (fromIntegral definitionsSize) ReadWrite (overlay definitionsBase definitionsSize (initialWords facts domain (caseRow one)))
        , mkRegion (RegionId "tasks") (GuestAddress (fromIntegral tasksBase)) (fromIntegral tasksSize) ReadWrite (overlay tasksBase tasksSize (initialWords facts domain (caseRow one)))
        , mkRegion (RegionId "map") (GuestAddress (fromIntegral mapBase)) (fromIntegral mapSize) ReadWrite (overlay mapBase mapSize (initialWords facts domain (caseRow one)))
        , mkRegion (RegionId "request") (GuestAddress (fromIntegral requestBase)) 0x18 ReadWrite (overlay requestBase 0x18 (initialWords facts domain (caseRow one)))
        , mkRegion (RegionId "stack") (GuestAddress (fromIntegral stackBase)) (fromIntegral stackSize) ReadWrite (B.replicate (fromIntegral stackSize) 0)
        , mkRegion (RegionId "exit") (GuestAddress (fromIntegral exitAddress)) 0x10 ReadExecute (B.replicate 0x10 0)
        ]
    , caseFunctions = functions
    , caseInitial =
        [ (R0, (rowClass row `shiftL` 16) .|. rowSlot row)
        , (R1, if caseOp one == Create then requestBase else rowMode row)
        , (R2, seedRegister R2)
        , (R3, seedRegister R3)
        ]
          ++ [(register, seedRegister register) | register <- [R4 .. R12]]
          ++ [(Sp, stackPointer), (Lr, exitAddress), (Cpsr, rowCpsr row)]
    , caseExpectedStatus = Returned
    , caseExpectedRegisters =
        (R0, expectedResult facts role (caseOp one) row)
          : [(register, seedRegister register) | register <- [R4 .. R11]]
          ++ [(Sp, stackPointer)]
    , caseExpectedMemory = []
    , caseIgnoredMemory = []
    , caseCp14Identity = Nothing
    , caseKernel = Just (KernelCase plainLimit Nothing (Just 0x8000_1234) (Just 0) (Just 0) [R1, R2, R3, R12, Lr, Cpsr] [])
    }
  where
    row = caseRow one
    d = factsDefinition facts
    t = factsTask facts
    domainWords = domainFacts facts domain
    stockEntry = case caseOp one of
      Create -> createAddress domain
      _ -> deleteAddress domain
    stockExtent = case caseOp one of
      Create -> createExtent domain
      _ -> deleteExtent domain
    ownedEntry = case [address | (name, address, _) <- symbols, name == ownedName] of
      (address : _) -> address
      [] -> error ("owned symbol missing: " ++ ownedName)
    ownedName = if caseOp one == Create then "xm1_rtos_task_create" else "xm1_rtos_task_delete"
    entry = case role of
      Stock -> stockEntry
      Owned -> if aggregate then stockEntry else ownedEntry
    (codeBase, codeBytes) = case role of
      Stock -> (stockEntry, slice (imageBase stockImage) (imageBytes stockImage) stockEntry stockExtent)
      Owned | aggregate -> (imageBase stockImage, aggregateBytes)
      Owned -> (imageBase ownedImage, imageBytes ownedImage)
    maskSave = dfMaskSave domainWords
    maskBytes = slice (imageBase stockImage) (imageBytes stockImage) maskSave 0x68
    aggregate = any (\(name, _, _) -> "xm1_platform_veneer_" `isPrefixOf` name) symbols
    aggregateBytes =
      let image = Agg.AggregateImage (imageBase ownedImage) (imageBytes ownedImage)
          carrier = Agg.AggregateSymbol "effective-carrier" (imageBase ownedImage) (fromIntegral (B.length (imageBytes ownedImage)))
       in either error id (Agg.installSymbol (imageBase stockImage) (imageBytes stockImage) image carrier)
    maskRegions = [mkRegion (RegionId "mask") (GuestAddress (fromIntegral maskSave)) 0x68 ReadExecute maskBytes | role == Stock || not aggregate]
    definitionsBase = definitionBase facts domain
    definitionsSize = definitionRegionSize facts
    tasksBase = taskBase facts domain
    tasksSize = taskRegionSize facts
    mapBase = creationIdBase facts domain
    mapSize = creationIdRegionSize facts
    functions = case role of
      Stock ->
        [ (GuestAddress (fromIntegral stockEntry), stockExtent)
        , (GuestAddress (fromIntegral maskSave), 0x68)
        ]
      Owned | aggregate ->
        [(GuestAddress (fromIntegral address), size) | (address, size) <- admitted]
      Owned ->
        [(GuestAddress (fromIntegral address), size) | (_, address, size) <- symbols]
          ++ [(GuestAddress (fromIntegral maskSave), 0x68)]
    executable =
      [(address, size) | (_, address, size) <- symbols, size > 0]
        ++ [(Agg.placementAddress placement, Agg.placementBytes placement)
           | placement <- imagePlacements ownedImage
           , Agg.placementBytes placement > 0
           , ".text." `isPrefixOf` Agg.placementSection placement
           ]
    allocated =
      [(address, size) | (_, address, size) <- symbols, size > 0]
        ++ [(Agg.placementAddress placement, Agg.placementBytes placement)
           | placement <- imagePlacements ownedImage
           , Agg.placementBytes placement > 0
           ]
    overlaps (address, size) (other, bytes) =
      toInteger address < toInteger other + toInteger bytes
        && toInteger other < toInteger address + toInteger size
    preserved = [extent | extent <- [(maskSave, 0x68)], not (any (overlaps extent) allocated)]
    admitted = concatMap splitAtEntries (merge (sortOn fst (executable ++ preserved)))
    merge [] = []
    merge ((address, size) : rest) = extend address (address + size) rest
    extend address top [] = [(address, top - address)]
    extend address top remaining@((next, size) : rest)
      | next < top = extend address (max top (next + size)) rest
      | otherwise = (address, top - address) : merge remaining
    publicEntries = sort (nub [address | (name, address, size) <- symbols, size > 0, "xm1_platform_veneer_" `isPrefixOf` name])
    splitAtEntries (address, size) =
      let boundaries = address : [entryAddress | entryAddress <- publicEntries, address < entryAddress, entryAddress < address + size] ++ [address + size]
       in [(low, high - low) | (low, high) <- zip boundaries (drop 1 boundaries)]
    _unused = (d, t)

parseOwnedSymbols :: FilePath -> IO [(String, Word32, Word32)]
parseOwnedSymbols path = do
  rows <- lines <$> readFile path
  pure
    [ (name, address, size)
    | line <- rows
    , [addressText, sizeText, kind, name] <- [words line]
    , kind `elem` ["T", "t"]
    , Just address <- [parseWord32 addressText]
    , Just size <- [parseWord32 sizeText]
    , size > 0
    ]

runDomain
  :: FilePath
  -> FilePath
  -> FilePath
  -> Facts
  -> Domain
  -> Image
  -> Image
  -> [(String, Word32, Word32)]
  -> IO ([CaseRun], [ControlRun])
runDomain runner cacheRoot directory facts domain stockImage ownedImage symbols = do
  (coreRuns, coreControls) <- runCohort "core" (filter ((== "core") . cohortOf) selected) directory
  (smokeRuns, smokeControls) <- runCohort "smoke" (filter ((== "smoke") . cohortOf) selected) (directory </> "smoke")
  pure (coreRuns ++ smokeRuns, coreControls ++ smokeControls)
  where
    selected = cases facts domain
    runCohort cohort chosen base = do
      let stockSpecs = map (caseFor facts domain Stock stockImage ownedImage symbols) chosen
          ownedSpecs = map (caseFor facts domain Owned stockImage ownedImage symbols) chosen
      stockBatch <- writeBatch (base </> "stock") stockSpecs
      ownedBatch <- writeBatch (base </> "owned") ownedSpecs
      (stockReuse, stockRaw) <- cachedObservedBatch cacheRoot runner stockBatch
      (ownedReuse, ownedRaw) <- cachedObservedBatch cacheRoot runner ownedBatch
      let reference = M.fromList [(caseIdText ident, observation) | (ident, observation) <- stockRaw]
          subject = M.fromList [(caseIdText ident, observation) | (ident, observation) <- ownedRaw]
      results <- forM chosen $ \one -> do
        let ident = caseName one ++ "-" ++ domainLabel domain
            stock = M.lookup ident reference
            owned = M.lookup ident subject
            checks role observation = map (check facts domain one observation) (checksFor facts domain role one)
            statusCheck observation = case nativeStatus observation of
              Returned -> Right ()
              status -> Left ("status " ++ runnerStatusLabel status)
            errors role observation = statusCheck observation : checks role observation
            equalDifferences stock' owned' = case caseKind one of
              Equal -> regionDifferences dataRegions stock' owned'
              Repair _ -> []
            run =
              CaseRun
                { caseDomain = domain
                , runCohort = cohort
                , runName = caseName one
                , runKind = caseKind one
                , runChecks = case (stock, owned) of
                    (Just stock', Just owned') ->
                      [ ("stock: " ++ describe check', either (const False) (const True) (check facts domain one stock' check'))
                      | check' <- checksFor facts domain Stock one
                      ]
                        ++ [ ("owned: " ++ describe check', either (const False) (const True) (check facts domain one owned' check'))
                           | check' <- checksFor facts domain Owned one
                           ]
                    _ -> []
                , runDifferences = case (stock, owned) of
                    (Just stock', Just owned') ->
                      ["stock: " ++ message | message <- lefts (errors Stock stock')]
                        ++ ["owned: " ++ message | message <- lefts (errors Owned owned')]
                        ++ equalDifferences stock' owned'
                    (Nothing, _) -> ["stock case did not run"]
                    (_, Nothing) -> ["owned case did not run"]
                , runReuse = (stockReuse, ownedReuse)
                }
            controls = case (stock, owned) of
              (Just stock', Just owned') -> controlRuns facts domain cohort one stock' owned'
              _ -> []
        pure (run, controls)
      pure (map fst results, concatMap snd results)

lefts :: [Either a b] -> [a]
lefts values = [value | Left value <- values]

controlRuns :: Facts -> Domain -> String -> Case -> NativeObservation -> NativeObservation -> [ControlRun]
controlRuns facts domain cohort one stock owned =
  [ ControlRun cohort "cpsr-i" (caseName one) (refused (CpsrControlIs expected) (mutateCpsr (`xor` 0x80) owned))
  , ControlRun cohort "cpsr-f" (caseName one) (refused (CpsrControlIs expected) (mutateCpsr (`xor` 0x40) owned))
  , ControlRun cohort "caller-sp" (caseName one) (refused (RegisterIs Sp stackPointer) (mutateRegister Sp (+ 1) owned))
  , ControlRun cohort "caller-r4" (caseName one) (refused (RegisterIs R4 (seedRegister R4)) (mutateRegister R4 (+ 1) owned))
  , ControlRun cohort "status" (caseName one) (refused (RegisterIs R0 (expectedResult facts Owned (caseOp one) (caseRow one))) (mutateRegister R0 (+ 1) owned))
  ]
    ++ [ ControlRun cohort ("unchanged-" ++ region) (caseName one) (refused (RegionsUnchanged [region]) (mutateRegion region owned))
       | region <- declaredRegions
       ]
    ++ [ ControlRun cohort "equal-definitions" (caseName one) (not (null (regionDifferences ["definitions"] stock (mutateRegion "definitions" owned))))
       | caseKind one == Equal
       ]
    ++ [ ControlRun cohort "dmb" (caseName one) (refused (DmbBeforeWrite address) (mutateBarriers owned))
       | DmbBeforeWrite address <- caseOwnedOnlyChecks one
       ]
  where
    expected = cpsrControl (rowCpsr (caseRow one))
    refused check' mutated = case check facts domain one mutated check' of
      Left _ -> True
      Right () -> False
    declaredRegions = [region | RegionsUnchanged regions <- checksFor facts domain Owned one, region <- regions]
    mutateCpsr change observation = observation {nativeRegisters = M.adjust change Cpsr (nativeRegisters observation)}
    mutateRegister register change observation = observation {nativeRegisters = M.adjust change register (nativeRegisters observation)}
    mutateRegion region observation =
      observation
        { nativeMemory =
            [ (ident, if regionName ident == region then flipHead bytes else bytes)
            | (ident, bytes) <- nativeMemory observation
            ]
        }
    flipHead bytes = case B.uncons bytes of
      Just (first, rest) -> B.cons (first `xor` (0xff :: Word8)) rest
      Nothing -> bytes
    mutateBarriers observation =
      observation
        { nativeKernel =
            fmap
              (\kernel -> kernel {observedOrder = fmap (map replace) (observedOrder kernel)})
              (nativeKernel observation)
        }
    replace (OrderedBarrier Dmb) = OrderedBarrier Dsb
    replace other = other
