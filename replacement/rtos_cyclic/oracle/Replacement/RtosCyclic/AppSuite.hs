{-# LANGUAGE OverloadedStrings #-}

module Replacement.RtosCyclic.AppSuite
  ( CaseRun (..)
  , readOwnedFunctions
  , run
  ) where

import Control.Monad (forM)
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import Data.Int (Int32)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.CachedNativeBatch (cachedObservedBatch)
import Emulation.NativeObservation
import Emulation.OwnedCase
import Numeric (readHex)
import System.FilePath ((</>))

data Role = Stock | Owned deriving (Eq, Show)

data CaseRun = CaseRun
  { runName :: String
  , runChecks :: [(String, Bool)]
  , runDifferences :: [String]
  }

data Case = Case
  { cName :: String
  , cEntry :: Word32
  , cArg0 :: Word32
  , cArg1 :: Word32
  , cArg2 :: Word32
  , cArg3 :: Word32
  , cNextToken :: Word32
  , cUsedTokens :: [(Int, Word32)]
  , cBusySlots :: [Int]
  , cCreateStatus :: Word32
  , cStartStatus :: Word32
  , cExpectedStatus :: Word32
  }

stockBase, codeBase, codeSize, registerEntry, unregisterEntry :: Word32
stockBase = 0x61c400
codeBase = 0x725424
codeSize = 0x384
registerEntry = 0x725424
unregisterEntry = 0x7256c4

stubsBase, supportRecordBase, supportDispatchBase, objectBase :: Word32
stubsBase = 0x01001000
supportRecordBase = 0x00041a34
supportDispatchBase = 0x00723398
objectBase = 0x01002000

interfacesBase, registryBase, receiptsBase, lastErrorBase :: Word32
interfacesBase = 0x0096bd20
registryBase = 0x409c3500
receiptsBase = 0x01004000
lastErrorBase = 0x40a0f82c

stackBase, stackPointer, exitAddress, stubSize, supportSize :: Word32
stackBase = 0x00300000
stackPointer = stackBase + 0x800
exitAddress = 0x01000200
stubSize = 0x200
supportSize = 0x40

callbackAddress :: Word32
callbackAddress = 0x0069cc18

stubCurrent, stubCreate, stubStart, stubStop, stubDestroy, stubEnter, stubLeave :: Word32
stubCurrent = stubsBase + 0x00
stubCreate = stubsBase + 0x08
stubStart = stubsBase + 0x50
stubStop = stubsBase + 0x70
stubDestroy = stubsBase + 0x88
stubEnter = stubsBase + 0xa0
stubLeave = stubsBase + 0xb0

cases :: [Case]
cases =
  [ Case "register-success" registerEntry 30 0 callbackAddress (receiptsBase + 0x80) 30001 [] [] 0 0 0
  , Case "register-slot-63" registerEntry 30 0 callbackAddress (receiptsBase + 0x80) 30001 [] [64] 0 0 0
  , Case "register-slot-62" registerEntry 30 0 callbackAddress (receiptsBase + 0x80) 30001 [] [64, 63] 0 0 0
  , Case "register-no-slot" registerEntry 30 0 callbackAddress (receiptsBase + 0x80) 30001 [] [1 .. 64] 0 0 (negative 34)
  , Case "register-invalid-mode" registerEntry 30 1 callbackAddress (receiptsBase + 0x80) 30001 [] [] 0 0 (negative 30)
  , Case "register-null-callback" registerEntry 30 0 0 (receiptsBase + 0x80) 30001 [] [] 0 0 (negative 30)
  , Case "register-create-failure" registerEntry 30 0 callbackAddress (receiptsBase + 0x80) 30001 [] [] (negative 41) 0 (negative 41)
  , Case "register-start-failure" registerEntry 30 0 callbackAddress (receiptsBase + 0x80) 30001 [] [] 0 (negative 5) (negative 5)
  , Case "register-token-used" registerEntry 30 0 callbackAddress (receiptsBase + 0x80) 30001 [(1, 30001), (2, 30002)] [] 0 0 0
  , Case "register-token-wrap" registerEntry 30 0 callbackAddress (receiptsBase + 0x80) 39999 [(1, 39999)] [] 0 0 0
  , Case "unregister-slot-64" unregisterEntry 30001 0 0 0 30002 [(64, 30001)] [64] 0 0 0
  , Case "unregister-slot-1" unregisterEntry 30001 0 0 0 30002 [(1, 30001)] [1] 0 0 0
  , Case "unregister-missing" unregisterEntry 12345 0 0 0 30002 [] [] 0 0 0
  ]

negative :: Int -> Word32
negative value = fromIntegral (negate (fromIntegral value :: Int32))

seedRegister :: Register -> Word32
seedRegister register = 0xa5a50000 + fromIntegral (fromEnum register)

movwI, movtI, movImm, bxLr, filler :: Word32 -> Word32 -> Word32
ldrIoff, strIoff :: Word32 -> Word32 -> Word32 -> Word32
movwI rd imm = 0xe3000000 .|. (((imm `shiftR` 12) .&. 0xf) `shiftL` 16) .|. (rd `shiftL` 12) .|. (imm .&. 0xfff)
movtI rd imm = 0xe3400000 .|. (((imm `shiftR` 12) .&. 0xf) `shiftL` 16) .|. (rd `shiftL` 12) .|. (imm .&. 0xfff)
ldrIoff rd rn imm = 0xe5900000 .|. (rn `shiftL` 16) .|. (rd `shiftL` 12) .|. (imm .&. 0xfff)
strIoff rd rn imm = 0xe5800000 .|. (rn `shiftL` 16) .|. (rd `shiftL` 12) .|. (imm .&. 0xfff)
movImm rd imm = 0xe3a00000 .|. (rd `shiftL` 12) .|. (imm .&. 0xff)
bxLr _ _ = 0xe12fff1e
filler _ _ = 0xe7f000f0

movwPair :: Word32 -> Word32 -> Word32 -> [Word32]
movwPair rd value _ = [movwI rd (value .&. 0xffff), movtI rd (value `shiftR` 16)]

fillStubs :: Word32 -> [(Word32, [Word32])] -> B.ByteString
fillStubs total segments = B.concat (map word32le (go 0))
  where
    go offset = case lookup offset segments of
      Just words' -> words' ++ go (offset + 4 * fromIntegral (length words'))
      Nothing
        | offset < total -> filler 0 0 : go (offset + 4)
        | otherwise -> []

stubRegion :: B.ByteString
stubRegion = fillStubs stubSize
  [ (0x00, [movImm 0 0, bxLr 0 0])
  , (0x08, createBody)
  , (0x50, startBody)
  , (0x70, stopBody)
  , (0x88, destroyBody)
  , (0xa0, movwPair 0 0x5a5a 0 ++ [bxLr 0 0])
  , (0xb0, leaveBody)
  ]
  where
    createBody =
      movwPair 2 receiptsBase 0
        ++ [strIoff 0 2 0x18]
        ++ concat
          [ [ldrIoff 3 1 offset, strIoff 3 2 offset]
          | offset <- [0x00, 0x04, 0x08, 0x0c, 0x10]
          ]
        ++ movwPair 3 receiptsBase 0
        ++ [ldrIoff 0 3 0x48, bxLr 0 0]
    startBody =
      movwPair 2 receiptsBase 0
        ++ [strIoff 0 2 0x1c]
        ++ movwPair 3 receiptsBase 0
        ++ [ldrIoff 0 3 0x4c, bxLr 0 0]
    stopBody = movwPair 2 receiptsBase 0 ++ [strIoff 0 2 0x20, movImm 0 0, bxLr 0 0]
    destroyBody = movwPair 2 receiptsBase 0 ++ [strIoff 0 2 0x24, movImm 0 0, bxLr 0 0]
    leaveBody = movwPair 2 receiptsBase 0 ++ [strIoff 0 2 0x30, movImm 0 0, bxLr 0 0]

supportRecordStub :: B.ByteString
supportRecordStub = fillStubs supportSize
  [ (0x00, movwPair 3 receiptsBase 0 ++ [strIoff 0 3 0x38, strIoff 1 3 0x3c, strIoff 2 3 0x40, bxLr 0 0])
  ]

supportDispatchStub :: B.ByteString
supportDispatchStub = fillStubs supportSize
  [ (0x00, movwPair 2 receiptsBase 0 ++ [strIoff 0 2 0x44, bxLr 0 0])
  ]

poke :: B.ByteString -> Word32 -> Word32 -> B.ByteString
poke bytes offset value =
  let (before, after) = B.splitAt (fromIntegral offset) bytes
   in before <> word32le value <> B.drop 4 after

pokes :: B.ByteString -> [(Word32, Word32)] -> B.ByteString
pokes = foldl (\bytes (offset, value) -> poke bytes offset value)

objectBytes :: B.ByteString
objectBytes =
  pokes (B.replicate 0x1700 0)
    [ (0x108, stubCreate)
    , (0x10c, stubDestroy)
    , (0x110, stubStart)
    , (0x114, stubStop)
    , (0x164, stubEnter)
    , (0x168, stubLeave)
    , (0x1618, stubCurrent)
    ]

interfacesBytes :: B.ByteString
interfacesBytes = word32le objectBase <> word32le objectBase

registryBytes :: Case -> B.ByteString
registryBytes one =
  pokes (B.replicate 0x404 0)
    ( [(0, cNextToken one)]
        ++ [(0x104 + fromIntegral (slot - 1) * 4, token) | (slot, token) <- cUsedTokens one]
        ++ [(0x304 + fromIntegral (slot - 1) * 4, 1) | slot <- cBusySlots one]
    )

receiptsBytes :: Case -> B.ByteString
receiptsBytes one =
  pokes (B.replicate 0x100 0)
    [ (0x48, cCreateStatus one)
    , (0x4c, cStartStatus one)
    ]

slice :: B.ByteString -> Word32 -> Word32 -> B.ByteString
slice bytes address size =
  B.take (fromIntegral size) (B.drop (fromIntegral (address - stockBase)) bytes)

stockFunctions :: [(Word32, Word32)]
stockFunctions = [(registerEntry, 0x2a0), (unregisterEntry, 0xe4)]

caseFor :: Role -> B.ByteString -> B.ByteString -> [(Word32, Word32)] -> Case -> CaseSpec
caseFor role stockImage ownedImage ownedFunctions one =
  CaseSpec
    { caseId = cName one
    , caseProvenance = "X-M1 1.50 app callback stock slice at 0x725424 and the owned app-callback artifact; x70view 55436fc85c0560dd252b0410af6f2427fe198cd3 startup_requests.c:218-337"
    , caseExclusion = "Synthetic interface stubs, callback registry and error recorder; no app dispatcher, cyclic service or system integration."
    , caseEntry = GuestAddress (fromIntegral (cEntry one))
    , caseExit = GuestAddress (fromIntegral exitAddress)
    , caseRegions =
        [ mkRegion (RegionId "code") (GuestAddress (fromIntegral codeBase)) (fromIntegral (B.length codeBytes)) ReadExecute codeBytes
        , mkRegion (RegionId "stubs") (GuestAddress (fromIntegral stubsBase)) (fromIntegral stubSize) ReadExecute stubRegion
        , mkRegion (RegionId "support-record") (GuestAddress (fromIntegral supportRecordBase)) (fromIntegral supportSize) ReadExecute supportRecordStub
        , mkRegion (RegionId "support-dispatch") (GuestAddress (fromIntegral supportDispatchBase)) (fromIntegral supportSize) ReadExecute supportDispatchStub
        , mkRegion (RegionId "interfaces") (GuestAddress (fromIntegral interfacesBase)) 8 ReadOnly interfacesBytes
        , mkRegion (RegionId "object") (GuestAddress (fromIntegral objectBase)) 0x1700 ReadOnly objectBytes
        , mkRegion (RegionId "registry") (GuestAddress (fromIntegral registryBase)) 0x404 ReadWrite (registryBytes one)
        , mkRegion (RegionId "receipts") (GuestAddress (fromIntegral receiptsBase)) 0x100 ReadWrite (receiptsBytes one)
        , mkRegion (RegionId "last-error") (GuestAddress (fromIntegral lastErrorBase)) 4 ReadWrite (B.replicate 4 0)
        , mkRegion (RegionId "stack") (GuestAddress (fromIntegral stackBase)) 0x1000 ReadWrite (B.replicate 0x1000 0)
        , mkRegion (RegionId "exit") (GuestAddress (fromIntegral exitAddress)) 0x10 ReadExecute (B.replicate 0x10 0)
        ]
    , caseFunctions =
        [ (GuestAddress (fromIntegral address), size)
        | (address, size) <- functions
        ]
    , caseInitial =
        [ (R0, cArg0 one)
        , (R1, cArg1 one)
        , (R2, cArg2 one)
        , (R3, cArg3 one)
        ]
          ++ [(register, seedRegister register) | register <- [R4 .. R11]]
          ++ [ (R12, seedRegister R12)
             , (Sp, stackPointer)
             , (Lr, exitAddress)
             , (Cpsr, 0x10)
             ]
    , caseExpectedStatus = Returned
    , caseExpectedRegisters =
        (R0, cExpectedStatus one)
          : [(register, seedRegister register) | register <- [R4 .. R11]]
          ++ [(Sp, stackPointer)]
    , caseExpectedMemory = []
    , caseCp14Identity = Nothing
    , caseKernel = Nothing
    }
  where
    codeBytes = case role of
      Stock -> slice stockImage codeBase codeSize
      Owned -> ownedImage
    functions = case role of
      Stock -> stockFunctions ++ stubs
      Owned -> ownedFunctions ++ stubs
    stubs =
      [ (stubsBase, stubSize)
      , (supportRecordBase, supportSize)
      , (supportDispatchBase, supportSize)
      ]

regions :: [String]
regions = ["registry", "receipts", "last-error", "interfaces", "object"]

run :: FilePath -> FilePath -> FilePath -> B.ByteString -> B.ByteString -> [(Word32, Word32)] -> IO [CaseRun]
run runner cacheRoot directory stockImage ownedImage ownedFunctions = do
  let stockSpecs = map (caseFor Stock stockImage ownedImage ownedFunctions) cases
      ownedSpecs = map (caseFor Owned stockImage ownedImage ownedFunctions) cases
  stockBatch <- writeBatch (directory </> "stock") stockSpecs
  ownedBatch <- writeBatch (directory </> "owned") ownedSpecs
  (_, stockRaw) <- cachedObservedBatch cacheRoot runner stockBatch
  (_, ownedRaw) <- cachedObservedBatch cacheRoot runner ownedBatch
  let reference = M.fromList [(caseIdText ident, observation) | (ident, observation) <- stockRaw]
      subject = M.fromList [(caseIdText ident, observation) | (ident, observation) <- ownedRaw]
  forM cases $ \one -> do
    let ident = cName one
        stock = M.lookup ident reference
        owned = M.lookup ident subject
        expected = cExpectedStatus one
        statusCheck label observation =
          ( label ++ " status " ++ runnerStatusLabel (nativeStatus observation)
          , nativeStatus observation == Returned && registerValue observation R0 == expected
          )
        checks = case (stock, owned) of
          (Just stock', Just owned') ->
            [statusCheck "stock" stock', statusCheck "owned" owned']
              ++ [ ("region " ++ name ++ " matches", regionBytesOf stock' (RegionId name) == regionBytesOf owned' (RegionId name))
                 | name <- regions
                 ]
              ++ [ ("register " ++ registerLabel register ++ " matches", registerValue stock' register == registerValue owned' register)
                 | register <- R0 : [R4 .. R11] ++ [Sp]
                 ]
          _ -> []
        differences = case (stock, owned) of
          (Just _, Just _) -> [description | (description, False) <- checks]
          (Nothing, _) -> ["stock case did not run"]
          (_, Nothing) -> ["owned case did not run"]
    pure (CaseRun ident checks differences)

readOwnedFunctions :: FilePath -> IO [(Word32, Word32)]
readOwnedFunctions path = do
  rows <- lines <$> readFile path
  pure (concatMap row rows)
  where
    row line = case words line of
      [addressText, sizeText, kind, _]
        | kind `elem` ["T", "t"]
        , Just address <- parseHex addressText
        , Just size <- parseHex sizeText
        , size > 0
        , address >= codeBase
        , address + size <= codeBase + codeSize -> [(address, size)]
      _ -> []

parseHex :: String -> Maybe Word32
parseHex text = case readHex text of
  [(value, "")] -> Just value
  _ -> Nothing
