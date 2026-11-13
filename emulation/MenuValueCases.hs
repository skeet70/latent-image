{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Bounded stock/owned comparisons at the commit dispatcher's IMAGE SIZE arm.
-- The owned role points the dispatcher's jump-table word for property @0xf6@
-- at the seam image's @rb_menu_value_size_arm@ and traps the stock arm; the
-- stock role leaves both. Everything else is the same stock code, cells and
-- settings, so the two observations compare directly.
--
-- The pair cohort selects one row per case. The lifecycle cohort runs one
-- sequence on a resident image: row selections through the dispatcher,
-- settings edits and reads through the stock service, and the engine's Push,
-- Pop and Close notifications, which the production engine adapter forwards
-- to stock @0xd5b350@. Three wrong owned variants must each be reported
-- different in every case they run: an image that always declines, the
-- deleted picker session, and an engine adapter that consumes Pop.
module Main (main) where

import Control.Monad (forM, forM_, unless)
import Data.Aeson (Value, object, (.=))
import qualified Data.Aeson as A
import Data.Bits ((.&.), (.|.), shiftL, shiftR, testBit)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.Coverage (ExtentObservation (..), Observation (..), parseObservation)
import Emulation.NativeCase (loadCase, runCase)
import Emulation.NativeObservation
  ( GuestAddress (..)
  , NativeObservation (..)
  , Register (..)
  , RegionId (..)
  , RunnerStatus (..)
  , hexWord32
  , observeNative
  , reportRunnerExit
  , renderObservationError
  , runnerStatusLabel
  )
import Emulation.OwnedCase
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host (get)
import FujiAF.JSON (encodeCanonical)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))

-- Stock layout -------------------------------------------------------------

image4Base, image5Base :: Word32
image4Base = 0x0061c400
image5Base = 0x00bf1a00

dispatcherBase, dispatcherEntry, jumpWord, stockArm, stockArmEnd, notifyEntry, notifyEnd :: Word32
dispatcherBase = 0x00d4d604
dispatcherEntry = 0x00d4fc74
-- | The dispatcher's jump-table word for property @0xf6@.
jumpWord = 0x00d50084
stockArm = 0x00d51b58
stockArmEnd = 0x00d51c00
notifyEntry = 0x00d5b350
notifyEnd = 0x00d5b590

settingsWrite, settingsRead, bankCopy :: Word32
settingsWrite = 0x0061c91c
settingsRead = 0x0061c93c
bankCopy = 0x0061c90c

convTableBase, convPointerBase, pairBase, cellsBase, settingsBase :: Word32
convTableBase = 0x00774f48
convPointerBase = 0x0081b09c
pairBase = 0x007a00cc
cellsBase = 0x00956838
settingsBase = 0x0096cc64

ownedBase, ownedStateBase :: Word32
ownedBase = 0x00a00000
ownedStateBase = 0x00a80000

ownedTextBytes :: Int
ownedTextBytes = fromIntegral (ownedStateBase - ownedBase)

exitAddress, stackBase, stackTop :: Word32
exitAddress = 0x00958000
stackBase = 0x00959000
stackTop = 0x00959ff0

trapWord :: Word32
trapWord = 0xe7f000f0

-- Inputs -------------------------------------------------------------------

-- | A compiled seam image: text and read-only data from 'ownedBase', writable
-- data from 'ownedStateBase'.
data SeamImage = SeamImage
  { seamBytes :: B.ByteString
  , seamArm :: Word32
  , seamDigest :: String
  }

-- | The exports of the deleted picker session in the old-session image.
data SessionEntries = SessionEntries
  { sessionBegin :: Word32
  , sessionConfirm :: Word32
  , sessionCancel :: Word32
  , sessionRetained :: Word32
  }

loadSeam :: [String] -> FilePath -> FilePath -> IO (SeamImage, M.Map String Symbol)
loadSeam extra binPath symbolsPath = do
  bytes <- B.readFile binPath
  unless (B.length bytes >= 0x40 && B.length bytes <= ownedTextBytes + 0x1000)
    (fail (binPath ++ ": seam image size differs"))
  symbols <- parseSymbols ("rb_menu_value_size_arm" : extra) symbolsPath
  pure (SeamImage bytes (ownedSymbol symbols "rb_menu_value_size_arm") (sha256Hex (sha256 bytes)), symbols)

ownedSymbol :: M.Map String Symbol -> String -> Word32
ownedSymbol symbols name =
  let (GuestAddress address, size) = functionSymbol symbols name
      low = fromIntegral address :: Word32
  in if low >= ownedBase && low + size <= ownedStateBase && low .&. 3 == 0
       then low
       else error (name ++ " is not in the owned text window")

sessionNames :: [String]
sessionNames = ["rb_menu_value_begin", "rb_menu_value_confirm", "rb_menu_value_cancel", "rb_menu_value_retained"]

sessionEntries :: M.Map String Symbol -> SessionEntries
sessionEntries symbols = SessionEntries
  { sessionBegin = ownedSymbol symbols "rb_menu_value_begin"
  , sessionConfirm = ownedSymbol symbols "rb_menu_value_confirm"
  , sessionCancel = ownedSymbol symbols "rb_menu_value_cancel"
  , sessionRetained = ownedSymbol symbols "rb_menu_value_retained"
  }

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [runner, image4Path, image5Path, out, result, ownedPath, ownedSymbols, declinePath, declineSymbols, sessionPath, sessionSymbols] -> do
      image4 <- B.readFile image4Path
      image5 <- B.readFile image5Path
      validatePairs image4
      (owned, _) <- loadSeam [] ownedPath ownedSymbols
      (decline, _) <- loadSeam [] declinePath declineSymbols
      (session, symbols) <- loadSeam sessionNames sessionPath sessionSymbols
      runnerDigest <- sha256Hex . sha256 <$> B.readFile runner
      ownedSymbolsDigest <- sha256Hex . sha256 <$> B.readFile ownedSymbols
      let inputs = Inputs runner image4 image5 out
      pairStocks <- forM pairCohort $ \pair -> (,) pair <$> stockPair inputs pair
      pairs <- forM pairStocks $ \(pair, stock) -> runPair inputs owned pair stock
      declined <- forM [(pair, stock) | (pair, stock) <- pairStocks, pairOffered pair] $ \(pair, stock) ->
        runPairControl inputs decline pair stock
      lifecycleStocks <- forM (lifecycleCohort image4) $ \lifecycle -> (,) lifecycle <$> stockLifecycle inputs lifecycle
      lifecycles <- forM lifecycleStocks $ \(lifecycle, stock) -> runLifecycle inputs owned lifecycle stock
      let variants =
            [ ("old-session", session, Session (sessionEntries symbols))
            , ("pop-consumed", owned, PopConsumed)
            ]
      controls <- forM variants $ \(name, image, routing) ->
        (,) name <$> forM lifecycleStocks (\(lifecycle, stock) -> runLifecycleControl inputs name image routing lifecycle stock)
      let controlRows =
            controlJson "always-decline" (seamDigest decline) declined
              : [ controlJson name (seamDigest image) rows
                | ((name, rows), (_, image, _)) <- zip controls variants ]
          caseRows = map snd pairs ++ map snd lifecycles
          problems = concatMap fst pairs ++ concatMap fst lifecycles
          survivors =
            [ name ++ " " ++ caseName
            | (name, rows) <- ("always-decline", declined) : controls
            , (caseName, differences) <- rows
            , null differences ]
          equal = null problems && null survivors
      B.writeFile (result </> "menu-value.json") $ encodeCanonical (object
        [ "schema" .= (1 :: Int)
        , "suite" .= ("menu-value" :: String)
        , "image4_sha256" .= sha256Hex (sha256 image4)
        , "image5_sha256" .= sha256Hex (sha256 image5)
        , "owned_sha256" .= seamDigest owned
        , "owned_symbols_sha256" .= ownedSymbolsDigest
        , "runner_sha256" .= runnerDigest
        , "cases" .= caseRows
        , "controls" .= controlRows
        , "problems" .= problems
        , "survivors" .= survivors
        , "equal" .= equal
        ]) <> "\n"
      putStrLn ("menu-value: " ++ show (length pairs) ++ " pairs, " ++ show (length lifecycles)
        ++ " lifecycle cases, " ++ show (length controlRows) ++ " controls, "
        ++ show (length problems) ++ " problems, " ++ show (length survivors) ++ " survivors")
      forM_ (problems ++ map ("survivor: " ++) survivors) putStrLn
      unless equal exitFailure
    _ -> fail "usage: menu-value-cases RUNNER IMAGE4 IMAGE5 OUT RESULT OWNED_IMAGE OWNED_SYMBOLS DECLINE_IMAGE DECLINE_SYMBOLS SESSION_IMAGE SESSION_SYMBOLS"

data Inputs = Inputs
  { inputRunner :: FilePath
  , inputImage4 :: B.ByteString
  , inputImage5 :: B.ByteString
  , inputOut :: FilePath
  }

controlJson :: String -> String -> [(String, [String])] -> Value
controlJson name digest rows = object
  [ "name" .= name
  , "image_sha256" .= digest
  , "reported" .= all (not . null . snd) rows
  , "cases" .= [object ["id" .= caseName, "differences" .= differences] | (caseName, differences) <- rows]
  ]

-- Runner -------------------------------------------------------------------

data Run = Run
  { runObservation :: NativeObservation
  , runExecuted :: [ExtentObservation]
  }

runSpec :: Inputs -> CaseSpec -> IO Run
runSpec inputs spec = do
  path <- writeCase (inputOut inputs </> caseId spec) spec
  (_, regions, _, _) <- loadCase path
  (_, report) <- runCase (inputRunner inputs) path
  exit <- either (fail . renderObservationError) pure (reportRunnerExit report)
  observation <- either (fail . renderObservationError) pure (observeNative exit regions (get report "runner"))
  executed <- either fail pure (parseObservation (BL.toStrict (A.encode report)))
  pure (Run observation (observationExtents executed))

-- | Words the runner entered inside @[low, high)@ across the declared extents.
executedIn :: Run -> Word32 -> Word32 -> Int
executedIn run low high = sum
  [ length
      [ ()
      | index <- [fromIntegral ((from - base) `div` 4) .. fromIntegral ((to - base) `div` 4) - 1 :: Int]
      , index `div` 8 < B.length bitmap
      , testBit (B.index bitmap (index `div` 8)) (index `mod` 8)
      ]
  | ExtentObservation base bytes bitmap _ _ <- runExecuted run
  , let from = max low base
        to = min high (base + bytes)
  , from < to
  ]

status :: Run -> RunnerStatus
status = nativeStatus . runObservation

region :: Run -> String -> B.ByteString
region run name = regionBytesOf (runObservation run) (RegionId name)

-- | A stock run that does not return has not exercised the case.
requireReturned :: String -> Run -> IO ()
requireReturned label run =
  unless (status run == Returned)
    (fail (label ++ ": stock role status " ++ runnerStatusLabel (status run)))

regionDifferences :: [String] -> Run -> Run -> [String]
regionDifferences names stock subject =
  [ name ++ " differs" | name <- names, region stock name /= region subject name ]

-- Pair cohort --------------------------------------------------------------

data Pair = Pair
  { pairToken :: Word32
  , pairMode :: Word32
  , pairStored :: (Word32, Word32, Word32)
  , pairCaller :: Word32
  , pairOffered :: Bool
  }

pairCohort :: [Pair]
pairCohort =
  [ Pair token 0x2c (0, 0, 1) 0x00c2d924 True | token <- [0 .. 11] ]
    ++ [ Pair 2 mode (0, 0, 1) 0x00c2d924 True | mode <- [0x1f, 0x20, 0x21, 0x24, 0x25] ]
    ++ [ Pair 1 0x2c (0, 1, 0) 0x00c2d924 True
       , Pair 1 0x2c (0, 1, 1) 0x00c2d924 True
       , Pair 2 0x2c (0, 0, 1) 0x0061c408 True
       , Pair 2 0x2c (0, 0, 1) 0x00c321e0 True
       , Pair 12 0x2c (0, 0, 1) 0x00c2d924 False
       ]

pairName :: Pair -> String
pairName pair =
  let (size, aspect, third) = pairStored pair
  in "pair-token" ++ show (pairToken pair) ++ "-mode" ++ hexWord32 (pairMode pair)
       ++ "-stored" ++ concatMap show [size, aspect, third] ++ "-caller" ++ hexWord32 (pairCaller pair)

-- | The owned role, or the stock role when no seam image is given. An owned
-- role traps the stock arm when the token is offered, so a declining seam
-- fails instead of passing on the stock arm.
pairSpec :: Inputs -> Maybe SeamImage -> String -> Pair -> CaseSpec
pairSpec inputs seam label pair = CaseSpec
  { caseId = pairName pair ++ "-" ++ label
  , caseProvenance =
      "X-M1 1.50 image-5 dispatcher bytes [0xd4d604,0xd52684) with the 0x00d50084 jump-table word pointed at the owned seam for the owned role; image-4 code [0x61c91c,0x64891c) with 0x63c51c/0x641438 replaced by call-recording stubs; conversion, pair, cells and settings from the pinned images"
  , caseExclusion = "Synthetic admitted carrier: 0x63c51c and 0x641438 are recording stubs, so their internal display and reconciliation effects are not executed; the retained call presence is observed. Prepared state, not key-reachable acceptance."
  , caseEntry = at pairCarrier
  , caseExit = at exitAddress
  , caseRegions =
      [ mkRegion (RegionId "dispatcher") (at dispatcherBase) (B.length dispatcher) ReadExecute dispatcher
      , mkRegion (RegionId "code") (at codeBase) (B.length code) ReadExecute code
      , mkRegion (RegionId "carrier") (at pairCarrier) (B.length carrier) ReadExecute carrier
      , image4Region "conv-tables" convTableBase (0x0077b824 - convTableBase)
      , image4Region "conv-ptrs" convPointerBase 0x860
      , image4Region "pairs" pairBase 0x400
      , mkRegion (RegionId "cells") (at cellsBase) (B.length cellsInitial) ReadWrite cellsInitial
      , mkRegion (RegionId "mirror") (at 0x0089ec00) 0x200 ReadWrite (poke 0x1c8 (assemble [1]) (B.replicate 0x200 0))
      , mkRegion (RegionId "mode-cell") (at 0x00951564) 4 ReadWrite (B.replicate 4 0)
      , mkRegion (RegionId "settings") (at settingsBase) 0x1c2 ReadWrite (B.replicate 0x1c2 0xaa)
      , mkRegion (RegionId "slot-defaults") (at 0x007cb804) 0x28 ReadOnly (B.replicate 0x28 0)
      , mkRegion (RegionId "defaults") (at 0x0081b074) 0x28 ReadOnly (B.replicate 0x28 0)
      , mkRegion (RegionId "stack") (at stackBase) 0x1000 ReadWrite (B.replicate 0x1000 0x5a)
      , mkRegion (RegionId "exit") (at exitAddress) 4 ReadExecute (assemble [0xe12fff1e])
      ]
        ++ maybe [] seamRegions seam
  , caseFunctions =
      (at pairCarrier, fromIntegral (B.length carrier)) : dispatcherFunctions
        ++ [ (at ownedBase, fromIntegral ownedTextBytes) | Just _ <- [seam] ]
  , caseInitial = [(register, initial register) | register <- [R0, R1, R2, R3, R4, R5, R6, R7, R8, R9, R10, R11, R12, Sp, Lr, Cpsr]]
  , caseExpectedStatus = Returned
  , caseExpectedRegisters = [(R0, 0x4d), (Sp, stackTop)]
  , caseExpectedMemory = []
  , caseIgnoredMemory = []
  , caseCp14Identity = Nothing
  , caseKernel = Nothing
  }
 where
  codeBase = 0x0061c91c
  pairCarrier = pairCaller pair - 4
  image4Region name base size =
    let bytes = slice (inputImage4 inputs) image4Base base size
    in mkRegion (RegionId name) (at base) (B.length bytes) ReadOnly bytes
  dispatcher = dispatcherFor (inputImage5 inputs) 0x00d52684 seam (pairOffered pair)
  code = poke (0x0061d9ec - codeBase) (assemble [0xe12fff1e])
    (poke (0x0061d8dc - codeBase) (assemble [0xe12fff1e])
      (poke (0x00641438 - codeBase) (recordStub2 0x00956ca0)
        (poke (0x0063c51c - codeBase) (recordStub3 0x00956c90) (slice (inputImage4 inputs) image4Base codeBase 0x2c000))))
  carrier = assemble
    [ branchLinkWord pairCarrier dispatcherEntry
    , movw 14 (exitAddress .&. 0xffff)
    , movt 14 (exitAddress `shiftR` 16)
    , 0xe12fff1e
    ]
  cellsInitial =
    let (storedSize, storedAspect, storedThird) = pairStored pair
    in poke 0x448 (assemble [storedAspect])
      (poke 0x440 (assemble [storedThird])
        (poke 0x438 (assemble [storedSize])
          (poke 0 (assemble [pairMode pair]) (B.replicate 0x480 0))))
  initial register = case register of
    R0 -> 0xf6
    R1 -> pairToken pair
    Sp -> stackTop
    Lr -> exitAddress
    Cpsr -> 0x10
    _ -> 0

-- | The display-call arguments the pair carrier's recording stubs leave in the
-- cells region.
pairDisplayArguments :: Run -> ((Word32, Word32, Word32), (Word32, Word32))
pairDisplayArguments run =
  let cells = region run "cells"
  in ((readWordAt cells 0x458, readWordAt cells 0x45c, readWordAt cells 0x460), (readWordAt cells 0x468, readWordAt cells 0x46c))

pairExpectedDisplay :: Pair -> ((Word32, Word32, Word32), (Word32, Word32))
pairExpectedDisplay pair
  | pairMode pair == 0x2c = ((0, 0, 0), (0, 0))
  | otherwise = ((0x34, 0, 0xf6), (1, 0))

pairDifferences :: Pair -> Run -> Run -> [String]
pairDifferences pair stock subject =
  [ "status " ++ runnerStatusLabel (status subject) | status subject /= Returned ]
    ++ if status subject /= Returned then [] else
      regionDifferences ["cells", "settings"] stock subject
        ++ [ "r0 differs" | registerValue (runObservation stock) R0 /= registerValue (runObservation subject) R0 ]
        ++ [ "sp differs" | registerValue (runObservation stock) Sp /= registerValue (runObservation subject) Sp ]
        ++ [ "display arguments differ from the mode's" | pairDisplayArguments subject /= pairExpectedDisplay pair ]

-- | The stock role must return with the mode's display arguments, or the pair
-- does not describe stock.
stockPair :: Inputs -> Pair -> IO Run
stockPair inputs pair = do
  stock <- runSpec inputs (pairSpec inputs Nothing "stock" pair)
  requireReturned (pairName pair) stock
  unless (pairDisplayArguments stock == pairExpectedDisplay pair)
    (fail (pairName pair ++ ": stock display arguments differ from the mode's"))
  pure stock

runPair :: Inputs -> SeamImage -> Pair -> Run -> IO ([String], Value)
runPair inputs owned pair stock = do
  subject <- runSpec inputs (pairSpec inputs (Just owned) "owned" pair)
  let differences = pairDifferences pair stock subject
      inside = executedIn stock stockArm stockArmEnd
      problems =
        [ pairName pair ++ ": " ++ difference | difference <- differences ]
          ++ [ pairName pair ++ ": stock arm executed no instruction" | inside == 0 ]
  pure (problems, object
    [ "id" .= pairName pair
    , "entry" .= hexWord32 stockArm
    , "extent" .= (stockArmEnd - stockArm)
    , "stock_instructions_inside" .= inside
    , "owned_stock_arm_instructions" .= executedIn subject stockArm stockArmEnd
    , "owned_status" .= runnerStatusLabel (status subject)
    , "differences" .= differences
    ])

runPairControl :: Inputs -> SeamImage -> Pair -> Run -> IO (String, [String])
runPairControl inputs image pair stock = do
  subject <- runSpec inputs (pairSpec inputs (Just image) "always-decline" pair)
  pure (pairName pair, pairDifferences pair stock subject)

validatePairs :: B.ByteString -> IO ()
validatePairs image4 = do
  unless (imagePair image4 1 == (0, 1)) (fail "IMAGE SIZE token 1 pair differs")
  unless (imagePair image4 2 == (0, 2)) (fail "IMAGE SIZE token 2 pair differs")

imagePair :: B.ByteString -> Word32 -> (Word32, Word32)
imagePair image4 token =
  (readWordAt image4 offset, readWordAt image4 (offset + 4))
 where
  offset = fromIntegral (pairBase - image4Base) + 16 * fromIntegral token

-- Lifecycle cohort ---------------------------------------------------------

newtype PropertyId = PropertyId Word32
newtype RowToken = RowToken Word32
newtype MenuClass = MenuClass Word32

-- | One @0xd5b350(kind, node, id, class)@ call the engine emits.
data Notification = Notification
  { notifyKind :: Word32
  , notifyNode :: Word32
  , notifyId :: Word32
  , notifyClass :: Word32
  }

-- | Push and Pop carry the engine's stage kinds 1 and 0xf, Close kind 2.
pushNotification, popNotification, closeNotification :: Word32 -> Word32 -> Word32 -> Notification
pushNotification = Notification 1
popNotification = Notification 0xf
closeNotification = Notification 2

-- | Pop ids that ask stock to restore the settings checkpoint.
restoresCheckpoint :: Notification -> Bool
restoresCheckpoint notification = notifyId notification `elem` [0x1a, 0x1c]

data Step
  = Select RowToken
  | Push Notification MenuClass
  | Pop Notification
  | Close Notification
  | Write PropertyId Word32
  | Read PropertyId

data Lifecycle = Lifecycle
  { lifecycleId :: String
  , lifecycleSteps :: [Step]
  , lifecycleCheck :: [Word32] -> [String]
  }

-- | How the carrier forwards the engine's notifications: as the production
-- engine adapter does, or as one of the deliberately wrong variants.
data Routing
  = Forwarded
  | PopConsumed
  | Session SessionEntries

lifecycleCohort :: B.ByteString -> [Lifecycle]
lifecycleCohort image4 = [confirmedThenRestore, closedThenPopElsewhere]
 where
  (size2, aspect2) = imagePair image4 2
  imageSize = MenuClass 0xf6
  enter = Push (pushNotification 1 0x1d 5) imageSize
  elsewhere = popNotification 5 0x1a 5
  -- IMAGE SIZE is confirmed with OK; a later BACK elsewhere restores stock's
  -- checkpoint, which must hold the confirmed size.
  confirmedThenRestore = Lifecycle "lifecycle-confirmed-then-restore"
    [ Select (RowToken 1)
    , enter
    , Select (RowToken 2)
    , Pop (popNotification 1 0x1d 0xf6)
    , Pop elsewhere
    , Read (PropertyId 0xf6)
    , Read (PropertyId 0xf9)
    ]
    (\values -> case values of
      [size, aspect] -> [ "IMAGE SIZE after the restore is not the confirmed pair" | (size, aspect) /= (size2, aspect2) ]
      _ -> ["readout count differs"])
  -- The menu is closed from inside the IMAGE SIZE picker; another picker's edit
  -- is then cancelled with BACK. Stock restores that edit and leaves IMAGE SIZE.
  closedThenPopElsewhere = Lifecycle "lifecycle-closed-then-pop-elsewhere"
    [ Select (RowToken 1)
    , enter
    , Select (RowToken 2)
    , Close (closeNotification 1 0x1b 0xf6)
    , Push (pushNotification 5 0x1d 5) (MenuClass 5)
    , Read (PropertyId 0x13)
    , Write (PropertyId 0x13) 0
    , Read (PropertyId 0x13)
    , Pop elsewhere
    , Read (PropertyId 0xf6)
    , Read (PropertyId 0xf9)
    , Read (PropertyId 0x13)
    ]
    (\values -> case values of
      [before, edited, size, aspect, after] ->
        [ "the other picker's edit did not change its property" | edited == before ]
          ++ [ "the other picker's edit was not restored" | after /= before ]
          ++ [ "IMAGE SIZE changed" | (size, aspect) /= (size2, aspect2) ]
      _ -> ["readout count differs"])

lifecycleCarrierAt, scratchBase, scratchCells :: Word32
lifecycleCarrierAt = 0x00c2d920
scratchBase = 0x0095b000
scratchCells = 0x0095b400

loadReg :: Word32 -> Word32 -> [Word32]
loadReg register value = [movw register (value .&. 0xffff), movt register (value `shiftR` 16)]

-- | Words placed at @address@ that load @arguments@ into r0.. and call @target@.
callWords :: Word32 -> Word32 -> [Word32] -> [Word32]
callWords address target arguments =
  let load = concat (zipWith loadReg [0 ..] arguments)
  in load ++ [branchLinkWord (address + 4 * fromIntegral (length load)) target]

notifyWords :: Word32 -> Notification -> [Word32]
notifyWords address notification =
  callWords address notifyEntry [notifyKind notification, notifyNode notification, notifyId notification, notifyClass notification]

-- | The words for one step at @address@; @index@ is the next readout slot.
stepWords :: Routing -> Word32 -> Int -> Step -> [Word32]
stepWords routing address index step = case step of
  Select (RowToken token) -> callWords address dispatcherEntry [0xf6, token]
  Write (PropertyId property) value -> callWords address settingsWrite [property, value, 0]
  Read (PropertyId property) ->
    callWords address settingsRead [property, 0] ++ [0xe5840000 .|. fromIntegral (4 * index)]
  Close notification -> notifyWords address notification
  Push notification (MenuClass entered) -> case routing of
    Session entries ->
      let forwarded = notifyWords address notification
      in forwarded ++ callWords (address + 4 * fromIntegral (length forwarded)) (sessionBegin entries) [entered]
    _ -> notifyWords address notification
  Pop notification -> case routing of
    Forwarded -> notifyWords address notification
    PopConsumed -> []
    Session entries -> sessionPop entries address notification

-- | The deleted engine adapter's Pop: end the session with cancel or confirm,
-- and when one ended, run the owned handback instead of stock's notification.
sessionPop :: SessionEntries -> Word32 -> Notification -> [Word32]
sessionPop entries address notification =
  let restore = if restoresCheckpoint notification then 1 else 0
      ending = branchLinkWord address (if restore == 1 then sessionCancel entries else sessionConfirm entries)
      branchAt = address + 8
      handbackAt = address + 12
      handback = callWords handbackAt (sessionRetained entries) [restore]
      skipAt = handbackAt + 4 * fromIntegral (length handback)
      notifyAt = skipAt + 4
      forwarded = notifyWords notifyAt notification
      endAt = notifyAt + 4 * fromIntegral (length forwarded)
  in [ending, 0xe3500000, branchWord 0x0 branchAt notifyAt]
       ++ handback
       ++ [branchWord 0xe skipAt endAt]
       ++ forwarded

-- | The carrier at 'lifecycleCarrierAt': saves r4-r8 and lr, keeps the readout base in
-- r4, runs the steps and returns through the exit link.
lifecycleCarrier :: Routing -> [Step] -> B.ByteString
lifecycleCarrier routing steps =
  assemble ([0xe92d41f0] ++ loadReg 4 scratchBase ++ go (lifecycleCarrierAt + 12) 0 steps ++ [0xe8bd81f0])
 where
  go _ _ [] = []
  go address index (step : rest) =
    let words_ = stepWords routing address index step
        next = case step of
          Read _ -> index + 1
          _ -> index
    in words_ ++ go (address + 4 * fromIntegral (length words_)) next rest

readouts :: Lifecycle -> Run -> [Word32]
readouts lifecycle run =
  let scratch = region run "scratch"
      count = length [() | Read _ <- lifecycleSteps lifecycle]
  in [readWordAt scratch (4 * index) | index <- [0 .. count - 1]]

lifecycleSpec :: Inputs -> Maybe SeamImage -> Routing -> String -> Lifecycle -> CaseSpec
lifecycleSpec inputs seam routing label lifecycle = CaseSpec
  { caseId = lifecycleId lifecycle ++ "-" ++ label
  , caseProvenance =
      "X-M1 1.50 image-5 dispatcher and lifecycle notification bytes [0xd4d604,0xd5b590) with the 0x00d50084 jump-table word pointed at the owned seam for the owned role; image-4 code [0x61c90c,0x722178) with 0x63c51c/0x641438 and 0x63220c/0x63647c replaced by recording stubs; conversion, pair, cells and settings from the pinned images"
  , caseExclusion =
      "Synthetic admitted sequence: 0x63c51c, 0x641438, 0x63220c and 0x63647c are recording stubs, so the display, current-property, state-snapshot and auxiliary-restore effects are observed as arguments and not executed; the dispatcher, the lifecycle notification with its cell snapshot 0x637e5c and settings-bank copy 0x61c90c, and the settings service run as stock. Prepared state, not key-reachable acceptance."
  , caseEntry = at lifecycleCarrierAt
  , caseExit = at exitAddress
  , caseRegions = regions
  , caseFunctions =
      (at lifecycleCarrierAt, fromIntegral (B.length carrier)) : lifecycleFunctions
        ++ [ (at ownedBase, fromIntegral ownedTextBytes) | Just _ <- [seam] ]
  , caseInitial = [(register, initial register) | register <- [R0, R1, R2, R3, R4, R5, R6, R7, R8, R9, R10, R11, R12, Sp, Lr, Cpsr]]
  , caseExpectedStatus = Returned
  , caseExpectedRegisters = [(Sp, stackTop)]
  , caseExpectedMemory = []
  , caseIgnoredMemory = []
  , caseCp14Identity = Nothing
  , caseKernel = Nothing
  }
 where
  carrier = lifecycleCarrier routing (lifecycleSteps lifecycle)
  initial register = case register of
    Sp -> stackTop
    Lr -> exitAddress
    Cpsr -> 0x10
    _ -> 0
  image4Region name base size =
    let bytes = slice (inputImage4 inputs) image4Base base size
    in mkRegion (RegionId name) (at base) (B.length bytes) ReadOnly bytes
  cellsInitial = poke 0x55c (assemble [scratchCells]) (poke 0x174 (assemble [scratchCells]) (poke 0 (assemble [0x2c]) (B.replicate 0x800 0)))
  settingsInitial = B.replicate 0x1c2 0xaa <> B.replicate 0x1c2 0
  dispatcher = dispatcherFor (inputImage5 inputs) notifyEnd seam True
  code = poke (0x0061d9ec - bankCopy) (assemble [0xe12fff1e])
    (poke (0x0061d8dc - bankCopy) (assemble [0xe12fff1e])
      (poke (0x00641438 - bankCopy) (recordStub2 (scratchBase + 0x50))
        (poke (0x0063c51c - bankCopy) (recordStub3 (scratchBase + 0x40))
          (poke (0x0063647c - bankCopy) (appendStub (scratchBase + 0xa8))
            (poke (0x0063220c - bankCopy) (appendStub (scratchBase + 0x60))
              (slice (inputImage4 inputs) image4Base bankCopy (0x00722178 - bankCopy)))))))
  regions =
    [ mkRegion (RegionId "dispatcher") (at dispatcherBase) (B.length dispatcher) ReadExecute dispatcher
    , mkRegion (RegionId "code") (at bankCopy) (B.length code) ReadExecute code
    , mkRegion (RegionId "carrier") (at lifecycleCarrierAt) (B.length carrier) ReadExecute carrier
    , image4Region "conv-tables" convTableBase (0x0077b824 - convTableBase)
    , image4Region "pairs" pairBase 0x400
    , mkRegion (RegionId "cells") (at cellsBase) 0x800 ReadWrite cellsInitial
    , mkRegion (RegionId "mirror") (at 0x0089ec00) 0x200 ReadWrite (poke 0x1c8 (assemble [1]) (B.replicate 0x200 0))
    , mkRegion (RegionId "mode-cells") (at 0x00951564) 0x2d24 ReadWrite (B.replicate 0x2d24 0)
    , mkRegion (RegionId "settings") (at settingsBase) 0x384 ReadWrite settingsInitial
    , mkRegion (RegionId "slot-defaults") (at 0x007cb804) 0x28 ReadOnly (B.replicate 0x28 0)
    , mkRegion (RegionId "defaults-ptrs") (at 0x0081b074) 0x888 ReadOnly
        (B.replicate 0x28 0 <> slice (inputImage4 inputs) image4Base convPointerBase 0x860)
    , mkRegion (RegionId "stack") (at stackBase) 0x1000 ReadWrite (B.replicate 0x1000 0x5a)
    , mkRegion (RegionId "exit") (at exitAddress) 4 ReadExecute (assemble [0xe12fff1e])
    , mkRegion (RegionId "scratch") (at scratchBase) 0x1000 ReadWrite (B.replicate 0x1000 0)
    ]
      ++ maybe [] seamRegions seam

-- | The writable stock state both roles share. The owned role's own state and
-- the stack are not compared.
lifecycleRegions :: [String]
lifecycleRegions = ["cells", "mirror", "mode-cells", "settings", "scratch"]

lifecycleDifferences :: Lifecycle -> Run -> Run -> [String]
lifecycleDifferences lifecycle stock subject
  | status subject /= Returned = ["status " ++ runnerStatusLabel (status subject)]
  | otherwise =
      regionDifferences lifecycleRegions stock subject
        ++ [ "readouts " ++ show (readouts lifecycle subject) ++ " differ from stock's " ++ show (readouts lifecycle stock)
           | readouts lifecycle subject /= readouts lifecycle stock ]
        ++ lifecycleCheck lifecycle (readouts lifecycle subject)

-- | The stock role must return and meet the scenario's own expectation, or the
-- scenario does not describe stock.
stockLifecycle :: Inputs -> Lifecycle -> IO Run
stockLifecycle inputs lifecycle = do
  stock <- runSpec inputs (lifecycleSpec inputs Nothing Forwarded "stock" lifecycle)
  requireReturned (lifecycleId lifecycle) stock
  case lifecycleCheck lifecycle (readouts lifecycle stock) of
    [] -> pure stock
    problems -> fail (lifecycleId lifecycle ++ ": stock role: " ++ unwords problems)

runLifecycle :: Inputs -> SeamImage -> Lifecycle -> Run -> IO ([String], Value)
runLifecycle inputs owned lifecycle stock = do
  subject <- runSpec inputs (lifecycleSpec inputs (Just owned) Forwarded "owned" lifecycle)
  let differences = lifecycleDifferences lifecycle stock subject
      notifyInside = executedIn stock notifyEntry notifyEnd
      armInside = executedIn stock stockArm stockArmEnd
      copyInside = executedIn stock bankCopy (bankCopy + 0x10)
      label = lifecycleId lifecycle
      problems =
        [ label ++ ": " ++ difference | difference <- differences ]
          ++ [ label ++ ": stock lifecycle notification executed no instruction" | notifyInside == 0 ]
          ++ [ label ++ ": stock arm executed no instruction" | armInside == 0 ]
          ++ [ label ++ ": stock settings-bank copy executed no instruction" | copyInside == 0 ]
  pure (problems, object
    [ "id" .= label
    , "entry" .= hexWord32 notifyEntry
    , "extent" .= (notifyEnd - notifyEntry)
    , "stock_instructions_inside" .= notifyInside
    , "stock_arm_instructions" .= armInside
    , "stock_bank_copy_instructions" .= copyInside
    , "owned_notify_instructions" .= executedIn subject notifyEntry notifyEnd
    , "owned_stock_arm_instructions" .= executedIn subject stockArm stockArmEnd
    , "stock_readouts" .= readouts lifecycle stock
    , "owned_readouts" .= readouts lifecycle subject
    , "owned_status" .= runnerStatusLabel (status subject)
    , "differences" .= differences
    ])

runLifecycleControl :: Inputs -> String -> SeamImage -> Routing -> Lifecycle -> Run -> IO (String, [String])
runLifecycleControl inputs name image routing lifecycle stock = do
  subject <- runSpec inputs (lifecycleSpec inputs (Just image) routing name lifecycle)
  pure (lifecycleId lifecycle, lifecycleDifferences lifecycle stock subject)

-- Shared case parts --------------------------------------------------------

seamRegions :: SeamImage -> [CaseRegion]
seamRegions image =
  mkRegion (RegionId "owned") (at ownedBase) ownedTextBytes ReadExecute
    (B.take ownedTextBytes (seamBytes image <> B.replicate ownedTextBytes 0))
    : [ mkRegion (RegionId "owned-state") (at ownedStateBase) (B.length state) ReadWrite state | not (B.null state) ]
 where
  state = B.drop ownedTextBytes (seamBytes image)

-- | Image-5 bytes from the dispatcher to @end@. With a seam the jump-table
-- word points at its arm, and @trap@ replaces the stock arm's first word.
dispatcherFor :: B.ByteString -> Word32 -> Maybe SeamImage -> Bool -> B.ByteString
dispatcherFor image5 end seam trap = case seam of
  Nothing -> base
  Just image ->
    (if trap then poke (stockArm - dispatcherBase) (assemble [trapWord]) else id)
      (poke (jumpWord - dispatcherBase) (assemble [seamArm image]) base)
 where
  base = slice image5 image5Base dispatcherBase (end - dispatcherBase)

dispatcherFunctions :: [(GuestAddress, Word32)]
dispatcherFunctions =
  [ (at dispatcherEntry, 0x2a10)
  , (at dispatcherBase, 20)
  , (at settingsWrite, 16)
  , (at settingsRead, 16)
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

lifecycleFunctions :: [(GuestAddress, Word32)]
lifecycleFunctions =
  dispatcherFunctions
    ++ [ (at notifyEntry, notifyEnd - notifyEntry)
       , (at 0x00637ed0, 0x3c)
       , (at 0x00637e5c, 0x70)
       , (at 0x00637e38, 0x24)
       , (at 0x00637e14, 0x24)
       , (at bankCopy, 0x10)
       , (at 0x0061daa8, 0x58)
       , (at 0x0061d92c, 0x14)
       , (at 0x00722054, 0x124)
       , (at 0x0063219c, 0x3c)
       , (at 0x0063220c, 0x24)
       , (at 0x0063647c, 0x24)
       ]

branchWord :: Word32 -> Word32 -> Word32 -> Word32
branchWord condition from to =
  (condition `shiftL` 28) .|. 0x0a000000 .|. (((to - (from + 8)) `shiftR` 2) .&. 0x00ffffff)

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

-- | Records @(r0, r1)@ of each call at @counter + 4 + 8n@ and @counter + 8 + 8n@.
appendStub :: Word32 -> B.ByteString
appendStub counter = assemble
  [ movw 12 (counter .&. 0xffff)
  , movt 12 (counter `shiftR` 16)
  , 0xe59c3000
  , 0xe2833001
  , 0xe58c3000
  , 0xe08cc183
  , 0xe50c0004
  , 0xe58c1000
  , 0xe12fff1e
  ]

movw, movt :: Word32 -> Word32 -> Word32
movw register immediate = 0xe3000000 .|. (register `shiftL` 12) .|. ((immediate .&. 0xf000) `shiftL` 4) .|. (immediate .&. 0x0fff)
movt register immediate = 0xe3400000 .|. (register `shiftL` 12) .|. ((immediate .&. 0xf000) `shiftL` 4) .|. (immediate .&. 0x0fff)

at :: Word32 -> GuestAddress
at = GuestAddress . fromIntegral

slice :: B.ByteString -> Word32 -> Word32 -> Word32 -> B.ByteString
slice bytes base address count = B.take (fromIntegral count) (B.drop (fromIntegral (address - base)) bytes)

poke :: Word32 -> B.ByteString -> B.ByteString -> B.ByteString
poke offset value bytes = B.take start bytes <> value <> B.drop (start + B.length value) bytes
 where
  start = fromIntegral offset
