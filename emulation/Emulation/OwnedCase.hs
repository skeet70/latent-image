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
  , IgnoredMemory (..)
  , IgnoredReason (..)
  , KernelCase (..)
  , StackContext
  , mkStackContext
  , InstructionLimit (..)
  , plainLimit
  , CaseOutcome (..)
  , caseOutcome
  , KernelDifference (..)
  , compareKernelCase
  , writeCase
  , MemoryProfile (..)
  , writeCaseWithProfile
  , loadObserved
  , CaseId (..)
  , writeBatch
  , writeBatchWithProfile
  , writeBatchWithInputPool
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

import Control.Exception (IOException, try)
import Control.Monad (foldM, forM_, unless, when)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import Data.Bits (shiftR, (.&.))
import Data.IORef (modifyIORef', newIORef, readIORef)
import Data.List (nub, sort)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe)
import Data.String (fromString)
import Data.Word (Word32)
import Emulation.NativeCase (BatchCase (..), BatchStatus (..), BatchSummary (..), CaseId (..), CaseReport (..), CaseStatus (..), DigestHex (..), MemoryProfile (..), Region (..), artifactCount, batchStatusLabel, loadCase, profileSizeBytes, runBatch, runCase)
import Emulation.NativeObservation
  ( GuestAddress (..)
  , KernelObservation (..)
  , NativeObservation (..)
  , Ordered (..)
  , Register (..)
  , RegionId (..)
  , RunnerStatus (..)
  , Write (..)
  , WriteBytes (..)
  , WriteValue (..)
  , hexWord32
  , registerLabels
  , lookupRegister
  , observeNative
  , parseWord32
  , regionIdOf
  , registerLabel
  , renderObservationError
  , reportRunnerExit
  , runnerStatusLabel
  )
import qualified Emulation.NativeEvidence as E
import Emulation.Sha256 (sha256, sha256File, sha256Hex)
import FujiAF.Host (get, str)
import FujiAF.JSON (encodeCanonical)
import Numeric (showHex)
import System.Directory (createDirectoryIfMissing)
import System.FilePath ((</>), takeDirectory)
import System.IO.Error (isDoesNotExistError)
import System.Posix.Files (FileStatus, getSymbolicLinkStatus, isRegularFile)

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

reasonLabel :: IgnoredReason -> String
reasonLabel PrivateFrame = "private-frame"
reasonLabel CallerOutput = "caller-output"

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

data IgnoredReason = PrivateFrame | CallerOutput deriving (Eq, Show)

data IgnoredMemory = IgnoredMemory
  { ignoredRegion :: RegionId
  , ignoredOffset :: Int
  , ignoredExtent :: Int
  , ignoredReason :: IgnoredReason
  } deriving (Eq, Show)

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
  , caseIgnoredMemory :: [IgnoredMemory]
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

-- | One declared private stack: @low < SP <= high@, both bounds eight-byte aligned.
newtype StackContext = StackContext (GuestAddress, GuestAddress)

mkStackContext :: GuestAddress -> GuestAddress -> Either String StackContext
mkStackContext low high
  | guestAddress low .&. 7 /= 0 = Left "stack context low bound must be eight-byte aligned"
  | guestAddress high .&. 7 /= 0 = Left "stack context high bound must be eight-byte aligned"
  | guestAddress low >= guestAddress high = Left "stack context low bound must be below its high bound"
  | otherwise = Right (StackContext (low, high))

-- | A kernel case additionally executes word exclusives, barriers and @nop@, reads the declared
-- system registers, may end at a stop address and may map device regions.
data KernelCase = KernelCase
  { kernelLimit :: InstructionLimit
  , kernelStop :: Maybe GuestAddress
  , kernelTpidrurw :: Maybe Word32
  , -- | Needed with 'kernelFpexc'; FPEXC is accessible only when cp10 and cp11 are equal and 0b01 or 0b11.
    kernelCpacr :: Maybe Word32
  , -- | EN (bit 30) is the only bit it may set.
    kernelFpexc :: Maybe Word32
  , -- | Registers 'compareKernelCase' leaves out, such as a spinning loop's scratch and PC. 'Cpsr'
    -- here drops only its flags: mode, T, F, I and A (bits 8:0) are always compared.
    kernelUncompared :: [Register]
  , -- | Declared private stack contexts; empty for a single-context case.
    kernelStackContexts :: [StackContext]
  }

caseLimit :: CaseSpec -> InstructionLimit
caseLimit = maybe plainLimit kernelLimit . caseKernel

-- | How a case ended, the part two roles of one case compare before their state.
data CaseOutcome
  = OutcomeReturned
  | OutcomeStopped GuestAddress
  | -- | A kernel case that has not returned within its N instructions, with the word its last
    -- LDREX reserved.
    OutcomeNoReturnWithin InstructionLimit (Maybe GuestAddress)
  | OutcomeRefused RunnerStatus
  deriving (Eq, Show)

caseOutcome :: CaseSpec -> NativeObservation -> CaseOutcome
caseOutcome spec observation = case (nativeStatus observation, caseKernel spec) of
  (Returned, _) -> OutcomeReturned
  (StopAddress, _) -> OutcomeStopped (GuestAddress (fromIntegral (registerValue observation Pc)))
  (InstructionBudget, Just kernel) -> OutcomeNoReturnWithin (kernelLimit kernel) (nativeKernel observation >>= observedReserved)
  (status, _) -> OutcomeRefused status

data KernelDifference
  = NotKernelCase
  | OutcomeDiffers CaseOutcome CaseOutcome
  | KernelRegisterDiffers Register
  | KernelMemoryDiffers RegionId
  | WritesDiffer
  | SystemRegistersDiffer
  | WriteLogTruncated
  deriving (Eq, Show)

-- | Two roles of one kernel case: outcome, every register but the declared exclusions with CPSR
-- bits 8:0 always, the final bytes of the spec's writable and device regions, the final TPIDRURW and
-- FPEXC, and the multiset of memory writes with their values and of system register writes. Barrier
-- order is left to per-case assertions on 'observedOrder'.
compareKernelCase :: CaseSpec -> NativeObservation -> NativeObservation -> [KernelDifference]
compareKernelCase spec reference subject = case (caseKernel spec, nativeKernel reference, nativeKernel subject) of
  (Just kernel, Just referenceKernel, Just subjectKernel) ->
    [OutcomeDiffers (outcome reference) (outcome subject) | outcome reference /= outcome subject]
      ++ [ KernelRegisterDiffers register
         | register <- registerLabels
         , Just compared <- [comparedPart kernel register]
         , (compared <$> M.lookup register (nativeRegisters reference)) /= (compared <$> M.lookup register (nativeRegisters subject))
         ]
      ++ [ KernelMemoryDiffers ident
         | CaseRegion region permission <- caseRegions spec
         , writable permission
         , let ident = regionIdOf region
         , lookup ident (nativeMemory reference) /= lookup ident (nativeMemory subject)
         ]
      ++ [SystemRegistersDiffer | system referenceKernel /= system subjectKernel]
      ++ case (writes referenceKernel, writes subjectKernel) of
        (Just expected, Just actual) -> [WritesDiffer | expected /= actual]
        _ -> [WriteLogTruncated]
  _ -> [NotKernelCase]
  where
    outcome = caseOutcome spec
    comparedPart kernel register
      | register `notElem` kernelUncompared kernel = Just id
      | register == Cpsr = Just (.&. 0x1ff)
      | otherwise = Nothing
    writable permission = case permission of
      ReadWrite -> True
      Device -> True
      ReadOnly -> False
      ReadExecute -> False
    system observed = (observedTpidrurw observed, observedFpexc observed)
    writes observed = sort . concatMap written <$> observedOrder observed
    written (OrderedWrite (Write (GuestAddress address) (WriteBytes bytes)) (WriteValue stored)) = [Left (address, bytes, stored)]
    written (OrderedSystemWrite register value) = [Right (register, value)]
    written (OrderedBarrier _) = []

newtype InitializerId = InitializerId String
newtype InitializerOffset = InitializerOffset Int

data InitialArtifact = InitialArtifact
  { initializerId :: InitializerId
  , initializerBytes :: B.ByteString
  , initializerDigest :: DigestHex
  }

data InitialSlice = InitialSlice InitializerId InitializerOffset
data Initializers = Initializers [InitialArtifact] (M.Map RegionId InitialSlice)

initializersFor :: MemoryProfile -> CaseSpec -> Initializers
initializersFor profile spec = Initializers (map fst built) (M.fromList (concatMap snd built))
  where
    nonzero = [region | CaseRegion region _ <- caseRegions spec, not (B.all (== 0) (regionBytes region))]
    groups
      | length nonzero <= artifactCount = [(InitializerId (regionName (regionIdOf region)), [region]) | region <- nonzero]
      | otherwise = [(InitializerId ("initial-" ++ show index), regions) | (index, regions) <- zip [0 :: Int ..] (pack [] 0 nonzero)]
    pack [] _ [] = []
    pack pending _ [] = [reverse pending]
    pack pending bytes (region : rest)
      | not (null pending) && bytes + B.length (regionBytes region) > profileSizeBytes profile =
          reverse pending : pack [region] (B.length (regionBytes region)) rest
      | otherwise = pack (region : pending) (bytes + B.length (regionBytes region)) rest
    built = map build groups
    build (ident, regions) =
      let bytes = case regions of
            [region] -> regionBytes region
            _ -> B.concat (map regionBytes regions)
          offsets = scanl (\offset region -> offset + B.length (regionBytes region)) 0 regions
          slices = [(regionIdOf region, InitialSlice ident (InitializerOffset offset)) | (region, offset) <- zip regions offsets]
       in (InitialArtifact ident bytes (DigestHex (sha256Hex (sha256 bytes))), slices)

regionJson :: Initializers -> CaseRegion -> Value
regionJson (Initializers _ slices) (CaseRegion region permission) =
  object
    [ "id" .= regionName (regionIdOf region)
    , "address" .= hexAddress (GuestAddress (fromIntegral (baseAddress region)))
    , "size" .= regionSize region
    , "permissions" .= permissionLabel permission
    , "initial" .=
        ( case M.lookup (regionIdOf region) slices of
            Nothing -> object ["zero" .= True]
            Just (InitialSlice (InitializerId ident) (InitializerOffset offset)) -> object
              [ "artifact" .= ident
              , "offset" .= offset
              , "length" .= B.length (regionBytes region)
              ]
        )
    ]

artifactJson :: (InitialArtifact -> FilePath) -> InitialArtifact -> Value
artifactJson file artifact@(InitialArtifact (InitializerId ident) bytes (DigestHex digest)) =
  object
    [ "id" .= ident
    , "path" .= file artifact
    , "size" .= B.length bytes
    , "sha256" .= digest
    ]

caseValue :: MemoryProfile -> (InitialArtifact -> FilePath) -> Initializers -> CaseSpec -> Value
caseValue profile file initializers@(Initializers artifacts _) spec =
  object $
    [ "schema_version" .= (1 :: Int)
    , "id" .= caseId spec
    , "origin" .= ("synthetic" :: String)
    , "provenance" .= caseProvenance spec
    , "cpu" .= object ["architecture" .= ("arm" :: String), "model" .= ("cortex-r5f" :: String), "mode" .= ("arm" :: String), "endian" .= ("little" :: String)]
    , "artifacts" .= map (artifactJson file) artifacts
    , "entry" .= hexAddress (caseEntry spec)
    , "exit" .= hexAddress (caseExit spec)
    , "registers" .= object [fromString (registerLabel r) .= hexWord32 value | (r, value) <- caseInitial spec]
    , "regions" .= map (regionJson initializers) (caseRegions spec)
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
      ++ [ "ignored_memory" .=
             [ object (["region" .= regionName (ignoredRegion range), "offset" .= ignoredOffset range, "length" .= ignoredExtent range]
                 ++ ["reason" .= reasonLabel (ignoredReason range) | ignoredReason range /= PrivateFrame])
             | range <- caseIgnoredMemory spec
             ]
         | not (null (caseIgnoredMemory spec))
         ]
      ++ ["cp14_identity" .= hexWord32 identity | Just identity <- [caseCp14Identity spec]]
      ++ ["kernel" .= kernelJson kernel | Just kernel <- [caseKernel spec]]
      ++ ["memory_profile" .= ("resource8" :: String) | profile == Resource8]

kernelJson :: KernelCase -> Value
kernelJson kernel =
  object $
    ["stop" .= hexAddress stop | Just stop <- [kernelStop kernel]]
      ++ [fromString name .= hexWord32 value | (name, Just value) <- [("tpidrurw", kernelTpidrurw kernel), ("cpacr", kernelCpacr kernel), ("fpexc", kernelFpexc kernel)]]
      ++ [ "stack_contexts"
             .= [object ["low" .= hexAddress low, "high" .= hexAddress high] | StackContext (low, high) <- kernelStackContexts kernel]
         | not (null (kernelStackContexts kernel))
         ]

writeCase :: FilePath -> CaseSpec -> IO FilePath
writeCase = writeCaseWithProfile DefaultMemory

writeCaseWithProfile :: MemoryProfile -> FilePath -> CaseSpec -> IO FilePath
writeCaseWithProfile profile directory spec = do
  createDirectoryIfMissing True directory
  let initializers@(Initializers artifacts _) = initializersFor profile spec
      file artifact = case initializerId artifact of InitializerId ident -> ident ++ ".bin"
  forM_ artifacts $ \artifact ->
    B.writeFile (directory </> file artifact) (initializerBytes artifact)
  let path = directory </> "case.json"
  B.writeFile path (encodeCanonical (caseValue profile file initializers spec))
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
writeBatch = writeBatchUsing DefaultMemory B.writeFile

writeBatchWithProfile :: MemoryProfile -> FilePath -> [CaseSpec] -> IO FilePath
writeBatchWithProfile profile = writeBatchUsing profile B.writeFile

-- | Stages shared initial bytes through the input pool.
writeBatchWithInputPool :: E.InputPool -> FilePath -> [CaseSpec] -> IO FilePath
writeBatchWithInputPool pool = writeBatchUsing DefaultMemory (stageChecked pool)

writeBatchUsing :: MemoryProfile -> (FilePath -> B.ByteString -> IO ()) -> FilePath -> [CaseSpec] -> IO FilePath
writeBatchUsing profile writeAsset directory specs = do
  let ids = map caseId specs
  when (nub ids /= ids) (error "writeBatchUsing: duplicate case id")
  createDirectoryIfMissing True directory
  let planned = [(spec, initializersFor profile spec) | spec <- specs]
      file artifact = case initializerDigest artifact of DigestHex digest -> digest ++ ".bin"
      caseFile spec = caseId spec ++ ".case.json"
      artifacts = M.fromList [(file artifact, initializerBytes artifact) | (_, Initializers initializers _) <- planned, artifact <- initializers]
  forM_ (M.toList artifacts) $ \(name, bytes) -> writeAsset (directory </> name) bytes
  forM_ planned $ \(spec, initializers) -> B.writeFile (directory </> caseFile spec) (encodeCanonical (caseValue profile file initializers spec))
  let path = directory </> "batch.json"
  B.writeFile path $
    encodeCanonical
      ( object
          [ "schema_version" .= (1 :: Int)
          , "cases" .= [object ["id" .= caseId spec, "path" .= caseFile spec] | spec <- specs]
          ]
      )
  pure path

stageChecked :: E.InputPool -> FilePath -> B.ByteString -> IO ()
stageChecked pool path content = do
  existing <- try (getSymbolicLinkStatus path) :: IO (Either IOException FileStatus)
  case existing of
    Left exception
      | isDoesNotExistError exception -> stage
      | otherwise -> ioError exception
    Right status
      | not (isRegularFile status) -> fail ("staged asset is not a regular file: " ++ path)
      | otherwise -> do
          actual <- sha256File path
          unless (actual == sha256 content) (fail ("staged asset content differs: " ++ path))
          stage
  where
    stage = do
      _ <- E.stageInput pool path content
      pure ()

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
