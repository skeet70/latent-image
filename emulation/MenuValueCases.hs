{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Bounded stock/owned pairs for the commit dispatcher's IMAGE SIZE arm. The
-- owned role patches the dispatcher's jump-table word to the compiled adapter
-- image; the stock role leaves it. Both run the same stock dispatcher, code,
-- cells and settings, so the observations compare directly.
module Main where

import Control.Monad (forM_, unless, when)
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.NativeCase (loadCase, runCase)
import Emulation.NativeObservation
  ( GuestAddress (..)
  , NativeObservation (..)
  , Register (..)
  , RegionId (..)
  , RunnerStatus (..)
  , observeNative
  , reportRunnerExit
  , renderObservationError
  , runnerStatusLabel
  )
import Emulation.OwnedCase
import FujiAF.Host (get)
import System.Environment (getArgs)
import System.FilePath ((</>))

dispatcherBase, codeBase, convTableBase, convPointerBase, pairBase :: Word32
dispatcherBase = 0x00d4d604
codeBase = 0x0061c91c
convTableBase = 0x00774f48
convPointerBase = 0x0081b09c
pairBase = 0x007a00cc

cellsBase, settingsBase, ownedBase :: Word32
cellsBase = 0x00956838
settingsBase = 0x0096cc64
ownedBase = 0x00a00000

data Role = Stock | Owned
  deriving (Eq, Show)

data Scenario = Scenario
  { scenarioToken :: Word32
  , scenarioMode :: Word32
  , scenarioStored :: (Word32, Word32, Word32)
  , scenarioCaller :: Word32
  , scenarioRole :: Role
  , scenarioPoison :: Bool
  }
  deriving (Eq, Show)

roleText :: Role -> String
roleText Stock = "stock"
roleText Owned = "owned"

scenarios :: [Scenario]
scenarios =
  [ Scenario token 0x2c (0, 0, 1) 0x00c2d924 role (role == Owned)
  | token <- [0 .. 11], role <- [Stock, Owned]
  ]
    ++ [ Scenario 2 mode (0, 0, 1) 0x00c2d924 role (role == Owned)
       | mode <- [0x1f, 0x20, 0x21, 0x24, 0x25], role <- [Stock, Owned]
       ]
    ++ [ Scenario 1 0x2c (0, 1, 0) 0x00c2d924 role (role == Owned) | role <- [Stock, Owned] ]
    ++ [ Scenario 1 0x2c (0, 1, 1) 0x00c2d924 role (role == Owned) | role <- [Stock, Owned] ]
    ++ [ Scenario 2 0x2c (0, 0, 1) 0x0061c408 role (role == Owned) | role <- [Stock, Owned] ]
    ++ [ Scenario 2 0x2c (0, 0, 1) 0x00c321e0 role (role == Owned) | role <- [Stock, Owned] ]
    ++ [ Scenario 12 0x2c (0, 0, 1) 0x00c2d924 role False | role <- [Stock, Owned] ]

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [runner, image4Path, image5Path, ownedPath, out] -> do
      image4 <- B.readFile image4Path
      image5 <- B.readFile image5Path
      owned <- B.readFile ownedPath
      unless (B.length owned >= 0x40 && B.length owned <= 0x81000) (fail "owned image size differs")
      runPairs runner image4 image5 owned out
    [runner, image4Path, image5Path, ownedPath, out, symbolsPath] -> do
      image4 <- B.readFile image4Path
      image5 <- B.readFile image5Path
      owned <- B.readFile ownedPath
      unless (B.length owned > 0x80000 && B.length owned <= 0x81000) (fail "owned session image size differs")
      symbols <- parseSymbols lifecycleSymbols symbolsPath
      runPairs runner image4 image5 owned out
      runSessions runner image4 image5 owned out symbols
    _ -> fail "usage: menu-value-cases RUNNER IMAGE4 IMAGE5 OWNED_IMAGE OUT [OWNED_SYMBOLS]"

runPairs :: FilePath -> B.ByteString -> B.ByteString -> B.ByteString -> FilePath -> IO ()
runPairs runner image4 image5 owned out = do
  forM_ (groups scenarios) $ \(key, pair) -> do
    (stock, ownedObservation) <- runPair runner image4 image5 owned out pair
    compareObservations key stock ownedObservation
  putStrLn ("menu-value-pairs: " ++ show (length (groups scenarios)) ++ " stock/owned pairs")

groups :: [Scenario] -> [((Word32, Word32, (Word32, Word32, Word32), Word32), (Scenario, Scenario))]
groups entries =
  [ ((scenarioToken stock, scenarioMode stock, scenarioStored stock, scenarioCaller stock), (stock, owned))
  | stock <- entries
  , scenarioRole stock == Stock
  , Just owned <- [findOwned stock]
  ]
 where
  findOwned stock = case [candidate | candidate <- entries, scenarioRole candidate == Owned, sameKey candidate stock] of
    candidate : _ -> Just candidate
    [] -> Nothing
  sameKey candidate other =
    scenarioToken candidate == scenarioToken other
      && scenarioMode candidate == scenarioMode other
      && scenarioStored candidate == scenarioStored other
      && scenarioCaller candidate == scenarioCaller other

runPair
  :: FilePath
  -> B.ByteString
  -> B.ByteString
  -> B.ByteString
  -> FilePath
  -> (Scenario, Scenario)
  -> IO (NativeObservation, NativeObservation)
runPair runner image4 image5 owned out (stock, ownedRole) = do
  stockObservation <- runScenario runner image4 image5 owned out stock
  ownedObservation <- runScenario runner image4 image5 owned out ownedRole
  pure (stockObservation, ownedObservation)

runScenario
  :: FilePath
  -> B.ByteString
  -> B.ByteString
  -> B.ByteString
  -> FilePath
  -> Scenario
  -> IO NativeObservation
runScenario runner image4 image5 owned out scenario = do
  let ident = "menu-value-" ++ show (scenarioToken scenario) ++ "-" ++ show (scenarioMode scenario)
        ++ "-" ++ roleText (scenarioRole scenario) ++ "-" ++ show (scenarioCaller scenario)
      spec = specFor image4 image5 owned ident scenario
  path <- writeCase (out </> ident) spec
  (_, regions, _, _) <- loadCase path
  (_, report) <- runCase runner path
  exit <- either (error . renderObservationError) pure (reportRunnerExit report)
  either (error . renderObservationError) pure (observeNative exit regions (get report "runner"))

compareObservations
  :: (Word32, Word32, (Word32, Word32, Word32), Word32)
  -> NativeObservation
  -> NativeObservation
  -> IO ()
compareObservations key stock owned = do
  let label = show key
  unless (nativeStatus stock == Returned && nativeStatus owned == Returned) (fail (label ++ ": status"))
  unless (regionBytesOf stock (RegionId "cells") == regionBytesOf owned (RegionId "cells")) (fail (label ++ ": cells"))
  unless (regionBytesOf stock (RegionId "settings") == regionBytesOf owned (RegionId "settings")) (fail (label ++ ": settings"))
  unless (registerValue stock R0 == registerValue owned R0) (fail (label ++ ": r0"))
  unless (registerValue stock Sp == registerValue owned Sp) (fail (label ++ ": sp"))
  let (_, mode, _, _) = key
      cells = regionBytesOf stock (RegionId "cells")
      args1 = (wordAtBytes cells 0x458, wordAtBytes cells 0x45c, wordAtBytes cells 0x460)
      args2 = (wordAtBytes cells 0x468, wordAtBytes cells 0x46c)
      expected1 = if mode == 0x2c then (0, 0, 0) else (0x34, 0, 0xf6)
      expected2 = if mode == 0x2c then (0, 0) else (1, 0)
  unless (args1 == expected1) (fail (label ++ ": 0x63c51c args"))
  unless (args2 == expected2) (fail (label ++ ": 0x641438 args"))

wordAtBytes :: B.ByteString -> Int -> Word32
wordAtBytes bytes offset = fromIntegral (B.index bytes offset)
  .|. (fromIntegral (B.index bytes (offset + 1)) `shiftL` 8)
  .|. (fromIntegral (B.index bytes (offset + 2)) `shiftL` 16)
  .|. (fromIntegral (B.index bytes (offset + 3)) `shiftL` 24)

specFor :: B.ByteString -> B.ByteString -> B.ByteString -> String -> Scenario -> CaseSpec
specFor image4 image5 owned ident scenario = CaseSpec
  { caseId = ident
  , caseProvenance =
      "X-M1 1.50 image-5 dispatcher bytes [0xd4d604,0xd52684) with the 0x00d50084 jump-table word patched to the owned adapter image for the owned role; image-4 code [0x61c91c,0x64891c) with 0x63c51c/0x641438 replaced by call-recording stubs; conversion, pair, cells and settings from the pinned images"
  , caseExclusion = "Synthetic admitted carrier: 0x63c51c and 0x641438 are recording stubs, so their internal display and reconciliation effects are not executed; the retained call presence is observed. Prepared state, not key-reachable acceptance."
  , caseEntry = at (scenarioCaller scenario - 4)
  , caseExit = at 0x00958000
  , caseRegions =
      [ mkRegion (RegionId "dispatcher") (at dispatcherBase) (B.length dispatcher) ReadExecute dispatcher
      , mkRegion (RegionId "code") (at codeBase) (B.length code) ReadExecute code
      , mkRegion (RegionId "carrier") (at (scenarioCaller scenario - 4)) (B.length carrier) ReadExecute carrier
      , mkRegion (RegionId "conv-tables") (at convTableBase) (B.length (slice image4 0x0061c400 convTableBase (0x77b824 - convTableBase))) ReadOnly (slice image4 0x0061c400 convTableBase (0x77b824 - convTableBase))
      , mkRegion (RegionId "conv-ptrs") (at convPointerBase) (B.length (slice image4 0x0061c400 convPointerBase 0x860)) ReadOnly (slice image4 0x0061c400 convPointerBase 0x860)
      , mkRegion (RegionId "pairs") (at pairBase) (B.length (slice image4 0x0061c400 pairBase 0x400)) ReadOnly (slice image4 0x0061c400 pairBase 0x400)
      , mkRegion (RegionId "cells") (at cellsBase) (B.length cellsInitial) ReadWrite cellsInitial
      , mkRegion (RegionId "mirror") (at 0x0089ec00) 0x200 ReadWrite (poke 0x1c8 (assemble [1]) (B.replicate 0x200 0))
      , mkRegion (RegionId "mode-cell") (at 0x00951564) 4 ReadWrite (B.replicate 4 0)
      , mkRegion (RegionId "settings") (at settingsBase) 0x1c2 ReadWrite (B.replicate 0x1c2 0xaa)
      , mkRegion (RegionId "slot-defaults") (at 0x007cb804) 0x28 ReadOnly (B.replicate 0x28 0)
      , mkRegion (RegionId "defaults") (at 0x0081b074) 0x28 ReadOnly (B.replicate 0x28 0)
      , mkRegion (RegionId "stack") (at 0x00959000) 0x1000 ReadWrite (B.replicate 0x1000 0x5a)
      , mkRegion (RegionId "exit") (at 0x00958000) 4 ReadExecute (assemble [0xe12fff1e])
      ]
        ++ ( if scenarioRole scenario == Owned
               then [ mkRegion (RegionId "owned") (at ownedBase) ownedTextSize ReadExecute (B.take ownedTextSize (owned <> B.replicate ownedTextSize 0)) ]
                    ++ ( if stateSize > 0
                           then [ mkRegion (RegionId "owned-state") (at 0x00a80000) stateSize ReadWrite stateBytes ]
                           else []
                       )
               else []
           )
  , caseFunctions =
      (at (scenarioCaller scenario - 4), fromIntegral (B.length carrier)) : dispatcherFunctions
        ++ [ (at ownedBase, fromIntegral ownedTextSize) | scenarioRole scenario == Owned ]
  , caseInitial = [(register, value register) | register <- [R0,R1,R2,R3,R4,R5,R6,R7,R8,R9,R10,R11,R12,Sp,Lr,Cpsr]]
  , caseExpectedStatus = Returned
  , caseExpectedRegisters = [(R0, 0x4d), (Sp, 0x00959ff0)]
  , caseExpectedMemory = []
  , caseCp14Identity = Nothing
  , caseKernel = Nothing
  }
 where
  ownedTextSize = if B.length owned <= 0x1000 then 0x1000 else 0x80000 :: Int
  stateBytes = B.drop 0x80000 owned
  stateSize = B.length stateBytes
  dispatcher = if scenarioPoison scenario
    then poke (0x00d51b58 - dispatcherBase) (assemble [0xe7f000f0]) base
    else base
   where
    base = poke (0x00d50084 - dispatcherBase)
      (assemble [if scenarioRole scenario == Owned then ownedBase else 0x00d51b58])
      (slice image5 0x00bf1a00 dispatcherBase (0xd52684 - dispatcherBase))
  code = poke (0x0061d9ec - codeBase) (assemble [0xe12fff1e])
    (poke (0x0061d8dc - codeBase) (assemble [0xe12fff1e])
      (poke (0x00641438 - codeBase) (recordStub2 0x0095_6ca0)
        (poke (0x0063c51c - codeBase) (recordStub3 0x0095_6c90) (slice image4 0x0061c400 codeBase 0x2c000))))
  carrier = assemble
    [ branchLinkWord (scenarioCaller scenario - 4) 0x00d4fc74
    , movw 14 0x8000
    , movt 14 0x0095
    , 0xe12fff1e
    ]
  cellsInitial =
    let (storedSize, storedAspect, storedThird) = scenarioStored scenario
    in poke 0x448 (assemble [storedAspect])
      (poke 0x440 (assemble [storedThird])
        (poke 0x438 (assemble [storedSize])
          (poke 0 (assemble [scenarioMode scenario]) (B.replicate 0x480 0))))
  value register = case register of
    R0 -> 0xf6
    R1 -> scenarioToken scenario
    Sp -> 0x00959ff0
    Lr -> 0x00958000
    Cpsr -> 0x10
    _ -> 0

branchWord :: Word32 -> Word32 -> Word32
branchWord from to = 0xea000000 .|. (((to - (from + 8)) `shiftR` 2) .&. 0x00ffffff)

branchLinkWord :: Word32 -> Word32 -> Word32
branchLinkWord from to = 0xeb000000 .|. (((to - (from + 8)) `shiftR` 2) .&. 0x00ffffff)

recordStub3 :: Word32 -> B.ByteString
recordStub3 base = assemble
  [ movw 12 (base .&. 0xffff)
  , movt 12 (base `shiftR` 16)
  , 0xe58c0000
  , 0xe58c1004
  , 0xe58c2008
  , 0xe12fff1e
  ]

recordStub2 :: Word32 -> B.ByteString
recordStub2 base = assemble
  [ movw 12 (base .&. 0xffff)
  , movt 12 (base `shiftR` 16)
  , 0xe58c0000
  , 0xe58c1004
  , 0xe12fff1e
  ]

movw, movt :: Word32 -> Word32 -> Word32
movw register immediate = 0xe3000000 .|. (register `shiftL` 12) .|. ((immediate .&. 0xf000) `shiftL` 4) .|. (immediate .&. 0x0fff)
movt register immediate = 0xe3400000 .|. (register `shiftL` 12) .|. (immediate .&. 0x0fff)

at :: Word32 -> GuestAddress
at = GuestAddress . fromIntegral

slice :: B.ByteString -> Word32 -> Word32 -> Word32 -> B.ByteString
slice bytes base address count = B.take (fromIntegral count) (B.drop (fromIntegral (address - base)) bytes)

poke :: Word32 -> B.ByteString -> B.ByteString -> B.ByteString
poke offset value bytes = B.take start bytes <> value <> B.drop (start + B.length value) bytes
 where
  start = fromIntegral offset

-- Session cohort ------------------------------------------------------------

-- | One guest operation in a picker session. The carrier runs them in order on
-- the same resident image.
data SessionOp
  = SelectRow Word32
  | OpenPicker
  | AcceptPicker
  | CancelPicker
  | WriteSetting Word32 Word32
  | ReadSetting Word32
  deriving (Eq, Show)

data Session = Session
  { sessionId :: String
  , sessionRoles :: [Role]
  , sessionMode :: Word32
  , sessionOps :: [SessionOp]
  , sessionExpect :: [Word32]
  , sessionFinalPair :: (Word32, Word32)
  , sessionCheckSettings :: Bool
  , sessionUnrelated :: Maybe Int
  }

data Targets = Targets
  { targetDispatcher :: Word32
  , targetNotify :: Word32
  , targetWrite :: Word32
  , targetRead :: Word32
  , targetBegin :: Word32
  , targetConfirm :: Word32
  , targetCancel :: Word32
  , targetRetained :: Word32
  }

lifecycleSymbols :: [String]
lifecycleSymbols =
  [ "rb_menu_value_begin"
  , "rb_menu_value_confirm"
  , "rb_menu_value_cancel"
  , "rb_menu_value_retained"
  , "rb_menu_value_size_arm"
  , "rb_menu_value_size_apply"
  ]

sessionCarrierAt, sessionScratch, sessionScratchCells :: Word32
sessionCarrierAt = 0x00c2d920
sessionScratch = 0x0095b000
sessionScratchCells = 0x0095b400

sessionHandbackCount, sessionAuxCount, sessionDisplay1, sessionDisplay2 :: Word32
sessionHandbackCount = sessionScratch + 0x60
sessionAuxCount = sessionScratch + 0xa8
sessionDisplay1 = sessionScratch + 0x40
sessionDisplay2 = sessionScratch + 0x50

ownedSymbol :: M.Map String Symbol -> String -> Word32
ownedSymbol symbols name =
  let (GuestAddress address, size) = functionSymbol symbols name
      low = fromIntegral address :: Word32
  in if low >= ownedBase && low + size <= 0x00a80000 && low .&. 3 == 0
       then low
       else error (name ++ " is not in the owned text window")

resolveTargets :: M.Map String Symbol -> Targets
resolveTargets symbols = Targets
  { targetDispatcher = 0x00d4fc74
  , targetNotify = 0x00d5b350
  , targetWrite = 0x0061c91c
  , targetRead = 0x0061c93c
  , targetBegin = ownedSymbol symbols "rb_menu_value_begin"
  , targetConfirm = ownedSymbol symbols "rb_menu_value_confirm"
  , targetCancel = ownedSymbol symbols "rb_menu_value_cancel"
  , targetRetained = ownedSymbol symbols "rb_menu_value_retained"
  }

imagePair :: B.ByteString -> Word32 -> (Word32, Word32)
imagePair image4 token =
  (readWordAt image4 offset, readWordAt image4 (offset + 4))
 where
  offset = fromIntegral (pairBase - 0x0061c400) + 16 * fromIntegral token

validatePairs :: B.ByteString -> IO ()
validatePairs image4 = do
  unless (imagePair image4 1 == (0, 1)) (fail "IMAGE SIZE token 1 pair differs")
  unless (imagePair image4 2 == (0, 2)) (fail "IMAGE SIZE token 2 pair differs")

sessions :: B.ByteString -> [Session]
sessions image4 = [leftAccept, okAccept, repeatedMoves, unrelatedEdit, derivedProperties, bankCommitWithoutSession, displayMode]
 where
  (s1, a1) = imagePair image4 1
  (s2, a2) = imagePair image4 2
  readOps properties = map ReadSetting properties
  leftAccept = Session "session-left-accept" [Stock, Owned] 0x2c
    ([SelectRow 1, OpenPicker, SelectRow 2, AcceptPicker, OpenPicker, SelectRow 3, CancelPicker] ++ readOps [0xf6, 0xf9])
    [s2, a2] (s2, a2) True Nothing
  okAccept = Session "session-ok-accept" [Stock, Owned] 0x2c
    ([SelectRow 1, OpenPicker, SelectRow 2, AcceptPicker, OpenPicker, SelectRow 3, CancelPicker] ++ readOps [0xf6, 0xf9])
    [s2, a2] (s2, a2) True Nothing
  repeatedMoves = Session "session-repeated-moves" [Stock, Owned] 0x2c
    ([SelectRow 1, OpenPicker, SelectRow 2, SelectRow 3, SelectRow 2, AcceptPicker, OpenPicker, SelectRow 3, CancelPicker] ++ readOps [0xf6, 0xf9])
    [s2, a2] (s2, a2) True Nothing
  unrelatedEdit = Session "session-unrelated-edit" [Stock, Owned] 0x2c
    ([SelectRow 1, OpenPicker, SelectRow 2, WriteSetting 0x13 0, CancelPicker] ++ readOps [0xf6, 0xf9, 0x13])
    [s1, a1, 0] (s1, a1) False (Just 2)
  derivedProperties = Session "session-derived-properties" [Stock, Owned] 0x2c
    ([ WriteSetting 0x161 1
     , WriteSetting 0x165 2
     , SelectRow 1
     , WriteSetting 0x1e5 1
     , OpenPicker
     , SelectRow 2
     , WriteSetting 0x161 2
     , WriteSetting 0x165 1
     , CancelPicker
     ]
      ++ readOps [0xf6, 0xf9, 0x161, 0x165, 0x1e5])
    [s1, a1, 1, 2, 1] (s1, a1) True Nothing
  bankCommitWithoutSession = Session "session-bank-commit-without-session" [Owned] 0x2c
    ([SelectRow 1, CancelPicker] ++ readOps [0xf6, 0xf9])
    [s1, a1] (s1, a1) True Nothing
  displayMode = Session "session-display-mode" [Stock, Owned] 0x21
    ([SelectRow 1, OpenPicker, SelectRow 2, CancelPicker] ++ readOps [0xf6, 0xf9])
    [s1, a1] (s1, a1) True Nothing

loadReg :: Word32 -> Word32 -> [Word32]
loadReg register value = [movw register value, movt register (value `shiftR` 16)]

call :: Word32 -> Word32 -> [Word32] -> (Word32, [Word32])
call address target prefix =
  let site = address + 4 * fromIntegral (length prefix)
  in (site + 4, prefix ++ [branchLinkWord site target])

-- | The retained handback, run only when the preceding lifecycle call returned 1.
handback :: Targets -> Word32 -> Word32 -> (Word32, [Word32])
handback targets address param =
  let load = loadReg 0 param
      site = address + 8 + 4 * fromIntegral (length load)
      end = site + 4
      beq = 0x0a000000 .|. (((end - (address + 12)) `shiftR` 2) .&. 0x00ffffff)
  in (end, [0xe3500000, beq] ++ load ++ [branchLinkWord site (targetRetained targets)])

opWords :: Targets -> Role -> Word32 -> SessionOp -> (Word32, [Word32])
opWords targets role address op = case op of
  SelectRow token ->
    call address (targetDispatcher targets) (loadReg 0 0xf6 ++ loadReg 1 token)
  WriteSetting property value ->
    call address (targetWrite targets) (loadReg 0 property ++ loadReg 1 value ++ loadReg 2 0)
  OpenPicker -> case role of
    Stock -> notify 1 0x1d
    Owned ->
      let (afterBegin, beginWords) = call address (targetBegin targets) (loadReg 0 0xf6)
          (after, handbackWords) = handback targets afterBegin 0
      in (after, beginWords ++ handbackWords)
  AcceptPicker -> case role of
    Stock -> notify 0xf 0x1d
    Owned ->
      let (afterConfirm, confirmWords) = call address (targetConfirm targets) []
          (after, handbackWords) = handback targets afterConfirm 0
      in (after, confirmWords ++ handbackWords)
  CancelPicker -> case role of
    Stock -> notify 0xf 0x1a
    Owned ->
      let (afterCancel, cancelWords) = call address (targetCancel targets) []
          (after, handbackWords) = handback targets afterCancel 1
      in (after, cancelWords ++ handbackWords)
  ReadSetting _ -> error "ReadSetting is emitted by sessionWords"
 where
  notify kind identity =
    call address (targetNotify targets) (loadReg 0 kind ++ loadReg 1 1 ++ loadReg 2 identity ++ loadReg 3 0xf6)

sessionWords :: Targets -> Role -> Word32 -> [SessionOp] -> [Word32]
sessionWords targets role start ops = loadReg 4 sessionScratch ++ go (start + 8) (0 :: Int) ops
 where
  go _ _ [] = []
  go address index (op : rest) = case op of
    ReadSetting property ->
      let (after, callWords) = call address (targetRead targets) (loadReg 0 property ++ loadReg 1 0)
          store = 0xe5840000 .|. (fromIntegral index * 4 :: Word32)
      in callWords ++ [store] ++ go (after + 4) (index + 1) rest
    _ ->
      let (after, words_) = opWords targets role address op
      in words_ ++ go after index rest

sessionCarrier :: Targets -> Role -> [SessionOp] -> B.ByteString
sessionCarrier targets role ops =
  assemble ([0xe92d41f0] ++ sessionWords targets role (sessionCarrierAt + 4) ops ++ [0xe8bd81f0])

-- | Records @(r0, r1)@ of each call at @counter + 4 + 8n@ and @counter + 8 + 8n@.
appendStub :: Word32 -> B.ByteString
appendStub counter = assemble
  [ movw 12 (counter .&. 0xffff)
  , movt 12 (counter `shiftR` 16)
  , 0xe59c3000
  , 0xe2833001
  , 0xe58c3000
  , 0xe08cc103
  , 0xe50c0004
  , 0xe58c1000
  , 0xe12fff1e
  ]

dispatcherFunctions :: [(GuestAddress, Word32)]
dispatcherFunctions =
  [ (at 0x00d4fc74, 0x2a10)
  , (at 0x00d4d604, 20)
  , (at 0x0061c91c, 16)
  , (at 0x0061c93c, 16)
  , (at 0x0061d514, 16)
  , (at 0x0061d8dc, 80)
  , (at 0x0061d9ec, 80)
  , (at 0x0061e110, 25832)
  , (at 0x00624620, 44)
  , (at 0x00624694, 49968)
  , (at 0x006309c4, 112)
  , (at 0x00631a00, 336)
  , (at 0x0063396c, 32)
  , (at 0x00633a0c, 0x20)
  , (at 0x00633b0c, 0x20)
  , (at 0x00633b6c, 0x20)
  , (at 0x00633b8c, 0x20)
  , (at 0x0063c51c, 0x20)
  , (at 0x00641438, 0x20)
  , (at 0x00633cac, 32)
  , (at 0x00633d0c, 32)
  , (at 0x0063458c, 32)
  , (at 0x0063460c, 32)
  , (at 0x0063462c, 36)
  , (at 0x006347b0, 32)
  , (at 0x006347d0, 32)
  , (at 0x0063db24, 80)
  , (at 0x006439e0, 40)
  , (at 0x00643d94, 48)
  , (at 0x00643dc4, 40)
  , (at 0x00643fb8, 76)
  , (at 0x00644188, 56)
  , (at 0x006442b4, 56)
  , (at 0x00644848, 476)
  ]

sessionFunctions :: [(GuestAddress, Word32)]
sessionFunctions =
  dispatcherFunctions
    ++ [ (at 0x00d5b350, 0x240)
       , (at 0x00637ed0, 0x3c)
       , (at 0x00637e5c, 0x70)
       , (at 0x00637e38, 0x24)
       , (at 0x00637e14, 0x24)
       , (at 0x0061c90c, 0x10)
       , (at 0x0061daa8, 0x58)
       , (at 0x0061d92c, 0x14)
       , (at 0x00722054, 0x124)
       , (at 0x0063219c, 0x3c)
       , (at 0x0063220c, 0x24)
       , (at 0x0063647c, 0x24)
       ]

sessionSpec :: B.ByteString -> B.ByteString -> B.ByteString -> Targets -> Session -> Role -> CaseSpec
sessionSpec image4 image5 owned targets session role = CaseSpec
  { caseId = ident
  , caseProvenance =
      "X-M1 1.50 image-5 dispatcher and notify bytes [0xd4d604,0xd5b590) with the 0x00d50084 jump-table word patched to the owned adapter image for the owned role; image-4 code [0x61c91c,0x64891c) with 0x63c51c/0x641438 and 0x63220c/0x63647c replaced by recording stubs; conversion, pair, cells and settings from the pinned images"
  , caseExclusion =
      "Synthetic admitted session: 0x63c51c, 0x641438, 0x63220c and 0x63647c are recording stubs, so the display, current-property, state-snapshot and auxiliary-restore effects are observed as arguments and not executed; the cell snapshot 0x637e5c, the settings service and the dispatcher run as stock. Prepared state, not key-reachable acceptance."
  , caseEntry = at sessionCarrierAt
  , caseExit = at 0x00958000
  , caseRegions = regions
  , caseFunctions = ((at sessionCarrierAt, fromIntegral (B.length carrier)) : sessionFunctions)
      ++ [ (at ownedBase, 0x80000) | role == Owned ]
  , caseInitial = [(register, initial register) | register <- [R0, R1, R2, R3, R4, R5, R6, R7, R8, R9, R10, R11, R12, Sp, Lr, Cpsr]]
  , caseExpectedStatus = Returned
  , caseExpectedRegisters = [(Sp, 0x00959ff0)]
  , caseExpectedMemory = []
  , caseCp14Identity = Nothing
  , caseKernel = Nothing
  }
 where
  ident = sessionId session ++ "-" ++ roleText role
  codeStart = 0x0061c90c :: Word32
  carrier = sessionCarrier targets role (sessionOps session)
  ownedText = B.take 0x80000 (owned <> B.replicate 0x80000 0)
  state = B.drop 0x80000 owned
  initial register = case register of
    Sp -> 0x00959ff0
    Lr -> 0x00958000
    Cpsr -> 0x10
    _ -> 0
  cellsInitial = poke 0x55c (assemble [sessionScratchCells]) (poke 0x174 (assemble [sessionScratchCells]) (poke 0 (assemble [sessionMode session]) (B.replicate 0x800 0)))
  settingsInitial = B.replicate 0x1c2 0xaa <> B.replicate 0x1c2 0
  dispatcher = poke (0x00d50084 - 0x00d4d604) (assemble [if role == Owned then ownedBase else 0x00d51b58]) base
   where
    raw = slice image5 0x00bf1a00 0x00d4d604 (0xd5b590 - 0xd4d604)
    base = if role == Owned then poke (0x00d51b58 - 0x00d4d604) (assemble [0xe7f000f0]) raw else raw
  code = poke (0x0061d9ec - codeStart) (assemble [0xe12fff1e])
    (poke (0x0061d8dc - codeStart) (assemble [0xe12fff1e])
      (poke (0x00641438 - codeStart) (recordStub2 sessionDisplay2)
        (poke (0x0063c51c - codeStart) (recordStub3 sessionDisplay1)
          (poke (0x0063647c - codeStart) (appendStub sessionAuxCount)
            (poke (0x0063220c - codeStart) (appendStub sessionHandbackCount)
              (slice image4 0x0061c400 codeStart (0x00722178 - codeStart)))))))
  regions =
    [ mkRegion (RegionId "dispatcher") (at 0x00d4d604) (B.length dispatcher) ReadExecute dispatcher
    , mkRegion (RegionId "code") (at codeStart) (B.length code) ReadExecute code
    , mkRegion (RegionId "carrier") (at sessionCarrierAt) (B.length carrier) ReadExecute carrier
    , mkRegion (RegionId "conv-tables") (at convTableBase) (B.length (slice image4 0x0061c400 convTableBase (0x77b824 - convTableBase))) ReadOnly (slice image4 0x0061c400 convTableBase (0x77b824 - convTableBase))
    , mkRegion (RegionId "pairs") (at pairBase) (B.length (slice image4 0x0061c400 pairBase 0x400)) ReadOnly (slice image4 0x0061c400 pairBase 0x400)
    , mkRegion (RegionId "cells") (at cellsBase) 0x800 ReadWrite cellsInitial
    , mkRegion (RegionId "mirror") (at 0x0089ec00) 0x200 ReadWrite (poke 0x1c8 (assemble [1]) (B.replicate 0x200 0))
    , mkRegion (RegionId "mode-cells") (at 0x00951564) 0x2d24 ReadWrite (B.replicate 0x2d24 0)
    , mkRegion (RegionId "settings") (at settingsBase) 0x384 ReadWrite settingsInitial
    , mkRegion (RegionId "slot-defaults") (at 0x007cb804) 0x28 ReadOnly (B.replicate 0x28 0)
    , mkRegion (RegionId "defaults-ptrs") (at 0x0081b074) 0x888 ReadOnly (B.replicate 0x28 0 <> slice image4 0x0061c400 convPointerBase 0x860)
    , mkRegion (RegionId "stack") (at 0x00959000) 0x1000 ReadWrite (B.replicate 0x1000 0x5a)
    , mkRegion (RegionId "exit") (at 0x00958000) 4 ReadExecute (assemble [0xe12fff1e])
    , mkRegion (RegionId "scratch") (at sessionScratch) 0x1000 ReadWrite (B.replicate 0x1000 0)
    ]
      ++ ( if role == Owned
             then
               [ mkRegion (RegionId "owned") (at ownedBase) 0x80000 ReadExecute ownedText
               , mkRegion (RegionId "owned-state") (at 0x00a80000) (B.length state) ReadWrite state
               ]
             else []
         )

runSession
  :: FilePath
  -> B.ByteString
  -> B.ByteString
  -> B.ByteString
  -> FilePath
  -> Targets
  -> Session
  -> Role
  -> IO NativeObservation
runSession runner image4 image5 owned out targets session role = do
  let spec = sessionSpec image4 image5 owned targets session role
  path <- writeCase (out </> caseId spec) spec
  (_, regions, _, _) <- loadCase path
  (_, report) <- runCase runner path
  exit <- either (error . renderObservationError) pure (reportRunnerExit report)
  either (error . renderObservationError) pure (observeNative exit regions (get report "runner"))

runSessions :: FilePath -> B.ByteString -> B.ByteString -> B.ByteString -> FilePath -> M.Map String Symbol -> IO ()
runSessions runner image4 image5 owned out symbols = do
  validatePairs image4
  forM_ lifecycleSymbols $ \name -> unless (ownedSymbol symbols name >= ownedBase) (fail (name ++ " is not retained"))
  let targets = resolveTargets symbols
      cohort = sessions image4
  forM_ cohort $ \session -> do
    stock <- runRole targets session Stock
    ownedObservation <- runRole targets session Owned
    checkSession session stock ownedObservation
  putStrLn ("menu-value-sessions: " ++ show (length cohort) ++ " sessions")
 where
  runRole targets session role
    | role `elem` sessionRoles session = Just <$> runSession runner image4 image5 owned out targets session role
    | otherwise = pure Nothing

checkSession :: Session -> Maybe NativeObservation -> Maybe NativeObservation -> IO ()
checkSession session stock ownedObservation = do
  let label = sessionId session
      expect = sessionExpect session
      (finalSize, finalAspect) = sessionFinalPair session
      readouts observation =
        let scratch = regionBytesOf observation (RegionId "scratch")
        in [wordAtBytes scratch (4 * index) | index <- [0 .. length expect - 1]]
      checkReadouts role observation =
        forM_ (zip3 [0 :: Int ..] expect (readouts observation)) $ \(index, wanted, got) ->
          case (role, sessionUnrelated session) of
            (Stock, Just skipped) | skipped == index -> pure ()
            _ -> unless (got == wanted) (fail (label ++ ": " ++ roleText role ++ " readout " ++ show index ++ " is " ++ show got ++ ", wanted " ++ show wanted))
      checkStatus role observation =
        unless (nativeStatus observation == Returned)
          (fail (label ++ ": " ++ roleText role ++ " status " ++ runnerStatusLabel (nativeStatus observation)))
      checkCells role observation =
        let cells = regionBytesOf observation (RegionId "cells")
        in unless (wordAtBytes cells 0x50 == finalSize && wordAtBytes cells 0x60 == finalAspect)
             (fail (label ++ ": " ++ roleText role ++ " final pair differs"))
      checkDisplay role observation =
        let scratch = regionBytesOf observation (RegionId "scratch")
            args = (wordAtBytes scratch 0x40, wordAtBytes scratch 0x44, wordAtBytes scratch 0x48, wordAtBytes scratch 0x50, wordAtBytes scratch 0x54)
            wanted = if sessionMode session == 0x2c then (0, 0, 0, 0, 0) else (0x34, 0, 0xf6, 1, 0)
        in unless (args == wanted) (fail (label ++ ": " ++ roleText role ++ " display args differ"))
  forM_ stock $ \observation -> do
    checkStatus Stock observation
    checkReadouts Stock observation
    checkCells Stock observation
    checkDisplay Stock observation
  forM_ ownedObservation $ \observation -> do
    checkStatus Owned observation
    checkReadouts Owned observation
    checkCells Owned observation
    checkDisplay Owned observation
  case (stock, ownedObservation) of
    (Just reference, Just subject) -> do
      unless (regionBytesOf reference (RegionId "cells") == regionBytesOf subject (RegionId "cells")) (fail (label ++ ": cells differ"))
      when (sessionCheckSettings session) $
        unless (B.take 0x1c2 (regionBytesOf reference (RegionId "settings")) == B.take 0x1c2 (regionBytesOf subject (RegionId "settings")))
          (fail (label ++ ": settings differ"))
    _ -> pure ()
