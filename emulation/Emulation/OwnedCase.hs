{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
-- | Bounded native cases that run an original X-M1 body and its owned
-- replacement from the linked bank kernel under the same regions.
module Emulation.OwnedCase
  ( Symbol (..)
  , parseSymbols
  , symbolAddress
  , functionSymbol
  , Permission (..)
  , CaseRegion (..)
  , mkRegion
  , CaseSpec (..)
  , KernelCase (..)
  , InstructionLimit (..)
  , plainLimit
  , CaseOutcome (..)
  , caseOutcome
  , writeCase
  , loadObserved
  , CaseId (..)
  , writeBatch
  , loadObservedBatch
  , registerValue
  , regionBytesOf
  , cpsrControl
  , guestWord
  , hexAddress
  , word32le
  , assemble
  , readWordAt
  , parseAddress
  , required
  ) where

import Control.Monad (foldM, forM_, unless, when)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import Data.Bits (shiftR, (.&.))
import Data.IORef (modifyIORef', newIORef, readIORef)
import Data.List (nub)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe)
import Data.String (fromString)
import Data.Word (Word32)
import Emulation.NativeCase (BatchCase (..), BatchStatus (..), BatchSummary (..), CaseId (..), CaseReport (..), CaseStatus (..), Region (..), batchStatusLabel, loadCase, runBatch, runCase)
import Emulation.NativeObservation
  ( GuestAddress (..)
  , NativeObservation (..)
  , Register (..)
  , RegionId (..)
  , RunnerStatus (..)
  , hexWord32
  , lookupRegister
  , observeNative
  , parseWord32
  , regionIdOf
  , registerLabel
  , renderObservationError
  , reportRunnerExit
  , runnerStatusLabel
  )
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host (get, str)
import FujiAF.JSON (encodeCanonical)
import Numeric (showHex)
import System.Directory (createDirectoryIfMissing)
import System.FilePath ((</>), takeDirectory)

guestWord :: GuestAddress -> Word32
guestWord = fromIntegral . guestAddress

hexAddress :: GuestAddress -> String
hexAddress = hexWord32 . guestWord

hexB :: B.ByteString -> String
hexB = concatMap (\w -> let h = showHex w "" in replicate (2 - length h) '0' ++ h) . B.unpack

word32le :: Word32 -> B.ByteString
word32le w = B.pack
  [ fromIntegral w
  , fromIntegral (w `shiftR` 8)
  , fromIntegral (w `shiftR` 16)
  , fromIntegral (w `shiftR` 24)
  ]

assemble :: [Word32] -> B.ByteString
assemble = B.concat . map word32le

readWordAt :: B.ByteString -> Int -> Word32
readWordAt bytes offset =
  fromIntegral (B.index bytes offset)
    + fromIntegral (B.index bytes (offset + 1)) * 0x100
    + fromIntegral (B.index bytes (offset + 2)) * 0x10000
    + fromIntegral (B.index bytes (offset + 3)) * 0x1000000

-- | A kernel.syms row: address and size.
data Symbol = Symbol Word32 Word32

-- | Reads the named rows of an @llvm-nm --print-size@ listing.
parseSymbols :: [String] -> FilePath -> IO (M.Map String Symbol)
parseSymbols requiredNames path = do
  rows <- zip [1 :: Int ..] . lines <$> readFile path
  foldM add M.empty rows
  where
    add symbols (lineNumber, line) = case words line of
      [a, z, _, name] -> insert symbols lineNumber name a z
      [a, _, name] -> insert symbols lineNumber name a "0"
      _ -> pure symbols
    insert symbols lineNumber name a z
      | name `notElem` requiredNames = pure symbols
      | otherwise =
          case (parseWord32 a, parseWord32 z) of
            (Just address, Just size)
              | M.member name symbols -> error ("duplicate symbol " ++ name ++ " at line " ++ show lineNumber)
              | otherwise -> pure (M.insert name (Symbol address size) symbols)
            _ -> error ("invalid symbol line " ++ show lineNumber)

symbolAddress :: M.Map String Symbol -> String -> GuestAddress
symbolAddress symbols name = case lookupSymbol symbols name of Symbol address _ -> GuestAddress (fromIntegral address)

functionSymbol :: M.Map String Symbol -> String -> (GuestAddress, Word32)
functionSymbol symbols name = case lookupSymbol symbols name of
  Symbol address size
    | size == 0 -> error ("zero-size function symbol " ++ name)
    | otherwise -> (GuestAddress (fromIntegral address), size)

lookupSymbol :: M.Map String Symbol -> String -> Symbol
lookupSymbol symbols name = fromMaybe (error ("kernel symbol missing: " ++ name)) (M.lookup name symbols)

-- | 'Device' is one interrupt-controller bank word of a kernel case, taking aligned word stores only.
data Permission = ReadOnly | ReadWrite | ReadExecute | Device

permissionLabel :: Permission -> String
permissionLabel ReadOnly = "r"
permissionLabel ReadWrite = "rw"
permissionLabel ReadExecute = "rx"
permissionLabel Device = "device"

data CaseRegion = CaseRegion
  { caseRegion :: Region
  , casePermission :: Permission
  }

mkRegion :: RegionId -> GuestAddress -> Int -> Permission -> B.ByteString -> CaseRegion
mkRegion ident base size permission bytes =
  CaseRegion
    ( Region
        { description = object
            [ "id" .= regionName ident
            , "address" .= hexAddress base
            , "size" .= size
            , "permissions" .= permissionLabel permission
            ]
        , baseAddress = fromIntegral (guestAddress base)
        , regionSize = fromIntegral size
        , regionBytes = bytes
        }
    )
    permission

data CaseSpec = CaseSpec
  { caseId :: String
  , caseProvenance :: String
  , caseExclusion :: String
  , caseEntry :: GuestAddress
  , caseExit :: GuestAddress
  , caseRegions :: [CaseRegion]
  , caseFunctions :: [(GuestAddress, Word32)]
  , caseInitial :: [(Register, Word32)]
  , caseExpectedStatus :: RunnerStatus
  , caseExpectedRegisters :: [(Register, Word32)]
  , caseExpectedMemory :: [(RegionId, B.ByteString)]
  , -- | The value MRC p14, 0, Rt, c2, c0, 0 reads; only a privileged case may name one.
    caseCp14Identity :: Maybe Word32
  , -- | Only a privileged case may be a kernel case.
    caseKernel :: Maybe KernelCase
  }

-- | The instruction budget N of a case: it ends @instruction_budget@ when it has not returned within N instructions.
newtype InstructionLimit = InstructionLimit Word32
  deriving (Eq, Show)

-- | The budget of a case that is not a kernel case.
plainLimit :: InstructionLimit
plainLimit = InstructionLimit 4000000

-- | A kernel case additionally executes word exclusives, barriers and @nop@, reads the declared
-- system registers, may end at a stop address and may map device regions.
data KernelCase = KernelCase
  { kernelLimit :: InstructionLimit
  , kernelStop :: Maybe GuestAddress
  , kernelTpidrurw :: Maybe Word32
  , kernelCpacr :: Maybe Word32
  , -- | EN (bit 30) is the only bit it may set.
    kernelFpexc :: Maybe Word32
  }

caseLimit :: CaseSpec -> InstructionLimit
caseLimit = maybe plainLimit kernelLimit . caseKernel

-- | How a case ended, the part two roles of one case compare before their state.
data CaseOutcome
  = OutcomeReturned
  | OutcomeStopped GuestAddress
  | -- | No return within the case's N instructions.
    OutcomeNoReturnWithin InstructionLimit
  | OutcomeRefused RunnerStatus
  deriving (Eq, Show)

caseOutcome :: CaseSpec -> NativeObservation -> CaseOutcome
caseOutcome spec observation = case nativeStatus observation of
  Returned -> OutcomeReturned
  StopAddress -> OutcomeStopped (GuestAddress (fromIntegral (registerValue observation Pc)))
  InstructionBudget -> OutcomeNoReturnWithin (caseLimit spec)
  status -> OutcomeRefused status

regionJson :: CaseRegion -> Value
regionJson (CaseRegion region permission) =
  object
    [ "id" .= regionName (regionIdOf region)
    , "address" .= hexAddress (GuestAddress (fromIntegral (baseAddress region)))
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

artifactJson :: (Region -> FilePath) -> CaseRegion -> Value
artifactJson file (CaseRegion region _) =
  object
    [ "id" .= regionName (regionIdOf region)
    , "path" .= file region
    , "size" .= B.length (regionBytes region)
    , "sha256" .= sha256Hex (sha256 (regionBytes region))
    ]

-- | The case JSON; @file@ names the artifact file holding a region's initial bytes.
caseValue :: (Region -> FilePath) -> CaseSpec -> Value
caseValue file spec =
  object $
    [ "schema_version" .= (1 :: Int)
    , "id" .= caseId spec
    , "origin" .= ("synthetic" :: String)
    , "provenance" .= caseProvenance spec
    , "cpu" .= object ["architecture" .= ("arm" :: String), "model" .= ("cortex-r5f" :: String), "mode" .= ("arm" :: String), "endian" .= ("little" :: String)]
    , "artifacts" .= [artifactJson file r | r <- caseRegions spec, not (B.all (== 0) (regionBytes (caseRegion r)))]
    , "entry" .= hexAddress (caseEntry spec)
    , "exit" .= hexAddress (caseExit spec)
    , "registers" .= object [fromString (registerLabel r) .= hexWord32 value | (r, value) <- caseInitial spec]
    , "regions" .= map regionJson (caseRegions spec)
    , "functions" .=
        [ object ["address" .= hexAddress address, "size" .= size]
        | (address, size) <- caseFunctions spec
        ]
    , "stubs" .= ([] :: [Value])
    , "limits" .= object ["instructions" .= (let InstructionLimit limit = caseLimit spec in limit), "wall_ms" .= (4000 :: Int)]
    , "expected" .=
        object
          [ "status" .= runnerStatusLabel (caseExpectedStatus spec)
          , "registers" .= object [fromString (registerLabel r) .= hexWord32 value | (r, value) <- caseExpectedRegisters spec]
          , "memory" .=
              [ object ["region" .= regionName ident, "offset" .= (0 :: Int), "hex" .= hexB bytes]
              | (ident, bytes) <- caseExpectedMemory spec
              ]
          , "events" .= ([] :: [Value])
          ]
    , "exclusions" .= [caseExclusion spec]
    ]
      ++ ["cp14_identity" .= hexWord32 identity | Just identity <- [caseCp14Identity spec]]
      ++ ["kernel" .= kernelJson kernel | Just kernel <- [caseKernel spec]]

kernelJson :: KernelCase -> Value
kernelJson kernel =
  object $
    ["stop" .= hexAddress stop | Just stop <- [kernelStop kernel]]
      ++ [fromString name .= hexWord32 value | (name, Just value) <- [("tpidrurw", kernelTpidrurw kernel), ("cpacr", kernelCpacr kernel), ("fpexc", kernelFpexc kernel)]]

writeCase :: FilePath -> CaseSpec -> IO FilePath
writeCase directory spec = do
  createDirectoryIfMissing True directory
  let file region = regionName (regionIdOf region) ++ ".bin"
  forM_ (caseRegions spec) $ \(CaseRegion region _) ->
    when (not (B.all (== 0) (regionBytes region))) $
      B.writeFile (directory </> file region) (regionBytes region)
  let path = directory </> "case.json"
  B.writeFile path (encodeCanonical (caseValue file spec))
  pure path

loadObserved :: FilePath -> FilePath -> IO NativeObservation
loadObserved runner path = do
  (_, regions, _, _) <- loadCase path
  (_, report) <- runCase runner path
  B.writeFile (takeDirectory path </> "native-result.json") (encodeCanonical report)
  exit <- either (error . renderObservationError) pure (reportRunnerExit report)
  either (error . renderObservationError) pure (observeNative exit regions (get report "runner"))

-- | Writes the cases into one directory as @ID.case.json@, every distinct initial-bytes artifact
-- once as @SHA256.bin@, and the batch file @batch.json@, whose path is returned.
writeBatch :: FilePath -> [CaseSpec] -> IO FilePath
writeBatch directory specs = do
  let ids = map caseId specs
  when (nub ids /= ids) (error "writeBatch: duplicate case id")
  createDirectoryIfMissing True directory
  let file bytes = sha256Hex (sha256 bytes) ++ ".bin"
      caseFile spec = caseId spec ++ ".case.json"
      artifacts = M.fromList [(file bytes, bytes) | spec <- specs, CaseRegion region _ <- caseRegions spec, let bytes = regionBytes region, not (B.all (== 0) bytes)]
  forM_ (M.toList artifacts) $ \(name, bytes) -> B.writeFile (directory </> name) bytes
  forM_ specs $ \spec -> B.writeFile (directory </> caseFile spec) (encodeCanonical (caseValue (file . regionBytes) spec))
  let path = directory </> "batch.json"
  B.writeFile path $
    encodeCanonical
      ( object
          [ "schema_version" .= (1 :: Int)
          , "cases" .= [object ["id" .= caseId spec, "path" .= caseFile spec] | spec <- specs]
          ]
      )
  pure path

-- | Runs a batch in one runner process, writes each case's report beside the batch as
-- @ID.native-result.json@ and returns every case's observation in batch order, as
-- 'loadObserved' gives it for that case alone. Fails after the batch has run, naming each case
-- without an observation.
loadObservedBatch :: FilePath -> FilePath -> IO [(CaseId, NativeObservation)]
loadObservedBatch runner path = do
  collected <- newIORef []
  summary <- runBatch runner path $ \report -> do
    B.writeFile (takeDirectory path </> (caseIdText (reportCase report) ++ ".native-result.json")) (encodeCanonical (reportValue report))
    let value = reportValue report
        observed = case reportRegions report of
          Nothing -> Left (str (get value "status") ++ ": " ++ str (get value "reason"))
          Just regions -> either (Left . (\problem -> str (get value "status") ++ ": " ++ renderObservationError problem)) Right $ do
            exit <- reportRunnerExit value
            observeNative exit regions (get value "runner")
    modifyIORef' collected ((reportCase report, observed) :)
  results <- reverse <$> readIORef collected
  let missing =
        [caseIdText ident ++ ": " ++ reason | (ident, Left reason) <- results]
          ++ [caseIdText (batchCaseId c) ++ ": not_run" | c <- summaryCases summary, batchCaseStatus c == CaseNotRun]
      headline =
        unwords
          ( ["native batch", path ++ ":", batchStatusLabel (summaryStatus summary)]
              ++ maybe [] (\reason -> [reason]) (summaryReason summary)
              ++ maybe [] (\ident -> ["at", caseIdText ident]) (summaryFailedCase summary)
          )
  unless (summaryStatus summary == BatchComplete && null missing) $
    error (unlines (headline : missing))
  pure [(ident, observation) | (ident, Right observation) <- results]

registerValue :: NativeObservation -> Register -> Word32
registerValue observation register =
  fromMaybe (error ("missing register " ++ registerLabel register)) (lookupRegister register observation)

regionBytesOf :: NativeObservation -> RegionId -> B.ByteString
regionBytesOf observation ident =
  fromMaybe (error ("missing memory region " ++ regionName ident)) (lookup ident (nativeMemory observation))

cpsrControl :: Word32 -> Word32
cpsrControl word = word .&. 0x3ff

parseAddress :: String -> String -> Word32
parseAddress label text = fromMaybe (error ("invalid " ++ label ++ ": " ++ text)) (parseWord32 text)

required :: [String] -> String -> IO String
required arguments name =
  case dropWhile (/= name) arguments of
    (_ : value : _) -> pure value
    _ -> error ("missing " ++ name)
